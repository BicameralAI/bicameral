//! Canonical-constraint promotion surface.
//!
//! A reviewed (`SignoffState::Approved`) [`Decision`](crate::candidate::Decision)
//! can be promoted by an authorized actor into a [`CanonicalConstraint`] — the
//! durable rule that implementation must respect. Promotion preserves the
//! reviewer, the source decision, its evidence, and the review reason.
//!
//! This module mirrors the lean SDK contract shapes (`bicameral-sdk`):
//! `Attribution`/`ActorType` (provenance), `DecisionReason`, the canonical
//! subset of `ReviewState`, and the lean `Constraint`. The richer catalog read
//! model (`CatalogConstraint`: type/scope/version/supersession/query) is owned
//! by GH #482 and deliberately out of scope here.

use crate::candidate::Decision;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The kind of actor an attribution refers to.
///
/// Mirrors the SDK `ActorType` (`bicameral-sdk/src/provenance/index.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActorType {
    Human,
    Agent,
    System,
    Connector,
}

/// Who performed an action — the provenance anchor preserved through promotion.
///
/// Mirrors the SDK `Attribution` (`bicameral-sdk/src/provenance/index.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attribution {
    pub actor_id: String,
    pub actor_type: ActorType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

/// A coded reason recorded with a review verdict.
///
/// Mirrors the SDK `DecisionReason` (`bicameral-sdk/src/decisions/index.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionReason {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// The canonical-stage subset of the SDK `ReviewState`.
///
/// The full SDK `ReviewState` is `proposed | accepted | rejected | superseded |
/// deprecated`. Only the canonical stage is representable here — mirroring
/// `CanonicalDecision.reviewState: Extract<ReviewState,'accepted'|'superseded'|
/// 'deprecated'>` (`bicameral-sdk/src/decisions/index.ts`). A freshly promoted
/// constraint is always `Accepted`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalReviewState {
    Accepted,
    Superseded,
    Deprecated,
}

/// The full SDK `ReviewState` lifecycle vocabulary.
///
/// `proposed | accepted | rejected | superseded | deprecated` — the complete
/// review lifecycle a candidate artifact moves through, shared by surfaces that
/// hold a full-vocabulary review state (semantic mappings, correction findings).
/// Named `LifecycleReviewState` to avoid colliding with the review-queue
/// [`ReviewState`](crate::review::ReviewState) struct. The canonical promotion
/// stage uses the narrower [`CanonicalReviewState`] subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleReviewState {
    Proposed,
    Accepted,
    Rejected,
    Superseded,
    Deprecated,
}

/// Why a promotion to canonical was refused.
///
/// Mirrors the SDK `PromotionErrorCode` (`bicameral-sdk/src/decisions/index.ts`).
/// The SDK helper emits only `CandidateNotAccepted`; `Unauthorized` is enforced
/// here in the core/runtime — the SDK defines the shape, not the enforcement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PromotionErrorCode {
    CandidateNotAccepted,
    Unauthorized,
}

/// A refused promotion, carrying its coded reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct PromotionError {
    pub code: PromotionErrorCode,
    pub message: String,
}

impl PromotionError {
    /// The source decision was not in the reviewed-accepted state.
    pub fn candidate_not_accepted() -> Self {
        Self {
            code: PromotionErrorCode::CandidateNotAccepted,
            message: "Only an accepted reviewed decision can be promoted to canonical.".to_string(),
        }
    }

    /// The acting principal is not authorized to promote.
    pub fn unauthorized() -> Self {
        Self {
            code: PromotionErrorCode::Unauthorized,
            message: "Actor is not authorized to promote a decision to canonical.".to_string(),
        }
    }
}

/// A durable rule promoted from a reviewed decision.
///
/// Mirrors the lean SDK `Constraint` (`bicameral-sdk/src/models/index.ts`) plus
/// the promotion provenance carried by `CanonicalDecision`. It intentionally
/// omits `type`, `scope`, `version`, and supersession/query fields — those live
/// on the catalog read model (`CatalogConstraint`, GH #482).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalConstraint {
    pub id: Uuid,
    /// The reviewed decision this constraint was promoted from.
    pub decision_id: Uuid,
    pub title: String,
    pub statement: String,
    pub review_state: CanonicalReviewState,
    #[serde(default)]
    pub evidence_refs: Vec<Uuid>,
    #[serde(default)]
    pub affected_surfaces: Vec<String>,
    pub reviewer: Attribution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<DecisionReason>,
    pub promoted_at: DateTime<Utc>,
    pub idempotency_key: String,
}

impl CanonicalConstraint {
    /// Build a canonical constraint from an approved decision, preserving its
    /// identity, statement, and evidence and recording the promotion provenance.
    ///
    /// Pure: this does not enforce the approval/authorization gate (that is the
    /// runtime's job). It copies `decision.body` into `statement` and reuses the
    /// decision's `evidence_refs`, and sets `review_state` to `Accepted`.
    pub fn from_approved_decision(
        decision: &Decision,
        reviewer: Attribution,
        reason: Option<DecisionReason>,
        affected_surfaces: Vec<String>,
        idempotency_key: String,
        promoted_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            decision_id: decision.id,
            title: decision.title.clone(),
            statement: decision.body.clone(),
            review_state: CanonicalReviewState::Accepted,
            evidence_refs: decision.evidence_refs.clone(),
            affected_surfaces,
            reviewer,
            reason,
            promoted_at,
            idempotency_key,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::candidate::{DecisionCandidate, DecisionLevel};

    fn reviewer() -> Attribution {
        Attribution {
            actor_id: "u-1".to_string(),
            actor_type: ActorType::Human,
            display_name: Some("Reviewer One".to_string()),
        }
    }

    #[test]
    fn canonical_review_state_round_trips_snake_case() {
        let cases = [
            (CanonicalReviewState::Accepted, "\"accepted\""),
            (CanonicalReviewState::Superseded, "\"superseded\""),
            (CanonicalReviewState::Deprecated, "\"deprecated\""),
        ];
        for (variant, json) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), json);
            assert_eq!(
                serde_json::from_str::<CanonicalReviewState>(json).unwrap(),
                variant
            );
        }
    }

    #[test]
    fn canonical_review_state_inverse_coverage() {
        // Exactly the canonical subset deserializes; the non-canonical
        // ReviewState values ('proposed'/'rejected') are NOT representable.
        assert!(serde_json::from_str::<CanonicalReviewState>("\"proposed\"").is_err());
        assert!(serde_json::from_str::<CanonicalReviewState>("\"rejected\"").is_err());
        assert!(serde_json::from_str::<CanonicalReviewState>("\"bogus\"").is_err());
    }

    #[test]
    fn lifecycle_review_state_round_trips_and_inverse() {
        for (variant, json) in [
            (LifecycleReviewState::Proposed, "\"proposed\""),
            (LifecycleReviewState::Accepted, "\"accepted\""),
            (LifecycleReviewState::Rejected, "\"rejected\""),
            (LifecycleReviewState::Superseded, "\"superseded\""),
            (LifecycleReviewState::Deprecated, "\"deprecated\""),
        ] {
            assert_eq!(serde_json::to_string(&variant).unwrap(), json);
            assert_eq!(
                serde_json::from_str::<LifecycleReviewState>(json).unwrap(),
                variant
            );
        }
        // Inverse coverage: out-of-vocabulary and wrong-case strings fail.
        assert!(serde_json::from_str::<LifecycleReviewState>("\"archived\"").is_err());
        assert!(serde_json::from_str::<LifecycleReviewState>("\"Accepted\"").is_err());
    }

    #[test]
    fn actor_type_round_trips_snake_case() {
        let cases = [
            (ActorType::Human, "\"human\""),
            (ActorType::Agent, "\"agent\""),
            (ActorType::System, "\"system\""),
            (ActorType::Connector, "\"connector\""),
        ];
        for (variant, json) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), json);
            assert_eq!(serde_json::from_str::<ActorType>(json).unwrap(), variant);
        }
    }

    #[test]
    fn actor_type_inverse_coverage() {
        assert!(serde_json::from_str::<ActorType>("\"robot\"").is_err());
        assert!(serde_json::from_str::<ActorType>("\"Human\"").is_err());
    }

    #[test]
    fn promotion_error_code_round_trips_snake_case() {
        assert_eq!(
            serde_json::to_string(&PromotionErrorCode::CandidateNotAccepted).unwrap(),
            "\"candidate_not_accepted\""
        );
        assert_eq!(
            serde_json::to_string(&PromotionErrorCode::Unauthorized).unwrap(),
            "\"unauthorized\""
        );
        assert_eq!(
            serde_json::from_str::<PromotionErrorCode>("\"candidate_not_accepted\"").unwrap(),
            PromotionErrorCode::CandidateNotAccepted
        );
        assert_eq!(
            serde_json::from_str::<PromotionErrorCode>("\"unauthorized\"").unwrap(),
            PromotionErrorCode::Unauthorized
        );
    }

    #[test]
    fn promotion_error_code_inverse_coverage() {
        assert!(serde_json::from_str::<PromotionErrorCode>("\"forbidden\"").is_err());
        assert!(serde_json::from_str::<PromotionErrorCode>("\"accepted\"").is_err());
    }

    #[test]
    fn from_approved_decision_preserves_provenance() {
        let candidate = DecisionCandidate::new(
            "Title".into(),
            "The statement body".into(),
            DecisionLevel::Architecture,
            "jira://PROJ-1".into(),
        );
        let mut decision = Decision::from_candidate(&candidate, "promoter".into());
        let ev = Uuid::new_v4();
        decision.evidence_refs = vec![ev];

        let reason = DecisionReason {
            code: "approved-by-arch".into(),
            detail: Some("meets boundary".into()),
        };
        let at = Utc::now();
        let constraint = CanonicalConstraint::from_approved_decision(
            &decision,
            reviewer(),
            Some(reason.clone()),
            vec!["api/promotion".into()],
            "idem-1".into(),
            at,
        );

        assert_eq!(constraint.decision_id, decision.id);
        assert_eq!(constraint.title, "Title");
        assert_eq!(constraint.statement, "The statement body");
        assert_eq!(constraint.evidence_refs, vec![ev]);
        assert_eq!(
            constraint.affected_surfaces,
            vec!["api/promotion".to_string()]
        );
        assert_eq!(constraint.reviewer, reviewer());
        assert_eq!(constraint.reason, Some(reason));
        assert_eq!(constraint.review_state, CanonicalReviewState::Accepted);
        assert_eq!(constraint.idempotency_key, "idem-1");
        assert_eq!(constraint.promoted_at, at);
    }

    #[test]
    fn canonical_constraint_round_trips_json() {
        let candidate =
            DecisionCandidate::new("T".into(), "B".into(), DecisionLevel::Product, "src".into());
        let decision = Decision::from_candidate(&candidate, "p".into());
        let constraint = CanonicalConstraint::from_approved_decision(
            &decision,
            reviewer(),
            None,
            Vec::new(),
            "k".into(),
            Utc::now(),
        );
        let json = serde_json::to_string(&constraint).unwrap();
        let back: CanonicalConstraint = serde_json::from_str(&json).unwrap();
        assert_eq!(back, constraint);
    }
}
