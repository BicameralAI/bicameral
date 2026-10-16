//! Exact, read-only adapter for the reviewed SMR feedback experiment board.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const REVIEWED_BOARD_FILE_SHA256: &str =
    "bad78bae59137eb60c91f9863d5954f7ac677edcd31e97fec0630a0870e64a64";
pub const REVIEWED_BOARD_HEAD_COMMIT: &str = "bcbf14b638c44d460beadfd1f01fbbf33820fb7b";
pub const REVIEWED_BOARD_MERGED_COMMIT: &str = "d2b83826579c648da9b5cee6b9a92088ef3768a5";
pub const REVIEWED_BOARD_CLAIM_SET_DIGEST: &str =
    "sha256:350fa9f73cdf6b80e42822371973b27c1eb6611b52416cf2a92e7ac3bf7c98ae";
const REVIEWED_ANALYSIS_KEY: &str = "fixture.smr-feedback.product-creation-workspace-routing-v1";
const REVIEWED_SCOPE_ID: &str = "scope.alpha.product-creation-workspace-routing";
const REVIEWED_FIXTURE_ID: &str = "product-creation-workspace-routing-v1";
const REVIEWED_BOARD_BYTES: &[u8] = include_bytes!(
    "../../../protocol/smr-feedback-v0.1/fixtures/product-creation-workspace-routing.governed-domain.fixture.json"
);

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewedExperimentBoardV1 {
    fixture_id: String,
    fixture_file_sha256: String,
    fixture_head_commit: String,
    merged_commit: String,
    claim_set_digest: String,
    analysis_key: String,
    scope_id: String,
    scope_completeness: String,
    scope_exclusions: Vec<String>,
    normalized_claim_set: Value,
}

impl ReviewedExperimentBoardV1 {
    pub fn fixture_id(&self) -> &str {
        &self.fixture_id
    }

    pub fn fixture_file_sha256(&self) -> &str {
        &self.fixture_file_sha256
    }

    pub fn fixture_head_commit(&self) -> &str {
        &self.fixture_head_commit
    }

    pub fn merged_commit(&self) -> &str {
        &self.merged_commit
    }

    pub fn claim_set_digest(&self) -> &str {
        &self.claim_set_digest
    }

    pub fn analysis_key(&self) -> &str {
        &self.analysis_key
    }

    pub fn scope_id(&self) -> &str {
        &self.scope_id
    }

    pub fn scope_completeness(&self) -> &str {
        &self.scope_completeness
    }

    pub fn scope_exclusions(&self) -> &[String] {
        &self.scope_exclusions
    }

    pub fn normalized_claim_set(&self) -> &Value {
        &self.normalized_claim_set
    }

    pub const fn has_product_authority(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentBoardPinV1 {
    pub fixture_id: String,
    pub fixture_file_sha256: String,
    pub fixture_head_commit: String,
    pub merged_commit: String,
    pub claim_set_digest: String,
    pub analysis_key: String,
    pub scope_id: String,
    pub scope_completeness: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ExperimentBoardError {
    #[error("experiment board bytes do not match the declared fixture pin")]
    FixtureDigestMismatch,
    #[error("experiment board JSON is malformed: {0}")]
    Malformed(String),
    #[error("experiment board authority or identity fields do not match the declared fixture pin")]
    IdentityMismatch,
}

#[derive(Deserialize)]
struct Package {
    fixture_role: String,
    governed_domain: Domain,
    identity: Identity,
    replay: Replay,
    scope: Scope,
}

#[derive(Deserialize)]
struct Domain {
    analysis_key: String,
    is_source: bool,
    artifact_role: String,
    normalized_claim_set: Value,
}

#[derive(Deserialize)]
struct Identity {
    claim_set_digest: String,
}

#[derive(Deserialize)]
struct Replay {
    mode: String,
    base_event_position: u64,
    writes_product_state: bool,
    canonical_event_kind: Option<String>,
}

#[derive(Deserialize)]
struct Scope {
    name: String,
    completeness: String,
    exclusions: Vec<String>,
}

pub fn reviewed_experiment_board_pin() -> ExperimentBoardPinV1 {
    ExperimentBoardPinV1 {
        fixture_id: REVIEWED_FIXTURE_ID.into(),
        fixture_file_sha256: REVIEWED_BOARD_FILE_SHA256.into(),
        fixture_head_commit: REVIEWED_BOARD_HEAD_COMMIT.into(),
        merged_commit: REVIEWED_BOARD_MERGED_COMMIT.into(),
        claim_set_digest: REVIEWED_BOARD_CLAIM_SET_DIGEST.into(),
        analysis_key: REVIEWED_ANALYSIS_KEY.into(),
        scope_id: REVIEWED_SCOPE_ID.into(),
        scope_completeness: "partial".into(),
    }
}

pub fn load_experiment_board(
    bytes: &[u8],
    pin: &ExperimentBoardPinV1,
) -> Result<ReviewedExperimentBoardV1, ExperimentBoardError> {
    let file_digest = format!("{:x}", Sha256::digest(bytes));
    if file_digest != pin.fixture_file_sha256 {
        return Err(ExperimentBoardError::FixtureDigestMismatch);
    }

    let package: Package = serde_json::from_slice(bytes)
        .map_err(|error| ExperimentBoardError::Malformed(error.to_string()))?;
    let replay_is_read_only = package.replay.mode == "throwaway_fixture"
        && package.replay.base_event_position == 0
        && !package.replay.writes_product_state
        && package.replay.canonical_event_kind.is_none();
    let scope_is_declared = package
        .governed_domain
        .normalized_claim_set
        .get("scopes")
        .and_then(Value::as_array)
        .is_some_and(|scopes| {
            scopes.iter().any(|scope| {
                scope.get("scope_id").and_then(Value::as_str) == Some(pin.scope_id.as_str())
            })
        });
    if package.fixture_role != "non_authoritative_throwaway_replay"
        || pin.fixture_id.is_empty()
        || pin.fixture_head_commit.is_empty()
        || pin.merged_commit.is_empty()
        || package.governed_domain.analysis_key != pin.analysis_key
        || package.governed_domain.is_source
        || package.governed_domain.artifact_role != "provenance_only_not_product_truth"
        || package.identity.claim_set_digest != pin.claim_set_digest
        || package.scope.name != pin.fixture_id
        || package.scope.completeness != pin.scope_completeness
        || package.scope.exclusions.is_empty()
        || !scope_is_declared
        || !replay_is_read_only
    {
        return Err(ExperimentBoardError::IdentityMismatch);
    }

    Ok(ReviewedExperimentBoardV1 {
        fixture_id: pin.fixture_id.clone(),
        fixture_file_sha256: file_digest,
        fixture_head_commit: pin.fixture_head_commit.clone(),
        merged_commit: pin.merged_commit.clone(),
        claim_set_digest: package.identity.claim_set_digest,
        analysis_key: package.governed_domain.analysis_key,
        scope_id: pin.scope_id.clone(),
        scope_completeness: package.scope.completeness,
        scope_exclusions: package.scope.exclusions,
        normalized_claim_set: package.governed_domain.normalized_claim_set,
    })
}

pub fn load_reviewed_experiment_board(
    bytes: &[u8],
) -> Result<ReviewedExperimentBoardV1, ExperimentBoardError> {
    load_experiment_board(bytes, &reviewed_experiment_board_pin())
}

pub fn load_bundled_reviewed_experiment_board(
) -> Result<ReviewedExperimentBoardV1, ExperimentBoardError> {
    load_reviewed_experiment_board(REVIEWED_BOARD_BYTES)
}
