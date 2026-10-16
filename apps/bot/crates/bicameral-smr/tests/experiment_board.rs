use std::fs;
use std::path::PathBuf;

use bicameral_smr::{
    load_experiment_board, load_reviewed_experiment_board, reviewed_experiment_board_pin,
    ExperimentBoardError, REVIEWED_BOARD_CLAIM_SET_DIGEST, REVIEWED_BOARD_FILE_SHA256,
    REVIEWED_BOARD_HEAD_COMMIT, REVIEWED_BOARD_MERGED_COMMIT,
};
use sha2::{Digest, Sha256};

fn fixture_bytes() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocol/smr-feedback-v0.1/fixtures")
        .join("product-creation-workspace-routing.governed-domain.fixture.json");
    fs::read(path).expect("reviewed fixture exists")
}

#[test]
fn exact_reviewed_fixture_loads_as_non_authoritative_board() {
    let board = load_reviewed_experiment_board(&fixture_bytes()).expect("exact fixture loads");

    assert_eq!(board.fixture_file_sha256(), REVIEWED_BOARD_FILE_SHA256);
    assert_eq!(board.fixture_head_commit(), REVIEWED_BOARD_HEAD_COMMIT);
    assert_eq!(board.merged_commit(), REVIEWED_BOARD_MERGED_COMMIT);
    assert_eq!(board.claim_set_digest(), REVIEWED_BOARD_CLAIM_SET_DIGEST);
    assert_eq!(
        board.scope_id(),
        "scope.alpha.product-creation-workspace-routing"
    );
    assert_eq!(board.scope_completeness(), "partial");
    assert!(!board.has_product_authority());
    assert_eq!(
        board.normalized_claim_set()["claims"]
            .as_array()
            .expect("claims array")
            .len(),
        12
    );
}

#[test]
fn any_fixture_byte_change_fails_before_use() {
    let mut bytes = fixture_bytes();
    let index = bytes
        .windows(b"owner_review_required".len())
        .position(|window| window == b"owner_review_required")
        .expect("status exists");
    bytes[index] = b'X';

    assert_eq!(
        load_reviewed_experiment_board(&bytes),
        Err(ExperimentBoardError::FixtureDigestMismatch)
    );
}

#[test]
fn malformed_unreviewed_input_is_not_reinterpreted() {
    assert_eq!(
        load_reviewed_experiment_board(b"not-json"),
        Err(ExperimentBoardError::FixtureDigestMismatch)
    );
}

#[test]
fn second_non_authoritative_fixture_loads_without_rust_identity_constants() {
    let mut package: serde_json::Value = serde_json::from_slice(&fixture_bytes()).unwrap();
    package["governed_domain"]["analysis_key"] =
        "fixture.smr-feedback.second-synthetic-domain-v1".into();
    package["scope"]["name"] = "second-synthetic-domain-v1".into();
    let bytes = serde_json::to_vec(&package).unwrap();
    let mut pin = reviewed_experiment_board_pin();
    pin.fixture_id = "second-synthetic-domain-v1".into();
    pin.analysis_key = "fixture.smr-feedback.second-synthetic-domain-v1".into();
    pin.fixture_file_sha256 = format!("{:x}", Sha256::digest(&bytes));
    pin.fixture_head_commit = "2222222222222222222222222222222222222222".into();
    pin.merged_commit = "3333333333333333333333333333333333333333".into();

    let board = load_experiment_board(&bytes, &pin).expect("second pinned fixture loads");
    assert_eq!(board.fixture_id(), "second-synthetic-domain-v1");
    assert_eq!(board.analysis_key(), pin.analysis_key);
    assert!(!board.has_product_authority());
}

#[test]
fn external_fixture_identity_mismatch_fails_closed() {
    let mut pin = reviewed_experiment_board_pin();
    pin.analysis_key = "fixture.smr-feedback.wrong".into();
    assert_eq!(
        load_experiment_board(&fixture_bytes(), &pin),
        Err(ExperimentBoardError::IdentityMismatch)
    );
}
