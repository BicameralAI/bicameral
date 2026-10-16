# MCP Governance Inbox Strategy

**Date:** 2026-06-26
**Status:** draft
**Level:** L2 cross-repo contract / L3 governance policy
**Owns:** The decision procedure for surfacing, ranking, and explaining governance inbox items to MCP sessions, especially active contradiction findings involving approved Decisions.
**Related context:** BicameralAI/bicameral-bot#587; `docs/specs/mcp-authenticated-governance-surface.md`; `docs/specs/mcp-authenticated-governance-surface-runtime-architecture.md`; ADR-0011; ADR-0022; `docs/specs/preflight-search-readiness.md`
**Governance gate:** `bic:spec-governance-gate` advisory/no-op; Qor integration disabled pending factory policy revision.

## Reading Guide

Read the feature and runtime specs first. Strategy invariant:

```text
The inbox strategy may prioritize governance attention, but it must not create
authority, hide contested context, mutate signoff, or require a user to request
owner review.
```

## Strategy Problem

Once MCP can submit authenticated governance commands, the system still needs a
deterministic strategy for what to surface at session start and during preflight.
Without a strategy, implementations may either:

- hide active contradiction findings because affected Decisions remain approved;
- flood owners with ordinary proposed items;
- treat unauthorized actors as responsible for requesting owner review;
- rank collision work by weak relevance instead of governance urgency.

## Product Promise And Quality Bar

Promise: Product Owners see active contradiction findings early enough to
resolve them in context, while non-owners see only the read/inspect state they
are authorized to see.

Quality indicators:

- active contradiction finding recall rate for Product Owner inbox: target 100%
  within local searched product scope;
- false owner-action affordances for unauthorized users: zero;
- `request_owner_review` action occurrences: zero;
- explanation completeness: every active contradiction row names report id,
  finding status, affected Decisions/constraints, evidence refs, and allowed
  resolution commands;
- no-result honesty: empty inbox means no visible local inbox rows for that
  actor and scope, not no unresolved governance exists globally.

## Inputs, Corpus, Scope

| Input | Classification | Use |
| --- | --- | --- |
| Replayed Decision state | Canonical replayed state | Source of signoff/review state. |
| `ContradictionReport` | Durable finding / accepted governance input | Source of collision set identity, member refs, status, and evidence refs. |
| `conflicts_with` edges | Derived read-model edges | Fast lookup of related constraints/Decisions. |
| Actor/Product role and capabilities | Policy input | Filters allowed commands and visibility. |
| Product/workspace scope | Request scope | Limits inbox query. |
| Recall/preflight relevance context | Derived read model | May boost context-specific display, not authority. |
| Hosted/team advisory results | Optional hosted advisory signal | Future source of contradiction evidence only after governance acceptance. |
| MCP-local session hints | Local scratch/advisory | May influence presentation grouping, not policy. |

Excluded inputs:

- MCP-local role claims as authority.
- Source text requesting approval as approval proof.
- Low-signal approximate contradiction as durable collision trigger.

## Candidate Generation Strategy

Generate inbox candidates from replayed state and read models:

1. Resolve actor and product/workspace scope.
2. Query active `ContradictionReport`s with `status = open | acknowledged`.
3. Join related constraints to affected Decisions/candidates where available.
4. Include all visible active contradiction findings for Product Owner/delegate.
5. Include `needs_signoff_review`, `stale_source_pending`, and
   `needs_grounding_review` according to policy and filters.
6. Attach evidence refs and contradiction report ids.
7. Compute allowed commands from action authorization policy.
8. For non-authorized users, include read/inspect rows only when read policy
   allows; do not generate owner-request tasks.

Stop conditions:

- Missing actor context: typed refusal.
- Unknown product scope: ask for scope or return scoped refusal.
- Projection stale: return stale projection warning and refresh action when
  available.

## Ranking, Fusion, And Prioritization

Precedence:

1. Active contradiction findings involving approved Decisions.
2. Other blocked owner-signoff states requiring Product Owner action.
3. Stale source or grounding review that blocks current task.
4. Ordinary proposed Decisions.
5. Advisory candidates and correction requests.

Within a tier:

- exact current-task relevance beats general relevance;
- higher finding severity may be boosted;
- direct affected-surface overlap beats semantic similarity;
- stable tie-break by `ContradictionReport.id`, then Decision id.

Ranking fields are presentation only. They must not grant capability, resolve
conflicts, mutate signoff, or suppress mandatory contradiction rows.

Reason codes:

- `active_contradiction`
- `approved_decision_contested`
- `product_owner_action_required`
- `read_only_for_actor`
- `task_relevant`
- `projection_stale`
- `unknown_scope`

## Honesty Labels, Explanations, Stop Conditions

Every active contradiction row should render:

```text
Approved Decision with active collision context.
Resolution requires Product Owner or explicit delegate.
```

No-result labels:

- `no_visible_items_for_actor_scope`
- `scope_unknown`
- `projection_unavailable`
- `hosted_scope_not_checked`

Allowed actions:

- `inspect_evidence`
- `expand_scope` when search/read scope is partial
- `resolve_contradiction` only when actor is authorized

Not allowed:

- `request_owner_review`
- hiding a visible contradiction because actor cannot resolve it
- showing resolve buttons that will fail authorization
- "approval suspended" copy unless a future ADR introduces that transition

## Authority Boundary And Forbidden Moves

The strategy may observe, rank, explain, and route. It must not accept,
materialize, sign off, bind, enforce, mutate, or promote.

Forbidden paths:

```text
high inbox rank -> implied authority -> signoff mutation
candidate promotion -> contradiction context -> approved Decision demotion
preflight warning -> event emission
```

Ranking only affects attention ordering. Resolution still requires an explicit
bot-authorized governance command.

## Evaluation Plan

Fixture corpus:

- approved Decision involved in active contradiction report;
- two approved Decisions in same contradiction report;
- approved Decision conflicting with unapproved candidate;
- unauthorized Workspace Member viewing inbox;
- Product Owner with empty scope;
- stale projection;
- partial hosted/team scope;
- resolved/dismissed contradiction reports that must not produce active
  warnings.

Metrics:

- active contradiction row recall for owner inbox;
- unauthorized action false-positive count;
- explanation field coverage;
- stable ordering under shuffled inputs;
- no-result honesty labels;
- preflight idempotence for repeated query over same replay state.

Regression threshold:

- zero unauthorized mutation paths;
- zero `request_owner_review` actions;
- zero hidden visible active contradiction rows for Product Owner local scope;
- zero signoff mutations from inbox/preflight projection.

## Negative Fixtures

- Workspace Member cannot resolve contradiction even if MCP session says they are
  owner.
- Source evidence cannot carry approval authority.
- Approximate contradiction candidate cannot create durable `RaiseContradiction`.
- Empty local inbox cannot claim global governance is clean.
- Preflight cannot resolve contradiction from RecallPacket output.
- Ranking boost cannot mutate signoff.
- Resolved/dismissed contradiction reports do not produce active warnings.

## Issue Generation Notes

Slice strategy into:

1. Inbox candidate generation fixtures.
2. Ranking/explanation implementation.
3. Authorization-aware action affordances.
4. Preflight active contradiction projection.
5. MCP rendering.

Hard dependency: runtime command contract.

## ADR Spec Implications

- ADR-0011: amended to clarify contradiction findings and preflight behavior.
- ADR-0022: reaffirm Product Owner/delegate action boundary.
- `preflight-search-readiness`: reaffirm preflight cannot resolve contradictions;
  add active contradiction context result shape if needed.

## Acceptance Criteria

- Product Owner inbox includes all visible local active contradiction findings.
- Unauthorized actors do not see mutation affordances.
- `request_owner_review` is absent.
- Explanations distinguish approved signoff from active contested context.
- Empty results use scoped no-result labels.
- Preflight output is deterministic for a fixed replay state and query.

## Validation Plan

- Unit tests for candidate generation.
- Fixture replay for ranking and labels.
- Contract tests for allowed action affordances.
- Preflight idempotence fixtures.
- MCP snapshot/contract tests once client wiring exists.

## Open Questions

- Should ordinary proposed Decisions appear in the same default session-start
  inbox, or only when explicitly requested? If unresolved, default to active
  contradiction findings and other blocked owner-action states first.

## Request Coverage

| Request | Covered by |
| --- | --- |
| Surface collision queue to Product Owner at session start | Candidate Generation Strategy |
| Resolve inline when authorized | Honesty Labels and allowed actions |
| No `request_owner_review` | Honesty Labels; Negative Fixtures |
