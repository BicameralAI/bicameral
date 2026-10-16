//! Deterministic acceptance tests for the session-scoped `CandidateSetV1`
//! lifecycle — issue #986, Slice B.
//!
//! Governed spec revision:
//! `docs/specs/managed-mcp-drift-aware-continuation-runtime-architecture.md`
//! at `04e0471ad77cf1c7749bff8b0372d881bcd0d66e`.
//!
//! Conformance identifiers exercised here:
//!
//! - `CONF-MCP-CANDIDATE-SESSION-01` — CandidateSet is isolated by exact
//!   Product/session and survives matching-session turns and reconnect.
//! - `CONF-MCP-SESSION-REJECTION-01` — explicit rejection changes session state
//!   only and is erased at session end.
//! - `CONF-MCP-RECONNECT-CLEANUP-01` — same-session reconnect resumes within
//!   300 seconds; end and expiry erase transient state.
//!
//! # Production path
//!
//! Every test drives the real [`CandidateSessionStore`] — the production
//! CandidateSet service, session router, lease/cleanup logic, and disposition
//! transitions. There is no in-memory stand-in for the service under test.
//!
//! Permitted doubles, and only these:
//!
//! - a controlled clock (`DateTime<Utc>` values computed from one fixed
//!   `base()` instant, passed explicitly into every operation);
//! - generated session/product/lease identities;
//! - a real `MemoryAdapter` event store used strictly as a *witness* — it is
//!   never wired into the store, so an empty replay proves no canonical write
//!   could have occurred.

use bicameral_api::candidate_session::{
    CandidateDisposition, CandidateSessionAccessV1, CandidateSessionFailure, CandidateSetBindingV1,
    SessionCandidateDraftV1, SourceSpanV1, CANDIDATE_SESSION_CONTRACT_VERSION,
    CANDIDATE_SESSION_END_COMMAND, CANDIDATE_SESSION_HEARTBEAT_COMMAND,
    CANDIDATE_SESSION_RECONNECT_GRACE_SECONDS,
};
use bicameral_api::event_store::EventStoreAdapter;
use bicameral_runtime::candidate_session_store::{
    CandidateSessionStore, CreateCandidateSetRequest,
};
use bicameral_runtime::event_store_adapters::memory::MemoryAdapter;
use chrono::{DateTime, Duration, TimeZone, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

// ── Controlled clock and generated identities ─────────────────────────────

/// Fixed base instant. Every test time is `base() + Duration`, so no test
/// depends on wall-clock time.
fn base() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 30, 12, 0, 0).unwrap()
}

fn at(seconds: i64) -> DateTime<Utc> {
    base() + Duration::seconds(seconds)
}

fn digest(seed: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(seed.as_bytes()))
}

const PRODUCT_A: &str = "product-alpha";
const PRODUCT_B: &str = "product-beta";
const SESSION_1: &str = "host-session-0001";
const SESSION_2: &str = "host-session-0002";
const LEASE_1: &str = "lease-0001";
const LEASE_2: &str = "lease-0002";

fn binding(product: &str, session: &str) -> CandidateSetBindingV1 {
    CandidateSetBindingV1 {
        product_id: product.into(),
        host_session_id: session.into(),
        governing_spec_binding_digest: digest("governing-spec-04e0471a"),
        plan_digest: digest("completed-plan-bytes"),
        accepted_model_base_position: 42,
        accepted_model_base_digest: digest("accepted-model-base"),
    }
}

fn draft(n: u128) -> SessionCandidateDraftV1 {
    SessionCandidateDraftV1 {
        candidate_id: Uuid::from_u128(n),
        proposed_decision: format!("proposed decision {n}"),
        source_spans: vec![SourceSpanV1 {
            artifact_path: "docs/specs/managed-mcp.md".into(),
            start_byte: 100 * n as u64,
            end_byte: 100 * n as u64 + 40,
        }],
        contradiction_refs: vec![format!("contradiction-{n}")],
        dependency_candidate_ids: Vec::new(),
        claim_trace_digest: digest(&format!("claim-trace-{n}")),
        proposal_digest: digest(&format!("proposal-{n}")),
    }
}

fn create_request(product: &str, session: &str, lease: &str) -> CreateCandidateSetRequest {
    CreateCandidateSetRequest {
        contract_version: CANDIDATE_SESSION_CONTRACT_VERSION,
        binding: binding(product, session),
        session_lease_id: lease.into(),
        candidates: vec![draft(1), draft(2), draft(3)],
        coverage_limitations: vec!["analysis bounded to the bound spec revision".into()],
    }
}

fn access_for(
    store: &CandidateSessionStore,
    product: &str,
    session: &str,
    lease: &str,
    generation: u64,
) -> CandidateSessionAccessV1 {
    let session_lease_id = if lease.trim().is_empty() {
        lease.to_owned()
    } else {
        store
            .scope_session_lease_id(lease)
            .expect("scope session lease")
    };
    CandidateSessionAccessV1::new(product, session, session_lease_id, generation)
}

/// A store holding one live set for Product A / session 1, generation 1.
fn seeded() -> (CandidateSessionStore, CandidateSessionAccessV1) {
    let store = CandidateSessionStore::new();
    store
        .create(create_request(PRODUCT_A, SESSION_1, LEASE_1), base())
        .expect("seed create");
    let access = access_for(&store, PRODUCT_A, SESSION_1, LEASE_1, 1);
    (store, access)
}

// ── 1. CandidateSet creation with complete identity binding ───────────────

#[test]
fn create_binds_complete_identity() {
    let store = CandidateSessionStore::new();
    let set = store
        .create(create_request(PRODUCT_A, SESSION_1, LEASE_1), base())
        .expect("create");

    // Exact Product and host session identity.
    assert_eq!(set.binding.product_id, PRODUCT_A);
    assert_eq!(set.binding.host_session_id, SESSION_1);
    // Exact governed spec binding.
    assert_eq!(
        set.binding.governing_spec_binding_digest,
        digest("governing-spec-04e0471a")
    );
    assert_eq!(set.binding.plan_digest, digest("completed-plan-bytes"));
    // Accepted-model base.
    assert_eq!(set.binding.accepted_model_base_position, 42);
    assert_eq!(
        set.binding.accepted_model_base_digest,
        digest("accepted-model-base")
    );
    // Generation and deterministic digest.
    assert_eq!(set.candidate_set_generation, 1);
    assert!(set.candidate_set_digest.starts_with("sha256:"));
    assert_eq!(
        set.session_lease_id,
        store
            .scope_session_lease_id(LEASE_1)
            .expect("scope session lease")
    );
    assert_eq!(set.contract_version, CANDIDATE_SESSION_CONTRACT_VERSION);

    // Every candidate starts pending; disposition is daemon-authored.
    assert_eq!(set.candidates.len(), 3);
    assert!(set
        .candidates
        .iter()
        .all(|c| c.disposition == CandidateDisposition::Pending));
    assert_eq!(set.count_with(CandidateDisposition::Pending), 3);

    // Lease is the governed 300-second grace from creation.
    assert_eq!(
        set.lease_expires_at,
        base() + Duration::seconds(CANDIDATE_SESSION_RECONNECT_GRACE_SECONDS)
    );
}

#[test]
fn digest_is_deterministic_for_equal_identity_and_candidates() {
    // Two independent stores, same inputs, same digest: the digest is a pure
    // function of identity and candidate relations, not of instance state.
    let a = CandidateSessionStore::new();
    let b = CandidateSessionStore::new();
    let set_a = a
        .create(create_request(PRODUCT_A, SESSION_1, LEASE_1), base())
        .expect("a");
    let set_b = b
        .create(create_request(PRODUCT_A, SESSION_1, LEASE_1), at(90))
        .expect("b");
    assert_eq!(set_a.candidate_set_digest, set_b.candidate_set_digest);
    // ...while set instance identity still differs.
    assert_ne!(set_a.candidate_set_id, set_b.candidate_set_id);
}

#[test]
fn retrying_the_same_analysis_identity_reuses_the_generation() {
    let (store, access) = seeded();
    let first = store.enumerate(&access, at(10)).expect("enumerate");
    store
        .reject(&access, &[Uuid::from_u128(1)], at(10))
        .expect("reject");

    // Same analysis identity retried: reuse, do not open a generation and do
    // not reset the disposition already recorded in this session.
    let retried = store
        .create(create_request(PRODUCT_A, SESSION_1, LEASE_1), at(20))
        .expect("retry create");
    assert_eq!(retried.candidate_set_generation, 1);
    assert_eq!(retried.candidate_set_id, first.candidate_set_id);
    assert_eq!(retried.count_with(CandidateDisposition::Rejected), 1);
}

#[test]
fn a_new_planning_boundary_opens_a_generation_without_copying_dispositions() {
    let (store, access) = seeded();
    store
        .reject(&access, &[Uuid::from_u128(1)], at(10))
        .expect("reject");
    store
        .record_promoted_disposition(&access, &[Uuid::from_u128(2)], at(11))
        .expect("promote disposition");

    // A new completed Planning boundary: different plan digest.
    let mut next = create_request(PRODUCT_A, SESSION_1, LEASE_1);
    next.binding.plan_digest = digest("a-second-completed-plan");
    let set = store.create(next, at(20)).expect("new generation");

    assert_eq!(set.candidate_set_generation, 2);
    // Governed spec §2: a new generation does not silently copy pending or
    // rejected dispositions.
    assert_eq!(set.count_with(CandidateDisposition::Pending), 3);
    assert_eq!(set.count_with(CandidateDisposition::Rejected), 0);
    assert_eq!(set.count_with(CandidateDisposition::Promoted), 0);
    assert!(set.candidates.iter().all(|c| c.candidate_generation == 2));
}

// ── 2. Matching-session turns preserve state ──────────────────────────────

#[test]
fn matching_session_turns_preserve_state() {
    let (store, access) = seeded();
    let initial = store.enumerate(&access, at(1)).expect("turn 1");
    store
        .reject(&access, &[Uuid::from_u128(3)], at(2))
        .expect("reject on turn 2");

    // Several later turns in the same session, each well inside the lease.
    for turn in [30, 60, 120, 240] {
        let seen = store.enumerate(&access, at(turn)).expect("later turn");
        assert_eq!(seen.candidate_set_id, initial.candidate_set_id);
        assert_eq!(seen.candidate_set_generation, 1);
        assert_eq!(seen.candidate_set_digest, initial.candidate_set_digest);
        assert_eq!(seen.candidates.len(), 3);
        assert_eq!(seen.count_with(CandidateDisposition::Rejected), 1);
        assert_eq!(seen.count_with(CandidateDisposition::Pending), 2);
    }
}

// ── 3. Reconnect within 300 seconds ───────────────────────────────────────

#[test]
fn matching_session_reconnect_succeeds_within_grace() {
    let (store, access) = seeded();
    let initial = store.enumerate(&access, at(1)).expect("initial");
    store
        .reject(&access, &[Uuid::from_u128(1)], at(5))
        .expect("reject");
    store
        .record_promoted_disposition(&access, &[Uuid::from_u128(2)], at(6))
        .expect("promoted disposition");

    // Disconnect, then reconnect at 299s — inside the 300-second grace.
    let resumed = store.resume(&access, at(299)).expect("resume in grace");

    // Reconnect preserves candidate identities, generation, and digest.
    assert_eq!(resumed.candidate_set_id, initial.candidate_set_id);
    assert_eq!(resumed.candidate_set_generation, 1);
    assert_eq!(resumed.candidate_set_digest, initial.candidate_set_digest);
    let ids: Vec<Uuid> = resumed.candidates.iter().map(|c| c.candidate_id).collect();
    assert_eq!(
        ids,
        vec![Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3)]
    );

    // ...and every disposition: pending, rejected, and promoted session-UI.
    assert_eq!(resumed.count_with(CandidateDisposition::Rejected), 1);
    assert_eq!(resumed.count_with(CandidateDisposition::Promoted), 1);
    assert_eq!(resumed.count_with(CandidateDisposition::Pending), 1);

    // Resume extends the lease from the reconnect instant.
    assert_eq!(
        resumed.lease_expires_at,
        at(299) + Duration::seconds(CANDIDATE_SESSION_RECONNECT_GRACE_SECONDS)
    );
}

#[test]
fn reconnect_creates_no_canonical_event() {
    // The witness adapter is never handed to the store; an empty replay after a
    // full reconnect proves reconnect wrote nothing canonical.
    let witness = MemoryAdapter::new();
    let (store, access) = seeded();
    store
        .reject(&access, &[Uuid::from_u128(1)], at(5))
        .expect("reject");
    store.resume(&access, at(200)).expect("resume");
    store.heartbeat(&access, at(250)).expect("heartbeat");

    assert!(witness.replay().expect("replay").is_empty());
}

#[test]
fn heartbeat_extends_the_lease_beyond_the_original_grace() {
    let (store, access) = seeded();
    // Heartbeat at 250s pushes expiry to 550s, so a turn at 400s still works
    // even though the original lease ended at 300s.
    let beat = store.heartbeat(&access, at(250)).expect("heartbeat");
    assert_eq!(
        beat.lease_expires_at,
        at(250) + Duration::seconds(CANDIDATE_SESSION_RECONNECT_GRACE_SECONDS)
    );
    store.enumerate(&access, at(400)).expect("turn after beat");
}

// ── 4. Reconnect after expiry fails and erases ────────────────────────────

#[test]
fn reconnect_after_expiry_fails_and_erases_transient_state() {
    let (store, access) = seeded();
    store
        .reject(&access, &[Uuid::from_u128(1)], at(10))
        .expect("reject");
    assert_eq!(store.retained_candidate_count(), 3);

    // Exactly at the 300-second boundary the lease has elapsed.
    let err = store
        .resume(&access, at(CANDIDATE_SESSION_RECONNECT_GRACE_SECONDS))
        .expect_err("expired resume must fail");
    assert_eq!(err, CandidateSessionFailure::SessionLeaseExpired);

    // Failing closed erased the transient state.
    assert_eq!(store.retained_candidate_count(), 0);
    assert_eq!(store.active_set_count(), 0);

    // Nothing is retained afterwards, and nothing is reconstructed.
    let again = store.resume(&access, at(301)).expect_err("still gone");
    assert_eq!(again, CandidateSessionFailure::CandidateSetNotFound);
}

// ── 5. Different Product cannot enumerate or mutate ───────────────────────

#[test]
fn a_different_product_cannot_enumerate_or_mutate_the_set() {
    let (store, _owner) = seeded();
    // Same session id, same lease id, different Product: still not the owner.
    let intruder = access_for(&store, PRODUCT_B, SESSION_1, LEASE_1, 1);

    assert_eq!(
        store.enumerate(&intruder, at(10)).unwrap_err(),
        CandidateSessionFailure::CandidateSetNotFound
    );
    assert_eq!(
        store
            .reject(&intruder, &[Uuid::from_u128(1)], at(10))
            .unwrap_err(),
        CandidateSessionFailure::CandidateSetNotFound
    );
    assert_eq!(
        store
            .record_promoted_disposition(&intruder, &[Uuid::from_u128(1)], at(10))
            .unwrap_err(),
        CandidateSessionFailure::CandidateSetNotFound
    );

    // The owner's state is untouched.
    let owner = access_for(&store, PRODUCT_A, SESSION_1, LEASE_1, 1);
    let set = store
        .enumerate(&owner, at(11))
        .expect("owner still sees it");
    assert_eq!(set.count_with(CandidateDisposition::Pending), 3);
}

// ── 6. Different session cannot do anything ───────────────────────────────

#[test]
fn a_different_session_cannot_enumerate_resume_heartbeat_end_reject_or_promote() {
    let (store, owner) = seeded();
    let before = store.enumerate(&owner, at(1)).expect("before");

    // Every shape of "not this session": different session id, and the same
    // session id presenting a different (forged or stale) lease.
    let intruders = [
        access_for(&store, PRODUCT_A, SESSION_2, LEASE_2, 1),
        access_for(&store, PRODUCT_A, SESSION_2, LEASE_1, 1),
        access_for(&store, PRODUCT_A, SESSION_1, LEASE_2, 1),
    ];

    for intruder in &intruders {
        assert_eq!(
            store.enumerate(intruder, at(10)).unwrap_err(),
            CandidateSessionFailure::CandidateSetNotFound,
            "enumerate must fail closed"
        );
        assert_eq!(
            store.resume(intruder, at(10)).unwrap_err(),
            CandidateSessionFailure::CandidateSetNotFound,
            "resume must fail closed"
        );
        assert_eq!(
            store.heartbeat(intruder, at(10)).unwrap_err(),
            CandidateSessionFailure::CandidateSetNotFound,
            "heartbeat must fail closed"
        );
        assert_eq!(
            store.end(intruder, at(10)).unwrap_err(),
            CandidateSessionFailure::CandidateSetNotFound,
            "end must fail closed"
        );
        assert_eq!(
            store
                .reject(intruder, &[Uuid::from_u128(1)], at(10))
                .unwrap_err(),
            CandidateSessionFailure::CandidateSetNotFound,
            "reject must fail closed"
        );
        assert_eq!(
            store
                .record_promoted_disposition(intruder, &[Uuid::from_u128(1)], at(10))
                .unwrap_err(),
            CandidateSessionFailure::CandidateSetNotFound,
            "promote disposition must fail closed"
        );
    }

    // No unrelated state was mutated by any of that.
    let after = store.enumerate(&owner, at(20)).expect("after");
    assert_eq!(after.candidate_set_id, before.candidate_set_id);
    assert_eq!(after.candidate_set_digest, before.candidate_set_digest);
    assert_eq!(after.count_with(CandidateDisposition::Pending), 3);
    assert_eq!(store.retained_candidate_count(), 3);
}

#[test]
fn a_different_session_cannot_learn_whether_candidates_exist() {
    // Non-disclosure: probing a Product/session that owns a live set and one
    // that owns nothing must be indistinguishable.
    let (store, _owner) = seeded();

    let probe_existing = access_for(&store, PRODUCT_A, SESSION_1, LEASE_2, 1);
    let probe_absent = access_for(&store, PRODUCT_B, "host-session-9999", LEASE_2, 1);

    assert_eq!(
        store.enumerate(&probe_existing, at(10)).unwrap_err(),
        store.enumerate(&probe_absent, at(10)).unwrap_err()
    );
    assert_eq!(
        store.heartbeat(&probe_existing, at(10)).unwrap_err(),
        store.heartbeat(&probe_absent, at(10)).unwrap_err()
    );
    assert_eq!(
        store.end(&probe_existing, at(10)).unwrap_err(),
        store.end(&probe_absent, at(10)).unwrap_err()
    );

    // A non-owner also cannot learn the generation by probing: a stale-looking
    // generation from a non-owner still reads as "not found", never as
    // "candidate_set_stale" (which would confirm the set exists).
    let probe_generation = access_for(&store, PRODUCT_A, SESSION_1, LEASE_2, 99);
    assert_eq!(
        store.enumerate(&probe_generation, at(10)).unwrap_err(),
        CandidateSessionFailure::CandidateSetNotFound
    );
}

#[test]
fn concurrent_sessions_are_independently_routed() {
    let store = CandidateSessionStore::new();
    store
        .create(create_request(PRODUCT_A, SESSION_1, LEASE_1), base())
        .expect("session 1");
    store
        .create(create_request(PRODUCT_A, SESSION_2, LEASE_2), base())
        .expect("session 2");

    let one = access_for(&store, PRODUCT_A, SESSION_1, LEASE_1, 1);
    let two = access_for(&store, PRODUCT_A, SESSION_2, LEASE_2, 1);

    store
        .reject(&one, &[Uuid::from_u128(1)], at(5))
        .expect("reject in session 1");

    // Session 2 is entirely unaffected by session 1's disposition.
    let set_two = store.enumerate(&two, at(6)).expect("session 2 enumerate");
    assert_eq!(set_two.count_with(CandidateDisposition::Pending), 3);
    assert_eq!(set_two.count_with(CandidateDisposition::Rejected), 0);

    // Ending session 1 does not touch session 2.
    store.end(&one, at(7)).expect("end session 1");
    let still = store.enumerate(&two, at(8)).expect("session 2 survives");
    assert_eq!(still.candidates.len(), 3);
    assert_eq!(store.active_set_count(), 1);
}

// ── 7 & 8. Rejection is session-only, with no canonical trace ─────────────

#[test]
fn explicit_rejection_remains_session_only() {
    let (store, access) = seeded();
    let before = store.enumerate(&access, at(1)).expect("before");

    let result = store
        .reject(&access, &[Uuid::from_u128(1), Uuid::from_u128(2)], at(5))
        .expect("reject");

    assert_eq!(result.rejected_count, 2);
    assert_eq!(result.pending_count, 1);
    // Rejection does not change set identity or the digest a selection binds to.
    assert_eq!(result.candidate_set_digest, before.candidate_set_digest);
    assert_eq!(result.candidate_set_generation, 1);

    // The rejected candidates are still present in the session set — rejection
    // is a disposition, not a deletion, until cleanup erases everything.
    let after = store.enumerate(&access, at(6)).expect("after");
    assert_eq!(after.candidates.len(), 3);
    assert_eq!(after.count_with(CandidateDisposition::Rejected), 2);
}

#[test]
fn rejection_creates_no_canonical_event_or_suppression_record() {
    let witness = MemoryAdapter::new();
    let (store, access) = seeded();

    store
        .reject(&access, &[Uuid::from_u128(1)], at(5))
        .expect("reject");

    // No canonical event of any kind was appended.
    assert!(
        witness.replay().expect("replay").is_empty(),
        "session rejection must append no canonical event"
    );

    // No durable suppression: after the session ends, a fresh session for the
    // same Product proposing the same candidate identity sees it pending again.
    store.end(&access, at(10)).expect("end");
    let reproposed = store
        .create(create_request(PRODUCT_A, SESSION_2, LEASE_2), at(20))
        .expect("new session");
    let previously_rejected = reproposed
        .candidates
        .iter()
        .find(|c| c.candidate_id == Uuid::from_u128(1))
        .expect("same idea may be proposed again");
    assert_eq!(
        previously_rejected.disposition,
        CandidateDisposition::Pending,
        "a prior session rejection must not suppress a future proposal"
    );
    assert!(witness.replay().expect("replay").is_empty());
}

// ── 9. Promotion disposition is working state only ────────────────────────

#[test]
fn promotion_disposition_remains_working_state_only() {
    let witness = MemoryAdapter::new();
    let (store, access) = seeded();
    let before = store.enumerate(&access, at(1)).expect("before");

    let result = store
        .record_promoted_disposition(&access, &[Uuid::from_u128(2)], at(5))
        .expect("promoted disposition");

    assert_eq!(result.promoted_count, 1);
    assert_eq!(result.pending_count, 2);
    assert_eq!(result.candidate_set_digest, before.candidate_set_digest);

    // Recording a promoted disposition grants no canonical authority: the
    // store cannot append, and nothing was appended.
    assert!(
        witness.replay().expect("replay").is_empty(),
        "session promotion disposition must not append canonical state"
    );

    // It is visible only through the owning session.
    let intruder = access_for(&store, PRODUCT_A, SESSION_2, LEASE_2, 1);
    assert_eq!(
        store.enumerate(&intruder, at(6)).unwrap_err(),
        CandidateSessionFailure::CandidateSetNotFound
    );
}

// ── 10. Explicit end erases everything ────────────────────────────────────

#[test]
fn explicit_end_erases_all_transient_state() {
    let witness = MemoryAdapter::new();
    let (store, access) = seeded();
    store
        .reject(&access, &[Uuid::from_u128(1)], at(5))
        .expect("reject");
    store
        .record_promoted_disposition(&access, &[Uuid::from_u128(2)], at(6))
        .expect("promoted disposition");

    let ended = store.end(&access, at(10)).expect("end");

    assert_eq!(ended.erased_candidate_count, 3);
    assert_eq!(ended.erased_pending_count, 1);
    assert_eq!(ended.erased_rejected_count, 1);
    assert_eq!(ended.erased_promoted_disposition_count, 1);

    // Every unpromoted candidate, pending state, rejection state, transient
    // promotion state, and the session index/lease are gone.
    assert_eq!(store.retained_candidate_count(), 0);
    assert_eq!(store.active_set_count(), 0);
    assert_eq!(
        store.enumerate(&access, at(11)).unwrap_err(),
        CandidateSessionFailure::CandidateSetNotFound
    );

    // Cleanup synthesized no rejection event or other canonical state.
    assert!(
        witness.replay().expect("replay").is_empty(),
        "session end must not synthesize canonical state"
    );
}

// ── 11. Lease expiry erases everything ────────────────────────────────────

#[test]
fn lease_expiry_erases_all_transient_state() {
    let witness = MemoryAdapter::new();
    let (store, access) = seeded();
    store
        .reject(&access, &[Uuid::from_u128(1)], at(5))
        .expect("reject");
    store
        .record_promoted_disposition(&access, &[Uuid::from_u128(2)], at(6))
        .expect("promoted disposition");
    assert_eq!(store.retained_candidate_count(), 3);

    // The daemon's timer-driven sweep at expiry.
    let swept = store.sweep_expired(at(CANDIDATE_SESSION_RECONNECT_GRACE_SECONDS));
    assert_eq!(swept, 1);

    assert_eq!(store.retained_candidate_count(), 0);
    assert_eq!(store.active_set_count(), 0);
    assert_eq!(
        store.enumerate(&access, at(400)).unwrap_err(),
        CandidateSessionFailure::SessionLeaseExpired
    );
    assert_eq!(
        store.enumerate(&access, at(401)).unwrap_err(),
        CandidateSessionFailure::CandidateSetNotFound
    );
    assert!(
        witness.replay().expect("replay").is_empty(),
        "lease expiry must not synthesize canonical state"
    );
}

#[test]
fn sweep_leaves_live_sets_untouched() {
    let store = CandidateSessionStore::new();
    store
        .create(create_request(PRODUCT_A, SESSION_1, LEASE_1), base())
        .expect("expiring set");
    store
        .create(create_request(PRODUCT_A, SESSION_2, LEASE_2), at(200))
        .expect("later set");

    // At 300s the first set has elapsed; the second (created at 200s) has not.
    assert_eq!(store.sweep_expired(at(300)), 1);
    assert_eq!(store.active_set_count(), 1);
    store
        .enumerate(
            &access_for(&store, PRODUCT_A, SESSION_2, LEASE_2, 1),
            at(301),
        )
        .expect("live set survives the sweep");
}

// ── 12. End and heartbeat retries are idempotent ──────────────────────────

#[test]
fn heartbeat_retries_are_idempotent() {
    let (store, access) = seeded();
    let before = store.enumerate(&access, at(1)).expect("before");

    let first = store.heartbeat(&access, at(50)).expect("first");
    let second = store.heartbeat(&access, at(50)).expect("duplicate");
    let third = store.heartbeat(&access, at(50)).expect("duplicate again");

    // A repeated heartbeat at the same instant is fully idempotent.
    assert_eq!(first, second);
    assert_eq!(second, third);

    // It never advances the generation, changes the digest, or moves a
    // disposition.
    assert_eq!(first.candidate_set_generation, 1);
    assert_eq!(first.candidate_set_digest, before.candidate_set_digest);
    assert_eq!(first.pending_count, 3);
    assert_eq!(first.rejected_count, 0);
    assert_eq!(first.promoted_count, 0);
    assert_eq!(store.retained_candidate_count(), 3);
}

#[test]
fn end_retries_are_idempotent() {
    let witness = MemoryAdapter::new();
    let (store, access) = seeded();

    let first = store.end(&access, at(10)).expect("first end");
    assert_eq!(first.erased_candidate_count, 3);

    // Erasure converges: the retry finds nothing to erase, erases nothing a
    // second time, and returns the typed failure rather than inventing state.
    let retry = store.end(&access, at(11)).expect_err("duplicate end");
    assert_eq!(retry, CandidateSessionFailure::CandidateSetNotFound);
    let retry_again = store.end(&access, at(12)).expect_err("duplicate end again");
    assert_eq!(retry_again, CandidateSessionFailure::CandidateSetNotFound);

    assert_eq!(store.retained_candidate_count(), 0);
    assert_eq!(store.active_set_count(), 0);
    assert!(witness.replay().expect("replay").is_empty());
}

#[test]
fn heartbeat_and_end_after_expiry_are_typed_and_erase_once() {
    let (store, access) = seeded();

    // The first post-expiry touch reports the elapsed lease and erases.
    assert_eq!(
        store.heartbeat(&access, at(300)).unwrap_err(),
        CandidateSessionFailure::SessionLeaseExpired
    );
    assert_eq!(store.retained_candidate_count(), 0);

    // Afterwards nothing is retained, so the honest answer is "not found".
    assert_eq!(
        store.end(&access, at(301)).unwrap_err(),
        CandidateSessionFailure::CandidateSetNotFound
    );
}

// ── 13. Stale generations fail closed ─────────────────────────────────────

#[test]
fn stale_generations_fail_closed() {
    let (store, first_generation) = seeded();
    store
        .reject(&first_generation, &[Uuid::from_u128(1)], at(5))
        .expect("reject in generation 1");

    // A new Planning boundary opens generation 2.
    let mut next = create_request(PRODUCT_A, SESSION_1, LEASE_1);
    next.binding.plan_digest = digest("a-second-completed-plan");
    let generation_two = store.create(next, at(20)).expect("generation 2");
    assert_eq!(generation_two.candidate_set_generation, 2);

    // The owner presenting the stale generation fails closed — it must not be
    // silently retargeted onto the newer set.
    for probe in [
        store.enumerate(&first_generation, at(21)).err(),
        store.resume(&first_generation, at(21)).err(),
        store.heartbeat(&first_generation, at(21)).err(),
        store.end(&first_generation, at(21)).err(),
        store
            .reject(&first_generation, &[Uuid::from_u128(1)], at(21))
            .err(),
        store
            .record_promoted_disposition(&first_generation, &[Uuid::from_u128(1)], at(21))
            .err(),
    ] {
        assert_eq!(probe, Some(CandidateSessionFailure::CandidateSetStale));
    }

    // The newer generation is untouched by any of those stale attempts.
    let current = access_for(&store, PRODUCT_A, SESSION_1, LEASE_1, 2);
    let set = store.enumerate(&current, at(22)).expect("current");
    assert_eq!(set.count_with(CandidateDisposition::Pending), 3);
    assert_eq!(store.retained_candidate_count(), 3);
}

#[test]
fn unknown_candidate_ids_fail_closed_without_partial_application() {
    let (store, access) = seeded();

    // One known id and one that belongs to no candidate in this set.
    let err = store
        .reject(&access, &[Uuid::from_u128(1), Uuid::from_u128(999)], at(5))
        .expect_err("unknown id must fail closed");
    assert_eq!(err, CandidateSessionFailure::CandidateNotInSet);

    // The known id was not applied: validation precedes mutation.
    let set = store.enumerate(&access, at(6)).expect("enumerate");
    assert_eq!(set.count_with(CandidateDisposition::Rejected), 0);
    assert_eq!(set.count_with(CandidateDisposition::Pending), 3);
}

#[test]
fn malformed_identities_fail_closed() {
    let (store, _) = seeded();

    let empty_product = access_for(&store, "", SESSION_1, LEASE_1, 1);
    assert_eq!(
        store.enumerate(&empty_product, at(5)).unwrap_err(),
        CandidateSessionFailure::MalformedIdentity
    );

    let empty_session = access_for(&store, PRODUCT_A, "   ", LEASE_1, 1);
    assert_eq!(
        store.heartbeat(&empty_session, at(5)).unwrap_err(),
        CandidateSessionFailure::MalformedIdentity
    );

    let empty_lease = access_for(&store, PRODUCT_A, SESSION_1, "", 1);
    assert_eq!(
        store.end(&empty_lease, at(5)).unwrap_err(),
        CandidateSessionFailure::MalformedIdentity
    );

    // Nothing was mutated by any malformed request.
    assert_eq!(store.retained_candidate_count(), 3);
}

#[test]
fn unsupported_contract_versions_fail_closed_on_every_surface() {
    let (store, owner) = seeded();
    let mut stale_client = owner.clone();
    stale_client.contract_version = CANDIDATE_SESSION_CONTRACT_VERSION + 1;

    for probe in [
        store.enumerate(&stale_client, at(5)).err(),
        store.resume(&stale_client, at(5)).err(),
        store.heartbeat(&stale_client, at(5)).err(),
        store.end(&stale_client, at(5)).err(),
        store
            .reject(&stale_client, &[Uuid::from_u128(1)], at(5))
            .err(),
        store
            .record_promoted_disposition(&stale_client, &[Uuid::from_u128(1)], at(5))
            .err(),
    ] {
        assert_eq!(
            probe,
            Some(CandidateSessionFailure::UnsupportedContractVersion)
        );
    }

    assert_eq!(store.retained_candidate_count(), 3);
}

// ── 14. Daemon restart matches the documented support level ───────────────

#[test]
fn daemon_restart_is_not_recoverable_and_returns_typed_results() {
    // Documented support level: NOT RECOVERABLE. The store is memory-only; the
    // governed spec permits but does not require a durable operational store,
    // and #986 forbids adding one merely to pass a restart test.
    let mut store = CandidateSessionStore::new();
    store
        .create(create_request(PRODUCT_A, SESSION_1, LEASE_1), base())
        .expect("create");
    let access = access_for(&store, PRODUCT_A, SESSION_1, LEASE_1, 1);
    let epoch_before = store.lease_epoch();

    store.restart();

    assert_ne!(store.lease_epoch(), epoch_before);
    assert_eq!(store.active_set_count(), 0);
    assert_eq!(store.retained_candidate_count(), 0);

    // A typed result, not a reconstruction. Nothing is rebuilt from prompts,
    // transcripts, canonical history, global caches, or inferred context.
    let err = store
        .resume(&access, at(10))
        .expect_err("post-restart resume");
    assert_eq!(err, CandidateSessionFailure::CandidateSetNotFound);
    assert_eq!(
        store.heartbeat(&access, at(10)).unwrap_err(),
        CandidateSessionFailure::CandidateSetNotFound
    );
    assert_eq!(
        store.end(&access, at(10)).unwrap_err(),
        CandidateSessionFailure::CandidateSetNotFound
    );
}

#[test]
fn a_distinct_daemon_process_does_not_honor_a_prior_lease() {
    let store = CandidateSessionStore::new();
    let original = store
        .create(create_request(PRODUCT_A, SESSION_1, LEASE_1), base())
        .expect("create");
    let stale_access = access_for(
        &store,
        PRODUCT_A,
        SESSION_1,
        LEASE_1,
        original.candidate_set_generation,
    );
    let epoch = store.lease_epoch();

    let other = CandidateSessionStore::new();
    assert_ne!(other.lease_epoch(), epoch);
    let replacement = other
        .create(create_request(PRODUCT_A, SESSION_1, LEASE_1), at(1))
        .expect("replacement in distinct daemon process");
    assert_eq!(replacement.candidate_set_generation, 1);
    assert_eq!(
        other.enumerate(&stale_access, at(10)).unwrap_err(),
        CandidateSessionFailure::CandidateSetNotFound
    );
}

// ── 15. No prompt-carried, global, or canonical-event fallback ────────────

#[test]
fn no_global_or_canonical_fallback_exists_for_an_unbound_session() {
    let witness = MemoryAdapter::new();
    let store = CandidateSessionStore::new();

    // Live sets exist for two other sessions.
    store
        .create(create_request(PRODUCT_A, SESSION_1, LEASE_1), base())
        .expect("session 1");
    store
        .create(create_request(PRODUCT_B, SESSION_2, LEASE_2), base())
        .expect("session 2");

    // A session that never created a set gets nothing. There is no global
    // inbox to fall back to, no "current Product" default, and no canonical
    // event log to reconstruct from.
    let unbound = access_for(
        &store,
        PRODUCT_A,
        "host-session-unbound",
        "lease-unbound",
        1,
    );
    assert_eq!(
        store.enumerate(&unbound, at(10)).unwrap_err(),
        CandidateSessionFailure::CandidateSetNotFound
    );
    assert_eq!(
        store.resume(&unbound, at(10)).unwrap_err(),
        CandidateSessionFailure::CandidateSetNotFound
    );

    // Generation 0 and a very high generation are equally unhelpful — there is
    // no "latest set" lookup that ignores the presented identity.
    for generation in [0_u64, 1, 2, 99] {
        let probe = access_for(
            &store,
            PRODUCT_A,
            "host-session-unbound",
            "lease-unbound",
            generation,
        );
        assert_eq!(
            store.enumerate(&probe, at(10)).unwrap_err(),
            CandidateSessionFailure::CandidateSetNotFound
        );
    }

    assert!(witness.replay().expect("replay").is_empty());
    // Both real sets remain routed to their own owners only.
    assert_eq!(store.active_set_count(), 2);
}

#[test]
fn the_full_lifecycle_writes_nothing_canonical() {
    // One end-to-end pass over every operation this slice owns, with the
    // witness event store asserted empty at the end.
    let witness = MemoryAdapter::new();
    let store = CandidateSessionStore::new();

    store
        .create(create_request(PRODUCT_A, SESSION_1, LEASE_1), base())
        .expect("create");
    let access = access_for(&store, PRODUCT_A, SESSION_1, LEASE_1, 1);

    store.enumerate(&access, at(5)).expect("enumerate");
    store.heartbeat(&access, at(10)).expect("heartbeat");
    store
        .reject(&access, &[Uuid::from_u128(1)], at(15))
        .expect("reject");
    store
        .record_promoted_disposition(&access, &[Uuid::from_u128(2)], at(20))
        .expect("promoted disposition");
    store.resume(&access, at(250)).expect("resume");
    store.end(&access, at(260)).expect("end");
    store.sweep_expired(at(1000));

    assert!(
        witness.replay().expect("replay").is_empty(),
        "no operation in this slice may append canonical Product state"
    );
    assert_eq!(store.retained_candidate_count(), 0);
}

// ── Versioned lifecycle surface names ─────────────────────────────────────

#[test]
fn lifecycle_surface_names_match_the_governed_contract_table() {
    assert_eq!(
        CANDIDATE_SESSION_HEARTBEAT_COMMAND,
        "candidate_session.heartbeat"
    );
    assert_eq!(CANDIDATE_SESSION_END_COMMAND, "candidate_session.end");
    assert_eq!(CANDIDATE_SESSION_RECONNECT_GRACE_SECONDS, 300);
    assert_eq!(CANDIDATE_SESSION_CONTRACT_VERSION, 1);
}
