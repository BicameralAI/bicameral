# MCP Authenticated Governance Surface

**Date:** 2026-06-26
**Status:** draft
**Level:** L2 cross-repo contract / L3 governance policy
**Owns:** The feature contract for allowing MCP to submit authenticated governance commands through bot-owned `ToolRequest` authorization while keeping owner review queues automatic.
**Related context:** BicameralAI/bicameral-bot#587; ADR-0002; ADR-0007; ADR-0011; ADR-0022; `docs/specs/action-authorization-policy.md`; `docs/specs/constraint-lookup-recallpacket-contract.md`; `docs/specs/preflight-search-readiness.md`; `docs/specs/mcp-authenticated-governance-surface-runtime-architecture.md`; `docs/specs/mcp-governance-inbox-strategy.md`
**Governance gate:** `bic:spec-governance-gate` advisory/no-op; Qor integration disabled pending factory policy revision.
**Skill provenance:** Drafted with local copied `bic:to-spec` and `bic:to-runtime-spec` skill procedures. Their installed hashes differ from the factory manifest in this checkout; `bic:spec-governance-gate` matches the manifest. `bic:to-strategy-spec` was loaded directly from the referenced factory checkout because it is not copied under `.agents/skills/`.

## Reading Guide

Read `docs/specs/action-authorization-policy.md` first. The invariant is:

```text
MCP may render governance state and submit governance commands, but only
bot-owned action authorization may accept a command and materialize canonical
events.
```

## Problem

ADR-0011 now models collision as a durable contradiction finding, not as a
Decision signoff state. That leaves a concrete product flow to define:

- MCP sessions should surface active contradiction findings that matter to the
  actor.
- Product Owners should be able to resolve those findings in context.
- Workspace Members should not be able to use candidate promotion or MCP-local
  authority to nullify approved governance.

If MCP remains read-only recall/preflight, a Product Owner must switch surfaces
to resolve a governance interruption that is already in context. If MCP resolves
collisions locally, it bypasses Product Owner accountability and event-store
governance.

## Goals And Non-Goals

Goals:

- Allow MCP to surface governance inbox items at session start, including active
  contradiction findings involving approved Decisions.
- Allow authorized Product Owners or explicit delegates to resolve contradiction
  findings through bot-authorized `ToolRequest`s.
- Return typed authorization denial for unauthorized actors.
- Reuse `ToolRequest.AuthorityContext` and `ActionAuthorizationPolicy`; do not
  invent an MCP-only authority model.
- Keep Decision signoff state unchanged merely because a collision set exists.

Non-goals:

- Do not make MCP canonical authority.
- Do not let MCP compute Product Owner role, delegation, policy, or capability
  locally.
- Do not add a `request_owner_review` action. Owner inbox surfacing is
  automatic.
- Do not introduce enterprise RBAC, hosted identity, or multi-owner products.
- Do not let preflight or RecallPacket resolve conflicts without an explicit
  governed command.

## Decision Or Proposed Direction

Add an MCP authenticated governance surface over the existing `ToolRequest`
path.

MCP may submit local governance commands when it can provide an
`AuthorityContext`. The bot evaluates:

- surface category: `ToolRequest`;
- actor identity and workspace/product scope;
- required capability;
- target finding status or target lifecycle state;
- command-specific policy;
- idempotency and audit fields.

If allowed, the bot materializes the accepted review/finding event through the
selected event store substrate. If denied, the bot returns typed authorization
failure. MCP renders the response and must not retry by changing authority
fields.

For contradiction findings, MCP may expose:

- `governance.inbox.list`;
- `governance.inspect`;
- `governance.resolve_contradiction`.

No user-facing `request_owner_review` action is needed because Product Owner
inbox surfacing is automatic.

## State Refinement Check

Release state: authenticated MCP governance commands can inspect and resolve
active contradiction findings when the actor is authorized.

States added or refined:

| State / term | Refinement |
| --- | --- |
| `ContradictionReport` | Durable finding whose `id` is the collision set id and whose `related_constraint_ids` are the collision set members. |
| `Governance inbox item` | Derived projection/read-model row for governance attention. It is not canonical state. |
| `MCP governance command` | A `ToolRequest` command class carrying `AuthorityContext`; it is not a separate authority surface. |
| `authorization_denied` | Typed command result, not lifecycle state. It explains required actor/capability and allowed read-only fallbacks. |

Transitions:

| Transition | Authorized actor/component | Persistence |
| --- | --- | --- |
| Contradiction finding raised | Bot policy / detector after verified or policy-accepted evidence | `RaiseContradiction` event; replayed `ContradictionReport`; derived `conflicts_with` edges |
| Governance inbox row appears | Bot read model/projection | Derived projection only |
| Product Owner resolves contradiction | Product Owner or explicit delegate with `resolve:contradiction` capability through bot policy | `TransitionFinding` or accepted review event; replay updates finding/read model |
| Unauthorized actor attempts resolution | Bot action authorization policy | No canonical mutation; typed ToolResponse denial and trace only |

Invariants:

- Active contradiction findings must not be hidden from recall/preflight.
- Active contradiction findings must not change Decision signoff state by
  default.
- MCP must not create or resolve governance state without bot-accepted command
  materialization.
- Unauthorized actors may inspect evidence when read policy allows, but may not
  resolve contradiction findings.
- Owner inbox surfacing is automatic and must not depend on another user
  requesting owner review.

Deferred vocabulary:

- Hosted/team-wide owner inbox aggregation.
- Multi-owner product routing.
- Escalation SLA fields.
- Low-risk automatic contradiction resolution.
- MCP-local role cache.

## State Minimization Review

Rejected/collapsed states:

- `collision_pending`: rejected as canonical Decision state. It can remain a
  display/legacy label for active contradiction context.
- `approval_suspended`: rejected. Approved Decisions remain approved unless a
  future ADR introduces an explicit suspension command/state.
- `owner_review_requested`: rejected. The owner inbox is derived automatically
  from active findings, policy, and current actor scope.
- `mcp_authorized`: rejected as state. Authorization is evaluated per command.
- `collision_resolved_by_mcp`: rejected. MCP only submits; bot-accepted events
  resolve findings.

## Existing Infrastructure Inventory

| Item | Current role | Reuse |
| --- | --- | --- |
| ADR-0002 | Defines MCP as a local tool surface mapping to `ToolRequest`. | Reaffirm; add issue slices for governance command coverage. |
| ADR-0007 | Defines substrate-neutral governance flow and allowed-command gating. | Reaffirm as only mutation path. |
| ADR-0011 | Defines contradiction findings and collision sets. | Use `ContradictionReport.id` as collision set id. |
| ADR-0022 | Defines Product Owner authority boundary. | Use Product Owner/delegate as default contradiction resolver. |
| `docs/specs/action-authorization-policy.md` | Defines action classes and capability checks. | Add contradiction-resolution row and MCP governance command fixtures. |
| `ToolRequest.AuthorityContext` | Local actor/session/workspace/audit envelope. | Required input for MCP governance commands. |
| `RecallPacket` / preflight specs | Read-only recall and unknown-scope behavior. | Continue read-only; route governance actions through separate ToolRequest commands. |

## Contract Shape

### Command Classes

```text
governance.inbox.list
governance.inspect
governance.resolve_contradiction
```

### `governance.inbox.list`

Purpose: read derived governance attention items for the authenticated actor and
workspace.

Inputs:

- `AuthorityContext`;
- optional `workspace_id`;
- optional `product_id`;
- optional filters: `active_contradiction`, `needs_signoff_review`,
  `stale_source_pending`.

Output:

- derived inbox rows;
- contradiction report ids;
- affected Decision/candidate/constraint refs;
- target ids;
- allowed commands for the current actor;
- typed denial markers for commands the actor cannot perform.

### `governance.resolve_contradiction`

Purpose: submit an explicit resolution command for an active contradiction
finding.

Payload shape:

```text
contradiction_report_id
resolution_kind:
  supersede_decision
  mark_different_scopes
  reject_signoff
  dismiss_finding
  resolve_finding
  request_more_context
resolution_rationale
evidence_refs[]
idempotency_key
```

Validation rules:

- caller must carry `AuthorityContext`;
- target `ContradictionReport.status` must be `open` or `acknowledged` unless
  command is a read-only inspect;
- required capability is `resolve:contradiction` or equivalent Product Owner
  `review:signoff`;
- `request_more_context` records a request but does not mutate Decision signoff;
- command must be idempotent.

Forbidden fields:

- actor authority injected from source evidence;
- external target status as proof of resolution;
- MCP-local confidence score as policy proof;
- `request_owner_review`.

## Workflow Mapping

| Flow | Behavior |
| --- | --- |
| Session start for Product Owner | MCP may ask bot for governance inbox. Bot returns active contradiction findings automatically. |
| Session start for Workspace Member | MCP may show read-allowed inbox items, but resolution commands are denied or omitted according to policy. |
| Preflight surfaces affected approved Decision | Preflight labels it approved with active collision context. It does not resolve it and does not change signoff. |
| Product Owner resolves inline | MCP submits `governance.resolve_contradiction`; bot accepts or denies; event store materializes accepted finding transition/review event. |
| Unauthorized user tries to resolve | Bot returns typed denial. MCP may still render evidence if read policy permits. |

## Trust, Privacy, Security, And Authority Boundaries

- MCP may observe read-model state allowed by actor/workspace policy.
- MCP may propose governance commands by submitting `ToolRequest`s.
- MCP may not decide authority, infer Product Owner status, mutate canonical state,
  or suppress owner inbox items.
- Bot action authorization policy is authoritative for command acceptance.
- Event store adapter is authoritative for durable materialization.
- Logs and traces explain what happened, but do not approve decisions.

## Per-Surface Behavior

| Surface | Behavior |
| --- | --- |
| `bicameral-bot` | Owns command validation, actor/capability checks, event materialization, inbox projection semantics, typed denial. |
| `bicameral-mcp` | Renders inbox and submits ToolRequests. No local authority decisions. |
| Dashboard | Continues to support review actions; shares command semantics with MCP. |
| CLI/TUI | May later expose the same commands; not required for first slice. |
| External integrations | Do not use this path unless they are verified authenticated remote review actions. Source ingest remains evidence-only. |

## Implementation Plan

### Slice 1: Contract fixtures

- Target repo: `bicameral-bot`.
- Likely files: `crates/bicameral-api/src/tool_request.rs`,
  `protocol/schemas/v2/fixtures`.
- Acceptance: governance inbox and resolve contradiction command payloads
  serialize, deny invalid authority injection, and preserve forbidden fields.
- Validation: serde/schema fixture tests.
- Task class: `AFK_REVIEW_ONLY`.

### Slice 2: Runtime authorization

- Target repo: `bicameral-bot`.
- Likely files: `crates/bicameral-runtime/src/dispatch.rs`, action
  authorization modules.
- Acceptance: unauthorized contradiction resolution returns typed denial;
  authorized Product Owner path materializes accepted finding transition/review
  event.
- Validation: runtime dispatch tests.
- Task class: `AFK_REVIEW_ONLY`.

### Slice 3: Owner inbox projection

- Target repo: `bicameral-bot`.
- Likely files: dashboard/read-model/projection modules.
- Acceptance: active contradiction findings appear automatically for
  Product Owner/delegate inbox; no `request_owner_review` action exists.
- Validation: projection tests.
- Task class: `AFK_REVIEW_ONLY`.

### Slice 4: MCP client wiring

- Target repo: `bicameral-mcp`.
- Acceptance: MCP renders governance inbox and submits bot `ToolRequest`
  governance commands without local authorization logic.
- Validation: MCP contract tests against bot fixtures.
- Task class: `AFK_REVIEW_ONLY`.

## ADR Implications

- ADR-0002: reaffirmed; may need a small amendment naming MCP governance
  inbox/list and resolve commands as `ToolRequest`s.
- ADR-0007: reaffirmed; contradiction resolution follows governance flow.
- ADR-0011: amended to define contradiction findings/collision sets as durable
  event-store state.
- ADR-0022: reaffirmed; Product Owner/delegate owns contradiction resolution
  for Decision meaning.

## Issue Generation Notes

Create one implementation issue for `bicameral-bot` and link this spec plus the
runtime and strategy specs. A later `bicameral-mcp` issue should consume bot
fixtures after the bot contract lands.

Hard dependencies:

- Existing `ActionAuthorizationPolicy` contract.
- Existing `ToolRequest.AuthorityContext`.
- `ContradictionReport` / `RaiseContradiction` event model from ADR-0011.

Human-input blockers:

- None for first implementation slices after this owner decision, provided
  Product Owner is the default contradiction resolver.

## Acceptance Criteria

- Product Owner/delegate can resolve an active contradiction finding through MCP
  via bot-authorized `ToolRequest`.
- Unauthorized users get typed denial with required capability and target state;
  no canonical mutation occurs.
- Owner inbox automatically surfaces active contradiction findings; no
  `request_owner_review` action is present.
- Preflight/recall continue to surface affected approved Decisions as approved
  with active collision context.
- MCP does not compute authorization locally.

## Validation Plan

- Serde/schema fixture tests for new command payloads.
- Runtime dispatch tests for authorized and unauthorized actors.
- Projection tests for automatic owner inbox rows.
- Negative tests for source evidence attempting to carry review authority.
- MCP conformance tests once bot fixtures exist.

## Open Questions

- Should the first implementation expose `governance.inbox.list` only at session
  start, or also as an explicit MCP tool command? Leaving this unresolved
  affects MCP UX, not authority.

## Request Coverage

| Request | Covered by |
| --- | --- |
| Commit to `bot#587` collision behavior | State Refinement Check; Workflow Mapping; acceptance criteria |
| Link to MCP authenticated governance implementation issue | Issue Generation Notes |
| Leverage actor authority spec | Existing Infrastructure Inventory; Contract Shape |
| No `request_owner_review` action | Goals and Non-Goals; Contract Shape; Acceptance Criteria |
