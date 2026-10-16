use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use bicameral_api::event_store::{EventStoreAdapter, EventType};
use bicameral_api::reasoning_projection::{
    ProjectionDeferralReason, ProjectionProvider, ProjectionRequest, ProjectionResult,
    ReasoningMode,
};
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
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

struct DeferredProvider;

impl ProjectionProvider for DeferredProvider {
    fn project(&self, _request: &ProjectionRequest) -> ProjectionResult {
        ProjectionResult::deferred(
            ProjectionDeferralReason::ProviderUnavailable {
                detail: "simulated provider outage".into(),
            },
            self.mode(),
        )
    }

    fn mode(&self) -> ReasoningMode {
        ReasoningMode::LocalCli
    }
}

struct FailedProvider;

impl ProjectionProvider for FailedProvider {
    fn project(&self, _request: &ProjectionRequest) -> ProjectionResult {
        ProjectionResult::hard_rejected("simulated invalid provider output".into())
    }

    fn mode(&self) -> ReasoningMode {
        ReasoningMode::Deterministic
    }
}

struct Harness {
    _root: tempfile::TempDir,
    adapter: Arc<GatewayEventStore>,
    audit_root: PathBuf,
    evidence_root: PathBuf,
    sqlite_a: PathBuf,
    sqlite_b: PathBuf,
}

impl Harness {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        Self {
            adapter: Arc::new(GatewayEventStore::new()),
            audit_root: root.path().join("audit"),
            evidence_root: root.path().join("evidence"),
            sqlite_a: root.path().join("working-a.sqlite3"),
            sqlite_b: root.path().join("working-b.sqlite3"),
            _root: root,
        }
    }

    fn state(
        &self,
        sqlite_path: &Path,
        provider: Arc<dyn ProjectionProvider>,
    ) -> (AppState, Arc<LiveProjectionWorkStore>) {
        let audit_path = self.audit_root.join(Uuid::new_v4().to_string());
        let audit = AuditStore::open(&audit_path).unwrap();
        let repository = Arc::new(SqliteProjectionWorkRepository::open(sqlite_path).unwrap());
        let live = Arc::new(LiveProjectionWorkStore::open(repository).unwrap());
        let state = AppState::with_adapter(self.adapter.clone(), audit)
            .with_evidence_repository(Arc::new(FileEvidenceRepository::new(
                self.evidence_root.clone(),
            )))
            .with_projection_work_store(live.clone())
            .with_projection_provider(provider);
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

fn decision_payload() -> Value {
    json!({
        "source_system": "github",
        "source_uri": "https://github.com/BicameralAI/example/issues/84",
        "content": "Decision: the daemon must preserve accepted evidence before projection because working state is disposable.",
        "evidence": [{
            "excerpt": "the daemon must preserve accepted evidence before projection",
            "confidence": 0.98
        }],
        "candidate_hints": [{
            "title": "Preserve evidence before projection",
            "body": "Decision: the daemon must preserve accepted evidence before projection because working state is disposable.",
            "level": "architecture",
            "labels": ["durability"]
        }]
    })
}

fn event_count(adapter: &GatewayEventStore, event_type: EventType) -> usize {
    adapter
        .replay()
        .unwrap()
        .into_iter()
        .filter(|entry| entry.event_type == event_type)
        .count()
}

fn remove_sqlite_family(path: &Path) {
    for candidate in [
        path.to_path_buf(),
        PathBuf::from(format!("{}-wal", path.display())),
        PathBuf::from(format!("{}-shm", path.display())),
    ] {
        match std::fs::remove_file(candidate) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("failed to remove SQLite working state: {error}"),
        }
    }
}

#[tokio::test]
async fn deferred_provider_preserves_evidence_and_retry_state_across_reopen() {
    let harness = Harness::new();
    let (state, live) = harness.state(&harness.sqlite_a, Arc::new(DeferredProvider));
    let (status, body) = post(app(state), decision_payload()).await;
    assert_eq!(status, StatusCode::CREATED, "body: {body}");
    assert_eq!(body["projection_outcome"], "projection_deferred");
    assert!(body["candidate_ids"].as_array().unwrap().is_empty());

    let request_id = Uuid::parse_str(body["projection_request_id"].as_str().unwrap()).unwrap();
    let request = live.repository().get_request(request_id).unwrap().unwrap();
    assert_eq!(request.status, ProjectionRequestStatus::Deferred);
    assert_eq!(request.attempt, 1);
    assert_eq!(
        request.last_error_code.as_deref(),
        Some("provider_unavailable")
    );
    let outcomes = live.repository().list_outcomes(request_id).unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].kind, ProjectionOutcomeKind::Deferred);
    assert!(outcomes[0].retryable);
    assert!(live.load_resume_input(request_id).unwrap().is_some());
    assert_eq!(
        event_count(&harness.adapter, EventType::EvidenceAccepted),
        1
    );
    drop(live);

    let (restarted_state, restarted) = harness.state(&harness.sqlite_a, Arc::new(DeferredProvider));
    let (retry_status, retry_body) = post(app(restarted_state), decision_payload()).await;
    assert_eq!(retry_status, StatusCode::CREATED, "body: {retry_body}");
    assert_eq!(retry_body["projection_outcome"], "projection_deferred");
    let reopened = restarted
        .repository()
        .get_request(request_id)
        .unwrap()
        .unwrap();
    assert_eq!(reopened.status, ProjectionRequestStatus::Deferred);
    assert_eq!(reopened.attempt, 1);
    assert_eq!(
        restarted
            .repository()
            .list_outcomes(request_id)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        event_count(&harness.adapter, EventType::EvidenceAccepted),
        1
    );
}

#[tokio::test]
async fn failed_projection_preserves_canonical_evidence_and_terminal_local_failure() {
    let harness = Harness::new();
    let (state, live) = harness.state(&harness.sqlite_a, Arc::new(FailedProvider));
    let (status, body) = post(app(state), decision_payload()).await;
    assert_eq!(status, StatusCode::CREATED, "body: {body}");
    assert_eq!(body["projection_outcome"], "hard_rejected");
    assert!(body["candidate_ids"].as_array().unwrap().is_empty());

    let request_id = Uuid::parse_str(body["projection_request_id"].as_str().unwrap()).unwrap();
    let request = live.repository().get_request(request_id).unwrap().unwrap();
    assert_eq!(request.status, ProjectionRequestStatus::Failed);
    assert_eq!(request.last_error_code.as_deref(), Some("hard_rejected"));
    let outcomes = live.repository().list_outcomes(request_id).unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].kind, ProjectionOutcomeKind::Failed);
    assert!(!outcomes[0].retryable);
    assert!(live.load_resume_input(request_id).unwrap().is_some());
    assert_eq!(
        event_count(&harness.adapter, EventType::EvidenceAccepted),
        1
    );
}

#[tokio::test]
async fn deleting_working_store_does_not_delete_canonical_evidence() {
    let harness = Harness::new();
    let (state, live) = harness.state(
        &harness.sqlite_a,
        Arc::new(bicameral_api::reasoning_projection::DeterministicProjectionProvider),
    );
    let (status, body) = post(app(state), decision_payload()).await;
    assert_eq!(status, StatusCode::CREATED, "body: {body}");
    assert_eq!(body["projection_outcome"], "candidate_projected");
    assert_eq!(
        live.active_candidate_payloads(chrono::Utc::now())
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        event_count(&harness.adapter, EventType::EvidenceAccepted),
        1
    );
    drop(live);

    remove_sqlite_family(&harness.sqlite_a);
    let (_empty_state, empty_live) = harness.state(
        &harness.sqlite_a,
        Arc::new(bicameral_api::reasoning_projection::DeterministicProjectionProvider),
    );
    assert!(empty_live
        .active_candidate_payloads(chrono::Utc::now())
        .unwrap()
        .is_empty());
    assert_eq!(
        event_count(&harness.adapter, EventType::EvidenceAccepted),
        1
    );
    let accepted = harness
        .adapter
        .replay()
        .unwrap()
        .into_iter()
        .filter(|entry| entry.event_type == EventType::EvidenceAccepted)
        .collect::<Vec<_>>();
    assert_eq!(accepted.len(), 1);
    assert!(accepted[0].payload.get("content_digest").is_some());
}

#[tokio::test]
async fn independent_working_stores_may_expose_different_candidates() {
    let harness = Harness::new();
    let (state_a, live_a) = harness.state(
        &harness.sqlite_a,
        Arc::new(bicameral_api::reasoning_projection::DeterministicProjectionProvider),
    );
    let (status_a, body_a) = post(app(state_a), decision_payload()).await;
    assert_eq!(status_a, StatusCode::CREATED, "body: {body_a}");
    assert_eq!(body_a["projection_outcome"], "candidate_projected");

    let (state_b, live_b) = harness.state(&harness.sqlite_b, Arc::new(DeferredProvider));
    let (status_b, body_b) = post(app(state_b), decision_payload()).await;
    assert_eq!(status_b, StatusCode::CREATED, "body: {body_b}");
    assert_eq!(body_b["projection_outcome"], "projection_deferred");

    assert_eq!(
        live_a
            .active_candidate_payloads(chrono::Utc::now())
            .unwrap()
            .len(),
        1
    );
    assert!(live_b
        .active_candidate_payloads(chrono::Utc::now())
        .unwrap()
        .is_empty());
    assert_eq!(
        event_count(&harness.adapter, EventType::EvidenceAccepted),
        1
    );
    assert_eq!(event_count(&harness.adapter, EventType::ExternalIngest), 0);
}
