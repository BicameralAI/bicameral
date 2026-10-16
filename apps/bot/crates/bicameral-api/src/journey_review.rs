//! Read-only ingest journey review contract.
//!
//! A journey view composes canonical Product facts with daemon-local working
//! state at read time. It is never appended to canonical history and carries no
//! independent governance authority.

use crate::recall::RecallPacket;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const JOURNEY_REVIEW_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JourneyReviewView {
    pub schema_version: u32,
    pub journey_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product_scope: Option<String>,
    pub source: JourneySourceView,
    pub phases: Vec<JourneyPhaseView>,
    pub recall_packet: RecallPacket,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_actions: Vec<JourneyAllowedAction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caveats: Vec<JourneyCaveat>,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JourneySourceView {
    pub provider: String,
    pub evidence_identity: String,
    pub source_uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_version: Option<String>,
    pub content_digest: String,
    pub accepted_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JourneyPhaseView {
    pub phase_id: String,
    pub component: String,
    pub function: String,
    pub status: JourneyPhaseStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reason_codes: Vec<String>,
    pub persistence: JourneyPersistenceEffect,
    pub authority: JourneyAuthorityTransition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub sanitized_summary: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JourneyPhaseStatus {
    Accepted,
    Completed,
    Deferred,
    Failed,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JourneyPersistenceEffect {
    pub plane: JourneyPersistencePlane,
    pub operation: JourneyPersistenceOperation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JourneyPersistencePlane {
    None,
    EvidenceBody,
    LocalWork,
    CanonicalEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JourneyPersistenceOperation {
    None,
    Read,
    Write,
    Append,
    Update,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JourneyAuthorityTransition {
    pub before: JourneyAuthorityClass,
    pub after: JourneyAuthorityClass,
    pub transition: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JourneyAuthorityClass {
    Evidence,
    Proposal,
    Canonical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JourneyAllowedAction {
    pub command: JourneyActionKind,
    pub target_id: Uuid,
    pub requires_authority: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JourneyActionKind {
    CandidateHide,
    CandidateSuppress,
    CandidateReject,
    CandidatePromote,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JourneyCaveat {
    pub code: String,
    pub detail: String,
}
