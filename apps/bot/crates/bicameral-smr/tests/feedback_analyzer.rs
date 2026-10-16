use std::fs;
use std::path::PathBuf;

use bicameral_smr::{
    analyze_feedback_snapshot_pair, load_reviewed_experiment_board, plan_feedback_check_pair,
    serialize_feedback_alloy_snapshots, ArtifactAuthorityV1, BoundedCheckOutcome,
    FeedbackAlloyAnalysisError, FeedbackAlloyAnalyzerConfig, FeedbackAssertionWitnessRequestV1,
    FeedbackBooleanPropositionV1, FeedbackCheckRoleV1, FeedbackVerdict,
};

const FEEDBACK_ASSERTION_ID: &str =
    "sha256:2a845782c09d6d315523d13b46d49d7f52df6d24fc70fc3ffcfdbe2e6f15b4e3";
const CLAIM_ASSERTION: &str = "assert.alpha.c04.proof-before-publication";

fn snapshot_pair() -> bicameral_smr::FeedbackAlloySnapshotPairV1 {
    let bytes = fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../protocol/smr-feedback-v0.1/fixtures")
            .join("product-creation-workspace-routing.governed-domain.fixture.json"),
    )
    .expect("reviewed fixture exists");
    let board = load_reviewed_experiment_board(&bytes).expect("board loads");
    let plan = plan_feedback_check_pair(
        &board,
        FeedbackAssertionWitnessRequestV1 {
            assertion_id: FEEDBACK_ASSERTION_ID.into(),
            accepted_claim_refs: vec!["claim.alpha.c04.proof-before-publication".into()],
            accepted_atom_refs: vec![
                "sha256:1ac519de8f41a804ae6cda22550354ed0e85bff8414d14cc95a2a921771094db".into(),
                "sha256:e0e1dd9261c3634445ba13dd5acf31ae76b9046ae042715f440a73176a263a36".into(),
                "sha256:67ad889c830f43dfd65e9f46fb958a58296e09446cdd85dff95aed950c4d2539".into(),
            ],
            accepted_relation_refs: vec!["rel.alpha.c04.proof-guards-cloud-publication".into()],
            source_anchor_refs: vec!["anchor.alpha.product-creation".into()],
        },
    )
    .expect("mapping is closed");
    serialize_feedback_alloy_snapshots(
        &board,
        &plan,
        FeedbackBooleanPropositionV1 {
            accepted_assertion_ref: CLAIM_ASSERTION.into(),
            asserted_value: true,
        },
    )
    .expect("snapshots serialize")
}

fn unavailable_config() -> FeedbackAlloyAnalyzerConfig {
    FeedbackAlloyAnalyzerConfig {
        java_executable: "/not-used".into(),
        distribution_jar: "/not-used".into(),
    }
}

#[test]
fn tampered_snapshot_fails_before_process_execution() {
    let mut pair = snapshot_pair();
    pair.assertion.alloy_module.push_str("\nfact Tampered {}\n");

    assert!(matches!(
        analyze_feedback_snapshot_pair(&pair, &unavailable_config()),
        Err(FeedbackAlloyAnalysisError::SnapshotDigestMismatch {
            role: FeedbackCheckRoleV1::Assertion,
            ..
        })
    ));
}

#[test]
fn mismatched_pair_fails_before_process_execution() {
    let mut pair = snapshot_pair();
    pair.negated_assertion.feedback_assertion_id = "sha256:other".into();

    assert_eq!(
        analyze_feedback_snapshot_pair(&pair, &unavailable_config()),
        Err(FeedbackAlloyAnalysisError::PairMismatch {
            field: "feedback_assertion_id"
        })
    );
}

#[test]
#[ignore = "requires pinned Java 17 and Alloy 6.2.0"]
fn reviewed_assertion_pair_produces_an_entailed_bounded_evaluation() {
    let java_executable = std::env::var_os("BICAMERAL_JAVA17_EXECUTABLE")
        .map(PathBuf::from)
        .expect("set BICAMERAL_JAVA17_EXECUTABLE");
    let distribution_jar = std::env::var_os("BICAMERAL_ALLOY_DISTRIBUTION_JAR")
        .map(PathBuf::from)
        .expect("set BICAMERAL_ALLOY_DISTRIBUTION_JAR");
    let result = analyze_feedback_snapshot_pair(
        &snapshot_pair(),
        &FeedbackAlloyAnalyzerConfig {
            java_executable,
            distribution_jar,
        },
    )
    .expect("both exact checks execute");

    assert_eq!(result.authority, ArtifactAuthorityV1::BoundedEvidence);
    assert_eq!(result.verdict, FeedbackVerdict::Entailed);
    assert_eq!(
        result.assertion_check.outcome,
        BoundedCheckOutcome::Possible
    );
    assert_eq!(
        result.negated_assertion_check.outcome,
        BoundedCheckOutcome::Impossible
    );
    assert_eq!(result.scope_completeness, "partial");
    assert!(!result.coverage_limitations.is_empty());
    assert_eq!(result.baseline_merged_commit.len(), 40);
    assert_eq!(result.assertion_check.snapshot_id.len(), 86);
    assert!(result
        .assertion_check
        .analyzer_receipt
        .java_version
        .contains("17"));
    assert!(result
        .assertion_check
        .analyzer_receipt
        .alloy_version
        .starts_with("6.2.0"));
    assert_ne!(
        result.assertion_check.snapshot_digest,
        result.negated_assertion_check.snapshot_digest
    );
}
