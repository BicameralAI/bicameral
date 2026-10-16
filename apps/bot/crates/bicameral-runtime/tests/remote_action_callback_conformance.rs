//! Remote Action Callback Conformance Tests
//!
//! Verifies the bot-owned consumer for authenticated remote action callbacks
//! per the specs:
//! - `docs/specs/action-authorization-policy.md`
//! - `docs/specs/action-authorization-policy-runtime-architecture.md`
//! - `docs/domain/runtime-protocol.md`
//!
//! # Contract status
//!
//! These tests validate against the **proposed v1** envelope from
//! `BicameralAI/bicameral-integrations#206`. That RFQ remains the contract
//! authority. If the contract is revised, these tests must be updated.
//!
//! # Coverage
//!
//! 1. Valid authenticated callback → accepted with receipt
//! 2. Invalid signature → rejected before actor binding
//! 3. Replay/duplicate nonce → rejected with ReplayDetected
//! 4. Authority-smuggling fields → rejected with AuthoritySmugglingDetected
//! 5. Disallowed action (signoff through remote) → authorization denied
//! 6. Unbound actor → rejected with ActorBindingFailed
//! 7. Expired timestamp → rejected with TimestampExpired
//! 8. Success path end-to-end → validated + authorized + receipt

use bicameral_api::remote_action::{
    AuthenticatedRemoteActionEnvelope, CallbackSource, RemoteActionKind, RemoteActionOutcome,
    RemoteActionRejectionReason, RemoteSessionBinding,
};
use bicameral_runtime::remote_action_gateway::{
    AcceptAllVerifier, ActorBinding, MemoryActorBindingRegistry, MemoryNonceStore,
    RejectAllVerifier, RemoteActionGateway, RemoteActionGatewayConfig,
};
use chrono::{Duration, Utc};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_gateway_accept_all(
) -> RemoteActionGateway<AcceptAllVerifier, MemoryNonceStore, MemoryActorBindingRegistry> {
    let mut registry = MemoryActorBindingRegistry::new();
    registry.add_binding(ActorBinding {
        external_actor_id: "U_SLACK_ALICE".into(),
        callback_source: CallbackSource::Slack,
        bicameral_actor_id: "user-alice".into(),
    });
    registry.add_binding(ActorBinding {
        external_actor_id: "U_LINEAR_BOB".into(),
        callback_source: CallbackSource::Linear,
        bicameral_actor_id: "user-bob".into(),
    });

    RemoteActionGateway::new(AcceptAllVerifier, MemoryNonceStore::new(), registry)
}

fn build_gateway_reject_sig(
) -> RemoteActionGateway<RejectAllVerifier, MemoryNonceStore, MemoryActorBindingRegistry> {
    let mut registry = MemoryActorBindingRegistry::new();
    registry.add_binding(ActorBinding {
        external_actor_id: "U_SLACK_ALICE".into(),
        callback_source: CallbackSource::Slack,
        bicameral_actor_id: "user-alice".into(),
    });

    RemoteActionGateway::new(RejectAllVerifier, MemoryNonceStore::new(), registry)
}

fn valid_envelope() -> AuthenticatedRemoteActionEnvelope {
    AuthenticatedRemoteActionEnvelope {
        action_id: Uuid::new_v4(),
        action_kind: RemoteActionKind::AcceptCandidate,
        target_id: "CAND-42".into(),
        callback_source: CallbackSource::Slack,
        external_actor_id: "U_SLACK_ALICE".into(),
        provider_signature: "v0=valid_signature_bytes".into(),
        nonce: format!("nonce-{}", Uuid::new_v4()),
        issued_at: Utc::now(),
        reason: Some("evidence looks sufficient".into()),
        session_binding: Some(RemoteSessionBinding {
            product_id: Some("prod-1".into()),
            remote_session_id: Some("thread-abc".into()),
        }),
    }
}

fn valid_payload() -> serde_json::Value {
    serde_json::to_value(&valid_envelope()).unwrap()
}

// ===========================================================================
// Test 1: Valid authenticated callback → accepted with receipt
// ===========================================================================

#[test]
fn valid_callback_produces_accepted_receipt() {
    let gateway = build_gateway_accept_all();
    let payload = valid_payload();

    let (receipt, validated) = gateway.process_callback(&payload);

    assert_eq!(receipt.outcome, RemoteActionOutcome::Accepted);
    assert!(receipt.rejection.is_none());
    assert!(validated.is_some());

    let v = validated.unwrap();
    assert_eq!(v.resolved_actor_id, "user-alice");
    assert_eq!(v.action_kind, RemoteActionKind::AcceptCandidate);
    assert_eq!(v.target_id, "CAND-42");
}

// ===========================================================================
// Test 2: Invalid signature → rejected before actor binding
// ===========================================================================

#[test]
fn invalid_signature_rejects_callback() {
    let gateway = build_gateway_reject_sig();
    let payload = valid_payload();

    let (receipt, validated) = gateway.process_callback(&payload);

    assert_eq!(receipt.outcome, RemoteActionOutcome::ValidationFailed);
    assert!(validated.is_none());

    let rejection = receipt.rejection.unwrap();
    assert_eq!(
        rejection.reason,
        RemoteActionRejectionReason::InvalidSignature
    );
}

// ===========================================================================
// Test 3: Replay/duplicate nonce → rejected with ReplayDetected
// ===========================================================================

#[test]
fn replay_nonce_rejects_callback() {
    let gateway = build_gateway_accept_all();

    // First submission succeeds
    let envelope = valid_envelope();
    let payload = serde_json::to_value(&envelope).unwrap();
    let (receipt1, validated1) = gateway.process_callback(&payload);
    assert_eq!(receipt1.outcome, RemoteActionOutcome::Accepted);
    assert!(validated1.is_some());

    // Second submission with same nonce is rejected
    let (receipt2, validated2) = gateway.process_callback(&payload);
    assert_eq!(receipt2.outcome, RemoteActionOutcome::ValidationFailed);
    assert!(validated2.is_none());

    let rejection = receipt2.rejection.unwrap();
    assert_eq!(
        rejection.reason,
        RemoteActionRejectionReason::ReplayDetected
    );
}

// ===========================================================================
// Test 4: Authority-smuggling fields → rejected
// ===========================================================================

#[test]
fn authority_smuggling_fields_reject_callback() {
    let gateway = build_gateway_accept_all();

    let mut payload = valid_payload();
    // Inject forbidden authority fields
    payload.as_object_mut().unwrap().insert(
        "authority_context".into(),
        serde_json::json!({"actor_id": "attacker", "auth_method": "local_daemon"}),
    );
    payload
        .as_object_mut()
        .unwrap()
        .insert("signoff_approval".into(), serde_json::json!(true));

    let (receipt, validated) = gateway.process_callback(&payload);

    assert_eq!(receipt.outcome, RemoteActionOutcome::ValidationFailed);
    assert!(validated.is_none());

    let rejection = receipt.rejection.unwrap();
    assert_eq!(
        rejection.reason,
        RemoteActionRejectionReason::AuthoritySmugglingDetected
    );
    assert!(rejection.message.contains("authority_context"));
    assert!(rejection.message.contains("signoff_approval"));
}

// ===========================================================================
// Test 5: Signoff action through remote → authorization denied
// ===========================================================================

/// Remote callbacks cannot approve signoff — that requires owner authority
/// on a local surface. Since RemoteActionKind only has review-class actions,
/// we verify the authorization policy rejects signoff-class actions when
/// the auth method is HostedBridge.
#[test]
fn signoff_action_denied_through_remote_path() {
    use bicameral_api::action_authorization::{
        ActionAuthorizationPolicy, ActionDenialReason, GovernedActionClass,
    };
    use bicameral_api::tool_request::{AuthMethod, AuthorityContext};

    // Simulate what would happen if a remote callback tried to do signoff
    let authority = AuthorityContext {
        actor_id: "user-alice".into(),
        auth_method: AuthMethod::HostedBridge,
        session_id: None,
        workspace: None,
        policy_scope: vec!["remote_action_callback".into()],
        audit_metadata: serde_json::Map::new(),
    };

    let result = ActionAuthorizationPolicy::evaluate_action_class(
        &authority,
        GovernedActionClass::DecisionSignoff,
    );

    assert!(result.is_err());
    let denial = result.unwrap_err();
    assert_eq!(denial.reason, ActionDenialReason::Unauthorized);
    assert!(denial.message.contains("signoff"));
}

// ===========================================================================
// Test 6: Unbound actor → rejected with ActorBindingFailed
// ===========================================================================

#[test]
fn unbound_actor_rejects_callback() {
    let gateway = build_gateway_accept_all();

    let mut envelope = valid_envelope();
    envelope.external_actor_id = "U_UNKNOWN_USER".into();
    let payload = serde_json::to_value(&envelope).unwrap();

    let (receipt, validated) = gateway.process_callback(&payload);

    assert_eq!(receipt.outcome, RemoteActionOutcome::ValidationFailed);
    assert!(validated.is_none());

    let rejection = receipt.rejection.unwrap();
    assert_eq!(
        rejection.reason,
        RemoteActionRejectionReason::ActorBindingFailed
    );
}

// ===========================================================================
// Test 7: Expired timestamp → rejected with TimestampExpired
// ===========================================================================

#[test]
fn expired_timestamp_rejects_callback() {
    let mut registry = MemoryActorBindingRegistry::new();
    registry.add_binding(ActorBinding {
        external_actor_id: "U_SLACK_ALICE".into(),
        callback_source: CallbackSource::Slack,
        bicameral_actor_id: "user-alice".into(),
    });

    let config = RemoteActionGatewayConfig {
        max_callback_age_seconds: 300,
    };
    let gateway = RemoteActionGateway::with_config(
        AcceptAllVerifier,
        MemoryNonceStore::new(),
        registry,
        config,
    );

    let mut envelope = valid_envelope();
    // Set issued_at to 10 minutes ago (beyond 5-minute window)
    envelope.issued_at = Utc::now() - Duration::seconds(600);
    let payload = serde_json::to_value(&envelope).unwrap();

    let (receipt, validated) = gateway.process_callback(&payload);

    assert_eq!(receipt.outcome, RemoteActionOutcome::ValidationFailed);
    assert!(validated.is_none());

    let rejection = receipt.rejection.unwrap();
    assert_eq!(
        rejection.reason,
        RemoteActionRejectionReason::TimestampExpired
    );
}

// ===========================================================================
// Test 8: Success path end-to-end — validate + authorize + receipt
// ===========================================================================

#[test]
fn end_to_end_success_path() {
    let gateway = build_gateway_accept_all();

    let envelope = AuthenticatedRemoteActionEnvelope {
        action_id: Uuid::new_v4(),
        action_kind: RemoteActionKind::RejectCandidate,
        target_id: "CAND-99".into(),
        callback_source: CallbackSource::Linear,
        external_actor_id: "U_LINEAR_BOB".into(),
        provider_signature: "v0=linear_sig".into(),
        nonce: format!("nonce-e2e-{}", Uuid::new_v4()),
        issued_at: Utc::now(),
        reason: Some("evidence contradicts claim".into()),
        session_binding: None,
    };
    let payload = serde_json::to_value(&envelope).unwrap();

    let (receipt, validated) = gateway.process_callback(&payload);

    // Validation and authorization succeed
    assert_eq!(receipt.outcome, RemoteActionOutcome::Accepted);
    assert!(receipt.rejection.is_none());
    assert_eq!(receipt.action_kind, RemoteActionKind::RejectCandidate);
    assert_eq!(receipt.target_id, "CAND-99");
    assert_eq!(receipt.resolved_actor_id.as_deref(), Some("user-bob"));

    // Validated action is returned for dispatch
    let v = validated.unwrap();
    assert_eq!(v.resolved_actor_id, "user-bob");
    assert_eq!(v.action_kind, RemoteActionKind::RejectCandidate);
    assert_eq!(v.target_id, "CAND-99");
    assert_eq!(v.callback_source, CallbackSource::Linear);
    assert_eq!(v.reason.as_deref(), Some("evidence contradicts claim"));
}

// ===========================================================================
// Additional boundary tests
// ===========================================================================

/// Remote action callback cannot mint source-ingest authority.
#[test]
fn remote_callback_cannot_mint_source_ingest() {
    let gateway = build_gateway_accept_all();

    let mut payload = valid_payload();
    payload.as_object_mut().unwrap().insert(
        "source_binding".into(),
        serde_json::json!({"source_uri": "https://evil.com", "binding_type": "auto"}),
    );

    let (receipt, _) = gateway.process_callback(&payload);
    assert_eq!(receipt.outcome, RemoteActionOutcome::ValidationFailed);
    let rejection = receipt.rejection.unwrap();
    assert_eq!(
        rejection.reason,
        RemoteActionRejectionReason::AuthoritySmugglingDetected
    );
}

/// Remote action callback cannot mint event-store writes.
#[test]
fn remote_callback_cannot_mint_event_store_write() {
    let gateway = build_gateway_accept_all();

    let mut payload = valid_payload();
    payload.as_object_mut().unwrap().insert(
        "event_store_write".into(),
        serde_json::json!({"event_type": "AcceptCandidate", "payload": {}}),
    );

    let (receipt, _) = gateway.process_callback(&payload);
    assert_eq!(receipt.outcome, RemoteActionOutcome::ValidationFailed);
    let rejection = receipt.rejection.unwrap();
    assert_eq!(
        rejection.reason,
        RemoteActionRejectionReason::AuthoritySmugglingDetected
    );
}

/// Remote action callback cannot mint workspace access.
#[test]
fn remote_callback_cannot_mint_workspace_access() {
    let gateway = build_gateway_accept_all();

    let mut payload = valid_payload();
    payload.as_object_mut().unwrap().insert(
        "workspace_path".into(),
        serde_json::json!("/home/user/secrets"),
    );

    let (receipt, _) = gateway.process_callback(&payload);
    assert_eq!(receipt.outcome, RemoteActionOutcome::ValidationFailed);
    let rejection = receipt.rejection.unwrap();
    assert_eq!(
        rejection.reason,
        RemoteActionRejectionReason::AuthoritySmugglingDetected
    );
}

/// Candidate review action through remote callback IS authorized
/// (unlike signoff which is denied).
#[test]
fn candidate_review_authorized_through_remote_path() {
    use bicameral_api::action_authorization::{ActionAuthorizationPolicy, GovernedActionClass};
    use bicameral_api::tool_request::{AuthMethod, AuthorityContext};

    let authority = AuthorityContext {
        actor_id: "user-alice".into(),
        auth_method: AuthMethod::HostedBridge,
        session_id: None,
        workspace: None,
        policy_scope: vec!["remote_action_callback".into()],
        audit_metadata: serde_json::Map::new(),
    };

    let result = ActionAuthorizationPolicy::evaluate_action_class(
        &authority,
        GovernedActionClass::CandidateReview,
    );

    assert!(
        result.is_ok(),
        "Candidate review should be authorized via HostedBridge"
    );
}
