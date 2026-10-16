# GraphSlice Planning And Sufficiency Semantics

**Date:** 2026-06-06
**Status:** proposed
**Issue:** https://github.com/BicameralAI/bicameral-bot/issues/164
**Related:** ADR-0010, ADR-0017, ADR-0019, ADR-0021;
`../releases/alpha/desync-ux-hitl-readiness.md`;
`preflight-search-readiness.md`; issues #134, #136, #137, #138, #140, #141,
#154, #163

## Problem

ADR-0021 defines `GraphStore`, `GraphSnapshot`, and `GraphSlice` lifecycle
identity. It intentionally leaves one product-facing question open: when is a
scoped graph slice sufficient for the task that asked for graph evidence?

Without a sufficiency contract, local graph-backed behavior can overclaim in
several unsafe ways:

- a diff-scoped slice can be treated as repository-wide evidence;
- an uncovered caller/callee neighborhood can be reported as `not_found`;
- stale evidence can be repaired without proving the same target still exists;
- local preflight can imply no conflict across branches or worktrees it cannot
  see;
- LLM-proposed scope hints can be mistaken for verified graph authority.

This RFQ defines the proof boundary for local graph slices. Codegraph is a
deterministic prover only for the scope it can identify, inspect, and prove
complete. Everything outside that proof remains `unknown_not_indexed`,
`unknown_stale`, `unsupported`, `ambiguous`, `approximate_candidate`, or
`hosted_required`.

## Decision

Every graph-backed binding, preflight, compliance, remediation, revalidation,
search, or CodeIdentity query must start from a `TaskIntent`. The daemon maps
that intent into a deterministic `GraphScopePlan`, builds or selects one or
more `GraphSlice`s, and returns a `ScopeSufficiencyProof` before any graph
claim may be treated as verified for that task.

The required flow is:

```text
TaskIntent
  -> SnapshotResolution
  -> GraphScopePlan
  -> GraphSlice build/read/expand
  -> ScopeSufficiencyProof
  -> GraphEvidenceState / readiness output
```

`GraphSlice` remains a scoped subgraph, not a contiguous source chunk. It may
cover non-contiguous files, ranges, symbols, generated-source relationships,
and graph neighborhoods. A slice is sufficient only for a specific
`TaskIntent`, requested snapshot, and `scope_kind`.

No result may become `verified`, `not_found`, `no conflict`, `safe`, or
grounded compliance unless the returned sufficiency proof covers the task's
required scope. Partial evidence may improve recall and explain likely next
steps, but it remains advisory.

Alpha UX and HITL behavior for insufficient or unavailable proofs is defined in
`../releases/alpha/desync-ux-hitl-readiness.md`. The alpha release may expose
insufficient proof as `review_needed`, `readiness_blocked`, or `source_only`
product state, but it must not weaken the sufficiency proof required for
verified graph claims.

## Definitions

### TaskIntent

`TaskIntent` is the daemon's canonical description of why graph evidence is
being requested. It is derived from a `ToolRequest`, not directly from a
surface-specific tool shape.

Required fields:

- `intent_kind`;
- requested snapshot inputs, such as `commit_sha`, `ref_name`, base/head refs,
  or merge-base request;
- requested targets, such as files, ranges, symbols, decisions, bindings, diff
  hunks, or query terms;
- requested visibility, such as local current worktree, local base/head,
  additional local worktrees, hosted/team scope, or cross-repo scope;
- evidence purpose, such as binding materialization, preflight warning,
  compliance read, remediation proposal, revalidation, or search result.

Allowed initial `intent_kind` values:

| `intent_kind` | Purpose |
|---|---|
| `binding_create` | Validate file/range/symbol hints before materializing `BindingEvidence`. |
| `binding_inspect` | Inspect accepted binding evidence and optionally derive currentness. |
| `diff_preflight` | Find relevant Decisions/candidates for a local diff or planned edit. |
| `file_review` | Review graph evidence for explicit files or ranges. |
| `impact_query` | Ask what code depends on or is affected by a file, symbol, or decision. |
| `search_query` | Retrieve governed evidence with provenance and graph readiness. |
| `remediation` | Repair stale, moved, missing, or incomplete graph evidence when proof permits. |
| `revalidation` | Re-check accepted evidence against a later locally resolvable snapshot. |
| `code_identity_probe` | Explore deterministic identity continuity for future CodeIdentity work. |

### GraphScopePlan

`GraphScopePlan` is the deterministic scope identity produced from a
`TaskIntent` after snapshot resolution. It defines the graph facts required to
answer the task and the facts that are intentionally out of local scope.

Required fields:

- `scope_plan_id`;
- `intent_kind`;
- `scope_kind`;
- resolved `graph_snapshot_id`, `repo_id`, `validated_sha`, and
  `index_version` when locally resolvable;
- `scope_fingerprint`;
- required files, ranges, symbols, edges, and neighborhoods;
- required generated-source, macro, config, test, or deploy-surface expansion
  rules;
- `unknown_scope` for requested or implied visibility that is not locally
  available;
- caller-supplied hints after canonicalization, marked as non-authoritative.

`scope_plan_id` and `scope_fingerprint` must be deterministic for fixture
inputs. Implementations may choose the exact hash format, but the fingerprint
must include:

- intent kind and scope kind;
- resolved snapshot identity;
- sorted normalized target identities;
- requested neighborhood depths and edge kinds;
- index/resolver version;
- canonicalized hint identities, if hints affect expansion.

### ScopeKind

ADR-0021 defines initial `GraphSlice.scope_kind` values. This RFQ refines them
for task planning:

| `scope_kind` | Typical intents | Required slice shape |
|---|---|---|
| `binding` | `binding_create` | Target file/range and exact symbol occurrence evidence needed to validate the proposed binding. |
| `diff` | `diff_preflight` | Changed files/ranges plus local import/export, symbol, and decision-binding neighborhoods needed for preflight. |
| `file_set` | `file_review` | Explicit file/range set plus requested intra-file and cross-file edges. |
| `symbol_set` | `binding_create`, `impact_query`, `revalidation`, `code_identity_probe` | Explicit symbols plus occurrence, definition, reference, and identity-continuity evidence. |
| `query` | `search_query`, `impact_query` | Neighborhoods reached by deterministic query terms, binding evidence, source matches, or exact symbol overlaps. |
| `binding_revalidation` | `binding_inspect`, `revalidation` | Accepted binding anchor plus current-snapshot target, lineage, and mismatch evidence. |
| `remediation` | `remediation` | Old anchor, candidate repaired target, and proof that the repair covers the required identity and scope. |

ADR-0021's previous `file_set`, `symbol_set`, `query`,
`binding_revalidation`, and `remediation` values remain valid. `binding` is a
new explicit scope kind for first-pass binding materialization, and `diff`
continues to cover progressive preflight.

## Sufficiency Proof

`ScopeSufficiencyProof` is task-specific evidence that a slice covers the
facts required by the `GraphScopePlan`.

Required fields:

- `proof_state`: `sufficient | insufficient | stale | failed |
  hosted_required`;
- `scope_plan_id`;
- `graph_slice_id`, when a local slice exists;
- inspected snapshot identity;
- covered files, ranges, symbols, and edge kinds;
- unsupported files, languages, generated artifacts, macros, or resolver
  behaviors;
- uncovered requirements and why they are missing;
- stale inputs and invalidation triggers, if any;
- expansion action, if a deterministic local expansion is possible;
- required actor: daemon, workspace member, reviewer, admin, or hosted/team
  service.

Only `proof_state = sufficient` permits verified positive or verified negative
graph claims for the task. `not_found` is allowed only when the sufficient
proof covers the exact target and required neighborhood where absence is being
claimed.

## Task-Specific Sufficiency Predicates

### Binding Create

A binding slice is sufficient when:

- the requested snapshot resolves to a local `validated_sha`;
- every requested file/range exists at that snapshot with matching content
  hash;
- every requested symbol target is resolved exactly, or the unsupported symbol
  target is reported as `unsupported`;
- ambiguous symbols are returned as `ambiguous`, not guessed;
- the slice covers the symbol occurrence and the file/range evidence needed to
  materialize the proposed binding.

If exact validation succeeds, the result may become
`GraphEvidenceState::verified` and later `BindingEvidence` if policy accepts
it. If the target is absent after a sufficient exact inspection, the result is
`not_found`. If the relevant file, parser, resolver, or neighborhood is
uncovered, the result is `unknown_not_indexed` or `unsupported`.

### Binding Inspect And Revalidation

A revalidation slice is sufficient when:

- the accepted `BindingAnchor` is loaded;
- the current requested snapshot resolves locally;
- old and current file hashes, ranges, symbols, structural hashes, or
  resolver-specific identities are compared with the required continuity rules;
- the slice can distinguish same target, moved target, deleted target,
  ambiguous target, and unsupported target for the binding type.

A successful revalidation is derived view state by default. It becomes durable
only through a later governed event accepted by policy or reviewer action.

### Diff Preflight

A diff slice is sufficient for local preflight when:

- the diff base and head snapshots requested by the caller resolve locally, or
  unresolved refs are reported in `unknown_scope`;
- changed files/ranges are inspected;
- source-only relevance still runs when graph evidence is missing;
- graph-backed relevance covers requested binding overlaps, exact symbol
  overlaps, imports/exports, and local caller/callee neighborhoods included in
  the plan;
- hosted/team, active PR, cross-repo, and unseen worktree scope is reported as
  `hosted_required` or unknown, not as a clean pass.

Preflight sufficiency is view-level. Missing base refs, not-indexed graph
neighborhoods, or hosted/team visibility gaps must surface in readiness output
rather than being copied into every Decision row.

### File Review

A file-review slice is sufficient when:

- all requested files/ranges are inspected at the resolved snapshot;
- unsupported generated or language-specific regions are identified;
- requested intra-file symbols and cross-file edges are either covered or
  returned as uncovered requirements;
- absence claims are limited to covered files/ranges and requested edge kinds.

### Impact Query

An impact query slice is sufficient only for the declared depth and edge kinds.
For example, a one-hop caller slice may support "known direct callers in this
snapshot" but not "all downstream impact."

Required plan fields include:

- edge kinds: imports, exports, calls, references, trait/interface
  implementations, type relationships, tests, config, or deploy surfaces;
- traversal depth or cutoff;
- language/resolver support;
- whether generated code, macros, test fixtures, or deploy manifests are in
  scope.

If a requested edge kind is unsupported or not indexed, the response must
preserve that state. It must not collapse to an empty impact set.

### Search Query

A query slice is sufficient for graph-backed search claims when:

- deterministic source, path, symbol, or binding-overlap signals used for
  ranking are declared;
- graph-backed results include snapshot identity and evidence state;
- binding-aware search uses accepted bot-owned `BindingEvidence`, not MCP-local
  telemetry;
- empty binding results are returned only when the slice proves the relevant
  binding scope was inspected.

Approximate semantic or LLM-ranked search results are
`approximate_candidate`. They may seed exact validation or slice expansion, but
they do not prove absence or compliance.

### Remediation

A remediation slice is sufficient for automatic repair only when:

- the stale or incomplete state is identified from a `BindingAnchor`,
  `GraphSnapshot`, or `GraphSlice` invalidation trigger;
- the old target and candidate repaired target are both inspected;
- deterministic continuity evidence proves the repaired target is the same
  governed target for the binding type;
- all required supporting neighborhoods are covered;
- no ambiguity remains.

If those conditions are not met, remediation may propose reviewer actions,
source-only fallback, hosted check, or slice expansion, but it must not
auto-repair or materialize governance state.

### CodeIdentity Probe

A CodeIdentity probe is exploratory unless it is backed by exact provider
checks over the requested scope. Probabilistic CodeGenome-style evidence may
increase recall, suggest candidate scopes, or reduce review burden, but it must
remain advisory or confidence-rated. It must not become
`GraphEvidenceState::verified` without deterministic proof from an accepted
provider over a sufficient scope.

## Slice Expansion And State Transitions

When a requested graph fact is outside the current slice, the daemon must choose
one of these transitions:

| Transition | Use when | Result state |
|---|---|---|
| `expand_slice` | The parent snapshot is valid and the missing files or neighborhoods can be indexed locally. | `indexing`, then retry proof. |
| `rebuild_snapshot` | The snapshot or index version is stale, corrupt, or missing enough payload that targeted expansion cannot prove the task. | `stale` or `failed`, then retry after rebuild. |
| `hosted_required` | The requested scope requires team active PRs, cross-repo graph state, hosted Conflict Oracle behavior, or unavailable remote history. | `hosted_required`. |
| `unknown_not_indexed` | Required local files or neighborhoods are not currently indexed and no expansion has completed. | `unknown_not_indexed`. |
| `unknown_stale` | Available graph data does not match the requested `validated_sha`, file hash, parser, resolver, or index version. | `unknown_stale`. |
| `unsupported` | The file type, language, macro/generated-code behavior, or edge kind is outside exact local support. | `unsupported`. |
| `ambiguous` | Multiple candidate occurrences remain after deterministic resolution. | `ambiguous`. |
| `failed` | Indexing, parsing, resolving, or cache read failed. | `failed`. |

Expansion must update the scope fingerprint or derive a new scope plan when it
changes the required scope. Retrying with a larger slice must not reuse a proof
from the smaller slice as if it covered the larger request.

## Completeness Inputs

`GraphSnapshot.completeness = full` requires a manifest or equivalent algorithm
showing that every supported parseable file at `validated_sha` was considered
with the declared `index_version`.

Minimum manifest inputs:

- repository identity and `validated_sha`;
- index version, parser versions, resolver versions, and schema version;
- file inventory from the resolved snapshot;
- supported and unsupported file classifications;
- per-file content/blob hashes;
- generated-code and ignored-file policy;
- successful parse/resolve status per supported file;
- failure records for failed files;
- edge-kind coverage summary.

`partial` snapshots and slices need a scoped coverage manifest instead:

- included files/ranges/symbols;
- included edge kinds and traversal depth;
- explicitly excluded or unsupported scope;
- stale and dirty markers;
- expansion history when the slice was grown.

Completeness is local only. A full local snapshot does not mean the daemon has
team-active-PR, cross-branch, cross-repo, or hosted conflict visibility.

## Dirty Worktrees, Ref Movement, And Hash Mismatch

Dirty worktree handling must be explicit per request:

- clean `validated_sha` evidence may be used for the clean snapshot only;
- if requested current files differ from the clean snapshot, affected files or
  neighborhoods return `unknown_stale` unless the daemon creates an explicit
  ephemeral dirty slice;
- an ephemeral dirty slice must include workspace identity and file hashes, and
  it must not produce durable `BindingEvidence` anchored to a commit SHA;
- surfaces should ask the user to commit, choose a clean snapshot, or accept
  source-only/advisory output when durable graph evidence is required.

Ref movement handling:

- branch names are snapshot-resolution inputs and provenance only;
- if a ref now resolves to a different SHA, the daemon resolves or builds a new
  snapshot for the new SHA;
- old snapshots remain addressable by original `validated_sha`;
- a ref movement is never proof that old accepted evidence is invalid.

File hash mismatch handling:

- if a cached file hash differs from the blob expected for `validated_sha`, the
  affected neighborhood is `unknown_stale`;
- targeted re-index may repair the neighborhood when source is available;
- until repair succeeds, the daemon must not serve the stale neighborhood as
  verified or absent.

## LLM Scope Hints

Caller-supplied LLM reasoning may propose candidate files, symbols, keywords,
or graph neighborhoods. These hints are recall inputs only.

Rules:

- hints must be canonicalized into deterministic target identities before they
  affect `scope_fingerprint`;
- raw LLM rationale is not graph evidence;
- hint-derived candidates are `approximate_candidate` until exact validation
  succeeds;
- hints may trigger `expand_slice`, but the expanded slice still needs a
  normal sufficiency proof;
- hints must not suppress unknown, stale, unsupported, or hosted-required
  states.

## Consumer Obligations

Binding:

- materializes `BindingEvidence` only from verified evidence with a sufficient
  binding proof;
- rejects snapshot mismatch and stale/dirty evidence explicitly.

Preflight:

- reports readiness and unknown scope at the view level;
- never treats uncovered local or hosted/team scope as no conflict.

Compliance:

- uses graph evidence only when policy accepts sufficient verified evidence for
  the requested scope;
- treats source-only or approximate results as ungrounded/advisory.

Remediation:

- auto-remediates only when deterministic proof covers the remediation scope;
- routes ambiguity, unsupported targets, and probabilistic continuity to human
  review or hosted checks.

UI and tools:

- show whether evidence is verified, stale, partial, unsupported, approximate,
  or hosted-required;
- distinguish "known absent in covered scope" from "not in this slice."

## Downstream Implementation Handoff

The following issues should consume this RFQ:

- #138: implement snapshot-scoped GraphStore binding verification using
  `binding` and `symbol_set` sufficiency predicates.
- #140: implement progressive preflight over source, diff, and graph scope
  using `diff` sufficiency and view-level unknown propagation. See
  [Preflight Search Strategy And Readiness Semantics](preflight-search-readiness.md)
  for the deterministic search order and readiness modes.
- #141: define graph remediation and revalidation repair paths using the
  remediation sufficiency predicate as the auto-repair gate. See
  [Graph Remediation And Revalidation Repair Paths](graph-remediation-revalidation.md).
- #134: align `search.query` with governed evidence retrieval using `query`
  sufficiency and no silent empty binding results.
- #154: evaluate CodeIdentity using deterministic sufficiency as the boundary
  between verified evidence and probabilistic advisory evidence.
- #163: formalize the code-grounding governance guideline with this RFQ as the
  GraphSlice proof boundary.

## Required Tests For Implementations

Implementation issues should add deterministic fixtures for:

- stable `scope_fingerprint` generation from sorted targets and canonicalized
  hints;
- diff slice sufficiency versus uncovered caller/callee neighborhoods;
- `not_found` only after sufficient exact inspection;
- `unknown_not_indexed` outside a slice;
- `unknown_stale` on dirty file, file hash mismatch, ref movement, and
  index-version drift;
- unsupported generated code, macro, language, and edge-kind handling;
- hosted-required propagation for team/cross-repo scope;
- remediation auto-repair only when deterministic continuity proof covers the
  required scope;
- approximate/LLM hints remaining advisory until exact validation succeeds.

## Non-Goals

- Do not implement GraphStore or GraphSlice behavior here.
- Do not add hosted/team graph visibility here.
- Do not make probabilistic CodeGenome-style evidence equivalent to verified
  graph evidence.
- Do not create durable governance facts from graph remediation alone.
- Do not define final UI copy or dashboard layout.
