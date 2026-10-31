---
id: ARCH-0005
title: Reconstruct approved governed context with explainable, replay-stable reads
kind: decision
status: accepted
date: 2026-10-26
owners: [@Knapp-Kevin]
governs: [governed-context-reconstruction, governed-read-diagnostics]
depends_on: [ARCH-0003, ARCH-0004]
supersedes: []
references: [ARCH-0006]
---

# ARCH-0005: Reconstruct approved governed context with explainable, replay-stable reads

## Context

This record carries proposed Bot ADR-0035 into this repository. It was drafted on 2026-10-22 and has not been reviewed. Links to private trackers are removed. The text is otherwise unchanged except that ADR-0033, ADR-0034 and ADR-0036 now read ARCH-0003, ARCH-0004 and ARCH-0006. The source also listed Bot ADR-0007 as a dependency. That ADR was never accepted, and the reference was dropped when this record was accepted on 2026-10-28.

A daemon may preserve the correct event history and still return a misleading context read: fail to discover governing semantic content, include a tracked or superseded source as governing, discard an assertion's polarity or an `AtomRelationV1` edge, hide a stale/partial source, or return different accepted sets after equivalent replays. Semantic search rank and graph adjacency do not confer Product authority.

The **current accepted Product model** is ARCH-0003/0004: one `SourceSnapshot` has one `GovernedDomain` with claims, atoms, polarised claim assertions and `AtomRelationV1` relations; `Signoff` is the **approved state of that GovernedDomain**. A `CheckRecord` records an exact unordered pair of completed domain digests and `conflict`; missing checks do not mean `conflict: false`. Supersede names a successor snapshot where that event records one. An approved domain alone can govern. A pending/tracked, rejected, superseded, untracked, or failed-extraction domain cannot.

Some older Bot documents and issue descriptions still speak of `Decision`, `DecisionCandidate`, and `Decision Ledger`. ARCH-0004 explicitly retires these entities; **do not create, re-introduce, or derive a second authority object** while applying this record. A separate terminology-consolidation change can reconcile legacy documentation without holding reconstruction fixes hostage.

## Decision

**Improve the existing Bot daemon's derived reconstruction, diagnostic and regression behavior first.** Preserve accepted event replay and the ARCH-0003/0004 governed model. Use existing source/semantic identities and read contracts; introduce no new durable authority, persistence substrate, graph dependency, or client-owned policy.

1. **Daemon-side applicability:** Resolve the authenticated Product/Source scope and pinned event/substrate revision; reconstruct current `SourceSnapshot` and `GovernedDomain` approval from replay, never by latest timestamp, text ranking, graph degree, LLM confidence, or a UI heuristic. A derived index can propose relevant references but cannot assign `Signoff`.
2. **Fidelity:** Retain exact `source_uri`, snapshot address/content hash, `source_digest` of completed semantic content, relevant claim/atom/assertion/relation IDs and polarity. Do not fabricate source anchors or scope fields **inside** `GovernedDomain`, whose payload ARCH-0003 freezes. Map external query scope separately through existing access/read context.
3. **Read diagnostics:** For local authorized diagnostic inspection, associate governing selections, rejected/excluded candidates, scope actually inspected, provenance references, projection revision/freshness and reason codes with a single read receipt. Distinguish genuinely negative results inside a declared complete scope from incomplete, stale, inaccessible or unsupported inputs. Do not leak raw source bodies, local paths, policy internals or private cross-Product identifiers into hosted/MCP transport.
4. **Typed uncertainty:** Prefer existing typed error/coverage contracts; do not invent a broad status enum. Missing required source snapshots, pending/failed extract, missing required `CheckRecord`, stale or unavailable projection, and unsupported relation lowering must remain explicit. A missing check never becomes no-conflict or successful approval. If exact coverage cannot be established, fail closed or report an authorized scoped unknown.
5. **Deterministic replay:** With the same event set/order, semantic content, approved snapshots, resolver policy/version, query context and read implementation, repeated runs, process restart and full rebuild must yield identical **normalized** applicable results, authority labels, exclusions and receipts. Volatile diagnostics can have local timestamps, but they must not alter governing results or hash-stable semantic receipts.
6. **Reader parity:** Compose discovery/relevance and daemon-authored governed-state inspection as distinct operations. Any existing reader contract referencing retired `Decision` subjects must be reconciled to ARCH-0003/0004 before changing its public wire shape. MCP, HTTP and dashboard remain presentation/transport consumers, not new semantic classifiers.

### Not decided

- General `as_of` / recorded-time lookup, inferred causal lineage, new `GovernedStatusLookup` versions, network diagnostics, or blanket Product completeness.
- Full claim-to-utterance accuracy. ARCH-0006 proposed a downstream check for it and was rejected on 2026-10-28.
- TypeGraph installation, graph-store replacement, new LLM extraction/prompt, approval policy modification, schema/entity addition, hosted source storage, or a new benchmark authority.

### Minimum conformance fixtures

1. **Signoff:** completed but tracked `GovernedDomain` remains non-governing; only an approved snapshot with valid replayed signoff governs. New snapshot inherits neither previous signoff nor checks.
2. **Exact conflict:** pairwise `CheckRecord` of the same digests and `conflict` preserved; missing/failed check cannot be treated as `conflict: false` or approval.
3. **Revisions:** approved old snapshot superseded by successor, with later unapproved snapshot present; current read cannot select later draft; historical behavior is not silently asserted without a versioned time contract.
4. **Semantic relation fidelity:** claims, atoms, assertion polarity and `AtomRelationV1` edges survive traversal and re-serialization; negative polarity and alternative relations cannot be dropped.
5. **Scope/security:** wrong Product, inaccessible source, unsupported relation, stale or partially indexed corpus; any authorized result discloses only searched scope and typed unknowns, never forbidden data.
6. **Replay:** restart, deduplicated event application and full rebuild with the same immutable inputs produce the same normalized authorized result; malformed/partial replay must not result in a reassuring empty read.
7. **Consumer parity:** daemon-derived governing references and limitations are unchanged after passing through existing supported MCP/HTTP/UI read transports.

Human-approved gold labels and independent end-to-end qualification are separate from these fixtures. Individual unit and fixture passes do **not** prove end-user reliability.

## Consequences

- **Benefits:** near-term trust and debugging gains without a new dependency; concrete regressions for reported inconsistent results; deterministic authority behavior.
- **Costs:** per-read provenance/coverage accounting, fixtures, and optional local diagnostic storage; existing reader contracts need an ARCH-0004 terminology reconciliation.
- **Security:** permission filtering must precede cross-reference expansion; do not create an enumerating graph endpoint or return private source excerpts without authorization.
- **State refinement:** zero additional canonical lifecycle states and no new Product event kind. Receipts and coverage are rebuildable diagnostic/read data, not product authority.
- **Release:** existing alpha/current product claims do not expand. A design-approved record is not evidence that the end-to-end read path is working.

Considered alternatives:

1. **Selected:** strengthen existing replay and diagnostics with proof fixtures; keep graph library decisions separate.
2. **Rejected for this tranche:** deploy TypeGraph or another parallel graph database before demonstrating added value over native infrastructure.
3. **Rejected:** let semantic similarity/rank, recent documents or inferred graph relations select Product approval.
4. **Rejected:** collapse incomplete retrieval into empty success, no-conflict or generalized approval.

Review this architecture against the accepted ARCH-0003/0004 model and current code; if a normative schema change becomes necessary, stop and bring it through the governed exact-revision specification process before implementation. The conformance fixtures above change no product behavior and may land before this record is accepted. No release promotion follows from this record.
