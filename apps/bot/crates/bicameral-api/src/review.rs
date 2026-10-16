//! Review commands and state — the HITL review queue.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A command submitted to the review system.
///
/// Review commands represent explicit human or agent actions on the review queue:
/// submitting candidates for review, approving/rejecting them, requesting
/// additional evidence, or escalating to another reviewer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewCommand {
    pub id: Uuid,
    pub kind: ReviewCommandKind,
    pub candidate_id: Uuid,
    pub issued_by: String,
    pub issued_at: DateTime<Utc>,
    pub reason: Option<String>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

impl ReviewCommand {
    pub fn new(kind: ReviewCommandKind, candidate_id: Uuid, issued_by: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            kind,
            candidate_id,
            issued_by,
            issued_at: Utc::now(),
            reason: None,
            metadata: serde_json::Value::Null,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewCommandKind {
    /// Submit a candidate for review.
    Submit,
    /// Approve a candidate — governance may then materialize it.
    Approve,
    /// Reject a candidate.
    Reject,
    /// Request additional evidence before deciding.
    RequestEvidence,
    /// Escalate to a different reviewer.
    Escalate,
}

/// The current review state of a candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewState {
    pub candidate_id: Uuid,
    pub status: ReviewStatus,
    pub assigned_reviewers: Vec<String>,
    pub submitted_at: Option<DateTime<Utc>>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub resolution: Option<ReviewCommandKind>,
    #[serde(default)]
    pub history: Vec<ReviewCommand>,
}

impl ReviewState {
    pub fn new(candidate_id: Uuid) -> Self {
        Self {
            candidate_id,
            status: ReviewStatus::Pending,
            assigned_reviewers: Vec::new(),
            submitted_at: None,
            resolved_at: None,
            resolution: None,
            history: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    Pending,
    InReview,
    Approved,
    Rejected,
    NeedsEvidence,
    Escalated,
}
