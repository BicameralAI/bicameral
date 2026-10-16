//! Daemon-local SQLite working state for projection and candidate lifecycle.
//!
//! This repository is explicitly non-canonical. It stores retryable projection
//! work, typed outcomes, and converged candidate proposals. Canonical Product
//! replay never rebuilds these rows and never invokes a model.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use uuid::Uuid;

pub const SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Error)]
pub enum ProjectionWorkRepositoryError {
    #[error("projection working-state database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("projection working-state timestamp is invalid: {0}")]
    Timestamp(String),
    #[error("projection working-state JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("projection work request not found: {0}")]
    RequestNotFound(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionRequestStatus {
    Pending,
    Running,
    Deferred,
    Completed,
    Failed,
}

impl ProjectionRequestStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Deferred => "deferred",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    fn parse(value: &str) -> Result<Self, rusqlite::Error> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "deferred" => Ok(Self::Deferred),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            other => Err(rusqlite::Error::InvalidParameterName(format!(
                "unknown projection request status: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionOutcomeKind {
    Candidate,
    NoCandidate,
    Deferred,
    Failed,
}

impl ProjectionOutcomeKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Candidate => "candidate",
            Self::NoCandidate => "no_candidate",
            Self::Deferred => "deferred",
            Self::Failed => "failed",
        }
    }

    fn parse(value: &str) -> Result<Self, rusqlite::Error> {
        match value {
            "candidate" => Ok(Self::Candidate),
            "no_candidate" => Ok(Self::NoCandidate),
            "deferred" => Ok(Self::Deferred),
            "failed" => Ok(Self::Failed),
            other => Err(rusqlite::Error::InvalidParameterName(format!(
                "unknown projection outcome kind: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkingCandidateState {
    Active,
    Suppressed,
    Rejected,
    Promoted,
}

impl WorkingCandidateState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Suppressed => "suppressed",
            Self::Rejected => "rejected",
            Self::Promoted => "promoted",
        }
    }

    fn parse(value: &str) -> Result<Self, rusqlite::Error> {
        match value {
            "active" => Ok(Self::Active),
            "suppressed" => Ok(Self::Suppressed),
            "rejected" => Ok(Self::Rejected),
            "promoted" => Ok(Self::Promoted),
            other => Err(rusqlite::Error::InvalidParameterName(format!(
                "unknown working candidate state: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectionWorkRequest {
    pub request_id: Uuid,
    pub evidence_acceptance_key: String,
    pub product_scope: Option<String>,
    pub source_system: String,
    pub source_uri: String,
    pub status: ProjectionRequestStatus,
    pub attempt: u32,
    pub available_at: DateTime<Utc>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub last_error_code: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ProjectionWorkRequest {
    pub fn pending(
        evidence_acceptance_key: impl Into<String>,
        source_system: impl Into<String>,
        source_uri: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            request_id: Uuid::new_v4(),
            evidence_acceptance_key: evidence_acceptance_key.into(),
            product_scope: None,
            source_system: source_system.into(),
            source_uri: source_uri.into(),
            status: ProjectionRequestStatus::Pending,
            attempt: 0,
            available_at: now,
            lease_expires_at: None,
            last_error_code: None,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectionWorkOutcome {
    pub request_id: Uuid,
    pub attempt: u32,
    pub kind: ProjectionOutcomeKind,
    pub reason_code: Option<String>,
    pub retryable: bool,
    pub details: Value,
    pub completed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkingCandidate {
    pub candidate_id: Uuid,
    pub request_id: Uuid,
    pub evidence_acceptance_key: String,
    pub semantic_fingerprint: String,
    pub title: String,
    pub body: String,
    pub level: String,
    pub state: WorkingCandidateState,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

impl WorkingCandidate {
    pub fn active(
        request_id: Uuid,
        evidence_acceptance_key: impl Into<String>,
        semantic_fingerprint: impl Into<String>,
        title: impl Into<String>,
        body: impl Into<String>,
        level: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            candidate_id: Uuid::new_v4(),
            request_id,
            evidence_acceptance_key: evidence_acceptance_key.into(),
            semantic_fingerprint: semantic_fingerprint.into(),
            title: title.into(),
            body: body.into(),
            level: level.into(),
            state: WorkingCandidateState::Active,
            first_seen_at: now,
            last_seen_at: now,
        }
    }
}

pub trait ProjectionWorkRepository: Send + Sync {
    fn enqueue(
        &self,
        request: &ProjectionWorkRequest,
    ) -> Result<bool, ProjectionWorkRepositoryError>;
    fn claim_next(
        &self,
        now: DateTime<Utc>,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<Option<ProjectionWorkRequest>, ProjectionWorkRepositoryError>;
    fn record_outcome(
        &self,
        outcome: &ProjectionWorkOutcome,
        retry_at: Option<DateTime<Utc>>,
    ) -> Result<(), ProjectionWorkRepositoryError>;
    fn get_request(
        &self,
        request_id: Uuid,
    ) -> Result<Option<ProjectionWorkRequest>, ProjectionWorkRepositoryError>;
    fn list_outcomes(
        &self,
        request_id: Uuid,
    ) -> Result<Vec<ProjectionWorkOutcome>, ProjectionWorkRepositoryError>;
    fn converge_candidate(
        &self,
        candidate: &WorkingCandidate,
    ) -> Result<WorkingCandidate, ProjectionWorkRepositoryError>;
    fn get_candidate_by_fingerprint(
        &self,
        semantic_fingerprint: &str,
    ) -> Result<Option<WorkingCandidate>, ProjectionWorkRepositoryError>;
}

#[derive(Debug, Clone)]
pub struct SqliteProjectionWorkRepository {
    path: PathBuf,
}

impl SqliteProjectionWorkRepository {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, ProjectionWorkRepositoryError> {
        let repository = Self { path: path.into() };
        repository.migrate()?;
        Ok(repository)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn connection(&self) -> Result<Connection, ProjectionWorkRepositoryError> {
        let connection = Connection::open(&self.path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL;\n\
             PRAGMA synchronous=NORMAL;\n\
             PRAGMA foreign_keys=ON;",
        )?;
        Ok(connection)
    }

    fn migrate(&self) -> Result<(), ProjectionWorkRepositoryError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                ProjectionWorkRepositoryError::Sqlite(rusqlite::Error::ToSqlConversionFailure(
                    Box::new(error),
                ))
            })?;
        }
        let connection = self.connection()?;
        connection.execute_batch(
            "BEGIN IMMEDIATE;\n\
             CREATE TABLE IF NOT EXISTS working_schema_version (\n\
               singleton INTEGER PRIMARY KEY CHECK (singleton = 1),\n\
               version INTEGER NOT NULL\n\
             );\n\
             INSERT INTO working_schema_version(singleton, version) VALUES(1, 1)\n\
               ON CONFLICT(singleton) DO UPDATE SET version = excluded.version;\n\
             CREATE TABLE IF NOT EXISTS projection_requests (\n\
               request_id TEXT PRIMARY KEY,\n\
               evidence_acceptance_key TEXT NOT NULL UNIQUE,\n\
               product_scope TEXT,\n\
               source_system TEXT NOT NULL,\n\
               source_uri TEXT NOT NULL,\n\
               status TEXT NOT NULL CHECK(status IN ('pending','running','deferred','completed','failed')),\n\
               attempt INTEGER NOT NULL DEFAULT 0 CHECK(attempt >= 0),\n\
               available_at TEXT NOT NULL,\n\
               lease_expires_at TEXT,\n\
               last_error_code TEXT,\n\
               created_at TEXT NOT NULL,\n\
               updated_at TEXT NOT NULL\n\
             );\n\
             CREATE INDEX IF NOT EXISTS projection_requests_ready_idx\n\
               ON projection_requests(status, available_at, created_at);\n\
             CREATE TABLE IF NOT EXISTS projection_outcomes (\n\
               request_id TEXT NOT NULL REFERENCES projection_requests(request_id) ON DELETE CASCADE,\n\
               attempt INTEGER NOT NULL CHECK(attempt > 0),\n\
               kind TEXT NOT NULL CHECK(kind IN ('candidate','no_candidate','deferred','failed')),\n\
               reason_code TEXT,\n\
               retryable INTEGER NOT NULL CHECK(retryable IN (0,1)),\n\
               details_json TEXT NOT NULL,\n\
               completed_at TEXT NOT NULL,\n\
               PRIMARY KEY(request_id, attempt)\n\
             );\n\
             CREATE TABLE IF NOT EXISTS working_candidates (\n\
               candidate_id TEXT PRIMARY KEY,\n\
               request_id TEXT NOT NULL REFERENCES projection_requests(request_id),\n\
               evidence_acceptance_key TEXT NOT NULL,\n\
               semantic_fingerprint TEXT NOT NULL UNIQUE,\n\
               title TEXT NOT NULL,\n\
               body TEXT NOT NULL,\n\
               level TEXT NOT NULL,\n\
               state TEXT NOT NULL CHECK(state IN ('active','suppressed','rejected','promoted')),\n\
               first_seen_at TEXT NOT NULL,\n\
               last_seen_at TEXT NOT NULL\n\
             );\n\
             COMMIT;",
        )?;
        Ok(())
    }

    pub fn schema_version(&self) -> Result<i64, ProjectionWorkRepositoryError> {
        let connection = self.connection()?;
        Ok(connection.query_row(
            "SELECT version FROM working_schema_version WHERE singleton = 1",
            [],
            |row| row.get(0),
        )?)
    }

    pub fn journal_mode(&self) -> Result<String, ProjectionWorkRepositoryError> {
        let connection = self.connection()?;
        Ok(connection.query_row("PRAGMA journal_mode", [], |row| row.get(0))?)
    }
}

fn timestamp(value: &str) -> Result<DateTime<Utc>, rusqlite::Error> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })
}

fn optional_timestamp(value: Option<String>) -> Result<Option<DateTime<Utc>>, rusqlite::Error> {
    value.as_deref().map(timestamp).transpose()
}

fn request_from_row(row: &rusqlite::Row<'_>) -> Result<ProjectionWorkRequest, rusqlite::Error> {
    let request_id: String = row.get("request_id")?;
    let status: String = row.get("status")?;
    let available_at: String = row.get("available_at")?;
    let lease_expires_at: Option<String> = row.get("lease_expires_at")?;
    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;
    Ok(ProjectionWorkRequest {
        request_id: Uuid::parse_str(&request_id).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        evidence_acceptance_key: row.get("evidence_acceptance_key")?,
        product_scope: row.get("product_scope")?,
        source_system: row.get("source_system")?,
        source_uri: row.get("source_uri")?,
        status: ProjectionRequestStatus::parse(&status)?,
        attempt: row.get::<_, u32>("attempt")?,
        available_at: timestamp(&available_at)?,
        lease_expires_at: optional_timestamp(lease_expires_at)?,
        last_error_code: row.get("last_error_code")?,
        created_at: timestamp(&created_at)?,
        updated_at: timestamp(&updated_at)?,
    })
}

fn outcome_from_row(row: &rusqlite::Row<'_>) -> Result<ProjectionWorkOutcome, rusqlite::Error> {
    let request_id: String = row.get("request_id")?;
    let kind: String = row.get("kind")?;
    let details: String = row.get("details_json")?;
    let completed_at: String = row.get("completed_at")?;
    Ok(ProjectionWorkOutcome {
        request_id: Uuid::parse_str(&request_id).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        attempt: row.get::<_, u32>("attempt")?,
        kind: ProjectionOutcomeKind::parse(&kind)?,
        reason_code: row.get("reason_code")?,
        retryable: row.get::<_, i64>("retryable")? != 0,
        details: serde_json::from_str(&details).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        completed_at: timestamp(&completed_at)?,
    })
}

fn candidate_from_row(row: &rusqlite::Row<'_>) -> Result<WorkingCandidate, rusqlite::Error> {
    let candidate_id: String = row.get("candidate_id")?;
    let request_id: String = row.get("request_id")?;
    let state: String = row.get("state")?;
    let first_seen_at: String = row.get("first_seen_at")?;
    let last_seen_at: String = row.get("last_seen_at")?;
    Ok(WorkingCandidate {
        candidate_id: Uuid::parse_str(&candidate_id).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        request_id: Uuid::parse_str(&request_id).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        evidence_acceptance_key: row.get("evidence_acceptance_key")?,
        semantic_fingerprint: row.get("semantic_fingerprint")?,
        title: row.get("title")?,
        body: row.get("body")?,
        level: row.get("level")?,
        state: WorkingCandidateState::parse(&state)?,
        first_seen_at: timestamp(&first_seen_at)?,
        last_seen_at: timestamp(&last_seen_at)?,
    })
}

impl ProjectionWorkRepository for SqliteProjectionWorkRepository {
    fn enqueue(
        &self,
        request: &ProjectionWorkRequest,
    ) -> Result<bool, ProjectionWorkRepositoryError> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "INSERT OR IGNORE INTO projection_requests(\n\
               request_id, evidence_acceptance_key, product_scope, source_system, source_uri,\n\
               status, attempt, available_at, lease_expires_at, last_error_code, created_at, updated_at\n\
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                request.request_id.to_string(),
                request.evidence_acceptance_key,
                request.product_scope,
                request.source_system,
                request.source_uri,
                request.status.as_str(),
                request.attempt,
                request.available_at.to_rfc3339(),
                request.lease_expires_at.map(|value| value.to_rfc3339()),
                request.last_error_code,
                request.created_at.to_rfc3339(),
                request.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(changed == 1)
    }

    fn claim_next(
        &self,
        now: DateTime<Utc>,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<Option<ProjectionWorkRequest>, ProjectionWorkRepositoryError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let request_id: Option<String> = transaction
            .query_row(
                "SELECT request_id FROM projection_requests\n\
                 WHERE status IN ('pending','deferred') AND available_at <= ?1\n\
                 ORDER BY available_at ASC, created_at ASC LIMIT 1",
                [now.to_rfc3339()],
                |row| row.get(0),
            )
            .optional()?;
        let Some(request_id) = request_id else {
            transaction.commit()?;
            return Ok(None);
        };
        transaction.execute(
            "UPDATE projection_requests SET\n\
               status = 'running', attempt = attempt + 1, lease_expires_at = ?2,\n\
               last_error_code = NULL, updated_at = ?3\n\
             WHERE request_id = ?1",
            params![request_id, lease_expires_at.to_rfc3339(), now.to_rfc3339()],
        )?;
        let request = transaction.query_row(
            "SELECT * FROM projection_requests WHERE request_id = ?1",
            [&request_id],
            request_from_row,
        )?;
        transaction.commit()?;
        Ok(Some(request))
    }

    fn record_outcome(
        &self,
        outcome: &ProjectionWorkOutcome,
        retry_at: Option<DateTime<Utc>>,
    ) -> Result<(), ProjectionWorkRepositoryError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM projection_requests WHERE request_id = ?1)",
            [outcome.request_id.to_string()],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(ProjectionWorkRepositoryError::RequestNotFound(
                outcome.request_id.to_string(),
            ));
        }
        transaction.execute(
            "INSERT INTO projection_outcomes(\n\
               request_id, attempt, kind, reason_code, retryable, details_json, completed_at\n\
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)\n\
             ON CONFLICT(request_id, attempt) DO UPDATE SET\n\
               kind = excluded.kind, reason_code = excluded.reason_code,\n\
               retryable = excluded.retryable, details_json = excluded.details_json,\n\
               completed_at = excluded.completed_at",
            params![
                outcome.request_id.to_string(),
                outcome.attempt,
                outcome.kind.as_str(),
                outcome.reason_code,
                i64::from(outcome.retryable),
                serde_json::to_string(&outcome.details)?,
                outcome.completed_at.to_rfc3339(),
            ],
        )?;
        let (status, available_at) = match outcome.kind {
            ProjectionOutcomeKind::Candidate | ProjectionOutcomeKind::NoCandidate => {
                (ProjectionRequestStatus::Completed, outcome.completed_at)
            }
            ProjectionOutcomeKind::Deferred if outcome.retryable => (
                ProjectionRequestStatus::Deferred,
                retry_at.unwrap_or(outcome.completed_at),
            ),
            ProjectionOutcomeKind::Deferred | ProjectionOutcomeKind::Failed => {
                (ProjectionRequestStatus::Failed, outcome.completed_at)
            }
        };
        transaction.execute(
            "UPDATE projection_requests SET\n\
               status = ?2, available_at = ?3, lease_expires_at = NULL,\n\
               last_error_code = ?4, updated_at = ?5\n\
             WHERE request_id = ?1",
            params![
                outcome.request_id.to_string(),
                status.as_str(),
                available_at.to_rfc3339(),
                outcome.reason_code,
                outcome.completed_at.to_rfc3339(),
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn get_request(
        &self,
        request_id: Uuid,
    ) -> Result<Option<ProjectionWorkRequest>, ProjectionWorkRepositoryError> {
        let connection = self.connection()?;
        Ok(connection
            .query_row(
                "SELECT * FROM projection_requests WHERE request_id = ?1",
                [request_id.to_string()],
                request_from_row,
            )
            .optional()?)
    }

    fn list_outcomes(
        &self,
        request_id: Uuid,
    ) -> Result<Vec<ProjectionWorkOutcome>, ProjectionWorkRepositoryError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT * FROM projection_outcomes WHERE request_id = ?1 ORDER BY attempt ASC",
        )?;
        let rows = statement.query_map([request_id.to_string()], outcome_from_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn converge_candidate(
        &self,
        candidate: &WorkingCandidate,
    ) -> Result<WorkingCandidate, ProjectionWorkRepositoryError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO working_candidates(\n\
               candidate_id, request_id, evidence_acceptance_key, semantic_fingerprint,\n\
               title, body, level, state, first_seen_at, last_seen_at\n\
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)\n\
             ON CONFLICT(semantic_fingerprint) DO UPDATE SET\n\
               request_id = excluded.request_id,\n\
               evidence_acceptance_key = excluded.evidence_acceptance_key,\n\
               title = excluded.title, body = excluded.body, level = excluded.level,\n\
               last_seen_at = excluded.last_seen_at",
            params![
                candidate.candidate_id.to_string(),
                candidate.request_id.to_string(),
                candidate.evidence_acceptance_key,
                candidate.semantic_fingerprint,
                candidate.title,
                candidate.body,
                candidate.level,
                candidate.state.as_str(),
                candidate.first_seen_at.to_rfc3339(),
                candidate.last_seen_at.to_rfc3339(),
            ],
        )?;
        let converged = transaction.query_row(
            "SELECT * FROM working_candidates WHERE semantic_fingerprint = ?1",
            [&candidate.semantic_fingerprint],
            candidate_from_row,
        )?;
        transaction.commit()?;
        Ok(converged)
    }

    fn get_candidate_by_fingerprint(
        &self,
        semantic_fingerprint: &str,
    ) -> Result<Option<WorkingCandidate>, ProjectionWorkRepositoryError> {
        let connection = self.connection()?;
        Ok(connection
            .query_row(
                "SELECT * FROM working_candidates WHERE semantic_fingerprint = ?1",
                [semantic_fingerprint],
                candidate_from_row,
            )
            .optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;

    fn repository() -> (tempfile::TempDir, SqliteProjectionWorkRepository) {
        let dir = tempfile::tempdir().unwrap();
        let repo =
            SqliteProjectionWorkRepository::open(dir.path().join("working.sqlite3")).unwrap();
        (dir, repo)
    }

    #[test]
    fn initializes_wal_schema() {
        let (_dir, repo) = repository();
        assert_eq!(repo.schema_version().unwrap(), SCHEMA_VERSION);
        assert_eq!(repo.journal_mode().unwrap().to_lowercase(), "wal");
    }

    #[test]
    fn enqueue_is_idempotent_and_claim_is_atomic() {
        let (_dir, repo) = repository();
        let now = Utc::now();
        let request = ProjectionWorkRequest::pending("acceptance-1", "github", "uri:1", now);
        assert!(repo.enqueue(&request).unwrap());
        assert!(!repo.enqueue(&request).unwrap());

        let claimed = repo
            .claim_next(now, now + ChronoDuration::minutes(5))
            .unwrap()
            .unwrap();
        assert_eq!(claimed.request_id, request.request_id);
        assert_eq!(claimed.status, ProjectionRequestStatus::Running);
        assert_eq!(claimed.attempt, 1);
        assert!(repo
            .claim_next(now, now + ChronoDuration::minutes(5))
            .unwrap()
            .is_none());
    }

    #[test]
    fn deferred_retry_survives_repository_reopen() {
        let (dir, repo) = repository();
        let now = Utc::now();
        let request = ProjectionWorkRequest::pending("acceptance-2", "github", "uri:2", now);
        repo.enqueue(&request).unwrap();
        let claimed = repo
            .claim_next(now, now + ChronoDuration::minutes(5))
            .unwrap()
            .unwrap();
        let retry_at = now + ChronoDuration::minutes(10);
        repo.record_outcome(
            &ProjectionWorkOutcome {
                request_id: claimed.request_id,
                attempt: claimed.attempt,
                kind: ProjectionOutcomeKind::Deferred,
                reason_code: Some("provider_unavailable".to_string()),
                retryable: true,
                details: serde_json::json!({"provider": "test"}),
                completed_at: now,
            },
            Some(retry_at),
        )
        .unwrap();
        drop(repo);

        let reopened =
            SqliteProjectionWorkRepository::open(dir.path().join("working.sqlite3")).unwrap();
        let stored = reopened.get_request(request.request_id).unwrap().unwrap();
        assert_eq!(stored.status, ProjectionRequestStatus::Deferred);
        assert_eq!(stored.available_at, retry_at);
        assert!(reopened
            .claim_next(now, now + ChronoDuration::minutes(5))
            .unwrap()
            .is_none());
        assert_eq!(
            reopened.list_outcomes(request.request_id).unwrap()[0].kind,
            ProjectionOutcomeKind::Deferred
        );
    }

    #[test]
    fn no_candidate_completes_without_candidate_row() {
        let (_dir, repo) = repository();
        let now = Utc::now();
        let request = ProjectionWorkRequest::pending("acceptance-3", "linear", "uri:3", now);
        repo.enqueue(&request).unwrap();
        let claimed = repo
            .claim_next(now, now + ChronoDuration::minutes(5))
            .unwrap()
            .unwrap();
        repo.record_outcome(
            &ProjectionWorkOutcome {
                request_id: claimed.request_id,
                attempt: claimed.attempt,
                kind: ProjectionOutcomeKind::NoCandidate,
                reason_code: Some("no_decision_signal".to_string()),
                retryable: false,
                details: serde_json::json!({}),
                completed_at: now,
            },
            None,
        )
        .unwrap();
        assert_eq!(
            repo.get_request(request.request_id)
                .unwrap()
                .unwrap()
                .status,
            ProjectionRequestStatus::Completed
        );
        assert!(repo
            .get_candidate_by_fingerprint("missing")
            .unwrap()
            .is_none());
    }

    #[test]
    fn semantic_fingerprint_converges_across_restart() {
        let (dir, repo) = repository();
        let now = Utc::now();
        let first_request = ProjectionWorkRequest::pending("acceptance-4", "github", "uri:4", now);
        repo.enqueue(&first_request).unwrap();
        let first = WorkingCandidate::active(
            first_request.request_id,
            "acceptance-4",
            "sha256:fingerprint",
            "First title",
            "First body",
            "architecture",
            now,
        );
        let converged_first = repo.converge_candidate(&first).unwrap();
        drop(repo);

        let reopened =
            SqliteProjectionWorkRepository::open(dir.path().join("working.sqlite3")).unwrap();
        let second_request = ProjectionWorkRequest::pending(
            "acceptance-5",
            "github",
            "uri:5",
            now + ChronoDuration::seconds(1),
        );
        reopened.enqueue(&second_request).unwrap();
        let second = WorkingCandidate::active(
            second_request.request_id,
            "acceptance-5",
            "sha256:fingerprint",
            "Updated title",
            "Updated body",
            "architecture",
            now + ChronoDuration::seconds(1),
        );
        let converged_second = reopened.converge_candidate(&second).unwrap();

        assert_eq!(converged_second.candidate_id, converged_first.candidate_id);
        assert_eq!(converged_second.title, "Updated title");
        assert_eq!(converged_second.request_id, second_request.request_id);
        assert_eq!(
            converged_second.first_seen_at,
            converged_first.first_seen_at
        );
    }
}
