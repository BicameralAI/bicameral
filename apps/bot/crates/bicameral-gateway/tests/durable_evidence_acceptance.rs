//! D3a durable evidence acceptance contract tests.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use bicameral_api::event_store::{
    EventStoreAdapter, EventStoreEntry, EventStoreError, EventType, SubstrateCapability,
    SubstrateHealth, SubstrateKind,
};
use bicameral_api::evidence_acceptance::EvidenceAcceptedPayload;
use bicameral_audit::AuditStore;
use bicameral_gateway::evidence_repository::{EvidenceRepository, MemoryEvidenceRepository};
use bicameral_gateway::routes::external_ingest_routes;
use bicameral_gateway::state::AppState;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::sync::Arc;
use tower::ServiceExt;

fn test_audit() -> AuditStore {
    let dir = tempfile::tempdir().unwrap();
    AuditStore::open(dir.path()).unwrap()
}

fn app(state: AppState) -> Router {
    Router::new()
        .merge(external_ingest_routes())
        .with_state(state)
}

async fn post(app: Router, payload: serde_json::Value) -> (StatusCode, serde_json::Value) {
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

fn evidence_only_payload() -> serde_json::Value {
    serde_json::json!({
        "source_system": "github",
        "source_uri": "https://github.com/BicameralAI/example/issues/42#issuecomment-7",
        "content": "Routine implementation status update with no proposed decision.",
        "evidence": [{
            "excerpt": "Routine implementation status update",
            "confidence": 0.8
        }]
    })
}

#[tokio::test]
async fn no_candidate_still_appends_replayable_evidence_acceptance() {
    let state = AppState::new(test_audit());
    let (status, body) = post(app(state.clone()), evidence_only_payload()).await;
    assert_eq!(status, StatusCode::CREATED, "body: {body}");
    assert_eq!(body["projection_outcome"], "no_candidate");
    assert!(body["candidate_ids"].as_array().unwrap().is_empty());

    let entries = state.event_store.replay().unwrap();
    let accepted: Vec<_> = entries
        .iter()
        .filter(|entry| entry.event_type == EventType::EvidenceAccepted)
        .collect();
    assert_eq!(accepted.len(), 1);
    assert!(entries
        .iter()
        .all(|entry| entry.event_type != EventType::ExternalIngest));

    let payload: EvidenceAcceptedPayload =
        serde_json::from_value(accepted[0].payload.clone()).unwrap();
    assert_eq!(
        state
            .evidence_repository
            .read(&payload.content_locator)
            .unwrap(),
        b"Routine implementation status update with no proposed decision."
    );

    let replayed = bicameral_runtime::replay::replay_from_adapter(state.event_store.as_ref())
        .expect("acceptance replay must succeed");
    assert_eq!(replayed.accepted_evidence.len(), 1);
    assert!(replayed.candidates.is_empty());
}

#[tokio::test]
async fn duplicate_delivery_keeps_one_canonical_acceptance_fact() {
    let state = AppState::new(test_audit());
    for _ in 0..2 {
        let (status, body) = post(app(state.clone()), evidence_only_payload()).await;
        assert_eq!(status, StatusCode::CREATED, "body: {body}");
    }

    let entries = state.event_store.replay().unwrap();
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.event_type == EventType::EvidenceAccepted)
            .count(),
        1
    );
}

struct FailingAppendStore;

impl EventStoreAdapter for FailingAppendStore {
    fn append(&self, _entry: EventStoreEntry) -> Result<(), EventStoreError> {
        Err(EventStoreError::Io {
            reason: "injected append failure".to_string(),
        })
    }

    fn replay(&self) -> Result<Vec<EventStoreEntry>, EventStoreError> {
        Ok(Vec::new())
    }

    fn substrate_kind(&self) -> SubstrateKind {
        SubstrateKind::Memory
    }

    fn health_check(&self) -> Result<SubstrateHealth, EventStoreError> {
        Ok(SubstrateHealth {
            kind: SubstrateKind::Memory,
            reachable: true,
            last_synced: None,
            entry_count: 0,
            freshness: None,
            warnings: Vec::new(),
            capabilities: BTreeSet::new(),
        })
    }

    fn capabilities(&self) -> BTreeSet<SubstrateCapability> {
        BTreeSet::new()
    }
}

#[tokio::test]
async fn append_failure_leaves_reusable_orphan_blob_without_canonical_fact() {
    let repository = Arc::new(MemoryEvidenceRepository::new());
    let state = AppState::with_adapter(Arc::new(FailingAppendStore), test_audit())
        .with_evidence_repository(repository.clone());
    let payload = evidence_only_payload();
    let content = payload["content"].as_str().unwrap().to_string();

    let (status, _body) = post(app(state.clone()), payload).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(state.event_store.replay().unwrap().is_empty());

    let digest = format!("{:x}", Sha256::digest(content.as_bytes()));
    let locator = format!("sha256/{}/{}", &digest[..2], digest);
    assert_eq!(repository.read(&locator).unwrap(), content.as_bytes());

    // The same body can be written again without duplication or corruption,
    // allowing a later retry against a healthy event store to append the fact.
    let stored = repository.put(content.as_bytes()).unwrap();
    assert_eq!(stored.locator, locator);
}
