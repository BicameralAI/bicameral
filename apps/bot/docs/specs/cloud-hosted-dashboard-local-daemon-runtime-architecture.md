# Runtime Architecture: Cloud-Hosted Dashboard With Local-Daemon Authority

Date: 2026-06-26

Status: draft

Level: L2 cross-repo contract

Owns: the runtime contract for presenting the Bicameral dashboard from hosted cloud while keeping canonical authority in the paired local daemon.

Related context: BicameralAI/bicameral-cloud#5, BicameralAI/bicameral-cloud#7, BicameralAI/bicameral-cloud#8, BicameralAI/bicameral-bot#535, BicameralAI/bicameral-bot#539, BicameralAI/bicameral-bot#577-#582, `docs/releases/alpha/hosted-cloud-responsibility.md`, `docs/specs/hosted-bridge-user-level-pairing.md`, `docs/specs/explicit-project-registration-hosted-local-discovery.md`, `docs/domain/runtime-protocol.md`, ADR-0002, ADR-0003, ADR-0007, ADR-0020, ADR-0023.

Governance gate: `bic:spec-governance-gate` advisory/no-op; Qor integration disabled pending factory policy revision.

## Reading Guide

Read `Runtime Invariant`, `Canonical vs Derived State`, `Protocol Contracts`, and `Forbidden Flows` first.

```text
Hosted cloud may render and coordinate the dashboard. The paired local daemon owns authority, secrets, local workspace access, canonical mutation, and projection truth.
```

## Runtime Problem

The hosted cloud issues originally described a hosted amenity service, GitHub App token broker, and hosted/local boundary RFQ. That is too small if the product goal is to replicate the `bicameral-bot` dashboard as a hosted web experience.

Without this contract, implementation slices could accidentally create a second dashboard authority surface in `bicameral-cloud`, duplicate local daemon policy checks, cache sensitive local projections in cloud, or let cloud version drift silently break local daemon workflows.

The conventional architecture for this product shape is a cloud-hosted dashboard shell paired to a local daemon. Cloud provides reachability, identity/session coordination, team previews, and browser delivery. The local daemon remains the agent with filesystem access, provider secrets, event-store access, action authorization, and canonical materialization. Bicameral's added constraint is that the shell never becomes a canonical Decision Ledger writer.

## Goals

- Replicate the bot dashboard UX through hosted cloud without moving canonical authority into cloud.
- Define the daemon/cloud projection, action, pairing, and compatibility contract.
- Preserve local-only dashboard operation as a first-class fallback.
- Make version desynchronization explicit and fail-closed for unsupported authority paths.
- Keep provider tokens, local paths, source bodies, local code indexes, and event-store contents out of cloud unless a later approved spec creates a narrower exception.

## Non-Goals

- No cloud-owned Decision Ledger writes.
- No cloud-owned canonical project settings.
- No direct cloud mutation of signoff, compliance, no-conflict, BindingEvidence, SourceEvidence, or SourceBinding.
- No cloud storage of local provider refresh tokens, local filesystem paths, private source text, graph indexes, or event-store bodies.
- No requirement that every dashboard screen becomes interactive in the first implementation slice.

## Runtime Components

| Component | Responsibility | Persistent state | Must not decide or mutate |
| --- | --- | --- | --- |
| Hosted dashboard shell | Render dashboard chrome, route screens, coordinate hosted session, call paired daemon bridge | Hosted session, team/invite/preview metadata, allowed UI preferences | Canonical project settings, Decision Ledger events, local workspace binding, source/signoff/compliance authority |
| Hosted discovery/session service | Team/project preview and pairing coordination | Non-canonical preview/session metadata | Event-store truth, local operator identity, provider secret custody |
| Hosted bridge route family | Browser-origin route family on bot gateway for paired hosted access | Operator-home pairing/session records, expiry/revocation receipts | Bypass local action authorization, external ingest, direct ToolRequest fallback |
| Local daemon | Validate operator, project registration, provider access, workspace binding, action authorization, event-store materialization, projections | Operator home, project registry, local caches, local receipts | Hosted team membership, cloud account truth |
| EventStoreAdapter | Persist accepted canonical project settings and Decision Ledger events | Git, Drive, or future event-store artifacts | Hosted pairing, local UI sessions, advisory cloud receipts |
| Provider connection service | Store and validate local provider connections | Non-secret connection metadata plus local secrets | Source binding, project identity, event-store authority |
| Local workspace/code binding | Explicit local code folder relation for grounding and preflight | Operator-local binding metadata and caches | Project identity, BindingEvidence, canonical source truth |
| GitHub App token broker | Optional hosted credential amenity for installation tokens | Cloud secret boundary only, token issue receipts | Source binding, Decision authority, long-lived local token custody |

## Canonical vs Derived State

| Artifact | Classification | Authoritative writer | Prohibited shortcut |
| --- | --- | --- | --- |
| Decision Ledger events | Accepted canonical events | EventStoreAdapter after bot governance accepts a command | Hosted dashboard writes events directly |
| Event-store project descriptor | Canonical or substrate-authoritative project setting | Selected event-store substrate | Hosted preview supersedes descriptor |
| ProjectRegistration | Operator-local state | Local daemon setup/registration path | Cloud treats preview/team membership as registration |
| HostedProjectPreview | Derived preview/routing metadata | Hosted discovery service | Treat as event-store truth |
| DashboardProjectionEnvelope | Derived read model | Local daemon projection reader | Cloud constructs projection from cached fragments |
| DashboardActionRequest | Proposed action | Hosted dashboard or local dashboard | Treat request as accepted event |
| DashboardActionReceipt | Audit/receipt/log | Local daemon | Treat receipt as governance approval |
| CapabilityReport | Compatibility/readiness metadata | Local daemon | Treat capability as governance authority |
| Provider token | Secret credential | Provider connection service or hosted broker | Persist in dashboard state, descriptors, logs, comments, fixtures |
| Local path/source body/graph index | Local private data | Local daemon/workspace services | Store in cloud projection cache by default |

## State Refinement Check

Release state: a hosted browser can render the Bicameral dashboard shell, pair with a local daemon, negotiate dashboard capabilities, read daemon-owned projections for supported screens, and submit only actions the daemon advertises and authorizes. Unsupported, stale, or unsafe paths fail closed with typed repair/upgrade guidance.

States added or refined:

- `hosted_dashboard_unpaired`: hosted shell loaded, no local daemon pairing.
- `hosted_dashboard_paired`: hosted shell has a valid bridge session to a local daemon.
- `hosted_dashboard_degraded`: paired session exists, but one or more dashboard surfaces are unavailable.
- `hosted_dashboard_upgrade_required`: hosted shell and daemon cannot safely interoperate for the requested surface.
- `hosted_dashboard_revoked`: prior pairing cannot be reused.

Invariants:

- Pairing is not project membership, project selection, workspace binding, source authority, signoff, event-store authority, or permission bypass.
- Degraded dashboard state is not compliance state.
- Capability support is not governance authority.
- Hosted receipts are not accepted events.

Deferred vocabulary:

- `cloud_dashboard_canonical_project`
- `hosted_workspace_binding`
- `cloud_projection_authority`
- `cloud_source_cache`
- `team_wide_local_daemon_policy`

## State Minimization Review

`hosted_dashboard_unpaired`, `hosted_dashboard_paired`, and `hosted_dashboard_revoked` reuse the existing bridge lifecycle. `hosted_dashboard_degraded` is needed because a paired dashboard may be partially usable when a specific projection, action, provider, project, or workspace capability is absent. `hosted_dashboard_upgrade_required` is needed for protocol or schema incompatibility where rendering stale controls would be unsafe.

Rejected or collapsed states:

- `hosted_dashboard_readonly` collapses into per-surface capability metadata.
- `hosted_dashboard_offline` collapses into unpaired or degraded depending on whether a session exists.
- `cloud_dashboard_authoritative` is rejected by the responsibility boundary.

## Runtime Scenarios

### Startup And Pairing

```text
hosted dashboard shell
  -> hosted session service: load account/team/project previews
  -> local bridge route: request pairing
  -> local daemon: validate origin, local operator, CSRF/replay, bridge capability
  -> hosted dashboard shell: paired or typed rejection
```

### Projection Read

```text
hosted dashboard screen
  -> /api/v1/hosted-bridge/projection/<surface>
  -> bridge guard validates session
  -> local daemon projection reader validates project/workspace/readiness
  -> local daemon returns DashboardProjectionEnvelope
  -> hosted shell renders payload according to schema_version and capabilities
```

Projection reads never append Decision Ledger events.

### Governed Action

```text
hosted dashboard control
  -> DashboardActionRequest
  -> hosted bridge route validates browser-origin session
  -> local daemon action authorization validates actor, capability, project state, workspace state, and command policy
  -> governed handler accepts/rejects/deferred/unsupported
  -> EventStoreAdapter materializes accepted canonical mutation
  -> local daemon returns DashboardActionReceipt plus refreshed projection pointer
```

### Version Desynchronization

```text
hosted shell declares supported dashboard_protocol versions
local daemon declares hosted_bridge_protocol_version,
  dashboard_projection_schema_version,
  supported projection surfaces,
  supported action surfaces,
  migration/deprecation metadata
hosted shell intersects capabilities
```

If no compatible intersection exists, the shell must not render mutable controls for that surface.

### Forbidden Flow

```text
hosted dashboard
  -> cloud service writes Decision Ledger event
```

This is forbidden. Accepted events can only be materialized by the selected event-store path after local daemon governance accepts the command.

## Event, Command, And Protocol Contracts

### HostedDashboardHandshake

Purpose: negotiate whether hosted shell and local daemon can safely operate a dashboard session.

Request fields:

- `request_id`
- `hosted_session_id`
- `origin`
- `dashboard_protocol_versions_supported`
- `projection_schema_versions_supported`
- `action_schema_versions_supported`
- `requested_surfaces`
- `csrf_nonce`

Response fields:

- `hosted_bridge_protocol_version`
- `daemon_build_id`
- `pairing_state`
- `project_state`
- `workspace_state`
- `supported_surfaces`
- `unsupported_surfaces`
- `degraded_surfaces`
- `migration_notes`
- `expires_at`

Validation: origin must be allowed, session must be paired or pairable, version intersection must be explicit, and response must not include local paths, provider tokens, source bodies, or event-store bodies.

### DashboardProjectionEnvelope

Fields:

- `request_id`
- `surface_id`
- `projection_schema_version`
- `project_ref`
- `read_model_revision`
- `freshness`
- `capabilities`
- `payload`
- `redactions`
- `caveats`
- `next_actions`

Privacy exclusions: no provider secrets, no local absolute paths by default, no source bodies/private code by default, no raw event-store body by default.

### DashboardActionRequest

Fields:

- `request_id`
- `surface_id`
- `action_id`
- `action_schema_version`
- `project_ref`
- `read_model_revision_seen`
- `idempotency_key`
- `arguments`
- `user_intent_summary`

Validation: valid pairing, daemon-supported action, current or refreshable read-model revision, local actor mapping through policy, and authority-smuggling rejection.

### DashboardActionReceipt

Fields:

- `request_id`
- `idempotency_key`
- `status`
- `denial_reason`
- `accepted_event_refs`
- `read_model_revision_after`
- `receipt_ref`
- `warnings`

`accepted_event_refs` may identify accepted events, but the receipt itself is not the accepted event.

## Failure, Recovery, And Compatibility

Typed failures required:

- `not_paired`
- `pairing_revoked`
- `pairing_expired`
- `origin_rejected`
- `csrf_rejected`
- `protocol_unsupported`
- `projection_schema_unsupported`
- `surface_unsupported`
- `action_unsupported`
- `project_unregistered`
- `registration_repair_required`
- `workspace_unbound`
- `workspace_repair_required`
- `provider_readiness_missing`
- `projection_stale`
- `wrong_daemon`
- `wrong_project`
- `authority_denied`

Compatibility policy:

- Protocol mismatch fails closed.
- Unsupported surfaces render degraded, readonly, or hidden based on daemon classification.
- Mutable controls require explicit action support and schema compatibility.
- Hosted cloud may support a bounded compatibility window, but the exact support window is an RFQ until product/release owner decides it.

## Security, Privacy, Authority Boundaries

Cloud may observe hosted account/team/session metadata, project preview metadata approved for hosted display, pairing relay metadata, non-secret readiness/account/installation metadata, and redacted projection metadata explicitly allowed by the daemon.

Cloud may decide hosted session validity, hosted team/invite display, and whether to render or hide hosted UI shell affordances.

Cloud may not decide local actor authority, project registration validity, workspace binding validity, source binding, BindingEvidence, signoff, compliance, no-conflict, or accepted Decision status.

Local daemon may decide local actor mapping, project registration and repair state, workspace binding and graph readiness, action authorization, and projection truth from event replay/local read models.

## Implementation Sequencing

1. Contract fixtures for handshake, projection, action, receipt, and typed errors.
2. Bridge capability handshake with dashboard protocol and surface support.
3. Projection surface inventory for every existing bot dashboard route/view.
4. Read-only hosted dashboard parity from daemon projections.
5. Governed action routing through local action authorization.
6. Version-desync conformance for old/new shell and daemon combinations.

## ADR Implications

- ADR-0002 reaffirmed: dashboards remain agent surfaces that call bot-owned runtime contracts.
- ADR-0003 reaffirmed and clarified: cloud-hosted dashboard shell does not move local code grounding to cloud.
- ADR-0007 reaffirmed: accepted canonical state still materializes only through substrate-neutral governance flow.
- ADR-0020 reaffirmed and amended by implementation detail: hosted bridge route family needs dashboard-specific capability, projection, and action contracts.
- ADR-0023 reaffirmed: setup state remains explicit operator/project/workspace split.
- Missing ADR likely needed if product accepts a durable hosted-dashboard compatibility support window.

## Acceptance Criteria

- Hosted dashboard cannot render mutable controls unless daemon reports compatible action capability.
- Cloud never writes Decision Ledger events or canonical project settings.
- Projection responses exclude provider secrets, local paths, source bodies, private code, and raw event-store bodies by default.
- Version desynchronization produces typed degraded or upgrade-required outcomes.
- Local dashboard remains usable without hosted cloud.
- Dashboard action receipts distinguish proposed request, accepted event refs, and non-authoritative logs.

## Validation Plan

- Contract fixture tests for handshake, projection, action, receipt, typed errors.
- Gateway tests for unpaired, revoked, expired, origin rejected, and authority-smuggling rejection.
- Cross-version fixture replay for old/new shell and daemon combinations.
- Privacy tests that assert forbidden fields are absent from projection payloads and logs.
- Manual review of dashboard surface inventory against current `dashboard/` implementation before parity implementation begins.

## Open Questions / RFQs

- What hosted dashboard compatibility support window must cloud honor before requiring daemon upgrade?
- May cloud cache any dashboard projection payloads, or only non-secret metadata/receipts?
- Are customer-owned hosted domains part of alpha, and who approves origin allowlists?
