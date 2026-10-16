//! RecallPacket — alpha decision recall and candidate promotion protocol.
//!
//! `RecallPacket` is a read-only lookup artifact exposing canonical decisions,
//! decision candidates, unknown scope areas, and allowed next actions for the
//! current recall context.
//!
//! Alpha supported actions:
//! - `inspect_evidence`: read-only drill-in to cited evidence refs
//! - `expand_scope`: explicit scope expansion returning a new RecallPacket
//! - `promote_decision_candidate`: governed candidate promotion with authority proof
//!
//! RecallPacket actions may route, inspect, request, and explain; they must not
//! accept, materialize, sign off, bind, enforce, or mutate canonical state
//! except through the governed candidate promotion/signoff path.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::candidate::{
    CandidateState, DecisionCandidate, DecisionLevel, ExtractionConfidence, ReviewLifecycleState,
};
use crate::dashboard::SignoffState;
use crate::evidence::CodeSourceCurrentness;

// ---------------------------------------------------------------------------
// RecallPacket
// ---------------------------------------------------------------------------

/// A read-only lookup artifact for decision recall.
///
/// Exposes canonical decisions, decision candidates, unknown scope areas,
/// and allowed next actions. Does not carry governance authority — it is a
/// projection surface for human review and action selection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallPacket {
    /// Unique identifier for this packet instance.
    pub packet_id: Uuid,

    /// Canonical decisions relevant to the current recall context.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub canonical_decisions: Vec<CanonicalDecisionEntry>,

    /// Decision candidates surfaced for review.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decision_candidates: Vec<SurfacedDecisionCandidate>,

    /// Typed, daemon-authored decision-attention signal (issue #827).
    ///
    /// Tells a consumer whether explicit decision review should be requested
    /// for this packet, without the consumer inferring the signal from packet
    /// contents. Always present so absence of attention is an explicit
    /// `NotWarranted` state rather than a missing field. Carries a typed reason
    /// only — never UI prose, compliance, safety, or canonical lifecycle state.
    #[serde(default)]
    pub decision_attention: DecisionAttention,

    /// Scope areas where the system has no canonical coverage.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unknown_scope: Vec<UnknownScopeEntry>,

    /// Actions available to the caller for this packet.
    pub allowed_next_actions: Vec<RecallAction>,

    /// Searched scope — the exact bounded set actually searched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub searched_scope: Option<SearchedScope>,

    /// Recall trace reference for reproducibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recall_trace: Option<RecallTrace>,

    /// Daemon-authored commit-driven freshness for code evidence in this
    /// packet (issue #625). Evidence-only read-model: carries freshness
    /// labels, caveats, and next actions for surfaces to render. It never
    /// mutates accepted evidence, signoff, compliance, no-conflict, or
    /// merge-safety.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness: Option<crate::freshness::PreflightFreshness>,

    /// Daemon-authored semantic recall contribution for this packet (issue
    /// #640). Evidence-only read-model: renders graph-verified semantic
    /// targets and approximate hints (kept distinct) plus caveats/next
    /// actions. Never mutates canonical decisions, candidates, signoff,
    /// compliance, no-conflict, or merge-safety. Absent (`None`) when no
    /// semantic provider is configured, so the no-op stays quiet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_recall: Option<crate::semantic_recall::SemanticRecallView>,
}

impl RecallPacket {
    /// Create a new empty RecallPacket with alpha-default allowed actions.
    pub fn new_alpha() -> Self {
        Self {
            packet_id: Uuid::new_v4(),
            canonical_decisions: Vec::new(),
            decision_candidates: Vec::new(),
            decision_attention: DecisionAttention::default(),
            unknown_scope: Vec::new(),
            allowed_next_actions: RecallAction::alpha_actions(),
            searched_scope: None,
            recall_trace: None,
            freshness: None,
            semantic_recall: None,
        }
    }

    /// Attach a daemon-authored freshness read-model to this packet.
    ///
    /// Freshness is evidence-only: attaching it never changes the packet's
    /// canonical decisions, candidates, allowed actions, or any authority.
    pub fn with_freshness(mut self, freshness: crate::freshness::PreflightFreshness) -> Self {
        self.freshness = Some(freshness);
        self
    }

    /// Attach a daemon-authored semantic recall view to this packet.
    ///
    /// The view is evidence-only: attaching it never changes the packet's
    /// canonical decisions, candidates, allowed actions, or any authority.
    pub fn with_semantic_recall(
        mut self,
        semantic_recall: crate::semantic_recall::SemanticRecallView,
    ) -> Self {
        self.semantic_recall = Some(semantic_recall);
        self
    }

    /// Recompute the daemon-authored decision-attention signal from the
    /// packet's currently surfaced candidates (issue #827).
    ///
    /// The daemon owns this determination; consumers read the resulting typed
    /// signal rather than inferring it. Read-only: recomputing attention never
    /// mutates canonical decisions, candidates, or any authority.
    pub fn with_derived_decision_attention(mut self) -> Self {
        self.decision_attention =
            DecisionAttention::from_surfaced_candidates(&self.decision_candidates);
        self
    }

    /// Check whether a specific action is allowed in this packet.
    pub fn is_action_allowed(&self, action: RecallActionKind) -> bool {
        self.allowed_next_actions.iter().any(|a| a.kind == action)
    }
}

// ---------------------------------------------------------------------------
// Canonical Decision entry (read-only projection)
// ---------------------------------------------------------------------------

/// A canonical Decision entry within a RecallPacket.
///
/// Read-only projection — does not carry governance authority.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalDecisionEntry {
    /// Decision identifier.
    pub decision_id: Uuid,

    /// Decision title.
    pub title: String,

    /// Decision body/summary.
    pub summary: String,

    /// Decision level (product, architecture, implementation).
    pub level: DecisionLevel,

    /// Signoff state.
    pub signoff_state: SignoffState,

    /// Evidence references cited by this decision.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_refs: Vec<EvidenceRef>,

    /// Authority labels describing the decision's governance posture.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authority_labels: Vec<AuthorityLabel>,
}

// ---------------------------------------------------------------------------
// Surfaced DecisionCandidate entry
// ---------------------------------------------------------------------------

/// A DecisionCandidate surfaced within a RecallPacket for review.
///
/// Contains the minimum alpha fields required for informed action selection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurfacedDecisionCandidate {
    /// Candidate identifier.
    pub candidate_id: Uuid,

    /// Title.
    pub title: String,

    /// Summary description.
    pub summary: String,

    /// Source references and cited spans.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_refs: Vec<SourceRef>,

    /// Author/origin surface (e.g. MCP, Linear, meeting, PR, chat).
    pub origin_surface: String,

    /// Extraction confidence or signal metadata (non-authoritative).
    pub extraction_confidence: ExtractionConfidence,

    /// Ambiguities or open questions identified during extraction.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ambiguities: Vec<String>,

    /// Related canonical Decision references, if any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_decision_refs: Vec<Uuid>,

    /// Candidate relationship indicating the proposed disposition.
    pub candidate_relationship: CandidateRelationship,

    /// Typed reasons this candidate is relevant to the recall query (issue
    /// #827). Empty means no relevance rationale was recorded rather than
    /// implying irrelevance.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relevance_reasons: Vec<CandidateRelevance>,

    /// Freshness/readiness of the candidate's cited evidence (issue #827).
    /// Read-model only; never asserts compliance, signoff, or no-conflict.
    #[serde(default)]
    pub evidence_freshness: CandidateEvidenceFreshness,

    /// Exact promotion outcomes allowed for this candidate in the current
    /// actor/context (issue #827). Supersession and keep-both are present only
    /// when a related canonical Decision exists to act against.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_outcomes: Vec<PromotionOutcome>,

    /// Code evidence references when available.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub code_evidence_refs: Vec<CodeEvidenceRef>,

    /// Current review/signoff state.
    pub review_state: ReviewLifecycleState,

    /// Candidate lifecycle state.
    pub candidate_state: CandidateState,
}

impl SurfacedDecisionCandidate {
    /// Project a daemon-owned [`DecisionCandidate`] into a reviewable recall
    /// entry (issue #827).
    ///
    /// Read-only: reads replayed candidate state and returns a projection. It
    /// appends no events and mutates no Decision, candidate, signoff, binding,
    /// compliance, or lifecycle state.
    ///
    /// `relevance_reasons` are supplied by the producer that matched the
    /// candidate against the recall query. `related_decision_refs` gate which
    /// promotion outcomes are offered: supersession and keep-both require an
    /// existing Decision to act against.
    pub fn from_candidate(
        candidate: &DecisionCandidate,
        relevance_reasons: Vec<CandidateRelevance>,
        related_decision_refs: Vec<Uuid>,
    ) -> Self {
        let evidence_freshness =
            CandidateEvidenceFreshness::from_review_state(candidate.review_state);

        let mut ambiguities = Vec::new();
        if candidate.extraction_confidence == ExtractionConfidence::Low {
            ambiguities.push(
                "Low extraction confidence; proposed claim may be ambiguous and needs review."
                    .to_string(),
            );
        }
        if let Some(items) = candidate
            .metadata
            .get("ambiguities")
            .and_then(|v| v.as_array())
        {
            for item in items {
                if let Some(text) = item.as_str() {
                    if !text.trim().is_empty() {
                        ambiguities.push(text.to_string());
                    }
                }
            }
        }

        let mut allowed_outcomes = vec![
            PromotionOutcome::NewConstraint,
            PromotionOutcome::NeedsSignoffReview,
            PromotionOutcome::DismissedAsNoise,
        ];
        if !related_decision_refs.is_empty() {
            allowed_outcomes.push(PromotionOutcome::SupersedesExistingDecision);
            allowed_outcomes.push(PromotionOutcome::KeepsBothWithScope);
        }

        Self {
            candidate_id: candidate.id,
            title: candidate.title.clone(),
            summary: candidate.description.clone(),
            source_refs: vec![SourceRef {
                uri: candidate.source.clone(),
                kind: "unknown".into(),
                cited_span: None,
            }],
            origin_surface: candidate.extracted_by.clone(),
            extraction_confidence: candidate.extraction_confidence,
            ambiguities,
            related_decision_refs,
            candidate_relationship: CandidateRelationship::NeedsSignoffReview,
            relevance_reasons,
            evidence_freshness,
            allowed_outcomes,
            code_evidence_refs: Vec::new(),
            review_state: candidate.review_state,
            candidate_state: candidate.candidate_state,
        }
    }
}

/// Typed reason a surfaced candidate is relevant to a recall query (issue
/// #827). Daemon-authored; never inferred by a consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateRelevance {
    /// The query terms matched the candidate's source text.
    SourceTextMatch,
    /// The candidate was returned as part of an unfiltered ledger scan (no
    /// discriminating query terms were provided).
    FullLedgerScan,
    /// The candidate overlaps a related canonical Decision in scope.
    RelatedDecisionOverlap,
}

/// Freshness/readiness of a surfaced candidate's cited evidence (issue #827).
///
/// Read-model only: it reports what the daemon could observe about evidence
/// currency and never asserts compliance, signoff, no-conflict, or merge
/// safety.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CandidateEvidenceFreshness {
    /// Evidence is current as of the replayed state.
    #[default]
    Current,
    /// Evidence is pending a stale-source review and may be out of date.
    Stale,
    /// Evidence currency is unknown/unverified in this bounded scope.
    Unknown,
}

impl CandidateEvidenceFreshness {
    /// Derive candidate evidence freshness from the review lifecycle state.
    pub fn from_review_state(state: ReviewLifecycleState) -> Self {
        match state {
            ReviewLifecycleState::StaleSourcePending => Self::Stale,
            _ => Self::Current,
        }
    }
}

/// Typed, daemon-authored decision-attention signal (issue #827).
///
/// A consumer renders `Warranted` as a prompt to review the packet's proposed
/// decisions and `NotWarranted` as no attention signal. The consumer must not
/// infer this from packet contents. The signal carries a typed reason only; it
/// encodes no UI prose, compliance verdict, safety claim, or canonical
/// lifecycle mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionAttention {
    /// Whether explicit decision attention is warranted.
    pub state: DecisionAttentionState,

    /// Typed reason for the state.
    pub reason: DecisionAttentionReason,

    /// Short daemon-authored explanation for review context. Descriptive
    /// evidence only — not a UI string, compliance verdict, or safety claim.
    pub detail: String,
}

impl Default for DecisionAttention {
    fn default() -> Self {
        Self::not_warranted()
    }
}

impl DecisionAttention {
    /// No decision attention is warranted.
    pub fn not_warranted() -> Self {
        Self {
            state: DecisionAttentionState::NotWarranted,
            reason: DecisionAttentionReason::NoReviewableCandidates,
            detail: "No reviewable decision candidates were surfaced in the searched scope.".into(),
        }
    }

    /// Derive the signal from the candidates surfaced in a packet.
    ///
    /// The daemon owns this determination: attention is warranted when at least
    /// one reviewable candidate is present. Ambiguity in any surfaced candidate
    /// is reported as the more specific reason.
    pub fn from_surfaced_candidates(candidates: &[SurfacedDecisionCandidate]) -> Self {
        if candidates.is_empty() {
            return Self::not_warranted();
        }
        let ambiguous = candidates.iter().any(|c| !c.ambiguities.is_empty());
        let (reason, detail) = if ambiguous {
            (
                DecisionAttentionReason::CandidateAmbiguity,
                format!(
                    "{} reviewable candidate(s) surfaced; at least one carries unresolved \
                     ambiguity requiring explicit review.",
                    candidates.len()
                ),
            )
        } else {
            (
                DecisionAttentionReason::ReviewableCandidatesPresent,
                format!(
                    "{} reviewable decision candidate(s) surfaced for explicit review.",
                    candidates.len()
                ),
            )
        };
        Self {
            state: DecisionAttentionState::Warranted,
            reason,
            detail,
        }
    }
}

/// Whether explicit decision attention is warranted (issue #827).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionAttentionState {
    /// The consumer should request explicit decision review.
    Warranted,
    /// No attention signal — the consumer should not request review.
    NotWarranted,
}

/// Typed reason for a [`DecisionAttention`] state (issue #827).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionAttentionReason {
    /// One or more reviewable decision candidates were surfaced.
    ReviewableCandidatesPresent,
    /// A surfaced candidate carries unresolved ambiguity.
    CandidateAmbiguity,
    /// No reviewable decision candidates were surfaced.
    NoReviewableCandidates,
}

// ---------------------------------------------------------------------------
// Unknown scope
// ---------------------------------------------------------------------------

/// An area of unknown scope — no canonical coverage exists.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnknownScopeEntry {
    /// Scope identifier or descriptor.
    pub scope_id: String,

    /// Human-readable description of the unknown area.
    pub description: String,

    /// Suggested expansion targets for this scope.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expansion_targets: Vec<String>,
}

// ---------------------------------------------------------------------------
// Allowed actions
// ---------------------------------------------------------------------------

/// An allowed action within a RecallPacket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallAction {
    /// Action kind.
    pub kind: RecallActionKind,

    /// Human-readable description of what this action does.
    pub description: String,

    /// Whether this action requires authority proof.
    pub requires_authority: bool,
}

impl RecallAction {
    /// Alpha-supported actions.
    pub fn alpha_actions() -> Vec<Self> {
        vec![
            Self {
                kind: RecallActionKind::InspectEvidence,
                description: "Read-only drill-in to cited evidence refs, source spans, \
                              code evidence, freshness/readiness, and authority labels."
                    .into(),
                requires_authority: false,
            },
            Self {
                kind: RecallActionKind::ExpandScope,
                description: "Explicit user-selected read-only search over a named \
                              unknown/additional source scope. Returns a new or refreshed \
                              RecallPacket."
                    .into(),
                requires_authority: false,
            },
            Self {
                kind: RecallActionKind::PromoteDecisionCandidate,
                description: "Explicit governed request to promote, supersede, or route \
                              a DecisionCandidate for signoff. Requires a bot-known \
                              candidate or packet item ref."
                    .into(),
                requires_authority: true,
            },
            Self {
                kind: RecallActionKind::RequestCorrection,
                description: "Explicit user-approved request to start a governed \
                              correction/corpus-update path. References a prior \
                              RecallPacket or selected packet items. Requires approval \
                              proof and bot authorization."
                    .into(),
                requires_authority: true,
            },
        ]
    }
}

/// Kind of action available in a RecallPacket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallActionKind {
    /// Read-only evidence inspection.
    InspectEvidence,
    /// Read-only scope expansion.
    ExpandScope,
    /// Governed candidate promotion.
    PromoteDecisionCandidate,
    /// Explicit correction/corpus-update request referencing a prior RecallPacket.
    RequestCorrection,
}

// ---------------------------------------------------------------------------
// Action request payloads
// ---------------------------------------------------------------------------

/// Payload for `recall.inspect_evidence`.
///
/// Read-only drill-in to cited evidence. Preserves authority labels.
/// Does not mutate any state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallInspectEvidenceParams {
    /// RecallPacket reference.
    pub packet_id: Uuid,

    /// Target to inspect: a decision id or candidate id from the packet.
    pub target_id: Uuid,

    /// Optional specific evidence ref to drill into.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_ref_id: Option<Uuid>,
}

/// Payload for `recall.expand_scope`.
///
/// Explicit user-selected scope expansion. Returns read-only recall output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallExpandScopeParams {
    /// RecallPacket reference.
    pub packet_id: Uuid,

    /// The scope or expansion target to search.
    pub scope_target: String,

    /// Optional additional search hints.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<String>,
}

/// Payload for `recall.request_correction`.
///
/// Explicit, approval-gated request that starts a governed correction /
/// corpus-update path using existing Bicameral evidence mechanics.
/// References a prior RecallPacket or selected packet items.
///
/// Per ADR-0024 and the RecallPacket contract spec:
/// - Requires explicit approval proof (single-use).
/// - Cannot create accepted Decision, accepted BindingEvidence, signoff,
///   compliance, or enforcement state.
/// - Routes output as governed correction/corpus-update evidence or
///   candidate flow, not automatic Decision acceptance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallRequestCorrectionParams {
    /// RecallPacket reference — the correction must reference a prior packet.
    pub packet_id: Uuid,

    /// Selected item references from the packet (decision or candidate ids).
    /// At least one must be provided.
    pub selected_item_ids: Vec<Uuid>,

    /// What kind of correction is being requested.
    pub correction_kind: CorrectionKind,

    /// Human-readable description of the correction rationale.
    pub rationale: String,

    /// Approval proof — required. Single-use: consumed on first successful use.
    ///
    /// MCP/caller intent is not approval proof (per #501 adversarial review).
    pub approval_proof: ApprovalProof,
}

/// Kind of correction being requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionKind {
    /// A product feature definition is about to change.
    FeatureDefinitionChange,
    /// A source contradicts the trusted corpus.
    SourceContradiction,
    /// A relevant constraint is missing from the trusted corpus.
    MissingConstraint,
    /// Stale or ambiguous corpus text needs product-owner clarification.
    StaleOrAmbiguous,
}

/// Result of `recall.request_correction`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallRequestCorrectionResult {
    /// Packet id that was referenced.
    pub packet_id: Uuid,

    /// Outcome of the correction request.
    pub outcome: CorrectionRequestOutcome,

    /// When the correction request is accepted, a correction candidate id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correction_candidate_id: Option<Uuid>,

    /// Non-authoritative trace reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_ref: Option<String>,
}

/// Outcome of a correction request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionRequestOutcome {
    /// Correction request accepted — routed to governed correction path.
    Accepted,
    /// Correction request refused (see refusal details).
    Refused,
}

/// Payload for `recall.promote_decision_candidate`.
///
/// Governed request to promote a DecisionCandidate. References a bot-known
/// candidate; free-floating LLM text is refused.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallPromoteCandidateParams {
    /// RecallPacket reference — the candidate must appear in this packet.
    pub packet_id: Uuid,

    /// Candidate identifier — must be a bot-known candidate from the packet.
    pub candidate_id: Uuid,

    /// Intended promotion outcome.
    pub promotion_outcome: PromotionOutcome,

    /// When superseding, the existing Decision being superseded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supersedes_decision_id: Option<Uuid>,

    /// When keeping both, the explicit scoping relationship description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scoping_relationship: Option<String>,

    /// Approval proof — required when actor lacks intrinsic authority.
    ///
    /// Caller LLM text is advisory and cannot become approval proof.
    /// This must be a verifiable proof (e.g. signoff token, review approval ID).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_proof: Option<ApprovalProof>,

    /// Confirmation token for the daemon-issued `ConfirmationChallenge`
    /// protocol (issue #829).
    ///
    /// Absent on the initial governed promotion request: the daemon issues a
    /// bound challenge and returns [`PromotionRequestOutcome::ConfirmationRequired`]
    /// without any canonical write. Present on the confirmed resubmission: the
    /// daemon validates and single-use-consumes the challenge before applying
    /// the permitted transition. Reuse the initial request's `request_id` so a
    /// lost-response retry returns the already-applied canonical result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmation: Option<crate::confirmation_challenge::ConfirmationToken>,
}

/// Intended outcome of a candidate promotion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromotionOutcome {
    /// Promote as a new governing constraint.
    NewConstraint,
    /// Supersede an existing Decision with explicit link/provenance.
    SupersedesExistingDecision,
    /// Keep both Decisions with explicit scoping relationship.
    KeepsBothWithScope,
    /// Route to signoff review when actor lacks authority.
    NeedsSignoffReview,
    /// Dismiss as local/noise without canonical mutation.
    DismissedAsNoise,
}

/// Approval proof for authority-requiring actions.
///
/// Verifiable proof that the actor has authority to perform the action.
/// Caller LLM text is advisory only and cannot serve as approval proof.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalProof {
    /// Proof kind (e.g. `"signoff_token"`, `"review_approval_id"`).
    pub kind: String,

    /// Proof value — opaque token validated by the daemon.
    pub value: String,

    /// Actor who provided the proof.
    pub actor_id: String,

    /// Timestamp of proof provision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provided_at: Option<chrono::DateTime<chrono::Utc>>,
}

// ---------------------------------------------------------------------------
// Action results
// ---------------------------------------------------------------------------

/// Result of `recall.inspect_evidence`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallInspectEvidenceResult {
    /// Target that was inspected.
    pub target_id: Uuid,

    /// Evidence items found.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_items: Vec<InspectedEvidenceItem>,

    /// Authority labels for the inspected target.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authority_labels: Vec<AuthorityLabel>,

    /// Non-authoritative trace reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_ref: Option<String>,
}

/// A single evidence item returned by inspection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectedEvidenceItem {
    /// Evidence reference identifier.
    pub evidence_ref_id: Uuid,

    /// Source span excerpt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_span: Option<String>,

    /// Code evidence excerpt (when available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_evidence: Option<String>,

    /// Freshness/readiness status.
    pub freshness: EvidenceFreshness,

    /// Authority label for this evidence item.
    pub authority_label: AuthorityLabel,
}

/// Result of `recall.expand_scope`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallExpandScopeResult {
    /// The refreshed or new RecallPacket after scope expansion.
    pub packet: RecallPacket,

    /// Non-authoritative trace reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_ref: Option<String>,
}

/// Result of `recall.promote_decision_candidate`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallPromoteCandidateResult {
    /// Candidate identifier that was acted upon.
    pub candidate_id: Uuid,

    /// Outcome of the promotion request.
    pub outcome: PromotionRequestOutcome,

    /// When promotion succeeds, the new Decision identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision_id: Option<Uuid>,

    /// When routed to signoff, the review state transition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_state: Option<ReviewLifecycleState>,

    /// Set when `outcome` is [`PromotionRequestOutcome::ConfirmationRequired`]:
    /// the daemon-issued challenge to render and re-present. No canonical write
    /// occurred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmation_required: Option<crate::confirmation_challenge::ConfirmationRequiredResponse>,

    /// Set when a confirmed resubmission fails closed: the typed confirmation
    /// rejection. No canonical write occurred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmation_rejection: Option<crate::confirmation_challenge::ConfirmationRejection>,

    /// Non-authoritative trace reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_ref: Option<String>,
}

/// Outcome of a candidate promotion request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromotionRequestOutcome {
    /// Candidate was promoted to a canonical Decision.
    Promoted,
    /// Candidate was routed to signoff review (actor lacked authority).
    RoutedToSignoff,
    /// Candidate was dismissed as noise.
    Dismissed,
    /// A daemon-issued `ConfirmationChallenge` must be confirmed and resubmitted
    /// before any canonical write (issue #829). No canonical event was created.
    ConfirmationRequired,
    /// Promotion was refused (see refusal details).
    Refused,
}

// ---------------------------------------------------------------------------
// Typed refusals
// ---------------------------------------------------------------------------

/// Typed refusal for RecallPacket actions.
///
/// Each variant represents a specific reason why an action was refused.
/// MCP routing emits exactly one ToolRequest or a typed refusal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallRefusal {
    /// Refusal code.
    pub code: RecallRefusalCode,

    /// Human-readable detail.
    pub detail: String,

    /// Non-authoritative trace reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_ref: Option<String>,
}

/// Refusal codes for RecallPacket actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallRefusalCode {
    /// Action is not in the packet's `allowed_next_actions`.
    ActionNotAllowed,
    /// Packet reference is missing or invalid.
    MissingPacketRef,
    /// Candidate reference is missing or invalid.
    MissingCandidateRef,
    /// Packet is stale (superseded by a newer version).
    StalePacket,
    /// Required selected refs (packet/candidate) not provided.
    MissingSelectedRefs,
    /// Approval proof required but not provided or invalid.
    MissingApprovalProof,
    /// Approval proof has already been consumed (single-use).
    ApprovalAlreadyConsumed,
    /// Action is not supported in the current alpha version.
    UnsupportedAction,
    /// Daemon is unavailable.
    DaemonUnavailable,
    /// Scope expansion is required before this action can proceed.
    ScopeExpansionRequired,
    /// Free-floating claim refused — must reference a bot-known candidate.
    FreeFloatingClaimRefused,
    /// Local authority refused — cannot create authority locally.
    LocalAuthorityRefused,
}

impl RecallRefusal {
    pub fn action_not_allowed(detail: impl Into<String>) -> Self {
        Self {
            code: RecallRefusalCode::ActionNotAllowed,
            detail: detail.into(),
            trace_ref: None,
        }
    }

    pub fn missing_packet_ref(detail: impl Into<String>) -> Self {
        Self {
            code: RecallRefusalCode::MissingPacketRef,
            detail: detail.into(),
            trace_ref: None,
        }
    }

    pub fn missing_candidate_ref(detail: impl Into<String>) -> Self {
        Self {
            code: RecallRefusalCode::MissingCandidateRef,
            detail: detail.into(),
            trace_ref: None,
        }
    }

    pub fn stale_packet(detail: impl Into<String>) -> Self {
        Self {
            code: RecallRefusalCode::StalePacket,
            detail: detail.into(),
            trace_ref: None,
        }
    }

    pub fn missing_selected_refs(detail: impl Into<String>) -> Self {
        Self {
            code: RecallRefusalCode::MissingSelectedRefs,
            detail: detail.into(),
            trace_ref: None,
        }
    }

    pub fn missing_approval_proof(detail: impl Into<String>) -> Self {
        Self {
            code: RecallRefusalCode::MissingApprovalProof,
            detail: detail.into(),
            trace_ref: None,
        }
    }

    pub fn approval_already_consumed(detail: impl Into<String>) -> Self {
        Self {
            code: RecallRefusalCode::ApprovalAlreadyConsumed,
            detail: detail.into(),
            trace_ref: None,
        }
    }

    pub fn unsupported_action(detail: impl Into<String>) -> Self {
        Self {
            code: RecallRefusalCode::UnsupportedAction,
            detail: detail.into(),
            trace_ref: None,
        }
    }

    pub fn daemon_unavailable(detail: impl Into<String>) -> Self {
        Self {
            code: RecallRefusalCode::DaemonUnavailable,
            detail: detail.into(),
            trace_ref: None,
        }
    }

    pub fn scope_expansion_required(detail: impl Into<String>) -> Self {
        Self {
            code: RecallRefusalCode::ScopeExpansionRequired,
            detail: detail.into(),
            trace_ref: None,
        }
    }

    pub fn free_floating_claim_refused(detail: impl Into<String>) -> Self {
        Self {
            code: RecallRefusalCode::FreeFloatingClaimRefused,
            detail: detail.into(),
            trace_ref: None,
        }
    }

    pub fn local_authority_refused(detail: impl Into<String>) -> Self {
        Self {
            code: RecallRefusalCode::LocalAuthorityRefused,
            detail: detail.into(),
            trace_ref: None,
        }
    }

    /// Attach a trace reference.
    pub fn with_trace(mut self, trace_ref: impl Into<String>) -> Self {
        self.trace_ref = Some(trace_ref.into());
        self
    }
}

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

/// Evidence reference within a RecallPacket entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRef {
    /// Evidence reference identifier.
    pub id: Uuid,

    /// Brief label describing this evidence.
    pub label: String,

    /// Evidence kind (source span, code region, tool output, etc.).
    pub kind: EvidenceRefKind,
}

/// Kind of evidence reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceRefKind {
    /// Source span from an operational source.
    SourceSpan,
    /// Code region from the workspace.
    CodeRegion,
    /// Tool/command output.
    ToolOutput,
    /// External document or artifact.
    ExternalDocument,
}

/// Authority label describing governance posture of a decision or evidence item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorityLabel {
    /// Label kind.
    pub kind: AuthorityLabelKind,

    /// Human-readable description.
    pub description: String,
}

/// Kind of authority label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityLabelKind {
    /// Human-ratified authority.
    HumanRatified,
    /// Bot-proposed, awaiting human review.
    BotProposed,
    /// Advisory only — no governance authority.
    Advisory,
    /// Stale — authority may no longer be current.
    Stale,
}

/// Source reference for a candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRef {
    /// Source URI.
    pub uri: String,

    /// Source kind (Slack, Jira, GitHub, meeting, etc.).
    pub kind: String,

    /// Cited span excerpt (when available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cited_span: Option<String>,
}

/// Code evidence reference for a candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeEvidenceRef {
    /// File path.
    pub file: String,

    /// Line range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,

    /// Symbol name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,

    /// Whether this evidence is verified or approximate.
    pub verification_state: CodeEvidenceVerification,
}

/// Verification state of code evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeEvidenceVerification {
    /// Evidence verified against the current graph snapshot.
    Verified,
    /// Evidence is approximate (not graph-verified).
    Approximate,
}

/// Candidate relationship indicating proposed disposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateRelationship {
    /// New governing constraint.
    NewConstraint,
    /// Supersedes an existing Decision.
    SupersedesExistingDecision,
    /// Keeps both with explicit scope delineation.
    KeepsBothWithScope,
    /// Needs signoff review before determination.
    NeedsSignoffReview,
    /// Dismissed as noise.
    DismissedAsNoise,
}

/// Evidence freshness status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceFreshness {
    /// Evidence is current.
    Current,
    /// Evidence may be stale.
    PossiblyStale,
    /// Evidence is confirmed stale.
    Stale,
    /// Freshness cannot be determined.
    Unknown,
}

// ---------------------------------------------------------------------------
// RecallPreferenceProfile — preference caps and audit
// ---------------------------------------------------------------------------

/// A versioned preference profile that tunes recall ranking within
/// policy-bounded caps.
///
/// Preferences may weight attention, group/sort results, affect notification
/// priority, and personalise within policy bounds. They must not:
/// - create authority, approve signoff, or create `BindingEvidence`;
/// - assert compliance or assert no-conflict;
/// - hide accepted deterministic evidence;
/// - mutate hidden ranking behaviour without audit.
///
/// Policy caps beat preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallPreferenceProfile {
    /// Unique identifier for this profile instance.
    pub profile_id: Uuid,

    /// Human-readable name (e.g. "security-heavy", "developer-local").
    pub name: String,

    /// Profile version — monotonically increasing per profile_id.
    pub version: u32,

    /// Optional parent profile to inherit from.
    /// Child weights override parent weights; policy caps are always applied
    /// after inheritance resolution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_profile_id: Option<Uuid>,

    /// Scope at which this profile applies.
    pub scope: PreferenceScope,

    /// Route weight overrides — each entry adjusts attention for a recall
    /// route within the allowed [`ROUTE_WEIGHT_FLOOR`]..=[`ROUTE_WEIGHT_CAP`]
    /// range.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub route_weights: Vec<RouteWeightOverride>,

    /// Tier weight overrides — adjust relative attention for ranking tiers
    /// within the allowed [`TIER_WEIGHT_FLOOR`]..=[`TIER_WEIGHT_CAP`] range.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tier_weights: Vec<TierWeightOverride>,

    /// Source class boosts/penalties within policy bounds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_class_adjustments: Vec<SourceClassAdjustment>,

    /// Notification priority overrides.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notification_overrides: Vec<NotificationOverride>,

    /// When this profile was created.
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// When this profile was last modified.
    pub updated_at: chrono::DateTime<chrono::Utc>,

    /// Actor who created or last modified the profile.
    pub actor_id: String,
}

/// Route weight cap — no route may receive weight above this value.
pub const ROUTE_WEIGHT_CAP: f64 = 2.0;

/// Route weight floor — no route may be suppressed below this value.
/// A floor > 0 prevents preferences from hiding a route entirely.
pub const ROUTE_WEIGHT_FLOOR: f64 = 0.1;

/// Tier weight cap — no tier may receive weight above this value.
pub const TIER_WEIGHT_CAP: f64 = 3.0;

/// Tier weight floor — no tier may be suppressed below this value.
pub const TIER_WEIGHT_FLOOR: f64 = 0.1;

/// Source class adjustment cap (absolute value).
pub const SOURCE_CLASS_ADJUSTMENT_CAP: f64 = 1.0;

/// Scope at which a preference profile applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreferenceScope {
    /// Policy-level defaults (applied first, set by governance).
    Policy,
    /// Workspace-level preferences.
    Workspace,
    /// User-level preferences (most specific, applied last).
    User,
}

/// Recall route identifier for weight overrides.
///
/// Mirrors the route categories from the recall strategy spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallRoute {
    /// Route A: accepted BindingEvidence backtrace.
    BindingBacktrace,
    /// Route B: graph overlap and neighbourhood expansion.
    GraphOverlap,
    /// Route C: structured trusted-corpus search.
    StructuredCorpus,
    /// Route D: semantic/textual reviewed-corpus search.
    SemanticCorpus,
    /// Route E: ownership, project, and source-overlap boosting.
    OwnershipOverlap,
    /// Route F: CodeGenome-style advisory hints.
    CodegenomeAdvisory,
}

/// A weight override for a specific recall route.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteWeightOverride {
    /// Which route to adjust.
    pub route: RecallRoute,

    /// Weight multiplier, clamped to [`ROUTE_WEIGHT_FLOOR`]..=[`ROUTE_WEIGHT_CAP`].
    pub weight: f64,
}

/// Ranking tier identifier for weight overrides.
///
/// Mirrors the tier precedence from the recall strategy spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RankingTier {
    /// Tier 1: accepted binding overlap.
    AcceptedBindingOverlap,
    /// Tier 2: verified graph/symbol overlap.
    VerifiedGraphOverlap,
    /// Tier 3: high-confidence projected impact.
    ProjectedImpact,
    /// Tier 4: accepted decision/source overlap.
    DecisionSourceOverlap,
    /// Tier 5: ownership/project/source overlap.
    OwnershipOverlap,
    /// Tier 6: semantic/textual similarity.
    SemanticSimilarity,
    /// Tier 7: CodeGenome advisory hints.
    CodegenomeHints,
    /// Tier 8: approximate candidates.
    ApproximateCandidates,
}

/// A weight override for a specific ranking tier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierWeightOverride {
    /// Which tier to adjust.
    pub tier: RankingTier,

    /// Weight multiplier, clamped to [`TIER_WEIGHT_FLOOR`]..=[`TIER_WEIGHT_CAP`].
    pub weight: f64,
}

/// Source class adjustment for boosting or penalising specific source kinds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceClassAdjustment {
    /// Source class to adjust (e.g. "slack", "linear", "github", "meeting").
    pub source_class: String,

    /// Adjustment value, clamped to -[`SOURCE_CLASS_ADJUSTMENT_CAP`]..=[`SOURCE_CLASS_ADJUSTMENT_CAP`].
    pub adjustment: f64,
}

/// Notification priority override for recall results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationOverride {
    /// Category of notification to adjust.
    pub category: NotificationCategory,

    /// Priority level.
    pub priority: NotificationPriority,
}

/// Notification category for recall results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationCategory {
    /// Critical governance overlap found.
    GovernanceOverlap,
    /// Binding staleness detected.
    BindingStaleness,
    /// Unknown scope warning.
    UnknownScope,
    /// New candidates surfaced.
    NewCandidates,
}

/// Notification priority level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationPriority {
    /// High — always surface.
    High,
    /// Normal — surface in default views.
    Normal,
    /// Low — surface only when expanded.
    Low,
}

// ---------------------------------------------------------------------------
// Policy caps — forbidden preference actions
// ---------------------------------------------------------------------------

/// Actions that preferences are forbidden from performing.
///
/// This enum is checked at profile application time. Any preference that would
/// trigger a forbidden action is refused with a typed error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForbiddenPreferenceAction {
    /// Preferences cannot create governance authority.
    CreateAuthority,
    /// Preferences cannot approve signoff.
    ApproveSignoff,
    /// Preferences cannot create BindingEvidence.
    CreateBindingEvidence,
    /// Preferences cannot assert compliance.
    AssertCompliance,
    /// Preferences cannot assert no-conflict.
    AssertNoConflict,
    /// Preferences cannot hide accepted deterministic evidence.
    HideAcceptedEvidence,
}

impl ForbiddenPreferenceAction {
    /// All forbidden actions.
    pub fn all() -> &'static [ForbiddenPreferenceAction] {
        &[
            Self::CreateAuthority,
            Self::ApproveSignoff,
            Self::CreateBindingEvidence,
            Self::AssertCompliance,
            Self::AssertNoConflict,
            Self::HideAcceptedEvidence,
        ]
    }
}

// ---------------------------------------------------------------------------
// Profile application and conflict resolution
// ---------------------------------------------------------------------------

/// Error produced when a preference profile violates policy caps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreferenceCapViolation {
    /// Which cap was violated.
    pub violation_kind: PreferenceCapViolationKind,

    /// Human-readable detail.
    pub detail: String,

    /// The field/value that caused the violation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,

    /// The value that was out of bounds (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,

    /// The bound that was exceeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bound: Option<f64>,
}

/// Kind of preference cap violation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreferenceCapViolationKind {
    /// Route weight exceeds cap.
    RouteWeightAboveCap,
    /// Route weight below floor.
    RouteWeightBelowFloor,
    /// Tier weight exceeds cap.
    TierWeightAboveCap,
    /// Tier weight below floor.
    TierWeightBelowFloor,
    /// Source class adjustment exceeds cap.
    SourceClassAboveCap,
    /// Attempted forbidden action.
    ForbiddenAction,
    /// Duplicate route entry in a single profile.
    DuplicateRouteEntry,
    /// Duplicate tier entry in a single profile.
    DuplicateTierEntry,
}

/// Conflict resolution strategy when merging inherited profiles.
///
/// Inheritance resolution: policy (scope=Policy) applies first, then
/// workspace, then user. Within a scope, child overrides parent for the
/// same route/tier/source. Policy caps are applied after all inheritance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictResolution {
    /// Child value wins (default for same-scope inheritance).
    ChildOverridesParent,
    /// More restrictive value wins (used when policy conflicts with user).
    MostRestrictiveWins,
}

/// Resolved preference profile — the result of applying inheritance,
/// conflict resolution, and policy cap enforcement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedPreferenceProfile {
    /// Resolved route weights (after inheritance and cap clamping).
    pub route_weights: Vec<RouteWeightOverride>,

    /// Resolved tier weights (after inheritance and cap clamping).
    pub tier_weights: Vec<TierWeightOverride>,

    /// Resolved source class adjustments (after inheritance and cap clamping).
    pub source_class_adjustments: Vec<SourceClassAdjustment>,

    /// Resolved notification overrides.
    pub notification_overrides: Vec<NotificationOverride>,

    /// Any violations that were clamped during resolution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clamped_violations: Vec<PreferenceCapViolation>,

    /// Lineage: profile IDs in application order (policy first, user last).
    pub lineage: Vec<Uuid>,

    /// The profile version string for RecallTrace.
    pub version_tag: String,
}

// ---------------------------------------------------------------------------
// Preference audit records
// ---------------------------------------------------------------------------

/// Audit record for a preference profile change.
///
/// Profile changes are replayable through `RecallTrace` — each change
/// produces an audit record capturing the before/after state and the actor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreferenceAuditRecord {
    /// Unique identifier for this audit entry.
    pub audit_id: Uuid,

    /// Profile that was changed.
    pub profile_id: Uuid,

    /// Profile version after this change.
    pub new_version: u32,

    /// Profile version before this change (None for creation).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_version: Option<u32>,

    /// What kind of change occurred.
    pub change_kind: PreferenceChangeKind,

    /// Specific field changes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub field_changes: Vec<PreferenceFieldChange>,

    /// Violations that were clamped or refused.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub violations: Vec<PreferenceCapViolation>,

    /// Actor who made the change.
    pub actor_id: String,

    /// When the change occurred.
    pub changed_at: chrono::DateTime<chrono::Utc>,

    /// Non-authoritative trace reference for replay.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_ref: Option<String>,
}

/// Kind of preference profile change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreferenceChangeKind {
    /// Profile was created.
    Created,
    /// Profile was updated.
    Updated,
    /// Profile was deactivated.
    Deactivated,
    /// Profile inherited from a new parent.
    InheritanceChanged,
}

/// A single field change within a preference audit record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreferenceFieldChange {
    /// Path to the changed field (e.g. "route_weights.binding_backtrace").
    pub field_path: String,

    /// Previous value (serialised as JSON string for portability).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_value: Option<String>,

    /// New value (serialised as JSON string for portability).
    pub new_value: String,
}

// ---------------------------------------------------------------------------
// RecallPreferenceProfile — cap enforcement helpers
// ---------------------------------------------------------------------------

impl RecallPreferenceProfile {
    /// Clamp all weights to policy caps/floors.
    ///
    /// Returns the list of violations that required clamping.
    pub fn clamp_to_policy_caps(&mut self) -> Vec<PreferenceCapViolation> {
        let mut violations = Vec::new();

        for rw in &mut self.route_weights {
            if rw.weight > ROUTE_WEIGHT_CAP {
                violations.push(PreferenceCapViolation {
                    violation_kind: PreferenceCapViolationKind::RouteWeightAboveCap,
                    detail: format!(
                        "Route {:?} weight {} exceeds cap {}",
                        rw.route, rw.weight, ROUTE_WEIGHT_CAP
                    ),
                    field: Some(format!("route_weights.{:?}", rw.route)),
                    value: Some(rw.weight),
                    bound: Some(ROUTE_WEIGHT_CAP),
                });
                rw.weight = ROUTE_WEIGHT_CAP;
            } else if rw.weight < ROUTE_WEIGHT_FLOOR {
                violations.push(PreferenceCapViolation {
                    violation_kind: PreferenceCapViolationKind::RouteWeightBelowFloor,
                    detail: format!(
                        "Route {:?} weight {} below floor {}",
                        rw.route, rw.weight, ROUTE_WEIGHT_FLOOR
                    ),
                    field: Some(format!("route_weights.{:?}", rw.route)),
                    value: Some(rw.weight),
                    bound: Some(ROUTE_WEIGHT_FLOOR),
                });
                rw.weight = ROUTE_WEIGHT_FLOOR;
            }
        }

        for tw in &mut self.tier_weights {
            if tw.weight > TIER_WEIGHT_CAP {
                violations.push(PreferenceCapViolation {
                    violation_kind: PreferenceCapViolationKind::TierWeightAboveCap,
                    detail: format!(
                        "Tier {:?} weight {} exceeds cap {}",
                        tw.tier, tw.weight, TIER_WEIGHT_CAP
                    ),
                    field: Some(format!("tier_weights.{:?}", tw.tier)),
                    value: Some(tw.weight),
                    bound: Some(TIER_WEIGHT_CAP),
                });
                tw.weight = TIER_WEIGHT_CAP;
            } else if tw.weight < TIER_WEIGHT_FLOOR {
                violations.push(PreferenceCapViolation {
                    violation_kind: PreferenceCapViolationKind::TierWeightBelowFloor,
                    detail: format!(
                        "Tier {:?} weight {} below floor {}",
                        tw.tier, tw.weight, TIER_WEIGHT_FLOOR
                    ),
                    field: Some(format!("tier_weights.{:?}", tw.tier)),
                    value: Some(tw.weight),
                    bound: Some(TIER_WEIGHT_FLOOR),
                });
                tw.weight = TIER_WEIGHT_FLOOR;
            }
        }

        for sa in &mut self.source_class_adjustments {
            if sa.adjustment > SOURCE_CLASS_ADJUSTMENT_CAP {
                violations.push(PreferenceCapViolation {
                    violation_kind: PreferenceCapViolationKind::SourceClassAboveCap,
                    detail: format!(
                        "Source class '{}' adjustment {} exceeds cap {}",
                        sa.source_class, sa.adjustment, SOURCE_CLASS_ADJUSTMENT_CAP
                    ),
                    field: Some(format!("source_class_adjustments.{}", sa.source_class)),
                    value: Some(sa.adjustment),
                    bound: Some(SOURCE_CLASS_ADJUSTMENT_CAP),
                });
                sa.adjustment = SOURCE_CLASS_ADJUSTMENT_CAP;
            } else if sa.adjustment < -SOURCE_CLASS_ADJUSTMENT_CAP {
                violations.push(PreferenceCapViolation {
                    violation_kind: PreferenceCapViolationKind::SourceClassAboveCap,
                    detail: format!(
                        "Source class '{}' adjustment {} below floor {}",
                        sa.source_class, sa.adjustment, -SOURCE_CLASS_ADJUSTMENT_CAP
                    ),
                    field: Some(format!("source_class_adjustments.{}", sa.source_class)),
                    value: Some(sa.adjustment),
                    bound: Some(-SOURCE_CLASS_ADJUSTMENT_CAP),
                });
                sa.adjustment = -SOURCE_CLASS_ADJUSTMENT_CAP;
            }
        }

        violations
    }

    /// Validate the profile for duplicate entries.
    ///
    /// Returns violations for duplicate route or tier entries.
    pub fn validate_no_duplicates(&self) -> Vec<PreferenceCapViolation> {
        let mut violations = Vec::new();

        let mut seen_routes = Vec::new();
        for rw in &self.route_weights {
            if seen_routes.contains(&rw.route) {
                violations.push(PreferenceCapViolation {
                    violation_kind: PreferenceCapViolationKind::DuplicateRouteEntry,
                    detail: format!("Duplicate route weight entry for {:?}", rw.route),
                    field: Some(format!("route_weights.{:?}", rw.route)),
                    value: None,
                    bound: None,
                });
            } else {
                seen_routes.push(rw.route);
            }
        }

        let mut seen_tiers = Vec::new();
        for tw in &self.tier_weights {
            if seen_tiers.contains(&tw.tier) {
                violations.push(PreferenceCapViolation {
                    violation_kind: PreferenceCapViolationKind::DuplicateTierEntry,
                    detail: format!("Duplicate tier weight entry for {:?}", tw.tier),
                    field: Some(format!("tier_weights.{:?}", tw.tier)),
                    value: None,
                    bound: None,
                });
            } else {
                seen_tiers.push(tw.tier);
            }
        }

        violations
    }
}

/// Resolve a chain of profiles applying inheritance and policy caps.
///
/// Profiles must be ordered: policy-scope first, then workspace, then user.
/// Within each scope level, child profiles override parent values for the
/// same route/tier/source. Policy caps are applied after all merging.
pub fn resolve_preference_chain(profiles: &[RecallPreferenceProfile]) -> ResolvedPreferenceProfile {
    let mut merged_routes: Vec<RouteWeightOverride> = Vec::new();
    let mut merged_tiers: Vec<TierWeightOverride> = Vec::new();
    let mut merged_source: Vec<SourceClassAdjustment> = Vec::new();
    let mut merged_notif: Vec<NotificationOverride> = Vec::new();
    let mut lineage: Vec<Uuid> = Vec::new();

    for profile in profiles {
        lineage.push(profile.profile_id);

        // Route weights: later profiles override earlier ones for the same route.
        for rw in &profile.route_weights {
            if let Some(existing) = merged_routes.iter_mut().find(|r| r.route == rw.route) {
                existing.weight = rw.weight;
            } else {
                merged_routes.push(rw.clone());
            }
        }

        // Tier weights: same override logic.
        for tw in &profile.tier_weights {
            if let Some(existing) = merged_tiers.iter_mut().find(|t| t.tier == tw.tier) {
                existing.weight = tw.weight;
            } else {
                merged_tiers.push(tw.clone());
            }
        }

        // Source class adjustments: same override logic.
        for sa in &profile.source_class_adjustments {
            if let Some(existing) = merged_source
                .iter_mut()
                .find(|s| s.source_class == sa.source_class)
            {
                existing.adjustment = sa.adjustment;
            } else {
                merged_source.push(sa.clone());
            }
        }

        // Notification overrides: same override logic.
        for no in &profile.notification_overrides {
            if let Some(existing) = merged_notif.iter_mut().find(|n| n.category == no.category) {
                existing.priority = no.priority;
            } else {
                merged_notif.push(no.clone());
            }
        }
    }

    // Apply policy caps to merged values.
    let mut violations = Vec::new();

    for rw in &mut merged_routes {
        if rw.weight > ROUTE_WEIGHT_CAP {
            violations.push(PreferenceCapViolation {
                violation_kind: PreferenceCapViolationKind::RouteWeightAboveCap,
                detail: format!(
                    "Merged route {:?} weight {} clamped to cap {}",
                    rw.route, rw.weight, ROUTE_WEIGHT_CAP
                ),
                field: Some(format!("route_weights.{:?}", rw.route)),
                value: Some(rw.weight),
                bound: Some(ROUTE_WEIGHT_CAP),
            });
            rw.weight = ROUTE_WEIGHT_CAP;
        } else if rw.weight < ROUTE_WEIGHT_FLOOR {
            violations.push(PreferenceCapViolation {
                violation_kind: PreferenceCapViolationKind::RouteWeightBelowFloor,
                detail: format!(
                    "Merged route {:?} weight {} clamped to floor {}",
                    rw.route, rw.weight, ROUTE_WEIGHT_FLOOR
                ),
                field: Some(format!("route_weights.{:?}", rw.route)),
                value: Some(rw.weight),
                bound: Some(ROUTE_WEIGHT_FLOOR),
            });
            rw.weight = ROUTE_WEIGHT_FLOOR;
        }
    }

    for tw in &mut merged_tiers {
        if tw.weight > TIER_WEIGHT_CAP {
            violations.push(PreferenceCapViolation {
                violation_kind: PreferenceCapViolationKind::TierWeightAboveCap,
                detail: format!(
                    "Merged tier {:?} weight {} clamped to cap {}",
                    tw.tier, tw.weight, TIER_WEIGHT_CAP
                ),
                field: Some(format!("tier_weights.{:?}", tw.tier)),
                value: Some(tw.weight),
                bound: Some(TIER_WEIGHT_CAP),
            });
            tw.weight = TIER_WEIGHT_CAP;
        } else if tw.weight < TIER_WEIGHT_FLOOR {
            violations.push(PreferenceCapViolation {
                violation_kind: PreferenceCapViolationKind::TierWeightBelowFloor,
                detail: format!(
                    "Merged tier {:?} weight {} clamped to floor {}",
                    tw.tier, tw.weight, TIER_WEIGHT_FLOOR
                ),
                field: Some(format!("tier_weights.{:?}", tw.tier)),
                value: Some(tw.weight),
                bound: Some(TIER_WEIGHT_FLOOR),
            });
            tw.weight = TIER_WEIGHT_FLOOR;
        }
    }

    for sa in &mut merged_source {
        if sa.adjustment > SOURCE_CLASS_ADJUSTMENT_CAP {
            violations.push(PreferenceCapViolation {
                violation_kind: PreferenceCapViolationKind::SourceClassAboveCap,
                detail: format!(
                    "Merged source '{}' adjustment {} clamped to cap {}",
                    sa.source_class, sa.adjustment, SOURCE_CLASS_ADJUSTMENT_CAP
                ),
                field: Some(format!("source_class_adjustments.{}", sa.source_class)),
                value: Some(sa.adjustment),
                bound: Some(SOURCE_CLASS_ADJUSTMENT_CAP),
            });
            sa.adjustment = SOURCE_CLASS_ADJUSTMENT_CAP;
        } else if sa.adjustment < -SOURCE_CLASS_ADJUSTMENT_CAP {
            violations.push(PreferenceCapViolation {
                violation_kind: PreferenceCapViolationKind::SourceClassAboveCap,
                detail: format!(
                    "Merged source '{}' adjustment {} clamped to floor {}",
                    sa.source_class, sa.adjustment, -SOURCE_CLASS_ADJUSTMENT_CAP
                ),
                field: Some(format!("source_class_adjustments.{}", sa.source_class)),
                value: Some(sa.adjustment),
                bound: Some(-SOURCE_CLASS_ADJUSTMENT_CAP),
            });
            sa.adjustment = -SOURCE_CLASS_ADJUSTMENT_CAP;
        }
    }

    let version_tag = if let Some(last) = profiles.last() {
        format!("{}@v{}", last.profile_id, last.version)
    } else {
        "empty".to_string()
    };

    ResolvedPreferenceProfile {
        route_weights: merged_routes,
        tier_weights: merged_tiers,
        source_class_adjustments: merged_source,
        notification_overrides: merged_notif,
        clamped_violations: violations,
        lineage,
        version_tag,
    }
}

// ---------------------------------------------------------------------------
// SearchedScope — honest scope reporting for recall
// ---------------------------------------------------------------------------

/// The exact bounded set Bicameral actually searched during a recall operation.
///
/// No `RecallPacket` may make a negative claim stronger than its searched scope
/// supports. Unknown scope must be surfaced honestly rather than implying
/// coverage that did not happen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchedScope {
    /// Overall scope level achieved by this search.
    pub level: ScopeLevel,

    /// Source scope — operational sources searched (docs, tickets, chat, etc.).
    pub source: ScopeSection,

    /// Code scope — workspace code and file targets searched.
    pub code: ScopeSection,

    /// Graph scope — graph snapshot, bindings, and symbol neighborhoods searched.
    pub graph: ScopeSection,

    /// Retrieval scope — structured and semantic index queries performed.
    pub retrieval: ScopeSection,

    /// Advisory scope — CodeGenome-style hints and weak signals consulted.
    pub advisory: ScopeSection,

    /// Unknown scope — areas known to be unsearched or unreachable.
    pub unknown: ScopeSection,
}

impl SearchedScope {
    /// Whether this scope is sufficient for positive recall candidates.
    ///
    /// Positive candidates require enumerated scope (level 2) or stronger.
    pub fn allows_positive_candidates(&self) -> bool {
        self.level as u8 >= ScopeLevel::Enumerated as u8
    }

    /// Whether this scope is sufficient for no-result-in-searched-scope claims.
    ///
    /// No-result claims require replayable scope (level 3) or stronger.
    pub fn allows_no_result_claim(&self) -> bool {
        self.level as u8 >= ScopeLevel::Replayable as u8
    }

    /// Whether any section has a caveat that must be surfaced.
    pub fn has_caveats(&self) -> bool {
        self.source.has_caveat()
            || self.code.has_caveat()
            || self.graph.has_caveat()
            || self.retrieval.has_caveat()
            || self.advisory.has_caveat()
            || self.unknown.has_caveat()
    }

    /// Collect all sections that carry caveats.
    pub fn caveat_sections(&self) -> Vec<(&'static str, &ScopeSection)> {
        let mut out = Vec::new();
        if self.source.has_caveat() {
            out.push(("source", &self.source));
        }
        if self.code.has_caveat() {
            out.push(("code", &self.code));
        }
        if self.graph.has_caveat() {
            out.push(("graph", &self.graph));
        }
        if self.retrieval.has_caveat() {
            out.push(("retrieval", &self.retrieval));
        }
        if self.advisory.has_caveat() {
            out.push(("advisory", &self.advisory));
        }
        if self.unknown.has_caveat() {
            out.push(("unknown", &self.unknown));
        }
        out
    }
}

/// Scope level describing the strength of searched-scope evidence.
///
/// Higher levels enable stronger claims about recall results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum ScopeLevel {
    /// Level 0: unscoped or ambiguous — typed refusal only.
    Unscoped = 0,
    /// Level 1: declared scope — routing/setup, not meaningful no-result confidence.
    Declared = 1,
    /// Level 2: enumerated scope — sufficient for positive advisory recall candidates.
    Enumerated = 2,
    /// Level 3: replayable scope — required for "no relevant Decision found in searched scope."
    Replayable = 3,
    /// Level 4: audited scope — required for governed disputes or later audit.
    Audited = 4,
}

/// A single section of searched scope describing what was searched within one domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeSection {
    /// Status of this scope section.
    pub status: ScopeSectionStatus,

    /// Targets or areas actually searched within this section.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub searched_targets: Vec<String>,

    /// Version or identity of the index/snapshot used (when available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Human-readable caveat when status is not `searched`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caveat: Option<String>,
}

impl ScopeSection {
    /// Create a section with `searched` status and the given targets.
    pub fn searched(targets: Vec<String>, version: Option<String>) -> Self {
        Self {
            status: ScopeSectionStatus::Searched,
            searched_targets: targets,
            version,
            caveat: None,
        }
    }

    /// Create a section with `not_applicable` status.
    pub fn not_applicable() -> Self {
        Self {
            status: ScopeSectionStatus::NotApplicable,
            searched_targets: Vec::new(),
            version: None,
            caveat: None,
        }
    }

    /// Create a section with `unavailable` status and a caveat.
    pub fn unavailable(caveat: impl Into<String>) -> Self {
        Self {
            status: ScopeSectionStatus::Unavailable,
            searched_targets: Vec::new(),
            version: None,
            caveat: Some(caveat.into()),
        }
    }

    /// Create a section with `stale` status and a caveat.
    pub fn stale(caveat: impl Into<String>, version: Option<String>) -> Self {
        Self {
            status: ScopeSectionStatus::Stale,
            searched_targets: Vec::new(),
            version,
            caveat: Some(caveat.into()),
        }
    }

    /// Create a section with `unsupported` status and a caveat.
    pub fn unsupported(caveat: impl Into<String>) -> Self {
        Self {
            status: ScopeSectionStatus::Unsupported,
            searched_targets: Vec::new(),
            version: None,
            caveat: Some(caveat.into()),
        }
    }

    /// Create a section with `partial` status and a caveat.
    pub fn partial(
        targets: Vec<String>,
        caveat: impl Into<String>,
        version: Option<String>,
    ) -> Self {
        Self {
            status: ScopeSectionStatus::Partial,
            searched_targets: targets,
            version,
            caveat: Some(caveat.into()),
        }
    }

    /// Whether this section has a caveat that must be surfaced to the caller.
    pub fn has_caveat(&self) -> bool {
        matches!(
            self.status,
            ScopeSectionStatus::Partial
                | ScopeSectionStatus::Unavailable
                | ScopeSectionStatus::Stale
                | ScopeSectionStatus::Unsupported
        )
    }
}

/// Status of a scope section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeSectionStatus {
    /// Section was fully searched.
    Searched,
    /// Section was partially searched — caveat required.
    Partial,
    /// Section data was unavailable — caveat required.
    Unavailable,
    /// Section data was stale — caveat required.
    Stale,
    /// Section is not supported in this context — caveat required.
    Unsupported,
    /// Section is not applicable to this recall context.
    NotApplicable,
}

// ---------------------------------------------------------------------------
// RecallTrace — replayable evidence for recall reproduction
// ---------------------------------------------------------------------------

/// Replayable trace for a recall operation.
///
/// Every `RecallPacket` must be reproducible from its `RecallTrace` or explain
/// why replay is unavailable. The trace records all inputs, versions, and
/// intermediate decisions that produced the packet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallTrace {
    /// Unique trace identifier.
    pub trace_id: String,

    /// Hash of the original recall request.
    pub request_hash: String,

    /// Hash of the normalized input after preprocessing.
    pub normalized_input_hash: String,

    /// Version of the decision index used.
    pub decision_index_version: String,

    /// Version of the metadata schema used.
    pub metadata_schema_version: String,

    /// Version of the controlled taxonomy used for deterministic metadata.
    pub taxonomy_version: String,

    /// Graph snapshot identifier (when graph-backed recall was used).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_snapshot_id: Option<String>,

    /// Version of the source corpus searched.
    pub source_corpus_version: String,

    /// Version of the semantic index (when semantic search was used).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_index_version: Option<String>,

    /// Version of CodeGenome-style signals (when advisory signals were used).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codegenome_signal_version: Option<String>,

    /// Version of the preference profile applied.
    pub preference_profile_version: String,

    /// Version of the scoring function used for ranking.
    pub scoring_function_version: String,

    /// Results from each recall route that was executed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub route_results: Vec<RouteResult>,

    /// Signals admitted through the recall-admissibility gate.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub admitted_signals: Vec<RecallSignal>,

    /// Signals downgraded (used with reduced weight or caveats).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub downgraded_signals: Vec<RecallSignal>,

    /// Signals rejected by the recall-admissibility gate.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejected_signals: Vec<RecallSignal>,

    /// Final rank tuple per result, recording the scoring breakdown.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub final_rank_tuples: Vec<RankTuple>,

    /// Stop conditions that ended the search.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stop_conditions: Vec<StopCondition>,

    /// Whether this trace is sufficient for deterministic replay.
    pub replay_available: bool,

    /// Explanation when replay is not available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replay_unavailable_reason: Option<String>,
}

/// Result from a single recall route execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteResult {
    /// Route identifier (e.g. "binding_backtrace", "graph_overlap",
    /// "structured_search", "semantic_search", "ownership_boost", "advisory_hints").
    pub route_id: String,

    /// Whether the route executed successfully.
    pub status: RouteStatus,

    /// Number of candidates produced by this route.
    pub candidate_count: u32,

    /// Detail or explanation of the route result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Status of a recall route execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteStatus {
    /// Route executed and produced results.
    Completed,
    /// Route executed but found no results.
    Empty,
    /// Route was skipped (not applicable or not configured).
    Skipped,
    /// Route failed during execution.
    Failed,
}

/// A signal considered during recall ranking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallSignal {
    /// Signal kind (e.g. "binding_overlap", "graph_neighbor", "semantic_similarity",
    /// "ownership_match", "codegenome_hint").
    pub kind: String,

    /// Source or origin of the signal.
    pub source: String,

    /// Reason for admission, downgrade, or rejection.
    pub reason: String,
}

/// Final rank tuple for a single recall result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankTuple {
    /// Result identifier (decision_id or candidate_id).
    pub result_id: String,

    /// Causal tier assigned (1 = strongest, 8 = weakest).
    pub tier: u8,

    /// Score within the tier.
    pub score: f64,

    /// Signal contributions that determined this rank.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contributing_signals: Vec<String>,
}

/// A stop condition that ended or bounded the recall search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopCondition {
    /// Kind of stop condition.
    pub kind: StopConditionKind,

    /// Detail about why the search stopped.
    pub detail: String,
}

/// Kind of stop condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopConditionKind {
    /// Search budget exhausted (traversal depth, candidate count, etc.).
    BudgetExhausted,
    /// All configured routes completed.
    AllRoutesCompleted,
    /// Sufficient results found within scope.
    SufficientResults,
    /// Scope boundary reached — expansion required for more.
    ScopeBoundary,
    /// Timeout reached.
    Timeout,
}

// ---------------------------------------------------------------------------
// Recall provider seam
// ---------------------------------------------------------------------------

/// Runtime status for recall and semantic-code-search providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallProviderStatus {
    Ready,
    Stale,
    NotConfigured,
    Indexing,
    Failed,
    Unsupported,
}

/// Scope kind requested from a recall provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallScopeKind {
    Binding,
    Diff,
    FileSet,
    SymbolSet,
    Query,
    BindingRevalidation,
}

/// Scope a provider actually searched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallSearchedScope {
    pub scope_kind: RecallScopeKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corpus_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corpus_version: Option<String>,
}

impl RecallSearchedScope {
    pub fn query() -> Self {
        Self {
            scope_kind: RecallScopeKind::Query,
            scope_fingerprint: None,
            files: Vec::new(),
            symbols: Vec::new(),
            corpus_id: None,
            corpus_version: None,
        }
    }
}

/// Explicit unknown, skipped, stale, or unsupported provider scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallUnknownProviderScope {
    pub scope: String,
    pub reason: RecallUnknownProviderScopeReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Why a provider could not cover part of the requested scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallUnknownProviderScopeReason {
    NotIndexed,
    StaleIndex,
    UnsupportedLanguage,
    UnsupportedRepository,
    ProviderUnavailable,
    OutsideRequestedScope,
    Unknown,
}

/// Stable provider provenance that downstream gates can inspect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallProviderProvenance {
    pub provider_id: String,
    pub provider_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlay_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_version: Option<String>,
}

/// Input to a generic recall signal provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallSignalRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_evidence_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_id: Option<String>,
    #[serde(default = "default_provider_limit")]
    pub limit: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_kind: Option<RecallScopeKind>,
}

impl RecallSignalRequest {
    pub fn searched_scope(&self) -> RecallSearchedScope {
        RecallSearchedScope {
            scope_kind: self.scope_kind.unwrap_or(RecallScopeKind::Query),
            scope_fingerprint: None,
            files: self.files.clone(),
            symbols: self.symbols.clone(),
            corpus_id: self.workspace_id.clone(),
            corpus_version: self
                .graph_snapshot_id
                .clone()
                .or_else(|| self.commit_sha.clone()),
        }
    }
}

/// Input to a semantic code search provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticCodeSearchRequest {
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_evidence_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_id: Option<String>,
    #[serde(default = "default_provider_limit")]
    pub limit: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_kind: Option<RecallScopeKind>,
}

impl SemanticCodeSearchRequest {
    pub fn searched_scope(&self) -> RecallSearchedScope {
        RecallSignalRequest {
            query: Some(self.query.clone()),
            workspace_id: self.workspace_id.clone(),
            graph_snapshot_id: self.graph_snapshot_id.clone(),
            commit_sha: self.commit_sha.clone(),
            ref_name: self.ref_name.clone(),
            files: self.files.clone(),
            symbols: self.symbols.clone(),
            changed_files: self.changed_files.clone(),
            source_evidence_ids: self.source_evidence_ids.clone(),
            decision_id: self.decision_id.clone(),
            candidate_id: self.candidate_id.clone(),
            limit: self.limit,
            scope_kind: self.scope_kind,
        }
        .searched_scope()
    }
}

fn default_provider_limit() -> usize {
    20
}

/// Generic provider signal. It ranks and explains; it does not govern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallProviderSignal {
    pub target_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_ref: Option<String>,
    pub signal_kind: RecallProviderSignalKind,
    pub signal_weight: f64,
    pub relevance_reason: String,
    pub currentness: CodeSourceCurrentness,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ranking_metadata: Vec<RecallRankingMetadata>,
    pub provenance: RecallProviderProvenance,
}

/// Families of provider signals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallProviderSignalKind {
    Exact,
    Lexical,
    Semantic,
    Graph,
    Codegenome,
    Ownership,
    SourceOverlap,
    ChangedFile,
    PriorBinding,
}

/// Deterministic ranking metadata attached to a signal or candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallRankingMetadata {
    pub key: String,
    pub value: String,
}

/// A semantic code candidate. It is approximate until deterministic graph
/// verification proves an exact target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticCodeCandidate {
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    pub target_type: SemanticCodeTargetType,
    pub score: f64,
    pub score_kind: SemanticScoreKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matched_terms: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_excerpt: Option<String>,
    pub currentness: CodeSourceCurrentness,
    pub provenance: RecallProviderProvenance,
}

/// Semantic candidate target type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticCodeTargetType {
    FilePath,
    CodeRegion,
    Symbol,
    Dependency,
    DeploySurface,
}

/// Meaning of a provider score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticScoreKind {
    SemanticSimilarity,
    ProviderRank,
    HybridRank,
}

/// Generic recall provider result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallSignalResult {
    pub status: RecallProviderStatus,
    pub provenance: RecallProviderProvenance,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_snapshot_id: Option<String>,
    pub searched_scope: RecallSearchedScope,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unknown_scope: Vec<RecallUnknownProviderScope>,
    pub currentness: CodeSourceCurrentness,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signals: Vec<RecallProviderSignal>,
}

/// Semantic code search provider result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticCodeSearchResult {
    pub status: RecallProviderStatus,
    pub provenance: RecallProviderProvenance,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_snapshot_id: Option<String>,
    pub searched_scope: RecallSearchedScope,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unknown_scope: Vec<RecallUnknownProviderScope>,
    pub currentness: CodeSourceCurrentness,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<SemanticCodeCandidate>,
}

// ---------------------------------------------------------------------------
// No-result language enforcement
// ---------------------------------------------------------------------------

/// Outcome of a no-result evaluation against searched scope.
///
/// Enforces that no-result language is proportional to searched scope level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoResultLanguage {
    /// The searched scope that produced no results.
    pub scope: SearchedScope,

    /// The strongest permitted no-result claim given the scope level.
    pub permitted_claim: NoResultClaim,

    /// Whether the scope had caveats that weaken even the permitted claim.
    pub has_scope_caveats: bool,
}

impl NoResultLanguage {
    /// Derive the strongest permitted no-result claim from a searched scope.
    pub fn from_scope(scope: SearchedScope) -> Self {
        let has_scope_caveats = scope.has_caveats();
        let permitted_claim = match scope.level {
            ScopeLevel::Unscoped => NoResultClaim::Refused,
            ScopeLevel::Declared => NoResultClaim::DeclaredScopeOnly,
            ScopeLevel::Enumerated => NoResultClaim::EnumeratedScopeOnly,
            ScopeLevel::Replayable | ScopeLevel::Audited => {
                NoResultClaim::NoRelevantDecisionInSearchedScope
            }
        };
        Self {
            scope,
            permitted_claim,
            has_scope_caveats,
        }
    }
}

/// Permitted no-result claim based on scope level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoResultClaim {
    /// Scope is insufficient — no no-result claim is permitted (typed refusal).
    Refused,
    /// Declared scope only — no meaningful no-result confidence.
    DeclaredScopeOnly,
    /// Enumerated scope — positive candidates could be found but no-result
    /// language is advisory only.
    EnumeratedScopeOnly,
    /// Replayable or audited scope — "No relevant Decision found in searched scope."
    NoRelevantDecisionInSearchedScope,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recall_packet_new_alpha_has_four_actions() {
        let packet = RecallPacket::new_alpha();
        assert_eq!(packet.allowed_next_actions.len(), 4);
        assert!(packet.is_action_allowed(RecallActionKind::InspectEvidence));
        assert!(packet.is_action_allowed(RecallActionKind::ExpandScope));
        assert!(packet.is_action_allowed(RecallActionKind::PromoteDecisionCandidate));
        assert!(packet.is_action_allowed(RecallActionKind::RequestCorrection));
    }

    #[test]
    fn recall_action_kind_serializes_to_snake_case() {
        assert_eq!(
            serde_json::to_string(&RecallActionKind::InspectEvidence).unwrap(),
            "\"inspect_evidence\""
        );
        assert_eq!(
            serde_json::to_string(&RecallActionKind::ExpandScope).unwrap(),
            "\"expand_scope\""
        );
        assert_eq!(
            serde_json::to_string(&RecallActionKind::PromoteDecisionCandidate).unwrap(),
            "\"promote_decision_candidate\""
        );
        assert_eq!(
            serde_json::to_string(&RecallActionKind::RequestCorrection).unwrap(),
            "\"request_correction\""
        );
    }

    #[test]
    fn recall_action_kind_deserializes_from_snake_case() {
        assert_eq!(
            serde_json::from_str::<RecallActionKind>("\"inspect_evidence\"").unwrap(),
            RecallActionKind::InspectEvidence
        );
        assert_eq!(
            serde_json::from_str::<RecallActionKind>("\"expand_scope\"").unwrap(),
            RecallActionKind::ExpandScope
        );
        assert_eq!(
            serde_json::from_str::<RecallActionKind>("\"promote_decision_candidate\"").unwrap(),
            RecallActionKind::PromoteDecisionCandidate
        );
        assert_eq!(
            serde_json::from_str::<RecallActionKind>("\"request_correction\"").unwrap(),
            RecallActionKind::RequestCorrection
        );
    }

    #[test]
    fn promotion_outcome_serializes_to_snake_case() {
        let cases = vec![
            (PromotionOutcome::NewConstraint, "\"new_constraint\""),
            (
                PromotionOutcome::SupersedesExistingDecision,
                "\"supersedes_existing_decision\"",
            ),
            (
                PromotionOutcome::KeepsBothWithScope,
                "\"keeps_both_with_scope\"",
            ),
            (
                PromotionOutcome::NeedsSignoffReview,
                "\"needs_signoff_review\"",
            ),
            (PromotionOutcome::DismissedAsNoise, "\"dismissed_as_noise\""),
        ];
        for (variant, expected) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    #[test]
    fn candidate_relationship_serializes_to_snake_case() {
        let cases = vec![
            (CandidateRelationship::NewConstraint, "\"new_constraint\""),
            (
                CandidateRelationship::SupersedesExistingDecision,
                "\"supersedes_existing_decision\"",
            ),
            (
                CandidateRelationship::KeepsBothWithScope,
                "\"keeps_both_with_scope\"",
            ),
            (
                CandidateRelationship::NeedsSignoffReview,
                "\"needs_signoff_review\"",
            ),
            (
                CandidateRelationship::DismissedAsNoise,
                "\"dismissed_as_noise\"",
            ),
        ];
        for (variant, expected) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    #[test]
    fn refusal_code_serializes_to_snake_case() {
        let cases = vec![
            (
                RecallRefusalCode::ActionNotAllowed,
                "\"action_not_allowed\"",
            ),
            (
                RecallRefusalCode::MissingPacketRef,
                "\"missing_packet_ref\"",
            ),
            (
                RecallRefusalCode::MissingCandidateRef,
                "\"missing_candidate_ref\"",
            ),
            (RecallRefusalCode::StalePacket, "\"stale_packet\""),
            (
                RecallRefusalCode::MissingSelectedRefs,
                "\"missing_selected_refs\"",
            ),
            (
                RecallRefusalCode::MissingApprovalProof,
                "\"missing_approval_proof\"",
            ),
            (
                RecallRefusalCode::ApprovalAlreadyConsumed,
                "\"approval_already_consumed\"",
            ),
            (
                RecallRefusalCode::UnsupportedAction,
                "\"unsupported_action\"",
            ),
            (
                RecallRefusalCode::DaemonUnavailable,
                "\"daemon_unavailable\"",
            ),
            (
                RecallRefusalCode::ScopeExpansionRequired,
                "\"scope_expansion_required\"",
            ),
            (
                RecallRefusalCode::FreeFloatingClaimRefused,
                "\"free_floating_claim_refused\"",
            ),
            (
                RecallRefusalCode::LocalAuthorityRefused,
                "\"local_authority_refused\"",
            ),
        ];
        for (variant, expected) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    #[test]
    fn recall_refusal_constructors_produce_correct_codes() {
        assert_eq!(
            RecallRefusal::action_not_allowed("test").code,
            RecallRefusalCode::ActionNotAllowed
        );
        assert_eq!(
            RecallRefusal::missing_packet_ref("test").code,
            RecallRefusalCode::MissingPacketRef
        );
        assert_eq!(
            RecallRefusal::missing_candidate_ref("test").code,
            RecallRefusalCode::MissingCandidateRef
        );
        assert_eq!(
            RecallRefusal::stale_packet("test").code,
            RecallRefusalCode::StalePacket
        );
        assert_eq!(
            RecallRefusal::missing_selected_refs("test").code,
            RecallRefusalCode::MissingSelectedRefs
        );
        assert_eq!(
            RecallRefusal::missing_approval_proof("test").code,
            RecallRefusalCode::MissingApprovalProof
        );
        assert_eq!(
            RecallRefusal::unsupported_action("test").code,
            RecallRefusalCode::UnsupportedAction
        );
        assert_eq!(
            RecallRefusal::daemon_unavailable("test").code,
            RecallRefusalCode::DaemonUnavailable
        );
        assert_eq!(
            RecallRefusal::scope_expansion_required("test").code,
            RecallRefusalCode::ScopeExpansionRequired
        );
        assert_eq!(
            RecallRefusal::free_floating_claim_refused("test").code,
            RecallRefusalCode::FreeFloatingClaimRefused
        );
        assert_eq!(
            RecallRefusal::local_authority_refused("test").code,
            RecallRefusalCode::LocalAuthorityRefused
        );
        assert_eq!(
            RecallRefusal::approval_already_consumed("test").code,
            RecallRefusalCode::ApprovalAlreadyConsumed
        );
    }

    #[test]
    fn recall_refusal_with_trace_attaches_ref() {
        let refusal = RecallRefusal::stale_packet("old packet").with_trace("trace-001");
        assert_eq!(refusal.trace_ref, Some("trace-001".into()));
    }

    #[test]
    fn recall_packet_serializes_and_deserializes() {
        let packet = RecallPacket::new_alpha();
        let json = serde_json::to_string(&packet).unwrap();
        let deserialized: RecallPacket = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.packet_id, packet.packet_id);
        assert_eq!(
            deserialized.allowed_next_actions.len(),
            packet.allowed_next_actions.len()
        );
    }

    #[test]
    fn promote_params_serializes_with_approval_proof() {
        let params = RecallPromoteCandidateParams {
            packet_id: Uuid::nil(),
            candidate_id: Uuid::nil(),
            promotion_outcome: PromotionOutcome::NewConstraint,
            supersedes_decision_id: None,
            scoping_relationship: None,
            approval_proof: Some(ApprovalProof {
                kind: "signoff_token".into(),
                value: "tok_abc123".into(),
                actor_id: "user-1".into(),
                provided_at: None,
            }),
            confirmation: None,
        };
        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("signoff_token"));
        let deserialized: RecallPromoteCandidateParams = serde_json::from_str(&json).unwrap();
        assert!(deserialized.approval_proof.is_some());
    }

    #[test]
    fn evidence_freshness_serializes_to_snake_case() {
        assert_eq!(
            serde_json::to_string(&EvidenceFreshness::Current).unwrap(),
            "\"current\""
        );
        assert_eq!(
            serde_json::to_string(&EvidenceFreshness::PossiblyStale).unwrap(),
            "\"possibly_stale\""
        );
        assert_eq!(
            serde_json::to_string(&EvidenceFreshness::Stale).unwrap(),
            "\"stale\""
        );
        assert_eq!(
            serde_json::to_string(&EvidenceFreshness::Unknown).unwrap(),
            "\"unknown\""
        );
    }

    #[test]
    fn code_evidence_verification_serializes() {
        assert_eq!(
            serde_json::to_string(&CodeEvidenceVerification::Verified).unwrap(),
            "\"verified\""
        );
        assert_eq!(
            serde_json::to_string(&CodeEvidenceVerification::Approximate).unwrap(),
            "\"approximate\""
        );
    }

    #[test]
    fn authority_label_kind_serializes() {
        assert_eq!(
            serde_json::to_string(&AuthorityLabelKind::HumanRatified).unwrap(),
            "\"human_ratified\""
        );
        assert_eq!(
            serde_json::to_string(&AuthorityLabelKind::BotProposed).unwrap(),
            "\"bot_proposed\""
        );
        assert_eq!(
            serde_json::to_string(&AuthorityLabelKind::Advisory).unwrap(),
            "\"advisory\""
        );
        assert_eq!(
            serde_json::to_string(&AuthorityLabelKind::Stale).unwrap(),
            "\"stale\""
        );
    }

    // -- Negative tests: RecallPacket actions cannot create unauthorized state --

    #[test]
    fn inspect_evidence_is_read_only_no_mutation_path() {
        // RecallInspectEvidenceResult does not carry any mutation payload.
        // It cannot create SourceSnapshot, SourceEvidence, Decision,
        // BindingEvidence, signoff, compliance, or enforcement state.
        let result = RecallInspectEvidenceResult {
            target_id: Uuid::nil(),
            evidence_items: vec![],
            authority_labels: vec![],
            trace_ref: None,
        };
        let json = serde_json::to_value(&result).unwrap();
        // No mutation-capable fields exist in the serialized form
        assert!(!json.as_object().unwrap().contains_key("decision_id"));
        assert!(!json.as_object().unwrap().contains_key("signoff_state"));
        assert!(!json.as_object().unwrap().contains_key("compliance_state"));
        assert!(!json.as_object().unwrap().contains_key("binding_evidence"));
    }

    #[test]
    fn expand_scope_returns_read_only_packet() {
        // RecallExpandScopeResult returns a RecallPacket — a read-only artifact.
        // It cannot directly create SourceSnapshot, Decision, BindingEvidence,
        // signoff, compliance, or enforcement state.
        let result = RecallExpandScopeResult {
            packet: RecallPacket::new_alpha(),
            trace_ref: None,
        };
        let json = serde_json::to_value(&result).unwrap();
        let packet_val = &json["packet"];
        // Packet has no mutation-capable fields
        assert!(!packet_val
            .as_object()
            .unwrap()
            .contains_key("binding_evidence"));
        assert!(!packet_val
            .as_object()
            .unwrap()
            .contains_key("compliance_state"));
        assert!(!packet_val.as_object().unwrap().contains_key("enforcement"));
    }

    #[test]
    fn promote_candidate_result_does_not_create_binding_evidence() {
        // PromoteCandidateResult can only report outcome and decision_id.
        // It cannot create BindingEvidence, compliance, enforcement, or
        // signoff state outside the governed promotion path.
        let result = RecallPromoteCandidateResult {
            candidate_id: Uuid::nil(),
            outcome: PromotionRequestOutcome::Promoted,
            decision_id: Some(Uuid::nil()),
            review_state: Some(ReviewLifecycleState::Proposed),
            confirmation_required: None,
            confirmation_rejection: None,
            trace_ref: None,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert!(!json.as_object().unwrap().contains_key("binding_evidence"));
        assert!(!json.as_object().unwrap().contains_key("compliance_state"));
        assert!(!json.as_object().unwrap().contains_key("enforcement"));
        assert!(!json.as_object().unwrap().contains_key("signoff_approval"));
    }

    #[test]
    fn promotion_requires_bot_known_candidate_ref() {
        // RecallPromoteCandidateParams requires candidate_id — free-floating
        // LLM claims cannot be promoted without a packet/candidate reference.
        let json = r#"{
            "packet_id": "00000000-0000-0000-0000-000000000000",
            "promotion_outcome": "new_constraint"
        }"#;
        // Missing candidate_id should fail deserialization
        let result = serde_json::from_str::<RecallPromoteCandidateParams>(json);
        assert!(result.is_err());
    }

    #[test]
    fn promotion_with_supersedes_requires_decision_ref() {
        // When outcome is supersedes_existing_decision, the caller should
        // provide supersedes_decision_id. The type allows Option for it,
        // but validation at dispatch time should enforce it.
        let params = RecallPromoteCandidateParams {
            packet_id: Uuid::nil(),
            candidate_id: Uuid::nil(),
            promotion_outcome: PromotionOutcome::SupersedesExistingDecision,
            supersedes_decision_id: None, // missing — runtime should refuse
            scoping_relationship: None,
            approval_proof: None,
            confirmation: None,
        };
        // Type system allows this, but runtime validation should reject.
        // This test documents the expectation.
        assert!(params.supersedes_decision_id.is_none());
        assert_eq!(
            params.promotion_outcome,
            PromotionOutcome::SupersedesExistingDecision
        );
    }

    #[test]
    fn recall_packet_is_action_allowed_returns_false_for_unknown() {
        // An empty allowed_next_actions should refuse all actions.
        let packet = RecallPacket {
            packet_id: Uuid::nil(),
            canonical_decisions: vec![],
            decision_candidates: vec![],
            decision_attention: DecisionAttention::not_warranted(),
            unknown_scope: vec![],
            allowed_next_actions: vec![],
            searched_scope: None,
            recall_trace: None,
            freshness: None,
            semantic_recall: None,
        };
        assert!(!packet.is_action_allowed(RecallActionKind::InspectEvidence));
        assert!(!packet.is_action_allowed(RecallActionKind::ExpandScope));
        assert!(!packet.is_action_allowed(RecallActionKind::PromoteDecisionCandidate));
        assert!(!packet.is_action_allowed(RecallActionKind::RequestCorrection));
    }

    // -- request_correction shape tests --

    #[test]
    fn correction_kind_serializes_to_snake_case() {
        let cases = vec![
            (
                CorrectionKind::FeatureDefinitionChange,
                "\"feature_definition_change\"",
            ),
            (
                CorrectionKind::SourceContradiction,
                "\"source_contradiction\"",
            ),
            (CorrectionKind::MissingConstraint, "\"missing_constraint\""),
            (CorrectionKind::StaleOrAmbiguous, "\"stale_or_ambiguous\""),
        ];
        for (variant, expected) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    #[test]
    fn correction_request_outcome_serializes_to_snake_case() {
        assert_eq!(
            serde_json::to_string(&CorrectionRequestOutcome::Accepted).unwrap(),
            "\"accepted\""
        );
        assert_eq!(
            serde_json::to_string(&CorrectionRequestOutcome::Refused).unwrap(),
            "\"refused\""
        );
    }

    #[test]
    fn request_correction_params_requires_packet_and_items() {
        // Missing packet_id should fail deserialization
        let json = r#"{
            "selected_item_ids": ["00000000-0000-0000-0000-000000000001"],
            "correction_kind": "missing_constraint",
            "rationale": "test",
            "approval_proof": {
                "kind": "signoff_token",
                "value": "tok_1",
                "actor_id": "user-1"
            }
        }"#;
        assert!(serde_json::from_str::<RecallRequestCorrectionParams>(json).is_err());

        // Missing selected_item_ids should fail
        let json2 = r#"{
            "packet_id": "00000000-0000-0000-0000-000000000000",
            "correction_kind": "missing_constraint",
            "rationale": "test",
            "approval_proof": {
                "kind": "signoff_token",
                "value": "tok_1",
                "actor_id": "user-1"
            }
        }"#;
        assert!(serde_json::from_str::<RecallRequestCorrectionParams>(json2).is_err());

        // Missing approval_proof should fail
        let json3 = r#"{
            "packet_id": "00000000-0000-0000-0000-000000000000",
            "selected_item_ids": ["00000000-0000-0000-0000-000000000001"],
            "correction_kind": "missing_constraint",
            "rationale": "test"
        }"#;
        assert!(serde_json::from_str::<RecallRequestCorrectionParams>(json3).is_err());
    }

    #[test]
    fn request_correction_params_roundtrip() {
        let params = RecallRequestCorrectionParams {
            packet_id: Uuid::nil(),
            selected_item_ids: vec![Uuid::nil()],
            correction_kind: CorrectionKind::SourceContradiction,
            rationale: "corpus text contradicts ticket".into(),
            approval_proof: ApprovalProof {
                kind: "signoff_token".into(),
                value: "tok_correction_1".into(),
                actor_id: "user-owner-01".into(),
                provided_at: None,
            },
        };
        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("source_contradiction"));
        assert!(json.contains("signoff_token"));
        let deserialized: RecallRequestCorrectionParams = serde_json::from_str(&json).unwrap();
        assert_eq!(
            deserialized.correction_kind,
            CorrectionKind::SourceContradiction
        );
        assert_eq!(deserialized.selected_item_ids.len(), 1);
    }

    #[test]
    fn request_correction_result_does_not_create_canonical_state() {
        // RecallRequestCorrectionResult cannot create Decision,
        // BindingEvidence, signoff, compliance, or enforcement state.
        let result = RecallRequestCorrectionResult {
            packet_id: Uuid::nil(),
            outcome: CorrectionRequestOutcome::Accepted,
            correction_candidate_id: Some(Uuid::nil()),
            trace_ref: None,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert!(!json.as_object().unwrap().contains_key("decision_id"));
        assert!(!json.as_object().unwrap().contains_key("signoff_state"));
        assert!(!json.as_object().unwrap().contains_key("compliance_state"));
        assert!(!json.as_object().unwrap().contains_key("binding_evidence"));
        assert!(!json.as_object().unwrap().contains_key("enforcement"));
    }

    #[test]
    fn approval_already_consumed_refusal_constructor() {
        let refusal = RecallRefusal::approval_already_consumed("proof already used");
        assert_eq!(refusal.code, RecallRefusalCode::ApprovalAlreadyConsumed);
        assert_eq!(refusal.detail, "proof already used");
    }

    // -- SearchedScope tests --

    fn make_replayable_scope() -> SearchedScope {
        SearchedScope {
            level: ScopeLevel::Replayable,
            source: ScopeSection::searched(
                vec!["docs/adr".into(), "docs/specs".into()],
                Some("v1".into()),
            ),
            code: ScopeSection::searched(vec!["crates/".into()], Some("abc123".into())),
            graph: ScopeSection::searched(vec!["snapshot-1".into()], Some("gs-1".into())),
            retrieval: ScopeSection::searched(vec!["decision-index".into()], Some("idx-v2".into())),
            advisory: ScopeSection::not_applicable(),
            unknown: ScopeSection::not_applicable(),
        }
    }

    fn make_partial_scope() -> SearchedScope {
        SearchedScope {
            level: ScopeLevel::Enumerated,
            source: ScopeSection::searched(vec!["docs/".into()], Some("v1".into())),
            code: ScopeSection::partial(
                vec!["crates/bicameral-api".into()],
                "only searched one crate out of six",
                Some("abc123".into()),
            ),
            graph: ScopeSection::unavailable("graph store not configured"),
            retrieval: ScopeSection::searched(vec!["decision-index".into()], None),
            advisory: ScopeSection::unsupported("CodeGenome not available"),
            unknown: ScopeSection::not_applicable(),
        }
    }

    #[test]
    fn searched_scope_has_six_sections() {
        let scope = make_replayable_scope();
        assert_eq!(scope.level, ScopeLevel::Replayable);
        // All six sections are accessible
        assert_eq!(scope.source.status, ScopeSectionStatus::Searched);
        assert_eq!(scope.code.status, ScopeSectionStatus::Searched);
        assert_eq!(scope.graph.status, ScopeSectionStatus::Searched);
        assert_eq!(scope.retrieval.status, ScopeSectionStatus::Searched);
        assert_eq!(scope.advisory.status, ScopeSectionStatus::NotApplicable);
        assert_eq!(scope.unknown.status, ScopeSectionStatus::NotApplicable);
    }

    #[test]
    fn scope_level_serializes_to_snake_case() {
        let cases = vec![
            (ScopeLevel::Unscoped, "\"unscoped\""),
            (ScopeLevel::Declared, "\"declared\""),
            (ScopeLevel::Enumerated, "\"enumerated\""),
            (ScopeLevel::Replayable, "\"replayable\""),
            (ScopeLevel::Audited, "\"audited\""),
        ];
        for (variant, expected) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    #[test]
    fn scope_level_ordering_is_monotonic() {
        assert!(ScopeLevel::Unscoped < ScopeLevel::Declared);
        assert!(ScopeLevel::Declared < ScopeLevel::Enumerated);
        assert!(ScopeLevel::Enumerated < ScopeLevel::Replayable);
        assert!(ScopeLevel::Replayable < ScopeLevel::Audited);
    }

    #[test]
    fn scope_section_status_serializes_to_snake_case() {
        let cases = vec![
            (ScopeSectionStatus::Searched, "\"searched\""),
            (ScopeSectionStatus::Partial, "\"partial\""),
            (ScopeSectionStatus::Unavailable, "\"unavailable\""),
            (ScopeSectionStatus::Stale, "\"stale\""),
            (ScopeSectionStatus::Unsupported, "\"unsupported\""),
            (ScopeSectionStatus::NotApplicable, "\"not_applicable\""),
        ];
        for (variant, expected) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    #[test]
    fn positive_candidates_require_enumerated_or_stronger() {
        // Unscoped and declared do not allow positive candidates
        let mut scope = make_replayable_scope();
        scope.level = ScopeLevel::Unscoped;
        assert!(!scope.allows_positive_candidates());

        scope.level = ScopeLevel::Declared;
        assert!(!scope.allows_positive_candidates());

        // Enumerated, replayable, and audited allow positive candidates
        scope.level = ScopeLevel::Enumerated;
        assert!(scope.allows_positive_candidates());

        scope.level = ScopeLevel::Replayable;
        assert!(scope.allows_positive_candidates());

        scope.level = ScopeLevel::Audited;
        assert!(scope.allows_positive_candidates());
    }

    #[test]
    fn no_result_claim_requires_replayable_or_stronger() {
        let mut scope = make_replayable_scope();

        // Unscoped, declared, enumerated cannot make no-result claims
        scope.level = ScopeLevel::Unscoped;
        assert!(!scope.allows_no_result_claim());

        scope.level = ScopeLevel::Declared;
        assert!(!scope.allows_no_result_claim());

        scope.level = ScopeLevel::Enumerated;
        assert!(!scope.allows_no_result_claim());

        // Replayable and audited can make no-result claims
        scope.level = ScopeLevel::Replayable;
        assert!(scope.allows_no_result_claim());

        scope.level = ScopeLevel::Audited;
        assert!(scope.allows_no_result_claim());
    }

    #[test]
    fn partial_scope_produces_visible_caveats() {
        let scope = make_partial_scope();
        assert!(scope.has_caveats());

        let sections = scope.caveat_sections();
        // code (partial), graph (unavailable), advisory (unsupported)
        assert_eq!(sections.len(), 3);

        let names: Vec<&str> = sections.iter().map(|(name, _)| *name).collect();
        assert!(names.contains(&"code"));
        assert!(names.contains(&"graph"));
        assert!(names.contains(&"advisory"));
    }

    #[test]
    fn fully_searched_scope_has_no_caveats() {
        let scope = make_replayable_scope();
        assert!(!scope.has_caveats());
        assert!(scope.caveat_sections().is_empty());
    }

    #[test]
    fn stale_scope_section_produces_caveat() {
        let section = ScopeSection::stale("graph snapshot is 3 days old", Some("gs-old".into()));
        assert!(section.has_caveat());
        assert_eq!(section.status, ScopeSectionStatus::Stale);
        assert_eq!(
            section.caveat.as_deref(),
            Some("graph snapshot is 3 days old")
        );
    }

    #[test]
    fn searched_and_not_applicable_sections_have_no_caveat() {
        let searched = ScopeSection::searched(vec!["target".into()], None);
        assert!(!searched.has_caveat());

        let na = ScopeSection::not_applicable();
        assert!(!na.has_caveat());
    }

    #[test]
    fn no_partial_scope_result_claims_global_absence() {
        // Partial scope (enumerated level) must not allow no-result claims
        let scope = make_partial_scope();
        assert_eq!(scope.level, ScopeLevel::Enumerated);
        assert!(!scope.allows_no_result_claim());

        // Even with caveats, replayable scope allows scoped no-result claims
        let mut replayable_with_caveats = make_replayable_scope();
        replayable_with_caveats.code =
            ScopeSection::partial(vec!["crates/bicameral-api".into()], "only one crate", None);
        replayable_with_caveats.level = ScopeLevel::Replayable;
        assert!(replayable_with_caveats.allows_no_result_claim());
        assert!(replayable_with_caveats.has_caveats());
    }

    #[test]
    fn searched_scope_serializes_and_deserializes() {
        let scope = make_replayable_scope();
        let json = serde_json::to_string(&scope).unwrap();
        let deserialized: SearchedScope = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.level, scope.level);
        assert_eq!(deserialized.source.status, ScopeSectionStatus::Searched);
        assert_eq!(deserialized.source.searched_targets.len(), 2);
    }

    // -- RecallTrace tests --

    fn make_minimal_trace() -> RecallTrace {
        RecallTrace {
            trace_id: "trace-001".into(),
            request_hash: "req-hash-abc".into(),
            normalized_input_hash: "norm-hash-def".into(),
            decision_index_version: "idx-v1".into(),
            metadata_schema_version: "schema-v1".into(),
            taxonomy_version: "tax-v1".into(),
            graph_snapshot_id: Some("gs-abc123".into()),
            source_corpus_version: "corpus-v1".into(),
            semantic_index_version: None,
            codegenome_signal_version: None,
            preference_profile_version: "pref-v1".into(),
            scoring_function_version: "score-v1".into(),
            route_results: vec![
                RouteResult {
                    route_id: "binding_backtrace".into(),
                    status: RouteStatus::Completed,
                    candidate_count: 3,
                    detail: None,
                },
                RouteResult {
                    route_id: "semantic_search".into(),
                    status: RouteStatus::Skipped,
                    candidate_count: 0,
                    detail: Some("semantic index not configured".into()),
                },
            ],
            admitted_signals: vec![RecallSignal {
                kind: "binding_overlap".into(),
                source: "route_a".into(),
                reason: "exact binding match in scope".into(),
            }],
            downgraded_signals: vec![RecallSignal {
                kind: "ownership_match".into(),
                source: "route_e".into(),
                reason: "ownership is ranking signal only".into(),
            }],
            rejected_signals: vec![RecallSignal {
                kind: "codegenome_hint".into(),
                source: "route_f".into(),
                reason: "signal is stale".into(),
            }],
            final_rank_tuples: vec![RankTuple {
                result_id: "decision-1".into(),
                tier: 1,
                score: 0.95,
                contributing_signals: vec!["binding_overlap".into()],
            }],
            stop_conditions: vec![StopCondition {
                kind: StopConditionKind::AllRoutesCompleted,
                detail: "all configured routes finished".into(),
            }],
            replay_available: true,
            replay_unavailable_reason: None,
        }
    }

    #[test]
    fn recall_trace_serializes_and_deserializes() {
        let trace = make_minimal_trace();
        let json = serde_json::to_string(&trace).unwrap();
        let deserialized: RecallTrace = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.trace_id, "trace-001");
        assert_eq!(deserialized.route_results.len(), 2);
        assert_eq!(deserialized.admitted_signals.len(), 1);
        assert_eq!(deserialized.downgraded_signals.len(), 1);
        assert_eq!(deserialized.rejected_signals.len(), 1);
        assert_eq!(deserialized.final_rank_tuples.len(), 1);
        assert!(deserialized.replay_available);
    }

    #[test]
    fn recall_trace_records_route_results() {
        let trace = make_minimal_trace();
        let binding_route = &trace.route_results[0];
        assert_eq!(binding_route.route_id, "binding_backtrace");
        assert_eq!(binding_route.status, RouteStatus::Completed);
        assert_eq!(binding_route.candidate_count, 3);

        let semantic_route = &trace.route_results[1];
        assert_eq!(semantic_route.route_id, "semantic_search");
        assert_eq!(semantic_route.status, RouteStatus::Skipped);
        assert_eq!(semantic_route.candidate_count, 0);
    }

    #[test]
    fn recall_trace_separates_admitted_downgraded_rejected_signals() {
        let trace = make_minimal_trace();

        assert_eq!(trace.admitted_signals[0].kind, "binding_overlap");
        assert_eq!(trace.downgraded_signals[0].kind, "ownership_match");
        assert_eq!(trace.rejected_signals[0].kind, "codegenome_hint");
        assert_eq!(trace.rejected_signals[0].reason, "signal is stale");
    }

    #[test]
    fn recall_trace_rank_tuples_carry_tier_and_score() {
        let trace = make_minimal_trace();
        let rank = &trace.final_rank_tuples[0];
        assert_eq!(rank.tier, 1);
        assert!((rank.score - 0.95).abs() < f64::EPSILON);
        assert_eq!(rank.contributing_signals, vec!["binding_overlap"]);
    }

    #[test]
    fn recall_trace_replay_unavailable_carries_reason() {
        let mut trace = make_minimal_trace();
        trace.replay_available = false;
        trace.replay_unavailable_reason =
            Some("semantic index was rebuilt since trace was recorded".into());

        let json = serde_json::to_string(&trace).unwrap();
        let deserialized: RecallTrace = serde_json::from_str(&json).unwrap();
        assert!(!deserialized.replay_available);
        assert!(deserialized
            .replay_unavailable_reason
            .unwrap()
            .contains("rebuilt"));
    }

    #[test]
    fn route_status_serializes_to_snake_case() {
        let cases = vec![
            (RouteStatus::Completed, "\"completed\""),
            (RouteStatus::Empty, "\"empty\""),
            (RouteStatus::Skipped, "\"skipped\""),
            (RouteStatus::Failed, "\"failed\""),
        ];
        for (variant, expected) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    #[test]
    fn stop_condition_kind_serializes_to_snake_case() {
        let cases = vec![
            (StopConditionKind::BudgetExhausted, "\"budget_exhausted\""),
            (
                StopConditionKind::AllRoutesCompleted,
                "\"all_routes_completed\"",
            ),
            (
                StopConditionKind::SufficientResults,
                "\"sufficient_results\"",
            ),
            (StopConditionKind::ScopeBoundary, "\"scope_boundary\""),
            (StopConditionKind::Timeout, "\"timeout\""),
        ];
        for (variant, expected) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    // -- NoResultLanguage tests --

    #[test]
    fn no_result_language_from_unscoped_is_refused() {
        let mut scope = make_replayable_scope();
        scope.level = ScopeLevel::Unscoped;
        let nrl = NoResultLanguage::from_scope(scope);
        assert_eq!(nrl.permitted_claim, NoResultClaim::Refused);
    }

    #[test]
    fn no_result_language_from_declared_is_declared_only() {
        let mut scope = make_replayable_scope();
        scope.level = ScopeLevel::Declared;
        let nrl = NoResultLanguage::from_scope(scope);
        assert_eq!(nrl.permitted_claim, NoResultClaim::DeclaredScopeOnly);
    }

    #[test]
    fn no_result_language_from_enumerated_is_enumerated_only() {
        let mut scope = make_replayable_scope();
        scope.level = ScopeLevel::Enumerated;
        let nrl = NoResultLanguage::from_scope(scope);
        assert_eq!(nrl.permitted_claim, NoResultClaim::EnumeratedScopeOnly);
    }

    #[test]
    fn no_result_language_from_replayable_permits_searched_scope_claim() {
        let scope = make_replayable_scope();
        let nrl = NoResultLanguage::from_scope(scope);
        assert_eq!(
            nrl.permitted_claim,
            NoResultClaim::NoRelevantDecisionInSearchedScope
        );
        assert!(!nrl.has_scope_caveats);
    }

    #[test]
    fn no_result_language_from_audited_permits_searched_scope_claim() {
        let mut scope = make_replayable_scope();
        scope.level = ScopeLevel::Audited;
        let nrl = NoResultLanguage::from_scope(scope);
        assert_eq!(
            nrl.permitted_claim,
            NoResultClaim::NoRelevantDecisionInSearchedScope
        );
    }

    #[test]
    fn no_result_language_surfaces_scope_caveats() {
        let scope = make_partial_scope();
        let nrl = NoResultLanguage::from_scope(scope);
        assert!(nrl.has_scope_caveats);
        assert_eq!(nrl.permitted_claim, NoResultClaim::EnumeratedScopeOnly);
    }

    #[test]
    fn no_result_claim_serializes_to_snake_case() {
        let cases = vec![
            (NoResultClaim::Refused, "\"refused\""),
            (NoResultClaim::DeclaredScopeOnly, "\"declared_scope_only\""),
            (
                NoResultClaim::EnumeratedScopeOnly,
                "\"enumerated_scope_only\"",
            ),
            (
                NoResultClaim::NoRelevantDecisionInSearchedScope,
                "\"no_relevant_decision_in_searched_scope\"",
            ),
        ];
        for (variant, expected) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    // -- RecallPacket integration with SearchedScope and RecallTrace --

    #[test]
    fn recall_packet_with_searched_scope_serializes() {
        let mut packet = RecallPacket::new_alpha();
        packet.searched_scope = Some(make_replayable_scope());
        packet.recall_trace = Some(make_minimal_trace());

        let json = serde_json::to_string(&packet).unwrap();
        let deserialized: RecallPacket = serde_json::from_str(&json).unwrap();
        assert!(deserialized.searched_scope.is_some());
        assert!(deserialized.recall_trace.is_some());

        let scope = deserialized.searched_scope.unwrap();
        assert_eq!(scope.level, ScopeLevel::Replayable);

        let trace = deserialized.recall_trace.unwrap();
        assert_eq!(trace.trace_id, "trace-001");
    }

    #[test]
    fn recall_packet_without_scope_omits_fields_in_json() {
        let packet = RecallPacket::new_alpha();
        let json = serde_json::to_string(&packet).unwrap();
        assert!(!json.contains("searched_scope"));
        assert!(!json.contains("recall_trace"));
    }

    #[test]
    fn recall_packet_deserializes_from_legacy_json_without_scope() {
        // Packets serialized before SearchedScope was added should still
        // deserialize successfully with searched_scope and recall_trace as None.
        let legacy_json = serde_json::json!({
            "packet_id": "00000000-0000-0000-0000-000000000000",
            "canonical_decisions": [],
            "decision_candidates": [],
            "unknown_scope": [],
            "allowed_next_actions": []
        });
        let packet: RecallPacket = serde_json::from_value(legacy_json).unwrap();
        assert!(packet.searched_scope.is_none());
        assert!(packet.recall_trace.is_none());
    }

    // -----------------------------------------------------------------------
    // RecallPreferenceProfile tests
    // -----------------------------------------------------------------------

    fn make_test_profile(
        name: &str,
        scope: PreferenceScope,
        parent: Option<Uuid>,
    ) -> RecallPreferenceProfile {
        let now = chrono::Utc::now();
        RecallPreferenceProfile {
            profile_id: Uuid::new_v4(),
            name: name.into(),
            version: 1,
            parent_profile_id: parent,
            scope,
            route_weights: Vec::new(),
            tier_weights: Vec::new(),
            source_class_adjustments: Vec::new(),
            notification_overrides: Vec::new(),
            created_at: now,
            updated_at: now,
            actor_id: "test-actor".into(),
        }
    }

    #[test]
    fn preference_profile_shape_serializes_and_deserializes() {
        let profile = make_test_profile("default", PreferenceScope::Policy, None);
        let json = serde_json::to_string(&profile).unwrap();
        let deserialized: RecallPreferenceProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.profile_id, profile.profile_id);
        assert_eq!(deserialized.name, "default");
        assert_eq!(deserialized.version, 1);
        assert_eq!(deserialized.scope, PreferenceScope::Policy);
        assert!(deserialized.parent_profile_id.is_none());
    }

    #[test]
    fn preference_scope_serializes_to_snake_case() {
        assert_eq!(
            serde_json::to_string(&PreferenceScope::Policy).unwrap(),
            "\"policy\""
        );
        assert_eq!(
            serde_json::to_string(&PreferenceScope::Workspace).unwrap(),
            "\"workspace\""
        );
        assert_eq!(
            serde_json::to_string(&PreferenceScope::User).unwrap(),
            "\"user\""
        );
    }

    #[test]
    fn recall_route_serializes_to_snake_case() {
        let cases = vec![
            (RecallRoute::BindingBacktrace, "\"binding_backtrace\""),
            (RecallRoute::GraphOverlap, "\"graph_overlap\""),
            (RecallRoute::StructuredCorpus, "\"structured_corpus\""),
            (RecallRoute::SemanticCorpus, "\"semantic_corpus\""),
            (RecallRoute::OwnershipOverlap, "\"ownership_overlap\""),
            (RecallRoute::CodegenomeAdvisory, "\"codegenome_advisory\""),
        ];
        for (variant, expected) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    #[test]
    fn ranking_tier_serializes_to_snake_case() {
        let cases = vec![
            (
                RankingTier::AcceptedBindingOverlap,
                "\"accepted_binding_overlap\"",
            ),
            (
                RankingTier::VerifiedGraphOverlap,
                "\"verified_graph_overlap\"",
            ),
            (RankingTier::ProjectedImpact, "\"projected_impact\""),
            (
                RankingTier::DecisionSourceOverlap,
                "\"decision_source_overlap\"",
            ),
            (RankingTier::OwnershipOverlap, "\"ownership_overlap\""),
            (RankingTier::SemanticSimilarity, "\"semantic_similarity\""),
            (RankingTier::CodegenomeHints, "\"codegenome_hints\""),
            (
                RankingTier::ApproximateCandidates,
                "\"approximate_candidates\"",
            ),
        ];
        for (variant, expected) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    #[test]
    fn route_weight_clamped_to_cap() {
        let mut profile = make_test_profile("test", PreferenceScope::User, None);
        profile.route_weights.push(RouteWeightOverride {
            route: RecallRoute::SemanticCorpus,
            weight: 5.0, // exceeds cap of 2.0
        });
        let violations = profile.clamp_to_policy_caps();
        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].violation_kind,
            PreferenceCapViolationKind::RouteWeightAboveCap
        );
        assert!((profile.route_weights[0].weight - ROUTE_WEIGHT_CAP).abs() < f64::EPSILON);
    }

    #[test]
    fn route_weight_clamped_to_floor() {
        let mut profile = make_test_profile("test", PreferenceScope::User, None);
        profile.route_weights.push(RouteWeightOverride {
            route: RecallRoute::BindingBacktrace,
            weight: 0.0, // below floor of 0.1
        });
        let violations = profile.clamp_to_policy_caps();
        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].violation_kind,
            PreferenceCapViolationKind::RouteWeightBelowFloor
        );
        assert!((profile.route_weights[0].weight - ROUTE_WEIGHT_FLOOR).abs() < f64::EPSILON);
    }

    #[test]
    fn tier_weight_clamped_to_cap() {
        let mut profile = make_test_profile("test", PreferenceScope::User, None);
        profile.tier_weights.push(TierWeightOverride {
            tier: RankingTier::SemanticSimilarity,
            weight: 10.0, // exceeds cap of 3.0
        });
        let violations = profile.clamp_to_policy_caps();
        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].violation_kind,
            PreferenceCapViolationKind::TierWeightAboveCap
        );
        assert!((profile.tier_weights[0].weight - TIER_WEIGHT_CAP).abs() < f64::EPSILON);
    }

    #[test]
    fn tier_weight_clamped_to_floor() {
        let mut profile = make_test_profile("test", PreferenceScope::User, None);
        profile.tier_weights.push(TierWeightOverride {
            tier: RankingTier::AcceptedBindingOverlap,
            weight: 0.01, // below floor of 0.1
        });
        let violations = profile.clamp_to_policy_caps();
        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].violation_kind,
            PreferenceCapViolationKind::TierWeightBelowFloor
        );
        assert!((profile.tier_weights[0].weight - TIER_WEIGHT_FLOOR).abs() < f64::EPSILON);
    }

    #[test]
    fn source_class_adjustment_clamped_to_cap() {
        let mut profile = make_test_profile("test", PreferenceScope::User, None);
        profile
            .source_class_adjustments
            .push(SourceClassAdjustment {
                source_class: "slack".into(),
                adjustment: 2.0, // exceeds cap of 1.0
            });
        let violations = profile.clamp_to_policy_caps();
        assert_eq!(violations.len(), 1);
        assert!(
            (profile.source_class_adjustments[0].adjustment - SOURCE_CLASS_ADJUSTMENT_CAP).abs()
                < f64::EPSILON
        );
    }

    #[test]
    fn source_class_negative_adjustment_clamped_to_floor() {
        let mut profile = make_test_profile("test", PreferenceScope::User, None);
        profile
            .source_class_adjustments
            .push(SourceClassAdjustment {
                source_class: "linear".into(),
                adjustment: -3.0, // below -cap
            });
        let violations = profile.clamp_to_policy_caps();
        assert_eq!(violations.len(), 1);
        assert!(
            (profile.source_class_adjustments[0].adjustment - (-SOURCE_CLASS_ADJUSTMENT_CAP)).abs()
                < f64::EPSILON
        );
    }

    #[test]
    fn valid_weights_produce_no_violations() {
        let mut profile = make_test_profile("test", PreferenceScope::User, None);
        profile.route_weights.push(RouteWeightOverride {
            route: RecallRoute::SemanticCorpus,
            weight: 1.5,
        });
        profile.tier_weights.push(TierWeightOverride {
            tier: RankingTier::SemanticSimilarity,
            weight: 2.0,
        });
        profile
            .source_class_adjustments
            .push(SourceClassAdjustment {
                source_class: "slack".into(),
                adjustment: 0.5,
            });
        let violations = profile.clamp_to_policy_caps();
        assert!(violations.is_empty());
    }

    #[test]
    fn duplicate_route_entries_detected() {
        let mut profile = make_test_profile("test", PreferenceScope::User, None);
        profile.route_weights.push(RouteWeightOverride {
            route: RecallRoute::SemanticCorpus,
            weight: 1.0,
        });
        profile.route_weights.push(RouteWeightOverride {
            route: RecallRoute::SemanticCorpus,
            weight: 1.5,
        });
        let violations = profile.validate_no_duplicates();
        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].violation_kind,
            PreferenceCapViolationKind::DuplicateRouteEntry
        );
    }

    #[test]
    fn duplicate_tier_entries_detected() {
        let mut profile = make_test_profile("test", PreferenceScope::User, None);
        profile.tier_weights.push(TierWeightOverride {
            tier: RankingTier::SemanticSimilarity,
            weight: 1.0,
        });
        profile.tier_weights.push(TierWeightOverride {
            tier: RankingTier::SemanticSimilarity,
            weight: 2.0,
        });
        let violations = profile.validate_no_duplicates();
        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].violation_kind,
            PreferenceCapViolationKind::DuplicateTierEntry
        );
    }

    // -- Inheritance and conflict resolution tests --

    #[test]
    fn inheritance_child_overrides_parent_route_weight() {
        let parent = {
            let mut p = make_test_profile("parent", PreferenceScope::Policy, None);
            p.route_weights.push(RouteWeightOverride {
                route: RecallRoute::SemanticCorpus,
                weight: 0.5,
            });
            p.route_weights.push(RouteWeightOverride {
                route: RecallRoute::BindingBacktrace,
                weight: 1.0,
            });
            p
        };

        let child = {
            let mut c = make_test_profile("child", PreferenceScope::User, Some(parent.profile_id));
            c.route_weights.push(RouteWeightOverride {
                route: RecallRoute::SemanticCorpus,
                weight: 1.8, // override parent's 0.5
            });
            c
        };

        let resolved = resolve_preference_chain(&[parent.clone(), child]);
        // SemanticCorpus should be the child's value (1.8)
        let semantic = resolved
            .route_weights
            .iter()
            .find(|rw| rw.route == RecallRoute::SemanticCorpus)
            .unwrap();
        assert!((semantic.weight - 1.8).abs() < f64::EPSILON);

        // BindingBacktrace should be inherited from parent (1.0)
        let binding = resolved
            .route_weights
            .iter()
            .find(|rw| rw.route == RecallRoute::BindingBacktrace)
            .unwrap();
        assert!((binding.weight - 1.0).abs() < f64::EPSILON);

        // Lineage should contain both profile IDs
        assert_eq!(resolved.lineage.len(), 2);
        assert_eq!(resolved.lineage[0], parent.profile_id);
    }

    #[test]
    fn policy_caps_applied_after_inheritance() {
        let policy = {
            let mut p = make_test_profile("policy", PreferenceScope::Policy, None);
            p.route_weights.push(RouteWeightOverride {
                route: RecallRoute::SemanticCorpus,
                weight: 1.0,
            });
            p
        };

        let user = {
            let mut u = make_test_profile("user", PreferenceScope::User, None);
            u.route_weights.push(RouteWeightOverride {
                route: RecallRoute::SemanticCorpus,
                weight: 5.0, // exceeds cap — should be clamped after merge
            });
            u
        };

        let resolved = resolve_preference_chain(&[policy, user]);
        let semantic = resolved
            .route_weights
            .iter()
            .find(|rw| rw.route == RecallRoute::SemanticCorpus)
            .unwrap();
        // Should be clamped to ROUTE_WEIGHT_CAP
        assert!((semantic.weight - ROUTE_WEIGHT_CAP).abs() < f64::EPSILON);
        assert_eq!(resolved.clamped_violations.len(), 1);
    }

    #[test]
    fn empty_chain_produces_empty_resolved_profile() {
        let resolved = resolve_preference_chain(&[]);
        assert!(resolved.route_weights.is_empty());
        assert!(resolved.tier_weights.is_empty());
        assert!(resolved.source_class_adjustments.is_empty());
        assert!(resolved.notification_overrides.is_empty());
        assert!(resolved.lineage.is_empty());
        assert_eq!(resolved.version_tag, "empty");
    }

    #[test]
    fn three_level_inheritance_policy_workspace_user() {
        let policy = {
            let mut p = make_test_profile("policy", PreferenceScope::Policy, None);
            p.route_weights.push(RouteWeightOverride {
                route: RecallRoute::BindingBacktrace,
                weight: 1.0,
            });
            p.tier_weights.push(TierWeightOverride {
                tier: RankingTier::AcceptedBindingOverlap,
                weight: 1.0,
            });
            p
        };

        let workspace = {
            let mut w = make_test_profile("workspace", PreferenceScope::Workspace, None);
            w.source_class_adjustments.push(SourceClassAdjustment {
                source_class: "slack".into(),
                adjustment: 0.5,
            });
            w
        };

        let user = {
            let mut u = make_test_profile("user", PreferenceScope::User, None);
            u.route_weights.push(RouteWeightOverride {
                route: RecallRoute::BindingBacktrace,
                weight: 1.5, // override policy
            });
            u.source_class_adjustments.push(SourceClassAdjustment {
                source_class: "slack".into(),
                adjustment: 0.8, // override workspace
            });
            u
        };

        let resolved = resolve_preference_chain(&[policy, workspace, user]);

        // Route: user override wins
        let binding = resolved
            .route_weights
            .iter()
            .find(|rw| rw.route == RecallRoute::BindingBacktrace)
            .unwrap();
        assert!((binding.weight - 1.5).abs() < f64::EPSILON);

        // Tier: only policy set it, so it persists
        let tier = resolved
            .tier_weights
            .iter()
            .find(|tw| tw.tier == RankingTier::AcceptedBindingOverlap)
            .unwrap();
        assert!((tier.weight - 1.0).abs() < f64::EPSILON);

        // Source: user override wins over workspace
        let slack = resolved
            .source_class_adjustments
            .iter()
            .find(|sa| sa.source_class == "slack")
            .unwrap();
        assert!((slack.adjustment - 0.8).abs() < f64::EPSILON);

        assert_eq!(resolved.lineage.len(), 3);
        assert!(resolved.clamped_violations.is_empty());
    }

    // -- Audit record tests --

    #[test]
    fn audit_record_serializes_and_deserializes() {
        let record = PreferenceAuditRecord {
            audit_id: Uuid::new_v4(),
            profile_id: Uuid::new_v4(),
            new_version: 2,
            previous_version: Some(1),
            change_kind: PreferenceChangeKind::Updated,
            field_changes: vec![PreferenceFieldChange {
                field_path: "route_weights.semantic_corpus".into(),
                old_value: Some("1.0".into()),
                new_value: "1.5".into(),
            }],
            violations: vec![],
            actor_id: "user-1".into(),
            changed_at: chrono::Utc::now(),
            trace_ref: Some("trace-001".into()),
        };
        let json = serde_json::to_string(&record).unwrap();
        let deserialized: PreferenceAuditRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.audit_id, record.audit_id);
        assert_eq!(deserialized.new_version, 2);
        assert_eq!(deserialized.previous_version, Some(1));
        assert_eq!(deserialized.change_kind, PreferenceChangeKind::Updated);
        assert_eq!(deserialized.field_changes.len(), 1);
    }

    #[test]
    fn audit_record_creation_has_no_previous_version() {
        let record = PreferenceAuditRecord {
            audit_id: Uuid::new_v4(),
            profile_id: Uuid::new_v4(),
            new_version: 1,
            previous_version: None,
            change_kind: PreferenceChangeKind::Created,
            field_changes: vec![],
            violations: vec![],
            actor_id: "user-1".into(),
            changed_at: chrono::Utc::now(),
            trace_ref: None,
        };
        let json = serde_json::to_value(&record).unwrap();
        // previous_version should be absent (skip_serializing_if)
        assert!(!json.as_object().unwrap().contains_key("previous_version"));
    }

    #[test]
    fn preference_change_kind_serializes() {
        assert_eq!(
            serde_json::to_string(&PreferenceChangeKind::Created).unwrap(),
            "\"created\""
        );
        assert_eq!(
            serde_json::to_string(&PreferenceChangeKind::Updated).unwrap(),
            "\"updated\""
        );
        assert_eq!(
            serde_json::to_string(&PreferenceChangeKind::Deactivated).unwrap(),
            "\"deactivated\""
        );
        assert_eq!(
            serde_json::to_string(&PreferenceChangeKind::InheritanceChanged).unwrap(),
            "\"inheritance_changed\""
        );
    }

    #[test]
    fn forbidden_preference_action_all_returns_six_items() {
        assert_eq!(ForbiddenPreferenceAction::all().len(), 6);
    }

    #[test]
    fn forbidden_preference_action_serializes() {
        assert_eq!(
            serde_json::to_string(&ForbiddenPreferenceAction::CreateAuthority).unwrap(),
            "\"create_authority\""
        );
        assert_eq!(
            serde_json::to_string(&ForbiddenPreferenceAction::HideAcceptedEvidence).unwrap(),
            "\"hide_accepted_evidence\""
        );
    }

    // -- Safety: preferences cannot create authority --

    #[test]
    fn preference_profile_cannot_carry_governance_state() {
        let profile = make_test_profile("test", PreferenceScope::User, None);
        let json = serde_json::to_value(&profile).unwrap();
        let obj = json.as_object().unwrap();
        // RecallPreferenceProfile must not carry fields that could create
        // authority, signoff, compliance, binding evidence, or enforcement.
        assert!(!obj.contains_key("decision_id"));
        assert!(!obj.contains_key("signoff_state"));
        assert!(!obj.contains_key("compliance_state"));
        assert!(!obj.contains_key("binding_evidence"));
        assert!(!obj.contains_key("enforcement"));
        assert!(!obj.contains_key("approval_proof"));
    }

    #[test]
    fn resolved_profile_cannot_carry_governance_state() {
        let resolved = resolve_preference_chain(&[]);
        let json = serde_json::to_value(&resolved).unwrap();
        let obj = json.as_object().unwrap();
        assert!(!obj.contains_key("decision_id"));
        assert!(!obj.contains_key("signoff_state"));
        assert!(!obj.contains_key("compliance_state"));
        assert!(!obj.contains_key("binding_evidence"));
        assert!(!obj.contains_key("enforcement"));
    }

    #[test]
    fn preference_cap_violation_kind_serializes() {
        let cases = vec![
            (
                PreferenceCapViolationKind::RouteWeightAboveCap,
                "\"route_weight_above_cap\"",
            ),
            (
                PreferenceCapViolationKind::RouteWeightBelowFloor,
                "\"route_weight_below_floor\"",
            ),
            (
                PreferenceCapViolationKind::TierWeightAboveCap,
                "\"tier_weight_above_cap\"",
            ),
            (
                PreferenceCapViolationKind::TierWeightBelowFloor,
                "\"tier_weight_below_floor\"",
            ),
            (
                PreferenceCapViolationKind::SourceClassAboveCap,
                "\"source_class_above_cap\"",
            ),
            (
                PreferenceCapViolationKind::ForbiddenAction,
                "\"forbidden_action\"",
            ),
            (
                PreferenceCapViolationKind::DuplicateRouteEntry,
                "\"duplicate_route_entry\"",
            ),
            (
                PreferenceCapViolationKind::DuplicateTierEntry,
                "\"duplicate_tier_entry\"",
            ),
        ];
        for (variant, expected) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    #[test]
    fn conflict_resolution_serializes() {
        assert_eq!(
            serde_json::to_string(&ConflictResolution::ChildOverridesParent).unwrap(),
            "\"child_overrides_parent\""
        );
        assert_eq!(
            serde_json::to_string(&ConflictResolution::MostRestrictiveWins).unwrap(),
            "\"most_restrictive_wins\""
        );
    }

    #[test]
    fn notification_category_serializes() {
        let cases = vec![
            (
                NotificationCategory::GovernanceOverlap,
                "\"governance_overlap\"",
            ),
            (
                NotificationCategory::BindingStaleness,
                "\"binding_staleness\"",
            ),
            (NotificationCategory::UnknownScope, "\"unknown_scope\""),
            (NotificationCategory::NewCandidates, "\"new_candidates\""),
        ];
        for (variant, expected) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    #[test]
    fn notification_priority_serializes() {
        assert_eq!(
            serde_json::to_string(&NotificationPriority::High).unwrap(),
            "\"high\""
        );
        assert_eq!(
            serde_json::to_string(&NotificationPriority::Normal).unwrap(),
            "\"normal\""
        );
        assert_eq!(
            serde_json::to_string(&NotificationPriority::Low).unwrap(),
            "\"low\""
        );
    }

    #[test]
    fn resolved_profile_version_tag_uses_last_profile() {
        let profile = make_test_profile("test", PreferenceScope::User, None);
        let expected_tag = format!("{}@v{}", profile.profile_id, profile.version);
        let resolved = resolve_preference_chain(&[profile]);
        assert_eq!(resolved.version_tag, expected_tag);
    }

    #[test]
    fn provider_status_serializes_snake_case() {
        let json = serde_json::to_string(&RecallProviderStatus::NotConfigured).unwrap();
        assert_eq!(json, "\"not_configured\"");
        assert_eq!(
            serde_json::from_str::<RecallProviderStatus>("\"indexing\"").unwrap(),
            RecallProviderStatus::Indexing
        );
    }

    #[test]
    fn provider_request_derives_searched_scope_from_workspace_and_snapshot() {
        let request = RecallSignalRequest {
            query: Some("checkout flow".to_string()),
            workspace_id: Some("workspace-a".to_string()),
            graph_snapshot_id: Some("graph-123".to_string()),
            commit_sha: Some("sha-ignored".to_string()),
            ref_name: None,
            files: vec!["src/checkout.rs".to_string()],
            symbols: vec!["Checkout".to_string()],
            changed_files: Vec::new(),
            source_evidence_ids: Vec::new(),
            decision_id: None,
            candidate_id: None,
            limit: 10,
            scope_kind: Some(RecallScopeKind::FileSet),
        };

        let searched = request.searched_scope();
        assert_eq!(searched.scope_kind, RecallScopeKind::FileSet);
        assert_eq!(searched.corpus_id.as_deref(), Some("workspace-a"));
        assert_eq!(searched.corpus_version.as_deref(), Some("graph-123"));
        assert_eq!(searched.files, vec!["src/checkout.rs"]);
        assert_eq!(searched.symbols, vec!["Checkout"]);
    }

    #[test]
    fn semantic_candidate_carries_provenance_and_currentness() {
        let candidate = SemanticCodeCandidate {
            file: "src/lib.rs".to_string(),
            range: Some("1-10".to_string()),
            symbol: Some("run".to_string()),
            chunk_id: Some("chunk-1".to_string()),
            content_hash: Some("hash-1".to_string()),
            target_type: SemanticCodeTargetType::Symbol,
            score: 0.82,
            score_kind: SemanticScoreKind::SemanticSimilarity,
            reasons: vec!["embedding_match".to_string()],
            matched_terms: vec!["run".to_string()],
            matched_excerpt: Some("fn run()".to_string()),
            currentness: CodeSourceCurrentness::GraphUnavailable,
            provenance: RecallProviderProvenance {
                provider_id: "semantic-test".to_string(),
                provider_version: "1".to_string(),
                index_version: Some("idx-1".to_string()),
                overlay_id: None,
                model_id: Some("model".to_string()),
                model_version: Some("v1".to_string()),
            },
        };

        let value = serde_json::to_value(candidate).unwrap();
        assert_eq!(value["provenance"]["provider_id"], "semantic-test");
        assert_eq!(value["currentness"], "graph_unavailable");
        assert_eq!(value["score_kind"], "semantic_similarity");
    }
}
