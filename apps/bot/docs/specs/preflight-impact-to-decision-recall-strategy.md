# Preflight Impact-to-Decision Recall Strategy

**Date:** 2026-07-30
**Status:** amended by the owner-confirmed two-lane boundary in
BicameralAI/bicameral-bot#967
**Level:** L2 bot-owned recall strategy
**Owns:** Read-only strategy for resolving planned work into relevant prior
Decisions before implementation.
**Related context:** BicameralAI/bicameral-bot#537,
BicameralAI/bicameral-bot#525, BicameralAI/bicameral-bot#530,
`docs/specs/constraint-lookup-recallpacket-contract.md`,
`docs/specs/preflight-search-readiness.md`,
`docs/specs/librarian-recall-readme.md`.

## Principle

Preflight impact recall may find, rank, explain, and request scope expansion. It
must not prove, approve, bind, sign off, enforce, assert compliance, assert
no-conflict, or mutate canonical state.

## Distinction

Binding accuracy answers whether Bicameral can prove a selected Decision/code
target relationship is valid. Preflight impact recall answers which prior
Decisions are likely relevant to planned work before implementation begins.

Binding accuracy belongs to the governed `binding.create` path and may
materialize accepted `BindingEvidence` only after deterministic verification.
Preflight impact recall is a read-model and retrieval strategy. It may rank,
explain, cite, disclose scope, and request expansion. It creates no `Decision`,
`DecisionCandidate`, `SourceEvidence`, `BindingEvidence`, compliance, signoff,
or enforcement state.

## High-Level Flow

```text
preflight request
  -> normalize task intent and declared scope
  -> extract ImpactSeeds
  -> project ImpactSurfaces
  -> run recall routes
  -> apply recall-admissibility gate
  -> merge and dedup candidates
  -> rank within policy-bounded tiers
  -> return RecallPacket plus RecallTrace
```

The alpha promise is high recall within declared searched scope, with honest
unknowns. It is not global completeness.

## Inputs

`ImpactSeed` is any input that hints at what work may be affected: task or
ticket text, planned action, files, diffs, symbols, modules, routes, packages,
APIs, database tables, deployment surfaces, branch refs, owner/team/project,
product area, source document, external event, or CodeGenome-style advisory
hint.

Each seed carries one role: `exact`, `declared`, `derived`, or `advisory`.
Caller hints are never authority.

## Projected Impact

`ImpactSurface` is a projected area that may be affected: file, symbol, module,
route, workflow, provider connector, projection profile, decision lifecycle,
binding lifecycle, external target, deployment path, or product area.

Each surface carries one status: `declared`, `exact`, `inferred`, `projected`,
`graph_supported`, `binding_supported`, `stale`, `unsupported`, or
`unknown_scope`.

Projected impact is a search strategy input, not evidence of implementation
impact.

For SMR alpha, artifact/context and product-area surfaces are sufficient to run
core trusted-corpus recall. File, symbol, graph-supported, binding-supported,
stale, and unsupported surfaces are optional expansion inputs; they must not
gate SMR recall or make its result an implementation claim.

## Recall Routes

Route A: accepted BindingEvidence backtrace.

- Reverse lookup accepted Decisions previously bound to exact or nearby impacted
  targets.
- Run as the independently release-validatable deterministic grounded-recall
  lane for pinned configured workspaces.
- Preserve exact GraphSnapshot, scope proof, BindingEvidence, Claim revision,
  ModelAtom, match reason, searched scope, limitation, and unknown coverage.
- Invoke no LLM and return without waiting for deep semantic analysis.
- Binding applicability does not establish implementation currentness,
  compliance, approval, safety, or SMR closure.

Route B: graph overlap and neighborhood expansion.

- Expand from projected surfaces through bounded graph neighborhoods.
- Record seed nodes, edge types, traversal depth, budget, skipped nodes, and
  stop reason.
- Return `expand_scope` instead of hiding insufficient graph scope.
- Treat unavailable, stale, or unsupported graph expansion as an optional-route
  limitation, not a core SMR `unknown_scope` result.

Route C: structured trusted-corpus search.

- Search paths, modules, source refs, owners, lifecycle tags, external targets,
  and controlled metadata over reviewed corpus/indexes.

Route D: semantic/textual reviewed-corpus search.

- Use semantic similarity only over reviewed corpus text.
- Semantic results may rank and explain, not assert compliance or no-conflict.

Route E: ownership, project, and source-overlap boosting.

- Boost candidates that share owner, project, source class, product area, or
  external target metadata.

Route F: CodeGenome-style advisory hints.

- Admit only fresh, explainable, scoped signals.
- Use them for expansion order and weak ranking support only.
- They cannot satisfy deterministic verification or create authority.

## Searched Scope Contract

Searched scope is the exact bounded set Bicameral actually searched. Unknown
scope is relevant area Bicameral knows it did not or could not search.

No `RecallPacket` may make a negative claim stronger than its searched scope
supports.

Allowed: `No relevant Decision found in searched scope.`

Forbidden: `No relevant Decision exists`, `No conflict found`, `This work is
clear`, and `No governing constraint applies`.

| Level | Meaning | Use |
| --- | --- | --- |
| 0 | unscoped / ambiguous | Typed refusal only. |
| 1 | declared scope | Routing/setup, not meaningful no-result confidence. |
| 2 | enumerated scope | Positive advisory recall candidates. |
| 3 | replayable scope | Required for "no relevant Decision found in searched scope." |
| 4 | audited scope | Required for governed disputes or later audit. |

## Candidate Shape

`DecisionRecallCandidate` minimum shape:

```text
decision_id
decision_status
routes_that_found_it
reason_codes
supporting_refs
searched_scope
unknown_scope
freshness/currentness
ranking_tier
rank_score_optional
caveats
suggested_actions
```

Candidates remain distinct from accepted Decisions and source-only correction or
contradiction candidates.

## Ranking

The deterministic resolver first assigns a causal tier, then scores inside that
tier.

Initial tier precedence:

1. Accepted BindingEvidence backtrace for an exact pinned code target.
2. Accepted Decision/source overlap from the trusted corpus.
3. High-confidence projected product area with searched-scope proof.
4. Ownership, project, or source overlap.
5. Semantic/textual similarity over reviewed corpus text.
6. Verified graph overlap without accepted binding evidence.
7. Fresh, explainable CodeGenome-style propagated hints.
8. Approximate candidates with lower or incomplete provenance.

Preferences may tune attention within policy-bounded caps. They may not create
authority, hide accepted binding overlap, waive caveats, or assert no-conflict.

## Deterministic Metadata Governance

Every recall-relevant controlled value must come from a governed taxonomy
registry: `DecisionType`, `LifecycleTag`, `CapabilityFamily`, `ExternalTarget`,
`AuthorityBoundary`, `AffectedSurface`, `RiskClass`, `ReasonCode`,
`ResultClass`, `ScopeLevel`, `FreshnessState`, `EvidenceState`,
`MetadataQuality`, `ApplicabilityStatus`, `SourceClass`, `RouteClass`, and
`PreferenceGuard`.

Metadata used for deterministic recall must be accepted, versioned, and
traceable. Auto-compiled metadata may participate only with metadata-quality
caveats.

## Preferences

`RecallPreferenceProfile` is a versioned input to scoring. Preferences may
weight attention, group or sort results, affect notification priority, and
personalize within policy bounds.

Preferences must not weaken governance constraints, create `BindingEvidence`,
approve signoff, assert compliance, assert no-conflict, hide accepted
deterministic evidence, or directly mutate hidden ranking behavior without
audit.

Policy caps beat preferences.

## RecallTrace

Every packet should include or reference a replay trace:

```text
trace_id
request_hash
normalized_input_hash
decision_index_version
metadata_schema_version
taxonomy_version
graph_snapshot_id optional
source_corpus_version
semantic_index_version optional
codegenome_signal_version optional
preference_profile_version
scoring_function_version
route_results
admitted_signals
downgraded_signals
rejected_signals
final_rank_tuple_per_result
stop_conditions
```

Every `RecallPacket` must be reproducible from its `RecallTrace` or explain why
replay is unavailable.

## Metadata Correction

Users need a governed way to correct bad recall metadata:

```text
metadata_correction_request
  -> review
  -> accepted metadata update
  -> DecisionIndex recompile
```

User feedback may request correction. It must not directly mutate hidden
ranking behavior.

## Evaluation

Required metrics: recall@k, precision@k, first relevant rank or MRR, critical
false-negative count, false-positive/noise count, honesty-label coverage,
deterministic replay, and preference boundary violations.

Required negative fixtures:

- high ranking score cannot create or promote `Decision`;
- impact projection cannot create `BindingEvidence`;
- semantic similarity cannot assert compliance;
- CodeGenome fused confidence cannot prove no-conflict;
- stale or unscoped signals are rejected or downgraded;
- no-match from partial scope cannot claim global absence;
- MCP cannot compute ranking/fusion locally beyond rendering daemon output;
- preferences cannot create authority, waive gates, or hide accepted evidence.

## Implementation Slices

1. Define taxonomy registry and controlled recall enums.
2. Define `DecisionIndexRecord` schema and metadata compiler.
3. Define `SearchedScope` and `RecallTrace` schemas.
4. Define `RecallPreferenceProfile` and safe weighting caps.
5. Define deterministic applicability resolver and scoring function v1.
6. Define reason-code catalog and explanation contract.
7. Define metadata correction workflow.
8. Build alpha fixtures and negative safety tests.
9. Implement metadata coverage read model.
10. Implement deterministic BindingEvidence backtrace and exact identity
    preservation.
11. Integrate independent grounded and deep results into the preflight
    `RecallPacket` surface.

## Acceptance Criteria

- Binding accuracy and preflight impact recall stay separate.
- Candidate generation routes and backtrace procedure are concrete enough for
  implementation slicing.
- Ranking precedence and reason codes are defined before tuned weights.
- Search scope, unknown scope, no-result semantics, and expand-scope behavior
  are explicit.
- Evaluation metrics and negative fixtures are named.
- User and workspace preferences tune attention only, not truth.
- Deterministic grounded recall invokes no LLM and does not wait for deep
  semantic analysis.
- Code `ScopeSufficiencyProof` and semantic `SmrCheckScopeAssessmentV1` remain
  independent.
