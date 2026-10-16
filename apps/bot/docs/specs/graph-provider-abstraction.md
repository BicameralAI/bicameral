# Graph Provider Abstraction And CodeTargetIdentity

**Date:** 2026-06-06
**Status:** proposed
**Issue:** https://github.com/BicameralAI/bicameral-bot/issues/154
**Related:** ADR-0010, ADR-0012, ADR-0015, ADR-0017, ADR-0021;
`code-grounding-governance-guideline.md`;
`enforcement-memory-projection.md`

## Problem

Bicameral's current local grounding path is intentionally conservative. The
Rust `SymbolResolver` is a safe first implementation slice for exact
snapshot-scoped evidence, but it is not the completed graph-grounding roadmap.

Issue #154 asks whether Bicameral should evaluate a CodeGenome-style graph
substrate for:

- code facts;
- code target identity and continuity;
- impact analysis;
- cross-language graph intelligence;
- preflight and enforcement-memory grounding;
- richer support for `BindingProposal`, Binding Revalidation, and
  `RevalidationOutcome`.

The question is not whether CodeGenome, or any graph engine, should own
Bicameral governance authority. It must not. The question is whether richer
graph engines should be accepted as optional evidence providers behind the
ADR-0010 codegraph contract.

## External Reference

The referenced CodeGenome repository describes itself as a research prototype
for a content-addressed, multi-layer program analysis graph. Its README says it
combines syntax, semantic, control/data-flow, process, runtime, SCIP, and LSP
observations into one graph; uses BLAKE3 identities; fuses independent edge
confidence via noisy-OR; and includes an experiment engine for hill-climbing on
graph quality. It also explicitly marks the project as not production-ready
with changing APIs.

That makes CodeGenome useful as direction and research input, not as a direct
production dependency for Bicameral v0.x.

Reference:
https://github.com/MythologIQ-Labs-LLC/CodeGenome

## Decision

Adopt a graph provider abstraction direction.

CodeGenome-style capability is directionally useful, but it should be treated
as an optional provider behind ADR-0010, not as:

- a replacement for the Bicameral event store;
- a replacement for daemon governance policy;
- a direct required dependency;
- an authority surface;
- a reason to retire the Rust resolver before a provider contract is accepted.

The Rust `SymbolResolver` remains the reference exact-validation slice and
conformance baseline. A richer provider may add language coverage, identity
continuity, impact analysis, and advisory ranking only by mapping its outputs
into Bicameral-owned graph vocabulary.

## 2026-06-22 RFQ Resolution: CodeGenome In Existing Runtime

Issue #498 resolves the follow-up question this way:

```text
CodeGenome-style memory may be a layered graph-memory provider behind existing
CodeLocator, GraphProvider, GraphStore, catalogue projection, and RecallPacket
contracts. It must not become a second CodeLocator, a binding lifecycle
orchestrator, or Bicameral governance authority.
```

The accepted integration shape is deliberately two-layered:

1. **Graph-provider layer.** CodeGenome-style extraction, composition, query,
   overlay confidence, signal propagation, and evaluation may produce graph
   facts, graph-memory edges, ranking reasons, readiness labels, and provider
   provenance.
2. **Bicameral interpretation layer.** The daemon maps those outputs into
   `GraphSnapshot`, `GraphSlice`, `CodeTargetIdentity`,
   `GraphEvidenceState`, `ScopeSufficiencyProof`, `BindingProposal`,
   catalogue projection rows, and RecallPacket fields before any product
   surface consumes them.

The provider may maintain canonical code-graph reality for a declared
repository snapshot: files, symbols, references, flows, graph edges, overlay
membership, and provider-local edge confidence. Bicameral remains canonical for
Decision Ledger state, governed sources, evidence state, binding state,
review/signoff/compliance authority, correction acceptance, and event-store
mutation.

Issue #525 extends this authority boundary with the recall-quality decision in
[Alpha Recall Multi-Signal Fusion And Recall-Influence Boundary](alpha-recall-multi-signal-fusion.md):
CodeGenome-style signals may enrich recall ranking, slice planning, and
explanation under bounded Option B, but only after a bot-owned
recall-admissibility gate and without weakening GraphStore identity,
CodeLocator verification, or governed `BindingEvidence` materialization.

### Preserved CodeGenome Concepts

The six CodeGenome-style layers are useful when scoped to graph memory:

| Layer | Bicameral posture |
|---|---|
| L1 Canonical Reality | Keep only as code artifact reality inside a declared repository snapshot. It is not Bicameral decision truth. |
| L2 Extraction | Keep as provider extraction into graph artifacts. |
| L3 Composition | Keep as graph-state fusion. Noisy-OR and confidence fusion are allowed for graph reliability and ranking only. |
| L4 Query | Keep as provider query capability behind bot-owned query/slice contracts. |
| L5 Governance | Adapt to graph-operation audit, provider provenance, action constraints, and experiment controls. It does not replace Bicameral governance. |
| L6 Evaluation | Keep for graph substrate quality, recall quality, stale edge rates, overlay usefulness, and retrieval efficiency. Evaluation cannot mutate authority. |

The overlay model is also preserved as provenance metadata:

| Overlay | Allowed contribution |
|---|---|
| Syntax | AST edges and exact parser provenance. |
| Semantic | Calls, imports, implements, references, or symbol-resolution hints with provider confidence. |
| Flow | Control/data-flow facts within declared supported scope. |
| Process | Entrypoint or workflow traversal hints with declared depth/decay. |
| PDG | Control/data dependence facts when the provider can explain scope. |
| Runtime | Trace-derived weighted edges with trace coverage disclosed. |
| SCIP | Compiler-index references and implementations inside indexed scope. |
| LSP | Tool-supported references with tool/version provenance. |
| Fused | Merged edge view with fusion method and confidence metadata. |

Overlay confidence and fused confidence can order recall, suggest expansion,
and explain why a result was included. They cannot authorize mutation,
promotion, signoff, compliance, enforcement, no-conflict, or verified
`BindingEvidence`.

### Mapping To Existing Bicameral Terms

CodeGenome-style outputs must map into existing Bicameral vocabulary:

| Provider output | Bicameral mapping |
|---|---|
| Repository/index run | `GraphSnapshot` with provider id, provider version, index version, validated SHA, readiness, and freshness. |
| Scoped graph subset | `GraphSlice` with `scope_kind`, `scope_fingerprint`, covered files/symbols/edges, skipped scope, and unknown scope. |
| File/symbol/edge target | `CodeTargetIdentity` evidence, not canonical identity by itself. |
| Exact provider proof | Candidate for `GraphEvidenceState::verified` only after sufficient deterministic proof for the requested task. |
| Confidence-fused edge or ranked result | `approximate_candidate`, ranking reason, or provider-backed relevance reason. |
| Impact/staleness propagation | Affected-surface hint or freshness/currentness signal. |
| Provider operation metadata | Provider provenance, overlay provenance, trace/evaluation metadata, readiness labels. |
| Candidate binding target | `BindingProposal` only after bot-owned validation; never direct `BindingEvidence`. |
| Search/recall result | Catalogue projection row and RecallPacket contribution with searched-scope proof. |

### Slice Planning And Query Expansion

CodeGenome-style memory may help propose graph slice expansion, but it does not
own `GraphScopePlan`.

Allowed:

- provider suggests files, symbols, neighborhoods, edge kinds, or affected
  surfaces to inspect;
- provider ranks expansion candidates;
- provider reports which overlays and edge kinds support a suggestion;
- provider reports stale, partial, unsupported, or unknown scope.

Required:

- the daemon canonicalizes suggestions into deterministic target identities;
- the daemon produces or updates `TaskIntent`, `GraphScopePlan`,
  `scope_fingerprint`, and `ScopeSufficiencyProof`;
- exact claims remain non-verified until the resulting slice is sufficient for
  the task;
- expansion output is replayable from provider snapshot identity, index
  version, normalized inputs, and declared ranking metadata.

### Probabilistic Reasoning Boundary

Probabilistic or confidence-scored provider work is allowed in:

- provider extraction and graph composition;
- provider indexing quality evaluation;
- query expansion suggestions;
- candidate ranking;
- affected-surface prioritization;
- recall result ordering;
- graph substrate experiments that are reviewed before product behavior
  changes.

It is not allowed as:

- `CodeLocator` verified execution;
- `ScopeSufficiencyProof`;
- acceptance of `DecisionCandidate`, `Decision`, `SourceEvidence`, or
  `BindingEvidence`;
- correction acceptance;
- signoff, compliance, enforcement, no-conflict, or merge-safety proof;
- a hidden hot-path dependency required to assert graph facts.

`CodeLocator` remains deterministic. It may consume provider-ranked candidates,
but verification is still exact provider proof over sufficient bot-owned scope.

### RecallPacket Contribution

RecallPacket is an honest read artifact over searched scope. CodeGenome-style
memory and catalogue projection may contribute compact fields, but must not
embed an unbounded graph dump or hide scope limits.

Allowed RecallPacket contributions:

- `matched_nodes`;
- `matched_edges`;
- `overlay_sources`;
- `evidence_refs`;
- `relevance_reasons`;
- `authority_labels`;
- `freshness_labels`;
- `readiness_labels`;
- `searched_scope`;
- `unknown_scope`;
- `affected_surface_hints`;
- `allowed_next_actions`.

Each graph-backed RecallPacket contribution must carry searched-scope proof:

- searched corpus/source set;
- graph snapshot id;
- graph slice id, when used;
- provider/index version;
- covered files, symbols, ranges, and edge kinds;
- skipped or unsupported sources;
- unknown scope;
- freshness/currentness state;
- available expansion action, when local expansion is possible.

RecallPacket output may help a user inspect evidence, expand scope, or route a
candidate into governed promotion. It must not create or promote
`DecisionCandidate`, create `Decision`, create `SourceEvidence`, create
`BindingEvidence`, accept signoff, assert compliance/enforcement, or assert
no conflict.

### Fallbacks

When graph memory is stale, partial, unsupported, or unavailable, consumers
must choose an honest fallback:

- source-only recall;
- explicit `expand_scope` action;
- `hosted_required` or team-required readiness;
- review-needed/currentness label;
- `unknown_not_indexed`, `unknown_stale`, `unsupported`, `ambiguous`, or
  `approximate_candidate` evidence state.

No-match from a partial or stale provider is not no-conflict. An affected
surface hint is not `BindingEvidence`. A strong fused confidence score is not
approval proof.

### Required Negative Fixtures

Implementation slices that consume this RFQ must prove:

- CodeGenome or catalogue projection cannot create or mutate `Decision`;
- CodeGenome or catalogue projection cannot create or mutate
  `DecisionCandidate`;
- CodeGenome or catalogue projection cannot create or mutate `SourceEvidence`;
- CodeGenome or catalogue projection cannot create or mutate `BindingEvidence`;
- graph confidence cannot authorize correction acceptance;
- no-match cannot produce no-conflict;
- stale or partial projection surfaces readiness labels;
- affected-surface hints cannot become `BindingEvidence`;
- graph-derived compliance, signoff, enforcement, or merge-safety claims are
  refused;
- adaptive experiments cannot mutate authority.

## Provider Contract

A graph provider may feed Bicameral graph-backed workflows only when it can
return provider output through this minimum contract:

- `GraphSnapshot` identity:
  - `graph_snapshot_id`;
  - `repo_id`;
  - `validated_sha` / commit SHA;
  - `index_version`;
  - provider identity and version.
- claim identity:
  - file path;
  - range, when relevant;
  - symbol name or qualified name, when relevant;
  - binding target kind;
  - structural hash or provider-specific identity, when available.
- evidence state:
  - one ADR-0010 `GraphEvidenceState`;
  - exact-vs-approximate classification;
  - readiness/freshness metadata.
- provenance:
  - parser/resolver/indexer identity;
  - source overlay or observation kind;
  - confidence score when the provider has one;
  - evidence bundle or trace sufficient for review.
- scope:
  - requested `TaskIntent`;
  - `GraphScopePlan`;
  - `ScopeSufficiencyProof` for verified or absent claims.

Only substrate-specific exact checks over a sufficient scope may return
`GraphEvidenceState::verified`. Confidence-fused or probabilistic provider
outputs may rank candidates and support policy recommendations, but they remain
`approximate_candidate` or another non-verified state until exact validation
succeeds.

## CodeTargetIdentity

`CodeTargetIdentity` is a Bicameral-owned abstraction. Providers may contribute
identity evidence, but they do not define canonical product identity.

`CodeTargetIdentity` relates code targets across `GraphSnapshot`s when line
ranges, file locations, symbols, or implementation structure change.

Required shape:

- original and current `GraphSnapshot` identity;
- target kind, such as file path, code region, symbol, dependency, workflow, or
  deploy surface;
- original and current target coordinates;
- continuity evidence, such as:
  - content/blob hash;
  - structural hash;
  - symbol name or qualified name;
  - resolver-specific identity;
  - provider-specific identity;
  - move/rename evidence;
  - matching imports, callers, callees, implementations, or references when
    relevant;
- evidence state;
- confidence, when the provider supplies it;
- provenance and freshness;
- sufficiency proof for the requested identity task.

Provider-specific identity, including UOR-style or content-addressed identity,
is evidence inside `CodeTargetIdentity`. It is not the canonical identity by
itself.

## Evidence Mapping Rules

Graph providers must map into ADR-0010 states without probability laundering:

| Provider output | Bicameral state |
|---|---|
| Exact proof over sufficient requested scope | `verified` |
| Exact inspected absence over sufficient requested scope | `not_found` |
| Unindexed required file/neighborhood | `unknown_not_indexed` |
| Stale snapshot, stale file hash, parser/index drift, or dirty mismatch | `unknown_stale` |
| Multiple unresolved candidate targets | `ambiguous` |
| Language, file type, generated artifact, macro behavior, or edge kind outside exact support | `unsupported` |
| Ranked, semantic, confidence-fused, or LLM-assisted candidate without exact proof | `approximate_candidate` |
| Cross-repo, team-active-PR, hosted-only, or unavailable remote visibility | `hosted_required` readiness/scope output |

High confidence may influence ordering, reviewer prompts, cost policy, hosted
recommendations, or advisory governance proposals. It must not promote a claim
to `verified`.

## Useful v0.x Capabilities

Useful local or optional-provider capabilities for v0.x:

- multi-language extraction that maps into Bicameral evidence states;
- content-addressed file/region/symbol identity as continuity evidence;
- snapshot-scoped occurrence evidence;
- move and rename candidate generation;
- bounded impact analysis over local files/symbols;
- exact validation gates for AI-suggested grounding;
- provider provenance and freshness reporting.

Hosted/team-only or future capabilities:

- cross-repo/federated graphs;
- continuous multi-branch refresh;
- team active-PR visibility;
- large-scale confidence fusion;
- Conflict-Oracle-style intelligence;
- historical organization-wide grounding optimization.

## Research Loop Option

Kevin's review noted that CodeGenome is itself a lab experiment around active
memory representation, retrieval, and self-improving graph quality. Bicameral
should consider owning that research loop under its governance model rather
than only consuming an external provider.

If Bicameral runs an active-memory experiment loop, each experiment step should
be governed as evidence-bearing work:

- proposed graph-quality change;
- input corpus and snapshot scope;
- metric and fitness function;
- observed result;
- regression or falsification evidence;
- reviewer or policy acceptance before the result affects product behavior.

This remains future research scope. It does not change v0.x provider
boundaries.

## Integration Posture

Evaluation stance:

- Direct dependency: rejected for now.
- Optional provider behind ADR-0010: accepted direction.
- Hosted advisory inspiration: accepted interim use.
- Substrate replacement: rejected.

No implementation should wire a provider directly into governance
materialization. Provider output enters Bicameral as graph facts,
`BindingProposal`s, search/relevance candidates, impact evidence, or
revalidation observations. Accepted `BindingEvidence`, durable
`RevalidateBinding` history, compliance, signoff, and conflict outcomes still
require daemon policy and event-store materialization.

## Required Follow-Up

Open implementation/design follow-ups only after this RFQ is accepted:

- define concrete provider response structs and fixtures;
- sharpen `CodeTargetIdentity` in protocol/API docs when implementation starts;
- decide whether provider support lands first as local optional configuration
  or hosted advisory service;
- add conformance fixtures showing probability does not become `verified`;
- add provider-agnostic enforcement-memory fixtures for accepted
  `BindingEvidence`;
- decide whether CodeGenome research-loop evaluation belongs in a factory or
  product research track.

## Source Anchors

- [ADR-0010](../adr/0010-snapshot-scoped-local-code-graph-contract.md)
  defines snapshot-scoped graph evidence and the `GraphEvidenceState`
  authority boundary.
- [ADR-0012](../adr/0012-rust-symbolresolver-boundary-and-followups.md)
  keeps the Rust resolver as a first local slice, not the completed roadmap.
- [ADR-0015](../adr/0015-reasoning-provider-boundary-and-hosted-artifact-exception.md)
  provides the analogous provider-backed enrichment boundary for LLM reasoning.
- [ADR-0017](../adr/0017-binding-toolrequest-evidence-contract.md)
  restricts binding materialization to verified graph evidence.
- [ADR-0021](../adr/0021-graphstore-snapshot-slice-lifecycle.md)
  defines GraphStore snapshot and slice lifecycle identity.
- [Code-Grounding Governance Guideline](../code-grounding-governance-guideline.md)
  states the bot-owned authority and CodeIdentity/CodeGenome direction.
- [Enforcement Memory Projection](enforcement-memory-projection.md)
  defines provider-agnostic projection of accepted authority to code/work
  relationships.

## Non-Goals

- Do not implement a CodeGenome integration here.
- Do not require replacing the Rust resolver.
- Do not change governance authority or event-store materialization rules.
- Do not make provider confidence equivalent to accepted governance evidence.
- Do not store provider graph indexes as canonical event-store artifacts.
