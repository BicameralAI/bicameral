# Runbook: produce the Cloud #106 reconciliation evidence packet

Audience: an **authorized owner** with read access to the authorized
live/non-production GCP project and Supabase/Postgres instance. Devin and CI never
run this against live infrastructure — they exercise the same commands against
sanitized fixtures.

This runbook is the single documented command sequence required by cloud #141
acceptance criterion 1. No command needs to be improvised.

## 0. Prerequisites (read-only)

- `gcloud` authenticated to the target project with at least:
  `roles/run.viewer`, `roles/iam.securityReviewer` (or viewer on the service),
  and `roles/compute.viewer` for the load-balancer/IAP facts.
- `psql` reachable at a **read-only** connection string for the target database.
- Never point this at production customer data. Use the authorized non-production
  environment per ADR-0008 (environment separation).

## 1. Review the exact command plan

```bash
python3 scripts/reconciliation_evidence.py plan
```

This prints every command (in `${PLACEHOLDER}` form) that the export will run. Review
it before exporting — nothing else runs.

## 2. Set the environment (identifiers are scrubbed from the bundle)

```bash
export GCP_PROJECT="<authorized-project-id>"
export GCP_REGION="us-central1"
export CLOUD_RUN_SERVICE="bicameral-cloud-staging"   # or the authorized service
export SUPABASE_DB_URL="postgresql://readonly:<...>@<host>:5432/postgres"
# Optional: also scrubbed from output if set
export SUPABASE_PROJECT_REF="<project-ref>"
export SUPABASE_DB_HOST="<host>"
```

`GCP_PROJECT`, `SUPABASE_PROJECT_REF`, and `SUPABASE_DB_HOST` are redacted wherever
they appear in the produced bundle.

## 3. Produce the evidence bundle

```bash
python3 scripts/reconciliation_evidence.py export \
  --environment staging \
  --description "authorized non-production reconciliation for Cloud #106" \
  --output evidence-bundle.json
```

- Facts that cannot be observed (missing credential/CLI/infrastructure) are recorded
  as `environment_inconclusive` — this is expected and is **not** a failure.
- Add `--fail-on-inconclusive` to make the command exit non-zero if any fact was
  inconclusive (useful once the environment is fully provisioned).

## 4. Validate and compare

```bash
python3 scripts/reconciliation_evidence.py validate evidence-bundle.json
python3 scripts/reconciliation_evidence.py compare evidence-bundle.json \
  --output reconciliation-report.json
```

The comparison report classifies every field as `agreement`, `drift`, `live_only`,
`tracked_only`, or `inconclusive`. `--fail-on-drift` exits non-zero when drift is
detected.

## 5. Attach to the gate

Attach `evidence-bundle.json` and `reconciliation-report.json` to
[cloud #106](https://github.com/BicameralAI/bicameral-cloud/issues/106). The tooling
redacts secret-shaped keys/values, JWTs, and the declared confidential identifiers
before anything is written, and every retained payload carries a `sha256:` digest.
Review the bundle before attaching. That attachment is the release evidence gate;
producing it is the only owner action remaining after cloud #141 merges.
