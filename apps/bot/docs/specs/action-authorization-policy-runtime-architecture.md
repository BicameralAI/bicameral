# Runtime Architecture: Action Authorization Policy

**Date:** 2026-06-24
**Status:** draft
**Level:** L2 cross-repo contract / L3 governance policy
**Owns:** The live runtime path for authenticating callers, classifying surface contracts, authorizing governed actions, materializing accepted events, and executing approved egress.
**Related context:** `docs/specs/action-authorization-policy.md`; ADR-0002, ADR-0007, ADR-0016, ADR-0018, ADR-0020, ADR-0022; RFQs BicameralAI/bicameral-bot#527, #528, BicameralAI/bicameral-integrations#200.
**Governance gate:** `bic:spec-governance-gate` advisory/no-op; Qor integration disabled pending factory policy revision.

## Reading Guide

Start with `docs/specs/action-authorization-policy.md`. The runtime invariant is: **authentication proves who or what is calling; authorization decides whether that caller may cause a specific state transition or external effect; neither surface identity nor target capability is authority by itself.**

## Runtime Problem

Today the runtime has strong contract language but uneven enforcement:

- `ToolRequest` carries `AuthorityContext`, but the dispatcher currently only rejects empty `actor_id`.
- Gateway dashboard commands can still apply review mutations without deriving a complete actor/capability context.
- External ingest correctly rejects authority-shaped fields, but the docs need a separate path for authenticated user actions arriving through external products.
- Egress RFQs need to know when approval happens, how target adapters are constrained, and what gets receipted.

Implementation workers need one runtime sequence so they do not independently invent authorization behavior for MCP, dashboard, integrations, and egress.

## Goals And Non-Goals

Goals:

- Define the runtime components and command/event flow needed for alpha action authorization.
- Preserve separate ingress contracts for local tool action, source evidence, authenticated remote review action, and egress.
- Define where authentication is converted into actor context.
- Define where policy is evaluated and where materialization/external effects happen.
- Define forbidden flows and failure behavior.

Non-goals:

- Do not implement hosted identity, enterprise RBAC, or multi-tenant auth.
- Do not make target API credentials equivalent to Product Owner approval.
- Do not make external target status map directly into Decision, signoff, binding, or compliance state.
- Do not require egress implementation before the policy/evaluator fixtures exist.

## Runtime Components

| Component | Responsibility | Inputs | Outputs | Must not decide/mutate |
| --- | --- | --- | --- | --- |
| Local surface adapter: MCP/CLI/dashboard/TUI | Build local `ToolRequest` with `AuthorityContext`; render responses/questions. | User/tool action, local session identity. | `ToolRequest`, display output. | Must not write canonical events or external targets directly. |
| External ingest gateway | Validate source adapter evidence payloads and reject authority injection. | `ExternalIngestEnvelope`, adapter/source credential if configured. | Source/evidence/candidate/logged-only results. | Must not accept review commands, actor authority, signoff, compliance, or policy grants. |
| Remote review action gateway | Verify signed external callbacks from Slack/Linear/etc. and derive actor context. | Provider callback, action id, actor identity, target id, nonce/signature. | Internal governed action request or rejected callback. | Must not treat source text, status, comment, or reaction as approval without verified actor action. |
| Egress request surface | Requests egress eligibility, preview, approval, or publish. | Local/remote actor action, canonical ref, target/profile request. | Policy request to egress service. | Must not call TargetAdapter directly. |
| Actor context resolver | Converts authenticated local/remote identity into actor context and capabilities. | `AuthorityContext`, gateway token, provider callback identity, actor binding. | `ActorContext`/capability set. | Must not infer Product Owner from target username without binding. |
| Action authorization evaluator | Central policy decision for action kind, target state, actor, capability, and surface category. | Surface category, actor context, target state, profile metadata, requested effect. | Allow/reject/defer/approval-required plus audit reason. | Must not materialize events or execute target writes. |
| Governance materializer | Appends accepted governance events and lets replay produce canonical state. | Allowed review/action command. | EventStore entry, replayed Decision Ledger/projection state. | Must not accept commands rejected by policy. |
| Egress projection service | Builds bot-owned EgressProjection from canonical state after eligibility and profile compatibility checks. | Canonical artifact, approved ProjectionProfile reference, policy result. | EgressProjection/preview or approved operation request. | Must not let profile grant permission or mutate canonical state. |
| TargetAdapter | Executes one approved target operation and returns target result. | Approved projection operation, target credential, target-specific payload. | Delivery result, receipt fields, target object refs. | Must not choose profile, alter policy, suppress required fields, or update Bicameral canonical state. |
| Reconciliation observer | Observes external target drift relative to receipts. | Target state/read events, receipts. | ReconciliationObservation/correction candidate. | Must not directly mutate Decision/signoff/compliance/binding state. |

## Canonical vs Derived State

| Artifact | Classification | Authoritative writer | Readers | Prohibited shortcut |
| --- | --- | --- | --- | --- |
| `Decision`, signoff, accepted BindingEvidence, compliance events | Canonical/replayed governance state | EventStoreAdapter after policy accepts event | Runtime, dashboard, MCP, search, egress | Surface route, adapter, or external status writes directly |
| `Source`, `SourceSnapshot`, `SourceEvidence` | Evidence | Ingest/runtime after source validation | Projection, review, lookup, egress | Evidence carries actor authority or signoff |
| `DecisionCandidate`, `BindingProposal` | Candidate/proposal | Projection/runtime | Review surfaces, policy | Candidate/proposal treated as approved Decision or BindingEvidence |
| `ProjectionProfile` | Target capability metadata | `bicameral-integrations` artifact/release | Bot policy/evaluation, TargetAdapter | Profile grants permission or waives approval |
| `EgressProjection` | Bot-owned projection payload | Egress projection service | TargetAdapter, receipt/audit, user preview | Adapter constructs arbitrary payload outside projection |
| `EgressReceipt` | Durable projection/external-effect receipt | Bot egress service from TargetAdapter result | Reconciliation, audit, dashboard | Receipt treated as canonical Decision truth or created without approved operation |
| `ReconciliationObservation` | Observation/candidate evidence | Reconciliation observer | Ingest/correction/review | External edit directly mutates canonical state |
| Runtime trace/log | Audit/receipt/log | Dispatcher/gateway/runtime | Operators/debugging | Log or trace becomes approval |

## State Refinement Check

Release state: a governed action request is either rejected before materialization, accepted into canonical governance events, routed for explicit approval, or executed as an approved egress operation with receipt.

States added/refined:

- `authorization_rejected`: policy/authentication/capability/state failure before materialization.
- `approval_required`: action is valid but needs explicit accountable actor approval.
- `egress_previewed`: derived projection preview with no external effect.
- `egress_dispatched`: approved outbound operation sent to TargetAdapter.
- `egress_receipted`: target operation returned a receipt/result.
- `reconciliation_observed`: external drift/target change observed as non-canonical evidence.

Transitions:

| From | To | Trigger | Authority | Persistence |
| --- | --- | --- | --- | --- |
| Request received | `authorization_rejected` | Missing auth, capability, valid state, or allowed surface | Runtime policy | Response/audit only |
| Request received | `approval_required` | Policy says explicit approval needed | Runtime policy | Review item/projection as applicable |
| Candidate proposed | Decision proposed | Authorized `review.accept_candidate` | `review:candidate` actor; policy | Event store |
| Decision proposed | Signoff approved | Authorized `review.approve_signoff` | Product Owner or delegate; policy | Event store |
| Eligible artifact | `egress_previewed` | Preview request allowed | Workspace read/member policy | Derived projection only |
| Approved egress action | `egress_dispatched` | Product Owner approval and profile compatibility | Bot egress policy | Egress operation record |
| `egress_dispatched` | `egress_receipted` | TargetAdapter returns result | Bot records receipt | Egress receipt/event/projection store |
| Target changed | `reconciliation_observed` | Adapter observes external drift | Adapter observation only | Observation/correction candidate |

Deferred states:

- Hosted/team actor sync.
- Multiple Product Owners.
- Automated egress approval.
- Target-specific compliance/signoff mapping.
- Bidirectional sync state.

## State Minimization Review

`authorization_rejected` can remain a response/audit outcome, not a durable domain state, unless repeated rejection history becomes product-facing.

`approval_required` is necessary because egress and remote review actions may be valid requests that still cannot execute without accountable approval.

`egress_previewed` is derived state only; it must not persist as authority.

`egress_dispatched` and `egress_receipted` are separated because external delivery can fail or partially complete. This distinction is necessary for retry and reconciliation.

`reconciliation_observed` is an observation, not a canonical mutation state. It exists to prevent external edits from being over-trusted.

## Authority Ownership Matrix

| Actor/component | observe | propose | validate | accept | materialize | project | notify/write external | mutate canonical |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| External source adapter | yes | evidence/candidate hints only | payload shape only | no | no | no | no | no |
| Workspace Member | yes | evidence, correction request, candidate review when delegated | no | member-authorized actions only | no | preview request | only if policy allows low-risk future class | no direct mutation |
| Product Owner | yes | yes | no | signoff, egress approval, source binding approval | no | approve projection | yes through bot egress path | no direct mutation |
| MCP/CLI/dashboard | yes | local ToolRequests | no | no | no | render preview | no direct external write | no |
| Remote review surface | yes | signed action callback | callback validity only | no | no | no | no direct external write | no |
| Action authorization evaluator | no | no | actor/action/state/profile | allow/reject/approval-required | no | no | no | no |
| EventStoreAdapter | no | no | event append validity | no | yes | no | no | yes, only accepted events |
| Egress projection service | canonical state | projection payload | eligibility/profile compatibility | no | egress records only | yes | approved request to adapter | no |
| TargetAdapter | target state | target result | target API response | no | no | no | approved operation only | no |

## Runtime Scenarios

### 1. Local MCP review action

```text
MCP tool
  -> ToolRequest { command=review.accept_candidate, AuthorityContext }
  -> dispatcher validates AuthorityContext shape
  -> actor context resolver maps local actor/capability
  -> action authorization evaluator checks surface=tool_request, action, target state
  -> if allowed: governance materializer appends AcceptCandidate event
  -> replay produces Decision(proposed)
```

Failure behavior: missing actor returns rejected response; missing capability returns policy rejection; candidate not proposed returns conflict/illegal state; no event is appended.

### 2. External source says "approve this"

```text
Linear issue body/comment
  -> ExternalIngestEnvelope
  -> external ingest gateway rejects forbidden authority fields
  -> otherwise captures text as SourceEvidence/candidate hint
  -> review surface may later show correction/contradiction pressure
```

Expected result: no approval, signoff, supersede, compliance, or egress operation occurs from source text alone.

### 3. Authenticated Slack or Linear approval action

```text
User clicks signed external action
  -> remote review action gateway verifies provider signature/nonce/action id
  -> actor context resolver maps external account to Bicameral actor and capabilities
  -> action authorization evaluator checks action and target state
  -> if allowed: materializer appends review event
```

Failure behavior: unknown external account, stale action id, replayed nonce, missing capability, or illegal target state fails before event append.

### 4. Egress preview

```text
Actor requests projection preview
  -> authorization evaluator checks read/preview policy
  -> egress projection service builds EgressProjection from canonical state
  -> response renders preview with canonical ref, authority label, profile id
```

Expected result: no TargetAdapter call, no external object mutation, no delivery receipt.

### 5. Egress external write

```text
Actor requests publish/create/update
  -> actor authenticated
  -> authorization evaluator checks required approval class
  -> egress service checks canonical artifact eligibility
  -> egress service validates ProjectionProfile compatibility
  -> Product Owner approval recorded when required
  -> egress service builds approved EgressProjection
  -> TargetAdapter executes exactly requested operation
  -> egress service records EgressReceipt/result
```

Failure behavior: unknown profile, profile incompatibility, missing target credential, missing approval, target failure, or adapter overreach yields no canonical Decision mutation. Partial target results require explicit failed/partial receipt semantics.

### 6. Reconciliation observation

```text
Target object changed
  -> adapter/reconciliation observer reads target state
  -> compares against EgressReceipt/canonical revision
  -> records ReconciliationObservation or correction candidate
  -> review/ingest path may surface it
```

Expected result: external edits never directly mutate Decision, signoff, BindingEvidence, compliance, or source truth.

### Forbidden flow

```text
ExternalIngestEnvelope { actor_id, review_command=approve_signoff }
  -> accepted as source evidence
  -> append ApproveSignoff
```

This must fail at raw ingest guard or schema validation. No event, receipt, or target effect may be produced.

## Event, Command, And Protocol Contracts

### Local `ToolRequest`

- Producer: MCP, CLI, dashboard, TUI.
- Consumer: ToolRequest dispatcher.
- Required fields: command, `AuthorityContext`, request id, issued time.
- Validation: actor id present, auth method supported, command payload valid, action policy permits actor/capability/state.
- Compatibility: protocol version through CapabilityReport.
- Exclusions: must not be used for external source ingest or egress target callbacks.

### `ExternalIngestEnvelope`

- Producer: source adapters/integrations.
- Consumer: external ingest gateway.
- Validation: raw forbidden-field scan, schema, size/rate/sensitive/canary guardrails, source binding/readiness where configured.
- Exclusions: actor authority, review commands, signoff, compliance, policy grants, canonical decisions, blocking governance results.

### Authenticated remote review action

- Producer: verified remote review surface callback.
- Consumer: remote review action gateway.
- Required fields, when schema-backed: action id, target id, action kind, target system, external actor id, provider signature/proof, nonce/issued time, callback source, optional reason.
- Validation: provider signature, anti-replay, actor binding, workspace/product scope, capability, target state, policy.
- Exclusions: source evidence payloads, target status-only events, unbound external identities.

### Egress request and operation

- Producer: authorized local/remote actor through bot egress API or review flow.
- Consumer: egress projection service and TargetAdapter.
- Required fields: canonical ref/type/revision, target system, requested profile id, operation, actor approval reference if required.
- Validation: eligibility, profile compatibility, approval class, target credential readiness, adapter supported operation.
- Exclusions: adapter-selected profile, arbitrary projection body, missing canonical ref, missing authority label, missing receipt behavior.

## Substrate Capability Matrix

| Substrate/surface | Identity check | Capability check | Canonical event write | Evidence ingest | External write | Offline/local |
| --- | --- | --- | --- | --- | --- | --- |
| Local MCP | MCP/session plus daemon local policy | Via `AuthorityContext`/policy | No direct write | Via `ToolRequest` | No direct write | yes |
| CLI | Local user/session | Via local actor context | No direct write | Via `ToolRequest` | No direct write | yes |
| Dashboard | Local gateway auth per ADR-0020 | Via actor context/capabilities | No direct write | via gateway/runtime | No direct write | yes |
| External ingest adapter | Adapter/source credential only | No actor capability | no | yes | no | depends on adapter |
| Remote review surface | Provider callback plus actor binding | yes | no direct write | no, unless separate ingest path | no direct write | hosted/online usually |
| Egress TargetAdapter | Target credential only | no policy authority | no | no | approved operation only | target-dependent |
| Event store substrate | n/a | n/a | yes, after policy | accepted events/evidence refs | no | substrate-dependent |

## Deployment And Runtime Topology

Alpha local-first topology:

```text
local MCP/CLI/dashboard
  -> local gateway/runtime
  -> action authorization evaluator
  -> event store adapter / egress projection service

external source adapter
  -> external ingest gateway
  -> evidence/candidate projection

future remote review callback
  -> gateway callback endpoint
  -> actor context resolver
  -> action authorization evaluator

egress projection service
  -> integrations-owned TargetAdapter
  -> external target
```

Secrets and provider tokens stay in approved local/runtime secret storage. Projection profiles may reference required scopes but must not contain secret material.

## Error, Retry, And Recovery Behavior

| Error | Runtime behavior |
| --- | --- |
| Missing actor for governed local action | Reject before payload mutation; no event. |
| Missing capability | Reject/403; no event. |
| Illegal target state | Conflict/409 or typed rejection; no event. |
| Source ingest carries authority fields | Reject raw payload; no evidence/candidate/event. |
| Remote action signature invalid | Reject callback; no event. |
| Remote actor not bound to Product | Reject/route setup; no event. |
| ProjectionProfile unknown or incompatible | Reject egress request; no target call. |
| Target credential lacks write scope | Reject or action-needed; no target call if known before dispatch. |
| Target call partially succeeds | Record partial/failed receipt semantics; do not mutate Decision truth. |
| Reconciliation sees external drift | Record observation/correction candidate; no canonical mutation. |

## Observability, Audit, And Receipts

Every governed action evaluation should record:

- request id / action id;
- surface category;
- actor id or adapter id;
- action kind;
- target kind/id;
- policy decision;
- rejection/approval reason;
- materialized event id or egress receipt id when applicable.

Runtime traces and receipts explain what happened. They do not themselves approve, sign off, prove compliance, or make external target state canonical.

## Security, Privacy, And Authority Boundaries

- Source ingest is untrusted content, even when the source system is authenticated.
- Remote review callbacks need signed provider proof, anti-replay, and a Bicameral actor binding.
- Target write credentials authorize Bicameral to call an API; they do not authorize a specific business action.
- Egress projections must avoid leaking forbidden internal confidence, unsupported advisory state, secrets, or unreviewed candidate text into external targets.
- Unknown or unimplemented commands must fail closed. They must not return success or accepted audit-only no-ops.

## Implementation Sequencing

### Phase 1: Policy model and fixtures

- Target: `bicameral-bot`.
- Authority sections: Action Authorization Matrix, Event/Command Contracts.
- Acceptance: fixture coverage for allowed and forbidden surface/action pairs.
- Validation: `cargo test --workspace --locked` plus schema fixture checks.
- Task class: `AFK_REVIEW_ONLY`.

### Phase 2: Central evaluator in runtime/gateway

- Target: `bicameral-bot`.
- Authority sections: Runtime Components, Authority Ownership Matrix.
- Acceptance: dispatcher/gateway call one evaluator before materialization.
- Validation: negative tests for missing actor/capability/state/surface.
- Task class: `AFK_REVIEW_ONLY`.

### Phase 3: Remote review action contract draft

- Target: `bicameral-bot` plus `bicameral-integrations` RFQ input.
- Authority sections: Authenticated remote review action contract.
- Acceptance: a Slack/Linear-style callback cannot be confused with source ingest.
- Validation: contract fixtures.
- Task class: `HITL_BLOCKED` until Kevin fills integration-specific action payload and provider proof details.

### Phase 4: Egress eligibility and receipt preflight

- Target: `bicameral-bot`.
- Authority sections: Egress request and operation, Egress scenarios.
- Acceptance: preview and publish are different paths; publish requires approval and receipt.
- Validation: unit tests with fake TargetAdapter.
- Task class: `AFK_REVIEW_ONLY` after RFQ acceptance.

## Issue Generation Notes

Create agent tasks only after the draft spec is accepted enough to slice:

1. Add `ActionAuthorizationPolicy` structs/fixtures and test matrix.
2. Route runtime review mutations through evaluator.
3. Patch dashboard command route to stop direct mutation and unknown-command acceptances.
4. Add remote review callback RFQ/schema draft.
5. Add egress eligibility/preview/publish fake adapter tests.

Do not create implementation tasks for full remote identity, production OAuth, or bidirectional sync yet.

## ADR Implications

- ADR-0002: needs an amendment for authenticated remote review/action callbacks as separate from source ingest and egress.
- ADR-0007: reaffirm; add action authorization evaluator before accepted review event/materialization.
- ADR-0020: reaffirm and generalize; local gateway actor context becomes one input to the central evaluator.
- ADR-0022: reaffirm Product Owner as alpha signoff and egress-write approval authority unless policy delegates a narrower action.
- New ADR may be warranted when remote action callback becomes a durable public protocol.

## Acceptance Criteria

- Every governed mutation path calls authorization before event append or target write.
- `ExternalIngestEnvelope` cannot cause review/signoff/compliance/egress authority.
- Authenticated remote review actions have a separate runtime path from source ingest.
- Egress preview produces no external effect.
- Egress publish/update/create requires eligibility, profile compatibility, approval if required, adapter execution, and receipt.
- TargetAdapter cannot choose profile, waive approval, mutate canonical state, or reinterpret target status as Bicameral truth.
- Failure paths create no canonical mutation unless a receipt explicitly records an attempted/failed egress operation.

## Validation Plan

- Runtime tests for `review.accept_candidate` and `review.approve_signoff` with missing actor, missing capability, wrong target state, and allowed path.
- External ingest tests for top-level and nested forbidden authority fields.
- Remote review callback fixtures for invalid signature, replayed nonce, unbound actor, missing capability, allowed action.
- Egress fake-adapter tests for preview-only, missing approval, unknown profile, adapter overreach, target failure, successful receipt.
- Trace/audit assertions for actor id, action kind, policy decision, and event/receipt id.

## Open Questions

1. Should `review.accept_candidate` require Product Owner in alpha, or can Workspace Members with `review:candidate` promote into `Decision(proposed)`?
2. Should `AuthMethod` grow a remote callback variant, or should remote callbacks resolve into a separate internal actor context without changing `ToolRequest`?
3. Should EgressReceipt be event-store-backed in alpha or initially projection-store-backed with event-store migration planned?

## Request Coverage

| Request | Runtime answer |
| --- | --- |
| Use `bic:to-runtime-spec`. | Runtime components, state classifications, scenarios, protocols, substrate matrix, failures, and validation are defined. |
| Define what is strictly necessary for existing RFQs. | Runtime scope is limited to action classification, remote review separation, egress approval/write flow, and adapter/profile restrictions. |
| Think through MCP/tool vs egress authentication. | MCP uses local `ToolRequest`/actor context; egress uses approved outbound operation plus target credential, not target credential as actor approval. |
| Preserve ingest/tool/egress separation. | Separate runtime paths and forbidden flows are explicit. |
