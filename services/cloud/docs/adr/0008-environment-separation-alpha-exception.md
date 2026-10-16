# Environment Separation — Alpha Exception with Staging In Flight

**Status:** accepted

**Related:** #43 (application-level SOC 2 controls), #44 (merge-policy / branch protection), BicameralAI/bicameral#131 (privacy invariant), ADR-0001 (cloud oracle, not authority), ADR-0006 (cloud auth/tenancy/database boundary)

## Context

The SOC 2 operations-security control asks that production be segregated from dev/staging so that changes are validated outside production, production data cannot leak into a lower environment, and a bad deploy does not land directly on customers.

Today bicameral-cloud runs a **single GCP project** (`bicameral-cloud-500911`) and a **single Cloud Run service** (`bicameral-cloud`) that *is* production. There is no deployed non-production environment; "dev" is an integration **git branch**, and only a push to `main` deploys. Every promotion to `main` therefore serves the live `app.bicameral-ai.com` directly, with no place to smoke-test the built image first.

## Decision

**Accept a single-environment (branch-gated) posture for alpha as a documented SOC 2 exception, and concurrently authorize staging-environment setup to proceed in parallel ("in flight").** The exception is time-boxed by an explicit revisit trigger, not open-ended.

The exception is justified by **compensating controls**, not by a second environment:

- The cloud coordination store is **in-memory** (`src/bicameral_cloud/store.py`); Supabase holds only auth users + the design-partner allowlist. There is no persistent production customer-data pool to leak downward.
- ADR-0001: the cloud is an **oracle, not authority** — canonical data and compliance outcomes stay with the local daemon.
- ADR-0006 keeps customer Confidential data out of the cloud entirely.
- Cloud Run `min-instances 0 / max-instances 3`: small blast radius; a bad deploy scales to zero and is rolled back by redeploy.

**Revisit trigger (the exception expires at the earlier of):**
1. this service is about to house **customer Confidential data** — the same design gate inherited from bicameral#131; or
2. onboarding **GA / paying customers** who depend on production uptime.

A staging environment must be live **before** either trigger fires.

## Staging plan (in flight)

Because Cloud Run scales to zero, a staging environment is cheap; the only real work is the data plane. Target design:

- A second Cloud Run service **`bicameral-cloud-staging`** in the same project, deployed on push to `dev`.
- A **separate Supabase project** for staging (its own auth users, allowlist, anon key, service-role key, and its own `supabase-service-key-staging` Secret Manager secret) — so staging never touches production data.
- Deploy workflow parameterized by branch (`main` → prod service + prod secrets; `dev` → staging service + staging secrets). The scaffolding lands **inert**: the staging job is guarded by a `STAGING_ENABLED` repository variable and staging-specific secrets, so it is a no-op until provisioning is complete — merging it cannot break `dev` CI or the production path.
- Optional dovetail with #44: a GitHub Environment with a required-reviewer rule on production, so promotion to `main` requires review (today it deploys on unreviewed push).

Provisioning (new Supabase project, staging service + secrets, flipping `STAGING_ENABLED`) requires GCP/Supabase console access and is tracked as owner/console work.

## Consequences

- Production continues to deploy on push to `main` until #44 adds a review gate; this ADR does not change that path.
- Once staging is provisioned and enabled, `dev` pushes deploy to `bicameral-cloud-staging`, giving a real pre-production smoke-test target with no production-data exposure.
- The exception is **auditable and bounded**: an auditor sees an accepted exception with compensating controls and a concrete expiry, not an indefinite gap.
- The attestation (`docs/governance/soc2-application-controls.md` §1) records this decision as the disposition of the environment-separation control.

Governance gate: bic:spec-governance-gate advisory/no-op; Qor integration disabled pending factory policy revision.
