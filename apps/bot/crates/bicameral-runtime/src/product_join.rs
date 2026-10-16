//! Runtime side of `product.join` — Ed25519 ticket verification, canonical
//! state materialization, and the dispatch handler (issue #767).
//!
//! The pure decision logic and contract types live in
//! [`bicameral_api::product_join`]; this module supplies the daemon-local
//! trust anchors and crypto that the API module deliberately excludes, plus the
//! event materialization/replay glue.
//!
//! Authority boundary: the trust anchors (issuer, audience, verifying keys)
//! come only from daemon-local configuration, never from the request. The
//! confirmed local actor is the daemon-authenticated `AuthorityContext.actor_id`
//! and the paired Cloud account is read from daemon-derived pairing metadata —
//! never from an SPA-asserted field.

use std::collections::{BTreeMap, BTreeSet};

use base64::Engine as _;
use bicameral_api::event_store::{
    EventStoreAdapter, EventStoreEntry, EventType, ProductCreatedPayload,
};
use bicameral_api::product_join::{
    evaluate_join, GrantedRole, JoinDecision, JoinEvaluationInput, PairingContext,
    ProductAutoJoinPolicy, ProductJoinCommandParams, ProductJoinResult, ProductMemberAddedPayload,
    ProductMembership, SignedProductJoinTicket, TicketAuthenticity,
};
use bicameral_api::tool_request::{AuthorityContext, ToolRequest, ToolResponse};
use chrono::Utc;
use uuid::Uuid;

pub use bicameral_api::product_join::PAIRED_CLOUD_ACCOUNT_METADATA_KEY;

// ---------------------------------------------------------------------------
// Trust anchors + Ed25519 verification
// ---------------------------------------------------------------------------

/// A trusted issuer verifying key (Ed25519, raw 32-byte public key).
#[derive(Debug, Clone)]
pub struct VerifyingKey {
    pub key_id: String,
    /// Raw 32-byte Ed25519 public key.
    pub public_key: Vec<u8>,
}

impl VerifyingKey {
    /// Build a verifying key from a base64 (standard, padded) raw public key.
    pub fn from_base64(key_id: impl Into<String>, public_key_b64: &str) -> Option<Self> {
        let public_key = base64::engine::general_purpose::STANDARD
            .decode(public_key_b64)
            .ok()?;
        Some(Self {
            key_id: key_id.into(),
            public_key,
        })
    }
}

/// Daemon-local trust anchors for `product.join` ticket verification.
///
/// Sourced only from daemon configuration / operator-home bootstrap. The daemon
/// never mints tickets; it only verifies against these anchors.
#[derive(Debug, Clone)]
pub struct ProductJoinTrust {
    /// The single trusted hosted issuer identity.
    pub issuer: String,
    /// When `None`, expected audience is Cloud's `daemon:{product_id}` so an
    /// operator-scoped multi-Product daemon can verify joins for many Products.
    /// When `Some`, tickets must match that fixed audience (tests / lab).
    pub fixed_audience: Option<String>,
    /// Trusted issuer verifying keys, addressed by `key_id`.
    pub keys: Vec<VerifyingKey>,
}

impl ProductJoinTrust {
    /// Audience the ticket must carry for this Product.
    pub fn expected_audience_for(&self, product_id: &str) -> String {
        self.fixed_audience
            .clone()
            .unwrap_or_else(|| format!("daemon:{product_id}"))
    }

    /// Build trust anchors from daemon configuration.
    ///
    /// Keys whose `public_key_b64` cannot be decoded are skipped (they can
    /// never authenticate a ticket, so a malformed key simply drops out rather
    /// than opening the join path). Returns `None` when no usable trust anchor
    /// remains, keeping `product.join` fail-closed.
    ///
    /// Audience: empty or `"product_scoped"` → product-scoped
    /// `daemon:{product_id}`; any other string is a fixed audience.
    pub fn from_config(config: &bicameral_config::ProductJoinTrustConfig) -> Option<Self> {
        let keys: Vec<VerifyingKey> = config
            .verifying_keys
            .iter()
            .filter_map(|k| VerifyingKey::from_base64(&k.key_id, &k.public_key_b64))
            .collect();
        if config.issuer.is_empty() || keys.is_empty() {
            return None;
        }
        let fixed_audience = {
            let a = config.audience.trim();
            if a.is_empty() || a == "product_scoped" {
                None
            } else {
                Some(a.to_string())
            }
        };
        Some(Self {
            issuer: config.issuer.clone(),
            fixed_audience,
            keys,
        })
    }

    /// Authenticate a signed ticket against the trust anchors.
    ///
    /// Returns [`TicketAuthenticity::Verified`] only when the ticket issuer
    /// matches the trusted issuer, the `key_id` resolves to a trusted key, and
    /// the Ed25519 signature verifies over the canonical signing bytes.
    pub fn authenticate(&self, signed: &SignedProductJoinTicket) -> TicketAuthenticity {
        if signed.ticket.issuer != self.issuer {
            return TicketAuthenticity::Invalid;
        }
        let Some(key) = self.keys.iter().find(|k| k.key_id == signed.key_id) else {
            return TicketAuthenticity::Invalid;
        };
        let Ok(signature) = base64::engine::general_purpose::STANDARD.decode(&signed.signature)
        else {
            return TicketAuthenticity::Invalid;
        };
        let public_key =
            ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, &key.public_key);
        match public_key.verify(&signed.ticket.signing_bytes(), &signature) {
            Ok(()) => TicketAuthenticity::Verified,
            Err(_) => TicketAuthenticity::Invalid,
        }
    }
}

/// Daemon-owned context the dispatcher needs to verify and decide a
/// `product.join` request. Absent (e.g. pure-runtime test entrypoints or a
/// daemon without configured trust anchors) means `product.join` fails closed.
pub struct ProductJoinDispatchContext<'a> {
    pub trust: &'a ProductJoinTrust,
}

/// Authenticate the ticket identity used to select canonical Product state.
///
/// This check runs before operator-scoped route lookup. A parsed ticket is not
/// routing authority: only a ticket signed by daemon-local trust may supply the
/// Product id, and any independently supplied outer id must agree exactly.
pub(crate) fn authenticated_routing_product_id(
    request: &ToolRequest,
    params: &ProductJoinCommandParams,
    trust: &ProductJoinTrust,
) -> Result<String, Box<ToolResponse>> {
    let ticket = &params.signed_ticket.ticket;
    if trust.authenticate(&params.signed_ticket) != TicketAuthenticity::Verified {
        let result = ProductJoinResult::rejected(
            params.request_id.clone(),
            ticket.product_id.clone(),
            ticket.cloud_account_id.clone(),
            bicameral_api::product_join::JoinRejectionCode::InvalidSignature,
            Utc::now(),
        );
        return Err(Box::new(success_result(request, &result)));
    }
    if ticket.product_id.trim().is_empty() {
        return Err(Box::new(ToolResponse::rejected(
            request.request_id,
            "product_context_invalid: verified ProductJoinTicket product_id is empty".into(),
        )));
    }
    if request
        .product_id
        .as_deref()
        .is_some_and(|outer| outer != ticket.product_id)
    {
        return Err(Box::new(ToolResponse::rejected(
            request.request_id,
            "product_context_mismatch: ToolRequest.product_id does not match verified ProductJoinTicket product_id"
                .into(),
        )));
    }
    Ok(ticket.product_id.clone())
}

// ---------------------------------------------------------------------------
// Canonical replay materialization
// ---------------------------------------------------------------------------

/// Canonical Product membership + policy + replay-defense state materialized
/// exclusively from the event ledger (never from Cloud membership state).
#[derive(Debug, Default, Clone)]
pub struct ProductJoinState {
    /// `(product_id, cloud_account_id)` → canonical membership.
    pub memberships: BTreeMap<(String, String), ProductMembership>,
    /// Consumed single-use ticket nonces (replay defense).
    pub consumed_nonces: BTreeSet<String>,
    /// `product_id` → active auto-join policy (last writer wins).
    pub policies: BTreeMap<String, ProductAutoJoinPolicy>,
}

impl ProductJoinState {
    /// Apply a single event to the materialized state. Non-product events are
    /// ignored. Malformed payloads are skipped defensively (replay never
    /// panics on a bad entry).
    pub fn apply(&mut self, entry: &EventStoreEntry) {
        match entry.event_type {
            EventType::CreateProduct => {
                let Ok(payload) =
                    serde_json::from_value::<ProductCreatedPayload>(entry.payload.clone())
                else {
                    return;
                };
                let Some(verified_domain) = payload
                    .verified_domain
                    .filter(|domain| !domain.trim().is_empty())
                else {
                    return;
                };

                // Product creation is one atomic canonical event. Its verified
                // domain and initial Governance Policy Version are therefore
                // also the initial same-domain auto-join policy. A later
                // ProductAutoJoinPolicySet event replaces this replayed default
                // in ledger order without creating a partial two-event create.
                self.policies.insert(
                    payload.product_id.clone(),
                    ProductAutoJoinPolicy {
                        product_id: payload.product_id,
                        governance_policy_version: payload.governance_policy_version,
                        auto_join_enabled: true,
                        allowed_domains: vec![verified_domain],
                    },
                );
            }
            EventType::ProductMemberAdded => {
                let Ok(payload) =
                    serde_json::from_value::<ProductMemberAddedPayload>(entry.payload.clone())
                else {
                    return;
                };
                self.consumed_nonces.insert(payload.ticket_nonce.clone());
                let membership = ProductMembership {
                    product_id: payload.product_id.clone(),
                    cloud_account_id: payload.cloud_account_id.clone(),
                    local_actor_ref: payload.local_actor_ref,
                    granted_role: payload.granted_role,
                    verified_domain: payload.verified_domain,
                    governance_policy_version: payload.governance_policy_version,
                    membership_event_ref: entry.id.to_string(),
                    receipt_id: payload.receipt_id,
                    joined_at: payload.decided_at,
                };
                self.memberships
                    .entry((payload.product_id, payload.cloud_account_id))
                    // First grant wins: a later replayed duplicate never
                    // overwrites the canonical membership event reference.
                    .or_insert(membership);
            }
            EventType::ProductAutoJoinPolicySet => {
                let Ok(policy) =
                    serde_json::from_value::<ProductAutoJoinPolicy>(entry.payload.clone())
                else {
                    return;
                };
                self.policies.insert(policy.product_id.clone(), policy);
            }
            _ => {}
        }
    }

    /// Materialize the full state from an ordered event slice.
    pub fn from_entries(entries: &[EventStoreEntry]) -> Self {
        let mut state = Self::default();
        for entry in entries {
            state.apply(entry);
        }
        state
    }
}

// ---------------------------------------------------------------------------
// Dispatch handler
// ---------------------------------------------------------------------------

/// Derive the daemon-confirmed pairing context from the request authority.
///
/// The confirmed local actor is the daemon-authenticated `actor_id`; the paired
/// Cloud account is read from daemon-set audit metadata. Nothing here is
/// SPA-asserted.
fn pairing_from_authority(authority: &AuthorityContext) -> Option<PairingContext> {
    let paired_cloud_account_id = authority
        .audit_metadata
        .get(PAIRED_CLOUD_ACCOUNT_METADATA_KEY)?
        .as_str()?
        .to_string();
    if paired_cloud_account_id.is_empty() || authority.actor_id.is_empty() {
        return None;
    }
    Some(PairingContext {
        paired_cloud_account_id,
        confirmed_local_actor_ref: authority.actor_id.clone(),
    })
}

/// Dispatch a `product.join` command.
///
/// Verifies the signed ticket against daemon-local trust anchors, binds the
/// ticket account to the daemon-confirmed pairing/actor context, re-evaluates
/// the Product's active auto-join policy in canonical state, and idempotently
/// materializes a Product Member-only membership event. Fail-closed: any
/// mismatch yields a typed, non-secret [`ProductJoinResult`] and no event.
pub(crate) fn dispatch_product_join(
    request: &ToolRequest,
    params: &ProductJoinCommandParams,
    adapter: &dyn EventStoreAdapter,
    ctx: Option<&ProductJoinDispatchContext<'_>>,
) -> ToolResponse {
    // Fail closed when the daemon has no configured trust anchors: without a
    // trusted issuer key the ticket signature cannot be verified.
    let Some(ctx) = ctx else {
        return ToolResponse::rejected(
            request.request_id,
            "unsupported_capability: product.join requires configured issuer trust anchors".into(),
        );
    };

    let routed_product_id = match authenticated_routing_product_id(request, params, ctx.trust) {
        Ok(product_id) => product_id,
        Err(response) => return *response,
    };
    let signed = &params.signed_ticket;
    let ticket = &signed.ticket;

    // Redaction-safe log only; never the nonce, signature, or any token.
    let summary = ticket.redacted_summary();
    tracing::debug!(
        issuer = %summary.issuer,
        audience = %summary.audience,
        product_id = %summary.product_id,
        cloud_account_id = %summary.cloud_account_id,
        verified_domain = %summary.verified_domain,
        "product.join request received"
    );

    let authenticity = TicketAuthenticity::Verified;
    let pairing = pairing_from_authority(&request.authority);

    // Materialize canonical state exclusively from the event ledger.
    let entries = match adapter.replay() {
        Ok(entries) => entries,
        Err(e) => {
            return ToolResponse::error(
                request.request_id,
                format!("daemon_unavailable: replay failed: {e}"),
            );
        }
    };
    let state = ProductJoinState::from_entries(&entries);
    let membership_key = (ticket.product_id.clone(), ticket.cloud_account_id.clone());
    let existing_membership = state.memberships.get(&membership_key);
    let policy = state.policies.get(&routed_product_id);
    let nonce_consumed = state.consumed_nonces.contains(&ticket.nonce);

    let expected_audience = ctx.trust.expected_audience_for(&ticket.product_id);
    let input = JoinEvaluationInput {
        ticket,
        authenticity,
        expected_audience: &expected_audience,
        local_actor_ref: &params.local_actor_ref,
        pairing: pairing.as_ref(),
        policy,
        existing_membership,
        nonce_consumed,
        now: Utc::now(),
    };

    let decision = evaluate_join(&input);
    let decided_at = Utc::now();

    match decision {
        JoinDecision::Grant(grant) => {
            let receipt_id = Uuid::new_v4().to_string();
            let payload = ProductMemberAddedPayload {
                product_id: grant.product_id.clone(),
                cloud_account_id: grant.cloud_account_id.clone(),
                local_actor_ref: grant.local_actor_ref,
                granted_role: GrantedRole::ProductMember,
                verified_domain: grant.verified_domain,
                governance_policy_version: grant.governance_policy_version,
                ticket_nonce: grant.ticket_nonce,
                receipt_id: receipt_id.clone(),
                decided_at,
            };
            let payload_json = match serde_json::to_value(&payload) {
                Ok(v) => v,
                Err(e) => {
                    return ToolResponse::error(
                        request.request_id,
                        format!("runtime_error: failed to serialize membership payload: {e}"),
                    );
                }
            };
            let entry = EventStoreEntry::new(
                EventType::ProductMemberAdded,
                payload_json,
                request.authority.actor_id.clone(),
            );
            let membership_event_ref = entry.id.to_string();
            match adapter.append(entry) {
                Ok(()) => {
                    let result = ProductJoinResult::granted(
                        params.request_id.clone(),
                        grant.product_id,
                        grant.cloud_account_id,
                        membership_event_ref,
                        receipt_id,
                        decided_at,
                    );
                    success_result(request, &result)
                }
                Err(e) => ToolResponse::error(
                    request.request_id,
                    format!("daemon_unavailable: event append failed: {e}"),
                ),
            }
        }
        JoinDecision::AlreadyMember {
            membership_event_ref,
            receipt_id,
        } => {
            let result = ProductJoinResult::granted(
                params.request_id.clone(),
                ticket.product_id.clone(),
                ticket.cloud_account_id.clone(),
                membership_event_ref,
                receipt_id,
                decided_at,
            );
            success_result(request, &result)
        }
        JoinDecision::Reject(code) => {
            let result = ProductJoinResult::rejected(
                params.request_id.clone(),
                ticket.product_id.clone(),
                ticket.cloud_account_id.clone(),
                code,
                decided_at,
            );
            success_result(request, &result)
        }
    }
}

/// Serialize a decided [`ProductJoinResult`] into a successful transport
/// response. The result payload itself carries the granted/rejected outcome
/// and any typed, non-secret failure.
fn success_result(request: &ToolRequest, result: &ProductJoinResult) -> ToolResponse {
    match serde_json::to_value(result) {
        Ok(v) => ToolResponse::success(request.request_id, v),
        Err(e) => ToolResponse::error(
            request.request_id,
            format!("runtime_error: failed to serialize product.join result: {e}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_store_adapters::MemoryAdapter;
    use bicameral_api::product_creation::{HostedActorContext, ProductCreateParams};
    use bicameral_api::product_join::{
        JoinOutcome, JoinRejectionCode, ProductJoinResult, ProductJoinTicket,
        SignedProductJoinTicket,
    };
    use bicameral_api::tool_request::{AuthMethod, ToolCommand, ToolResponseStatus};
    use chrono::Duration;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    const ISSUER: &str = "https://cloud.bicameral-ai.com";
    const AUDIENCE: &str = "daemon:prd_test";
    const PRODUCT: &str = "prd_test";
    const ACCOUNT: &str = "acct_alice";
    const DOMAIN: &str = "acme.example";
    const KEY_ID: &str = "issuer-key-1";

    /// A test issuer keypair that mints signed tickets and exposes the matching
    /// daemon-side trust anchors.
    struct TestIssuer {
        key_pair: Ed25519KeyPair,
    }

    impl TestIssuer {
        fn new() -> Self {
            let rng = ring::rand::SystemRandom::new();
            let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).expect("generate pkcs8");
            let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).expect("parse pkcs8");
            Self { key_pair }
        }

        fn trust(&self) -> ProductJoinTrust {
            ProductJoinTrust {
                issuer: ISSUER.to_string(),
                fixed_audience: Some(AUDIENCE.to_string()),
                keys: vec![VerifyingKey {
                    key_id: KEY_ID.to_string(),
                    public_key: self.key_pair.public_key().as_ref().to_vec(),
                }],
            }
        }

        fn sign(&self, ticket: ProductJoinTicket) -> SignedProductJoinTicket {
            let signature = base64::engine::general_purpose::STANDARD
                .encode(self.key_pair.sign(&ticket.signing_bytes()).as_ref());
            SignedProductJoinTicket {
                ticket,
                key_id: KEY_ID.to_string(),
                signature,
            }
        }
    }

    fn ticket() -> ProductJoinTicket {
        let now = Utc::now();
        ProductJoinTicket {
            issuer: ISSUER.to_string(),
            audience: AUDIENCE.to_string(),
            product_id: PRODUCT.to_string(),
            cloud_account_id: ACCOUNT.to_string(),
            verified_domain: DOMAIN.to_string(),
            issued_at: now - Duration::minutes(1),
            expiry: now + Duration::minutes(5),
            nonce: Uuid::new_v4().to_string(),
            registration_ref: "reg_1".to_string(),
            provider_hint: None,
            substrate_hint: None,
        }
    }

    fn policy_event(enabled: bool, domains: &[&str]) -> EventStoreEntry {
        let policy = ProductAutoJoinPolicy {
            product_id: PRODUCT.to_string(),
            governance_policy_version: "gpv-1".to_string(),
            auto_join_enabled: enabled,
            allowed_domains: domains.iter().map(|d| d.to_string()).collect(),
        };
        EventStoreEntry::new(
            EventType::ProductAutoJoinPolicySet,
            serde_json::to_value(&policy).unwrap(),
            "daemon".to_string(),
        )
    }

    /// Build a `product.join` request with daemon-derived pairing metadata for
    /// the given paired account (as the gateway would set it).
    fn request(signed: SignedProductJoinTicket, paired_account: &str) -> ToolRequest {
        let confirmed_actor = format!("bridge:{paired_account}");
        let params = ProductJoinCommandParams {
            request_id: Uuid::new_v4().to_string(),
            signed_ticket: signed,
            local_actor_ref: confirmed_actor.clone(),
            requested_at: Utc::now(),
        };
        let mut audit = serde_json::Map::new();
        audit.insert(
            PAIRED_CLOUD_ACCOUNT_METADATA_KEY.into(),
            serde_json::Value::String(paired_account.to_string()),
        );
        let authority = AuthorityContext {
            actor_id: confirmed_actor,
            auth_method: AuthMethod::HostedBridge,
            session_id: Some("sess-1".to_string()),
            workspace: None,
            policy_scope: vec!["hosted_bridge".to_string()],
            audit_metadata: audit,
        };
        ToolRequest::new(ToolCommand::ProductJoin(params), authority)
    }

    fn dispatch(
        request: &ToolRequest,
        adapter: &MemoryAdapter,
        trust: &ProductJoinTrust,
    ) -> ToolResponse {
        let ctx = ProductJoinDispatchContext { trust };
        crate::dispatch::dispatch_with_product_join(request, adapter, None, None, &ctx)
    }

    fn result_of(response: &ToolResponse) -> ProductJoinResult {
        assert_eq!(response.status, ToolResponseStatus::Ok, "{response:?}");
        serde_json::from_value(response.result.clone().expect("result payload")).unwrap()
    }

    fn membership_events(adapter: &MemoryAdapter) -> usize {
        adapter
            .replay()
            .unwrap()
            .iter()
            .filter(|e| e.event_type == EventType::ProductMemberAdded)
            .count()
    }

    #[test]
    fn created_product_initializes_same_domain_join_policy_on_replay() {
        let issuer = TestIssuer::new();
        let adapter = MemoryAdapter::new();
        let created = crate::product_creation::create_product(
            &ProductCreateParams {
                idempotency_key: "idem-owner-create".into(),
                product_display_name: "Acme Product".into(),
                event_store_descriptor: None,
                asserted_cloud_account_id: Some("acct_owner".into()),
                team_display_name: None,
                verified_domain: Some(DOMAIN.into()),
                event_store_hint: None,
                required_connection: None,
            },
            Some(&HostedActorContext {
                actor_id: "bridge:acct_owner".into(),
                cloud_account_id: "acct_owner".into(),
                verified_domain: Some(DOMAIN.into()),
                paired: true,
                authorized: true,
            }),
            &adapter,
            Utc::now(),
        )
        .expect("canonical Product creation succeeds");

        let product_id = created.result.product_id;
        let mut trust = issuer.trust();
        // Product-scoped audience (Cloud minting convention).
        trust.fixed_audience = None;
        let mut peer_ticket = ticket();
        peer_ticket.product_id = product_id.clone();
        peer_ticket.audience = trust.expected_audience_for(&product_id);

        let result = result_of(&dispatch(
            &request(issuer.sign(peer_ticket), ACCOUNT),
            &adapter,
            &trust,
        ));

        assert_eq!(result.outcome, JoinOutcome::Granted);
        assert_eq!(result.granted_role, Some(GrantedRole::ProductMember));
        assert_eq!(membership_events(&adapter), 1);
    }

    #[test]
    fn explicit_policy_event_overrides_created_product_default() {
        let created = EventStoreEntry::new(
            EventType::CreateProduct,
            serde_json::to_value(ProductCreatedPayload {
                product_id: PRODUCT.into(),
                creator_cloud_account_id: "acct_owner".into(),
                creator_roles: vec!["Policy Owner".into(), "Product Owner".into()],
                governance_policy_version: "gpv_1".into(),
                receipt_id: "rcpt_create".into(),
                idempotency_key: "idem-create".into(),
                product_display_name: "Acme Product".into(),
                team_display_name: None,
                verified_domain: Some(DOMAIN.into()),
                event_store_hint: None,
                required_connection: None,
                created_at: Utc::now().to_rfc3339(),
            })
            .unwrap(),
            "bridge:acct_owner".into(),
        );
        let mut state = ProductJoinState::default();
        state.apply(&created);
        assert_eq!(
            state
                .policies
                .get(PRODUCT)
                .map(|policy| policy.auto_join_enabled),
            Some(true)
        );

        state.apply(&policy_event(false, &[DOMAIN]));
        assert_eq!(
            state
                .policies
                .get(PRODUCT)
                .map(|policy| policy.auto_join_enabled),
            Some(false)
        );
    }

    #[test]
    fn valid_ticket_grants_product_member_and_appends_one_event() {
        let issuer = TestIssuer::new();
        let trust = issuer.trust();
        let adapter = MemoryAdapter::new();
        adapter.append(policy_event(true, &[DOMAIN])).unwrap();

        let req = request(issuer.sign(ticket()), ACCOUNT);
        let resp = dispatch(&req, &adapter, &trust);
        let result = result_of(&resp);

        assert_eq!(result.outcome, JoinOutcome::Granted);
        assert_eq!(result.granted_role, Some(GrantedRole::ProductMember));
        assert!(result.membership_event_ref.is_some());
        assert!(result.receipt_id.is_some());
        assert!(result.failure.is_none());
        assert_eq!(membership_events(&adapter), 1);
    }

    fn assert_rejected(resp: &ToolResponse, code: JoinRejectionCode, adapter: &MemoryAdapter) {
        let result = result_of(resp);
        assert_eq!(result.outcome, JoinOutcome::Rejected);
        assert_eq!(result.granted_role, None);
        assert!(result.membership_event_ref.is_none());
        assert_eq!(result.failure.as_ref().map(|f| f.code), Some(code));
        assert_eq!(
            membership_events(adapter),
            0,
            "rejection must append no event"
        );
    }

    #[test]
    fn invalid_signature_fails_closed() {
        let issuer = TestIssuer::new();
        let trust = issuer.trust();
        let adapter = MemoryAdapter::new();
        adapter.append(policy_event(true, &[DOMAIN])).unwrap();

        let mut signed = issuer.sign(ticket());
        // Tamper with the ticket after signing.
        signed.ticket.verified_domain = "evil.example".to_string();
        let req = request(signed, ACCOUNT);
        let resp = dispatch(&req, &adapter, &trust);
        assert_rejected(&resp, JoinRejectionCode::InvalidSignature, &adapter);
    }

    #[test]
    fn wrong_audience_fails_closed() {
        let issuer = TestIssuer::new();
        let trust = issuer.trust();
        let adapter = MemoryAdapter::new();
        adapter.append(policy_event(true, &[DOMAIN])).unwrap();

        let mut t = ticket();
        t.audience = "daemon:someone_else".to_string();
        let req = request(issuer.sign(t), ACCOUNT);
        let resp = dispatch(&req, &adapter, &trust);
        assert_rejected(&resp, JoinRejectionCode::WrongAudience, &adapter);
    }

    #[test]
    fn missing_pairing_fails_closed() {
        let issuer = TestIssuer::new();
        let trust = issuer.trust();
        let adapter = MemoryAdapter::new();
        adapter.append(policy_event(true, &[DOMAIN])).unwrap();

        // Build a request whose authority carries no paired-account metadata.
        let mut req = request(issuer.sign(ticket()), ACCOUNT);
        if let ToolCommand::ProductJoin(_) = &req.command {
            req.authority.audit_metadata.clear();
        }
        let resp = dispatch(&req, &adapter, &trust);
        assert_rejected(&resp, JoinRejectionCode::MissingPairing, &adapter);
    }

    #[test]
    fn wrong_account_fails_closed() {
        let issuer = TestIssuer::new();
        let trust = issuer.trust();
        let adapter = MemoryAdapter::new();
        adapter.append(policy_event(true, &[DOMAIN])).unwrap();

        // Ticket is for ACCOUNT but the pairing session is for another account.
        let req = request(issuer.sign(ticket()), "acct_someone_else");
        let resp = dispatch(&req, &adapter, &trust);
        assert_rejected(&resp, JoinRejectionCode::WrongAccount, &adapter);
    }

    #[test]
    fn wrong_product_fails_closed() {
        let issuer = TestIssuer::new();
        let trust = issuer.trust();
        let adapter = MemoryAdapter::new();
        // Policy exists for PRODUCT, but the ticket references another product.
        adapter.append(policy_event(true, &[DOMAIN])).unwrap();

        let mut t = ticket();
        t.product_id = "prd_other".to_string();
        let req = request(issuer.sign(t), ACCOUNT);
        let resp = dispatch(&req, &adapter, &trust);
        assert_rejected(&resp, JoinRejectionCode::WrongProduct, &adapter);
    }

    #[test]
    fn expired_ticket_fails_closed() {
        let issuer = TestIssuer::new();
        let trust = issuer.trust();
        let adapter = MemoryAdapter::new();
        adapter.append(policy_event(true, &[DOMAIN])).unwrap();

        let mut t = ticket();
        let now = Utc::now();
        t.issued_at = now - Duration::hours(2);
        t.expiry = now - Duration::hours(1);
        let req = request(issuer.sign(t), ACCOUNT);
        let resp = dispatch(&req, &adapter, &trust);
        assert_rejected(&resp, JoinRejectionCode::ExpiredTicket, &adapter);
    }

    #[test]
    fn not_yet_valid_ticket_fails_closed() {
        let issuer = TestIssuer::new();
        let trust = issuer.trust();
        let adapter = MemoryAdapter::new();
        adapter.append(policy_event(true, &[DOMAIN])).unwrap();

        let mut t = ticket();
        let now = Utc::now();
        t.issued_at = now + Duration::hours(1);
        t.expiry = now + Duration::hours(2);
        let req = request(issuer.sign(t), ACCOUNT);
        let resp = dispatch(&req, &adapter, &trust);
        assert_rejected(&resp, JoinRejectionCode::NotYetValid, &adapter);
    }

    #[test]
    fn wrong_domain_fails_closed() {
        let issuer = TestIssuer::new();
        let trust = issuer.trust();
        let adapter = MemoryAdapter::new();
        // Policy only permits a different domain.
        adapter
            .append(policy_event(true, &["other.example"]))
            .unwrap();

        let req = request(issuer.sign(ticket()), ACCOUNT);
        let resp = dispatch(&req, &adapter, &trust);
        assert_rejected(&resp, JoinRejectionCode::WrongDomain, &adapter);
    }

    #[test]
    fn disabled_policy_after_issuance_is_respected() {
        let issuer = TestIssuer::new();
        let trust = issuer.trust();
        let adapter = MemoryAdapter::new();
        // Ticket is otherwise valid, but the current canonical policy disables
        // auto-join (a policy change after ticket issuance).
        adapter.append(policy_event(false, &[DOMAIN])).unwrap();

        let req = request(issuer.sign(ticket()), ACCOUNT);
        let resp = dispatch(&req, &adapter, &trust);
        assert_rejected(&resp, JoinRejectionCode::PolicyChanged, &adapter);
    }

    #[test]
    fn missing_policy_is_wrong_product() {
        let issuer = TestIssuer::new();
        let trust = issuer.trust();
        let adapter = MemoryAdapter::new();
        // No policy event at all.
        let req = request(issuer.sign(ticket()), ACCOUNT);
        let resp = dispatch(&req, &adapter, &trust);
        assert_rejected(&resp, JoinRejectionCode::WrongProduct, &adapter);
    }

    #[test]
    fn duplicate_retry_is_idempotent_single_event() {
        let issuer = TestIssuer::new();
        let trust = issuer.trust();
        let adapter = MemoryAdapter::new();
        adapter.append(policy_event(true, &[DOMAIN])).unwrap();

        let signed = issuer.sign(ticket());
        // Two dispatches reusing the same signed ticket (duplicate UI retry).
        let first = dispatch(&request(signed.clone(), ACCOUNT), &adapter, &trust);
        let second = dispatch(&request(signed, ACCOUNT), &adapter, &trust);

        let r1 = result_of(&first);
        let r2 = result_of(&second);
        assert_eq!(r1.outcome, JoinOutcome::Granted);
        assert_eq!(r2.outcome, JoinOutcome::Granted);
        // Idempotent: the second returns the same canonical membership event.
        assert_eq!(r1.membership_event_ref, r2.membership_event_ref);
        assert_eq!(membership_events(&adapter), 1);
    }

    #[test]
    fn replayed_nonce_new_account_fails_closed() {
        let issuer = TestIssuer::new();
        let trust = issuer.trust();
        let adapter = MemoryAdapter::new();
        adapter
            .append(policy_event(true, &[DOMAIN, "beta.example"]))
            .unwrap();

        // Grant once for ACCOUNT, consuming the nonce.
        let t = ticket();
        let nonce = t.nonce.clone();
        let granted = dispatch(&request(issuer.sign(t), ACCOUNT), &adapter, &trust);
        assert_eq!(result_of(&granted).outcome, JoinOutcome::Granted);

        // A different account reuses the SAME nonce: replay defense fires.
        let mut t2 = ticket();
        t2.nonce = nonce;
        t2.cloud_account_id = "acct_bob".to_string();
        t2.verified_domain = "beta.example".to_string();
        let resp = dispatch(&request(issuer.sign(t2), "acct_bob"), &adapter, &trust);
        let result = result_of(&resp);
        assert_eq!(result.outcome, JoinOutcome::Rejected);
        assert_eq!(
            result.failure.as_ref().map(|f| f.code),
            Some(JoinRejectionCode::ReplayedTicket)
        );
        assert_eq!(membership_events(&adapter), 1);
    }

    #[test]
    fn receipt_and_event_are_redacted_of_secrets() {
        let issuer = TestIssuer::new();
        let trust = issuer.trust();
        let adapter = MemoryAdapter::new();
        adapter.append(policy_event(true, &[DOMAIN])).unwrap();

        let signed = issuer.sign(ticket());
        let signature = signed.signature.clone();
        let resp = dispatch(&request(signed, ACCOUNT), &adapter, &trust);
        let result_json = serde_json::to_string(&result_of(&resp)).unwrap();

        // The result must not carry the signature, session token, or nonce.
        assert!(!result_json.contains(&signature));
        assert!(!result_json.contains("sess-1"));
        assert!(!result_json.to_lowercase().contains("signature"));
    }

    #[test]
    fn replay_reconstructs_membership_from_events_only() {
        let issuer = TestIssuer::new();
        let trust = issuer.trust();
        let adapter = MemoryAdapter::new();
        adapter.append(policy_event(true, &[DOMAIN])).unwrap();
        dispatch(&request(issuer.sign(ticket()), ACCOUNT), &adapter, &trust);

        // Simulate a restart: rebuild state purely from the canonical ledger.
        let entries = adapter.replay().unwrap();
        let state = ProductJoinState::from_entries(&entries);
        let membership = state
            .memberships
            .get(&(PRODUCT.to_string(), ACCOUNT.to_string()))
            .expect("membership reconstructed from events");
        assert_eq!(membership.granted_role, GrantedRole::ProductMember);
        assert_eq!(membership.verified_domain, DOMAIN);
        assert_eq!(
            state.policies.get(PRODUCT).map(|p| p.auto_join_enabled),
            Some(true)
        );
    }

    #[test]
    fn unconfigured_trust_fails_closed_unsupported() {
        let issuer = TestIssuer::new();
        let adapter = MemoryAdapter::new();
        adapter.append(policy_event(true, &[DOMAIN])).unwrap();
        let req = request(issuer.sign(ticket()), ACCOUNT);
        // No trust context -> product.join is unsupported.
        let resp =
            crate::dispatch::dispatch_with_workspace_and_commit_watcher(&req, &adapter, None, None);
        assert_eq!(resp.status, ToolResponseStatus::Rejected);
    }
}
