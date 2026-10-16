//! Session-scoped `CandidateSetV1` lifecycle contract for Managed MCP
//! (issue [#986], governed spec revision `04e0471a`).
//!
//! [#986]: https://github.com/BicameralAI/bicameral-bot/issues/986
//!
//! These are the protocol types for Slice B of
//! `docs/specs/managed-mcp-drift-aware-continuation-runtime-architecture.md`.
//! The daemon-owned production service that enforces them lives in
//! `bicameral_runtime::candidate_session_store`.
//!
//! # What this state is, and is not
//!
//! A [`CandidateSetV1`] is **transient daemon-owned working state** for exactly
//! one Product and one host coding session. It is `candidate_or_proposal`
//! state in the governed state classification (`STATE-CANDIDATE-SET`), never
//! canonical Product state:
//!
//! - it is routed by `(product_id, host_session_id, candidate_set_generation)`,
//!   never by Product history;
//! - it is never replayed as canonical state;
//! - a [`CandidateDisposition::Rejected`] entry is a session disposition, not a
//!   canonical rejection event, tombstone, or suppression record;
//! - a [`CandidateDisposition::Promoted`] entry is session UI-reconciliation
//!   state only. Canonical authority comes solely from the separate authorized
//!   append flow (Bot #987), which is out of scope here.
//!
//! Session end and lease expiry **erase** this state. Erasure is not rejection:
//! it writes no event, mints no tombstone, and creates no future suppression.
//!
//! # Identity is bound, never inferred
//!
//! Every set binds an exact [`CandidateSetBindingV1`]. Product and session
//! identity are supplied by the caller and matched exactly. Nothing in this
//! contract permits deriving identity from the current working directory,
//! transcript content, agent prose, global active-Product state, or prior
//! prompt content.
//!
//! # Conformance
//!
//! - `CONF-MCP-CANDIDATE-SESSION-01` — isolate by Product/session, preserve
//!   through matching reconnect.
//! - `CONF-MCP-SESSION-REJECTION-01` — explicit rejection changes session state
//!   only.
//! - `CONF-MCP-RECONNECT-CLEANUP-01` — resume within 300 seconds; erase on end
//!   or expiry.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Contract constants ────────────────────────────────────────────────────

/// Version discriminator carried by every payload in this contract.
///
/// The governed spec requires that "every new payload has a version
/// discriminator" and that "older clients receive `unsupported_contract_version`".
/// A request carrying any other value fails closed with
/// [`CandidateSessionFailure::UnsupportedContractVersion`].
pub const CANDIDATE_SESSION_CONTRACT_VERSION: u32 = 1;

/// Reconnect grace after the last valid heartbeat, in seconds.
///
/// Alpha policy default from governed spec §3 ("reconnect grace after the last
/// valid heartbeat: 300 seconds").
pub const CANDIDATE_SESSION_RECONNECT_GRACE_SECONDS: i64 = 300;

/// Wire name of the lease-extension operation (`MCP → daemon`).
pub const CANDIDATE_SESSION_HEARTBEAT_COMMAND: &str = "candidate_session.heartbeat";

/// Wire name of the explicit session-end operation (`Host/MCP → daemon`).
pub const CANDIDATE_SESSION_END_COMMAND: &str = "candidate_session.end";

// ── Typed failures ────────────────────────────────────────────────────────

/// Typed, fail-closed outcomes for every candidate-session operation.
///
/// Per the governed spec's error contract, no failure may be translated into
/// `no_candidate`, `no_conflict`, approval, readiness, or release safety.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateSessionFailure {
    /// The payload's `contract_version` is not
    /// [`CANDIDATE_SESSION_CONTRACT_VERSION`].
    UnsupportedContractVersion,

    /// A required identity field was empty, a candidate graph was
    /// ambiguous or malformed, or a digest field was not a well-formed
    /// `sha256:<64 lowercase hex>` value.
    ///
    /// Malformed identities never fall back to a "best match" lookup.
    MalformedIdentity,

    /// No CandidateSet is reachable for the presented identity.
    ///
    /// This is deliberately the **only** answer a non-owning caller can
    /// receive. A different Product or host session gets exactly this outcome
    /// whether or not an unrelated set exists, so probing cannot reveal the
    /// existence of another session's candidates.
    CandidateSetNotFound,

    /// The presented `candidate_set_generation` is not the active generation
    /// for this Product and session. Stale generations fail closed; they never
    /// silently retarget the newer set.
    CandidateSetStale,

    /// A referenced `candidate_id` does not belong to the addressed set.
    CandidateNotInSet,

    /// The set's reconnect lease elapsed before this operation. The transient
    /// state is erased as part of returning this failure.
    SessionLeaseExpired,
}

impl CandidateSessionFailure {
    /// Stable snake_case wire code.
    pub fn code(self) -> &'static str {
        match self {
            Self::UnsupportedContractVersion => "unsupported_contract_version",
            Self::MalformedIdentity => "malformed_identity",
            Self::CandidateSetNotFound => "candidate_set_not_found",
            Self::CandidateSetStale => "candidate_set_stale",
            Self::CandidateNotInSet => "candidate_not_in_set",
            Self::SessionLeaseExpired => "session_lease_expired",
        }
    }
}

impl std::fmt::Display for CandidateSessionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for CandidateSessionFailure {}

// ── Identity binding ──────────────────────────────────────────────────────

/// The exact analysis identity a [`CandidateSetV1`] is bound to.
///
/// Two `create` requests carrying an equal binding address the *same* analysis
/// identity and therefore reuse one generation. Any change to the governing
/// spec binding, Plan digest, or accepted-model base is a different analysis
/// identity and opens a new generation with fresh `pending` dispositions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateSetBindingV1 {
    /// Exact Product identity. Never inferred from working directory.
    pub product_id: String,

    /// Exact host coding-session identity. Never inferred from transcript or
    /// agent prose.
    pub host_session_id: String,

    /// `sha256:` digest of the exact governing spec binding.
    pub governing_spec_binding_digest: String,

    /// `sha256:` digest of the exact completed Plan bytes.
    pub plan_digest: String,

    /// Accepted Product model base position this analysis was computed against.
    pub accepted_model_base_position: u64,

    /// `sha256:` digest of the accepted Product model at that position.
    pub accepted_model_base_digest: String,
}

// ── Candidates ────────────────────────────────────────────────────────────

/// Session disposition of one candidate.
///
/// All three values are transient session working state. `Rejected` asserts
/// nothing about semantic truth and `Promoted` grants no canonical authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateDisposition {
    /// Not yet acted on in this session.
    Pending,

    /// Explicitly rejected **in this session only**.
    ///
    /// Writes no `claim_set_candidate_rejected` event, creates no cross-session
    /// tombstone or suppression, does not assert the candidate is semantically
    /// false, and does not prevent the same idea from being proposed again in a
    /// future session. Erased with the set at session end.
    Rejected,

    /// Promoted in session UI-reconciliation state.
    ///
    /// This records that a separate authorized append flow reported success so
    /// the session surface can reconcile. It is not itself canonical authority
    /// and is erased with the set at session end; the canonical Decision, if
    /// one was appended, lives in Product history and is untouched by cleanup.
    Promoted,
}

/// A minimal source span reference retained on a session candidate.
///
/// Span/candidate projection semantics are owned by
/// `CONF-MCP-SPAN-PROJECTION-01` (Slice C). This slice carries spans only so
/// they participate in candidate identity and survive reconnect unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpanV1 {
    /// Normalized repo-relative path within the bound governing spec.
    pub artifact_path: String,
    /// Inclusive start byte offset.
    pub start_byte: u64,
    /// Exclusive end byte offset.
    pub end_byte: u64,
}

/// One session-scoped DecisionCandidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCandidateV1 {
    pub candidate_id: Uuid,
    pub candidate_generation: u64,
    pub proposed_decision: String,
    #[serde(default)]
    pub source_spans: Vec<SourceSpanV1>,
    #[serde(default)]
    pub contradiction_refs: Vec<String>,
    #[serde(default)]
    pub dependency_candidate_ids: Vec<Uuid>,
    /// `sha256:` digest of the validated ClaimTrace fragment.
    pub claim_trace_digest: String,
    /// `sha256:` digest of the exact proposed effect.
    pub proposal_digest: String,
    /// Transient session disposition. Not part of the set digest.
    pub disposition: CandidateDisposition,
}

/// Daemon-authored input describing one candidate at set-creation time.
///
/// Disposition is not accepted from the caller: a newly materialized candidate
/// is always [`CandidateDisposition::Pending`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCandidateDraftV1 {
    pub candidate_id: Uuid,
    pub proposed_decision: String,
    #[serde(default)]
    pub source_spans: Vec<SourceSpanV1>,
    #[serde(default)]
    pub contradiction_refs: Vec<String>,
    #[serde(default)]
    pub dependency_candidate_ids: Vec<Uuid>,
    pub claim_trace_digest: String,
    pub proposal_digest: String,
}

// ── CandidateSet ──────────────────────────────────────────────────────────

/// A session-scoped CandidateSet.
///
/// Transient working state. See the module docs for the authority boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateSetV1 {
    pub contract_version: u32,
    pub candidate_set_id: Uuid,
    pub candidate_set_generation: u64,
    /// `sha256:` digest over ordered candidate identities and relations.
    ///
    /// Deliberately excludes dispositions, so the digest a selection binds to
    /// stays stable while candidates move between pending/rejected/promoted
    /// within one generation, and is unchanged by reconnect.
    pub candidate_set_digest: String,
    /// Exact analysis identity this set is bound to.
    pub binding: CandidateSetBindingV1,
    /// Opaque lease identity. A reconnect must present this exact value.
    pub session_lease_id: String,
    pub created_at: DateTime<Utc>,
    /// Last heartbeat plus [`CANDIDATE_SESSION_RECONNECT_GRACE_SECONDS`].
    pub lease_expires_at: DateTime<Utc>,
    pub candidates: Vec<SessionCandidateV1>,
    #[serde(default)]
    pub coverage_limitations: Vec<String>,
}

impl CandidateSetV1 {
    /// Count of candidates currently in `disposition`.
    pub fn count_with(&self, disposition: CandidateDisposition) -> usize {
        self.candidates
            .iter()
            .filter(|c| c.disposition == disposition)
            .count()
    }
}

// ── Operation payloads ────────────────────────────────────────────────────

/// Identity presented by every non-creating operation.
///
/// The daemon matches all four fields exactly. A mismatch on Product, host
/// session, or lease yields [`CandidateSessionFailure::CandidateSetNotFound`];
/// a mismatch on generation yields [`CandidateSessionFailure::CandidateSetStale`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateSessionAccessV1 {
    pub contract_version: u32,
    pub product_id: String,
    pub host_session_id: String,
    pub session_lease_id: String,
    pub candidate_set_generation: u64,
}

impl CandidateSessionAccessV1 {
    /// Build an access payload at the current contract version.
    pub fn new(
        product_id: impl Into<String>,
        host_session_id: impl Into<String>,
        session_lease_id: impl Into<String>,
        candidate_set_generation: u64,
    ) -> Self {
        Self {
            contract_version: CANDIDATE_SESSION_CONTRACT_VERSION,
            product_id: product_id.into(),
            host_session_id: host_session_id.into(),
            session_lease_id: session_lease_id.into(),
            candidate_set_generation,
        }
    }
}

/// Request for `candidate_session.heartbeat`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateSessionHeartbeatRequestV1 {
    pub access: CandidateSessionAccessV1,
}

/// Result of `candidate_session.heartbeat`.
///
/// Carries no candidate bodies: a heartbeat extends a lease, it does not
/// re-enumerate review content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateSessionHeartbeatResultV1 {
    pub contract_version: u32,
    pub candidate_set_id: Uuid,
    pub candidate_set_generation: u64,
    pub candidate_set_digest: String,
    pub lease_expires_at: DateTime<Utc>,
    pub pending_count: usize,
    pub rejected_count: usize,
    pub promoted_count: usize,
}

/// Request for `candidate_session.end`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateSessionEndRequestV1 {
    pub access: CandidateSessionAccessV1,
}

/// Result of `candidate_session.end`.
///
/// Reports what was erased. Erasure is not rejection: the counts below describe
/// deleted transient state and correspond to no canonical write of any kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateSessionEndResultV1 {
    pub contract_version: u32,
    pub candidate_set_id: Uuid,
    pub candidate_set_generation: u64,
    /// Total candidates erased, in every disposition.
    pub erased_candidate_count: usize,
    pub erased_pending_count: usize,
    /// Erased session rejections. These were never canonical and leave nothing
    /// behind — no tombstone, no suppression, no future effect.
    pub erased_rejected_count: usize,
    /// Erased transient promotion/UI-reconciliation entries. Any canonical
    /// Decision appended by the separate authorized flow is untouched.
    pub erased_promoted_disposition_count: usize,
}

/// Outcome of a disposition transition (reject or promote-in-session).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateDispositionResultV1 {
    pub contract_version: u32,
    pub candidate_set_id: Uuid,
    pub candidate_set_generation: u64,
    /// Unchanged by a disposition transition; echoed so callers can verify.
    pub candidate_set_digest: String,
    pub applied: Vec<Uuid>,
    pub pending_count: usize,
    pub rejected_count: usize,
    pub promoted_count: usize,
}

/// Well-formedness check for a `sha256:<64 lowercase hex>` digest field.
pub fn is_sha256_digest(value: &str) -> bool {
    match value.strip_prefix("sha256:") {
        Some(hex) => hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_codes_are_stable_wire_strings() {
        assert_eq!(
            CandidateSessionFailure::UnsupportedContractVersion.code(),
            "unsupported_contract_version"
        );
        assert_eq!(
            CandidateSessionFailure::SessionLeaseExpired.code(),
            "session_lease_expired"
        );
        assert_eq!(
            CandidateSessionFailure::CandidateSetStale.code(),
            "candidate_set_stale"
        );
        assert_eq!(
            CandidateSessionFailure::CandidateNotInSet.code(),
            "candidate_not_in_set"
        );
    }

    #[test]
    fn reconnect_grace_matches_governed_policy() {
        assert_eq!(CANDIDATE_SESSION_RECONNECT_GRACE_SECONDS, 300);
    }

    #[test]
    fn command_names_match_governed_contract_table() {
        assert_eq!(
            CANDIDATE_SESSION_HEARTBEAT_COMMAND,
            "candidate_session.heartbeat"
        );
        assert_eq!(CANDIDATE_SESSION_END_COMMAND, "candidate_session.end");
    }

    #[test]
    fn digest_shape_is_validated() {
        assert!(is_sha256_digest(&format!("sha256:{}", "a".repeat(64))));
        assert!(!is_sha256_digest(&format!("sha256:{}", "A".repeat(64))));
        assert!(!is_sha256_digest(&format!("sha256:{}", "a".repeat(63))));
        assert!(!is_sha256_digest(&"a".repeat(64)));
        assert!(!is_sha256_digest("sha256:"));
    }
}
