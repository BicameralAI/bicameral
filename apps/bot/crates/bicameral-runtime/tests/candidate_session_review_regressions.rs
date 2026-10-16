//! Regression witnesses for the blocking human review on PR #989.
//!
//! These tests exercise the production `CandidateSessionStore`. They are
//! expected to fail on the pre-repair head and must pass before the PR can be
//! represented as review-addressed.

use bicameral_api::candidate_session::{
    CandidateSessionAccessV1, CandidateSessionFailure, CandidateSetBindingV1,
    SessionCandidateDraftV1, CANDIDATE_SESSION_CONTRACT_VERSION,
    CANDIDATE_SESSION_RECONNECT_GRACE_SECONDS,
};
use bicameral_runtime::candidate_session_store::{
    CandidateSessionStore, CreateCandidateSetRequest,
};
use chrono::{DateTime, Duration, TimeZone, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const PRODUCT: &str = "product-review-regression";
const SESSION: &str = "host-session-review-regression";
const LEASE_A: &str = "lease-review-a";
const LEASE_B: &str = "lease-review-b";

fn base() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 2, 12, 0, 0).unwrap()
}

fn at(seconds: i64) -> DateTime<Utc> {
    base() + Duration::seconds(seconds)
}

fn digest(seed: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(seed.as_bytes()))
}

fn binding() -> CandidateSetBindingV1 {
    CandidateSetBindingV1 {
        product_id: PRODUCT.into(),
        host_session_id: SESSION.into(),
        governing_spec_binding_digest: digest("governing-spec-review-regression"),
        plan_digest: digest("plan-review-regression"),
        accepted_model_base_position: 1,
        accepted_model_base_digest: digest("accepted-model-review-regression"),
    }
}

fn draft() -> SessionCandidateDraftV1 {
    SessionCandidateDraftV1 {
        candidate_id: Uuid::from_u128(989),
        proposed_decision: "review regression candidate".into(),
        source_spans: Vec::new(),
        contradiction_refs: Vec::new(),
        dependency_candidate_ids: Vec::new(),
        claim_trace_digest: digest("claim-trace-review-regression"),
        proposal_digest: digest("proposal-review-regression"),
    }
}

fn request(lease: &str) -> CreateCandidateSetRequest {
    CreateCandidateSetRequest {
        contract_version: CANDIDATE_SESSION_CONTRACT_VERSION,
        binding: binding(),
        session_lease_id: lease.into(),
        candidates: vec![draft()],
        coverage_limitations: Vec::new(),
    }
}

fn access(store: &CandidateSessionStore, lease: &str, generation: u64) -> CandidateSessionAccessV1 {
    CandidateSessionAccessV1::new(
        PRODUCT,
        SESSION,
        store
            .scope_session_lease_id(lease)
            .expect("scope session lease"),
        generation,
    )
}

#[test]
fn equal_binding_create_requires_exact_live_lease_ownership() {
    let store = CandidateSessionStore::new();
    store.create(request(LEASE_A), base()).expect("seed set");

    let error = store
        .create(request(LEASE_B), at(1))
        .expect_err("a forged lease must not enumerate or reuse the live set");

    assert_eq!(error, CandidateSessionFailure::CandidateSetNotFound);
}

#[test]
fn stale_access_cannot_address_recreated_set_after_explicit_end() {
    let store = CandidateSessionStore::new();
    let original = store.create(request(LEASE_A), base()).expect("seed set");
    let stale_access = access(&store, LEASE_A, original.candidate_set_generation);

    store.end(&stale_access, at(1)).expect("end original set");
    let replacement = store
        .create(request(LEASE_A), at(2))
        .expect("create replacement set");

    assert_ne!(replacement.candidate_set_id, original.candidate_set_id);

    let error = store
        .enumerate(&stale_access, at(3))
        .expect_err("old access must never address replacement state");

    assert!(matches!(
        error,
        CandidateSessionFailure::CandidateSetStale | CandidateSessionFailure::CandidateSetNotFound
    ));
}

#[test]
fn timer_sweep_preserves_typed_expiry_for_exact_owner_without_candidate_state() {
    let store = CandidateSessionStore::new();
    let set = store.create(request(LEASE_A), base()).expect("seed set");
    let owner = access(&store, LEASE_A, set.candidate_set_generation);
    let expired_at = at(CANDIDATE_SESSION_RECONNECT_GRACE_SECONDS);

    assert_eq!(store.sweep_expired(expired_at), 1);
    assert_eq!(store.active_set_count(), 0);
    assert_eq!(store.retained_candidate_count(), 0);

    let error = store
        .enumerate(&owner, expired_at)
        .expect_err("exact owner should receive the governed typed expiry outcome");

    assert_eq!(error, CandidateSessionFailure::SessionLeaseExpired);
}

#[test]
fn stale_access_cannot_address_recreated_set_after_lazy_expiry() {
    let store = CandidateSessionStore::new();
    let original = store.create(request(LEASE_A), base()).expect("seed set");
    let stale_access = access(&store, LEASE_A, original.candidate_set_generation);

    assert_eq!(
        store
            .enumerate(&stale_access, at(CANDIDATE_SESSION_RECONNECT_GRACE_SECONDS))
            .unwrap_err(),
        CandidateSessionFailure::SessionLeaseExpired
    );

    let replacement = store
        .create(
            request(LEASE_A),
            at(CANDIDATE_SESSION_RECONNECT_GRACE_SECONDS + 1),
        )
        .expect("create replacement set");
    assert!(
        replacement.candidate_set_generation > original.candidate_set_generation,
        "cleanup must not reset the route generation"
    );
    assert_eq!(
        store
            .enumerate(
                &stale_access,
                at(CANDIDATE_SESSION_RECONNECT_GRACE_SECONDS + 2),
            )
            .unwrap_err(),
        CandidateSessionFailure::CandidateSetStale
    );
}

#[test]
fn different_lease_cannot_replace_live_set_under_changed_binding() {
    let store = CandidateSessionStore::new();
    let original = store.create(request(LEASE_A), base()).expect("seed set");

    let mut forged = request(LEASE_B);
    forged.binding.plan_digest = digest("forged replacement plan");
    assert_eq!(
        store.create(forged, at(1)).unwrap_err(),
        CandidateSessionFailure::CandidateSetNotFound
    );

    let owner_view = store
        .enumerate(
            &access(&store, LEASE_A, original.candidate_set_generation),
            at(2),
        )
        .expect("owner set remains intact");
    assert_eq!(owner_view.candidate_set_id, original.candidate_set_id);
    assert_eq!(
        owner_view.candidate_set_generation,
        original.candidate_set_generation
    );
}

#[test]
fn timer_sweep_expiry_receipt_is_bounded_and_candidate_free() {
    let store = CandidateSessionStore::new();
    let set = store.create(request(LEASE_A), base()).expect("seed set");
    let owner = access(&store, LEASE_A, set.candidate_set_generation);
    let swept_at = at(CANDIDATE_SESSION_RECONNECT_GRACE_SECONDS);

    assert_eq!(store.sweep_expired(swept_at), 1);
    assert_eq!(store.retained_candidate_count(), 0);
    assert_eq!(store.active_set_count(), 0);

    assert_eq!(
        store
            .enumerate(
                &owner,
                swept_at + Duration::seconds(CANDIDATE_SESSION_RECONNECT_GRACE_SECONDS),
            )
            .unwrap_err(),
        CandidateSessionFailure::CandidateSetNotFound
    );
}

#[test]
fn in_place_restart_preserves_generation_floor_without_candidate_state() {
    let mut store = CandidateSessionStore::new();
    let original = store.create(request(LEASE_A), base()).expect("seed set");
    let stale_access = access(&store, LEASE_A, original.candidate_set_generation);

    store.restart();
    assert_eq!(store.active_set_count(), 0);
    assert_eq!(store.retained_candidate_count(), 0);

    let replacement = store
        .create(request(LEASE_A), at(1))
        .expect("replacement after restart");
    assert!(replacement.candidate_set_generation > original.candidate_set_generation);
    assert_eq!(
        store.enumerate(&stale_access, at(2)).unwrap_err(),
        CandidateSessionFailure::CandidateSetNotFound
    );
}

#[test]
fn duplicate_candidate_ids_are_rejected_before_materialization() {
    let store = CandidateSessionStore::new();
    let mut duplicate = request(LEASE_A);
    duplicate.candidates = vec![draft(), draft()];

    assert_eq!(
        store.create(duplicate, base()).unwrap_err(),
        CandidateSessionFailure::MalformedIdentity
    );
    assert_eq!(store.active_set_count(), 0);
}

#[test]
fn dangling_candidate_dependencies_are_rejected_before_materialization() {
    let store = CandidateSessionStore::new();
    let mut dangling = request(LEASE_A);
    dangling.candidates[0].dependency_candidate_ids = vec![Uuid::from_u128(990)];

    assert_eq!(
        store.create(dangling, base()).unwrap_err(),
        CandidateSessionFailure::MalformedIdentity
    );
    assert_eq!(store.active_set_count(), 0);
}

#[test]
fn prior_process_access_cannot_address_replacement_generation_one() {
    let first = CandidateSessionStore::new();
    let original = first.create(request(LEASE_A), base()).expect("original");
    let stale_access = access(&first, LEASE_A, original.candidate_set_generation);

    let replacement_store = CandidateSessionStore::new();
    assert_ne!(replacement_store.lease_epoch(), first.lease_epoch());
    let replacement = replacement_store
        .create(request(LEASE_A), at(1))
        .expect("replacement");
    assert_eq!(replacement.candidate_set_generation, 1);
    assert_eq!(
        replacement_store
            .enumerate(&stale_access, at(2))
            .unwrap_err(),
        CandidateSessionFailure::CandidateSetNotFound
    );
}
