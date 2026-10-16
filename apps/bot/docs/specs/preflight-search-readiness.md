# Code Evidence Preflight Search Strategy And Readiness Semantics

**Date:** 2026-06-06
**Status:** proposed
**Issue:** https://github.com/BicameralAI/bicameral-bot/issues/137
**Related:** ADR-0011, ADR-0019, ADR-0021;
`graphslice-planning-sufficiency.md`;
`alpha-recall-multi-signal-fusion.md`;
`../releases/alpha/desync-ux-hitl-readiness.md`; issues #117, #134, #140

## Problem

The Code Evidence/Compliance `preflight.run` profile must help agents find relevant Decisions before code changes
begin without pretending local Bicameral can see every graph, branch, active PR,
team worktree, or hosted Conflict Oracle result.

ADR-0019 defines Code Evidence preflight readiness. The GraphSlice sufficiency RFQ
defines when scoped graph facts are provable. This RFQ specializes those
contracts into the deterministic search order and progressive readiness states
that implementation issues should use.

The goal is not to make this Code Evidence profile globally complete. The goal is to make
preflight useful, deterministic, and honest about its proof boundary so Devin
can implement local source, diff, binding, and graph-backed retrieval without
reintroducing MCP-owned authority or hidden CodeGenome-style telemetry. This
specification does not define SMR alpha tracked-spec recall: that core result
requires no graph readiness, sufficiency proof, or code-derived absence/all-clear
claim (ADR-0028).

## Decision

`preflight.run` is a bot-owned `ToolRequest` query. It must run a progressive,
deterministic pipeline:

```text
request
  -> snapshot/scope resolution
  -> source-only retrieval
  -> diff/path/symbol extraction
  -> GraphScopePlan for diff preflight
  -> GraphSlice lookup or expansion
  -> binding and graph overlap
  -> deduplication
  -> deterministic ordering
  -> top-level readiness plus per-result evidence state
```

Preflight may return useful source-only and partial results at any stage. Those
results are advisory until verified graph or accepted binding evidence supports
a grounded claim for the requested scope.

Preflight must not mutate signoff, compliance, binding, review, or conflict
state. It returns query output and readiness facts only.

## Search Pipeline

### 1. Snapshot And Scope Resolution

The daemon first resolves the requested preflight scope into local snapshots and
unknown visibility.

Locally resolvable inputs may include:

- current `HEAD`;
- authoritative base when present locally;
- feature branch `HEAD` when present locally;
- merge-base when both refs are locally resolvable;
- explicitly requested local worktree branches when present and resolvable.

Unresolved or unavailable inputs become top-level readiness facts:

- missing base ref;
- missing feature ref;
- dirty files without an explicit ephemeral dirty slice;
- active PR or team-wide branch visibility;
- cross-repo scope;
- hosted Conflict Oracle scope;
- unavailable remote history.

Unresolved visibility must be reported as `unknown_scope` or
`hosted_required`, not as a clean local pass.

### 2. Source-Only Retrieval

Source-only retrieval runs even when graph evidence is missing, stale, or not
indexed.

Allowed source-only signals:

- Decision title, body, source-evidence, and reviewed source text;
- candidate text and source spans;
- normalized tags, feature hints, scope hints, and path hints;
- deterministic query terms from the request;
- deterministic terms extracted from diff text or planned file paths.

Source-only matches may produce `relevant_decisions` and
`relevant_candidates`, but their compliance state remains `ungrounded` unless
durable binding or graph-backed compliance evidence exists.

### 3. Diff, Path, And Symbol Extraction

When the caller provides a diff, planned edit, path set, or base/head refs, the
daemon extracts deterministic local inputs:

- changed files;
- changed ranges or hunks when available;
- import/export names;
- declared symbols;
- referenced symbols;
- package, module, route, workflow, or deploy-surface names when a supported
  parser can extract them deterministically.

LLM-proposed files, symbols, or rationales are recall hints only. They may seed
source retrieval or graph slice expansion after canonicalization, but they are
not graph evidence and must not suppress readiness warnings.

### 4. Graph Scope Planning And Lookup

Graph-backed preflight uses:

- `TaskIntent.intent_kind = diff_preflight`;
- `GraphScopePlan.scope_kind = diff`;
- requested local snapshots from scope resolution;
- changed files, ranges, symbols, and deterministic scope hints;
- required local import/export, binding-overlap, exact-symbol, and
  caller/callee neighborhoods.

The daemon selects, builds, or expands `GraphSlice`s for that plan. The slice
is sufficient only when the GraphSlice sufficiency RFQ's `diff_preflight`
predicate covers the required local scope.

If the required local graph is unavailable, preflight may still return
source-only and diff-scoped context, but graph-backed absence, compliance, or
no-conflict claims are not permitted.

### 5. Binding And Graph Overlap

Binding-aware relevance may use accepted bot-owned `BindingEvidence` and
snapshot-scoped graph facts.

Allowed graph-backed signals:

- accepted Decision-to-code bindings whose target overlaps changed files,
  ranges, symbols, or graph neighborhoods;
- exact `SymbolOccurrence` overlap;
- import/export overlap;
- local caller/callee neighborhood overlap included in the sufficient slice;
- accepted compliance or binding evidence whose snapshot is compatible with
  the requested preflight scope.

MCP-local telemetry, previous tool-call logs, caller LLM output, and
probabilistic continuity guesses are not relevance authority. They may become
future observability or advisory candidates only when explicitly modeled
outside this RFQ.

### 6. Deduplication

Deduplication must be stable for fixture inputs:

- a Decision appears once even if multiple signals match;
- a promoted candidate does not duplicate its materialized Decision;
- source-only and graph-backed reasons may both appear on one result row;
- duplicate bindings to the same target are represented as evidence reasons,
  not duplicated Decision rows;
- approximate candidates remain separate from verified Decision rows.

### 7. Deterministic Ordering

Ordering must be deterministic and fixture-testable. Implementations may tune
weights, but the ordering contract must preserve this precedence:

1. rows that require immediate readiness or reviewer attention for the
   requested scope;
2. rows with direct accepted binding overlap on changed files, ranges, or
   symbols;
3. rows with exact graph symbol or neighborhood overlap;
4. rows with diff path, source evidence, title, or source-text matches;
5. candidates before unrelated low-confidence approximate matches only when the
   candidate is closer to the request;
6. stable tie-break by canonical Decision id, candidate id, then title.

`hosted_required` may raise the top-level readiness attention, but it must not
invent relevance for a Decision row that did not otherwise match.

The alpha recall-quality boundary in
[Alpha Recall Multi-Signal Fusion And Recall-Influence Boundary](alpha-recall-multi-signal-fusion.md)
permits CodeGenome-style overlay signals, semantic/textual similarity, and
ownership overlap to enrich ranking and slice planning after a bot-owned
recall-admissibility gate. Those enriched signals do not change the authority
rules in this RFQ: verified, not-found, no-conflict, compliance, signoff, and
BindingEvidence claims still require deterministic proof and governed command
acceptance.

## Progressive Readiness Modes

Preflight returns a top-level readiness state for the requested query. These
states describe the view/query boundary, not durable Decision lifecycle state.

| Mode | Use when | Allowed claim |
|---|---|---|
| `source_only` | Source or text retrieval ran, but no graph or accepted binding evidence supports the query. | Useful Decision context exists; compliance and code grounding are ungrounded. |
| `diff_scoped` | Diff/path/symbol inputs were extracted and local reasoning is bounded to changed files or planned edits. | Results are useful for local planning within the diff scope. |
| `partially_grounded` | Some rows have verified graph or accepted binding overlap, but other required local neighborhoods are missing, stale, unsupported, or not indexed. | Grounded claims are allowed only for covered rows and covered scope. |
| `grounded_local` | All requested local snapshots and graph slices needed for the preflight task are sufficient. | Known local graph-backed relevance and no-known-local-conflict claims are limited to resolved local snapshots. |
| `hosted_required` | The requested or implied scope requires active PRs, team branches, cross-repo graph state, hosted Conflict Oracle behavior, or unavailable remote history. | Local output is incomplete; hosted/team service is required for the broader scope. |
| `stale` | Available graph, binding, file hash, ref, parser, resolver, or index-version evidence does not match the requested snapshot/scope. | Existing graph evidence must be refreshed, rebuilt, or treated as advisory. |
| `not_indexed` | Required local files, symbols, edge kinds, or neighborhoods are not indexed and no sufficient expansion has completed. | Source-only or partial context may be returned; absence and compliance claims are not allowed for uncovered scope. |

Implementations may expose finer `GraphReadiness` or `GraphEvidenceState`
values internally, such as `unknown_stale`, `unknown_not_indexed`,
`unsupported`, `ambiguous`, `approximate_candidate`, or `failed`. The top-level
preflight mode should remain one of the progressive modes above unless a later
ADR expands this contract.

## Partial Graph Sufficiency

A partial graph slice is enough for:

- source-plus-diff planning;
- ranking Decisions that overlap covered changed files or exact symbols;
- explaining which local neighborhoods were inspected;
- returning verified binding or symbol overlap for the covered scope;
- asking the daemon to expand a local slice when source is available.

A partial graph slice is not enough for:

- repository-wide absence claims;
- global, team-wide, or active-PR no-conflict claims;
- grounded compliance for uncovered Decisions or targets;
- claiming an uncovered caller/callee neighborhood has no relevant Decisions;
- materializing `BindingEvidence`;
- resolving Decision signoff, review, compliance, or conflict state.

`not_found`, `no conflict`, `safe`, `verified`, and grounded compliance require
a sufficient proof for the exact requested task and scope.

## Top-Level Readiness Versus Per-Result Evidence

Top-level readiness belongs on the response and applies to the requested
preflight query.

Required top-level fields:

- `mode`;
- `snapshot_scope`;
- `snapshot_resolution`;
- `unknown_scope`;
- `warnings`;
- `required_actor`;
- `allowed_next_actions`.

Per-result evidence belongs on a Decision or candidate row only when the state
is specific to that result:

- accepted binding overlap;
- exact symbol overlap;
- stale binding evidence for that Decision;
- ambiguous occurrence for that Decision;
- unsupported target for that Decision;
- approximate-only relevance for that candidate;
- snapshot mismatch between that Decision's accepted evidence and the requested
  preflight scope.

General graph indexing, missing refs, dirty workspace scope, and hosted/team
visibility gaps are view-level readiness facts. They must not be copied into
every Decision as durable lifecycle state.

## Active Contradiction Context

Preflight must compose Decision signoff state with active contradiction findings
from ADR-0011. Collision context is not a Decision lifecycle state.

For each relevant Decision, preflight should attach contradiction context when
replay contains an active `ContradictionReport` (`status = open | acknowledged`)
whose related constraints map to that Decision.

Required behavior:

- keep the Decision's signoff state unchanged;
- include `contradiction_report_id`, finding status, related
  Decision/candidate/constraint refs, evidence refs when available, and reason
  code `active_contradiction`;
- dedupe contradiction context by `ContradictionReport.id` when multiple recall
  routes find the same Decision;
- make repeated preflight calls side-effect free and deterministic for a fixed
  event-store replay state, search index version, graph snapshot, and request.

Allowed wording:

- "Approved Decision with active collision context."
- "This Decision remains approved, but contradicting evidence exists in the
  searched scope."

Disallowed behavior:

- emitting `RaiseContradiction`, `TransitionFinding`, signoff, compliance, or
  review events from preflight;
- treating active contradiction context as proof the Decision is no longer
  approved;
- claiming "no conflict found" when an active contradiction report exists in the
  searched scope.

## Local No-Conflict Reporting

Local preflight may report only known-evidence results.

Allowed wording:

- "no known local conflict in resolved snapshots";
- "no matching accepted binding in the covered diff slice";
- "no relevant Decision found in source-only retrieval";
- "hosted/team scope not checked";
- "uncovered graph neighborhoods remain unknown."

Disallowed wording:

- "no conflict" without scope;
- "safe";
- "globally consistent";
- "all branches clear";
- "team state clean";
- "no binding exists" when the relevant binding scope was not sufficiently
  inspected.

When `unknown_scope` is non-empty, the response must include a warning that
local no-conflict output is limited to resolved local snapshots and covered
graph scope.

## Response Shape

The exact Rust structs may evolve, but the first implementation should preserve
this shape:

```json
{
  "readiness": {
    "mode": "partially_grounded",
    "snapshot_scope": [
      {
        "name": "HEAD",
        "validated_sha": "abc123",
        "graph_snapshot_id": "gs-head"
      }
    ],
    "snapshot_resolution": {
      "resolved": ["HEAD"],
      "unresolved": ["origin/dev"]
    },
    "unknown_scope": [
      {
        "scope": "team_active_prs",
        "mode": "hosted_required",
        "detail": "Local preflight cannot inspect team active PR branches."
      }
    ],
    "warnings": [
      "Local no-conflict output is limited to resolved local snapshots."
    ],
    "required_actor": "hosted/team service",
    "allowed_next_actions": [
      "fetch_base_ref",
      "expand_slice",
      "request_hosted_check",
      "proceed_source_only"
    ]
  },
  "relevant_decisions": [
    {
      "decision_id": "dec_123",
      "title": "Use snapshot-scoped graph evidence",
      "relevance_reasons": [
        "accepted_binding_overlap",
        "diff_symbol_overlap",
        "source_text_match"
      ],
      "evidence_state": "verified",
      "compliance_state": "grounded_for_covered_scope"
    }
  ],
  "relevant_candidates": [
    {
      "candidate_id": "cand_456",
      "title": "Possible parser boundary",
      "relevance_reasons": ["source_text_match"],
      "evidence_state": "approximate_candidate",
      "compliance_state": "ungrounded"
    }
  ]
}
```

## Downstream Implementation Handoff

The following implementation work should consume this RFQ:

- #140 implements progressive `preflight.run` over source, diff, graph, and
  binding overlap using this search order and readiness model.
- #134 aligns `search.query` with governed evidence retrieval by reusing the
  source, binding, graph, deduplication, and no-silent-empty-result boundaries.
- #117 extends the bot-owned ToolRequest preflight response with readiness,
  snapshot scope, warnings, and per-result evidence reasons.
- Future MCP work maps MCP surfaces to canonical bot `ToolRequest` responses
  without restoring MCP-local graph authority.

## Required Tests For Implementations

Implementation issues should add fixture coverage for:

- source-only preflight returning useful Decision context with ungrounded
  compliance;
- diff-scoped preflight with deterministic path and symbol extraction;
- accepted binding overlap outranking source-only matches;
- exact graph symbol overlap and local caller/callee overlap;
- duplicate suppression across source, candidate, binding, and graph signals;
- stable ordering under shuffled input fixtures;
- `partially_grounded` when some rows are verified and others are source-only
  or not indexed;
- `grounded_local` limited to resolved local snapshots;
- `hosted_required` for active PR, team branch, cross-repo, or Conflict Oracle
  scope;
- `stale` on ref movement, file hash mismatch, parser/resolver/index drift, or
  dirty workspace mismatch;
- `not_indexed` for uncovered local files, symbols, edge kinds, or
  neighborhoods;
- top-level warnings for unknown scope;
- per-result evidence state only for row-specific grounding or binding
  problems;
- local no-known-conflict wording that never implies global/team consistency.

## Non-Goals

- Do not implement `preflight.run` here.
- Do not define final API struct names beyond the required response shape.
- Do not add hosted/team graph visibility.
- Do not create durable governance events from preflight output.
- Do not make source-only, LLM, MCP telemetry, or probabilistic continuity
  evidence equivalent to verified graph evidence.
