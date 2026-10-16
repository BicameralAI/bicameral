use bicameral_smr::{
    analyze_alloy_snapshot, lower_claim_trace, serialize_alloy_snapshot, validate_claim_trace,
    AlloyAnalysisError, AlloyAnalysisStatus, AlloyAnalyzerConfig, AlloyCapacity, AlloyCommand,
    AlloySerializationError, ArtifactAuthorityV1, ClaimSemanticTraceV1, CoverageStatus,
    LoweringStatus, SemanticRelationKind, SemanticRelationV1, SourceCatalog, SourceSpan,
    SourceUnit, TraceLimits,
};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

fn trace_fixture() -> ClaimSemanticTraceV1 {
    serde_json::from_str(include_str!("fixtures/valid-adjacent-states.json"))
        .expect("fixture follows ClaimTrace contract")
}

fn lower(trace: ClaimSemanticTraceV1) -> bicameral_smr::LoweredClaimTraceV1 {
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
    let validated =
        validate_claim_trace(trace, &catalog, TraceLimits::default()).expect("trace is valid");
    lower_claim_trace(&validated)
}

#[test]
fn runtime_snapshot_has_exact_bounds_and_reverse_provenance() {
    let lowered = lower(trace_fixture());
    let snapshot = serialize_alloy_snapshot(&lowered, AlloyCapacity::alpha_default())
        .expect("complete supported hypergraph can be serialized");

    assert_eq!(snapshot.exact_cardinalities.model_atoms, 3);
    assert_eq!(snapshot.exact_cardinalities.claim_assertions, 1);
    assert_eq!(snapshot.exact_cardinalities.atom_relations, 1);

    let expected_ids = lowered
        .hypergraph
        .nodes
        .iter()
        .map(|node| node.node_id.as_str())
        .chain(
            lowered
                .hypergraph
                .assertions
                .iter()
                .map(|assertion| assertion.assertion_id.as_str()),
        )
        .chain(
            lowered
                .hypergraph
                .relations
                .iter()
                .map(|relation| relation.relation_id.as_str()),
        )
        .collect::<BTreeSet<_>>();
    let represented_ids = snapshot
        .provenance
        .iter()
        .map(|entry| entry.semantic_id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(represented_ids, expected_ids);
    assert_eq!(snapshot.provenance.len(), expected_ids.len());

    assert!(snapshot.alloy_module.contains("ModelAtom = N_"));
    assert!(snapshot.alloy_module.contains("ClaimAssertion = A_"));
    assert!(snapshot.alloy_module.contains("AtomRelation = R_"));
    assert!(snapshot
        .alloy_module
        .contains("run SnapshotInstance for exactly"));
    assert!(snapshot
        .alloy_module
        .contains("check NoSemanticIdentityConflict for exactly"));
    assert!(snapshot.alloy_module.contains("open smr_meta_model"));
    assert!(snapshot.alloy_module.contains("open smr_checks"));
    assert!(!snapshot
        .alloy_module
        .contains("assert NoSemanticIdentityConflict"));
    assert!(snapshot.meta_model_digest.starts_with("sha256:"));
    assert!(snapshot.checks_digest.starts_with("sha256:"));
    assert!(snapshot.check_evidence_map.is_empty());

    let failed_state = snapshot
        .provenance
        .iter()
        .find(|entry| entry.semantic_id == "STATE-failed")
        .expect("state has reverse provenance");
    assert!(failed_state.alloy_symbol.starts_with("N_"));
    assert_eq!(
        failed_state.source_spans,
        [SourceSpan {
            byte_start: 0,
            byte_end: 18,
            line_start: 1,
            line_end: 1,
        }]
    );
}

#[test]
fn partial_coverage_stops_before_alloy_serialization() {
    let mut trace = trace_fixture();
    trace.coverage.status = CoverageStatus::Partial;
    trace.coverage.unknown_dimensions = vec!["cancel_branch".into()];

    let error = serialize_alloy_snapshot(&lower(trace), AlloyCapacity::alpha_default())
        .expect_err("partial coverage cannot become a bounded all-clear");

    assert_eq!(
        error,
        AlloySerializationError::IncompleteLowering {
            semantic_id: "coverage".into(),
            status: LoweringStatus::LoweredWithUnknowns,
        }
    );
}

#[test]
fn capacity_is_never_raised_or_truncated_silently() {
    let error = serialize_alloy_snapshot(
        &lower(trace_fixture()),
        AlloyCapacity {
            max_model_atoms: 2,
            max_claim_assertions: 5,
            max_atom_relations: 5,
        },
    )
    .expect_err("the exact snapshot does not fit");

    assert_eq!(
        error,
        AlloySerializationError::CapacityExceeded {
            collection: "model_atoms",
            actual: 3,
            maximum: 2,
        }
    );
}

#[test]
#[ignore = "requires the pinned Alloy 6.2.0 distribution"]
fn generated_snapshot_executes_with_pinned_alloy() {
    let executable = std::env::var_os("BICAMERAL_ALLOY_EXECUTABLE")
        .map(PathBuf::from)
        .expect("set BICAMERAL_ALLOY_EXECUTABLE");
    let distribution_jar = std::env::var_os("BICAMERAL_ALLOY_DISTRIBUTION_JAR")
        .map(PathBuf::from)
        .expect("set BICAMERAL_ALLOY_DISTRIBUTION_JAR");
    let snapshot =
        serialize_alloy_snapshot(&lower(trace_fixture()), AlloyCapacity::alpha_default())
            .expect("snapshot serializes");

    let result = analyze_alloy_snapshot(
        &snapshot,
        &AlloyAnalyzerConfig {
            command: AlloyCommand::SnapshotInstance,
            executable,
            distribution_jar,
            expected_distribution_sha256:
                "6b8c1cb5bc93bedfc7c61435c4e1ab6e688a242dc702a394628d9a9801edb78d".into(),
            expected_version_prefix: "6.2.0".into(),
            solver: "SAT4J".into(),
            timeout: Duration::from_secs(15),
        },
    )
    .expect("generated model executes");

    assert_eq!(result.status, AlloyAnalysisStatus::InstanceFound);
    assert_eq!(result.authority, ArtifactAuthorityV1::BoundedEvidence);
    assert!(result.analyzer_receipt.version.starts_with("6.2.0"));
    assert_eq!(result.analyzer_receipt.solver, "SAT4J");
    assert_eq!(
        result.analyzer_receipt.distribution_sha256,
        "6b8c1cb5bc93bedfc7c61435c4e1ab6e688a242dc702a394628d9a9801edb78d"
    );
}

#[test]
fn analyzer_rejects_a_tampered_generated_snapshot_before_execution() {
    let mut snapshot =
        serialize_alloy_snapshot(&lower(trace_fixture()), AlloyCapacity::alpha_default())
            .expect("snapshot serializes");
    snapshot.alloy_module.push_str("\nfact Tampered {}\n");

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
    .expect_err("snapshot digest must bind the exact generated module");

    assert!(matches!(
        error,
        AlloyAnalysisError::SnapshotDigestMismatch { .. }
    ));
}

#[test]
fn analyzer_rejects_tampered_reverse_provenance_before_execution() {
    let mut snapshot =
        serialize_alloy_snapshot(&lower(trace_fixture()), AlloyCapacity::alpha_default())
            .expect("snapshot serializes");
    snapshot.provenance[0].semantic_id = "laundered-semantic-id".into();

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
    .expect_err("snapshot digest must bind reverse provenance");

    assert!(matches!(
        error,
        AlloyAnalysisError::SnapshotDigestMismatch { .. }
    ));
}

#[test]
#[ignore = "requires the pinned Alloy 6.2.0 distribution"]
fn stable_identity_check_finds_and_explains_a_source_bound_counterexample() {
    let executable = std::env::var_os("BICAMERAL_ALLOY_EXECUTABLE")
        .map(PathBuf::from)
        .expect("set BICAMERAL_ALLOY_EXECUTABLE");
    let distribution_jar = std::env::var_os("BICAMERAL_ALLOY_DISTRIBUTION_JAR")
        .map(PathBuf::from)
        .expect("set BICAMERAL_ALLOY_DISTRIBUTION_JAR");
    let config = |command| AlloyAnalyzerConfig {
        command,
        executable: executable.clone(),
        distribution_jar: distribution_jar.clone(),
        expected_distribution_sha256:
            "6b8c1cb5bc93bedfc7c61435c4e1ab6e688a242dc702a394628d9a9801edb78d".into(),
        expected_version_prefix: "6.2.0".into(),
        solver: "SAT4J".into(),
        timeout: Duration::from_secs(15),
    };

    let clean_snapshot =
        serialize_alloy_snapshot(&lower(trace_fixture()), AlloyCapacity::alpha_default())
            .expect("clean snapshot serializes");
    let clean_result = analyze_alloy_snapshot(
        &clean_snapshot,
        &config(AlloyCommand::NoSemanticIdentityConflict),
    )
    .expect("stable check executes");
    assert_eq!(
        clean_result.status,
        AlloyAnalysisStatus::NoCounterexampleWithinScope
    );
    assert!(clean_result.findings.is_empty());

    let mut conflicting_trace = trace_fixture();
    conflicting_trace
        .semantic_relations
        .push(SemanticRelationV1 {
            relation_id: "REL-failed-is-queued".into(),
            left_occurrence_id: "STATE-failed".into(),
            right_occurrence_id: "STATE-retry-queued".into(),
            relation: SemanticRelationKind::EquivalentTo,
            source_unit_refs: vec!["UNIT-2".into()],
        });
    let conflicting_snapshot =
        serialize_alloy_snapshot(&lower(conflicting_trace), AlloyCapacity::alpha_default())
            .expect("conflicting snapshot remains exactly representable");
    assert_eq!(conflicting_snapshot.check_evidence_map.len(), 1);

    let result = analyze_alloy_snapshot(
        &conflicting_snapshot,
        &config(AlloyCommand::NoSemanticIdentityConflict),
    )
    .expect("stable check executes");

    assert_eq!(result.status, AlloyAnalysisStatus::CounterexampleFound);
    assert_eq!(result.command, AlloyCommand::NoSemanticIdentityConflict);
    assert_eq!(result.findings.len(), 1);
    assert_eq!(
        result.findings[0].semantic_ids,
        ["REL-failed-is-not-queued", "REL-failed-is-queued"]
    );
    assert_eq!(
        result.findings[0].source_spans,
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
}
