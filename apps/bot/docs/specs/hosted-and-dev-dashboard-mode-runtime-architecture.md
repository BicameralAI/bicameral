# Runtime Architecture: Production Hosted Mode And Dev Dashboard Mode

- **Date:** 2026-07-08
- **Status:** draft
- **Level:** L2 (cross-repo contract: `bicameral-cloud` ↔ browser ↔ `bicameral-bot` gateway/daemon)
- **Owns:** the runtime contract that names the two supported dashboard runtime modes — production `hosted` mode and `dev_dashboard_mode` — declares production local dashboard mode deprecated/non-product, and fixes which setup, discovery, and projection surfaces are product versus dev-only.
- **Related context:**
  - `docs/specs/cloud-served-dashboard-direct-gateway-data-plane-runtime-architecture.md` (data plane for the served SPA; source of the `DashboardDataMode = "local" | "hosted" | "fixture_fallback"` values this contract reclassifies).
  - `docs/specs/cloud-hosted-dashboard-local-daemon-runtime-architecture.md`, `docs/specs/cloud-hosted-dashboard-parity-feature-spec.md`, `docs/specs/alpha-hosted-dashboard-ui.md`, `docs/specs/hosted-bridge-user-level-pairing.md`.
  - `docs/specs/explicit-project-registration-hosted-local-discovery.md` (registration/discovery authority split this contract reuses verbatim).
  - `bicameral-cloud/docs/adr/0005-hosted-dashboard-shell-local-daemon-authority.md`, `bicameral-cloud/docs/adr/0006-cloud-auth-tenancy-and-database-boundary.md`, `bicameral-cloud/docs/adr/0007-hosted-bridge-routing-and-discovery-boundary.md`.
  - `bicameral-cloud/docs/specs/design-partner-signup-runtime-architecture.md`, `bicameral-cloud/docs/specs/design-partner-signup.md`.
  - `CONTEXT.md` (`Hosted cloud`, `Cloud account`, `Product`, provider-based project discovery note, issue #344).
  - Shipped surfaces referenced (not modified here): `dashboard/src/App.svelte`, `dashboard/src/lib/hosted-bridge-types.ts` (`DashboardDataMode`), `dashboard/src/pages/SetupWizard.svelte`, `dashboard/src/lib/project-fixtures.ts`, `crates/bicameral-gateway/src/routes.rs` (`/api/v1/dashboard/*`, `hosted_bridge_routes`).
  - Source issue: BicameralAI/bicameral-bot#716. Blocks #717, #718, and bicameral-cloud project-discovery contract work.
- **Governance gate:** `bic:spec-governance-gate` advisory/no-op; Qor integration disabled pending factory policy revision. Authority-sensitive surfaces touched (as contract only): hosted/local boundary, project discovery source, projection read model, setup lifecycle. State Refinement Checkpoint and Minimization Review are recorded below.

---

## Reading Guide

Read `Runtime Invariant`, `Mode Definitions`, `Product vs Dev-Only Surfaces`, and `State Refinement Checkpoint` first. This is a **contract/spec only**: it names modes and fixes authority; it wires no code (see Non-Goals).

**Main runtime invariant:**

```text
Production dashboard access is hosted mode: the SPA is served by Hosted cloud,
project discovery comes from Cloud/Supabase, and all workspace/projection/
canonical authority stays in the paired Local daemon. Local same-origin
dashboard mode (SetupWizard, /api/v1/dashboard/* same-origin APIs, local
project fixtures) is dev_dashboard_mode: a developer-only convenience, not a
product runtime, unless a future ADR explicitly re-admits a local product mode.
```

---

## Runtime Problem

The dashboard SPA carries two runtime shapes today, and `DashboardDataMode` (`dashboard/src/lib/hosted-bridge-types.ts`) enumerates `"local" | "hosted" | "fixture_fallback"` without a product-level statement of which are product runtimes. This leaves three ambiguities that downstream issues (#717, #718, bicameral-cloud project discovery) would otherwise each resolve independently and inconsistently:

1. **Is "local" a product runtime?** The same-origin local dashboard (SetupWizard create-workspace path, `GET /api/v1/dashboard/ledger`, `GET /api/v1/dashboard/ingestion-gate`, local project fixtures) still reads as a first-class product surface, even though the accepted hosted-cloud direction (ADR-0005) makes hosted the browser-reachable product plane. Without an explicit deprecation, later work keeps a competing local-product identity model — the same failure `explicit-project-registration-hosted-local-discovery.md` warned about for project identity.
2. **Where do production projects come from?** Provider-based project discovery (CONTEXT.md, #344) retired implicit `.bicameral/` discovery, and hosted work introduced Cloud/Supabase preview/discovery, but no contract states that Cloud/Supabase is *the* production project source while the local daemon stays workspace/projection authority.
3. **Which surfaces are dev-only?** SetupWizard, same-origin `/api/v1/dashboard/*`, and local project fixtures have no durable "dev-only unless a future ADR allows" boundary, so they risk being treated as supported product entry points.

This spec resolves the ambiguity by naming the modes and fixing the product boundary. It changes no wiring.

---

## Goals And Non-Goals

### Goals

- G1. Define two named dashboard runtime modes: production `hosted` mode and `dev_dashboard_mode`.
- G2. Declare production local (same-origin) dashboard mode deprecated / non-product.
- G3. Fix `SetupWizard`, local same-origin dashboard APIs (`/api/v1/dashboard/*`), and local project fixtures as **dev-only unless a future ADR explicitly re-admits them** as product.
- G4. State that Cloud/Supabase project discovery is the production project source, while the Local daemon remains authority for workspace binding, projection truth, canonical mutation, and Decision Ledger acceptance.
- G5. Record a State Refinement Checkpoint that maps the modes onto the already-shipped dashboard-session lifecycle without inventing new lifecycle states.

### Non-Goals

- N1. **No implementation wiring.** No code, type, route, or component changes. `DashboardDataMode` keeps its current variants; this spec only classifies them.
- N2. **No Supabase schema migration** and no new Supabase tables, columns, or auth changes.
- N3. **No removal** of SetupWizard, `/api/v1/dashboard/*`, or project fixtures. They remain available for dev; only their product classification changes.
- N4. **No new tenancy vocabulary.** No `Workspace`/`Team`/`Organization` canonical entities (retired by ADR-0025 / CONTEXT.md).
- N5. **No new project-discovery protocol.** This names Cloud/Supabase as the production source; the discovery wire contract is downstream (bicameral-cloud project-discovery work, blocked-by-this).
- N6. **No hosted governed-mutation** enablement; hosted read-only posture from `alpha-hosted-dashboard-ui.md` is unchanged.

---

## Mode Definitions

### `hosted` mode (production)

Production browser access to the dashboard.

- The SPA is served by Hosted cloud (`bicameral-cloud`, `/app`); Supabase authenticates the `Cloud account` and applies the design-partner allowlist.
- **Project discovery source:** Cloud/Supabase (hosted preview/discovery + design-partner signup). This is the production answer to "which projects can I see and pick".
- **Data plane:** browser ↔ Local daemon direct, over the origin-allowlisted, session-token-gated, CORS+PNA hosted-bridge routes (`/api/v1/hosted-bridge/*`). Hosted cloud is never in the data path and never holds Confidential data (ADR-0005; `cloud-served-dashboard-direct-gateway-data-plane-runtime-architecture.md`).
- **Authority:** the Local daemon remains authoritative for project registration, workspace binding, projection truth, canonical mutation, and Decision Ledger acceptance. Cloud/Supabase discovery is non-canonical preview/routing metadata (`HostedProjectPreview`); it is not project authority.
- Maps to `DashboardDataMode = "hosted"`. `fixture_fallback` is a degraded rendering attribute of hosted mode (empty/unreachable projection), not a separate product mode.

### `dev_dashboard_mode` (developer-only)

The same-origin local dashboard shape, retained for local development and testing only.

- The SPA is served locally and talks **same-origin** to the local gateway's `/api/v1/dashboard/*` routes; `SetupWizard` create-workspace flow and local project fixtures are available.
- **Not a product runtime.** It is a developer convenience for exercising the daemon and dashboard without hosted cloud.
- Carries no production project-discovery guarantee: fixtures and same-origin reads are dev scaffolding, not the production project source.
- Maps to `DashboardDataMode = "local"`. The `"local"` value is retained for dev but is **classified non-product** by this contract.

### Deprecated: production local dashboard mode

Treating the same-origin local dashboard (`dev_dashboard_mode` surfaces) as a **production** product runtime is **deprecated / non-product**. Production dashboard access is hosted mode. Re-admitting a local product mode requires a future ADR that explicitly supersedes this classification and re-defines its authority, discovery source, and redaction posture.

---

## Product vs Dev-Only Surfaces

| Surface | Classification | Product mode | Rationale |
| --- | --- | --- | --- |
| Hosted-served SPA + Supabase auth (`/app`, `/dashboard`) | Product | `hosted` | Accepted hosted direction (ADR-0005). |
| `/api/v1/hosted-bridge/*` redacted projections (browser↔daemon direct) | Product | `hosted` | Redacted, origin/token-gated read plane. |
| Cloud/Supabase project discovery/preview | Product (project **source**, non-canonical) | `hosted` | Production project source; not project authority. |
| Local daemon workspace binding / projection / Decision Ledger | Product (authority) | both | Canonical authority stays local regardless of mode. |
| `SetupWizard` (create-workspace path) | **Dev-only** unless future ADR | `dev_dashboard_mode` | Local same-origin setup is dev convenience, not product entry. |
| Same-origin `/api/v1/dashboard/*` (e.g. `ledger`, `ingestion-gate`) | **Dev-only** unless future ADR | `dev_dashboard_mode` | Loopback/local-surface only; not the network-origin product read plane. |
| Local project fixtures (`dashboard/src/lib/project-fixtures.ts`, `HOSTED_BRIDGE_FIXTURES`) | **Dev-only** unless future ADR | `dev_dashboard_mode` | Fixtures are dev scaffolding, not a production project source. |

"Dev-only unless a future ADR explicitly allows them" is the durable classification: these surfaces may keep existing and running locally, but they must not be presented, documented, or wired as production product runtimes without an ADR reversal.

---

## Canonical vs Derived State

This contract adds no stateful artifacts. It classifies discovery/authority sources for the two modes:

| Artifact | Classification | Authoritative writer | Allowed readers | Prohibited shortcut |
| --- | --- | --- | --- | --- |
| Decision Ledger events | Canonical / accepted event | Local daemon (post policy-accept + EventStoreAdapter) | Local daemon replay; redacted subset to browser via hosted bridge | Cloud/Supabase or same-origin dev route never writes canonical events |
| `ProjectRegistration` | Operator-local state | Local daemon | Local daemon, paired hosted UI via daemon | Cloud/Supabase discovery treated as project authority |
| Cloud/Supabase project discovery (`HostedProjectPreview`) | Derived preview / routing metadata (production project **source**) | Hosted cloud / Supabase | Hosted UI, paired daemon on user selection | Treated as event-store truth or workspace authority |
| Redacted projections (`DashboardProjectionEnvelope`) | Derived read model | Local daemon (bridge redaction) | Paired browser only, token-gated | Persisted/cached cloud-side |
| Local project fixtures | Dev scratch/fixture | Dev checkout | Dev SPA (`dev_dashboard_mode`) only | Rendered as production project data |
| Same-origin `/api/v1/dashboard/*` payloads | Local/loopback surface | Local daemon | Local dev SPA only | Fetched cross-origin from the cloud origin as a product read plane |

---

## State Refinement Checkpoint

- **Release state (smallest honest lifecycle):** the production dashboard session runs the already-shipped hosted lifecycle `hosted_dashboard_unpaired → hosted_dashboard_paired → (reads redacted projections) → hosted_dashboard_revoked | hosted_dashboard_upgrade_required | expired` (from `cloud-served-dashboard-direct-gateway-data-plane-runtime-architecture.md` and `cloud-hosted-dashboard-local-daemon-runtime-architecture.md`). This contract **adds no new lifecycle state.** It attaches a **product/dev classification attribute** to the existing `DashboardDataMode` values: `"hosted"` = product, `"local"` = `dev_dashboard_mode` (non-product), `"fixture_fallback"` = degraded attribute of hosted rendering.
- **States added/split/merged/refined:** none added. `hosted` and `dev_dashboard_mode` are **mode labels/classifications over existing `DashboardDataMode` values**, not new lifecycle states. The project-registration/discovery lifecycle (`unregistered → pending_registration → registered_project → registration_repair_required → normal_project_running`) from `explicit-project-registration-hosted-local-discovery.md` is reused unchanged; this contract only names Cloud/Supabase as the production discovery input to that lifecycle.
- **Transition triggers / authority:** unchanged from the referenced hosted specs. Mode is selected at SPA load (hosted handoff present ⇒ `hosted`; local same-origin serve ⇒ `dev_dashboard_mode`); the Local daemon remains the authority that validates pairing, registration, and projection reads.
- **Persistence:** no new persistence. Pairing session (daemon memory, expiring), bridge token (browser memory, transient), `ProjectRegistration` (operator-home), canonical events (local event store). Cloud/Supabase discovery metadata is non-canonical hosted state.
- **Invariants later refinements must preserve:**
  1. Production dashboard access is hosted mode; production local dashboard mode is deprecated/non-product.
  2. Cloud/Supabase project discovery is the production project **source**, never project/workspace/canonical **authority**.
  3. The Local daemon remains authoritative for workspace binding, projection truth, canonical mutation, and Decision Ledger acceptance in both modes.
  4. `SetupWizard`, same-origin `/api/v1/dashboard/*`, and local project fixtures are dev-only unless a future ADR explicitly re-admits them as product.
  5. Naming `hosted`/`dev_dashboard_mode` adds no lifecycle state and no persistence; it is a classification over existing `DashboardDataMode` values.
- **Deferred vocabulary:** a future re-admitted local product mode; a canonical cloud project store; hosted governed-mutation confirmation states; a hosted project-discovery wire contract (downstream bicameral-cloud work). Each is deferred because its authority, persistence, and recovery behavior are undefined for this release.

## State Minimization Review

- **Reviewed:** yes.
- **New states rejected/collapsed:**
  - A distinct `dev_dashboard_mode` **lifecycle state** was rejected. Mode is a classification attribute over the existing `DashboardDataMode = "local"` value and the existing hosted session lifecycle, not a new authority/persistence/recovery state.
  - A separate `production_local_mode` state was rejected outright: it is the exact posture being deprecated; representing it as a live state would contradict the deprecation invariant.
  - `fixture_fallback` was **not** promoted to a mode: it stays a degraded-rendering attribute of hosted mode.
- **Deferred without lying:** a re-admitted local product mode and a hosted project-discovery wire contract are genuinely out of scope for a contract-only PR; deferring them keeps the state set minimal and honest.
- **Rationale:** the smallest honest change is a naming + classification contract over the already-shipped hosted lifecycle and existing `DashboardDataMode` values. Introducing new states would overstate what a docs-only PR changes and would pre-commit downstream issues to unproven lifecycle shapes.

---

## Authority Ownership Matrix

| Actor / component | observe | discover (project source) | validate | project | render | mutate (canonical) | notes |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Hosted cloud / Supabase | hosted account/session | ✓ production project **source** (non-canonical preview) | hosted identity/allowlist only | ✕ | serves SPA | ✕ | never Confidential data; never project/workspace authority |
| Dashboard SPA (browser) | ✓ redacted projections (hosted) | selects from discovered previews | ✕ | ✕ | ✓ | ✕ | holds bridge token in memory only |
| Local daemon (hosted bridge + core) | ✓ | validates selected registration | ✓ pairing/registration/policy | ✓ builds + redacts projections | — | ✓ via governed command path | authoritative for workspace/projection/canonical state |
| Dev SPA + `/api/v1/dashboard/*` + fixtures | ✓ local (dev) | ✕ (fixtures are not a product source) | ✕ | ✕ | ✓ (dev only) | ✕ | `dev_dashboard_mode`; non-product |

---

## Runtime Scenarios

### S1. Production project discovery + open (hosted mode)

- **Trigger:** Cloud account opens the hosted dashboard.
- **Flow:** Hosted cloud serves SPA + Supabase auth → Cloud/Supabase presents discovered project previews (production source) → user selects a project → hosted handoff seeds the SPA → SPA pairs with the paired Local daemon → daemon validates `ProjectRegistration` → SPA renders redacted projections pulled browser↔daemon direct.
- **Authority boundary:** discovery preview (cloud) → registration/projection authority (daemon). Cloud discovery never becomes project authority.
- **Result:** `hosted` mode, `DashboardDataMode = "hosted"`.

### S2. Dev use (dev_dashboard_mode)

- **Trigger:** developer serves the SPA locally.
- **Flow:** SPA runs same-origin against local `/api/v1/dashboard/*`; `SetupWizard` create-workspace path and local project fixtures are available.
- **Result:** `dev_dashboard_mode`, `DashboardDataMode = "local"`. Explicitly **non-product**; carries no production project-discovery guarantee.

### S3. FORBIDDEN flow (must be rejected by downstream implementation)

1. Presenting/documenting/wiring `dev_dashboard_mode` surfaces (`SetupWizard`, same-origin `/api/v1/dashboard/*`, fixtures) as a **production** dashboard runtime without an ADR that supersedes this classification.
2. Treating Cloud/Supabase discovery as project/workspace/canonical **authority** instead of a non-canonical production project **source**.
3. Fetching same-origin `/api/v1/dashboard/*` cross-origin from the cloud origin as the network-origin product read plane (that plane is the redacted `hosted-bridge` projection).
4. Rendering local project fixtures as production project data in hosted mode.

---

## ADR Implications

- **Reaffirmed:** `bicameral-cloud` ADR-0005 (hosted dashboard shell, local-daemon authority), ADR-0007 (hosted-bridge routing and discovery boundary), ADR-0006 (cloud auth/tenancy/database boundary). This contract is consistent with all three: hosted rendering + Cloud/Supabase discovery, local canonical authority.
- **Extends (no contradiction):** `explicit-project-registration-hosted-local-discovery.md` — names Cloud/Supabase as the production discovery input to the existing registration lifecycle.
- **Missing / candidate ADR:** the deprecation of production local dashboard mode is durable and hard-to-reverse. If bicameral-bot maintains local ADRs for this boundary, a short ADR ("Production dashboard access is hosted mode; local dashboard mode is dev-only") may be warranted. Flagged as a follow-up, not created here (contract-only scope).
- **Reversal cost:** re-admitting a local product mode is a deliberate reversal requiring a future ADR that redefines its authority, discovery source, and redaction posture.

---

## Acceptance Criteria

- AC1. Contract names `hosted` (production) and `dev_dashboard_mode` explicitly and maps each to a `DashboardDataMode` value. *(Mode Definitions)*
- AC2. Production local dashboard mode is stated deprecated / non-product. *(Mode Definitions → Deprecated)*
- AC3. `SetupWizard`, same-origin `/api/v1/dashboard/*`, and local project fixtures are classified dev-only unless a future ADR explicitly allows them. *(Product vs Dev-Only Surfaces)*
- AC4. Cloud/Supabase project discovery is named the production project source while the Local daemon remains workspace/projection authority. *(Mode Definitions, Authority Ownership Matrix, S1)*
- AC5. A State Refinement Checkpoint is recorded, adding no new lifecycle state and passing minimization review. *(State Refinement Checkpoint, State Minimization Review)*
- AC6 (negative). No implementation wiring and no Supabase schema migration are introduced by this PR. *(Non-Goals; diff is docs-only)*

## Validation Plan

- Documentation review of this spec against issue #716 acceptance criteria (mapping in Acceptance Criteria above).
- Deterministic terminology check: `rg "local mode|dev_dashboard_mode|hosted" docs dashboard/src README.md` — confirms the new mode vocabulary is present in docs and consistent with existing dashboard usage.
- No build/test surface changes (contract-only). Downstream issues (#717, #718, bicameral-cloud project discovery) carry implementation validation.

## Open Questions

- OQ1. Should the deprecation be promoted to a standalone bicameral-bot ADR now, or remain a spec-level classification until the first downstream implementation PR? Consequence if unresolved: the deprecation stays spec-level and downstream issues cite this spec rather than an ADR.
- OQ2. Final name of the hosted project-discovery wire contract (bicameral-cloud). Consequence if unresolved: this spec references "Cloud/Supabase project discovery" generically; the downstream contract fixes the concrete shape.

## Request Coverage

| Issue #716 request | Answered in |
| --- | --- |
| Runtime contract names hosted vs dev mode explicitly | Mode Definitions; AC1 |
| Production local mode marked deprecated/non-product | Mode Definitions → Deprecated; AC2 |
| SetupWizard / local same-origin dashboard APIs / local fixtures dev-only unless future ADR | Product vs Dev-Only Surfaces; AC3 |
| Cloud/Supabase discovery as production source; daemon workspace/projection authority | Mode Definitions; Authority Ownership Matrix; S1; AC4 |
| State refinement checkpoint recorded | State Refinement Checkpoint; State Minimization Review; AC5 |
| No implementation wiring; no Supabase schema migration | Non-Goals; AC6 |
