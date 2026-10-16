use bicameral_smr::{
    analyze_alloy_snapshot, lower_claim_trace, serialize_alloy_snapshot, validate_claim_trace,
    AlloyAnalysisError, AlloyAnalyzerConfig, AlloyCapacity, AlloyCommand, ArtifactAuthorityV1,
    AtomBinding, ClaimAssertionV1, ClaimEdgeKind, ClaimEdgeV1, ClaimOccurrenceV1,
    ClaimSemanticTraceV1, Modality, OccurrenceKind, Polarity, Quantifier, SourceCatalog,
    SourceSpan, SourceUnit, TraceLimits,
};
use std::path::PathBuf;
use std::time::Duration;

fn trace_fixture() -> ClaimSemanticTraceV1 {
    serde_json::from_str(include_str!("fixtures/valid-adjacent-states.json"))
        .expect("fixture follows ClaimTrace contract")
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

fn lower(trace: ClaimSemanticTraceV1) -> bicameral_smr::LoweredClaimTraceV1 {
    let validated = validate_claim_trace(trace, &source_catalog(), TraceLimits::default())
        .expect("trace is valid");
    lower_claim_trace(&validated)
}

#[test]
fn happy_path_linearization_cannot_drop_alternative_or_exception_structure() {
    let mut trace = trace_fixture();
    trace.occurrences.push(ClaimOccurrenceV1 {
        occurrence_id: "ACTION-cancel".into(),
        kind: OccurrenceKind::Action,
        source_unit_refs: vec!["UNIT-2".into()],
        atom_binding: AtomBinding::ProposedIdentity {
            normalized_identity: "cancel".into(),
        },
        unknown_dimensions: vec![],
    });
    trace.assertions.push(ClaimAssertionV1 {
        assertion_id: "ASSERT-cancel-branch".into(),
        source_unit_refs: vec!["UNIT-2".into()],
        actor_occurrence_id: None,
        action_occurrence_id: "ACTION-cancel".into(),
        target_occurrence_id: None,
        binding_mode: bicameral_smr::BindingMode::ExactTuple,
        modality: Modality::Permitted,
        polarity: Polarity::Positive,
        quantifier: Quantifier::One,
        pre_state_occurrence_id: Some("STATE-failed".into()),
        post_state_occurrence_id: Some("STATE-failed".into()),
        guard_occurrence_ids: vec![],
        effect_occurrence_ids: vec![],
        exception_assertion_ids: vec![],
        unknown_dimensions: vec![],
    });
    trace.assertions[0].exception_assertion_ids = vec!["ASSERT-cancel-branch".into()];
    trace.edges.push(ClaimEdgeV1 {
        edge_id: "EDGE-retry-or-cancel".into(),
        from_assertion_id: "ASSERT-retry-transition".into(),
        to_assertion_id: "ASSERT-cancel-branch".into(),
        relation: ClaimEdgeKind::AlternativeTo,
        source_unit_refs: vec!["UNIT-2".into()],
    });

    let lowered = lower(trace);
    let snapshot = serialize_alloy_snapshot(&lowered, AlloyCapacity::alpha_default())
        .expect("complete branch structure must serialize exactly");
    let retry_assertion = lowered
        .hypergraph
        .assertions
        .iter()
        .find(|assertion| assertion.assertion_id == "ASSERT-retry-transition")
        .expect("retry branch exists");

    assert_eq!(
        retry_assertion.exception_assertion_ids,
        ["ASSERT-cancel-branch"]
    );
    assert!(snapshot.alloy_module.contains(".exceptionAssertions = A_"));
    assert!(snapshot
        .alloy_module
        .contains(".relationKind = AlternativeTo"));
}

#[test]
fn derived_artifacts_cannot_be_laundered_into_product_authority() {
    let lowered = lower(trace_fixture());
    assert_eq!(
        lowered.hypergraph.authority,
        ArtifactAuthorityV1::NonCanonicalCandidate
    );
    let mut snapshot = serialize_alloy_snapshot(&lowered, AlloyCapacity::alpha_default())
        .expect("snapshot serializes");
    assert_eq!(
        snapshot.authority,
        ArtifactAuthorityV1::NonCanonicalProjection
    );

    snapshot.authority = ArtifactAuthorityV1::BoundedEvidence;
    let error = analyze_alloy_snapshot(
        &snapshot,
        &AlloyAnalyzerConfig {
            command: AlloyCommand::SnapshotInstance,
            executable: PathBuf::from("/not-used"),
            distribution_jar: PathBuf::from("/not-used"),
            expected_distribution_sha256: "not-used".into(),
            expected_version_prefix: "not-used".into(),
            solver: "SAT4J".into(),
            timeout: Duration::from_secs(1),
        },
    )
    .expect_err("authority classification participates in snapshot identity");
    assert!(matches!(
        error,
        AlloyAnalysisError::SnapshotDigestMismatch { .. }
    ));
}

#[test]
fn host_specific_temporal_unknowns_stop_before_alloy() {
    let mut trace = trace_fixture();
    trace.assertions[0]
        .unknown_dimensions
        .push("focus_settling_and_confirmation_order".into());

    let error = serialize_alloy_snapshot(&lower(trace), AlloyCapacity::alpha_default())
        .expect_err("unsupported host temporal semantics cannot be modeled as a generic success");

    assert!(matches!(
        error,
        bicameral_smr::AlloySerializationError::IncompleteLowering { .. }
    ));
}

#[test]
fn generic_profile_substitution_is_rejected() {
    let mut trace = trace_fixture();
    trace.extraction_profile.profile_id = "generic-harness-v1".into();

    let error = validate_claim_trace(trace, &source_catalog(), TraceLimits::default())
        .expect_err("alpha semantics require the exact reviewed interaction contract");

    assert!(matches!(
        error,
        bicameral_smr::TraceValidationError::UnsupportedExtractionProfile { .. }
    ));
}
