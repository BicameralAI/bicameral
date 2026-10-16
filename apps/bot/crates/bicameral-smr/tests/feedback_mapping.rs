use std::fs;
use std::path::PathBuf;

use bicameral_smr::{
    load_reviewed_experiment_board, plan_feedback_check_pair, FeedbackAssertionWitnessRequestV1,
    FeedbackBaselineAuthorityV1, FeedbackCheckRoleV1, FeedbackMappingError,
    REVIEWED_BOARD_CLAIM_SET_DIGEST, REVIEWED_BOARD_FILE_SHA256, REVIEWED_BOARD_HEAD_COMMIT,
    REVIEWED_BOARD_MERGED_COMMIT,
};

const FEEDBACK_ASSERTION_ID: &str =
    "sha256:2a845782c09d6d315523d13b46d49d7f52df6d24fc70fc3ffcfdbe2e6f15b4e3";
const CLAIM: &str = "claim.alpha.c04.proof-before-publication";
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

fn request() -> FeedbackAssertionWitnessRequestV1 {
    FeedbackAssertionWitnessRequestV1 {
        assertion_id: FEEDBACK_ASSERTION_ID.into(),
        accepted_claim_refs: vec![CLAIM.into()],
        accepted_atom_refs: vec![TO_ATOM.into(), ASSERTED_ATOM.into(), FROM_ATOM.into()],
        accepted_relation_refs: vec![RELATION.into()],
        source_anchor_refs: vec![ANCHOR.into()],
    }
}

#[test]
fn exact_source_grounded_mapping_plans_both_required_checks() {
    let board = load_reviewed_experiment_board(&fixture_bytes()).expect("board loads");
    let plan = plan_feedback_check_pair(&board, request()).expect("mapping is closed");

    assert_eq!(plan.assertion_id, FEEDBACK_ASSERTION_ID);
    assert_eq!(
        plan.baseline_fixture_file_sha256,
        REVIEWED_BOARD_FILE_SHA256
    );
    assert_eq!(plan.baseline_head_commit, REVIEWED_BOARD_HEAD_COMMIT);
    assert_eq!(plan.baseline_merged_commit, REVIEWED_BOARD_MERGED_COMMIT);
    assert_eq!(
        plan.baseline_claim_set_digest,
        REVIEWED_BOARD_CLAIM_SET_DIGEST
    );
    assert_eq!(
        plan.baseline_authority,
        FeedbackBaselineAuthorityV1::NonAuthoritativeExperiment
    );
    assert_eq!(plan.scope_completeness, "partial");
    assert!(!plan.coverage_limitations.is_empty());
    assert_eq!(
        plan.checks,
        [
            FeedbackCheckRoleV1::Assertion,
            FeedbackCheckRoleV1::NegatedAssertion
        ]
    );
    assert_eq!(
        plan.accepted_atom_refs,
        [ASSERTED_ATOM, TO_ATOM, FROM_ATOM]
            .map(str::to_string)
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
    );
}

#[test]
fn missing_relation_endpoint_is_typed_before_model_construction() {
    let board = load_reviewed_experiment_board(&fixture_bytes()).expect("board loads");
    let mut request = request();
    request.accepted_atom_refs.retain(|atom| atom != TO_ATOM);

    assert_eq!(
        plan_feedback_check_pair(&board, request),
        Err(FeedbackMappingError::IncompleteRelationWitness {
            relation_id: RELATION.into(),
            collection: "accepted_atom_refs",
            reference: TO_ATOM.into(),
        })
    );
}

#[test]
fn selected_claim_requires_its_exact_assertion_atom() {
    let board = load_reviewed_experiment_board(&fixture_bytes()).expect("board loads");
    let mut request = request();
    request
        .accepted_atom_refs
        .retain(|atom| atom != ASSERTED_ATOM);

    assert_eq!(
        plan_feedback_check_pair(&board, request),
        Err(FeedbackMappingError::IncompleteClaimWitness {
            claim_id: CLAIM.into(),
            collection: "accepted_atom_refs",
            reference: ASSERTED_ATOM.into(),
        })
    );
}

#[test]
fn unknown_and_unconnected_witnesses_are_not_silently_accepted() {
    let board = load_reviewed_experiment_board(&fixture_bytes()).expect("board loads");
    let mut unknown = request();
    unknown.accepted_claim_refs = vec!["claim.alpha.missing".into()];
    assert_eq!(
        plan_feedback_check_pair(&board, unknown),
        Err(FeedbackMappingError::UnknownReference {
            collection: "accepted_claim_refs",
            reference: "claim.alpha.missing".into(),
        })
    );

    let mut unconnected = request();
    unconnected
        .accepted_atom_refs
        .push("sha256:501cbb9f6295bad2e6f654309381c752d56e8a427d8fc58d1ecda41e7b2c9e8c".into());
    assert_eq!(
        plan_feedback_check_pair(&board, unconnected),
        Err(FeedbackMappingError::UnconnectedReference {
            collection: "accepted_atom_refs",
            reference: "sha256:501cbb9f6295bad2e6f654309381c752d56e8a427d8fc58d1ecda41e7b2c9e8c"
                .into(),
        })
    );
}

#[test]
fn direct_claim_mapping_can_preserve_zero_governing_relations() {
    let board = load_reviewed_experiment_board(&fixture_bytes()).expect("board loads");
    let plan = plan_feedback_check_pair(
        &board,
        FeedbackAssertionWitnessRequestV1 {
            assertion_id: FEEDBACK_ASSERTION_ID.into(),
            accepted_claim_refs: vec!["claim.alpha.c07.idempotent-recovery".into()],
            accepted_atom_refs: vec![
                "sha256:56f39c2bb9ae82a5e3706dd233d4e99a82ff3b77e58a9a8c0d25c1c82968e8c8".into(),
            ],
            accepted_relation_refs: vec![],
            source_anchor_refs: vec![ANCHOR.into()],
        },
    )
    .expect("an exact accepted Claim assertion remains a valid witness");

    assert!(plan.accepted_relation_refs.is_empty());
    assert_eq!(plan.scope_completeness, "partial");
}
