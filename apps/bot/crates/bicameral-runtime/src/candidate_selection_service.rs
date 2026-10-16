//! Daemon-owned CandidateSet selection prepare/commit service (issue #987).
//!
//! The service validates exact stable identities, dependency closure, current
//! Product authority, spec/base/policy currentness, and explicit human
//! confirmation. Canonical promotion events cross one atomic event-store batch
//! boundary; explicit rejection remains CandidateSet session state only.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use bicameral_api::candidate::{
    CandidateState, DecisionCandidate, DecisionLevel, ExtractionConfidence, ReviewLifecycleState,
};
use bicameral_api::candidate_promotion::CandidatePromotedPayload;
use bicameral_api::candidate_selection::{
    CandidatePromotionEffectV1, CandidateRejectionEffectV1, CandidateSelectionAuthorityV1,
    CandidateSelectionCommitV1, CandidateSelectionConfirmationV1, CandidateSelectionFailureCode,
    CandidateSelectionFailureV1, CandidateSelectionPreviewV1, CandidateSelectionReceiptV1,
    CandidateSelectionResult, CandidateSelectionSignoffMode, CandidateSelectionV1,
    CANDIDATE_SELECTION_CONTRACT_VERSION,
};
use bicameral_api::candidate_session::{
    CandidateDisposition, CandidateSessionAccessV1, CandidateSessionFailure, CandidateSetV1,
    SessionCandidateV1,
};
use bicameral_api::confirmation_challenge::{
    ChallengeSecret, ConfirmationOutcome, CONFIRMATION_CHALLENGE_TTL_SECONDS,
};
use bicameral_api::dashboard::SignoffState;
use bicameral_api::event_store::{EventStoreEntry, EventType};
use chrono::{DateTime, Duration, Utc};
use ring::rand::{SecureRandom, SystemRandom};
use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::atomic_event_store::{AtomicEventBatchV1, AtomicEventStoreAdapter};
use crate::candidate_session_store::CandidateSessionStore;

const SELECTION_SECRET_BYTES: usize = 32;

/// Trusted authority/currentness boundary used by the daemon.
///
/// Implementations read canonical Product membership and policy projections.
/// MCP requests never supply or override the returned snapshot.
pub trait CandidateSelectionAuthorityProvider: Send + Sync {
    fn current_authority(
        &self,
        product_id: &str,
        actor_id: &str,
    ) -> CandidateSelectionResult<CandidateSelectionAuthorityV1>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewStatus {
    Pending,
    Applying,
    Consumed,
}

#[derive(Debug, Clone)]
struct StoredPreview {
    selection: CandidateSelectionV1,
    preview: CandidateSelectionPreviewV1,
    status: PreviewStatus,
}

#[derive(Default)]
struct PreviewStoreInner {
    previews: HashMap<Uuid, StoredPreview>,
    applied: HashMap<Uuid, CandidateSelectionReceiptV1>,
}

/// Daemon-memory preview/challenge authority.
struct CandidateSelectionPreviewStore {
    restart_epoch: Uuid,
    rng: SystemRandom,
    inner: Mutex<PreviewStoreInner>,
}

impl CandidateSelectionPreviewStore {
    fn new() -> Self {
        Self {
            restart_epoch: Uuid::new_v4(),
            rng: SystemRandom::new(),
            inner: Mutex::new(PreviewStoreInner::default()),
        }
    }

    fn issue(
        &self,
        selection: CandidateSelectionV1,
        draft: PreviewDraft,
        now: DateTime<Utc>,
    ) -> CandidateSelectionPreviewV1 {
        let challenge_id = Uuid::new_v4();
        let secret = self.mint_secret();
        let confirmation_challenge = CandidateSelectionConfirmationV1 {
            challenge_id,
            restart_epoch: self.restart_epoch,
            secret,
            expires_at: now + Duration::seconds(CONFIRMATION_CHALLENGE_TTL_SECONDS),
        };
        let preview = draft.into_preview(confirmation_challenge);
        let stored = StoredPreview {
            selection,
            preview: preview.clone(),
            status: PreviewStatus::Pending,
        };
        self.inner
            .lock()
            .expect("selection preview store poisoned")
            .previews
            .insert(challenge_id, stored);
        preview
    }

    fn applied(
        &self,
        operation_id: Uuid,
        selection_digest: &str,
    ) -> Option<CandidateSelectionReceiptV1> {
        self.inner
            .lock()
            .expect("selection preview store poisoned")
            .applied
            .get(&operation_id)
            .filter(|receipt| receipt.selection_digest == selection_digest)
            .cloned()
    }

    fn begin(
        &self,
        commit: &CandidateSelectionCommitV1,
        now: DateTime<Utc>,
    ) -> CandidateSelectionResult<StoredPreview> {
        if commit.confirmation_token.restart_epoch != self.restart_epoch {
            return Err(confirmation_failure(
                ConfirmationOutcome::ChallengeUnavailableAfterRestart,
            ));
        }

        let mut inner = self.inner.lock().expect("selection preview store poisoned");
        let stored = inner
            .previews
            .get_mut(&commit.confirmation_token.challenge_id)
            .ok_or_else(|| {
                CandidateSelectionFailureV1::new(
                    CandidateSelectionFailureCode::PreviewNotFound,
                    "selection preview is not available",
                )
            })?;

        if stored.status != PreviewStatus::Pending {
            return Err(confirmation_failure(ConfirmationOutcome::AlreadyConsumed));
        }
        if now >= stored.preview.confirmation_challenge.expires_at {
            return Err(confirmation_failure(ConfirmationOutcome::Expired));
        }
        if !stored
            .preview
            .confirmation_challenge
            .secret
            .matches(&commit.confirmation_token.secret)
        {
            return Err(confirmation_failure(ConfirmationOutcome::Unauthorized));
        }
        if stored.preview.selection_operation_id != commit.selection_operation_id
            || stored.preview.selection_digest != commit.selection_digest
        {
            return Err(CandidateSelectionFailureV1::new(
                CandidateSelectionFailureCode::PreviewStale,
                "confirmed selection does not match the issued preview",
            ));
        }
        if stored.preview.actor_id != commit.actor_id {
            return Err(confirmation_failure(ConfirmationOutcome::ActorMismatch));
        }

        stored.status = PreviewStatus::Applying;
        Ok(stored.clone())
    }

    fn abort(&self, challenge_id: Uuid) {
        let mut inner = self.inner.lock().expect("selection preview store poisoned");
        if let Some(stored) = inner.previews.get_mut(&challenge_id) {
            if stored.status == PreviewStatus::Applying {
                stored.status = PreviewStatus::Pending;
            }
        }
    }

    fn complete(&self, challenge_id: Uuid, receipt: CandidateSelectionReceiptV1) {
        let mut inner = self.inner.lock().expect("selection preview store poisoned");
        if let Some(stored) = inner.previews.get_mut(&challenge_id) {
            stored.status = PreviewStatus::Consumed;
        }
        inner
            .applied
            .insert(receipt.selection_operation_id, receipt);
    }

    fn mint_secret(&self) -> ChallengeSecret {
        let mut bytes = [0u8; SELECTION_SECRET_BYTES];
        self.rng
            .fill(&mut bytes)
            .expect("system CSPRNG must mint selection challenge");
        ChallengeSecret::from_minted(hex_encode(&bytes))
    }

    fn product_id_for(&self, selection_operation_id: Uuid) -> Option<String> {
        let inner = self.inner.lock().expect("selection preview store poisoned");
        if let Some(receipt) = inner.applied.get(&selection_operation_id) {
            return Some(receipt.product_id.clone());
        }
        inner
            .previews
            .values()
            .find(|stored| stored.selection.selection_operation_id == selection_operation_id)
            .map(|stored| stored.selection.product_id.clone())
    }
}

#[derive(Debug, Clone)]
struct PreviewDraft {
    selection_operation_id: Uuid,
    selection_digest: String,
    display_digest: String,
    candidate_set_id: Uuid,
    candidate_set_generation: u64,
    candidate_set_digest: String,
    product_id: String,
    host_session_id: String,
    actor_id: String,
    governing_spec_binding_digest: String,
    accepted_model_base_position: u64,
    accepted_model_base_digest: String,
    policy_revision: String,
    promote_effects: Vec<CandidatePromotionEffectV1>,
    reject_effects: Vec<CandidateRejectionEffectV1>,
    pending_candidate_count: usize,
    resulting_signoff: CandidateSelectionSignoffMode,
}

impl PreviewDraft {
    fn into_preview(
        self,
        confirmation_challenge: CandidateSelectionConfirmationV1,
    ) -> CandidateSelectionPreviewV1 {
        CandidateSelectionPreviewV1 {
            contract_version: CANDIDATE_SELECTION_CONTRACT_VERSION,
            selection_operation_id: self.selection_operation_id,
            selection_digest: self.selection_digest,
            display_digest: self.display_digest,
            candidate_set_id: self.candidate_set_id,
            candidate_set_generation: self.candidate_set_generation,
            candidate_set_digest: self.candidate_set_digest,
            product_id: self.product_id,
            host_session_id: self.host_session_id,
            actor_id: self.actor_id,
            governing_spec_binding_digest: self.governing_spec_binding_digest,
            accepted_model_base_position: self.accepted_model_base_position,
            accepted_model_base_digest: self.accepted_model_base_digest,
            policy_revision: self.policy_revision,
            promote_effects: self.promote_effects,
            reject_effects: self.reject_effects,
            pending_candidate_count: self.pending_candidate_count,
            resulting_signoff: self.resulting_signoff,
            confirmation_challenge,
        }
    }
}

#[derive(Debug, Clone)]
struct ValidatedSelection {
    candidates: BTreeMap<Uuid, SessionCandidateV1>,
    promote_ids: Vec<Uuid>,
    reject_ids: Vec<Uuid>,
    resulting_signoff: CandidateSelectionSignoffMode,
}

/// Production CandidateSet selection service.
pub struct CandidateSelectionService {
    sessions: Arc<CandidateSessionStore>,
    authority: Arc<dyn CandidateSelectionAuthorityProvider>,
    event_store: Arc<dyn AtomicEventStoreAdapter>,
    previews: CandidateSelectionPreviewStore,
}

impl CandidateSelectionService {
    pub fn new(
        sessions: Arc<CandidateSessionStore>,
        authority: Arc<dyn CandidateSelectionAuthorityProvider>,
        event_store: Arc<dyn AtomicEventStoreAdapter>,
    ) -> Self {
        Self {
            sessions,
            authority,
            event_store,
            previews: CandidateSelectionPreviewStore::new(),
        }
    }

    /// Validate a stable-id subset and mint an exact, no-write preview.
    pub fn prepare(
        &self,
        selection: CandidateSelectionV1,
        now: DateTime<Utc>,
    ) -> CandidateSelectionResult<CandidateSelectionPreviewV1> {
        validate_selection_shape(&selection)?;
        let authority = self
            .authority
            .current_authority(&selection.product_id, &selection.actor_id)?;
        let set = self
            .sessions
            .enumerate(&selection_access(&selection), now)
            .map_err(map_session_failure)?;
        let validated = validate_current_selection(&selection, &set, &authority)?;
        let draft = build_preview_draft(&selection, &set, &authority, &validated)?;
        Ok(self.previews.issue(selection, draft, now))
    }

    /// Consume explicit human confirmation and atomically apply the exact
    /// previewed selection against the service's bound event store.
    pub fn commit(
        &self,
        commit: CandidateSelectionCommitV1,
        now: DateTime<Utc>,
    ) -> CandidateSelectionResult<CandidateSelectionReceiptV1> {
        self.commit_with_store(commit, now, self.event_store.as_ref())
    }

    /// Commit against a product-routed atomic event store.
    ///
    /// Operator-scoped daemons resolve the Product ledger per request. Prepare
    /// still uses the long-lived preview/challenge store on `self`; only the
    /// canonical append/replay boundary uses `event_store`.
    pub fn commit_with_store(
        &self,
        commit: CandidateSelectionCommitV1,
        now: DateTime<Utc>,
        event_store: &dyn AtomicEventStoreAdapter,
    ) -> CandidateSelectionResult<CandidateSelectionReceiptV1> {
        if commit.contract_version != CANDIDATE_SELECTION_CONTRACT_VERSION {
            return Err(CandidateSelectionFailureV1::new(
                CandidateSelectionFailureCode::UnsupportedContractVersion,
                "unsupported candidate selection contract version",
            ));
        }
        if !commit.human_confirmed {
            return Err(CandidateSelectionFailureV1::new(
                CandidateSelectionFailureCode::HumanConfirmationRequired,
                "an explicit human confirmation is required",
            ));
        }
        if let Some(receipt) = self
            .previews
            .applied(commit.selection_operation_id, &commit.selection_digest)
        {
            return Ok(receipt);
        }

        let stored = self.previews.begin(&commit, now)?;
        let challenge_id = commit.confirmation_token.challenge_id;
        match self.commit_stored(&stored, now, event_store) {
            Ok(receipt) => {
                self.previews.complete(challenge_id, receipt.clone());
                Ok(receipt)
            }
            Err(error) => {
                self.previews.abort(challenge_id);
                Err(error)
            }
        }
    }

    fn commit_stored(
        &self,
        stored: &StoredPreview,
        now: DateTime<Utc>,
        event_store: &dyn AtomicEventStoreAdapter,
    ) -> CandidateSelectionResult<CandidateSelectionReceiptV1> {
        let selection = &stored.selection;
        let preview = &stored.preview;
        let authority = self
            .authority
            .current_authority(&selection.product_id, &selection.actor_id)?;
        let set = self
            .sessions
            .enumerate(&selection_access(selection), now)
            .map_err(map_session_failure)?;
        let validated = validate_current_selection(selection, &set, &authority)?;
        let current_draft = build_preview_draft(selection, &set, &authority, &validated)?;

        if current_draft.selection_digest != preview.selection_digest
            || current_draft.display_digest != preview.display_digest
            || current_draft.policy_revision != preview.policy_revision
            || current_draft.resulting_signoff != preview.resulting_signoff
        {
            return Err(CandidateSelectionFailureV1::new(
                CandidateSelectionFailureCode::PreviewStale,
                "selection effects or current authority changed after preview",
            ));
        }

        let expected_entries = build_promotion_entries(selection, preview, &validated, now)?;
        let expected_event_ids: Vec<Uuid> = expected_entries.iter().map(|entry| entry.id).collect();
        let existing_ids = existing_operation_event_ids(
            event_store,
            selection.selection_operation_id,
            &preview.selection_digest,
        )?;

        if !existing_ids.is_empty() && existing_ids.len() != expected_event_ids.len() {
            return Err(CandidateSelectionFailureV1::new(
                CandidateSelectionFailureCode::PartialCanonicalStateDetected,
                "only part of the deterministic promotion batch exists",
            ));
        }

        if existing_ids.is_empty() && !expected_entries.is_empty() {
            let batch_id = deterministic_batch_id(selection.selection_operation_id);
            let batch =
                AtomicEventBatchV1::new(batch_id, expected_entries.clone()).map_err(|e| {
                    CandidateSelectionFailureV1::new(
                        CandidateSelectionFailureCode::AtomicAppendFailed,
                        e.to_string(),
                    )
                })?;
            event_store.append_atomic_batch(batch).map_err(|e| {
                CandidateSelectionFailureV1::new(
                    CandidateSelectionFailureCode::AtomicAppendFailed,
                    e.to_string(),
                )
            })?;
        }

        let replayed_ids = existing_operation_event_ids(
            event_store,
            selection.selection_operation_id,
            &preview.selection_digest,
        )?;
        if replayed_ids.len() != expected_event_ids.len()
            || !expected_event_ids
                .iter()
                .all(|id| replayed_ids.contains(id))
        {
            return Err(CandidateSelectionFailureV1::new(
                CandidateSelectionFailureCode::ReplayVerificationFailed,
                "replay did not contain the complete deterministic promotion batch",
            ));
        }

        let reconciled = self
            .sessions
            .apply_confirmed_selection(
                &selection_access(selection),
                &validated.promote_ids,
                &validated.reject_ids,
                now,
            )
            .map_err(|e| {
                CandidateSelectionFailureV1::new(
                    CandidateSelectionFailureCode::SessionReconciliationFailed,
                    e.to_string(),
                )
            })?;

        Ok(CandidateSelectionReceiptV1 {
            contract_version: CANDIDATE_SELECTION_CONTRACT_VERSION,
            selection_operation_id: selection.selection_operation_id,
            selection_digest: preview.selection_digest.clone(),
            product_id: selection.product_id.clone(),
            actor_id: selection.actor_id.clone(),
            promoted_candidate_ids: validated.promote_ids.clone(),
            rejected_candidate_ids: validated.reject_ids.clone(),
            decision_ids: preview
                .promote_effects
                .iter()
                .map(|effect| effect.decision_id)
                .collect(),
            canonical_event_ids: expected_event_ids,
            resulting_signoff: preview.resulting_signoff,
            pending_candidate_count: reconciled.count_with(CandidateDisposition::Pending),
            replay_verified: true,
            session_reconciled: true,
        })
    }

    /// Shared session store for daemon-local materialization and tests.
    pub fn sessions(&self) -> &Arc<CandidateSessionStore> {
        &self.sessions
    }

    /// Product id bound to a pending or applied selection preview, if any.
    ///
    /// Used by the daemon to reproject authority on commit when
    /// `ToolRequest.product_id` is absent.
    pub fn preview_product_id(&self, selection_operation_id: Uuid) -> Option<String> {
        self.previews.product_id_for(selection_operation_id)
    }
}

fn validate_selection_shape(selection: &CandidateSelectionV1) -> CandidateSelectionResult<()> {
    if selection.contract_version != CANDIDATE_SELECTION_CONTRACT_VERSION {
        return Err(CandidateSelectionFailureV1::new(
            CandidateSelectionFailureCode::UnsupportedContractVersion,
            "unsupported candidate selection contract version",
        ));
    }
    if selection.product_id.trim().is_empty()
        || selection.host_session_id.trim().is_empty()
        || selection.session_lease_id.trim().is_empty()
        || selection.actor_id.trim().is_empty()
        || selection.candidate_set_digest.trim().is_empty()
    {
        return Err(CandidateSelectionFailureV1::new(
            CandidateSelectionFailureCode::MalformedIdentity,
            "selection contains an empty identity field",
        ));
    }
    if selection.promote_candidate_ids.is_empty() && selection.reject_candidate_ids.is_empty() {
        return Err(CandidateSelectionFailureV1::new(
            CandidateSelectionFailureCode::EmptySelection,
            "selection contains no promote or reject candidates",
        ));
    }

    let promote: BTreeSet<Uuid> = selection.promote_candidate_ids.iter().copied().collect();
    let reject: BTreeSet<Uuid> = selection.reject_candidate_ids.iter().copied().collect();
    if promote.len() != selection.promote_candidate_ids.len()
        || reject.len() != selection.reject_candidate_ids.len()
    {
        return Err(CandidateSelectionFailureV1::new(
            CandidateSelectionFailureCode::MalformedIdentity,
            "selection contains duplicate candidate ids",
        ));
    }
    let overlap: Vec<Uuid> = promote.intersection(&reject).copied().collect();
    if !overlap.is_empty() {
        return Err(CandidateSelectionFailureV1::with_candidates(
            CandidateSelectionFailureCode::OverlappingDispositions,
            "promote and reject subsets must be disjoint",
            overlap,
        ));
    }
    Ok(())
}

fn validate_current_selection(
    selection: &CandidateSelectionV1,
    set: &CandidateSetV1,
    authority: &CandidateSelectionAuthorityV1,
) -> CandidateSelectionResult<ValidatedSelection> {
    if set.candidate_set_id != selection.candidate_set_id
        || set.binding.product_id != selection.product_id
        || set.binding.host_session_id != selection.host_session_id
    {
        return Err(CandidateSelectionFailureV1::new(
            CandidateSelectionFailureCode::CandidateSetIdentityMismatch,
            "selection does not address the exact current CandidateSet",
        ));
    }
    if set.candidate_set_generation != selection.candidate_set_generation {
        return Err(CandidateSelectionFailureV1::new(
            CandidateSelectionFailureCode::CandidateSetStale,
            "candidate set generation changed",
        ));
    }
    if set.candidate_set_digest != selection.candidate_set_digest {
        return Err(CandidateSelectionFailureV1::new(
            CandidateSelectionFailureCode::CandidateSetDigestMismatch,
            "candidate set digest changed",
        ));
    }
    if authority.product_id != selection.product_id || authority.actor_id != selection.actor_id {
        return Err(CandidateSelectionFailureV1::new(
            CandidateSelectionFailureCode::ActorUnauthorized,
            "authority snapshot does not match the selected Product and actor",
        ));
    }
    if !authority.may_promote {
        return Err(CandidateSelectionFailureV1::new(
            CandidateSelectionFailureCode::ActorUnauthorized,
            "actor is not currently authorized to promote candidates",
        ));
    }
    if authority.promotion_policy.enabled && !authority.is_current_product_owner {
        return Err(CandidateSelectionFailureV1::new(
            CandidateSelectionFailureCode::CurrentProductOwnerRequired,
            "current Product Owner authority is required by auto-approve policy",
        ));
    }
    if set.binding.governing_spec_binding_digest != authority.governing_spec_binding_digest {
        return Err(CandidateSelectionFailureV1::new(
            CandidateSelectionFailureCode::GoverningSpecChanged,
            "governing specification binding changed",
        ));
    }
    if set.binding.accepted_model_base_position != authority.accepted_model_base_position
        || set.binding.accepted_model_base_digest != authority.accepted_model_base_digest
    {
        return Err(CandidateSelectionFailureV1::new(
            CandidateSelectionFailureCode::AcceptedModelBaseChanged,
            "accepted Product model base changed",
        ));
    }

    let candidates: BTreeMap<Uuid, SessionCandidateV1> = set
        .candidates
        .iter()
        .cloned()
        .map(|candidate| (candidate.candidate_id, candidate))
        .collect();
    let selected_ids: BTreeSet<Uuid> = selection
        .promote_candidate_ids
        .iter()
        .chain(selection.reject_candidate_ids.iter())
        .copied()
        .collect();
    let missing: Vec<Uuid> = selected_ids
        .iter()
        .filter(|id| !candidates.contains_key(id))
        .copied()
        .collect();
    if !missing.is_empty() {
        return Err(CandidateSelectionFailureV1::with_candidates(
            CandidateSelectionFailureCode::CandidateNotFound,
            "selected candidate does not belong to the current set",
            missing,
        ));
    }

    let not_pending: Vec<Uuid> = selected_ids
        .iter()
        .filter(|id| {
            candidates
                .get(id)
                .is_some_and(|candidate| candidate.disposition != CandidateDisposition::Pending)
        })
        .copied()
        .collect();
    if !not_pending.is_empty() {
        return Err(CandidateSelectionFailureV1::with_candidates(
            CandidateSelectionFailureCode::CandidateNotPending,
            "selected candidate already has a terminal session disposition",
            not_pending,
        ));
    }

    let promote_set: BTreeSet<Uuid> = selection.promote_candidate_ids.iter().copied().collect();
    let mut missing_dependencies = BTreeSet::new();
    for candidate_id in &promote_set {
        let candidate = candidates
            .get(candidate_id)
            .expect("selected candidate existence checked");
        for dependency_id in &candidate.dependency_candidate_ids {
            let dependency_already_promoted = candidates
                .get(dependency_id)
                .is_some_and(|dependency| dependency.disposition == CandidateDisposition::Promoted);
            if !promote_set.contains(dependency_id) && !dependency_already_promoted {
                missing_dependencies.insert(*dependency_id);
            }
        }
    }
    if !missing_dependencies.is_empty() {
        return Err(CandidateSelectionFailureV1::with_candidates(
            CandidateSelectionFailureCode::DependencyClosureRequired,
            "promoted subset is not dependency-closed",
            missing_dependencies.into_iter().collect(),
        ));
    }

    let mut promote_ids: Vec<Uuid> = promote_set.into_iter().collect();
    let mut reject_ids: Vec<Uuid> = selection.reject_candidate_ids.clone();
    promote_ids.sort_unstable();
    reject_ids.sort_unstable();

    Ok(ValidatedSelection {
        candidates,
        promote_ids,
        reject_ids,
        resulting_signoff: if authority.promotion_policy.enabled {
            CandidateSelectionSignoffMode::ApprovedByOwnerPolicy
        } else {
            CandidateSelectionSignoffMode::Proposed
        },
    })
}

fn build_preview_draft(
    selection: &CandidateSelectionV1,
    set: &CandidateSetV1,
    authority: &CandidateSelectionAuthorityV1,
    validated: &ValidatedSelection,
) -> CandidateSelectionResult<PreviewDraft> {
    let promote_effects: Vec<CandidatePromotionEffectV1> = validated
        .promote_ids
        .iter()
        .map(|candidate_id| {
            let candidate = validated
                .candidates
                .get(candidate_id)
                .expect("validated candidate exists");
            CandidatePromotionEffectV1 {
                candidate_id: *candidate_id,
                decision_id: deterministic_decision_id(
                    &selection.product_id,
                    selection.selection_operation_id,
                    *candidate_id,
                ),
                proposed_decision: candidate.proposed_decision.clone(),
                proposal_digest: candidate.proposal_digest.clone(),
                resulting_signoff: validated.resulting_signoff,
            }
        })
        .collect();
    let reject_effects: Vec<CandidateRejectionEffectV1> = validated
        .reject_ids
        .iter()
        .map(|candidate_id| CandidateRejectionEffectV1 {
            candidate_id: *candidate_id,
            resulting_disposition: "rejected_session_only".to_string(),
        })
        .collect();
    let pending_candidate_count = set
        .count_with(CandidateDisposition::Pending)
        .saturating_sub(promote_effects.len() + reject_effects.len());

    #[derive(Serialize)]
    struct DigestMaterial<'a> {
        contract_version: u32,
        selection_operation_id: Uuid,
        candidate_set_id: Uuid,
        candidate_set_generation: u64,
        candidate_set_digest: &'a str,
        product_id: &'a str,
        host_session_id: &'a str,
        actor_id: &'a str,
        governing_spec_binding_digest: &'a str,
        accepted_model_base_position: u64,
        accepted_model_base_digest: &'a str,
        policy_revision: &'a str,
        promote_effects: &'a [CandidatePromotionEffectV1],
        reject_effects: &'a [CandidateRejectionEffectV1],
        pending_candidate_count: usize,
        resulting_signoff: CandidateSelectionSignoffMode,
    }

    let material = DigestMaterial {
        contract_version: CANDIDATE_SELECTION_CONTRACT_VERSION,
        selection_operation_id: selection.selection_operation_id,
        candidate_set_id: set.candidate_set_id,
        candidate_set_generation: set.candidate_set_generation,
        candidate_set_digest: &set.candidate_set_digest,
        product_id: &selection.product_id,
        host_session_id: &selection.host_session_id,
        actor_id: &selection.actor_id,
        governing_spec_binding_digest: &set.binding.governing_spec_binding_digest,
        accepted_model_base_position: set.binding.accepted_model_base_position,
        accepted_model_base_digest: &set.binding.accepted_model_base_digest,
        policy_revision: &authority.promotion_policy.policy_revision,
        promote_effects: &promote_effects,
        reject_effects: &reject_effects,
        pending_candidate_count,
        resulting_signoff: validated.resulting_signoff,
    };
    let encoded = serde_json::to_vec(&material).map_err(|e| {
        CandidateSelectionFailureV1::new(
            CandidateSelectionFailureCode::PreviewStale,
            format!("failed to encode selection preview: {e}"),
        )
    })?;
    let selection_digest = sha256_digest(&encoded);
    let display_digest = sha256_digest(
        serde_json::to_string(&json!({
            "promote": promote_effects,
            "reject": reject_effects,
            "pending": pending_candidate_count,
            "signoff": validated.resulting_signoff,
        }))
        .map_err(|e| {
            CandidateSelectionFailureV1::new(
                CandidateSelectionFailureCode::PreviewStale,
                format!("failed to encode display preview: {e}"),
            )
        })?
        .as_bytes(),
    );

    Ok(PreviewDraft {
        selection_operation_id: selection.selection_operation_id,
        selection_digest,
        display_digest,
        candidate_set_id: set.candidate_set_id,
        candidate_set_generation: set.candidate_set_generation,
        candidate_set_digest: set.candidate_set_digest.clone(),
        product_id: selection.product_id.clone(),
        host_session_id: selection.host_session_id.clone(),
        actor_id: selection.actor_id.clone(),
        governing_spec_binding_digest: set.binding.governing_spec_binding_digest.clone(),
        accepted_model_base_position: set.binding.accepted_model_base_position,
        accepted_model_base_digest: set.binding.accepted_model_base_digest.clone(),
        policy_revision: authority.promotion_policy.policy_revision.clone(),
        promote_effects,
        reject_effects,
        pending_candidate_count,
        resulting_signoff: validated.resulting_signoff,
    })
}

fn build_promotion_entries(
    selection: &CandidateSelectionV1,
    preview: &CandidateSelectionPreviewV1,
    validated: &ValidatedSelection,
    now: DateTime<Utc>,
) -> CandidateSelectionResult<Vec<EventStoreEntry>> {
    let mut entries = Vec::with_capacity(preview.promote_effects.len());
    for effect in &preview.promote_effects {
        let source = validated
            .candidates
            .get(&effect.candidate_id)
            .expect("preview effect candidate was validated");
        let candidate = decision_candidate_from_session(source, selection, now);
        let mut audit_metadata = Map::new();
        audit_metadata.insert(
            "selection_operation_id".to_string(),
            Value::String(selection.selection_operation_id.to_string()),
        );
        audit_metadata.insert(
            "selection_digest".to_string(),
            Value::String(preview.selection_digest.clone()),
        );
        audit_metadata.insert(
            "product_id".to_string(),
            Value::String(selection.product_id.clone()),
        );
        audit_metadata.insert(
            "candidate_set_id".to_string(),
            Value::String(selection.candidate_set_id.to_string()),
        );
        audit_metadata.insert(
            "policy_revision".to_string(),
            Value::String(preview.policy_revision.clone()),
        );
        audit_metadata.insert(
            "rejected_candidate_ids".to_string(),
            Value::Array(
                validated
                    .reject_ids
                    .iter()
                    .map(|id| Value::String(id.to_string()))
                    .collect(),
            ),
        );
        let initial_signoff = match preview.resulting_signoff {
            CandidateSelectionSignoffMode::Proposed => None,
            CandidateSelectionSignoffMode::ApprovedByOwnerPolicy => Some(SignoffState::Approved),
        };
        let payload = CandidatePromotedPayload::new_with_initial_signoff(
            candidate,
            effect.decision_id,
            Some("Managed MCP explicit confirmed selection".to_string()),
            selection.actor_id.clone(),
            initial_signoff,
            audit_metadata,
        );
        let payload = serde_json::to_value(payload).map_err(|e| {
            CandidateSelectionFailureV1::new(
                CandidateSelectionFailureCode::AtomicAppendFailed,
                format!("failed to serialize promotion event: {e}"),
            )
        })?;
        entries.push(EventStoreEntry {
            id: deterministic_event_id(selection.selection_operation_id, effect.candidate_id),
            event_type: EventType::AcceptCandidate,
            payload,
            timestamp: now,
            actor: selection.actor_id.clone(),
            governance_result_id: None,
        });
    }
    Ok(entries)
}

fn decision_candidate_from_session(
    source: &SessionCandidateV1,
    selection: &CandidateSelectionV1,
    now: DateTime<Utc>,
) -> DecisionCandidate {
    let mut candidate = DecisionCandidate::new(
        source.proposed_decision.clone(),
        source.proposed_decision.clone(),
        DecisionLevel::Implementation,
        format!(
            "managed-mcp://{}/{}/candidate/{}",
            selection.product_id, selection.host_session_id, source.candidate_id
        ),
    );
    candidate.id = source.candidate_id;
    candidate.extraction_confidence = ExtractionConfidence::High;
    candidate.extracted_at = now;
    candidate.extracted_by = "managed_mcp_candidate_selection".to_string();
    candidate.candidate_state = CandidateState::Proposed;
    candidate.review_state = ReviewLifecycleState::Proposed;
    candidate.metadata = json!({
        "claim_trace_digest": source.claim_trace_digest,
        "proposal_digest": source.proposal_digest,
        "source_spans": source.source_spans,
        "contradiction_refs": source.contradiction_refs,
        "dependency_candidate_ids": source.dependency_candidate_ids,
        "candidate_set_id": selection.candidate_set_id,
        "candidate_set_generation": selection.candidate_set_generation,
    });
    candidate
}

fn existing_operation_event_ids(
    adapter: &dyn AtomicEventStoreAdapter,
    operation_id: Uuid,
    selection_digest: &str,
) -> CandidateSelectionResult<BTreeSet<Uuid>> {
    let entries = adapter.replay().map_err(|e| {
        CandidateSelectionFailureV1::new(
            CandidateSelectionFailureCode::ReplayVerificationFailed,
            e.to_string(),
        )
    })?;
    Ok(entries
        .into_iter()
        .filter(|entry| {
            entry.event_type == EventType::AcceptCandidate
                && entry
                    .payload
                    .get("audit_metadata")
                    .and_then(|metadata| metadata.get("selection_operation_id"))
                    .and_then(Value::as_str)
                    == Some(operation_id.to_string().as_str())
                && entry
                    .payload
                    .get("audit_metadata")
                    .and_then(|metadata| metadata.get("selection_digest"))
                    .and_then(Value::as_str)
                    == Some(selection_digest)
        })
        .map(|entry| entry.id)
        .collect())
}

fn selection_access(selection: &CandidateSelectionV1) -> CandidateSessionAccessV1 {
    CandidateSessionAccessV1::new(
        selection.product_id.clone(),
        selection.host_session_id.clone(),
        selection.session_lease_id.clone(),
        selection.candidate_set_generation,
    )
}

fn deterministic_decision_id(product_id: &str, operation_id: Uuid, candidate_id: Uuid) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("{product_id}|{operation_id}|{candidate_id}").as_bytes(),
    )
}

fn deterministic_event_id(operation_id: Uuid, candidate_id: Uuid) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("candidate-selection-event|{operation_id}|{candidate_id}").as_bytes(),
    )
}

fn deterministic_batch_id(operation_id: Uuid) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("candidate-selection-batch|{operation_id}").as_bytes(),
    )
}

fn sha256_digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn map_session_failure(error: CandidateSessionFailure) -> CandidateSelectionFailureV1 {
    let code = match error {
        CandidateSessionFailure::UnsupportedContractVersion => {
            CandidateSelectionFailureCode::UnsupportedContractVersion
        }
        CandidateSessionFailure::MalformedIdentity => {
            CandidateSelectionFailureCode::MalformedIdentity
        }
        CandidateSessionFailure::CandidateSetNotFound
        | CandidateSessionFailure::SessionLeaseExpired => {
            CandidateSelectionFailureCode::CandidateSetNotFound
        }
        CandidateSessionFailure::CandidateSetStale => {
            CandidateSelectionFailureCode::CandidateSetStale
        }
        CandidateSessionFailure::CandidateNotInSet => {
            CandidateSelectionFailureCode::CandidateNotFound
        }
    };
    CandidateSelectionFailureV1::new(code, error.to_string())
}

fn confirmation_failure(outcome: ConfirmationOutcome) -> CandidateSelectionFailureV1 {
    CandidateSelectionFailureV1::new(
        CandidateSelectionFailureCode::ConfirmationRejected,
        outcome.code(),
    )
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
