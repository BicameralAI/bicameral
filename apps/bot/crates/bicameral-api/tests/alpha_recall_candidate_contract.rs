//! Canonical alpha-recall DecisionCandidate + decision-attention contract
//! (issue #827).
//!
//! Proves the daemon-owned product packet carries reviewable candidates with
//! the informed-review fields the issue enumerates, a typed decision-attention
//! signal the consumer reads rather than infers, and a producer/consumer drift
//! failure that is explicit and typed. Deterministic — no network, no graph.

use bicameral_api::alpha_recall::{AlphaRecallMismatchKind, ALPHA_RECALL_ENVELOPE_VERSION};
use bicameral_api::candidate::{
    CandidateState, DecisionCandidate, DecisionLevel, ExtractionConfidence, ReviewLifecycleState,
};
use bicameral_api::recall::{
    CandidateEvidenceFreshness, CandidateRelevance, DecisionAttention, DecisionAttentionReason,
    DecisionAttentionState, PromotionOutcome, RecallPacket, SurfacedDecisionCandidate,
};
use uuid::Uuid;

fn proposed_candidate() -> DecisionCandidate {
    let mut candidate = DecisionCandidate::new(
        "Rate limit public API before launch".into(),
        "Public API endpoints must enforce a per-client rate limit before launch.".into(),
        DecisionLevel::Architecture,
        "https://linear.app/team/issue/PROJ-101".into(),
    );
    candidate.extraction_confidence = ExtractionConfidence::Medium;
    candidate.extracted_by = "linear".into();
    candidate
}

/// The full informed-review field set the issue enumerates is present on a
/// surfaced proposed candidate.
#[test]
fn surfaced_candidate_carries_informed_review_fields() {
    let candidate = proposed_candidate();
    let surfaced = SurfacedDecisionCandidate::from_candidate(
        &candidate,
        vec![CandidateRelevance::SourceTextMatch],
        Vec::new(),
    );

    assert_eq!(surfaced.candidate_id, candidate.id);
    assert_eq!(surfaced.title, candidate.title);
    assert_eq!(surfaced.summary, candidate.description);
    assert_eq!(surfaced.origin_surface, "linear");
    assert_eq!(surfaced.extraction_confidence, ExtractionConfidence::Medium);
    assert_eq!(
        surfaced.relevance_reasons,
        vec![CandidateRelevance::SourceTextMatch]
    );
    assert_eq!(
        surfaced.evidence_freshness,
        CandidateEvidenceFreshness::Current
    );
    assert_eq!(surfaced.review_state, ReviewLifecycleState::Proposed);
    assert_eq!(surfaced.candidate_state, CandidateState::Proposed);
    assert!(!surfaced.source_refs.is_empty());
    // Non-Medium/High confidence would add an ambiguity note; Medium does not.
    assert!(surfaced.ambiguities.is_empty());
}

/// Supersession and keep-both outcomes are offered only when a related
/// canonical Decision exists to act against.
#[test]
fn allowed_outcomes_gate_supersession_on_related_decision() {
    let candidate = proposed_candidate();

    let without = SurfacedDecisionCandidate::from_candidate(&candidate, Vec::new(), Vec::new());
    assert_eq!(
        without.allowed_outcomes,
        vec![
            PromotionOutcome::NewConstraint,
            PromotionOutcome::NeedsSignoffReview,
            PromotionOutcome::DismissedAsNoise,
        ],
        "no related decision: supersession/keep-both must not be offered"
    );

    let related = vec![Uuid::new_v4()];
    let with = SurfacedDecisionCandidate::from_candidate(&candidate, Vec::new(), related.clone());
    assert_eq!(with.related_decision_refs, related);
    assert!(with
        .allowed_outcomes
        .contains(&PromotionOutcome::SupersedesExistingDecision));
    assert!(with
        .allowed_outcomes
        .contains(&PromotionOutcome::KeepsBothWithScope));
}

/// Low extraction confidence and recorded metadata ambiguities surface as
/// explicit ambiguity, driving the attention reason.
#[test]
fn ambiguity_is_surfaced_and_drives_attention_reason() {
    let mut candidate = proposed_candidate();
    candidate.extraction_confidence = ExtractionConfidence::Low;
    candidate.metadata = serde_json::json!({
        "ambiguities": ["Scope of rate limit is unclear", ""]
    });

    let surfaced = SurfacedDecisionCandidate::from_candidate(
        &candidate,
        vec![CandidateRelevance::SourceTextMatch],
        Vec::new(),
    );
    // Empty metadata entries are dropped; low confidence adds its own note.
    assert_eq!(surfaced.ambiguities.len(), 2);
    assert!(surfaced
        .ambiguities
        .iter()
        .any(|a| a.contains("Scope of rate limit is unclear")));

    let attention = DecisionAttention::from_surfaced_candidates(&[surfaced]);
    assert_eq!(attention.state, DecisionAttentionState::Warranted);
    assert_eq!(
        attention.reason,
        DecisionAttentionReason::CandidateAmbiguity
    );
}

/// A stale-source-pending candidate is reported with stale evidence freshness.
#[test]
fn stale_source_candidate_reports_stale_freshness() {
    let mut candidate = proposed_candidate();
    candidate.review_state = ReviewLifecycleState::StaleSourcePending;

    let surfaced = SurfacedDecisionCandidate::from_candidate(&candidate, Vec::new(), Vec::new());
    assert_eq!(
        surfaced.evidence_freshness,
        CandidateEvidenceFreshness::Stale
    );
    assert_eq!(
        surfaced.review_state,
        ReviewLifecycleState::StaleSourcePending
    );
}

/// Attention is warranted (non-ambiguous reason) when reviewable candidates are
/// present without ambiguity.
#[test]
fn attention_warranted_for_clean_reviewable_candidate() {
    let candidate = proposed_candidate();
    let surfaced = SurfacedDecisionCandidate::from_candidate(
        &candidate,
        vec![CandidateRelevance::SourceTextMatch],
        Vec::new(),
    );
    let attention = DecisionAttention::from_surfaced_candidates(&[surfaced]);
    assert_eq!(attention.state, DecisionAttentionState::Warranted);
    assert_eq!(
        attention.reason,
        DecisionAttentionReason::ReviewableCandidatesPresent
    );
    assert!(!attention.detail.is_empty());
}

/// With no candidates, the packet carries an explicit "not warranted" signal —
/// absence is a typed state, not a missing field the consumer must infer.
#[test]
fn absent_candidates_yield_explicit_not_warranted_signal() {
    let packet = RecallPacket::new_alpha().with_derived_decision_attention();
    assert!(packet.decision_candidates.is_empty());
    assert_eq!(
        packet.decision_attention.state,
        DecisionAttentionState::NotWarranted
    );
    assert_eq!(
        packet.decision_attention.reason,
        DecisionAttentionReason::NoReviewableCandidates
    );

    // The signal is present in the serialized wire form (explicit, not omitted).
    let value = serde_json::to_value(&packet).unwrap();
    assert_eq!(
        value["decision_attention"]["state"],
        serde_json::json!("not_warranted")
    );
}

/// The decision-attention signal round-trips through JSON as a typed shape.
#[test]
fn decision_attention_round_trips_typed() {
    let candidate = proposed_candidate();
    let surfaced = SurfacedDecisionCandidate::from_candidate(&candidate, Vec::new(), Vec::new());
    let mut packet = RecallPacket::new_alpha();
    packet.decision_candidates = vec![surfaced];
    let packet = packet.with_derived_decision_attention();

    let json = serde_json::to_string(&packet).unwrap();
    let reparsed: RecallPacket = serde_json::from_str(&json).unwrap();
    assert_eq!(reparsed.decision_attention, packet.decision_attention);
    assert_eq!(
        reparsed.decision_candidates[0].allowed_outcomes,
        packet.decision_candidates[0].allowed_outcomes
    );
    assert_eq!(
        reparsed.decision_candidates[0].relevance_reasons,
        packet.decision_candidates[0].relevance_reasons
    );
}

/// Producer/consumer drift on the envelope version is a visible, typed failure
/// — never a silent legacy fallback or a fabricated success.
#[test]
fn envelope_version_drift_fails_visibly() {
    use bicameral_api::fixtures::alpha_recall_envelope;

    let envelope = alpha_recall_envelope();
    assert_eq!(envelope.envelope_version, ALPHA_RECALL_ENVELOPE_VERSION);
    assert_eq!(ALPHA_RECALL_ENVELOPE_VERSION, 2);

    // A v2-aware consumer renders the envelope.
    assert!(envelope
        .check_compatibility(&["v2"], ALPHA_RECALL_ENVELOPE_VERSION)
        .is_ok());

    // A consumer pinned to the legacy v1 envelope must refuse with a typed
    // mismatch rather than render an incomplete success.
    let err = envelope
        .check_compatibility(&["v2"], 1)
        .expect_err("legacy v1 consumer must refuse the v2 envelope");
    assert_eq!(
        err.kind,
        AlphaRecallMismatchKind::UnsupportedEnvelopeVersion
    );
    assert!(err.detail.contains("no legacy fallback"));
}
