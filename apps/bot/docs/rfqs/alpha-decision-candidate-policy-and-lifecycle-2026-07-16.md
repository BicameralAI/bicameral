# RFQ: Alpha Decision-Candidate Policy and Lifecycle

Date: 2026-07-16
Status: open for Kevin Knapp and Jin Kuan review
Owner: Bicameral Bot
Related: merged PR #325, `docs/specs/shared-observation-candidate-pipeline.md`, `docs/specs/factory-alpha-librarian-lifecycle-delta.md`, `docs/specs/alpha-recall-multi-signal-fusion.md`

## Purpose

Resolve the remaining alpha policy choices after the shared observation-to-candidate runtime architecture was established. This RFQ does not reopen Bot ownership of projection, convergence, review routing, or persistent decision authority.

The July 16 huddle narrowed the open work to four coupled decisions:

1. deterministic bot-side filtration and exclusion policy;
2. candidate metadata and weighting semantics;
3. transient-candidate lifecycle and decay;
4. retrieval and routing to MCP-facing users.

## Fixed constraints

- GitHub issues are the first internal dogfooding source.
- Integrations and external producers provide screened evidence and advisory hints, not authoritative candidates.
- Valid evidence capture must survive projection or provider failure.
- Filtering is heuristic-first and fail-open. Unknown input remains eligible unless an explicit exclusion rule matches.
- LLM assistance is acceptable for alpha candidate projection and retrieval.
- Ratified Decisions, code grounding, provenance, signoff, and canonical event state remain deterministic.
- Candidate ranking, decay, or retrieval must never mutate, demote, hide, or weaken a ratified Decision.
- Every stage must expose inspectable inputs, outputs, reasons, policy version, and provenance.

## RFQ A: Bot-side filtration and exclusion

### Question

Which deterministic rules should run after evidence normalization and before optional model processing?

### Required rule classes

- provider boilerplate and machine-generated noise;
- duplicate delivery or replay-safe duplicate observation;
- empty, malformed, or content-free records;
- source-specific non-decision events;
- explicit security, privacy, or unsupported-content exclusions;
- configurable source ceilings and workspace exclusions.

### Required behavior

Each record must produce one of:

- `eligible`;
- `excluded`;
- `quarantined`;
- `deferred`.

Each outcome must include rule identifier, reason, configuration version, source reference, and evidence reference. No record may disappear silently. A rule miss yields `eligible`.

### Decision requested

Select the initial GitHub-issue ruleset, configuration shape, rule ordering, and override mechanism. Determine whether source trust tier can only affect review priority or may also cap eligible projection lanes.

## RFQ B: Candidate metadata and weighting

### Question

Which metadata is captured as stable source/provenance state, and which values are derived ranking inputs?

### Stable metadata candidates

- source type and stable source identifier;
- source timestamp and ingest timestamp;
- repository, issue, pull request, commit, and external references;
- contributor and participant identifiers where permitted;
- participant roles where known;
- delivery mode and verification state;
- evidence and projection provenance;
- extraction profile and policy version;
- candidate lifecycle and review state.

### Derived weighting candidates

- source reliability posture;
- contributor diversity;
- recency;
- explicit decision language;
- issue or thread participation;
- linkage to active code or product scope;
- contradiction or novelty signal;
- prior human interaction;
- retrieval task relevance.

### Options

1. deterministic weighted score only;
2. deterministic baseline plus LLM enrichment;
3. constrained LLM score over deterministic features;
4. graph-native relevance plus semantic reranking;
5. bounded LLM selection over a deterministic shortlist.

### Decision requested

Choose the alpha option, persistence strategy for calculated scores, replay behavior when policy changes, and explanation contract for each score contribution.

## RFQ C: Transient candidate lifecycle

### Proposed lifecycle

```text
captured
  -> filtration_eligible | excluded | quarantined
  -> projection_requested
  -> projected | no_candidate | projection_deferred | projection_failed
  -> convergence_pending
  -> materialized | duplicate | conflict_flagged
  -> review_pending
  -> ratified | rejected | expired | superseded_candidate
```

These are candidate and processing states, not new canonical Decision states.

### Decay question

Unpromoted candidates may lose retrieval priority over time, but decay must be a derived ranking function rather than destructive state mutation.

Potential inputs:

- source-specific half-life;
- candidate class;
- review state;
- new corroborating evidence;
- unresolved conflict;
- active repository or feature relevance;
- explicit human pinning.

### Required invariants

- raw evidence and accepted events remain replayable;
- decay never deletes evidence or candidates required for audit;
- rejected candidates remain available as governed learning evidence;
- ratification exits the transient lifecycle and enters existing persistent Decision semantics;
- policy changes permit recomputation without source re-ingestion.

### Decision requested

Choose the alpha state names, terminal-state retention, half-life defaults, pinning behavior, and recomputation trigger.

## RFQ D: Retrieval and MCP routing

### Question

How should alpha retrieval surface provisional candidates without presenting them as ratified truth?

### Options

- vector retrieval over shaped candidate records;
- graph traversal with semantic reranking;
- hybrid lexical, graph, and vector retrieval;
- deterministic shortlist followed by bounded LLM selection;
- query-time summaries versus precomputed candidate summaries.

### Required response contract

Every surfaced candidate must include:

- candidate identifier and lifecycle state;
- source and evidence references;
- retrieval rationale or score factors;
- freshness or decay state;
- uncertainty and limitations;
- explicit provisional status;
- allowed next human action.

Retrieval must support abstention and `unknown_scope`. It cannot ratify, reject, alter, or promote a candidate.

### Decision requested

Choose the alpha retrieval mechanism, shortlist boundary, model role, response shape, and separation from deterministic persistent-decision preflight.

## Modular alpha validation

Each stage must be independently testable:

1. GitHub issue acquisition fixture enters normalized evidence.
2. Filtration produces an inspectable outcome.
3. Projection produces a normalized result or typed deferral.
4. Weighting produces replayable score factors.
5. Lifecycle projection produces a derived candidate state.
6. Retrieval returns or abstains with provenance.
7. Ratification crosses into the existing deterministic Decision lifecycle.

## Evaluation

Candidate pipeline evaluation uses labeled real samples and reports:

- filtration precision and recall;
- candidate extraction precision and recall;
- retrieval precision at K and recall at K;
- provenance correctness;
- duplicate and conflict-classification accuracy;
- abstention quality;
- model and token cost per accepted candidate.

These metrics are separate from persistent code-grounding, drift-detection, and binding accuracy.

## Requested decisions

Kevin and Jin should record one answer for each RFQ section, including:

- selected alpha option;
- rejected alternatives and reason;
- implementation owner;
- schema or contract owner;
- required fixtures;
- post-alpha optimization explicitly deferred.
