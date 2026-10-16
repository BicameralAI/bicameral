# Runtime Architecture and Algorithm Spec — SMR Alpha Claim Semantic Trace ↔ Alloy Bridge

## Metadata

| Field | Value |
|---|---|
| Date | 2026-07-29 |
| Status | Draft for implementation |
| Level | L1 repository architecture and local runtime contract |
| Owns | The source-grounded natural-language-to-SMR-hypergraph-to-Alloy bridge and typed reverse provenance used by Managed Preflight |
| Canonical runtime contract | [`smr-alpha-claim-semantic-trace-alloy-bridge.contract.json`](smr-alpha-claim-semantic-trace-alloy-bridge.contract.json) |
| Governing design | [`smr-alpha-managed-preflight-technical-design.md`](smr-alpha-managed-preflight-technical-design.md) |
| Minimal honest harness | [`smr-alpha-minimal-honest-test-harness.md`](smr-alpha-minimal-honest-test-harness.md) |
| Governing journeys | [`smr-alpha-managed-preflight-user-journeys.md`](smr-alpha-managed-preflight-user-journeys.md) |
| Existing SMR runtime | [`state-machine-resolver-v1-runtime-architecture.md`](state-machine-resolver-v1-runtime-architecture.md) |
| Governance gate | `bic:spec-governance-gate`; Qor advisory/no-op |
| UserJourney | `developer_host_mcp_smr_managed_preflight_v1` |

## Reading guide

Product **why** (5-minute bet): [Why SMR and Spec Governance](../strategy/why-smr-and-spec-governance.md).

Read **Decision**, **Alpha algorithm**, and **Acceptance criteria** first for the technical bridge.

Main invariant:

> The LLM proposes a source-grounded candidate hypergraph. A deterministic
> serializer projects the complete supported alpha-domain hypergraphs into one
> stable hand-written Alloy model. Neither projection nor analysis can change
> accepted Product truth.

## 1. Problem

Impact-atom extraction can retrieve the right semantic neighborhood while
erasing the difference that constitutes drift:

```text
Reviewer recommends promotion.
Reviewer approves promotion.
Product Owner approves promotion.
```

The atoms overlap. Actor/action/authority relationships do not.

Direct natural-language-to-Alloy generation is also unsafe. A probabilistic
provider would implicitly choose quantifiers, closure, facts, transition
effects, and frame conditions.

SMR therefore needs:

1. a source-grounded semantic representation rich enough to preserve the Plan;
2. alignment with the accepted SMR hypergraph;
3. bounded formal analysis without making the LLM an Alloy author.

## 2. Decision

Alpha uses a complete exact hypergraph projection, not intelligent slicing.

```text
Exact Plan
    |
    v
ClaimSemanticTraceV1
  source-grounded proposed occurrences and assertions
    |
    v
Candidate Plan hypergraph
  same typed semantic vocabulary as accepted SMR
    |
    +--------------------------+
    |                          |
    v                          v
Accepted SMR hypergraph      Claim alignment
  pinned alpha domain         per-assertion relationship
    |                          |
    +------------+-------------+
                 v
CandidatePromotionAlloySnapshotV1
  complete exact accepted + proposed alpha-domain hypergraphs
                 |
                 v
stable hand-written smr-meta-model.als + smr-checks.als
                 |
                 v
bounded Alloy result
                 |
                 v
finding mapped to hypergraph IDs, accepted Claims, and Plan spans
```

Alpha has no generic Claim-family compiler and no template-specific graph
slicer.

The stable Alloy model understands the fixed alpha vocabulary. The only
generated input is an exact serialization of typed hypergraph records plus the
selected check and bounds.

## 3. Goals

- Preserve actor, action, target, modality, polarity, quantifier, states,
  guards, effects, order, branches, exceptions, alternatives, and unknowns.
- Bind every proposed assertion to exact Plan spans.
- Represent the Plan as a candidate hypergraph compatible with accepted SMR
  concepts without making it accepted.
- Serialize the complete supported Candidate Promotion domain into Alloy.
- Pin exact accepted-model, trace, serializer, Alloy model, analyzer, and scope
  identities.
- Prevent Alloy from inventing extra snapshot records through exact bounds.
- Run stable authorization, lifecycle, invariant, precedence, and reachability
  checks.
- Map findings back through stable IDs.
- Keep all bridge behavior read-only.
- Make the full projection the reference oracle for any future slicer.

## 4. Non-goals

- General natural-language formalization.
- LLM-authored Alloy.
- Generic hypergraph slicing or a generic Claim-to-Alloy compiler in alpha.
- Unbounded proof or universal safety.
- Complex liveness, fairness, deadlines, or probabilistic behavior.
- CodeGraph, CodeLocator, graph refresh, or code compliance in alpha.
- Automatic Candidate Promotion.
- Making Alloy the canonical Product store.

## 5. Why the hypergraph can be represented in Alloy

Alloy supports finite sets and n-ary relations. The SMR hypergraph can be
projected directly.

### 5.1 Reified representation

The alpha prefers reified hyperedges because provenance must remain
independently addressable:

```alloy
sig ModelAtom {}

sig Claim {
  assertions: some ClaimAssertion
}

sig ClaimAssertion {
  atom: one ModelAtom,
  kind: one AssertionKind,
  value: one SemanticValue
}

sig AtomRelation {
  source: one ModelAtom,
  relationType: one RelationType,
  target: one ModelAtom
}
```

The stable model also represents Plan occurrences, assertion membership,
alignment candidates, accepted/candidate authority class, and provenance IDs.

### 5.2 Responsibilities

The SMR hypergraph owns:

- stable semantic identity;
- Claim-to-assertion membership;
- accepted provenance and authority;
- persistence and replay;
- ordinary retrieval.

Alloy owns:

- finite relational instances;
- bounded traces;
- satisfiability and unsat checks;
- counterexamples;
- reachability within explicit scope.

Alloy is an analysis snapshot, not the database.

### 5.3 Structural closure versus Product completeness

The serializer emits every supported record in the pinned Candidate Promotion
domain and uses exact bounds. Alloy therefore sees exactly that snapshot.

This does not mean the Product is completely specified.

```text
No Reviewer authorization record exists in the snapshot
```

does not imply:

```text
Reviewers are forbidden
```

unless an accepted Claim closes that dimension, such as:

```text
Only Product Owners may approve.
```

Missing Product closure remains `unknown_scope`.

## 6. Alpha product cut

The bounded microdomain is Candidate Promotion:

```text
States:
  Proposed
  Accepted
  Rejected

Actors:
  Reviewer
  Product Owner

Actions:
  Recommend
  Approve
  Reject
```

Supported semantics:

1. authorization and prohibition;
2. lifecycle source, guard, effect, and destination;
3. invariants and cardinality;
4. simple precedence and mutually exclusive branches;
5. bounded reachability.

Stable checks:

1. accepted-Claims-plus-Plan satisfiability;
2. unauthorized transition;
3. invalid or skipped lifecycle transition;
4. intended-outcome reachability.

The versioned `alloy-alpha-v1` policy uses no more than five atoms per
top-level signature by default and one through eight represented steps. Every
result exposes its actual bounds, solver, timeout, and model version.

The complete supported snapshot must fit the configured capacity. If it does
not, the serializer returns `capacity_exceeded` before Alloy runs. It must not
truncate records, create an implicit slice, or silently increase the bound.

## 7. Claim Semantic Trace

`ClaimSemanticTraceV1` is a source-grounded, noncanonical candidate hypergraph:

```text
ClaimSemanticTraceV1 {
  trace_id
  source_binding
  accepted_model_base
  extraction_profile: ExtractionProfileV1

  occurrences[] {
    occurrence_id
    kind: actor | role | action | target | state | condition | value
    source_unit_refs[]
    source_spans[]              # validator-resolved; absent from provider output
    atom_binding:
      exact_ref(atom_id)
      | proposed_identity(normalized_identity)
      | alternatives(candidate_refs[])
      | unresolved
    unknown_dimensions[]
  }

  assertions[] {
    assertion_id
    source_unit_refs[]
    source_spans[]              # validator-resolved; absent from provider output
    actor_occurrence_id?
    action_occurrence_id
    target_occurrence_id?
    binding_mode: exact_tuple | compatible_selector
    modality: asserted | permitted | required | prohibited | conditional | unknown
    polarity: positive | negative | unknown
    quantifier: all | some | one | lone | none | unspecified
    pre_state_occurrence_id?
    post_state_occurrence_id?
    guard_occurrence_ids[]
    effect_occurrence_ids[]
    exception_assertion_ids[]
    unknown_dimensions[]
  }

  edges[] {
    edge_id
    from_assertion_id
    to_assertion_id
    relation:
      precedes | requires | enables | causes_transition
      | conditional_on | alternative_to | excludes
    source_unit_refs[]
    source_spans[]              # validator-resolved; absent from provider output
  }

  semantic_relations[] {
    relation_id
    left_occurrence_id
    right_occurrence_id
    relation: equivalent_to | distinct_from
    source_unit_refs[]
    source_spans[]              # validator-resolved; absent from provider output
  }

  coverage
  result_digest
}
```

`impact_atoms` are derived from occurrences for retrieval. They are not the
semantic source of truth.

### 7.1 Alpha extraction profile

`smr-claim-trace-alpha-v1` is the required alpha task profile:

```text
ExtractionProfileV1 {
  profile_id: smr-claim-trace-alpha-v1
  placement: caller | local
  provider_id
  model_id
  prompt_template_id: smr-claim-trace-alpha-v1
  output_schema_id: ClaimSemanticTraceV1
  decoding_profile_id: strict-structured-output-v1
  provider_config_digest
}
```

The profile keeps the existing SMR provider boundary: a caller LLM or
configured local `ClaimExtractionProvider` may execute it. Hosted execution
and silent fallback are disabled for alpha.

There is deliberately no product-global vendor model. Every analysis binds the
exact provider and model that actually ran. `default`, `latest`, missing model
identity, or a provider that cannot report its model returns
`unsupported_extraction_profile` before projection. The strict decoding
profile requires schema-constrained output, one candidate hypergraph, no model
tools, and sampling disabled when the provider exposes that control. Any
provider-fixed or unsupported decoding control is recorded in the receipt.

The normalized profile participates in the analysis key. Changing the model,
prompt, schema, decoding profile, provider configuration, or placement creates
a new identity and cannot reuse the prior trace.

## 8. Alpha algorithm

### Step 1 — Exact source units

Validate the Plan source identity and digest, then deterministically assign
immutable IDs to retained source units such as changed ranges. The provider
sees the exact bytes, their IDs, and pinned accepted vocabulary. Source-unit
construction is clerical provenance framing, not semantic slicing. Missing
source returns `incomplete`.

### Step 2 — Candidate hypergraph projection

The specialized provider:

- segments propositions rather than sentences;
- resolves cross-sentence references;
- emits occurrences and assertions;
- preserves partial order and branches;
- emits alternative atom mappings;
- records unknown dimensions;
- binds every assertion and semantic relation to supplied `source_unit_refs`.

It cannot emit Alloy.

The provider may leave an unstated transition dimension unknown. It must not
invent a post-state merely to satisfy a full tuple. It emits explicit
`equivalent_to` or `distinct_from` semantic relations when the Plan collapses
or separates adjacent states.

### Step 3 — Deterministic validation

Validate the extraction profile, schema, source-unit IDs, references, enums,
branch structure, size bounds, and digest. Resolve valid `source_unit_refs` to
exact source bytes, character/line spans, and digests. Invalid output is
rejected, not repaired; the validator never repairs semantic content.

### Step 4 — Accepted Claim alignment

For every assertion:

- retrieve exact/related accepted ModelAtoms;
- reverse-join to accepted ClaimAssertions and Claims;
- compare dimensions;
- retain alternatives and unknowns;
- classify independently:

```text
equivalent | narrows | broadens | contradicts
| overlaps | novel | ambiguous | unresolved
```

A `compatible_selector` is lowered only when its stated dimensions and
quantifier define a meaningful selector. The aligner returns the exact set of
accepted transitions compatible with those stated dimensions while retaining
every unknown dimension. It never fills an omitted value. No compatible
transition is `unresolved`; incompatible alternatives are `ambiguous_scope`.
Semantic relations are aligned separately so an adjacent-state collapse
cannot disappear into one selected transition.

`exact_tuple` requires every dimension required by its assertion kind.
`compatible_selector` requires every omitted dimension to appear in
`unknown_dimensions`; accidental omission is `trace_invalid`.

### Step 5 — Complete alpha-domain projection

The serializer reads:

- every supported accepted `ModelAtom`;
- every supported accepted `ClaimAssertion`;
- every parent Claim and membership hyperedge;
- every supported `AtomRelation`;
- acceptance and source provenance identifiers;
- every validated proposed Plan occurrence, assertion, edge, and semantic
  relation;
- alignment relations;
- explicit closure Claims.

It emits a structurally isomorphic `CandidatePromotionAlloySnapshotV1`.
Nothing is selected by embedding similarity, graph distance, or another LLM.

### Step 6 — Exact Alloy serialization

The serializer deterministically maps every snapshot record to:

- a singleton or bounded atom;
- a reified Claim/Assertion/Relation record;
- an exact relation tuple;
- stable symbol names derived from SMR/trace IDs;
- a provenance table.

Exact scopes and relation equalities prevent extra snapshot records.

### Step 7 — Stable checks

The hand-written `smr-checks.als` applies stable semantics for:

- `authorizes`;
- `prohibits`;
- `actor_of`;
- `action_of`;
- `target_of`;
- `source_state_of`;
- `destination_state_of`;
- `guards`;
- `effects`;
- `precedes`;
- `alternative_to`.

The LLM does not select or author these meanings.

### Step 8 — Bounded execution

Run the chosen fixed check with explicit bounds and resource limits.

### Step 9 — Reverse provenance

Map:

```text
Alloy tuple
  -> snapshot record
  -> ClaimAssertion or Plan assertion
  -> accepted Claim and source
  -> exact Plan span
```

Render the semantic difference, bounded finding, scope, unknowns, and next
action.

## 9. Stable and generated artifacts

Stable, hand-written:

```text
smr-claim-trace-alpha-v1 prompt template
ClaimSemanticTraceV1 output schema
smr-meta-model.als
smr-checks.als
```

Generated or constructed through the Alloy API:

```text
snapshot
  exact accepted + proposed hypergraph records

analysis request
  named check + exact bounds
```

The generated snapshot is data, not a new semantic program.

Cache identity:

```text
accepted_model_base
accepted_alpha_domain_digest
extraction_profile_digest
trace_digest
alignment_digest
serializer_version
meta_model_digest
checks_digest
analyzer_distribution_digest
solver
exact_scope
```

Identical inputs reuse the snapshot and result. A changed Plan, accepted alpha
domain, model/check version, analyzer, or scope creates a new identity.

Future implementations may supply exact Kodkod bounds directly instead of
emitting textual snapshot declarations. The semantics and provenance contract
stay the same.

## 10. Runtime contracts

### 10.1 `CandidatePromotionAlloySnapshotV1`

```text
CandidatePromotionAlloySnapshotV1 {
  snapshot_id
  accepted_model_base
  accepted_domain_digest
  trace_digest
  alignment_digest
  serializer_version
  meta_model_digest
  checks_digest

  model_atoms[]
  claims[]
  claim_assertions[]
  claim_memberships[]
  atom_relations[]
  plan_occurrences[]
  plan_assertions[]
  plan_edges[]
  plan_semantic_relations[]
  alignments[]
  closure_assertions[]
  provenance[]

  exact_cardinalities
  result_digest
}
```

### 10.2 `AlloyAnalysisResultV1`

```text
AlloyAnalysisResultV1 {
  analysis_id
  snapshot_digest
  check_id
  status:
    counterexample_found
    | instance_found
    | unsat_within_scope
    | no_counterexample_within_scope
    | unknown_scope
    | capacity_exceeded
    | timed_out
    | failed
  findings[]
  exact_scope
  analyzer_receipt
  warnings[]
  result_digest
}
```

### 10.3 `SmrAlphaResultV2` fail-closed envelope

```text
SmrAlphaResultV2 {
  source_identity
  accepted_model_base
  extraction_profile_digest
  status:
    bounded_result
    | trace_invalid
    | ambiguous_scope
    | unknown_scope
    | unsupported_extraction_profile
    | capacity_exceeded
    | analyzer_unavailable
  trace_digest?
  snapshot_digest?
  assertion_results[]
  accepted_claims[]
  candidates[]
  alloy_findings[]
  coverage
  exact_scope?
  unknowns[]
  result_digest
}
```

`trace_invalid`, `ambiguous_scope`, `unknown_scope`, and
`unsupported_extraction_profile` stop before snapshot serialization and Alloy.
Their absent `snapshot_digest` and `exact_scope` are meaningful; the renderer
must not synthesize a no-counterexample outcome.

### 10.4 Public language

- `counterexample_found`: one bounded counterexample exists.
- `unsat_within_scope`: no instance satisfies the exact snapshot and selected
  scenario within the stated bounds.
- `no_counterexample_within_scope`: the check found none within the stated
  bounds.
- none means globally safe, compliant, correct, or complete.

## 11. Deontic boundary

| Natural language | Alpha interpretation |
|---|---|
| `A may X` | explicit authorization; not eventual occurrence |
| `Only A may X` | accepted closure plus authorization restriction |
| `A must X` | unsupported unless resolved to invariant, guard, effect, or simple precedence |
| `A must not X` | prohibition when exact |
| `After X, Y may occur` | precedence plus permission |
| `X requires Y` | guard/invariant, not automatically temporal succession |
| `Unless E` | explicit exception; never dropped |

Alloy temporal modality is not governance modality.

## 12. Authority and persistence

| Artifact | Classification | Authority |
|---|---|---|
| Accepted SMR hypergraph | Canonical state projection | Replayed from canonical claim-bearing events |
| Exact Plan | Evidence | Source-bound local input |
| Claim Semantic Trace | Derived candidate hypergraph | None |
| Alignment | Derived read model | None |
| Alloy snapshot | Derived projection | None |
| Alloy result | Bounded evidence | None |
| Preflight | Derived read model | None |
| DecisionCandidate | Local candidate | Becomes accepted only through separate promotion |

Preflight never writes accepted state. Alloy findings may inform Candidate
Promotion review but cannot authorize it.

## 13. State refinement and minimization

Product lifecycle states added: none.

Derived artifacts added:

- Claim Semantic Trace;
- alignment;
- exact Alloy snapshot;
- bounded Alloy result.

Rejected Product states:

- `trace_projected`;
- `alloy_snapshot_ready`;
- `alloy_checked`;
- `counterexample_found`;
- `preflight_clear`.

Artifact existence and typed result status are sufficient. None has distinct
Product transition authority or canonical persistence.

Invariants:

- trace is not accepted Claim;
- exact atom match is not semantic equivalence;
- exact snapshot is not complete Product truth;
- Alloy result is not Product state;
- no bounded counterexample is not an all-clear;
- Candidate Promotion remains the only accepted append boundary;
- CodeGraph is absent from alpha.

## 14. Failure, retry, and recovery

| Condition | Required behavior | Forbidden fallback |
|---|---|---|
| Exact Plan unavailable | Incomplete | Transcript scraping |
| Extraction profile omitted, aliased, or unreportable | `unsupported_extraction_profile`; no provider call | Use `default`, `latest`, or a silent fallback |
| Provider unavailable/invalid or empty consequential trace | `trace_invalid` or `unknown_scope`; no Alloy | Impact-atom bag or unchanged accepted snapshot presented as coverage |
| Ambiguous consequential mapping | Preserve alternatives and return `ambiguous_scope`; no Alloy | Pick top score |
| Unsupported semantics | Visible unknown | Invent Alloy tuple |
| Accepted base changes | New snapshot identity | Mix bases |
| Complete snapshot exceeds configured capacity | Typed `capacity_exceeded` | Truncate, slice, or silently raise scope |
| Serializer mismatch | Reject snapshot | Analyze stale data |
| Alloy unavailable/timeout | Typed unavailable/timeout | Claim no gap |
| Retry/restart | Reattach by digest | Duplicate trace/candidate |

## 15. Security and privacy

- Plans remain local by default.
- Remote provider egress requires explicit configuration.
- Provider output is untrusted and bounded.
- Analyzer receives stable generated identifiers, not raw Plan text or
  canonical event-store credentials.
- Stable Alloy files are reviewed source.
- Snapshot data is content-addressed and sanitized.
- Analyzer version, solver, bounds, timeout, and memory are pinned.

## 16. Runtime architecture and conformance

The canonical component/state/relationship contract is the JSON companion.
The implementation must satisfy:

- source-grounded trace provenance;
- retrieval/comparison/authority separation;
- complete exact alpha-domain snapshot serialization;
- stable hand-written model and checks;
- bounded analyzer execution;
- no canonical mutation;
- reverse provenance;
- retry/restart identity.

No intelligent slicing conformance requirement exists in alpha.

## 17. Validation

The executable experiment design is
[`smr-alpha-minimal-honest-test-harness.md`](smr-alpha-minimal-honest-test-harness.md).
It uses a version-bound Factory proposal for the model input, a held-out
accepted baseline and answer key, paired oracle-trace and model-trace paths,
and the fixture-pinned Alloy 6.2.0 distribution.

### Semantic mutations

```text
Product Owner -> Reviewer
recommend -> approve
may -> must
before -> after
Proposed -> Accepted
some -> all
one -> many
guard present -> absent
exception present -> absent
```

### Required checks

- complete accepted-domain record count matches exact Alloy snapshot count;
- every hypergraph ID round-trips through Alloy symbol provenance;
- exact bounds prevent extra records;
- mixed Plans preserve assertion-level outcomes;
- authorization and lifecycle mutations produce expected findings or visible
  unknowns;
- accepted-store digest is unchanged;
- retries and restarts reuse identities.

Primary safety metric:

> False-safe rate: consequential semantic mutation is erased or mistranslated
> so preflight reports no visible gap or limitation.

The curated critical fixture suite permits zero known false-safe outcomes.

## 18. Implementation sequencing

1. Publish trace, snapshot, analysis, and result contracts plus adversarial
   fixtures.
2. Run the minimal honest model-to-trace-to-Alloy harness against the pinned
   Factory proposal.
3. Implement trace provider and validator.
4. Implement assertion-level Claim alignment.
5. Hand-write and review `smr-meta-model.als` and `smr-checks.als`.
6. Implement complete Candidate Promotion hypergraph serializer.
7. Integrate pinned bounded Alloy analyzer.
8. Implement reverse provenance and preflight rendering.
9. Run retry/restart, authority, benchmark, and authentic-host validation.

CodeGraph has no alpha slice.

## 19. Future extension

### Slicing

When the full Product hypergraph becomes too large:

1. introduce a template-specific, provenance-preserving slicer;
2. keep the complete projection as a reference implementation;
3. compare sliced and complete results on bounded fixtures;
4. require the slicer to preserve closure, provenance, and finding behavior.

Slicing is an optimization, not a new semantic authority.

### Broader Alloy semantics

- nested conditions and exceptions;
- concurrency;
- liveness and fairness with explicit assumptions;
- temporal intervals and deadlines;
- cross-Claim refinement;
- counterexample-guided repair candidates;
- direct Kodkod bounds and incremental solving.

### Post-alpha CodeGraph

CodeGraph may later supply snapshot-pinned `EvidenceObservation`s. It does not
replace the Claim Semantic Trace, create accepted semantics, or imply code
compliance.

## 20. ADR implications

- ADR-0004 reaffirmed: extraction, formal evidence, and authority remain
  separate.
- ADR-0014 reaffirmed: the required trace profile is probabilistic enrichment
  and never an authority boundary.
- The merged SMR journey and runtime contracts remain authoritative for local
  candidate staging and Candidate Promotion; this design introduces no
  separate candidate-persistence decision.
- ADR-0029 records complete alpha hypergraph projection into a stable
  hand-written Alloy model and defers slicing as a scale optimization.

## 21. Acceptance criteria

- Every successful projection returns a valid source-grounded candidate
  hypergraph.
- The Plan representation preserves assertion relationships, partial
  selectors, state-identity relations, alternatives, and unknowns.
- Invalid, empty, or consequentially ambiguous traces stop before Alloy with a
  typed incomplete result.
- Every supported accepted and proposed alpha-domain record appears exactly
  once in the Alloy snapshot.
- Stable checks—not generated semantics—interpret relation types.
- Exact bounds prevent extra snapshot records.
- No LLM output contains executable Alloy.
- Every finding maps to snapshot record, ClaimAssertion/Plan assertion,
  accepted Claim, and Plan span.
- Ambiguity and missing closure remain visible.
- No result uses safe, compliant, verified, or clear language.
- Bridge operations leave accepted Product state unchanged.
- Identical inputs reuse the same snapshot and analysis identity.
- CodeGraph is absent from alpha.
- Critical fixtures have zero known false-safe outcomes.

## 22. Journey Boundary Review

```yaml
journey_boundary_review:
  status: passed
  journey_ids:
    - developer_host_mcp_smr_managed_preflight_v1
  authoritative_commit_points:
    - planning_source_evidence_ingest
    - local_candidate_stage
  independently_observable_facts:
    - exact_plan_snapshot
    - validated_claim_semantic_trace
    - accepted_and_proposed_alloy_snapshot
    - bounded_alloy_result
    - read_only_preflight_result
  partial_failure_reviewed: true
  retry_and_idempotency_reviewed: true
  validation_witness_separated_from_product_state: true
  user_confirmed: true
  source_ref: https://github.com/BicameralAI/bicameral-bot/blob/faa26c3c88ef8d8acc598cf0542917a7759f27cd/docs/specs/smr-alpha-managed-preflight-user-journeys.md#journey-boundary-review
```

The bridge introduces no additional user journey or Product state. Its
terminal host evidence uses the governing design's canonical
`developer_host_mcp_v1` environment selection.

## 23. Resolved alpha choices and remaining measurement gate

- Analyzer: Alloy 6.2.0 distribution JAR, SAT4J, Java 17, distribution
  SHA-256
  `6b8c1cb5bc93bedfc7c61435c4e1ab6e688a242dc702a394628d9a9801edb78d`.
- Extractor: `smr-claim-trace-alpha-v1`, with caller/local placement and an
  exact per-analysis provider and model identity; no product-global vendor
  model and no silent fallback.
- Snapshot form: textual exact snapshot declarations for alpha
  inspectability; direct Alloy/Kodkod bounds remain a compatible future
  implementation.
- Remaining gate: set the efficacy and latency threshold after the corrected
  representation runs at least three repetitions per controlled case and adds
  full-Plan cases. The current pilot is directional evidence only.

## 24. Request coverage

| Request | Covered by |
|---|---|
| NL ↔ Alloy bridge algorithm | Sections 7–10 |
| Hypergraph/Alloy relationship | Section 5 |
| Absolutely necessary alpha cut | Sections 6, 17, 18, 21 |
| Stable script versus regeneration | Section 9 |
| Current cut and future extension | Sections 6 and 19 |
| CodeGraph after alpha | Sections 4, 18, 19, 21 |
