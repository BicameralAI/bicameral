//! Daemon-owned session store for Managed MCP `CandidateSetV1` (issue #986).
//!
//! This is the production implementation of Slice B of
//! `docs/specs/managed-mcp-drift-aware-continuation-runtime-architecture.md`
//! at revision `04e0471a`. It owns CandidateSet routing, the reconnect lease,
//! heartbeat/end, generation reuse, disposition transitions, and cleanup.
//!
//! Conformance: `CONF-MCP-CANDIDATE-SESSION-01`,
//! `CONF-MCP-SESSION-REJECTION-01`, `CONF-MCP-RECONNECT-CLEANUP-01`.
//!
//! # Authority boundary
//!
//! This module makes **no canonical Product write and has no way to make one**.
//! It holds no [`EventStoreAdapter`](bicameral_api::event_store::EventStoreAdapter),
//! no projection handle, and no Decision ledger reference. Rejection and
//! promotion here move a [`CandidateDisposition`] inside transient session
//! state and nothing else. Canonical Product state is advanced only by the
//! separate authorized append flow (Bot #987).
//!
//! # Routing and isolation
//!
//! Sets are routed by `(product_id, host_session_id)` with an active
//! `candidate_set_generation`, exactly as the governed spec requires. Working
//! directory, transcript content, agent prose, global active-Product state, and
//! prompt content are not inputs to routing — the store receives explicit
//! identity or it fails closed.
//!
//! A caller that does not own a set cannot distinguish "exists but is not
//! yours" from "does not exist": both return
//! [`CandidateSessionFailure::CandidateSetNotFound`].
//!
//! # Clock
//!
//! Every lease decision takes an explicit `now: DateTime<Utc>` from the caller.
//! The store reads no ambient clock, so lease behavior is deterministic under
//! test and the daemon remains the single source of time.
//!
//! # Restart and recovery — supported level: NOT RECOVERABLE
//!
//! State lives in daemon memory only. The governed spec permits ("may") a
//! durable operational store but does not require one, and issue #986 forbids
//! adding durable storage merely to make restart tests pass. So:
//!
//! - a daemon restart drops every CandidateSet;
//! - every post-restart operation returns
//!   [`CandidateSessionFailure::CandidateSetNotFound`], because the state is
//!   genuinely gone rather than merely unreachable;
//! - nothing is reconstructed from prompts, transcripts, canonical Product
//!   history, global caches, or inferred session context.
//!
//! [`CandidateSessionFailure::SessionLeaseExpired`] is returned when the
//! daemon can still distinguish the exact owner: either the active set has just
//! crossed its lease boundary or a timer sweep retained a bounded,
//! candidate-free expiry receipt. Candidate bodies and dispositions are erased
//! before that receipt is created.
//!
//! [`CandidateSessionStore::restart`] models this in-place for tests and for a
//! live daemon reusing the handle.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use bicameral_api::candidate_session::{
    is_sha256_digest, CandidateDisposition, CandidateDispositionResultV1, CandidateSessionAccessV1,
    CandidateSessionEndResultV1, CandidateSessionFailure, CandidateSessionHeartbeatResultV1,
    CandidateSetBindingV1, CandidateSetV1, SessionCandidateDraftV1, SessionCandidateV1,
    CANDIDATE_SESSION_CONTRACT_VERSION, CANDIDATE_SESSION_RECONNECT_GRACE_SECONDS,
};

/// Result alias for every store operation.
pub type CandidateSessionResult<T> = Result<T, CandidateSessionFailure>;

/// Routing key: exact Product plus exact host session.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RouteKey {
    product_id: String,
    host_session_id: String,
}

impl RouteKey {
    fn from_binding(binding: &CandidateSetBindingV1) -> Self {
        Self {
            product_id: binding.product_id.clone(),
            host_session_id: binding.host_session_id.clone(),
        }
    }

    fn from_access(access: &CandidateSessionAccessV1) -> Self {
        Self {
            product_id: access.product_id.clone(),
            host_session_id: access.host_session_id.clone(),
        }
    }
}

/// Candidate-free receipt retained briefly after timer-driven expiry.
///
/// It carries only enough identity to return `session_lease_expired` once to
/// the exact owning access capability. It contains no candidate body,
/// disposition, rejection, suppression, or canonical Product state.
#[derive(Debug, Clone)]
struct ExpiredReceipt {
    session_lease_id: String,
    generation: u64,
    retain_until: DateTime<Utc>,
}

/// Internally synchronized operational state.
///
/// `generation_floors` prevents a cleaned route from reusing an old
/// generation. It retains no candidate or disposition data and is bounded by
/// the number of Product/session routes observed during the daemon process.
#[derive(Debug, Default)]
struct StoreState {
    active: HashMap<RouteKey, StoredSet>,
    generation_floors: HashMap<RouteKey, u64>,
    expired_receipts: HashMap<RouteKey, ExpiredReceipt>,
}

/// Everything the daemon retains for one active CandidateSet.
#[derive(Debug, Clone)]
struct StoredSet {
    candidate_set_id: Uuid,
    generation: u64,
    digest: String,
    binding: CandidateSetBindingV1,
    session_lease_id: String,
    created_at: DateTime<Utc>,
    lease_expires_at: DateTime<Utc>,
    candidates: Vec<SessionCandidateV1>,
    coverage_limitations: Vec<String>,
}

impl StoredSet {
    fn view(&self) -> CandidateSetV1 {
        CandidateSetV1 {
            contract_version: CANDIDATE_SESSION_CONTRACT_VERSION,
            candidate_set_id: self.candidate_set_id,
            candidate_set_generation: self.generation,
            candidate_set_digest: self.digest.clone(),
            binding: self.binding.clone(),
            session_lease_id: self.session_lease_id.clone(),
            created_at: self.created_at,
            lease_expires_at: self.lease_expires_at,
            candidates: self.candidates.clone(),
            coverage_limitations: self.coverage_limitations.clone(),
        }
    }

    fn count_with(&self, disposition: CandidateDisposition) -> usize {
        self.candidates
            .iter()
            .filter(|c| c.disposition == disposition)
            .count()
    }
}

/// Daemon-owned CandidateSet session store.
///
/// Shared behind `&self`; all mutation is internally synchronized so one
/// daemon-owned instance can be threaded through dispatch.
pub struct CandidateSessionStore {
    /// Per-process epoch, embedded in every minted lease. Rotated on restart.
    lease_epoch: Uuid,
    inner: Mutex<StoreState>,
}

impl std::fmt::Debug for CandidateSessionStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Coarse counts only: candidate bodies and spec content stay local and
        // never reach a log line or debug rendering.
        let inner = self.inner.lock().expect("candidate session store poisoned");
        f.debug_struct("CandidateSessionStore")
            .field("lease_epoch", &self.lease_epoch)
            .field("active_sets", &inner.active.len())
            .field("generation_floors", &inner.generation_floors.len())
            .field("expiry_receipts", &inner.expired_receipts.len())
            .finish()
    }
}

impl Default for CandidateSessionStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Request to materialize (or reuse) one CandidateSet generation.
#[derive(Debug, Clone)]
pub struct CreateCandidateSetRequest {
    pub contract_version: u32,
    pub binding: CandidateSetBindingV1,
    /// Opaque lease identity minted by the daemon for this coding session.
    pub session_lease_id: String,
    pub candidates: Vec<SessionCandidateDraftV1>,
    pub coverage_limitations: Vec<String>,
}

impl CandidateSessionStore {
    /// Construct an empty store under a fresh lease epoch.
    pub fn new() -> Self {
        Self {
            lease_epoch: Uuid::new_v4(),
            inner: Mutex::new(StoreState::default()),
        }
    }

    /// Current lease epoch. Exposed for diagnostics and restart tests.
    pub fn lease_epoch(&self) -> Uuid {
        self.lease_epoch
    }

    /// Bind an opaque host-session lease identity to this daemon process.
    ///
    /// The returned token is the only lease value that may be used for
    /// CandidateSet access. Binding the process epoch into the opaque token
    /// prevents an access capability from a prior daemon process from
    /// addressing replacement state that happens to reuse the same Product,
    /// host-session identity, and generation.
    pub fn scope_session_lease_id(
        &self,
        host_session_lease_id: &str,
    ) -> CandidateSessionResult<String> {
        const PREFIX: &str = "daemon-lease-v1:";
        let value = host_session_lease_id.trim();
        if value.is_empty() {
            return Err(CandidateSessionFailure::MalformedIdentity);
        }
        if let Some(scoped) = value.strip_prefix(PREFIX) {
            let Some((epoch, lease)) = scoped.split_once(':') else {
                return Err(CandidateSessionFailure::MalformedIdentity);
            };
            let epoch =
                Uuid::parse_str(epoch).map_err(|_| CandidateSessionFailure::MalformedIdentity)?;
            if lease.trim().is_empty() {
                return Err(CandidateSessionFailure::MalformedIdentity);
            }
            if epoch != self.lease_epoch {
                return Err(CandidateSessionFailure::CandidateSetNotFound);
            }
            return Ok(value.to_owned());
        }
        Ok(format!("{PREFIX}{}:{value}", self.lease_epoch))
    }

    fn lease_matches_epoch(&self, session_lease_id: &str) -> bool {
        let prefix = format!("daemon-lease-v1:{}:", self.lease_epoch);
        session_lease_id
            .strip_prefix(&prefix)
            .is_some_and(|lease| !lease.trim().is_empty())
    }

    /// Model a daemon restart in place: rotate the epoch and drop all sets.
    ///
    /// This is the honest production behavior, not a test affordance: the store
    /// is memory-only, so a real process restart loses exactly this state.
    /// Constructing a new store has the same effect.
    pub fn restart(&mut self) {
        self.lease_epoch = Uuid::new_v4();
        let mut inner = self.inner.lock().expect("candidate session store poisoned");
        inner.active.clear();
        inner.expired_receipts.clear();
        // Preserve candidate-free generation floors so an in-place daemon
        // restart cannot make an old access capability address replacement
        // state. A real process restart also rotates the daemon-minted lease
        // epoch, so old lease identities are not reused.
    }

    /// Number of active sets. Diagnostics and cleanup assertions.
    pub fn active_set_count(&self) -> usize {
        let inner = self.inner.lock().expect("candidate session store poisoned");
        inner.active.len()
    }

    /// Total retained candidates across all sets, in every disposition.
    ///
    /// Cleanup evidence: after end or expiry this must reach zero for the
    /// erased session.
    pub fn retained_candidate_count(&self) -> usize {
        let inner = self.inner.lock().expect("candidate session store poisoned");
        inner.active.values().map(|s| s.candidates.len()).sum()
    }

    // ── Creation ──────────────────────────────────────────────────────────

    /// Materialize a CandidateSet for one analysis identity.
    ///
    /// Retrying the same analysis identity (equal [`CandidateSetBindingV1`])
    /// for a live, unexpired set reuses that generation and returns it
    /// unchanged, preserving dispositions and digest.
    ///
    /// A different analysis identity for the same Product/session opens the
    /// next generation. Pending and rejected dispositions are **not** copied
    /// forward: every candidate in the new generation starts `pending`.
    pub fn create(
        &self,
        request: CreateCandidateSetRequest,
        now: DateTime<Utc>,
    ) -> CandidateSessionResult<CandidateSetV1> {
        if request.contract_version != CANDIDATE_SESSION_CONTRACT_VERSION {
            return Err(CandidateSessionFailure::UnsupportedContractVersion);
        }
        validate_binding(&request.binding)?;
        let session_lease_id = self.scope_session_lease_id(&request.session_lease_id)?;
        validate_drafts(&request.candidates)?;

        let key = RouteKey::from_binding(&request.binding);
        let mut inner = self.inner.lock().expect("candidate session store poisoned");

        inner
            .expired_receipts
            .retain(|_, receipt| now < receipt.retain_until);

        // Lazy create-path expiry erases candidate/disposition state but keeps
        // a bounded, candidate-free receipt so the exact old owner can still
        // receive the governed typed expiry result.
        let should_expire = inner
            .active
            .get(&key)
            .is_some_and(|existing| now >= existing.lease_expires_at);
        if should_expire {
            let expired = inner
                .active
                .remove(&key)
                .expect("expired set present after inspection");
            inner.expired_receipts.insert(
                key.clone(),
                ExpiredReceipt {
                    session_lease_id: expired.session_lease_id,
                    generation: expired.generation,
                    retain_until: now
                        + Duration::seconds(CANDIDATE_SESSION_RECONNECT_GRACE_SECONDS),
                },
            );
        }

        if let Some(existing) = inner.active.get(&key) {
            // Exact lease ownership is required before either idempotent reuse
            // or replacement under a new Planning boundary. A forged lease is
            // indistinguishable from absence.
            if existing.session_lease_id != session_lease_id {
                return Err(CandidateSessionFailure::CandidateSetNotFound);
            }
            if existing.binding == request.binding {
                return Ok(existing.view());
            }
        }

        if let Some(receipt) = inner.expired_receipts.get(&key) {
            // While an expiry receipt is live, only the same lease may reopen
            // this exact Product/session route. A new host session must use a
            // new host_session_id rather than taking over another lease.
            if receipt.session_lease_id != session_lease_id {
                return Err(CandidateSessionFailure::CandidateSetNotFound);
            }
        }

        let next_generation = inner
            .generation_floors
            .get(&key)
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(CandidateSessionFailure::CandidateSetStale)?;
        inner.generation_floors.insert(key.clone(), next_generation);

        let candidates: Vec<SessionCandidateV1> = request
            .candidates
            .iter()
            .map(|draft| SessionCandidateV1 {
                candidate_id: draft.candidate_id,
                candidate_generation: next_generation,
                proposed_decision: draft.proposed_decision.clone(),
                source_spans: draft.source_spans.clone(),
                contradiction_refs: draft.contradiction_refs.clone(),
                dependency_candidate_ids: draft.dependency_candidate_ids.clone(),
                claim_trace_digest: draft.claim_trace_digest.clone(),
                proposal_digest: draft.proposal_digest.clone(),
                // Disposition is daemon-authored, never caller-supplied.
                disposition: CandidateDisposition::Pending,
            })
            .collect();

        let stored = StoredSet {
            candidate_set_id: Uuid::new_v4(),
            generation: next_generation,
            digest: candidate_set_digest(&request.binding, next_generation, &candidates),
            binding: request.binding,
            session_lease_id,
            created_at: now,
            lease_expires_at: now + Duration::seconds(CANDIDATE_SESSION_RECONNECT_GRACE_SECONDS),
            candidates,
            coverage_limitations: request.coverage_limitations,
        };

        let view = stored.view();
        inner.active.insert(key, stored);
        Ok(view)
    }

    // ── Read / resume ─────────────────────────────────────────────────────

    /// Enumerate the CandidateSet for its exact owning Product and session.
    ///
    /// Read-only: does not extend the lease. Used for same-session turns.
    pub fn enumerate(
        &self,
        access: &CandidateSessionAccessV1,
        now: DateTime<Utc>,
    ) -> CandidateSessionResult<CandidateSetV1> {
        let mut inner = self.inner.lock().expect("candidate session store poisoned");
        let stored = self.locate(&mut inner, access, now)?;
        Ok(stored.view())
    }

    /// Resume the CandidateSet after a disconnect, within the 300-second grace.
    ///
    /// On success the lease is extended from `now` and the full set is
    /// returned with candidate identities, generation, digest, and every
    /// pending/rejected/promoted disposition preserved. Resume creates no
    /// canonical event.
    pub fn resume(
        &self,
        access: &CandidateSessionAccessV1,
        now: DateTime<Utc>,
    ) -> CandidateSessionResult<CandidateSetV1> {
        let mut inner = self.inner.lock().expect("candidate session store poisoned");
        let stored = self.locate(&mut inner, access, now)?;
        stored.lease_expires_at =
            now + Duration::seconds(CANDIDATE_SESSION_RECONNECT_GRACE_SECONDS);
        Ok(stored.view())
    }

    // ── Lifecycle surfaces ────────────────────────────────────────────────

    /// `candidate_session.heartbeat` — extend only the matching session lease.
    ///
    /// Idempotent: repeating a heartbeat at the same `now` produces an
    /// identical result and mutates nothing else. It never advances the
    /// generation, changes the digest, or touches a disposition.
    pub fn heartbeat(
        &self,
        access: &CandidateSessionAccessV1,
        now: DateTime<Utc>,
    ) -> CandidateSessionResult<CandidateSessionHeartbeatResultV1> {
        let mut inner = self.inner.lock().expect("candidate session store poisoned");
        let stored = self.locate(&mut inner, access, now)?;
        stored.lease_expires_at =
            now + Duration::seconds(CANDIDATE_SESSION_RECONNECT_GRACE_SECONDS);
        Ok(CandidateSessionHeartbeatResultV1 {
            contract_version: CANDIDATE_SESSION_CONTRACT_VERSION,
            candidate_set_id: stored.candidate_set_id,
            candidate_set_generation: stored.generation,
            candidate_set_digest: stored.digest.clone(),
            lease_expires_at: stored.lease_expires_at,
            pending_count: stored.count_with(CandidateDisposition::Pending),
            rejected_count: stored.count_with(CandidateDisposition::Rejected),
            promoted_count: stored.count_with(CandidateDisposition::Promoted),
        })
    }

    /// `candidate_session.end` — erase unpromoted session state.
    ///
    /// Erases every candidate in every disposition, the lease, and the session
    /// index entry. It synthesizes no rejection and writes no canonical state;
    /// any canonical Decision appended earlier by the separate authorized flow
    /// lives in Product history and is untouched.
    ///
    /// Idempotency: erasure converges. A duplicate end finds nothing to erase
    /// and returns the typed
    /// [`CandidateSessionFailure::CandidateSetNotFound`] rather than erasing a
    /// second time or synthesizing state. That is also exactly the outcome a
    /// non-owning caller receives, so a retry reveals nothing new.
    pub fn end(
        &self,
        access: &CandidateSessionAccessV1,
        now: DateTime<Utc>,
    ) -> CandidateSessionResult<CandidateSessionEndResultV1> {
        let mut inner = self.inner.lock().expect("candidate session store poisoned");
        // Ownership, generation, lease, and version are all validated before a
        // single byte is erased, so a mismatched or stale caller cannot delete
        // another session's state.
        let stored = self.locate(&mut inner, access, now)?;

        let result = CandidateSessionEndResultV1 {
            contract_version: CANDIDATE_SESSION_CONTRACT_VERSION,
            candidate_set_id: stored.candidate_set_id,
            candidate_set_generation: stored.generation,
            erased_candidate_count: stored.candidates.len(),
            erased_pending_count: stored.count_with(CandidateDisposition::Pending),
            erased_rejected_count: stored.count_with(CandidateDisposition::Rejected),
            erased_promoted_disposition_count: stored.count_with(CandidateDisposition::Promoted),
        };

        inner.active.remove(&RouteKey::from_access(access));
        Ok(result)
    }

    // ── Disposition transitions ───────────────────────────────────────────

    /// Mark candidates [`CandidateDisposition::Rejected`] in this session only.
    ///
    /// This writes no `claim_set_candidate_rejected` event, creates no
    /// tombstone or durable suppression, asserts nothing about semantic truth,
    /// and does not prevent the same idea from being proposed in a future
    /// session. The rejection is erased with the set at end or expiry.
    pub fn reject(
        &self,
        access: &CandidateSessionAccessV1,
        candidate_ids: &[Uuid],
        now: DateTime<Utc>,
    ) -> CandidateSessionResult<CandidateDispositionResultV1> {
        self.set_disposition(access, candidate_ids, CandidateDisposition::Rejected, now)
    }

    /// Record a promoted disposition as session UI-reconciliation state.
    ///
    /// This grants no canonical authority. It records only that the session
    /// surface should render the candidate as promoted; the canonical Decision
    /// is appended, if at all, by the separate authorized flow (Bot #987).
    pub fn record_promoted_disposition(
        &self,
        access: &CandidateSessionAccessV1,
        candidate_ids: &[Uuid],
        now: DateTime<Utc>,
    ) -> CandidateSessionResult<CandidateDispositionResultV1> {
        self.set_disposition(access, candidate_ids, CandidateDisposition::Promoted, now)
    }

    /// Atomically reconcile one confirmed selection into session-only
    /// dispositions after the canonical batch has replay-verified.
    ///
    /// Every id and every pending precondition is checked before any candidate
    /// changes, so a reconciliation failure never leaves a partially updated
    /// CandidateSet. This method writes no canonical event.
    pub fn apply_confirmed_selection(
        &self,
        access: &CandidateSessionAccessV1,
        promoted_ids: &[Uuid],
        rejected_ids: &[Uuid],
        now: DateTime<Utc>,
    ) -> CandidateSessionResult<CandidateSetV1> {
        let promoted: BTreeSet<Uuid> = promoted_ids.iter().copied().collect();
        let rejected: BTreeSet<Uuid> = rejected_ids.iter().copied().collect();
        if promoted.len() != promoted_ids.len()
            || rejected.len() != rejected_ids.len()
            || promoted.iter().any(|id| rejected.contains(id))
        {
            return Err(CandidateSessionFailure::MalformedIdentity);
        }

        let mut inner = self.inner.lock().expect("candidate session store poisoned");
        let stored = self.locate(&mut inner, access, now)?;
        let selected: BTreeSet<Uuid> = promoted.union(&rejected).copied().collect();
        for candidate_id in &selected {
            let candidate = stored
                .candidates
                .iter()
                .find(|candidate| candidate.candidate_id == *candidate_id)
                .ok_or(CandidateSessionFailure::CandidateNotInSet)?;
            if candidate.disposition != CandidateDisposition::Pending {
                return Err(CandidateSessionFailure::CandidateSetStale);
            }
        }

        for candidate in &mut stored.candidates {
            if promoted.contains(&candidate.candidate_id) {
                candidate.disposition = CandidateDisposition::Promoted;
            } else if rejected.contains(&candidate.candidate_id) {
                candidate.disposition = CandidateDisposition::Rejected;
            }
        }
        Ok(stored.view())
    }

    fn set_disposition(
        &self,
        access: &CandidateSessionAccessV1,
        candidate_ids: &[Uuid],
        disposition: CandidateDisposition,
        now: DateTime<Utc>,
    ) -> CandidateSessionResult<CandidateDispositionResultV1> {
        let mut inner = self.inner.lock().expect("candidate session store poisoned");
        let stored = self.locate(&mut inner, access, now)?;

        // Validate every id before mutating any: an unknown id fails the whole
        // request closed rather than applying a partial transition.
        for id in candidate_ids {
            if !stored.candidates.iter().any(|c| c.candidate_id == *id) {
                return Err(CandidateSessionFailure::CandidateNotInSet);
            }
        }

        for candidate in stored.candidates.iter_mut() {
            if candidate_ids.contains(&candidate.candidate_id) {
                candidate.disposition = disposition;
            }
        }

        Ok(CandidateDispositionResultV1 {
            contract_version: CANDIDATE_SESSION_CONTRACT_VERSION,
            candidate_set_id: stored.candidate_set_id,
            candidate_set_generation: stored.generation,
            // Unchanged: the digest covers identities and relations, not
            // dispositions.
            candidate_set_digest: stored.digest.clone(),
            applied: candidate_ids.to_vec(),
            pending_count: stored.count_with(CandidateDisposition::Pending),
            rejected_count: stored.count_with(CandidateDisposition::Rejected),
            promoted_count: stored.count_with(CandidateDisposition::Promoted),
        })
    }

    // ── Cleanup ───────────────────────────────────────────────────────────

    /// Erase every set whose reconnect lease has elapsed at `now`.
    ///
    /// The daemon calls this on a timer; access paths also expire lazily. Both
    /// routes erase identically and neither synthesizes a rejection or any
    /// other state. Returns the number of sets erased.
    pub fn sweep_expired(&self, now: DateTime<Utc>) -> usize {
        let mut inner = self.inner.lock().expect("candidate session store poisoned");
        inner
            .expired_receipts
            .retain(|_, receipt| now < receipt.retain_until);

        let expired_keys: Vec<RouteKey> = inner
            .active
            .iter()
            .filter(|(_, set)| now >= set.lease_expires_at)
            .map(|(key, _)| key.clone())
            .collect();

        for key in &expired_keys {
            let expired = inner
                .active
                .remove(key)
                .expect("expired set present after collection");
            inner.expired_receipts.insert(
                key.clone(),
                ExpiredReceipt {
                    session_lease_id: expired.session_lease_id,
                    generation: expired.generation,
                    retain_until: now
                        + Duration::seconds(CANDIDATE_SESSION_RECONNECT_GRACE_SECONDS),
                },
            );
        }

        expired_keys.len()
    }

    // ── Internal routing ──────────────────────────────────────────────────

    /// Resolve `access` to a live, owned, current-generation set.
    ///
    /// Check order is deliberate. Version and shape are checked first so a
    /// malformed request never reaches routing. Ownership is checked before
    /// generation and lease so a non-owning caller always receives
    /// `CandidateSetNotFound` and can never observe `candidate_set_stale` or
    /// `session_lease_expired` for a set it does not own — those codes would
    /// otherwise confirm the set exists.
    fn locate<'a>(
        &self,
        inner: &'a mut StoreState,
        access: &CandidateSessionAccessV1,
        now: DateTime<Utc>,
    ) -> CandidateSessionResult<&'a mut StoredSet> {
        if access.contract_version != CANDIDATE_SESSION_CONTRACT_VERSION {
            return Err(CandidateSessionFailure::UnsupportedContractVersion);
        }
        if access.product_id.trim().is_empty()
            || access.host_session_id.trim().is_empty()
            || access.session_lease_id.trim().is_empty()
        {
            return Err(CandidateSessionFailure::MalformedIdentity);
        }
        if !self.lease_matches_epoch(&access.session_lease_id) {
            return Err(CandidateSessionFailure::CandidateSetNotFound);
        }

        let key = RouteKey::from_access(access);
        let receipt_state = inner.expired_receipts.get(&key).map(|receipt| {
            (
                now >= receipt.retain_until,
                receipt.session_lease_id == access.session_lease_id
                    && receipt.generation == access.candidate_set_generation,
            )
        });

        match receipt_state {
            Some((true, _)) => {
                inner.expired_receipts.remove(&key);
            }
            Some((false, true)) => {
                // One-shot typed expiry for the exact owner. Removing the
                // receipt leaves no candidate, rejection, or suppression state.
                inner.expired_receipts.remove(&key);
                return Err(CandidateSessionFailure::SessionLeaseExpired);
            }
            _ => {}
        }

        let expired = match inner.active.get(&key) {
            None => return Err(CandidateSessionFailure::CandidateSetNotFound),
            Some(set) => {
                if set.session_lease_id != access.session_lease_id {
                    return Err(CandidateSessionFailure::CandidateSetNotFound);
                }
                if set.generation != access.candidate_set_generation {
                    return Err(CandidateSessionFailure::CandidateSetStale);
                }
                now >= set.lease_expires_at
            }
        };

        if expired {
            inner.active.remove(&key);
            return Err(CandidateSessionFailure::SessionLeaseExpired);
        }

        Ok(inner
            .active
            .get_mut(&key)
            .expect("set present after validation"))
    }
}

// ── Digest ────────────────────────────────────────────────────────────────

/// Deterministic `sha256:` digest over ordered candidate identities and
/// relations, plus the exact analysis identity binding.
///
/// Dispositions are excluded on purpose: a selection binds to this digest, and
/// it must stay stable while candidates move between pending, rejected, and
/// promoted within one generation, and across a reconnect.
fn candidate_set_digest(
    binding: &CandidateSetBindingV1,
    generation: u64,
    candidates: &[SessionCandidateV1],
) -> String {
    let mut hasher = Sha256::new();
    // Length-prefixed field framing so no two distinct inputs can serialize to
    // the same byte stream.
    let mut field = |value: &str| {
        hasher.update((value.len() as u64).to_be_bytes());
        hasher.update(value.as_bytes());
    };

    field("candidate_set_v1");
    field(&binding.product_id);
    field(&binding.host_session_id);
    field(&binding.governing_spec_binding_digest);
    field(&binding.plan_digest);
    field(&binding.accepted_model_base_position.to_string());
    field(&binding.accepted_model_base_digest);
    field(&generation.to_string());
    field(&candidates.len().to_string());

    for candidate in candidates {
        field(&candidate.candidate_id.to_string());
        field(&candidate.candidate_generation.to_string());
        field(&candidate.claim_trace_digest);
        field(&candidate.proposal_digest);

        // Relations are order-normalized so an equivalent set of relations
        // always digests identically.
        let mut deps: Vec<String> = candidate
            .dependency_candidate_ids
            .iter()
            .map(|d| d.to_string())
            .collect();
        deps.sort();
        field(&deps.len().to_string());
        for dep in &deps {
            field(dep);
        }

        let mut contradictions = candidate.contradiction_refs.clone();
        contradictions.sort();
        field(&contradictions.len().to_string());
        for reference in &contradictions {
            field(reference);
        }

        let mut spans: Vec<String> = candidate
            .source_spans
            .iter()
            .map(|s| format!("{}:{}:{}", s.artifact_path, s.start_byte, s.end_byte))
            .collect();
        spans.sort();
        field(&spans.len().to_string());
        for span in &spans {
            field(span);
        }
    }

    format!("sha256:{:x}", hasher.finalize())
}

// ── Validation ────────────────────────────────────────────────────────────

fn validate_binding(binding: &CandidateSetBindingV1) -> CandidateSessionResult<()> {
    if binding.product_id.trim().is_empty() || binding.host_session_id.trim().is_empty() {
        return Err(CandidateSessionFailure::MalformedIdentity);
    }
    if !is_sha256_digest(&binding.governing_spec_binding_digest)
        || !is_sha256_digest(&binding.plan_digest)
        || !is_sha256_digest(&binding.accepted_model_base_digest)
    {
        return Err(CandidateSessionFailure::MalformedIdentity);
    }
    Ok(())
}

fn validate_drafts(drafts: &[SessionCandidateDraftV1]) -> CandidateSessionResult<()> {
    let mut candidate_ids = HashSet::with_capacity(drafts.len());
    for draft in drafts {
        validate_draft(draft)?;
        if !candidate_ids.insert(draft.candidate_id) {
            return Err(CandidateSessionFailure::MalformedIdentity);
        }
    }

    for draft in drafts {
        let mut dependencies = HashSet::with_capacity(draft.dependency_candidate_ids.len());
        for dependency_id in &draft.dependency_candidate_ids {
            if *dependency_id == draft.candidate_id
                || !candidate_ids.contains(dependency_id)
                || !dependencies.insert(*dependency_id)
            {
                return Err(CandidateSessionFailure::MalformedIdentity);
            }
        }
    }
    Ok(())
}

fn validate_draft(draft: &SessionCandidateDraftV1) -> CandidateSessionResult<()> {
    if draft.candidate_id.is_nil()
        || draft.proposed_decision.trim().is_empty()
        || !is_sha256_digest(&draft.claim_trace_digest)
        || !is_sha256_digest(&draft.proposal_digest)
        || draft
            .source_spans
            .iter()
            .any(|span| span.artifact_path.trim().is_empty() || span.start_byte >= span.end_byte)
    {
        return Err(CandidateSessionFailure::MalformedIdentity);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(seed: &str) -> String {
        format!("sha256:{:x}", Sha256::digest(seed.as_bytes()))
    }

    fn binding() -> CandidateSetBindingV1 {
        CandidateSetBindingV1 {
            product_id: "product-a".into(),
            host_session_id: "session-1".into(),
            governing_spec_binding_digest: digest("spec"),
            plan_digest: digest("plan"),
            accepted_model_base_position: 7,
            accepted_model_base_digest: digest("base"),
        }
    }

    fn draft(n: u128) -> SessionCandidateDraftV1 {
        SessionCandidateDraftV1 {
            candidate_id: Uuid::from_u128(n),
            proposed_decision: format!("decision {n}"),
            source_spans: Vec::new(),
            contradiction_refs: Vec::new(),
            dependency_candidate_ids: Vec::new(),
            claim_trace_digest: digest(&format!("trace-{n}")),
            proposal_digest: digest(&format!("proposal-{n}")),
        }
    }

    fn create_request() -> CreateCandidateSetRequest {
        CreateCandidateSetRequest {
            contract_version: CANDIDATE_SESSION_CONTRACT_VERSION,
            binding: binding(),
            session_lease_id: "lease-1".into(),
            candidates: vec![draft(1), draft(2)],
            coverage_limitations: vec!["bounded to the bound spec revision".into()],
        }
    }

    #[test]
    fn digest_is_stable_across_disposition_changes() {
        let store = CandidateSessionStore::new();
        let now = Utc::now();
        let set = store.create(create_request(), now).expect("create");
        let access = CandidateSessionAccessV1::new(
            "product-a",
            "session-1",
            store
                .scope_session_lease_id("lease-1")
                .expect("scope lease"),
            1,
        );
        let after = store
            .reject(&access, &[Uuid::from_u128(1)], now)
            .expect("reject");
        assert_eq!(after.candidate_set_digest, set.candidate_set_digest);
    }

    #[test]
    fn digest_changes_when_relations_change() {
        let store = CandidateSessionStore::new();
        let now = Utc::now();
        let a = store.create(create_request(), now).expect("a");

        let mut request = create_request();
        request.binding.host_session_id = "session-2".into();
        request.candidates[1].dependency_candidate_ids = vec![Uuid::from_u128(1)];
        let b = store.create(request, now).expect("b");

        assert_ne!(a.candidate_set_digest, b.candidate_set_digest);
    }

    #[test]
    fn malformed_digest_fails_closed() {
        let store = CandidateSessionStore::new();
        let mut request = create_request();
        request.binding.plan_digest = "not-a-digest".into();
        assert_eq!(
            store.create(request, Utc::now()).unwrap_err(),
            CandidateSessionFailure::MalformedIdentity
        );
    }

    #[test]
    fn unsupported_contract_version_fails_closed() {
        let store = CandidateSessionStore::new();
        let mut request = create_request();
        request.contract_version = 99;
        assert_eq!(
            store.create(request, Utc::now()).unwrap_err(),
            CandidateSessionFailure::UnsupportedContractVersion
        );
    }

    #[test]
    fn debug_never_renders_candidate_bodies() {
        let store = CandidateSessionStore::new();
        store.create(create_request(), Utc::now()).expect("create");
        let rendered = format!("{store:?}");
        assert!(!rendered.contains("decision 1"));
        assert!(rendered.contains("active_sets"));
    }
}
