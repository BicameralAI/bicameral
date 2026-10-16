# Cloud Run IAM & Supabase reconciliation evidence tooling

Security-prep tooling for the **owner-only live evidence gate** tracked in
[cloud #106](https://github.com/BicameralAI/bicameral-cloud/issues/106) and the
Path-to-Alpha program in
[bicameral-factory #210](https://github.com/BicameralAI/bicameral-factory/issues/210).
Implemented for
[cloud #141](https://github.com/BicameralAI/bicameral-cloud/issues/141).

The goal is that when an authorized owner eventually runs the export against the
authorized live/non-production environment, the result is **deterministic**: every
command, parser, comparison, and CI assertion already exists and is reviewed. This
package needs no live GCP or Supabase credentials to run or parse — absence of a
credential, CLI, or infrastructure is a typed `environment_inconclusive` outcome,
never a false pass.

## What it produces

`scripts/reconciliation_evidence.py export` emits a machine-readable **evidence
bundle** (schema: [`evidence-bundle.schema.json`](evidence-bundle.schema.json)).
Each item records:

- the **exact command** (in `${PLACEHOLDER}` form — never a resolved secret);
- a **timestamp** and an **environment identifier**;
- a typed **status** (`collected` / `absent` / `environment_inconclusive` / `parse_error`);
- a **sanitized payload** and its **`sha256:` digest** (only when collected).

Collected facts (Cloud #106 required list):

| Area     | Facts |
|----------|-------|
| Cloud Run | service description, ingress, ready revision + traffic, runtime service account, invoker IAM policy |
| Edge      | external load-balancer backend services, IAP web IAM policy (both when present) |
| Supabase  | schemas, `public` tables, role table grants, default privileges, RLS enablement/forcing, RLS policies |
| Runtime   | SELECT/INSERT reachability of `public` tables for `anon`, `authenticated`, `service_role` |

## Comparator

`scripts/reconciliation_evidence.py compare <bundle.json>` maps a produced bundle
against the repository's tracked expectations and reports, per field:

- `agreement` — tracked and live agree;
- `drift` — tracked and live disagree (IAM, ingress, schema, grant, RLS);
- `live_only` — configuration observed live but not represented in the repository;
- `tracked_only` — repository configuration not observed live;
- `inconclusive` — the backing live evidence was inconclusive (no false pass).

Tracked expectations come from two honest sources:

- **Supabase** — derived directly from `supabase/migrations/*.sql` (tables, RLS
  enablement, and the fail-closed revoke-by-default principal posture).
- **Cloud Run / IAM** — the reviewed [`expected-config.json`](expected-config.json),
  where every field is explicitly pinned or `null` (unpinned).

## Redaction & non-goals

The produced bundle is safe to attach to #106: secret-shaped keys/values, JWTs,
and caller-declared confidential identifiers (`GCP_PROJECT`, `SUPABASE_PROJECT_REF`,
`SUPABASE_DB_HOST`) are scrubbed. The tooling is **read-only** — it never mutates
live IAM, ingress, schema, grants, or RLS, and it never requests or embeds live
credentials or customer data. Passing fixture-based CI does **not** claim the live
environment is secure; that judgement is the owner's, against #106.

## Factory threat-model & Road-to-Alpha reconciliation

- Parent live evidence gate: cloud #106 (owner-only live export remains a release
  evidence gate, not an implementation blocker).
- Path to Alpha: bicameral-factory #210.
- Governance owner and required checks: `.bicameral/repo-governance.yaml`
  (`classification: product-critical`, `runtime_role: authority-runtime`). This repo
  is the org's largest attack surface (hosted GCP Cloud Run + Supabase data plane);
  the revoke-by-default hardening migration in this change closes repository-known
  gaps that do not require the live export to identify.

See [`RUNBOOK.md`](RUNBOOK.md) for the exact owner command sequence.
