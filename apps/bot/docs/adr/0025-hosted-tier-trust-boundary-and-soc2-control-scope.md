# ADR-0025: Hosted-Tier Trust Boundary and SOC 2 Control Scope

**Date:** 2026-07-02
**Status:** accepted
**Level:** L2 cross-repo contract
**Related:** ADR-0003 (local code grounding / cloud conflict boundary), ADR-0007 (governance flow), ADR-0020 (gateway authority boundary), ADR-0023 (installation/setup state machine); `docs/releases/alpha/hosted-cloud-responsibility.md`; `docs/specs/cloud-hosted-dashboard-local-daemon-runtime-architecture.md`; BicameralAI/bicameral-daemon#7, #9; BicameralAI/bicameral-bot#547, #226; `SOC2-READINESS.md`

## Problem

The imminent "hosted/team tier" forced a SOC 2 question: which application-level
controls apply to it? The phrase was load-bearing but ambiguous — it named two
different trust boundaries with very different control scope:

- A **hosted coordination shell** that renders/coordinates while canonical data
  stays local (already committed in `hosted-cloud-responsibility.md` and the
  cloud-hosted-dashboard spec, already partially shipped in alpha).
- A **server-side shared store** of canonical/Confidential decision data for
  team access (never committed; would reverse the invariants above).

Opening control tickets before fixing the boundary would either over-scope
(build multi-tenant controls for data that never leaves the operator's machine)
or under-scope (miss the controls that do apply). daemon#7 and daemon#9 tracked
the impact but conflated hosted mode with shared-deployment mode.

## Decision

1. **Data locality — coordination-only.** The hosted tier keeps the committed
   invariant: hosted cloud may discover, preview, relay, advise, coordinate, and
   render; it never holds Confidential data (no Decision Ledger events, provider
   secrets, source bodies, local paths, or code indexes). Canonical authority
   and secret custody remain in the paired Local daemon. Reaffirms ADR-0003 /
   ADR-0007 / ADR-0020 and `hosted-cloud-responsibility.md`.

2. **Deployment model — daemon-per-member.** A team is a shared `Product` in the
   event store; each `Product Member` (a `Cloud account`) runs their own Local
   daemon. The OS-user trust boundary (bot#547) remains valid, so the MCP
   transport auth-shim (daemon#9 Track 2) stays **deferred** — there is no
   shared daemon custodying multiple members' secrets. There is no canonical
   `Organization`, `Workspace`, or `Team` entity: "company" is a non-canonical
   billing label on a Cloud account. A canonical org/tenancy layer is the same
   reversal shape as decision (6).

3. **In-scope hosted controls** (attach to already-shipping surfaces —
   `crates/bicameral-api/src/hosted_bridge.rs`, cloud#7 token broker):
   - TLS cipher grade in transit (browser ↔ cloud, cloud ↔ daemon bridge).
   - GitHub App token-broker secret handling (app private key stays in the cloud
     secret boundary; short-lived installation tokens only; no key material in
     responses or logs).
   - Hosted-bridge pairing/session auth (origin, CSRF, replay, expiry,
     revocation) — this *is* daemon#9's auth requirement for the hosted path.
   - Non-canonical metadata retention (session/pairing/preview/team metadata,
     broker audit identifiers).

4. **Out-of-scope — operator responsibility** (documented as accepted scope
   statements, not built): backup encryption, RTO/RPO, and scheduled retention
   of the Decision Ledger. This data lives in the operator's own event store
   (Git/Drive), covered by endpoint full-disk encryption and the availability
   stance (bot#226).

5. **Activation trigger.** The in-scope controls are audit-attested before the
   **first paying hosted/team customer**. Accepted residual risk: an alpha/free
   external user could pair through the hosted bridge before attestation; the
   blast radius is bounded because — per decision (1) — the cloud holds no
   Confidential data.

6. **Deferred reversal.** A server-side shared canonical store (multi-tenant
   SaaS) is explicitly out of scope. Adopting it requires a new ADR reversing
   decision (1) plus the full multi-tenant control cluster (tenant isolation,
   encryption at rest, backup encryption, RTO/RPO, retention enforcement,
   tenant-scoped audit). It is **not** opened speculatively; it is gated behind a
   customer contract that contractually requires it.

## Consequences

- daemon#7 becomes the single umbrella (retitled), folding daemon#9. No new
  build tickets are opened in bot/mcp/cloud/integrations now.
- The in-scope controls become an attestation checklist
  (`docs/specs/hosted-tier-soc2-control-design.md`) gated on the commercial
  trigger, mostly verifying already-shipping surfaces rather than building.
- The `Cloud account`, `Product Member`, and `Hosted cloud` glossary terms are
  canonicalized in `CONTEXT.md` (and "Workspace"/"Team"/"Organization" retired as
  tenancy terms) to prevent future drift back toward a server-side store or a
  canonical org hierarchy.

## Alternatives considered

- **Server-side shared store (multi-tenant SaaS).** Rejected: reverses committed
  invariants, is the largest possible SOC 2 + engineering scope, and has no
  current demand. Kept available behind decision (6).
- **Shared / hosted daemon serving multiple members.** Rejected: reintroduces
  the secret-custody problem the local-authority model was designed to avoid and
  invalidates the OS-user trust boundary, forcing daemon#9 Track 2 immediately.
