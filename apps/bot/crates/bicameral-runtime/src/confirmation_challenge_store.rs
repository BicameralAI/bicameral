//! Daemon-owned, in-memory `ConfirmationChallenge` store (issue #829).
//!
//! This store is the sole authority that mints, validates, single-use-consumes,
//! and cancels [`ConfirmationChallenge`](bicameral_api::confirmation_challenge)
//! grants for candidate promotion. It lives entirely in daemon memory for
//! alpha: there is no durable persistence, and a daemon restart (modeled by
//! [`ConfirmationChallengeStore::restart`], or simply by constructing a new
//! store) invalidates every pending challenge.
//!
//! Security properties enforced here:
//!
//! - **Unpredictable value.** Each challenge carries a 256-bit secret drawn from
//!   the platform CSPRNG (`ring::rand::SystemRandom`), hex-encoded for transport.
//! - **Single use.** A successful consume flips the challenge to `Consumed`; any
//!   later presentation fails closed.
//! - **Five-minute validity.** Expiry is checked against a caller-supplied clock
//!   so tests are deterministic.
//! - **Restart invalidation.** Every challenge and idempotency record is tagged
//!   with a per-process `restart_epoch`; a token minted under a prior epoch is
//!   rejected as `challenge_unavailable_after_restart`.
//! - **No value leakage.** Secrets are compared in constant time and never
//!   logged; the store holds them in a redacting [`ChallengeSecret`] wrapper.
//! - **Idempotent apply.** The confirmed operation's canonical result is cached
//!   by `request_id`, so a lost-response retry returns the prior result without
//!   consuming a second grant or duplicating the transition.

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};
use ring::rand::{SecureRandom, SystemRandom};
use uuid::Uuid;

use bicameral_api::confirmation_challenge::{
    ChallengeBinding, ChallengeSecret, ConfirmationOutcome, ConfirmationRejection,
    ConfirmationRequiredResponse, ConfirmationToken, CONFIRMATION_CHALLENGE_PROTOCOL_VERSION,
    CONFIRMATION_CHALLENGE_TTL_SECONDS,
};
use bicameral_api::recall::RecallPromoteCandidateResult;

/// Number of random bytes in a challenge secret (256 bits).
const CHALLENGE_SECRET_BYTES: usize = 32;

/// Lifecycle status of a stored challenge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChallengeStatus {
    Pending,
    Consumed,
    Cancelled,
    Expired,
}

/// A challenge held in daemon memory.
#[derive(Debug)]
struct StoredChallenge {
    secret: ChallengeSecret,
    binding: ChallengeBinding,
    expires_at: DateTime<Utc>,
    status: ChallengeStatus,
}

/// Successful consume outcome: the caller may now apply the bound transition.
#[derive(Debug, Clone)]
pub struct ConsumedChallenge {
    /// The tuple the challenge was bound to (echoes back the recorded binding).
    pub binding: ChallengeBinding,
}

/// In-memory confirmation-challenge authority.
///
/// Shared behind `&self` (all mutation is internally synchronized) so a single
/// daemon-owned instance can be threaded through dispatch.
pub struct ConfirmationChallengeStore {
    /// Per-process epoch. Rotated on restart; embedded in every issued token.
    restart_epoch: Uuid,
    rng: SystemRandom,
    inner: Mutex<StoreInner>,
}

#[derive(Default)]
struct StoreInner {
    challenges: HashMap<Uuid, StoredChallenge>,
    /// Applied canonical results keyed by the confirmed operation's request id.
    applied: HashMap<Uuid, RecallPromoteCandidateResult>,
}

impl std::fmt::Debug for ConfirmationChallengeStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never render secrets or bindings; expose only coarse counts.
        let inner = self.inner.lock().expect("challenge store poisoned");
        f.debug_struct("ConfirmationChallengeStore")
            .field("restart_epoch", &self.restart_epoch)
            .field("pending_challenges", &inner.challenges.len())
            .field("applied_results", &inner.applied.len())
            .finish()
    }
}

impl Default for ConfirmationChallengeStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfirmationChallengeStore {
    /// Construct a fresh store with a new restart epoch and empty state.
    pub fn new() -> Self {
        Self {
            restart_epoch: Uuid::new_v4(),
            rng: SystemRandom::new(),
            inner: Mutex::new(StoreInner::default()),
        }
    }

    /// The current restart epoch. Exposed for diagnostics/tests.
    pub fn restart_epoch(&self) -> Uuid {
        self.restart_epoch
    }

    /// Simulate a daemon restart in-place: rotate the epoch and drop all
    /// pending challenges and cached results.
    ///
    /// Constructing a new store has the same effect; this variant lets a test
    /// (or a live daemon reusing the handle) invalidate state without swapping
    /// the instance. Tokens minted under the prior epoch then fail closed with
    /// [`ConfirmationOutcome::ChallengeUnavailableAfterRestart`].
    pub fn restart(&mut self) {
        self.restart_epoch = Uuid::new_v4();
        let mut inner = self.inner.lock().expect("challenge store poisoned");
        inner.challenges.clear();
        inner.applied.clear();
    }

    /// Mint a new challenge bound to `binding`, valid for five minutes from
    /// `now`. Returns the typed `confirmation_required` response the daemon
    /// hands back before any canonical write.
    pub fn issue(
        &self,
        binding: ChallengeBinding,
        now: DateTime<Utc>,
    ) -> ConfirmationRequiredResponse {
        let challenge_id = Uuid::new_v4();
        let secret = self.mint_secret();
        let expires_at = now + Duration::seconds(CONFIRMATION_CHALLENGE_TTL_SECONDS);

        let stored = StoredChallenge {
            secret: secret.clone(),
            binding: binding.clone(),
            expires_at,
            status: ChallengeStatus::Pending,
        };
        {
            let mut inner = self.inner.lock().expect("challenge store poisoned");
            inner.challenges.insert(challenge_id, stored);
        }

        ConfirmationRequiredResponse {
            protocol_version: CONFIRMATION_CHALLENGE_PROTOCOL_VERSION,
            challenge_id,
            restart_epoch: self.restart_epoch,
            secret,
            expires_at,
            prompt: confirmation_prompt(&binding),
            binding,
        }
    }

    /// Return a previously applied canonical result for `request_id`, if any.
    ///
    /// Used for idempotent lost-response retries: the confirmed apply caches its
    /// result here so a retry with the same `request_id` returns it without
    /// consuming a second grant.
    pub fn applied_result(&self, request_id: Uuid) -> Option<RecallPromoteCandidateResult> {
        let inner = self.inner.lock().expect("challenge store poisoned");
        inner.applied.get(&request_id).cloned()
    }

    /// Record the canonical result of a confirmed apply, keyed by `request_id`.
    pub fn record_applied(&self, request_id: Uuid, result: RecallPromoteCandidateResult) {
        let mut inner = self.inner.lock().expect("challenge store poisoned");
        inner.applied.insert(request_id, result);
    }

    /// Explicitly cancel a pending challenge. Idempotent; unknown or
    /// already-terminal challenges are left untouched.
    pub fn cancel(&self, challenge_id: Uuid) {
        let mut inner = self.inner.lock().expect("challenge store poisoned");
        if let Some(c) = inner.challenges.get_mut(&challenge_id) {
            if c.status == ChallengeStatus::Pending {
                c.status = ChallengeStatus::Cancelled;
            }
        }
    }

    /// Validate and single-use-consume a challenge presented by a confirmed
    /// resubmission.
    ///
    /// `presented` is the binding derived from the resubmission (authority +
    /// params + request id). On success the challenge is flipped to `Consumed`
    /// and the recorded binding is returned so the caller can apply the
    /// permitted transition. Every failure path returns a typed, fail-closed
    /// [`ConfirmationRejection`] and never echoes the secret.
    ///
    /// Note: target eligibility (candidate promotable / packet currentness) is
    /// re-checked by the caller against replayed canonical state; this method
    /// owns only challenge authority.
    pub fn consume(
        &self,
        token: &ConfirmationToken,
        presented: &ChallengeBinding,
        now: DateTime<Utc>,
    ) -> Result<ConsumedChallenge, ConfirmationRejection> {
        // Restart invalidation: a token minted under a prior epoch cannot be
        // honored. Checked first so post-restart replays never touch state.
        if token.restart_epoch != self.restart_epoch {
            return Err(ConfirmationRejection::new(
                ConfirmationOutcome::ChallengeUnavailableAfterRestart,
                "daemon restarted; pending challenge is no longer valid",
            ));
        }

        let mut inner = self.inner.lock().expect("challenge store poisoned");
        let challenge = match inner.challenges.get_mut(&token.challenge_id) {
            Some(c) => c,
            // Unknown id under the current epoch: fail closed as if consumed.
            None => {
                return Err(ConfirmationRejection::new(
                    ConfirmationOutcome::AlreadyConsumed,
                    "challenge is not available",
                ));
            }
        };

        match challenge.status {
            ChallengeStatus::Consumed => {
                return Err(ConfirmationRejection::new(
                    ConfirmationOutcome::AlreadyConsumed,
                    "challenge was already consumed",
                ));
            }
            ChallengeStatus::Cancelled => {
                return Err(ConfirmationRejection::new(
                    ConfirmationOutcome::Cancelled,
                    "challenge was cancelled",
                ));
            }
            ChallengeStatus::Expired => {
                return Err(ConfirmationRejection::new(
                    ConfirmationOutcome::Expired,
                    "challenge has expired",
                ));
            }
            ChallengeStatus::Pending => {}
        }

        // Expiry: a pending-but-elapsed challenge fails closed and is marked so
        // subsequent presentations stay typed as expired.
        if now >= challenge.expires_at {
            challenge.status = ChallengeStatus::Expired;
            return Err(ConfirmationRejection::new(
                ConfirmationOutcome::Expired,
                "challenge has expired",
            ));
        }

        // Binding checks: cross-workspace/actor/packet/candidate/outcome/session
        // /request-id misuse fails closed with the typed mismatch. Checked
        // before the secret so a stolen secret presented against the wrong
        // context still reveals only the (non-secret) mismatch.
        if let Some(outcome) = challenge.binding.first_mismatch(presented) {
            return Err(ConfirmationRejection::new(
                outcome,
                "presented context does not match the challenge binding",
            ));
        }

        // Secret verification (constant time). A wrong/forged/stolen-but-wrong
        // value is an unauthorized attempt.
        if !challenge.secret.matches(&token.secret) {
            return Err(ConfirmationRejection::new(
                ConfirmationOutcome::Unauthorized,
                "challenge value did not verify",
            ));
        }

        challenge.status = ChallengeStatus::Consumed;
        Ok(ConsumedChallenge {
            binding: challenge.binding.clone(),
        })
    }

    fn mint_secret(&self) -> ChallengeSecret {
        let mut bytes = [0u8; CHALLENGE_SECRET_BYTES];
        self.rng
            .fill(&mut bytes)
            .expect("system CSPRNG must be available to mint a confirmation challenge");
        ChallengeSecret::from_minted(hex_encode(&bytes))
    }
}

/// Human-readable confirmation prompt. Contains no secret.
fn confirmation_prompt(binding: &ChallengeBinding) -> String {
    format!(
        "Confirm {outcome:?} for candidate {candidate} in workspace {workspace}.",
        outcome = binding.requested_outcome,
        candidate = binding.candidate_id,
        workspace = binding.workspace,
    )
}

/// Lowercase hex encoding of `bytes`.
fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_api::recall::{PromotionOutcome, PromotionRequestOutcome};

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

    fn token_from(resp: &ConfirmationRequiredResponse) -> ConfirmationToken {
        ConfirmationToken {
            challenge_id: resp.challenge_id,
            restart_epoch: resp.restart_epoch,
            secret: resp.secret.clone(),
        }
    }

    #[test]
    fn issue_returns_bound_challenge_and_writes_no_result() {
        let store = ConfirmationChallengeStore::new();
        let now = Utc::now();
        let resp = store.issue(binding(), now);
        assert_eq!(
            resp.protocol_version,
            CONFIRMATION_CHALLENGE_PROTOCOL_VERSION
        );
        assert_eq!(resp.binding, binding());
        assert_eq!(
            resp.expires_at,
            now + Duration::seconds(CONFIRMATION_CHALLENGE_TTL_SECONDS)
        );
        assert!(store.applied_result(binding().request_id).is_none());
    }

    #[test]
    fn issued_secrets_are_unique_and_hex() {
        let store = ConfirmationChallengeStore::new();
        let now = Utc::now();
        let a = store.issue(binding(), now);
        let b = store.issue(binding(), now);
        let sa = a.secret.expose_for_transport();
        let sb = b.secret.expose_for_transport();
        assert_ne!(sa, sb, "secrets must be unpredictable/unique");
        assert_eq!(sa.len(), CHALLENGE_SECRET_BYTES * 2);
        assert!(sa.bytes().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn valid_confirmation_consumes_exactly_once() {
        let store = ConfirmationChallengeStore::new();
        let now = Utc::now();
        let resp = store.issue(binding(), now);
        let token = token_from(&resp);

        let ok = store
            .consume(&token, &binding(), now)
            .expect("first consume");
        assert_eq!(ok.binding, binding());

        let err = store
            .consume(&token, &binding(), now)
            .expect_err("second consume must fail");
        assert_eq!(err.outcome, ConfirmationOutcome::AlreadyConsumed);
    }

    #[test]
    fn expiry_after_five_minutes() {
        let store = ConfirmationChallengeStore::new();
        let now = Utc::now();
        let resp = store.issue(binding(), now);
        let token = token_from(&resp);
        let later = now + Duration::seconds(CONFIRMATION_CHALLENGE_TTL_SECONDS + 1);
        let err = store
            .consume(&token, &binding(), later)
            .expect_err("expired challenge must fail");
        assert_eq!(err.outcome, ConfirmationOutcome::Expired);
    }

    #[test]
    fn cancellation_fails_closed() {
        let store = ConfirmationChallengeStore::new();
        let now = Utc::now();
        let resp = store.issue(binding(), now);
        store.cancel(resp.challenge_id);
        let err = store
            .consume(&token_from(&resp), &binding(), now)
            .expect_err("cancelled challenge must fail");
        assert_eq!(err.outcome, ConfirmationOutcome::Cancelled);
    }

    #[test]
    fn restart_invalidates_pending_challenge() {
        let mut store = ConfirmationChallengeStore::new();
        let now = Utc::now();
        let resp = store.issue(binding(), now);
        let token = token_from(&resp);
        store.restart();
        let err = store
            .consume(&token, &binding(), now)
            .expect_err("post-restart challenge must fail");
        assert_eq!(
            err.outcome,
            ConfirmationOutcome::ChallengeUnavailableAfterRestart
        );
    }

    #[test]
    fn workspace_mismatch_is_typed() {
        let store = ConfirmationChallengeStore::new();
        let now = Utc::now();
        let resp = store.issue(binding(), now);
        let mut presented = binding();
        presented.workspace = "/evil/workspace".into();
        let err = store
            .consume(&token_from(&resp), &presented, now)
            .expect_err("cross-workspace use must fail");
        assert_eq!(err.outcome, ConfirmationOutcome::WorkspaceMismatch);
    }

    #[test]
    fn actor_mismatch_is_typed() {
        let store = ConfirmationChallengeStore::new();
        let now = Utc::now();
        let resp = store.issue(binding(), now);
        let mut presented = binding();
        presented.actor_id = "user-thief".into();
        let err = store
            .consume(&token_from(&resp), &presented, now)
            .expect_err("actor mismatch must fail");
        assert_eq!(err.outcome, ConfirmationOutcome::ActorMismatch);
    }

    #[test]
    fn packet_mismatch_is_typed() {
        let store = ConfirmationChallengeStore::new();
        let now = Utc::now();
        let resp = store.issue(binding(), now);
        let mut presented = binding();
        presented.packet_id = Uuid::from_u128(0xDEAD);
        let err = store
            .consume(&token_from(&resp), &presented, now)
            .expect_err("packet mismatch must fail");
        assert_eq!(err.outcome, ConfirmationOutcome::PacketMismatch);
    }

    #[test]
    fn candidate_mismatch_is_typed() {
        let store = ConfirmationChallengeStore::new();
        let now = Utc::now();
        let resp = store.issue(binding(), now);
        let mut presented = binding();
        presented.candidate_id = Uuid::from_u128(0xBEEF);
        let err = store
            .consume(&token_from(&resp), &presented, now)
            .expect_err("candidate mismatch must fail");
        assert_eq!(err.outcome, ConfirmationOutcome::CandidateMismatch);
    }

    #[test]
    fn stolen_secret_wrong_context_reveals_only_mismatch() {
        // Theft/replay: an attacker with the real secret but a different
        // workspace/actor cannot consume it; they get a binding mismatch, not a
        // success.
        let store = ConfirmationChallengeStore::new();
        let now = Utc::now();
        let resp = store.issue(binding(), now);
        let mut presented = binding();
        presented.workspace = "/attacker".into();
        presented.actor_id = "user-thief".into();
        let err = store
            .consume(&token_from(&resp), &presented, now)
            .expect_err("theft must fail");
        assert_eq!(err.outcome, ConfirmationOutcome::WorkspaceMismatch);
    }

    #[test]
    fn wrong_secret_is_unauthorized() {
        let store = ConfirmationChallengeStore::new();
        let now = Utc::now();
        let resp = store.issue(binding(), now);
        let forged = ConfirmationToken {
            challenge_id: resp.challenge_id,
            restart_epoch: resp.restart_epoch,
            secret: ChallengeSecret::from_minted("deadbeef"),
        };
        let err = store
            .consume(&forged, &binding(), now)
            .expect_err("forged secret must fail");
        assert_eq!(err.outcome, ConfirmationOutcome::Unauthorized);
    }

    #[test]
    fn outcome_mismatch_is_typed() {
        let store = ConfirmationChallengeStore::new();
        let now = Utc::now();
        let resp = store.issue(binding(), now);
        let mut presented = binding();
        presented.requested_outcome = PromotionOutcome::DismissedAsNoise;
        let err = store
            .consume(&token_from(&resp), &presented, now)
            .expect_err("outcome mismatch must fail");
        assert_eq!(err.outcome, ConfirmationOutcome::OutcomeMismatch);
    }

    #[test]
    fn unknown_challenge_fails_closed() {
        let store = ConfirmationChallengeStore::new();
        let now = Utc::now();
        let token = ConfirmationToken {
            challenge_id: Uuid::from_u128(777),
            restart_epoch: store.restart_epoch(),
            secret: ChallengeSecret::from_minted("nope"),
        };
        let err = store
            .consume(&token, &binding(), now)
            .expect_err("unknown challenge must fail");
        assert_eq!(err.outcome, ConfirmationOutcome::AlreadyConsumed);
    }

    #[test]
    fn applied_result_roundtrips_for_idempotency() {
        let store = ConfirmationChallengeStore::new();
        let result = RecallPromoteCandidateResult {
            candidate_id: Uuid::from_u128(2),
            outcome: PromotionRequestOutcome::Promoted,
            decision_id: Some(Uuid::from_u128(42)),
            review_state: None,
            confirmation_required: None,
            confirmation_rejection: None,
            trace_ref: None,
        };
        store.record_applied(Uuid::from_u128(3), result.clone());
        let got = store.applied_result(Uuid::from_u128(3)).expect("cached");
        assert_eq!(got.decision_id, result.decision_id);
    }

    #[test]
    fn debug_never_leaks_secret() {
        let store = ConfirmationChallengeStore::new();
        let resp = store.issue(binding(), Utc::now());
        let raw = resp.secret.expose_for_transport().to_string();
        let dbg = format!("{store:?}");
        assert!(!dbg.contains(&raw), "store debug must not leak secret");
    }
}
