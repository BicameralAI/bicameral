//! Governance policy and result types.
//!
//! Governance decides what becomes canonical. Results are advisory unless
//! materialized through an event-store adapter.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A governance policy definition controlling what review/approval is required.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernancePolicy {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub rules: Vec<PolicyRule>,
}

/// A single rule within a governance policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRule {
    pub condition: String,
    pub action: PolicyAction,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyAction {
    /// Requires explicit human approval before materialization.
    RequireApproval,
    /// Routes to a specific reviewer or team.
    RouteForReview,
    /// Advisory warning — does not block.
    Warn,
    /// Automatically reject if condition matches.
    Reject,
}

/// The result of evaluating governance policy against a candidate or command.
///
/// GovernanceResults are advisory. They do not directly write canonical state.
/// Only acceptance through the event-store adapter path materializes a decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceResult {
    pub candidate_id: Uuid,
    pub verdict: GovernanceVerdict,
    pub policy_id: Option<Uuid>,
    pub reason: String,
    #[serde(default)]
    pub required_reviewers: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl GovernanceResult {
    pub fn accept(candidate_id: Uuid, reason: String) -> Self {
        Self {
            candidate_id,
            verdict: GovernanceVerdict::Accepted,
            policy_id: None,
            reason,
            required_reviewers: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn needs_review(candidate_id: Uuid, reason: String, reviewers: Vec<String>) -> Self {
        Self {
            candidate_id,
            verdict: GovernanceVerdict::NeedsReview,
            policy_id: None,
            reason,
            required_reviewers: reviewers,
            warnings: Vec::new(),
        }
    }

    pub fn reject(candidate_id: Uuid, reason: String) -> Self {
        Self {
            candidate_id,
            verdict: GovernanceVerdict::Rejected,
            policy_id: None,
            reason,
            required_reviewers: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernanceVerdict {
    /// Candidate accepted — may be materialized via event-store adapter.
    Accepted,
    /// Candidate needs human review before acceptance.
    NeedsReview,
    /// Candidate rejected by policy.
    Rejected,
}
