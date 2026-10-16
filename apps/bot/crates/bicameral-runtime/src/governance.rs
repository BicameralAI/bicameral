//! Governance engine — evaluates policy against candidates and commands.

use bicameral_api::candidate::{Decision, DecisionCandidate, ExtractionConfidence};
use bicameral_api::constraint::{Attribution, DecisionReason, PromotionError};
use bicameral_api::dashboard::SignoffState;
use bicameral_api::event_store::PromoteToCanonicalPayload;
use bicameral_api::governance::{GovernanceResult, GovernanceVerdict};
use bicameral_config::GovernancePolicyConfig;

/// The governance engine evaluates whether candidates meet policy requirements.
///
/// It produces advisory GovernanceResults — it does not directly write canonical
/// state. Only event-store adapter materialization can promote accepted results.
pub struct GovernanceEngine {
    config: GovernancePolicyConfig,
}

impl GovernanceEngine {
    pub fn new(config: GovernancePolicyConfig) -> Self {
        Self { config }
    }

    /// Evaluate a candidate against the current governance policy.
    pub fn evaluate(&self, candidate: &DecisionCandidate) -> GovernanceResult {
        // Check minimum extraction confidence
        if let Some(ref min_conf) = self.config.min_extraction_confidence {
            let threshold = match min_conf.as_str() {
                "high" => ExtractionConfidence::High,
                "medium" => ExtractionConfidence::Medium,
                _ => ExtractionConfidence::Low,
            };
            if candidate.extraction_confidence < threshold {
                return GovernanceResult::reject(
                    candidate.id,
                    format!(
                        "Extraction confidence {:?} below minimum {:?}",
                        candidate.extraction_confidence, threshold
                    ),
                );
            }
        }

        // If review is required, route to configured approvers
        if self.config.require_review {
            return GovernanceResult::needs_review(
                candidate.id,
                "Policy requires human review before acceptance".to_string(),
                self.config.approvers.clone(),
            );
        }

        GovernanceResult::accept(candidate.id, "Policy satisfied".to_string())
    }

    /// Check whether a governance result permits materialization.
    pub fn may_materialize(result: &GovernanceResult) -> bool {
        result.verdict == GovernanceVerdict::Accepted
    }

    /// Gate the promotion of a reviewed decision into a canonical constraint.
    ///
    /// Authority is enforced here in the core — the SDK defines only the error
    /// shape. Authorization is checked BEFORE decision state so an unauthorized
    /// actor learns nothing about whether the decision was approved.
    ///
    /// Returns the durable [`PromoteToCanonicalPayload`] on success, or a
    /// [`PromotionError`]: `Unauthorized` when `authorized` is false, or
    /// `CandidateNotAccepted` when the decision is not `SignoffState::Approved`
    /// (mirrors the SDK `promoteToCanonical` gate on `reviewState === 'accepted'`).
    pub fn promote_to_canonical(
        decision: &Decision,
        reviewer: Attribution,
        reason: Option<DecisionReason>,
        affected_surfaces: Vec<String>,
        idempotency_key: String,
        authorized: bool,
    ) -> Result<PromoteToCanonicalPayload, PromotionError> {
        if !authorized {
            return Err(PromotionError::unauthorized());
        }
        if decision.signoff_state != SignoffState::Approved {
            return Err(PromotionError::candidate_not_accepted());
        }
        Ok(PromoteToCanonicalPayload {
            decision_id: decision.id,
            title: decision.title.clone(),
            statement: decision.body.clone(),
            evidence_refs: decision.evidence_refs.clone(),
            affected_surfaces,
            reviewer,
            reason,
            idempotency_key,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_api::candidate::DecisionLevel;

    #[test]
    fn test_requires_review_by_default() {
        let engine = GovernanceEngine::new(GovernancePolicyConfig::default());
        let candidate = DecisionCandidate::new(
            "Test".to_string(),
            "Description".to_string(),
            DecisionLevel::Architecture,
            "test-source".to_string(),
        );
        let result = engine.evaluate(&candidate);
        assert_eq!(result.verdict, GovernanceVerdict::NeedsReview);
    }

    #[test]
    fn default_policy_does_not_permit_materialization() {
        let engine = GovernanceEngine::new(GovernancePolicyConfig::default());
        let candidate = DecisionCandidate::new(
            "Test".to_string(),
            "Description".to_string(),
            DecisionLevel::Architecture,
            "test-source".to_string(),
        );
        let result = engine.evaluate(&candidate);
        assert!(
            !GovernanceEngine::may_materialize(&result),
            "Default policy must not permit materialization without human review"
        );
    }

    #[test]
    fn test_rejects_low_confidence() {
        let config = GovernancePolicyConfig {
            require_review: false,
            min_extraction_confidence: Some("high".to_string()),
            approvers: Vec::new(),
        };
        let engine = GovernanceEngine::new(config);
        let candidate = DecisionCandidate::new(
            "Test".to_string(),
            "Description".to_string(),
            DecisionLevel::Product,
            "test-source".to_string(),
        );
        let result = engine.evaluate(&candidate);
        assert_eq!(result.verdict, GovernanceVerdict::Rejected);
    }

    use bicameral_api::candidate::Decision;
    use bicameral_api::constraint::{ActorType, Attribution, PromotionErrorCode};
    use bicameral_api::dashboard::SignoffState;

    fn reviewer() -> Attribution {
        Attribution {
            actor_id: "rev-1".to_string(),
            actor_type: ActorType::Human,
            display_name: None,
        }
    }

    fn decision_with_signoff(state: SignoffState) -> Decision {
        let candidate = DecisionCandidate::new(
            "Title".to_string(),
            "Body statement".to_string(),
            DecisionLevel::Architecture,
            "jira://PROJ-1".to_string(),
        );
        let mut decision = Decision::from_candidate(&candidate, "promoter".to_string());
        decision.signoff_state = state;
        decision
    }

    #[test]
    fn promote_rejects_unauthorized_actor() {
        let decision = decision_with_signoff(SignoffState::Approved);
        let err = GovernanceEngine::promote_to_canonical(
            &decision,
            reviewer(),
            None,
            Vec::new(),
            "k".to_string(),
            false,
        )
        .unwrap_err();
        assert_eq!(err.code, PromotionErrorCode::Unauthorized);
    }

    #[test]
    fn promote_rejects_non_approved_decision() {
        let decision = decision_with_signoff(SignoffState::Proposed);
        let err = GovernanceEngine::promote_to_canonical(
            &decision,
            reviewer(),
            None,
            Vec::new(),
            "k".to_string(),
            true,
        )
        .unwrap_err();
        assert_eq!(err.code, PromotionErrorCode::CandidateNotAccepted);
    }

    #[test]
    fn promote_unauthorized_takes_precedence_over_state() {
        // Unauthorized actor on a non-approved decision must learn only that it
        // is unauthorized — never the decision's state.
        let decision = decision_with_signoff(SignoffState::Proposed);
        let err = GovernanceEngine::promote_to_canonical(
            &decision,
            reviewer(),
            None,
            Vec::new(),
            "k".to_string(),
            false,
        )
        .unwrap_err();
        assert_eq!(err.code, PromotionErrorCode::Unauthorized);
    }

    #[test]
    fn promote_approved_authorized_yields_payload_with_preserved_provenance() {
        let mut decision = decision_with_signoff(SignoffState::Approved);
        let ev = uuid::Uuid::new_v4();
        decision.evidence_refs = vec![ev];

        let payload = GovernanceEngine::promote_to_canonical(
            &decision,
            reviewer(),
            None,
            vec!["api/promotion".to_string()],
            "idem-1".to_string(),
            true,
        )
        .unwrap();

        assert_eq!(payload.decision_id, decision.id);
        assert_eq!(payload.statement, "Body statement");
        assert_eq!(payload.evidence_refs, vec![ev]);
        assert_eq!(payload.affected_surfaces, vec!["api/promotion".to_string()]);
        assert_eq!(payload.reviewer, reviewer());
        assert_eq!(payload.idempotency_key, "idem-1");
    }
}
