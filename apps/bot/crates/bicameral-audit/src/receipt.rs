//! Audit receipt types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// A single audit receipt recording a governance action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditReceipt {
    pub id: Uuid,
    pub action: AuditAction,
    pub actor: String,
    pub timestamp: DateTime<Utc>,
    pub target_id: Option<Uuid>,
    pub summary: String,
    /// SHA-256 of the previous receipt for chain integrity.
    pub prev_hash: Option<String>,
    /// SHA-256 of this receipt's content (excluding this field).
    pub content_hash: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

impl AuditReceipt {
    pub fn new(
        action: AuditAction,
        actor: String,
        target_id: Option<Uuid>,
        summary: String,
        prev_hash: Option<String>,
    ) -> Self {
        let id = Uuid::new_v4();
        let timestamp = Utc::now();

        let content_hash = compute_content_hash(&id, &action, &actor, &timestamp, &summary);

        Self {
            id,
            action,
            actor,
            timestamp,
            target_id,
            summary,
            prev_hash,
            content_hash,
            metadata: serde_json::Value::Null,
        }
    }
}

/// The type of governance action being recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    CandidateIngested,
    EvidenceAttached,
    ReviewCommandIssued,
    GovernanceEvaluated,
    EventMaterialized,
    ModExecuted,
    PolicyUpdated,
    WorkspaceInitialized,
    GatewayStarted,
}

fn compute_content_hash(
    id: &Uuid,
    action: &AuditAction,
    actor: &str,
    timestamp: &DateTime<Utc>,
    summary: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(id.as_bytes());
    hasher.update(format!("{:?}", action).as_bytes());
    hasher.update(actor.as_bytes());
    hasher.update(timestamp.to_rfc3339().as_bytes());
    hasher.update(summary.as_bytes());
    format!("{:x}", hasher.finalize())
}
