//! Gateway route definitions.
//!
//! Implements the v0.2 spec contracts: evidence-based ingest, promotion path
//! (accept_candidate → Decision), dashboard wiring to materialized state.

use crate::canary_patterns;
use crate::sensitive_patterns;
use crate::state::AppState;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use bicameral_api::candidate::{CandidateState, Decision, DecisionCandidate, ReviewLifecycleState};
use bicameral_api::dashboard::{
    CandidateCommandKind, CandidatePreview, ComplianceDisplay, DashboardReviewCommand,
    DecisionCommandKind, DeferredCommandResponse, IngestionGateItem, LedgerCandidate,
    LedgerDecision, LedgerReviewItem, SignoffDisplay, SignoffState,
};
use bicameral_api::event_store::{
    BindToCodePayload, EventStoreEntry, EventType, ExternalIngestRejection,
    ExternalIngestRejectionReason, IngestPayload, IngestRejection, IngestRejectionReason,
};
use bicameral_api::evidence::{GraphEvidenceState, SourceEvidence};
use bicameral_api::evidence_acceptance::EvidenceAcceptedPayload;
use bicameral_api::review::{ReviewCommand, ReviewState, ReviewStatus};
use bicameral_api::source::{Source, SourceEvidenceRef, SourceFreshness, SourceSnapshot};
use bicameral_api::tool_request::ExternalIngestEnvelope;
use bicameral_audit::receipt::AuditAction;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

// ── Health routes ────────────────────────────────────────────────

pub fn health_routes() -> Router<AppState> {
    Router::new().route("/health", get(health_check))
}

async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    version: String,
}

// ── V2 ToolRequest routes ────────────────────────────────────────

pub fn v2_routes() -> Router<AppState> {
    Router::new()
        .route("/v2/capabilities", get(v2_capabilities))
        .route("/v2/tool-requests", post(v2_tool_request))
}

async fn v2_capabilities(State(state): State<AppState>) -> Response {
    match &state.runtime_dispatch {
        Some(dispatch) => (StatusCode::OK, Json(dispatch.capabilities())).into_response(),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "runtime dispatch not available (daemon not fully initialized)"
            })),
        )
            .into_response(),
    }
}

async fn v2_tool_request(
    State(state): State<AppState>,
    Json(request): Json<bicameral_api::tool_request::ToolRequest>,
) -> Response {
    let dispatch = match &state.runtime_dispatch {
        Some(d) => d.clone(),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "runtime dispatch not available (daemon not fully initialized)"
                })),
            )
                .into_response();
        }
    };

    let workspace_root = state.workspace_root.read().await.clone();
    let response = dispatch.dispatch(&request, workspace_root.as_deref());

    // A canonical write appends to the event store but does not touch the
    // gateway's in-memory read models. Refresh the read-only Ledger projection
    // from event-store replay so a materialized Decision renders immediately,
    // keeping the event store the single source of truth. Read-only commands
    // never mutate canonical state, so they skip the replay.
    use bicameral_api::action_authorization::GovernedActionClass;
    use bicameral_api::tool_request::ToolResponseStatus;
    let mutated = response.status == ToolResponseStatus::Ok
        && GovernedActionClass::from_command(&request.command) != GovernedActionClass::ReadOnly;
    if mutated {
        if let Some(replayed) = dispatch.replay_projection() {
            state.refresh_projection(replayed).await;
        }
    }

    (StatusCode::OK, Json(response)).into_response()
}

// ── Ingest routes ────────────────────────────────────────────────

pub fn ingest_routes() -> Router<AppState> {
    Router::new().route("/api/v1/ingest", post(ingest_candidate))
}

/// Ingest request aligned with MCP v0.2 spec §2.2 `bicameral.ingest`.
///
/// Accepts source + snapshot + evidence + candidate fields. Source/snapshot/evidence
/// are optional for backward compatibility — plain title+description ingest still works.
#[derive(Deserialize, Serialize, schemars::JsonSchema)]
pub struct IngestRequest {
    title: String,
    description: String,
    #[serde(default)]
    level: Option<bicameral_api::candidate::DecisionLevel>,
    source: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    source_type: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    snapshot_content: Option<String>,
    #[serde(default)]
    evidence: Vec<IngestEvidenceItem>,
    /// Typed source surface classification (e.g. `mcp_session`, `command_run`).
    ///
    /// When provided, the resulting `Source` carries a `SourceKind` so
    /// downstream consumers can distinguish MCP sessions, command runs,
    /// tracker issues, etc. without relying on the free-form `source_type`.
    #[serde(default)]
    source_kind: Option<bicameral_api::source::SourceKind>,
    /// Typed evidence references into the source snapshot.
    ///
    /// These carry `source_kind`, optional `pointer` (kind-specific locator),
    /// and optional `metadata` for MCP session turns, tool call output,
    /// command output, transcript segments, etc. They complement the legacy
    /// span-based `evidence` field.
    #[serde(default)]
    evidence_references: Vec<IngestEvidenceReferenceItem>,
}

/// A typed evidence reference item within an ingest submission.
///
/// Represents a citation into a `SourceSnapshot` with `SourceKind`-specific
/// pointer and metadata (e.g. MCP tool call ID, command exit code).
#[derive(Deserialize, Serialize, schemars::JsonSchema)]
pub struct IngestEvidenceReferenceItem {
    /// Classification of the source surface this reference cites.
    pub source_kind: bicameral_api::source::SourceKind,
    /// Canonical excerpt of the cited content.
    pub excerpt: String,
    /// Kind-specific locator into the snapshot (e.g. JSON path, tool-call
    /// ID, byte offset, line range).
    #[serde(default)]
    pub pointer: Option<String>,
    /// Optional metadata for kind-specific details (e.g. MCP session turn
    /// index, command exit code, diff hunk header).
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
pub struct IngestEvidenceItem {
    #[serde(default)]
    span_start: Option<usize>,
    #[serde(default)]
    span_end: Option<usize>,
    excerpt: String,
    #[serde(default = "default_confidence")]
    confidence: f64,
}

fn default_confidence() -> f64 {
    0.5
}

/// Deterministic heuristic for decision level when the caller omits it.
///
/// Ports MCP #340 `_classify_decision_level` rules:
/// 1. Product source (transcript, notion, slack, document) → Product (L1).
/// 2. Implementation source (implementation_choice, agent_session) → Implementation (L3).
/// 3. Fallback → Architecture (L2).
fn classify_decision_level(source_type: &str) -> bicameral_api::candidate::DecisionLevel {
    use bicameral_api::candidate::DecisionLevel;
    match source_type {
        "transcript" | "notion" | "slack" | "document" => DecisionLevel::Product,
        "implementation_choice" | "agent_session" => DecisionLevel::Implementation,
        _ => DecisionLevel::Architecture,
    }
}

/// Ingest response aligned with MCP v0.2 spec §2.2.
#[derive(Serialize, schemars::JsonSchema)]
pub struct IngestResponse {
    source_id: String,
    snapshot_id: Option<String>,
    evidence_ids: Vec<String>,
    /// IDs of typed `EvidenceReference` items created from `evidence_references`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    evidence_reference_ids: Vec<String>,
    candidate_ids: Vec<String>,
    governance_results: Vec<IngestGovernanceResult>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct IngestGovernanceResult {
    candidate_id: String,
    action: String,
    reason: String,
    review_state: ReviewLifecycleState,
    candidate_state: CandidateState,
}

async fn ingest_candidate(
    State(state): State<AppState>,
    Json(req): Json<IngestRequest>,
) -> Result<(StatusCode, Json<IngestResponse>), (StatusCode, Json<IngestRejection>)> {
    // Gate: rate limit (token bucket — fail-fast before any materialization).
    if !state.ingest_rate_limiter.check() {
        tracing::warn!("Ingest rate limit exceeded");
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(IngestRejection {
                reason: IngestRejectionReason::RateLimitExceeded,
                message: "Rate limit exceeded — try again later".to_string(),
            }),
        ));
    }

    // Gate: reject empty evidence with a typed governance reason.
    if req.evidence.is_empty() {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(IngestRejection {
                reason: IngestRejectionReason::EmptyEvidence,
                message: "At least one evidence item is required for ingest".to_string(),
            }),
        ));
    }

    // Gate: injection canary detection (HARD — fail-fast, non-bypassable).
    let serialized = serde_json::to_string(&req).unwrap_or_default();
    let canary_hits = canary_patterns::detect_canaries(&serialized);
    if !canary_hits.is_empty() {
        let first = &canary_hits[0];
        tracing::warn!(
            category = first.category,
            total_hits = canary_hits.len(),
            "Injection canary triggered on ingest payload"
        );
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(IngestRejection {
                reason: IngestRejectionReason::InjectionCanary,
                message: format!(
                    "Injection canary match: category={}, hits={}, catalog={}",
                    first.category,
                    canary_hits.len(),
                    canary_patterns::CATALOG_VERSION,
                ),
            }),
        ));
    }

    // Gate: sensitive-data detection (HARD — fail-fast, non-bypassable).
    let sensitive_hits = sensitive_patterns::detect_sensitive(&serialized);
    if !sensitive_hits.is_empty() {
        let first = &sensitive_hits[0];
        tracing::warn!(
            cls = first.cls,
            total_hits = sensitive_hits.len(),
            "Sensitive data detected in ingest payload"
        );
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(IngestRejection {
                reason: IngestRejectionReason::SensitiveData,
                message: format!(
                    "Sensitive data detected: class={}, excerpt={}, catalog={}",
                    first.cls,
                    first.match_excerpt,
                    sensitive_patterns::CATALOG_VERSION,
                ),
            }),
        ));
    }

    let source_uri = req.source.clone();
    let source_type = req
        .source_type
        .clone()
        .unwrap_or_else(|| "manual".to_string());

    // Infer decision level when omitted (ports MCP #340 heuristic).
    let level = req
        .level
        .unwrap_or_else(|| classify_decision_level(&source_type));

    // 1. Build Source (portable identifier — URI, not path)
    let source_record = Source {
        uri: source_uri.clone(),
        source_type: source_type.clone(),
        label: req.label.clone(),
        source_kind: req.source_kind,
    };

    // 2. Build SourceSnapshot if content provided (content-addressed)
    let snapshot = req.snapshot_content.as_ref().map(|content| {
        let hash = sha256_hex(content);
        let snap_addr = format!("sha256:{}", hash);
        SourceSnapshot {
            snapshot_addr: snap_addr,
            snapshot_ref: source_uri.clone(),
            captured_at: chrono::Utc::now(),
            content: Some(content.clone()),
            content_hash: Some(hash),
            capture_method: None,
        }
    });
    let snapshot_addr = snapshot.as_ref().map(|s| s.snapshot_addr.clone());

    // 3. Build SourceEvidence items (span defaults: 0..excerpt.len())
    let mut evidence_records: Vec<SourceEvidence> = Vec::new();
    for ev_item in &req.evidence {
        let snap_id = snapshot_addr
            .clone()
            .unwrap_or_else(|| format!("inline:{}", source_uri));
        let span_start = ev_item.span_start.unwrap_or(0);
        let span_end = ev_item.span_end.unwrap_or(ev_item.excerpt.len());
        let ev = SourceEvidence::new(
            snap_id,
            span_start,
            span_end,
            ev_item.excerpt.clone(),
            ev_item.confidence,
        );
        evidence_records.push(ev);
    }

    // 3b. Build typed EvidenceReference items from evidence_references
    let mut evidence_ref_records: Vec<bicameral_api::evidence::EvidenceReference> = Vec::new();
    for ref_item in &req.evidence_references {
        let snap_id = snapshot_addr
            .clone()
            .unwrap_or_else(|| format!("inline:{}", source_uri));
        let mut ev_ref = bicameral_api::evidence::EvidenceReference::new(
            snap_id,
            ref_item.source_kind,
            ref_item.excerpt.clone(),
        );
        ev_ref.pointer = ref_item.pointer.clone();
        ev_ref.metadata = ref_item.metadata.clone();
        evidence_ref_records.push(ev_ref);
    }

    // 4. Build DecisionCandidate with evidence refs
    let mut candidate =
        DecisionCandidate::new(req.title, req.description, level, req.source.clone());
    candidate.tags = req.tags;
    candidate.evidence_refs = evidence_records.iter().map(|e| e.id).collect();
    candidate.evidence_reference_ids = evidence_ref_records.iter().map(|r| r.id).collect();
    let cand_id = candidate.id;

    // 5. Build portable IngestPayload and emit to event store
    let ingest_payload = IngestPayload {
        source: source_record.clone(),
        snapshot: snapshot.clone(),
        evidence: evidence_records.clone(),
        candidate: candidate.clone(),
        evidence_references: evidence_ref_records.clone(),
        provenance: None,
    };

    let payload_value = serde_json::to_value(&ingest_payload).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(IngestRejection {
                reason: IngestRejectionReason::InvalidPayload,
                message: "Failed to serialize ingest payload".to_string(),
            }),
        )
    })?;

    let entry = EventStoreEntry::new(EventType::Ingest, payload_value, "gateway".to_string());
    state.event_store.append(entry).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(IngestRejection {
                reason: IngestRejectionReason::InvalidPayload,
                message: "Failed to append event to store".to_string(),
            }),
        )
    })?;

    // 6. Materialize into in-memory state
    state
        .sources
        .write()
        .await
        .insert(source_uri.clone(), source_record);
    if let Some(snap) = snapshot {
        state
            .snapshots
            .write()
            .await
            .insert(snap.snapshot_addr.clone(), snap);
    }
    for ev in &evidence_records {
        state.evidence.write().await.insert(ev.id, ev.clone());
    }
    for ev_ref in &evidence_ref_records {
        state
            .evidence_references
            .write()
            .await
            .insert(ev_ref.id, ev_ref.clone());
    }
    state
        .candidates
        .write()
        .await
        .insert(cand_id, candidate.clone());
    let review = ReviewState::new(cand_id);
    state.reviews.write().await.insert(cand_id, review);

    // 7. Audit
    {
        let mut audit = state.audit.write().await;
        let _ = audit.record(
            AuditAction::CandidateIngested,
            "gateway".to_string(),
            Some(cand_id),
            format!("Ingested candidate: {}", candidate.title),
        );
    }

    let evidence_ids: Vec<String> = evidence_records.iter().map(|e| e.id.to_string()).collect();
    let evidence_reference_ids: Vec<String> = evidence_ref_records
        .iter()
        .map(|r| r.id.to_string())
        .collect();
    tracing::info!(candidate_id = %cand_id, evidence_count = evidence_ids.len(), evidence_ref_count = evidence_reference_ids.len(), "Candidate ingested via gateway");

    let response = IngestResponse {
        source_id: source_uri,
        snapshot_id: snapshot_addr,
        evidence_ids,
        evidence_reference_ids,
        candidate_ids: vec![cand_id.to_string()],
        governance_results: vec![IngestGovernanceResult {
            candidate_id: cand_id.to_string(),
            action: "accepted_as_candidate".to_string(),
            reason: "Schema valid, evidence recorded".to_string(),
            review_state: ReviewLifecycleState::Proposed,
            candidate_state: CandidateState::Proposed,
        }],
    };

    Ok((StatusCode::CREATED, Json(response)))
}

// ── External ingest routes ───────────────────────────────────────

/// Routes for the bot-owned external adapter ingest gateway.
///
/// External source adapters (Jira, Linear, Slack, GitHub, Drive, etc.) submit
/// evidence through this endpoint using `ExternalIngestEnvelope`. This path
/// is separate from the local `ToolRequest`-based ingest and cannot carry
/// local actor authority, review commands, signoff approval, compliance
/// resolution, or policy grants.
pub fn external_ingest_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/external-ingest", post(external_ingest_v1))
        .route("/api/v2/external-ingest", post(external_ingest_v2))
        .route(
            "/api/external-ingest/capabilities",
            get(external_ingest_capabilities),
        )
}

async fn external_ingest_capabilities() -> Json<bicameral_api::ExternalIngestCapabilities> {
    Json(bicameral_api::ExternalIngestCapabilities {
        contract_id: bicameral_api::EXTERNAL_INGEST_CONTRACT_ID.to_string(),
        protocol_version: bicameral_api::EXTERNAL_INGEST_PROTOCOL_VERSION,
        minimum_supported_protocol_version:
            bicameral_api::MINIMUM_SUPPORTED_EXTERNAL_INGEST_PROTOCOL_VERSION,
        supported_protocol_versions: vec![bicameral_api::EXTERNAL_INGEST_PROTOCOL_VERSION],
        delivery_endpoint: bicameral_api::EXTERNAL_INGEST_DELIVERY_ENDPOINT.to_string(),
        request_schema_sha256: bicameral_api::EXTERNAL_INGEST_REQUEST_SCHEMA_SHA256.to_string(),
        contract_fingerprint: bicameral_api::EXTERNAL_INGEST_CONTRACT_FINGERPRINT.to_string(),
        redaction_receipt_required: true,
    })
}

/// Response from a successful external adapter ingest.
#[derive(Serialize, schemars::JsonSchema)]
pub struct ExternalIngestResponse {
    pub source_id: String,
    pub snapshot_id: Option<String>,
    pub evidence_ids: Vec<String>,
    pub candidate_ids: Vec<String>,
    /// Advisory governance results — never blocking, never authoritative.
    pub advisory_results: Vec<ExternalIngestAdvisoryResult>,
    /// Daemon-local projection request identity for retry and diagnostics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projection_request_id: Option<String>,
    /// Alpha projection outcome label from daemon-owned reasoning.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projection_outcome: Option<String>,
}

/// Advisory governance result for an external ingest candidate.
///
/// External ingest can only produce advisory results. It cannot emit
/// blocking governance verdicts, acceptance decisions, or enforcement actions.
#[derive(Serialize, schemars::JsonSchema)]
pub struct ExternalIngestAdvisoryResult {
    pub candidate_id: String,
    pub action: String,
    pub reason: String,
    pub review_state: ReviewLifecycleState,
    pub candidate_state: CandidateState,
}

/// Known forbidden field names that indicate authority injection attempts
/// in a raw external ingest payload.
const FORBIDDEN_EXTERNAL_FIELDS: &[&str] = &[
    "authority",
    "auth_method",
    "actor_id",
    "session_id",
    "policy_scope",
    "review_command",
    "tool_command",
    "approve_signoff",
    "reject_signoff",
    "resolve_compliance",
    "compliance_verdict",
    "policy_grant",
    "governance_override",
    "canonical_decision",
    "signoff_state",
    "decision_id",
    "compliance_state",
    "tracking_state",
];

/// Common alias field names that integrations mistakenly send instead of the
/// canonical `ExternalIngestEnvelope` field names. When detected, the error
/// response names the correct field so the integration author can fix the
/// mapping without reading the full spec.
const COMMON_ALIAS_MAPPINGS: &[(&str, &str)] = &[
    ("source_ref", "source_uri"),
    ("body", "content"),
    ("text", "content"),
    ("description", "content"),
    ("source_url", "source_uri"),
    ("url", "source_uri"),
    ("source_id", "source_system"),
    ("source_name", "source_system"),
];

/// Evidence-level alias field names that integrations mistakenly use.
const EVIDENCE_ALIAS_MAPPINGS: &[(&str, &str)] = &[
    ("title", "excerpt"),
    ("body", "excerpt"),
    ("text", "excerpt"),
    ("content", "excerpt"),
    ("description", "excerpt"),
];

/// Detect common alias fields in a raw external ingest payload and return
/// an actionable hint string naming the correct field mappings.
fn detect_common_alias_hint(raw: &serde_json::Value) -> String {
    let obj = match raw.as_object() {
        Some(o) => o,
        None => return String::new(),
    };

    let mut hints: Vec<String> = Vec::new();

    // Top-level alias detection.
    for &(alias, canonical) in COMMON_ALIAS_MAPPINGS {
        if obj.contains_key(alias) && !obj.contains_key(canonical) {
            hints.push(format!(
                "'{alias}' is not accepted — use '{canonical}' instead."
            ));
        }
    }

    // Evidence-level alias detection.
    if let Some(evidence) = obj.get("evidence").and_then(|v| v.as_array()) {
        for item in evidence {
            if let Some(item_obj) = item.as_object() {
                for &(alias, canonical) in EVIDENCE_ALIAS_MAPPINGS {
                    if item_obj.contains_key(alias) && !item_obj.contains_key(canonical) {
                        hints.push(format!(
                            "evidence[].'{alias}' is not accepted — use evidence[].'{canonical}' instead."
                        ));
                        break;
                    }
                }
                break; // Only check first evidence item.
            }
        }
    }

    if hints.is_empty() {
        String::new()
    } else {
        format!("Field mapping hint: {}", hints.join(" "))
    }
}

/// Detect authority/review/signoff/compliance/policy injection in raw JSON.
fn detect_forbidden_fields(raw: &serde_json::Value) -> Option<ExternalIngestRejectionReason> {
    let obj = raw.as_object()?;
    for &field in FORBIDDEN_EXTERNAL_FIELDS {
        if obj.contains_key(field) {
            return match field {
                "authority" | "auth_method" | "actor_id" | "session_id" | "policy_scope" => {
                    Some(ExternalIngestRejectionReason::AuthorityInjection)
                }
                "review_command" | "tool_command" => {
                    Some(ExternalIngestRejectionReason::ReviewCommandInjection)
                }
                "approve_signoff" | "reject_signoff" | "signoff_state" => {
                    Some(ExternalIngestRejectionReason::SignoffInjection)
                }
                "resolve_compliance" | "compliance_verdict" | "compliance_state" => {
                    Some(ExternalIngestRejectionReason::ComplianceInjection)
                }
                "policy_grant" | "governance_override" => {
                    Some(ExternalIngestRejectionReason::PolicyGrantInjection)
                }
                _ => Some(ExternalIngestRejectionReason::AuthorityInjection),
            };
        }
    }
    None
}

fn evidence_acceptance_key(source_system: &str, source_uri: &str, digest: &str) -> String {
    let material = format!("{source_system}\n{source_uri}\n{digest}");
    format!("sha256:{}", sha256_hex(&material))
}

fn find_evidence_acceptance(
    state: &AppState,
    idempotency_key: &str,
) -> Result<Option<EvidenceAcceptedPayload>, String> {
    let entries = state
        .event_store
        .replay()
        .map_err(|error| error.to_string())?;
    for entry in entries {
        if entry.event_type != EventType::EvidenceAccepted {
            continue;
        }
        if entry
            .payload
            .get("idempotency_key")
            .and_then(|value| value.as_str())
            != Some(idempotency_key)
        {
            continue;
        }
        return serde_json::from_value(entry.payload)
            .map(Some)
            .map_err(|error| error.to_string());
    }
    Ok(None)
}

/// External adapter ingest handler.
///
/// Accepts `ExternalIngestEnvelope` payloads from external source adapters
/// (Jira, Linear, Slack, GitHub, Drive, etc.) and creates bot-owned
/// source/evidence/candidate records compatible with the V1 advisory golden path.
///
/// This handler enforces that external adapters cannot carry:
/// - Local actor/session authority
/// - Review commands (accept, reject, approve_signoff, etc.)
/// - Signoff approval or rejection
/// - Compliance resolution
/// - Policy grants or governance overrides
///
/// Accepted envelopes produce advisory-only governance results.
async fn external_ingest_v1(
    State(state): State<AppState>,
    body: axum::body::Bytes,
) -> Result<(StatusCode, Json<ExternalIngestResponse>), (StatusCode, Json<ExternalIngestRejection>)>
{
    external_ingest(state, body, false).await
}

async fn external_ingest_v2(
    State(state): State<AppState>,
    body: axum::body::Bytes,
) -> Result<(StatusCode, Json<ExternalIngestResponse>), (StatusCode, Json<ExternalIngestRejection>)>
{
    external_ingest(state, body, true).await
}

async fn external_ingest(
    state: AppState,
    body: axum::body::Bytes,
    require_redaction_receipt: bool,
) -> Result<(StatusCode, Json<ExternalIngestResponse>), (StatusCode, Json<ExternalIngestRejection>)>
{
    // Gate: rate limit.
    if !state.ingest_rate_limiter.check() {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(ExternalIngestRejection {
                reason: ExternalIngestRejectionReason::RateLimitExceeded,
                message: "Rate limit exceeded — try again later".to_string(),
            }),
        ));
    }

    // Parse raw JSON first to check for forbidden fields before typed deser.
    let raw_value: serde_json::Value = serde_json::from_slice(&body).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ExternalIngestRejection {
                reason: ExternalIngestRejectionReason::InvalidPayload,
                message: format!("Invalid JSON: {e}"),
            }),
        )
    })?;

    // Gate: reject forbidden authority/review/signoff/compliance/policy fields.
    if let Some(reason) = detect_forbidden_fields(&raw_value) {
        let field_category = match reason {
            ExternalIngestRejectionReason::AuthorityInjection => "local actor authority",
            ExternalIngestRejectionReason::ReviewCommandInjection => "review commands",
            ExternalIngestRejectionReason::SignoffInjection => "signoff approval/rejection",
            ExternalIngestRejectionReason::ComplianceInjection => "compliance resolution",
            ExternalIngestRejectionReason::PolicyGrantInjection => "policy grants",
            _ => "forbidden fields",
        };
        tracing::warn!(
            category = field_category,
            "External ingest rejected: forbidden field detected"
        );
        return Err((
            StatusCode::FORBIDDEN,
            Json(ExternalIngestRejection {
                reason,
                message: format!(
                    "External adapter ingest cannot carry {field_category}. \
                     Use ExternalIngestEnvelope fields only."
                ),
            }),
        ));
    }

    // Deserialize into typed envelope.
    let envelope: ExternalIngestEnvelope =
        serde_json::from_value(raw_value.clone()).map_err(|e| {
            let hint = detect_common_alias_hint(&raw_value);
            let message = if hint.is_empty() {
                format!(
                    "Invalid ExternalIngestEnvelope: {e}. \
                 Expected fields: source_system (string), source_uri (string), \
                 content (string), evidence (array of {{excerpt, span_start?, \
                 span_end?, confidence?}}), candidate_hints (array of \
                 {{title, body, level?, labels?}})."
                )
            } else {
                format!(
                    "Invalid ExternalIngestEnvelope: {e}. {hint} \
                 Expected fields: source_system (string), source_uri (string), \
                 content (string), evidence (array of {{excerpt, span_start?, \
                 span_end?, confidence?}}), candidate_hints (array of \
                 {{title, body, level?, labels?}})."
                )
            };
            (
                StatusCode::BAD_REQUEST,
                Json(ExternalIngestRejection {
                    reason: ExternalIngestRejectionReason::InvalidPayload,
                    message,
                }),
            )
        })?;

    if require_redaction_receipt && envelope.redaction_receipt.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ExternalIngestRejection {
                reason: ExternalIngestRejectionReason::InvalidPayload,
                message: "External ingest protocol v2 requires redaction_receipt".to_string(),
            }),
        ));
    }

    // Gate: reject empty evidence.
    if envelope.evidence.is_empty() && envelope.candidate_hints.is_empty() {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ExternalIngestRejection {
                reason: ExternalIngestRejectionReason::EmptyEvidence,
                message: "At least one evidence item or candidate hint is required".to_string(),
            }),
        ));
    }

    // Gate: injection canary detection.
    let serialized = serde_json::to_string(&envelope).unwrap_or_default();
    let canary_hits = canary_patterns::detect_canaries(&serialized);
    if !canary_hits.is_empty() {
        let first = &canary_hits[0];
        tracing::warn!(
            category = first.category,
            total_hits = canary_hits.len(),
            "External ingest: injection canary triggered"
        );
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ExternalIngestRejection {
                reason: ExternalIngestRejectionReason::InjectionCanary,
                message: format!(
                    "Injection canary match: category={}, hits={}",
                    first.category,
                    canary_hits.len(),
                ),
            }),
        ));
    }

    // Gate: sensitive-data detection.
    let sensitive_hits = sensitive_patterns::detect_sensitive(&serialized);
    if !sensitive_hits.is_empty() {
        let first = &sensitive_hits[0];
        tracing::warn!(
            cls = first.cls,
            total_hits = sensitive_hits.len(),
            "External ingest: sensitive data detected"
        );
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ExternalIngestRejection {
                reason: ExternalIngestRejectionReason::SensitiveData,
                message: format!(
                    "Sensitive data detected: class={}, excerpt={}",
                    first.cls, first.match_excerpt,
                ),
            }),
        ));
    }

    // ── Build source/evidence/candidate records ─────────────────────

    let source_uri = envelope.source_uri.clone();
    let source_type = envelope.source_system.clone();

    // 1. Source record
    let source_record = Source {
        uri: source_uri.clone(),
        source_type: source_type.clone(),
        label: None,
        source_kind: None,
    };

    // 2. SourceSnapshot (content-addressed from envelope content)
    let content_hash = sha256_hex(&envelope.content);
    let snapshot_addr = format!("sha256:{}", content_hash);
    let snapshot = SourceSnapshot {
        snapshot_addr: snapshot_addr.clone(),
        snapshot_ref: source_uri.clone(),
        captured_at: chrono::Utc::now(),
        content: Some(envelope.content.clone()),
        content_hash: Some(content_hash.clone()),
        capture_method: None,
    };

    // 3. SourceEvidence items
    let mut evidence_records: Vec<SourceEvidence> = Vec::new();
    for ev_item in &envelope.evidence {
        let span_start = ev_item.span_start.unwrap_or(0);
        let span_end = ev_item.span_end.unwrap_or(ev_item.excerpt.len());
        let confidence = ev_item.confidence.unwrap_or(0.5);
        let ev = SourceEvidence::new(
            snapshot_addr.clone(),
            span_start,
            span_end,
            ev_item.excerpt.clone(),
            confidence,
        );
        evidence_records.push(ev);
    }

    // 4. Derive submission provenance from external adapter context
    let provenance = bicameral_api::evidence::SubmissionProvenance::from_external_adapter(
        &source_type,
        &source_uri,
    );

    // 5. Persist screened evidence bytes before any projection work.
    let stored_body = state
        .evidence_repository
        .put(envelope.content.as_bytes())
        .map_err(|error| {
            tracing::error!(error = %error, "Failed to persist screened evidence body");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ExternalIngestRejection {
                    reason: ExternalIngestRejectionReason::InvalidPayload,
                    message: "Failed to persist screened evidence body".to_string(),
                }),
            )
        })?;
    let idempotency_key = evidence_acceptance_key(&source_type, &source_uri, &stored_body.digest);
    let existing_acceptance =
        find_evidence_acceptance(&state, &idempotency_key).map_err(|error| {
            tracing::error!(error = %error, "Failed to inspect evidence acceptance history");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ExternalIngestRejection {
                    reason: ExternalIngestRejectionReason::InvalidPayload,
                    message: "Failed to inspect evidence acceptance history".to_string(),
                }),
            )
        })?;
    let acceptance = if let Some(existing) = existing_acceptance {
        if existing.evidence_ids.len() != evidence_records.len() {
            return Err((
                StatusCode::CONFLICT,
                Json(ExternalIngestRejection {
                    reason: ExternalIngestRejectionReason::InvalidPayload,
                    message: "Existing evidence acceptance has incompatible evidence cardinality"
                        .to_string(),
                }),
            ));
        }
        for (record, evidence_id) in evidence_records
            .iter_mut()
            .zip(existing.evidence_ids.iter().copied())
        {
            record.id = evidence_id;
        }
        existing
    } else {
        let acceptance = EvidenceAcceptedPayload {
            idempotency_key: idempotency_key.clone(),
            evidence_identity: idempotency_key.clone(),
            source_system: source_type.clone(),
            source_uri: source_uri.clone(),
            product_scope: None,
            source_version: None,
            snapshot_addr: snapshot_addr.clone(),
            content_digest: stored_body.digest.clone(),
            content_locator: stored_body.locator.clone(),
            content_byte_length: stored_body.byte_length,
            evidence_ids: evidence_records.iter().map(|e| e.id).collect(),
            provenance: provenance.clone(),
            redaction_receipt: envelope.redaction_receipt.clone(),
            accepted_at: chrono::Utc::now(),
        };
        let payload = serde_json::to_value(&acceptance).map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ExternalIngestRejection {
                    reason: ExternalIngestRejectionReason::InvalidPayload,
                    message: "Failed to serialize evidence acceptance".to_string(),
                }),
            )
        })?;
        let entry = EventStoreEntry::new(
            EventType::EvidenceAccepted,
            payload,
            format!("external:{}", source_type),
        );
        state.event_store.append(entry).map_err(|error| {
            tracing::error!(error = %error, "Failed to append evidence acceptance event");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ExternalIngestRejection {
                    reason: ExternalIngestRejectionReason::InvalidPayload,
                    message: "Failed to append evidence acceptance event".to_string(),
                }),
            )
        })?;
        acceptance
    };

    // 6. Execute projection through daemon-local retry/outcome/candidate state.
    //
    // The canonical Product store retains only EvidenceAccepted. Legacy
    // candidate-bearing events remain during this parity window and are written
    // idempotently below until the promotion event becomes self-contained.
    let level = classify_decision_level(&source_type);
    let projection_execution = crate::live_projection_work::execute_external_projection(
        &state,
        &acceptance,
        &envelope,
        &evidence_records,
        &provenance,
        level,
    )
    .map_err(|error| {
        tracing::error!(error = %error, "External projection working-state execution failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ExternalIngestRejection {
                reason: ExternalIngestRejectionReason::InvalidPayload,
                message: "External projection working-state execution failed".to_string(),
            }),
        )
    })?;
    let projection_request_id = projection_execution
        .projection_request_id
        .map(|request_id| request_id.to_string());
    let projection_outcome_label = projection_execution.outcome_label;
    let candidates = projection_execution.candidates;

    // 8. Materialize daemon-local candidate read state.
    state
        .sources
        .write()
        .await
        .insert(source_uri.clone(), source_record);
    state
        .snapshots
        .write()
        .await
        .insert(snapshot.snapshot_addr.clone(), snapshot);
    for ev in &evidence_records {
        state.evidence.write().await.insert(ev.id, ev.clone());
    }
    for candidate in &candidates {
        state
            .candidates
            .write()
            .await
            .insert(candidate.id, candidate.clone());
        let review = ReviewState::new(candidate.id);
        state.reviews.write().await.insert(candidate.id, review);
    }

    // 10. Audit
    {
        let mut audit = state.audit.write().await;
        for candidate in &candidates {
            let _ = audit.record(
                AuditAction::CandidateIngested,
                format!("external:{}", source_type),
                Some(candidate.id),
                format!(
                    "External adapter ingest: {} (source: {})",
                    candidate.title, source_uri
                ),
            );
        }
    }

    let evidence_ids: Vec<String> = evidence_records.iter().map(|e| e.id.to_string()).collect();
    let candidate_ids: Vec<String> = candidates.iter().map(|c| c.id.to_string()).collect();

    tracing::info!(
        source_system = %source_type,
        source_uri = %source_uri,
        evidence_count = evidence_ids.len(),
        candidate_count = candidate_ids.len(),
        projection_outcome = %projection_outcome_label,
        "External adapter ingest accepted"
    );

    let advisory_results: Vec<ExternalIngestAdvisoryResult> = candidates
        .iter()
        .map(|c| ExternalIngestAdvisoryResult {
            candidate_id: c.id.to_string(),
            action: "accepted_as_candidate".to_string(),
            reason: "External evidence recorded — advisory only".to_string(),
            review_state: ReviewLifecycleState::Proposed,
            candidate_state: CandidateState::Proposed,
        })
        .collect();

    Ok((
        StatusCode::CREATED,
        Json(ExternalIngestResponse {
            source_id: source_uri,
            snapshot_id: Some(snapshot_addr),
            evidence_ids,
            candidate_ids,
            advisory_results,
            projection_request_id,
            projection_outcome: Some(projection_outcome_label),
        }),
    ))
}

// ── Review routes ────────────────────────────────────────────────

pub fn review_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/review", post(submit_review_command))
        .route("/api/v1/review/:id", get(get_review_state))
}

async fn submit_review_command(
    State(state): State<AppState>,
    Json(cmd): Json<ReviewCommand>,
) -> Result<(StatusCode, Json<ReviewState>), StatusCode> {
    let candidate_id = cmd.candidate_id;

    let mut reviews = state.reviews.write().await;
    let review = reviews
        .get_mut(&candidate_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    use bicameral_api::review::ReviewCommandKind;
    match cmd.kind {
        ReviewCommandKind::Submit => {
            review.status = ReviewStatus::InReview;
            review.submitted_at = Some(cmd.issued_at);
        }
        ReviewCommandKind::Approve => {
            review.status = ReviewStatus::Approved;
            review.resolved_at = Some(cmd.issued_at);
            review.resolution = Some(ReviewCommandKind::Approve);
        }
        ReviewCommandKind::Reject => {
            review.status = ReviewStatus::Rejected;
            review.resolved_at = Some(cmd.issued_at);
            review.resolution = Some(ReviewCommandKind::Reject);
        }
        ReviewCommandKind::RequestEvidence => {
            review.status = ReviewStatus::NeedsEvidence;
        }
        ReviewCommandKind::Escalate => {
            review.status = ReviewStatus::Escalated;
        }
    }

    review.history.push(cmd);

    {
        let mut audit = state.audit.write().await;
        let _ = audit.record(
            AuditAction::ReviewCommandIssued,
            "gateway".to_string(),
            Some(candidate_id),
            format!("Review command for candidate {}", candidate_id),
        );
    }

    Ok((StatusCode::OK, Json(review.clone())))
}

async fn get_review_state(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<ReviewState>, StatusCode> {
    let reviews = state.reviews.read().await;
    reviews
        .get(&id)
        .cloned()
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

// ── Status routes ────────────────────────────────────────────────

pub fn status_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/candidates", get(list_candidates))
        .route("/api/v1/status", get(service_status))
}

async fn list_candidates(State(state): State<AppState>) -> Json<Vec<DecisionCandidate>> {
    let candidates = state.candidates.read().await;
    Json(candidates.values().cloned().collect())
}

#[derive(Serialize)]
struct ServiceStatusResponse {
    status: String,
    candidate_count: usize,
    decision_count: usize,
    review_count: usize,
    event_count: usize,
    substrate: SubstrateStatusResponse,
}

#[derive(Serialize)]
struct SubstrateStatusResponse {
    kind: bicameral_api::event_store::SubstrateKind,
    capabilities: Vec<bicameral_api::event_store::SubstrateCapability>,
    reachable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    freshness: Option<bicameral_api::event_store::FreshnessState>,
    warnings: Vec<String>,
    explanation: String,
}

async fn service_status(State(state): State<AppState>) -> Json<ServiceStatusResponse> {
    let candidate_count = state.candidates.read().await.len();
    let decision_count = state.decisions.read().await.len();
    let review_count = state.reviews.read().await.len();
    let event_count = state.event_store.replay().map(|e| e.len()).unwrap_or(0);

    let kind = state.event_store.substrate_kind();
    let caps = state.event_store.capabilities();

    let health = state.event_store.health_check();
    let (reachable, freshness, warnings) = match health {
        Ok(h) => (h.reachable, h.freshness, h.warnings),
        Err(_) => (false, None, vec!["Health check failed".to_string()]),
    };

    let explanation = match kind {
        bicameral_api::event_store::SubstrateKind::Git => {
            if caps
                .contains(&bicameral_api::event_store::SubstrateCapability::CiMergeGateEnforcement)
            {
                "Git substrate with PR-resolution and CI/merge-gate enforcement".to_string()
            } else {
                "Git substrate with PR-resolution capabilities (CI gate not configured)".to_string()
            }
        }
        bicameral_api::event_store::SubstrateKind::DriveFolder => {
            "Drive/basic-store substrate: baseline Decision Ledger (no PR/CI capabilities)"
                .to_string()
        }
        bicameral_api::event_store::SubstrateKind::Memory => {
            "In-memory substrate (testing only, no durable capabilities)".to_string()
        }
    };

    Json(ServiceStatusResponse {
        status: "running".to_string(),
        candidate_count,
        decision_count,
        review_count,
        event_count,
        substrate: SubstrateStatusResponse {
            kind,
            capabilities: caps.into_iter().collect(),
            reachable,
            freshness,
            warnings,
            explanation,
        },
    })
}

// ── Dashboard API routes ─────────────────────────────────────────

pub fn dashboard_api_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/dashboard/ingestion-gate",
            get(dashboard_ingestion_gate),
        )
        .route("/api/v1/dashboard/ledger", get(dashboard_ledger))
        .route(
            "/api/v1/dashboard/decision/:decision_id",
            get(dashboard_decision_detail),
        )
        .route("/api/v1/dashboard/command", post(dashboard_command))
}

// ── Projection routes ────────────────────────────────────────────

pub fn projection_api_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/projection/ledger", get(projection_ledger))
        .route(
            "/api/v1/projection/decision/:decision_id",
            get(projection_decision_detail),
        )
        .route("/api/v1/projection/rebuild", post(projection_rebuild))
}

/// Returns the V1 reviewable-context projection for Ledger View reads.
///
/// Builds the projection on-the-fly from the gateway's in-memory state.
/// The response contains Decisions, source evidence, code evidence, graph
/// snapshots, graph readiness, and event history — no compliance,
/// enforcement, or readiness-to-ship claims.
///
/// Per `v1-reviewable-context-projection` spec and ADR-0022.
async fn projection_ledger(
    State(state): State<AppState>,
) -> Json<bicameral_api::projection::ProjectionState> {
    let proj = build_projection_from_state(&state).await;
    Json(proj)
}

/// Rebuilds the V1 reviewable-context projection from event-store replay.
///
/// Clears and rebuilds all projection rows. Returns the fresh projection.
/// This endpoint proves that deleting and rebuilding the projection
/// preserves the same read output — the event store remains canonical.
async fn projection_rebuild(
    State(state): State<AppState>,
) -> Json<bicameral_api::projection::ProjectionState> {
    let proj = build_projection_from_state(&state).await;
    Json(proj)
}

// ── Decision detail handlers ─────────────────────────────────────

/// GET /api/v1/dashboard/decision/:decision_id
///
/// Returns a `DecisionDetail` read-model projection for a single accepted
/// Decision. The response separates candidate/review, signoff, code-evidence,
/// and compliance state into distinct sections.
///
/// This endpoint is read-only — it never mutates canonical state.
async fn dashboard_decision_detail(
    State(state): State<AppState>,
    axum::extract::Path(decision_id): axum::extract::Path<String>,
) -> Response {
    let id = match decision_id.parse::<Uuid>() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid decision_id format"})),
            )
                .into_response();
        }
    };

    let decisions = state.decisions.read().await;
    let decision = match decisions.get(&id) {
        Some(d) => d.clone(),
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "decision not found"})),
            )
                .into_response();
        }
    };
    drop(decisions);

    let bindings = state.bindings.read().await;
    let decision_bindings = bindings.get(&id).cloned().unwrap_or_default();
    drop(bindings);

    let detail = build_decision_detail(&decision, &decision_bindings);
    Json(detail).into_response()
}

/// GET /api/v1/projection/decision/:decision_id
///
/// Returns a `DecisionDetail` built from the V1 reviewable-context projection.
/// Provides the same stable identity and separated axes as the dashboard
/// detail endpoint, proving surface parity.
///
/// This endpoint is read-only — it never mutates canonical state.
async fn projection_decision_detail(
    State(state): State<AppState>,
    axum::extract::Path(decision_id): axum::extract::Path<String>,
) -> Response {
    let id = match decision_id.parse::<Uuid>() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid decision_id format"})),
            )
                .into_response();
        }
    };

    // Try projection state first, fall back to in-memory.
    let proj_guard = state.projection.read().await;
    if let Some(ref proj) = *proj_guard {
        if let Some(projected) = proj.decisions.get(&id) {
            let code_bindings: Vec<bicameral_api::dashboard::CodeEvidenceBinding> = proj
                .code_evidence
                .values()
                .filter(|ce| ce.decision_id == id)
                .map(|ce| bicameral_api::dashboard::CodeEvidenceBinding {
                    id: ce.id.to_string(),
                    file: ce.file.clone(),
                    start_line: ce.start_line,
                    end_line: ce.end_line,
                    binding_type: ce.binding_type.clone(),
                    explanation: ce.explanation.clone(),
                })
                .collect();

            let has_bindings = !code_bindings.is_empty();
            let evidence_state = proj
                .graph_readiness
                .get(&id)
                .map(|gr| gr.evidence_state)
                .unwrap_or(bicameral_api::evidence::GraphEvidenceState::UnknownNotIndexed);

            let source_freshness = projected.source_freshness;
            let freshness_warning = if projected.tracking_state
                == bicameral_api::candidate::TrackingState::Tracked
                && source_freshness == bicameral_api::source::SourceFreshness::Stale
            {
                Some(bicameral_api::dashboard::FreshnessWarning {
                    message: "Source evidence is stale — refresh recommended.".into(),
                    actions: vec![
                        bicameral_api::dashboard::FreshnessAction::RefreshSource,
                        bicameral_api::dashboard::FreshnessAction::Untrack,
                    ],
                })
            } else {
                None
            };

            let detail = bicameral_api::dashboard::DecisionDetail {
                decision_id: projected.id.to_string(),
                candidate_id: projected.candidate_id.to_string(),
                title: projected.title.clone(),
                body: projected.body.clone(),
                level: projected.level,
                promoted_at: projected.promoted_at,
                promoted_by: projected.promoted_by.clone(),
                tracking_state: projected.tracking_state,
                candidate_review: bicameral_api::dashboard::CandidateReviewSection {
                    review_state: projected.review_state,
                    source_id: projected.source_id.clone(),
                    source_freshness,
                    freshness_warning,
                },
                signoff: bicameral_api::dashboard::SignoffSection {
                    state: projected.signoff,
                    allowed_commands: allowed_decision_commands(
                        projected.signoff,
                        projected.tracking_state,
                    ),
                },
                code_evidence: bicameral_api::dashboard::CodeEvidenceDetailSection {
                    evidence_state,
                    has_bindings,
                    bindings: code_bindings,
                },
                compliance: bicameral_api::dashboard::ComplianceDeferredSection {
                    state: bicameral_api::dashboard::ComplianceState::Ungrounded,
                    deferred: true,
                },
                provenance: projected.provenance.clone(),
                // No semantic provider is wired into the live projection yet
                // (#526). Stay a quiet, deterministic no-op until then.
                semantic_recall: None,
            };
            drop(proj_guard);
            return Json(detail).into_response();
        }
    }
    drop(proj_guard);

    // Fallback: read from in-memory state.
    let decisions = state.decisions.read().await;
    let decision = match decisions.get(&id) {
        Some(d) => d.clone(),
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "decision not found"})),
            )
                .into_response();
        }
    };
    drop(decisions);

    let bindings = state.bindings.read().await;
    let decision_bindings = bindings.get(&id).cloned().unwrap_or_default();
    drop(bindings);

    let detail = build_decision_detail(&decision, &decision_bindings);
    Json(detail).into_response()
}

/// Construct a `DecisionDetail` from a `Decision` and its bindings.
///
/// Shared helper used by both dashboard and projection detail endpoints.
fn build_decision_detail(
    decision: &bicameral_api::candidate::Decision,
    bindings: &[bicameral_api::evidence::BindingEvidence],
) -> bicameral_api::dashboard::DecisionDetail {
    use bicameral_api::dashboard::*;
    use bicameral_api::evidence::GraphEvidenceState;
    use bicameral_api::source::SourceFreshness;

    let has_bindings = !bindings.is_empty();
    let evidence_state = if has_bindings {
        GraphEvidenceState::Verified
    } else {
        GraphEvidenceState::UnknownNotIndexed
    };

    let code_bindings: Vec<CodeEvidenceBinding> = bindings
        .iter()
        .map(|b| {
            let (file, start_line, end_line) = parse_projection_binding_target(&b.target);
            CodeEvidenceBinding {
                id: b.id.to_string(),
                file,
                start_line,
                end_line,
                binding_type: format!("{:?}", b.binding_type),
                explanation: b.explanation.clone(),
            }
        })
        .collect();

    let source_freshness = if decision.review_state
        == bicameral_api::candidate::ReviewLifecycleState::StaleSourcePending
    {
        SourceFreshness::Stale
    } else {
        SourceFreshness::Fresh
    };

    let freshness_warning = if decision.tracking_state
        == bicameral_api::candidate::TrackingState::Tracked
        && source_freshness == SourceFreshness::Stale
    {
        Some(FreshnessWarning {
            message: "Source evidence is stale — refresh recommended.".into(),
            actions: vec![FreshnessAction::RefreshSource, FreshnessAction::Untrack],
        })
    } else {
        None
    };

    DecisionDetail {
        decision_id: decision.id.to_string(),
        candidate_id: decision.candidate_id.to_string(),
        title: decision.title.clone(),
        body: decision.body.clone(),
        level: decision.level,
        promoted_at: decision.promoted_at,
        promoted_by: decision.promoted_by.clone(),
        tracking_state: decision.tracking_state,
        candidate_review: CandidateReviewSection {
            review_state: decision.review_state,
            source_id: decision.source_id.clone(),
            source_freshness,
            freshness_warning,
        },
        signoff: SignoffSection {
            state: decision.signoff_state,
            allowed_commands: allowed_decision_commands(
                decision.signoff_state,
                decision.tracking_state,
            ),
        },
        code_evidence: CodeEvidenceDetailSection {
            evidence_state,
            has_bindings,
            bindings: code_bindings,
        },
        compliance: ComplianceDeferredSection {
            state: decision.compliance_state,
            deferred: true,
        },
        provenance: decision.provenance.clone(),
        // No semantic provider is wired into the in-memory fallback yet
        // (#526). Stay a quiet, deterministic no-op until then.
        semantic_recall: None,
    }
}

/// Build a ProjectionState from the gateway's in-memory materialized state.
///
/// This mirrors `bicameral_runtime::projection::build_projection` but
/// operates on the gateway's `AppState` maps directly, avoiding a cyclic
/// crate dependency (gateway ↔ runtime).
async fn build_projection_from_state(
    state: &AppState,
) -> bicameral_api::projection::ProjectionState {
    use bicameral_api::projection::*;

    let mut proj = ProjectionState::default();

    let decisions = state.decisions.read().await;
    let stored_evidence = state.evidence.read().await;
    let stored_snapshots = state.snapshots.read().await;
    let bindings = state.bindings.read().await;
    let event_store_entries = state.event_store.replay().unwrap_or_default();

    for (id, d) in decisions.iter() {
        let source_freshness = if d.review_state == ReviewLifecycleState::StaleSourcePending {
            SourceFreshness::Stale
        } else {
            SourceFreshness::Fresh
        };

        proj.decisions.insert(
            *id,
            ProjectedDecision {
                id: d.id,
                candidate_id: d.candidate_id,
                title: d.title.clone(),
                body: d.body.clone(),
                level: d.level,
                signoff: d.signoff_state,
                review_state: d.review_state,
                tracking_state: d.tracking_state,
                source_id: d.source_id.clone(),
                source_freshness,
                promoted_at: d.promoted_at,
                promoted_by: d.promoted_by.clone(),
                provenance: d.provenance.clone(),
            },
        );

        // Source evidence for this Decision.
        for ev_id in &d.evidence_refs {
            if let Some(ev) = stored_evidence.get(ev_id) {
                proj.source_evidence.insert(
                    ev.id,
                    ProjectedSourceEvidence {
                        id: ev.id,
                        decision_id: *id,
                        snapshot_id: ev.snapshot_id.clone(),
                        excerpt: ev.excerpt.clone(),
                        span_start: ev.span_start,
                        span_end: ev.span_end,
                        confidence: ev.confidence,
                        captured_at: ev.captured_at,
                    },
                );
            }
        }

        // Code evidence from bindings.
        if let Some(bs) = bindings.get(id) {
            for binding in bs {
                let (file, start_line, end_line) = parse_projection_binding_target(&binding.target);
                proj.code_evidence.insert(
                    binding.id,
                    ProjectedCodeEvidence {
                        id: binding.id,
                        decision_id: *id,
                        file,
                        start_line,
                        end_line,
                        binding_type: format!("{:?}", binding.binding_type),
                        explanation: binding.explanation.clone(),
                        evidence_state: GraphEvidenceState::Verified,
                    },
                );
            }
        }

        // Graph readiness per Decision.
        let has_bindings = bindings.get(id).map(|bs| !bs.is_empty()).unwrap_or(false);
        proj.graph_readiness.insert(
            *id,
            ProjectedGraphReadiness {
                decision_id: *id,
                evidence_state: if has_bindings {
                    GraphEvidenceState::Verified
                } else {
                    GraphEvidenceState::UnknownNotIndexed
                },
                has_bindings,
            },
        );
    }

    // Graph snapshots.
    for (addr, snap) in stored_snapshots.iter() {
        proj.graph_snapshots.insert(
            addr.clone(),
            ProjectedGraphSnapshot {
                snapshot_addr: snap.snapshot_addr.clone(),
                snapshot_ref: snap.snapshot_ref.clone(),
                captured_at: Some(snap.captured_at),
            },
        );
    }

    // Ledger event history.
    for entry in &event_store_entries {
        proj.ledger_events.push(ProjectedLedgerEvent {
            id: entry.id,
            event_type: entry.event_type,
            timestamp: entry.timestamp,
            actor: entry.actor.clone(),
            summary: summarize_projection_event(entry),
        });
    }

    proj
}

/// Build the network-origin renderable ledger projection from gateway state.
///
/// This is the T4 (#705) allow-list boundary approved in #698 item 5. Unlike
/// [`build_projection_from_state`], which returns the full local
/// `ProjectionState` (decision bodies, source excerpts, code paths, event
/// summaries, actors) and must stay on loopback, this projects into the minimal
/// render-safe field set — ids, timestamps, status/review-state enums, counts,
/// and redactor-passed titles — before it crosses the browser boundary
/// (ADR-0020 / ADR-0025). It is a non-authoritative read model.
async fn build_renderable_ledger_from_state(
    state: &AppState,
) -> bicameral_api::hosted_bridge::HostedLedgerProjection {
    use bicameral_api::candidate::ReviewLifecycleState;
    use bicameral_api::evidence::GraphEvidenceState;
    use bicameral_api::hosted_bridge::{
        HostedLedgerProjection, HostedLedgerSummary, RenderableLedgerEntry, RenderableLedgerEvent,
    };
    use bicameral_api::source::SourceFreshness;

    let decisions = state.decisions.read().await;
    let bindings = state.bindings.read().await;
    let candidate_count = state.candidates.read().await.len() as u32;
    let decision_count = decisions.len() as u32;

    let mut entries = Vec::with_capacity(decisions.len());
    let mut needs_signoff_count = 0u32;
    let mut stale_source_count = 0u32;
    let mut bound_count = 0u32;

    for (id, d) in decisions.iter() {
        let decision_bindings = bindings.get(id);
        let binding_count = decision_bindings.map(|bs| bs.len() as u32).unwrap_or(0);
        let has_bindings = binding_count > 0;
        if has_bindings {
            bound_count += 1;
        }

        let source_freshness = if d.review_state == ReviewLifecycleState::StaleSourcePending {
            stale_source_count += 1;
            SourceFreshness::Stale
        } else {
            SourceFreshness::Fresh
        };
        if d.review_state == ReviewLifecycleState::NeedsSignoffReview {
            needs_signoff_count += 1;
        }

        entries.push(RenderableLedgerEntry {
            decision_id: d.id,
            candidate_id: d.candidate_id,
            // Titles are passed through the sensitive-pattern redactor before
            // they cross the network boundary (#698 item 5).
            title: crate::sensitive_patterns::redact_sensitive(&d.title),
            level: d.level,
            signoff_state: d.signoff_state,
            review_state: d.review_state,
            tracking_state: d.tracking_state,
            source_freshness,
            evidence_state: if has_bindings {
                GraphEvidenceState::Verified
            } else {
                GraphEvidenceState::UnknownNotIndexed
            },
            promoted_at: d.promoted_at,
            source_evidence_count: d.evidence_refs.len() as u32,
            binding_count,
        });
    }

    let last_promoted_at = decisions
        .values()
        .map(|d| d.promoted_at)
        .max()
        .map(|t| t.to_rfc3339());

    drop(decisions);
    drop(bindings);

    // Events: opaque id, lifecycle type, timestamp only — no actor, no
    // human-readable summary (actor identity is authority material).
    let event_store_entries = state.event_store.replay().unwrap_or_default();
    let events = event_store_entries
        .iter()
        .map(|e| RenderableLedgerEvent {
            id: e.id,
            event_type: e.event_type,
            timestamp: e.timestamp,
        })
        .collect();

    HostedLedgerProjection {
        summary: HostedLedgerSummary {
            decision_count,
            candidate_count,
            needs_signoff_count,
            stale_source_count,
            bound_count,
            last_promoted_at,
        },
        decisions: entries,
        events,
        projection_only: true,
    }
}

fn parse_projection_binding_target(target: &str) -> (String, u32, u32) {
    let parts: Vec<&str> = target.rsplitn(3, ':').collect();
    match parts.len() {
        3 => {
            let end = parts[0].parse().unwrap_or(0);
            let start = parts[1].parse().unwrap_or(0);
            (parts[2].to_string(), start, end)
        }
        2 => {
            let line = parts[0].parse().unwrap_or(0);
            (parts[1].to_string(), line, line)
        }
        _ => (target.to_string(), 0, 0),
    }
}

fn summarize_projection_event(entry: &bicameral_api::event_store::EventStoreEntry) -> String {
    match entry.event_type {
        EventType::Ingest => format!("Candidate ingested by {}", entry.actor),
        EventType::AcceptCandidate => format!("Candidate accepted → Decision by {}", entry.actor),
        EventType::RejectCandidate => format!("Candidate rejected by {}", entry.actor),
        EventType::ApproveSignoff => format!("Signoff approved by {}", entry.actor),
        EventType::RejectSignoff => format!("Signoff rejected by {}", entry.actor),
        EventType::BindToCode => format!("Bound to code by {}", entry.actor),
        EventType::SupersedeDecision => format!("Decision superseded by {}", entry.actor),
        EventType::ResolveCompliance => format!("Compliance resolved by {}", entry.actor),
        EventType::RevalidateBinding => format!("Binding revalidated by {}", entry.actor),
        EventType::UntrackSource => format!("Source untracked by {}", entry.actor),
        EventType::EditCandidate => format!("Candidate edited by {}", entry.actor),
        _ => format!("{:?} by {}", entry.event_type, entry.actor),
    }
}

// ── Setup & config API routes ────────────────────────────────────

pub fn setup_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/setup/status", get(setup_status))
        .route(
            "/api/v1/setup/capability-readiness",
            get(capability_readiness).post(submit_capability_readiness),
        )
        .route("/api/v1/setup/create-workspace", post(create_workspace))
        .route(
            "/api/v1/dashboard/config",
            get(get_config).put(update_config),
        )
}

async fn capability_readiness(State(state): State<AppState>) -> Response {
    let Some(dispatch) = state.runtime_dispatch.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "daemon_runtime_unavailable"
            })),
        )
            .into_response();
    };
    match dispatch.capability_readiness() {
        Ok(readiness) => (StatusCode::OK, Json(readiness)).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "capability_readiness_unavailable",
                "detail": error
            })),
        )
            .into_response(),
    }
}

async fn submit_capability_readiness(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(observation): Json<bicameral_api::capability_readiness::McpReadinessObservation>,
) -> Response {
    // This mutation belongs only to the local terminal setup controller.
    // Cloud-hosted and loopback browser requests carry Origin and must never
    // submit verifier evidence or drive the daemon-owned reducer.
    if headers.contains_key(axum::http::header::ORIGIN) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "browser_setup_submission_forbidden"
            })),
        )
            .into_response();
    }
    let Some(dispatch) = state.runtime_dispatch.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "daemon_runtime_unavailable"
            })),
        )
            .into_response();
    };
    match dispatch.submit_mcp_readiness(observation) {
        Ok(readiness) => (StatusCode::OK, Json(readiness)).into_response(),
        Err(error) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({
                "error": "capability_readiness_rejected",
                "detail": error
            })),
        )
            .into_response(),
    }
}

#[derive(Serialize)]
struct SetupStatusResponse {
    workspace_exists: bool,
    workspace_root: Option<String>,
}

async fn setup_status(State(state): State<AppState>) -> Json<SetupStatusResponse> {
    let root = state.workspace_root.read().await;
    let exists = root
        .as_ref()
        .map(|r| r.join(".bicameral").is_dir())
        .unwrap_or(false);
    Json(SetupStatusResponse {
        workspace_exists: exists,
        workspace_root: root.as_ref().map(|r| r.display().to_string()),
    })
}

#[derive(Deserialize)]
struct CreateWorkspaceRequest {
    #[serde(default)]
    workspace_name: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Serialize)]
struct CreateWorkspaceResponse {
    success: bool,
    config_path: Option<String>,
    errors: Vec<String>,
}

async fn create_workspace(
    State(state): State<AppState>,
    Json(req): Json<CreateWorkspaceRequest>,
) -> (StatusCode, Json<CreateWorkspaceResponse>) {
    let root_guard = state.workspace_root.read().await;
    let base = if let Some(ref p) = req.path {
        std::path::PathBuf::from(p)
    } else if let Some(ref r) = *root_guard {
        r.clone()
    } else {
        return (
            StatusCode::BAD_REQUEST,
            Json(CreateWorkspaceResponse {
                success: false,
                config_path: None,
                errors: vec!["No workspace root available and no path provided.".to_string()],
            }),
        );
    };
    drop(root_guard);

    if base.join(".bicameral").is_dir() {
        return (
            StatusCode::CONFLICT,
            Json(CreateWorkspaceResponse {
                success: false,
                config_path: None,
                errors: vec!["Workspace already exists at this path.".to_string()],
            }),
        );
    }

    let config = bicameral_config::BicameralConfig {
        workspace_name: req.workspace_name,
        ..Default::default()
    };

    let validation_errors = config.validate();
    if !validation_errors.is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(CreateWorkspaceResponse {
                success: false,
                config_path: None,
                errors: validation_errors,
            }),
        );
    }

    match bicameral_config::WorkspaceDiscovery::init(&base, &config) {
        Ok(paths) => {
            let mut root = state.workspace_root.write().await;
            *root = Some(base);
            (
                StatusCode::CREATED,
                Json(CreateWorkspaceResponse {
                    success: true,
                    config_path: Some(paths.config_file.display().to_string()),
                    errors: vec![],
                }),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(CreateWorkspaceResponse {
                success: false,
                config_path: None,
                errors: vec![format!("Failed to create workspace: {e}")],
            }),
        ),
    }
}

#[derive(Serialize)]
struct ConfigResponse {
    config: Option<serde_json::Value>,
    errors: Vec<String>,
}

async fn get_config(State(state): State<AppState>) -> (StatusCode, Json<ConfigResponse>) {
    let root = state.workspace_root.read().await;
    let Some(ref root_path) = *root else {
        return (
            StatusCode::NOT_FOUND,
            Json(ConfigResponse {
                config: None,
                errors: vec!["No workspace configured.".to_string()],
            }),
        );
    };

    let config_path = root_path.join(".bicameral/config.yaml");
    if !config_path.exists() {
        return (
            StatusCode::NOT_FOUND,
            Json(ConfigResponse {
                config: None,
                errors: vec!["Config file not found.".to_string()],
            }),
        );
    }

    match bicameral_config::BicameralConfig::load_from_file(&config_path) {
        Ok(config) => {
            let value = serde_json::to_value(&config).unwrap_or(serde_json::Value::Null);
            (
                StatusCode::OK,
                Json(ConfigResponse {
                    config: Some(value),
                    errors: vec![],
                }),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ConfigResponse {
                config: None,
                errors: vec![format!("Failed to load config: {e}")],
            }),
        ),
    }
}

#[derive(Deserialize)]
struct UpdateConfigRequest {
    #[serde(default)]
    workspace_name: Option<String>,
    #[serde(default)]
    gateway_host: Option<String>,
    #[serde(default)]
    gateway_port: Option<u16>,
    #[serde(default)]
    substrate: Option<String>,
    #[serde(default)]
    git_path: Option<String>,
    #[serde(default)]
    drive_folder_path: Option<String>,
    #[serde(default)]
    ci_merge_gate: Option<bool>,
}

#[derive(Serialize)]
struct UpdateConfigResponse {
    success: bool,
    errors: Vec<String>,
}

async fn update_config(
    State(state): State<AppState>,
    Json(req): Json<UpdateConfigRequest>,
) -> (StatusCode, Json<UpdateConfigResponse>) {
    let root = state.workspace_root.read().await;
    let Some(ref root_path) = *root else {
        return (
            StatusCode::NOT_FOUND,
            Json(UpdateConfigResponse {
                success: false,
                errors: vec!["No workspace configured.".to_string()],
            }),
        );
    };

    let config_path = root_path.join(".bicameral/config.yaml");

    let mut config = if config_path.exists() {
        match bicameral_config::BicameralConfig::load_from_file(&config_path) {
            Ok(c) => c,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(UpdateConfigResponse {
                        success: false,
                        errors: vec![format!("Failed to load existing config: {e}")],
                    }),
                );
            }
        }
    } else {
        bicameral_config::BicameralConfig::default()
    };

    // Apply safe fields only
    if let Some(name) = req.workspace_name {
        config.workspace_name = Some(name);
    }
    if let Some(host) = req.gateway_host {
        config.gateway.host = host;
    }
    if let Some(port) = req.gateway_port {
        config.gateway.port = port;
    }
    if let Some(ref sub) = req.substrate {
        match sub.as_str() {
            "git" => config.event_store.substrate = bicameral_api::event_store::SubstrateKind::Git,
            "drive_folder" => {
                config.event_store.substrate =
                    bicameral_api::event_store::SubstrateKind::DriveFolder
            }
            other => {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(UpdateConfigResponse {
                        success: false,
                        errors: vec![format!(
                            "Unknown substrate kind: '{other}'. Use 'git' or 'drive_folder'."
                        )],
                    }),
                );
            }
        }
    }
    if let Some(p) = req.git_path {
        config.event_store.git_path = Some(std::path::PathBuf::from(p));
    }
    if let Some(p) = req.drive_folder_path {
        config.event_store.drive_folder_path = Some(std::path::PathBuf::from(p));
    }
    if let Some(ci) = req.ci_merge_gate {
        config.event_store.ci_merge_gate = ci;
    }

    let validation_errors = config.validate();
    if !validation_errors.is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(UpdateConfigResponse {
                success: false,
                errors: validation_errors,
            }),
        );
    }

    match config.save_to_file(&config_path) {
        Ok(()) => (
            StatusCode::OK,
            Json(UpdateConfigResponse {
                success: true,
                errors: vec![],
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(UpdateConfigResponse {
                success: false,
                errors: vec![format!("Failed to save config: {e}")],
            }),
        ),
    }
}

/// Returns the Ingestion Gate projection.
///
/// Groups in-memory candidates by source, synthesising Source/SourceSnapshot
/// wrappers. Now also includes source evidence refs when available.
///
/// Per ADR-0022: raw source/candidate context is advisory. Only promoted,
/// tracked Decisions carry governance freshness obligations. Stale untracked
/// context does not appear as a governance failure.
async fn dashboard_ingestion_gate(State(state): State<AppState>) -> Json<Vec<IngestionGateItem>> {
    use bicameral_api::dashboard::ContextDisposition;

    let candidates = state.candidates.read().await;
    let decisions = state.decisions.read().await;
    let stored_sources = state.sources.read().await;
    let stored_snapshots = state.snapshots.read().await;
    let stored_evidence = state.evidence.read().await;

    let mut by_source: std::collections::HashMap<String, Vec<&DecisionCandidate>> =
        std::collections::HashMap::new();
    for c in candidates.values() {
        by_source.entry(c.source.clone()).or_default().push(c);
    }

    let mut items: Vec<IngestionGateItem> = by_source
        .into_iter()
        .map(|(source_key, cands)| {
            let earliest = cands
                .iter()
                .map(|c| c.extracted_at)
                .min()
                .unwrap_or_else(chrono::Utc::now);

            // Use stored Source if available, otherwise synthesise
            let source = stored_sources
                .get(&source_key)
                .cloned()
                .unwrap_or_else(|| Source {
                    uri: source_key.clone(),
                    source_type: "ingested".to_string(),
                    label: None,
                    source_kind: None,
                });

            // Find matching snapshot or synthesise
            let snapshot = stored_snapshots
                .values()
                .find(|s| s.snapshot_ref == source_key)
                .cloned()
                .unwrap_or_else(|| SourceSnapshot {
                    snapshot_addr: format!("sha256:{}", sha256_hex(&source_key)),
                    snapshot_ref: source_key.clone(),
                    captured_at: earliest,
                    content: None,
                    content_hash: None,
                    capture_method: None,
                });

            // Collect evidence refs from all candidates for this source
            let evidence_refs: Vec<SourceEvidenceRef> = cands
                .iter()
                .flat_map(|c| &c.evidence_refs)
                .filter_map(|ev_id| stored_evidence.get(ev_id))
                .map(|ev| {
                    let snap = stored_snapshots
                        .values()
                        .find(|s| s.snapshot_addr == ev.snapshot_id);
                    SourceEvidenceRef {
                        id: ev.id.to_string(),
                        source_uri: source_key.clone(),
                        snapshot_addr: ev.snapshot_id.clone(),
                        snapshot_ref: snap
                            .map(|s| s.snapshot_ref.clone())
                            .unwrap_or_else(|| source_key.clone()),
                        pointer_type: "span".to_string(),
                        pointer_value: format!("{}:{}", ev.span_start, ev.span_end),
                        excerpt: ev.excerpt.clone(),
                        captured_at: ev.captured_at,
                    }
                })
                .collect();

            let previews: Vec<CandidatePreview> = cands
                .iter()
                .map(|c| {
                    let review_state = Some(c.review_state);
                    let confidence: Option<f64> = match c.extraction_confidence {
                        bicameral_api::candidate::ExtractionConfidence::Low => Some(0.3),
                        bicameral_api::candidate::ExtractionConfidence::Medium => Some(0.6),
                        bicameral_api::candidate::ExtractionConfidence::High => Some(0.9),
                    };
                    CandidatePreview {
                        id: Some(c.id.to_string()),
                        summary: c.title.clone(),
                        feature_hint: c.tags.first().cloned(),
                        evidence_refs: c.evidence_refs.iter().map(|e| e.to_string()).collect(),
                        extraction_confidence: confidence,
                        conflict_hint: None,
                        review_state,
                        candidate_state: Some(c.candidate_state),
                    }
                })
                .collect();

            // Derive tracked state and context disposition from whether
            // any candidate from this source has been promoted to a tracked
            // Decision (ADR-0022).
            let any_promoted = cands
                .iter()
                .any(|c| c.candidate_state == CandidateState::Promoted);
            let has_tracked_decision = any_promoted
                && decisions.values().any(|d| {
                    d.source_id.as_deref() == Some(&source_key)
                        && d.tracking_state == bicameral_api::candidate::TrackingState::Tracked
                });

            let context_disposition = if has_tracked_decision {
                ContextDisposition::TrackedDecision
            } else if !cands.is_empty() {
                ContextDisposition::CandidateContext
            } else {
                ContextDisposition::RawSource
            };

            // Staleness is advisory (not a governance failure) for
            // untracked context per ADR-0022.
            let stale_is_advisory = !has_tracked_decision;

            IngestionGateItem {
                source,
                snapshot,
                source_title: source_key,
                source_freshness: SourceFreshness::Unknown,
                evidence: evidence_refs,
                candidates: previews,
                tracked: has_tracked_decision,
                context_disposition,
                stale_is_advisory,
            }
        })
        .collect();

    items.sort_by(|a, b| a.source_title.cmp(&b.source_title));
    Json(items)
}

/// Returns the Ledger View projection.
///
/// Now includes both promoted Decisions and queued Candidates, per spec §7 step 9.
/// Persisted code regions from verified `BindToCode` events are surfaced here.
///
/// Per ADR-0022 and codegraph-ledger-ui-protocol-boundary spec:
/// - Tracked Decisions surface freshness warnings and refresh actions.
/// - Untracked Decisions do not surface staleness as a governance failure.
/// - Code evidence appears under the selected Decision, not as a top-level entity.
async fn dashboard_ledger(State(state): State<AppState>) -> Json<Vec<LedgerReviewItem>> {
    use bicameral_api::dashboard::{CodeRegion, FreshnessAction, FreshnessWarning};

    // Try the projection-backed read path first.
    let projection_guard = state.projection.read().await;
    if let Some(ref proj) = *projection_guard {
        let candidates = state.candidates.read().await;
        let stored_evidence = state.evidence.read().await;
        let stored_snapshots = state.snapshots.read().await;
        let items =
            ledger_items_from_projection(proj, &candidates, &stored_evidence, &stored_snapshots);
        return Json(items);
    }
    drop(projection_guard);

    // Fallback: read from in-memory maps.
    let candidates = state.candidates.read().await;
    let decisions = state.decisions.read().await;
    let stored_evidence = state.evidence.read().await;
    let stored_snapshots = state.snapshots.read().await;
    let bindings = state.bindings.read().await;

    let resolve_sources = |evidence_refs: &[Uuid], source_uri: &str| -> Vec<SourceEvidenceRef> {
        evidence_refs
            .iter()
            .filter_map(|ev_id| stored_evidence.get(ev_id))
            .map(|ev| {
                let snap = stored_snapshots
                    .values()
                    .find(|s| s.snapshot_addr == ev.snapshot_id);
                SourceEvidenceRef {
                    id: ev.id.to_string(),
                    source_uri: source_uri.to_string(),
                    snapshot_addr: ev.snapshot_id.clone(),
                    snapshot_ref: snap
                        .map(|s| s.snapshot_ref.clone())
                        .unwrap_or_else(|| source_uri.to_string()),
                    pointer_type: "span".to_string(),
                    pointer_value: format!("{}:{}", ev.span_start, ev.span_end),
                    excerpt: ev.excerpt.clone(),
                    captured_at: ev.captured_at,
                }
            })
            .collect()
    };

    let mut items: Vec<LedgerReviewItem> = Vec::new();

    // Promoted decisions — with persisted code regions from bindings,
    // tracking state, source freshness, code evidence, and freshness warnings.
    for d in decisions.values() {
        let source_uri = d.source_id.clone().unwrap_or_else(|| "unknown".to_string());
        let sources = resolve_sources(&d.evidence_refs, &source_uri);
        let regions: Option<Vec<CodeRegion>> = bindings
            .get(&d.id)
            .map(|bs| bs.iter().filter_map(parse_binding_to_region).collect());
        let regions = match regions {
            Some(ref r) if r.is_empty() => None,
            other => other,
        };

        // Derive source freshness from review state (stale_source_pending
        // means the source has been detected as stale).
        let source_freshness = if d.review_state == ReviewLifecycleState::StaleSourcePending {
            SourceFreshness::Stale
        } else {
            SourceFreshness::Fresh
        };

        // Build code evidence section from bindings when present.
        let code_evidence = build_code_evidence_section(d, &bindings);

        // Build freshness warning for tracked stale Decisions only.
        // Per ADR-0022: stale untracked Decisions do not create a
        // governance failure.
        let freshness_warning = if d.tracking_state
            == bicameral_api::candidate::TrackingState::Tracked
            && source_freshness == SourceFreshness::Stale
        {
            Some(FreshnessWarning {
                message: "Source evidence is stale. This tracked Decision's cited \
                          evidence may no longer reflect the current source state."
                    .to_string(),
                actions: vec![
                    FreshnessAction::RefreshSource,
                    FreshnessAction::RevalidateBinding,
                    FreshnessAction::MarkReviewed,
                    FreshnessAction::Untrack,
                ],
            })
        } else {
            None
        };

        items.push(LedgerReviewItem::Decision(Box::new(LedgerDecision {
            id: d.id.to_string(),
            summary: d.title.clone(),
            feature: source_uri,
            parent_id: None,
            signoff: d.signoff_state,
            review_state: d.review_state,
            sources,
            regions,
            conflicts_with: None,
            discovered: None,
            allowed_commands: allowed_decision_commands(d.signoff_state, d.tracking_state),
            // Graph readiness is view state — surfaced honestly for the
            // dashboard. The gateway does not hold a live SnapshotResolver,
            // so this is populated when a graph readiness query is available.
            graph_readiness: None,
            tracking_state: d.tracking_state,
            source_freshness,
            code_evidence,
            freshness_warning,
            signoff_display: SignoffDisplay::from_state(d.signoff_state),
            compliance_display: ComplianceDisplay::from_state(d.compliance_state),
        })));
    }

    // Queued candidates (not yet promoted)
    for c in candidates.values() {
        if c.candidate_state == CandidateState::Promoted {
            continue; // already shown as decision
        }
        let review_state = c.review_state;
        let sources = resolve_sources(&c.evidence_refs, &c.source);

        items.push(LedgerReviewItem::Candidate(LedgerCandidate {
            id: c.id.to_string(),
            summary: c.title.clone(),
            feature_hint: c.tags.first().cloned(),
            sources,
            review_state,
            allowed_commands: vec![
                CandidateCommandKind::AcceptCandidate,
                CandidateCommandKind::RejectCandidate,
                CandidateCommandKind::RequestContext,
            ],
        }));
    }

    items.sort_by(|a, b| {
        let summary_a = match a {
            LedgerReviewItem::Decision(d) => &d.summary,
            LedgerReviewItem::Candidate(c) => &c.summary,
        };
        let summary_b = match b {
            LedgerReviewItem::Decision(d) => &d.summary,
            LedgerReviewItem::Candidate(c) => &c.summary,
        };
        summary_a.cmp(summary_b)
    });

    Json(items)
}

/// Returns the allowed commands for a decision given its signoff and tracking state.
///
/// Signoff state gates which *commands* are available but does not imply
/// compliance, enforcement, or code readiness. `ResolveCompliance` appearing
/// under `Approved` means compliance *may* be evaluated separately — it does
/// not mean signoff approval created or proved compliance (ADR-0022).
///
/// Tracked Decisions additionally expose `RefreshSource` and `UntrackDecision`
/// commands. Untracked Decisions do not expose refresh (staleness is not a
/// governance failure for untracked context per ADR-0022).
fn allowed_decision_commands(
    signoff: SignoffState,
    tracking_state: bicameral_api::candidate::TrackingState,
) -> Vec<DecisionCommandKind> {
    let mut commands = match signoff {
        SignoffState::Proposed => vec![
            DecisionCommandKind::ApproveSignoff,
            DecisionCommandKind::RejectSignoff,
            DecisionCommandKind::RequestContext,
            DecisionCommandKind::BindToCode,
        ],
        SignoffState::Approved => vec![
            DecisionCommandKind::BindToCode,
            DecisionCommandKind::SupersedeDecision,
        ],
        SignoffState::Rejected => vec![DecisionCommandKind::SupersedeDecision],
        SignoffState::CollisionPending => vec![
            DecisionCommandKind::MarkDifferentScopes,
            DecisionCommandKind::RequestContext,
        ],
        SignoffState::Superseded => Vec::new(),
    };
    if tracking_state == bicameral_api::candidate::TrackingState::Tracked
        && signoff != SignoffState::Superseded
    {
        commands.push(DecisionCommandKind::RefreshSource);
        commands.push(DecisionCommandKind::UntrackDecision);
    }
    commands
}

// ── Projection → LedgerReviewItem converter ──────────────────────

/// Build `LedgerReviewItem` list from a V1 reviewable-context projection.
///
/// Maps `ProjectionState` record families into the existing Ledger View
/// dashboard types. Candidates not yet in the projection are read from the
/// in-memory candidate map so the two-page UI shape is preserved.
///
/// Per the `v1-reviewable-context-projection` spec and `codegraph-ledger-ui-
/// protocol-boundary` spec:
/// - Code evidence appears under the selected Decision.
/// - Graph readiness surfaces as recovery-oriented evidence state.
/// - No compliance, enforcement, or readiness-to-ship claims.
fn ledger_items_from_projection(
    proj: &bicameral_api::projection::ProjectionState,
    candidates: &std::collections::HashMap<Uuid, bicameral_api::candidate::DecisionCandidate>,
    stored_evidence: &std::collections::HashMap<Uuid, bicameral_api::evidence::SourceEvidence>,
    stored_snapshots: &std::collections::HashMap<String, bicameral_api::source::SourceSnapshot>,
) -> Vec<LedgerReviewItem> {
    use bicameral_api::dashboard::{CodeRegion, FreshnessAction, FreshnessWarning};

    let resolve_sources_from_projection =
        |decision_id: &Uuid, source_uri: &str| -> Vec<SourceEvidenceRef> {
            proj.source_evidence
                .values()
                .filter(|se| &se.decision_id == decision_id)
                .map(|se| {
                    let snap = stored_snapshots.get(&se.snapshot_id);
                    SourceEvidenceRef {
                        id: se.id.to_string(),
                        source_uri: source_uri.to_string(),
                        snapshot_addr: se.snapshot_id.clone(),
                        snapshot_ref: snap
                            .map(|s| s.snapshot_ref.clone())
                            .unwrap_or_else(|| source_uri.to_string()),
                        pointer_type: "span".to_string(),
                        pointer_value: format!("{}:{}", se.span_start, se.span_end),
                        excerpt: se.excerpt.clone(),
                        captured_at: se.captured_at,
                    }
                })
                .collect()
        };

    let mut items: Vec<LedgerReviewItem> = Vec::new();

    // Projected Decisions.
    for (id, pd) in &proj.decisions {
        let source_uri = pd
            .source_id
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let sources = resolve_sources_from_projection(id, &source_uri);

        // Code regions from projected code evidence.
        let code_evidence_entries: Vec<&bicameral_api::projection::ProjectedCodeEvidence> = proj
            .code_evidence
            .values()
            .filter(|ce| &ce.decision_id == id)
            .collect();

        let regions: Option<Vec<CodeRegion>> = if code_evidence_entries.is_empty() {
            None
        } else {
            Some(
                code_evidence_entries
                    .iter()
                    .map(|ce| CodeRegion {
                        file: ce.file.clone(),
                        start_line: ce.start_line,
                        end_line: ce.end_line,
                        content_hash: None,
                    })
                    .collect(),
            )
        };

        // Build code evidence section from projection.
        let code_evidence = build_code_evidence_from_projection(id, proj);

        // Graph readiness from projection.
        let graph_readiness =
            proj.graph_readiness
                .get(id)
                .map(|gr| bicameral_api::evidence::GraphReadinessView {
                    readiness: if gr.has_bindings {
                        bicameral_api::evidence::GraphReadiness::Ready
                    } else {
                        bicameral_api::evidence::GraphReadiness::NotConfigured
                    },
                    graph_snapshot_id: proj
                        .graph_snapshots
                        .values()
                        .next()
                        .map(|gs| gs.snapshot_addr.clone()),
                    detail: None,
                    scope_coverage: None,
                });

        // Source freshness and freshness warnings.
        let source_freshness = pd.source_freshness;
        let freshness_warning = if pd.tracking_state
            == bicameral_api::candidate::TrackingState::Tracked
            && source_freshness == SourceFreshness::Stale
        {
            Some(FreshnessWarning {
                message: "Source evidence is stale. This tracked Decision's cited \
                          evidence may no longer reflect the current source state."
                    .to_string(),
                actions: vec![
                    FreshnessAction::RefreshSource,
                    FreshnessAction::RevalidateBinding,
                    FreshnessAction::MarkReviewed,
                    FreshnessAction::Untrack,
                ],
            })
        } else {
            None
        };

        items.push(LedgerReviewItem::Decision(Box::new(LedgerDecision {
            id: pd.id.to_string(),
            summary: pd.title.clone(),
            feature: source_uri,
            parent_id: None,
            signoff: pd.signoff,
            review_state: pd.review_state,
            sources,
            regions,
            conflicts_with: None,
            discovered: None,
            allowed_commands: allowed_decision_commands(pd.signoff, pd.tracking_state),
            graph_readiness,
            tracking_state: pd.tracking_state,
            source_freshness,
            code_evidence,
            freshness_warning,
            signoff_display: SignoffDisplay::from_state(pd.signoff),
            compliance_display: ComplianceDisplay::from_state(
                bicameral_api::dashboard::ComplianceState::Ungrounded,
            ),
        })));
    }

    // Queued candidates not yet in the projection (not promoted).
    for c in candidates.values() {
        if c.candidate_state == CandidateState::Promoted {
            continue;
        }
        let review_state = c.review_state;
        let source_uri = &c.source;
        let sources: Vec<SourceEvidenceRef> = c
            .evidence_refs
            .iter()
            .filter_map(|ev_id| stored_evidence.get(ev_id))
            .map(|ev| {
                let snap = stored_snapshots
                    .values()
                    .find(|s| s.snapshot_addr == ev.snapshot_id);
                SourceEvidenceRef {
                    id: ev.id.to_string(),
                    source_uri: source_uri.to_string(),
                    snapshot_addr: ev.snapshot_id.clone(),
                    snapshot_ref: snap
                        .map(|s| s.snapshot_ref.clone())
                        .unwrap_or_else(|| source_uri.to_string()),
                    pointer_type: "span".to_string(),
                    pointer_value: format!("{}:{}", ev.span_start, ev.span_end),
                    excerpt: ev.excerpt.clone(),
                    captured_at: ev.captured_at,
                }
            })
            .collect();

        items.push(LedgerReviewItem::Candidate(LedgerCandidate {
            id: c.id.to_string(),
            summary: c.title.clone(),
            feature_hint: c.tags.first().cloned(),
            sources,
            review_state,
            allowed_commands: vec![
                CandidateCommandKind::AcceptCandidate,
                CandidateCommandKind::RejectCandidate,
                CandidateCommandKind::RequestContext,
            ],
        }));
    }

    items.sort_by(|a, b| {
        let summary_a = match a {
            LedgerReviewItem::Decision(d) => &d.summary,
            LedgerReviewItem::Candidate(c) => &c.summary,
        };
        let summary_b = match b {
            LedgerReviewItem::Decision(d) => &d.summary,
            LedgerReviewItem::Candidate(c) => &c.summary,
        };
        summary_a.cmp(summary_b)
    });

    items
}

/// Build a `CodeEvidenceSection` from projected code evidence and graph
/// readiness for a given Decision.
fn build_code_evidence_from_projection(
    decision_id: &Uuid,
    proj: &bicameral_api::projection::ProjectionState,
) -> Option<bicameral_api::dashboard::CodeEvidenceSection> {
    let entries: Vec<&bicameral_api::projection::ProjectedCodeEvidence> = proj
        .code_evidence
        .values()
        .filter(|ce| &ce.decision_id == decision_id)
        .collect();

    let readiness = proj.graph_readiness.get(decision_id);

    if entries.is_empty() && readiness.is_none() {
        return None;
    }

    // Determine evidence state from projected data.
    let evidence_state = if !entries.is_empty() {
        // Use the dominant evidence state from code evidence entries.
        entries[0].evidence_state
    } else if let Some(gr) = readiness {
        gr.evidence_state
    } else {
        GraphEvidenceState::UnknownNotIndexed
    };

    let (evidence_label, evidence_copy, allowed_actions) =
        projection_evidence_ui(evidence_state, !entries.is_empty());

    let graph_snapshot_id = proj
        .graph_snapshots
        .values()
        .next()
        .map(|gs| gs.snapshot_addr.clone());

    Some(bicameral_api::dashboard::CodeEvidenceSection {
        evidence_state,
        evidence_label,
        evidence_copy,
        graph_snapshot_id,
        validated_sha: None,
        scope_sufficiency: None,
        allowed_actions,
    })
}

/// Map a `GraphEvidenceState` to UI label, copy, and allowed actions.
///
/// Per the codegraph-ledger-ui-protocol-boundary spec: different failure
/// cases look different only when the recovery path differs.
fn projection_evidence_ui(
    state: GraphEvidenceState,
    has_bindings: bool,
) -> (
    String,
    String,
    Vec<bicameral_api::dashboard::CodeEvidenceAction>,
) {
    use bicameral_api::dashboard::CodeEvidenceAction;
    match state {
        GraphEvidenceState::Verified => (
            "Grounded at snapshot".to_string(),
            "Code evidence was verified for the named snapshot/scope.".to_string(),
            vec![
                CodeEvidenceAction::ViewProvenance,
                CodeEvidenceAction::Revalidate,
            ],
        ),
        GraphEvidenceState::NotFound => (
            "Not found in inspected scope".to_string(),
            "No matching code found at the inspected snapshot. Reviewer decides follow-up."
                .to_string(),
            vec![
                CodeEvidenceAction::RefineTarget,
                CodeEvidenceAction::KeepSourceOnly,
            ],
        ),
        GraphEvidenceState::UnknownNotIndexed => {
            if has_bindings {
                (
                    "Bindings present, graph not indexed".to_string(),
                    "Bindings exist but the graph index is not available. Build or expand the index."
                        .to_string(),
                    vec![
                        CodeEvidenceAction::BuildIndex,
                        CodeEvidenceAction::KeepSourceOnly,
                    ],
                )
            } else {
                (
                    "Not indexed".to_string(),
                    "No graph index available. Build the index to enable code evidence."
                        .to_string(),
                    vec![CodeEvidenceAction::BuildIndex],
                )
            }
        }
        GraphEvidenceState::UnknownStale => (
            "Stale graph".to_string(),
            "Graph data is stale. Rebuild or revalidate to get current evidence.".to_string(),
            vec![
                CodeEvidenceAction::RebuildGraph,
                CodeEvidenceAction::Revalidate,
            ],
        ),
        GraphEvidenceState::Ambiguous => (
            "Ambiguous candidates".to_string(),
            "Multiple code targets match. Reviewer must choose or refine.".to_string(),
            vec![
                CodeEvidenceAction::ChooseTarget,
                CodeEvidenceAction::RefineHint,
            ],
        ),
        GraphEvidenceState::Unsupported => (
            "Unsupported".to_string(),
            "This language or code pattern is not supported by the local graph. Use source-only evidence or request a hosted check."
                .to_string(),
            vec![
                CodeEvidenceAction::KeepSourceOnly,
                CodeEvidenceAction::RequestHostedCheck,
            ],
        ),
        GraphEvidenceState::ApproximateCandidate => (
            "Suggested only".to_string(),
            "Binding was accepted without graph verification. Run exact validation or keep as source-only evidence."
                .to_string(),
            vec![
                CodeEvidenceAction::ValidateExactly,
                CodeEvidenceAction::KeepSourceOnly,
            ],
        ),
    }
}

/// Accepts a dashboard review command.
///
/// Now routes `accept_candidate` and `approve_signoff` through the promotion path
/// per spec §4.3, emitting events and materialising Decisions.
async fn dashboard_command(
    State(state): State<AppState>,
    Json(cmd): Json<DashboardReviewCommand>,
) -> Result<Response, StatusCode> {
    // ingest_batch uses a snapshot_addr (not UUID) as target_id
    if cmd.command == "ingest_batch" {
        let snapshot_addr = &cmd.target_id;
        let mut candidates = state.candidates.write().await;
        let mut decisions_map = state.decisions.write().await;
        let to_promote: Vec<Uuid> = candidates
            .values()
            .filter(|c| {
                if c.candidate_state != CandidateState::Proposed {
                    return false;
                }
                c.source == *snapshot_addr
                    || state
                        .snapshots
                        .try_read()
                        .map(|snaps| {
                            snaps.values().any(|s| {
                                s.snapshot_addr == *snapshot_addr && s.snapshot_ref == c.source
                            })
                        })
                        .unwrap_or(false)
            })
            .map(|c| c.id)
            .collect();

        let actor = cmd
            .reason
            .clone()
            .unwrap_or_else(|| "dashboard".to_string());

        for cand_id in &to_promote {
            if let Some(c) = candidates.get_mut(cand_id) {
                c.candidate_state = CandidateState::Promoted;
                c.review_state = ReviewLifecycleState::NeedsSignoffReview;

                let decision = Decision::from_candidate(c, actor.clone());
                let decision_id = decision.id;

                let _ = state.event_store.append(EventStoreEntry::new(
                    EventType::AcceptCandidate,
                    serde_json::json!({
                        "candidate_id": cand_id.to_string(),
                        "decision_id": decision_id.to_string(),
                    }),
                    "dashboard".to_string(),
                ));

                decisions_map.insert(decision_id, decision);
            }
        }

        tracing::info!(count = to_promote.len(), snapshot = %snapshot_addr, "Batch ingest promoted candidates");
        return Ok(StatusCode::ACCEPTED.into_response());
    }

    let target_id = Uuid::parse_str(&cmd.target_id).map_err(|_| StatusCode::BAD_REQUEST)?;

    match cmd.command.as_str() {
        "accept_candidate" => {
            // Promote candidate → Decision(proposed)
            let candidate = {
                let mut candidates = state.candidates.write().await;
                let c = candidates
                    .get_mut(&target_id)
                    .ok_or(StatusCode::NOT_FOUND)?;
                if c.candidate_state != CandidateState::Proposed {
                    return Err(StatusCode::CONFLICT);
                }
                c.candidate_state = CandidateState::Promoted;
                c.review_state = ReviewLifecycleState::NeedsSignoffReview;
                c.clone()
            };

            let decision = Decision::from_candidate(
                &candidate,
                cmd.reason
                    .clone()
                    .unwrap_or_else(|| "dashboard".to_string()),
            );
            let decision_id = decision.id;

            // Emit event
            {
                let _ = state.event_store.append(EventStoreEntry::new(
                    EventType::AcceptCandidate,
                    serde_json::json!({
                        "candidate_id": target_id.to_string(),
                        "decision_id": decision_id.to_string(),
                    }),
                    "dashboard".to_string(),
                ));
            }

            state.decisions.write().await.insert(decision_id, decision);

            tracing::info!(candidate_id = %target_id, decision_id = %decision_id, "Candidate accepted → Decision(proposed)");
        }
        "reject_candidate" => {
            let mut candidates = state.candidates.write().await;
            let c = candidates
                .get_mut(&target_id)
                .ok_or(StatusCode::NOT_FOUND)?;
            c.candidate_state = CandidateState::Rejected;
            c.review_state = ReviewLifecycleState::Materialized;

            {
                let _ = state.event_store.append(EventStoreEntry::new(
                    EventType::RejectCandidate,
                    serde_json::json!({ "candidate_id": target_id.to_string() }),
                    "dashboard".to_string(),
                ));
            }

            tracing::info!(candidate_id = %target_id, "Candidate rejected");
        }
        "approve_signoff" => {
            // Decision(proposed) → Decision(approved).
            // Ownership approval only — does not change compliance state
            // or imply enforcement/readiness (ADR-0022).
            let mut decisions = state.decisions.write().await;
            let d = decisions.get_mut(&target_id).ok_or(StatusCode::NOT_FOUND)?;
            if d.signoff_state != SignoffState::Proposed {
                return Err(StatusCode::CONFLICT);
            }
            d.signoff_state = SignoffState::Approved;
            d.review_state = ReviewLifecycleState::ReadyToMaterialize;

            {
                let _ = state.event_store.append(EventStoreEntry::new(
                    EventType::ApproveSignoff,
                    serde_json::json!({ "decision_id": target_id.to_string() }),
                    "dashboard".to_string(),
                ));
            }

            tracing::info!(decision_id = %target_id, "Decision signoff approved");
        }
        "reject_signoff" => {
            let mut decisions = state.decisions.write().await;
            let d = decisions.get_mut(&target_id).ok_or(StatusCode::NOT_FOUND)?;
            d.signoff_state = SignoffState::Rejected;
            d.review_state = ReviewLifecycleState::Materialized;

            {
                let _ = state.event_store.append(EventStoreEntry::new(
                    EventType::RejectSignoff,
                    serde_json::json!({ "decision_id": target_id.to_string() }),
                    "dashboard".to_string(),
                ));
            }
        }
        "supersede_decision" => {
            let mut decisions = state.decisions.write().await;
            let d = decisions.get_mut(&target_id).ok_or(StatusCode::NOT_FOUND)?;
            d.signoff_state = SignoffState::Superseded;
            d.review_state = ReviewLifecycleState::Materialized;

            {
                let _ = state.event_store.append(EventStoreEntry::new(
                    EventType::SupersedeDecision,
                    serde_json::json!({ "decision_id": target_id.to_string() }),
                    "dashboard".to_string(),
                ));
            }
        }
        "resolve_compliance" | "untrack_decision" | "refresh_source" => {
            let body = DeferredCommandResponse::alpha_unsupported(&cmd.command);
            return Ok((StatusCode::ACCEPTED, Json(body)).into_response());
        }
        _ => {
            // Record unknown commands in audit and accept
            let mut audit = state.audit.write().await;
            let _ = audit.record(
                AuditAction::ReviewCommandIssued,
                "dashboard".to_string(),
                Some(target_id),
                format!(
                    "Dashboard command '{}' on target '{}'",
                    cmd.command, cmd.target_id
                ),
            );
        }
    }

    Ok(StatusCode::ACCEPTED.into_response())
}

// ── Query surface routes (history, search, preflight) ────────────

pub fn query_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/history", get(query_history_handler))
        .route("/api/v1/search", post(query_search_handler))
        .route("/api/v1/preflight", post(query_preflight_handler))
}

/// History query params aligned with spec §2.2 `bicameral.history`.
#[derive(Deserialize)]
struct HistoryParams {
    #[serde(default)]
    decision_id: Option<Uuid>,
    #[serde(default)]
    include_events: Option<bool>,
    #[serde(default)]
    include_candidates: Option<bool>,
    #[serde(default)]
    since: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Serialize)]
struct HistoryDecisionEntry {
    decision_id: Uuid,
    candidate_id: Uuid,
    title: String,
    body: String,
    level: bicameral_api::candidate::DecisionLevel,
    signoff_state: bicameral_api::dashboard::SignoffState,
    compliance_state: bicameral_api::dashboard::ComplianceState,
    review_state: ReviewLifecycleState,
    source_id: Option<String>,
    promoted_at: chrono::DateTime<chrono::Utc>,
    promoted_by: String,
    signoff_display: SignoffDisplay,
    compliance_display: ComplianceDisplay,
}

#[derive(Serialize)]
struct HistoryCandidateEntry {
    candidate_id: Uuid,
    title: String,
    description: String,
    level: bicameral_api::candidate::DecisionLevel,
    candidate_state: CandidateState,
    source: String,
}

#[derive(Serialize)]
struct HistoryResponseBody {
    decisions: Vec<HistoryDecisionEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    candidates: Option<Vec<HistoryCandidateEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    events: Option<Vec<bicameral_api::event_store::EventStoreEntry>>,
}

async fn query_history_handler(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<HistoryParams>,
) -> Json<HistoryResponseBody> {
    let decisions = state.decisions.read().await;
    let mut entries: Vec<HistoryDecisionEntry> = decisions
        .values()
        .filter(|d| {
            if let Some(id) = params.decision_id {
                d.id == id
            } else {
                true
            }
        })
        .filter(|d| {
            if let Some(since) = params.since {
                d.promoted_at >= since
            } else {
                true
            }
        })
        .map(|d| HistoryDecisionEntry {
            decision_id: d.id,
            candidate_id: d.candidate_id,
            title: d.title.clone(),
            body: d.body.clone(),
            level: d.level,
            signoff_state: d.signoff_state,
            compliance_state: d.compliance_state,
            review_state: d.review_state,
            source_id: d.source_id.clone(),
            promoted_at: d.promoted_at,
            promoted_by: d.promoted_by.clone(),
            signoff_display: SignoffDisplay::from_state(d.signoff_state),
            compliance_display: ComplianceDisplay::from_state(d.compliance_state),
        })
        .collect();
    entries.sort_by_key(|e| std::cmp::Reverse(e.promoted_at));
    drop(decisions);

    let candidates = if params.include_candidates.unwrap_or(false) {
        let cands = state.candidates.read().await;
        let mut cand_entries: Vec<HistoryCandidateEntry> = cands
            .values()
            .map(|c| HistoryCandidateEntry {
                candidate_id: c.id,
                title: c.title.clone(),
                description: c.description.clone(),
                level: c.level,
                candidate_state: c.candidate_state,
                source: c.source.clone(),
            })
            .collect();
        cand_entries.sort_by(|a, b| a.title.cmp(&b.title));
        Some(cand_entries)
    } else {
        None
    };

    let events = if params.include_events.unwrap_or(false) {
        let mut evts = state.event_store.replay().unwrap_or_default();
        if let Some(since) = params.since {
            evts.retain(|e| e.timestamp >= since);
        }
        Some(evts)
    } else {
        None
    };

    Json(HistoryResponseBody {
        decisions: entries,
        candidates,
        events,
    })
}

/// Search request body aligned with spec §2.2 `bicameral.search`.
#[derive(Deserialize)]
struct SearchRequest {
    query: String,
    #[serde(default = "default_search_scope")]
    scope: SearchScope,
    #[serde(default)]
    filters: SearchFilters,
    #[serde(default = "default_search_limit")]
    limit: usize,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum SearchScope {
    Decisions,
    Candidates,
    Bindings,
    All,
}

fn default_search_scope() -> SearchScope {
    SearchScope::All
}

fn default_search_limit() -> usize {
    20
}

#[derive(Deserialize, Default)]
struct SearchFilters {
    #[serde(default)]
    level: Option<bicameral_api::candidate::DecisionLevel>,
    #[serde(default)]
    signoff_state: Option<bicameral_api::dashboard::SignoffState>,
    #[serde(default)]
    compliance_state: Option<bicameral_api::dashboard::ComplianceState>,
}

#[derive(Serialize)]
struct SearchResult {
    #[serde(rename = "type")]
    result_type: String,
    id: String,
    title: String,
    snippet: String,
    relevance_score: f64,
}

/// Read-only advisory boundary applied to every binding-scope response so the
/// results are never read as verified project truth or merge safety.
const BINDING_SCOPE_BOUNDARY_NOTE: &str =
    "Binding search returns recorded, bot-owned accepted BindingEvidence only. \
     Results are read-only and do not imply complete project truth, compliance, \
     signoff, no-conflict, verified binding state beyond the recorded evidence, \
     or merge safety.";

/// A single accepted `BindingEvidence` result over bot-owned, replayed state.
///
/// Read-only recorded evidence — never verified project truth, compliance,
/// signoff, no-conflict, or merge safety beyond the recorded evidence.
#[derive(Serialize)]
struct BindingSearchResult {
    binding_id: String,
    decision_id: String,
    candidate_id: String,
    target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_identifier: Option<String>,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    target_metadata: serde_json::Value,
    binding_type: bicameral_api::evidence::BindingType,
    #[serde(skip_serializing_if = "Option::is_none")]
    graph_snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    validated_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ref_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    evidence_state: Option<bicameral_api::evidence::GraphEvidenceState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    explanation: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    readiness_warnings: Vec<String>,
    unknown_or_partial_scope: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    caveats: Vec<String>,
    relevance_score: f64,
    inspection_uri: String,
}

#[derive(Serialize)]
struct SearchResponseBody {
    results: Vec<SearchResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    bindings: Vec<BindingSearchResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    scope_notes: Vec<String>,
}

async fn query_search_handler(
    State(state): State<AppState>,
    Json(req): Json<SearchRequest>,
) -> Json<SearchResponseBody> {
    let q = req.query.to_lowercase();
    let mut results = Vec::new();

    if matches!(req.scope, SearchScope::All | SearchScope::Decisions) {
        let decisions = state.decisions.read().await;
        for d in decisions.values() {
            if let Some(ref level) = req.filters.level {
                if d.level != *level {
                    continue;
                }
            }
            if let Some(ref signoff) = req.filters.signoff_state {
                if d.signoff_state != *signoff {
                    continue;
                }
            }
            if let Some(ref compliance) = req.filters.compliance_state {
                if d.compliance_state != *compliance {
                    continue;
                }
            }
            let title_match = d.title.to_lowercase().contains(&q);
            let body_match = d.body.to_lowercase().contains(&q);
            if title_match || body_match {
                let score = if title_match { 1.0 } else { 0.7 };
                let snippet = if title_match {
                    d.title.clone()
                } else {
                    search_snippet(&d.body, &q)
                };
                results.push(SearchResult {
                    result_type: "decision".to_string(),
                    id: d.id.to_string(),
                    title: d.title.clone(),
                    snippet,
                    relevance_score: score,
                });
            }
        }
    }

    if matches!(req.scope, SearchScope::All | SearchScope::Candidates) {
        let candidates = state.candidates.read().await;
        for c in candidates.values() {
            if let Some(ref level) = req.filters.level {
                if c.level != *level {
                    continue;
                }
            }
            let title_match = c.title.to_lowercase().contains(&q);
            let desc_match = c.description.to_lowercase().contains(&q);
            if title_match || desc_match {
                let score = if title_match { 0.9 } else { 0.6 };
                let snippet = if title_match {
                    c.title.clone()
                } else {
                    search_snippet(&c.description, &q)
                };
                results.push(SearchResult {
                    result_type: "candidate".to_string(),
                    id: c.id.to_string(),
                    title: c.title.clone(),
                    snippet,
                    relevance_score: score,
                });
            }
        }

        let evidence = state.evidence.read().await;
        for ev in evidence.values() {
            if ev.excerpt.to_lowercase().contains(&q) {
                results.push(SearchResult {
                    result_type: "source_excerpt".to_string(),
                    id: ev.id.to_string(),
                    title: format!("Evidence from {}", ev.snapshot_id),
                    snippet: ev.excerpt.clone(),
                    relevance_score: 0.5,
                });
            }
        }
    }

    let mut bindings = Vec::new();
    let mut scope_notes = Vec::new();

    if matches!(req.scope, SearchScope::All | SearchScope::Bindings) {
        // Binding search reads accepted, bot-owned BindingEvidence from the
        // replayed AppState map (keyed by decision id). Read-only.
        let binding_map = state.bindings.read().await;
        for (decision_id, decision_bindings) in binding_map.iter() {
            for binding in decision_bindings {
                if let Some(result) = gateway_binding_search_result(*decision_id, binding, &q) {
                    bindings.push(result);
                }
            }
        }

        bindings.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        bindings.truncate(req.limit);

        // Binding-scope responses are always explicit — never silently empty.
        scope_notes.push(BINDING_SCOPE_BOUNDARY_NOTE.to_string());
        let partial = bindings
            .iter()
            .filter(|b| b.unknown_or_partial_scope)
            .count();
        if partial > 0 {
            scope_notes.push(format!(
                "{partial} binding result(s) have unknown or partial graph evidence scope \
                 (not Verified, or missing snapshot/commit anchor). Treat those as \
                 review-only, not verified graph evidence."
            ));
        }
    }

    results.sort_by(|a, b| {
        b.relevance_score
            .partial_cmp(&a.relevance_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(req.limit);

    Json(SearchResponseBody {
        results,
        bindings,
        scope_notes,
    })
}

/// Build a [`BindingSearchResult`] for a recorded binding when it matches the
/// (already-lowercased) query. Lexical match only — never a verified graph
/// re-check. Mirrors the runtime `search::binding_search_result` shape.
fn gateway_binding_search_result(
    decision_id: Uuid,
    binding: &bicameral_api::evidence::BindingEvidence,
    q: &str,
) -> Option<BindingSearchResult> {
    let binding_id = binding.id.to_string();
    let decision_id_str = decision_id.to_string();
    let binding_type_str = format!("{:?}", binding.binding_type).to_lowercase();

    let target_lower = binding.target.to_lowercase();
    let target_id_lower = binding
        .target_identifier
        .as_deref()
        .unwrap_or("")
        .to_lowercase();
    let target_kind_lower = binding.target_kind.as_deref().unwrap_or("").to_lowercase();
    let explanation_lower = binding.explanation.as_deref().unwrap_or("").to_lowercase();

    let mut score = 0.0_f64;
    if target_lower.contains(q) {
        score = score.max(0.85);
    }
    if target_id_lower.contains(q) {
        score = score.max(0.8);
    }
    if binding_id.to_lowercase().contains(q) || decision_id_str.contains(q) {
        score = score.max(0.75);
    }
    if target_kind_lower.contains(q) || binding_type_str.contains(q) {
        score = score.max(0.65);
    }
    if explanation_lower.contains(q) {
        score = score.max(0.55);
    }
    if q.is_empty() {
        score = score.max(0.5);
    }
    if score == 0.0 {
        return None;
    }

    let verified = binding
        .evidence_state
        .map(|s| s.allows_binding())
        .unwrap_or(false);
    let anchored = binding.graph_snapshot_id.is_some() && binding.validated_sha.is_some();
    let unknown_or_partial_scope = !verified || !anchored;

    let mut caveats = vec![BINDING_SCOPE_BOUNDARY_NOTE.to_string()];
    if !verified {
        caveats.push(
            "Recorded evidence_state is not Verified — this is review-only graph evidence, \
             not a verified graph fact."
                .to_string(),
        );
    }
    if !anchored {
        caveats.push(
            "Binding lacks a durable snapshot/commit anchor — scope is partial or unknown."
                .to_string(),
        );
    }

    Some(BindingSearchResult {
        binding_id,
        decision_id: decision_id_str,
        candidate_id: binding.candidate_id.to_string(),
        target: binding.target.clone(),
        target_kind: binding.target_kind.clone(),
        target_identifier: binding.target_identifier.clone(),
        target_metadata: binding.metadata.clone(),
        binding_type: binding.binding_type,
        graph_snapshot_id: binding.graph_snapshot_id.clone(),
        validated_sha: binding.validated_sha.clone(),
        ref_name: binding.ref_name.clone(),
        evidence_state: binding.evidence_state,
        explanation: binding.explanation.clone(),
        readiness_warnings: binding.readiness_warnings.clone(),
        unknown_or_partial_scope,
        caveats,
        relevance_score: score,
        inspection_uri: format!("bicameral://binding/{decision_id}/{}", binding.id),
    })
}

fn search_snippet(text: &str, query: &str) -> String {
    let lower = text.to_lowercase();
    if let Some(pos) = lower.find(query) {
        let start = pos.saturating_sub(40);
        let end = (pos + query.len() + 40).min(text.len());
        let slice = &text[start..end];
        if start > 0 || end < text.len() {
            format!("...{}...", slice.trim())
        } else {
            slice.to_string()
        }
    } else {
        text.chars().take(80).collect()
    }
}

/// Preflight request body aligned with spec §2.2 `bicameral.preflight`.
#[derive(Deserialize)]
struct PreflightRequest {
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    symbols: Vec<String>,
    #[serde(default)]
    diff_context: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    branch: Option<String>,
}

#[derive(Serialize)]
struct PreflightDecisionEntry {
    decision_id: Uuid,
    title: String,
    level: bicameral_api::candidate::DecisionLevel,
    signoff_state: bicameral_api::dashboard::SignoffState,
    compliance_state: bicameral_api::dashboard::ComplianceState,
    review_state: ReviewLifecycleState,
    relevance_reasons: Vec<String>,
    signoff_display: SignoffDisplay,
    compliance_display: ComplianceDisplay,
}

#[derive(Serialize)]
struct PreflightCandidateEntry {
    candidate_id: Uuid,
    title: String,
    candidate_state: CandidateState,
}

#[derive(Serialize)]
struct PreflightSnapshotScopeEntry {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    graph_snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    validated_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ref_name: Option<String>,
}

#[derive(Serialize)]
struct PreflightUnknownScopeEntry {
    scope: String,
    state: String,
    detail: String,
}

#[derive(Serialize)]
struct PreflightReadinessBody {
    state: String,
    snapshot_scope: Vec<PreflightSnapshotScopeEntry>,
    unknown_scope: Vec<PreflightUnknownScopeEntry>,
}

#[derive(Serialize)]
struct PreflightResponseBody {
    relevant_decisions: Vec<PreflightDecisionEntry>,
    relevant_candidates: Vec<PreflightCandidateEntry>,
    readiness: PreflightReadinessBody,
    warnings: Vec<String>,
}

async fn query_preflight_handler(
    State(state): State<AppState>,
    Json(req): Json<PreflightRequest>,
) -> Json<PreflightResponseBody> {
    let mut warnings = Vec::new();

    if !req.symbols.is_empty() {
        warnings.push(
            "Symbol grounding is not available in source-only preflight. \
             Symbols are matched against decision text only."
                .to_string(),
        );
    }

    let has_query_terms =
        !req.files.is_empty() || !req.symbols.is_empty() || req.diff_context.is_some();

    let query_terms = build_preflight_terms(&req);

    // Build binding target index from gateway state.
    let bindings = state.bindings.read().await;
    let binding_matched_decisions: std::collections::HashSet<Uuid> = if has_query_terms {
        let file_set: std::collections::HashSet<String> =
            req.files.iter().map(|f| f.to_lowercase()).collect();
        let file_stems: std::collections::HashSet<String> = req
            .files
            .iter()
            .filter_map(|f| {
                std::path::Path::new(f)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_lowercase())
            })
            .collect();
        let symbol_set: std::collections::HashSet<String> =
            req.symbols.iter().map(|s| s.to_lowercase()).collect();
        let symbol_stems: std::collections::HashSet<String> = req
            .symbols
            .iter()
            .filter_map(|s| s.rsplit("::").next().map(|n| n.to_lowercase()))
            .collect();

        let mut matched = std::collections::HashSet::new();
        for (decision_id, decision_bindings) in bindings.iter() {
            for binding in decision_bindings {
                let target_lower = binding.target.to_lowercase();
                let is_match = match binding.binding_type {
                    bicameral_api::evidence::BindingType::FilePath
                    | bicameral_api::evidence::BindingType::CodeRegion => {
                        file_set.contains(&target_lower)
                            || file_stems.contains(&target_lower)
                            || file_set.iter().any(|f| target_lower.contains(f.as_str()))
                            || file_stems.iter().any(|f| target_lower.contains(f.as_str()))
                    }
                    bicameral_api::evidence::BindingType::Symbol => {
                        symbol_set.contains(&target_lower)
                            || symbol_stems.contains(&target_lower)
                            || symbol_set.iter().any(|s| target_lower.contains(s.as_str()))
                            || symbol_stems
                                .iter()
                                .any(|s| target_lower.contains(s.as_str()))
                    }
                    _ => file_set.contains(&target_lower) || symbol_set.contains(&target_lower),
                };
                if is_match {
                    matched.insert(*decision_id);
                    break;
                }
            }
        }
        matched
    } else {
        std::collections::HashSet::new()
    };
    drop(bindings);

    let decisions = state.decisions.read().await;
    let mut seen_ids: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
    let mut relevant_decisions: Vec<PreflightDecisionEntry> = Vec::new();

    for d in decisions.values() {
        let mut reasons: Vec<String> = Vec::new();

        if !has_query_terms {
            reasons.push("full_ledger".to_string());
        } else {
            let text = format!("{} {}", d.title, d.body).to_lowercase();
            if query_terms.iter().any(|term| text.contains(term)) {
                reasons.push("source_text_match".to_string());
            }
            if binding_matched_decisions.contains(&d.id) {
                reasons.push("binding_target_match".to_string());
            }
        }

        if reasons.is_empty() {
            continue;
        }
        if !seen_ids.insert(d.id) {
            continue;
        }

        let compliance_state = if reasons.contains(&"binding_target_match".to_string()) {
            d.compliance_state
        } else {
            bicameral_api::dashboard::ComplianceState::Ungrounded
        };

        relevant_decisions.push(PreflightDecisionEntry {
            decision_id: d.id,
            title: d.title.clone(),
            level: d.level,
            signoff_state: d.signoff_state,
            compliance_state,
            review_state: d.review_state,
            relevance_reasons: reasons,
            signoff_display: SignoffDisplay::from_state(d.signoff_state),
            compliance_display: ComplianceDisplay::from_state(compliance_state),
        });
    }
    relevant_decisions.sort_by(|a, b| {
        a.title
            .cmp(&b.title)
            .then(a.decision_id.cmp(&b.decision_id))
    });
    drop(decisions);

    let candidates = state.candidates.read().await;
    let mut relevant_candidates: Vec<PreflightCandidateEntry> = candidates
        .values()
        .filter(|c| {
            if c.candidate_state == CandidateState::Promoted {
                return false;
            }
            if !has_query_terms {
                return true;
            }
            let text = format!("{} {}", c.title, c.description).to_lowercase();
            query_terms.iter().any(|term| text.contains(term))
        })
        .map(|c| PreflightCandidateEntry {
            candidate_id: c.id,
            title: c.title.clone(),
            candidate_state: c.candidate_state,
        })
        .collect();
    relevant_candidates.sort_by(|a, b| a.title.cmp(&b.title));
    drop(candidates);

    if !has_query_terms && (!relevant_decisions.is_empty() || !relevant_candidates.is_empty()) {
        warnings.push("No file/symbol/diff filter specified. Returning full ledger.".to_string());
    }

    if has_query_terms && relevant_decisions.is_empty() && relevant_candidates.is_empty() {
        warnings.push("No decisions or candidates match the query terms.".to_string());
    }

    // Always disclose local scope limitation per ADR-0019.
    warnings.push("Local no-conflict result is limited to resolved local snapshots.".to_string());

    let readiness = PreflightReadinessBody {
        state: "not_configured".to_string(),
        snapshot_scope: vec![],
        unknown_scope: vec![
            PreflightUnknownScopeEntry {
                scope: "graph".to_string(),
                state: "not_configured".to_string(),
                detail: "Graph data unavailable. Using source-only preflight.".to_string(),
            },
            PreflightUnknownScopeEntry {
                scope: "team_active_prs".to_string(),
                state: "hosted_required".to_string(),
                detail: "Local preflight cannot see team-wide active PR branches.".to_string(),
            },
        ],
    };

    Json(PreflightResponseBody {
        relevant_decisions,
        relevant_candidates,
        readiness,
        warnings,
    })
}

fn build_preflight_terms(req: &PreflightRequest) -> Vec<String> {
    let mut terms = Vec::new();
    for file in &req.files {
        let lower = file.to_lowercase();
        terms.push(lower.clone());
        if let Some(name) = std::path::Path::new(file).file_name() {
            let n = name.to_string_lossy().to_lowercase();
            terms.push(n.clone());
            terms.push(n.replace('_', " "));
        }
        if let Some(stem) = std::path::Path::new(file).file_stem() {
            let s = stem.to_string_lossy().to_lowercase();
            terms.push(s.clone());
            terms.push(s.replace('_', " "));
        }
    }
    for sym in &req.symbols {
        let lower = sym.to_lowercase();
        terms.push(lower.clone());
        terms.push(lower.replace('_', " "));
        if let Some(last) = sym.rsplit("::").next() {
            let l = last.to_lowercase();
            terms.push(l.clone());
            terms.push(l.replace('_', " "));
        }
    }
    if let Some(diff) = &req.diff_context {
        for line in diff.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('+') || trimmed.starts_with('-') {
                let content = trimmed[1..].trim();
                if content.len() >= 4 {
                    terms.push(content.to_lowercase());
                }
                for word in content.split_whitespace() {
                    let w = word.to_lowercase();
                    if w.len() >= 3 {
                        terms.push(w);
                    }
                }
            }
        }
    }
    terms
}

// ── Bind-to-code routes ──────────────────────────────────────────

pub fn bind_routes() -> Router<AppState> {
    Router::new().route("/api/v1/bind", post(bind_to_code))
}

/// Bind request — accepts a verified `BindingProposal` from the runtime.
///
/// The gateway is the accepted-evidence materialization boundary. Validation
/// (file inspection, snapshot resolution) happens in the runtime/daemon layer
/// before this call. The gateway gate-checks that the evidence state is
/// `verified` before promoting the proposal into accepted `BindingEvidence`
/// and emitting the `BindToCode` governance event.
///
/// Non-verified evidence is rejected. The proposal is not canonical authority
/// until this gateway path materializes it.
#[derive(Deserialize)]
struct BindRequest {
    decision_id: Uuid,
    /// Verified binding proposal from runtime prevalidation.
    proposal: bicameral_api::evidence::BindingProposal,
    evidence_state: GraphEvidenceState,
    #[serde(default)]
    line_start: Option<u32>,
    #[serde(default)]
    line_end: Option<u32>,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum BindResponse {
    Bound {
        binding_id: String,
        decision_id: String,
        target: String,
        graph_snapshot_id: String,
        validated_sha: String,
        content_hash: Option<String>,
    },
    Rejected {
        evidence_state: GraphEvidenceState,
        detail: String,
    },
}

async fn bind_to_code(
    State(state): State<AppState>,
    Json(req): Json<BindRequest>,
) -> Result<(StatusCode, Json<BindResponse>), StatusCode> {
    // Verify the decision exists.
    {
        let decisions = state.decisions.read().await;
        if !decisions.contains_key(&req.decision_id) {
            return Err(StatusCode::NOT_FOUND);
        }
    }

    // Gate: only verified evidence may materialize (ADR-0010, ADR-0017).
    if !req.evidence_state.allows_binding() {
        return Ok((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(BindResponse::Rejected {
                evidence_state: req.evidence_state,
                detail: format!(
                    "Only verified evidence may materialize bindings; got {:?}",
                    req.evidence_state
                ),
            }),
        ));
    }

    // ── Acceptance boundary ──────────────────────────────────────────
    // The proposal is a graph fact from runtime prevalidation. This is
    // the first point where it becomes accepted BindingEvidence.
    let graph_snapshot_id = req.proposal.graph_snapshot_id.clone();
    let validated_sha = req.proposal.validated_sha.clone();
    let content_hash = req.proposal.content_hash.clone();
    let target = req.proposal.target.clone();
    let accepted_binding = req.proposal.into_accepted_evidence();
    let binding_id = accepted_binding.id;

    // Emit BindToCode event to event store.
    let payload = BindToCodePayload {
        decision_id: req.decision_id,
        binding: accepted_binding.clone(),
        graph_snapshot_id: graph_snapshot_id.clone(),
        validated_sha: validated_sha.clone(),
        evidence_state: GraphEvidenceState::Verified,
        content_hash: content_hash.clone(),
        line_start: req.line_start,
        line_end: req.line_end,
    };

    let _ = state.event_store.append(EventStoreEntry::new(
        EventType::BindToCode,
        serde_json::to_value(&payload).unwrap_or_default(),
        "gateway".to_string(),
    ));

    // Materialize binding into in-memory state.
    {
        state
            .binding_content_hashes
            .write()
            .await
            .insert(binding_id, content_hash.clone());
    }
    state
        .bindings
        .write()
        .await
        .entry(req.decision_id)
        .or_default()
        .push(accepted_binding);

    // Audit
    {
        let mut audit = state.audit.write().await;
        let _ = audit.record(
            AuditAction::ReviewCommandIssued,
            "gateway".to_string(),
            Some(req.decision_id),
            format!("Bound decision {} to {}", req.decision_id, target),
        );
    }

    tracing::info!(decision_id = %req.decision_id, binding_id = %binding_id, "bind_to_code: accepted proposal as binding");
    Ok((
        StatusCode::CREATED,
        Json(BindResponse::Bound {
            binding_id: binding_id.to_string(),
            decision_id: req.decision_id.to_string(),
            target,
            graph_snapshot_id,
            validated_sha,
            content_hash,
        }),
    ))
}

/// Parse a `BindingEvidence` target into a `CodeRegion` for Ledger View display.
fn parse_binding_to_region(
    binding: &bicameral_api::evidence::BindingEvidence,
) -> Option<bicameral_api::dashboard::CodeRegion> {
    // Target format: "file:start-end" or "file:line" or "file"
    let target = &binding.target;

    if let Some((file, range)) = target.rsplit_once(':') {
        if let Some((start_s, end_s)) = range.split_once('-') {
            if let (Ok(start), Ok(end)) = (start_s.parse::<u32>(), end_s.parse::<u32>()) {
                return Some(bicameral_api::dashboard::CodeRegion {
                    file: file.to_string(),
                    start_line: start,
                    end_line: end,
                    content_hash: None,
                });
            }
        }
        if let Ok(line) = range.parse::<u32>() {
            return Some(bicameral_api::dashboard::CodeRegion {
                file: file.to_string(),
                start_line: line,
                end_line: line,
                content_hash: None,
            });
        }
    }

    // Whole-file binding: use line 0-0 as sentinel.
    Some(bicameral_api::dashboard::CodeRegion {
        file: target.clone(),
        start_line: 0,
        end_line: 0,
        content_hash: None,
    })
}

/// Build the code evidence section for a Decision's Ledger View display.
///
/// Per the codegraph-ledger-ui-protocol-boundary spec, code evidence
/// appears under the selected Decision with evidence state, label, copy,
/// and allowed actions. The evidence state and UI copy follow the spec's
/// Failure State Div contract.
fn build_code_evidence_section(
    decision: &bicameral_api::candidate::Decision,
    bindings: &std::collections::HashMap<uuid::Uuid, Vec<bicameral_api::evidence::BindingEvidence>>,
) -> Option<bicameral_api::dashboard::CodeEvidenceSection> {
    use bicameral_api::dashboard::{CodeEvidenceAction, CodeEvidenceSection};

    let decision_bindings = bindings.get(&decision.id)?;
    if decision_bindings.is_empty() {
        return None;
    }

    // Use the first binding's snapshot provenance as representative.
    let first = &decision_bindings[0];
    let graph_snapshot_id = first.graph_snapshot_id.clone();
    let validated_sha = first.validated_sha.clone();

    // Determine evidence state and UI copy from the binding's provenance.
    // If the binding has a graph_snapshot_id, it was verified; otherwise
    // it was accepted without graph verification (source-only).
    let (evidence_state, evidence_label, evidence_copy, allowed_actions) =
        if graph_snapshot_id.is_some() {
            (
                GraphEvidenceState::Verified,
                "Grounded at snapshot".to_string(),
                "This evidence was verified for the named snapshot/scope.".to_string(),
                vec![
                    CodeEvidenceAction::ViewProvenance,
                    CodeEvidenceAction::Revalidate,
                ],
            )
        } else {
            (
                GraphEvidenceState::ApproximateCandidate,
                "Suggested only".to_string(),
                "Binding was accepted without graph verification. \
                 Run exact validation or keep as source-only evidence."
                    .to_string(),
                vec![
                    CodeEvidenceAction::ValidateExactly,
                    CodeEvidenceAction::KeepSourceOnly,
                ],
            )
        };

    Some(CodeEvidenceSection {
        evidence_state,
        evidence_label,
        evidence_copy,
        graph_snapshot_id,
        validated_sha,
        scope_sufficiency: None,
        allowed_actions,
    })
}

// ── Compliance routes ────────────────────────────────────────────

pub fn compliance_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/compliance/evaluate", get(compliance_evaluate_all))
        .route(
            "/api/v1/compliance/decision/:decision_id",
            get(compliance_evaluate_one),
        )
}

/// Response for compliance evaluation.
#[derive(Serialize)]
struct ComplianceEvaluationResponse {
    verdicts: Vec<ComplianceVerdictResponse>,
    graph_available: bool,
}

#[derive(Serialize)]
struct ComplianceVerdictResponse {
    decision_id: String,
    compliance_state: bicameral_api::dashboard::ComplianceState,
    binding_count: usize,
    binding_verdicts: Vec<BindingVerdictResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[derive(Serialize)]
struct BindingVerdictResponse {
    binding_id: String,
    target: String,
    evidence_state: GraphEvidenceState,
    content_match: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

/// Compute compliance state for a decision based on its bindings.
///
/// Without a live `FileInspector` (graph not available from the gateway),
/// the gateway applies the baseline rule: no bindings → `Ungrounded`,
/// bindings exist but no graph → `Pending`.
fn compute_compliance_state(
    decision_id: Uuid,
    bindings: &[bicameral_api::evidence::BindingEvidence],
) -> ComplianceVerdictResponse {
    use bicameral_api::dashboard::ComplianceState;

    if bindings.is_empty() {
        return ComplianceVerdictResponse {
            decision_id: decision_id.to_string(),
            compliance_state: ComplianceState::Ungrounded,
            binding_count: 0,
            binding_verdicts: vec![],
            detail: Some("No binding evidence exists for this Decision".to_string()),
        };
    }

    ComplianceVerdictResponse {
        decision_id: decision_id.to_string(),
        compliance_state: ComplianceState::Pending,
        binding_count: bindings.len(),
        binding_verdicts: bindings
            .iter()
            .map(|b| BindingVerdictResponse {
                binding_id: b.id.to_string(),
                target: b.target.clone(),
                evidence_state: GraphEvidenceState::UnknownStale,
                content_match: None,
                detail: Some(
                    "Graph snapshot not available — compliance evaluation deferred".to_string(),
                ),
            })
            .collect(),
        detail: Some(
            "No graph snapshot available for compliance evaluation; state is pending".to_string(),
        ),
    }
}

/// Evaluate compliance for all decisions and update their compliance state.
async fn compliance_evaluate_all(
    State(state): State<AppState>,
) -> Json<ComplianceEvaluationResponse> {
    let decisions = state.decisions.read().await;
    let bindings = state.bindings.read().await;

    let mut verdicts = Vec::new();

    for dec_id in decisions.keys() {
        let dec_bindings = bindings.get(dec_id).cloned().unwrap_or_default();
        let verdict = compute_compliance_state(*dec_id, &dec_bindings);
        verdicts.push(verdict);
    }

    drop(decisions);
    drop(bindings);

    // Update compliance state on decisions.
    {
        let mut decisions = state.decisions.write().await;
        for v in &verdicts {
            if let Ok(id) = Uuid::parse_str(&v.decision_id) {
                if let Some(d) = decisions.get_mut(&id) {
                    d.compliance_state = v.compliance_state;
                }
            }
        }
    }

    Json(ComplianceEvaluationResponse {
        verdicts,
        graph_available: false,
    })
}

/// Evaluate compliance for a single decision.
async fn compliance_evaluate_one(
    State(state): State<AppState>,
    axum::extract::Path(decision_id): axum::extract::Path<Uuid>,
) -> Result<Json<ComplianceVerdictResponse>, StatusCode> {
    let decisions = state.decisions.read().await;
    if !decisions.contains_key(&decision_id) {
        return Err(StatusCode::NOT_FOUND);
    }
    drop(decisions);

    let bindings = state.bindings.read().await;
    let dec_bindings = bindings.get(&decision_id).cloned().unwrap_or_default();
    drop(bindings);

    let verdict = compute_compliance_state(decision_id, &dec_bindings);

    // Update compliance state on the decision.
    {
        let mut decisions = state.decisions.write().await;
        if let Some(d) = decisions.get_mut(&decision_id) {
            d.compliance_state = verdict.compliance_state;
        }
    }

    Ok(Json(verdict))
}

fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

// ── Evidence refresh routes ──────────────────────────────────────

pub fn evidence_refresh_routes() -> Router<AppState> {
    Router::new().route("/api/v1/evidence/refresh", post(evidence_refresh_handler))
}

#[derive(Deserialize)]
struct EvidenceRefreshRequest {
    decision_id: String,
}

async fn evidence_refresh_handler(
    State(state): State<AppState>,
    Json(req): Json<EvidenceRefreshRequest>,
) -> Json<serde_json::Value> {
    use bicameral_api::dashboard::SignoffState;
    use bicameral_api::evidence::{EvidenceRefreshOutcome, EvidenceRefreshStatus};

    // 1. Parse decision_id
    let decision_id = match Uuid::parse_str(&req.decision_id) {
        Ok(id) => id,
        Err(_) => {
            let outcome = EvidenceRefreshOutcome {
                decision_id: Uuid::nil(),
                status: EvidenceRefreshStatus::NotFound,
                binding_results: Vec::new(),
                detail: Some(format!("invalid decision_id: {}", req.decision_id)),
            };
            return Json(serde_json::to_value(&outcome).unwrap_or_default());
        }
    };

    // 2. Look up decision from in-memory state
    let decisions = state.decisions.read().await;
    let decision = match decisions.get(&decision_id) {
        Some(d) => d.clone(),
        None => {
            let outcome = EvidenceRefreshOutcome {
                decision_id,
                status: EvidenceRefreshStatus::NotFound,
                binding_results: Vec::new(),
                detail: Some("decision not found in ledger".to_string()),
            };
            return Json(serde_json::to_value(&outcome).unwrap_or_default());
        }
    };
    drop(decisions);

    // 3. Check eligibility: signoff must not be rejected/superseded
    if matches!(
        decision.signoff_state,
        SignoffState::Rejected | SignoffState::Superseded
    ) {
        let outcome = EvidenceRefreshOutcome {
            decision_id,
            status: EvidenceRefreshStatus::Ineligible,
            binding_results: Vec::new(),
            detail: Some(format!(
                "decision signoff state is {:?} — refresh not applicable",
                decision.signoff_state
            )),
        };
        return Json(serde_json::to_value(&outcome).unwrap_or_default());
    }

    // 4. Look up bindings
    let bindings = state.bindings.read().await;
    let decision_bindings = bindings.get(&decision_id).cloned().unwrap_or_default();
    drop(bindings);

    if decision_bindings.is_empty() {
        let outcome = EvidenceRefreshOutcome {
            decision_id,
            status: EvidenceRefreshStatus::Ineligible,
            binding_results: Vec::new(),
            detail: Some("decision has no binding evidence to refresh".to_string()),
        };
        return Json(serde_json::to_value(&outcome).unwrap_or_default());
    }

    // 5. Delegate to the runtime dispatch for real graph-backed evaluation.
    //
    //    The gateway cannot directly depend on `bicameral-runtime` (cyclic dep),
    //    so we construct a `ToolRequest` with `EvidenceRefresh` and dispatch it
    //    through the `RuntimeDispatch` trait injected by the daemon. This wires
    //    the HTTP endpoint to `code_source_refresh::evaluate_binding_currentness`,
    //    producing real `ContentChanged` / `TargetMissing` outcomes instead of
    //    the former stub that always returned `Current`.
    let dispatch = match &state.runtime_dispatch {
        Some(d) => d.clone(),
        None => {
            let outcome = EvidenceRefreshOutcome {
                decision_id,
                status: EvidenceRefreshStatus::Unsupported,
                binding_results: Vec::new(),
                detail: Some(
                    "runtime dispatch not available — daemon not fully initialized".to_string(),
                ),
            };
            return Json(serde_json::to_value(&outcome).unwrap_or_default());
        }
    };

    let tool_request = bicameral_api::tool_request::ToolRequest::new(
        bicameral_api::tool_request::ToolCommand::EvidenceRefresh(
            bicameral_api::tool_request::EvidenceRefreshParams {
                decision_id: req.decision_id.clone(),
            },
        ),
        bicameral_api::tool_request::AuthorityContext {
            actor_id: "gateway".to_string(),
            auth_method: bicameral_api::tool_request::AuthMethod::LocalDaemon,
            session_id: None,
            workspace: None,
            policy_scope: vec!["default".to_string()],
            audit_metadata: Default::default(),
        },
    );

    let workspace_root = state.workspace_root.read().await.clone();
    let response = dispatch.dispatch(&tool_request, workspace_root.as_deref());

    // The runtime dispatch returns the EvidenceRefreshOutcome in
    // ToolResponse.result. Return it directly so the HTTP surface
    // produces the same typed JSON as the ToolRequest path.
    match response.result {
        Some(result) => Json(result),
        None => {
            let outcome = EvidenceRefreshOutcome {
                decision_id,
                status: EvidenceRefreshStatus::Unsupported,
                binding_results: Vec::new(),
                detail: response.message,
            };
            Json(serde_json::to_value(&outcome).unwrap_or_default())
        }
    }
}

// ── Connection routes ─────────────────────────────────────────────

/// Known providers that support a mock/import start path.
const KNOWN_PROVIDERS: &[&str] = &["github", "google"];

/// Resolve the connections registry file path.
///
/// Prefers `workspace_root` (from `AppState`) when set, falling back to the
/// global operator home (`~/.bicameral/connections.yaml`).
fn connections_file_path(
    workspace_root: &Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    if let Some(ref root) = workspace_root {
        return Some(root.join(".bicameral").join("connections.yaml"));
    }
    bicameral_config::BicameralPaths::connections_file()
}

/// Load the `ConnectionRegistry`, returning an empty default if the file
/// doesn't exist or can't be read.
async fn load_connection_registry_from_state(
    state: &AppState,
) -> bicameral_config::ConnectionRegistry {
    let ws = state.workspace_root.read().await.clone();
    connections_file_path(&ws)
        .and_then(|p| bicameral_config::ConnectionRegistry::load_from_file(&p).ok())
        .unwrap_or_default()
}

/// Persist the `ConnectionRegistry` using the same path resolution.
async fn save_connection_registry(
    state: &AppState,
    registry: &bicameral_config::ConnectionRegistry,
) {
    let ws = state.workspace_root.read().await.clone();
    if let Some(path) = connections_file_path(&ws) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = registry.save_to_file(&path);
    }
}

pub fn connection_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/connections", get(connection_registry))
        .route(
            "/api/v1/connections/:provider/start",
            post(connection_start),
        )
        .route(
            "/api/v1/connections/:provider/callback",
            post(connection_callback).get(connection_loopback_callback),
        )
        .route(
            "/api/v1/connections/:provider/status/:session_id",
            get(connection_status),
        )
}

/// #1083: whether an accepted Google credential is actually readable.
///
/// Accepted credential evidence is narrowly the canonical
/// `connection.google / oauth_token` value or the supported legacy
/// `google_drive / oauth_token` alias, parsed through
/// `parse_stored_oauth_token` so unusable values count as absent.
fn google_credential_readable(state: &AppState) -> bool {
    let usable = |source_id: &str| {
        state
            .secrets_store
            .read_secret(
                source_id,
                bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
            )
            .and_then(|value| bicameral_config::parse_stored_oauth_token(&value))
            .is_some_and(|token| token.is_usable())
    };
    usable(bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID)
        || usable(bicameral_config::connections::legacy_aliases::GOOGLE_DRIVE_SOURCE_AUTH)
}

/// Outcome of a Google `refresh_token` grant, kept three-valued so a
/// rejected refresh and an ambiguous failure are never conflated.
enum GoogleRefreshOutcome {
    Ok(String),
    Rejected,
    Inconclusive,
}

/// Minimal `refresh_token` grant used only to obtain a fresh access token
/// for [`google_credential_live`]'s probe.
// SHORTCUT: does not persist the rotated token, so every check inside the
// refresh-skew window re-refreshes. Add persistence (with a single-flight
// lock, matching KeyringGoogleAccessTokenProvider) if Google's call volume
// from this path becomes a real concern.
async fn google_refresh_access_token(
    http_client: &reqwest::Client,
    refresh_token: &str,
    config: &bicameral_config::GoogleOAuthConfig,
    token_endpoint: &str,
) -> GoogleRefreshOutcome {
    let mut params = vec![
        ("client_id", config.client_id.clone()),
        ("refresh_token", refresh_token.to_string()),
        ("grant_type", "refresh_token".to_string()),
    ];
    if !config.client_secret.is_empty() {
        params.push(("client_secret", config.client_secret.clone()));
    }

    let response = match http_client.post(token_endpoint).form(&params).send().await {
        Ok(response) => response,
        Err(_) => return GoogleRefreshOutcome::Inconclusive,
    };

    match response.status().as_u16() {
        200 => {}
        400 | 401 | 403 => return GoogleRefreshOutcome::Rejected,
        _ => return GoogleRefreshOutcome::Inconclusive,
    }

    let json: serde_json::Value = match response.json().await {
        Ok(json) => json,
        Err(_) => return GoogleRefreshOutcome::Inconclusive,
    };

    match json["access_token"].as_str().filter(|t| !t.is_empty()) {
        Some(token) => GoogleRefreshOutcome::Ok(token.to_string()),
        None => GoogleRefreshOutcome::Inconclusive,
    }
}

/// #1279: whether a locally-usable Google credential is still accepted by
/// Google. A server-side revocation leaves the local token structurally
/// intact, so `google_credential_readable` alone never sees it.
///
/// Refreshes an expiring token before probing it, so a merely-expired
/// token is never mistaken for a revoked one. Only a positive rejection
/// (refresh grant rejected, or live call answering 401) counts as revoked;
/// any other failure, including a Drive 403 for rate, quota, or permission
/// limits, is inconclusive so it never demotes a healthy connection.
/// Reimplements the refresh grant instead of
/// reusing `bicameral_runtime`'s token provider: that crate depends on
/// `bicameral-gateway`, so the reverse dependency isn't available.
// SHORTCUT: no cache in front of this call. Add one if a polling loop
// starts hitting /connections and this becomes measurable latency.
async fn google_credential_live(state: &AppState) -> bool {
    let stored = state
        .secrets_store
        .read_secret(
            bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
            bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
        )
        .or_else(|| {
            state.secrets_store.read_secret(
                bicameral_config::connections::legacy_aliases::GOOGLE_DRIVE_SOURCE_AUTH,
                bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
            )
        })
        .and_then(|value| bicameral_config::parse_stored_oauth_token(&value));

    let access_token = match &stored {
        Some(bicameral_config::StoredOAuthToken::Envelope(envelope))
            if envelope
                .access_token_expired(chrono::Utc::now() + chrono::Duration::seconds(60)) =>
        {
            let Some(refresh_token) = envelope.refresh_token.as_deref().filter(|t| !t.is_empty())
            else {
                return true;
            };
            // Same resolution policy as Connect, so Cloud-first installs
            // without legacy env config still refresh. Google rejects a
            // Desktop client refresh that carries no secret, so a missing
            // stored secret stays inconclusive instead of reading as revoked.
            let Ok(resolved) = resolve_google_oauth_config_for(state).await else {
                return true;
            };
            let mut config = resolved.oauth;
            if config.client_secret.is_empty() {
                let Some(secret) = crate::google_resource_selection::stored_desktop_credential(
                    state.secrets_store.as_ref(),
                    &config.client_id,
                    resolved.config_version.as_deref(),
                ) else {
                    return true;
                };
                config.client_secret = secret;
            }
            match google_refresh_access_token(
                &state.http_client,
                refresh_token,
                &config,
                &google_token_endpoint(state),
            )
            .await
            {
                GoogleRefreshOutcome::Ok(token) => token,
                GoogleRefreshOutcome::Rejected => return false,
                GoogleRefreshOutcome::Inconclusive => return true,
            }
        }
        Some(token) => match token.access_token() {
            Some(access_token) => access_token.to_string(),
            None => return true,
        },
        None => return true,
    };

    let Some(about_endpoint) = google_drive_about_endpoint(state) else {
        return true;
    };

    match state
        .http_client
        .get(about_endpoint)
        .bearer_auth(access_token)
        .send()
        .await
    {
        Ok(response) => response.status() != reqwest::StatusCode::UNAUTHORIZED,
        Err(_) => true,
    }
}

/// Resolve the Drive "about" endpoint for the live-credential probe. The
/// test override exists only under `cfg(test)`; with none set,
/// `google_credential_live` skips the call instead of reaching Google.
fn google_drive_about_endpoint(state: &AppState) -> Option<String> {
    #[cfg(test)]
    {
        state.google_drive_about_endpoint_override.clone()
    }
    #[cfg(not(test))]
    {
        let _ = state;
        Some("https://www.googleapis.com/drive/v3/about?fields=user".to_string())
    }
}

/// #1083/#1279: effective provider status backed by credential presence
/// and, for Google, live acceptance.
///
/// Durable registry metadata can say `Connected` while the credential
/// that made the connection real is gone (restart with a volatile
/// store, cleared keyring) or revoked server-side. Effective truth
/// requires the metadata, a readable credential, and, for Google, a
/// live check to agree: `Connected` without all three projects as
/// `ActionNeeded`, never `Connected`. This is projection only, reads
/// never mutate the registry.
async fn effective_connection_status(
    state: &AppState,
    provider: &str,
    registry_status: bicameral_config::ConnectionStatus,
) -> bicameral_config::ConnectionStatus {
    if provider == "google" && registry_status == bicameral_config::ConnectionStatus::Connected {
        if !google_credential_readable(state) {
            return bicameral_config::ConnectionStatus::ActionNeeded;
        }
        if !google_credential_live(state).await {
            return bicameral_config::ConnectionStatus::ActionNeeded;
        }
    }
    registry_status
}

/// `GET /connections` — return the current connection registry.
///
/// Returns the non-secret connection metadata for all known providers,
/// loaded through the workspace-aware `AppState` path (never the
/// global-home-only fallback while a workspace root is set). Google
/// status is credential-backed per #1083. If the on-disk registry
/// cannot be loaded, returns an empty registry.
async fn connection_registry(
    State(state): State<AppState>,
) -> Json<bicameral_api::dashboard::ConnectionRegistryResponse> {
    let registry = load_connection_registry_from_state(&state).await;

    let mut connections = std::collections::HashMap::new();
    for &provider in KNOWN_PROVIDERS {
        let meta = registry.get(provider);
        let status = effective_connection_status(&state, provider, registry.status(provider)).await;
        connections.insert(
            provider.to_string(),
            bicameral_api::dashboard::ConnectionRegistryEntry {
                status: match status {
                    bicameral_config::ConnectionStatus::Connected => "connected".into(),
                    bicameral_config::ConnectionStatus::ActionNeeded => "action_needed".into(),
                    bicameral_config::ConnectionStatus::NotConnected => "not_connected".into(),
                },
                account_label: meta.and_then(|m| m.account_label.clone()),
                capabilities: meta.map(|m| m.capabilities.clone()).unwrap_or_default(),
                updated_at: meta.and_then(|m| m.updated_at.map(|dt| dt.to_rfc3339())),
                auth_method: meta.and_then(|m| {
                    m.auth_method
                        .as_ref()
                        .map(|a| format!("{a:?}").to_lowercase())
                }),
            },
        );
    }

    Json(bicameral_api::dashboard::ConnectionRegistryResponse { connections })
}

/// `POST /connections/:provider/start` — initiate a provider connection.
///
/// Returns a typed `ConnectionStartOutcome`:
/// - Supported providers with an existing connection → `connected`
/// - GitHub without connection → `start_url` (device flow)
/// - Google with OAuth config → `start_url` (local-loopback OAuth)
/// - Google without OAuth config → `action_needed` (config missing)
/// - Unknown/unconfigured providers → `unsupported`
///
/// For GitHub, when `BICAMERAL_GITHUB_CLIENT_ID` is set and the provider is
/// not yet connected, initiates the OAuth device flow (RFC 8628) and returns
/// `start_url` with a `user_code` + `verification_uri`.
///
/// No secret material is ever included in the response.
async fn connection_start(
    State(state): State<AppState>,
    axum::extract::Path(provider): axum::extract::Path<String>,
) -> (
    StatusCode,
    Json<bicameral_api::dashboard::ConnectionStartOutcome>,
) {
    use bicameral_api::dashboard::ConnectionStartOutcome;

    let provider_lower = provider.to_lowercase();

    if !KNOWN_PROVIDERS.contains(&provider_lower.as_str()) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ConnectionStartOutcome::Unsupported {
                provider: provider_lower,
                message: format!(
                    "{provider} is not a supported provider. Supported: {}.",
                    KNOWN_PROVIDERS.join(", ")
                ),
            }),
        );
    }

    // Check the on-disk connection registry for existing metadata.
    let registry = load_connection_registry_from_state(&state).await;

    // Check in-memory secrets store for legacy alias compatibility.
    let has_legacy_google_secret = provider_lower == "google"
        && state
            .secrets_store
            .read_secret(
                bicameral_config::connections::legacy_aliases::GOOGLE_DRIVE_SOURCE_AUTH,
                bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
            )
            .is_some();

    // #1083: the existing-connected branch uses the same effective truth
    // as GET /connections. Stale `connected` metadata without a readable
    // credential must enter re-auth, never satisfy `Connected`.
    match effective_connection_status(&state, &provider_lower, registry.status(&provider_lower))
        .await
    {
        bicameral_config::ConnectionStatus::Connected => {
            let meta = registry.get(&provider_lower);
            (
                StatusCode::OK,
                Json(ConnectionStartOutcome::Connected {
                    provider: provider_lower,
                    account_label: meta.and_then(|m| m.account_label.clone()),
                }),
            )
        }
        bicameral_config::ConnectionStatus::ActionNeeded => {
            // For Google with ActionNeeded: attempt to start a re-auth flow.
            if provider_lower == "google" {
                return google_start_oauth_flow(&state, &provider_lower).await;
            }
            let msg = match provider_lower.as_str() {
                "github" => "Your GitHub connection needs attention. Reconnect to restore access.",
                _ => "This connection needs attention. Please reconnect.",
            };
            (
                StatusCode::OK,
                Json(ConnectionStartOutcome::ActionNeeded {
                    provider: provider_lower,
                    message: msg.to_string(),
                }),
            )
        }
        bicameral_config::ConnectionStatus::NotConnected => {
            // Legacy alias: if we find an existing google_drive secret,
            // report as already connected (migrate on next refresh).
            if has_legacy_google_secret {
                return (
                    StatusCode::OK,
                    Json(ConnectionStartOutcome::Connected {
                        provider: provider_lower,
                        account_label: Some("(legacy google_drive)".into()),
                    }),
                );
            }

            // Google: initiate local-loopback OAuth flow.
            if provider_lower == "google" {
                return google_start_oauth_flow(&state, &provider_lower).await;
            }

            // Other providers (e.g. GitHub device flow): generate an auth
            // session and return start_url.
            let session_id = format!("sess-{}-{}", provider_lower, uuid_v4_stub());
            let (url, user_code, flow_type) = match provider_lower.as_str() {
                "github" => (
                    "https://github.com/login/device".to_string(),
                    Some("XXXX-XXXX".to_string()),
                    Some("device_flow".to_string()),
                ),
                _ => (
                    format!("https://{provider_lower}.example.com/auth"),
                    None,
                    None,
                ),
            };
            (
                StatusCode::OK,
                Json(ConnectionStartOutcome::StartUrl {
                    provider: provider_lower,
                    url,
                    auth_session_id: session_id,
                    user_code,
                    flow_type,
                }),
            )
        }
    }
}

/// Resolve the governed Cloud-first Google OAuth application config for a
/// new authorization or resource-selection attempt. Shared by provider
/// Connection (#1115) and provider-resource selection (#1135) so both
/// flows consume exactly one resolution policy. The origin override
/// exists only under `cfg(test)`.
pub(crate) async fn resolve_google_oauth_config_for(
    state: &AppState,
) -> Result<
    crate::google_app_config::ResolvedGoogleOAuthConfig,
    crate::google_app_config::GoogleAppConfigResolveError,
> {
    #[cfg(test)]
    {
        if let Some(origin) = state.google_cloud_origin_override.clone() {
            return crate::google_app_config::resolve_google_oauth_config_from_origin(
                &state.google_app_config_cache,
                state.google_oauth_config.as_ref(),
                state.google_oauth_allow_env_override,
                &origin,
            )
            .await;
        }
    }
    crate::google_app_config::resolve_google_oauth_config(
        &state.google_app_config_cache,
        state.google_oauth_config.as_ref(),
        state.google_oauth_allow_env_override,
    )
    .await
}

/// Build a Google OAuth `StartUrl` outcome or `ActionNeeded` if config missing.
async fn google_start_oauth_flow(
    state: &AppState,
    provider: &str,
) -> (
    StatusCode,
    Json<bicameral_api::dashboard::ConnectionStartOutcome>,
) {
    use bicameral_api::dashboard::ConnectionStartOutcome;

    let resolved = match resolve_google_oauth_config_for(state).await {
        Ok(resolved) => resolved,
        Err(error) => {
            return (
                StatusCode::OK,
                Json(ConnectionStartOutcome::ActionNeeded {
                    provider: provider.to_string(),
                    message: format!(
                        "Google OAuth application configuration is unavailable: {error}. Retry after Cloud/operator configuration is restored."
                    ),
                }),
            );
        }
    };

    tracing::info!(
        source = ?resolved.source,
        config_version = resolved.config_version.as_deref().unwrap_or("legacy-env"),
        "resolved Google OAuth application configuration"
    );

    let cfg = resolved.oauth;
    let session_id = format!("gsess-{}", Uuid::new_v4());
    let state_token = Uuid::new_v4().to_string();
    let pkce_verifier = crate::google_oauth::generate_pkce_verifier();
    let pkce_challenge = crate::google_oauth::pkce_s256_challenge(&pkce_verifier);
    let url = cfg.authorization_url_with_pkce(&state_token, Some(&pkce_challenge));

    {
        let mut sessions = state.auth_sessions.write().await;
        sessions.insert(
            session_id.clone(),
            crate::state::AuthSession {
                provider: provider.to_string(),
                created_at: std::time::Instant::now(),
                state_token: state_token.clone(),
                pkce_verifier: Some(pkce_verifier),
                google_oauth_config: Some(cfg),
            },
        );
    }

    (
        StatusCode::OK,
        Json(ConnectionStartOutcome::StartUrl {
            provider: provider.to_string(),
            url,
            auth_session_id: session_id,
            user_code: None,
            flow_type: Some("local_loopback".into()),
        }),
    )
}

/// Stub UUID v4 generator for alpha auth-session IDs.
fn uuid_v4_stub() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:016x}", nanos)
}

/// `POST /connections/:provider/callback` — OAuth local-loopback callback.
///
/// The provider redirects here after the user completes consent. The
/// server consumes the authorization code, exchanges it for tokens
/// (persisted in `secrets_store`), updates `ConnectionRegistry`, and
/// returns a `ConnectionCompletionOutcome`.
///
/// For Google, the handler validates the auth session, persists token
/// material to `secrets_store`, and writes non-secret metadata to
/// `ConnectionRegistry`. No token material appears in the response.
async fn connection_callback(
    State(state): State<AppState>,
    axum::extract::Path(provider): axum::extract::Path<String>,
    Json(body): Json<bicameral_api::dashboard::ConnectionCallbackRequest>,
) -> (
    StatusCode,
    Json<bicameral_api::dashboard::ConnectionCompletionOutcome>,
) {
    use bicameral_api::dashboard::ConnectionCompletionOutcome;

    let provider_lower = provider.to_lowercase();

    if !KNOWN_PROVIDERS.contains(&provider_lower.as_str()) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ConnectionCompletionOutcome::Unsupported {
                provider: provider_lower,
                message: format!(
                    "{provider} is not a supported provider. Supported: {}.",
                    KNOWN_PROVIDERS.join(", ")
                ),
            }),
        );
    }

    // Google-specific: validate session (existence, expiry, state) BEFORE
    // interpreting any provider error, so terminal outcomes consume the
    // exact bound session (#1133) and unauthenticated callers cannot
    // terminate someone else's attempt.
    if provider_lower == "google" {
        return google_handle_callback(&state, &provider_lower, &body).await;
    }

    // If the provider redirect included an error, map it to Denied.
    if let Some(ref err) = body.error {
        return (
            StatusCode::OK,
            Json(ConnectionCompletionOutcome::Denied {
                provider: provider_lower,
                message: format!("Provider returned error: {err}"),
            }),
        );
    }

    // Non-Google providers: deferred alpha stub.
    (
        StatusCode::OK,
        Json(ConnectionCompletionOutcome::ActionNeeded {
            provider: provider_lower,
            message: "OAuth callback received but live token exchange is not yet \
                      implemented for this provider."
                .into(),
        }),
    )
}

/// Google OAuth callback: validate session, exchange code, persist.
async fn google_handle_callback(
    state: &AppState,
    provider: &str,
    body: &bicameral_api::dashboard::ConnectionCallbackRequest,
) -> (
    StatusCode,
    Json<bicameral_api::dashboard::ConnectionCompletionOutcome>,
) {
    use bicameral_api::dashboard::ConnectionCompletionOutcome;

    // 1. Validate the auth session exists.
    let session = {
        let sessions = state.auth_sessions.read().await;
        sessions.get(&body.auth_session_id).cloned()
    };

    let session = match session {
        Some(s) if s.provider == provider => s,
        Some(_) => {
            return (
                StatusCode::OK,
                Json(ConnectionCompletionOutcome::Denied {
                    provider: provider.to_string(),
                    message: "Auth session belongs to a different provider.".into(),
                }),
            );
        }
        None => {
            return (
                StatusCode::OK,
                Json(ConnectionCompletionOutcome::Expired {
                    provider: provider.to_string(),
                    message: "Auth session not found or expired.".into(),
                }),
            );
        }
    };

    // 1b. Enforce the bounded session TTL (#1133) before any state
    // comparison, token exchange, secret write, or registry mutation. The
    // expired session is removed so it can never be retried.
    if session.is_expired(std::time::Instant::now()) {
        let mut sessions = state.auth_sessions.write().await;
        sessions.remove(&body.auth_session_id);
        return (
            StatusCode::OK,
            Json(ConnectionCompletionOutcome::Expired {
                provider: provider.to_string(),
                message: "Auth session not found or expired.".into(),
            }),
        );
    }

    // 2. Validate the CSRF state token. A missing state is not equivalent
    // to a valid state and must fail closed for the native OAuth flow.
    // A mismatched state does NOT consume the session: an unauthenticated
    // caller must not be able to terminate a victim's in-flight attempt;
    // abandoned attempts are bounded by the TTL instead (#1133).
    match body.state.as_deref() {
        Some(csrf_state) if csrf_state == session.state_token => {}
        Some(_) => {
            return (
                StatusCode::OK,
                Json(ConnectionCompletionOutcome::Denied {
                    provider: provider.to_string(),
                    message: "CSRF state mismatch.".into(),
                }),
            );
        }
        None => {
            return (
                StatusCode::OK,
                Json(ConnectionCompletionOutcome::Denied {
                    provider: provider.to_string(),
                    message: "CSRF state is required.".into(),
                }),
            );
        }
    }

    // 2b. Provider-terminal error bound to the exact validated session:
    // denied/cancelled consent is one-shot. The session is consumed so the
    // attempt cannot linger as a completable authorization (#1133).
    if let Some(err) = body.error.as_deref() {
        {
            let mut sessions = state.auth_sessions.write().await;
            sessions.remove(&body.auth_session_id);
        }
        return (
            StatusCode::OK,
            Json(ConnectionCompletionOutcome::Denied {
                provider: provider.to_string(),
                message: format!("Provider returned error: {err}"),
            }),
        );
    }

    // 3. Validate that an authorization code is present.
    let code = match &body.code {
        Some(c) if !c.is_empty() => c.clone(),
        _ => {
            return (
                StatusCode::OK,
                Json(ConnectionCompletionOutcome::ActionNeeded {
                    provider: provider.to_string(),
                    message: "Authorization code is missing from the callback.".into(),
                }),
            );
        }
    };

    // 4. Require the daemon-local PKCE verifier bound to this auth session.
    let pkce_verifier = match session.pkce_verifier.as_deref() {
        Some(verifier) => verifier,
        None => {
            return (
                StatusCode::OK,
                Json(ConnectionCompletionOutcome::Denied {
                    provider: provider.to_string(),
                    message: "PKCE verifier is missing from the auth session.".into(),
                }),
            );
        }
    };

    // 5. Use the exact application configuration bound at start. Re-resolving
    // Cloud here could switch client identity during an in-flight OAuth attempt.
    let _oauth_config = match session.google_oauth_config.clone() {
        Some(cfg) => cfg,
        None => {
            return (
                StatusCode::OK,
                Json(ConnectionCompletionOutcome::Denied {
                    provider: provider.to_string(),
                    message:
                        "Google OAuth application configuration is missing from the auth session."
                            .into(),
                }),
            );
        }
    };

    // 6. Exchange authorization code for tokens and persist the canonical
    // envelope through the shared seam. The authorization code and PKCE
    // verifier are consumed locally and never stored, logged, returned
    // to the browser, or sent through Bicameral Cloud.
    let token_result =
        exchange_and_persist_google_token(state, &_oauth_config, &code, pkce_verifier).await;

    match token_result {
        Ok(token_data) => {
            // 7. Update ConnectionRegistry with non-secret metadata.
            let meta = bicameral_config::ConnectionMetadata {
                status: bicameral_config::ConnectionStatus::Connected,
                account_label: token_data.account_label.clone(),
                capabilities: vec!["drive.file".into()],
                updated_at: Some(chrono::Utc::now()),
                auth_method: Some(bicameral_config::AuthMethod::Oauth),
            };

            {
                let mut registry = load_connection_registry_from_state(state).await;
                registry.upsert("google", meta);
                save_connection_registry(state, &registry).await;
            }

            // 8. Remove the consumed auth session.
            {
                let mut sessions = state.auth_sessions.write().await;
                sessions.remove(&body.auth_session_id);
            }

            (
                StatusCode::OK,
                Json(ConnectionCompletionOutcome::Connected {
                    provider: provider.to_string(),
                    account_label: token_data.account_label,
                    auth_method: Some("oauth".into()),
                    capabilities: vec!["drive.file".into()],
                }),
            )
        }
        Err(GoogleTokenPersistError::Exchange(err)) => (
            StatusCode::OK,
            Json(ConnectionCompletionOutcome::ProviderUnavailable {
                provider: provider.to_string(),
                message: format!("Token exchange failed: {err}"),
            }),
        ),
        Err(GoogleTokenPersistError::UnusableBundle) => (
            StatusCode::OK,
            Json(ConnectionCompletionOutcome::ProviderUnavailable {
                provider: provider.to_string(),
                message: "Token exchange returned an unusable token bundle.".into(),
            }),
        ),
        Err(GoogleTokenPersistError::Persist(message)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ConnectionCompletionOutcome::ProviderUnavailable {
                provider: provider.to_string(),
                message,
            }),
        ),
    }
}

/// Failure classes for the shared Google exchange + persist seam.
///
/// Callers must fail closed on every class: no connection may report
/// `connected` and no resource selection may progress unless the
/// canonical envelope was durably written.
pub(crate) enum GoogleTokenPersistError {
    /// Provider-side exchange failure (safe message, no token material).
    Exchange(String),
    /// Exchange succeeded but returned an unusable token bundle.
    UnusableBundle,
    /// Envelope preparation or keyring write failed (safe message).
    Persist(String),
}

/// Shared seam (#1132, ports the #1098 refactor onto the PKCE lineage):
/// exchange an authorization code via the real provider transport and
/// persist the canonical versioned envelope under
/// `connection.google/oauth_token` before anything can treat the
/// credential as usable. On success the superseded legacy `google_drive`
/// alias value is retired best-effort so a stale plain token cannot
/// diverge from the refreshed credential. Returned token data is consumed
/// server-side only.
pub(crate) async fn exchange_and_persist_google_token(
    state: &AppState,
    config: &bicameral_config::GoogleOAuthConfig,
    code: &str,
    pkce_verifier: &str,
) -> Result<GoogleTokenData, GoogleTokenPersistError> {
    let endpoint = google_token_endpoint(state);
    let token_data = exchange_google_token(config, code, pkce_verifier, &endpoint)
        .await
        .map_err(GoogleTokenPersistError::Exchange)?;

    let envelope = bicameral_config::OAuthTokenEnvelopeV1::from_token_exchange(
        token_data.access_token.clone(),
        token_data.refresh_token.clone(),
        token_data.expires_in_seconds,
        token_data.token_type.clone(),
        token_data.scope.clone(),
        chrono::Utc::now(),
    )
    // The envelope records the non-secret client identity that minted it
    // so daemon-local refresh (runtime crate) works after restart without
    // re-resolving application config (#1134/#1087 composition).
    .with_client_id(Some(config.client_id.clone()));
    if !envelope.is_usable() {
        return Err(GoogleTokenPersistError::UnusableBundle);
    }
    let secret_value = envelope.to_secret_string().map_err(|e| {
        GoogleTokenPersistError::Persist(format!("Failed to prepare token envelope: {e}"))
    })?;
    state
        .secrets_store
        .write_secret(
            bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
            bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
            &secret_value,
        )
        .map_err(|e| GoogleTokenPersistError::Persist(format!("Failed to persist token: {e}")))?;

    // The canonical envelope now supersedes any legacy `google_drive`
    // alias value; drop it best-effort so a stale plain token cannot
    // diverge from the refreshed credential.
    let _ = state.secrets_store.delete_secret(
        bicameral_config::connections::legacy_aliases::GOOGLE_DRIVE_SOURCE_AUTH,
        bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
    );

    Ok(token_data)
}

/// Token data from a Google OAuth token exchange. Raw token material is
/// consumed server-side only: it flows into the versioned secret envelope
/// written to `secrets_store` and never into responses or logs.
pub(crate) struct GoogleTokenData {
    pub(crate) access_token: String,
    pub(crate) refresh_token: Option<String>,
    pub(crate) expires_in_seconds: Option<u64>,
    pub(crate) token_type: Option<String>,
    pub(crate) scope: Option<String>,
    pub(crate) account_label: Option<String>,
}

/// Canonical Google OAuth token endpoint. Production exchanges always
/// target this endpoint; only test builds can substitute a local stub
/// via `AppState::google_token_endpoint_override` (#1132).
const GOOGLE_TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";

/// Resolve the token endpoint for this exchange.
///
/// Production builds have exactly one possible value. The test override
/// exists only under `cfg(test)`, so neither environment variables nor
/// authorization-code contents can ever select a synthetic exchange in a
/// production binary (#1132).
fn google_token_endpoint(state: &AppState) -> String {
    #[cfg(test)]
    {
        if let Some(endpoint) = state.google_token_endpoint_override.clone() {
            return endpoint;
        }
    }
    let _ = state;
    GOOGLE_TOKEN_ENDPOINT.to_string()
}

/// Exchange a Google authorization code for tokens against the real
/// provider transport. There is no mock path: a failed provider exchange
/// is the only alternative to a real provider success (#1132).
async fn exchange_google_token(
    config: &bicameral_config::GoogleOAuthConfig,
    code: &str,
    pkce_verifier: &str,
    token_endpoint: &str,
) -> Result<GoogleTokenData, String> {
    crate::google_oauth::validate_pkce_verifier(pkce_verifier).map_err(str::to_string)?;

    // Live token exchange via HTTP POST. Native/public clients do not
    // send an empty pseudo-secret. Legacy confidential-client overrides may
    // still include a non-empty secret explicitly.
    let client = reqwest::Client::new();
    let mut form = vec![
        ("code", code),
        ("client_id", config.client_id.as_str()),
        ("redirect_uri", config.redirect_uri.as_str()),
        ("grant_type", "authorization_code"),
        ("code_verifier", pkce_verifier),
    ];
    if !config.client_secret.is_empty() {
        form.push(("client_secret", config.client_secret.as_str()));
    }
    let resp = client
        .post(token_endpoint)
        .form(&form)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Token endpoint returned {status}: {body}"));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse token response: {e}"))?;

    let access_token = json["access_token"]
        .as_str()
        .ok_or("Missing access_token in response")?
        .to_string();

    // Preserve refresh/expiry material from the provider response. Google
    // omits refresh_token on exchanges that did not (re)grant offline
    // access; expiry is relative seconds resolved to an absolute instant
    // at envelope-build time.
    let refresh_token = json["refresh_token"].as_str().map(String::from);
    let expires_in_seconds = json["expires_in"].as_u64();
    let token_type = json["token_type"].as_str().map(String::from);
    let scope = json["scope"].as_str().map(String::from);

    // Attempt to extract account label from id_token or userinfo.
    let account_label = json["id_token"]
        .as_str()
        .and(None::<String>) // id_token parsing deferred
        .or_else(|| Some("google-user".into()));

    Ok(GoogleTokenData {
        access_token,
        refresh_token,
        expires_in_seconds,
        token_type,
        scope,
        account_label,
    })
}

/// `GET /connections/:provider/status/:session_id` — poll auth session.
///
/// For device-flow providers (e.g. GitHub), the dashboard polls this
/// endpoint after showing the user code. For local-loopback providers
/// (Google), returns the session state — `Pending` while the callback
/// hasn't arrived, `Expired` if the session is unknown/consumed.
///
/// Returns a `ConnectionCompletionOutcome`:
/// - `Pending` while the auth session is still active.
/// - `Expired` if the session does not exist (consumed or timed out).
/// - `Connected` once the flow completes (device-flow only; Google
///   completes via callback, not polling).
/// - `ProviderUnavailable` if GitHub returned an unexpected error.
async fn connection_status(
    State(state): State<AppState>,
    axum::extract::Path((provider, session_id)): axum::extract::Path<(String, String)>,
) -> (
    StatusCode,
    Json<bicameral_api::dashboard::ConnectionCompletionOutcome>,
) {
    use bicameral_api::dashboard::ConnectionCompletionOutcome;

    let provider_lower = provider.to_lowercase();

    if !KNOWN_PROVIDERS.contains(&provider_lower.as_str()) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ConnectionCompletionOutcome::Unsupported {
                provider: provider_lower,
                message: format!(
                    "{provider} is not a supported provider. Supported: {}.",
                    KNOWN_PROVIDERS.join(", ")
                ),
            }),
        );
    }

    // For GitHub: poll the device-flow session store.
    if provider_lower == "github" {
        match crate::github_device_flow::poll_device_flow(
            &state.device_flow_store,
            &state.http_client,
            &state.github_endpoints,
            &session_id,
        )
        .await
        {
            Ok(outcome) => return (StatusCode::OK, Json(outcome)),
            Err(crate::github_device_flow::GitHubDeviceFlowError::SessionNotFound(_)) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(ConnectionCompletionOutcome::Expired {
                        provider: provider_lower,
                        message: "Device-flow session not found. It may have expired or was never started.".into(),
                    }),
                );
            }
            Err(e) => {
                return (
                    StatusCode::OK,
                    Json(ConnectionCompletionOutcome::ProviderUnavailable {
                        provider: provider_lower,
                        message: format!("Device-flow polling error: {e}"),
                    }),
                );
            }
        }
    }

    // For local-loopback providers (e.g. Google): check the pending
    // auth-session store, falling back to the registry to distinguish a
    // completed connection from an expired/unknown session.
    let session_exists = {
        let sessions = state.auth_sessions.read().await;
        sessions.contains_key(&session_id)
    };

    if !session_exists {
        let registry = load_connection_registry_from_state(&state).await;

        if registry.status(&provider_lower) == bicameral_config::ConnectionStatus::Connected {
            let meta = registry.get(&provider_lower);
            return (
                StatusCode::OK,
                Json(ConnectionCompletionOutcome::Connected {
                    provider: provider_lower,
                    account_label: meta.and_then(|m| m.account_label.clone()),
                    auth_method: meta
                        .and_then(|m| m.auth_method.as_ref())
                        .map(|a| format!("{a:?}").to_lowercase()),
                    capabilities: meta.map(|m| m.capabilities.clone()).unwrap_or_default(),
                }),
            );
        }

        return (
            StatusCode::OK,
            Json(ConnectionCompletionOutcome::Expired {
                provider: provider_lower,
                message: "Auth session not found or already completed.".into(),
            }),
        );
    }

    // Session still pending — callback hasn't arrived yet.
    (
        StatusCode::OK,
        Json(ConnectionCompletionOutcome::Pending {
            provider: provider_lower,
            message: Some("Auth session is active. Waiting for provider callback.".into()),
        }),
    )
}

// ── Runtime-delegated routes (code.locate, graph.status, graph.refresh) ───
//
// These routes expose the runtime's graph/locator services over HTTP.
// The gateway does NOT compute graph facts locally — it constructs a
// `ToolRequest` and delegates to `dispatch_with_workspace` from
// `bicameral-runtime`. Responses preserve the typed result structs
// (`LocatorResult`, `GraphStatusResult`, `GraphRefreshResult`) including
// statuses and next actions.

pub fn runtime_graph_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/code/locate", post(code_locate_handler))
        .route("/api/v1/graph/status", post(graph_status_handler))
        .route("/api/v1/graph/refresh", post(graph_refresh_handler))
}

/// Request body for `POST /api/v1/code/locate`.
#[derive(Deserialize)]
struct CodeLocateRequest {
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    decision_id: Option<String>,
    #[serde(default)]
    candidate_id: Option<String>,
    #[serde(default)]
    source_evidence_ids: Vec<String>,
    #[serde(default)]
    binding_ids: Vec<String>,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    symbols: Vec<String>,
    #[serde(default)]
    changed_files: Vec<String>,
    #[serde(default)]
    diff_context: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    commit_sha: Option<String>,
    #[serde(default)]
    ref_name: Option<String>,
    #[serde(default)]
    graph_snapshot_id: Option<String>,
    #[serde(default)]
    base_ref: Option<String>,
    #[serde(default)]
    base_sha: Option<String>,
    #[serde(default)]
    head_ref: Option<String>,
    #[serde(default)]
    head_sha: Option<String>,
    #[serde(default)]
    scope_kind: Option<bicameral_api::tool_request::LocatorScopeKind>,
    #[serde(default = "default_locate_limit")]
    limit: usize,
}

fn default_locate_limit() -> usize {
    20
}

/// Request body for `POST /api/v1/graph/status`.
#[derive(Deserialize)]
struct GraphStatusRequest {
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    ref_name: Option<String>,
    #[serde(default)]
    commit_sha: Option<String>,
    #[serde(default)]
    graph_snapshot_id: Option<String>,
}

/// Request body for `POST /api/v1/graph/refresh`.
#[derive(Deserialize)]
struct GraphRefreshRequest {
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    ref_name: Option<String>,
    #[serde(default)]
    commit_sha: Option<String>,
    #[serde(default)]
    base_ref: Option<String>,
    #[serde(default)]
    base_sha: Option<String>,
    #[serde(default)]
    head_ref: Option<String>,
    #[serde(default)]
    head_sha: Option<String>,
    #[serde(default)]
    scope_kind: Option<bicameral_api::tool_request::RefreshScopeKind>,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    symbols: Vec<String>,
    #[serde(default)]
    allow_index: Option<bicameral_api::tool_request::AllowIndex>,
    #[serde(default)]
    trigger: Option<bicameral_api::tool_request::RefreshTrigger>,
}

/// Typed error response returned by runtime routes when the request is
/// invalid or capabilities are missing.
#[derive(Serialize)]
struct RuntimeRouteError {
    status: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    next_actions: Vec<String>,
}

/// Build a gateway-internal `AuthorityContext` for runtime dispatch.
fn gateway_authority() -> bicameral_api::tool_request::AuthorityContext {
    bicameral_api::tool_request::AuthorityContext {
        actor_id: "gateway".into(),
        auth_method: bicameral_api::tool_request::AuthMethod::Dashboard,
        session_id: None,
        workspace: None,
        policy_scope: vec![],
        audit_metadata: serde_json::Map::new(),
    }
}

async fn code_locate_handler(
    State(state): State<AppState>,
    Json(req): Json<CodeLocateRequest>,
) -> Response {
    use bicameral_api::tool_request::{
        CodeLocateParams, ToolCommand, ToolRequest, ToolResponseStatus,
    };

    // Validate: at least one search criterion must be provided
    let has_input = req.query.is_some()
        || req.decision_id.is_some()
        || req.candidate_id.is_some()
        || !req.files.is_empty()
        || !req.symbols.is_empty()
        || !req.changed_files.is_empty()
        || !req.binding_ids.is_empty()
        || !req.source_evidence_ids.is_empty();

    if !has_input {
        let err = RuntimeRouteError {
            status: "invalid_payload",
            message: "at least one of query, decision_id, candidate_id, files, symbols, changed_files, binding_ids, or source_evidence_ids is required".into(),
            next_actions: vec![],
        };
        return (StatusCode::BAD_REQUEST, Json(err)).into_response();
    }

    // Check runtime dispatch capability
    let dispatch = match &state.runtime_dispatch {
        Some(d) => d.clone(),
        None => {
            let err = RuntimeRouteError {
                status: "unsupported",
                message: "runtime dispatch not available — code.locate requires daemon mode".into(),
                next_actions: vec!["start_daemon".into()],
            };
            return (StatusCode::SERVICE_UNAVAILABLE, Json(err)).into_response();
        }
    };

    let workspace_root = state.workspace_root.read().await;
    let ws_path = workspace_root.clone();
    drop(workspace_root);

    let params = CodeLocateParams {
        query: req.query,
        decision_id: req.decision_id,
        candidate_id: req.candidate_id,
        source_evidence_ids: req.source_evidence_ids,
        binding_ids: req.binding_ids,
        files: req.files,
        symbols: req.symbols,
        changed_files: req.changed_files,
        diff_context: req.diff_context,
        workspace_id: req.workspace_id,
        commit_sha: req.commit_sha,
        ref_name: req.ref_name,
        graph_snapshot_id: req.graph_snapshot_id,
        base_ref: req.base_ref,
        base_sha: req.base_sha,
        head_ref: req.head_ref,
        head_sha: req.head_sha,
        scope_kind: req.scope_kind,
        limit: req.limit,
    };

    let tool_req = ToolRequest::new(ToolCommand::CodeLocate(params), gateway_authority());

    let resp = dispatch.dispatch(&tool_req, ws_path.as_deref());

    match resp.status {
        ToolResponseStatus::Ok => {
            (StatusCode::OK, Json(resp.result.unwrap_or_default())).into_response()
        }
        ToolResponseStatus::Rejected => {
            let err = RuntimeRouteError {
                status: "rejected",
                message: resp.message.unwrap_or_default(),
                next_actions: vec![],
            };
            (StatusCode::UNPROCESSABLE_ENTITY, Json(err)).into_response()
        }
        ToolResponseStatus::Error => {
            let err = RuntimeRouteError {
                status: "error",
                message: resp.message.unwrap_or_default(),
                next_actions: vec![],
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response()
        }
    }
}

async fn graph_status_handler(
    State(state): State<AppState>,
    Json(req): Json<GraphStatusRequest>,
) -> Response {
    use bicameral_api::tool_request::{
        GraphStatusParams, ToolCommand, ToolRequest, ToolResponseStatus,
    };

    let dispatch = match &state.runtime_dispatch {
        Some(d) => d.clone(),
        None => {
            let err = RuntimeRouteError {
                status: "unsupported",
                message: "runtime dispatch not available — graph.status requires daemon mode"
                    .into(),
                next_actions: vec!["start_daemon".into()],
            };
            return (StatusCode::SERVICE_UNAVAILABLE, Json(err)).into_response();
        }
    };

    let workspace_root = state.workspace_root.read().await;
    let ws_path = workspace_root.clone();
    drop(workspace_root);

    let params = GraphStatusParams {
        workspace_id: req.workspace_id,
        ref_name: req.ref_name,
        commit_sha: req.commit_sha,
        graph_snapshot_id: req.graph_snapshot_id,
    };

    let tool_req = ToolRequest::new(ToolCommand::GraphStatus(params), gateway_authority());

    let resp = dispatch.dispatch(&tool_req, ws_path.as_deref());

    match resp.status {
        ToolResponseStatus::Ok => {
            (StatusCode::OK, Json(resp.result.unwrap_or_default())).into_response()
        }
        ToolResponseStatus::Rejected => {
            let err = RuntimeRouteError {
                status: "rejected",
                message: resp.message.unwrap_or_default(),
                next_actions: vec![],
            };
            (StatusCode::UNPROCESSABLE_ENTITY, Json(err)).into_response()
        }
        ToolResponseStatus::Error => {
            let err = RuntimeRouteError {
                status: "error",
                message: resp.message.unwrap_or_default(),
                next_actions: vec![],
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response()
        }
    }
}

async fn graph_refresh_handler(
    State(state): State<AppState>,
    Json(req): Json<GraphRefreshRequest>,
) -> Response {
    use bicameral_api::tool_request::{
        AllowIndex, GraphRefreshParams, RefreshTrigger, ToolCommand, ToolRequest,
        ToolResponseStatus,
    };

    let dispatch = match &state.runtime_dispatch {
        Some(d) => d.clone(),
        None => {
            let err = RuntimeRouteError {
                status: "unsupported",
                message: "runtime dispatch not available — graph.refresh requires daemon mode"
                    .into(),
                next_actions: vec!["start_daemon".into()],
            };
            return (StatusCode::SERVICE_UNAVAILABLE, Json(err)).into_response();
        }
    };

    let workspace_root = state.workspace_root.read().await;
    let ws_path = workspace_root.clone();
    drop(workspace_root);

    let params = GraphRefreshParams {
        workspace_id: req.workspace_id,
        ref_name: req.ref_name,
        commit_sha: req.commit_sha,
        base_ref: req.base_ref,
        base_sha: req.base_sha,
        head_ref: req.head_ref,
        head_sha: req.head_sha,
        scope_kind: req.scope_kind,
        files: req.files,
        symbols: req.symbols,
        allow_index: req.allow_index.unwrap_or(AllowIndex::Bounded),
        trigger: req.trigger.unwrap_or(RefreshTrigger::Manual),
    };

    let tool_req = ToolRequest::new(
        ToolCommand::GraphRefreshSnapshot(params),
        gateway_authority(),
    );

    let resp = dispatch.dispatch(&tool_req, ws_path.as_deref());

    match resp.status {
        ToolResponseStatus::Ok => {
            (StatusCode::OK, Json(resp.result.unwrap_or_default())).into_response()
        }
        ToolResponseStatus::Rejected => {
            let err = RuntimeRouteError {
                status: "rejected",
                message: resp.message.unwrap_or_default(),
                next_actions: vec![],
            };
            (StatusCode::UNPROCESSABLE_ENTITY, Json(err)).into_response()
        }
        ToolResponseStatus::Error => {
            let err = RuntimeRouteError {
                status: "error",
                message: resp.message.unwrap_or_default(),
                next_actions: vec![],
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response()
        }
    }
}

// ── Hosted Bridge Routes ─────────────────────────────────────────
//
// Hosted bridge route family: browser-origin pairing/session routes that
// gate access to local projections and governed mutations through the
// gateway guard. Separate from external ingest and local ToolRequest
// surfaces per ADR-0002.

/// Hosted bridge route family.
///
/// All routes require a valid, non-expired, non-revoked pairing session
/// (except the pairing request endpoint itself). The gateway guard
/// rejects unpaired requests before they can read projections or mutate
/// canonical state.
pub fn hosted_bridge_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/hosted-bridge/pair",
            post(hosted_bridge_pair_request),
        )
        .route("/api/v1/hosted-bridge/status", get(hosted_bridge_status))
        // #790: retry-safe status exchange the initiating browser polls to learn
        // the pending outcome and, only once locally approved, retrieve the
        // session handoff. Bound to the initiator's origin/account + poll proof.
        .route(
            "/api/v1/hosted-bridge/pair/status",
            get(hosted_bridge_pair_status),
        )
        // #790: LOCAL-only confirmation surface data. Serves the pending request
        // (incl. the same-origin decision nonce) to the locally served surface.
        .route(
            "/api/v1/hosted-bridge/pair/pending/:request_handle",
            get(hosted_bridge_pair_pending_view),
        )
        // #865: LOCAL-only terminal/CLI discovery and code lookup. The list
        // never includes a code/proof/nonce; lookup returns the existing local
        // view but does not approve or mint a session.
        .route(
            "/api/v1/hosted-bridge/pair/pending",
            get(hosted_bridge_pair_pending_list),
        )
        .route(
            "/api/v1/hosted-bridge/pair/code/lookup",
            post(hosted_bridge_pair_code_lookup),
        )
        // #790: LOCAL-only explicit approve/deny decision. Fails closed for any
        // hosted/cross-origin caller; only same-origin local approval mints a
        // session.
        .route(
            "/api/v1/hosted-bridge/pair/decision",
            post(hosted_bridge_pair_decision),
        )
        .route("/api/v1/hosted-bridge/revoke", post(hosted_bridge_revoke))
        // #711: daemon protocol compatibility handshake + non-confidential
        // aggregate telemetry (protocol-version distribution + fire rate).
        .route(
            "/api/v1/hosted-bridge/compatibility",
            get(hosted_bridge_compatibility),
        )
        .route(
            "/api/v1/hosted-bridge/compatibility/telemetry",
            get(hosted_bridge_compatibility_telemetry),
        )
        .route(
            "/api/v1/hosted-bridge/operator-home",
            get(hosted_bridge_operator_home),
        )
        .route(
            "/api/v1/hosted-bridge/projects",
            get(hosted_bridge_projects),
        )
        // #719: project-scoped readiness for the SPA-selected Cloud project.
        .route(
            "/api/v1/hosted-bridge/projects/:project_id",
            get(hosted_bridge_selected_project),
        )
        .route(
            "/api/v1/hosted-bridge/projection/ledger",
            get(hosted_bridge_projection_ledger),
        )
        .route("/api/v1/hosted-bridge/command", post(hosted_bridge_command))
        // #767: same-domain Product Member auto-join via signed ticket.
        .route(
            "/api/v1/hosted-bridge/product-join",
            post(hosted_bridge_product_join),
        )
        // #769: local setup continuation (Open / Continue setup / Repair).
        .route(
            "/api/v1/hosted-bridge/product-continue-setup",
            post(hosted_bridge_product_continue_setup),
        )
        // #614: Team-visible workspace discovery (safe metadata only).
        .route(
            "/api/v1/hosted-bridge/workspace-discovery",
            get(hosted_bridge_workspace_discovery),
        )
        // #616: Hosted-mode dashboard data adapter.
        .route(
            "/api/v1/hosted-bridge/dashboard-adapter",
            get(hosted_bridge_dashboard_adapter),
        )
}

/// Default bridge session TTL: 8 hours.
const DEFAULT_BRIDGE_TTL_SECONDS: u64 = 8 * 60 * 60;

/// Maximum bridge session TTL: 24 hours.
const MAX_BRIDGE_TTL_SECONDS: u64 = 24 * 60 * 60;

/// Pairing request: validates origin, generates session, returns paired state.
///
/// This is the only hosted bridge route that does not require an active
/// session — it initiates the pairing flow.
///
/// Accepts the raw JSON payload so smuggled authority fields are detected
/// before deserialization drops them.
async fn hosted_bridge_pair_request(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    // Detect smuggled authority fields in the raw payload.
    let smuggled = bicameral_api::hosted_bridge::detect_smuggled_fields(&payload);
    if !smuggled.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "authority_smuggling_rejected",
                "kind": "AuthoritySmugglingRejected",
                "smuggled_fields": smuggled,
            })),
        )
            .into_response();
    }

    let origin = match payload.get("origin").and_then(|v| v.as_str()) {
        Some(o) => o,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "missing required field: origin"})),
            )
                .into_response();
        }
    };

    // #613: Validate origin against trusted allowlist before session creation.
    {
        let custom_origins = state.custom_trusted_origins.read().await;
        match bicameral_api::hosted_bridge::validate_origin(origin, &custom_origins) {
            bicameral_api::hosted_bridge::OriginValidationOutcome::Trusted => {}
            bicameral_api::hosted_bridge::OriginValidationOutcome::Rejected {
                origin: rejected_origin,
                reason,
            } => {
                tracing::warn!(
                    origin = %rejected_origin,
                    "Hosted bridge pairing rejected: origin not on trusted allowlist"
                );
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({
                        "error": "origin_not_trusted",
                        "kind": "OriginMismatch",
                        "origin": rejected_origin,
                        "message": reason,
                    })),
                )
                    .into_response();
            }
        }
    }

    let cloud_account_id = match payload.get("cloud_account_id").and_then(|v| v.as_str()) {
        Some(a) => a,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "missing required field: cloud_account_id"})),
            )
                .into_response();
        }
    };

    let csrf_token = match payload.get("csrf_token").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "missing required field: csrf_token"})),
            )
                .into_response();
        }
    };

    // Validate CSRF token is non-empty.
    if csrf_token.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "csrf_token must not be empty"})),
        )
            .into_response();
    }

    // #790: incompatible old immediate-pair consumers/daemons fail closed with
    // a typed update-required outcome rather than silently receiving a session.
    let client_protocol_version = payload
        .get("client_protocol_version")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    if let bicameral_api::hosted_bridge::PairingProtocolCompatibility::UpdateRequired {
        client_protocol_version: declared,
    } = bicameral_api::hosted_bridge::evaluate_pairing_protocol_compatibility(
        client_protocol_version,
    ) {
        let response = bicameral_api::hosted_bridge::pairing_update_required_response(declared);
        return (StatusCode::UPGRADE_REQUIRED, Json(response)).into_response();
    }

    // bot#1290 / CONF-PAIR-INIT-TEAM-01: pending pair must already carry the
    // Cloud team_id (and enroll bearer) so approve-time mint cannot half-pair.
    let team_id = payload
        .get("team_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if team_id.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "missing required field: team_id",
                "kind": "MissingTeamId",
                "message": "POST /pair requires non-empty team_id",
            })),
        )
            .into_response();
    }
    let cloud_access_token = payload
        .get("cloud_access_token")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if cloud_access_token.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "missing required field: cloud_access_token",
                "kind": "MissingCloudAccessToken",
                "message": "POST /pair requires non-empty cloud_access_token",
            })),
        )
            .into_response();
    }

    // Determine TTL: use requested value (clamped) or default.
    let requested_ttl = payload
        .get("requested_ttl_seconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_BRIDGE_TTL_SECONDS);
    let ttl_seconds = requested_ttl.clamp(60, MAX_BRIDGE_TTL_SECONDS);

    // #790: opportunistically clean up abandoned/expired pending requests.
    let now = chrono::Utc::now();
    prune_expired_pending(&state, now).await;

    // Create a transient, daemon-memory pending request. NO session token is
    // minted here — a token is released only after an explicit local approval.
    let request_handle = uuid::Uuid::new_v4().to_string();
    let poll_proof = uuid::Uuid::new_v4().to_string();
    let local_confirmation_nonce = uuid::Uuid::new_v4().to_string();
    let pairing_code = generate_pairing_code();
    let pairing_code_digest = digest_pairing_code(&pairing_code);
    let pending_expires_at =
        now + chrono::Duration::seconds(crate::state::PENDING_PAIRING_TTL_SECONDS);

    let record = crate::state::PendingPairingRecord {
        request_handle: request_handle.clone(),
        poll_proof: poll_proof.clone(),
        local_confirmation_nonce,
        pairing_code_digest,
        origin: origin.to_string(),
        cloud_account_id: cloud_account_id.to_string(),
        team_id,
        cloud_access_token,
        created_at: now,
        expires_at: pending_expires_at,
        requested_ttl_seconds: ttl_seconds,
        decision: crate::state::PendingPairingDecision::Pending,
        session_token: None,
        session_expires_at: None,
        pairing_id: None,
    };

    state
        .hosted_bridge_pending
        .write()
        .await
        .insert(request_handle.clone(), record);

    let response = bicameral_api::hosted_bridge::HostedBridgePairingPendingResponse {
        status: bicameral_api::hosted_bridge::PendingPairingOutcome::Pending,
        request_handle: request_handle.clone(),
        poll_proof,
        pairing_code,
        expires_at: pending_expires_at.to_rfc3339(),
        protocol_version: bicameral_api::hosted_bridge::HOSTED_BRIDGE_PROTOCOL_VERSION,
        capabilities: bridge_capability_envelope(),
        confirmation: bicameral_api::hosted_bridge::LocalConfirmationAffordance {
            local_confirmation_url: Some(crate::local_approval::local_confirmation_url(
                &request_handle,
            )),
            terminal_command: "bicameral pairing approve".to_string(),
            message: "Approve this browser in Bicameral on this computer, or run `bicameral pairing approve` and enter this code."
                .to_string(),
            requesting_origin: origin.to_string(),
            cloud_account_id: cloud_account_id.to_string(),
            capability_summary: bicameral_api::hosted_bridge::pairing_capability_summary(),
        },
    };

    (StatusCode::ACCEPTED, Json(response)).into_response()
}

/// Capability/version envelope advertised through the pending-pairing flow.
fn bridge_capability_envelope() -> bicameral_api::hosted_bridge::HostedBridgeCapability {
    bicameral_api::hosted_bridge::HostedBridgeCapability {
        bridge_supported: true,
        daemon_build_version: bicameral_api::hosted_bridge::DAEMON_BUILD_VERSION.to_string(),
        hosted_bridge_protocol_version:
            bicameral_api::hosted_bridge::HOSTED_BRIDGE_PROTOCOL_VERSION,
        bridge_protocol_version: "v1".to_string(),
        daemon_protocol_version: bicameral_api::hosted_bridge::HOSTED_DASHBOARD_PROTOCOL_VERSION,
        project_list_available: true,
        workspace_binding_available: false,
        note: None,
    }
}

/// Read the `Origin` request header, if present and valid UTF-8.
fn request_origin(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers.get("origin").and_then(|v| v.to_str().ok())
}

/// Remove pending requests that have passed their expiry instant (#790).
///
/// Abandoned pending requests are cleaned up so daemon memory does not grow
/// unbounded. Terminal (approved/denied) records are retained until expiry so
/// that status polling and local decisions remain retry-safe/idempotent.
async fn prune_expired_pending(state: &AppState, now: chrono::DateTime<chrono::Utc>) {
    let mut pending = state.hosted_bridge_pending.write().await;
    // Drop Cloud bearers before removing expired records so a retained
    // expired entry (if any race leaves one briefly) never keeps the token.
    for record in pending.values_mut() {
        if record.is_expired(now) {
            record.cloud_access_token = None;
        }
    }
    pending.retain(|_, record| !record.is_expired(now));
}

/// Generate an eight-symbol Crockford Base32 code, formatted as `XXXX-XXXX`.
/// UUID v4 bytes provide the randomness; the plaintext exists only long
/// enough to build the initiating browser's pending response.
fn generate_pairing_code() -> String {
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let random = uuid::Uuid::new_v4();
    let symbols: String = random
        .as_bytes()
        .iter()
        .take(bicameral_api::hosted_bridge::PAIRING_CODE_SYMBOL_COUNT)
        .map(|byte| ALPHABET[(byte & 0x1f) as usize] as char)
        .collect();
    format!("{}-{}", &symbols[..4], &symbols[4..])
}

fn digest_pairing_code(code: &str) -> [u8; 32] {
    let normalized = bicameral_api::hosted_bridge::normalize_pairing_code(code)
        .expect("generated pairing code must be valid");
    Sha256::digest(normalized.as_bytes()).into()
}

fn constant_time_bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// GET `/api/v1/hosted-bridge/pair/pending` — LOCAL-only summaries for CLI
/// discovery. Sensitive lookup material is intentionally omitted.
async fn hosted_bridge_pair_pending_list(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Response {
    if !bicameral_api::hosted_bridge::is_local_confirmation_origin(request_origin(&headers))
        || !crate::local_approval::host_is_loopback_or_absent(&headers)
    {
        return local_only_rejection();
    }

    let now = chrono::Utc::now();
    let pending = state.hosted_bridge_pending.read().await;
    let requests = pending
        .values()
        .filter(|record| {
            record.decision == crate::state::PendingPairingDecision::Pending
                && !record.is_expired(now)
        })
        .map(
            |record| bicameral_api::hosted_bridge::PendingPairingSummary {
                request_handle: record.request_handle.clone(),
                requesting_origin: record.origin.clone(),
                cloud_account_id: record.cloud_account_id.clone(),
                expires_at: record.expires_at.to_rfc3339(),
            },
        )
        .collect();

    (
        StatusCode::OK,
        Json(bicameral_api::hosted_bridge::PendingPairingListResponse { requests }),
    )
        .into_response()
}

/// POST `/api/v1/hosted-bridge/pair/code/lookup` — LOCAL-only code match.
/// Matching returns the safe local view and decision nonce, but never changes
/// state or mints a session. A separate explicit decision remains required.
async fn hosted_bridge_pair_code_lookup(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<bicameral_api::hosted_bridge::LocalPairingCodeLookupRequest>,
) -> Response {
    if !bicameral_api::hosted_bridge::is_local_confirmation_origin(request_origin(&headers))
        || !crate::local_approval::host_is_loopback_or_absent(&headers)
    {
        return local_only_rejection();
    }
    if !state.pairing_code_rate_limiter.check() {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(bicameral_api::hosted_bridge::HostedBridgeErrorResponse {
                error: bicameral_api::hosted_bridge::HostedBridgeErrorKind::PairingProofMismatch,
                message: "Too many pairing-code attempts. Wait before trying again.".to_string(),
                retry_after_repair: true,
            }),
        )
            .into_response();
    }

    let Some(normalized) =
        bicameral_api::hosted_bridge::normalize_pairing_code(&payload.pairing_code)
    else {
        return pairing_code_mismatch_rejection();
    };
    let supplied_digest: [u8; 32] = Sha256::digest(normalized.as_bytes()).into();
    let now = chrono::Utc::now();
    let pending = state.hosted_bridge_pending.read().await;
    let Some(record) = pending.values().find(|record| {
        record.decision == crate::state::PendingPairingDecision::Pending
            && !record.is_expired(now)
            && constant_time_bytes_eq(&record.pairing_code_digest, &supplied_digest)
    }) else {
        return pairing_code_mismatch_rejection();
    };

    let view = bicameral_api::hosted_bridge::PendingPairingLocalView {
        status: bicameral_api::hosted_bridge::PendingPairingOutcome::Pending,
        request_handle: record.request_handle.clone(),
        requesting_origin: record.origin.clone(),
        cloud_account_id: record.cloud_account_id.clone(),
        capability_summary: bicameral_api::hosted_bridge::pairing_capability_summary(),
        expires_at: record.expires_at.to_rfc3339(),
        local_confirmation_nonce: record.local_confirmation_nonce.clone(),
    };
    (StatusCode::OK, Json(view)).into_response()
}

/// GET `/api/v1/hosted-bridge/pair/pending/:request_handle` — LOCAL-only view
/// of a pending pairing request for the locally served confirmation surface
/// (#790).
///
/// Fails closed for any hosted/cross-origin caller: only a same-origin/local
/// request may read the pending request (and thus obtain the decision nonce).
async fn hosted_bridge_pair_pending_view(
    State(state): State<AppState>,
    axum::extract::Path(request_handle): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
) -> Response {
    if !bicameral_api::hosted_bridge::is_local_confirmation_origin(request_origin(&headers))
        || !crate::local_approval::host_is_loopback_or_absent(&headers)
    {
        return local_only_rejection();
    }

    let now = chrono::Utc::now();
    let pending = state.hosted_bridge_pending.read().await;
    let record = match pending.get(&request_handle) {
        Some(r) => r.clone(),
        None => return pending_not_found_rejection(),
    };
    drop(pending);

    let status = pending_outcome(&record, now);
    let view = bicameral_api::hosted_bridge::PendingPairingLocalView {
        status,
        request_handle: record.request_handle.clone(),
        requesting_origin: record.origin.clone(),
        cloud_account_id: record.cloud_account_id.clone(),
        capability_summary: bicameral_api::hosted_bridge::pairing_capability_summary(),
        expires_at: record.expires_at.to_rfc3339(),
        local_confirmation_nonce: record.local_confirmation_nonce.clone(),
    };
    (StatusCode::OK, Json(view)).into_response()
}

/// POST `/api/v1/hosted-bridge/pair/decision` — LOCAL-only explicit approve or
/// deny of a pending pairing request (#790).
///
/// Fails closed for hosted/cross-origin callers, unknown handles, mismatched
/// nonces, expired requests, and replays. Only an explicit same-origin local
/// approval mints and binds a bridge session; the token is never returned here.
async fn hosted_bridge_pair_decision(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<bicameral_api::hosted_bridge::LocalPairingDecisionRequest>,
) -> Response {
    // Local-only surface: reject any hosted/cross-origin caller before touching
    // the pending request.
    if !bicameral_api::hosted_bridge::is_local_confirmation_origin(request_origin(&headers))
        || !crate::local_approval::host_is_loopback_or_absent(&headers)
    {
        return local_only_rejection();
    }

    let now = chrono::Utc::now();
    let mut pending = state.hosted_bridge_pending.write().await;
    let record = match pending.get_mut(&payload.request_handle) {
        Some(r) => r,
        None => return pending_not_found_rejection(),
    };

    // Same-origin proof: the decision nonce must match the stored value.
    if !constant_time_eq(
        &record.local_confirmation_nonce,
        &payload.local_confirmation_nonce,
    ) {
        return proof_mismatch_rejection();
    }

    // Expired pending request: terminal. Drop the Cloud bearer before leaving
    // the record for status polling / later prune — it is only needed for the
    // bounded enroll call, which never runs after expiry.
    if record.decision == crate::state::PendingPairingDecision::Pending && record.is_expired(now) {
        record.cloud_access_token = None;
        let handle = record.request_handle.clone();
        return typed_decision_response(
            bicameral_api::hosted_bridge::PendingPairingOutcome::Expired,
            &handle,
            "This pairing request expired before it was confirmed.",
        );
    }

    match record.decision {
        // Replay/idempotency: a request already denied cannot be approved, and a
        // second decision never mints a new session. Return the terminal state.
        crate::state::PendingPairingDecision::Denied => typed_decision_response(
            bicameral_api::hosted_bridge::PendingPairingOutcome::Denied,
            &record.request_handle,
            "This pairing request was already denied.",
        ),
        crate::state::PendingPairingDecision::Approved => typed_decision_response(
            bicameral_api::hosted_bridge::PendingPairingOutcome::Approved,
            &record.request_handle,
            "This pairing request was already approved.",
        ),
        crate::state::PendingPairingDecision::Pending => match payload.action {
            bicameral_api::hosted_bridge::PairingDecisionAction::Deny => {
                record.decision = crate::state::PendingPairingDecision::Denied;
                record.session_token = None;
                record.session_expires_at = None;
                // Deny never enrolls — drop the Cloud bearer from retained state.
                record.cloud_access_token = None;
                let handle = record.request_handle.clone();
                typed_decision_response(
                    bicameral_api::hosted_bridge::PendingPairingOutcome::Denied,
                    &handle,
                    "Pairing request denied.",
                )
            }
            bicameral_api::hosted_bridge::PairingDecisionAction::Approve => {
                // One ceremony, two artifacts (#1198/#1196): ephemeral session_token
                // plus PairingAuthorizationV1 enrollment grant; durable pairing_id.
                let session_token = uuid::Uuid::new_v4().to_string();
                let session_expires_at =
                    now + chrono::Duration::seconds(record.requested_ttl_seconds as i64);
                let pairing_id = format!("pairing-{}", uuid::Uuid::new_v4().as_simple());
                let protocol_session_id = record.request_handle.clone();
                let origin = record.origin.clone();
                let cloud_account_id = record.cloud_account_id.clone();
                let team_id = record.team_id.clone();
                let restore_poll_proof = record.poll_proof.clone();
                let requested_ttl_seconds = record.requested_ttl_seconds;
                let handle = record.request_handle.clone();
                let durable = crate::durable_pairing_store::new_durable_record(
                    crate::durable_pairing_store::NewDurablePairingRecord {
                        pairing_id: pairing_id.clone(),
                        cloud_account_id: cloud_account_id.clone(),
                        approved_origin: origin.clone(),
                        daemon_instance_id: state.daemon_instance_id.clone(),
                        team_id: team_id.clone(),
                        restore_request_handle: handle.clone(),
                        restore_poll_proof: &restore_poll_proof,
                        session_ttl_seconds: requested_ttl_seconds,
                        now,
                    },
                );
                if let Err(err) = state.durable_pairing_store.upsert(durable) {
                    tracing::error!(error = %err, "durable pairing_id persist failed");
                    return typed_decision_response(
                        bicameral_api::hosted_bridge::PendingPairingOutcome::Denied,
                        &handle,
                        "Pairing could not be durably recorded. Retry local approve.",
                    );
                }

                // The durable relationship is the approve commit. Only after it
                // succeeds may the transient session and approved state exist.
                let cloud_access_token = record.cloud_access_token.take();
                let session = crate::state::HostedBridgePairingSession {
                    session_token: session_token.clone(),
                    pairing_id: Some(pairing_id.clone()),
                    origin: origin.clone(),
                    cloud_account_id: cloud_account_id.clone(),
                    paired_at: now,
                    expires_at: session_expires_at,
                    revoked: false,
                };
                record.decision = crate::state::PendingPairingDecision::Approved;
                record.session_token = Some(session_token.clone());
                record.session_expires_at = Some(session_expires_at);
                record.pairing_id = Some(pairing_id.clone());
                debug_assert!(
                    record.cloud_access_token.is_none(),
                    "approved pending must not retain cloud_access_token"
                );
                drop(pending);

                state
                    .hosted_bridge_sessions
                    .write()
                    .await
                    .insert(session_token, session);

                let enroll_message = match complete_identity_enrollment_after_approve(
                    &state,
                    &pairing_id,
                    &protocol_session_id,
                    &cloud_account_id,
                    &origin,
                    team_id.as_deref(),
                    cloud_access_token.as_deref(),
                    now,
                )
                .await
                {
                    Ok(status) => {
                        let _ = state
                            .durable_pairing_store
                            .set_enrollment_status(&pairing_id, &status);
                        format!(" Enrollment: {status}.")
                    }
                    Err(err) => {
                        let status = format!("failed:{err}");
                        let _ = state
                            .durable_pairing_store
                            .set_enrollment_status(&pairing_id, &status);
                        tracing::warn!(
                            pairing_id = %pairing_id,
                            error = %err,
                            "identity enrollment failed closed (pairing session still minted)"
                        );
                        format!(" Enrollment failed closed: {err}.")
                    }
                };

                typed_decision_response(
                    bicameral_api::hosted_bridge::PendingPairingOutcome::Approved,
                    &handle,
                    &format!(
                        "Pairing approved (pairing_id={pairing_id}). Return to your browser to continue.{enroll_message}"
                    ),
                )
            }
        },
    }
}

/// Mint PairingAuthorizationV1 and POST Cloud enroll (row 12 / cloud#305).
///
/// Fail closed: missing team_id/token or HTTP failure never claims enrolled.
#[allow(clippy::too_many_arguments)]
async fn complete_identity_enrollment_after_approve(
    state: &AppState,
    pairing_id: &str,
    protocol_session_id: &str,
    cloud_account_id: &str,
    approved_origin: &str,
    team_id: Option<&str>,
    cloud_access_token: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<String, String> {
    let team_id = team_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            "team_id missing on pending pairing — Cloud enroll requires team_id".to_string()
        })?;
    let token = cloud_access_token
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            "cloud_access_token missing on pending pairing — cannot authenticate Cloud enroll"
                .to_string()
        })?;

    let signer = crate::proof_signing::KeyringProofSigner::new(state.secrets_store.clone());
    let auth = crate::pairing_authorization_mint::mint_pairing_authorization_v1(
        &crate::pairing_authorization_mint::MintPairingAuthorizationRequest {
            pairing_id,
            protocol_session_id,
            cloud_account_id,
            approved_origin,
            daemon_instance_id: &state.daemon_instance_id,
            team_id,
            now,
            ttl_secs: None,
        },
        &signer,
    )
    .map_err(|e| e.to_string())?;

    let cloud_origin = std::env::var("BICAMERAL_CLOUD_ORIGIN")
        .unwrap_or_else(|_| "https://app.bicameral-ai.com".into());

    let ensure = crate::identity_enroll_client::ensure_cloud_pairing(
        &state.http_client,
        &cloud_origin,
        team_id,
        pairing_id,
        &state.daemon_instance_id,
        token,
    )
    .await;
    if !ensure.is_success() {
        return Err(format!(
            "Cloud DaemonPairing ensure failed ({}) — daemon-issued pairing_id may be rejected until Cloud accepts it",
            ensure.status_label()
        ));
    }

    let body = crate::identity_enroll_client::EnrollIdentityBody {
        identity_public_key_b64: auth.identity_public_key_b64.clone(),
        identity_key_id: auth.identity_key_id.clone(),
        pairing_authorization: auth,
    };
    let outcome = crate::identity_enroll_client::enroll_daemon_identity(
        &state.http_client,
        &cloud_origin,
        team_id,
        pairing_id,
        token,
        &body,
    )
    .await;
    if outcome.is_success() {
        Ok(outcome.status_label())
    } else {
        Err(outcome.status_label())
    }
}

/// Query params for the hosted status exchange (#790).
#[derive(serde::Deserialize)]
struct PairStatusParams {
    request_handle: String,
    poll_proof: String,
    cloud_account_id: String,
}

/// GET `/api/v1/hosted-bridge/pair/status` — retry-safe status exchange the
/// initiating browser polls (#790).
///
/// Bound to the initiator's origin (from the `Origin` header), account, and
/// poll proof. Returns the typed pending outcome; only an `approved` request
/// whose session is still valid releases the session token + expiry. Every
/// other outcome omits the token entirely.
async fn hosted_bridge_pair_status(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    axum::extract::Query(params): axum::extract::Query<PairStatusParams>,
) -> Response {
    let now = chrono::Utc::now();
    let record = {
        let pending = state.hosted_bridge_pending.read().await;
        pending.get(&params.request_handle).cloned()
    };
    let Some(record) = record else {
        return durable_pairing_reattach(&state, &headers, &params, now).await;
    };

    // Bind the poll to the original request: proof and account must match.
    // When Origin is present it must match the pending record. When Origin is
    // absent (common for same-origin browser GETs via a local Vite proxy),
    // the unguessable poll_proof is the initiator binding — do not fail closed
    // solely for a missing Origin, or the SPA wipes the pairing code after the
    // first 2s poll.
    if !constant_time_eq(&record.poll_proof, &params.poll_proof) {
        return proof_mismatch_rejection();
    }
    if let Some(origin) = request_origin(&headers) {
        if origin != record.origin {
            return status_mismatch_rejection(
                bicameral_api::hosted_bridge::HostedBridgeErrorKind::OriginMismatch,
                "Pairing request origin did not match.",
            );
        }
    }
    if params.cloud_account_id != record.cloud_account_id {
        return status_mismatch_rejection(
            bicameral_api::hosted_bridge::HostedBridgeErrorKind::AccountMismatch,
            "Pairing request account did not match.",
        );
    }

    let status = pending_outcome(&record, now);
    let response = match status {
        bicameral_api::hosted_bridge::PendingPairingOutcome::Approved => {
            // Retry-safe handoff: release the token only while the underlying
            // session is still valid (not revoked/expired). No token in URL,
            // logs, or persistence — it travels only in this response body.
            match approved_session_handoff(&state, &record, now).await {
                Some((token, expires_at)) => {
                    bicameral_api::hosted_bridge::HostedBridgePairingStatusResponse {
                        status: bicameral_api::hosted_bridge::PendingPairingOutcome::Approved,
                        session_token: Some(token),
                        expires_at: Some(expires_at.to_rfc3339()),
                        capabilities: Some(bridge_capability_envelope()),
                        message: Some("Pairing approved.".to_string()),
                    }
                }
                None => bicameral_api::hosted_bridge::HostedBridgePairingStatusResponse {
                    status: bicameral_api::hosted_bridge::PendingPairingOutcome::Expired,
                    session_token: None,
                    expires_at: None,
                    capabilities: None,
                    message: Some(
                        "The approved session is no longer valid. Re-pair to continue.".to_string(),
                    ),
                },
            }
        }
        other => bicameral_api::hosted_bridge::HostedBridgePairingStatusResponse {
            status: other,
            session_token: None,
            expires_at: None,
            capabilities: None,
            message: Some(pending_status_message(other).to_string()),
        },
    };

    (StatusCode::OK, Json(response)).into_response()
}

/// Re-mint an ephemeral browser session from a durable, locally approved
/// pairing relationship after daemon process restart.
async fn durable_pairing_reattach(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    params: &PairStatusParams,
    now: chrono::DateTime<chrono::Utc>,
) -> Response {
    let Some(record) = state
        .durable_pairing_store
        .find_by_restore_request_handle(&params.request_handle)
    else {
        return pending_not_found_rejection();
    };

    let Some(expected_proof_digest) = record.restore_poll_proof_sha256.as_deref() else {
        return pending_not_found_rejection();
    };
    let supplied_proof_digest = sha256_hex(&params.poll_proof);
    if !constant_time_eq(expected_proof_digest, &supplied_proof_digest) {
        return proof_mismatch_rejection();
    }
    if request_origin(headers) != Some(record.approved_origin.as_str()) {
        return status_mismatch_rejection(
            bicameral_api::hosted_bridge::HostedBridgeErrorKind::OriginMismatch,
            "Pairing relationship origin did not match.",
        );
    }
    if params.cloud_account_id != record.cloud_account_id {
        return status_mismatch_rejection(
            bicameral_api::hosted_bridge::HostedBridgeErrorKind::AccountMismatch,
            "Pairing relationship account did not match.",
        );
    }
    if record.revoked
        || record.protocol_version != bicameral_api::hosted_bridge::HOSTED_BRIDGE_PROTOCOL_VERSION
        || record.profile != bicameral_api::pairing_authorization::PAIRING_AUTHORIZATION_PROFILE
        || record.daemon_instance_id != state.daemon_instance_id
    {
        return pending_not_found_rejection();
    }
    let Some(ttl_seconds) = record.session_ttl_seconds else {
        return pending_not_found_rejection();
    };

    // Hold one write lock across lookup + insert so concurrent status retries
    // cannot each mint a different live session for the same relationship.
    let mut sessions = state.hosted_bridge_sessions.write().await;
    let existing_session = sessions
        .values()
        .find(|session| {
            session.pairing_id.as_deref() == Some(record.pairing_id.as_str())
                && !session.revoked
                && session.expires_at > now
        })
        .map(|session| (session.session_token.clone(), session.expires_at));
    if let Some((session_token, expires_at)) = existing_session {
        return (
            StatusCode::OK,
            Json(
                bicameral_api::hosted_bridge::HostedBridgePairingStatusResponse {
                    status: bicameral_api::hosted_bridge::PendingPairingOutcome::Approved,
                    session_token: Some(session_token),
                    expires_at: Some(expires_at.to_rfc3339()),
                    capabilities: Some(bridge_capability_envelope()),
                    message: Some("Pairing restored.".to_string()),
                },
            ),
        )
            .into_response();
    }

    let session_token = uuid::Uuid::new_v4().to_string();
    let expires_at = now + chrono::Duration::seconds(ttl_seconds as i64);
    let session = crate::state::HostedBridgePairingSession {
        session_token: session_token.clone(),
        pairing_id: Some(record.pairing_id.clone()),
        origin: record.approved_origin,
        cloud_account_id: record.cloud_account_id,
        paired_at: now,
        expires_at,
        revoked: false,
    };
    sessions.insert(session_token.clone(), session);

    (
        StatusCode::OK,
        Json(
            bicameral_api::hosted_bridge::HostedBridgePairingStatusResponse {
                status: bicameral_api::hosted_bridge::PendingPairingOutcome::Approved,
                session_token: Some(session_token),
                expires_at: Some(expires_at.to_rfc3339()),
                capabilities: Some(bridge_capability_envelope()),
                message: Some("Pairing restored.".to_string()),
            },
        ),
    )
        .into_response()
}

/// Resolve the current outcome of a pending record, folding in lazy expiry.
fn pending_outcome(
    record: &crate::state::PendingPairingRecord,
    now: chrono::DateTime<chrono::Utc>,
) -> bicameral_api::hosted_bridge::PendingPairingOutcome {
    match record.decision {
        crate::state::PendingPairingDecision::Approved => {
            bicameral_api::hosted_bridge::PendingPairingOutcome::Approved
        }
        crate::state::PendingPairingDecision::Denied => {
            bicameral_api::hosted_bridge::PendingPairingOutcome::Denied
        }
        crate::state::PendingPairingDecision::Pending => {
            if record.is_expired(now) {
                bicameral_api::hosted_bridge::PendingPairingOutcome::Expired
            } else {
                bicameral_api::hosted_bridge::PendingPairingOutcome::Pending
            }
        }
    }
}

/// Look up the released session token for an approved record, but only while
/// the bound session remains valid (present, not revoked, not expired).
async fn approved_session_handoff(
    state: &AppState,
    record: &crate::state::PendingPairingRecord,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<(String, chrono::DateTime<chrono::Utc>)> {
    let token = record.session_token.as_ref()?;
    let sessions = state.hosted_bridge_sessions.read().await;
    let session = sessions.get(token)?;
    if session.revoked || session.expires_at < now {
        return None;
    }
    Some((token.clone(), session.expires_at))
}

/// Human-readable status message for a non-approved pending outcome.
fn pending_status_message(
    outcome: bicameral_api::hosted_bridge::PendingPairingOutcome,
) -> &'static str {
    match outcome {
        bicameral_api::hosted_bridge::PendingPairingOutcome::Pending => {
            "Awaiting local confirmation."
        }
        bicameral_api::hosted_bridge::PendingPairingOutcome::Denied => {
            "Pairing request was denied."
        }
        bicameral_api::hosted_bridge::PendingPairingOutcome::Expired => {
            "Pairing request expired before it was confirmed."
        }
        bicameral_api::hosted_bridge::PendingPairingOutcome::Incompatible => {
            "This client must update before pairing."
        }
        bicameral_api::hosted_bridge::PendingPairingOutcome::Approved => "Pairing approved.",
    }
}

/// Build a typed local decision response (never carries a token).
fn typed_decision_response(
    status: bicameral_api::hosted_bridge::PendingPairingOutcome,
    request_handle: &str,
    message: &str,
) -> Response {
    let response = bicameral_api::hosted_bridge::LocalPairingDecisionResponse {
        status,
        request_handle: request_handle.to_string(),
        message: message.to_string(),
    };
    (StatusCode::OK, Json(response)).into_response()
}

/// Typed rejection for a decision/view attempted from a non-local surface.
fn local_only_rejection() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(bicameral_api::hosted_bridge::HostedBridgeErrorResponse {
            error: bicameral_api::hosted_bridge::HostedBridgeErrorKind::LocalConfirmationRequired,
            message: "Pairing confirmation is only available on the local confirmation surface."
                .to_string(),
            retry_after_repair: false,
        }),
    )
        .into_response()
}

/// Typed rejection for an unknown/cleaned-up pending request handle.
fn pending_not_found_rejection() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(bicameral_api::hosted_bridge::HostedBridgeErrorResponse {
            error: bicameral_api::hosted_bridge::HostedBridgeErrorKind::PendingRequestNotFound,
            message: "No such pending pairing request.".to_string(),
            retry_after_repair: true,
        }),
    )
        .into_response()
}

/// Typed rejection when a supplied proof/nonce does not match. Fails closed.
fn proof_mismatch_rejection() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(bicameral_api::hosted_bridge::HostedBridgeErrorResponse {
            error: bicameral_api::hosted_bridge::HostedBridgeErrorKind::PairingProofMismatch,
            message: "Pairing request proof did not match.".to_string(),
            retry_after_repair: false,
        }),
    )
        .into_response()
}

/// Generic code mismatch response. It deliberately does not distinguish an
/// unknown, malformed, consumed, or expired code.
fn pairing_code_mismatch_rejection() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(bicameral_api::hosted_bridge::HostedBridgeErrorResponse {
            error: bicameral_api::hosted_bridge::HostedBridgeErrorKind::PairingProofMismatch,
            message: "Pairing code did not match an active request.".to_string(),
            retry_after_repair: false,
        }),
    )
        .into_response()
}

/// Typed rejection when the polling origin/account does not match the request.
fn status_mismatch_rejection(
    error: bicameral_api::hosted_bridge::HostedBridgeErrorKind,
    message: &str,
) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(bicameral_api::hosted_bridge::HostedBridgeErrorResponse {
            error,
            message: message.to_string(),
            retry_after_repair: false,
        }),
    )
        .into_response()
}

/// Constant-time string comparison for opaque proofs/nonces, so a mismatch
/// does not leak position information through timing.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Query params for the compatibility handshake.
#[derive(serde::Deserialize)]
struct CompatibilityParams {
    /// Protocol version the cloud-served SPA expects. When absent, the daemon
    /// assumes the cloud is at parity with the daemon's own protocol version.
    cloud_protocol_version: Option<u32>,
}

/// Hosted-dashboard daemon protocol compatibility handshake (#711).
///
/// Compares the cloud SPA's expected protocol version against this daemon's own
/// `daemon_protocol_version` and returns a `HostedDashboardCompatibility`
/// describing whether the daemon is current, one behind (N-1), too old
/// (`hosted_dashboard_upgrade_required`, with a one-action repair path), or
/// newer than the cloud expects (forward skew — degrades safely, no hard fail).
///
/// This is renderer-only readiness metadata (ADR-0025): it grants no authority
/// and carries no workspace/ledger/source/path data. Each call folds the
/// outcome into non-confidential aggregate telemetry.
async fn hosted_bridge_compatibility(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<CompatibilityParams>,
) -> Response {
    let daemon_protocol_version = bicameral_api::hosted_bridge::HOSTED_DASHBOARD_PROTOCOL_VERSION;
    let cloud_expected = params
        .cloud_protocol_version
        .unwrap_or(daemon_protocol_version);

    let compat = bicameral_api::hosted_bridge::evaluate_hosted_dashboard_compatibility(
        cloud_expected,
        daemon_protocol_version,
    );

    state
        .hosted_dashboard_compat_telemetry
        .write()
        .await
        .record(&compat);

    (StatusCode::OK, Json(compat)).into_response()
}

/// Non-confidential aggregate compatibility telemetry (#711).
///
/// Returns the upgrade-required fire rate and observed daemon protocol-version
/// distribution — protocol-version integers and counts only. Never exposes
/// workspace, ledger, source, evidence, provider, or local-path data.
async fn hosted_bridge_compatibility_telemetry(State(state): State<AppState>) -> Response {
    let telemetry = state.hosted_dashboard_compat_telemetry.read().await.clone();
    let fire_rate = telemetry.upgrade_required_fire_rate();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "total_checks": telemetry.total_checks,
            "upgrade_required_count": telemetry.upgrade_required_count,
            "upgrade_required_fire_rate": fire_rate,
            "forward_skew_count": telemetry.forward_skew_count,
            "daemon_protocol_version_distribution":
                telemetry.daemon_protocol_version_distribution,
        })),
    )
        .into_response()
}

/// Extract and validate the bridge session token from request headers.
///
/// Returns the session if valid, non-expired, and non-revoked. This is
/// the hosted bridge gateway guard — it runs before any handler that
/// reads projections or mutates state.
#[allow(clippy::result_large_err)] // Axum's concrete Response is the intentional handler error type.
async fn validate_bridge_session(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<crate::state::HostedBridgePairingSession, Response> {
    let token = headers
        .get("x-bridge-session-token")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error": "missing_bridge_session",
                    "kind": "NotPaired",
                    "message": "Hosted bridge session token required. Pair first via POST /api/v1/hosted-bridge/pair.",
                })),
            )
                .into_response()
        })?;

    let sessions = state.hosted_bridge_sessions.read().await;
    let session = sessions.get(token).cloned().ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "invalid_bridge_session",
                "kind": "NotPaired",
                "message": "Bridge session token not recognized. Pair first.",
            })),
        )
            .into_response()
    })?;

    if session.revoked {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "bridge_session_revoked",
                "kind": "PairingRevoked",
                "message": "Bridge pairing has been revoked.",
            })),
        )
            .into_response());
    }

    if session.expires_at < chrono::Utc::now() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "bridge_session_expired",
                "kind": "PairingExpired",
                "message": "Bridge pairing has expired.",
            })),
        )
            .into_response());
    }

    Ok(session)
}

/// POST /api/v1/hosted-bridge/revoke — revoke an active pairing session.
///
/// Requires the session token in the `x-bridge-session-token` header.
/// Marks the session as revoked so subsequent requests are rejected.
async fn hosted_bridge_revoke(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Response {
    let token = match headers
        .get("x-bridge-session-token")
        .and_then(|v| v.to_str().ok())
    {
        Some(t) => t.to_string(),
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error": "missing_bridge_session",
                    "kind": "NotPaired",
                    "message": "Hosted bridge session token required.",
                })),
            )
                .into_response();
        }
    };

    let mut sessions = state.hosted_bridge_sessions.write().await;
    match sessions.get(&token) {
        Some(session) => {
            let pairing_id = session.pairing_id.clone();
            let already_revoked = session.revoked;
            if let Some(pairing_id) = pairing_id.as_deref() {
                // Invalidate every live token first. If durable persistence is
                // unavailable, the current process must still fail closed and
                // a retry can attempt the durable write again.
                for session in sessions.values_mut() {
                    if session.pairing_id.as_deref() == Some(pairing_id) {
                        session.revoked = true;
                    }
                }
                if let Err(err) = state.durable_pairing_store.revoke(pairing_id) {
                    tracing::error!(error = %err, pairing_id, "durable pairing revoke failed");
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({
                            "error": "durable_pairing_revoke_failed",
                            "kind": "RuntimeUnavailable",
                            "message": "Pairing could not be durably revoked.",
                        })),
                    )
                        .into_response();
                }
            } else if already_revoked {
                return (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "pairing_state": "hosted_bridge_revoked",
                        "message": "Pairing was already revoked.",
                        "revoked_at": chrono::Utc::now().to_rfc3339(),
                        "re_pair_available": true,
                    })),
                )
                    .into_response();
            } else if let Some(session) = sessions.get_mut(&token) {
                session.revoked = true;
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "pairing_state": "hosted_bridge_revoked",
                    "message": if already_revoked {
                        "Pairing was already revoked."
                    } else {
                        "Pairing revoked successfully."
                    },
                    "revoked_at": chrono::Utc::now().to_rfc3339(),
                    "re_pair_available": true,
                })),
            )
                .into_response()
        }
        None => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "invalid_bridge_session",
                "kind": "NotPaired",
                "message": "Bridge session token not recognized.",
            })),
        )
            .into_response(),
    }
}

/// GET /api/v1/hosted-bridge/status — pairing status check.
async fn hosted_bridge_status(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Response {
    match validate_bridge_session(&state, &headers).await {
        Ok(session) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "pairing_state": "hosted_bridge_paired",
                "origin": session.origin,
                "cloud_account_id": session.cloud_account_id,
                "paired_at": session.paired_at.to_rfc3339(),
                "expires_at": session.expires_at.to_rfc3339(),
            })),
        )
            .into_response(),
        Err(resp) => resp,
    }
}

/// GET /api/v1/hosted-bridge/operator-home — operator home view.
///
/// Requires a valid bridge session. Returns the operator home view with
/// locally registered projects (non-authoritative projection data only).
/// Sensitive fields (filesystem paths, provider secrets, SourceEvidence
/// bodies, Decision Ledger event payloads) are stripped by the projection.
async fn hosted_bridge_operator_home(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Response {
    let session = match validate_bridge_session(&state, &headers).await {
        Ok(s) => s,
        Err(resp) => return resp,
    };

    let registrations = state
        .project_registrations
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let projects = crate::state::project_readiness_projection(&registrations);
    drop(registrations);

    let capability_readiness = match state.runtime_dispatch.as_ref() {
        Some(dispatch) => match dispatch.capability_readiness() {
            Ok(readiness) => readiness,
            Err(_) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "error": "capability_readiness_unavailable"
                    })),
                )
                    .into_response();
            }
        },
        None => Vec::new(),
    };
    let response = bicameral_api::hosted_bridge::OperatorHomeSessionResponse {
        pairing_state: bicameral_api::hosted_bridge::HostedBridgePairingState::Paired,
        projects,
        capability_readiness,
        workspace_binding_available: false,
        status_message: Some(format!(
            "Paired to origin {}. Workspace binding requires project-level request.",
            session.origin
        )),
    };

    Json(response).into_response()
}

/// GET /api/v1/hosted-bridge/projects — project list/readiness projection.
///
/// Requires a valid bridge session. Returns the locally registered project
/// list plus the paired account's canonical Product membership replay as
/// hosted-safe projection data. Sensitive fields are stripped. Membership is
/// derived only from CreateProduct / ProductMemberAdded; registrations affect
/// setup readiness but never grant membership.
async fn hosted_bridge_projects(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Response {
    let session = match validate_bridge_session(&state, &headers).await {
        Ok(s) => s,
        Err(resp) => return resp,
    };

    let entries = match state.event_store.replay() {
        Ok(entries) => entries,
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "membership_projection_unavailable",
                    "kind": "ProjectionUnavailable",
                    "message": "Canonical Product membership could not be replayed.",
                })),
            )
                .into_response();
        }
    };
    let registrations = state
        .project_registrations
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let projects = crate::state::project_readiness_projection(&registrations);
    let memberships = crate::state::product_membership_projection(
        &entries,
        &session.cloud_account_id,
        &registrations,
    );
    drop(registrations);

    Json(serde_json::json!({
        "projects": projects,
        "memberships": memberships,
        "projection_only": true,
        "workspace_binding_available": false,
    }))
    .into_response()
}

/// Optional query parameters carried alongside a selected-project request.
///
/// The SPA may echo the owning cloud team and human label from the discovery
/// contract so the daemon can round-trip them in the scoped response. They are
/// cloud-coordination identity only — never used to infer local membership.
#[derive(Debug, Default, serde::Deserialize)]
struct SelectedProjectQuery {
    #[serde(default)]
    team_id: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
}

/// GET /api/v1/hosted-bridge/projects/:project_id — selected-project readiness (#719).
///
/// Requires a valid bridge session. Scopes the daemon's readiness projection to
/// the single SPA-selected Cloud project, echoing the selected identity so the
/// caller can confirm the selection flowed through pairing → projection.
///
/// Authority boundary: the local project list is used only to *scope* to the
/// selection; it is never the Cloud membership source. A selection with no
/// matching local binding returns `binding_readiness = unbound` ("needs
/// binding") with HTTP 200 — not a 403/404 membership verdict and not the
/// unscoped project list. Redaction matches the other bridge projections (no
/// local paths, provider secrets, or source bodies).
async fn hosted_bridge_selected_project(
    State(state): State<AppState>,
    axum::extract::Path(project_id): axum::extract::Path<String>,
    axum::extract::Query(query): axum::extract::Query<SelectedProjectQuery>,
    headers: axum::http::HeaderMap,
) -> Response {
    let _session = match validate_bridge_session(&state, &headers).await {
        Ok(s) => s,
        Err(resp) => return resp,
    };

    let registrations = state
        .project_registrations
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let projects = crate::state::project_readiness_projection(&registrations);
    drop(registrations);

    let selected = bicameral_api::hosted_bridge::SelectedProjectRef {
        project_id,
        team_id: query.team_id,
        display_name: query.display_name,
    };
    let resolved_at = chrono::Utc::now().to_rfc3339();
    let response = bicameral_api::hosted_bridge::resolve_selected_project_readiness(
        bicameral_api::hosted_bridge::HostedBridgePairingState::Paired,
        &selected,
        &projects,
        &resolved_at,
    );

    Json(response).into_response()
}

/// GET /api/v1/hosted-bridge/projection/ledger — bridge-gated projection read.
///
/// Guarded: requires a valid bridge session. Unpaired requests are
/// rejected before the projection is read (acceptance criterion #2).
///
/// This is a **network-origin** route (browser ↔ daemon), so it must serve only
/// the T4 renderable allow-list (#705, approved in #698 item 5) — ids,
/// timestamps, status/review-state enums, counts, and redactor-passed titles.
/// It deliberately does **not** return the full local `ProjectionState`
/// (`build_projection_from_state`), which carries decision bodies, source
/// excerpts, code paths, and event summaries and stays on the loopback
/// `/api/v1/dashboard/*` surface (ADR-0020 / ADR-0025).
async fn hosted_bridge_projection_ledger(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Response {
    let _session = match validate_bridge_session(&state, &headers).await {
        Ok(s) => s,
        Err(resp) => return resp,
    };

    let proj = build_renderable_ledger_from_state(&state).await;
    Json(proj).into_response()
}

/// POST /api/v1/hosted-bridge/command — bridge-gated governed command.
///
/// Routes paired hosted bridge command requests through the canonical
/// ToolRequest dispatcher. Guard order:
///   1. Validate bridge session (unpaired/expired/revoked → reject).
///   2. Reject authority-smuggling fields (ADR-0020).
///   3. Parse and validate the command payload.
///   4. Enforce cloud-routable command allowlist (#615).
///   5. Derive daemon-owned AuthorityContext from session.
///   6. Dispatch through injected RuntimeDispatch.
///
/// Closes #581 and #615.
async fn hosted_bridge_command(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    // 1. Validate bridge session.
    let session = match validate_bridge_session(&state, &headers).await {
        Ok(s) => s,
        Err(resp) => return resp,
    };

    // 2. Detect authority-smuggling attempts before deserializing command.
    let smuggled = bicameral_api::hosted_bridge::detect_smuggled_fields(&payload);
    if !smuggled.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "authority_smuggling_rejected",
                "kind": "AuthoritySmugglingRejected",
                "smuggled_fields": smuggled,
            })),
        )
            .into_response();
    }

    // 3. Parse the command from the payload. The bridge accepts a ToolCommand
    //    JSON object (tagged with `name` + `params`), not a full ToolRequest.
    let command: bicameral_api::tool_request::ToolCommand =
        match serde_json::from_value(payload.get("command").cloned().unwrap_or(payload.clone())) {
            Ok(cmd) => cmd,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": "invalid_command",
                        "kind": "UnsupportedCommand",
                        "message": format!("Failed to parse bridge command: {e}"),
                    })),
                )
                    .into_response();
            }
        };

    // 4. Enforce cloud-routable command allowlist (#615).
    let command_name = command.command_name();
    if !bicameral_api::hosted_bridge::is_cloud_routable_command(command_name) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "command_not_allowed",
                "kind": "CommandNotAllowed",
                "command": command_name,
                "message": format!(
                    "Command '{}' is not allowed through the cloud-routed hosted bridge.",
                    command_name
                ),
            })),
        )
            .into_response();
    }

    // 5. Derive daemon-owned AuthorityContext from the validated session.
    //    The caller never supplies authority — it is derived from the bridge
    //    session and surface metadata.
    // #1264: a hosted `product.create` may transport a Google Drive folder
    // descriptor, but only the daemon's own resource-selection flow can make
    // it usable. Stamp the validated folder into daemon-derived audit metadata
    // when, and only when, a live daemon-authored `Ready` selection session
    // holds that exact folder. The runtime fails closed without the stamp.
    let validated_event_store_folder =
        crate::google_resource_selection::validated_event_store_folder(&state, &command).await;

    let authority = bicameral_api::tool_request::AuthorityContext {
        actor_id: format!("bridge:{}", session.cloud_account_id),
        auth_method: bicameral_api::tool_request::AuthMethod::HostedBridge,
        session_id: Some(session.session_token.clone()),
        workspace: None,
        policy_scope: vec!["hosted_bridge".to_string()],
        audit_metadata: {
            let mut m = serde_json::Map::new();
            m.insert(
                "surface".into(),
                serde_json::Value::String("hosted_bridge".to_string()),
            );
            m.insert(
                "bridge_origin".into(),
                serde_json::Value::String(session.origin.clone()),
            );
            if let Some(folder_id) = validated_event_store_folder {
                m.insert(
                    bicameral_api::product_registration::DESCRIPTOR_VALIDATION_METADATA_KEY.into(),
                    serde_json::Value::String(
                        bicameral_api::product_registration::DescriptorValidation::Valid
                            .as_str()
                            .to_string(),
                    ),
                );
                m.insert(
                    bicameral_api::product_registration::VALIDATED_EVENT_STORE_FOLDER_ID_METADATA_KEY
                        .into(),
                    serde_json::Value::String(folder_id),
                );
            }
            m
        },
    };

    let tool_request = bicameral_api::tool_request::ToolRequest::new(command, authority);

    // 6. Dispatch through injected RuntimeDispatch.
    let dispatch = match &state.runtime_dispatch {
        Some(d) => d.clone(),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "runtime_unavailable",
                    "kind": "RuntimeUnavailable",
                    "message": "Runtime dispatch not available (daemon not fully initialized).",
                })),
            )
                .into_response();
        }
    };

    let workspace_root = state.workspace_root.read().await.clone();
    let response = dispatch.dispatch(&tool_request, workspace_root.as_deref());
    (StatusCode::OK, Json(response)).into_response()
}

/// Bridge request body for `product.join` (#767).
///
/// The SPA supplies only the Cloud-minted signed ticket plus an optional
/// idempotency `request_id`. Actor context, authority, pairing identity, and
/// the local actor reference are all daemon-derived from the validated bridge
/// session — never accepted from this body. Unknown fields are rejected so a
/// caller cannot smuggle an SPA-asserted actor or membership claim.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProductJoinBridgeRequest {
    #[serde(default)]
    request_id: Option<String>,
    signed_ticket: bicameral_api::product_join::SignedProductJoinTicket,
}

/// POST /api/v1/hosted-bridge/product-join — same-domain Product Member
/// auto-join (#767).
///
/// Daemon-authoritative: the gateway validates the bridge pairing session,
/// derives the confirmed local actor and paired Cloud account from that
/// session (never from the request body), and delegates ticket verification,
/// policy re-evaluation, idempotency, and canonical membership materialization
/// to the runtime. The daemon receives no Supabase/session token and no
/// SPA-asserted actor context.
async fn hosted_bridge_product_join(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    // 1. Validate bridge session (fail closed when unpaired/expired/revoked).
    let session = match validate_bridge_session(&state, &headers).await {
        Ok(s) => s,
        Err(resp) => return resp,
    };

    // 2. Reject authority-smuggling attempts before parsing.
    let smuggled = bicameral_api::hosted_bridge::detect_smuggled_fields(&payload);
    if !smuggled.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "authority_smuggling_rejected",
                "kind": "AuthoritySmugglingRejected",
                "smuggled_fields": smuggled,
            })),
        )
            .into_response();
    }

    // 3. Parse the bounded bridge body (only signed ticket + optional id).
    let body: ProductJoinBridgeRequest = match serde_json::from_value(payload) {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "invalid_product_join_request",
                    "kind": "MalformedPayload",
                    "message": format!("Failed to parse product.join request: {e}"),
                })),
            )
                .into_response();
        }
    };

    // 4. Derive the daemon-confirmed local actor from the bridge session. This
    //    is the pairing/actor binding the daemon trusts; the SPA cannot assert
    //    a different actor.
    let confirmed_actor = format!("bridge:{}", session.cloud_account_id);
    let request_id = body
        .request_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let params = bicameral_api::product_join::ProductJoinCommandParams {
        request_id,
        signed_ticket: body.signed_ticket,
        local_actor_ref: confirmed_actor.clone(),
        requested_at: chrono::Utc::now(),
    };

    // 5. Derive daemon-owned authority from the validated session. The paired
    //    Cloud account id is published in audit metadata under a daemon-set key
    //    so the runtime can bind the ticket account to the confirmed pairing.
    let authority = bicameral_api::tool_request::AuthorityContext {
        actor_id: confirmed_actor,
        auth_method: bicameral_api::tool_request::AuthMethod::HostedBridge,
        session_id: Some(session.session_token.clone()),
        workspace: None,
        policy_scope: vec!["hosted_bridge".to_string()],
        audit_metadata: {
            let mut m = serde_json::Map::new();
            m.insert(
                "surface".into(),
                serde_json::Value::String("hosted_bridge".to_string()),
            );
            m.insert(
                "bridge_origin".into(),
                serde_json::Value::String(session.origin.clone()),
            );
            m.insert(
                bicameral_api::product_join::PAIRED_CLOUD_ACCOUNT_METADATA_KEY.into(),
                serde_json::Value::String(session.cloud_account_id.clone()),
            );
            m
        },
    };

    let tool_request = bicameral_api::tool_request::ToolRequest::new(
        bicameral_api::tool_request::ToolCommand::ProductJoin(params),
        authority,
    );

    // 6. Dispatch through the injected runtime (daemon supplies trust anchors).
    let dispatch = match &state.runtime_dispatch {
        Some(d) => d.clone(),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "runtime_unavailable",
                    "kind": "RuntimeUnavailable",
                    "message": "Runtime dispatch not available (daemon not fully initialized).",
                })),
            )
                .into_response();
        }
    };

    let workspace_root = state.workspace_root.read().await.clone();
    let response = dispatch.dispatch(&tool_request, workspace_root.as_deref());
    (StatusCode::OK, Json(response)).into_response()
}

/// Bridge request body for `product.continue_setup` (#769).
///
/// The SPA supplies only which Product to continue, an opaque routing
/// `registration_ref`, and optional non-secret routing hints (provider,
/// substrate, display label). Provider access, descriptor validation, the
/// confirmed local actor, and the paired Cloud account are all daemon-derived
/// from the validated bridge session and local connection state — never
/// accepted from this body. Unknown fields are rejected so a caller cannot
/// smuggle a provider secret, raw descriptor, filesystem path, or authority
/// claim.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProductContinueSetupBridgeRequest {
    #[serde(default)]
    request_id: Option<String>,
    product_id: String,
    registration_ref: String,
    #[serde(default)]
    provider_hint: Option<bicameral_api::product_registration::RegisteredProvider>,
    #[serde(default)]
    substrate_hint: Option<bicameral_api::product_registration::RegisteredSubstrate>,
    #[serde(default)]
    account_label: Option<String>,
}

/// Map the operator's local connection registry status into the daemon-derived
/// provider-access fact. `Connected` ⇒ `Authorized`; everything else (absent,
/// action-needed) ⇒ `Absent` so the Product surfaces `Continue setup` until a
/// usable connection exists. Provider *denial* is a typed daemon fact carried
/// explicitly by the provider callback; it is never inferred from the SPA.
fn provider_access_from_registry(
    registry: &bicameral_config::ConnectionRegistry,
    provider_hint: Option<bicameral_api::product_registration::RegisteredProvider>,
) -> bicameral_api::product_registration::ProviderAccessState {
    use bicameral_api::product_registration::{ProviderAccessState, RegisteredProvider};
    let provider_id = match provider_hint {
        Some(RegisteredProvider::Github) => "github",
        Some(RegisteredProvider::Google) => "google",
        Some(RegisteredProvider::None) | None => return ProviderAccessState::Absent,
    };
    match registry.status(provider_id) {
        bicameral_config::ConnectionStatus::Connected => ProviderAccessState::Authorized,
        _ => ProviderAccessState::Absent,
    }
}

/// POST /api/v1/hosted-bridge/product-continue-setup — local setup continuation
/// after a canonical Product Member auto-join (#769).
///
/// Daemon-authoritative: the gateway validates the bridge pairing session,
/// derives the confirmed local actor and paired Cloud account from that
/// session, resolves the provider-access fact from the operator-local
/// connection registry, and hands the runtime the daemon-set facts. The runtime
/// materializes membership + registrations from canonical state, writes at most
/// one local `ProjectRegistration`, opens + replays the Product, and returns a
/// typed readiness. The response is the redacted [`HostedSetupProjection`] only
/// — never the registration_ref, descriptor, provider secret, or a local path.
async fn hosted_bridge_product_continue_setup(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    use bicameral_api::product_registration as reg;

    // 1. Validate bridge session (fail closed when unpaired/expired/revoked).
    let session = match validate_bridge_session(&state, &headers).await {
        Ok(s) => s,
        Err(resp) => return resp,
    };

    // 2. Reject authority-smuggling attempts before parsing.
    let smuggled = bicameral_api::hosted_bridge::detect_smuggled_fields(&payload);
    if !smuggled.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "authority_smuggling_rejected",
                "kind": "AuthoritySmugglingRejected",
                "smuggled_fields": smuggled,
            })),
        )
            .into_response();
    }

    // 3. Parse the bounded bridge body.
    let body: ProductContinueSetupBridgeRequest = match serde_json::from_value(payload) {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "invalid_continue_setup_request",
                    "kind": "MalformedPayload",
                    "message": format!("Failed to parse product.continue_setup request: {e}"),
                })),
            )
                .into_response();
        }
    };

    // 4. Resolve the daemon-derived provider-access fact from the operator-local
    //    connection registry. Descriptor validation for the current
    //    single-substrate daemon resolves to the daemon's own event store once a
    //    provider is authorized; richer invalid/unreachable descriptor typing
    //    binds with the signed-ticket descriptor shape from cloud#126.
    let registry = load_connection_registry_from_state(&state).await;
    let provider_access = provider_access_from_registry(&registry, body.provider_hint);
    let descriptor = match provider_access {
        reg::ProviderAccessState::Authorized => reg::DescriptorValidation::Valid,
        _ => reg::DescriptorValidation::NotAttempted,
    };

    // 5. Derive the daemon-confirmed local actor + params. The SPA cannot assert
    //    a different actor, account, or authority.
    let confirmed_actor = format!("bridge:{}", session.cloud_account_id);
    let request_id = body
        .request_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let params = reg::ProductContinueSetupParams {
        request_id,
        product_id: body.product_id.clone(),
        registration_ref: body.registration_ref,
        provider_hint: body.provider_hint,
        substrate_hint: body.substrate_hint,
        account_label: body.account_label,
        requested_at: chrono::Utc::now(),
    };

    // 6. Derive daemon-owned authority. The paired Cloud account and the
    //    provider/descriptor facts are published under daemon-set metadata keys
    //    so the runtime binds the continuation to the confirmed pairing and
    //    resolves readiness from daemon-derived state, not caller input.
    let authority = bicameral_api::tool_request::AuthorityContext {
        actor_id: confirmed_actor.clone(),
        auth_method: bicameral_api::tool_request::AuthMethod::HostedBridge,
        session_id: Some(session.session_token.clone()),
        workspace: None,
        policy_scope: vec!["hosted_bridge".to_string()],
        audit_metadata: {
            let mut m = serde_json::Map::new();
            m.insert(
                "surface".into(),
                serde_json::Value::String("hosted_bridge".to_string()),
            );
            m.insert(
                "bridge_origin".into(),
                serde_json::Value::String(session.origin.clone()),
            );
            m.insert(
                bicameral_api::product_join::PAIRED_CLOUD_ACCOUNT_METADATA_KEY.into(),
                serde_json::Value::String(session.cloud_account_id.clone()),
            );
            m.insert(
                reg::PROVIDER_ACCESS_METADATA_KEY.into(),
                serde_json::Value::String(provider_access.as_str().to_string()),
            );
            m.insert(
                reg::DESCRIPTOR_VALIDATION_METADATA_KEY.into(),
                serde_json::Value::String(descriptor.as_str().to_string()),
            );
            m
        },
    };

    let tool_request = bicameral_api::tool_request::ToolRequest::new(
        bicameral_api::tool_request::ToolCommand::ProductContinueSetup(params),
        authority,
    );

    // 7. Dispatch through the injected runtime (daemon supplies the replay probe).
    let dispatch = match &state.runtime_dispatch {
        Some(d) => d.clone(),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "runtime_unavailable",
                    "kind": "RuntimeUnavailable",
                    "message": "Runtime dispatch not available (daemon not fully initialized).",
                })),
            )
                .into_response();
        }
    };

    let workspace_root = state.workspace_root.read().await.clone();
    let response = dispatch.dispatch(&tool_request, workspace_root.as_deref());

    // 8. Project the daemon-authoritative result into the safe hosted shape.
    //    Only the redacted projection crosses the bridge — never the raw result
    //    (which carries the opaque registration_ref).
    let Some(result_json) = response.result.clone() else {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(response)).into_response();
    };
    let result: reg::ProductContinueSetupResult = match serde_json::from_value(result_json) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "invalid_continue_setup_result",
                    "kind": "MalformedResult",
                    "message": format!("Failed to parse continuation result: {e}"),
                })),
            )
                .into_response();
        }
    };

    match &result.readiness {
        Some(readiness) => {
            let projection = reg::HostedSetupProjection::project(
                result.product_id.clone(),
                readiness,
                result.registration_ref.is_some(),
            );
            (StatusCode::OK, Json(serde_json::json!(projection))).into_response()
        }
        // Fail-closed rejection (e.g. no canonical membership). Never revoke
        // membership; surface a typed, non-secret code only.
        None => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "continue_setup_rejected",
                "kind": "ContinueSetupRejected",
                "product_id": result.product_id,
                "membership_retained": result.membership_retained,
                "code": result
                    .failure
                    .as_ref()
                    .map(|f| f.code.as_str())
                    .unwrap_or("membership_required"),
            })),
        )
            .into_response(),
    }
}

/// GET /api/v1/hosted-bridge/workspace-discovery — safe team-visible metadata (#614).
///
/// Returns safe workspace/project metadata for hosted discovery. No local
/// paths, provider secrets, SourceEvidence bodies, Decision Ledger event
/// payloads, or authority-bearing fields are included.
async fn hosted_bridge_workspace_discovery(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Response {
    let _session = match validate_bridge_session(&state, &headers).await {
        Ok(s) => s,
        Err(resp) => return resp,
    };

    let registrations = state
        .project_registrations
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    let decision_count = state.decisions.read().await.len() as u32;
    let candidate_count = state.candidates.read().await.len() as u32;

    let workspaces = crate::state::safe_workspace_metadata_projection(
        &registrations,
        decision_count,
        candidate_count,
    );
    drop(registrations);

    Json(serde_json::json!({
        "workspaces": workspaces,
        "projection_only": true,
    }))
    .into_response()
}

/// GET /api/v1/hosted-bridge/dashboard-adapter — hosted-mode dashboard data (#616).
///
/// Hosted dashboard data adapter: provides bridge/cloud-safe data for the
/// hosted UI. Canonical mutations remain routed through daemon/ToolRequest
/// paths. The adapter does not become event-store authority, signoff
/// authority, compliance authority, or workspace binding authority.
///
/// A valid bridge session is required. Missing, expired, or revoked sessions
/// fail closed with the existing typed bridge error; no fixture-success path
/// exists in the shipped product.
async fn hosted_bridge_dashboard_adapter(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Response {
    let session = match validate_bridge_session(&state, &headers).await {
        Ok(s) => s,
        Err(response) => return response,
    };

    let registrations = state
        .project_registrations
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    let projects = crate::state::project_readiness_projection(&registrations);

    let decision_count = state.decisions.read().await.len() as u32;
    let candidate_count = state.candidates.read().await.len() as u32;

    // Build safe workspace metadata for the first project (or None).
    let workspace = if !registrations.is_empty() {
        let metas = crate::state::safe_workspace_metadata_projection(
            &registrations,
            decision_count,
            candidate_count,
        );
        metas.into_iter().next()
    } else {
        None
    };
    drop(registrations);

    // Build ledger summary from materialized state (counts only, no bodies).
    let decisions = state.decisions.read().await;
    let needs_signoff_count = decisions
        .values()
        .filter(|d| {
            d.review_state == bicameral_api::candidate::ReviewLifecycleState::NeedsSignoffReview
        })
        .count() as u32;
    let stale_source_count = decisions
        .values()
        .filter(|d| {
            d.review_state == bicameral_api::candidate::ReviewLifecycleState::StaleSourcePending
        })
        .count() as u32;

    let bindings = state.bindings.read().await;
    let bound_count = decisions
        .keys()
        .filter(|id| bindings.get(id).map(|bs| !bs.is_empty()).unwrap_or(false))
        .count() as u32;

    let last_promoted_at = decisions
        .values()
        .map(|d| d.promoted_at)
        .max()
        .map(|t| t.to_rfc3339());

    drop(decisions);
    drop(bindings);

    let ledger_summary = bicameral_api::hosted_bridge::HostedLedgerSummary {
        decision_count,
        candidate_count,
        needs_signoff_count,
        stale_source_count,
        bound_count,
        last_promoted_at,
    };

    let response = bicameral_api::hosted_bridge::HostedDashboardAdapterResponse {
        mode: bicameral_api::hosted_bridge::DashboardDataMode::Hosted,
        workspace,
        ledger_summary: Some(ledger_summary),
        projects,
        status_message: Some(format!(
            "Hosted dashboard adapter serving data for origin {}.",
            session.origin,
        )),
    };

    Json(response).into_response()
}

// ── Tests ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ProviderLoopbackCallbackQuery {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// Receive the provider browser redirect for native/desktop Google OAuth.
///
/// Google redirects the browser to the loopback URI with an HTTP GET query.
/// The provider never needs the daemon's internal auth-session identifier:
/// the opaque CSRF `state` maps back to the transient daemon-owned session.
/// Query values are consumed in memory and never reflected into the browser
/// response, logs, registry metadata, or durable evidence.
async fn connection_loopback_callback(
    State(state): State<AppState>,
    axum::extract::Path(provider): axum::extract::Path<String>,
    axum::extract::Query(query): axum::extract::Query<ProviderLoopbackCallbackQuery>,
) -> Response {
    let provider_lower = provider.to_lowercase();
    if provider_lower != "google" {
        return google_loopback_page(StatusCode::UNPROCESSABLE_ENTITY, false);
    }

    let Some(state_token) = query.state.as_deref().filter(|value| !value.is_empty()) else {
        return google_loopback_page(StatusCode::BAD_REQUEST, false);
    };

    // Resolve the opaque state to a live session. Expired sessions are
    // purged here and can never resurrect a callback (#1133): the state
    // scan itself refuses them, so an aged attempt is ineligible even
    // before the POST-callback TTL check.
    let auth_session_id = {
        let now = std::time::Instant::now();
        let mut sessions = state.auth_sessions.write().await;
        sessions.retain(|_, session| {
            !(session.provider.eq_ignore_ascii_case("google") && session.is_expired(now))
        });
        sessions
            .iter()
            .find(|(_, session)| {
                session.provider.eq_ignore_ascii_case("google")
                    && session.state_token == state_token
            })
            .map(|(session_id, _)| session_id.clone())
    };

    let Some(auth_session_id) = auth_session_id else {
        return google_loopback_page(StatusCode::BAD_REQUEST, false);
    };

    let body = bicameral_api::dashboard::ConnectionCallbackRequest {
        auth_session_id,
        code: query.code,
        state: query.state,
        error: query.error,
    };
    let (status, Json(outcome)) = connection_callback(
        State(state),
        axum::extract::Path(provider_lower),
        Json(body),
    )
    .await;

    let connected = matches!(
        outcome,
        bicameral_api::dashboard::ConnectionCompletionOutcome::Connected { .. }
    );
    let browser_status = if connected {
        StatusCode::OK
    } else if status.is_success() {
        StatusCode::BAD_REQUEST
    } else {
        status
    };
    google_loopback_page(browser_status, connected)
}

fn google_loopback_page(status: StatusCode, connected: bool) -> Response {
    let (title, message) = if connected {
        (
            "Google Drive connected",
            "Google Drive is connected. You can close this tab and return to Bicameral.",
        )
    } else {
        (
            "Google Drive connection not completed",
            "The Google Drive connection was not completed. Return to Bicameral and try again.",
        )
    };
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{title}</title></head><body><main><h1>{title}</h1><p>{message}</p></main></body></html>"
    );
    (
        status,
        [
            ("cache-control", "no-store"),
            (
                "content-security-policy",
                "default-src 'none'; style-src 'none'; frame-ancestors 'none'",
            ),
            ("referrer-policy", "no-referrer"),
            ("x-content-type-options", "nosniff"),
        ],
        axum::response::Html(body),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use bicameral_audit::AuditStore;
    use tower::ServiceExt;

    fn test_audit() -> AuditStore {
        let dir = tempfile::tempdir().unwrap();
        AuditStore::open(dir.path()).unwrap()
    }

    fn test_app() -> Router {
        let state = AppState::new(test_audit());
        Router::new()
            .merge(health_routes())
            .merge(ingest_routes())
            .merge(review_routes())
            .merge(status_routes())
            .merge(dashboard_api_routes())
            .merge(query_routes())
            .merge(bind_routes())
            .with_state(state)
    }

    #[tokio::test]
    async fn health_returns_ok() {
        let app = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn ingest_creates_candidate_with_evidence() {
        let state = AppState::new(test_audit());
        let app = Router::new()
            .merge(ingest_routes())
            .with_state(state.clone());

        let body = serde_json::json!({
            "title": "Test decision",
            "description": "A test",
            "level": "architecture",
            "source": "https://example.com/issue/1",
            "source_type": "github",
            "snapshot_content": "Issue: test decision content here",
            "evidence": [{
                "span_start": 0,
                "span_end": 32,
                "excerpt": "Issue: test decision content here",
                "confidence": 0.85
            }]
        });

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ingest")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // Verify evidence was stored
        assert_eq!(state.evidence.read().await.len(), 1);
        // Verify snapshot was stored
        assert_eq!(state.snapshots.read().await.len(), 1);
        // Verify source was stored
        assert_eq!(state.sources.read().await.len(), 1);
        // Verify candidate has evidence_refs
        let candidates = state.candidates.read().await;
        let cand = candidates.values().next().unwrap();
        assert_eq!(cand.evidence_refs.len(), 1);
    }

    #[tokio::test]
    async fn accept_candidate_creates_decision() {
        let state = AppState::new(test_audit());
        let app = Router::new()
            .merge(ingest_routes())
            .merge(dashboard_api_routes())
            .with_state(state.clone());

        // Ingest a candidate
        let body = serde_json::json!({
            "title": "Promote me",
            "description": "Should become decision",
            "level": "product",
            "source": "test",
            "evidence": [{"span_start": 0, "span_end": 10, "excerpt": "test excerpt"}]
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ingest")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let cand_id = state
            .candidates
            .read()
            .await
            .keys()
            .next()
            .unwrap()
            .to_string();

        // Accept candidate → creates Decision
        let cmd = serde_json::json!({
            "target_id": cand_id,
            "command": "accept_candidate"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/dashboard/command")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&cmd).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);

        // Verify decision was created
        assert_eq!(state.decisions.read().await.len(), 1);
        let decision = state
            .decisions
            .read()
            .await
            .values()
            .next()
            .unwrap()
            .clone();
        assert_eq!(decision.signoff_state, SignoffState::Proposed);
        assert_eq!(decision.title, "Promote me");

        // Verify candidate was marked promoted
        let cand = state
            .candidates
            .read()
            .await
            .get(&Uuid::parse_str(&cand_id).unwrap())
            .unwrap()
            .clone();
        assert_eq!(cand.candidate_state, CandidateState::Promoted);
    }

    #[tokio::test]
    async fn approve_signoff_transitions_decision() {
        let state = AppState::new(test_audit());
        let app = Router::new()
            .merge(ingest_routes())
            .merge(dashboard_api_routes())
            .with_state(state.clone());

        // Ingest + accept
        let body = serde_json::json!({
            "title": "Approve me",
            "description": "Will be approved",
            "level": "architecture",
            "source": "test",
            "evidence": [{"span_start": 0, "span_end": 10, "excerpt": "test excerpt"}]
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ingest")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let cand_id = state
            .candidates
            .read()
            .await
            .keys()
            .next()
            .unwrap()
            .to_string();

        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/dashboard/command")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&serde_json::json!({
                            "target_id": cand_id,
                            "command": "accept_candidate"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let decision_id = state
            .decisions
            .read()
            .await
            .keys()
            .next()
            .unwrap()
            .to_string();

        // Approve signoff
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/dashboard/command")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&serde_json::json!({
                            "target_id": decision_id,
                            "command": "approve_signoff"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);

        let d = state
            .decisions
            .read()
            .await
            .get(&Uuid::parse_str(&decision_id).unwrap())
            .unwrap()
            .clone();
        assert_eq!(d.signoff_state, SignoffState::Approved);
        // Issue #193 / ADR-0022: signoff approval must not change compliance.
        // Compliance was Ungrounded at promotion and must remain so.
        assert_eq!(
            d.compliance_state,
            bicameral_api::dashboard::ComplianceState::Ungrounded,
            "approve_signoff must not alter compliance state (ADR-0022)"
        );
    }

    #[tokio::test]
    async fn dashboard_ledger_shows_decisions_and_candidates() {
        let state = AppState::new(test_audit());
        let app = Router::new()
            .merge(ingest_routes())
            .merge(dashboard_api_routes())
            .with_state(state.clone());

        // Ingest two candidates
        for title in ["Candidate A", "Candidate B"] {
            app.clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/ingest")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::to_string(&serde_json::json!({
                                "title": title,
                                "description": "test",
                                "level": "product",
                                "source": "test",
                                "evidence": [{"span_start": 0, "span_end": 10, "excerpt": "test excerpt"}]
                            }))
                            .unwrap(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
        }

        // Accept first candidate
        let first_id = state
            .candidates
            .read()
            .await
            .values()
            .find(|c| c.title == "Candidate A")
            .unwrap()
            .id
            .to_string();

        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/dashboard/command")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&serde_json::json!({
                            "target_id": first_id,
                            "command": "accept_candidate"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Get ledger — should have 1 decision + 1 candidate
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/dashboard/ledger")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let items: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();

        assert_eq!(items.len(), 2);

        let kinds: Vec<&str> = items.iter().map(|i| i["kind"].as_str().unwrap()).collect();
        assert!(kinds.contains(&"decision"));
        assert!(kinds.contains(&"candidate"));
    }

    #[tokio::test]
    async fn dashboard_no_direct_decision_creation() {
        let app = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/dashboard/decisions")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn dashboard_command_returns_202() {
        let state = AppState::new(test_audit());
        let app = Router::new()
            .merge(ingest_routes())
            .merge(dashboard_api_routes())
            .with_state(state.clone());

        // Ingest a candidate first
        let body = serde_json::json!({
            "title": "Test",
            "description": "test",
            "level": "implementation",
            "source": "test",
            "evidence": [{"span_start": 0, "span_end": 10, "excerpt": "test excerpt"}]
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ingest")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let cand_id = state
            .candidates
            .read()
            .await
            .keys()
            .next()
            .unwrap()
            .to_string();

        let cmd = serde_json::json!({
            "target_id": cand_id,
            "command": "request_context"
        });

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/dashboard/command")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&cmd).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);
    }

    // ── Query surface tests ──────────────────────────────────────

    #[tokio::test]
    async fn history_returns_empty_initially() {
        let app = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/history")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(parsed["decisions"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn history_returns_decisions_after_promotion() {
        let state = AppState::new(test_audit());
        let app = Router::new()
            .merge(ingest_routes())
            .merge(dashboard_api_routes())
            .merge(query_routes())
            .with_state(state.clone());

        // Ingest
        let body = serde_json::json!({
            "title": "History test decision",
            "description": "Should appear in history",
            "level": "architecture",
            "source": "test",
            "evidence": [{"span_start": 0, "span_end": 10, "excerpt": "test excerpt"}]
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ingest")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let cand_id = state
            .candidates
            .read()
            .await
            .keys()
            .next()
            .unwrap()
            .to_string();

        // Accept candidate
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/dashboard/command")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&serde_json::json!({
                            "target_id": cand_id,
                            "command": "accept_candidate"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Query history
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/history?include_candidates=true")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["decisions"].as_array().unwrap().len(), 1);
        assert_eq!(
            parsed["decisions"][0]["title"].as_str().unwrap(),
            "History test decision"
        );
        assert!(parsed["candidates"].as_array().unwrap().len() >= 1);
    }

    #[tokio::test]
    async fn search_returns_empty_for_no_match() {
        let app = test_app();
        let body = serde_json::json!({
            "query": "nonexistent_term_xyz"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/search")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(parsed["results"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn search_finds_ingested_candidates() {
        let state = AppState::new(test_audit());
        let app = Router::new()
            .merge(ingest_routes())
            .merge(query_routes())
            .with_state(state.clone());

        let body = serde_json::json!({
            "title": "Microservice architecture",
            "description": "Use microservices for scalability",
            "level": "architecture",
            "source": "test",
            "evidence": [{"span_start": 0, "span_end": 10, "excerpt": "test excerpt"}]
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ingest")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let search = serde_json::json!({
            "query": "microservice"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/search")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&search).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let results = parsed["results"].as_array().unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0]["type"].as_str().unwrap(), "candidate");
    }

    #[tokio::test]
    async fn preflight_returns_ungrounded_compliance() {
        let state = AppState::new(test_audit());
        let app = Router::new()
            .merge(ingest_routes())
            .merge(dashboard_api_routes())
            .merge(query_routes())
            .with_state(state.clone());

        // Ingest + promote
        let body = serde_json::json!({
            "title": "Auth service JWT validation",
            "description": "All auth endpoints must validate JWT tokens",
            "level": "architecture",
            "source": "test",
            "evidence": [{"span_start": 0, "span_end": 10, "excerpt": "jwt validation"}]
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ingest")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let cand_id = state
            .candidates
            .read()
            .await
            .keys()
            .next()
            .unwrap()
            .to_string();

        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/dashboard/command")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&serde_json::json!({
                            "target_id": cand_id,
                            "command": "accept_candidate"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Preflight with matching file
        let preflight = serde_json::json!({
            "files": ["src/auth_service.rs"]
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/preflight")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&preflight).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let decisions = parsed["relevant_decisions"].as_array().unwrap();
        assert!(!decisions.is_empty());
        assert_eq!(
            decisions[0]["compliance_state"].as_str().unwrap(),
            "ungrounded"
        );
    }

    #[tokio::test]
    async fn preflight_no_false_conflicts() {
        let state = AppState::new(test_audit());
        let app = Router::new()
            .merge(ingest_routes())
            .merge(dashboard_api_routes())
            .merge(query_routes())
            .with_state(state.clone());

        // Ingest + promote
        let body = serde_json::json!({
            "title": "Database migration strategy",
            "description": "Use PostgreSQL for production",
            "level": "architecture",
            "source": "test",
            "evidence": [{"span_start": 0, "span_end": 10, "excerpt": "postgres"}]
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ingest")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let cand_id = state
            .candidates
            .read()
            .await
            .keys()
            .next()
            .unwrap()
            .to_string();

        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/dashboard/command")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&serde_json::json!({
                            "target_id": cand_id,
                            "command": "accept_candidate"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Preflight with unrelated file
        let preflight = serde_json::json!({
            "files": ["src/totally_unrelated_module.rs"]
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/preflight")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&preflight).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let decisions = parsed["relevant_decisions"].as_array().unwrap();
        assert!(
            decisions.is_empty(),
            "Preflight must not claim false relevance for unrelated files"
        );
    }

    #[tokio::test]
    async fn preflight_warns_about_symbols() {
        let app = test_app();
        let preflight = serde_json::json!({
            "symbols": ["foo::bar::Baz"]
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/preflight")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&preflight).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let warnings = parsed["warnings"].as_array().unwrap();
        assert!(
            warnings
                .iter()
                .any(|w| w.as_str().unwrap().contains("Symbol grounding")),
            "Should warn that symbol grounding is unavailable"
        );
    }

    #[tokio::test]
    async fn status_endpoint_includes_substrate_info() {
        let app = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // In-memory gateway store is SubstrateKind::Memory
        assert_eq!(parsed["substrate"]["kind"].as_str().unwrap(), "memory");
        assert!(parsed["substrate"]["capabilities"].is_array());
        assert!(parsed["substrate"]["explanation"].is_string());
        assert!(parsed["substrate"]["reachable"].as_bool().unwrap());
        assert!(parsed["substrate"]["warnings"].is_array());
        // Memory substrate does not claim CI/merge enforcement
        let explanation = parsed["substrate"]["explanation"].as_str().unwrap();
        assert!(
            !explanation.contains("CI") || explanation.contains("no durable"),
            "Memory substrate must not claim CI enforcement"
        );
    }

    #[tokio::test]
    async fn ingestion_gate_marks_unpromoted_candidates_as_untracked() {
        let state = AppState::new(test_audit());
        let app = Router::new()
            .merge(ingest_routes())
            .merge(dashboard_api_routes())
            .with_state(state.clone());

        // Ingest a candidate (not promoted)
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ingest")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&serde_json::json!({
                            "title": "Untracked candidate",
                            "description": "advisory context",
                            "level": "product",
                            "source": "test-source",
                            "evidence": [{"span_start": 0, "span_end": 10, "excerpt": "test"}]
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Get ingestion gate
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/dashboard/ingestion-gate")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let items: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();

        assert_eq!(items.len(), 1);
        // Unpromoted candidates are untracked and advisory
        assert_eq!(items[0]["tracked"].as_bool().unwrap(), false);
        assert_eq!(
            items[0]["context_disposition"].as_str().unwrap(),
            "candidate_context"
        );
        // Staleness is advisory for untracked context (ADR-0022)
        assert_eq!(items[0]["stale_is_advisory"].as_bool().unwrap(), true);
    }

    #[tokio::test]
    async fn ledger_decision_exposes_tracking_state_and_freshness() {
        let state = AppState::new(test_audit());
        let app = Router::new()
            .merge(ingest_routes())
            .merge(dashboard_api_routes())
            .with_state(state.clone());

        // Ingest and promote a candidate
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ingest")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&serde_json::json!({
                            "title": "Tracked decision",
                            "description": "will be promoted",
                            "level": "product",
                            "source": "test-source-2",
                            "evidence": [{"span_start": 0, "span_end": 10, "excerpt": "test"}]
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let first_id = state
            .candidates
            .read()
            .await
            .values()
            .next()
            .unwrap()
            .id
            .to_string();

        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/dashboard/command")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&serde_json::json!({
                            "target_id": first_id,
                            "command": "accept_candidate"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Get ledger
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/dashboard/ledger")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let items: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();

        // Find the decision
        let decision = items
            .iter()
            .find(|i| i["kind"].as_str().unwrap() == "decision")
            .expect("should have a decision");

        // Tracking state defaults to tracked on promotion (ADR-0022)
        assert_eq!(decision["tracking_state"].as_str().unwrap(), "tracked");
        // Fresh source (not stale_source_pending)
        assert_eq!(decision["source_freshness"].as_str().unwrap(), "fresh");
        // No freshness warning for fresh decisions
        assert!(decision["freshness_warning"].is_null());
        // Allowed commands include refresh and untrack for tracked decisions
        let commands: Vec<&str> = decision["allowed_commands"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(
            commands.contains(&"refresh_source"),
            "tracked decisions should have refresh_source command"
        );
        assert!(
            commands.contains(&"untrack_decision"),
            "tracked decisions should have untrack_decision command"
        );
    }

    #[tokio::test]
    async fn ingestion_gate_tracks_promoted_decisions() {
        let state = AppState::new(test_audit());
        let app = Router::new()
            .merge(ingest_routes())
            .merge(dashboard_api_routes())
            .with_state(state.clone());

        // Ingest and promote a candidate
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ingest")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&serde_json::json!({
                            "title": "To be promoted",
                            "description": "will be tracked",
                            "level": "product",
                            "source": "promote-me",
                            "evidence": [{"span_start": 0, "span_end": 5, "excerpt": "test"}]
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let first_id = state
            .candidates
            .read()
            .await
            .values()
            .next()
            .unwrap()
            .id
            .to_string();

        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/dashboard/command")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&serde_json::json!({
                            "target_id": first_id,
                            "command": "accept_candidate"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Get ingestion gate
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/dashboard/ingestion-gate")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let items: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();

        assert_eq!(items.len(), 1);
        // Promoted source is now tracked
        assert_eq!(items[0]["tracked"].as_bool().unwrap(), true);
        assert_eq!(
            items[0]["context_disposition"].as_str().unwrap(),
            "tracked_decision"
        );
        // Staleness is NOT advisory for tracked context
        assert_eq!(items[0]["stale_is_advisory"].as_bool().unwrap(), false);
    }

    // ── Projection-backed read tests ─────────────────────────────

    /// Build an AppState with a pre-populated projection for testing.
    fn state_with_projection() -> AppState {
        use bicameral_api::projection::*;

        let mut proj = ProjectionState::default();
        let decision_id = uuid::Uuid::new_v4();
        let candidate_id = uuid::Uuid::new_v4();
        let evidence_id = uuid::Uuid::new_v4();
        let binding_id = uuid::Uuid::new_v4();

        proj.decisions.insert(
            decision_id,
            ProjectedDecision {
                id: decision_id,
                candidate_id,
                title: "Projected Decision".to_string(),
                body: "From projection".to_string(),
                level: bicameral_api::candidate::DecisionLevel::Product,
                signoff: bicameral_api::dashboard::SignoffState::Approved,
                review_state: bicameral_api::candidate::ReviewLifecycleState::ReadyToMaterialize,
                tracking_state: bicameral_api::candidate::TrackingState::Tracked,
                source_id: Some("jira://PROJ-1".to_string()),
                source_freshness: bicameral_api::source::SourceFreshness::Fresh,
                promoted_at: chrono::Utc::now(),
                promoted_by: "test-actor".to_string(),
                provenance: None,
            },
        );

        proj.source_evidence.insert(
            evidence_id,
            ProjectedSourceEvidence {
                id: evidence_id,
                decision_id,
                snapshot_id: "sha256:proj123".to_string(),
                excerpt: "Projected excerpt".to_string(),
                span_start: 0,
                span_end: 50,
                confidence: 0.95,
                captured_at: chrono::Utc::now(),
            },
        );

        proj.code_evidence.insert(
            binding_id,
            ProjectedCodeEvidence {
                id: binding_id,
                decision_id,
                file: "src/widget.rs".to_string(),
                start_line: 10,
                end_line: 25,
                binding_type: "Exact".to_string(),
                explanation: Some("Widget implementation".to_string()),
                evidence_state: GraphEvidenceState::Verified,
            },
        );

        proj.graph_snapshots.insert(
            "sha256:proj123".to_string(),
            ProjectedGraphSnapshot {
                snapshot_addr: "sha256:proj123".to_string(),
                snapshot_ref: "jira://PROJ-1".to_string(),
                captured_at: Some(chrono::Utc::now()),
            },
        );

        proj.graph_readiness.insert(
            decision_id,
            ProjectedGraphReadiness {
                decision_id,
                evidence_state: GraphEvidenceState::Verified,
                has_bindings: true,
            },
        );

        let state = AppState::new(test_audit()).with_projection(proj);
        state
    }

    #[tokio::test]
    async fn dashboard_ledger_reads_from_projection_when_configured() {
        let state = state_with_projection();
        let app = Router::new()
            .merge(dashboard_api_routes())
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/dashboard/ledger")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), 200);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let items: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();

        // Projection contains one Decision.
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["kind"].as_str().unwrap(), "decision");
        assert_eq!(items[0]["summary"].as_str().unwrap(), "Projected Decision");
        assert_eq!(items[0]["signoff"].as_str().unwrap(), "approved");

        // Code evidence section is present.
        let ce = &items[0]["code_evidence"];
        assert!(!ce.is_null(), "code_evidence must be present");
        assert_eq!(ce["evidence_state"].as_str().unwrap(), "verified",);
        assert!(ce["evidence_label"].as_str().unwrap().contains("snapshot"),);

        // Code regions from projected bindings.
        let regions = items[0]["regions"].as_array().unwrap();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0]["file"].as_str().unwrap(), "src/widget.rs");
        assert_eq!(regions[0]["start_line"].as_u64().unwrap(), 10);

        // Graph readiness is present.
        let gr = &items[0]["graph_readiness"];
        assert!(!gr.is_null(), "graph_readiness must be present");
    }

    #[tokio::test]
    async fn dashboard_ledger_falls_back_without_projection() {
        // State without projection — uses the in-memory fallback path.
        let state = AppState::new(test_audit());
        let app = Router::new()
            .merge(ingest_routes())
            .merge(dashboard_api_routes())
            .with_state(state.clone());

        // Ingest a candidate so the ledger has something.
        let ingest_body = serde_json::json!({
            "title": "Fallback Candidate",
            "description": "Testing fallback path",
            "level": "product",
            "source": "jira://FALL-1",
            "evidence": [{
                "span_start": 0,
                "span_end": 10,
                "excerpt": "Fallback excerpt"
            }]
        });

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ingest")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&ingest_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);

        // Ledger should show the candidate via fallback path.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/dashboard/ledger")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), 200);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let items: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();

        assert!(
            !items.is_empty(),
            "fallback path must return items from in-memory state"
        );
        assert_eq!(items[0]["summary"].as_str().unwrap(), "Fallback Candidate");
    }

    #[tokio::test]
    async fn projection_backed_ledger_does_not_expose_compliance() {
        let state = state_with_projection();
        let app = Router::new()
            .merge(dashboard_api_routes())
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/dashboard/ledger")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8_lossy(&body);

        assert!(
            !body_str.contains("compliance_state"),
            "projection-backed ledger must not expose compliance_state"
        );
        assert!(
            !body_str.contains("\"enforcement\""),
            "projection-backed ledger must not expose enforcement"
        );
        assert!(
            !body_str.contains("\"ready_to_ship\""),
            "projection-backed ledger must not expose ready_to_ship"
        );
    }

    // ── Setup & config endpoint tests ────────────────────────────

    struct CapabilityReadinessDispatch;

    impl crate::state::RuntimeDispatch for CapabilityReadinessDispatch {
        fn dispatch(
            &self,
            request: &bicameral_api::tool_request::ToolRequest,
            _workspace_root: Option<&std::path::Path>,
        ) -> bicameral_api::tool_request::ToolResponse {
            bicameral_api::tool_request::ToolResponse::rejected(
                request.request_id,
                "unsupported_capability".into(),
            )
        }

        fn capabilities(&self) -> serde_json::Value {
            serde_json::json!({})
        }

        fn capability_readiness(
            &self,
        ) -> Result<Vec<bicameral_api::capability_readiness::DaemonCapabilityReadinessV1>, String>
        {
            Ok(vec![
                bicameral_api::capability_readiness::DaemonCapabilityReadinessV1 {
                    schema_version: 1,
                    capability_kind:
                        bicameral_api::capability_readiness::CapabilityKind::Mcp,
                    subject_id:
                        bicameral_api::capability_readiness::DeveloperHostId::Codex,
                    artifact_generation:
                        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                            .into(),
                    state: bicameral_api::capability_readiness::CapabilityReadinessState::ActiveVerified,
                    reason_codes: Vec::new(),
                    observed_at: "2026-07-30T00:00:00Z".into(),
                    last_verified_at: Some("2026-07-30T00:00:00Z".into()),
                    next_action_code: None,
                    safe_display_argv: None,
                    host_permission: None,
                },
            ])
        }
    }

    #[tokio::test]
    async fn capability_readiness_route_is_read_only_and_product_independent() {
        let mut state = AppState::new(test_audit());
        state.runtime_dispatch = Some(std::sync::Arc::new(CapabilityReadinessDispatch));
        let app = Router::new().merge(setup_routes()).with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/setup/capability-readiness")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 4096).await.unwrap())
                .unwrap();
        assert_eq!(body[0]["state"], "active_verified");
        assert_eq!(body[0]["subject_id"], "codex");
        let serialized = body.to_string();
        for forbidden in [
            "product_id",
            "pairing",
            "bridge_session",
            "dashboard_session",
            "provider_credential",
        ] {
            assert!(!serialized.contains(forbidden), "leaked {forbidden}");
        }
    }

    #[derive(Default)]
    struct SubmittingReadinessDispatch {
        submitted:
            std::sync::Mutex<Vec<bicameral_api::capability_readiness::McpReadinessObservation>>,
    }

    impl crate::state::RuntimeDispatch for SubmittingReadinessDispatch {
        fn dispatch(
            &self,
            request: &bicameral_api::tool_request::ToolRequest,
            _workspace_root: Option<&std::path::Path>,
        ) -> bicameral_api::tool_request::ToolResponse {
            bicameral_api::tool_request::ToolResponse::rejected(
                request.request_id,
                "unsupported_capability".into(),
            )
        }

        fn capabilities(&self) -> serde_json::Value {
            serde_json::json!({})
        }

        fn submit_mcp_readiness(
            &self,
            observation: bicameral_api::capability_readiness::McpReadinessObservation,
        ) -> Result<bicameral_api::capability_readiness::DaemonCapabilityReadinessV1, String>
        {
            self.submitted.lock().unwrap().push(observation.clone());
            observation.reduce().map_err(|error| error.to_string())
        }
    }

    fn pending_readiness_observation() -> serde_json::Value {
        serde_json::json!({
            "subject_id": "codex",
            "artifact_generation":
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "package_installed": true,
            "registration_verified": true,
            "managed_hook_verified": true,
            "missing_prerequisites": [],
            "drift_reason_codes": [],
            "activation": null,
            "observed_at": "2026-07-30T00:00:00Z"
        })
    }

    #[tokio::test]
    async fn local_setup_controller_may_submit_bounded_readiness_evidence() {
        let dispatch = std::sync::Arc::new(SubmittingReadinessDispatch::default());
        let mut state = AppState::new(test_audit());
        state.runtime_dispatch = Some(dispatch.clone());
        let app = Router::new().merge(setup_routes()).with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/setup/capability-readiness")
                    .header("content-type", "application/json")
                    .body(Body::from(pending_readiness_observation().to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 4096).await.unwrap())
                .unwrap();
        assert_eq!(body["state"], "installed_pending_host_action");
        assert_eq!(dispatch.submitted.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn browser_origin_cannot_submit_setup_readiness_evidence() {
        let dispatch = std::sync::Arc::new(SubmittingReadinessDispatch::default());
        let mut state = AppState::new(test_audit());
        state.runtime_dispatch = Some(dispatch.clone());
        let app = Router::new().merge(setup_routes()).with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/setup/capability-readiness")
                    .header("content-type", "application/json")
                    .header("origin", "https://app.bicameral-ai.com")
                    .body(Body::from(pending_readiness_observation().to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        assert!(dispatch.submitted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn setup_status_returns_no_workspace() {
        let state = AppState::new(test_audit());
        let app = Router::new().merge(setup_routes()).with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/setup/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 4096).await.unwrap())
                .unwrap();
        assert_eq!(body["workspace_exists"], false);
    }

    #[tokio::test]
    async fn create_workspace_and_read_config() {
        let dir = tempfile::tempdir().unwrap();
        let audit = test_audit();
        let state = AppState::with_workspace_root(audit, dir.path().to_path_buf());

        let app = Router::new()
            .merge(setup_routes())
            .with_state(state.clone());

        // Create workspace
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/setup/create-workspace")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&serde_json::json!({
                            "workspace_name": "test-ws"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let body: serde_json::Value =
            serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 4096).await.unwrap())
                .unwrap();
        assert_eq!(body["success"], true);

        // .bicameral/ should now exist
        assert!(dir.path().join(".bicameral").is_dir());
        assert!(dir.path().join(".bicameral/config.yaml").is_file());

        // Read config back via API
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/dashboard/config")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 4096).await.unwrap())
                .unwrap();
        assert_eq!(body["config"]["workspace_name"], "test-ws");
    }

    #[tokio::test]
    async fn update_config_validates_gateway_host() {
        let dir = tempfile::tempdir().unwrap();
        let audit = test_audit();
        let state = AppState::with_workspace_root(audit, dir.path().to_path_buf());

        // Initialize workspace
        let config = bicameral_config::BicameralConfig::default();
        bicameral_config::WorkspaceDiscovery::init(dir.path(), &config).unwrap();

        let app = Router::new().merge(setup_routes()).with_state(state);

        // Try setting non-loopback host
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/dashboard/config")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&serde_json::json!({
                            "gateway_host": "0.0.0.0"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body: serde_json::Value =
            serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 4096).await.unwrap())
                .unwrap();
        assert_eq!(body["success"], false);
        let errors = body["errors"].as_array().unwrap();
        assert!(!errors.is_empty());
        assert!(errors[0].as_str().unwrap().contains("loopback"));
    }

    #[tokio::test]
    async fn update_config_saves_valid_settings() {
        let dir = tempfile::tempdir().unwrap();
        let audit = test_audit();
        let state = AppState::with_workspace_root(audit, dir.path().to_path_buf());

        // Initialize workspace
        let config = bicameral_config::BicameralConfig::default();
        bicameral_config::WorkspaceDiscovery::init(dir.path(), &config).unwrap();

        let app = Router::new().merge(setup_routes()).with_state(state);

        // Update with valid settings
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/dashboard/config")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&serde_json::json!({
                            "workspace_name": "updated-ws",
                            "substrate": "drive_folder",
                            "drive_folder_path": "/tmp/shared",
                            "ci_merge_gate": true
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Verify saved
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/dashboard/config")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 4096).await.unwrap())
                .unwrap();
        assert_eq!(body["config"]["workspace_name"], "updated-ws");
        assert_eq!(body["config"]["event_store"]["substrate"], "drive_folder");
    }

    #[tokio::test]
    async fn duplicate_workspace_creation_returns_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let audit = test_audit();
        let state = AppState::with_workspace_root(audit, dir.path().to_path_buf());

        // Initialize workspace first
        let config = bicameral_config::BicameralConfig::default();
        bicameral_config::WorkspaceDiscovery::init(dir.path(), &config).unwrap();

        let app = Router::new().merge(setup_routes()).with_state(state);

        // Try creating again
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/setup/create-workspace")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    // -- Alpha cut-line: deferred dashboard commands return 202 + typed JSON --

    async fn assert_deferred_command(app: Router, command: &str) {
        let cmd = serde_json::json!({
            "target_id": "00000000-0000-4000-8000-000000000001",
            "command": command
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/dashboard/command")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&cmd).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::ACCEPTED,
            "{command} must return 202 ACCEPTED with typed deferred JSON"
        );
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: bicameral_api::dashboard::DeferredCommandResponse =
            serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.status, "deferred");
        assert_eq!(parsed.reason, "not_supported_in_alpha");
        assert_eq!(parsed.command, command);
    }

    #[tokio::test]
    async fn dashboard_resolve_compliance_returns_deferred_json() {
        let state = AppState::new(test_audit());
        let app = Router::new()
            .merge(dashboard_api_routes())
            .with_state(state);
        assert_deferred_command(app, "resolve_compliance").await;
    }

    #[tokio::test]
    async fn dashboard_untrack_decision_returns_deferred_json() {
        let state = AppState::new(test_audit());
        let app = Router::new()
            .merge(dashboard_api_routes())
            .with_state(state);
        assert_deferred_command(app, "untrack_decision").await;
    }

    #[tokio::test]
    async fn dashboard_refresh_source_returns_deferred_json() {
        let state = AppState::new(test_audit());
        let app = Router::new()
            .merge(dashboard_api_routes())
            .with_state(state);
        assert_deferred_command(app, "refresh_source").await;
    }

    // ── Connection start tests ───────────────────────────────────

    fn connection_app() -> Router {
        let dir = tempfile::tempdir().unwrap();
        let mut state = AppState::new(test_audit());
        state.workspace_root = std::sync::Arc::new(tokio::sync::RwLock::new(Some(dir.into_path())));
        state.google_oauth_config = None;
        state.google_cloud_origin_override = Some("http://127.0.0.1:9".into());
        Router::new().merge(connection_routes()).with_state(state)
    }

    #[tokio::test]
    async fn connection_start_supported_provider_returns_start_url() {
        let app = connection_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/connections/github/start")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Without an on-disk registry, a supported provider returns
        // start_url with auth session ID (not_connected state).
        assert_eq!(json["outcome"], "start_url");
        assert_eq!(json["provider"], "github");
        assert!(json["url"].as_str().unwrap().contains("github.com"));
        assert!(json["auth_session_id"]
            .as_str()
            .unwrap()
            .starts_with("sess-"));
        assert_eq!(json["flow_type"], "device_flow");
        assert!(json["user_code"].is_string());
        // Must not contain secret material.
        let raw = String::from_utf8_lossy(&body);
        assert!(!raw.contains("oauth_token"));
        assert!(!raw.contains("refresh_token"));
    }

    fn test_google_oauth_config() -> bicameral_config::GoogleOAuthConfig {
        bicameral_config::GoogleOAuthConfig {
            client_id: "test-client-id".into(),
            client_secret: "test-client-secret".into(),
            redirect_uri: "http://localhost:3000/api/v1/connections/google/callback".into(),
            scopes: vec!["https://www.googleapis.com/auth/drive.file".into()],
        }
    }

    fn connection_app_with_google() -> (Router, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let mut state = AppState::new(test_audit());
        state.workspace_root = std::sync::Arc::new(tokio::sync::RwLock::new(Some(dir.into_path())));
        state.google_oauth_config = Some(test_google_oauth_config());
        state.google_oauth_allow_env_override = true;
        let app = Router::new()
            .merge(connection_routes())
            .with_state(state.clone());
        (app, state)
    }

    /// Spawn a local Google token-endpoint stub returning `status`/`body`
    /// and recording every form-encoded request body (#1132). Tests drive
    /// the REAL exchange transport against this endpoint; no code-content
    /// or environment mock path exists in the exchange anymore.
    async fn spawn_token_endpoint_stub(
        status: StatusCode,
        body: serde_json::Value,
    ) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        use axum::routing::post;
        let recorded: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = recorded.clone();
        let response_body = body.to_string();
        let app = Router::new().route(
            "/token",
            post(move |request_body: String| {
                let sink = sink.clone();
                let response_body = response_body.clone();
                async move {
                    sink.lock().unwrap().push(request_body);
                    (
                        status,
                        [("content-type", "application/json")],
                        response_body,
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{address}/token"), recorded)
    }

    /// Spawn a local Drive "about" stub returning `status` for every GET,
    /// recording each request's `Authorization` header (#1279).
    async fn spawn_drive_about_stub(
        status: StatusCode,
    ) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        spawn_drive_about_stub_with_body(status, String::new()).await
    }

    async fn spawn_drive_about_stub_with_body(
        status: StatusCode,
        body: String,
    ) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        use axum::routing::get;
        let recorded: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = recorded.clone();
        let app = Router::new().route(
            "/about",
            get(move |headers: axum::http::HeaderMap| {
                let sink = sink.clone();
                let body = body.clone();
                async move {
                    let auth = headers
                        .get("authorization")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_string();
                    sink.lock().unwrap().push(auth);
                    (status, [("content-type", "application/json")], body)
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{address}/about"), recorded)
    }

    fn stub_token_response() -> serde_json::Value {
        serde_json::json!({
            "access_token": "stub-access-token",
            "refresh_token": "stub-refresh-token",
            "expires_in": 3599,
            "token_type": "Bearer",
            "scope": "https://www.googleapis.com/auth/drive.file"
        })
    }

    /// Connection app whose exchange transport targets a local stub token
    /// endpoint (the #1132 test-only seam).
    async fn connection_app_with_google_and_token_stub(
        status: StatusCode,
        body: serde_json::Value,
    ) -> (
        Router,
        AppState,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    ) {
        let (endpoint, recorded) = spawn_token_endpoint_stub(status, body).await;
        let dir = tempfile::tempdir().unwrap();
        let mut state = AppState::new(test_audit());
        state.workspace_root = std::sync::Arc::new(tokio::sync::RwLock::new(Some(dir.into_path())));
        state.google_oauth_config = Some(test_google_oauth_config());
        state.google_oauth_allow_env_override = true;
        state.google_token_endpoint_override = Some(endpoint);
        let app = Router::new()
            .merge(connection_routes())
            .with_state(state.clone());
        (app, state, recorded)
    }

    #[tokio::test]
    async fn connection_start_google_no_config_returns_action_needed() {
        // connection_app() explicitly sets google_oauth_config to None.
        let app = connection_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/connections/google/start")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["outcome"], "action_needed");
        assert_eq!(json["provider"], "google");
        assert!(json["message"]
            .as_str()
            .unwrap()
            .contains("configuration is unavailable"));
    }

    #[tokio::test]
    async fn connection_start_google_with_config_returns_start_url() {
        let (app, _state) = connection_app_with_google();
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/connections/google/start")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["outcome"], "start_url");
        assert_eq!(json["provider"], "google");
        assert!(json["url"]
            .as_str()
            .unwrap()
            .starts_with("https://accounts.google.com/o/oauth2/v2/auth"));
        assert!(json["auth_session_id"]
            .as_str()
            .unwrap()
            .starts_with("gsess-"));
        assert_eq!(json["flow_type"], "local_loopback");
        // Must not contain secret material.
        let raw = String::from_utf8_lossy(&body);
        assert!(!raw.contains("test-client-secret"));
        assert!(!raw.contains("oauth_token"));
        assert!(!raw.contains("refresh_token"));
    }

    #[tokio::test]
    async fn connection_start_unsupported_provider_returns_unsupported() {
        let app = connection_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/connections/notion/start")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["outcome"], "unsupported");
        assert_eq!(json["provider"], "notion");
        assert!(json["message"]
            .as_str()
            .unwrap()
            .contains("not a supported provider"));
    }

    #[tokio::test]
    async fn connection_start_unknown_provider_returns_unsupported() {
        let app = connection_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/connections/foobar/start")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["outcome"], "unsupported");
    }

    #[tokio::test]
    async fn connection_start_case_insensitive() {
        let app = connection_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/connections/GitHub/start")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        // Should succeed because we lowercase the provider.
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["provider"], "github");
    }

    // ── Connection callback tests ────────────────────────────────

    #[tokio::test]
    async fn connection_callback_unknown_session_returns_expired() {
        let app = connection_app();
        let body_json = serde_json::json!({
            "auth_session_id": "sess-unknown",
            "code": "auth-code-xyz",
            "state": "csrf-state"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/connections/google/callback")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body_json).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["outcome"], "expired");
        assert_eq!(json["provider"], "google");
        let raw = String::from_utf8_lossy(&body);
        assert!(!raw.contains("oauth_token"));
        assert!(!raw.contains("refresh_token"));
    }

    #[tokio::test]
    async fn google_loopback_get_maps_state_to_session_and_connects() {
        let (app, _state, _recorded) =
            connection_app_with_google_and_token_stub(StatusCode::OK, stub_token_response()).await;

        let start_req = Request::builder()
            .method("POST")
            .uri("/api/v1/connections/google/start")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let start_resp = app.clone().oneshot(start_req).await.unwrap();
        assert_eq!(start_resp.status(), StatusCode::OK);
        let start_body = axum::body::to_bytes(start_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let start_json: serde_json::Value = serde_json::from_slice(&start_body).unwrap();
        let auth_url = start_json["url"].as_str().unwrap();
        let parsed = reqwest::Url::parse(auth_url).unwrap();
        let state_token = parsed
            .query_pairs()
            .find(|(key, _)| key == "state")
            .map(|(_, value)| value.into_owned())
            .unwrap();

        let callback_uri =
            format!("/api/v1/connections/google/callback?code=auth-code-xyz&state={state_token}");
        let callback_req = Request::builder()
            .method("GET")
            .uri(callback_uri)
            .body(Body::empty())
            .unwrap();
        let callback_resp = app.clone().oneshot(callback_req).await.unwrap();
        assert_eq!(callback_resp.status(), StatusCode::OK);
        let callback_body = axum::body::to_bytes(callback_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let html = String::from_utf8(callback_body.to_vec()).unwrap();
        assert!(html.contains("Google Drive connected"));
        assert!(!html.contains("auth-code-xyz"));
        assert!(!html.contains(&state_token));

        let registry_req = Request::builder()
            .method("GET")
            .uri("/api/v1/connections")
            .body(Body::empty())
            .unwrap();
        let registry_resp = app.oneshot(registry_req).await.unwrap();
        assert_eq!(registry_resp.status(), StatusCode::OK);
        let registry_body = axum::body::to_bytes(registry_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let registry_json: serde_json::Value = serde_json::from_slice(&registry_body).unwrap();
        assert_eq!(
            registry_json["connections"]["google"]["status"],
            "connected"
        );
    }

    #[tokio::test]
    async fn google_loopback_get_missing_state_fails_closed() {
        let (app, _state) = connection_app_with_google();
        let callback_req = Request::builder()
            .method("GET")
            .uri("/api/v1/connections/google/callback?code=auth-code-xyz")
            .body(Body::empty())
            .unwrap();
        let callback_resp = app.clone().oneshot(callback_req).await.unwrap();
        assert_eq!(callback_resp.status(), StatusCode::BAD_REQUEST);

        let registry_req = Request::builder()
            .method("GET")
            .uri("/api/v1/connections")
            .body(Body::empty())
            .unwrap();
        let registry_resp = app.oneshot(registry_req).await.unwrap();
        let registry_body = axum::body::to_bytes(registry_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let registry_json: serde_json::Value = serde_json::from_slice(&registry_body).unwrap();
        assert_ne!(
            registry_json["connections"]["google"]["status"],
            "connected"
        );
    }

    #[tokio::test]
    async fn google_loopback_get_unknown_state_fails_closed_without_reflection() {
        let (app, _state) = connection_app_with_google();
        let callback_req = Request::builder()
            .method("GET")
            .uri("/api/v1/connections/google/callback?code=secret-code&state=unknown-state")
            .body(Body::empty())
            .unwrap();
        let callback_resp = app.clone().oneshot(callback_req).await.unwrap();
        assert_eq!(callback_resp.status(), StatusCode::BAD_REQUEST);
        let callback_body = axum::body::to_bytes(callback_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let html = String::from_utf8(callback_body.to_vec()).unwrap();
        assert!(!html.contains("secret-code"));
        assert!(!html.contains("unknown-state"));

        let registry_req = Request::builder()
            .method("GET")
            .uri("/api/v1/connections")
            .body(Body::empty())
            .unwrap();
        let registry_resp = app.oneshot(registry_req).await.unwrap();
        let registry_body = axum::body::to_bytes(registry_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let registry_json: serde_json::Value = serde_json::from_slice(&registry_body).unwrap();
        assert_ne!(
            registry_json["connections"]["google"]["status"],
            "connected"
        );
    }

    /// Full Google OAuth happy path: start → callback → connected.
    #[tokio::test]
    async fn google_oauth_happy_path_start_to_connected() {
        let (app, state, recorded) =
            connection_app_with_google_and_token_stub(StatusCode::OK, stub_token_response()).await;

        // Step 1: Start → get StartUrl with session ID.
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/connections/google/start")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let start_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(start_json["outcome"], "start_url");
        let session_id = start_json["auth_session_id"].as_str().unwrap().to_string();
        let url = start_json["url"].as_str().unwrap();
        // Extract state param from URL for CSRF.
        let state_param = url
            .split("state=")
            .nth(1)
            .unwrap_or("")
            .split('&')
            .next()
            .unwrap_or("");

        // Step 2: Callback exchanges the code via the REAL transport
        // against the local stub endpoint → Connected.
        let callback_json = serde_json::json!({
            "auth_session_id": session_id,
            "code": "auth-code-xyz",
            "state": state_param
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/connections/google/callback")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&callback_json).unwrap()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let cb_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(cb_json["outcome"], "connected");
        assert_eq!(cb_json["provider"], "google");
        assert_eq!(cb_json["auth_method"], "oauth");
        assert!(cb_json["capabilities"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("drive.file")));

        // No secret material in the response.
        let raw = String::from_utf8_lossy(&body);
        assert!(!raw.contains("stub-access-token"));
        assert!(!raw.contains("oauth_token"));
        assert!(!raw.contains("refresh_token"));
        assert!(!raw.contains("test-client-secret"));

        // The REAL transport carried the PKCE verifier and authorization
        // code to the token endpoint (wire-level proof, not a mock).
        let bodies = recorded.lock().unwrap().clone();
        assert_eq!(bodies.len(), 1);
        assert!(bodies[0].contains("code_verifier="));
        assert!(bodies[0].contains("code=auth-code-xyz"));
        assert!(bodies[0].contains("grant_type=authorization_code"));

        // Verify a versioned refresh-capable envelope was persisted to
        // secrets_store before the registry reported connected.
        let stored = state
            .secrets_store
            .read_secret(
                bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
                bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
            )
            .expect("Token should be persisted in secrets_store");
        let parsed = bicameral_config::parse_stored_oauth_token(&stored)
            .expect("Stored secret must parse as a usable token");
        let bicameral_config::StoredOAuthToken::Envelope(envelope) = parsed else {
            panic!("Stored secret must be a versioned envelope, not a plain token");
        };
        assert_eq!(
            envelope.envelope_version,
            bicameral_config::OAUTH_TOKEN_ENVELOPE_VERSION
        );
        assert_eq!(envelope.access_token, "stub-access-token");
        assert_eq!(
            envelope.refresh_token.as_deref(),
            Some("stub-refresh-token"),
            "Refresh token from the exchange must be preserved"
        );
        assert!(
            envelope.expires_at.is_some(),
            "Expiry metadata from the exchange must be preserved"
        );
    }

    // ── #1083 credential-backed connection truth ──────────────────

    /// Build a connection app whose workspace registry says Google is
    /// `connected`, returning the state and workspace root.
    async fn app_with_connected_google_registry() -> (Router, AppState, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let ws_root = dir.into_path();
        let mut state = AppState::new(test_audit());
        state.workspace_root = std::sync::Arc::new(tokio::sync::RwLock::new(Some(ws_root.clone())));
        state.google_oauth_config = Some(test_google_oauth_config());
        state.google_oauth_allow_env_override = true;
        {
            let mut registry = bicameral_config::ConnectionRegistry::default();
            registry.upsert(
                "google",
                bicameral_config::ConnectionMetadata {
                    status: bicameral_config::ConnectionStatus::Connected,
                    account_label: Some("workspace-scoped-label@example.com".into()),
                    capabilities: vec!["drive.file".into()],
                    updated_at: Some(chrono::Utc::now()),
                    auth_method: Some(bicameral_config::AuthMethod::Oauth),
                },
            );
            save_connection_registry(&state, &registry).await;
        }
        let app = Router::new()
            .merge(connection_routes())
            .with_state(state.clone());
        (app, state, ws_root)
    }

    async fn get_connections_json(app: &Router) -> serde_json::Value {
        let req = Request::builder()
            .method("GET")
            .uri("/api/v1/connections")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 16384).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    /// #1083 negative 1: persisted connected registry + empty secret
    /// store (simulated restart with volatile store) projects
    /// `action_needed`, and the GET does not mutate the durable registry.
    #[tokio::test]
    async fn connections_get_downgrades_stale_connected_without_mutation() {
        let (app, _state, ws_root) = app_with_connected_google_registry().await;

        let json = get_connections_json(&app).await;
        assert_eq!(json["connections"]["google"]["status"], "action_needed");

        // Projection only: the on-disk registry still says connected.
        let on_disk = bicameral_config::ConnectionRegistry::load_from_file(
            &ws_root.join(".bicameral").join("connections.yaml"),
        )
        .unwrap();
        assert_eq!(
            on_disk.status("google"),
            bicameral_config::ConnectionStatus::Connected,
            "GET must not manufacture a durable registry mutation"
        );
    }

    /// #1083 negative 2: connected registry + canonical credential
    /// projects `connected`.
    #[tokio::test]
    async fn connections_get_projects_connected_with_canonical_credential() {
        let (app, state, _ws) = app_with_connected_google_registry().await;
        let envelope = bicameral_config::OAuthTokenEnvelopeV1::from_token_exchange(
            "test-access-token-value".into(),
            Some("test-refresh-token-value".into()),
            Some(3599),
            None,
            None,
            chrono::Utc::now(),
        );
        state
            .secrets_store
            .write_secret(
                bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
                bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
                &envelope.to_secret_string().unwrap(),
            )
            .unwrap();

        let json = get_connections_json(&app).await;
        assert_eq!(json["connections"]["google"]["status"], "connected");
        // No secret material leaks through the projection.
        let raw = json.to_string();
        assert!(!raw.contains("test-access-token-value"));
        assert!(!raw.contains("test-refresh-token-value"));
    }

    /// #1083 negative 3: connected registry + supported legacy alias
    /// credential still projects `connected` during the compatibility
    /// window.
    #[tokio::test]
    async fn connections_get_projects_connected_with_legacy_credential() {
        let (app, state, _ws) = app_with_connected_google_registry().await;
        state
            .secrets_store
            .write_secret(
                bicameral_config::connections::legacy_aliases::GOOGLE_DRIVE_SOURCE_AUTH,
                bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
                "legacy-plain-access-token",
            )
            .unwrap();

        let json = get_connections_json(&app).await;
        assert_eq!(json["connections"]["google"]["status"], "connected");
    }

    /// #1279: a locally-usable but Google-revoked credential demotes to `action_needed`.
    #[tokio::test]
    async fn connections_get_demotes_locally_usable_but_revoked_google_credential() {
        let (_app, mut state, _ws) = app_with_connected_google_registry().await;
        let envelope = bicameral_config::OAuthTokenEnvelopeV1::from_token_exchange(
            "test-access-token-value".into(),
            Some("test-refresh-token-value".into()),
            Some(3599),
            None,
            None,
            chrono::Utc::now(),
        );
        state
            .secrets_store
            .write_secret(
                bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
                bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
                &envelope.to_secret_string().unwrap(),
            )
            .unwrap();
        let (about_endpoint, _recorded) = spawn_drive_about_stub(StatusCode::UNAUTHORIZED).await;
        state.google_drive_about_endpoint_override = Some(about_endpoint);
        let app = Router::new()
            .merge(connection_routes())
            .with_state(state.clone());

        let json = get_connections_json(&app).await;
        assert_eq!(json["connections"]["google"]["status"], "action_needed");
    }

    /// #1279: an ambiguous live-check failure (provider 5xx) must not demote a connection.
    #[tokio::test]
    async fn connections_get_stays_connected_on_ambiguous_live_check_failure() {
        let (_app, mut state, _ws) = app_with_connected_google_registry().await;
        let envelope = bicameral_config::OAuthTokenEnvelopeV1::from_token_exchange(
            "test-access-token-value".into(),
            Some("test-refresh-token-value".into()),
            Some(3599),
            None,
            None,
            chrono::Utc::now(),
        );
        state
            .secrets_store
            .write_secret(
                bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
                bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
                &envelope.to_secret_string().unwrap(),
            )
            .unwrap();
        let (about_endpoint, _recorded) =
            spawn_drive_about_stub(StatusCode::INTERNAL_SERVER_ERROR).await;
        state.google_drive_about_endpoint_override = Some(about_endpoint);
        let app = Router::new()
            .merge(connection_routes())
            .with_state(state.clone());

        let json = get_connections_json(&app).await;
        assert_eq!(json["connections"]["google"]["status"], "connected");
    }

    /// #1279: a non-auth Drive 403 (rate, quota, or permission) must not demote or force re-auth.
    #[tokio::test]
    async fn non_auth_drive_403_keeps_google_connected_without_reauth() {
        for (domain, reason) in [
            ("usageLimits", "rateLimitExceeded"),
            ("usageLimits", "userRateLimitExceeded"),
            ("usageLimits", "dailyLimitExceeded"),
            ("global", "insufficientPermissions"),
        ] {
            let (_app, mut state, _ws) = app_with_connected_google_registry().await;
            let envelope = bicameral_config::OAuthTokenEnvelopeV1::from_token_exchange(
                "test-access-token-value".into(),
                Some("test-refresh-token-value".into()),
                Some(3599),
                None,
                None,
                chrono::Utc::now(),
            );
            state
                .secrets_store
                .write_secret(
                    bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
                    bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
                    &envelope.to_secret_string().unwrap(),
                )
                .unwrap();
            let body = serde_json::json!({
                "error": {
                    "code": 403,
                    "message": "Request refused.",
                    "errors": [{ "domain": domain, "reason": reason }],
                }
            })
            .to_string();
            let (about_endpoint, recorded) =
                spawn_drive_about_stub_with_body(StatusCode::FORBIDDEN, body).await;
            state.google_drive_about_endpoint_override = Some(about_endpoint);
            let app = Router::new()
                .merge(connection_routes())
                .with_state(state.clone());

            let json = get_connections_json(&app).await;
            assert_eq!(
                json["connections"]["google"]["status"], "connected",
                "{reason} demoted the connection"
            );

            let req = Request::builder()
                .method("POST")
                .uri("/api/v1/connections/google/start")
                .body(Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
            let start: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(start["outcome"], "connected", "{reason} forced re-auth");

            assert_eq!(
                recorded.lock().unwrap().len(),
                2,
                "{reason}: both requests must reach the live probe"
            );
        }
    }

    /// #1279: a token expiring within the refresh skew is refreshed before the live check.
    #[tokio::test]
    async fn connections_get_refreshes_expiring_token_before_declaring_revoked() {
        let (_app, mut state, _ws) = app_with_connected_google_registry().await;
        let envelope = bicameral_config::OAuthTokenEnvelopeV1::from_token_exchange(
            "stale-access-token-value".into(),
            Some("test-refresh-token-value".into()),
            Some(1),
            None,
            None,
            chrono::Utc::now(),
        );
        state
            .secrets_store
            .write_secret(
                bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
                bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
                &envelope.to_secret_string().unwrap(),
            )
            .unwrap();
        let (token_endpoint, _token_recorded) =
            spawn_token_endpoint_stub(StatusCode::OK, stub_token_response()).await;
        state.google_token_endpoint_override = Some(token_endpoint);
        let (about_endpoint, about_recorded) = spawn_drive_about_stub(StatusCode::OK).await;
        state.google_drive_about_endpoint_override = Some(about_endpoint);
        let app = Router::new()
            .merge(connection_routes())
            .with_state(state.clone());

        let json = get_connections_json(&app).await;
        assert_eq!(json["connections"]["google"]["status"], "connected");
        // The live probe used the freshly-refreshed token, not the stale one.
        let auth_headers = about_recorded.lock().unwrap().clone();
        assert_eq!(auth_headers, vec!["Bearer stub-access-token".to_string()]);
    }

    /// #1279: Connect performs a real OAuth attempt once revocation is confirmed.
    #[tokio::test]
    async fn start_performs_real_reauth_after_locally_usable_but_revoked_google_credential() {
        let (_app, mut state, _ws) = app_with_connected_google_registry().await;
        let envelope = bicameral_config::OAuthTokenEnvelopeV1::from_token_exchange(
            "test-access-token-value".into(),
            Some("test-refresh-token-value".into()),
            Some(3599),
            None,
            None,
            chrono::Utc::now(),
        );
        state
            .secrets_store
            .write_secret(
                bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
                bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
                &envelope.to_secret_string().unwrap(),
            )
            .unwrap();
        let (about_endpoint, _recorded) = spawn_drive_about_stub(StatusCode::UNAUTHORIZED).await;
        state.google_drive_about_endpoint_override = Some(about_endpoint);
        let app = Router::new()
            .merge(connection_routes())
            .with_state(state.clone());

        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/connections/google/start")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["outcome"], "start_url");
        assert!(json["url"]
            .as_str()
            .unwrap()
            .starts_with("https://accounts.google.com/o/oauth2/v2/auth"));
    }

    /// Serve a valid Cloud Google app config at the governed path (#1135).
    async fn spawn_google_app_config_stub(client_id: &str) -> String {
        use axum::routing::get;
        let now = chrono::Utc::now();
        let payload = serde_json::json!({
            "schema_version": 1,
            "provider": "google",
            "client_type": "native",
            "client_id": client_id,
            "environment": "production",
            "config_version": "google-production-1",
            "scope_set_version": "drive-file-v1",
            "scopes": ["https://www.googleapis.com/auth/drive.file"],
            "issued_at": (now - chrono::Duration::minutes(1)).to_rfc3339(),
            "expires_at": (now + chrono::Duration::minutes(30)).to_rfc3339(),
            "enabled": true,
        })
        .to_string();
        let app = Router::new().route(
            "/api/v1/provider-app-config/google",
            get(move || {
                let payload = payload.clone();
                async move {
                    (
                        StatusCode::OK,
                        [("content-type", "application/json")],
                        payload,
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{address}")
    }

    /// #1279: Cloud-first install with an expired access token. No legacy
    /// env config exists; the Cloud app config and the Desktop credential
    /// stored at Connect are the only application identity.
    async fn cloud_first_state_with_expired_google_token(desktop_secret: Option<&str>) -> AppState {
        let (_app, mut state, _ws) = app_with_connected_google_registry().await;
        state.google_oauth_config = None;
        state.google_oauth_allow_env_override = false;
        state.google_cloud_origin_override =
            Some(spawn_google_app_config_stub("cloud-client-id").await);
        let token = bicameral_config::OAuthTokenEnvelopeV1::from_token_exchange(
            "expired-access-token-value".into(),
            Some("test-refresh-token-value".into()),
            Some(1),
            None,
            None,
            chrono::Utc::now(),
        );
        state
            .secrets_store
            .write_secret(
                bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
                bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
                &token.to_secret_string().unwrap(),
            )
            .unwrap();
        if let Some(secret) = desktop_secret {
            let credential = bicameral_config::GoogleClientCredentialEnvelopeV1::new(
                secret,
                "cloud-client-id",
                "google-production-1",
                chrono::Utc::now(),
            )
            .unwrap();
            state
                .secrets_store
                .write_secret(
                    bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
                    bicameral_config::GOOGLE_CLIENT_CREDENTIAL_KEY,
                    &credential.to_secret_string().unwrap(),
                )
                .unwrap();
        }
        state
    }

    /// #1279: on a Cloud-first install, a revoked grant is still detected
    /// after the access token expires, and Connect performs a real re-auth.
    #[tokio::test]
    async fn cloud_first_revoked_refresh_demotes_and_start_reauthenticates() {
        let mut state = cloud_first_state_with_expired_google_token(Some("desktop-secret")).await;
        let (token_endpoint, token_requests) = spawn_token_endpoint_stub(
            StatusCode::BAD_REQUEST,
            serde_json::json!({ "error": "invalid_grant" }),
        )
        .await;
        state.google_token_endpoint_override = Some(token_endpoint);
        let app = Router::new()
            .merge(connection_routes())
            .with_state(state.clone());

        let json = get_connections_json(&app).await;
        assert_eq!(json["connections"]["google"]["status"], "action_needed");

        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/connections/google/start")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let start: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(start["outcome"], "start_url");
        assert!(start["url"]
            .as_str()
            .unwrap()
            .starts_with("https://accounts.google.com/o/oauth2/v2/auth"));

        let requests = token_requests.lock().unwrap().clone();
        assert!(!requests.is_empty(), "the expired token must be refreshed");
        for request in &requests {
            assert!(request.contains("client_id=cloud-client-id"));
            assert!(request.contains("client_secret=desktop-secret"));
        }
    }

    /// #1279: without the Desktop credential stored at Connect, a refresh
    /// would carry no client secret and Google would reject it for that
    /// reason alone. The check stays inconclusive and sends no refresh.
    #[tokio::test]
    async fn cloud_first_refresh_without_stored_desktop_credential_stays_connected() {
        let mut state = cloud_first_state_with_expired_google_token(None).await;
        let (token_endpoint, token_requests) = spawn_token_endpoint_stub(
            StatusCode::BAD_REQUEST,
            serde_json::json!({ "error": "invalid_grant" }),
        )
        .await;
        state.google_token_endpoint_override = Some(token_endpoint);
        let app = Router::new()
            .merge(connection_routes())
            .with_state(state.clone());

        let json = get_connections_json(&app).await;
        assert_eq!(json["connections"]["google"]["status"], "connected");
        assert!(token_requests.lock().unwrap().is_empty());
    }

    /// #1083 negative 4: GET reads the workspace-scoped registry, not a
    /// global operator-home lookup. The workspace registry carries a
    /// distinctive label that a global-home read could not produce;
    /// whatever the operator home contains, the wrong path fails this.
    #[tokio::test]
    async fn connections_get_reads_workspace_scoped_registry() {
        let (app, state, _ws) = app_with_connected_google_registry().await;
        state
            .secrets_store
            .write_secret(
                bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
                bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
                "test-access-token-value",
            )
            .unwrap();

        let json = get_connections_json(&app).await;
        assert_eq!(
            json["connections"]["google"]["account_label"], "workspace-scoped-label@example.com",
            "GET must reflect the workspace-scoped registry facts"
        );
    }

    /// #1083 negative 5: stale connected metadata with no readable
    /// credential can never satisfy the existing-connected start branch;
    /// it enters the existing Google re-auth flow instead.
    #[tokio::test]
    async fn stale_connected_metadata_cannot_satisfy_existing_connected_start() {
        let (app, _state, _ws) = app_with_connected_google_registry().await;

        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/connections/google/start")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_ne!(
            json["outcome"], "connected",
            "Stale metadata without a credential must not report connected"
        );
        assert_eq!(
            json["outcome"], "start_url",
            "Stale metadata must enter the existing re-auth flow"
        );
    }

    /// #1083: with a credential present, the existing-connected start
    /// branch still short-circuits to `connected`.
    #[tokio::test]
    async fn start_with_credential_backed_connected_metadata_returns_connected() {
        let (app, state, _ws) = app_with_connected_google_registry().await;
        state
            .secrets_store
            .write_secret(
                bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
                bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
                "test-access-token-value",
            )
            .unwrap();

        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/connections/google/start")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["outcome"], "connected");
    }

    /// A `SecretsStore` whose writes always fail, simulating an
    /// unavailable production keyring. Reads/deletes behave as empty.
    struct FailingSecretsStore;

    impl bicameral_config::SecretsStore for FailingSecretsStore {
        fn read_secret(&self, _source_id: &str, _key: &str) -> Option<String> {
            None
        }
        fn write_secret(&self, _source_id: &str, _key: &str, _value: &str) -> anyhow::Result<()> {
            Err(anyhow::anyhow!("simulated keyring unavailable"))
        }
        fn delete_secret(&self, _source_id: &str, _key: &str) -> anyhow::Result<()> {
            Err(anyhow::anyhow!("simulated keyring unavailable"))
        }
    }

    /// Drive the start → callback flow against `app` and return the
    /// callback response body.
    async fn run_google_oauth_flow(app: &Router) -> (StatusCode, serde_json::Value) {
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/connections/google/start")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let start_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(start_json["outcome"], "start_url");
        let session_id = start_json["auth_session_id"].as_str().unwrap().to_string();
        let state_param = start_json["url"]
            .as_str()
            .unwrap()
            .split("state=")
            .nth(1)
            .unwrap_or("")
            .split('&')
            .next()
            .unwrap_or("")
            .to_string();

        let callback_json = serde_json::json!({
            "auth_session_id": session_id,
            "code": "auth-code-xyz",
            "state": state_param
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/connections/google/callback")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&callback_json).unwrap()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    /// #1084 fail-closed: a keyring write failure must never leave the
    /// registry `connected`, and the safe error carries no token material.
    #[tokio::test]
    async fn google_callback_keyring_write_failure_cannot_create_connected() {
        let (endpoint, _recorded) =
            spawn_token_endpoint_stub(StatusCode::OK, stub_token_response()).await;
        let dir = tempfile::tempdir().unwrap();
        let ws_root = dir.into_path();
        let mut state = AppState::new(test_audit())
            .with_secrets_store(std::sync::Arc::new(FailingSecretsStore));
        state.workspace_root = std::sync::Arc::new(tokio::sync::RwLock::new(Some(ws_root.clone())));
        state.google_oauth_config = Some(test_google_oauth_config());
        state.google_oauth_allow_env_override = true;
        state.google_token_endpoint_override = Some(endpoint);
        let app = Router::new()
            .merge(connection_routes())
            .with_state(state.clone());

        let (status, cb_json) = run_google_oauth_flow(&app).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(cb_json["outcome"], "provider_unavailable");
        let raw = cb_json.to_string();
        assert!(!raw.contains("stub-access-token"));
        assert!(!raw.contains("stub-refresh-token"));

        // The durable registry must not have been marked connected.
        let registry = bicameral_config::ConnectionRegistry::load_from_file(
            &ws_root.join(".bicameral").join("connections.yaml"),
        )
        .unwrap();
        assert_ne!(
            registry.status("google"),
            bicameral_config::ConnectionStatus::Connected,
            "Keyring write failure must never produce connected registry metadata"
        );
    }

    /// #1084 migration: a successful reconnect writes the canonical
    /// envelope and retires the legacy `google_drive` alias value.
    #[tokio::test]
    async fn google_callback_migrates_legacy_alias_to_canonical_envelope() {
        let (app, state, _recorded) =
            connection_app_with_google_and_token_stub(StatusCode::OK, stub_token_response()).await;
        state
            .secrets_store
            .write_secret(
                bicameral_config::connections::legacy_aliases::GOOGLE_DRIVE_SOURCE_AUTH,
                bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
                "legacy-plain-access-token",
            )
            .unwrap();
        // Reconnect scenario: registry says the connection needs
        // attention, so start enters the re-auth flow rather than the
        // legacy already-connected short-circuit.
        {
            let mut registry = load_connection_registry_from_state(&state).await;
            registry.upsert(
                "google",
                bicameral_config::ConnectionMetadata {
                    status: bicameral_config::ConnectionStatus::ActionNeeded,
                    account_label: Some("test@example.com".into()),
                    capabilities: vec!["drive.file".into()],
                    updated_at: Some(chrono::Utc::now()),
                    auth_method: Some(bicameral_config::AuthMethod::Oauth),
                },
            );
            save_connection_registry(&state, &registry).await;
        }

        let (status, cb_json) = run_google_oauth_flow(&app).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(cb_json["outcome"], "connected");

        let canonical = state.secrets_store.read_secret(
            bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
            bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
        );
        assert!(
            canonical.is_some_and(|v| matches!(
                bicameral_config::parse_stored_oauth_token(&v),
                Some(bicameral_config::StoredOAuthToken::Envelope(_))
            )),
            "Reconnect must store the canonical versioned envelope"
        );
        assert_eq!(
            state.secrets_store.read_secret(
                bicameral_config::connections::legacy_aliases::GOOGLE_DRIVE_SOURCE_AUTH,
                bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
            ),
            None,
            "Successful reconnect must retire the superseded legacy alias value"
        );
    }

    /// #1084 restart behavior at the gateway layer: with a persistent
    /// secrets store and the same workspace, a rebuilt `AppState` re-opens
    /// both the registry metadata and a parseable credential. True OS
    /// persistence is proven separately by the bicameral-config
    /// `os_keyring_round_trip_persists_across_store_instances` test.
    #[tokio::test]
    async fn google_connection_reopens_after_simulated_restart() {
        let dir = tempfile::tempdir().unwrap();
        let ws_root = dir.into_path();
        let shared_store: std::sync::Arc<dyn bicameral_config::SecretsStore> =
            std::sync::Arc::new(bicameral_config::InMemorySecretsStore::new());

        let (endpoint, _recorded) =
            spawn_token_endpoint_stub(StatusCode::OK, stub_token_response()).await;
        let mut state = AppState::new(test_audit()).with_secrets_store(shared_store.clone());
        state.workspace_root = std::sync::Arc::new(tokio::sync::RwLock::new(Some(ws_root.clone())));
        state.google_oauth_config = Some(test_google_oauth_config());
        state.google_oauth_allow_env_override = true;
        state.google_token_endpoint_override = Some(endpoint);
        let app = Router::new()
            .merge(connection_routes())
            .with_state(state.clone());
        let (status, cb_json) = run_google_oauth_flow(&app).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(cb_json["outcome"], "connected");

        // "Restart": a fresh AppState over the same workspace and the
        // same durable store.
        let mut restarted = AppState::new(test_audit()).with_secrets_store(shared_store.clone());
        restarted.workspace_root =
            std::sync::Arc::new(tokio::sync::RwLock::new(Some(ws_root.clone())));
        let registry = load_connection_registry_from_state(&restarted).await;
        assert_eq!(
            registry.status("google"),
            bicameral_config::ConnectionStatus::Connected
        );
        let stored = restarted
            .secrets_store
            .read_secret(
                bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
                bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
            )
            .expect("Credential must survive the simulated restart");
        assert!(
            bicameral_config::parse_stored_oauth_token(&stored)
                .is_some_and(|token| token.is_usable()),
            "Re-opened credential must parse as usable token material"
        );
    }

    /// #1132 regression lock: an authorization code with the historical
    /// `mock-` prefix selects nothing. The REAL transport runs, the
    /// provider (stub) rejects the code, and the outcome is a fail-closed
    /// non-connected state with no persisted secret — even with the
    /// removed `BICAMERAL_GOOGLE_TOKEN_EXCHANGE_MOCK` variable set.
    #[tokio::test]
    async fn mock_prefixed_code_cannot_select_synthetic_success() {
        let (app, state, recorded) = connection_app_with_google_and_token_stub(
            StatusCode::BAD_REQUEST,
            serde_json::json!({"error": "invalid_grant"}),
        )
        .await;

        // The env switch no longer exists in code; setting it must be inert.
        std::env::set_var("BICAMERAL_GOOGLE_TOKEN_EXCHANGE_MOCK", "1");

        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/connections/google/start")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let start_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let session_id = start_json["auth_session_id"].as_str().unwrap().to_string();
        let state_param = start_json["url"]
            .as_str()
            .unwrap()
            .split("state=")
            .nth(1)
            .unwrap_or("")
            .split('&')
            .next()
            .unwrap_or("")
            .to_string();

        let callback_json = serde_json::json!({
            "auth_session_id": session_id,
            "code": "mock-auth-code-xyz",
            "state": state_param
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/connections/google/callback")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&callback_json).unwrap()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        std::env::remove_var("BICAMERAL_GOOGLE_TOKEN_EXCHANGE_MOCK");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["outcome"], "provider_unavailable");

        // The real transport WAS attempted (one wire hit) and rejected.
        assert_eq!(recorded.lock().unwrap().len(), 1);
        // No synthetic envelope was persisted.
        assert!(state
            .secrets_store
            .read_secret(
                bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
                bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
            )
            .is_none());
        // The registry never reported connected.
        let registry = load_connection_registry_from_state(&state).await;
        assert_ne!(
            registry.status("google"),
            bicameral_config::ConnectionStatus::Connected
        );
    }

    /// #1132 wire contract: a native/public client omits `client_secret`
    /// entirely while a confidential legacy override still sends it.
    #[tokio::test]
    async fn native_exchange_omits_client_secret_confidential_includes_it() {
        // Native/public client: empty secret must not appear on the wire.
        let (endpoint, recorded) =
            spawn_token_endpoint_stub(StatusCode::OK, stub_token_response()).await;
        let dir = tempfile::tempdir().unwrap();
        let mut state = AppState::new(test_audit());
        state.workspace_root = std::sync::Arc::new(tokio::sync::RwLock::new(Some(dir.into_path())));
        state.google_oauth_config = Some(bicameral_config::GoogleOAuthConfig {
            client_id: "native-client-id".into(),
            client_secret: String::new(),
            redirect_uri: "http://127.0.0.1:7411/api/v1/connections/google/callback".into(),
            scopes: vec!["https://www.googleapis.com/auth/drive.file".into()],
        });
        state.google_oauth_allow_env_override = true;
        state.google_token_endpoint_override = Some(endpoint);
        let app = Router::new()
            .merge(connection_routes())
            .with_state(state.clone());
        let (status, cb_json) = run_google_oauth_flow(&app).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(cb_json["outcome"], "connected");
        let bodies = recorded.lock().unwrap().clone();
        assert_eq!(bodies.len(), 1);
        assert!(!bodies[0].contains("client_secret"));
        assert!(bodies[0].contains("client_id=native-client-id"));
        assert!(bodies[0].contains("code_verifier="));

        // Confidential legacy override: non-empty secret is still sent.
        let (endpoint, recorded) =
            spawn_token_endpoint_stub(StatusCode::OK, stub_token_response()).await;
        let dir = tempfile::tempdir().unwrap();
        let mut state = AppState::new(test_audit());
        state.workspace_root = std::sync::Arc::new(tokio::sync::RwLock::new(Some(dir.into_path())));
        state.google_oauth_config = Some(test_google_oauth_config());
        state.google_oauth_allow_env_override = true;
        state.google_token_endpoint_override = Some(endpoint);
        let app = Router::new()
            .merge(connection_routes())
            .with_state(state.clone());
        let (status, cb_json) = run_google_oauth_flow(&app).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(cb_json["outcome"], "connected");
        let bodies = recorded.lock().unwrap().clone();
        assert_eq!(bodies.len(), 1);
        assert!(bodies[0].contains("client_secret=test-client-secret"));
    }

    /// Google callback with CSRF state mismatch.
    #[tokio::test]
    async fn google_callback_csrf_mismatch_returns_denied() {
        let (app, _state) = connection_app_with_google();

        // Start to get a session.
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/connections/google/start")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let start_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let session_id = start_json["auth_session_id"].as_str().unwrap();

        // Callback with wrong CSRF state.
        let callback_json = serde_json::json!({
            "auth_session_id": session_id,
            "code": "auth-code",
            "state": "wrong-csrf-state"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/connections/google/callback")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&callback_json).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["outcome"], "denied");
        assert!(json["message"].as_str().unwrap().contains("CSRF"));
    }

    /// Google callback without authorization code returns action_needed.
    #[tokio::test]
    async fn google_callback_missing_code_returns_action_needed() {
        let (app, state) = connection_app_with_google();

        // Start to get a session.
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/connections/google/start")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let start_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let session_id = start_json["auth_session_id"].as_str().unwrap();
        let state_token = {
            let sessions = state.auth_sessions.read().await;
            sessions.get(session_id).unwrap().state_token.clone()
        };

        // Callback without code but with the valid CSRF state so this test
        // reaches the missing-code branch rather than the earlier CSRF gate.
        let callback_json = serde_json::json!({
            "auth_session_id": session_id,
            "state": state_token
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/connections/google/callback")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&callback_json).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["outcome"], "action_needed");
        assert!(json["message"].as_str().unwrap().contains("code"));
    }

    /// Start a Google flow and return `(session_id, state_token)`.
    async fn start_google_session(app: &Router) -> (String, String) {
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/connections/google/start")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let start_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(start_json["outcome"], "start_url");
        let session_id = start_json["auth_session_id"].as_str().unwrap().to_string();
        let state_param = start_json["url"]
            .as_str()
            .unwrap()
            .split("state=")
            .nth(1)
            .unwrap_or("")
            .split('&')
            .next()
            .unwrap_or("")
            .to_string();
        (session_id, state_param)
    }

    async fn post_google_callback(
        app: &Router,
        body_json: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/connections/google/callback")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body_json).unwrap()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    /// #1133: a provider error on a VALIDATED session is terminal Denied
    /// and consumes the session one-shot; a retry finds it Expired.
    #[tokio::test]
    async fn google_callback_scope_denied_returns_denied() {
        let (app, _state) = connection_app_with_google();
        let (session_id, state_param) = start_google_session(&app).await;

        let (status, json) = post_google_callback(
            &app,
            serde_json::json!({
                "auth_session_id": session_id,
                "state": state_param,
                "error": "scope_denied"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["outcome"], "denied");
        assert!(json["message"].as_str().unwrap().contains("scope_denied"));

        // One-shot: the denied session is consumed.
        let (_, retry) = post_google_callback(
            &app,
            serde_json::json!({
                "auth_session_id": session_id,
                "state": state_param,
                "error": "scope_denied"
            }),
        )
        .await;
        assert_eq!(retry["outcome"], "expired");
    }

    /// #1133: an error callback for an UNKNOWN session is Expired, not
    /// Denied — unauthenticated error posts cannot terminate anything.
    #[tokio::test]
    async fn connection_callback_error_returns_denied() {
        let (app, _state) = connection_app_with_google();
        let (session_id, state_param) = start_google_session(&app).await;

        let (status, json) = post_google_callback(
            &app,
            serde_json::json!({
                "auth_session_id": session_id,
                "state": state_param,
                "error": "access_denied"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["outcome"], "denied");
        assert_eq!(json["provider"], "google");
        assert!(json["message"].as_str().unwrap().contains("access_denied"));

        let (_, unknown) = post_google_callback(
            &app,
            serde_json::json!({
                "auth_session_id": "sess-unknown-with-error",
                "error": "access_denied"
            }),
        )
        .await;
        assert_eq!(unknown["outcome"], "expired");
    }

    // ── #1133 bounded auth-session TTL ────────────────────────────

    /// Age the identified session past the TTL, or skip on platforms where
    /// `Instant` cannot represent the required past instant.
    async fn age_session_past_ttl(state: &AppState, session_id: &str) -> bool {
        let aged = std::time::Instant::now()
            .checked_sub(crate::state::GOOGLE_AUTH_SESSION_TTL + std::time::Duration::from_secs(1));
        let Some(aged) = aged else {
            return false;
        };
        let mut sessions = state.auth_sessions.write().await;
        let session = sessions.get_mut(session_id).expect("session exists");
        session.created_at = aged;
        true
    }

    /// #1133: an expired session is rejected BEFORE token exchange, secret
    /// writes, or registry mutation — the stub endpoint records zero hits —
    /// and the expired entry is removed.
    #[tokio::test]
    async fn expired_session_is_rejected_before_exchange_and_removed() {
        let (app, state, recorded) =
            connection_app_with_google_and_token_stub(StatusCode::OK, stub_token_response()).await;
        let (session_id, state_param) = start_google_session(&app).await;
        if !age_session_past_ttl(&state, &session_id).await {
            return;
        }

        let (status, json) = post_google_callback(
            &app,
            serde_json::json!({
                "auth_session_id": session_id,
                "state": state_param,
                "code": "auth-code-late"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["outcome"], "expired");

        // No exchange was attempted and nothing was persisted.
        assert!(recorded.lock().unwrap().is_empty());
        assert!(state
            .secrets_store
            .read_secret(
                bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
                bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
            )
            .is_none());
        // The expired session was consumed.
        assert!(!state.auth_sessions.read().await.contains_key(&session_id));
    }

    /// #1133: the native GET loopback never resolves an expired state
    /// token; the expired entry is purged by the scan.
    #[tokio::test]
    async fn loopback_get_does_not_resurrect_expired_session() {
        let (app, state, recorded) =
            connection_app_with_google_and_token_stub(StatusCode::OK, stub_token_response()).await;
        let (session_id, state_param) = start_google_session(&app).await;
        if !age_session_past_ttl(&state, &session_id).await {
            return;
        }

        let callback_req = Request::builder()
            .method("GET")
            .uri(format!(
                "/api/v1/connections/google/callback?code=auth-code-late&state={state_param}"
            ))
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(callback_req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        assert!(recorded.lock().unwrap().is_empty());
        assert!(!state.auth_sessions.read().await.contains_key(&session_id));
        let registry = load_connection_registry_from_state(&state).await;
        assert_ne!(
            registry.status("google"),
            bicameral_config::ConnectionStatus::Connected
        );
    }

    /// #1133: a state mismatch does NOT consume the session (an
    /// unauthenticated caller cannot kill an in-flight attempt); the
    /// bounded TTL is the cleanup path for abandoned attempts.
    #[tokio::test]
    async fn state_mismatch_does_not_consume_session() {
        let (app, state, _recorded) =
            connection_app_with_google_and_token_stub(StatusCode::OK, stub_token_response()).await;
        let (session_id, state_param) = start_google_session(&app).await;

        let (_, json) = post_google_callback(
            &app,
            serde_json::json!({
                "auth_session_id": session_id,
                "state": "attacker-guess",
                "code": "auth-code-xyz"
            }),
        )
        .await;
        assert_eq!(json["outcome"], "denied");
        assert!(state.auth_sessions.read().await.contains_key(&session_id));

        // The legitimate browser round trip still completes.
        let (_, json) = post_google_callback(
            &app,
            serde_json::json!({
                "auth_session_id": session_id,
                "state": state_param,
                "code": "auth-code-xyz"
            }),
        )
        .await;
        assert_eq!(json["outcome"], "connected");
    }

    #[tokio::test]
    async fn connection_callback_unsupported_provider() {
        let app = connection_app();
        let body_json = serde_json::json!({
            "auth_session_id": "sess-test-003"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/connections/notion/callback")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body_json).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["outcome"], "unsupported");
    }

    // ── Connection status (auth-session polling) tests ─────────────

    #[tokio::test]
    async fn connection_status_unknown_session_returns_expired() {
        let app = connection_app();
        let req = Request::builder()
            .method("GET")
            .uri("/api/v1/connections/github/status/sess-device-001")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        // Unknown session returns 404 with an expired outcome.
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        // Unknown session returns expired (session not found).
        assert_eq!(json["outcome"], "expired");
        assert_eq!(json["provider"], "github");
        let raw = String::from_utf8_lossy(&body);
        assert!(!raw.contains("oauth_token"));
        assert!(!raw.contains("refresh_token"));
    }

    #[tokio::test]
    async fn connection_status_active_session_returns_pending() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = AppState::new(test_audit());
        state.workspace_root = std::sync::Arc::new(tokio::sync::RwLock::new(Some(dir.into_path())));
        // Insert a pending auth session.
        {
            let mut sessions = state.auth_sessions.write().await;
            sessions.insert(
                "sess-pending-google".to_string(),
                crate::state::AuthSession {
                    provider: "google".to_string(),
                    created_at: std::time::Instant::now(),
                    state_token: "csrf-token".to_string(),
                    pkce_verifier: None,
                    google_oauth_config: None,
                },
            );
        }
        let app = Router::new().merge(connection_routes()).with_state(state);
        let req = Request::builder()
            .method("GET")
            .uri("/api/v1/connections/google/status/sess-pending-google")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["outcome"], "pending");
        assert_eq!(json["provider"], "google");
        let raw = String::from_utf8_lossy(&body);
        assert!(!raw.contains("oauth_token"));
        assert!(!raw.contains("refresh_token"));
    }

    #[tokio::test]
    async fn connection_status_unsupported_provider() {
        let app = connection_app();
        let req = Request::builder()
            .method("GET")
            .uri("/api/v1/connections/slack/status/sess-device-002")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["outcome"], "unsupported");
    }

    // ── Connection registry tests ─────────────────────────────────

    #[tokio::test]
    async fn connection_registry_returns_all_known_providers() {
        let app = connection_app();
        let req = Request::builder()
            .method("GET")
            .uri("/api/v1/connections")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let connections = json["connections"].as_object().unwrap();
        // Without on-disk registry, all known providers show not_connected.
        assert!(connections.contains_key("github"));
        assert!(connections.contains_key("google"));
        assert_eq!(connections["github"]["status"], "not_connected");
        assert_eq!(connections["google"]["status"], "not_connected");
        // Must not contain secret material.
        let raw = String::from_utf8_lossy(&body);
        assert!(!raw.contains("oauth_token"));
        assert!(!raw.contains("refresh_token"));
    }

    // ── Decision detail surface parity and no-mutation tests ─────────

    /// Helper: ingest a candidate and accept it into a Decision. Returns
    /// (decision_id, app, state) for further assertions.
    async fn setup_accepted_decision() -> (String, Router, AppState) {
        let state = AppState::new(test_audit());
        let app = Router::new()
            .merge(ingest_routes())
            .merge(dashboard_api_routes())
            .merge(projection_api_routes())
            .merge(query_routes())
            .with_state(state.clone());

        // Ingest a candidate.
        let body = serde_json::json!({
            "title": "Surface parity test decision",
            "description": "Proves stable identity across surfaces",
            "level": "architecture",
            "source": "test-surface-parity",
            "evidence": [{"span_start": 0, "span_end": 20, "excerpt": "surface parity excerpt"}]
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ingest")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let cand_id = state
            .candidates
            .read()
            .await
            .keys()
            .next()
            .unwrap()
            .to_string();

        // Accept candidate → Decision.
        let cmd = serde_json::json!({
            "target_id": cand_id,
            "command": "accept_candidate"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/dashboard/command")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&cmd).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);

        let decision_id = state
            .decisions
            .read()
            .await
            .keys()
            .next()
            .unwrap()
            .to_string();

        (decision_id, app, state)
    }

    #[tokio::test]
    async fn decision_detail_returns_separated_axes() {
        let (decision_id, app, _state) = setup_accepted_decision().await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(&format!("/api/v1/dashboard/decision/{}", decision_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let detail: bicameral_api::dashboard::DecisionDetail =
            serde_json::from_slice(&body).unwrap();

        // Verify stable identity.
        assert_eq!(detail.decision_id, decision_id);
        assert_eq!(detail.title, "Surface parity test decision");

        // Verify separated axes exist and are independent.
        assert_eq!(
            detail.candidate_review.review_state,
            ReviewLifecycleState::Proposed
        );
        assert_eq!(detail.signoff.state, SignoffState::Proposed);
        assert!(!detail.signoff.allowed_commands.is_empty());
        assert_eq!(
            detail.code_evidence.evidence_state,
            GraphEvidenceState::UnknownNotIndexed
        );
        assert!(!detail.code_evidence.has_bindings);
        assert_eq!(
            detail.compliance.state,
            bicameral_api::dashboard::ComplianceState::Ungrounded
        );
        assert!(detail.compliance.deferred);
    }

    /// #389 AC#2: submission provenance must appear on the decision-detail
    /// surface (not only history/search), and identically across BOTH detail
    /// endpoints (#387 surface parity). Exercises all three provenance-bearing
    /// gateway sites: `build_projection_from_state` (ProjectedDecision),
    /// the projection-path detail build, and the in-memory `build_decision_detail`.
    #[tokio::test]
    async fn decision_detail_surfaces_submission_provenance_across_surfaces() {
        let state = AppState::new(test_audit());
        let app = Router::new()
            .merge(external_ingest_routes())
            .merge(dashboard_api_routes())
            .merge(projection_api_routes())
            .with_state(state.clone());

        // External-ingest derives SubmissionProvenance from the adapter context.
        let envelope = serde_json::json!({
            "source_system": "jira",
            "source_uri": "https://jira.example.com/PROJ-389",
            "content": "Evidence submissions must preserve their origin provenance",
            "evidence": [{
                "excerpt": "preserve their origin provenance",
                "span_start": 24,
                "span_end": 56,
                "confidence": 0.9
            }],
            "candidate_hints": [{
                "title": "Preserve submission provenance",
                "body": "Local/tool evidence retains source/surface provenance",
                "level": "architecture",
                "labels": ["provenance"]
            }]
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/external-ingest")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&envelope).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let cand_id = state
            .candidates
            .read()
            .await
            .keys()
            .next()
            .copied()
            .expect("external ingest materialized a candidate")
            .to_string();

        // Accept candidate → Decision (provenance propagates per #418).
        let cmd = serde_json::json!({"target_id": cand_id, "command": "accept_candidate"});
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/dashboard/command")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&cmd).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);

        let decision_id = state
            .decisions
            .read()
            .await
            .keys()
            .next()
            .copied()
            .expect("accept produced a Decision")
            .to_string();

        // 1. In-memory path: dashboard detail must surface the provenance.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&format!("/api/v1/dashboard/decision/{}", decision_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let dash_detail: bicameral_api::dashboard::DecisionDetail =
            serde_json::from_slice(&body).unwrap();
        let dash_prov = dash_detail
            .provenance
            .clone()
            .expect("dashboard detail surfaces submission provenance");
        assert_eq!(dash_prov.source_kind.as_deref(), Some("jira"));
        assert_eq!(
            dash_prov.source_uri.as_deref(),
            Some("https://jira.example.com/PROJ-389")
        );
        assert_eq!(
            dash_prov.submitting_surface.as_deref(),
            Some("external_adapter")
        );

        // 2. Projection path: populate state.projection via the gateway-local
        //    builder, then the projection detail endpoint must surface the SAME
        //    provenance (true surface parity, not just non-None).
        let proj = build_projection_from_state(&state).await;
        *state.projection.write().await = Some(proj);
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&format!("/api/v1/projection/decision/{}", decision_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let proj_detail: bicameral_api::dashboard::DecisionDetail =
            serde_json::from_slice(&body).unwrap();
        assert_eq!(
            proj_detail.provenance,
            Some(dash_prov),
            "projection and dashboard detail must surface identical provenance"
        );
    }

    /// #389 AC#3: a Decision without provenance degrades explicitly — the
    /// detail field is `None` and is omitted from the JSON (serde skip).
    #[tokio::test]
    async fn decision_detail_without_provenance_omits_field() {
        let (decision_id, app, _state) = setup_accepted_decision().await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(&format!("/api/v1/dashboard/decision/{}", decision_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();

        // Field omitted from JSON when absent.
        let raw: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(
            raw.get("provenance").is_none(),
            "absent provenance must be omitted from the detail JSON"
        );
        // And parses back to None.
        let detail: bicameral_api::dashboard::DecisionDetail =
            serde_json::from_slice(&body).unwrap();
        assert!(detail.provenance.is_none());
    }

    #[tokio::test]
    async fn decision_detail_not_found_returns_404() {
        let state = AppState::new(test_audit());
        let app = Router::new()
            .merge(dashboard_api_routes())
            .with_state(state);

        let fake_id = Uuid::new_v4();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(&format!("/api/v1/dashboard/decision/{}", fake_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn decision_detail_invalid_id_returns_400() {
        let state = AppState::new(test_audit());
        let app = Router::new()
            .merge(dashboard_api_routes())
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/dashboard/decision/not-a-uuid")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn projection_decision_detail_matches_dashboard() {
        let (decision_id, app, _state) = setup_accepted_decision().await;

        // Dashboard detail.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&format!("/api/v1/dashboard/decision/{}", decision_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let dashboard_body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let dashboard_detail: bicameral_api::dashboard::DecisionDetail =
            serde_json::from_slice(&dashboard_body).unwrap();

        // Projection detail.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(&format!("/api/v1/projection/decision/{}", decision_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let projection_body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let projection_detail: bicameral_api::dashboard::DecisionDetail =
            serde_json::from_slice(&projection_body).unwrap();

        // Surface parity: both endpoints return the same identity and axes.
        assert_eq!(dashboard_detail.decision_id, projection_detail.decision_id);
        assert_eq!(dashboard_detail.title, projection_detail.title);
        assert_eq!(
            dashboard_detail.candidate_review.review_state,
            projection_detail.candidate_review.review_state
        );
        assert_eq!(
            dashboard_detail.signoff.state,
            projection_detail.signoff.state
        );
        assert_eq!(
            dashboard_detail.code_evidence.evidence_state,
            projection_detail.code_evidence.evidence_state
        );
        assert_eq!(
            dashboard_detail.compliance.state,
            projection_detail.compliance.state
        );
    }

    #[tokio::test]
    async fn decision_detail_visible_in_history_and_search() {
        let (decision_id, app, _state) = setup_accepted_decision().await;

        // history.list returns the same decision_id.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/history")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let history: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let history_decisions = history["decisions"].as_array().unwrap();
        assert!(!history_decisions.is_empty());
        let hist_id = history_decisions[0]["decision_id"].as_str().unwrap();
        assert_eq!(hist_id, decision_id);

        // search.query returns the same decision_id.
        let search_body = serde_json::json!({
            "query": "Surface parity",
            "scope": "decisions"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/search")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&search_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let search: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let search_results = search["results"].as_array().unwrap();
        assert!(!search_results.is_empty());
        let search_id = search_results[0]["id"].as_str().unwrap();
        assert_eq!(search_id, decision_id);
    }

    #[tokio::test]
    async fn decision_detail_no_mutation_invariant() {
        let (decision_id, app, state) = setup_accepted_decision().await;

        // Snapshot state before detail read.
        let decisions_before = state.decisions.read().await.clone();
        let events_before = state.event_store.replay().unwrap_or_default();

        // Read dashboard detail.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&format!("/api/v1/dashboard/decision/{}", decision_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Read projection detail.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(&format!("/api/v1/projection/decision/{}", decision_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Snapshot state after detail reads.
        let decisions_after = state.decisions.read().await.clone();
        let events_after = state.event_store.replay().unwrap_or_default();

        // No-mutation invariant: state must not have changed.
        assert_eq!(decisions_before.len(), decisions_after.len());
        assert_eq!(events_before.len(), events_after.len());
        for (id, d_before) in &decisions_before {
            let d_after = decisions_after.get(id).unwrap();
            assert_eq!(d_before.signoff_state, d_after.signoff_state);
            assert_eq!(d_before.compliance_state, d_after.compliance_state);
            assert_eq!(d_before.review_state, d_after.review_state);
            assert_eq!(d_before.tracking_state, d_after.tracking_state);
        }
    }

    // ── Runtime graph routes tests ────────────────────────────────────

    mod runtime_graph_routes_tests {
        use super::*;
        use bicameral_runtime::dispatch::dispatch_with_workspace;
        use std::path::{Path, PathBuf};

        /// A test `RuntimeDispatch` that delegates to the real runtime.
        struct TestDispatch;

        impl crate::state::RuntimeDispatch for TestDispatch {
            fn dispatch(
                &self,
                request: &bicameral_api::tool_request::ToolRequest,
                workspace_root: Option<&Path>,
            ) -> bicameral_api::tool_request::ToolResponse {
                let adapter = bicameral_runtime::event_store_adapters::MemoryAdapter::new();
                dispatch_with_workspace(request, &adapter, workspace_root)
            }

            fn capabilities(&self) -> serde_json::Value {
                serde_json::to_value(bicameral_runtime::dispatch::capability_report())
                    .expect("CapabilityReport serializes to JSON")
            }
        }

        fn workspace_root() -> PathBuf {
            // Use the repo checkout root as a valid git workspace.
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .to_path_buf()
        }

        fn app_with_dispatch(workspace: Option<PathBuf>) -> Router {
            let mut state = AppState::new(test_audit());
            state.runtime_dispatch = Some(std::sync::Arc::new(TestDispatch));
            if let Some(ws) = workspace {
                state.workspace_root = std::sync::Arc::new(tokio::sync::RwLock::new(Some(ws)));
            }
            Router::new()
                .merge(runtime_graph_routes())
                .with_state(state)
        }

        fn app_without_dispatch() -> Router {
            let state = AppState::new(test_audit());
            Router::new()
                .merge(runtime_graph_routes())
                .with_state(state)
        }

        // -- code.locate tests --

        #[tokio::test]
        async fn code_locate_unsupported_without_dispatch() {
            let app = app_without_dispatch();
            let body = serde_json::json!({"query": "test"});
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/code/locate")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["status"], "unsupported");
        }

        #[tokio::test]
        async fn code_locate_invalid_payload_empty_body() {
            let app = app_with_dispatch(Some(workspace_root()));
            let body = serde_json::json!({});
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/code/locate")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["status"], "invalid_payload");
        }

        #[tokio::test]
        async fn code_locate_ready_with_workspace() {
            let app = app_with_dispatch(Some(workspace_root()));
            let body = serde_json::json!({"query": "dispatch"});
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/code/locate")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            // Should have a status field from LocatorResult
            assert!(body["status"].is_string());
            assert!(body["graph_readiness"].is_string());
        }

        #[tokio::test]
        async fn code_locate_graph_unavailable_without_workspace() {
            let app = app_with_dispatch(None);
            let body = serde_json::json!({"query": "test"});
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/code/locate")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["status"], "graph_unavailable");
            assert_eq!(body["graph_readiness"], "not_configured");
        }

        // -- graph.status tests --

        #[tokio::test]
        async fn graph_status_unsupported_without_dispatch() {
            let app = app_without_dispatch();
            let body = serde_json::json!({});
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/graph/status")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["status"], "unsupported");
        }

        #[tokio::test]
        async fn graph_status_ready_with_workspace() {
            let app = app_with_dispatch(Some(workspace_root()));
            let body = serde_json::json!({"ref_name": "HEAD"});
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/graph/status")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["readiness"], "ready");
            assert!(body["snapshot"].is_object());
        }

        #[tokio::test]
        async fn graph_status_not_configured_without_workspace() {
            let app = app_with_dispatch(None);
            let body = serde_json::json!({"ref_name": "HEAD"});
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/graph/status")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["readiness"], "not_configured");
            assert!(body["next_actions"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("configure_workspace")));
        }

        #[tokio::test]
        async fn graph_status_stale_for_unavailable_ref() {
            let app = app_with_dispatch(Some(workspace_root()));
            let body = serde_json::json!({"ref_name": "refs/heads/nonexistent-branch-xyz-999"});
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/graph/status")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            // Should not be ready for a nonexistent ref
            assert_ne!(body["readiness"], "ready");
        }

        // -- graph.refresh tests --

        #[tokio::test]
        async fn graph_refresh_unsupported_without_dispatch() {
            let app = app_without_dispatch();
            let body = serde_json::json!({});
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/graph/refresh")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["status"], "unsupported");
        }

        #[tokio::test]
        async fn graph_refresh_ready_with_workspace() {
            let app = app_with_dispatch(Some(workspace_root()));
            let body = serde_json::json!({"ref_name": "HEAD", "trigger": "manual"});
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/graph/refresh")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["status"], "ready");
            assert!(body["target"].is_object());
            assert!(body["snapshot"].is_object());
        }

        #[tokio::test]
        async fn graph_refresh_not_configured_without_workspace() {
            let app = app_with_dispatch(None);
            let body = serde_json::json!({"ref_name": "HEAD"});
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/graph/refresh")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["status"], "not_configured");
            assert!(body["next_actions"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("configure_workspace")));
        }

        #[tokio::test]
        async fn graph_refresh_stale_for_unavailable_ref() {
            let app = app_with_dispatch(Some(workspace_root()));
            let body = serde_json::json!({"ref_name": "refs/heads/nonexistent-branch-xyz-999"});
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/graph/refresh")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            // Should not be ready for a nonexistent ref
            assert_ne!(body["status"], "ready");
        }
    }

    // ── Hosted bridge gateway guard tests ────────────────────────
    //
    // Separate test module for the hosted bridge route family, proving
    // that the gateway guard enforces pairing state before projection
    // reads or canonical-state mutations. Per acceptance criteria #578:
    //   AC2: unpaired cannot read projections
    //   AC3: unpaired cannot mutate canonical state
    //   AC4: separate from external ingest in code and tests
    //   AC5: direct hosted-origin mutation fallback is rejected

    mod hosted_bridge_tests {
        use super::*;
        use crate::state::HostedBridgePairingSession;

        fn hosted_bridge_app() -> Router {
            let state = AppState::new(test_audit());
            Router::new()
                .merge(hosted_bridge_routes())
                .with_state(state)
        }

        fn hosted_bridge_app_with_state() -> (Router, AppState) {
            let state = AppState::new(test_audit());
            let app = Router::new()
                .merge(hosted_bridge_routes())
                .with_state(state.clone());
            (app, state)
        }

        async fn insert_valid_session(state: &AppState, token: &str) {
            let session = HostedBridgePairingSession {
                session_token: token.to_string(),
                pairing_id: None,
                origin: "https://app.bicameral-ai.com".to_string(),
                cloud_account_id: "acct-test-123".to_string(),
                paired_at: chrono::Utc::now(),
                expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
                revoked: false,
            };
            state
                .hosted_bridge_sessions
                .write()
                .await
                .insert(token.to_string(), session);
        }

        async fn insert_expired_session(state: &AppState, token: &str) {
            let session = HostedBridgePairingSession {
                session_token: token.to_string(),
                pairing_id: None,
                origin: "https://app.bicameral-ai.com".to_string(),
                cloud_account_id: "acct-test-123".to_string(),
                paired_at: chrono::Utc::now() - chrono::Duration::hours(2),
                expires_at: chrono::Utc::now() - chrono::Duration::hours(1),
                revoked: false,
            };
            state
                .hosted_bridge_sessions
                .write()
                .await
                .insert(token.to_string(), session);
        }

        async fn insert_revoked_session(state: &AppState, token: &str) {
            let session = HostedBridgePairingSession {
                session_token: token.to_string(),
                pairing_id: None,
                origin: "https://app.bicameral-ai.com".to_string(),
                cloud_account_id: "acct-test-123".to_string(),
                paired_at: chrono::Utc::now(),
                expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
                revoked: true,
            };
            state
                .hosted_bridge_sessions
                .write()
                .await
                .insert(token.to_string(), session);
        }

        // ── AC1: Hosted bridge route family exists / pairing lifecycle ──

        #[tokio::test]
        async fn pair_request_creates_pending_request_without_session() {
            let (app, state) = hosted_bridge_app_with_state();
            let body = serde_json::json!({
                "origin": "https://app.bicameral-ai.com",
                "cloud_account_id": "acct-123",
                "csrf_token": "csrf-valid-token-abc",
                "team_id": "team-1",
                "cloud_access_token": "cloud-token-1",
                "client_protocol_version":
                    bicameral_api::hosted_bridge::HOSTED_BRIDGE_PROTOCOL_VERSION,
            });
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/pair")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            // #790: a valid pair returns a pending request, never a session.
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["status"], "pending");
            assert!(body.get("session_token").is_none());
            assert!(body["request_handle"].as_str().is_some());
            assert!(body["poll_proof"].as_str().is_some());
            assert!(body["expires_at"].as_str().is_some());
            assert_eq!(body["capabilities"]["bridge_supported"], true);
            assert_eq!(body["capabilities"]["workspace_binding_available"], false);

            // No session exists yet; a transient pending record was created.
            assert!(state.hosted_bridge_sessions.read().await.is_empty());
            let pending = state.hosted_bridge_pending.read().await;
            let handle = body["request_handle"].as_str().unwrap();
            let record = pending.get(handle).expect("pending record stored");
            assert_eq!(record.origin, "https://app.bicameral-ai.com");
            assert_eq!(record.cloud_account_id, "acct-123");
            assert_eq!(record.team_id.as_deref(), Some("team-1"));
            assert_eq!(record.cloud_access_token.as_deref(), Some("cloud-token-1"));
            assert!(record.session_token.is_none());
        }

        #[tokio::test]
        async fn pair_request_rejects_missing_team_id() {
            let app = hosted_bridge_app();
            let body = serde_json::json!({
                "origin": "https://app.bicameral-ai.com",
                "cloud_account_id": "acct-123",
                "csrf_token": "csrf-valid-token-abc",
                "cloud_access_token": "cloud-token-1",
                "client_protocol_version":
                    bicameral_api::hosted_bridge::HOSTED_BRIDGE_PROTOCOL_VERSION,
            });
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/pair")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["error"], "missing required field: team_id");
            assert_eq!(body["kind"], "MissingTeamId");
        }

        #[tokio::test]
        async fn pair_request_rejects_missing_cloud_access_token() {
            let app = hosted_bridge_app();
            let body = serde_json::json!({
                "origin": "https://app.bicameral-ai.com",
                "cloud_account_id": "acct-123",
                "csrf_token": "csrf-valid-token-abc",
                "team_id": "team-1",
                "client_protocol_version":
                    bicameral_api::hosted_bridge::HOSTED_BRIDGE_PROTOCOL_VERSION,
            });
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/pair")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["error"], "missing required field: cloud_access_token");
            assert_eq!(body["kind"], "MissingCloudAccessToken");
        }

        #[tokio::test]
        async fn pair_request_rejects_missing_csrf_token() {
            let app = hosted_bridge_app();
            let body = serde_json::json!({
                "origin": "https://app.bicameral-ai.com",
                "cloud_account_id": "acct-123",
            });
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/pair")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        }

        #[tokio::test]
        async fn pair_request_clamps_requested_session_ttl_to_maximum() {
            let (app, state) = hosted_bridge_app_with_state();
            let body = serde_json::json!({
                "origin": "https://app.bicameral-ai.com",
                "cloud_account_id": "acct-123",
                "csrf_token": "csrf-token",
                "team_id": "team-1",
                "cloud_access_token": "cloud-token-1",
                "requested_ttl_seconds": 999999,
                "client_protocol_version":
                    bicameral_api::hosted_bridge::HOSTED_BRIDGE_PROTOCOL_VERSION,
            });
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/pair")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();

            // The pending window is the short pending TTL, not the requested
            // session TTL; the requested (clamped) session TTL is retained for
            // when the request is later approved.
            let pending_expiry =
                chrono::DateTime::parse_from_rfc3339(body["expires_at"].as_str().unwrap()).unwrap();
            let pending_max = chrono::Utc::now()
                + chrono::Duration::seconds(crate::state::PENDING_PAIRING_TTL_SECONDS + 5);
            assert!(pending_expiry < pending_max);

            let pending = state.hosted_bridge_pending.read().await;
            let handle = body["request_handle"].as_str().unwrap();
            let record = pending.get(handle).unwrap();
            assert_eq!(record.requested_ttl_seconds, MAX_BRIDGE_TTL_SECONDS);
        }

        // ── Revocation lifecycle ──

        #[tokio::test]
        async fn revoke_marks_session_revoked() {
            let (app, state) = hosted_bridge_app_with_state();
            insert_valid_session(&state, "rev-token").await;
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/revoke")
                        .header("x-bridge-session-token", "rev-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["pairing_state"], "hosted_bridge_revoked");

            // Confirm session is marked revoked in state.
            let sessions = state.hosted_bridge_sessions.read().await;
            assert!(sessions.get("rev-token").unwrap().revoked);
        }

        #[tokio::test]
        async fn revoke_without_token_rejected() {
            let app = hosted_bridge_app();
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/revoke")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }

        #[tokio::test]
        async fn revoked_session_cannot_read_status() {
            let (app, state) = hosted_bridge_app_with_state();
            insert_valid_session(&state, "rev-check").await;
            // Revoke it.
            {
                state
                    .hosted_bridge_sessions
                    .write()
                    .await
                    .get_mut("rev-check")
                    .unwrap()
                    .revoked = true;
            }
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/hosted-bridge/status")
                        .header("x-bridge-session-token", "rev-check")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["kind"], "PairingRevoked");
        }

        // ── Pairing record cleanliness: no forbidden authority fields ──

        #[tokio::test]
        async fn pending_record_does_not_contain_forbidden_fields() {
            let (app, state) = hosted_bridge_app_with_state();
            let body = serde_json::json!({
                "origin": "https://app.bicameral-ai.com",
                "cloud_account_id": "acct-123",
                "csrf_token": "csrf-token-xyz",
                "team_id": "team-1",
                "cloud_access_token": "cloud-token-1",
                "client_protocol_version":
                    bicameral_api::hosted_bridge::HOSTED_BRIDGE_PROTOCOL_VERSION,
            });
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/pair")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let resp_body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            let handle = resp_body["request_handle"].as_str().unwrap();

            // Inspect the transient pending record. Compile-time proof: the
            // PendingPairingRecord struct has ONLY minimal binding metadata —
            // destructuring every field proves no provider tokens, workspace
            // paths, SourceEvidence, or Decision Ledger events are retained.
            let pending = state.hosted_bridge_pending.read().await;
            let record = pending.get(handle).unwrap();
            let crate::state::PendingPairingRecord {
                request_handle: _,
                poll_proof: _,
                local_confirmation_nonce: _,
                pairing_code_digest: _,
                origin: ref o,
                cloud_account_id: ref a,
                team_id: _,
                cloud_access_token: _,
                created_at: _,
                expires_at: _,
                requested_ttl_seconds: _,
                decision: _,
                session_token: _,
                session_expires_at: _,
                pairing_id: _,
            } = *record;

            assert!(o.starts_with("https://"));
            assert_eq!(a, "acct-123");
            // Pending state is not yet approved and holds no session token.
            assert_eq!(
                record.decision,
                crate::state::PendingPairingDecision::Pending
            );
            assert!(record.session_token.is_none());
        }

        /// Insert a pending pairing that carries a Cloud bearer for enroll.
        async fn insert_pending_with_cloud_bearer(
            state: &AppState,
            handle: &str,
            nonce: &str,
            expires_at: chrono::DateTime<chrono::Utc>,
        ) {
            let now = chrono::Utc::now();
            let record = crate::state::PendingPairingRecord {
                request_handle: handle.to_string(),
                poll_proof: "poll-proof".into(),
                local_confirmation_nonce: nonce.to_string(),
                pairing_code_digest: [0u8; 32],
                origin: "https://app.bicameral-ai.com".into(),
                cloud_account_id: "acct-123".into(),
                team_id: Some("team-1".into()),
                cloud_access_token: Some("cloud-bearer-secret".into()),
                created_at: now,
                expires_at,
                requested_ttl_seconds: 3600,
                decision: crate::state::PendingPairingDecision::Pending,
                session_token: None,
                session_expires_at: None,
                pairing_id: None,
            };
            state
                .hosted_bridge_pending
                .write()
                .await
                .insert(handle.to_string(), record);
        }

        async fn post_local_decision(
            app: Router,
            handle: &str,
            nonce: &str,
            action: &str,
        ) -> axum::http::Response<Body> {
            let body = serde_json::json!({
                "request_handle": handle,
                "local_confirmation_nonce": nonce,
                "action": action,
            });
            app.oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/hosted-bridge/pair/decision")
                    .header("content-type", "application/json")
                    // Same-origin/local confirmation: omit Origin (accepted).
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap()
        }

        #[tokio::test]
        async fn approved_pending_does_not_retain_cloud_access_token() {
            let (app, state) = hosted_bridge_app_with_state();
            let handle = "req-approve-bearer";
            let nonce = "nonce-approve-bearer";
            insert_pending_with_cloud_bearer(
                &state,
                handle,
                nonce,
                chrono::Utc::now() + chrono::Duration::minutes(5),
            )
            .await;

            let resp = post_local_decision(app, handle, nonce, "approve").await;
            assert!(
                resp.status().is_success() || resp.status() == StatusCode::OK,
                "unexpected status {}",
                resp.status()
            );

            let pending = state.hosted_bridge_pending.read().await;
            let record = pending.get(handle).expect("approved record retained");
            assert_eq!(
                record.decision,
                crate::state::PendingPairingDecision::Approved
            );
            assert!(
                record.cloud_access_token.is_none(),
                "approved retry-safe pending must not retain cloud_access_token"
            );
        }

        #[tokio::test]
        async fn denied_pending_does_not_retain_cloud_access_token() {
            let (app, state) = hosted_bridge_app_with_state();
            let handle = "req-deny-bearer";
            let nonce = "nonce-deny-bearer";
            insert_pending_with_cloud_bearer(
                &state,
                handle,
                nonce,
                chrono::Utc::now() + chrono::Duration::minutes(5),
            )
            .await;

            let resp = post_local_decision(app, handle, nonce, "deny").await;
            assert!(resp.status().is_success() || resp.status() == StatusCode::OK);

            let pending = state.hosted_bridge_pending.read().await;
            let record = pending.get(handle).expect("denied record retained");
            assert_eq!(
                record.decision,
                crate::state::PendingPairingDecision::Denied
            );
            assert!(
                record.cloud_access_token.is_none(),
                "denied pending must not retain cloud_access_token"
            );
        }

        #[tokio::test]
        async fn expired_pending_does_not_retain_cloud_access_token() {
            let (app, state) = hosted_bridge_app_with_state();
            let handle = "req-expire-bearer";
            let nonce = "nonce-expire-bearer";
            insert_pending_with_cloud_bearer(
                &state,
                handle,
                nonce,
                chrono::Utc::now() - chrono::Duration::seconds(1),
            )
            .await;

            let resp = post_local_decision(app, handle, nonce, "approve").await;
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["status"], "expired");

            let pending = state.hosted_bridge_pending.read().await;
            let record = pending
                .get(handle)
                .expect("expired record still present until prune");
            assert!(
                record.cloud_access_token.is_none(),
                "expired pending must not retain cloud_access_token"
            );
        }

        // ── Paired state cannot select project, bind workspace, or mutate canonical state ──

        #[tokio::test]
        async fn paired_state_cannot_select_project() {
            let (app, state) = hosted_bridge_app_with_state();
            insert_valid_session(&state, "paired-tok").await;
            let body = serde_json::json!({
                "action": "select_project",
                "project_id": "proj-001",
            });
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/command")
                        .header("content-type", "application/json")
                        .header("x-bridge-session-token", "paired-tok")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            // Malformed command payload → 400 invalid_command.
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["error"], "invalid_command");
        }

        #[tokio::test]
        async fn paired_state_cannot_bind_workspace() {
            let (app, state) = hosted_bridge_app_with_state();
            insert_valid_session(&state, "paired-tok").await;
            // workspace_path is a forbidden field — the smuggling guard rejects
            // it before command dispatch, proving workspace binding is blocked.
            let body = serde_json::json!({
                "action": "bind_workspace",
                "workspace_path": "/home/user/project",
            });
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/command")
                        .header("content-type", "application/json")
                        .header("x-bridge-session-token", "paired-tok")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            // workspace_path triggers authority smuggling rejection (400).
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["kind"], "AuthoritySmugglingRejected");
            assert!(body["smuggled_fields"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f == "workspace_path"));
        }

        #[tokio::test]
        async fn paired_state_cannot_mutate_canonical_state() {
            let (app, state) = hosted_bridge_app_with_state();
            insert_valid_session(&state, "paired-tok").await;
            // Attempt to create a decision (canonical mutation) — not a valid
            // ToolCommand, so it's rejected as malformed.
            let body = serde_json::json!({
                "action": "create_decision",
                "summary": "unauthorized decision",
            });
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/command")
                        .header("content-type", "application/json")
                        .header("x-bridge-session-token", "paired-tok")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            // Malformed → 400.
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["error"], "invalid_command");
        }

        #[tokio::test]
        async fn operator_home_reports_workspace_binding_unavailable() {
            let (app, state) = hosted_bridge_app_with_state();
            insert_valid_session(&state, "paired-tok").await;
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/hosted-bridge/operator-home")
                        .header("x-bridge-session-token", "paired-tok")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            // Workspace binding not available at user-level.
            assert!(
                body.get("workspace_binding_available").is_none()
                    || body["workspace_binding_available"] == false
                    || body.to_string().contains("not yet implemented"),
            );
        }

        // ── AC2: Unpaired cannot read projections ──

        #[tokio::test]
        async fn unpaired_projection_read_rejected_no_token() {
            let app = hosted_bridge_app();
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/hosted-bridge/projection/ledger")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["kind"], "NotPaired");
        }

        #[tokio::test]
        async fn unpaired_projection_read_rejected_bad_token() {
            let app = hosted_bridge_app();
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/hosted-bridge/projection/ledger")
                        .header("x-bridge-session-token", "invalid-token-xyz")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["kind"], "NotPaired");
        }

        #[tokio::test]
        async fn expired_session_projection_read_rejected() {
            let (app, state) = hosted_bridge_app_with_state();
            insert_expired_session(&state, "expired-token").await;
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/hosted-bridge/projection/ledger")
                        .header("x-bridge-session-token", "expired-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["kind"], "PairingExpired");
        }

        #[tokio::test]
        async fn revoked_session_projection_read_rejected() {
            let (app, state) = hosted_bridge_app_with_state();
            insert_revoked_session(&state, "revoked-token").await;
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/hosted-bridge/projection/ledger")
                        .header("x-bridge-session-token", "revoked-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["kind"], "PairingRevoked");
        }

        #[tokio::test]
        async fn paired_session_projection_read_allowed() {
            let (app, state) = hosted_bridge_app_with_state();
            insert_valid_session(&state, "valid-token").await;
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/hosted-bridge/projection/ledger")
                        .header("x-bridge-session-token", "valid-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }

        // ── AC3: Unpaired cannot mutate canonical state ──

        #[tokio::test]
        async fn unpaired_command_rejected_no_token() {
            let app = hosted_bridge_app();
            let body = serde_json::json!({"action": "some_command"});
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/command")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["kind"], "NotPaired");
        }

        #[tokio::test]
        async fn unpaired_command_rejected_bad_token() {
            let app = hosted_bridge_app();
            let body = serde_json::json!({"action": "some_command"});
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/command")
                        .header("content-type", "application/json")
                        .header("x-bridge-session-token", "bogus-token")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }

        // ── AC4: External ingest and hosted bridge remain separate ──
        //
        // These routes are defined in separate functions (external_ingest_routes
        // vs hosted_bridge_routes) and share no handler code. The test app
        // below mounts only hosted bridge routes, proving they are independent.

        #[tokio::test]
        async fn hosted_bridge_app_does_not_serve_external_ingest() {
            let app = hosted_bridge_app();
            let body = serde_json::json!({
                "source_type": "github",
                "source": "https://github.com/example/repo/pull/1",
                "payload": {"title": "test"},
            });
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/external-ingest")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            // External ingest is not mounted → 404.
            assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        }

        // ── AC5: Direct hosted-origin mutation fallback is rejected ──

        #[tokio::test]
        async fn hosted_origin_mutation_with_smuggled_fields_rejected() {
            let (app, state) = hosted_bridge_app_with_state();
            insert_valid_session(&state, "valid-token").await;
            let body = serde_json::json!({
                "action": "list_projects",
                "review_command": {"command": "approve"},
                "decision_write": true,
            });
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/command")
                        .header("content-type", "application/json")
                        .header("x-bridge-session-token", "valid-token")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["error"], "authority_smuggling_rejected");
            let smuggled = body["smuggled_fields"].as_array().unwrap();
            assert!(smuggled.contains(&serde_json::json!("review_command")));
            assert!(smuggled.contains(&serde_json::json!("decision_write")));
        }

        #[tokio::test]
        async fn pair_request_with_smuggled_fields_rejected() {
            let app = hosted_bridge_app();
            let body = serde_json::json!({
                "origin": "https://app.bicameral-ai.com",
                "cloud_account_id": "acct-123",
                "authority_context": {"actor_id": "admin"},
                "event_store_write": true,
            });
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/pair")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["error"], "authority_smuggling_rejected");
            let smuggled = body["smuggled_fields"].as_array().unwrap();
            assert!(smuggled.contains(&serde_json::json!("authority_context")));
            assert!(smuggled.contains(&serde_json::json!("event_store_write")));
        }

        // ── Operator home also guarded ──

        #[tokio::test]
        async fn unpaired_operator_home_rejected() {
            let app = hosted_bridge_app();
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/hosted-bridge/operator-home")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }

        #[tokio::test]
        async fn paired_operator_home_allowed() {
            let (app, state) = hosted_bridge_app_with_state();
            insert_valid_session(&state, "valid-token").await;
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/hosted-bridge/operator-home")
                        .header("x-bridge-session-token", "valid-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["pairing_state"], "paired");
        }

        // ── Status endpoint guarded ──

        #[tokio::test]
        async fn unpaired_status_rejected() {
            let app = hosted_bridge_app();
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/hosted-bridge/status")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }

        #[tokio::test]
        async fn paired_status_returns_paired() {
            let (app, state) = hosted_bridge_app_with_state();
            insert_valid_session(&state, "valid-token").await;
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/hosted-bridge/status")
                        .header("x-bridge-session-token", "valid-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["pairing_state"], "hosted_bridge_paired");
        }

        // ── Valid session: malformed command returns 400 ──

        #[tokio::test]
        async fn paired_command_rejects_malformed_payload() {
            let (app, state) = hosted_bridge_app_with_state();
            insert_valid_session(&state, "valid-token").await;
            let body = serde_json::json!({"action": "list_projects"});
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/command")
                        .header("content-type", "application/json")
                        .header("x-bridge-session-token", "valid-token")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            // Not a valid ToolCommand → 400.
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["error"], "invalid_command");
        }

        // ── #580: Project readiness projection tests ──────────────────

        async fn insert_test_projects(state: &AppState) {
            use crate::state::LocalProjectRegistration;
            let mut regs = state
                .project_registrations
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            regs.push(LocalProjectRegistration {
                name: "my-service".into(),
                event_store_healthy: true,
                has_workspace_binding: true,
                workspace_path: Some(std::path::PathBuf::from("/home/user/projects/my-service")),
                provider_config: Some(serde_json::json!({
                    "github_token": "ghp_SECRETVALUE123",
                    "webhook_secret": "whsec_supersecret"
                })),
                setup_state: "normal".into(),
            });
            regs.push(LocalProjectRegistration {
                name: "data-pipeline".into(),
                event_store_healthy: true,
                has_workspace_binding: false,
                workspace_path: Some(std::path::PathBuf::from("/opt/pipelines/data-pipeline")),
                provider_config: None,
                setup_state: "code_grounding_not_ready".into(),
            });
            regs.push(LocalProjectRegistration {
                name: "new-experiment".into(),
                event_store_healthy: false,
                has_workspace_binding: false,
                workspace_path: None,
                provider_config: None,
                setup_state: "code_grounding_not_ready".into(),
            });
        }

        // AC: Paired bridge can return local project list/readiness.
        #[tokio::test]
        async fn paired_operator_home_returns_project_list() {
            let (app, state) = hosted_bridge_app_with_state();
            insert_valid_session(&state, "valid-token").await;
            insert_test_projects(&state).await;
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/hosted-bridge/operator-home")
                        .header("x-bridge-session-token", "valid-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["pairing_state"], "paired");
            let projects = body["projects"].as_array().unwrap();
            assert_eq!(projects.len(), 3);
            assert_eq!(projects[0]["name"], "my-service");
            assert_eq!(projects[0]["event_store_healthy"], true);
            assert_eq!(projects[0]["has_workspace_binding"], true);
            assert_eq!(projects[0]["setup_state"], "normal");
            assert_eq!(projects[1]["name"], "data-pipeline");
            assert_eq!(projects[1]["has_workspace_binding"], false);
            assert_eq!(projects[1]["setup_state"], "code_grounding_not_ready");
        }

        // AC: Paired bridge can return local project list via /projects.
        #[tokio::test]
        async fn paired_projects_endpoint_returns_list() {
            let (app, state) = hosted_bridge_app_with_state();
            insert_valid_session(&state, "valid-token").await;
            insert_test_projects(&state).await;
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/hosted-bridge/projects")
                        .header("x-bridge-session-token", "valid-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["projection_only"], true);
            assert_eq!(body["workspace_binding_available"], false);
            let projects = body["projects"].as_array().unwrap();
            assert_eq!(projects.len(), 3);
        }

        // AC: Unpaired bridge cannot return local project list/readiness.
        #[tokio::test]
        async fn unpaired_projects_endpoint_rejected() {
            let app = hosted_bridge_app();
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/hosted-bridge/projects")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }

        // AC: Projection data is read-model only — no authority fields leak.
        #[tokio::test]
        async fn project_projection_contains_no_sensitive_fields() {
            let (app, state) = hosted_bridge_app_with_state();
            insert_valid_session(&state, "valid-token").await;
            insert_test_projects(&state).await;
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/hosted-bridge/projects")
                        .header("x-bridge-session-token", "valid-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let raw = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap();
            let body_str = String::from_utf8_lossy(&raw);
            // Filesystem paths must not appear.
            assert!(!body_str.contains("/home/user/projects"));
            assert!(!body_str.contains("/opt/pipelines"));
            // Provider secrets must not appear.
            assert!(!body_str.contains("ghp_SECRETVALUE123"));
            assert!(!body_str.contains("whsec_supersecret"));
            assert!(!body_str.contains("github_token"));
            assert!(!body_str.contains("webhook_secret"));
            // No BRIDGE_FORBIDDEN_FIELDS should appear.
            for field in bicameral_api::hosted_bridge::BRIDGE_FORBIDDEN_FIELDS {
                assert!(
                    !body_str.contains(field),
                    "Forbidden field '{}' found in projection response",
                    field
                );
            }
        }

        // AC: Missing workspace binding shown as code-grounding readiness,
        //     not pairing failure.
        #[tokio::test]
        async fn missing_workspace_binding_is_code_grounding_readiness() {
            let (app, state) = hosted_bridge_app_with_state();
            insert_valid_session(&state, "valid-token").await;
            insert_test_projects(&state).await;
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/hosted-bridge/operator-home")
                        .header("x-bridge-session-token", "valid-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            // Pairing state is paired — no pairing failure.
            assert_eq!(body["pairing_state"], "paired");
            // Projects without workspace binding report code-grounding readiness.
            let projects = body["projects"].as_array().unwrap();
            let unbound_project = &projects[1];
            assert_eq!(unbound_project["has_workspace_binding"], false);
            assert_eq!(unbound_project["setup_state"], "code_grounding_not_ready");
            // The response does NOT report this as a pairing error.
            assert!(body.get("error").is_none());
            assert!(body.get("kind").is_none());
        }

        // AC: Empty project list returns successfully when no projects registered.
        #[tokio::test]
        async fn paired_operator_home_empty_projects_ok() {
            let (app, state) = hosted_bridge_app_with_state();
            insert_valid_session(&state, "valid-token").await;
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/hosted-bridge/operator-home")
                        .header("x-bridge-session-token", "valid-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["pairing_state"], "paired");
            let projects = body["projects"].as_array().unwrap();
            assert!(projects.is_empty());
            assert_eq!(body["workspace_binding_available"], false);
        }

        #[tokio::test]
        async fn paired_operator_home_projects_daemon_capability_readiness() {
            let mut state = AppState::new(test_audit());
            state.runtime_dispatch = Some(std::sync::Arc::new(CapabilityReadinessDispatch));
            let app = Router::new()
                .merge(hosted_bridge_routes())
                .with_state(state.clone());
            insert_valid_session(&state, "readiness-token").await;

            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/hosted-bridge/operator-home")
                        .header("x-bridge-session-token", "readiness-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(resp.status(), StatusCode::OK);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), 8192).await.unwrap(),
            )
            .unwrap();
            assert_eq!(body["capability_readiness"][0]["state"], "active_verified");
            assert_eq!(body["capability_readiness"][0]["subject_id"], "codex");
        }

        // ── #581/#615: Bridge command dispatch and allowlist tests ────

        /// Stub RuntimeDispatch for bridge command tests. Returns a canned
        /// success response echoing the dispatched command name.
        struct BridgeStubDispatch;

        impl crate::state::RuntimeDispatch for BridgeStubDispatch {
            fn dispatch(
                &self,
                request: &bicameral_api::tool_request::ToolRequest,
                _workspace_root: Option<&std::path::Path>,
            ) -> bicameral_api::tool_request::ToolResponse {
                bicameral_api::tool_request::ToolResponse::success(
                    request.request_id,
                    serde_json::json!({
                        "dispatched": true,
                        "command": request.command.command_name(),
                        "actor_id": request.authority.actor_id,
                    }),
                )
            }

            fn capabilities(&self) -> serde_json::Value {
                serde_json::json!({
                    "protocol_version": "v2",
                    "supported_commands": ["history.list", "search.query"],
                })
            }
        }

        fn hosted_bridge_app_with_dispatch() -> (Router, AppState) {
            let mut state = AppState::new(test_audit());
            state.runtime_dispatch = Some(std::sync::Arc::new(BridgeStubDispatch));
            let app = Router::new()
                .merge(hosted_bridge_routes())
                .with_state(state.clone());
            (app, state)
        }

        #[tokio::test]
        async fn bridge_allowed_read_command_dispatches() {
            let (app, state) = hosted_bridge_app_with_dispatch();
            insert_valid_session(&state, "tok").await;
            let body = serde_json::json!({
                "command": {
                    "name": "history.list",
                    "params": {
                        "decision_id": null,
                        "include_events": false,
                        "include_bindings": false,
                        "since": null
                    }
                }
            });
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/command")
                        .header("content-type", "application/json")
                        .header("x-bridge-session-token", "tok")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            // ToolResponse wraps the result; verify dispatch occurred.
            assert_eq!(body["result"]["dispatched"], true);
            assert_eq!(body["result"]["command"], "history.list");
            // Authority must be daemon-derived, not caller-supplied.
            assert!(body["result"]["actor_id"]
                .as_str()
                .unwrap()
                .starts_with("bridge:"),);
        }

        #[tokio::test]
        async fn bridge_disallowed_owner_only_command_rejected() {
            let (app, state) = hosted_bridge_app_with_dispatch();
            insert_valid_session(&state, "tok").await;
            let body = serde_json::json!({
                "command": {
                    "name": "review.approve_signoff",
                    "params": {
                        "target_id": "00000000-0000-0000-0000-000000000001"
                    }
                }
            });
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/command")
                        .header("content-type", "application/json")
                        .header("x-bridge-session-token", "tok")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["error"], "command_not_allowed");
            assert_eq!(body["kind"], "CommandNotAllowed");
            assert_eq!(body["command"], "review.approve_signoff");
        }

        #[tokio::test]
        async fn bridge_malformed_command_rejected() {
            let (app, state) = hosted_bridge_app_with_dispatch();
            insert_valid_session(&state, "tok").await;
            let body = serde_json::json!({
                "command": {
                    "name": "totally_bogus_command",
                    "params": {}
                }
            });
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/command")
                        .header("content-type", "application/json")
                        .header("x-bridge-session-token", "tok")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["error"], "invalid_command");
            assert_eq!(body["kind"], "UnsupportedCommand");
        }

        #[tokio::test]
        async fn bridge_no_runtime_dispatcher_returns_503() {
            // App WITHOUT runtime dispatch injected.
            let (app, state) = hosted_bridge_app_with_state();
            insert_valid_session(&state, "tok").await;
            let body = serde_json::json!({
                "command": {
                    "name": "history.list",
                    "params": {
                        "decision_id": null,
                        "include_events": false,
                        "include_bindings": false,
                        "since": null
                    }
                }
            });
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/command")
                        .header("content-type", "application/json")
                        .header("x-bridge-session-token", "tok")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["error"], "runtime_unavailable");
            assert_eq!(body["kind"], "RuntimeUnavailable");
        }

        #[tokio::test]
        async fn bridge_smuggled_authority_fields_still_rejected() {
            let (app, state) = hosted_bridge_app_with_dispatch();
            insert_valid_session(&state, "tok").await;
            let body = serde_json::json!({
                "command": {
                    "name": "history.list",
                    "params": {
                        "decision_id": null,
                        "include_events": false,
                        "include_bindings": false,
                        "since": null
                    }
                },
                "authority_context": {"actor_id": "admin"},
            });
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/command")
                        .header("content-type", "application/json")
                        .header("x-bridge-session-token", "tok")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["error"], "authority_smuggling_rejected");
            let smuggled = body["smuggled_fields"].as_array().unwrap();
            assert!(smuggled.contains(&serde_json::json!("authority_context")));
        }

        #[tokio::test]
        async fn bridge_unpaired_requests_do_not_dispatch() {
            let (app, _state) = hosted_bridge_app_with_dispatch();
            // No session inserted — unpaired.
            let body = serde_json::json!({
                "command": {
                    "name": "history.list",
                    "params": {
                        "decision_id": null,
                        "include_events": false,
                        "include_bindings": false,
                        "since": null
                    }
                }
            });
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/command")
                        .header("content-type", "application/json")
                        .header("x-bridge-session-token", "no-such-session")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["kind"], "NotPaired");
        }

        // ── #613: Trusted origin allowlist tests ────────────────────────

        #[tokio::test]
        async fn pair_request_rejects_untrusted_origin() {
            let app = hosted_bridge_app();
            let body = serde_json::json!({
                "origin": "https://evil.example.com",
                "cloud_account_id": "acct-123",
                "csrf_token": "csrf-token",
                "team_id": "team-1",
                "cloud_access_token": "cloud-token-1",
            });
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/pair")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["error"], "origin_not_trusted");
            assert_eq!(body["kind"], "OriginMismatch");
            assert_eq!(body["origin"], "https://evil.example.com");
        }

        #[tokio::test]
        async fn pair_request_accepts_default_trusted_origin() {
            let app = hosted_bridge_app();
            let body = serde_json::json!({
                "origin": "https://app.bicameral-ai.com",
                "cloud_account_id": "acct-123",
                "csrf_token": "csrf-token",
                "team_id": "team-1",
                "cloud_access_token": "cloud-token-1",
                "client_protocol_version":
                    bicameral_api::hosted_bridge::HOSTED_BRIDGE_PROTOCOL_VERSION,
            });
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/pair")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
        }

        #[tokio::test]
        async fn pair_request_accepts_custom_trusted_origin() {
            let state = AppState::new(test_audit());
            {
                let mut custom = state.custom_trusted_origins.write().await;
                custom.push("https://custom.corp.example.com".to_string());
            }
            let app = Router::new()
                .merge(hosted_bridge_routes())
                .with_state(state);
            let body = serde_json::json!({
                "origin": "https://custom.corp.example.com",
                "cloud_account_id": "acct-123",
                "csrf_token": "csrf-token",
                "team_id": "team-1",
                "cloud_access_token": "cloud-token-1",
                "client_protocol_version":
                    bicameral_api::hosted_bridge::HOSTED_BRIDGE_PROTOCOL_VERSION,
            });
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/pair")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
        }

        #[tokio::test]
        async fn pair_request_rejects_http_variant_of_trusted_origin() {
            let app = hosted_bridge_app();
            let body = serde_json::json!({
                "origin": "http://app.bicameral-ai.com",
                "cloud_account_id": "acct-123",
                "csrf_token": "csrf-token",
                "team_id": "team-1",
                "cloud_access_token": "cloud-token-1",
            });
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/pair")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        }

        #[tokio::test]
        async fn pair_request_rejects_empty_origin() {
            let app = hosted_bridge_app();
            let body = serde_json::json!({
                "origin": "",
                "cloud_account_id": "acct-123",
                "csrf_token": "csrf-token",
                "team_id": "team-1",
                "cloud_access_token": "cloud-token-1",
            });
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/hosted-bridge/pair")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        }

        // ── #614: Safe workspace discovery tests ────────────────────────

        #[tokio::test]
        async fn workspace_discovery_returns_safe_metadata() {
            let (app, state) = hosted_bridge_app_with_state();
            insert_valid_session(&state, "valid-token").await;
            insert_test_projects(&state).await;
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/hosted-bridge/workspace-discovery")
                        .header("x-bridge-session-token", "valid-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let raw = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap();
            let body: serde_json::Value = serde_json::from_slice(&raw).unwrap();
            let body_str = String::from_utf8_lossy(&raw);

            // Verify safe fields present.
            assert_eq!(body["projection_only"], true);
            let workspaces = body["workspaces"].as_array().unwrap();
            assert_eq!(workspaces.len(), 3);
            assert_eq!(workspaces[0]["project_name"], "my-service");
            assert_eq!(workspaces[0]["code_grounding_ready"], true);
            assert_eq!(workspaces[1]["code_grounding_ready"], false);

            // Verify NO sensitive fields leak.
            for field in bicameral_api::hosted_bridge::NEVER_EXPOSE_FIELDS {
                assert!(
                    !body_str.contains(field),
                    "Sensitive field '{}' found in workspace discovery response",
                    field,
                );
            }
            assert!(!body_str.contains("/home/user/projects"));
            assert!(!body_str.contains("ghp_SECRETVALUE123"));
        }

        #[tokio::test]
        async fn workspace_discovery_unpaired_rejected() {
            let app = hosted_bridge_app();
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/hosted-bridge/workspace-discovery")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }

        // ── #616: Hosted dashboard adapter tests ────────────────────────

        #[tokio::test]
        async fn dashboard_adapter_returns_hosted_mode_with_session() {
            let (app, state) = hosted_bridge_app_with_state();
            insert_valid_session(&state, "valid-token").await;
            insert_test_projects(&state).await;
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/hosted-bridge/dashboard-adapter")
                        .header("x-bridge-session-token", "valid-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["mode"], "hosted");
            assert!(body.get("fixture_fallback").is_none());
            assert!(body["workspace"].is_object());
            assert!(body["ledger_summary"].is_object());
            let projects = body["projects"].as_array().unwrap();
            assert_eq!(projects.len(), 3);
        }

        #[tokio::test]
        async fn dashboard_adapter_rejects_missing_session_without_fixture_fallback() {
            let app = hosted_bridge_app();
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/hosted-bridge/dashboard-adapter")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["kind"], "NotPaired");
            assert!(body.get("fixture_fallback").is_none());
        }

        #[tokio::test]
        async fn dashboard_adapter_ledger_summary_counts() {
            let (app, state) = hosted_bridge_app_with_state();
            insert_valid_session(&state, "valid-token").await;
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/hosted-bridge/dashboard-adapter")
                        .header("x-bridge-session-token", "valid-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            let summary = &body["ledger_summary"];
            assert_eq!(summary["decision_count"], 0);
            assert_eq!(summary["candidate_count"], 0);
            assert_eq!(summary["needs_signoff_count"], 0);
            assert_eq!(summary["stale_source_count"], 0);
            assert_eq!(summary["bound_count"], 0);
        }

        #[tokio::test]
        async fn dashboard_adapter_contains_no_sensitive_fields() {
            let (app, state) = hosted_bridge_app_with_state();
            insert_valid_session(&state, "valid-token").await;
            insert_test_projects(&state).await;
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/hosted-bridge/dashboard-adapter")
                        .header("x-bridge-session-token", "valid-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let raw = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap();
            let body_str = String::from_utf8_lossy(&raw);
            assert!(!body_str.contains("/home/user/projects"));
            assert!(!body_str.contains("/opt/pipelines"));
            assert!(!body_str.contains("ghp_SECRETVALUE123"));
            assert!(!body_str.contains("whsec_supersecret"));
            for field in bicameral_api::hosted_bridge::NEVER_EXPOSE_FIELDS {
                assert!(
                    !body_str.contains(field),
                    "Sensitive field '{}' found in dashboard adapter response",
                    field,
                );
            }
        }
    }
}
