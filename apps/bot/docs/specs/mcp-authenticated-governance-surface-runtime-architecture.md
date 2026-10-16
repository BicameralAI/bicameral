# Runtime Architecture: MCP Authenticated Governance Surface

**Date:** 2026-06-26
**Status:** draft
**Level:** L2 cross-repo contract / L3 governance policy
**Owns:** Runtime command, event, read-model, and authority flow for MCP-submitted governance commands and automatic owner governance inbox surfacing.
**Related context:** BicameralAI/bicameral-bot#587; `docs/specs/mcp-authenticated-governance-surface.md`; `docs/specs/action-authorization-policy.md`; ADR-0002; ADR-0007; ADR-0011; ADR-0022
**Governance gate:** `bic:spec-governance-gate` advisory/no-op; Qor integration disabled pending factory policy revision.

## Reading Guide

Read `docs/specs/mcp-authenticated-governance-surface.md` first. Runtime invariant:

```text
The bot is the only runtime that may validate authority and materialize
governance events; MCP is only a local submitting and rendering surface.
```

## Runtime Problem

The current runtime model has recall/preflight, contradiction findings, and
dashboard review paths, but the live-system behavior for MCP governance commands
is incomplete. Implementation workers could otherwise guess:

- whether MCP owns contradiction resolution;
- whether contradiction findings are signoff states or event-store findings;
- whether unauthorized users get a read-only fallback or a mutation attempt;
- whether owner review is a requested action or an automatic projection.

This spec defines the runtime flow using the reduced-state model from ADR-0011.

## Goals And Non-Goals

Goals:

- Define runtime components and state boundaries for MCP governance commands.
- Define command/event flow for `governance.inbox.list` and
  `governance.resolve_contradiction`.
- Define owner inbox as derived projection over active contradiction findings.
- Define failure behavior for missing/unauthorized actor context.
- Keep preflight idempotent and side-effect free.

Non-goals:

- Do not implement hosted/team inbox aggregation.
- Do not change source ingest, egress, or authenticated remote review callbacks.
- Do not create MCP-owned persistence.
- Do not add canonical `collision_pending` or `approval_suspended` Decision
  states.

## Runtime Components

| Component | Responsibility | Inputs | Outputs | Persistent state | Must not |
| --- | --- | --- | --- | --- | --- |
| MCP surface | Render inbox and submit governance `ToolRequest`s. | User session, bot capability report, ToolResponse. | ToolRequest; rendered results. | Optional helper cache only. | Decide actor authority or mutate canonical state. |
| Bot gateway/API | Receive ToolRequest, validate shape, forward to runtime dispatcher. | ToolRequest. | ToolResponse. | Request trace/log. | Treat source evidence as actor authority. |
| Runtime dispatcher | Route governance commands and invoke action authorization. | Validated ToolRequest. | Accepted command result or typed denial. | Runtime trace. | Materialize event before authorization. |
| Action Authorization Policy | Decide whether actor/surface/finding status/capability permits command. | AuthorityContext, target finding, command class. | Allow/deny decision. | Policy config/fixtures. | Infer Product Owner from MCP-local data. |
| Event store adapter | Persist accepted review/governance events. | Accepted event. | Durable event record. | Canonical event store. | Accept MCP direct writes. |
| Replay/read model | Materialize Decision state, contradiction reports, conflict edges, and inbox projection. | Event stream. | Decision Ledger, `ContradictionReport`s, `conflicts_with`, derived inbox rows. | Derived projection/cache. | Treat inbox rows as canonical events. |

## Canonical vs Derived State

| Artifact | Classification | Writer | Readers | Mutation path | Prohibited shortcut |
| --- | --- | --- | --- | --- | --- |
| Decision signoff state | Canonical replayed state from accepted events | Event store adapter after bot policy acceptance | Bot, dashboard, MCP, CLI | Governed review command | `RaiseContradiction` changes signoff |
| `ContradictionReport` | Durable finding / accepted governance input | `RaiseContradiction` event | Bot, dashboard, MCP, preflight | `RaiseContradiction` / `TransitionFinding` | Approximate similarity creates durable finding |
| `conflicts_with` edges | Derived read-model edges | Replay from active contradiction reports | Search, preflight, dashboard, MCP | Rebuild from event log | Manual dashboard edge edit |
| Governance inbox item | Derived projection/read model | Bot projection from replayed state and actor policy | MCP, dashboard, CLI | Rebuild from canonical/finding state | Persist as independent truth |
| Authorization denial | ToolResponse result / trace | Bot action authorization policy | MCP/caller | Per-command response | Lifecycle state mutation |
| Runtime trace | Audit/observability | Bot runtime | Operators/devs | Runtime logging | Approval evidence |

## State Refinement Check

Release state: a Product Owner can open an MCP session, see active
contradiction findings automatically, inspect evidence, and resolve one through
a bot-authorized ToolRequest.

States refined:

- `ContradictionReport`: durable finding; `id` is collision set id.
- `Governance inbox item`: derived row, not canonical state.

Transitions:

1. Verified or policy-accepted contradiction evidence is accepted.
2. Bot materializes `RaiseContradiction`.
3. Replay stores the `ContradictionReport` and derives `conflicts_with` edges.
4. Decision signoff state remains unchanged.
5. Inbox projection includes active finding row for Product Owner/delegate.
6. MCP lists inbox and renders allowed commands.
7. Authorized actor resolves the finding; bot materializes `TransitionFinding`
   or a related accepted review event.

Persistence:

- contradiction finding: selected event store;
- inbox: derived read model/cache;
- denial: ToolResponse/trace only.

Invariants:

- Active contradiction findings do not make approved Decisions cleanly safe.
- Active contradiction findings do not change Decision signoff state by default.
- Owner inbox surfacing is automatic.
- Unauthorized resolve attempts do not create review requests or owner tasks.

## State Minimization Review

- `collision_pending`: rejected as canonical Decision state; can remain a
  display/legacy label for active collision context.
- `approval_suspended`: rejected; no signoff mutation is needed for current
  behavior.
- `owner_inbox_task`: collapsed into derived inbox item.
- `mcp_session_governance_state`: rejected; session state may cache but cannot
  persist lifecycle authority.

## Authority Ownership Matrix

| Actor/component | observe | propose | validate | accept | materialize | render | mutate |
| --- | --- | --- | --- | --- | --- | --- | --- |
| MCP | Yes, via bot read policy | ToolRequest only | No | No | No | Yes | No |
| Product Owner/delegate | Yes | Yes | Through bot policy | Review command intent | No direct write | Yes | Only through bot accepted command |
| Bot runtime | Yes | No user intent | Yes | Yes, policy acceptance | Via event store adapter | Produces response/projection | Canonical mutation only through accepted events |
| Event store adapter | No product judgment | No | Payload integrity only | No | Yes | No | Writes accepted events |

## Runtime Scenarios

### Session start inbox

Trigger: MCP session initializes with `AuthorityContext`.

```text
MCP -> bot: governance.inbox.list ToolRequest
bot -> policy: actor/workspace/product read check
bot -> replay/read model: query active ContradictionReports and other inbox rows
bot -> MCP: ToolResponse with rows and allowed commands
```

Failure:

- Missing actor context returns typed denial or anonymous read refusal.
- No rows returns empty inbox, not "no governance issues exist globally."

### Preflight with active contradiction context

```text
MCP -> bot: preflight.run ToolRequest
bot -> replay/read model: relevant approved Decisions
bot -> replay/read model: active ContradictionReports involving those Decisions
bot -> MCP: preflight result with approved signoff + active collision context
```

No event is emitted. Repeating the same preflight request against the same
replayed state returns the same collision context.

### Authorized contradiction resolution

```text
MCP -> bot: governance.resolve_contradiction(report_id, resolution_kind, rationale)
bot -> policy: validate Product Owner/delegate capability and finding status
bot -> event store: append TransitionFinding or accepted review event
event store -> replay: update finding/read model
bot -> MCP: accepted result + updated state ref
```

Failure:

- stale target finding returns refresh-required response;
- missing evidence/rationale returns validation failure where policy requires it;
- invalid resolution kind returns unsupported command.

### Unauthorized contradiction resolution

```text
MCP -> bot: governance.resolve_contradiction(...)
bot -> policy: actor lacks resolve capability
bot -> MCP: authorization_denied(required_capability, required_actor_class)
```

No canonical mutation occurs. MCP does not offer `request_owner_review`; the
owner inbox projection already covers owner attention.

### Forbidden flow

```text
MCP local session role
  -> direct Decision state edit
  -> event store write
```

This is forbidden. MCP-local role/capability data is advisory UI context only;
bot policy must validate every governed command.

## Event, Command, And Protocol Contracts

### `governance.inbox.list`

- Producer: MCP, dashboard, CLI/TUI later.
- Consumer: bot runtime dispatcher.
- Validation: read policy for actor/workspace/product.
- Idempotency: read-only.
- Privacy: only rows visible to actor/product scope.

### `governance.resolve_contradiction`

- Producer: MCP or another review surface.
- Consumer: bot runtime dispatcher.
- Validation: `AuthorityContext`, `ContradictionReport.status`, required
  capability, resolution kind, idempotency key.
- Idempotency: repeat with same idempotency key returns prior accepted/denied
  outcome where possible.
- Privacy/security exclusions: source text cannot carry actor authority;
  external statuses cannot prove resolution.

## Substrate Capability Matrix

| Capability | Local/free bot | Hosted/team service |
| --- | --- | --- |
| Persist canonical review/finding event | Yes, via selected local event store | Yes, future hosted event store |
| Render owner inbox | Yes, for local product/workspace | Yes, broader team aggregation later |
| Verify Product Owner/delegate | Yes, local Product boundary | Yes, hosted RBAC later |
| Cross-repo collision discovery | Limited/local | Hosted/team advisory possible |
| MCP command submission | Yes | Yes if connected to bot-compatible gateway |

## Error, Retry, And Recovery Behavior

- Missing `AuthorityContext`: typed auth failure.
- Unauthorized actor: typed denial with required actor/capability; no
  owner-review request.
- Stale read model: return refresh-required or retryable stale-state response.
- Duplicate idempotency key: return prior result where available.
- Event store write failure: command remains unmaterialized; response must not
  claim resolution.
- Replay/projection lag: return accepted event ref plus projection pending/stale
  metadata.

## Observability, Audit, And Receipts

Required:

- ToolRequest trace id.
- actor id, workspace/product scope, surface category.
- command kind and target report id.
- allow/deny decision and policy reason.
- accepted event id when materialized.

Traces are observability, not approval authority.

## Security, Privacy, And Authority Boundaries

- Actor authority must come from `AuthorityContext` verified by bot policy.
- MCP must not persist secrets or external identity bindings.
- Contradiction evidence may be displayed according to read policy.
- Unauthorized users may not get hidden Product Owner-only rationale if read
  policy denies it.

## Implementation Sequencing

1. Bot command schema and fixtures.
2. Runtime dispatcher authorization and denial behavior.
3. Derived owner inbox projection over `ContradictionReport`.
4. Preflight collision-context projection over active findings.
5. MCP client rendering and command submission.
6. Optional dashboard/CLI alignment.

## Issue Generation Notes

The first implementation issue should target `bicameral-bot`
command/runtime/projection fixtures. A dependent `bicameral-mcp` issue should
wire the surface once bot fixtures are accepted.

## ADR Implications

- ADR-0002: reaffirmed/amendment candidate for command names.
- ADR-0007: reaffirmed.
- ADR-0011: amended to make collision an event-store finding axis.
- ADR-0022: reaffirmed.

## Acceptance Criteria

- Runtime accepts authorized `governance.resolve_contradiction` and
  materializes only after policy allow.
- Runtime denies unauthorized attempts without creating owner-review tasks.
- Owner inbox projection includes active contradiction findings automatically.
- Preflight includes active contradiction context without mutating signoff.
- MCP never writes event store directly.

## Validation Plan

- Runtime dispatch tests.
- Event replay fixture for contradiction resolution.
- Projection tests for Product Owner and Workspace Member visibility.
- Preflight idempotence fixture keyed by `ContradictionReport.id`.
- Negative fixture for MCP-local role/capability spoofing.

## Open Questions

- Should inbox rows be paginated in the first release? This affects UX only.

## Request Coverage

| Request | Covered by |
| --- | --- |
| MCP governance decisions auth-gated | Runtime Components; Command Contracts |
| Collision queue owner inbox | Runtime Scenarios; Projection state |
| No `request_owner_review` | Error behavior; unauthorized flow |
