//! Versioned daemon-issued `ConfirmationChallenge` protocol (issue #829).
//!
//! Explicit human confirmation is required before an MCP-requested candidate
//! promotion can materialize canonical `Decision` state. The daemon issues a
//! challenge in response to an initial governed promotion request and returns a
//! typed `confirmation_required` response before any canonical write. A
//! confirmed resubmission carries the challenge back; the daemon validates it,
//! consumes it exactly once, and only then applies the permitted transition.
//!
//! Authority boundary (see the source issue): MCP may *request, transport,
//! render, and resubmit* a daemon-issued challenge. The coding agent, hook, MCP
//! process, or server identity may not mint, validate, consume, or self-confirm
//! one. This module defines the transport-neutral contract types; the daemon
//! owns minting/validation/consumption via the runtime challenge store.
//!
//! Security invariants:
//!
//! - The challenge value ([`ChallengeSecret`]) is cryptographically
//!   unpredictable, single-use, and valid for five minutes.
//! - It is held in memory only for alpha and invalidated by daemon restart.
//! - It is bound to workspace, actor, agent session, RecallPacket, candidate,
//!   requested outcome, and request id. Every mismatch, expiry, cancellation,
//!   and restart fails closed with a typed [`ConfirmationOutcome`].
//! - The value never appears in logs, telemetry, or retained traces: the
//!   [`ChallengeSecret`] `Debug`/`Display` impls redact it, and typed outcomes
//!   never echo it.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

use crate::recall::PromotionOutcome;

/// Protocol version for the `ConfirmationChallenge` contract.
///
/// The challenge is versioned so that daemon and MCP surfaces can detect and
/// reject a challenge minted under an incompatible protocol. A version mismatch
/// fails closed as [`ConfirmationOutcome::Unauthorized`].
pub const CONFIRMATION_CHALLENGE_PROTOCOL_VERSION: u32 = 1;

/// Challenge validity window: five minutes.
pub const CONFIRMATION_CHALLENGE_TTL_SECONDS: i64 = 300;

// ---------------------------------------------------------------------------
// Challenge secret
// ---------------------------------------------------------------------------

/// A cryptographically unpredictable, single-use challenge value.
///
/// The daemon mints the value; MCP transports and re-presents it. The value is
/// **never** written to logs, telemetry, error text, or fixtures — the `Debug`
/// and `Display` impls redact it, so accidental interpolation into a tracing
/// span or an error string cannot leak it. Equality is compared in constant
/// time to avoid a timing side channel.
#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChallengeSecret(String);

impl ChallengeSecret {
    /// Wrap a freshly minted secret value.
    ///
    /// Callers must supply a cryptographically unpredictable value; minting is
    /// the daemon's responsibility (the runtime challenge store).
    pub fn from_minted(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Expose the raw value for transport back to the daemon.
    ///
    /// This is intentionally explicit so that every read site is auditable. Do
    /// not log or interpolate the returned value into diagnostics.
    pub fn expose_for_transport(&self) -> &str {
        &self.0
    }

    /// Constant-time equality against a presented secret.
    pub fn matches(&self, presented: &ChallengeSecret) -> bool {
        constant_time_eq(self.0.as_bytes(), presented.0.as_bytes())
    }
}

/// Redacted debug — never render the secret value.
impl fmt::Debug for ChallengeSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ChallengeSecret(<redacted>)")
    }
}

/// Redacted display — never render the secret value.
impl fmt::Display for ChallengeSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Compare two byte slices in constant time relative to their content.
///
/// Length differences short-circuit (lengths are not secret), but for equal
/// lengths every byte is compared so a partial-match attacker learns nothing
/// from timing.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ---------------------------------------------------------------------------
// Challenge binding
// ---------------------------------------------------------------------------

/// The immutable tuple a [`ConfirmationChallenge`] is bound to.
///
/// A confirmed resubmission must present a binding that matches the one the
/// daemon recorded at issue time. Any mismatch fails closed with a typed
/// [`ConfirmationOutcome`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChallengeBinding {
    /// Protocol version the challenge was minted under.
    pub protocol_version: u32,
    /// Workspace root (repo path or equivalent) the promotion is scoped to.
    pub workspace: String,
    /// Actor identity that requested the promotion.
    pub actor_id: String,
    /// Agent session the request originated from.
    pub session_id: String,
    /// RecallPacket the candidate was surfaced in.
    pub packet_id: Uuid,
    /// Candidate to be promoted.
    pub candidate_id: Uuid,
    /// Requested promotion outcome.
    pub requested_outcome: PromotionOutcome,
    /// Request id — the idempotency key for the confirmed operation.
    pub request_id: Uuid,
}

impl ChallengeBinding {
    /// Return the first typed mismatch between `self` (the recorded binding) and
    /// `presented` (the binding a confirmed resubmission carries), or `None`
    /// when every bound field matches.
    ///
    /// Fields are checked in a fixed order so the outcome is deterministic.
    /// `session_id`, `request_id`, and `protocol_version` are bound but have no
    /// dedicated typed outcome; a mismatch on them fails closed as
    /// [`ConfirmationOutcome::Unauthorized`].
    pub fn first_mismatch(&self, presented: &ChallengeBinding) -> Option<ConfirmationOutcome> {
        if self.protocol_version != presented.protocol_version {
            return Some(ConfirmationOutcome::Unauthorized);
        }
        if self.workspace != presented.workspace {
            return Some(ConfirmationOutcome::WorkspaceMismatch);
        }
        if self.actor_id != presented.actor_id {
            return Some(ConfirmationOutcome::ActorMismatch);
        }
        if self.session_id != presented.session_id {
            return Some(ConfirmationOutcome::Unauthorized);
        }
        if self.packet_id != presented.packet_id {
            return Some(ConfirmationOutcome::PacketMismatch);
        }
        if self.candidate_id != presented.candidate_id {
            return Some(ConfirmationOutcome::CandidateMismatch);
        }
        if self.requested_outcome != presented.requested_outcome {
            return Some(ConfirmationOutcome::OutcomeMismatch);
        }
        if self.request_id != presented.request_id {
            return Some(ConfirmationOutcome::Unauthorized);
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Typed outcomes
// ---------------------------------------------------------------------------

/// Typed, fail-closed outcome of validating/consuming a confirmation challenge.
///
/// Every rejection path maps to exactly one of these; none echoes the secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationOutcome {
    /// The challenge's five-minute validity window has elapsed.
    Expired,
    /// The challenge was already consumed by a prior confirmed resubmission.
    AlreadyConsumed,
    /// The challenge was explicitly cancelled.
    Cancelled,
    /// The presented actor does not match the bound actor.
    ActorMismatch,
    /// The presented workspace does not match the bound workspace.
    WorkspaceMismatch,
    /// The presented RecallPacket does not match the bound packet.
    PacketMismatch,
    /// The presented candidate does not match the bound candidate.
    CandidateMismatch,
    /// The presented promotion outcome does not match the bound outcome.
    OutcomeMismatch,
    /// The referenced RecallPacket/candidate view is stale (already moved on).
    StalePacket,
    /// The request is not authorized (wrong secret, session, request id, or
    /// protocol version).
    Unauthorized,
    /// The promotion target is not in a state eligible for promotion.
    IneligibleTarget,
    /// A daemon restart invalidated all pending challenges; the caller must
    /// rerun preflight to obtain a fresh challenge.
    ChallengeUnavailableAfterRestart,
}

impl ConfirmationOutcome {
    /// Stable wire-format code string.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Expired => "expired",
            Self::AlreadyConsumed => "already_consumed",
            Self::Cancelled => "cancelled",
            Self::ActorMismatch => "actor_mismatch",
            Self::WorkspaceMismatch => "workspace_mismatch",
            Self::PacketMismatch => "packet_mismatch",
            Self::CandidateMismatch => "candidate_mismatch",
            Self::OutcomeMismatch => "outcome_mismatch",
            Self::StalePacket => "stale_packet",
            Self::Unauthorized => "unauthorized",
            Self::IneligibleTarget => "ineligible_target",
            Self::ChallengeUnavailableAfterRestart => "challenge_unavailable_after_restart",
        }
    }
}

/// A typed, fail-closed confirmation rejection.
///
/// Carries the [`ConfirmationOutcome`] and a human-readable detail. The detail
/// never contains the challenge secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmationRejection {
    /// Typed outcome.
    pub outcome: ConfirmationOutcome,
    /// Human-readable detail (never contains the secret).
    pub detail: String,
}

impl ConfirmationRejection {
    /// Build a rejection with a static-style detail message.
    pub fn new(outcome: ConfirmationOutcome, detail: impl Into<String>) -> Self {
        Self {
            outcome,
            detail: detail.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Confirmation-required response (phase 1 output)
// ---------------------------------------------------------------------------

/// The typed `confirmation_required` response returned before any canonical
/// write.
///
/// The daemon returns this in response to an initial governed promotion
/// request. MCP renders the prompt to the human, and on explicit confirmation
/// resubmits a [`ConfirmationToken`] built from `challenge_id`, `secret`, and
/// `restart_epoch`, reusing the same `request_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmationRequiredResponse {
    /// Protocol version the challenge was minted under.
    pub protocol_version: u32,
    /// Opaque challenge identifier.
    pub challenge_id: Uuid,
    /// Daemon restart epoch the challenge belongs to. A resubmission carrying a
    /// stale epoch is rejected as
    /// [`ConfirmationOutcome::ChallengeUnavailableAfterRestart`].
    pub restart_epoch: Uuid,
    /// The single-use challenge value to transport and re-present. Redacted in
    /// `Debug`; never log it.
    pub secret: ChallengeSecret,
    /// When the challenge expires (issue time + five minutes).
    pub expires_at: DateTime<Utc>,
    /// Echo of the bound tuple so the caller can confirm what it is authorizing.
    pub binding: ChallengeBinding,
    /// Human-readable confirmation prompt (contains no secret).
    pub prompt: String,
}

// ---------------------------------------------------------------------------
// Confirmation token (phase 2 input)
// ---------------------------------------------------------------------------

/// The token a confirmed resubmission carries back to the daemon.
///
/// Built by MCP from a [`ConfirmationRequiredResponse`]. `Debug` redacts the
/// secret via [`ChallengeSecret`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmationToken {
    /// The challenge identifier issued by the daemon.
    pub challenge_id: Uuid,
    /// The daemon restart epoch the challenge was issued under.
    pub restart_epoch: Uuid,
    /// The single-use challenge value being re-presented.
    pub secret: ChallengeSecret,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> ChallengeBinding {
        ChallengeBinding {
            protocol_version: CONFIRMATION_CHALLENGE_PROTOCOL_VERSION,
            workspace: "/home/user/project".into(),
            actor_id: "user-a".into(),
            session_id: "sess-1".into(),
            packet_id: Uuid::from_u128(1),
            candidate_id: Uuid::from_u128(2),
            requested_outcome: PromotionOutcome::NewConstraint,
            request_id: Uuid::from_u128(3),
        }
    }

    #[test]
    fn secret_debug_and_display_redact_value() {
        let secret = ChallengeSecret::from_minted("super-secret-value");
        assert_eq!(format!("{secret:?}"), "ChallengeSecret(<redacted>)");
        assert_eq!(format!("{secret}"), "<redacted>");
        assert!(!format!("{secret:?}").contains("super-secret-value"));
    }

    #[test]
    fn token_debug_redacts_secret() {
        let token = ConfirmationToken {
            challenge_id: Uuid::from_u128(9),
            restart_epoch: Uuid::from_u128(10),
            secret: ChallengeSecret::from_minted("leak-me-not"),
        };
        assert!(!format!("{token:?}").contains("leak-me-not"));
    }

    #[test]
    fn secret_matches_is_constant_time_correct() {
        let a = ChallengeSecret::from_minted("abcdef");
        assert!(a.matches(&ChallengeSecret::from_minted("abcdef")));
        assert!(!a.matches(&ChallengeSecret::from_minted("abcdeg")));
        assert!(!a.matches(&ChallengeSecret::from_minted("abcde")));
    }

    #[test]
    fn matching_binding_has_no_mismatch() {
        assert!(binding().first_mismatch(&binding()).is_none());
    }

    #[test]
    fn binding_mismatches_are_typed_in_fixed_order() {
        let base = binding();

        let mut b = binding();
        b.workspace = "/other".into();
        assert_eq!(
            base.first_mismatch(&b),
            Some(ConfirmationOutcome::WorkspaceMismatch)
        );

        let mut b = binding();
        b.actor_id = "user-b".into();
        assert_eq!(
            base.first_mismatch(&b),
            Some(ConfirmationOutcome::ActorMismatch)
        );

        let mut b = binding();
        b.packet_id = Uuid::from_u128(99);
        assert_eq!(
            base.first_mismatch(&b),
            Some(ConfirmationOutcome::PacketMismatch)
        );

        let mut b = binding();
        b.candidate_id = Uuid::from_u128(99);
        assert_eq!(
            base.first_mismatch(&b),
            Some(ConfirmationOutcome::CandidateMismatch)
        );

        let mut b = binding();
        b.requested_outcome = PromotionOutcome::DismissedAsNoise;
        assert_eq!(
            base.first_mismatch(&b),
            Some(ConfirmationOutcome::OutcomeMismatch)
        );

        let mut b = binding();
        b.session_id = "sess-2".into();
        assert_eq!(
            base.first_mismatch(&b),
            Some(ConfirmationOutcome::Unauthorized)
        );

        let mut b = binding();
        b.request_id = Uuid::from_u128(99);
        assert_eq!(
            base.first_mismatch(&b),
            Some(ConfirmationOutcome::Unauthorized)
        );
    }

    #[test]
    fn workspace_checked_before_actor() {
        let base = binding();
        let mut b = binding();
        b.workspace = "/other".into();
        b.actor_id = "user-b".into();
        // Both differ; workspace is checked first.
        assert_eq!(
            base.first_mismatch(&b),
            Some(ConfirmationOutcome::WorkspaceMismatch)
        );
    }
}
