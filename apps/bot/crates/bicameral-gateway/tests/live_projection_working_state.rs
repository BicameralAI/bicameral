//! Live D3b external-ingest projection and restart contract tests.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use bicameral_api::event_store::{EventStoreAdapter, EventType};
use bicameral_api::evidence_acceptance::EvidenceAcceptedPayload;
use bicameral_audit::AuditStore;
use bicameral_gateway::evidence_repository::FileEvidenceRepository;
use bicameral_gateway::live_projection_work::LiveProjectionWorkStore;
use bicameral_gateway::projection_work_repository::{
    ProjectionOutcomeKind, ProjectionRequestStatus, ProjectionWorkRepository,
    SqliteProjectionWorkRepository,
};
use bicameral_gateway::routes::external_ingest_routes;
use bicameral_gateway::state::{AppState, GatewayEventStore};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

struct Harness {
    _root: tempfile::TempDir,
    adapter: Arc<GatewayEventStore>,
    audit_root: PathBuf,
    evidence_root: PathBuf,
    sqlite_path: PathBuf,
}

impl Harness {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        Self {
            adapter: Arc::new(GatewayEventStore::new()),
            audit_root: root.path().join("audit"),
            evidence_root: root.path().join("evidence"),
            sqlite_path: root.path().join("projection-working-state.sqlite3"),
            _root: root,
        }
    }

    fn state(&self) -> (AppState, Arc<LiveProjectionWorkStore>) {
        let audit_path = self.audit_root.join(Uuid::new_v4().to_string());
        let audit = AuditStore::open(&audit_path).unwrap();
        let repository =
            Arc::new(SqliteProjectionWorkRepository::open(self.sqlite_path.clone()).unwrap());
        let live = Arc::new(LiveProjectionWorkStore::open(repository).unwrap());
        let state = AppState::with_adapter(self.adapter.clone(), audit)
            .with_evidence_repository(Arc::new(FileEvidenceRepository::new(
                self.evidence_root.clone(),
            )))
            .with_projection_work_store(live.clone());
        (state, live)
    }
}

fn app(state: AppState) -> Router {
    Router::new()
        .merge(external_ingest_routes())
        .with_state(state)
}

async fn post(app: Router, payload: Value) -> (StatusCode, Value) {
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/external-ingest")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

fn candidate_payload() -> Value {
    json!({
        "source_system": "github",
        "source_uri": "https://github.com/BicameralAI/example/issues/42#issuecomment-7",
        "content": "The ingestion adapter must preserve screened evidence before projection.",
        "evidence": [{
            "excerpt": "preserve screened evidence before projection",
            "confidence": 0.95
        }],
        "candidate_hints": [{
            "title": "Persist evidence before projection",
            "body": "The daemon must accept screened evidence before candidate projection runs.",
            "level": "architecture",
            "labels": ["ingest", "durability"]
        }]
    })
}

fn no_candidate_payload() -> Value {
    json!({
        "source_system": "slack",
        "source_uri": "slack://dev-environment/thread/1784333970",
        "content": "Routine status update. No decision or proposal was made.",
        "evidence": [{
            "excerpt": "Routine status update",
            "confidence": 0.8
        }]
    })
}

fn accepted_payload(adapter: &GatewayEventStore) -> EvidenceAcceptedPayload {
    let entry = adapter
        .replay()
        .unwrap()
        .into_iter()
        .find(|entry| entry.event_type == EventType::EvidenceAccepted)
        .expect("EvidenceAccepted event");
    serde_json::from_value(entry.payload).unwrap()
}

fn event_count(adapter: &GatewayEventStore, event_type: EventType) -> usize {
    adapter
        .replay()
        .unwrap()
        .into_iter()
        .filter(|entry| entry.event_type == event_type)
        .count()
}

#[tokio::test]
async fn candidate_ingest_persists_working_state_without_parity_event() {
    let harness = Harness::new();
    let (state, live) = harness.state();

    let (status, body) = post(app(state), candidate_payload()).await;
    assert_eq!(status, StatusCode::CREATED, "body: {body}");
    assert_eq!(body["projection_outcome"], "candidate_projected");
    let request_id = Uuid::parse_str(body["projection_request_id"].as_str().unwrap()).unwrap();
    let candidate_id = Uuid::parse_str(body["candidate_ids"][0].as_str().unwrap()).unwrap();

    let acceptance = accepted_payload(harness.adapter.as_ref());
    let request = live
        .request_by_acceptance_key(&acceptance.idempotency_key)
        .unwrap()
        .unwrap();
    assert_eq!(request.request_id, request_id);
    assert_eq!(request.status, ProjectionRequestStatus::Completed);
    assert_eq!(request.attempt, 1);

    let outcomes = live.repository().list_outcomes(request_id).unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].kind, ProjectionOutcomeKind::Candidate);
    assert_eq!(outcomes[0].details["outcome_label"], "candidate_projected");
    let evaluations = outcomes[0].details["decision_likeness"]
        .as_array()
        .expect("typed decision-likeness records");
    assert!(!evaluations.is_empty());
    assert!(evaluations.iter().any(|record| {
        record["evaluation"]["qualified"] == true && record["evaluation"]["reason_codes"].is_array()
    }));

    let candidates = live.candidate_payloads(request_id).unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].id, candidate_id);
    assert_eq!(candidates[0].evidence_refs, acceptance.evidence_ids);
    assert!(live.load_resume_input(request_id).unwrap().is_some());

    assert_eq!(
        event_count(harness.adapter.as_ref(), EventType::EvidenceAccepted),
        1
    );
    assert_eq!(
        event_count(harness.adapter.as_ref(), EventType::ExternalIngest),
        0
    );
}

#[tokio::test]
async fn duplicate_delivery_reuses_evidence_candidate_and_outcome_identity() {
    let harness = Harness::new();
    let (state, live) = harness.state();

    let (first_status, first) = post(app(state.clone()), candidate_payload()).await;
    let (second_status, second) = post(app(state), candidate_payload()).await;
    assert_eq!(first_status, StatusCode::CREATED, "body: {first}");
    assert_eq!(second_status, StatusCode::CREATED, "body: {second}");
    assert_eq!(
        first["projection_request_id"],
        second["projection_request_id"]
    );
    assert_eq!(first["evidence_ids"], second["evidence_ids"]);
    assert_eq!(first["candidate_ids"], second["candidate_ids"]);

    let request_id = Uuid::parse_str(first["projection_request_id"].as_str().unwrap()).unwrap();
    assert_eq!(
        live.repository().list_outcomes(request_id).unwrap().len(),
        1
    );
    assert_eq!(live.candidate_payloads(request_id).unwrap().len(), 1);
    assert_eq!(
        event_count(harness.adapter.as_ref(), EventType::EvidenceAccepted),
        1
    );
    assert_eq!(
        event_count(harness.adapter.as_ref(), EventType::ExternalIngest),
        0
    );
}

#[tokio::test]
async fn no_candidate_is_completed_without_candidate_or_parity_event() {
    let harness = Harness::new();
    let (state, live) = harness.state();

    let (status, body) = post(app(state), no_candidate_payload()).await;
    assert_eq!(status, StatusCode::CREATED, "body: {body}");
    assert_eq!(body["projection_outcome"], "no_candidate");
    assert!(body["candidate_ids"].as_array().unwrap().is_empty());

    let request_id = Uuid::parse_str(body["projection_request_id"].as_str().unwrap()).unwrap();
    let request = live.repository().get_request(request_id).unwrap().unwrap();
    assert_eq!(request.status, ProjectionRequestStatus::Completed);
    let outcomes = live.repository().list_outcomes(request_id).unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].kind, ProjectionOutcomeKind::NoCandidate);
    assert!(live.candidate_payloads(request_id).unwrap().is_empty());

    assert_eq!(
        event_count(harness.adapter.as_ref(), EventType::EvidenceAccepted),
        1
    );
    assert_eq!(
        event_count(harness.adapter.as_ref(), EventType::ExternalIngest),
        0
    );
}

#[tokio::test]
async fn reopened_repository_returns_stable_candidate_without_duplicate_events() {
    let harness = Harness::new();
    let (first_state, first_live) = harness.state();
    let (first_status, first) = post(app(first_state), candidate_payload()).await;
    assert_eq!(first_status, StatusCode::CREATED, "body: {first}");
    let request_id = Uuid::parse_str(first["projection_request_id"].as_str().unwrap()).unwrap();
    assert!(first_live.load_resume_input(request_id).unwrap().is_some());
    drop(first_live);

    let (restarted_state, restarted_live) = harness.state();
    let (second_status, second) = post(app(restarted_state), candidate_payload()).await;
    assert_eq!(second_status, StatusCode::CREATED, "body: {second}");
    assert_eq!(
        first["projection_request_id"],
        second["projection_request_id"]
    );
    assert_eq!(first["evidence_ids"], second["evidence_ids"]);
    assert_eq!(first["candidate_ids"], second["candidate_ids"]);

    let reopened_request = restarted_live
        .repository()
        .get_request(request_id)
        .unwrap()
        .unwrap();
    assert_eq!(reopened_request.status, ProjectionRequestStatus::Completed);
    assert_eq!(reopened_request.attempt, 1);
    assert_eq!(
        restarted_live
            .repository()
            .list_outcomes(request_id)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        restarted_live.candidate_payloads(request_id).unwrap().len(),
        1
    );
    assert!(restarted_live
        .load_resume_input(request_id)
        .unwrap()
        .is_some());

    assert_eq!(
        event_count(harness.adapter.as_ref(), EventType::EvidenceAccepted),
        1
    );
    assert_eq!(
        event_count(harness.adapter.as_ref(), EventType::ExternalIngest),
        0
    );
}
