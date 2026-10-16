use bicameral_smr::{
    validate_claim_trace, AtomBinding, BindingMode, ClaimSemanticTraceV1, SemanticRelationKind,
    SourceCatalog, SourceSpan, SourceUnit, TraceLimits, TraceValidationError,
};

fn trace_fixture() -> ClaimSemanticTraceV1 {
    serde_json::from_str(include_str!("fixtures/valid-adjacent-states.json"))
        .expect("fixture follows the public ClaimTrace contract")
}

fn source_catalog() -> SourceCatalog {
    SourceCatalog::new([
        SourceUnit {
            source_unit_id: "UNIT-1".into(),
            span: SourceSpan {
                byte_start: 0,
                byte_end: 18,
                line_start: 1,
                line_end: 1,
            },
            content_digest: "sha256:unit-1".into(),
        },
        SourceUnit {
            source_unit_id: "UNIT-2".into(),
            span: SourceSpan {
                byte_start: 19,
                byte_end: 47,
                line_start: 2,
                line_end: 2,
            },
            content_digest: "sha256:unit-2".into(),
        },
    ])
    .expect("catalog is valid")
}

#[test]
fn valid_trace_preserves_adjacent_distinct_states() {
    let validated =
        validate_claim_trace(trace_fixture(), &source_catalog(), TraceLimits::default())
            .expect("trace is valid");

    assert_eq!(validated.trace.occurrences.len(), 3);
    assert_eq!(
        validated.trace.semantic_relations[0].relation,
        SemanticRelationKind::DistinctFrom
    );
    assert_eq!(
        validated
            .source_spans_for("REL-failed-is-not-queued")
            .expect("relation provenance"),
        &[
            SourceSpan {
                byte_start: 0,
                byte_end: 18,
                line_start: 1,
                line_end: 1,
            },
            SourceSpan {
                byte_start: 19,
                byte_end: 47,
                line_start: 2,
                line_end: 2,
            },
        ]
    );
}

#[test]
fn compatible_selector_rejects_an_unmarked_omitted_state() {
    let mut trace = trace_fixture();
    trace.assertions[0].binding_mode = BindingMode::CompatibleSelector;
    trace.assertions[0].post_state_occurrence_id = None;
    trace.assertions[0].unknown_dimensions = vec!["actor".into(), "target".into()];

    let error = validate_claim_trace(trace, &source_catalog(), TraceLimits::default())
        .expect_err("an omitted semantic dimension must be explicit");

    assert_eq!(
        error,
        TraceValidationError::UnmarkedUnknownDimension {
            semantic_id: "ASSERT-retry-transition".into(),
            dimension: "post_state".into(),
        }
    );
}

#[test]
fn unresolved_atom_binding_must_remain_a_typed_unknown() {
    let mut trace = trace_fixture();
    trace.occurrences[0].atom_binding = AtomBinding::Unresolved;

    let error = validate_claim_trace(trace, &source_catalog(), TraceLimits::default())
        .expect_err("unresolved alignment cannot become silent");

    assert_eq!(
        error,
        TraceValidationError::UnmarkedUnknownDimension {
            semantic_id: "STATE-failed".into(),
            dimension: "atom_binding".into(),
        }
    );
}

#[test]
fn explicitly_unknown_atom_binding_remains_visible() {
    let mut trace = trace_fixture();
    trace.occurrences[0].atom_binding = AtomBinding::Unresolved;
    trace.occurrences[0].unknown_dimensions = vec!["atom_binding".into()];

    let validated = validate_claim_trace(trace, &source_catalog(), TraceLimits::default())
        .expect("explicit unknowns are valid");
    let unresolved = validated
        .trace
        .occurrences
        .iter()
        .find(|occurrence| occurrence.occurrence_id == "STATE-failed")
        .expect("unresolved occurrence remains addressable");

    assert_eq!(unresolved.atom_binding, AtomBinding::Unresolved);
    assert_eq!(unresolved.unknown_dimensions, ["atom_binding"]);
}

#[test]
fn trace_digest_is_stable_across_nonsemantic_record_order() {
    let original = validate_claim_trace(trace_fixture(), &source_catalog(), TraceLimits::default())
        .expect("original trace is valid");
    let mut reordered_trace = trace_fixture();
    reordered_trace.occurrences.reverse();
    reordered_trace.assertions[0].source_unit_refs.reverse();
    reordered_trace.semantic_relations[0]
        .source_unit_refs
        .reverse();
    reordered_trace.coverage.source_unit_refs.reverse();
    let reordered =
        validate_claim_trace(reordered_trace, &source_catalog(), TraceLimits::default())
            .expect("reordered trace is valid");

    assert_eq!(
        reordered.trace.result_digest, original.trace.result_digest,
        "list order must not create a new semantic identity"
    );
}
