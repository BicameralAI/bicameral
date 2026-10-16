//! Live external-ingest projection over daemon-local SQLite working state.
//!
//! This module deliberately keeps projection requests, provider outcomes, retries,
//! and candidate convergence outside canonical Product replay. During the parity
//! window, the gateway still writes legacy candidate-bearing events after the
//! working-state transaction succeeds so existing review and promotion paths
//! remain replayable until their self-contained canonical replacement lands.

use crate::projection_work_repository::{
    ProjectionOutcomeKind, ProjectionRequestStatus, ProjectionWorkOutcome,
    ProjectionWorkRepository, ProjectionWorkRepositoryError, ProjectionWorkRequest,
    SqliteProjectionWorkRepository, WorkingCandidate,
};
use crate::state::AppState;
use bicameral_api::candidate::{DecisionCandidate, DecisionLevel};
use bicameral_api::evidence::{SourceEvidence, SubmissionProvenance};
use bicameral_api::evidence_acceptance::EvidenceAcceptedPayload;
use bicameral_api::reasoning_projection::{
    projection_request_from_external_ingest, AlphaProjectionOutcome, CandidateHintInput,
    ProjectionDeferralReason, ProjectionResult,
};
use bicameral_api::tool_request::ExternalIngestEnvelope;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;
use uuid::Uuid;

const CLAIM_LEASE_MINUTES: i64 = 5;
const RETRY_DELAY_SECONDS: i64 = 30;

#[derive(Debug, Error)]
pub enum LiveProjectionWorkError {
    #[error(transparent)]
    Repository(#[from] ProjectionWorkRepositoryError),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("projection request id in working state is invalid: {0}")]
    InvalidRequestId(String),
    #[error("projection resume input missing for request: {0}")]
    MissingResumeInput(String),
    #[error("candidate suppression expiry must be in the future")]
    InvalidSuppressionExpiry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateSuppression {
    pub candidate_id: Uuid,
    pub semantic_fingerprint: String,
    pub evidence_acceptance_key: String,
    pub source_uri: String,
    pub content_digest: String,
    pub reason: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Resume-safe inputs that intentionally omit the screened evidence body.
///
/// The body remains in the content-addressed evidence repository and is loaded
/// by `content_locator` for a future asynchronous worker. The synchronous HTTP
/// path already has the same screened bytes and therefore does not reread them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionResumeInput {
    pub content_locator: String,
    pub content_digest: String,
    pub evidence_excerpts: Vec<String>,
    pub candidate_hints: Vec<CandidateHintInput>,
}

#[derive(Debug, Clone)]
pub struct ProjectionExecution {
    pub projection_request_id: Option<Uuid>,
    pub outcome_label: String,
    pub candidates: Vec<DecisionCandidate>,
}

/// SQLite-backed live projection state layered over the D3b repository.
#[derive(Debug, Clone)]
pub struct LiveProjectionWorkStore {
    repository: Arc<SqliteProjectionWorkRepository>,
    hidden_candidates: Arc<Mutex<HashMap<String, BTreeSet<Uuid>>>>,
}

impl LiveProjectionWorkStore {
    pub fn open(
        repository: Arc<SqliteProjectionWorkRepository>,
    ) -> Result<Self, LiveProjectionWorkError> {
        let store = Self {
            repository,
            hidden_candidates: Arc::new(Mutex::new(HashMap::new())),
        };
        store.migrate_live_tables()?;
        Ok(store)
    }

    pub fn repository(&self) -> &SqliteProjectionWorkRepository {
        self.repository.as_ref()
    }

    fn connection(&self) -> Result<Connection, LiveProjectionWorkError> {
        let connection = Connection::open(self.repository.path())?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL;\n\
             PRAGMA synchronous=NORMAL;\n\
             PRAGMA foreign_keys=ON;",
        )?;
        Ok(connection)
    }

    fn migrate_live_tables(&self) -> Result<(), LiveProjectionWorkError> {
        let connection = self.connection()?;
        connection.execute_batch(
            "BEGIN IMMEDIATE;\n\
             CREATE TABLE IF NOT EXISTS projection_resume_inputs (\n\
               request_id TEXT PRIMARY KEY REFERENCES projection_requests(request_id) ON DELETE CASCADE,\n\
               input_json TEXT NOT NULL,\n\
               updated_at TEXT NOT NULL\n\
             );\n\
             CREATE TABLE IF NOT EXISTS projection_candidate_payloads (\n\
               candidate_id TEXT PRIMARY KEY,\n\
               request_id TEXT NOT NULL REFERENCES projection_requests(request_id) ON DELETE CASCADE,\n\
               evidence_acceptance_key TEXT NOT NULL,\n\
               candidate_json TEXT NOT NULL,\n\
               updated_at TEXT NOT NULL\n\
             );\n\
             CREATE INDEX IF NOT EXISTS projection_candidate_payloads_request_idx
\
                ON projection_candidate_payloads(request_id, updated_at);
\
             CREATE TABLE IF NOT EXISTS candidate_suppressions (
\
               semantic_fingerprint TEXT PRIMARY KEY,
\
               candidate_id TEXT NOT NULL,
\
               evidence_acceptance_key TEXT NOT NULL,
\
               source_uri TEXT NOT NULL,
\
               content_digest TEXT NOT NULL,
\
               reason TEXT NOT NULL,
\
               expires_at TEXT,
\
               created_at TEXT NOT NULL,
\
               updated_at TEXT NOT NULL
\
             );
\
             CREATE INDEX IF NOT EXISTS candidate_suppressions_candidate_idx
\
               ON candidate_suppressions(candidate_id);
\
             COMMIT;",
        )?;
        Ok(())
    }

    pub fn request_by_acceptance_key(
        &self,
        acceptance_key: &str,
    ) -> Result<Option<ProjectionWorkRequest>, LiveProjectionWorkError> {
        let connection = self.connection()?;
        let request_id: Option<String> = connection
            .query_row(
                "SELECT request_id FROM projection_requests WHERE evidence_acceptance_key = ?1",
                [acceptance_key],
                |row| row.get(0),
            )
            .optional()?;
        let Some(request_id) = request_id else {
            return Ok(None);
        };
        let request_id = Uuid::parse_str(&request_id)
            .map_err(|_| LiveProjectionWorkError::InvalidRequestId(request_id))?;
        Ok(self.repository.get_request(request_id)?)
    }

    /// Claim one exact request, including recovery of an expired running lease.
    pub fn claim_request(
        &self,
        request_id: Uuid,
        now: DateTime<Utc>,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<Option<ProjectionWorkRequest>, LiveProjectionWorkError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = transaction.execute(
            "UPDATE projection_requests SET\n\
               status = 'running', attempt = attempt + 1, lease_expires_at = ?2,\n\
               last_error_code = NULL, updated_at = ?3\n\
             WHERE request_id = ?1 AND (\n\
               (status IN ('pending','deferred') AND available_at <= ?3) OR\n\
               (status = 'running' AND lease_expires_at IS NOT NULL AND lease_expires_at <= ?3)\n\
             )",
            params![
                request_id.to_string(),
                lease_expires_at.to_rfc3339(),
                now.to_rfc3339(),
            ],
        )?;
        transaction.commit()?;
        if changed == 0 {
            return Ok(None);
        }
        Ok(self.repository.get_request(request_id)?)
    }

    pub fn store_resume_input(
        &self,
        request_id: Uuid,
        input: &ProjectionResumeInput,
        now: DateTime<Utc>,
    ) -> Result<(), LiveProjectionWorkError> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO projection_resume_inputs(request_id, input_json, updated_at)\n\
             VALUES(?1, ?2, ?3)\n\
             ON CONFLICT(request_id) DO UPDATE SET\n\
               input_json = excluded.input_json, updated_at = excluded.updated_at",
            params![
                request_id.to_string(),
                serde_json::to_string(input)?,
                now.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn load_resume_input(
        &self,
        request_id: Uuid,
    ) -> Result<Option<ProjectionResumeInput>, LiveProjectionWorkError> {
        let connection = self.connection()?;
        let input_json: Option<String> = connection
            .query_row(
                "SELECT input_json FROM projection_resume_inputs WHERE request_id = ?1",
                [request_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        input_json
            .map(|value| serde_json::from_str(&value).map_err(LiveProjectionWorkError::from))
            .transpose()
    }

    pub fn store_candidate_payload(
        &self,
        request_id: Uuid,
        acceptance_key: &str,
        candidate: &DecisionCandidate,
        now: DateTime<Utc>,
    ) -> Result<(), LiveProjectionWorkError> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO projection_candidate_payloads(\n\
               candidate_id, request_id, evidence_acceptance_key, candidate_json, updated_at\n\
             ) VALUES(?1, ?2, ?3, ?4, ?5)\n\
             ON CONFLICT(candidate_id) DO UPDATE SET\n\
               request_id = excluded.request_id,\n\
               evidence_acceptance_key = excluded.evidence_acceptance_key,\n\
               candidate_json = excluded.candidate_json,\n\
               updated_at = excluded.updated_at",
            params![
                candidate.id.to_string(),
                request_id.to_string(),
                acceptance_key,
                serde_json::to_string(candidate)?,
                now.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn hide_candidate(
        &self,
        scope: &str,
        candidate_id: Uuid,
    ) -> Result<bool, LiveProjectionWorkError> {
        if self.candidate_payload(candidate_id)?.is_none() {
            return Ok(false);
        }
        let mut hidden = self
            .hidden_candidates
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        hidden
            .entry(scope.to_string())
            .or_default()
            .insert(candidate_id);
        Ok(true)
    }

    pub fn is_hidden(&self, scope: &str, candidate_id: Uuid) -> bool {
        self.hidden_candidates
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(scope)
            .is_some_and(|ids| ids.contains(&candidate_id))
    }

    pub fn visible_candidate_payloads(
        &self,
        scope: &str,
    ) -> Result<Vec<DecisionCandidate>, LiveProjectionWorkError> {
        let hidden = self
            .hidden_candidates
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(scope)
            .cloned()
            .unwrap_or_default();
        Ok(self
            .active_candidate_payloads(Utc::now())?
            .into_iter()
            .filter(|candidate| !hidden.contains(&candidate.id))
            .collect())
    }

    pub fn suppress_candidate(
        &self,
        candidate_id: Uuid,
        reason: &str,
        expires_at: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> Result<Option<CandidateSuppression>, LiveProjectionWorkError> {
        if expires_at.is_some_and(|expiry| expiry <= now) {
            return Err(LiveProjectionWorkError::InvalidSuppressionExpiry);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row: Option<(String, String, String, String)> = transaction
            .query_row(
                "SELECT wc.semantic_fingerprint, wc.request_id, wc.evidence_acceptance_key, pr.source_uri \
                 FROM working_candidates wc \
                 JOIN projection_requests pr ON pr.request_id = wc.request_id \
                 WHERE wc.candidate_id = ?1",
                [candidate_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let Some((semantic_fingerprint, request_id, evidence_acceptance_key, source_uri)) = row
        else {
            transaction.commit()?;
            return Ok(None);
        };
        let input_json: Option<String> = transaction
            .query_row(
                "SELECT input_json FROM projection_resume_inputs WHERE request_id = ?1",
                [&request_id],
                |row| row.get(0),
            )
            .optional()?;
        let input_json = input_json
            .ok_or_else(|| LiveProjectionWorkError::MissingResumeInput(request_id.clone()))?;
        let resume: ProjectionResumeInput = serde_json::from_str(&input_json)?;
        transaction.execute(
            r#"INSERT INTO candidate_suppressions(
      semantic_fingerprint, candidate_id, evidence_acceptance_key, source_uri,
      content_digest, reason, expires_at, created_at, updated_at
    ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
    ON CONFLICT(semantic_fingerprint) DO UPDATE SET
      candidate_id = excluded.candidate_id,
      evidence_acceptance_key = excluded.evidence_acceptance_key,
      source_uri = excluded.source_uri,
      content_digest = excluded.content_digest,
      reason = excluded.reason,
      expires_at = excluded.expires_at,
      updated_at = excluded.updated_at"#,
            params![
                semantic_fingerprint,
                candidate_id.to_string(),
                evidence_acceptance_key,
                source_uri,
                resume.content_digest,
                reason,
                expires_at.map(|value| value.to_rfc3339()),
                now.to_rfc3339(),
            ],
        )?;
        transaction.execute(
            "UPDATE working_candidates SET state = 'suppressed' WHERE candidate_id = ?1",
            [candidate_id.to_string()],
        )?;
        transaction.commit()?;
        Ok(Some(CandidateSuppression {
            candidate_id,
            semantic_fingerprint,
            evidence_acceptance_key,
            source_uri,
            content_digest: resume.content_digest,
            reason: reason.to_string(),
            expires_at,
            created_at: now,
        }))
    }

    pub fn suppression_applies(
        &self,
        semantic_fingerprint: &str,
        evidence_acceptance_key: &str,
        source_uri: &str,
        content_digest: &str,
        now: DateTime<Utc>,
    ) -> Result<bool, LiveProjectionWorkError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row: Option<(String, String, String, Option<String>)> = transaction
            .query_row(
                "SELECT evidence_acceptance_key, source_uri, content_digest, expires_at \
                 FROM candidate_suppressions WHERE semantic_fingerprint = ?1",
                [semantic_fingerprint],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let Some((stored_acceptance, stored_source, stored_digest, stored_expiry)) = row else {
            transaction.commit()?;
            return Ok(false);
        };
        let expired = stored_expiry
            .as_deref()
            .map(DateTime::parse_from_rfc3339)
            .transpose()
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?
            .is_some_and(|expiry| expiry.with_timezone(&Utc) <= now);
        let invalidated = expired
            || stored_acceptance != evidence_acceptance_key
            || stored_source != source_uri
            || stored_digest != content_digest;
        if invalidated {
            transaction.execute(
                "DELETE FROM candidate_suppressions WHERE semantic_fingerprint = ?1",
                [semantic_fingerprint],
            )?;
            transaction.execute(
                "UPDATE working_candidates SET state = 'active' WHERE semantic_fingerprint = ?1",
                [semantic_fingerprint],
            )?;
            transaction.commit()?;
            return Ok(false);
        }
        transaction.commit()?;
        Ok(true)
    }

    pub fn candidate_payload(
        &self,
        candidate_id: Uuid,
    ) -> Result<Option<DecisionCandidate>, LiveProjectionWorkError> {
        let connection = self.connection()?;
        let candidate_json: Option<String> = connection
            .query_row(
                "SELECT candidate_json FROM projection_candidate_payloads WHERE candidate_id = ?1",
                [candidate_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        candidate_json
            .map(|value| serde_json::from_str(&value).map_err(LiveProjectionWorkError::from))
            .transpose()
    }

    pub fn all_candidate_payloads(
        &self,
    ) -> Result<Vec<DecisionCandidate>, LiveProjectionWorkError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT candidate_json FROM projection_candidate_payloads ORDER BY updated_at ASC, candidate_id ASC",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let value = row?;
            Ok(serde_json::from_str::<DecisionCandidate>(&value)?)
        })
        .collect()
    }

    pub fn active_candidate_payloads(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<DecisionCandidate>, LiveProjectionWorkError> {
        let connection = self.connection()?;
        connection.execute(
            "DELETE FROM candidate_suppressions WHERE expires_at IS NOT NULL AND expires_at <= ?1",
            [now.to_rfc3339()],
        )?;
        connection.execute(
            "UPDATE working_candidates SET state = 'active'              WHERE state = 'suppressed' AND semantic_fingerprint NOT IN                (SELECT semantic_fingerprint FROM candidate_suppressions)",
            [],
        )?;
        let mut statement = connection.prepare(
            "SELECT p.candidate_json FROM projection_candidate_payloads p              JOIN working_candidates w ON w.candidate_id = p.candidate_id              WHERE w.state = 'active'              ORDER BY p.updated_at ASC, p.candidate_id ASC",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let value = row?;
            Ok(serde_json::from_str::<DecisionCandidate>(&value)?)
        })
        .collect()
    }

    pub fn candidate_payloads(
        &self,
        request_id: Uuid,
    ) -> Result<Vec<DecisionCandidate>, LiveProjectionWorkError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT p.candidate_json FROM projection_candidate_payloads p              JOIN working_candidates w ON w.candidate_id = p.candidate_id              WHERE p.request_id = ?1 AND w.state = 'active'              ORDER BY p.updated_at ASC, p.candidate_id ASC",
        )?;
        let rows = statement.query_map([request_id.to_string()], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let value = row?;
            Ok(serde_json::from_str::<DecisionCandidate>(&value)?)
        })
        .collect()
    }

    fn latest_outcome(
        &self,
        request_id: Uuid,
    ) -> Result<Option<ProjectionWorkOutcome>, LiveProjectionWorkError> {
        Ok(self.repository.list_outcomes(request_id)?.pop())
    }
}

pub fn execute_external_projection(
    state: &AppState,
    acceptance: &EvidenceAcceptedPayload,
    envelope: &ExternalIngestEnvelope,
    evidence_records: &[SourceEvidence],
    provenance: &SubmissionProvenance,
    default_level: DecisionLevel,
) -> Result<ProjectionExecution, LiveProjectionWorkError> {
    let hint_inputs = envelope
        .candidate_hints
        .iter()
        .map(|hint| CandidateHintInput {
            title: hint.title.clone(),
            body: hint.body.clone(),
            level: hint.level.clone(),
            labels: hint.labels.clone(),
        })
        .collect::<Vec<_>>();
    let evidence_excerpts = envelope
        .evidence
        .iter()
        .map(|evidence| evidence.excerpt.clone())
        .collect::<Vec<_>>();
    let projection_request = projection_request_from_external_ingest(
        &envelope.source_uri,
        &envelope.source_system,
        &envelope.content,
        &evidence_excerpts,
        &hint_inputs,
    );

    let Some(store) = state.projection_work_store.as_ref() else {
        let result = state.projection_provider.project(&projection_request);
        return Ok(ProjectionExecution {
            projection_request_id: None,
            outcome_label: result.outcome_label().to_string(),
            candidates: materialize_candidates(
                &result,
                default_level,
                &envelope.source_uri,
                evidence_records,
                provenance,
            ),
        });
    };

    let now = Utc::now();
    let resume_input = ProjectionResumeInput {
        content_locator: acceptance.content_locator.clone(),
        content_digest: acceptance.content_digest.clone(),
        evidence_excerpts,
        candidate_hints: hint_inputs,
    };
    let pending = ProjectionWorkRequest::pending(
        acceptance.idempotency_key.clone(),
        envelope.source_system.clone(),
        envelope.source_uri.clone(),
        now,
    );
    let inserted = store.repository().enqueue(&pending)?;
    let request = if inserted {
        store.store_resume_input(pending.request_id, &resume_input, now)?;
        pending
    } else {
        let existing = store
            .request_by_acceptance_key(&acceptance.idempotency_key)?
            .ok_or_else(|| {
                ProjectionWorkRepositoryError::RequestNotFound(acceptance.idempotency_key.clone())
            })?;
        if store.load_resume_input(existing.request_id)?.is_none() {
            store.store_resume_input(existing.request_id, &resume_input, now)?;
        }
        existing
    };

    if matches!(
        request.status,
        ProjectionRequestStatus::Completed | ProjectionRequestStatus::Failed
    ) {
        let outcome_label = store
            .latest_outcome(request.request_id)?
            .and_then(|outcome| {
                outcome
                    .details
                    .get("outcome_label")
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| request.status.label().to_string());
        return Ok(ProjectionExecution {
            projection_request_id: Some(request.request_id),
            outcome_label,
            candidates: store.candidate_payloads(request.request_id)?,
        });
    }

    let Some(claimed) = store.claim_request(
        request.request_id,
        now,
        now + ChronoDuration::minutes(CLAIM_LEASE_MINUTES),
    )?
    else {
        let outcome_label = store
            .latest_outcome(request.request_id)?
            .and_then(|outcome| {
                outcome
                    .details
                    .get("outcome_label")
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "projection_in_progress".to_string());
        return Ok(ProjectionExecution {
            projection_request_id: Some(request.request_id),
            outcome_label,
            candidates: store.candidate_payloads(request.request_id)?,
        });
    };

    let result = state.projection_provider.project(&projection_request);
    let projected_candidates = materialize_candidates(
        &result,
        default_level,
        &envelope.source_uri,
        evidence_records,
        provenance,
    );
    let mut candidates = Vec::new();
    let mut suppressed_fingerprints = Vec::new();
    for mut candidate in projected_candidates {
        let fingerprint = semantic_fingerprint(&candidate);
        if store.suppression_applies(
            &fingerprint,
            &acceptance.idempotency_key,
            &envelope.source_uri,
            &acceptance.content_digest,
            now,
        )? {
            suppressed_fingerprints.push(fingerprint);
            continue;
        }
        let working = WorkingCandidate::active(
            claimed.request_id,
            acceptance.idempotency_key.clone(),
            fingerprint,
            candidate.title.clone(),
            candidate.description.clone(),
            decision_level_label(candidate.level),
            now,
        );
        let converged = store.repository().converge_candidate(&working)?;
        candidate.id = converged.candidate_id;
        store.store_candidate_payload(
            claimed.request_id,
            &acceptance.idempotency_key,
            &candidate,
            now,
        )?;
        candidates.push(candidate);
    }
    let outcome_label = if candidates.is_empty() && !suppressed_fingerprints.is_empty() {
        "candidate_suppressed".to_string()
    } else {
        result.outcome_label().to_string()
    };

    let (kind, reason_code, retryable, retry_at) = outcome_metadata(&result, now);
    let outcome = ProjectionWorkOutcome {
        request_id: claimed.request_id,
        attempt: claimed.attempt,
        kind,
        reason_code,
        retryable,
        details: serde_json::json!({
            "outcome_label": outcome_label,
            "projection_id": result.projection_id,
            "reasoning_mode": result.reasoning_mode,
            "candidate_ids": candidates.iter().map(|candidate| candidate.id.to_string()).collect::<Vec<_>>(),
            "suppressed_fingerprints": suppressed_fingerprints,
            "decision_likeness": result.decision_likeness.clone(),
            "detail": result.detail.clone(),
        }),
        completed_at: now,
    };
    store.repository().record_outcome(&outcome, retry_at)?;

    Ok(ProjectionExecution {
        projection_request_id: Some(claimed.request_id),
        outcome_label,
        candidates,
    })
}

fn materialize_candidates(
    result: &ProjectionResult,
    default_level: DecisionLevel,
    source_uri: &str,
    evidence_records: &[SourceEvidence],
    provenance: &SubmissionProvenance,
) -> Vec<DecisionCandidate> {
    if !result.outcome.candidates_materialized() {
        return Vec::new();
    }
    result
        .candidates
        .iter()
        .map(|proposal| {
            let level = proposal
                .suggested_level
                .as_deref()
                .and_then(parse_decision_level)
                .unwrap_or(default_level);
            let mut candidate = DecisionCandidate::new(
                proposal.title.clone(),
                proposal.body.clone(),
                level,
                source_uri.to_string(),
            );
            candidate.tags = proposal.tags.clone();
            candidate.evidence_refs = evidence_records
                .iter()
                .map(|evidence| evidence.id)
                .collect();
            candidate.provenance = Some(provenance.clone());
            candidate.extracted_by = "daemon:reasoning_projection".to_string();
            candidate
        })
        .collect()
}

fn parse_decision_level(value: &str) -> Option<DecisionLevel> {
    match value {
        "product" => Some(DecisionLevel::Product),
        "architecture" => Some(DecisionLevel::Architecture),
        "implementation" => Some(DecisionLevel::Implementation),
        _ => None,
    }
}

fn decision_level_label(level: DecisionLevel) -> &'static str {
    match level {
        DecisionLevel::Product => "product",
        DecisionLevel::Architecture => "architecture",
        DecisionLevel::Implementation => "implementation",
    }
}

fn semantic_fingerprint(candidate: &DecisionCandidate) -> String {
    let material = format!(
        "{}\n{}\n{}",
        candidate.title.trim().to_lowercase(),
        candidate.description.trim().to_lowercase(),
        decision_level_label(candidate.level),
    );
    format!("sha256:{:x}", Sha256::digest(material.as_bytes()))
}

fn outcome_metadata(
    result: &ProjectionResult,
    now: DateTime<Utc>,
) -> (
    ProjectionOutcomeKind,
    Option<String>,
    bool,
    Option<DateTime<Utc>>,
) {
    match &result.outcome {
        AlphaProjectionOutcome::CandidateProjected { .. } => {
            (ProjectionOutcomeKind::Candidate, None, false, None)
        }
        AlphaProjectionOutcome::NoCandidate => {
            (ProjectionOutcomeKind::NoCandidate, None, false, None)
        }
        AlphaProjectionOutcome::ProjectionDeferred { reason } => {
            let (reason_code, retryable) = match reason {
                ProjectionDeferralReason::ProviderUnavailable { .. } => {
                    ("provider_unavailable", true)
                }
                ProjectionDeferralReason::ProviderTimeout { .. } => ("provider_timeout", true),
                ProjectionDeferralReason::ProviderConfigMissing => {
                    ("provider_config_missing", false)
                }
                ProjectionDeferralReason::ProviderConfigInvalid { .. } => {
                    ("provider_config_invalid", false)
                }
                ProjectionDeferralReason::ProviderInvalidOutput { .. } => {
                    ("provider_invalid_output", false)
                }
            };
            (
                ProjectionOutcomeKind::Deferred,
                Some(reason_code.to_string()),
                retryable,
                retryable.then_some(now + ChronoDuration::seconds(RETRY_DELAY_SECONDS)),
            )
        }
        AlphaProjectionOutcome::HardRejected { .. } => (
            ProjectionOutcomeKind::Failed,
            Some("hard_rejected".to_string()),
            false,
            None,
        ),
    }
}

trait ProjectionRequestStatusLabel {
    fn label(self) -> &'static str;
}

impl ProjectionRequestStatusLabel for ProjectionRequestStatus {
    fn label(self) -> &'static str {
        match self {
            ProjectionRequestStatus::Pending => "projection_pending",
            ProjectionRequestStatus::Running => "projection_in_progress",
            ProjectionRequestStatus::Deferred => "projection_deferred",
            ProjectionRequestStatus::Completed => "projection_completed",
            ProjectionRequestStatus::Failed => "projection_failed",
        }
    }
}
