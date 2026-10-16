//! Live Product authority projection for CandidateSet selection prepare/commit.

use std::sync::Arc;

use bicameral_api::candidate_selection::{
    CandidateSelectionV1, CANDIDATE_SELECTION_CONTRACT_VERSION,
};
use bicameral_api::candidate_session::{
    CandidateSetBindingV1, SessionCandidateDraftV1, CANDIDATE_SESSION_CONTRACT_VERSION,
};
use bicameral_api::event_store::{
    EventStoreAdapter, EventStoreEntry, EventType, ProductCreatedPayload,
};
use bicameral_api::tool_request::{
    AuthMethod, AuthorityContext, ToolCommand, ToolRequest, ToolResponseStatus,
};
use bicameral_runtime::candidate_selection_authority::{
    project_live_authority, CandidateSelectionAuthorityRegistry, ProductCurrentnessStore,
    ProductCurrentnessV1,
};
use bicameral_runtime::candidate_selection_service::CandidateSelectionService;
use bicameral_runtime::candidate_session_store::{
    CandidateSessionStore, CreateCandidateSetRequest,
};
use bicameral_runtime::dispatch::{
    dispatch_with_candidate_selection, CandidateSelectionDispatchContext,
};
use bicameral_runtime::event_store_adapters::MemoryAdapter;
use chrono::{TimeZone, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const PRODUCT: &str = "prd_authority";
const SESSION: &str = "host-auth";
const LEASE: &str = "lease-auth";
const OWNER: &str = "acct_owner";
const CANDIDATE: Uuid = Uuid::from_u128(0xA01);

fn digest(seed: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(seed.as_bytes()))
}

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 4, 15, 0, 0).unwrap()
}

fn binding() -> CandidateSetBindingV1 {
    CandidateSetBindingV1 {
        product_id: PRODUCT.into(),
        host_session_id: SESSION.into(),
        governing_spec_binding_digest: digest("spec-auth"),
        plan_digest: digest("plan-auth"),
        accepted_model_base_position: 3,
        accepted_model_base_digest: digest("base-auth"),
    }
}

fn seed_product(adapter: &MemoryAdapter) {
    adapter
        .append(EventStoreEntry::new(
            EventType::CreateProduct,
            serde_json::to_value(ProductCreatedPayload {
                product_id: PRODUCT.into(),
                creator_cloud_account_id: OWNER.into(),
                creator_roles: vec!["Policy Owner".into(), "Product Owner".into()],
                governance_policy_version: "gpv_1".into(),
                receipt_id: "rcpt-auth".into(),
                created_at: now().to_rfc3339(),
                idempotency_key: "idem-auth".into(),
                product_display_name: "Authority Product".into(),
                team_display_name: None,
                verified_domain: Some("example.com".into()),
                event_store_hint: None,
                required_connection: None,
            })
            .unwrap(),
            format!("bridge:{OWNER}"),
        ))
        .unwrap();
}

#[test]
fn prepare_succeeds_after_live_owner_projection() {
    let events = Arc::new(MemoryAdapter::new());
    seed_product(events.as_ref());
    let sessions = Arc::new(CandidateSessionStore::new());
    // Seed with wall clock: dispatch_with_candidate_selection prepare uses Utc::now(),
    // and session leases expire after CANDIDATE_SESSION_RECONNECT_GRACE_SECONDS.
    let set = sessions
        .create(
            CreateCandidateSetRequest {
                contract_version: CANDIDATE_SESSION_CONTRACT_VERSION,
                binding: binding(),
                session_lease_id: LEASE.into(),
                candidates: vec![SessionCandidateDraftV1 {
                    candidate_id: CANDIDATE,
                    proposed_decision: "Promote under live owner authority".into(),
                    source_spans: Vec::new(),
                    contradiction_refs: Vec::new(),
                    dependency_candidate_ids: Vec::new(),
                    claim_trace_digest: digest("claim-auth"),
                    proposal_digest: digest("proposal-auth"),
                }],
                coverage_limitations: Vec::new(),
            },
            Utc::now(),
        )
        .unwrap();
    let registry = Arc::new(CandidateSelectionAuthorityRegistry::new());
    let currentness = ProductCurrentnessStore::new();
    // No manual project_current — only live projection from ledger + set binding.
    project_live_authority(
        registry.as_ref(),
        &currentness,
        events.as_ref(),
        PRODUCT,
        &format!("bridge:{OWNER}"),
        Some(&set.binding),
    )
    .expect("live owner projection");

    let service = Arc::new(CandidateSelectionService::new(
        sessions,
        registry,
        events.clone(),
    ));
    let request = ToolRequest::new(
        ToolCommand::CandidateSelectionPrepare(CandidateSelectionV1 {
            contract_version: CANDIDATE_SELECTION_CONTRACT_VERSION,
            selection_operation_id: Uuid::new_v4(),
            candidate_set_id: set.candidate_set_id,
            candidate_set_generation: set.candidate_set_generation,
            candidate_set_digest: set.candidate_set_digest.clone(),
            product_id: PRODUCT.into(),
            host_session_id: SESSION.into(),
            session_lease_id: set.session_lease_id.clone(),
            promote_candidate_ids: vec![CANDIDATE],
            reject_candidate_ids: vec![],
            actor_id: format!("bridge:{OWNER}"),
        }),
        AuthorityContext {
            actor_id: format!("bridge:{OWNER}"),
            auth_method: AuthMethod::McpSession,
            session_id: Some(SESSION.into()),
            workspace: None,
            policy_scope: vec![],
            audit_metadata: Default::default(),
        },
    )
    .for_product(PRODUCT);

    let ctx = CandidateSelectionDispatchContext {
        service: service.as_ref(),
        atomic_event_store: Some(events.as_ref()),
    };
    let response = dispatch_with_candidate_selection(&request, events.as_ref(), None, None, &ctx);
    assert_eq!(response.status, ToolResponseStatus::Ok, "{response:?}");
    assert!(response
        .result
        .as_ref()
        .and_then(|v| v.get("candidate_selection_preview"))
        .is_some());
}

#[test]
fn advanced_currentness_fails_prepare_with_governing_spec_changed() {
    let events = Arc::new(MemoryAdapter::new());
    seed_product(events.as_ref());
    let sessions = Arc::new(CandidateSessionStore::new());
    let set = sessions
        .create(
            CreateCandidateSetRequest {
                contract_version: CANDIDATE_SESSION_CONTRACT_VERSION,
                binding: binding(),
                session_lease_id: LEASE.into(),
                candidates: vec![SessionCandidateDraftV1 {
                    candidate_id: CANDIDATE,
                    proposed_decision: "Stale set after base advance".into(),
                    source_spans: Vec::new(),
                    contradiction_refs: Vec::new(),
                    dependency_candidate_ids: Vec::new(),
                    claim_trace_digest: digest("claim-stale"),
                    proposal_digest: digest("proposal-stale"),
                }],
                coverage_limitations: Vec::new(),
            },
            now(),
        )
        .unwrap();
    let registry = Arc::new(CandidateSelectionAuthorityRegistry::new());
    let currentness = ProductCurrentnessStore::new();
    project_live_authority(
        registry.as_ref(),
        &currentness,
        events.as_ref(),
        PRODUCT,
        OWNER,
        Some(&set.binding),
    )
    .unwrap();
    currentness.advance(ProductCurrentnessV1 {
        product_id: PRODUCT.into(),
        governing_spec_binding_digest: digest("spec-advanced"),
        accepted_model_base_position: 3,
        accepted_model_base_digest: digest("base-auth"),
        promotion_policy: bicameral_api::candidate_selection::CandidatePromotionPolicyV1 {
            enabled: false,
            policy_revision: "gpv_1".into(),
        },
    });
    project_live_authority(
        registry.as_ref(),
        &currentness,
        events.as_ref(),
        PRODUCT,
        OWNER,
        Some(&set.binding), // seed-if-absent must not clobber advanced currentness
    )
    .unwrap();

    let service = CandidateSelectionService::new(sessions, registry, events.clone());
    let error = service
        .prepare(
            CandidateSelectionV1 {
                contract_version: CANDIDATE_SELECTION_CONTRACT_VERSION,
                selection_operation_id: Uuid::new_v4(),
                candidate_set_id: set.candidate_set_id,
                candidate_set_generation: set.candidate_set_generation,
                candidate_set_digest: set.candidate_set_digest,
                product_id: PRODUCT.into(),
                host_session_id: SESSION.into(),
                session_lease_id: set.session_lease_id.clone(),
                promote_candidate_ids: vec![CANDIDATE],
                reject_candidate_ids: vec![],
                actor_id: OWNER.into(),
            },
            now(),
        )
        .unwrap_err();
    assert_eq!(
        error.code,
        bicameral_api::candidate_selection::CandidateSelectionFailureCode::GoverningSpecChanged
    );
}
