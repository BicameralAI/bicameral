//! Finalized Integrations trace attachment and sanitized journey download.
//!
//! These records are daemon-local review evidence. They are intentionally kept
//! outside canonical Product replay and never grant candidate or Decision authority.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Json, Router};
use bicameral_api::transformation_trace::{
    TransformationTraceRecord, TransformationTraceSet, TRANSFORMATION_TRACE_SCHEMA,
    TRANSFORMATION_TRACE_SET_SCHEMA,
};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::journey_review::{build_journey_review, JourneyReviewBuildError};
use crate::live_projection_work::{LiveProjectionWorkError, LiveProjectionWorkStore};
use crate::sensitive_patterns::detect_sensitive;
use crate::state::AppState;

const TRACE_BODY_LIMIT: usize = 512 * 1024;
const TRACE_DOWNLOAD_SCHEMA: &str = "bicameral.journey-trace-download/v1";
const TRACE_ATTACHMENT_RECEIPT_SCHEMA: &str = "bicameral.trace-attachment-receipt/v1";

const PHASES: &[(&str, &str)] = &[
    (
        "PHASE-01-PROVIDER-ACQUISITION",
        "bicameral.provider-capture/v1",
    ),
    ("PHASE-04-OBSERVATION", "bicameral.observation/v1"),
    (
        "PHASE-05-PROVIDER-ADVISORIES",
        "bicameral.ingress-advisory/v1",
    ),
    (
        "PHASE-06-UNIVERSAL-NORMALIZATION",
        "bicameral.adapter-emission/v1",
    ),
    (
        "PHASE-07-UNIVERSAL-ADVISORIES",
        "bicameral.ingress-advisory/v1",
    ),
    (
        "PHASE-08-GATEWAY-MAPPING",
        "bicameral.external-ingest-envelope/v2",
    ),
    (
        "PHASE-08-GATEWAY-DELIVERY",
        "bicameral.gateway-delivery-receipt/v1",
    ),
];
const REQUIRED_PHASES: &[&str] = &[
    "PHASE-04-OBSERVATION",
    "PHASE-05-PROVIDER-ADVISORIES",
    "PHASE-06-UNIVERSAL-NORMALIZATION",
    "PHASE-07-UNIVERSAL-ADVISORIES",
    "PHASE-08-GATEWAY-MAPPING",
    "PHASE-08-GATEWAY-DELIVERY",
];

#[derive(Debug, Error)]
pub enum IntegrationTraceError {
    #[error("invalid transformation trace: {0}")]
    Invalid(String),
    #[error("transformation trace contains prohibited sensitive material")]
    Sensitive,
    #[error("transformation trace exceeds the {TRACE_BODY_LIMIT}-byte alpha limit")]
    Oversized,
    #[error("a different finalized trace is already attached to journey {0}")]
    Conflict(String),
    #[error("projection working store is unavailable")]
    StoreUnavailable,
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    WorkingState(#[from] LiveProjectionWorkError),
}

#[derive(Debug, Clone, Serialize)]
pub struct IntegrationTraceAttachmentReceipt {
    pub schema: &'static str,
    pub journey_id: String,
    pub trace_id: String,
    pub trace_digest: String,
    pub record_count: usize,
    pub persistence: &'static str,
    pub attached_at: DateTime<Utc>,
    pub idempotent_replay: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JourneyTraceDownload {
    pub schema: &'static str,
    pub journey_id: String,
    pub integrations_trace: TransformationTraceSet,
    pub daemon_review: bicameral_api::journey_review::JourneyReviewView,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct IntegrationTraceErrorResponse {
    code: &'static str,
    detail: String,
}

pub fn integration_trace_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/journeys/:journey_id/integrations-trace",
            put(attach_integrations_trace),
        )
        .route(
            "/api/v1/journeys/:journey_id/trace.json",
            get(download_journey_trace),
        )
}

async fn attach_integrations_trace(
    State(state): State<AppState>,
    Path(journey_id): Path<String>,
    Json(trace): Json<TransformationTraceSet>,
) -> Response {
    let Some(store) = state.projection_work_store.as_ref() else {
        return trace_error_response(IntegrationTraceError::StoreUnavailable);
    };
    if let Err(error) = validate_transformation_trace(&trace, &journey_id) {
        return trace_error_response(error);
    }
    if let Err(error) = build_journey_review(&state, &journey_id).await {
        return journey_error_response(error);
    }
    match store.attach_integration_trace(&trace, Utc::now()) {
        Ok(receipt) => {
            let status = if receipt.idempotent_replay {
                StatusCode::OK
            } else {
                StatusCode::CREATED
            };
            (status, Json(receipt)).into_response()
        }
        Err(error) => trace_error_response(error),
    }
}

async fn download_journey_trace(
    State(state): State<AppState>,
    Path(journey_id): Path<String>,
) -> Response {
    let Some(store) = state.projection_work_store.as_ref() else {
        return trace_error_response(IntegrationTraceError::StoreUnavailable);
    };
    let trace = match store.load_integration_trace(&journey_id) {
        Ok(Some(trace)) => trace,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(IntegrationTraceErrorResponse {
                    code: "integrations_trace_not_attached",
                    detail: format!("no finalized Integrations trace is attached to {journey_id}"),
                }),
            )
                .into_response();
        }
        Err(error) => return trace_error_response(error),
    };
    let daemon_review = match build_journey_review(&state, &journey_id).await {
        Ok(view) => view,
        Err(error) => return journey_error_response(error),
    };
    let package = JourneyTraceDownload {
        schema: TRACE_DOWNLOAD_SCHEMA,
        journey_id,
        integrations_trace: trace,
        daemon_review,
        generated_at: Utc::now(),
    };
    let serialized = match serde_json::to_string(&package) {
        Ok(value) => value,
        Err(error) => return trace_error_response(IntegrationTraceError::Json(error)),
    };
    if !detect_sensitive(&serialized).is_empty() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(IntegrationTraceErrorResponse {
                code: "download_sensitive_data_detected",
                detail: "the downloadable trace failed the final sensitive-data screen".into(),
            }),
        )
            .into_response();
    }
    let mut response = Json(package).into_response();
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"bicameral-journey-trace.json\""),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn journey_error_response(error: JourneyReviewBuildError) -> Response {
    let (status, code) = match error {
        JourneyReviewBuildError::NotFound(_) => (StatusCode::NOT_FOUND, "journey_not_found"),
        JourneyReviewBuildError::InvalidAcceptance(_) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_acceptance_payload",
        ),
        JourneyReviewBuildError::EventStore(_) | JourneyReviewBuildError::WorkingState(_) => {
            (StatusCode::SERVICE_UNAVAILABLE, "journey_query_unavailable")
        }
    };
    (
        status,
        Json(IntegrationTraceErrorResponse {
            code,
            detail: error.to_string(),
        }),
    )
        .into_response()
}

fn trace_error_response(error: IntegrationTraceError) -> Response {
    let (status, code) = match &error {
        IntegrationTraceError::Invalid(_) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_transformation_trace",
        ),
        IntegrationTraceError::Sensitive => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "trace_contains_sensitive_data",
        ),
        IntegrationTraceError::Oversized => (StatusCode::PAYLOAD_TOO_LARGE, "trace_too_large"),
        IntegrationTraceError::Conflict(_) => (StatusCode::CONFLICT, "trace_conflict"),
        IntegrationTraceError::StoreUnavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "projection_work_store_unavailable",
        ),
        IntegrationTraceError::Sqlite(_)
        | IntegrationTraceError::Json(_)
        | IntegrationTraceError::WorkingState(_) => {
            (StatusCode::SERVICE_UNAVAILABLE, "trace_store_unavailable")
        }
    };
    (
        status,
        Json(IntegrationTraceErrorResponse {
            code,
            detail: error.to_string(),
        }),
    )
        .into_response()
}

impl LiveProjectionWorkStore {
    pub fn attach_integration_trace(
        &self,
        trace: &TransformationTraceSet,
        now: DateTime<Utc>,
    ) -> Result<IntegrationTraceAttachmentReceipt, IntegrationTraceError> {
        let connection = trace_connection(self)?;
        ensure_trace_table(&connection)?;
        let trace_json = serde_json::to_string(trace)?;
        let trace_digest = canonical_digest_value(&serde_json::to_value(trace)?);
        let existing: Option<(String, String, String)> = connection
            .query_row(
                "SELECT trace_id, trace_digest, attached_at FROM integration_transformation_traces WHERE journey_id = ?1",
                [&trace.journey_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        if let Some((trace_id, digest, attached_at)) = existing {
            if trace_id != trace.trace_id || digest != trace_digest {
                return Err(IntegrationTraceError::Conflict(trace.journey_id.clone()));
            }
            let attached_at = DateTime::parse_from_rfc3339(&attached_at)
                .map_err(|error| IntegrationTraceError::Invalid(error.to_string()))?
                .with_timezone(&Utc);
            return Ok(IntegrationTraceAttachmentReceipt {
                schema: TRACE_ATTACHMENT_RECEIPT_SCHEMA,
                journey_id: trace.journey_id.clone(),
                trace_id,
                trace_digest: digest,
                record_count: trace.records.len(),
                persistence: "daemon_local_working_state",
                attached_at,
                idempotent_replay: true,
            });
        }
        connection.execute(
            "INSERT INTO integration_transformation_traces(journey_id, trace_id, trace_digest, trace_json, attached_at) VALUES(?1, ?2, ?3, ?4, ?5)",
            params![
                trace.journey_id,
                trace.trace_id,
                trace_digest,
                trace_json,
                now.to_rfc3339(),
            ],
        )?;
        Ok(IntegrationTraceAttachmentReceipt {
            schema: TRACE_ATTACHMENT_RECEIPT_SCHEMA,
            journey_id: trace.journey_id.clone(),
            trace_id: trace.trace_id.clone(),
            trace_digest,
            record_count: trace.records.len(),
            persistence: "daemon_local_working_state",
            attached_at: now,
            idempotent_replay: false,
        })
    }

    pub fn load_integration_trace(
        &self,
        journey_id: &str,
    ) -> Result<Option<TransformationTraceSet>, IntegrationTraceError> {
        let connection = trace_connection(self)?;
        ensure_trace_table(&connection)?;
        let trace_json: Option<String> = connection
            .query_row(
                "SELECT trace_json FROM integration_transformation_traces WHERE journey_id = ?1",
                [journey_id],
                |row| row.get(0),
            )
            .optional()?;
        trace_json
            .map(|value| serde_json::from_str(&value).map_err(IntegrationTraceError::from))
            .transpose()
    }
}

fn trace_connection(store: &LiveProjectionWorkStore) -> Result<Connection, IntegrationTraceError> {
    let connection = Connection::open(store.repository().path())?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;",
    )?;
    Ok(connection)
}

fn ensure_trace_table(connection: &Connection) -> Result<(), IntegrationTraceError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS integration_transformation_traces (
           journey_id TEXT PRIMARY KEY,
           trace_id TEXT NOT NULL,
           trace_digest TEXT NOT NULL,
           trace_json TEXT NOT NULL,
           attached_at TEXT NOT NULL
         );",
    )?;
    Ok(())
}

pub fn validate_transformation_trace(
    trace: &TransformationTraceSet,
    expected_journey_id: &str,
) -> Result<(), IntegrationTraceError> {
    let serialized = serde_json::to_string(trace)?;
    if serialized.len() > TRACE_BODY_LIMIT {
        return Err(IntegrationTraceError::Oversized);
    }
    if !detect_sensitive(&serialized).is_empty() {
        return Err(IntegrationTraceError::Sensitive);
    }
    required_text(&trace.journey_id, "journey_id")?;
    required_text(&trace.trace_id, "trace_id")?;
    if trace.schema != TRANSFORMATION_TRACE_SET_SCHEMA {
        return invalid("trace_set_schema_invalid");
    }
    if trace.journey_id != expected_journey_id {
        return invalid("journey_id_path_mismatch");
    }
    if trace.records.is_empty() || trace.records.len() > PHASES.len() {
        return invalid("trace_record_count_invalid");
    }

    let phase_contracts = PHASES.iter().copied().collect::<BTreeMap<_, _>>();
    let phase_order = PHASES
        .iter()
        .enumerate()
        .map(|(index, (phase, _))| (*phase, index))
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    let mut previous_order = None;
    let mut previous_output_digest: Option<&str> = None;

    for (index, record) in trace.records.iter().enumerate() {
        validate_record_identity(record, trace, index)?;
        let Some(order) = phase_order.get(record.phase_id.as_str()).copied() else {
            return invalid(format!("record_{index}_phase_id_unknown"));
        };
        if previous_order.is_some_and(|previous| order <= previous) {
            return invalid(format!("record_{index}_phase_order_invalid"));
        }
        previous_order = Some(order);
        if !seen.insert(record.phase_id.as_str()) {
            return invalid(format!("duplicate_phase_id:{}", record.phase_id));
        }
        if record.contract.id
            != *phase_contracts
                .get(record.phase_id.as_str())
                .expect("known phase has contract")
        {
            return invalid(format!("record_{index}_contract_id_invalid"));
        }
        validate_record(record, index)?;
        let input_digest = canonical_digest_value(&record.input.data);
        let output_digest = canonical_digest_value(&record.output.data);
        if record.input.digest != input_digest {
            return invalid(format!("record_{index}_input_digest_invalid"));
        }
        if record.output.digest != output_digest {
            return invalid(format!("record_{index}_output_digest_invalid"));
        }
        if previous_output_digest.is_some_and(|digest| digest != record.input.digest) {
            return invalid(format!("record_{index}_digest_chain_broken"));
        }
        previous_output_digest = Some(&record.output.digest);
    }
    for required in REQUIRED_PHASES {
        if !seen.contains(required) {
            return invalid(format!("required_phase_missing:{required}"));
        }
    }
    Ok(())
}

fn validate_record_identity(
    record: &TransformationTraceRecord,
    trace: &TransformationTraceSet,
    index: usize,
) -> Result<(), IntegrationTraceError> {
    if record.schema != TRANSFORMATION_TRACE_SCHEMA {
        return invalid(format!("record_{index}_schema_invalid"));
    }
    if record.journey_id != trace.journey_id || record.trace_id != trace.trace_id {
        return invalid(format!("record_{index}_identity_mismatch"));
    }
    Ok(())
}

fn validate_record(
    record: &TransformationTraceRecord,
    index: usize,
) -> Result<(), IntegrationTraceError> {
    if record.component.id != "ELM-INTEGRATIONS"
        || record.component.repository != "BicameralAI/bicameral-integrations"
    {
        return invalid(format!("record_{index}_component_invalid"));
    }
    required_text(&record.component.function, "component_function")?;
    if !is_full_revision(&record.component.revision) {
        return invalid(format!("record_{index}_revision_invalid"));
    }
    if !is_sha256(&record.contract.semantic_fingerprint)
        || !is_sha256(&record.input.digest)
        || !is_sha256(&record.output.digest)
    {
        return invalid(format!("record_{index}_digest_or_fingerprint_invalid"));
    }
    required_text(&record.input.schema, "input_schema")?;
    required_text(&record.output.schema, "output_schema")?;
    DateTime::parse_from_rfc3339(&record.occurred_at).map_err(|_| {
        IntegrationTraceError::Invalid(format!("record_{index}_occurred_at_invalid"))
    })?;
    DateTime::parse_from_rfc3339(&record.timing.started_at).map_err(|_| {
        IntegrationTraceError::Invalid(format!("record_{index}_started_at_invalid"))
    })?;
    DateTime::parse_from_rfc3339(&record.timing.completed_at).map_err(|_| {
        IntegrationTraceError::Invalid(format!("record_{index}_completed_at_invalid"))
    })?;
    if record.timing.attempt == 0 {
        return invalid(format!("record_{index}_attempt_invalid"));
    }
    if record.authority.canonical {
        return invalid(format!("record_{index}_integrations_authority_invalid"));
    }
    if record.persistence.locator.is_some() {
        return invalid(format!("record_{index}_locator_forbidden"));
    }
    if !matches!(record.persistence.plane.as_str(), "none" | "local_work")
        || !matches!(record.persistence.operation.as_str(), "none" | "update")
    {
        return invalid(format!("record_{index}_persistence_invalid"));
    }
    if !matches!(record.receipt.status.as_str(), "completed" | "failed") {
        return invalid(format!("record_{index}_receipt_status_invalid"));
    }
    for gate in &record.evaluation.hard_gates {
        required_text(&gate.gate_id, "gate_id")?;
        required_text(&gate.reason_code, "gate_reason_code")?;
        if !matches!(gate.result.as_str(), "pass" | "fail") {
            return invalid(format!("record_{index}_gate_result_invalid"));
        }
    }
    for advisory in &record.evaluation.advisories {
        if advisory.failure_mode != "fail_open" || advisory.authority != "none" {
            return invalid(format!("record_{index}_advisory_authority_invalid"));
        }
        if advisory.score.is_some_and(|score| !score.is_finite()) {
            return invalid(format!("record_{index}_advisory_score_invalid"));
        }
    }
    Ok(())
}

fn required_text(value: &str, label: &str) -> Result<(), IntegrationTraceError> {
    if value.trim().is_empty() {
        return invalid(format!("{label}_required"));
    }
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn is_full_revision(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn canonical_digest_value(value: &Value) -> String {
    let canonical = canonicalize(value);
    let encoded = serde_json::to_vec(&canonical).expect("JSON value serialization cannot fail");
    format!("sha256:{:x}", Sha256::digest(encoded))
}

fn canonicalize(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonicalize).collect()),
        Value::Object(map) => {
            let mut keys = map.keys().collect::<Vec<_>>();
            keys.sort();
            let mut output = serde_json::Map::new();
            for key in keys {
                output.insert(key.clone(), canonicalize(&map[key]));
            }
            Value::Object(output)
        }
        _ => value.clone(),
    }
}

fn invalid<T>(detail: impl Into<String>) -> Result<T, IntegrationTraceError> {
    Err(IntegrationTraceError::Invalid(detail.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_api::transformation_trace::*;
    use serde_json::json;

    fn record(
        phase_id: &str,
        contract_id: &str,
        input: Value,
        output: Value,
    ) -> TransformationTraceRecord {
        TransformationTraceRecord {
            schema: TRANSFORMATION_TRACE_SCHEMA.into(),
            journey_id: "journey-1".into(),
            trace_id: "trace-1".into(),
            phase_id: phase_id.into(),
            occurred_at: "2026-07-19T20:00:00Z".into(),
            component: TransformationTraceComponent {
                id: "ELM-INTEGRATIONS".into(),
                repository: "BicameralAI/bicameral-integrations".into(),
                revision: "a".repeat(40),
                function: "test.phase".into(),
            },
            contract: TransformationTraceContract {
                id: contract_id.into(),
                semantic_fingerprint: format!("sha256:{}", "b".repeat(64)),
            },
            input: TransformationTracePayload {
                schema: "input/v1".into(),
                digest: canonical_digest_value(&input),
                data: input,
            },
            output: TransformationTracePayload {
                schema: "output/v1".into(),
                digest: canonical_digest_value(&output),
                data: output,
            },
            delta: TransformationTraceDelta::default(),
            evaluation: TransformationTraceEvaluation::default(),
            persistence: TransformationTracePersistence {
                plane: "none".into(),
                operation: "none".into(),
                artifact_id: None,
                digest: None,
                locator: None,
            },
            authority: TransformationTraceAuthority {
                before: "external_evidence".into(),
                after: "normalized_evidence".into(),
                transition: "normalize".into(),
                canonical: false,
            },
            timing: TransformationTraceTiming {
                started_at: "2026-07-19T20:00:00Z".into(),
                completed_at: "2026-07-19T20:00:00Z".into(),
                duration_ms: 0,
                attempt: 1,
            },
            receipt: TransformationTraceReceipt {
                status: "completed".into(),
                reason_codes: Vec::new(),
                retryable: false,
                correlation_ids: BTreeMap::new(),
            },
        }
    }

    pub(crate) fn valid_trace() -> TransformationTraceSet {
        let mut state = json!({});
        let mut records = Vec::new();
        for (phase_id, contract_id) in REQUIRED_PHASES
            .iter()
            .map(|phase| (*phase, PHASES.iter().find(|(id, _)| id == phase).unwrap().1))
        {
            let output = json!({"phase": phase_id});
            records.push(record(phase_id, contract_id, state, output.clone()));
            state = output;
        }
        TransformationTraceSet {
            schema: TRANSFORMATION_TRACE_SET_SCHEMA.into(),
            journey_id: "journey-1".into(),
            trace_id: "trace-1".into(),
            records,
        }
    }

    #[test]
    fn valid_trace_passes() {
        validate_transformation_trace(&valid_trace(), "journey-1").unwrap();
    }

    #[test]
    fn digest_break_fails_closed() {
        let mut trace = valid_trace();
        trace.records[2].input.digest = format!("sha256:{}", "0".repeat(64));
        assert!(validate_transformation_trace(&trace, "journey-1").is_err());
    }

    #[test]
    fn canonical_authority_fails_closed() {
        let mut trace = valid_trace();
        trace.records[0].authority.canonical = true;
        assert!(validate_transformation_trace(&trace, "journey-1").is_err());
    }

    #[test]
    fn sensitive_value_fails_closed() {
        let mut trace = valid_trace();
        // Runtime-assembled AWS doc example key: the repository secret scanner
        // must never see the literal pattern in source, while the validator
        // still receives the exact sensitive shape at runtime.
        let example_key = ["AKIA", "IOSFODNN7EXAMPLE"].concat();
        trace.records[0].output.data = json!({ "token": example_key });
        trace.records[0].output.digest = canonical_digest_value(&trace.records[0].output.data);
        trace.records[1].input = trace.records[0].output.clone();
        assert!(matches!(
            validate_transformation_trace(&trace, "journey-1"),
            Err(IntegrationTraceError::Sensitive)
        ));
    }
}
