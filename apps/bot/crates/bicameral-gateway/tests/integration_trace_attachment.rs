use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use axum::Router;
use bicameral_api::event_store::{EventStoreEntry, EventType};
use bicameral_api::evidence::SubmissionProvenance;
use bicameral_api::evidence_acceptance::EvidenceAcceptedPayload;
use bicameral_api::transformation_trace::{
    TransformationTraceAdvisory, TransformationTraceAuthority, TransformationTraceComponent,
    TransformationTraceContract, TransformationTraceDelta, TransformationTraceEvaluation,
    TransformationTraceHardGate, TransformationTracePayload, TransformationTracePersistence,
    TransformationTraceReceipt, TransformationTraceRecord, TransformationTraceSet,
    TransformationTraceTiming, TRANSFORMATION_TRACE_SCHEMA, TRANSFORMATION_TRACE_SET_SCHEMA,
};
use bicameral_audit::store::AuditStore;
use bicameral_gateway::integration_trace::integration_trace_routes;
use bicameral_gateway::journey_review::journey_review_routes;
use bicameral_gateway::live_projection_work::LiveProjectionWorkStore;
use bicameral_gateway::projection_work_repository::{
    ProjectionOutcomeKind, ProjectionWorkOutcome, ProjectionWorkRepository, ProjectionWorkRequest,
    SqliteProjectionWorkRepository,
};
use bicameral_gateway::AppState;
use chrono::{Duration, Utc};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

const JOURNEY_ID: &str = "journey-http-trace-1";
const TRACE_ID: &str = "trace-http-1";

struct Harness {
    _dir: tempfile::TempDir,
    app: Router,
}

fn harness() -> Harness {
    let dir = tempfile::tempdir().expect("tempdir");
    let audit = AuditStore::open(&dir.path().join("audit")).expect("audit store");
    let repository = Arc::new(
        SqliteProjectionWorkRepository::open(dir.path().join("projection.sqlite3"))
            .expect("projection repository"),
    );
    let live_store =
        Arc::new(LiveProjectionWorkStore::open(repository.clone()).expect("live projection store"));
    let state = AppState::new(audit).with_projection_work_store(live_store);

    let stored = state
        .evidence_repository
        .put(b"screened github evidence")
        .expect("evidence body");
    let now = Utc::now();
    let acceptance = EvidenceAcceptedPayload {
        idempotency_key: JOURNEY_ID.into(),
        evidence_identity: "github:issue:42".into(),
        source_system: "github".into(),
        source_uri: "https://github.example/issues/42".into(),
        product_scope: Some("product-1".into()),
        source_version: Some("sha-1".into()),
        snapshot_addr: "sha256:snapshot".into(),
        content_digest: stored.digest,
        content_locator: stored.locator,
        content_byte_length: stored.byte_length,
        evidence_ids: Vec::new(),
        provenance: SubmissionProvenance::from_external_adapter(
            "github",
            "https://github.example/issues/42",
        ),
        redaction_receipt: None,
        accepted_at: now,
    };
    state
        .event_store
        .append(EventStoreEntry::new(
            EventType::EvidenceAccepted,
            serde_json::to_value(acceptance).expect("acceptance payload"),
            "gateway".into(),
        ))
        .expect("append acceptance");

    let pending = ProjectionWorkRequest::pending(
        JOURNEY_ID,
        "github",
        "https://github.example/issues/42",
        now,
    );
    assert!(repository.enqueue(&pending).expect("enqueue request"));
    let claimed = repository
        .claim_next(now, now + Duration::minutes(5))
        .expect("claim request")
        .expect("claimed request");
    repository
        .record_outcome(
            &ProjectionWorkOutcome {
                request_id: claimed.request_id,
                attempt: claimed.attempt,
                kind: ProjectionOutcomeKind::NoCandidate,
                reason_code: Some("content_not_decision_like".into()),
                retryable: false,
                details: json!({"outcome_label": "no_candidate"}),
                completed_at: now + Duration::seconds(1),
            },
            None,
        )
        .expect("record no-candidate outcome");

    let app = Router::new()
        .merge(journey_review_routes())
        .merge(integration_trace_routes())
        .with_state(state);
    Harness { _dir: dir, app }
}

fn digest(value: &Value) -> String {
    let encoded = serde_json::to_vec(&canonicalize(value)).expect("serialize canonical JSON");
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

fn record(
    phase_id: &str,
    contract_id: &str,
    input_data: Value,
    output_data: Value,
) -> TransformationTraceRecord {
    TransformationTraceRecord {
        schema: TRANSFORMATION_TRACE_SCHEMA.into(),
        journey_id: JOURNEY_ID.into(),
        trace_id: TRACE_ID.into(),
        phase_id: phase_id.into(),
        occurred_at: "2026-07-19T20:00:00Z".into(),
        component: TransformationTraceComponent {
            id: "ELM-INTEGRATIONS".into(),
            repository: "BicameralAI/bicameral-integrations".into(),
            revision: "a".repeat(40),
            function: format!("integration.{phase_id}"),
        },
        contract: TransformationTraceContract {
            id: contract_id.into(),
            semantic_fingerprint: format!("sha256:{}", "b".repeat(64)),
        },
        input: TransformationTracePayload {
            schema: "input/v1".into(),
            digest: digest(&input_data),
            data: input_data,
        },
        output: TransformationTracePayload {
            schema: "output/v1".into(),
            digest: digest(&output_data),
            data: output_data,
        },
        delta: TransformationTraceDelta::default(),
        evaluation: TransformationTraceEvaluation {
            hard_gates: vec![TransformationTraceHardGate {
                gate_id: format!("gate:{phase_id}"),
                result: "pass".into(),
                reason_code: "PHASE_ACCEPTED".into(),
                evidence: Vec::new(),
            }],
            advisories: vec![TransformationTraceAdvisory {
                advisory_id: "status_only".into(),
                result: "signal".into(),
                score: None,
                authority: "none".into(),
                failure_mode: "fail_open".into(),
                scope: "universal".into(),
                confidence: "low".into(),
                recommended_effect: "annotate".into(),
                basis: "fixture".into(),
            }],
        },
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

fn valid_trace() -> TransformationTraceSet {
    let phases = [
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
    let mut state = json!({});
    let records = phases
        .into_iter()
        .map(|(phase_id, contract_id)| {
            let output = json!({"phase": phase_id});
            let record = record(phase_id, contract_id, state.clone(), output.clone());
            state = output;
            record
        })
        .collect();
    TransformationTraceSet {
        schema: TRANSFORMATION_TRACE_SET_SCHEMA.into(),
        journey_id: JOURNEY_ID.into(),
        trace_id: TRACE_ID.into(),
        records,
    }
}

async fn put_trace(
    app: Router,
    trace: &TransformationTraceSet,
    journey_id: &str,
) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .method("PUT")
            .uri(format!("/api/v1/journeys/{journey_id}/integrations-trace"))
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(trace).expect("trace JSON")))
            .expect("request"),
    )
    .await
    .expect("response")
}

#[tokio::test]
async fn finalized_trace_attaches_idempotently_and_downloads_with_no_store() {
    let harness = harness();
    let trace = valid_trace();

    let created = put_trace(harness.app.clone(), &trace, JOURNEY_ID).await;
    assert_eq!(created.status(), StatusCode::CREATED);

    let replayed = put_trace(harness.app.clone(), &trace, JOURNEY_ID).await;
    assert_eq!(replayed.status(), StatusCode::OK);
    let replayed_body = to_bytes(replayed.into_body(), usize::MAX)
        .await
        .expect("replay receipt");
    let replayed_json: Value = serde_json::from_slice(&replayed_body).expect("receipt JSON");
    assert_eq!(replayed_json["idempotent_replay"], true);

    let response = harness
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/journeys/{JOURNEY_ID}/trace.json"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert!(response.headers()[header::CONTENT_DISPOSITION]
        .to_str()
        .expect("content disposition")
        .contains("bicameral-journey-trace.json"));
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("download body");
    let package: Value = serde_json::from_slice(&body).expect("download JSON");
    assert_eq!(package["schema"], "bicameral.journey-trace-download/v1");
    assert_eq!(package["journey_id"], JOURNEY_ID);
    assert_eq!(package["integrations_trace"]["trace_id"], TRACE_ID);
    assert_eq!(
        package["integrations_trace"]["records"]
            .as_array()
            .expect("trace records")
            .len(),
        6
    );
    assert_eq!(package["daemon_review"]["journey_id"], JOURNEY_ID);
}

#[tokio::test]
async fn conflicting_finalized_trace_is_rejected_without_rewrite() {
    let harness = harness();
    let trace = valid_trace();
    assert_eq!(
        put_trace(harness.app.clone(), &trace, JOURNEY_ID)
            .await
            .status(),
        StatusCode::CREATED
    );

    let mut conflicting = valid_trace();
    conflicting.trace_id = "trace-http-2".into();
    for record in &mut conflicting.records {
        record.trace_id = conflicting.trace_id.clone();
    }
    let response = put_trace(harness.app, &conflicting, JOURNEY_ID).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn malformed_and_sensitive_traces_fail_before_storage() {
    let harness = harness();

    let mut malformed = valid_trace();
    malformed.records[1].input.digest = format!("sha256:{}", "0".repeat(64));
    let malformed_response = put_trace(harness.app.clone(), &malformed, JOURNEY_ID).await;
    assert_eq!(
        malformed_response.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let mut sensitive = valid_trace();
    let secret = format!("AKIA{}", "A".repeat(16));
    let last = sensitive.records.last_mut().expect("last record");
    last.output.data = json!({"token": secret});
    last.output.digest = digest(&last.output.data);
    let sensitive_response = put_trace(harness.app.clone(), &sensitive, JOURNEY_ID).await;
    assert_eq!(
        sensitive_response.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let download = harness
        .app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/journeys/{JOURNEY_ID}/trace.json"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(download.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn trace_cannot_attach_to_unknown_canonical_journey() {
    let harness = harness();
    let mut trace = valid_trace();
    trace.journey_id = "missing-journey".into();
    for record in &mut trace.records {
        record.journey_id = trace.journey_id.clone();
    }
    let response = put_trace(harness.app, &trace, "missing-journey").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
