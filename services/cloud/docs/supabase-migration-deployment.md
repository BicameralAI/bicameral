# Database migration delivery

Cloud database migrations are first-class, fail-closed deployment state. They
remain inside the selected Cloud release closure: an individual SQL file is not
a Product Release Unit, does not carry Product authority, and cannot authorize
deployment or publication.

Governance gate: `bic:spec-governance-gate`; Qor advisory/no-op pending Factory
policy revision. UserJourney: not applicable because this is an
operator/deployment-control path with no user-visible interaction. Journey
Boundary Review: not applicable for the same reason.

## Immutable MigrationSet

`scripts/migration_delivery.py compile` builds a content-addressed
`MigrationSet` from:

- the exact ordered SQL files under `supabase/migrations/`;
- one exact safety declaration per version under
  `supabase/migration-metadata/`;
- each SQL and metadata SHA-256 digest; and
- the exact Cloud commit.

The Cloud release descriptor publishes the resulting `database_migration_set`
artifact digest. The immutable release manifest selects that descriptor. Before
database access, the deployment controller rebuilds the set from the checked-out
bytes and requires an exact match with both the committed set and manifest.

Every migration declares one compatibility class:

| Class | Meaning | Automatic lane |
| --- | --- | --- |
| `expand` | Backward-compatible schema expansion that may safely remain after application rollback. | Allowed when explicitly marked `automatic`. |
| `backfill` | Existing-row or data-population work with separate operational consequences. | Separate authorization required. |
| `contract` | Removal, restriction, or compatibility narrowing. | Separate authorization required. |
| `destructive` | Data or schema loss is possible. | Separate authorization required. |

Classification is evaluated against the exact pending suffix. A historical
non-expand migration that is already recorded remotely remains identity-bound
but does not re-request authority. A pending non-expand migration blocks the
automatic deployment before mutation.

## MigrationExecution lifecycle

The Cloud deployment controller is the only module in this repository that
advances the operational lifecycle:

```text
planned → preflight_passed → applied → verified
   └────→ preflight_failed
               └──────────→ apply_failed
                                  └──────→ verification_failed
```

For the selected target environment it:

1. reads local and remote Supabase migration history;
2. requires remote history to be an exact prefix of the selected set;
3. derives the exact pending suffix and checks its execution policy;
4. runs `db push --dry-run` before mutation;
5. applies the pending suffix;
6. reads remote history again and requires exact equality with the set; and
7. runs a final dry-run to prove convergence.

Only `verified` with `deployment_prerequisite_satisfied: true` permits the
subsequent Cloud Run build. Every other result blocks application deployment.

## Immutable execution receipt

Every attempt writes a secret-free, content-addressed
`MigrationExecutionReceipt` governed by
`schemas/database-migration-execution-receipt.schema.json`. It binds:

- the migration-set and release-manifest identities;
- exact Cloud commit and target environment;
- remote versions before execution;
- the exact pending/applied versions;
- remote versions after execution;
- the ordered operational states and typed failure, if any;
- pinned Supabase CLI version; and
- exact GitHub workflow run and commit.

The workflow uploads this receipt for 90 days even when migration execution
fails. The GitHub artifact and step summary are authoritative operational
evidence. The `#dev-environment` Slack message is best-effort notification and
cannot strengthen or weaken the deployment result. Raw database output,
connection URLs, and application secrets are excluded from the receipt.

## Deployment-only credentials

The migration runner uses a PostgreSQL connection URL with schema-migration
privileges. It is separate from the browser anon key and the Cloud Run
`SUPABASE_SERVICE_KEY`; it must never be added to Cloud Run, source control,
workflow artifacts, or application logs.

| Environment | GitHub Actions secret |
| --- | --- |
| Production | `SUPABASE_DB_URL` |
| Staging | `SUPABASE_DB_URL_STAGING` |

Use the migration-capable connection URL supplied by the corresponding
Supabase project and require TLS. Limit repository administration and Actions
secret access to release operators. The selected value is masked and passed to
the controller only through an environment variable.

## Bootstrap and history reconciliation

Before enabling a new environment, compare its remote migration history with
the complete `MigrationSet`. If tables were created manually but versions were
not recorded, reconcile history explicitly only after verifying the
corresponding schema, indexes, triggers, data, and access controls.

Do not mark versions applied merely to make deployment green. A new environment
whose pending suffix includes `backfill`, `contract`, or `destructive` work
requires the separately authorized lane; the automatic controller will not
bootstrap through it.

Deployments are serialized per branch so two releases cannot mutate the same
environment concurrently.

## Rollback

Application rollback and database rollback are intentionally distinct. If
Cloud Run deployment or served-identity verification fails after a verified
`expand` migration, traffic returns to the prior application revision while the
backward-compatible schema expansion remains.

The automatic lane never reverses SQL, rewrites migration history, or treats
traffic restoration as proof that the database was rolled back. Contract,
destructive, and compensating migrations require their own reviewed plan and
authorization.

## State refinement

This change adds operational delivery states only. It does not add Product
states or change canonical Product authority: the daemon remains authoritative,
while Supabase remains Cloud-owned coordination/projection storage.

The state machine is intentionally minimized. Environment readiness,
notification delivery, release publication, served identity, and rollback are
separate evidence or actions rather than additional migration states.
