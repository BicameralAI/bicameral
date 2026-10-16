# Deterministic CodeLocator And Graph Snapshot Workflow

**Date:** 2026-06-16  
**Status:** proposed implementation spec  
**Scope:** `bicameral-bot` runtime, local daemon/gateway, MCP/dashboard/API surfaces  
**Related:** ADR-0010, ADR-0017, ADR-0019, ADR-0021; `graphslice-planning-sufficiency.md`; `graphstore-snapshot-service-runtime-architecture.md`; `graph-remediation-revalidation.md`; `preflight-search-readiness.md`; issues #230, #377, #378, BicameralAI/bicameral-mcp#594

## Summary

`CodeLocator` is a deterministic retrieval and verification service. It uses
explicit caller hints, local search, graph snapshots, graph slices, accepted
bindings, and source evidence to find likely code targets for a Decision or
candidate. It must not call an LLM.

LLMs, MCP callers, dashboard users, integrations, or other agents may form
queries and hints. Those hints are advisory input only. `CodeLocator` verifies
only deterministic facts against a bot-owned `GraphSnapshotService`.

The code graph is a navigation and evidence substrate, not a graph viewer and
not a governance authority. Users should see ranked code targets, evidence,
confidence, graph readiness, and next actions. They should not be shown the
whole graph or asked to reason about internal graph topology.

## Goals

- Define a deterministic `CodeLocator` contract that humans and agents can
  implement without relying on LLM-generated code targets as truth.
- Define how `CodeLocator` uses `GraphSnapshot`, `GraphSlice`,
  `GraphScopePlan`, and `ScopeSufficiencyProof`.
- Define the data flow for manual refresh, PR/head refresh, locator search,
  binding creation, binding inspection, and evidence currentness.
- Define proposed ToolRequest/API command shapes for implementation.
- Preserve the authority boundary: graph evidence can support navigation,
  binding proposals, currentness checks, and review prompts, but cannot assert
  compliance, signoff, enforcement, merge safety, or "safe to proceed."

## Non-Goals

- Do not implement a graph visualization UI.
- Do not feed the whole graph to users or LLMs.
- Do not require a full repository graph before useful locator results can
  exist.
- Do not snapshot every commit in a fast AI-generated commit stream.
- Do not make graph cache entries, slices, or currentness observations
  canonical Decision Ledger events.
- Do not use LLM confidence as `GraphEvidenceState::Verified`.
- Do not implement hosted/team graph visibility in the first slice.

## Existing Decisions This Spec Depends On

- `GraphSnapshot` identity is commit-addressed. Durable graph identity is based
  on repository identity, commit SHA / `validated_sha`, and graph/index version.
  Moving branch names or PR heads are provenance and refresh inputs, not durable
  identity.
- Accepted `BindingEvidence` is immutable and may carry `graph_snapshot_id` and
  `validated_sha`.
- `GraphStore` and `GraphSlice` are bot-owned cache/proof machinery. They are
  not ledger entities.
- MCP is a thin client. MCP can submit ToolRequests and render daemon results,
  but cannot rebuild graphs, compute graph evidence, accept bindings, or infer
  compliance.
- Evidence refresh/currentness is separate from signoff and compliance.

## Definitions

### CodeLocator

`CodeLocator` is a deterministic runtime service that retrieves, ranks, and
verifies code target candidates for a specific task.

It may use:

- exact path lookup;
- file/range inspection;
- symbol lookup;
- lexical search;
- changed-file matching;
- import/reference/call neighborhood expansion;
- prior accepted binding anchors;
- source evidence references;
- graph slice coverage and sufficiency proofs.

It must not use:

- LLM calls;
- model-generated claims as verified evidence;
- unsupported graph neighborhoods as negative proof;
- stale graph data as verified evidence.

### Query Or Hint

A query or hint is caller-supplied input. It may come from a human, LLM,
dashboard, MCP, integration, source evidence, or prior runtime step.

Examples:

- query terms: `"github device flow provider auth"`;
- files: `["crates/bicameral-gateway/src/routes.rs"]`;
- symbols: `["start_provider_connection"]`;
- changed files from a PR;
- accepted binding IDs;
- source evidence excerpts;
- decision or candidate IDs.

Hints are not authority. They only seed deterministic lookup.

### GraphSnapshot

A `GraphSnapshot` describes graph facts for one immutable commit and graph
index version.

Identity:

- `graph_snapshot_id`;
- `repo_id`;
- `validated_sha` / `commit_sha`;
- `index_version`.

Provenance:

- `ref_name`;
- `merge_base_sha`;
- `workspace_id`;
- `index_source`.

A snapshot is commit-addressed, but the system does not need to build a
snapshot for every commit.

### Moving Refresh Target

A moving refresh target is what the user or workflow points at:

- workspace `HEAD`;
- local branch head;
- PR head;
- PR base;
- merge base;
- explicit ref.

Moving targets can change while indexing is queued or running. Completed graph
data remains valid for the immutable commit it was built for, but may become
stale relative to the current moving target.

### GraphSlice

A `GraphSlice` is a scoped graph subset derived from one `GraphSnapshot`.

A slice is not the combination of snapshots. It is the relevant subset of graph
facts for a task.

Examples:

- changed files and symbols in a PR head snapshot;
- symbol neighborhood around an explicit function;
- accepted binding anchor and current target neighborhood;
- query-reached files/symbols/references;
- file set and import/reference edges needed for locator ranking.

A comparison workflow may use:

```text
base GraphSnapshot
+ head GraphSnapshot
+ one or more GraphSlices
```

The slice is the task-specific subset extracted from those snapshots.

### Comparison Slice

A comparison slice is a task-specific view over base/head snapshots. It is used
for PR and diff workflows.

It may include:

- files changed between base and head;
- symbols touched by those files;
- import/call/reference neighborhood around touched symbols;
- accepted binding anchors that overlap changed regions;
- currentness observations for previously accepted bindings.

### ScopeSufficiencyProof

`ScopeSufficiencyProof` is the proof that a graph slice covers the facts
required for a specific task.

Only a sufficient proof can support:

- `GraphEvidenceState::Verified`;
- verified `NotFound`;
- exact binding proposal;
- exact currentness result.

If the slice does not cover the required scope, the response must use
`unknown_not_indexed`, `unknown_stale`, `unsupported`, `ambiguous`,
`approximate_candidate`, `graph_unavailable`, or another typed non-authority
state.

### BindingProposal

A `BindingProposal` is a verified or advisory graph-backed candidate for future
accepted `BindingEvidence`.

It is not accepted evidence by itself. It becomes accepted `BindingEvidence`
only through the governed binding materialization path.

### CodeSourceCurrentness

`CodeSourceCurrentness` is derived read-model state for an existing accepted
binding evaluated against a later snapshot.

Examples:

- `current`;
- `content_changed`;
- `target_missing`;
- `graph_unavailable`;
- `graph_not_configured`.

Currentness does not mutate signoff, compliance, or the original accepted
binding.

## Design Principles

1. **Deterministic locator, advisory query formation.**  
   LLMs and users may form queries. `CodeLocator` deterministically retrieves
   and verifies candidates.

2. **Commit-addressed identity, moving-target scheduling.**  
   Completed graph data is anchored to immutable commits. Refresh scheduling
   targets moving PR/branch/workspace heads and coalesces rapid changes.

3. **Graph as evidence, not UI.**  
   User-facing surfaces show ranked code targets, reasons, readiness, and next
   actions, not a graph viewer.

4. **Slices over whole graph by default.**  
   Full snapshots are allowed, but the first implementation should use bounded
   slices for locator, PR, binding, and currentness work.

5. **Verified means exact proof over sufficient scope.**  
   High confidence, lexical similarity, LLM hints, and approximate graph
   neighborhoods may rank candidates but do not become verified evidence.

6. **No hidden mutation during reads.**  
   Search, preflight, history, and locate reads may check readiness and return
   next actions. Only explicit graph refresh/index commands may mutate graph
   cache state.

7. **No governance overclaim.**  
   Graph readiness/currentness never means compliance, signoff, enforcement,
   implementation correctness, merge safety, or "safe to proceed."

## Workflow Overview

### Manual Graph Refresh V1

The first implementation should support manual graph refresh as the only
mutating graph trigger.

```text
user clicks "Refresh code graph"
-> surface sends ToolRequest graph.refresh_snapshot
-> daemon resolves workspace/ref/PR target
-> daemon resolves immutable commit SHA
-> GraphSnapshotService checks cache
-> service builds or refreshes bounded snapshot/slice
-> service returns graph readiness and snapshot identity
-> UI shows ready/indexing/stale/failed/not_configured
```

If the moving target changes while indexing is running:

```text
indexing started for commit C10
-> PR head moves to C12
-> service may complete C10 snapshot
-> snapshot remains valid for C10
-> target readiness becomes stale relative to C12
-> response includes next action: refresh_latest
```

V1 should not automatically index C12 unless explicit policy allows it.

### Future Automatic Refresh

The runtime should keep trigger handling generic so future triggers can use the
same backend:

- manual refresh;
- locator request;
- evidence refresh;
- preflight;
- PR opened;
- PR updated;
- branch head changed;
- git hook;
- daemon watcher.

Only manual refresh mutates cache in V1. Automatic PR/watch/git-hook behavior
is deferred.

### Code Locate

```text
caller submits query/hints
-> daemon builds TaskIntent(intent_kind = code_locate)
-> snapshot target resolves to commit/snapshot readiness
-> GraphScopePlan defines required files/symbols/neighborhoods
-> GraphSnapshotService loads/builds eligible GraphSlice when already available
-> CodeLocator runs deterministic retrieval and ranking
-> service returns LocatorResult with candidates, reasons, evidence state,
   sufficiency, readiness, and next actions
```

If graph is missing or stale, `CodeLocator` may still return source-only or
lexical candidates, but they must be marked as non-verified.

### Binding Create

```text
caller submits explicit selected target or locator candidate
-> binding.create ToolRequest
-> daemon resolves requested snapshot
-> CodeLocator / GraphSnapshotService validates file/range/symbol
-> if exact proof sufficient: BindingProposal with verified graph evidence
-> governance policy decides whether proposal materializes as BindingEvidence
-> accepted BindingEvidence stores graph_snapshot_id + validated_sha
```

`binding.create` should not invoke an LLM. If an LLM suggested the file or
symbol, that suggestion is just a hint that must pass deterministic validation.

### Binding Inspect / Currentness

```text
caller requests binding.inspect or evidence.refresh
-> daemon loads accepted BindingEvidence
-> daemon resolves current requested snapshot
-> GraphSnapshotService builds/selects binding_revalidation slice
-> CodeLocator/FileInspector checks target existence/content/identity
-> returns CodeSourceCurrentness result
```

Possible outputs:

- `current`;
- `content_changed`;
- `target_missing`;
- `graph_unavailable`;
- `graph_not_configured`;
- `ambiguous`;
- `unsupported`.

No signoff/compliance state changes.

### PR With Ten New Commits

Assume base commit `A` and a PR that quickly advances:

```text
A -> C1 -> C2 -> C3 -> C4 -> C5 -> C6 -> C7 -> C8 -> C9 -> C10
```

When a refresh or locate request runs, the service resolves:

```text
base = A
target = latest PR head = C10
```

The service should not build snapshots for `C1` through `C9` unless a specific
audit/debug task requires them.

The preferred graph work is:

```text
GraphSnapshot(A), usually cached or built as base
GraphSnapshot(C10), latest useful target
GraphSlice(A..C10, scope = changed files + locator neighborhood)
```

If new commits arrive while indexing:

```text
C10 indexing starts
-> C11, C12 arrive
-> C10 result is stored for C10
-> current target is C12
-> user-facing state: stale relative to latest target
-> next action: refresh_latest
```

Later automatic scheduling may debounce/coalesce and index `C12`, but V1 can
require manual refresh.

## Proposed Runtime Components

### GraphSnapshotService

Bot-owned service responsible for graph snapshot and slice lifecycle.

Responsibilities:

- resolve workspace/ref/commit/PR target to immutable commit SHA;
- compute or load `GraphSnapshot`;
- compute or load `GraphSlice`;
- track readiness: `ready`, `indexing`, `stale`, `failed`,
  `not_configured`;
- coalesce/supersede indexing jobs for moving targets;
- expose graph readiness and currentness;
- return next actions for missing/stale/unsupported states.

Non-responsibilities:

- signoff;
- compliance;
- event-store governance writes;
- MCP/dashboard local fallback behavior;
- graph visualization.

### CodeLocator

Bot-owned deterministic service responsible for candidate retrieval, ranking,
and verification.

Responsibilities:

- canonicalize caller hints;
- run deterministic retrieval strategies;
- request graph slices or use available slices;
- produce `LocatorResult`;
- produce `BindingProposal` only when exact validation succeeds;
- provide reason codes for each candidate.

Non-responsibilities:

- LLM calls;
- accepting binding evidence;
- computing compliance;
- deciding whether implementation satisfies a Decision.

### RefreshScheduler

Internal daemon component for graph refresh jobs.

V1 may be minimal and manual-only. It should still represent job state so
future automatic triggers do not require a rewrite.

Internal job states:

- `queued`;
- `indexing`;
- `ready`;
- `stale`;
- `failed`;
- `superseded`;
- `cancelled`.

User-facing state should generally collapse `superseded` into `stale` or
`indexing_latest_available`.

## Proposed Data Types

These names are proposed. Implementers may adjust names while preserving the
fields and semantics.

### CodeLocateParams

```json
{
  "query": "string",
  "decision_id": "string optional",
  "candidate_id": "string optional",
  "source_evidence_ids": ["string"],
  "binding_ids": ["string"],
  "files": ["string"],
  "symbols": ["string"],
  "changed_files": ["string"],
  "diff_context": "string optional",
  "workspace_id": "string optional",
  "commit_sha": "string optional",
  "ref_name": "string optional",
  "graph_snapshot_id": "string optional",
  "base_ref": "string optional",
  "base_sha": "string optional",
  "head_ref": "string optional",
  "head_sha": "string optional",
  "scope_kind": "binding | diff | file_set | symbol_set | query | binding_revalidation",
  "limit": 20
}
```

Rules:

- At least one of `query`, `decision_id`, `candidate_id`, `files`, `symbols`,
  `changed_files`, `binding_ids`, or `source_evidence_ids` is required.
- If `commit_sha` is provided, it is the immutable target.
- If only `ref_name` / `head_ref` is provided, the daemon resolves it to a
  commit.
- `graph_snapshot_id` is an expectation. If it does not match the resolved
  target, return a typed snapshot mismatch.

### LocatorResult

```json
{
  "status": "found | ambiguous | not_found | graph_unavailable | graph_not_configured | unsupported | failed",
  "query": "string optional",
  "graph_readiness": "ready | indexing | stale | failed | not_configured",
  "snapshot": {
    "graph_snapshot_id": "string optional",
    "repo_id": "string optional",
    "commit_sha": "string optional",
    "index_version": "string optional",
    "ref_name": "string optional"
  },
  "scope": {
    "scope_kind": "string",
    "scope_fingerprint": "string optional",
    "proof_state": "sufficient | insufficient | stale | failed | hosted_required | not_available"
  },
  "candidates": [
    {
      "target": "string",
      "target_type": "file_path | code_region | symbol | dependency | deploy_surface",
      "file": "string",
      "range": "string optional",
      "symbol": "string optional",
      "evidence_state": "verified | not_found | unknown_not_indexed | unknown_stale | ambiguous | unsupported | approximate_candidate",
      "confidence": "high | medium | low | unknown",
      "reasons": [
        {
          "code": "exact_path | exact_symbol | lexical_match | changed_file | prior_binding | import_edge | call_edge | reference_edge | source_evidence_overlap",
          "detail": "string"
        }
      ],
      "graph_snapshot_id": "string optional",
      "validated_sha": "string optional",
      "content_hash": "string optional"
    }
  ],
  "warnings": ["string"],
  "next_actions": [
    "refresh_graph",
    "refresh_latest",
    "choose_candidate",
    "narrow_query",
    "continue_source_only",
    "request_hosted_graph"
  ]
}
```

### GraphRefreshParams

```json
{
  "workspace_id": "string optional",
  "ref_name": "string optional",
  "commit_sha": "string optional",
  "base_ref": "string optional",
  "base_sha": "string optional",
  "head_ref": "string optional",
  "head_sha": "string optional",
  "scope_kind": "diff | file_set | symbol_set | query | binding_revalidation",
  "files": ["string"],
  "symbols": ["string"],
  "allow_index": "false | bounded | full",
  "trigger": "manual | locator_request | evidence_refresh | preflight | pr_opened | pr_updated | branch_head_changed | git_hook | daemon_watcher"
}
```

V1 rules:

- `trigger = manual` is the only mutating trigger.
- `allow_index = bounded` is the default for manual refresh.
- `allow_index = full` may require a future admin/policy gate.
- Missing local refs return typed `ref_not_available` / next action instead
  of silently fetching.

### GraphRefreshResult

```json
{
  "status": "ready | indexing | stale | failed | not_configured | unsupported",
  "target": {
    "requested_ref": "string optional",
    "resolved_sha": "string optional",
    "current_head_sha": "string optional",
    "is_current_for_target": true
  },
  "snapshot": {
    "graph_snapshot_id": "string optional",
    "repo_id": "string optional",
    "commit_sha": "string optional",
    "index_version": "string optional"
  },
  "slice": {
    "graph_slice_id": "string optional",
    "scope_kind": "string optional",
    "scope_fingerprint": "string optional",
    "completeness": "diff_scoped | file_scoped | symbol_scoped | query_scoped | repair_scoped | not_available"
  },
  "job": {
    "job_id": "string optional",
    "state": "queued | indexing | ready | stale | failed | superseded | cancelled",
    "superseded_by_sha": "string optional"
  },
  "warnings": ["string"],
  "next_actions": ["string"]
}
```

## Proposed ToolRequest Commands

The following commands should be added to the local command registry when
implementation begins.

### `code.locate`

Purpose:

- deterministic code target retrieval and verification;
- returns `LocatorResult`;
- does not materialize `BindingEvidence`;
- may be used by dashboard, MCP, CLI, preflight, search, and binding flows.

Payload:

- `CodeLocateParams`.

Output:

- `LocatorResult`.

Authority:

- local read/query authority;
- may read graph cache and projection state;
- must not write governance state;
- may not mutate graph cache unless a future policy explicitly allows
  read-triggered bounded expansion. V1 should not mutate graph cache from this
  command.

### `graph.refresh_snapshot`

Purpose:

- explicit graph refresh/index command;
- V1 manual mutating trigger;
- builds or refreshes bounded graph snapshot/slice for a requested target.

Payload:

- `GraphRefreshParams`.

Output:

- `GraphRefreshResult`.

Authority:

- local workspace authority;
- may write graph cache/job state;
- must not write governance events;
- must not mutate working-tree files or git refs.

### `graph.status`

Purpose:

- read graph readiness for a workspace/ref/snapshot without indexing.

Payload:

```json
{
  "workspace_id": "string optional",
  "ref_name": "string optional",
  "commit_sha": "string optional",
  "graph_snapshot_id": "string optional"
}
```

Output:

- collapsed graph readiness, snapshot identity when available, stale detail,
  and next actions.

Authority:

- read-only.

### Existing Commands That Should Use CodeLocator

#### `binding.create`

Should accept explicit target(s) or selected `LocatorResult` candidate(s). It
should validate deterministically before producing `BindingProposal` or
accepted `BindingEvidence`.

Add fields when implementation begins:

- `locator_candidate_id` or embedded selected candidate;
- `graph_snapshot_id`;
- `validated_sha`;
- `scope_proof_id`, if sufficiency proof is persisted.

#### `binding.inspect`

Should use `CodeLocator` / `GraphSnapshotService` for deterministic
revalidation and currentness.

#### `preflight.run`

Should use `code.locate` internally for diff/file/symbol scope when available,
but must preserve preflight's read-only advisory boundary.

#### `search.query`

May call `CodeLocator` for code-aware result expansion, but search results must
show graph readiness and evidence caveats.

#### `evidence.refresh`

Should use `GraphSnapshotService` and binding revalidation slices. It should
not implement its own graph lifecycle.

## HTTP / Gateway API Shape

The canonical local contract is `ToolRequest`, but the gateway may expose
stable HTTP routes for dashboard and local API clients.

Proposed routes:

| Route | Method | Maps to | Notes |
|---|---|---|---|
| `/api/v1/code/locate` | `POST` | `code.locate` | Deterministic locator query. Read-only in V1. |
| `/api/v1/graph/status` | `GET` or `POST` | `graph.status` | Read readiness for workspace/ref/snapshot. |
| `/api/v1/graph/refresh` | `POST` | `graph.refresh_snapshot` | Manual mutating graph refresh. |
| `/api/v1/bindings/create` | `POST` | `binding.create` | Optional HTTP wrapper if dashboard needs direct binding. |
| `/api/v1/bindings/inspect` | `POST` | `binding.inspect` | Optional HTTP wrapper for binding currentness. |

Implementation may route these through a generic daemon ToolRequest endpoint
instead. If a generic endpoint exists, these route names remain product-level
aliases and documentation targets.

HTTP responses must preserve the same typed statuses as ToolResponse. Do not
return empty `202 Accepted` for deferred or unsupported graph behavior.

## MCP Tool Surface

MCP should expose only daemon-backed tools.

Proposed additions after bot commands exist:

| MCP tool | ToolRequest command | Notes |
|---|---|---|
| `bicameral.code.locate` | `code.locate` | Deterministic locate. MCP may pass LLM/user hints, but bot verifies. |
| `bicameral.graph.status` | `graph.status` | Read-only readiness check. |
| `bicameral.graph.refresh` | `graph.refresh_snapshot` | Manual refresh only; requires daemon capability. |

MCP must not:

- run local code locator fallback;
- inspect git directly for graph authority;
- rebuild graph slices;
- claim no conflict / safe to proceed;
- accept binding evidence locally.

## Readiness And Evidence State Mapping

### User-Facing Graph Readiness

| State | Meaning | User copy direction |
|---|---|---|
| `ready` | Graph/slice covers the requested target. | "Code graph ready for commit X." |
| `indexing` | Job is running. | "Indexing code graph." |
| `stale` | Graph exists for an older target or stale index version. | "Graph is stale; refresh latest." |
| `failed` | Refresh/index failed. | "Refresh failed; continue source-only or retry." |
| `not_configured` | Workspace/provider not configured. | "Graph unavailable; configure workspace or continue source-only." |

### Locator Evidence State

| State | Meaning | May become accepted binding? |
|---|---|---|
| `verified` | Exact proof over sufficient scope. | Yes, after governance acceptance. |
| `not_found` | Exact inspected absence over sufficient scope. | No, but useful for currentness/review. |
| `unknown_not_indexed` | Required scope not indexed. | No. |
| `unknown_stale` | Required scope stale. | No. |
| `ambiguous` | Multiple unresolved candidates. | No, user must choose/narrow. |
| `unsupported` | Language/file/pattern unsupported. | No, source-only or hosted path. |
| `approximate_candidate` | Ranked but not exactly verified. | No, unless later exact validation succeeds. |

## Ranking Rules

`CodeLocator` ranking must be deterministic and explainable.

Suggested confidence mapping:

| Signal | Confidence |
|---|---|
| exact path exists at snapshot | high |
| exact symbol occurrence verified | high |
| selected locator candidate revalidated exactly | high |
| prior accepted binding target overlaps current query | high/medium |
| changed file + lexical match | medium |
| import/call/reference neighborhood match | medium |
| source evidence term overlap only | low/medium |
| approximate provider output without exact proof | low or unknown |
| unsupported/stale/unindexed graph | unknown |

Confidence is not authority. Only `evidence_state = verified` plus governance
acceptance can materialize accepted binding evidence.

## Data Flow Examples

### Example: User Locates Code For A Decision

Input:

```json
{
  "decision_id": "decision-123",
  "query": "GitHub device flow for provider auth",
  "ref_name": "HEAD",
  "scope_kind": "query"
}
```

Runtime:

```text
ToolRequest code.locate
-> replay Decision/source evidence
-> resolve HEAD to commit a77b83a
-> check GraphSnapshot readiness
-> build/select query GraphSlice if available
-> run deterministic path/symbol/lexical/neighborhood retrieval
-> return ranked candidates
```

Output excerpt:

```json
{
  "status": "found",
  "graph_readiness": "ready",
  "candidates": [
    {
      "target": "crates/bicameral-gateway/src/github_device_flow.rs",
      "target_type": "file_path",
      "file": "crates/bicameral-gateway/src/github_device_flow.rs",
      "evidence_state": "verified",
      "confidence": "high",
      "reasons": [
        {"code": "exact_path", "detail": "File exists at snapshot a77b83a"},
        {"code": "lexical_match", "detail": "Matched github/device/provider/auth terms"}
      ],
      "graph_snapshot_id": "gs1:...",
      "validated_sha": "a77b83a..."
    }
  ],
  "next_actions": ["choose_candidate"]
}
```

### Example: User Creates Binding From Candidate

```text
user chooses candidate
-> binding.create with target and expected graph_snapshot_id
-> daemon verifies target still matches snapshot
-> governance accepts materialization
-> BindingEvidence stores graph_snapshot_id and validated_sha
```

If snapshot moved:

```json
{
  "status": "rejected",
  "error": "snapshot_mismatch",
  "next_actions": ["refresh_latest", "rerun_code_locate"]
}
```

### Example: Fast AI Commits During Refresh

```text
PR base A
PR head C10 at refresh start
indexing C10 starts
PR head moves to C12
C10 job completes
snapshot C10 is stored
current target is C12
graph status is stale relative to target
```

User sees:

```json
{
  "status": "stale",
  "target": {
    "resolved_sha": "C10",
    "current_head_sha": "C12",
    "is_current_for_target": false
  },
  "next_actions": ["refresh_latest", "continue_source_only"]
}
```

## Persistence Model

### Durable Governance State

- accepted Decisions;
- accepted SourceEvidence;
- accepted BindingEvidence;
- future accepted RevalidationOutcome, if governed.

### Graph Cache State

- GraphSnapshot metadata;
- GraphSlice metadata;
- index job records;
- stale/invalidation markers;
- retained BindingAnchor metadata.

Graph cache state is recomputable and not governance authority.

### Derived Read-Model State

- LocatorResult history, if stored;
- CodeSourceCurrentness;
- GraphReadinessView;
- dashboard display sections;
- search/preflight caveats.

Derived state can be rebuilt or corrected without rewriting accepted evidence.

## Security And Authority Boundaries

- Graph refresh must stay inside configured workspace roots.
- Caller-provided paths must be normalized and rejected if they escape the
  workspace.
- MCP/dashboard cannot supply graph cache paths.
- Missing local refs must not trigger network fetch unless a future policy
  explicitly allows it.
- Dirty worktree evidence must not become commit-anchored verified evidence
  unless a future dirty-snapshot mode is explicitly designed.
- LLM-generated hints must be stored, if at all, as caller hints, not verified
  evidence.
- `not_found` requires sufficient exact inspection. Uncovered graph scope is
  `unknown_not_indexed`, not `not_found`.
- `ready` graph state does not imply compliance or implementation correctness.

## Implementation Sequencing

### Stage 0: Spec And Type Alignment

- Add API structs for `CodeLocateParams`, `LocatorResult`,
  `GraphRefreshParams`, and `GraphRefreshResult`.
- Add command variants for `code.locate`, `graph.status`, and
  `graph.refresh_snapshot`.
- Add fixtures for ready, stale, not configured, ambiguous, unsupported, and
  approximate candidate outputs.

### Stage 1: Manual Graph Refresh

- Implement `graph.status` read-only readiness.
- Implement `graph.refresh_snapshot` manual trigger.
- Use existing `SnapshotResolver` and `FileInspector` first.
- Persist minimal graph job/cache metadata only as needed.
- Return typed stale/superseded behavior when target moved.

### Stage 2: Deterministic CodeLocator

- Implement exact path lookup.
- Implement lexical file search.
- Implement symbol lookup for supported languages.
- Implement prior binding anchor lookup.
- Implement changed-file/diff scope when base/head are available.
- Return deterministic reasons and confidence.

### Stage 3: Binding Integration

- Let `binding.create` consume selected locator candidates.
- Enforce snapshot mismatch checks.
- Materialize only verified candidates through governance.
- Let `binding.inspect` use revalidation slices/currentness.

### Stage 4: Surface Integration

- Add dashboard code-locate UI: ranked targets, reasons, graph readiness, next
  actions.
- Add MCP tools only after daemon capability advertises commands.
- Add `/api/v1/code/locate`, `/api/v1/graph/status`, and
  `/api/v1/graph/refresh` if direct HTTP routes are desired.

### Stage 5: PR-Aware Workflow

- Resolve base/head/merge-base for PRs when metadata is available.
- Build comparison slices over latest head only.
- Debounce/coalesce rapid moving target changes.
- Mark completed old-head jobs as stale when current head moved.

### Stage 6: Automatic Triggers

- Add daemon watcher/git hook/PR webhook triggers only after manual refresh and
  code locate are stable.
- Reuse the same `GraphSnapshotService` and scheduler contract.
- Keep automatic work bounded by policy and cost limits.

## Validation Plan

### Unit Tests

- deterministic query canonicalization;
- exact path lookup;
- exact symbol lookup;
- lexical ranking;
- confidence/reason mapping;
- stale graph mapping;
- unsupported language mapping;
- ambiguous candidate mapping.

### Runtime Tests

- `code.locate` returns verified path candidate with ready graph;
- `code.locate` returns approximate candidates when graph unavailable;
- `graph.refresh_snapshot` resolves HEAD and returns snapshot identity;
- moving target changes during refresh produce stale/superseded result;
- `binding.create` rejects snapshot mismatch;
- `binding.inspect` returns current/content_changed/target_missing without
  mutating signoff or compliance.

### Conformance Tests

- MCP cannot call code locator before daemon capability advertises it;
- MCP maps `bicameral.code.locate` to exactly one ToolRequest;
- MCP renders daemon graph readiness without local fallback;
- dashboard/API responses never say safe/no conflict/compliant from locator
  output;
- LLM/caller hints cannot set `GraphEvidenceState::verified`.

## Open Questions

- Should `code.locate` be read-only in V1 even when a small bounded slice could
  be built cheaply?
- Should direct HTTP routes be first-class, or should dashboard use a generic
  ToolRequest endpoint?
- Should `GraphRefreshResult` expose internal job IDs in V1, or only collapsed
  readiness?
- How much PR metadata should local-only mode try to infer from git remotes
  without network fetch?
- What is the first supported language/symbol provider beyond existing Rust
  resolver behavior?
- Should locator result history be persisted as derived read-model state or
  remain transient?

## Decision Record

For V1 implementation:

- `CodeLocator` is deterministic and does not call an LLM.
- LLM/user/MCP/dashboard hints are advisory inputs only.
- Manual graph refresh is the first mutating trigger.
- Graph snapshots are commit-addressed, but refresh targets are moving
  PR/branch/workspace heads.
- The scheduler should coalesce rapid target movement and avoid snapshotting
  every commit.
- Graph slices are task-specific subsets, not combinations of snapshots.
- Code locator output is ranked targets plus evidence/reasons/readiness, not a
  graph visualization.
- Graph readiness/currentness does not imply compliance, signoff, enforcement,
  merge safety, or implementation correctness.
