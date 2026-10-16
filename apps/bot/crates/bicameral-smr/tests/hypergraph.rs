use bicameral_smr::{
    lower_claim_trace, validate_claim_trace, ClaimSemanticTraceV1, CoverageStatus,
    HypergraphRelationKind, LoweringStatus, SourceCatalog, SourceSpan, SourceUnit, TraceLimits,
};

fn trace_fixture() -> ClaimSemanticTraceV1 {
    serde_json::from_str(include_str!("fixtures/valid-adjacent-states.json"))
        .expect("fixture follows ClaimTrace contract")
}

fn validate(trace: ClaimSemanticTraceV1) -> bicameral_smr::ValidatedClaimTrace {
    let trace: ClaimSemanticTraceV1 = trace;
    let catalog = SourceCatalog::new([
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
    .expect("catalog is valid");
    validate_claim_trace(trace, &catalog, TraceLimits::default()).expect("trace is valid")
}

#[test]
fn lowering_preserves_state_identity_transition_and_provenance() {
    let validated = validate(trace_fixture());
    let lowered = lower_claim_trace(&validated);

    assert_eq!(
        lowered
            .hypergraph
            .nodes
            .iter()
            .map(|node| node.node_id.as_str())
            .collect::<Vec<_>>(),
        ["ACTION-retry", "STATE-failed", "STATE-retry-queued"]
    );

    let transition = &lowered.hypergraph.assertions[0];
    assert_eq!(
        transition.pre_state_node_id.as_deref(),
        Some("STATE-failed")
    );
    assert_eq!(
        transition.post_state_node_id.as_deref(),
        Some("STATE-retry-queued")
    );

    let distinction = lowered
        .hypergraph
        .relations
        .iter()
        .find(|relation| relation.relation_id == "REL-failed-is-not-queued")
        .expect("semantic distinction was lowered");
    assert_eq!(distinction.relation, HypergraphRelationKind::DistinctFrom);
    assert_eq!(
        distinction.source_spans,
        [
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

    assert!(lowered
        .loss_manifest
        .iter()
        .all(|entry| entry.status == LoweringStatus::Lowered));
    assert_eq!(
        lowered.hypergraph.source_trace_digest,
        validated.trace.result_digest
    );
}

#[test]
fn partial_coverage_is_explicit_in_the_loss_manifest() {
    let mut trace = trace_fixture();
    trace.coverage.status = CoverageStatus::Partial;
    trace.coverage.unknown_dimensions = vec!["cancel_branch".into()];

    let lowered = lower_claim_trace(&validate(trace));
    let coverage = lowered
        .loss_manifest
        .iter()
        .find(|entry| entry.semantic_id == "coverage")
        .expect("coverage must participate in loss accounting");

    assert_eq!(coverage.status, LoweringStatus::LoweredWithUnknowns);
    assert!(coverage
        .reason
        .as_deref()
        .expect("partial coverage needs a reason")
        .contains("cancel_branch"));
}
