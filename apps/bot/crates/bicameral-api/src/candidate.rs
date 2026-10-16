//! Decision candidate and promoted Decision types.
//!
//! `DecisionCandidate` is proposed; `Decision` is promoted via governance.
//! The promotion path: candidate → accept_candidate → Decision(proposed)
//! → approve_signoff → Decision(approved).

use crate::dashboard::{ComplianceState, SignoffState};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A proposed decision extracted from an operational source, awaiting review.
///
/// Candidates are never canonical. They represent probabilistic extraction from
/// sources like Jira tickets, Slack messages, meeting notes, or code comments.
/// Only governance acceptance through an event-store adapter can promote a
/// candidate into the decision ledger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionCandidate {
    pub id: Uuid,
    pub title: String,
    pub description: String,
    pub level: DecisionLevel,
    pub source: String,
    pub extraction_confidence: ExtractionConfidence,
    pub extracted_at: DateTime<Utc>,
    pub extracted_by: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<Uuid>,
    /// IDs of typed `EvidenceReference` items cited by this candidate.
    ///
    /// These complement `evidence_refs` (which track legacy `SourceEvidence`
    /// span pointers). Callers submitting MCP session turns, tool call output,
    /// or command output should populate this field so the evidence chain is
    /// preserved through promotion to a Decision.
    #[serde(default)]
    pub evidence_reference_ids: Vec<Uuid>,
    pub candidate_state: CandidateState,
    pub review_state: ReviewLifecycleState,
    #[serde(default)]
    pub metadata: serde_json::Value,
    /// Informational provenance metadata — which surface/actor submitted this
    /// candidate. Never creates governance authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<crate::evidence::SubmissionProvenance>,
}

impl DecisionCandidate {
    pub fn new(title: String, description: String, level: DecisionLevel, source: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            title,
            description,
            level,
            source,
            extraction_confidence: ExtractionConfidence::Low,
            extracted_at: Utc::now(),
            extracted_by: String::from("unknown"),
            tags: Vec::new(),
            evidence_refs: Vec::new(),
            evidence_reference_ids: Vec::new(),
            candidate_state: CandidateState::Proposed,
            review_state: ReviewLifecycleState::Proposed,
            metadata: serde_json::Value::Null,
            provenance: None,
        }
    }
}

/// A promoted Decision in the ledger — materialized from a candidate via
/// governance acceptance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub id: Uuid,
    pub candidate_id: Uuid,
    pub title: String,
    pub body: String,
    pub level: DecisionLevel,
    pub signoff_state: SignoffState,
    pub compliance_state: ComplianceState,
    pub review_state: ReviewLifecycleState,
    #[serde(default)]
    pub evidence_refs: Vec<Uuid>,
    /// IDs of typed `EvidenceReference` items preserved from the candidate.
    ///
    /// Promotion copies these from `DecisionCandidate.evidence_reference_ids`
    /// so the evidence chain back to MCP/tool SourceSnapshots is durable.
    #[serde(default)]
    pub evidence_reference_ids: Vec<Uuid>,
    #[serde(default)]
    pub source_id: Option<String>,
    /// Tracking lifecycle state per ADR-0022.
    ///
    /// Defaults to `Tracked` on promotion. Signoff is a separate axis.
    #[serde(default = "TrackingState::default_tracked")]
    pub tracking_state: TrackingState,
    pub promoted_at: DateTime<Utc>,
    pub promoted_by: String,
    /// Informational provenance metadata preserved from the candidate.
    /// Never creates governance authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<crate::evidence::SubmissionProvenance>,
}

impl Decision {
    pub fn from_candidate(candidate: &DecisionCandidate, promoted_by: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            candidate_id: candidate.id,
            title: candidate.title.clone(),
            body: candidate.description.clone(),
            level: candidate.level,
            signoff_state: SignoffState::Proposed,
            compliance_state: ComplianceState::Ungrounded,
            review_state: ReviewLifecycleState::Proposed,
            evidence_refs: candidate.evidence_refs.clone(),
            evidence_reference_ids: candidate.evidence_reference_ids.clone(),
            source_id: Some(candidate.source.clone()),
            tracking_state: TrackingState::Tracked,
            promoted_at: Utc::now(),
            promoted_by,
            provenance: candidate.provenance.clone(),
        }
    }

    /// Derived lifecycle strength (projection of `signoff_state`).
    pub fn strength(&self) -> Strength {
        Strength::from_signoff(self.signoff_state)
    }

    /// Derived downstream-authority class for this decision.
    pub fn authority(&self) -> Authority {
        self.strength().authority()
    }

    /// Whether this Decision is eligible for automatic source refresh.
    ///
    /// A Decision is auto-refresh-eligible when it is tracked and its signoff
    /// state has not been terminally resolved (rejected or superseded).
    /// Proposed, Approved, and CollisionPending Decisions remain eligible
    /// per ADR-0022.
    pub fn is_auto_refresh_eligible(&self) -> bool {
        self.tracking_state.is_auto_refresh_eligible()
            && !matches!(
                self.signoff_state,
                SignoffState::Rejected | SignoffState::Superseded
            )
    }
}

/// Candidate lifecycle state (spec §2.2 candidate_state).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CandidateState {
    Proposed,
    Rejected,
    Promoted,
}

/// Review lifecycle state from MCP v0.2 spec §2.2.
///
/// Tracks where a candidate or decision is in the governance pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReviewLifecycleState {
    Advisory,
    Proposed,
    NeedsSignoffReview,
    NeedsGroundingReview,
    NeedsComplianceReview,
    CollisionPending,
    StaleSourcePending,
    ReadyToMaterialize,
    Materialized,
}

/// Decision tier classification.
///
/// Separating levels prevents collapsing product intent, architectural
/// constraint, and implementation detail into a single undifferentiated pile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DecisionLevel {
    /// L1: Product-level decisions (what and why)
    Product,
    /// L2: Architecture-level decisions (structural how)
    Architecture,
    /// L3: Implementation-level decisions (tactical how)
    Implementation,
}

/// Extraction confidence — how certain the system is that this is actually a decision.
///
/// This is intentionally separate from binding confidence and compliance confidence.
/// Collapsing these into one score creates cognitive debt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionConfidence {
    Low,
    Medium,
    High,
}

/// Lifecycle strength — how mature and influential a decision's state is.
///
/// Orthogonal to [`SignoffState`] (ownership authority) and [`ComplianceState`]
/// (technical reality): strength is the *maturity* axis. In this projection it
/// is derived from `SignoffState` and is **not** an independently-stored field.
/// The future extension point is an independent writer — agent-implicit capture
/// landing items as `Tentative` rather than `Proposed` — which is what first
/// makes strength diverge from a pure function of signoff.
///
/// Ordering is meaningful: `Observed < Tentative < Reinforced < Promoted`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Strength {
    /// Captured signal, not yet trusted — inspection only.
    Observed,
    /// Plausible pattern awaiting review — recommendation influence only.
    Tentative,
    /// Repeated or corroborated support — stronger recommendation influence.
    Reinforced,
    /// Reviewed and governance-approved — default governed use within scope.
    Promoted,
}

/// Downstream-authority class — the maximum influence a decision's retained
/// state may exert. Rises with [`Strength`].
///
/// Ordering is meaningful: `RetrievalOnly < Recommendation < Draft < LocalMutation`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Authority {
    /// A0 — may appear in inspection/recall only.
    RetrievalOnly,
    /// A1 — may influence rankings, suggestions, and session banners.
    Recommendation,
    /// A2 — may shape drafts or proposed bindings for human review.
    Draft,
    /// A3 — may drive local workflow mutation (e.g. drift tracking against code).
    LocalMutation,
}

impl Strength {
    /// Derive strength from the ownership signoff axis.
    ///
    /// `Approved` is the human-promoted, drift-authoritative state; `Proposed`
    /// and `CollisionPending` are recommendation-only; `Rejected`/`Superseded`
    /// are retired to the lowest influence. No `SignoffState` yields
    /// `Reinforced` today — that strength is reserved for a future independent
    /// reinforcement writer (e.g. agent-implicit corroboration).
    pub fn from_signoff(state: SignoffState) -> Self {
        match state {
            SignoffState::Approved => Strength::Promoted,
            SignoffState::Proposed | SignoffState::CollisionPending => Strength::Tentative,
            SignoffState::Rejected | SignoffState::Superseded => Strength::Observed,
        }
    }

    /// The maximum downstream authority this strength may exert.
    pub fn authority(self) -> Authority {
        match self {
            Strength::Observed => Authority::RetrievalOnly,
            Strength::Tentative => Authority::Recommendation,
            Strength::Reinforced => Authority::Draft,
            Strength::Promoted => Authority::LocalMutation,
        }
    }
}

/// Tracking lifecycle state for a promoted Decision.
///
/// Per ADR-0022: candidate promotion makes a Decision tracked by default.
/// A tracked Decision's cited evidence remains within Bicameral's governance
/// freshness scope, enabling source refresh when cited Sources change or
/// become stale. Signoff is a separate ownership lifecycle and does not
/// imply tracking, compliance, or enforcement.
///
/// `Untrack` stops automatic refresh for the Decision. A user, connector,
/// or policy may still trigger manual refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackingState {
    /// Decision evidence is actively monitored for freshness (default on
    /// promotion per ADR-0022).
    Tracked,
    /// Automatic evidence refresh has been stopped. Evidence may go stale
    /// without creating a governance failure.
    Untracked,
}

impl TrackingState {
    /// Serde default function — promotion makes a Decision tracked.
    pub fn default_tracked() -> Self {
        TrackingState::Tracked
    }

    /// Whether this tracking state permits automatic refresh enqueuing.
    pub fn is_auto_refresh_eligible(&self) -> bool {
        matches!(self, TrackingState::Tracked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_state_serializes_to_snake_case() {
        assert_eq!(
            serde_json::to_string(&CandidateState::Proposed).unwrap(),
            "\"proposed\""
        );
        assert_eq!(
            serde_json::to_string(&CandidateState::Rejected).unwrap(),
            "\"rejected\""
        );
        assert_eq!(
            serde_json::to_string(&CandidateState::Promoted).unwrap(),
            "\"promoted\""
        );
    }

    #[test]
    fn candidate_state_deserializes_from_snake_case() {
        assert_eq!(
            serde_json::from_str::<CandidateState>("\"proposed\"").unwrap(),
            CandidateState::Proposed
        );
        assert_eq!(
            serde_json::from_str::<CandidateState>("\"rejected\"").unwrap(),
            CandidateState::Rejected
        );
        assert_eq!(
            serde_json::from_str::<CandidateState>("\"promoted\"").unwrap(),
            CandidateState::Promoted
        );
    }

    #[test]
    fn review_lifecycle_state_serializes_to_snake_case() {
        let cases = vec![
            (ReviewLifecycleState::Advisory, "\"advisory\""),
            (ReviewLifecycleState::Proposed, "\"proposed\""),
            (
                ReviewLifecycleState::NeedsSignoffReview,
                "\"needs_signoff_review\"",
            ),
            (
                ReviewLifecycleState::NeedsGroundingReview,
                "\"needs_grounding_review\"",
            ),
            (
                ReviewLifecycleState::NeedsComplianceReview,
                "\"needs_compliance_review\"",
            ),
            (
                ReviewLifecycleState::CollisionPending,
                "\"collision_pending\"",
            ),
            (
                ReviewLifecycleState::StaleSourcePending,
                "\"stale_source_pending\"",
            ),
            (
                ReviewLifecycleState::ReadyToMaterialize,
                "\"ready_to_materialize\"",
            ),
            (ReviewLifecycleState::Materialized, "\"materialized\""),
        ];
        for (variant, expected) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    #[test]
    fn review_lifecycle_state_deserializes_from_snake_case() {
        let cases = vec![
            ("\"advisory\"", ReviewLifecycleState::Advisory),
            ("\"proposed\"", ReviewLifecycleState::Proposed),
            (
                "\"needs_signoff_review\"",
                ReviewLifecycleState::NeedsSignoffReview,
            ),
            (
                "\"needs_grounding_review\"",
                ReviewLifecycleState::NeedsGroundingReview,
            ),
            (
                "\"needs_compliance_review\"",
                ReviewLifecycleState::NeedsComplianceReview,
            ),
            (
                "\"collision_pending\"",
                ReviewLifecycleState::CollisionPending,
            ),
            (
                "\"stale_source_pending\"",
                ReviewLifecycleState::StaleSourcePending,
            ),
            (
                "\"ready_to_materialize\"",
                ReviewLifecycleState::ReadyToMaterialize,
            ),
            ("\"materialized\"", ReviewLifecycleState::Materialized),
        ];
        for (json, expected) in cases {
            assert_eq!(
                serde_json::from_str::<ReviewLifecycleState>(json).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn strength_derives_from_each_signoff_state() {
        assert_eq!(
            Strength::from_signoff(SignoffState::Approved),
            Strength::Promoted
        );
        assert_eq!(
            Strength::from_signoff(SignoffState::Proposed),
            Strength::Tentative
        );
        assert_eq!(
            Strength::from_signoff(SignoffState::CollisionPending),
            Strength::Tentative
        );
        assert_eq!(
            Strength::from_signoff(SignoffState::Rejected),
            Strength::Observed
        );
        assert_eq!(
            Strength::from_signoff(SignoffState::Superseded),
            Strength::Observed
        );
    }

    // ── TrackingState serialization ──────────────────────────────────

    #[test]
    fn tracking_state_serializes_to_snake_case() {
        assert_eq!(
            serde_json::to_string(&TrackingState::Tracked).unwrap(),
            "\"tracked\""
        );
        assert_eq!(
            serde_json::to_string(&TrackingState::Untracked).unwrap(),
            "\"untracked\""
        );
    }

    #[test]
    fn authority_for_each_strength() {
        assert_eq!(Strength::Observed.authority(), Authority::RetrievalOnly);
        assert_eq!(Strength::Tentative.authority(), Authority::Recommendation);
        assert_eq!(Strength::Reinforced.authority(), Authority::Draft);
        assert_eq!(Strength::Promoted.authority(), Authority::LocalMutation);
    }

    #[test]
    fn authority_is_monotonic_in_strength() {
        assert!(Strength::Observed < Strength::Tentative);
        assert!(Strength::Tentative < Strength::Reinforced);
        assert!(Strength::Reinforced < Strength::Promoted);
        assert!(Strength::Observed.authority() < Strength::Tentative.authority());
        assert!(Strength::Tentative.authority() < Strength::Reinforced.authority());
        assert!(Strength::Reinforced.authority() < Strength::Promoted.authority());
    }

    #[test]
    fn reinforced_is_forward_only_no_signoff_yields_it() {
        // Inverse coverage: every Strength except Reinforced is produced by some
        // SignoffState; Reinforced is reserved for a future independent writer.
        let produced: Vec<Strength> = [
            SignoffState::Approved,
            SignoffState::Proposed,
            SignoffState::CollisionPending,
            SignoffState::Rejected,
            SignoffState::Superseded,
        ]
        .into_iter()
        .map(Strength::from_signoff)
        .collect();
        assert!(produced.contains(&Strength::Observed));
        assert!(produced.contains(&Strength::Tentative));
        assert!(produced.contains(&Strength::Promoted));
        assert!(!produced.contains(&Strength::Reinforced));
    }

    #[test]
    fn decision_exposes_derived_strength_and_authority() {
        let candidate = DecisionCandidate::new(
            "t".into(),
            "d".into(),
            DecisionLevel::Architecture,
            "src".into(),
        );
        // Decision::from_candidate starts at SignoffState::Proposed.
        let decision = Decision::from_candidate(&candidate, "tester".into());
        assert_eq!(decision.strength(), Strength::Tentative);
        assert_eq!(decision.authority(), Authority::Recommendation);
    }

    #[test]
    fn strength_and_authority_serialize_to_snake_case() {
        assert_eq!(
            serde_json::to_string(&Strength::Tentative).unwrap(),
            "\"tentative\""
        );
        assert_eq!(
            serde_json::to_string(&Authority::LocalMutation).unwrap(),
            "\"local_mutation\""
        );
    }

    #[test]
    fn tracking_state_deserializes_from_snake_case() {
        assert_eq!(
            serde_json::from_str::<TrackingState>("\"tracked\"").unwrap(),
            TrackingState::Tracked
        );
        assert_eq!(
            serde_json::from_str::<TrackingState>("\"untracked\"").unwrap(),
            TrackingState::Untracked
        );
    }

    #[test]
    fn tracking_state_rejects_unknown_variant() {
        assert!(serde_json::from_str::<TrackingState>("\"paused\"").is_err());
    }

    #[test]
    fn tracking_state_default_is_tracked() {
        assert_eq!(TrackingState::default_tracked(), TrackingState::Tracked);
    }

    #[test]
    fn decision_from_candidate_defaults_to_tracked() {
        let candidate = DecisionCandidate::new(
            "test".into(),
            "body".into(),
            DecisionLevel::Product,
            "jira://PROJ-1".into(),
        );
        let decision = Decision::from_candidate(&candidate, "agent".into());
        assert_eq!(decision.tracking_state, TrackingState::Tracked);
    }

    #[test]
    fn decision_deserializes_without_tracking_state_as_tracked() {
        let json = r#"{
            "id": "00000000-0000-0000-0000-000000000001",
            "candidate_id": "00000000-0000-0000-0000-000000000002",
            "title": "legacy",
            "body": "old decision without tracking_state",
            "level": "product",
            "signoff_state": "proposed",
            "compliance_state": "ungrounded",
            "review_state": "proposed",
            "promoted_at": "2026-01-01T00:00:00Z",
            "promoted_by": "human"
        }"#;
        let d: Decision = serde_json::from_str(json).unwrap();
        assert_eq!(d.tracking_state, TrackingState::Tracked);
    }

    #[test]
    fn decision_serializes_tracking_state() {
        let candidate = DecisionCandidate::new(
            "test".into(),
            "body".into(),
            DecisionLevel::Architecture,
            "slack://C01/msg1".into(),
        );
        let decision = Decision::from_candidate(&candidate, "agent".into());
        let json = serde_json::to_value(&decision).unwrap();
        assert_eq!(json["tracking_state"], "tracked");
    }
}
