//! Authenticated remote action callback contract.
//!
//! Defines the envelope and validation types for consuming verified user
//! actions initiated from external review surfaces (Slack modals, Linear
//! actions, hosted review UI) through the bot-owned authority path.
//!
//! # Contract status
//!
//! This implements against the **proposed v1** envelope from
//! `BicameralAI/bicameral-integrations#206`. That RFQ remains the contract
//! authority — this bot-side consumer is ready for the proposed shape but
//! does not claim final contract acceptance. If integrations#206 revises
//! the envelope schema, this module must be updated to match.
//!
//! # Authority boundary
//!
//! Remote action callbacks are **not** source ingest, local `ToolRequest`,
//! or egress. They represent a verified user action that the bot validates
//! and routes through `ActionAuthorizationPolicy` before any governed
//! mutation.
//!
//! Remote callback payloads **must not** directly mint:
//! - Authority or actor context
//! - Signoff approval
//! - Compliance resolution
//! - Event-store writes
//! - Local workspace access
//! - Source-ingest authority
//!
//! The bot owns authentication verification, replay/idempotency checking,
//! origin/session binding, actor mapping, and action authorization.
//!
//! # Spec references
//!
//! - `docs/specs/action-authorization-policy.md` — "Authenticated remote
//!   review action" surface category
//! - `docs/specs/action-authorization-policy-runtime-architecture.md` —
//!   Scenario 3 (authenticated Slack/Linear approval action)
//! - `docs/domain/runtime-protocol.md` — "Authenticated remote review action"
//! - `BicameralAI/bicameral-integrations#206` — contract RFQ (proposed, not
//!   yet accepted)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Remote action envelope
// ---------------------------------------------------------------------------

/// Authenticated remote action callback envelope.
///
/// Submitted by a verified remote review surface (Slack, Linear, hosted UI)
/// after the integration layer has verified the provider signature. The bot
/// validates the envelope, maps the external actor to a Bicameral actor,
/// checks replay/idempotency, and evaluates `ActionAuthorizationPolicy`
/// before routing to the governed command handler.
///
/// This type does **not** contain `AuthorityContext` — that is derived
/// by the bot after validation. The envelope carries provider-side proof
/// that the bot must verify independently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticatedRemoteActionEnvelope {
    /// Unique action identifier for idempotency and audit correlation.
    pub action_id: Uuid,

    /// The governed action being requested.
    pub action_kind: RemoteActionKind,

    /// Target entity identifier (e.g. "CAND-1", "DEC-42").
    pub target_id: String,

    /// The external review surface that originated this callback.
    pub callback_source: CallbackSource,

    /// External actor identifier from the provider (e.g. Slack user ID).
    pub external_actor_id: String,

    /// Provider-side signature or proof token for callback verification.
    pub provider_signature: String,

    /// Nonce for anti-replay protection.
    pub nonce: String,

    /// Timestamp when the action was issued at the provider.
    pub issued_at: DateTime<Utc>,

    /// Optional reason or note from the actor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    /// Session binding — the remote session or workspace context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_binding: Option<RemoteSessionBinding>,
}

/// Governed action kinds that may be requested through a remote callback.
///
/// Only review-class actions are supported in alpha. Signoff, compliance,
/// egress, and policy mutations are not remote-actionable in V1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteActionKind {
    /// Accept a decision candidate (review:candidate authority).
    AcceptCandidate,
    /// Reject a decision candidate (review:candidate authority).
    RejectCandidate,
    /// Request a correction capture (workspace member action).
    RequestCorrection,
}

/// The external review surface that originated the callback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallbackSource {
    /// Slack modal or interactive action.
    Slack,
    /// Linear action callback.
    Linear,
    /// Hosted Bicameral review UI.
    HostedReviewUi,
    /// Other verified external review surface.
    Other(String),
}

/// Remote session binding — contextual binding for the callback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteSessionBinding {
    /// Product or workspace scope for the action.
    pub product_id: Option<String>,
    /// Session or thread context from the remote surface.
    pub remote_session_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Validation outcomes
// ---------------------------------------------------------------------------

/// Result of validating a remote action callback.
pub type RemoteActionValidationResult = Result<ValidatedRemoteAction, RemoteActionRejection>;

/// A validated remote action ready for authorization evaluation.
///
/// Produced by the bot after signature verification, replay checking,
/// and actor binding. Contains the derived authority context that the
/// `ActionAuthorizationPolicy` evaluator will check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedRemoteAction {
    /// The original action ID for correlation.
    pub action_id: Uuid,

    /// The governed action kind.
    pub action_kind: RemoteActionKind,

    /// Target entity ID.
    pub target_id: String,

    /// The resolved Bicameral actor ID (mapped from external_actor_id).
    pub resolved_actor_id: String,

    /// The callback source for audit.
    pub callback_source: CallbackSource,

    /// Optional reason from the actor.
    pub reason: Option<String>,
}

/// Rejection reason for a remote action callback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteActionRejectionReason {
    /// Provider signature is invalid or missing.
    InvalidSignature,
    /// Nonce has already been consumed (replay attack).
    ReplayDetected,
    /// The external actor cannot be bound to a Bicameral actor.
    ActorBindingFailed,
    /// The action is issued_at time is outside the acceptable window.
    TimestampExpired,
    /// The callback source is not recognized or not configured.
    UnknownCallbackSource,
    /// The action kind is not supported for remote callbacks.
    UnsupportedAction,
    /// The target entity is not in a valid state for this action.
    TargetIneligible,
    /// The session binding is invalid or mismatched.
    SessionBindingInvalid,
    /// Payload contains forbidden authority-smuggling fields.
    AuthoritySmugglingDetected,
}

/// A rejected remote action callback with structured reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteActionRejection {
    /// Why the callback was rejected.
    pub reason: RemoteActionRejectionReason,
    /// Human-readable rejection message.
    pub message: String,
    /// The action_id of the rejected envelope (for audit).
    pub action_id: Uuid,
}

impl RemoteActionRejection {
    pub fn new(
        reason: RemoteActionRejectionReason,
        message: impl Into<String>,
        action_id: Uuid,
    ) -> Self {
        Self {
            reason,
            message: message.into(),
            action_id,
        }
    }
}

impl std::fmt::Display for RemoteActionRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.reason, self.message)
    }
}

// ---------------------------------------------------------------------------
// Audit receipt
// ---------------------------------------------------------------------------

/// Audit/provenance receipt for a processed remote action callback.
///
/// Recorded regardless of whether the action was ultimately permitted or
/// denied by `ActionAuthorizationPolicy`. This is an operational receipt,
/// not governance authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteActionReceipt {
    /// Unique receipt ID.
    pub receipt_id: Uuid,

    /// The action_id from the original envelope.
    pub action_id: Uuid,

    /// Callback source for audit trail.
    pub callback_source: CallbackSource,

    /// External actor ID (for audit, not authority).
    pub external_actor_id: String,

    /// Resolved Bicameral actor ID (if binding succeeded).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_actor_id: Option<String>,

    /// Action kind requested.
    pub action_kind: RemoteActionKind,

    /// Target entity ID.
    pub target_id: String,

    /// Final outcome of the callback processing.
    pub outcome: RemoteActionOutcome,

    /// Timestamp when the receipt was produced.
    pub processed_at: DateTime<Utc>,

    /// Rejection details if the callback was rejected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejection: Option<RemoteActionRejection>,
}

/// Outcome of processing a remote action callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteActionOutcome {
    /// Callback validated and action was authorized and executed.
    Accepted,
    /// Callback validated but action was denied by authorization policy.
    AuthorizationDenied,
    /// Callback failed validation (signature, replay, binding, etc.).
    ValidationFailed,
}

// ---------------------------------------------------------------------------
// Forbidden fields (authority-smuggling prevention)
// ---------------------------------------------------------------------------

/// Fields that are forbidden in remote action callback payloads.
///
/// If any raw JSON payload arriving at the remote action gateway contains
/// these fields, the entire callback is rejected as an authority-smuggling
/// attempt.
pub const REMOTE_ACTION_FORBIDDEN_FIELDS: &[&str] = &[
    "authority_context",
    "auth_method",
    "policy_scope",
    "policy_grant",
    "signoff_approval",
    "compliance_resolution",
    "event_store_write",
    "decision_write",
    "source_binding",
    "workspace_path",
    "provider_token",
    "canonical_decision",
    "no_conflict_claim",
    "egress_command",
];

/// Check whether a raw JSON payload contains forbidden authority-smuggling
/// fields for remote action callbacks.
pub fn detect_remote_action_smuggled_fields(payload: &serde_json::Value) -> Vec<String> {
    let mut found = Vec::new();
    if let Some(obj) = payload.as_object() {
        for &field in REMOTE_ACTION_FORBIDDEN_FIELDS {
            if obj.contains_key(field) {
                found.push(field.to_string());
            }
        }
    }
    found
}

// ---------------------------------------------------------------------------
// Action kind mapping
// ---------------------------------------------------------------------------

impl RemoteActionKind {
    /// Map a remote action kind to the governed action class for policy
    /// evaluation.
    pub fn to_governed_action_class(&self) -> crate::action_authorization::GovernedActionClass {
        use crate::action_authorization::GovernedActionClass;
        match self {
            Self::AcceptCandidate | Self::RejectCandidate => GovernedActionClass::CandidateReview,
            Self::RequestCorrection => GovernedActionClass::CandidateReview,
        }
    }

    /// Wire-format command name for this action kind.
    pub fn command_name(&self) -> &'static str {
        match self {
            Self::AcceptCandidate => "review.accept_candidate",
            Self::RejectCandidate => "review.reject_candidate",
            Self::RequestCorrection => "recall.request_correction",
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_roundtrip_serialization() {
        let envelope = AuthenticatedRemoteActionEnvelope {
            action_id: Uuid::nil(),
            action_kind: RemoteActionKind::AcceptCandidate,
            target_id: "CAND-42".into(),
            callback_source: CallbackSource::Slack,
            external_actor_id: "U01234ABC".into(),
            provider_signature: "v0=abcdef1234567890".into(),
            nonce: "nonce-001".into(),
            issued_at: Utc::now(),
            reason: Some("evidence looks good".into()),
            session_binding: Some(RemoteSessionBinding {
                product_id: Some("prod-1".into()),
                remote_session_id: Some("thread-xyz".into()),
            }),
        };

        let json = serde_json::to_string(&envelope).unwrap();
        let deserialized: AuthenticatedRemoteActionEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(envelope, deserialized);
    }

    #[test]
    fn forbidden_fields_are_detected() {
        let payload = serde_json::json!({
            "action_id": "00000000-0000-0000-0000-000000000000",
            "action_kind": "accept_candidate",
            "target_id": "CAND-1",
            "authority_context": {"actor_id": "attacker"},
            "signoff_approval": true,
            "policy_grant": {"cap": "review:signoff"}
        });

        let smuggled = detect_remote_action_smuggled_fields(&payload);
        assert!(smuggled.contains(&"authority_context".to_string()));
        assert!(smuggled.contains(&"signoff_approval".to_string()));
        assert!(smuggled.contains(&"policy_grant".to_string()));
    }

    #[test]
    fn clean_payload_has_no_forbidden_fields() {
        let payload = serde_json::json!({
            "action_id": "00000000-0000-0000-0000-000000000000",
            "action_kind": "accept_candidate",
            "target_id": "CAND-1",
            "callback_source": "slack",
            "external_actor_id": "U01234",
            "provider_signature": "sig",
            "nonce": "n1"
        });

        let smuggled = detect_remote_action_smuggled_fields(&payload);
        assert!(smuggled.is_empty());
    }

    #[test]
    fn action_kind_maps_to_correct_governed_class() {
        use crate::action_authorization::GovernedActionClass;
        assert_eq!(
            RemoteActionKind::AcceptCandidate.to_governed_action_class(),
            GovernedActionClass::CandidateReview
        );
        assert_eq!(
            RemoteActionKind::RejectCandidate.to_governed_action_class(),
            GovernedActionClass::CandidateReview
        );
        assert_eq!(
            RemoteActionKind::RequestCorrection.to_governed_action_class(),
            GovernedActionClass::CandidateReview
        );
    }

    #[test]
    fn rejection_display() {
        let rejection = RemoteActionRejection::new(
            RemoteActionRejectionReason::ReplayDetected,
            "nonce already consumed",
            Uuid::nil(),
        );
        let display = format!("{}", rejection);
        assert!(display.contains("ReplayDetected"));
        assert!(display.contains("nonce already consumed"));
    }
}
