//! Focused currentness and restart/replay evidence for Bot #987.

use std::sync::Arc;

use bicameral_api::candidate_selection::{
    CandidatePromotionPolicyV1, CandidateSelectionAuthorityV1, CandidateSelectionCommitV1,
    CandidateSelectionFailureCode, CandidateSelectionV1, CANDIDATE_SELECTION_CONTRACT_VERSION,
};
use bicameral_api::candidate_session::{
    CandidateSetBindingV1, SessionCandidateDraftV1, CANDIDATE_SESSION_CONTRACT_VERSION,
};
use bicameral_runtime::candidate_selection_authority::CandidateSelectionAuthorityRegistry;
use bicameral_runtime::candidate_selection_service::CandidateSelectionService;
use bicameral_runtime::candidate_session_store::{
    CandidateSessionStore, CreateCandidateSetRequest,
};
use bicameral_runtime::event_store_adapters::memory::MemoryAdapter;
use bicameral_runtime::replay_from_adapter;
use chrono::{Duration, TimeZone, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const PRODUCT: &str = "product-currentness";
const SESSION: &str = "session-currentness";
const LEASE: &str = "lease-currentness";
const OWNER: &str = "owner-currentness";
const CANDIDATE: Uuid = Uuid::from_u128(0x987);

fn digest(seed: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(seed.as_bytes()))
}

fn binding() -> CandidateSetBindingV1 {
    CandidateSetBindingV1 {
        product_id: PRODUCT.into(),
        host_session_id: SESSION.into(),
        governing_spec_binding_digest: digest("governing-spec-currentness"),
        plan_digest: digest("plan-currentness"),
        accepted_model_base_position: 12,
        accepted_model_base_digest: digest("accepted-base-currentness"),
    }
}

fn authority(
    policy_enabled: bool,
    is_current_product_owner: bool,
) -> CandidateSelectionAuthorityV1 {
    CandidateSelectionAuthorityV1 {
        product_id: PRODUCT.into(),
        actor_id: OWNER.into(),
        may_promote: true,
        is_current_product_owner,
        governing_spec_binding_digest: binding().governing_spec_binding_digest,
        accepted_model_base_position: binding().accepted_model_base_position,
        accepted_model_base_digest: binding().accepted_model_base_digest,
        promotion_policy: CandidatePromotionPolicyV1 {
            enabled: policy_enabled,
            policy_revision: if policy_enabled {
                "policy-auto-1".into()
            } else {
                "policy-proposed-1".into()
            },
        },
    }
}

struct Harness {
    sessions: Arc<CandidateSessionStore>,
    authorities: Arc<CandidateSelectionAuthorityRegistry>,
    events: Arc<MemoryAdapter>,
    service: CandidateSelectionService,
    set: bicameral_api::candidate_session::CandidateSetV1,
}

fn harness() -> Harness {
    let now = Utc.with_ymd_and_hms(2026, 8, 2, 20, 45, 0).unwrap();
    let sessions = Arc::new(CandidateSessionStore::new());
    let set = sessions
        .create(
            CreateCandidateSetRequest {
                contract_version: CANDIDATE_SESSION_CONTRACT_VERSION,
                binding: binding(),
                session_lease_id: LEASE.into(),
                candidates: vec![SessionCandidateDraftV1 {
                    candidate_id: CANDIDATE,
                    proposed_decision: "Keep exact-currentness checks fail closed".into(),
                    source_spans: Vec::new(),
                    contradiction_refs: Vec::new(),
                    dependency_candidate_ids: Vec::new(),
                    claim_trace_digest: digest("claim-currentness"),
                    proposal_digest: digest("proposal-currentness"),
                }],
                coverage_limitations: Vec::new(),
            },
            now,
        )
        .unwrap();
    let authorities = Arc::new(CandidateSelectionAuthorityRegistry::new());
    authorities.project_current(authority(false, true));
    let events = Arc::new(MemoryAdapter::new());
    let service =
        CandidateSelectionService::new(sessions.clone(), authorities.clone(), events.clone());
    Harness {
        sessions,
        authorities,
        events,
        service,
        set,
    }
}

fn selection(set: &bicameral_api::candidate_session::CandidateSetV1) -> CandidateSelectionV1 {
    CandidateSelectionV1 {
        contract_version: CANDIDATE_SELECTION_CONTRACT_VERSION,
        selection_operation_id: Uuid::new_v4(),
        candidate_set_id: set.candidate_set_id,
        candidate_set_generation: set.candidate_set_generation,
        candidate_set_digest: set.candidate_set_digest.clone(),
        product_id: PRODUCT.into(),
        host_session_id: SESSION.into(),
        session_lease_id: set.session_lease_id.clone(),
        promote_candidate_ids: vec![CANDIDATE],
        reject_candidate_ids: Vec::new(),
        actor_id: OWNER.into(),
    }
}

fn confirmed(
    preview: &bicameral_api::candidate_selection::CandidateSelectionPreviewV1,
) -> CandidateSelectionCommitV1 {
    CandidateSelectionCommitV1 {
        contract_version: CANDIDATE_SELECTION_CONTRACT_VERSION,
        selection_operation_id: preview.selection_operation_id,
        selection_digest: preview.selection_digest.clone(),
        actor_id: OWNER.into(),
        human_confirmed: true,
        confirmation_token: preview.confirmation_challenge.token(),
    }
}

#[test]
fn governing_spec_drift_before_commit_returns_typed_no_write() {
    let h = harness();
    let now = Utc.with_ymd_and_hms(2026, 8, 2, 20, 45, 0).unwrap();
    let preview = h.service.prepare(selection(&h.set), now).unwrap();
    let mut changed = authority(false, true);
    changed.governing_spec_binding_digest = digest("changed-governing-spec");
    h.authorities.project_current(changed);

    let error = h
        .service
        .commit(confirmed(&preview), now + Duration::seconds(1))
        .unwrap_err();
    assert_eq!(
        error.code,
        CandidateSelectionFailureCode::GoverningSpecChanged
    );
    assert!(
        bicameral_api::event_store::EventStoreAdapter::replay(h.events.as_ref())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn stale_generation_fails_prepare_before_challenge() {
    let h = harness();
    let now = Utc.with_ymd_and_hms(2026, 8, 2, 20, 45, 0).unwrap();
    let mut request = selection(&h.set);
    request.candidate_set_generation += 1;
    let error = h.service.prepare(request, now).unwrap_err();
    assert_eq!(error.code, CandidateSelectionFailureCode::CandidateSetStale);
}

#[test]
fn enabled_auto_approve_requires_current_product_owner() {
    let h = harness();
    let now = Utc.with_ymd_and_hms(2026, 8, 2, 20, 45, 0).unwrap();
    h.authorities.project_current(authority(true, false));
    let error = h.service.prepare(selection(&h.set), now).unwrap_err();
    assert_eq!(
        error.code,
        CandidateSelectionFailureCode::CurrentProductOwnerRequired
    );
    assert!(
        bicameral_api::event_store::EventStoreAdapter::replay(h.events.as_ref())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn canonical_replay_reconstructs_promoted_decision_after_service_restart() {
    let h = harness();
    let now = Utc.with_ymd_and_hms(2026, 8, 2, 20, 45, 0).unwrap();
    let preview = h.service.prepare(selection(&h.set), now).unwrap();
    let receipt = h
        .service
        .commit(confirmed(&preview), now + Duration::seconds(1))
        .unwrap();

    drop(h.service);
    drop(h.sessions);
    drop(h.authorities);

    let replayed = replay_from_adapter(h.events.as_ref()).unwrap();
    assert_eq!(replayed.decisions.len(), 1);
    assert!(replayed.decisions.contains_key(&receipt.decision_ids[0]));
}
