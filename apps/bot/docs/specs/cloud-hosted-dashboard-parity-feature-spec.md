# Feature Proposal: Hosted Dashboard Parity Over Local Daemon Projections

Date: 2026-06-26

Status: draft

Level: L2 cross-repo contract

Owns: the product and implementation plan for replicating the `bicameral-bot` dashboard in `bicameral-cloud` without transferring Bicameral authority to cloud.

Related context: `docs/specs/cloud-hosted-dashboard-local-daemon-runtime-architecture.md`, `docs/specs/hosted-bridge-user-level-pairing.md`, `docs/specs/explicit-project-registration-hosted-local-discovery.md`, `docs/releases/alpha/hosted-cloud-responsibility.md`, BicameralAI/bicameral-cloud#5, BicameralAI/bicameral-cloud#7, BicameralAI/bicameral-cloud#8.

Governance gate: `bic:spec-governance-gate` advisory/no-op; Qor integration disabled pending factory policy revision.

## Reading Guide

Read `Decision Or Proposed Direction`, `Dashboard Surface Contract`, and `Implementation Plan` first.

```text
Replicate the dashboard experience in hosted cloud, but source dashboard truth from local daemon projections and route mutations through local daemon authorization.
```

## Problem

The existing cloud issues are individually valid, but they do not describe the actual product scope if the hosted web experience must replicate the `bicameral-bot` dashboard. A Cloud Run skeleton, token broker, and boundary RFQ are sub-slices of a larger hosted dashboard parity program.

If these issues remain as the primary work plan, workers may build infrastructure that is technically conformant but insufficient for product parity, or they may overcorrect by moving dashboard authority into cloud.

## Goals

- Treat hosted dashboard parity as the main cloud product surface.
- Reuse the bot dashboard's concepts, flows, and local daemon contracts.
- Split dashboard display from dashboard authority.
- Define the surface inventory and projection/action contracts before implementation issues are generated.
- Close the generic cloud issues as superseded by this spec and narrower RFQs.

## Non-Goals

- No standalone cloud governance runtime.
- No cloud-owned Decision Ledger, project registration, local workspace binding, signoff, compliance, no-conflict, or BindingEvidence.
- No cloud persistence of provider secrets, local paths, source bodies, private code indexes, or raw event-store contents.
- No requirement to remove the local dashboard.
- No implementation of every dashboard action before read-only parity is available.

## Decision Or Proposed Direction

Use a cloud-hosted dashboard shell with local-daemon-owned authority.

This is a conventional approach for products that need browser accessibility plus local secrets, filesystem access, and privileged control. The hosted side provides account/team/session coordination and UI delivery. The local daemon provides policy enforcement, canonical mutations, provider/local workspace access, and projections. Bicameral's added constraint is that all governance-adjacent behavior remains local-daemon/materialization owned.

## State Refinement Check

Release state: hosted cloud can render the dashboard shell, pair to a local daemon, negotiate supported dashboard surfaces, show read-only projections for supported screens, and submit only daemon-authorized actions for compatible surfaces.

States added/refined:

- `hosted_dashboard_unpaired`
- `hosted_dashboard_paired`
- `hosted_dashboard_degraded`
- `hosted_dashboard_upgrade_required`
- `hosted_dashboard_revoked`

Transitions and persistence are defined by `docs/specs/cloud-hosted-dashboard-local-daemon-runtime-architecture.md`.

Invariants:

- A hosted dashboard session is not project membership.
- A displayed projection is not a canonical write.
- A hosted action request is not an accepted event.
- A cloud receipt is not governance authority.

Deferred vocabulary:

- cloud-canonical project;
- hosted workspace binding;
- hosted source/code cache;
- team-wide local daemon policy.

## State Minimization Review

The proposal reuses existing bridge and project/workspace states where possible. It adds only dashboard-specific degraded and upgrade-required states because dashboard parity creates partial-surface operation and version mismatch cases that are not represented by simple paired/unpaired bridge states.

Rejected:

- `hosted_dashboard_authoritative`
- `cloud_project_registered`
- `hosted_local_workspace_bound`

These conflict with existing hosted/local responsibility boundaries.

## Existing Infrastructure Inventory

| Existing asset | Reuse |
| --- | --- |
| `dashboard/` | Source of dashboard UX inventory and surface parity target |
| `crates/bicameral-api/src/dashboard.rs` | Existing dashboard DTO/read-model vocabulary |
| `crates/bicameral-gateway/src/routes.rs` dashboard routes | Existing local dashboard API behavior |
| Hosted bridge route family | Pairing/session guard and browser-origin route base |
| `docs/specs/hosted-bridge-user-level-pairing.md` | User/project/workspace state split |
| `docs/specs/explicit-project-registration-hosted-local-discovery.md` | Explicit registration and hosted preview boundary |
| `docs/domain/runtime-protocol.md` | Capability and protocol vocabulary |
| `crates/bicameral-api/src/capability.rs` | Feature support and migration metadata pattern |

## Dashboard Surface Contract

Each dashboard screen must be inventoried before cloud implementation. The inventory row is the implementation contract:

| Field | Meaning |
| --- | --- |
| `surface_id` | Stable dashboard surface identifier |
| Current local route/component | Existing bot dashboard implementation |
| Projection source | Local daemon projection/read-model endpoint |
| Actions | Supported action ids, if any |
| Authority owner | Local daemon service that validates action |
| Cloud cache class | none, metadata-only, receipt-only, or approved projection cache |
| Redaction requirements | Fields excluded from hosted payload |
| Degraded behavior | UI behavior when unsupported/stale/unpaired |
| Validation | Fixture, gateway test, UI test, or manual check |

Initial surface groups:

- setup and pairing;
- project list and explicit project registration;
- provider connection readiness;
- local workspace binding and repair;
- ledger/projection views;
- review/candidate/signoff flows;
- compliance/readiness panels;
- source/egress receipts and reconciliation;
- bridge status and diagnostics.

## Contract Shape

Primary runtime contracts are defined in `docs/specs/cloud-hosted-dashboard-local-daemon-runtime-architecture.md`:

- `HostedDashboardHandshake`
- `DashboardProjectionEnvelope`
- `DashboardActionRequest`
- `DashboardActionReceipt`
- typed compatibility and failure errors

Feature-level exclusions:

- no provider refresh tokens;
- no GitHub App private key/client secret outside approved cloud secret boundary;
- no local absolute paths by default;
- no source bodies/private code by default;
- no raw event-store bodies by default;
- no authority-smuggling fields in action payloads.

## Workflow Mapping

| Workflow | Hosted cloud behavior | Local daemon behavior |
| --- | --- | --- |
| Open dashboard | Render shell, hosted session, previews | No authority until pairing |
| Pair daemon | Coordinate pairing prompt | Validate origin/session/operator, persist local bridge session |
| Select project | Render previews and local projects | Validate explicit `ProjectRegistration` |
| Read dashboard | Request projection envelope | Build projection from local read model/event replay |
| Mutate dashboard | Submit action request | Authorize, dispatch, materialize if accepted |
| Provider readiness | Render non-secret readiness | Store/validate secrets and provider access |
| Version mismatch | Hide/degrade/upgrade prompt | Report capability/schema support |

## Trust, Privacy, Security, And Authority Boundaries

Cloud may render and coordinate. Cloud must not become the authority for canonical Bicameral claims.

The local daemon is authoritative for local actor mapping, explicit project registration, local workspace binding, provider secret custody and readiness validation, governed action authorization, accepted event materialization, and projection truth.

Cloud is authoritative only for hosted account/session state, hosted team/invite/project preview metadata, hosted UI delivery, and optional non-secret hosted receipts.

## Per-Surface Behavior

`bicameral-cloud`:

- renders hosted dashboard shell;
- stores hosted account/team/session metadata;
- coordinates pairing;
- calls local bridge routes;
- renders degraded/upgrade-required states from daemon capability metadata;
- does not synthesize canonical Bicameral state.

`bicameral-bot`:

- owns hosted bridge routes;
- owns local projection construction;
- owns action authorization and command dispatch;
- owns local secrets, event-store access, local workspace binding, and canonical materialization.

`bicameral-api`:

- should hold shared dashboard bridge DTOs/fixtures where cross-repo contract reuse is needed.

## Implementation Plan

1. **Dashboard inventory and fixtures**
   - Target: `bicameral-bot`
   - Output a machine-readable or markdown surface inventory.
   - Acceptance: every existing dashboard route/view mapped to projection/action/cache/redaction behavior.
   - Task class: `AFK_REVIEW_ONLY`

2. **Bridge handshake and version contract**
   - Target: `bicameral-bot`, `bicameral-cloud`
   - Implement dashboard protocol/schema/version negotiation fixtures.
   - Acceptance: old/new compatibility cases produce deterministic typed outcomes.
   - Task class: `AFK_REVIEW_ONLY`

3. **Read-only hosted dashboard shell**
   - Target: `bicameral-cloud`
   - Port dashboard navigation and read-only projected screens.
   - Acceptance: paired daemon projections render; unpaired/unsupported states do not show fake data.
   - Task class: `AFK_REVIEW_ONLY`

4. **Governed action routing**
   - Target: `bicameral-bot`, `bicameral-cloud`
   - Wire supported dashboard actions through local daemon action authorization.
   - Acceptance: no action can bypass local policy; stale projections are rejected or refreshed.
   - Dependencies: authenticated remote action callbacks and bridge authz-policy routing.
   - Task class: `AFK_REVIEW_ONLY`

5. **Provider token broker support**
   - Target: `bicameral-cloud`
   - Implement GitHub App installation token broker as a credential amenity.
   - Acceptance: app secrets stay in approved cloud secret boundary; tokens are short-lived and never governance authority.
   - Task class: `AFK_REVIEW_ONLY`

6. **Dashboard parity conformance**
   - Target: cross-repo
   - Add fixture/conformance tests for each surface group, including privacy and version-desync cases.
   - Task class: `AFK_REVIEW_ONLY`

## ADR Implications

- ADR-0002 reaffirmed: dashboard remains a surface over bot runtime contracts.
- ADR-0003 reaffirmed: local grounding remains local.
- ADR-0007 reaffirmed: canonical state materializes through substrate-neutral governance.
- ADR-0020 reaffirmed/amended: hosted bridge dashboard actions need explicit action authorization routing.
- ADR-0023 reaffirmed: setup, registration, and workspace binding remain separate states.
- Possible new ADR: hosted dashboard compatibility support window and cloud cache policy if accepted as durable product/release policy.

## Issue Generation Notes

Close existing `bicameral-cloud` issues as follows:

- `#5` superseded by this spec's service shell and hosted dashboard sequencing. Reopen only as a narrow Cloud Run skeleton implementation issue if no equivalent infra issue exists.
- `#7` superseded as a sub-slice of provider readiness/token broker support. Reopen only as a narrow broker implementation issue after cache/secret policy is accepted.
- `#8` answered by the runtime architecture spec and this feature spec. Remaining decisions should be RFQs, not a broad boundary issue.

Create RFQs for:

- hosted dashboard version support window;
- hosted projection cache/retention policy;
- customer-owned hosted domain/origin governance.

## Validation Plan

- Markdown review against responsibility doc and existing hosted bridge specs.
- Dashboard inventory review against `dashboard/` and gateway routes.
- Contract fixture tests for handshake/projection/action/receipt.
- Gateway tests for pairing guard and authority-smuggling rejection.
- Privacy tests for forbidden fields in projection payloads and logs.
- UI tests for unpaired, degraded, upgrade-required, and local-daemon-offline states.

## Open Questions

- What compatibility support window should cloud maintain for older local daemons?
- Which projection payload classes, if any, may be cached in cloud?
- Are customer-owned hosted domains and custom origin allowlists part of alpha?

## Request Coverage

- Sanity check conventionality: answered in `Decision Or Proposed Direction`.
- Close existing cloud issues: mapped in `Issue Generation Notes`.
- Push specs to main: this spec and the runtime spec are intended for `main` via protected-branch PR.
- Create RFQs: listed in `Open Questions` and `Issue Generation Notes`.
