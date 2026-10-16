use crate::candidate::DecisionCandidate;
use crate::dashboard::SignoffState;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidatePromotedPayload {
    pub version: u32,
    pub candidate_id: Uuid,
    pub decision_id: Uuid,
    pub candidate: DecisionCandidate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub actor_id: String,
    pub promoted_by: String,
    /// Optional signoff state materialized by the same atomic promotion event.
    ///
    /// `None` preserves the historical proposed-on-promotion behavior. The
    /// Managed MCP owner-policy path may set `Approved` only when the exact
    /// preview, current Product Owner authority, and current policy revision
    /// were revalidated immediately before the atomic append.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_signoff_state: Option<SignoffState>,
    #[serde(default)]
    pub audit_metadata: Map<String, Value>,
}

impl CandidatePromotedPayload {
    pub const VERSION: u32 = 1;

    pub fn new(
        candidate: DecisionCandidate,
        decision_id: Uuid,
        reason: Option<String>,
        actor_id: impl Into<String>,
        audit_metadata: Map<String, Value>,
    ) -> Self {
        Self::new_with_initial_signoff(
            candidate,
            decision_id,
            reason,
            actor_id,
            None,
            audit_metadata,
        )
    }

    pub fn new_with_initial_signoff(
        candidate: DecisionCandidate,
        decision_id: Uuid,
        reason: Option<String>,
        actor_id: impl Into<String>,
        initial_signoff_state: Option<SignoffState>,
        audit_metadata: Map<String, Value>,
    ) -> Self {
        let actor_id = actor_id.into();
        Self {
            version: Self::VERSION,
            candidate_id: candidate.id,
            decision_id,
            candidate,
            reason,
            promoted_by: actor_id.clone(),
            actor_id,
            initial_signoff_state,
            audit_metadata,
        }
    }
}
