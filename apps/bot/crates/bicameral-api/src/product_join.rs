//! `product.join` — canonical same-domain Product Member auto-join contract.
//!
//! Implements the daemon-authoritative verification and decision logic for the
//! onboarding contract in `docs/specs/domain-product-onboarding-contract.md`
//! (issue #765) and the runtime slice #767.
//!
//! A Cloud account whose verified email domain matches a Product's active
//! auto-join policy can become a canonical Product Member without owner review.
//! Cloud proves hosted identity and eligibility with a narrow, signed,
//! short-lived [`ProductJoinTicket`]; the paired daemon supplies locally
//! confirmed actor context, re-evaluates the applicable policy in canonical
//! state, and writes the canonical membership event.
//!
//! Authority boundary (fail-closed):
//!
//! - The daemon never accepts a raw Supabase/session token, an SPA-asserted
//!   actor, or Cloud membership state as canonical authority. The ticket
//!   carries only the minimum bounded claims (see [`ProductJoinTicket`]); any
//!   claim outside that set fails deserialization.
//! - The daemon binds the ticket account to the paired session and locally
//!   confirmed actor context and rejects mismatches before any mutation.
//! - The daemon re-evaluates the Product's active auto-join policy in canonical
//!   state rather than trusting the ticket alone.
//! - On success the daemon grants **Product Member only** — never Policy Owner
//!   or Product Owner — and appends a single canonical membership event.
//! - There is no durable pending-approval / join-request / invite state.
//!
//! This module is intentionally crypto-free: signature verification is injected
//! as [`TicketAuthenticity`] so the pure decision function [`evaluate_join`] is
//! fully unit-testable. The concrete Ed25519 verifier lives in the runtime.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// ProductJoinTicket — minimum bounded claims
// ---------------------------------------------------------------------------

/// Narrow, signed, short-lived Cloud assertion authorizing one eligible account
/// to request daemon membership for one Product.
///
/// Carries only the minimum bounded claims. `deny_unknown_fields` makes the
/// daemon fail closed on any claim outside this set: it never carries the full
/// hosted (Supabase) session token, refresh tokens, roles, or membership
/// assertions. Matches `protocol/onboarding/schemas/product-join-ticket.schema.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductJoinTicket {
    /// Hosted issuer identity (who minted the ticket).
    pub issuer: String,
    /// Intended daemon audience for the ticket.
    pub audience: String,
    /// Canonical Product the ticket authorizes a join request for.
    pub product_id: String,
    /// The single eligible Cloud account.
    pub cloud_account_id: String,
    /// Verified identity domain for same-domain eligibility.
    pub verified_domain: String,
    /// When the ticket was minted.
    pub issued_at: DateTime<Utc>,
    /// Short-lived expiry timestamp.
    pub expiry: DateTime<Utc>,
    /// Single-use nonce / JTI for replay detection.
    pub nonce: String,
    /// Opaque reference the daemon uses to correlate the directory row; not
    /// canonical settings.
    pub registration_ref: String,
    /// Optional safe provider hint for setup routing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_hint: Option<ProviderHint>,
    /// Optional safe event-store substrate hint for setup routing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub substrate_hint: Option<SubstrateHint>,
}

/// Optional safe provider hint carried by a [`ProductJoinTicket`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderHint {
    Github,
    Google,
}

/// Optional safe event-store substrate hint carried by a [`ProductJoinTicket`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubstrateHint {
    Git,
    DriveFolder,
}

impl ProductJoinTicket {
    /// Canonical, language-neutral byte serialization of the ticket claims used
    /// as the signing input. Object keys are sorted recursively and the JSON is
    /// emitted compactly so an independent (e.g. TypeScript) signer and this
    /// verifier agree on the exact bytes.
    pub fn signing_bytes(&self) -> Vec<u8> {
        let value = serde_json::to_value(self).expect("ProductJoinTicket serializes to JSON");
        canonical_json_string(&value).into_bytes()
    }

    /// A non-secret, redaction-safe summary of the ticket for receipts and
    /// logs. Deliberately omits the `nonce` (replay-defense secret) and never
    /// includes any signature or token material.
    pub fn redacted_summary(&self) -> RedactedTicketSummary {
        RedactedTicketSummary {
            issuer: self.issuer.clone(),
            audience: self.audience.clone(),
            product_id: self.product_id.clone(),
            cloud_account_id: self.cloud_account_id.clone(),
            verified_domain: self.verified_domain.clone(),
            issued_at: self.issued_at,
            expiry: self.expiry,
        }
    }
}

/// Redaction-safe projection of a [`ProductJoinTicket`] for receipts/logs.
///
/// Never carries the `nonce`, any signature, or any token material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactedTicketSummary {
    pub issuer: String,
    pub audience: String,
    pub product_id: String,
    pub cloud_account_id: String,
    pub verified_domain: String,
    pub issued_at: DateTime<Utc>,
    pub expiry: DateTime<Utc>,
}

/// Recursively canonicalize a JSON value to a compact, sorted-key string.
fn canonical_json_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let sorted: BTreeMap<&String, &serde_json::Value> = map.iter().collect();
            let mut out = String::from("{");
            for (i, (k, v)) in sorted.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(k).expect("string key serializes"));
                out.push(':');
                out.push_str(&canonical_json_string(v));
            }
            out.push('}');
            out
        }
        serde_json::Value::Array(items) => {
            let mut out = String::from("[");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&canonical_json_string(item));
            }
            out.push(']');
            out
        }
        other => serde_json::to_string(other).expect("scalar serializes"),
    }
}

// ---------------------------------------------------------------------------
// Signed ticket + command params (daemon-inbound wire; not the pinned contract)
// ---------------------------------------------------------------------------

/// A [`ProductJoinTicket`] plus the detached signature that authenticates it.
///
/// This envelope is the transport wire shape carried into the `product.join`
/// tool command. It is deliberately **not** part of the pinned cross-repo
/// contract schemas (which model the already-authenticated claims): the
/// signature is verified by the daemon and then discarded, never persisted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedProductJoinTicket {
    /// The bounded ticket claims.
    pub ticket: ProductJoinTicket,
    /// Identifier of the issuer key that signed the ticket.
    pub key_id: String,
    /// Base64 (standard, padded) detached signature over
    /// [`ProductJoinTicket::signing_bytes`].
    pub signature: String,
}

/// Parameters for the `product.join` tool command.
///
/// `local_actor_ref` is the locally confirmed actor context the daemon binds
/// the Cloud account to; it is daemon-derived from the validated pairing, never
/// SPA-supplied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductJoinCommandParams {
    /// Correlates the request/result pair.
    pub request_id: String,
    /// The signed ticket to verify.
    pub signed_ticket: SignedProductJoinTicket,
    /// Locally confirmed actor context bound to the Cloud account.
    pub local_actor_ref: String,
    /// When the surface created the request.
    pub requested_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// ProductJoinResult — daemon-authoritative outcome (pinned contract shape)
// ---------------------------------------------------------------------------

/// Canonical governance role granted on a successful auto-join.
///
/// Auto-join never grants Policy Owner or Product Owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GrantedRole {
    /// The only role a same-domain auto-join can grant.
    #[serde(rename = "Product Member")]
    ProductMember,
}

/// Typed, non-secret failure code for a rejected `product.join`.
///
/// Extends the original five contract codes (issue #765) with the finer typed
/// failures the runtime slice #767 must produce. All values are non-secret and
/// stable; see `protocol/onboarding/schemas/product-join-result.schema.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinRejectionCode {
    /// Ticket signature or issuer could not be authenticated.
    InvalidSignature,
    /// Ticket audience did not match this daemon.
    WrongAudience,
    /// Ticket product is not known to this daemon.
    WrongProduct,
    /// Ticket account did not match the paired/confirmed local actor context.
    WrongAccount,
    /// No confirmed pairing exists for the Cloud account.
    MissingPairing,
    /// The verified domain is not eligible for the Product's auto-join policy.
    WrongDomain,
    /// The active auto-join policy no longer permits this join.
    PolicyChanged,
    /// The ticket has expired.
    ExpiredTicket,
    /// The ticket is not yet valid (issued in the future).
    NotYetValid,
    /// The ticket nonce has already been consumed.
    ReplayedTicket,
}

impl JoinRejectionCode {
    /// Stable wire string (matches the serde `snake_case` rename).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidSignature => "invalid_signature",
            Self::WrongAudience => "wrong_audience",
            Self::WrongProduct => "wrong_product",
            Self::WrongAccount => "wrong_account",
            Self::MissingPairing => "missing_pairing",
            Self::WrongDomain => "wrong_domain",
            Self::PolicyChanged => "policy_changed",
            Self::ExpiredTicket => "expired_ticket",
            Self::NotYetValid => "not_yet_valid",
            Self::ReplayedTicket => "replayed_ticket",
        }
    }

    /// A non-secret, human-readable message. Never includes ticket secrets.
    pub fn message(self) -> &'static str {
        match self {
            Self::InvalidSignature => "ticket signature or issuer could not be authenticated",
            Self::WrongAudience => "ticket audience did not match this daemon",
            Self::WrongProduct => "ticket product is not known to this daemon",
            Self::WrongAccount => "ticket account did not match the confirmed local actor context",
            Self::MissingPairing => "no confirmed pairing exists for the cloud account",
            Self::WrongDomain => "verified domain is not eligible for the product auto-join policy",
            Self::PolicyChanged => "the active auto-join policy no longer permits this join",
            Self::ExpiredTicket => "the ticket has expired",
            Self::NotYetValid => "the ticket is not yet valid",
            Self::ReplayedTicket => "the ticket nonce has already been consumed",
        }
    }
}

/// Typed failure detail attached to a rejected [`ProductJoinResult`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductJoinFailure {
    pub code: JoinRejectionCode,
    pub message: String,
}

/// Outcome discriminant for a [`ProductJoinResult`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinOutcome {
    Granted,
    Rejected,
}

/// Daemon-authoritative outcome of a same-domain `product.join`.
///
/// On success the daemon grants Product Member only and appends a canonical
/// membership event + receipt. On failure it returns a typed rejection and
/// creates no membership. There is no durable pending-approval state.
///
/// Matches `protocol/onboarding/schemas/product-join-result.schema.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductJoinResult {
    pub request_id: String,
    pub product_id: String,
    pub cloud_account_id: String,
    pub outcome: JoinOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub granted_role: Option<GrantedRole>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub membership_event_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<ProductJoinFailure>,
    pub decided_at: DateTime<Utc>,
}

impl ProductJoinResult {
    /// Build a granted result carrying the canonical membership + receipt refs.
    pub fn granted(
        request_id: String,
        product_id: String,
        cloud_account_id: String,
        membership_event_ref: String,
        receipt_id: String,
        decided_at: DateTime<Utc>,
    ) -> Self {
        Self {
            request_id,
            product_id,
            cloud_account_id,
            outcome: JoinOutcome::Granted,
            granted_role: Some(GrantedRole::ProductMember),
            membership_event_ref: Some(membership_event_ref),
            receipt_id: Some(receipt_id),
            failure: None,
            decided_at,
        }
    }

    /// Build a rejected result with a typed, non-secret failure.
    pub fn rejected(
        request_id: String,
        product_id: String,
        cloud_account_id: String,
        code: JoinRejectionCode,
        decided_at: DateTime<Utc>,
    ) -> Self {
        Self {
            request_id,
            product_id,
            cloud_account_id,
            outcome: JoinOutcome::Rejected,
            granted_role: None,
            membership_event_ref: None,
            receipt_id: None,
            failure: Some(ProductJoinFailure {
                code,
                message: code.message().to_string(),
            }),
            decided_at,
        }
    }
}

// ---------------------------------------------------------------------------
// ProductJoinRequest — daemon-inbound request (pinned contract shape)
// ---------------------------------------------------------------------------

/// Daemon-inbound request to join a Product, carrying the already-authenticated
/// bounded [`ProductJoinTicket`] claims plus the locally confirmed actor
/// context the daemon binds the Cloud account to.
///
/// This is the pinned cross-repo contract shape
/// (`protocol/onboarding/schemas/product-join-request.schema.json`): it models
/// the authenticated ticket claims, not the transport signature envelope. The
/// daemon verifies the detached signature carried by
/// [`SignedProductJoinTicket`] and then discards it, so the persisted/inbound
/// contract view carries only the bounded claims. `deny_unknown_fields` keeps
/// it fail-closed against any field outside the pinned shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductJoinRequest {
    /// Correlates the request/result pair.
    pub request_id: String,
    /// The bounded, authenticated ticket claims.
    pub ticket: ProductJoinTicket,
    /// Locally confirmed actor context the Cloud account is bound to before any
    /// daemon mutation. Daemon-derived from the validated pairing, never
    /// SPA-supplied.
    pub local_actor_ref: String,
    /// When the surface created the request.
    pub requested_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Canonical policy + membership state
// ---------------------------------------------------------------------------

/// A Product's active auto-join policy, materialized from canonical events.
///
/// The daemon re-evaluates this in canonical state at join time; it is never
/// trusted from the ticket. The initial policy is replayed from the atomic
/// `CreateProduct` event; later `ProductAutoJoinPolicySet` events replace it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductAutoJoinPolicy {
    pub product_id: String,
    /// Active canonical Governance Policy Version this policy belongs to.
    pub governance_policy_version: String,
    /// Whether same-domain auto-join is currently enabled for the Product.
    pub auto_join_enabled: bool,
    /// Verified domains eligible for same-domain auto-join.
    pub allowed_domains: Vec<String>,
}

impl ProductAutoJoinPolicy {
    fn allows_domain(&self, domain: &str) -> bool {
        self.allowed_domains
            .iter()
            .any(|d| d.eq_ignore_ascii_case(domain))
    }
}

/// Canonical Product Member fact materialized from a `ProductMemberAdded` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductMembership {
    pub product_id: String,
    pub cloud_account_id: String,
    pub local_actor_ref: String,
    pub granted_role: GrantedRole,
    pub verified_domain: String,
    pub governance_policy_version: String,
    /// Canonical membership event reference (the event-store entry id).
    pub membership_event_ref: String,
    pub receipt_id: String,
    pub joined_at: DateTime<Utc>,
}

/// Canonical event payload for a granted Product Member auto-join.
///
/// This is the ONLY durable state written by `product.join`. It embeds the
/// ticket `nonce` as the minimum replay-defense datum (no ticket body, no
/// signature, no token). The nonce is retained for the life of the ledger,
/// which exceeds the bounded ticket lifetime, guaranteeing single-use replay
/// defense across restarts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductMemberAddedPayload {
    pub product_id: String,
    pub cloud_account_id: String,
    pub local_actor_ref: String,
    pub granted_role: GrantedRole,
    pub verified_domain: String,
    pub governance_policy_version: String,
    /// Single-use replay-defense nonce (JTI) from the consumed ticket.
    pub ticket_nonce: String,
    /// Redacted receipt identifier returned to the caller.
    pub receipt_id: String,
    pub decided_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Verification inputs + pure decision function
// ---------------------------------------------------------------------------

/// Result of cryptographic authentication of the ticket, injected into the
/// pure evaluator so this module stays crypto-free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketAuthenticity {
    /// Signature verified against a trusted issuer key.
    Verified,
    /// Signature invalid, key unknown, or issuer untrusted.
    Invalid,
}

/// Audit-metadata key the daemon uses to surface the paired Cloud account id
/// derived from the validated hosted-bridge pairing session. It is set by the
/// daemon from the pairing session, never by the SPA. Both the gateway (which
/// publishes it) and the runtime (which reads it) reference this constant.
pub const PAIRED_CLOUD_ACCOUNT_METADATA_KEY: &str = "paired_cloud_account_id";

/// Daemon-derived pairing context binding a Cloud account to a confirmed local
/// actor. Sourced from the validated pairing session, never from the SPA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingContext {
    pub paired_cloud_account_id: String,
    pub confirmed_local_actor_ref: String,
}

/// All inputs to the pure join decision.
///
/// Everything here is daemon-derived or canonical: the SPA supplies none of it.
pub struct JoinEvaluationInput<'a> {
    pub ticket: &'a ProductJoinTicket,
    pub authenticity: TicketAuthenticity,
    /// The daemon's own expected audience (trust anchor).
    pub expected_audience: &'a str,
    /// Locally confirmed actor context from the command.
    pub local_actor_ref: &'a str,
    /// Daemon-derived pairing, if any.
    pub pairing: Option<&'a PairingContext>,
    /// Current canonical auto-join policy for the ticket's product, if any.
    pub policy: Option<&'a ProductAutoJoinPolicy>,
    /// Existing canonical membership for `(product_id, cloud_account_id)`.
    pub existing_membership: Option<&'a ProductMembership>,
    /// Whether the ticket nonce has already been consumed.
    pub nonce_consumed: bool,
    /// Current time for validity-window checks.
    pub now: DateTime<Utc>,
}

/// The pure decision produced by [`evaluate_join`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinDecision {
    /// A new grant that must materialize a canonical membership event.
    Grant(ProductMemberGrant),
    /// An idempotent no-op: the account is already a member. Return the
    /// existing canonical references; append no event.
    AlreadyMember {
        membership_event_ref: String,
        receipt_id: String,
    },
    /// A typed, fail-closed rejection. Append no event.
    Reject(JoinRejectionCode),
}

/// The daemon-confirmed facts needed to materialize a new membership grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductMemberGrant {
    pub product_id: String,
    pub cloud_account_id: String,
    pub local_actor_ref: String,
    pub verified_domain: String,
    pub governance_policy_version: String,
    pub ticket_nonce: String,
}

/// Evaluate a `product.join` request against daemon-derived and canonical
/// inputs. Pure and fail-closed: any mismatch yields a typed rejection and no
/// membership. See the module docs for the authority boundary.
pub fn evaluate_join(input: &JoinEvaluationInput<'_>) -> JoinDecision {
    let ticket = input.ticket;

    // 1. Authenticate the ticket (signature + trusted issuer).
    if input.authenticity != TicketAuthenticity::Verified {
        return JoinDecision::Reject(JoinRejectionCode::InvalidSignature);
    }

    // 2. The ticket must be addressed to this daemon.
    if ticket.audience != input.expected_audience {
        return JoinDecision::Reject(JoinRejectionCode::WrongAudience);
    }

    // 3. A confirmed pairing must bind the account to a local actor.
    let Some(pairing) = input.pairing else {
        return JoinDecision::Reject(JoinRejectionCode::MissingPairing);
    };

    // 4. The ticket account and confirmed actor context must match.
    if ticket.cloud_account_id != pairing.paired_cloud_account_id
        || input.local_actor_ref != pairing.confirmed_local_actor_ref
    {
        return JoinDecision::Reject(JoinRejectionCode::WrongAccount);
    }

    // 5. Validity window.
    if input.now < ticket.issued_at {
        return JoinDecision::Reject(JoinRejectionCode::NotYetValid);
    }
    if input.now > ticket.expiry {
        return JoinDecision::Reject(JoinRejectionCode::ExpiredTicket);
    }

    // 6. Idempotency: an existing member short-circuits to a successful no-op,
    //    even across duplicate UI retries that reuse the same ticket/nonce.
    if let Some(existing) = input.existing_membership {
        return JoinDecision::AlreadyMember {
            membership_event_ref: existing.membership_event_ref.clone(),
            receipt_id: existing.receipt_id.clone(),
        };
    }

    // 7. Re-evaluate the Product's active auto-join policy in canonical state.
    let Some(policy) = input.policy else {
        return JoinDecision::Reject(JoinRejectionCode::WrongProduct);
    };
    if !policy.auto_join_enabled {
        return JoinDecision::Reject(JoinRejectionCode::PolicyChanged);
    }
    if !policy.allows_domain(&ticket.verified_domain) {
        return JoinDecision::Reject(JoinRejectionCode::WrongDomain);
    }

    // 8. Replay defense: a consumed nonce with no surviving membership is a
    //    replay attempt (the idempotent path above already handled live
    //    duplicates).
    if input.nonce_consumed {
        return JoinDecision::Reject(JoinRejectionCode::ReplayedTicket);
    }

    JoinDecision::Grant(ProductMemberGrant {
        product_id: ticket.product_id.clone(),
        cloud_account_id: ticket.cloud_account_id.clone(),
        local_actor_ref: input.local_actor_ref.to_string(),
        verified_domain: ticket.verified_domain.clone(),
        governance_policy_version: policy.governance_policy_version.clone(),
        ticket_nonce: ticket.nonce.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticket() -> ProductJoinTicket {
        ProductJoinTicket {
            issuer: "https://cloud.bicameral.dev".into(),
            audience: "daemon:workspace-1".into(),
            product_id: "product-abc".into(),
            cloud_account_id: "acct-123".into(),
            verified_domain: "acme.example".into(),
            issued_at: "2026-07-11T00:00:00Z".parse().unwrap(),
            expiry: "2026-07-11T00:05:00Z".parse().unwrap(),
            nonce: "nonce-1".into(),
            registration_ref: "reg-1".into(),
            provider_hint: None,
            substrate_hint: None,
        }
    }

    fn policy() -> ProductAutoJoinPolicy {
        ProductAutoJoinPolicy {
            product_id: "product-abc".into(),
            governance_policy_version: "gpv-1".into(),
            auto_join_enabled: true,
            allowed_domains: vec!["acme.example".into()],
        }
    }

    fn pairing() -> PairingContext {
        PairingContext {
            paired_cloud_account_id: "acct-123".into(),
            confirmed_local_actor_ref: "user-local-1".into(),
        }
    }

    fn base_input<'a>(
        t: &'a ProductJoinTicket,
        p: &'a ProductAutoJoinPolicy,
        pair: &'a PairingContext,
    ) -> JoinEvaluationInput<'a> {
        JoinEvaluationInput {
            ticket: t,
            authenticity: TicketAuthenticity::Verified,
            expected_audience: "daemon:workspace-1",
            local_actor_ref: "user-local-1",
            pairing: Some(pair),
            policy: Some(p),
            existing_membership: None,
            nonce_consumed: false,
            now: "2026-07-11T00:01:00Z".parse().unwrap(),
        }
    }

    #[test]
    fn valid_same_domain_join_grants_product_member_only() {
        let (t, p, pair) = (ticket(), policy(), pairing());
        let decision = evaluate_join(&base_input(&t, &p, &pair));
        match decision {
            JoinDecision::Grant(grant) => {
                assert_eq!(grant.governance_policy_version, "gpv-1");
                assert_eq!(grant.ticket_nonce, "nonce-1");
            }
            other => panic!("expected grant, got {other:?}"),
        }
    }

    #[test]
    fn invalid_signature_fails_closed() {
        let (t, p, pair) = (ticket(), policy(), pairing());
        let mut input = base_input(&t, &p, &pair);
        input.authenticity = TicketAuthenticity::Invalid;
        assert_eq!(
            evaluate_join(&input),
            JoinDecision::Reject(JoinRejectionCode::InvalidSignature)
        );
    }

    #[test]
    fn wrong_audience_rejected() {
        let (t, p, pair) = (ticket(), policy(), pairing());
        let mut input = base_input(&t, &p, &pair);
        input.expected_audience = "daemon:other";
        assert_eq!(
            evaluate_join(&input),
            JoinDecision::Reject(JoinRejectionCode::WrongAudience)
        );
    }

    #[test]
    fn missing_pairing_rejected() {
        let (t, p, pair) = (ticket(), policy(), pairing());
        let mut input = base_input(&t, &p, &pair);
        input.pairing = None;
        let _ = &pair;
        assert_eq!(
            evaluate_join(&input),
            JoinDecision::Reject(JoinRejectionCode::MissingPairing)
        );
    }

    #[test]
    fn wrong_account_rejected() {
        let (t, p, mut pair) = (ticket(), policy(), pairing());
        pair.paired_cloud_account_id = "acct-999".into();
        assert_eq!(
            evaluate_join(&base_input(&t, &p, &pair)),
            JoinDecision::Reject(JoinRejectionCode::WrongAccount)
        );
    }

    #[test]
    fn wrong_actor_binding_rejected() {
        let (t, p, pair) = (ticket(), policy(), pairing());
        let mut input = base_input(&t, &p, &pair);
        input.local_actor_ref = "user-different";
        assert_eq!(
            evaluate_join(&input),
            JoinDecision::Reject(JoinRejectionCode::WrongAccount)
        );
    }

    #[test]
    fn not_yet_valid_rejected() {
        let (t, p, pair) = (ticket(), policy(), pairing());
        let mut input = base_input(&t, &p, &pair);
        input.now = "2026-07-10T23:59:00Z".parse().unwrap();
        assert_eq!(
            evaluate_join(&input),
            JoinDecision::Reject(JoinRejectionCode::NotYetValid)
        );
    }

    #[test]
    fn expired_ticket_rejected() {
        let (t, p, pair) = (ticket(), policy(), pairing());
        let mut input = base_input(&t, &p, &pair);
        input.now = "2026-07-11T00:06:00Z".parse().unwrap();
        assert_eq!(
            evaluate_join(&input),
            JoinDecision::Reject(JoinRejectionCode::ExpiredTicket)
        );
    }

    #[test]
    fn wrong_product_when_no_policy() {
        let (t, p, pair) = (ticket(), policy(), pairing());
        let mut input = base_input(&t, &p, &pair);
        input.policy = None;
        assert_eq!(
            evaluate_join(&input),
            JoinDecision::Reject(JoinRejectionCode::WrongProduct)
        );
    }

    #[test]
    fn policy_changed_when_auto_join_disabled() {
        let (t, mut p, pair) = (ticket(), policy(), pairing());
        p.auto_join_enabled = false;
        assert_eq!(
            evaluate_join(&base_input(&t, &p, &pair)),
            JoinDecision::Reject(JoinRejectionCode::PolicyChanged)
        );
    }

    #[test]
    fn wrong_domain_when_domain_not_allowed() {
        let (t, mut p, pair) = (ticket(), policy(), pairing());
        p.allowed_domains = vec!["other.example".into()];
        assert_eq!(
            evaluate_join(&base_input(&t, &p, &pair)),
            JoinDecision::Reject(JoinRejectionCode::WrongDomain)
        );
    }

    #[test]
    fn replayed_nonce_without_membership_rejected() {
        let (t, p, pair) = (ticket(), policy(), pairing());
        let mut input = base_input(&t, &p, &pair);
        input.nonce_consumed = true;
        assert_eq!(
            evaluate_join(&input),
            JoinDecision::Reject(JoinRejectionCode::ReplayedTicket)
        );
    }

    #[test]
    fn existing_member_is_idempotent_noop() {
        let (t, p, pair) = (ticket(), policy(), pairing());
        let membership = ProductMembership {
            product_id: "product-abc".into(),
            cloud_account_id: "acct-123".into(),
            local_actor_ref: "user-local-1".into(),
            granted_role: GrantedRole::ProductMember,
            verified_domain: "acme.example".into(),
            governance_policy_version: "gpv-1".into(),
            membership_event_ref: "event-1".into(),
            receipt_id: "receipt-1".into(),
            joined_at: "2026-07-11T00:00:30Z".parse().unwrap(),
        };
        let mut input = base_input(&t, &p, &pair);
        input.existing_membership = Some(&membership);
        // Even a reused (consumed) nonce must still be an idempotent success.
        input.nonce_consumed = true;
        assert_eq!(
            evaluate_join(&input),
            JoinDecision::AlreadyMember {
                membership_event_ref: "event-1".into(),
                receipt_id: "receipt-1".into(),
            }
        );
    }

    #[test]
    fn granted_result_carries_product_member_only() {
        let result = ProductJoinResult::granted(
            "req-1".into(),
            "product-abc".into(),
            "acct-123".into(),
            "event-1".into(),
            "receipt-1".into(),
            Utc::now(),
        );
        assert_eq!(result.granted_role, Some(GrantedRole::ProductMember));
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"Product Member\""));
        assert!(!json.contains("Owner"));
    }

    #[test]
    fn redacted_summary_omits_nonce() {
        let summary = ticket().redacted_summary();
        let json = serde_json::to_string(&summary).unwrap();
        assert!(!json.contains("nonce"));
        assert!(!json.contains("nonce-1"));
    }

    #[test]
    fn signing_bytes_are_deterministic_and_sorted() {
        let bytes = ticket().signing_bytes();
        let s = String::from_utf8(bytes).unwrap();
        // audience sorts before issuer.
        assert!(s.find("\"audience\"").unwrap() < s.find("\"issuer\"").unwrap());
        assert_eq!(ticket().signing_bytes(), ticket().signing_bytes());
    }
}
