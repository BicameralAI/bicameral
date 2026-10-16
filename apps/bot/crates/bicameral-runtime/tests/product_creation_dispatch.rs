//! Hosted Product creation dispatch conformance (issue #766).
//!
//! Proves the daemon-canonical `product.create` / `product.publish` commands
//! route through the local `ToolRequest` dispatcher and honor the acceptance
//! criteria end-to-end:
//!
//! 1. A paired hosted actor creates a canonical Product with exactly
//!    Policy Owner + Product Owner and an initial Governance Policy Version.
//! 2. Missing actor context, unpaired session, wrong Cloud account, and
//!    unauthorized actor return typed denials and append no canonical event.
//! 3. Retrying the same idempotency key returns the existing Product and never
//!    creates a second Product or duplicate role event.
//! 4. Replay restores the Product and creator authority from the event store
//!    alone (Cloud is never consulted as canonical state).
//! 5. A failed publication can be retried independently by canonical Product id
//!    with a safe receipt, and publication never re-runs canonical creation.
//! 6. Receipts and projections carry no secret/credential/local-path material.
//!
//! All tests are deterministic — no network, no hosted graph, no credentials.

use bicameral_api::event_store::{EventStoreAdapter, EventType};
use bicameral_api::product_creation::{
    EventStoreHint, HostedActorContext, ProductCreateParams, ProductPublishParams,
    RequiredConnection, FORBIDDEN_RECEIPT_SUBSTRINGS,
};
use bicameral_api::tool_request::*;
use bicameral_runtime::dispatch::{dispatch_with_hosted_product, HostedProductDispatchContext};
use bicameral_runtime::event_store_adapters::MemoryAdapter;
use bicameral_runtime::replay::replay_from_adapter;

fn hosted_authority(cloud_account_id: &str) -> AuthorityContext {
    AuthorityContext {
        actor_id: format!("bridge:{cloud_account_id}"),
        auth_method: AuthMethod::HostedBridge,
        session_id: Some("bridge-sess".into()),
        workspace: None,
        policy_scope: vec!["hosted_bridge".into()],
        audit_metadata: serde_json::Map::new(),
    }
}

fn create_params(idempotency_key: &str) -> ProductCreateParams {
    ProductCreateParams {
        idempotency_key: idempotency_key.into(),
        product_display_name: "Acme Checkout".into(),
        event_store_descriptor: None,
        asserted_cloud_account_id: Some("acct_1".into()),
        team_display_name: Some("Acme".into()),
        verified_domain: Some("acme.example".into()),
        event_store_hint: Some(EventStoreHint {
            substrate: Some("git".into()),
            provider: Some("github".into()),
        }),
        required_connection: Some(RequiredConnection {
            provider: Some("github".into()),
        }),
    }
}

fn actor(cloud_account_id: &str) -> HostedActorContext {
    HostedActorContext {
        actor_id: format!("bridge:{cloud_account_id}"),
        cloud_account_id: cloud_account_id.into(),
        verified_domain: Some("acme.example".into()),
        paired: true,
        authorized: true,
    }
}

fn ctx(actor: Option<HostedActorContext>) -> HostedProductDispatchContext {
    HostedProductDispatchContext {
        actor,
        commit: None,
    }
}

fn create_request(idempotency_key: &str) -> ToolRequest {
    ToolRequest::new(
        ToolCommand::ProductCreate(create_params(idempotency_key)),
        hosted_authority("acct_1"),
    )
}

fn count_create_events(adapter: &MemoryAdapter) -> usize {
    adapter
        .replay()
        .unwrap()
        .iter()
        .filter(|e| e.event_type == EventType::CreateProduct)
        .count()
}

#[test]
fn paired_actor_creates_canonical_product_with_creator_roles() {
    let adapter = MemoryAdapter::new();
    let ctx = ctx(Some(actor("acct_1")));
    let resp = dispatch_with_hosted_product(&create_request("idem-1"), &adapter, None, None, &ctx);

    assert_eq!(resp.status, ToolResponseStatus::Ok);
    let result = &resp.result.as_ref().unwrap()["result"];
    assert_eq!(
        result["creator_roles"],
        serde_json::json!(["Policy Owner", "Product Owner"])
    );
    assert_eq!(result["creator_cloud_account_id"], "acct_1");
    assert_eq!(result["governance_policy_version"], "gpv_1");
    assert!(result["membership_event_ref"]
        .as_str()
        .unwrap()
        .starts_with("evt_"));
    assert_eq!(resp.result.as_ref().unwrap()["preview"]["canonical"], false);
    assert_eq!(count_create_events(&adapter), 1);
}

#[test]
fn missing_actor_context_is_denied_without_event() {
    let adapter = MemoryAdapter::new();
    let ctx = ctx(None);
    let resp = dispatch_with_hosted_product(&create_request("idem-1"), &adapter, None, None, &ctx);

    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert!(resp.message.unwrap().starts_with("missing_actor_context"));
    assert_eq!(count_create_events(&adapter), 0);
}

#[test]
fn unpaired_session_is_denied_without_event() {
    let adapter = MemoryAdapter::new();
    let mut a = actor("acct_1");
    a.paired = false;
    let ctx = ctx(Some(a));
    let resp = dispatch_with_hosted_product(&create_request("idem-1"), &adapter, None, None, &ctx);

    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert!(resp.message.unwrap().starts_with("unpaired_session"));
    assert_eq!(count_create_events(&adapter), 0);
}

#[test]
fn unauthorized_actor_is_denied_without_event() {
    let adapter = MemoryAdapter::new();
    let mut a = actor("acct_1");
    a.authorized = false;
    let ctx = ctx(Some(a));
    let resp = dispatch_with_hosted_product(&create_request("idem-1"), &adapter, None, None, &ctx);

    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert!(resp.message.unwrap().starts_with("unauthorized_actor"));
    assert_eq!(count_create_events(&adapter), 0);
}

#[test]
fn wrong_cloud_account_is_denied_without_event() {
    let adapter = MemoryAdapter::new();
    // Actor bound to acct_2, but the request asserts acct_1.
    let ctx = ctx(Some(actor("acct_2")));
    let resp = dispatch_with_hosted_product(&create_request("idem-1"), &adapter, None, None, &ctx);

    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert!(resp.message.unwrap().starts_with("wrong_cloud_account"));
    assert_eq!(count_create_events(&adapter), 0);
}

#[test]
fn duplicate_idempotency_key_does_not_create_second_product() {
    let adapter = MemoryAdapter::new();
    let ctx = ctx(Some(actor("acct_1")));
    let first = dispatch_with_hosted_product(&create_request("idem-1"), &adapter, None, None, &ctx);
    let second =
        dispatch_with_hosted_product(&create_request("idem-1"), &adapter, None, None, &ctx);

    let first_result = &first.result.as_ref().unwrap()["result"];
    let second_result = &second.result.as_ref().unwrap()["result"];
    assert_eq!(first_result["product_id"], second_result["product_id"]);
    assert_eq!(
        first_result["membership_event_ref"],
        second_result["membership_event_ref"]
    );
    assert_eq!(first.result.as_ref().unwrap()["created"], true);
    assert_eq!(second.result.as_ref().unwrap()["created"], false);
    assert_eq!(count_create_events(&adapter), 1);
}

#[test]
fn replay_restores_product_and_creator_authority() {
    let adapter = MemoryAdapter::new();
    let ctx = ctx(Some(actor("acct_1")));
    let resp = dispatch_with_hosted_product(&create_request("idem-1"), &adapter, None, None, &ctx);
    let product_id = resp.result.as_ref().unwrap()["result"]["product_id"]
        .as_str()
        .unwrap()
        .to_string();

    let state = replay_from_adapter(&adapter).unwrap();
    let record = state.products.get(&product_id).unwrap();
    assert_eq!(record.creator_cloud_account_id, "acct_1");
    assert_eq!(
        record.creator_roles,
        vec!["Policy Owner".to_string(), "Product Owner".to_string()]
    );
    assert_eq!(record.governance_policy_version, "gpv_1");
}

#[test]
fn publication_retry_is_independent_and_does_not_rerun_creation() {
    let adapter = MemoryAdapter::new();
    let ctx = ctx(Some(actor("acct_1")));
    let created =
        dispatch_with_hosted_product(&create_request("idem-1"), &adapter, None, None, &ctx);
    let product_id = created.result.as_ref().unwrap()["result"]["product_id"]
        .as_str()
        .unwrap()
        .to_string();
    let receipt_id = created.result.as_ref().unwrap()["result"]["receipt_id"]
        .as_str()
        .unwrap()
        .to_string();

    let publish_req = ToolRequest::new(
        ToolCommand::ProductPublish(ProductPublishParams {
            product_id: product_id.clone(),
            idempotency_key: "publish-1".into(),
        }),
        hosted_authority("acct_1"),
    );

    let pub1 = dispatch_with_hosted_product(&publish_req, &adapter, None, None, &ctx);
    let pub2 = dispatch_with_hosted_product(&publish_req, &adapter, None, None, &ctx);
    assert_eq!(pub1.status, ToolResponseStatus::Ok);
    assert_eq!(pub2.status, ToolResponseStatus::Ok);
    let publication = &pub1.result.as_ref().unwrap()["publication"];
    assert_eq!(publication["canonical"], false);
    assert_eq!(publication["product_id"], product_id);
    assert_eq!(publication["canonical_creation_receipt_ref"], receipt_id);
    // Publication is read-only: still exactly one canonical creation event.
    assert_eq!(count_create_events(&adapter), 1);
}

#[test]
fn publish_unknown_product_is_rejected() {
    let adapter = MemoryAdapter::new();
    let ctx = ctx(Some(actor("acct_1")));
    let publish_req = ToolRequest::new(
        ToolCommand::ProductPublish(ProductPublishParams {
            product_id: "prd_missing".into(),
            idempotency_key: "publish-1".into(),
        }),
        hosted_authority("acct_1"),
    );
    let resp = dispatch_with_hosted_product(&publish_req, &adapter, None, None, &ctx);
    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert!(resp
        .message
        .as_ref()
        .unwrap()
        .starts_with("product_not_found"));

    let diagnostics = &resp.result.as_ref().unwrap()["diagnostics"];
    assert_eq!(diagnostics["error_code"], "product_not_found");
    assert_eq!(diagnostics["canonical_product_count"], 0);
    assert_eq!(diagnostics["create_event_count"], 0);
    assert_eq!(diagnostics["malformed_create_event_count"], 0);
    assert_eq!(
        diagnostics["event_store_substrate"],
        serde_json::json!("memory")
    );
    assert_eq!(
        diagnostics["product_id_fingerprint"]
            .as_str()
            .unwrap()
            .len(),
        16
    );
    assert_eq!(
        diagnostics["idempotency_key_fingerprint"]
            .as_str()
            .unwrap()
            .len(),
        16
    );

    let serialized = serde_json::to_string(&resp).unwrap();
    assert!(!serialized.contains("prd_missing"));
    assert!(!serialized.contains("publish-1"));
}

#[test]
fn receipts_and_projections_contain_no_secret_material() {
    let adapter = MemoryAdapter::new();
    let ctx = ctx(Some(actor("acct_1")));
    let resp = dispatch_with_hosted_product(&create_request("idem-1"), &adapter, None, None, &ctx);
    let serialized = serde_json::to_string(&resp).unwrap().to_lowercase();
    for needle in FORBIDDEN_RECEIPT_SUBSTRINGS {
        assert!(
            !serialized.contains(needle),
            "response leaked forbidden material '{needle}'"
        );
    }

    // Canonical event payloads must also be free of secret/credential material.
    let events = serde_json::to_string(&adapter.replay().unwrap())
        .unwrap()
        .to_lowercase();
    for needle in FORBIDDEN_RECEIPT_SUBSTRINGS {
        assert!(
            !events.contains(needle),
            "event store leaked forbidden material '{needle}'"
        );
    }
}
