//! Canonical acceptance facts for screened external evidence.
//!
//! This payload deliberately excludes projection requests, provider/model
//! outcomes, candidates, ranking, and convergence state. Canonical replay may
//! restore accepted evidence facts but must never schedule projection work.

use crate::evidence::SubmissionProvenance;
use crate::external_ingest::ExternalRedactionReceipt;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceAcceptedPayload {
    /// Stable deduplication key derived from source identity and content digest.
    pub idempotency_key: String,
    /// Provider-neutral evidence identity retained across retries.
    pub evidence_identity: String,
    pub source_system: String,
    pub source_uri: String,
    /// Product scope is optional until the external-ingest envelope carries a
    /// daemon-validated Product route. Absence never grants cross-Product scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product_scope: Option<String>,
    /// Provider source version, when supplied by a future compatible envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_version: Option<String>,
    pub snapshot_addr: String,
    pub content_digest: String,
    /// Opaque repository-relative locator. Never an absolute local path.
    pub content_locator: String,
    pub content_byte_length: usize,
    #[serde(default)]
    pub evidence_ids: Vec<Uuid>,
    pub provenance: SubmissionProvenance,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redaction_receipt: Option<ExternalRedactionReceipt>,
    pub accepted_at: DateTime<Utc>,
}

impl EvidenceAcceptedPayload {
    pub fn replay_key(&self) -> &str {
        &self.idempotency_key
    }
}
