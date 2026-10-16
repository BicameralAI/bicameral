//! Versioned CandidateSet selection, preview, confirmation, and receipt contract.
//!
//! This contract belongs to the daemon-owned Managed MCP promotion boundary.
//! MCP may transport stable candidate ids and render daemon results, but it does
//! not validate closure, mint confirmation, decide authority, append canonical
//! events, or infer Product policy.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::confirmation_challenge::{ChallengeSecret, ConfirmationToken};

/// Wire contract version for `candidate_selection.prepare` and
/// `candidate_selection.commit`.
pub const CANDIDATE_SELECTION_CONTRACT_VERSION: u32 = 1;

/// Daemon command that validates a stable-id subset and returns an exact preview.
pub const CANDIDATE_SELECTION_PREPARE_COMMAND: &str = "candidate_selection.prepare";

/// Daemon command that consumes explicit human confirmation and atomically
/// appends the exact previewed promotion effects.
pub const CANDIDATE_SELECTION_COMMIT_COMMAND: &str = "candidate_selection.commit";

/// Stable-id subset submitted for daemon validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateSelectionV1 {
    pub contract_version: u32,
    pub selection_operation_id: Uuid,
    pub candidate_set_id: Uuid,
    pub candidate_set_generation: u64,
    pub candidate_set_digest: String,
    pub product_id: String,
    pub host_session_id: String,
    pub session_lease_id: String,
    #[serde(default)]
    pub promote_candidate_ids: Vec<Uuid>,
    #[serde(default)]
    pub reject_candidate_ids: Vec<Uuid>,
    /// Authenticated human actor bound by the daemon. Agent or MCP identities
    /// are not valid substitutes for this field.
    pub actor_id: String,
}

/// Canonical Product policy that governs the exact preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidatePromotionPolicyV1 {
    pub enabled: bool,
    pub policy_revision: String,
}

/// Current daemon-owned authority and currentness snapshot.
///
/// The runtime obtains this from its trusted Product authority boundary. It is
/// never accepted from an MCP request as authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateSelectionAuthorityV1 {
    pub product_id: String,
    pub actor_id: String,
    pub may_promote: bool,
    pub is_current_product_owner: bool,
    pub governing_spec_binding_digest: String,
    pub accepted_model_base_position: u64,
    pub accepted_model_base_digest: String,
    pub promotion_policy: CandidatePromotionPolicyV1,
}

/// Resulting Decision signoff mode previewed before confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateSelectionSignoffMode {
    Proposed,
    ApprovedByOwnerPolicy,
}

/// Exact canonical effect of promoting one candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidatePromotionEffectV1 {
    pub candidate_id: Uuid,
    pub decision_id: Uuid,
    pub proposed_decision: String,
    pub proposal_digest: String,
    pub resulting_signoff: CandidateSelectionSignoffMode,
}

/// Exact transient effect of explicitly rejecting one candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateRejectionEffectV1 {
    pub candidate_id: Uuid,
    pub resulting_disposition: String,
}

/// Opaque, daemon-minted challenge attached to an exact preview.
#[derive(Clone, Serialize, Deserialize)]
pub struct CandidateSelectionConfirmationV1 {
    pub challenge_id: Uuid,
    pub restart_epoch: Uuid,
    pub secret: ChallengeSecret,
    pub expires_at: DateTime<Utc>,
}

impl std::fmt::Debug for CandidateSelectionConfirmationV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CandidateSelectionConfirmationV1")
            .field("challenge_id", &self.challenge_id)
            .field("restart_epoch", &self.restart_epoch)
            .field("secret", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

impl CandidateSelectionConfirmationV1 {
    pub fn token(&self) -> ConfirmationToken {
        ConfirmationToken {
            challenge_id: self.challenge_id,
            restart_epoch: self.restart_epoch,
            secret: self.secret.clone(),
        }
    }
}

/// Exact daemon preview. Preparing this value writes no canonical Product state
/// and changes no Candidate disposition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateSelectionPreviewV1 {
    pub contract_version: u32,
    pub selection_operation_id: Uuid,
    pub selection_digest: String,
    pub display_digest: String,
    pub candidate_set_id: Uuid,
    pub candidate_set_generation: u64,
    pub candidate_set_digest: String,
    pub product_id: String,
    pub host_session_id: String,
    pub actor_id: String,
    pub governing_spec_binding_digest: String,
    pub accepted_model_base_position: u64,
    pub accepted_model_base_digest: String,
    pub policy_revision: String,
    pub promote_effects: Vec<CandidatePromotionEffectV1>,
    pub reject_effects: Vec<CandidateRejectionEffectV1>,
    pub pending_candidate_count: usize,
    pub resulting_signoff: CandidateSelectionSignoffMode,
    pub confirmation_challenge: CandidateSelectionConfirmationV1,
}

/// Explicit confirmed resubmission. The token is daemon-issued and single-use;
/// `human_confirmed` must be true and the authenticated actor must match the
/// preview binding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateSelectionCommitV1 {
    pub contract_version: u32,
    pub selection_operation_id: Uuid,
    pub selection_digest: String,
    pub actor_id: String,
    pub human_confirmed: bool,
    pub confirmation_token: ConfirmationToken,
}

/// Durable and transient results after one exact confirmed operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateSelectionReceiptV1 {
    pub contract_version: u32,
    pub selection_operation_id: Uuid,
    pub selection_digest: String,
    pub product_id: String,
    pub actor_id: String,
    pub promoted_candidate_ids: Vec<Uuid>,
    pub rejected_candidate_ids: Vec<Uuid>,
    pub decision_ids: Vec<Uuid>,
    pub canonical_event_ids: Vec<Uuid>,
    pub resulting_signoff: CandidateSelectionSignoffMode,
    pub pending_candidate_count: usize,
    pub replay_verified: bool,
    pub session_reconciled: bool,
}

/// Stable fail-closed result codes for prepare and commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateSelectionFailureCode {
    UnsupportedContractVersion,
    MalformedIdentity,
    EmptySelection,
    OverlappingDispositions,
    CandidateSetNotFound,
    CandidateSetStale,
    CandidateSetDigestMismatch,
    CandidateSetIdentityMismatch,
    CandidateNotFound,
    CandidateNotPending,
    DependencyClosureRequired,
    ActorUnauthorized,
    CurrentProductOwnerRequired,
    GoverningSpecChanged,
    AcceptedModelBaseChanged,
    PolicyChanged,
    PreviewNotFound,
    PreviewStale,
    HumanConfirmationRequired,
    ConfirmationRejected,
    AtomicAppendUnsupported,
    AtomicAppendFailed,
    PartialCanonicalStateDetected,
    ReplayVerificationFailed,
    SessionReconciliationFailed,
}

/// Typed failure that never contains a challenge secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateSelectionFailureV1 {
    pub code: CandidateSelectionFailureCode,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_ids: Vec<Uuid>,
}

impl CandidateSelectionFailureV1 {
    pub fn new(code: CandidateSelectionFailureCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
            candidate_ids: Vec::new(),
        }
    }

    pub fn with_candidates(
        code: CandidateSelectionFailureCode,
        detail: impl Into<String>,
        candidate_ids: Vec<Uuid>,
    ) -> Self {
        Self {
            code,
            detail: detail.into(),
            candidate_ids,
        }
    }
}

pub type CandidateSelectionResult<T> = Result<T, CandidateSelectionFailureV1>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_commands_are_stable() {
        assert_eq!(
            CANDIDATE_SELECTION_PREPARE_COMMAND,
            "candidate_selection.prepare"
        );
        assert_eq!(
            CANDIDATE_SELECTION_COMMIT_COMMAND,
            "candidate_selection.commit"
        );
    }

    #[test]
    fn confirmation_debug_redacts_secret() {
        let confirmation = CandidateSelectionConfirmationV1 {
            challenge_id: Uuid::from_u128(1),
            restart_epoch: Uuid::from_u128(2),
            secret: ChallengeSecret::from_minted("never-print-this"),
            expires_at: Utc::now(),
        };
        let rendered = format!("{confirmation:?}");
        assert!(!rendered.contains("never-print-this"));
        assert!(rendered.contains("<redacted>"));
    }
}
