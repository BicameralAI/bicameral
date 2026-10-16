//! Adversarial contract tests for Bot #987.
//!
//! These tests drive the production CandidateSelectionService,
//! CandidateSessionStore, replay projection, and AtomicEventStoreAdapter
//! boundary. Test-only adapters inject append failure and response loss; no
//! production fallback or fake-success path is used.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use bicameral_api::candidate::ReviewLifecycleState;
use bicameral_api::candidate_selection::{
    CandidatePromotionPolicyV1, CandidateSelectionAuthorityV1, CandidateSelectionCommitV1,
    CandidateSelectionFailureCode, CandidateSelectionSignoffMode, CandidateSelectionV1,
    CANDIDATE_SELECTION_CONTRACT_VERSION,
};
use bicameral_api::candidate_session::{
    CandidateDisposition, CandidateSetBindingV1, SessionCandidateDraftV1,
    CANDIDATE_SESSION_CONTRACT_VERSION,
};
use bicameral_api::dashboard::SignoffState;
use bicameral_api::event_store::{
    EventStoreAdapter, EventStoreEntry, EventStoreError, SubstrateCapability, SubstrateHealth,
    SubstrateKind,
};
use bicameral_runtime::atomic_event_store::{AtomicEventBatchV1, AtomicEventStoreAdapter};
use bicameral_runtime::candidate_selection_authority::CandidateSelectionAuthorityRegistry;
use bicameral_runtime::candidate_selection_service::CandidateSelectionService;
use bicameral_runtime::candidate_session_store::{
    CandidateSessionStore, CreateCandidateSetRequest,
};
use bicameral_runtime::event_store_adapters::memory::MemoryAdapter;
use bicameral_runtime::replay_from_adapter;
use chrono::{DateTime, Duration, TimeZone, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const PRODUCT: &str = "product-selection";
const SESSION: &str = "host-session-selection";
const LEASE: &str = "lease-selection";
const OWNER: &str = "owner-selection";
const CANDIDATE_A: Uuid = Uuid::from_u128(0xA1);
const CANDIDATE_B: Uuid = Uuid::from_u128(0xB2);
const CANDIDATE_C: Uuid = Uuid::from_u128(0xC3);

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 2, 20, 0, 0).unwrap()
}

fn digest(seed: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(seed.as_bytes()))
}

fn binding() -> CandidateSetBindingV1 {
    CandidateSetBindingV1 {
        product_id: PRODUCT.into(),
        host_session_id: SESSION.into(),
        governing_spec_binding_digest: digest("governing-spec-selection"),
        plan_digest: digest("completed-plan-selection"),
        accepted_model_base_position: 42,
        accepted_model_base_digest: digest("accepted-base-selection"),
    }
}

fn draft(id: Uuid, dependencies: Vec<Uuid>) -> SessionCandidateDraftV1 {
    SessionCandidateDraftV1 {
        candidate_id: id,
        proposed_decision: format!("Decision for candidate {id}"),
        source_spans: Vec::new(),
        contradiction_refs: Vec::new(),
        dependency_candidate_ids: dependencies,
        claim_trace_digest: digest(&format!("claim-{id}")),
        proposal_digest: digest(&format!("proposal-{id}")),
    }
}

fn authority(policy_enabled: bool, revision: &str) -> CandidateSelectionAuthorityV1 {
    CandidateSelectionAuthorityV1 {
        product_id: PRODUCT.into(),
        actor_id: OWNER.into(),
        may_promote: true,
        is_current_product_owner: true,
        governing_spec_binding_digest: binding().governing_spec_binding_digest,
        accepted_model_base_position: binding().accepted_model_base_position,
        accepted_model_base_digest: binding().accepted_model_base_digest,
        promotion_policy: CandidatePromotionPolicyV1 {
            enabled: policy_enabled,
            policy_revision: revision.into(),
        },
    }
}

struct Harness<A: AtomicEventStoreAdapter + 'static> {
    sessions: Arc<CandidateSessionStore>,
    authority: Arc<CandidateSelectionAuthorityRegistry>,
    events: Arc<A>,
    service: CandidateSelectionService,
    set: bicameral_api::candidate_session::CandidateSetV1,
}

fn harness_with<A: AtomicEventStoreAdapter + 'static>(events: Arc<A>) -> Harness<A> {
    let sessions = Arc::new(CandidateSessionStore::new());
    let set = sessions
        .create(
            CreateCandidateSetRequest {
                contract_version: CANDIDATE_SESSION_CONTRACT_VERSION,
                binding: binding(),
                session_lease_id: LEASE.into(),
                candidates: vec![
                    draft(CANDIDATE_A, Vec::new()),
                    draft(CANDIDATE_B, vec![CANDIDATE_A]),
                    draft(CANDIDATE_C, Vec::new()),
                ],
                coverage_limitations: Vec::new(),
            },
            now(),
        )
        .expect("seed CandidateSet");
    let authority_registry = Arc::new(CandidateSelectionAuthorityRegistry::new());
    authority_registry.project_current(authority(false, "policy-1"));
    let service = CandidateSelectionService::new(
        sessions.clone(),
        authority_registry.clone(),
        events.clone(),
    );
    Harness {
        sessions,
        authority: authority_registry,
        events,
        service,
        set,
    }
}

fn harness() -> Harness<MemoryAdapter> {
    harness_with(Arc::new(MemoryAdapter::new()))
}

fn selection(
    set: &bicameral_api::candidate_session::CandidateSetV1,
    promote: Vec<Uuid>,
    reject: Vec<Uuid>,
) -> CandidateSelectionV1 {
    CandidateSelectionV1 {
        contract_version: CANDIDATE_SELECTION_CONTRACT_VERSION,
        selection_operation_id: Uuid::new_v4(),
        candidate_set_id: set.candidate_set_id,
        candidate_set_generation: set.candidate_set_generation,
        candidate_set_digest: set.candidate_set_digest.clone(),
        product_id: PRODUCT.into(),
        host_session_id: SESSION.into(),
        session_lease_id: set.session_lease_id.clone(),
        promote_candidate_ids: promote,
        reject_candidate_ids: reject,
        actor_id: OWNER.into(),
    }
}

fn commit_from(
    preview: &bicameral_api::candidate_selection::CandidateSelectionPreviewV1,
) -> CandidateSelectionCommitV1 {
    CandidateSelectionCommitV1 {
        contract_version: CANDIDATE_SELECTION_CONTRACT_VERSION,
        selection_operation_id: preview.selection_operation_id,
        selection_digest: preview.selection_digest.clone(),
        actor_id: preview.actor_id.clone(),
        human_confirmed: true,
        confirmation_token: preview.confirmation_challenge.token(),
    }
}

#[test]
fn missing_dependency_returns_exact_ids_and_writes_nothing() {
    let h = harness();
    let error = h
        .service
        .prepare(selection(&h.set, vec![CANDIDATE_B], Vec::new()), now())
        .expect_err("selection is not dependency-closed");
    assert_eq!(
        error.code,
        CandidateSelectionFailureCode::DependencyClosureRequired
    );
    assert_eq!(error.candidate_ids, vec![CANDIDATE_A]);
    assert!(h.events.replay().unwrap().is_empty());
}

#[test]
fn overlapping_dispositions_fail_before_preview() {
    let h = harness();
    let error = h
        .service
        .prepare(
            selection(&h.set, vec![CANDIDATE_A], vec![CANDIDATE_A]),
            now(),
        )
        .expect_err("overlap must fail closed");
    assert_eq!(
        error.code,
        CandidateSelectionFailureCode::OverlappingDispositions
    );
    assert_eq!(error.candidate_ids, vec![CANDIDATE_A]);
    assert!(h.events.replay().unwrap().is_empty());
}

#[test]
fn dependency_closed_batch_appends_distinct_events_and_reconciles_session() {
    let h = harness();
    let preview = h
        .service
        .prepare(
            selection(&h.set, vec![CANDIDATE_A, CANDIDATE_B], vec![CANDIDATE_C]),
            now(),
        )
        .expect("prepare closed batch");
    assert_eq!(
        preview.resulting_signoff,
        CandidateSelectionSignoffMode::Proposed
    );
    assert_eq!(preview.promote_effects.len(), 2);
    assert_eq!(preview.reject_effects.len(), 1);
    assert_eq!(preview.pending_candidate_count, 0);

    let receipt = h
        .service
        .commit(commit_from(&preview), now() + Duration::seconds(1))
        .expect("commit closed batch");
    assert_eq!(receipt.canonical_event_ids.len(), 2);
    assert_eq!(receipt.decision_ids.len(), 2);
    assert_eq!(receipt.rejected_candidate_ids, vec![CANDIDATE_C]);
    assert!(receipt.replay_verified);
    assert!(receipt.session_reconciled);

    let events = h.events.replay().unwrap();
    assert_eq!(events.len(), 2);
    assert_ne!(events[0].id, events[1].id);

    let session = h
        .sessions
        .enumerate(
            &bicameral_api::candidate_session::CandidateSessionAccessV1::new(
                PRODUCT,
                SESSION,
                &h.set.session_lease_id,
                h.set.candidate_set_generation,
            ),
            now() + Duration::seconds(2),
        )
        .unwrap();
    assert_eq!(session.count_with(CandidateDisposition::Promoted), 2);
    assert_eq!(session.count_with(CandidateDisposition::Rejected), 1);
    assert_eq!(session.count_with(CandidateDisposition::Pending), 0);

    let replay = replay_from_adapter(h.events.as_ref()).expect("replay decisions");
    assert_eq!(replay.decisions.len(), 2);
    assert!(replay
        .decisions
        .values()
        .all(|decision| decision.signoff_state == SignoffState::Proposed));
}

#[test]
fn enabled_auto_approve_materializes_approved_without_proposed_intermediate() {
    let h = harness();
    h.authority
        .project_current(authority(true, "policy-auto-1"));
    let preview = h
        .service
        .prepare(selection(&h.set, vec![CANDIDATE_A], Vec::new()), now())
        .unwrap();
    assert_eq!(
        preview.resulting_signoff,
        CandidateSelectionSignoffMode::ApprovedByOwnerPolicy
    );
    let receipt = h
        .service
        .commit(commit_from(&preview), now() + Duration::seconds(1))
        .unwrap();
    assert_eq!(
        receipt.resulting_signoff,
        CandidateSelectionSignoffMode::ApprovedByOwnerPolicy
    );

    let replay = replay_from_adapter(h.events.as_ref()).unwrap();
    let decision = replay.decisions.values().next().unwrap();
    assert_eq!(decision.signoff_state, SignoffState::Approved);
    assert_eq!(
        decision.review_state,
        ReviewLifecycleState::ReadyToMaterialize
    );
    assert_eq!(h.events.replay().unwrap().len(), 1);
}

#[test]
fn rejection_only_changes_session_and_writes_no_event() {
    let h = harness();
    let preview = h
        .service
        .prepare(selection(&h.set, Vec::new(), vec![CANDIDATE_C]), now())
        .unwrap();
    let receipt = h
        .service
        .commit(commit_from(&preview), now() + Duration::seconds(1))
        .unwrap();
    assert!(receipt.canonical_event_ids.is_empty());
    assert!(h.events.replay().unwrap().is_empty());
    let session = h
        .sessions
        .enumerate(
            &bicameral_api::candidate_session::CandidateSessionAccessV1::new(
                PRODUCT,
                SESSION,
                &h.set.session_lease_id,
                h.set.candidate_set_generation,
            ),
            now() + Duration::seconds(2),
        )
        .unwrap();
    assert_eq!(session.count_with(CandidateDisposition::Rejected), 1);
}

#[test]
fn owner_revocation_before_commit_returns_typed_no_write() {
    let h = harness();
    let preview = h
        .service
        .prepare(selection(&h.set, vec![CANDIDATE_A], Vec::new()), now())
        .unwrap();
    h.authority.revoke(PRODUCT, OWNER);
    let error = h
        .service
        .commit(commit_from(&preview), now() + Duration::seconds(1))
        .expect_err("revoked owner must not append");
    assert_eq!(error.code, CandidateSelectionFailureCode::ActorUnauthorized);
    assert!(h.events.replay().unwrap().is_empty());
}

#[test]
fn policy_drift_invalidates_exact_preview() {
    let h = harness();
    let preview = h
        .service
        .prepare(selection(&h.set, vec![CANDIDATE_A], Vec::new()), now())
        .unwrap();
    h.authority.project_current(authority(true, "policy-2"));
    let error = h
        .service
        .commit(commit_from(&preview), now() + Duration::seconds(1))
        .expect_err("policy drift must invalidate preview");
    assert_eq!(error.code, CandidateSelectionFailureCode::PreviewStale);
    assert!(h.events.replay().unwrap().is_empty());
}

#[test]
fn accepted_model_base_drift_fails_before_append() {
    let h = harness();
    let preview = h
        .service
        .prepare(selection(&h.set, vec![CANDIDATE_A], Vec::new()), now())
        .unwrap();
    let mut changed = authority(false, "policy-1");
    changed.accepted_model_base_position += 1;
    changed.accepted_model_base_digest = digest("new-accepted-base");
    h.authority.project_current(changed);
    let error = h
        .service
        .commit(commit_from(&preview), now() + Duration::seconds(1))
        .expect_err("base drift must fail");
    assert_eq!(
        error.code,
        CandidateSelectionFailureCode::AcceptedModelBaseChanged
    );
    assert!(h.events.replay().unwrap().is_empty());
}

#[test]
fn changed_actor_cannot_confirm_another_humans_preview() {
    let h = harness();
    let preview = h
        .service
        .prepare(selection(&h.set, vec![CANDIDATE_A], Vec::new()), now())
        .unwrap();
    let mut commit = commit_from(&preview);
    commit.actor_id = "agent-self-confirmation".into();
    let error = h
        .service
        .commit(commit, now() + Duration::seconds(1))
        .expect_err("agent cannot self-confirm");
    assert_eq!(
        error.code,
        CandidateSelectionFailureCode::ConfirmationRejected
    );
    assert!(h.events.replay().unwrap().is_empty());
}

#[test]
fn human_confirmation_flag_is_required() {
    let h = harness();
    let preview = h
        .service
        .prepare(selection(&h.set, vec![CANDIDATE_A], Vec::new()), now())
        .unwrap();
    let mut commit = commit_from(&preview);
    commit.human_confirmed = false;
    let error = h.service.commit(commit, now()).unwrap_err();
    assert_eq!(
        error.code,
        CandidateSelectionFailureCode::HumanConfirmationRequired
    );
    assert!(h.events.replay().unwrap().is_empty());
}

#[test]
fn stale_candidate_set_digest_fails_prepare() {
    let h = harness();
    let mut request = selection(&h.set, vec![CANDIDATE_A], Vec::new());
    request.candidate_set_digest = digest("stale-set");
    let error = h.service.prepare(request, now()).unwrap_err();
    assert_eq!(
        error.code,
        CandidateSelectionFailureCode::CandidateSetDigestMismatch
    );
}

struct FailingAtomicAdapter {
    entries: Mutex<Vec<EventStoreEntry>>,
}

impl FailingAtomicAdapter {
    fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
        }
    }
}

impl EventStoreAdapter for FailingAtomicAdapter {
    fn append(&self, entry: EventStoreEntry) -> Result<(), EventStoreError> {
        self.entries.lock().unwrap().push(entry);
        Ok(())
    }

    fn replay(&self) -> Result<Vec<EventStoreEntry>, EventStoreError> {
        Ok(self.entries.lock().unwrap().clone())
    }

    fn substrate_kind(&self) -> SubstrateKind {
        SubstrateKind::Memory
    }

    fn health_check(&self) -> Result<SubstrateHealth, EventStoreError> {
        Ok(health(self.entries.lock().unwrap().len()))
    }

    fn capabilities(&self) -> BTreeSet<SubstrateCapability> {
        BTreeSet::new()
    }
}

impl AtomicEventStoreAdapter for FailingAtomicAdapter {
    fn append_atomic_batch(&self, _batch: AtomicEventBatchV1) -> Result<(), EventStoreError> {
        Err(EventStoreError::Io {
            reason: "injected atomic append failure".into(),
        })
    }
}

#[test]
fn append_failure_leaves_zero_partial_events_and_pending_session() {
    let h = harness_with(Arc::new(FailingAtomicAdapter::new()));
    let preview = h
        .service
        .prepare(
            selection(&h.set, vec![CANDIDATE_A, CANDIDATE_B], Vec::new()),
            now(),
        )
        .unwrap();
    let error = h
        .service
        .commit(commit_from(&preview), now() + Duration::seconds(1))
        .unwrap_err();
    assert_eq!(
        error.code,
        CandidateSelectionFailureCode::AtomicAppendFailed
    );
    assert!(h.events.replay().unwrap().is_empty());
    let current = h
        .sessions
        .enumerate(
            &bicameral_api::candidate_session::CandidateSessionAccessV1::new(
                PRODUCT,
                SESSION,
                &h.set.session_lease_id,
                h.set.candidate_set_generation,
            ),
            now() + Duration::seconds(2),
        )
        .unwrap();
    assert_eq!(current.count_with(CandidateDisposition::Pending), 3);
}

struct CommitThenErrorAdapter {
    entries: Mutex<Vec<EventStoreEntry>>,
    fail_response_once: AtomicBool,
}

impl CommitThenErrorAdapter {
    fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            fail_response_once: AtomicBool::new(true),
        }
    }
}

impl EventStoreAdapter for CommitThenErrorAdapter {
    fn append(&self, entry: EventStoreEntry) -> Result<(), EventStoreError> {
        self.entries.lock().unwrap().push(entry);
        Ok(())
    }

    fn replay(&self) -> Result<Vec<EventStoreEntry>, EventStoreError> {
        Ok(self.entries.lock().unwrap().clone())
    }

    fn substrate_kind(&self) -> SubstrateKind {
        SubstrateKind::Memory
    }

    fn health_check(&self) -> Result<SubstrateHealth, EventStoreError> {
        Ok(health(self.entries.lock().unwrap().len()))
    }

    fn capabilities(&self) -> BTreeSet<SubstrateCapability> {
        BTreeSet::new()
    }
}

impl AtomicEventStoreAdapter for CommitThenErrorAdapter {
    fn append_atomic_batch(&self, batch: AtomicEventBatchV1) -> Result<(), EventStoreError> {
        batch.validate()?;
        self.entries.lock().unwrap().extend(batch.entries);
        if self.fail_response_once.swap(false, Ordering::SeqCst) {
            return Err(EventStoreError::Io {
                reason: "response lost after atomic commit".into(),
            });
        }
        Ok(())
    }
}

#[test]
fn response_loss_retry_returns_same_result_without_duplicate_events() {
    let h = harness_with(Arc::new(CommitThenErrorAdapter::new()));
    let preview = h
        .service
        .prepare(
            selection(&h.set, vec![CANDIDATE_A, CANDIDATE_B], Vec::new()),
            now(),
        )
        .unwrap();
    let commit = commit_from(&preview);
    let first = h
        .service
        .commit(commit.clone(), now() + Duration::seconds(1))
        .unwrap_err();
    assert_eq!(
        first.code,
        CandidateSelectionFailureCode::AtomicAppendFailed
    );
    assert_eq!(h.events.replay().unwrap().len(), 2);

    let receipt = h
        .service
        .commit(commit, now() + Duration::seconds(2))
        .expect("retry must recover committed deterministic batch");
    assert_eq!(receipt.canonical_event_ids.len(), 2);
    assert_eq!(h.events.replay().unwrap().len(), 2);

    let replay = replay_from_adapter(h.events.as_ref()).unwrap();
    assert_eq!(replay.decisions.len(), 2);
}

fn health(entry_count: usize) -> SubstrateHealth {
    SubstrateHealth {
        kind: SubstrateKind::Memory,
        reachable: true,
        last_synced: None,
        entry_count,
        freshness: None,
        warnings: Vec::new(),
        capabilities: BTreeSet::new(),
    }
}
