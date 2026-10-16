# Action Authorization Policy

**Date:** 2026-06-24
**Status:** draft
**Level:** L2 cross-repo contract / L3 governance policy
**Owns:** The minimum shared authorization matrix for governed Bicameral actions across local tool requests, source ingest, authenticated remote review actions, and future egress.
**Related context:** ADR-0002, ADR-0007, ADR-0016, ADR-0018, ADR-0020, ADR-0022; `docs/specs/configurable-inline-governance-review.md`; `docs/specs/external-adapter-ingest-gateway.md`; RFQs BicameralAI/bicameral-bot#527, BicameralAI/bicameral-bot#528, BicameralAI/bicameral-integrations#200.
**Governance gate:** `bic:spec-governance-gate` advisory/no-op; Qor integration disabled pending factory policy revision.

## Reading Guide

Read ADR-0002 first for the three existing surface contracts, ADR-0007 for the governance flow, and ADR-0020 for the gateway actor/capability precedent. The invariant is: **a surface may observe, propose, or request, but only bot-owned policy may accept a governed action and materialize authority or external effect.**

## Problem

Bicameral already says `ToolRequest`, `ExternalIngestEnvelope`, and `Egress` are separate contract categories. The current RFQs need one sharper layer: which actions require which actor authority, and how that policy applies when the same business action appears through MCP, dashboard, Slack/Linear, integrations, or future egress.

Without an action authorization policy:

- source-adapter ingest can be mistaken for user-approved review;
- authenticated integration callbacks can be incorrectly rejected as "evidence only";
- projection profiles can accidentally behave like policy;
- egress can create external work without a governed approval path;
- `accept_candidate` and `approve_signoff` can remain conflated even though they change different authority axes.

## Goals And Non-Goals

Goals:

- Define a minimal action taxonomy that unblocks the current egress and integration RFQs.
- Separate source evidence submission from authenticated user action through an integration.
- Define the default alpha authorization for candidate promotion, signoff approval, source binding, correction capture, and egress write operations.
- Make ProjectionProfile and TargetAdapter incapable of granting permission.
- Provide implementation slices for a central policy evaluator without requiring full hosted RBAC.

Non-goals:

- Do not implement enterprise RBAC, OAuth, SSO, JWT, SCIM, or org-chart routing.
- Do not make external ingest a review-command surface.
- Do not make egress a `ToolRequest` or `ExternalIngestEnvelope`.
- Do not decide every future egress operation. This spec defines defaults and hard boundaries.
- Do not make target systems such as Linear, Jira, GitHub, Slack, docs, email, or CI canonical Bicameral state.

## Decision Or Proposed Direction

Introduce an internal **Action Authorization Policy** that evaluates governed action requests after the caller has entered through its proper surface contract:

| Surface contract | Purpose | Authority posture |
| --- | --- | --- |
| `ToolRequest` | Local MCP, CLI, dashboard, or TUI action/query under local actor/session authority. | May request governed mutation when `AuthorityContext` and policy permit. |
| `ExternalIngestEnvelope` | External/source-system evidence submission. | Evidence-only; cannot carry actor authority, review commands, signoff, compliance, policy grants, or canonical mutations. |
| Authenticated remote review action | Signed callback or action from an external review surface, such as Slack modal, Linear action, or future hosted UI. | May request governed mutation only after bot verifies actor identity, workspace binding, target state, and capability. Must not reuse `ExternalIngestEnvelope`. |
| `Egress` | Outbound publication, notification, sync, annotation, or work-item creation. | May execute external effect only after bot-owned eligibility, policy, approval if required, and receipt generation. |

The policy evaluates an internal action tuple:

```text
surface_category
authenticated_actor optional
adapter_or_connection_ref optional
action_kind
target_kind
target_id
requested_external_effect optional
target_state
requested_profile_id optional
evidence_summary optional
```

This tuple is an internal policy input, not a new public API. Each public contract keeps its separate shape.

## State Refinement Check

Release state: governed actions have explicit authorization classes before they can promote candidates, approve signoff, accept source bindings, capture correction requests, or execute egress writes.

States added or refined:

| State / term | Refinement |
| --- | --- |
| `ActionAuthorizationPolicy` | Internal policy table/evaluator for action class, required actor, lifecycle state, capability, approval class, and effect. |
| `Authenticated remote review action` | New contract category for user-approved actions initiated from an external surface. It is not source ingest and not egress. |
| `Egress approval class` | Policy classification for whether an outbound effect requires Product Owner approval, member confirmation, or no approval because it is preview-only. |

Transitions:

| Transition | Authorized actor/component | Persistence |
| --- | --- | --- |
| Evidence submission accepted | Gateway/daemon after surface validation | `Source`, `SourceSnapshot`, `SourceEvidence`, candidate/projection state, ingest event as applicable |
| Candidate promoted to Decision(proposed) | Actor with `review:candidate` capability; alpha default Product Owner or delegated Workspace Member | Accepted event; replayed Decision Ledger state |
| Decision signoff approved | Product Owner or explicit Product Owner delegate with `review:signoff` | Accepted signoff event; replayed Decision Ledger state |
| SourceBinding approved | Product Owner or explicit source-admin delegate | Project/source binding configuration or accepted setup event |
| Correction capture requested | Workspace Member may request; Product Owner reviews resulting corpus update | Evidence/candidate/correction path, not direct Decision mutation |
| Egress preview generated | Workspace Member or Product Owner according to read policy | Derived preview; no external effect |
| Egress external write executed | Product Owner approval by default for alpha; narrower policy exceptions require explicit config and audit | Egress projection/receipt state plus external target result |

Invariants:

- `approve_signoff` is ownership approval; it is not compliance or enforcement.
- `accept_candidate` creates `Decision(proposed)`; it is not signoff approval.
- External ingest may propose evidence or candidates; it cannot carry user authority.
- Authenticated remote review actions may carry user authority only through a verified action callback, never through source evidence fields.
- ProjectionProfile declares target shape/capability; it cannot grant permission or waive approval.
- TargetAdapter executes an approved operation; it cannot decide eligibility, mutate projection content, or interpret external state as canonical.

Deferred vocabulary:

- Full hosted RBAC.
- Multi-owner products.
- Target-specific action semantics such as "Linear Done means compliant".
- Durable `ChannelRoute`.
- Automated low-risk egress write approval beyond explicit alpha policy.

## State Minimization Review

`ActionAuthorizationPolicy` is needed because existing `Governance policy` is too broad for implementation workers to infer command-level authorization, but it remains a policy layer rather than a new lifecycle object.

`Authenticated remote review action` is needed because calling all integrations "evidence-only" would incorrectly forbid a user from approving through Slack/Linear, while putting review callbacks into `ExternalIngestEnvelope` would create an authority smuggling path.

`Egress approval class` is needed because preview, notification, comment, status update, work-item creation, and document publication have different external consequences. In alpha, the default can still be conservative: external writes require Product Owner approval unless an explicit policy class narrows that later.

Rejected/collapsed states:

- `IntegrationAuthority`: rejected. It conflates source adapters, provider auth, remote review callbacks, and egress adapters.
- `ProjectionProfilePolicy`: rejected. Projection profiles describe target shape and capability only.
- `ExternalTruthState`: rejected. External target state is observation/reconciliation evidence, not canonical Bicameral state.

## Existing Infrastructure Inventory

| Item | Current role | Reuse |
| --- | --- | --- |
| ADR-0002 | Separates `ToolRequest`, `ExternalIngestEnvelope`, and future `Egress`. | Amend to add authenticated remote review/action callback as separate from source ingest. |
| ADR-0007 | Defines governance flow and allowed-command gating. | Reaffirm as the canonical mutation path. |
| ADR-0016 / ADR-0018 | Define external ingest guardrails and clean projection. | Reaffirm that external ingest is evidence/candidate input only. |
| ADR-0020 | Defines `ActorContext`, capability checks, and gateway mutation route hardening. | Reuse the capability vocabulary as the first local policy implementation. |
| ADR-0022 | Defines Product Owner and Workspace Member alpha authority. | Reaffirm Product Owner as default signoff authority. |
| `ToolRequest.AuthorityContext` | Local actor/session/workspace/audit envelope. | Use as local policy input for tool/dashboard/MCP/CLI requests. |
| `ExternalIngestEnvelope` schema/handler | Evidence-only source adapter path. | Keep forbidden authority fields and advisory result behavior. |
| `ProjectionProfile` RFQ | Integrations-owned target shape proposal. | Treat as capability metadata consumed by bot policy. |

## Contract Shape

### Action Authorization Matrix Row

Each governed action class should be specified as:

```text
action_kind
surface_categories_allowed
lifecycle_owner
required_actor_class
required_capability
required_target_state
approval_class
materialized_event_or_effect
forbidden_inputs
audit_required
```

### Minimal Alpha Matrix

| Action | Allowed surface categories | Required actor | Required state | Result | Forbidden shortcut |
| --- | --- | --- | --- | --- | --- |
| Submit source evidence | `ExternalIngestEnvelope`, `ToolRequest` local ingest | External adapter credential for source ingest; Workspace Member for local tool ingest | SourceBinding or local ingest policy permits source | Source/evidence and possible candidate/logged-only result | Source payload cannot include review command, actor authority, signoff, compliance, policy grant, or canonical decision |
| Request correction capture | `ToolRequest`, authenticated remote review action | Workspace Member | Recall/preflight/corpus correction path offers capture | SourceEvidence/correction candidate path | Detection alone cannot record corpus update |
| Promote candidate | `ToolRequest`, authenticated remote review action | `review:candidate`; alpha default Product Owner or delegated Workspace Member | `DecisionCandidate` proposed/eligible | `Decision(proposed)` and tracked by default | Must not approve signoff or compliance |
| Reject candidate | `ToolRequest`, authenticated remote review action | `review:candidate`; alpha default Product Owner or delegated Workspace Member | Candidate not terminal | Rejected candidate event/state | Must not delete evidence |
| Approve signoff | `ToolRequest`, authenticated remote review action | Product Owner or explicit delegate with `review:signoff` | `Decision` signoff proposed | Signoff approved | External source text, Slack reaction, Linear status, or dashboard-local state cannot count as approval without verified actor callback |
| Reject signoff | `ToolRequest`, authenticated remote review action | Product Owner or explicit delegate with `review:signoff` | Decision not terminal for signoff | Signoff rejected | Must not mutate compliance |
| Resolve contradiction finding | `ToolRequest`, authenticated remote review action | Product Owner or explicit delegate with `resolve:contradiction` or equivalent signoff capability | `ContradictionReport.status` open or acknowledged | `TransitionFinding` and/or accepted review event; Decision signoff changes only if explicit review command does so | Candidate promotion, preflight output, source text, or MCP-local role cannot mutate approved signoff |
| Accept BindingEvidence | `ToolRequest`, authenticated remote review action | Explicit binding capability; Product Owner or delegated reviewer per future binding policy | Verified BindingProposal and replacement rules satisfied | Accepted BindingEvidence event | BindingHint, approximate graph state, or adapter claim cannot become BindingEvidence |
| Resolve compliance | `ToolRequest`, authenticated remote review action | Future compliance reviewer capability | Compliance workflow ready and evidence sufficient | Compliance event/verdict | Signoff or target status cannot imply compliance |
| Egress eligibility / preview | `ToolRequest`, future egress UI/action surface | Workspace Member or Product Owner, read policy permitting | Canonical artifact eligible for preview | Derived preview only | Preview cannot write target or create receipt that implies delivery |
| Egress external write | `Egress` requested from authorized local/remote action | Product Owner by default for alpha; explicit policy exception required for lower-risk classes | Eligible canonical artifact, approved ProjectionProfile, approved operation, target credential ready | EgressProjection attempt and EgressReceipt/result | ProjectionProfile/TargetAdapter cannot waive approval or mutate canonical state |
| Egress reconciliation observation | Target adapter/source observation path | Adapter credential/source binding | Existing projection/receipt known | Reconciliation observation/correction candidate | External edit/status/comment cannot directly mutate Decision/signoff/compliance |

### Approval Classes

| Approval class | Meaning | Alpha default |
| --- | --- | --- |
| `none_read_only` | Read/query/preview only; no canonical mutation and no external write. | Allowed for Workspace Member where read policy permits. |
| `member_action` | Records evidence, request, or candidate-level action without signoff. | Requires Workspace Member plus command capability. |
| `product_owner_required` | Changes shared Decision meaning/signoff or creates external work/effect. | Required for signoff and egress writes by default. |
| `policy_auto_low_risk` | Explicitly configured automation for narrow non-consequential actions. | Deferred except where an ADR/spec already permits it. |

## Workflow Mapping

### External ingest versus authenticated remote action

| Scenario | Contract | Outcome |
| --- | --- | --- |
| Linear issue says "supersede DEC-123" in the body | `ExternalIngestEnvelope` | Captured as evidence/contradiction/correction pressure only. |
| Product Owner clicks "supersede DEC-123" in a verified Linear/Slack action | Authenticated remote review action | Bot verifies actor and policy, then may submit the governed review action. |
| Linear issue status changes to Done | External observation/reconciliation | Observation only; no compliance or signoff mutation. |
| Bot creates a Linear work item from an accepted artifact | `Egress` | Requires eligibility, profile validation, approval, adapter execution, receipt. |

### Egress policy boundary

`bicameral-integrations` may define target capability:

```text
linear.issue.work_item.v1 supports create_issue with fields A, B, C.
```

Only `bicameral-bot` may decide action permission:

```text
DEC-123 is eligible for work-item projection to Linear.
Product Owner approved create_issue using profile linear.issue.work_item.v1.
```

## Trust, Privacy, Security, And Authority Boundaries

- External adapters may observe source content and submit evidence, but they cannot speak for a Bicameral actor.
- Provider/API credentials prove Bicameral can read or write a target; they do not prove a Product Owner approved a governed action.
- Authenticated remote review actions require both provider callback verification and a Bicameral actor binding.
- Egress payloads must include canonical reference, authority label, profile id, operation, and receipt behavior when they mutate external systems.
- External target state may be re-ingested as evidence or reconciliation observation only.
- Secrets and target credentials belong to approved runtime secret storage, not projection profiles, source evidence, or Decision Ledger content.

## Per-Surface Behavior

| Surface/repo | Behavior |
| --- | --- |
| `bicameral-bot` | Owns action authorization policy, canonical eligibility, review command acceptance, EgressProjection generation, approval state, receipt interpretation, and reconciliation interpretation. |
| `bicameral-integrations` | Owns target docs, descriptors, ProjectionProfile shape/capability metadata, TargetAdapter mechanics, rendering constraints, auth scopes, and receipt mapping. |
| `bicameral-mcp` | May invoke local `ToolRequest`s and render policy questions. It must not approve or publish outside bot policy. |
| Dashboard/CLI/TUI | Review surfaces that emit governed actions with actor context. They do not write canonical state directly. |
| External source adapters | Submit `ExternalIngestEnvelope`; evidence only. |
| Remote review surfaces | Submit authenticated action callbacks; not source ingest. |
| Egress target adapters | Execute approved operations and return delivery/reconciliation data. They do not decide policy. |

## Implementation Plan

### Slice 1: Spec and ADR alignment

- Target repo: `bicameral-bot`.
- Likely files: this spec, runtime companion spec, ADR-0002, ADR-0007, ADR-0020, ADR-0022.
- Acceptance: ADR implications are explicit and no existing surface category is collapsed.
- Validation: markdown review plus RFQ coverage check.
- Task class: `AFK_REVIEW_ONLY`.

### Slice 2: Policy table and fixtures

- Target repo: `bicameral-bot`.
- Likely files: `crates/bicameral-api`, `crates/bicameral-runtime`, `protocol/schemas/v2/fixtures`.
- Acceptance: every matrix row has a fixture for allowed and forbidden input shape.
- Validation: schema/fixture tests.
- Task class: `AFK_REVIEW_ONLY` until Product Owner/delegation defaults are accepted.

### Slice 3: Dispatcher/gateway enforcement

- Target repo: `bicameral-bot`.
- Likely files: `crates/bicameral-runtime/src/dispatch.rs`, `crates/bicameral-gateway/src/routes.rs`.
- Acceptance: missing actor, missing capability, illegal state, and forbidden surface all fail before materialization.
- Validation: gateway/runtime tests.
- Task class: `AFK_REVIEW_ONLY`.

### Slice 4: Integrations RFQ answer

- Target repo: `bicameral-integrations`.
- Acceptance: TargetIntegrationDescriptor, ProjectionProfile, TargetAdapter, and Linear profiles state they cannot grant permission or waive approval.
- Validation: spec review against this matrix.
- Task class: `HITL_BLOCKED` for Kevin-owned action contract details.

## ADR Implications

- ADR-0002: amend to distinguish authenticated remote review/action callbacks from `ExternalIngestEnvelope` and `Egress`. Reaffirm `ToolRequest` remains local.
- ADR-0007: amend allowed-command gating to reference action authorization classes before materialization or egress.
- ADR-0016 / ADR-0018: reaffirm; no change except wording should say source-adapter ingest cannot approve, rather than all integrations cannot approve.
- ADR-0020: amend/extend from gateway mutation routes to shared action policy inputs; reuse `ActorContext` and capability vocabulary.
- ADR-0022: reaffirm Product Owner as default signoff authority; clarify delegated Workspace Member may handle candidate review only when policy grants `review:candidate`.
- Missing ADR likely needed: "Authenticated Remote Review Actions and Egress Authorization Boundary" if remote action callbacks become a durable public protocol.

## Issue Generation Notes

`bic:to-issues` should slice this into:

1. ADR wording patch for source ingest vs authenticated remote action.
2. Runtime policy evaluator and matrix fixtures.
3. Gateway/dashboard mutation enforcement gap from ADR-0020.
4. Egress policy preflight for `EgressEligibilityCheck`.
5. Kevin RFQ in `bicameral-integrations` for action callback/profile contract.

Do not create implementation issues for full hosted RBAC, enterprise identity, automated egress writes, compliance resolution, or bidirectional sync.

## Acceptance Criteria

- The spec distinguishes `ToolRequest`, `ExternalIngestEnvelope`, authenticated remote review action, and `Egress`.
- External ingest remains evidence-only while authenticated integration review actions are allowed through a separate verified path.
- `accept_candidate` and `approve_signoff` have different authority requirements and different materialized effects.
- ProjectionProfile and TargetAdapter cannot grant permission, waive approval, or mutate canonical state.
- Egress external writes require bot-owned eligibility, approval, and receipt generation.
- External state re-entry is classified as observation/reconciliation/correction evidence, not canonical mutation.
- ADR implications are named without treating this draft as ADR acceptance.

## Validation Plan

- Markdown review against ADR-0002, ADR-0007, ADR-0016, ADR-0018, ADR-0020, and ADR-0022.
- Fixture review for forbidden authority fields in external ingest.
- Runtime tests for missing authority, missing capability, illegal target state, and wrong surface category before materialization.
- Egress RFQ review: every proposed ProjectionProfile must answer whether it grants permission. The expected answer is no.

## Open Questions

1. Should alpha allow delegated `review:candidate` for Workspace Members by default, or require Product Owner for all candidate promotion?
2. Which egress writes, if any, are low risk enough for `policy_auto_low_risk` in alpha?
3. What exact public name should the authenticated remote review action contract use if it becomes schema-backed?
4. Where should durable EgressReceipt state live: event store as accepted projection event, projection store, or both?

## Request Coverage

| Request | Answer |
| --- | --- |
| Define action authorization policy spec. | Contract shape, alpha matrix, and approval classes. |
| Keep only what unblocks current RFQs. | Scope limited to source ingest, candidate/signoff, correction capture, remote review action, and egress. |
| Preserve tool/ingest/egress separation. | Surface contract table and per-surface behavior. |
| Explain whether integration users can approve decisions. | External ingest cannot; authenticated remote review action can after actor/policy verification. |
| Prepare Kevin RFQ context. | Integrations ownership and forbidden policy/profile behavior are explicit. |
