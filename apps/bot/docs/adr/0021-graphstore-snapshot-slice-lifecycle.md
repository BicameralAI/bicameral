# ADR-0021: GraphStore Snapshot And Slice Lifecycle

**Date:** 2026-06-05
**Status:** proposed
**Level:** L1
**Issue:** https://github.com/BicameralAI/bicameral-bot/issues/136
**Related:** ADR-0010, ADR-0017, ADR-0019;
`../releases/alpha/desync-ux-hitl-readiness.md`;
`../specs/graphslice-planning-sufficiency.md`,
`../specs/graph-remediation-revalidation.md`; issues #137, #138, #140, #141,
#164

## Problem

ADR-0010 defines snapshot-scoped graph evidence. ADR-0017 binds verified
evidence to durable `BindingEvidence`. ADR-0019 uses graph readiness to keep
preflight honest about local versus hosted/team visibility.

Those decisions still leave one storage question open: how the bot-owned
`GraphStore` should cache, slice, invalidate, and retain local code graph data
without overclaiming completeness or losing the metadata needed to inspect old
binding evidence.

Issue #136 asks for an RFQ/design decision, not an implementation patch. The
decision must define:

- cache keys for graph data;
- the relationship between `GraphSnapshot` and `GraphSlice`;
- completeness states for full, partial, and scoped graph data;
- hot-memory and durable-cache lifecycle rules;
- stale invalidation triggers;
- metadata that must survive eviction for `BindingEvidence` inspection;
- branch-name semantics.

Without this contract, implementation work can drift in several unsafe ways:

- treating a partial or diff-scoped graph as complete;
- serving stale graph data after ref movement, parser changes, or dirty
  worktree edits;
- using branch names as durable identity even though branches move;
- evicting metadata required to explain or revalidate accepted
  `BindingEvidence`;
- conflating recomputable graph facts with durable governance facts.

## Decision

`GraphStore` is a bot-owned local cache/index subsystem. It is not a canonical
event-store artifact and does not write durable governance facts. It stores
recomputable graph facts and the minimum retained metadata needed to inspect
accepted evidence.

The first accepted design uses a hybrid storage posture:

- full snapshots are allowed and preferred when available;
- partial snapshots are allowed when every consumer can surface
  `unknown_not_indexed` for uncovered neighborhoods;
- scoped slices are allowed for diff, file, symbol, query, repair, and
  revalidation work;
- no local result may imply global/team completeness unless the relevant
  snapshots are actually locally resolvable and indexed.

The durable target is this GraphStore evidence contract. Alpha UX and HITL
behavior for incomplete local proof is defined in
`../releases/alpha/desync-ux-hitl-readiness.md`: incomplete graph scope must
surface as review, readiness, source-only, or hosted-required state rather than
verified graph evidence.

## Cache Identity

Every `GraphStore` cache entry is keyed by:

```text
(repo_id, workspace_id, validated_sha, index_version)
```

| Field | Meaning |
|---|---|
| `repo_id` | Stable repository identity for local graph scope. It may be derived from origin URL, repository root, or a configured repo identity, but it must distinguish separate repositories served by one daemon. |
| `workspace_id` | Worktree/workspace identity used for cache isolation, dirty-state tracking, and simultaneous local worktrees. |
| `validated_sha` | Durable commit anchor for graph facts. This is the identity used by binding, preflight, and inspection. |
| `index_version` | Composite graph/index version, including schema version, parser/tree-sitter grammar version, resolver version, and language-specific indexer versions. |

`ref_name` is not part of the cache key. Branch names are provenance and
snapshot-resolution inputs only.

`graph_snapshot_id` is derived from graph content identity:

```text
graph_snapshot_id = hash(repo_id, validated_sha, index_version)
```

The canonical form of `hash(repo_id, validated_sha, index_version)` is:

```text
graph_snapshot_id =
  "gs1:" + lowercase_hex(SHA-256(
    len(repo_id) || ":" || repo_id ||
    len(validated_sha) || ":" || lowercase_hex(validated_sha) ||
    len(index_version) || ":" || index_version
  ))
```

Inputs are UTF-8 strings. `validated_sha` must be normalized to lowercase hex
before hashing. Field order is exactly `repo_id`, `validated_sha`,
`index_version`. Length prefixes prevent separator ambiguity. The `gs1:`
prefix versions the ID derivation so a future ADR can define a replacement
without changing existing identifiers.

`workspace_id` is intentionally excluded from `graph_snapshot_id`. Two clean
worktrees at the same `validated_sha` with the same `index_version` describe
the same graph content. They may still have separate hot-cache entries because
their workspace dirtiness, open files, and local lifecycle differ.

## GraphSnapshot

A `GraphSnapshot` describes graph facts for one `validated_sha` and one
`index_version`.

Required identity:

- `graph_snapshot_id`;
- `repo_id`;
- `validated_sha`;
- `index_version`.

Required lifecycle metadata:

- `completeness`;
- `indexed_at`;
- `last_used_at`;
- stale/invalidation state;
- indexer and resolver version metadata.

Optional provenance:

- `ref_name`;
- `merge_base_sha`;
- `workspace_id`;
- `index_source`: `local_daemon | hosted | imported`.

`GraphSnapshot.completeness` has these states:

| State | Meaning | Consumer obligation |
|---|---|---|
| `full` | Every supported parseable file for `validated_sha` has been indexed with the declared `index_version`. | Consumers may answer snapshot-wide graph queries, while still returning `unsupported` for unsupported languages or file types. |
| `partial` | Some supported files or neighborhoods are indexed and others are not. | Consumers must answer only covered neighborhoods and return `unknown_not_indexed` for uncovered ones. |
| `indexing` | Index creation is in progress. | Consumers must report indexing/readiness state instead of serving verified graph evidence. |
| `failed` | Index creation or refresh failed. | Consumers must expose the failure and may fall back to source-only/advisory behavior when policy allows. |
| `stale` | Cached data no longer matches the requested snapshot or index version. | Consumers must rebuild, revalidate, or return `unknown_stale`; they must not serve stale data as verified. |

`full` means complete only for supported local graph capabilities. It does not
mean the local daemon can see hosted/team-only branches, private worktrees, or
cross-repo graph state.

## GraphSlice

A `GraphSlice` is a scoped graph subset derived from a `GraphSnapshot`. It is
used for targeted work where full repository indexing is unnecessary or
unavailable.

Required identity:

- `graph_slice_id`;
- `parent_graph_snapshot_id`;
- `repo_id`;
- `validated_sha`;
- `index_version`;
- `scope_kind`;
- `scope_fingerprint`.

Allowed `scope_kind` values:

- `binding`;
- `diff`;
- `file_set`;
- `symbol_set`;
- `query`;
- `binding_revalidation`;
- `remediation`.

Mapping to slice completeness:

| `scope_kind` | Required completeness |
|---|---|
| `binding` | `symbol_scoped` |
| `diff` | `diff_scoped` |
| `file_set` | `file_scoped` |
| `symbol_set` | `symbol_scoped` |
| `query` | `query_scoped` |
| `binding_revalidation` | `repair_scoped` |
| `remediation` | `repair_scoped` |

Allowed slice completeness values:

| State | Meaning |
|---|---|
| `diff_scoped` | The slice covers only files or symbol neighborhoods touched by a diff. |
| `file_scoped` | The slice covers an explicit file set. |
| `symbol_scoped` | The slice covers an explicit symbol set and the neighborhoods needed to validate those symbols. |
| `query_scoped` | The slice covers only the graph neighborhoods reached by a query. |
| `repair_scoped` | The slice covers a remediation/revalidation target and supporting evidence. |

A `GraphSlice` must never be labeled `full`. It inherits `validated_sha`,
`repo_id`, and `index_version` from its parent snapshot. If the parent snapshot
is stale, the slice is stale. If a slice is requested for a neighborhood outside
its scope, the answer is `unknown_not_indexed`, not `not_found`.

Slices may be materialized from a partial parent if the requested scope is
covered. They may also be used to expand a partial parent. The implementation
must preserve the difference between "not in this slice" and "verified absent
from the requested snapshot."

## Lifecycle

Graph data lives in two local cache tiers.

### Hot Memory

Hot memory is in-process daemon state: parsed ASTs, symbol maps, occurrence
indexes, adjacency lists, and active slice data.

Rules:

- hot entries are created by explicit indexing, lazy query expansion, binding
  validation, preflight, or remediation;
- every read or validation updates `last_used_at`;
- entries are evicted by least-recently-used order after configurable memory or
  entry-count limits;
- dirty entries must be flushed or discarded explicitly before shutdown;
- hot eviction must not delete retained `BindingAnchor` metadata.

Hot memory is an optimization. Restarting the daemon may discard hot graph data
without changing governance state.

### Durable Local Cache

The durable cache is local implementation storage: SQLite, a content-addressed
store, a cache directory, or another equivalent local store.

Rules:

- durable entries record `created_at`, `last_used_at`, `index_version`,
  completeness, and stale state;
- durable entries are evicted by least-recently-used order after configurable
  size limits;
- full graph indexes, parsed ASTs, complete occurrence lists, and slice bodies
  may be evicted;
- retained `BindingAnchor` metadata must survive automatic eviction while
  accepted `BindingEvidence` still references it;
- stale durable entries are marked stale before reuse and may be rebuilt lazily.

The durable cache must not be treated as a source of durable governance
authority. It can accelerate evidence inspection and revalidation, but accepted
state still comes from replayed governance events.

## Stale Invalidation

GraphStore must mark graph entries stale, failed, or unknown when the cached
data no longer supports verified evidence for the requested scope.

| Trigger | Detection | Required result |
|---|---|---|
| Ref movement | A provenance ref now resolves to a different SHA than the cached entry's `validated_sha`. | Resolve or build a new entry for the new SHA. Keep the old entry addressable by its original `validated_sha`. |
| Dirty worktree | Worktree files differ from the clean `validated_sha`. | Do not serve the clean snapshot as current for dirty files. Return `unknown_stale`, build an ephemeral dirty slice, or ask for a commit/snapshot choice. |
| Indexer version change | Schema, parser, grammar, or indexer version differs from `index_version`. | Mark stale and rebuild before verified graph claims. |
| Resolver version change | Symbol resolver behavior differs from `index_version`. | Mark stale and rebuild/revalidate before verified symbol claims. |
| File-hash mismatch | Cached file hash differs from the file/blob expected for `validated_sha`. | Mark affected neighborhoods `unknown_stale`; targeted re-index may repair them. |
| Parent snapshot stale | A slice parent is stale or unavailable. | Mark the slice stale; do not promote slice evidence as verified. |
| Cache corruption or missing payload | Retained identity exists but graph payload cannot be read. | Preserve `BindingAnchor`, return `unknown_not_indexed` or `failed`, and allow rebuild when source is available. |

Stale does not mean invalid governance history. It means the cache cannot make
fresh verified graph claims without rebuild or revalidation. Old accepted
`BindingEvidence` remains an immutable record of what was accepted at its
original `validated_sha`.

## BindingAnchor Retention

When accepted durable `BindingEvidence` references a `graph_snapshot_id`, the
daemon must retain a minimal metadata envelope even if the full graph index is
evicted. This retained envelope is `BindingAnchor`.

Required retained fields:

- `graph_snapshot_id`;
- `repo_id`;
- `validated_sha`;
- `index_version`;
- `ref_name`, when known, as provenance only;
- `merge_base_sha`, when relevant;
- bound file paths;
- per-bound-file content/blob hashes;
- bound ranges, when available;
- symbol names or qualified names, when available;
- per-bound-symbol structural hashes, when available;
- parser/resolver identity and version;
- evidence state originally accepted;
- `created_at` / `accepted_at` provenance sufficient for audit display.

`BindingAnchor` is enough to:

- show what snapshot an accepted binding came from;
- explain whether a current request matches the old snapshot identity;
- detect hash or resolver drift;
- decide whether revalidation requires rebuilding graph data;
- support issue #141 remediation explanations.

`BindingAnchor` is not enough to answer arbitrary graph-neighbor queries. If a
caller asks for full graph operations after the full payload was evicted,
GraphStore must rebuild from source, expand an appropriate slice, or return
`unknown_not_indexed`.

## Branch Names

Branch names are provenance only.

They may be:

- supplied to snapshot resolution;
- recorded on `GraphSnapshot` for display;
- recorded on `BindingAnchor` to explain where the evidence was first found;
- used to detect ref movement relative to a previously resolved SHA.

They must not be:

- part of `GraphStore` cache identity;
- part of `graph_snapshot_id`;
- part of durable `BindingEvidence` identity;
- used to decide whether two graph facts are the same durable snapshot.

Two branches pointing at the same `validated_sha` and `index_version` share the
same graph content identity. One branch advancing creates a new resolved
snapshot; it does not mutate the old snapshot.

## Consumer Contracts

Binding, preflight, compliance, and remediation consumers must preserve the
scope and readiness of graph data they use.

Binding:

- may materialize `BindingEvidence` only from verified evidence;
- must preserve `graph_snapshot_id` and `validated_sha`;
- must reject or revalidate snapshot mismatches explicitly.

Preflight:

- may use slices for diff/file/query-scoped work;
- must expose the local snapshot scope it inspected;
- must report hosted/team-only or unseen branch scope as unknown/hosted
  required.

Compliance:

- may use graph evidence only when policy accepts that evidence for the
  requested scope;
- must not treat local no-conflict as global/team no-conflict.

Remediation and revalidation:

- may use `BindingAnchor` to explain what changed;
- may rebuild a full snapshot or create a repair-scoped slice;
- must create durable governance events only through the accepted event-store
  flow, not by mutating cache state.

## Implementation Handoff

This ADR is the design deliverable for issue #136. It does not implement
GraphStore. Implementation issues should consume it as follows:

- #164 / [GraphSlice Planning And Sufficiency Semantics](../specs/graphslice-planning-sufficiency.md)
  defines how task intent becomes required graph scope and when a scoped slice
  is sufficient to support verified, absent, remediation, or readiness claims;
- #138 implements snapshot-scoped GraphStore binding verification;
- #140 implements progressive preflight over source, diff, and graph scope;
- #141 / [Graph Remediation And Revalidation Repair Paths](../specs/graph-remediation-revalidation.md)
  defines remediation and repair behavior;
- #137 defines preflight search/readiness semantics where it overlaps graph
  slicing.

First implementation slices should include deterministic tests for:

- cache key derivation excluding branch names;
- `graph_snapshot_id` derivation excluding `workspace_id`;
- dirty worktree returning `unknown_stale` or an explicit dirty-slice state;
- partial and slice scopes returning `unknown_not_indexed` outside coverage;
- stale invalidation on indexer/resolver version change;
- retained `BindingAnchor` inspection after full graph payload eviction.

## Consequences

Positive:

- GraphStore can support lazy and incremental indexing without claiming false
  completeness.
- Binding, preflight, compliance, and remediation share one graph lifecycle
  vocabulary.
- Branch movement cannot corrupt durable graph identity.
- Old accepted `BindingEvidence` remains inspectable even after graph payload
  eviction.
- The contract leaves room for hosted/team graph services without smuggling
  hosted visibility into local results.

Risks:

- partial and slice states require disciplined handling at every consumer
  boundary;
- `BindingAnchor` retention can grow over time and will need a later
  compaction/archive policy;
- `workspace_id` adds implementation complexity for multi-worktree local
  caches;
- consumers must distinguish cache stale/unknown states from governance
  lifecycle events.

## Non-Goals

- Do not implement GraphStore in this ADR.
- Do not define the hosted/team Conflict Oracle.
- Do not define full graph remediation repair paths; issue #141 owns that.
- Do not define the final preflight search-ranking strategy; issue #137 owns
  that.
- Do not implement validation-token handling.
- Do not store full graph indexes as canonical `.bicameral/` governance
  artifacts.
- Do not make branch names durable graph identity.
