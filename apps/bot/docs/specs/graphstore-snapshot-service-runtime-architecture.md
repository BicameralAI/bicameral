# GraphStore Snapshot Service Runtime Architecture

**Date:** 2026-06-16
**Status:** proposed
**Issue:** #378
**Related:** ADR-0010, ADR-0021, #230, #377, BicameralAI/bicameral-mcp#594

## Runtime Problem

Stage 1 manual evidence refresh needs to answer whether accepted
`BindingEvidence` is still current at a later code snapshot. Existing contracts
define snapshot-scoped graph evidence and GraphStore cache semantics, and the
runtime has first slices such as `SnapshotResolver`, `FileInspector`, and
`code_source_refresh`.

The unresolved runtime question is ownership:

```text
Who rebuilds, refreshes, caches, and serves codegraph / GraphSnapshot data for
a requested workspace, ref, and commit?
```

Until that boundary is explicit, dashboard and MCP surfaces can accidentally
overclaim code-evidence freshness. A manual refresh may prove the gateway path
works while still depending on stale or missing graph data. The safe behavior is
to return typed graph readiness/currentness outcomes rather than claiming a
binding is fresh, stale, compliant, or blocked without a bot-owned graph service
decision.

## Goals And Non-Goals

Goals:

- Define the bot-owned runtime component responsible for GraphStore
  snapshot rebuild, index, cache, and serve behavior.
- Classify graph lifecycle states as durable cache state, derived read-model
  state, command outcome, or collapsed API state.
- Define how Stage 1 manual evidence refresh checks graph readiness without
  mutating accepted binding, signoff, compliance, or review state.
- Define MCP and dashboard boundaries for requesting and displaying graph
  refresh outcomes.
- Give implementation issues a minimal sequencing path that can start before a
  full graph engine exists.

Non-goals:

- Do not implement hosted/team graph visibility.
- Do not turn GraphStore cache entries into Decision Ledger events.
- Do not make graph readiness a Decision lifecycle state.
- Do not define compliance, enforcement, automated signoff, or "safe to
  proceed" behavior.
- Do not require a full repository graph before Stage 1 can return honest typed
  readiness outcomes.

## Runtime Components

### GraphSnapshotService

`GraphSnapshotService` is the bot-owned runtime boundary for graph snapshot
resolution, indexing, cache lookup, slice expansion, and readiness reporting.

It owns these operations:

- resolve workspace/ref/commit to `SnapshotResolution`;
- check or create a `GraphSnapshot` cache entry keyed by ADR-0021 identity;
- start or poll graph indexing jobs;
- build minimal `GraphSlice` data when a bounded request can be satisfied
  locally;
- inspect retained `BindingAnchor` / accepted `BindingEvidence` against a
  requested current snapshot;
- return `GraphReadinessView`, `GraphEvidenceState`, and
  `CodeSourceRefreshResult`-style outcomes.

It must not:

- append governance events;
- accept `BindingEvidence`;
- mutate signoff, compliance, conflict, review, or Decision state;
- call MCP for grounding;
- compute "safe" or "blocked" conclusions from missing or stale graph data.

### Existing Runtime Slices

Existing code remains useful, but each piece is narrower than the service:

| Component | Current role | GraphSnapshotService role |
|---|---|---|
| `SnapshotResolver` | Resolves git refs to `GraphSnapshot` identity and typed `SnapshotResolution`. | Adapter used by the service for local git-backed snapshot resolution. |
| `FileInspector` | Inspects file/range evidence against a resolved snapshot. | Minimal exact local provider for Stage 1 file/range currentness. |
| `code_source_refresh` | Maps readiness and file inspection into derived `CodeSourceCurrentness`. | First evaluator run by manual evidence refresh after service readiness check. |
| `GraphReadinessView` | Surface-safe read model for graph readiness. | Output shape for UI/MCP display and API responses. |
| `ProjectionState` | Replayed V1 read model. | Consumer of accepted binding evidence and derived readiness/currentness views, not owner of graph indexing. |

### Provider Adapter

Graph indexing is provider-backed. The initial provider can be the existing
local git/file/symbol resolver path. Later providers may include a richer local
graph engine or hosted/imported graph data, but all providers must map into
ADR-0010/ADR-0021 vocabulary:

- `GraphSnapshot`;
- `GraphSlice`;
- `GraphReadiness`;
- `GraphEvidenceState`;
- `GraphScopeCoverage`;
- `CodeTargetIdentity` when continuity is needed.

Provider confidence must not become `verified` unless the provider supplies an
exact proof over a sufficient requested scope.

## Canonical vs Derived State

Canonical governance state:

- accepted `Decision`;
- accepted `BindingEvidence`;
- accepted `SourceEvidence`;
- accepted `RevalidationOutcome`, when a future governed command records it;
- signoff/review/compliance events, when those commands exist.

Durable local cache state:

- `GraphSnapshot` cache metadata;
- `GraphSlice` cache metadata;
- index job records;
- stale/invalidation markers;
- retained `BindingAnchor` metadata;
- provider/index version metadata.

Derived read-model state:

- `GraphReadinessView`;
- per-Decision code evidence currentness;
- manual refresh result summaries;
- source-only/advisory readiness summaries.

Transient command outcomes:

- `SnapshotResolution::RefNotAvailable`;
- `SnapshotResolution::HostedRequired`;
- `needs_index_refresh`;
- `unsupported_workspace`;
- request-specific failure details.

Graph cache and derived state are not Decision Ledger authority. They may be
recomputed, evicted, rebuilt, or corrected without rewriting accepted evidence.

## State Refinement Check

The states named in #378 should be handled as follows:

| State | Classification | API/display mapping |
|---|---|---|
| `ready` | Collapsed `GraphReadiness` value, derived from cache/job state. | `GraphReadiness::Ready`; verified claims still require exact evidence. |
| `indexing` | Durable job/cache lifecycle plus collapsed readiness. | `GraphReadiness::Indexing`; display job/scope when available. |
| `stale` | Durable cache lifecycle plus collapsed readiness. | `GraphReadiness::Stale`; evidence states map to `unknown_stale`. |
| `failed` | Durable job/cache lifecycle plus collapsed readiness. | `GraphReadiness::Failed`; include retryability and failure detail. |
| `not_configured` | Workspace capability/config state plus collapsed readiness. | `GraphReadiness::NotConfigured`. |
| `unknown_not_indexed` | Evidence state for a requested proof. | `GraphEvidenceState::UnknownNotIndexed`; may suggest indexing. |
| `unknown_stale` | Evidence state for a requested proof. | `GraphEvidenceState::UnknownStale`; may suggest rebuild/revalidate. |
| `needs_index_refresh` | Command outcome / next action. | Surface action derived from `stale`, `unknown_stale`, or version drift. |
| `graph_unavailable` | Collapsed currentness/readiness outcome. | `CodeSourceCurrentness::GraphUnavailable` or typed ToolResponse failure. |
| `unsupported_workspace` | Command outcome / capability failure. | Prefer typed unsupported response plus `GraphReadiness::NotConfigured` when graph support cannot run. |

## State Minimization Review

Do not add new top-level durable graph states unless a cache entry or job must
replay them after daemon restart.

Use the existing collapsed enums for public API where possible:

- readiness: `ready`, `indexing`, `stale`, `failed`, `not_configured`;
- evidence: `verified`, `not_found`, `unknown_not_indexed`,
  `unknown_stale`, `ambiguous`, `unsupported`, `approximate_candidate`;
- currentness: `current`, `content_changed`, `target_missing`,
  `graph_unavailable`, `graph_not_configured`.

Use command-specific next-action values for UX affordances:

- `needs_index_refresh`;
- `fetch_ref_required`;
- `retry_indexing`;
- `choose_target`;
- `continue_source_only`;
- `request_hosted_graph`.

These next actions are response fields, not new governance states.

## Authority Ownership Matrix

| Surface/component | May request | May compute graph facts | May write graph cache | May write governance facts | Notes |
|---|---:|---:|---:|---:|---|
| Dashboard | Yes | No | No | No | Renders daemon-authored outcomes and allowed actions. |
| MCP | Yes | No | No | No | Thin `ToolRequest` client; no legacy grounding fallback. |
| CLI/TUI | Yes | No | No | No | Same boundary as dashboard/MCP. |
| Gateway | Yes, through typed request | No | No | No | Adapts external inputs only. |
| Daemon dispatch | Yes | Coordinates | No direct governance write for graph facts | Only through governed commands | Validates authority context and routes. |
| `GraphSnapshotService` | Yes | Yes | Yes | No | Owns graph runtime state and derived outcomes. |
| Graph provider adapter | No external authority | Yes, scoped | Via service only | No | Provider output maps into Bicameral states. |
| EventStoreAdapter | No | No | No | Yes | Only accepted governance events are canonical. |

## Runtime Scenarios

### Manual Evidence Refresh

```text
manual refresh requested
-> daemon validates ToolRequest and actor/workspace authority
-> replayed state loads requested Decision and accepted bindings
-> GraphSnapshotService resolves requested workspace/ref/commit
-> service checks cache/job/readiness
-> if not ready: return typed readiness/currentness outcome
-> if ready: evaluate binding target through exact provider
-> return derived currentness result
```

Allowed terminal outcomes:

- current;
- content changed;
- target missing;
- unsupported;
- needs review;
- graph unavailable;
- graph not configured;
- failed.

None of these outcomes mutates signoff or compliance in Stage 1.

### Missing Or Stale Graph

```text
refresh requested
-> snapshot resolves to commit
-> cache missing/stale/version-mismatched
-> service starts bounded index/rebuild when allowed, or returns next action
-> response is indexing/stale/needs_index_refresh
```

The surface may show a refresh or retry action. It must not say the Decision is
fresh, stale, compliant, noncompliant, safe, or blocked.

### Unsupported Workspace

```text
refresh requested
-> workspace is missing, outside configured roots, non-git, unsupported language
   only, or inaccessible
-> service returns unsupported_workspace/not_configured
```

The correct user path is configure workspace, continue source-only, or request
hosted/team graph visibility when available.

### MCP Manual Refresh

```text
caller -> MCP tool -> daemon ToolRequest
daemon -> GraphSnapshotService -> derived response
MCP -> render response verbatim
```

MCP must not call old `link_commit`, `ensure_ledger_synced`, local code-locator
state, or any local graph rebuilding path.

## Event, Command, And Protocol Contracts

### Stage 1 Command

Add a bot-owned command for manual evidence refresh. Final naming can be decided
in implementation, but the contract should be equivalent to:

```text
evidence.refresh_manual
```

Minimum request fields:

- `decision_id`;
- optional `binding_id`;
- `workspace_id`;
- optional `requested_ref`, defaulting to `HEAD`;
- `mode = manual`;
- optional `allow_index = false | bounded`;

Minimum response fields:

- `decision_id`;
- `binding_id`, when scoped;
- `status`: accepted, rejected, unsupported, deferred, failed;
- `graph_readiness`;
- `currentness`, when evaluated;
- `snapshot_resolution`;
- `evaluated_snapshot_id`, when available;
- `next_actions`;
- `runtime_trace`.

### Graph Runtime Commands

Internal daemon operations may be exposed later as developer/admin commands,
but Stage 1 should treat them as service methods:

- `graph.resolve_snapshot`;
- `graph.ensure_snapshot`;
- `graph.index_snapshot`;
- `graph.refresh_snapshot`;
- `graph.expand_slice`;
- `graph.rebuild_snapshot`;
- `binding.evaluate_currentness`.

### Persistence Contract

Manual refresh may persist operational/cache records and derived projection
state when useful. It must not append an accepted governance event unless a
separate governed command is introduced for accepted revalidation.

If the implementation stores refresh observations, they should be operational
receipts or derived read-model records with explicit non-authority wording.

## Error, Retry, And Recovery Behavior

| Failure | Response | Retry/recovery |
|---|---|---|
| Missing Decision | Typed rejected. | Pick a valid Decision. |
| Decision has no accepted binding | Typed rejected or source-only outcome. | Add/accept binding through governed path. |
| Missing workspace | `unsupported_workspace` / `graph_not_configured`. | Configure local workspace. |
| Requested ref missing | `SnapshotResolution::RefNotAvailable`. | Fetch ref, choose available ref, or continue source-only. |
| Dirty worktree mismatch | `unknown_stale` / `needs_index_refresh` with dirty detail. | Commit/stash/choose clean snapshot or explicit non-durable dirty mode in future. |
| Cache missing | `unknown_not_indexed` or `indexing`. | Build bounded snapshot/slice. |
| Cache version drift | `stale` / `needs_index_refresh`. | Rebuild with current index version. |
| Cache corruption | `failed`, preserving retained anchors. | Clear/rebuild cache; do not rewrite accepted evidence. |
| Unsupported language/file | `unsupported`. | Source-only, hosted provider, or later provider support. |
| Ambiguous target | `ambiguous` / `needs_review`. | Reviewer refines or chooses target. |

## Security, Privacy, And Authority Boundaries

- Local graph indexing must stay inside configured workspace roots.
- Graph cache paths must not be accepted from MCP/dashboard input.
- Path traversal attempts return unsupported or rejected outcomes.
- Graph cache data is local runtime/cache data, not a portable governance
  artifact.
- Hosted/imported graph data must identify provider and index version.
- MCP and dashboard cannot cause unbounded indexing without daemon policy.
- Dirty worktree evidence must not become commit-anchored verified evidence.
- Missing graph data must not be converted into "no conflict", "safe to
  proceed", "blocked", compliance, or signoff state.

## Implementation Sequencing

1. Define `GraphSnapshotService` trait and in-memory/local provider boundary in
   `bicameral-runtime`.
2. Wrap existing `SnapshotResolver`, `FileInspector`, and
   `code_source_refresh` behind the service for Stage 1.
3. Add manual evidence refresh command/response structs in `bicameral-api`.
4. Add daemon dispatch for manual refresh with typed unsupported/deferred
   behavior when graph support is not configured.
5. Add focused tests for current, content-changed, target-missing,
   unsupported workspace, stale/indexing, and no signoff/compliance mutation.
6. Add MCP thin-client mapping after the bot command/capability exists.
7. Add durable local cache/job records only after the trait boundary and
   minimal command path are covered by conformance tests.

## Issue Generation Notes

Recommended implementation follow-ups:

- Define `GraphSnapshotService` trait and local provider adapter.
- Add `evidence.refresh_manual` ToolRequest command and fixtures.
- Implement Stage 1 manual refresh through `GraphSnapshotService`.
- Add dashboard display for daemon-authored graph readiness/currentness.
- Add MCP `#594` mapping after bot command shape is merged.
- Add graph cache/job persistence and rebuild lifecycle as a separate slice.
- Add automatic refresh scheduling only after the manual path is stable.

## ADR Implications

- ADR-0010 remains the authority for `GraphReadiness` and
  `GraphEvidenceState`.
- ADR-0021 remains the authority for GraphStore cache identity, slice lifecycle,
  stale invalidation, and retained `BindingAnchor` metadata.
- This spec adds the runtime service boundary between those ADRs and Stage 1
  manual evidence refresh.
- If implementation needs new public states beyond the minimized set above,
  update ADR-0010/ADR-0021 rather than introducing surface-only aliases.

## Acceptance Criteria

- The spec identifies a bot-owned `GraphSnapshotService` as the runtime owner.
- Manual evidence refresh has a command path that can return typed readiness
  outcomes without claiming currentness when the graph is unavailable.
- State names from #378 are classified as durable cache state, derived state,
  command outcome, or existing collapsed enum values.
- MCP/dashboard boundaries are explicit: request/render only, no graph rebuild
  or authority mutation.
- Implementation sequencing identifies a minimal Stage 1 path that does not
  require full GraphStore persistence before honest typed outcomes exist.

## Validation Plan

Docs/spec validation:

- review against ADR-0010 and ADR-0021 for state vocabulary drift;
- review against `CONTEXT.md` for product term consistency;
- review #377 and BicameralAI/bicameral-mcp#594 for command sequencing.

Implementation validation once follow-up work starts:

- unit tests for `GraphSnapshotService` readiness mapping;
- runtime tests for manual refresh current/content-changed/missing outcomes;
- negative tests proving signoff and compliance state do not mutate;
- protocol fixture for capability-present and capability-absent MCP paths;
- dashboard/API test proving typed JSON is returned for unsupported graph state.
