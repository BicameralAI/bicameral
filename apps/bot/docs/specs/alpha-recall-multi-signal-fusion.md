# Alpha Recall Multi-Signal Fusion And Recall-Influence Boundary

**Date:** 2026-06-23
**Status:** proposed
**Issue:** https://github.com/BicameralAI/bicameral-bot/issues/525
**Related:** #498, #522; ADR-0010, ADR-0019, ADR-0021, ADR-0024;
`graph-provider-abstraction.md`; `graphslice-planning-sufficiency.md`;
`librarian-recall-readme.md`; `preflight-search-readiness.md`

## Problem

Issue #498 settled the CodeGenome authority boundary: CodeGenome-style memory
may enrich graph-backed recall, but it must not bind, sign off, assert
compliance, assert no-conflict, create `BindingEvidence`, or replace
`CodeLocator` verification.

That left a separate product question open: how Bicameral should achieve high,
honest recall at preflight. The alpha experience needs a fast, trustable lookup
that can combine:

- code grounding and changed-file context;
- semantic or textual similarity;
- ownership and source overlap;
- prior bindings and governed Decision catalogue state;
- CodeGenome-style overlay signals, fused confidence, recency/currentness,
  affected-surface hints, and evaluation metadata.

The goal is high recall for navigation, not stronger autonomous authority.

## Decision

Adopt bounded Option B for alpha recall.

```text
CodeGenome enriches recall.
CodeGraph / GraphStore preserves deterministic graph identity.
CodeLocator verifies selected code targets.
RecallPacket presents ranked, cited, honest recall.
BindingEvidence is created only through governed binding workflow after
deterministic verification.
```

CodeGenome-style signals may feed recall ranking, slice planning, explanation,
and evaluation when they pass a bot-owned recall-admissibility gate.
CodeGraph / GraphStore still owns `graph_snapshot_id`, `validated_sha`,
`GraphSnapshot`, `GraphSlice`, and freshness/readiness identity. `CodeLocator`
still owns deterministic target verification. `binding.create` still revalidates
before any durable `BindingEvidence` materializes.

This is not Option A, where CodeGenome sits only as a downstream sibling
evidence source. Option A is safer but leaves recall quality weaker and slower
because slice planning cannot use enriched memory to decide where to look.
Bounded Option B preserves the recall advantage while keeping authority gates
unchanged.

## Runtime Flow

The accepted alpha flow is:

```text
source/code observation
  -> object identity resolution when available
  -> CodeGraph snapshot selection or production
  -> CodeGenome-style signal evaluation
  -> recall-admissibility gate
  -> GraphSlice plan construction
  -> RecallPacket production
  -> optional BindingHint / BindingProposal selection
  -> CodeLocator deterministic verification
  -> binding.create governed revalidation and materialization
```

### Step Responsibilities

| Step | Owner | Output role |
|---|---|---|
| Object identity resolution | provider or bot adapter | identity substrate / provenance |
| CodeGraph snapshot selection | GraphStore / GraphSnapshotService | identity anchor |
| CodeGenome overlay and fused signal evaluation | graph-memory provider | signal input |
| Recall-admissibility gate | bot runtime | deterministic filter |
| GraphSlice planning | bot runtime, using admitted hints | read-model planning artifact |
| RecallPacket production | bot runtime | read-model state / signal output |
| CodeLocator verification | bot runtime | authority-gate candidate |
| `binding.create` materialization | governed command path | durable authority |

Short form:

```text
CodeGenome helps decide where to look.
CodeGraph anchors the graph snapshot.
CodeLocator proves what was found.
RecallPacket explains what was searched and surfaced.
BindingEvidence records only what passed deterministic gates.
```

## Signal Fusion

Alpha recall should use a deterministic, replayable fusion envelope around
possibly probabilistic inputs. Implementations may tune weights, but every
ranked result must expose the normalized signals that contributed to it and the
metadata needed to replay the ordering for fixture inputs.

Allowed signal families:

| Signal family | Examples | Role |
|---|---|---|
| Governed catalogue | accepted Decision match, candidate relationship, source refs, prior binding refs | identity anchor, provenance, signal input |
| Code grounding | changed files, exact symbols, accepted binding overlap, graph neighborhoods, affected surface | identity anchor when exact; signal input when approximate |
| Semantic/textual | title/body/source text similarity, embedding similarity, query expansion terms | signal input |
| Ownership | code owner, source owner, decision owner, actor/team/project overlap | signal input for ranking; separate governance signal for signoff |
| Freshness/currentness | source last seen, graph staleness, snapshot age, provider recency | signal input and signal output |
| CodeGenome overlay | syntax, semantic, flow, process, PDG, runtime, SCIP, LSP, fused overlay confidence | signal input/provenance |
| User/task context | task intent, diff scope, selected project, requested source scope | provenance and signal input |

The fusion envelope must produce:

- `ranking_score` or equivalent internal ordering value;
- `ranking_reasons`, using stable reason codes;
- contributing signal kinds and weights;
- source refs and graph snapshot refs when available;
- searched scope and unknown scope;
- freshness/currentness labels;
- provenance for provider, index, overlay, and model/version where relevant.

No single scalar confidence may hide role differences from ADR-0024. Signals
guide ranking and explanation. Gates create durable authority.

### Ranking Precedence

For alpha fixtures, ordering should preserve this precedence before any tuned
score tie-break:

1. direct accepted binding or verified graph overlap in the requested scope;
2. high-confidence changed-file, symbol, or affected-surface overlap with
   searched-scope proof;
3. accepted Decision/source overlap from the reviewed corpus;
4. ownership/project/source overlap that matches the actor or task scope;
5. semantic/textual similarity over reviewed corpus text;
6. CodeGenome fused or propagated hints that are explained and fresh enough;
7. approximate candidates with lower or incomplete provenance.

Stable tie-breaks are required: canonical Decision id, candidate id, then title
or source ref.

## Recall-Admissibility Gate

Before enriched signals influence `RecallPacket` ranking, `GraphSlice` planning,
or explanations, the bot must apply a recall-admissibility gate.

Required admitted-signal fields:

- source ref or source object identity;
- signal kind;
- signal value or normalized weight;
- provenance, including provider id and provider/index/model version;
- corpus id and corpus version when source-backed;
- graph snapshot id and graph slice id when graph-backed;
- searched scope;
- unknown scope;
- freshness/currentness state;
- ranking or relevance reason;
- supported action, such as rank-only, slice-hint, explain-only, or evaluate.

Signals must be rejected or downgraded when they are:

- stale beyond the task's declared freshness threshold;
- unscoped or missing searched-scope proof;
- missing source refs or graph snapshot refs when those are required;
- produced by an unsupported overlay or provider version;
- outside the alpha corpus or requested source scope;
- unable to explain why a Decision, candidate, file, or symbol was surfaced;
- attempting to assert compliance, signoff, no-conflict, source approval,
  accepted correction, or binding authority.

The gate is deterministic for a fixed input packet. It may accept probabilistic
signal values as inputs, but the decision to admit, downgrade, or reject a
signal must be replayable from declared metadata.

## Slice Construction

`CodeLocator` owns the orchestration boundary for graph-backed target
verification, while `GraphScopePlan` remains the deterministic plan artifact.
CodeGenome-style memory may suggest where to look, but the daemon canonicalizes
those suggestions before a slice is built or used.

Allowed CodeGenome contributions to slice planning:

- suggested files, symbols, neighborhoods, edge kinds, and traversal depth;
- affected-surface hints with propagation depth and attenuation metadata;
- overlay provenance explaining why a neighborhood matters;
- stale, partial, unsupported, and unknown-scope disclosures;
- ownership or source-overlap hints that help prioritize expansion.

Required daemon behavior:

- derive `TaskIntent` from the ToolRequest or recall request;
- resolve `GraphSnapshot` identity through GraphStore / GraphSnapshotService;
- canonicalize admitted hints into file, symbol, edge, and source-scope
  identities;
- produce `GraphScopePlan.scope_fingerprint`;
- build or select `GraphSlice`;
- return `ScopeSufficiencyProof` before exact claims become verified.

Probabilistic ranking may decide expansion order. It may not decide that the
result is sufficient.

## Ownership As Recall Signal

Ownership has two roles and must stay split.

As a recall signal, ownership may improve ranking when:

- the task actor, project, source owner, code owner, Decision owner, or review
  owner overlap;
- a Decision has historically governed the actor's touched subsystem;
- a source, team, product area, repository, or integration namespace matches
  the requested scope;
- ownership metadata explains why a stale but important Decision still
  deserves attention.

As governance authority, ownership remains a separate gate for signoff or
review. A ranking-time ownership match must not become signoff proof, source
approval, compliance, or binding authority.

Recall output should label ownership reasons as ranking/explanation reasons,
for example:

```text
ranking_reason = owner_overlap
authority_label = ownership_signal_only
allowed_next_action = request_signoff_review
```

## RecallPacket Honesty Requirements

Every graph- or CodeGenome-backed RecallPacket contribution must carry
searched-scope proof and an expansion affordance when more scope is locally
available.

Required fields or equivalent:

- `searched_scope`;
- `unknown_scope`;
- corpus id and corpus version;
- source refs and cited spans;
- graph snapshot id, graph slice id, and provider/index version when
  graph-backed;
- signal kinds and ranking reasons;
- freshness/currentness labels;
- readiness labels;
- available `expand_scope` action when local expansion is possible;
- explicit `hosted_required`, `unsupported`, `stale`, `not_indexed`, or
  `source_only` labels when applicable.

"No relevant Decision found" means no relevant Decision was found in the
declared searched scope. It never means global completeness, no conflict,
compliance, signoff, or merge safety.

## Alpha Quality Bar

Alpha recall quality should be measured over fixed fixtures before broadening
the corpus.

Recommended fixture corpus:

- accepted Decisions and source evidence from the Git docs corpus;
- source-backed DecisionCandidates;
- representative tasks with file paths, diffs, symbols, source snippets, and
  ambiguous natural-language queries;
- stale, partial, unsupported, and unknown-scope cases;
- ownership and source-overlap cases;
- CodeGenome-style overlay/fusion fixtures with both useful and noisy signals.

Minimum metrics:

- recall@k for known relevant accepted Decisions;
- precision@k for top-ranked surfaced Decisions/candidates;
- MRR or first-relevant rank for preflight ergonomics;
- false-negative count for critical accepted Decisions;
- false-positive/noise count for irrelevant but high-scoring approximate
  results;
- honesty coverage: percentage of partial/stale/unsupported cases that surface
  the right scope label;
- deterministic replay: identical ordering for identical fixture inputs.

Alpha success is not perfect semantic search. Alpha success is that important
reviewed Decisions surface early, with honest scope labels, stable ordering,
and no authority overclaim.

## Research Loop

The CodeGenome Auto Research loop is a research and evaluation track, not a
product authority path.

It may evaluate:

- overlay usefulness;
- fusion weights and thresholds;
- propagation depth and attenuation;
- ownership-as-signal value;
- slice-planning hints;
- explanation quality;
- recall/precision tradeoffs.

Expected outputs:

- research run log;
- fitness comparison across overlay/fusion variants;
- recommended overlay set for alpha;
- recommended signal inputs for RecallPacket ranking;
- recommended GraphSlice planning inputs;
- failure and noise cases to exclude;
- implementation follow-up issues when warranted.

Research output may change recall defaults only after review. It must not
directly mutate Decisions, SourceEvidence, BindingEvidence, signoff,
compliance, no-conflict, or enforcement state.

## Implementation Slicing

This RFQ should produce implementation issues only after the design is merged.
Suggested slices:

1. Define typed recall signal and ranking-reason payloads for RecallPacket.
2. Add the recall-admissibility gate and negative fixtures.
3. Extend `GraphScopePlan` construction to consume admitted slice hints.
4. Add alpha recall-quality fixtures and metrics over the Git docs corpus.
5. Add optional CodeGenome-style overlay fixture provider for signal
   evaluation, not authority.

Issue #522 may proceed in parallel on candidate-promotion mechanics, typed
refs/refusals, and negative authority tests. It must not encode unresolved
ranking weights, fusion formulas, or Auto Research policy.

## Required Negative Fixtures

Implementation slices derived from this RFQ must prove:

- high ranking score cannot create or promote `Decision`;
- high ranking score cannot create or promote `DecisionCandidate`;
- high ranking score cannot create `SourceEvidence`;
- high ranking score cannot create `BindingEvidence`;
- ownership overlap cannot approve signoff;
- semantic similarity cannot assert compliance;
- CodeGenome fused confidence cannot prove no-conflict;
- stale or unscoped signals are rejected or downgraded;
- no-match from partial scope cannot claim global absence;
- `expand_scope` is offered when local unknown scope can be expanded;
- MCP cannot compute ranking/fusion locally beyond rendering daemon output.

## Non-Goals

- Do not define final production ranking weights.
- Do not implement hosted/team recall.
- Do not add autonomous correction, contradiction, or source-of-truth updates.
- Do not make embeddings, LLMs, CodeGenome confidence, or ownership overlap
  authority gates.
- Do not move graph, locator, recall, ranking, fusion, or binding authority
  into MCP.
