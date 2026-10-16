# Graph Remediation And Revalidation Repair Paths

**Date:** 2026-06-06
**Status:** proposed
**Issue:** https://github.com/BicameralAI/bicameral-bot/issues/141
**Related:** ADR-0010, ADR-0011, ADR-0017, ADR-0019, ADR-0021;
`graphslice-planning-sufficiency.md`; issues #84, #134, #136, #138, #140,
#164

## Problem

Graph-backed grounding is useful only when the daemon can distinguish a real
evidence failure from a repairable local graph gap. ADR-0010 defines graph
evidence states, ADR-0021 defines GraphStore lifecycle, and
[GraphSlice Planning And Sufficiency Semantics](graphslice-planning-sufficiency.md)
defines the proof boundary for scoped graph claims. This RFQ defines the next
operational layer: what the daemon may do when graph evidence is stale,
missing, partial, dirty, or unavailable.

The key boundary is:

- graph remediation repairs recomputable graph facts;
- binding revalidation inspects accepted binding evidence against a later
  snapshot;
- neither remediation nor revalidation creates durable governance facts unless
  a later governed command or reviewer action accepts the result.

Without this boundary, the daemon can overclaim by turning a stale local cache
into `verified`, converting a partial slice into `not_found`, silently treating
approximate candidates as repaired bindings, or escalating general graph
readiness into `needs_grounding_review` for every Decision.

## Decision

Graph remediation is a daemon-owned operational repair loop over
`GraphSnapshot`, `GraphSlice`, and `BindingAnchor` data. It may refresh,
reindex, invalidate, expand, rebuild, retry validation, or request external
visibility. It does not mutate canonical Decision Ledger state and does not
materialize `BindingEvidence`, `RevalidateBinding`, compliance, signoff,
conflict, or review-state events by itself.

The remediation loop is:

```text
GraphEvidenceState / GraphReadiness / invalidation trigger
  -> RemediationPlan
  -> remediation operation
  -> ScopeSufficiencyProof
  -> GraphEvidenceState / GraphReadiness / RemediationOutcome
```

Automatic remediation is allowed only when the operation is deterministic,
local, bounded, and authorized by the GraphSlice sufficiency proof for the
requested task. User-triggered or policy-triggered remediation is required when
the operation needs workspace changes, network/fetch behavior, hosted/team
visibility, reviewer disambiguation, or durable governance materialization.

## Authority Boundary

Remediation may create or update local cache records:

- `GraphSnapshot`;
- `GraphSlice`;
- slice coverage manifests;
- parse/resolve status;
- stale/invalidation markers;
- remediation job status;
- derived currentness view data.

Remediation must not directly create or mutate:

- accepted `BindingEvidence`;
- `RevalidateBinding` events;
- review state such as `needs_grounding_review`;
- compliance verdicts;
- signoff state;
- `collision_pending`;
- release-valve governance results.

A verified remediation result may feed a later governed command. For example,
`binding.revalidate` may show that the old target still verifies at a new
snapshot. Recording that as durable revalidation history still requires the
normal event-store path and policy or reviewer acceptance.

## Remediation Classes

| Class | Description | Default trigger | Durable governance? |
|---|---|---|---|
| `auto_local` | Deterministic local repair that does not require user input or external visibility. | Daemon may run automatically. | No. |
| `user_local` | Local repair requiring workspace member action, such as fetch, checkout, commit, or selecting a dirty-snapshot policy. | Surface prompts or command. | No. |
| `policy_local` | Repair or revalidation requested by policy before a governed action, such as merge-blocking compliance. | Policy-triggered command. | Only if later accepted. |
| `review_required` | Evidence can be inspected, but identity or meaning cannot be resolved without reviewer choice. | Reviewer command. | Only reviewer/governance event. |
| `hosted_required` | Required scope is outside local visibility. | Hosted/team graph request. | No local durable fact. |
| `non_remediable` | Exact local support is unavailable or the target is genuinely absent after sufficient proof. | Surface explanation. | Only if reviewer/policy acts. |

## State Handling

### Evidence States

| Input state | Remediation path | Automatic? | Output if successful | Escalation if unresolved |
|---|---|---|---|---|
| `unknown_not_indexed` | Build or expand the required snapshot/slice. | Yes, when local source is available and scope is bounded. | Retry proof; then `verified`, `not_found`, `ambiguous`, `unsupported`, or remaining unknown. | View-level `indexing`/`failed`; Decision-specific review only if tied to accepted binding evidence after retry. |
| `unknown_stale` | Refresh, invalidate, rebuild, or revalidate affected snapshot/slice. | Yes for cache/index-version/hash staleness; user action for dirty files or missing refs. | Retry proof at requested snapshot. | `needs_grounding_review` only for a specific binding mismatch after sufficient retry. |
| `ambiguous` | Re-run exact resolution only if stale/partial graph caused ambiguity; otherwise ask reviewer to choose target. | No, unless the ambiguity source is remediable indexing. | `verified` only after deterministic disambiguation or reviewer-selected revalidation. | `needs_grounding_review`. |
| `approximate_candidate` | Use candidate as a hint for exact validation or slice expansion. | Yes for bounded local exact validation. | Exact state from validation. | Remains advisory; never escalates by itself. |
| `unsupported` | No graph repair unless support is added or hosted provider supports exact validation. | No. | `verified` only after exact supported provider proof. | `needs_grounding_review` only when a specific accepted binding depends on unsupported exact evidence. |
| `not_found` | No repair if sufficiency proof covered the exact target and snapshot. Optional retry at a different requested snapshot. | No. | New state only for a different snapshot/target. | `needs_grounding_review` for accepted binding loss; source-only/advisory otherwise. |
| `verified` | No remediation required. | Not applicable. | Derived currentness or candidate input to governed command. | Not applicable. |
| `failed` | Retry if recoverable; otherwise surface failure reason. | Bounded retry only. | Retry proof. | View-level failure; review only for Decision-specific binding problem. |

### Readiness States

| Input readiness | Remediation path | Surface behavior |
|---|---|---|
| `ready` | None. | Use graph evidence normally within declared scope. |
| `indexing` | Continue or poll remediation job. | Show job id, scope, and retry status. |
| `stale` | Rebuild or revalidate snapshot/slice. | Show stale trigger and allowed repair action. |
| `failed` | Retry when recoverable; otherwise fall back to source-only/advisory when policy permits. | Show failure reason and required actor. |
| `not_configured` | Configure local graph support or request hosted/team graph check. | Do not claim verified graph evidence. |

## First-Pass Operations

These names describe daemon operations. They may be exposed through
`ToolRequest` commands later, but the first RFQ decision is the behavior and
authority boundary, not final API spelling.

| Operation | Purpose | Allowed actor/trigger | Notes |
|---|---|---|---|
| `graph.index_snapshot` | Build graph data for a resolved `validated_sha`. | Daemon auto, user, or policy. | Produces or updates `GraphSnapshot`; no governance state. |
| `graph.expand_slice` | Add files, symbols, or neighborhoods required by a `GraphScopePlan`. | Daemon auto when bounded; user/policy when expensive. | Must produce a new sufficiency proof. |
| `graph.refresh_snapshot` | Re-check snapshot freshness against index/parser/resolver versions. | Daemon auto or user. | May mark stale or ready. |
| `graph.invalidate_slice` | Mark affected slice/neighborhood stale after dirty file, hash mismatch, or version drift. | Daemon auto. | Does not delete accepted binding history. |
| `graph.rebuild_snapshot` | Rebuild stale, corrupt, or version-mismatched graph data. | Daemon auto when local and bounded; user/policy otherwise. | Old snapshot identity remains addressable. |
| `graph.resolve_snapshot` | Resolve refs, merge-base, worktrees, or requested SHAs. | Daemon auto; user action for missing refs. | Unresolved refs are snapshot-resolution failures, not `not_found`. |
| `binding.revalidate` | Inspect accepted binding evidence against a requested current snapshot. | User, policy, or reviewer; daemon may propose after remediation. | Read-only unless a later governed event records the result. |
| `remediation.plan` | Return suggested remediation actions without executing them. | Any local surface through daemon. | Useful for UI, MCP, and dry-run behavior. |

Automatic operations must be bounded by a `GraphScopePlan`. If the operation
would expand beyond local configured limits, require user or policy action and
return the next action rather than silently widening scope.

## Trigger Rules

### Automatic Remediation

The daemon may run remediation automatically when all of the following hold:

- the requested snapshot is locally resolvable;
- the source files required by the `GraphScopePlan` are locally available;
- the action is cache/index repair, slice expansion, version refresh, or
  targeted revalidation;
- the action cannot mutate working-tree files, refs, or durable governance
  state;
- the expected cost is within configured local limits;
- the final claim still requires a fresh `ScopeSufficiencyProof`.

Examples:

- expand a partial slice for a requested symbol neighborhood;
- rebuild a stale slice after index-version drift;
- retry validation after parsing a previously not-indexed file;
- invalidate a cached neighborhood when its stored file hash mismatches the
  expected blob.

### User-Triggered Remediation

User action is required when remediation needs:

- fetching a missing base ref;
- checking out or creating a local worktree;
- choosing whether to validate clean `HEAD` or a dirty worktree;
- committing, stashing, or discarding dirty changes;
- raising cost limits for broad graph expansion;
- requesting hosted/team graph visibility.

Dirty worktrees must not silently produce durable commit-anchored evidence.
The user may choose source-only/advisory output, clean-snapshot validation, or
an explicit ephemeral dirty slice that is marked non-durable.

### Policy-Triggered Remediation

Policy may require remediation before a governed action, for example before a
merge-blocking compliance read. Policy-triggered remediation can require a
fresh local graph proof, but unresolved graph state is not a conflict and does
not become `collision_pending`.

### Reviewer-Triggered Remediation

Reviewer action is required when:

- multiple possible targets remain after exact local resolution;
- a binding's product meaning may have changed even though code identity still
  appears continuous;
- an unsupported target needs manual grounding;
- a verified `not_found` contradicts an accepted binding;
- a replacement binding or durable revalidation event should be accepted.

## Cache And Snapshot Cases

### Dirty Worktree

If worktree files differ from the clean `validated_sha`, remediation must:

- return `unknown_stale` for affected durable-snapshot claims;
- offer clean snapshot validation, source-only/advisory output, or an explicit
  ephemeral dirty slice;
- prevent dirty-slice results from materializing commit-anchored
  `BindingEvidence`;
- preserve any old accepted `BindingEvidence` as history.

### Ref Movement

If a provenance ref moves:

- resolve the new SHA and build or reuse a snapshot for it;
- keep the old snapshot addressable by original `validated_sha`;
- do not treat ref movement alone as binding loss;
- revalidate accepted evidence only when a caller requests currentness or
  policy requires it.

### File Hash Mismatch

If cached file hashes differ from expected blobs:

- invalidate affected files and neighborhoods;
- perform targeted re-index when source is available;
- return `unknown_stale` until the repair succeeds;
- allow `not_found` only after a sufficient fresh proof over the requested
  target.

### Indexer Or Resolver Version Change

If indexer, parser, grammar, schema, or resolver versions change:

- mark affected snapshots/slices stale;
- rebuild before serving verified claims;
- preserve `BindingAnchor` metadata for inspection and explanation;
- retry binding revalidation only after the rebuilt slice has a sufficient
  proof.

### Partial Graph Slice

If a claim needs a neighborhood outside the current slice:

- run `graph.expand_slice` when bounded and local;
- return `unknown_not_indexed` when expansion is unavailable or incomplete;
- never treat "not in this slice" as "absent from the snapshot."

### Cache Payload Missing Or Corrupt

If retained identity exists but graph payload is missing or corrupt:

- preserve `BindingAnchor`;
- return `unknown_not_indexed` or `failed`;
- rebuild from source when locally available;
- route to user/hosted action if source is unavailable.

## Binding Revalidation

`binding.revalidate` is a read/inspect operation unless a later governed event
records its result. It starts from accepted binding evidence plus
`BindingAnchor`, resolves the requested current snapshot, builds a
`binding_revalidation` or `remediation` slice, and requires a sufficient proof.

Possible derived outcomes:

| Derived outcome | Meaning | Durable by default? |
|---|---|---|
| `confirmed` | The same target verifies at the requested current snapshot. | No. |
| `moved` | Deterministic continuity proves the same target moved or was renamed. | No. |
| `replaced_candidate` | A possible replacement target exists but needs reviewer/policy acceptance. | No. |
| `lost` | Sufficient proof shows the accepted target is absent at the requested snapshot. | No. |
| `ambiguous` | Multiple plausible targets remain. | No. |
| `inconclusive` | Required graph scope is stale, unsupported, unavailable, failed, or not indexed. | No. |
| `hosted_required` | Required visibility is outside local scope. | No. |

Mapping to durable `RevalidateBindingPayload` is a later governed action:

- `confirmed` may become `RevalidationOutcome::Confirmed`;
- reviewer-accepted replacement may become `RevalidationOutcome::Replaced`;
- accepted binding loss may become `RevalidationOutcome::Failed`;
- unresolved graph state may become `RevalidationOutcome::Inconclusive` only
  when policy/reviewer explicitly wants durable audit of that failed attempt.

The daemon must not record those outcomes just because remediation ran.

## Response Shape

Remediation responses should include:

- `status`: `planned | running | completed | failed | escalated |
  hosted_required`;
- `remediation_job_id`, when asynchronous;
- `operation`;
- `task_intent`;
- `scope_plan_id`;
- `graph_snapshot_id` and `graph_slice_id`, when known;
- `validated_sha`;
- `input_state`;
- `output_state`;
- `sufficiency_proof`;
- `retryable`;
- `required_actor`;
- `allowed_next_actions`;
- `warnings`;
- `escalation`, when review, user action, or hosted/team visibility is needed.

Example:

```json
{
  "status": "completed",
  "operation": "graph.expand_slice",
  "scope_plan_id": "gsp-...",
  "graph_snapshot_id": "gs1:...",
  "graph_slice_id": "gsl-...",
  "input_state": "unknown_not_indexed",
  "output_state": "verified",
  "retryable": false,
  "required_actor": null,
  "allowed_next_actions": ["binding.revalidate"],
  "warnings": []
}
```

Escalation responses should distinguish:

- `user_action_required`;
- `reviewer_disambiguation_required`;
- `hosted_required`;
- `unsupported_exact_validation`;
- `verified_absence_requires_grounding_review`;
- `source_only_available`.

## Preflight And Compliance Interaction

Preflight may trigger or suggest graph remediation, but it remains a query. It
must not mutate signoff, review, compliance, binding, or conflict state.

Rules:

- source-only relevance may still return relevant Decisions while graph
  remediation is pending;
- graph readiness gaps are view-level readiness facts;
- Decision-specific `needs_grounding_review` is reserved for accepted binding
  evidence problems after sufficient retry or reviewer escalation;
- local "no known conflict" remains scoped to locally resolved and sufficiently
  proven snapshots;
- hosted/team-only scope remains `hosted_required`, not a clean local pass.

Compliance may use refreshed graph results only when policy accepts the
evidence for the requested scope. A refreshed graph result does not itself
resolve compliance.

## Downstream Implementation Handoff

This RFQ should feed:

- #138: implement GraphStore-backed binding verification with remediation
  hooks for `unknown_not_indexed` and `unknown_stale`;
- #140: implement progressive preflight that can surface remediation status and
  retryable graph readiness without mutating governance state;
- #134: align `search.query` so approximate candidates can trigger exact
  validation without becoming verified search results;
- #154: evaluate CodeIdentity with remediation outcomes separate from
  probabilistic identity candidates;
- future UI/API work for remediation job status and allowed next actions;
- future governance-event work for accepting durable revalidation outcomes.

## Required Tests For Implementations

Implementation issues should cover:

- stale graph refreshed to verified after sufficient proof;
- not-indexed snapshot indexed and retried;
- partial slice expanded, with outside-slice claims staying
  `unknown_not_indexed`;
- dirty worktree returning `unknown_stale` or explicit ephemeral dirty-slice
  behavior;
- file hash mismatch invalidating and rebuilding affected neighborhoods;
- indexer/resolver version drift marking snapshots stale before verified
  claims;
- unavailable local ref requiring user fetch or checkout action;
- hosted/team scope surfacing `hosted_required`;
- authoritative `not_found` after fresh sufficient proof escalating to
  grounding review for accepted bindings;
- ambiguous targets requiring reviewer disambiguation;
- unsupported targets returning non-remediable status;
- approximate or LLM-assisted evidence remaining advisory until exact
  validation succeeds;
- remediation not creating durable governance events without accepted policy or
  reviewer action.

Validation path for docs-only RFQs:

```sh
git diff --check
```

Code implementation follow-ups should add deterministic fixtures in
`bicameral-runtime` and API response conformance coverage.

## Non-Goals

- Do not implement GraphStore storage here.
- Do not implement hosted/team graph visibility here.
- Do not define final durable governance event policy for accepted binding
  loss or replacement.
- Do not make approximate, semantic, or LLM-assisted evidence equivalent to
  verified graph evidence.
- Do not split canonical review state into new remediation-specific states.
