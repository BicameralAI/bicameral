use std::fs;
use std::path::PathBuf;
use std::process::Command;

use bicameral_smr::{
    load_reviewed_experiment_board, plan_feedback_check_pair, serialize_feedback_alloy_snapshots,
    FeedbackAlloySerializationError, FeedbackAssertionWitnessRequestV1,
    FeedbackBooleanPropositionV1, FeedbackCheckRoleV1,
};

const FEEDBACK_ASSERTION_ID: &str =
    "sha256:2a845782c09d6d315523d13b46d49d7f52df6d24fc70fc3ffcfdbe2e6f15b4e3";
const CLAIM: &str = "claim.alpha.c04.proof-before-publication";
const CLAIM_ASSERTION: &str = "assert.alpha.c04.proof-before-publication";
const ASSERTED_ATOM: &str =
    "sha256:1ac519de8f41a804ae6cda22550354ed0e85bff8414d14cc95a2a921771094db";
const FROM_ATOM: &str = "sha256:e0e1dd9261c3634445ba13dd5acf31ae76b9046ae042715f440a73176a263a36";
const TO_ATOM: &str = "sha256:67ad889c830f43dfd65e9f46fb958a58296e09446cdd85dff95aed950c4d2539";
const RELATION: &str = "rel.alpha.c04.proof-guards-cloud-publication";
const ANCHOR: &str = "anchor.alpha.product-creation";

fn fixture_bytes() -> Vec<u8> {
    fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../protocol/smr-feedback-v0.1/fixtures")
            .join("product-creation-workspace-routing.governed-domain.fixture.json"),
    )
    .expect("reviewed fixture exists")
}

fn board_and_plan() -> (
    bicameral_smr::ReviewedExperimentBoardV1,
    bicameral_smr::FeedbackCheckPairPlanV1,
) {
    let board = load_reviewed_experiment_board(&fixture_bytes()).expect("board loads");
    let plan = plan_feedback_check_pair(
        &board,
        FeedbackAssertionWitnessRequestV1 {
            assertion_id: FEEDBACK_ASSERTION_ID.into(),
            accepted_claim_refs: vec![CLAIM.into()],
            accepted_atom_refs: vec![ASSERTED_ATOM.into(), FROM_ATOM.into(), TO_ATOM.into()],
            accepted_relation_refs: vec![RELATION.into()],
            source_anchor_refs: vec![ANCHOR.into()],
        },
    )
    .expect("mapping is closed");
    (board, plan)
}

#[test]
fn exact_reviewed_board_serializes_distinct_assertion_and_negation_snapshots() {
    let (board, plan) = board_and_plan();
    let pair = serialize_feedback_alloy_snapshots(
        &board,
        &plan,
        FeedbackBooleanPropositionV1 {
            accepted_assertion_ref: CLAIM_ASSERTION.into(),
            asserted_value: true,
        },
    )
    .expect("supported proposition serializes");

    assert_eq!(pair.assertion.check_role, FeedbackCheckRoleV1::Assertion);
    assert_eq!(
        pair.negated_assertion.check_role,
        FeedbackCheckRoleV1::NegatedAssertion
    );
    assert!(pair.assertion.proposition_value);
    assert!(!pair.negated_assertion.proposition_value);
    assert_ne!(
        pair.assertion.result_digest,
        pair.negated_assertion.result_digest
    );
    assert!(pair.assertion.meta_model_digest.starts_with("sha256:"));
    assert!(pair.assertion.checks_digest.starts_with("sha256:"));
    assert_eq!(pair.assertion.exact_cardinalities.model_atoms, 27);
    assert_eq!(pair.assertion.exact_cardinalities.claim_assertions, 12);
    assert_eq!(pair.assertion.exact_cardinalities.atom_relations, 7);
    assert_eq!(pair.assertion.scope_completeness, "partial");
    assert!(!pair.assertion.coverage_limitations.is_empty());
    assert!(pair.assertion.alloy_module.contains("TrueValue"));
    assert!(pair
        .negated_assertion
        .alloy_module
        .contains("ProposedAssertion.truth = FalseValue"));
    for relation_kind in ["Precedes", "Guards", "Triggers", "Produces", "PartOf"] {
        assert!(pair.assertion.alloy_module.contains(relation_kind));
    }
    assert_eq!(
        pair.assertion
            .alloy_module
            .matches("extends ModelAtom")
            .count(),
        27
    );
    assert_eq!(
        pair.assertion
            .alloy_module
            .matches("extends AcceptedAssertion")
            .count(),
        12
    );
    assert_eq!(
        pair.assertion
            .alloy_module
            .matches("extends AtomRelation")
            .count(),
        7
    );
}

#[test]
fn proposition_must_be_inside_the_source_grounded_witness() {
    let (board, plan) = board_and_plan();
    let result = serialize_feedback_alloy_snapshots(
        &board,
        &plan,
        FeedbackBooleanPropositionV1 {
            accepted_assertion_ref: "assert.alpha.c03.adapter-owns-events".into(),
            asserted_value: true,
        },
    );

    assert_eq!(
        result,
        Err(FeedbackAlloySerializationError::PropositionOutsideWitness(
            "assert.alpha.c03.adapter-owns-events".into()
        ))
    );
}

#[test]
fn stale_plan_identity_fails_before_snapshot_construction() {
    let (board, mut plan) = board_and_plan();
    plan.baseline_claim_set_digest = "sha256:stale".into();

    assert_eq!(
        serialize_feedback_alloy_snapshots(
            &board,
            &plan,
            FeedbackBooleanPropositionV1 {
                accepted_assertion_ref: CLAIM_ASSERTION.into(),
                asserted_value: true,
            },
        ),
        Err(FeedbackAlloySerializationError::PlanIdentityMismatch {
            field: "baseline_claim_set_digest"
        })
    );
}

#[test]
#[ignore = "requires the pinned Alloy 6.2.0 distribution"]
fn reviewed_assertion_pair_executes_with_pinned_alloy() {
    let executable = std::env::var_os("BICAMERAL_ALLOY_EXECUTABLE")
        .map(PathBuf::from)
        .expect("set BICAMERAL_ALLOY_EXECUTABLE");
    let (board, plan) = board_and_plan();
    let pair = serialize_feedback_alloy_snapshots(
        &board,
        &plan,
        FeedbackBooleanPropositionV1 {
            accepted_assertion_ref: CLAIM_ASSERTION.into(),
            asserted_value: true,
        },
    )
    .expect("supported proposition serializes");

    for (name, snapshot, expect_instance) in [
        ("assertion", pair.assertion, true),
        ("negated", pair.negated_assertion, false),
    ] {
        let directory = tempfile::tempdir().expect("temporary Alloy directory");
        fs::write(
            directory.path().join("smr_feedback_meta_model.als"),
            include_str!("../alloy/smr_feedback_meta_model.als"),
        )
        .expect("write meta-model");
        fs::write(
            directory.path().join("smr_feedback_checks.als"),
            include_str!("../alloy/smr_feedback_checks.als"),
        )
        .expect("write checks");
        let snapshot_path = directory.path().join("snapshot.als");
        fs::write(&snapshot_path, snapshot.alloy_module).expect("write snapshot");
        let output = Command::new(&executable)
            .args([
                "exec",
                "-q",
                "-c",
                "FeedbackCompatibility",
                "-t",
                "json",
                "-o",
                "-",
                "-s",
                "SAT4J",
            ])
            .arg(snapshot_path)
            .output()
            .expect("execute Alloy");
        assert!(
            output.status.success(),
            "{name} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            output.stdout.iter().any(|byte| !byte.is_ascii_whitespace()),
            expect_instance,
            "unexpected {name} satisfiability"
        );
    }
}
