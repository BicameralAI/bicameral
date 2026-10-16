use std::fs;
use std::path::PathBuf;

use bicameral_smr::{
    evaluate_feedback_proposition, load_reviewed_experiment_board, FeedbackAlloyAnalyzerConfig,
    FeedbackAssertionWitnessRequestV1, FeedbackBooleanPropositionV1, FeedbackEvaluationAttemptV1,
    FeedbackUnevaluatedReasonV1,
};

const FEEDBACK_ASSERTION_ID: &str =
    "sha256:2a845782c09d6d315523d13b46d49d7f52df6d24fc70fc3ffcfdbe2e6f15b4e3";

fn inputs() -> (
    bicameral_smr::ReviewedExperimentBoardV1,
    FeedbackAssertionWitnessRequestV1,
    FeedbackBooleanPropositionV1,
) {
    let bytes = fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../protocol/smr-feedback-v0.1/fixtures")
            .join("product-creation-workspace-routing.governed-domain.fixture.json"),
    )
    .expect("reviewed fixture exists");
    let board = load_reviewed_experiment_board(&bytes).expect("board loads");
    let request = FeedbackAssertionWitnessRequestV1 {
        assertion_id: FEEDBACK_ASSERTION_ID.into(),
        accepted_claim_refs: vec!["claim.alpha.c04.proof-before-publication".into()],
        accepted_atom_refs: vec![
            "sha256:1ac519de8f41a804ae6cda22550354ed0e85bff8414d14cc95a2a921771094db".into(),
            "sha256:e0e1dd9261c3634445ba13dd5acf31ae76b9046ae042715f440a73176a263a36".into(),
            "sha256:67ad889c830f43dfd65e9f46fb958a58296e09446cdd85dff95aed950c4d2539".into(),
        ],
        accepted_relation_refs: vec!["rel.alpha.c04.proof-guards-cloud-publication".into()],
        source_anchor_refs: vec!["anchor.alpha.product-creation".into()],
    };
    let proposition = FeedbackBooleanPropositionV1 {
        accepted_assertion_ref: "assert.alpha.c04.proof-before-publication".into(),
        asserted_value: true,
    };
    (board, request, proposition)
}

fn unavailable_analyzer() -> FeedbackAlloyAnalyzerConfig {
    FeedbackAlloyAnalyzerConfig {
        java_executable: "/not-used".into(),
        distribution_jar: "/not-used".into(),
    }
}

#[test]
fn incomplete_mapping_is_typed_indeterminate_without_a_verdict() {
    let (board, mut request, proposition) = inputs();
    request.accepted_relation_refs.clear();

    match evaluate_feedback_proposition(&board, request, proposition, &unavailable_analyzer()) {
        FeedbackEvaluationAttemptV1::Indeterminate { unevaluated } => {
            assert_eq!(
                unevaluated.reason,
                FeedbackUnevaluatedReasonV1::MappingUnavailable
            );
            assert_eq!(unevaluated.scope_completeness, "partial");
            assert!(!unevaluated.coverage_limitations.is_empty());
        }
        other => panic!("expected indeterminate, got {other:?}"),
    }
}

#[test]
fn absent_analyzer_is_typed_unavailable_without_a_verdict() {
    let (board, request, proposition) = inputs();

    match evaluate_feedback_proposition(&board, request, proposition, &unavailable_analyzer()) {
        FeedbackEvaluationAttemptV1::Unavailable { unevaluated } => assert_eq!(
            unevaluated.reason,
            FeedbackUnevaluatedReasonV1::AnalyzerUnavailable
        ),
        other => panic!("expected unavailable, got {other:?}"),
    }
}

#[test]
#[ignore = "requires pinned Java 17 and Alloy 6.2.0"]
fn exact_attempt_returns_bounded_result_only_after_both_checks() {
    let (board, request, proposition) = inputs();
    let analyzer = FeedbackAlloyAnalyzerConfig {
        java_executable: std::env::var_os("BICAMERAL_JAVA17_EXECUTABLE")
            .map(PathBuf::from)
            .expect("set BICAMERAL_JAVA17_EXECUTABLE"),
        distribution_jar: std::env::var_os("BICAMERAL_ALLOY_DISTRIBUTION_JAR")
            .map(PathBuf::from)
            .expect("set BICAMERAL_ALLOY_DISTRIBUTION_JAR"),
    };

    match evaluate_feedback_proposition(&board, request, proposition, &analyzer) {
        FeedbackEvaluationAttemptV1::BoundedResult { evaluation } => {
            assert_eq!(evaluation.verdict, bicameral_smr::FeedbackVerdict::Entailed);
            assert_ne!(
                evaluation.assertion_check.check_id,
                evaluation.negated_assertion_check.check_id
            );
        }
        other => panic!("expected bounded result, got {other:?}"),
    }
}
