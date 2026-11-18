use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use bicameral_api::event_store::{EventStoreAdapter, EventStoreEntry, EventType};
use bicameral_audit::AuditStore;
use bicameral_gateway::routes::{hosted_bridge_routes, projection_api_routes, query_routes};
use bicameral_gateway::state::{AppState, HostedBridgePairingSession};
use bicameral_runtime::event_store_adapters::git::GitAdapter;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use tower::ServiceExt;

const ROUTES: [(&str, &str, &str); 4] = [
    ("GET", "/api/v1/history?include_events=true", "events"),
    ("GET", "/api/v1/hosted-bridge/projection/ledger", "events"),
    ("GET", "/api/v1/projection/ledger", "ledger_events"),
    ("POST", "/api/v1/projection/rebuild", "ledger_events"),
];

async fn app(root: &Path) -> Router {
    let state = AppState::with_adapter(
        Arc::new(GitAdapter::new(root)),
        AuditStore::open(&root.join("audit")).unwrap(),
    );
    state.hosted_bridge_sessions.write().await.insert(
        "test-session".into(),
        HostedBridgePairingSession {
            session_token: "test-session".into(),
            pairing_id: None,
            origin: "https://app.bicameral-ai.com".into(),
            cloud_account_id: "test-account".into(),
            paired_at: chrono::Utc::now(),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
            revoked: false,
        },
    );
    projection_api_routes()
        .merge(query_routes())
        .merge(hosted_bridge_routes())
        .with_state(state)
}

async fn read(app: &Router, method: &str, path: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("x-bridge-session-token", "test-session")
                .body(Body::empty())
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

#[tokio::test]
async fn ledger_reads_refuse_malformed_replay_without_leaking_storage_details() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("private-event.yaml"), "malformed: [").unwrap();
    assert!(GitAdapter::new(root.path()).replay().is_err());
    let app = app(root.path()).await;
    for (method, path, _) in ROUTES {
        let (status, body) = read(&app, method, path).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{path}: {body}");
        assert_eq!(
            body,
            json!({
                "error": "ledger_projection_unavailable",
                "kind": "ProjectionUnavailable",
                "message": "Canonical ledger could not be replayed.",
            }),
            "{path}"
        );
    }
}

#[tokio::test]
async fn ledger_reads_preserve_empty_and_valid_replay() {
    let root = tempfile::tempdir().unwrap();
    let app = app(root.path()).await;
    let entry = EventStoreEntry::new(EventType::Ingest, json!({}), "test-actor".into());
    for count in [0, 1] {
        if count == 1 {
            GitAdapter::new(root.path()).append(entry.clone()).unwrap();
        }
        for (method, path, events_key) in ROUTES {
            let (status, body) = read(&app, method, path).await;
            assert_eq!(status, StatusCode::OK, "{path}: {body}");
            assert_eq!(body[events_key].as_array().unwrap().len(), count, "{path}");
            if count == 1 {
                assert_eq!(body[events_key][0]["id"], entry.id.to_string(), "{path}");
            }
        }
    }
}
