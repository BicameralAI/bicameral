//! Route-level proof for pre-Bot redaction receipts.
//!
//! Valid sanitized envelopes are admitted through the external-ingest route.
//! Malformed receipts fail before evidence materialization, and removed values
//! never appear in daemon responses or stored evidence.
//!
//! These tests intentionally exercise the public HTTP boundary rather than only
//! validating the receipt type in isolation.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use bicameral_audit::AuditStore;
use bicameral_gateway::routes::external_ingest_routes;
use bicameral_gateway::state::AppState;
use tower::ServiceExt;

fn test_audit() -> AuditStore {
    let dir = tempfile::tempdir().unwrap();
    AuditStore::open(dir.path()).unwrap()
}

fn build_app(state: AppState) -> Router {
    Router::new()
        .merge(external_ingest_routes())
        .with_state(state)
}

async fn post_json(app: Router, payload: serde_json::Value) -> (StatusCode, String) {
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/external-ingest")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&body).to_string())
}

fn valid_payload() -> serde_json::Value {
    serde_json::json!({
        "source_system": "github",
        "source_uri": "https://github.com/BicameralAI/example/issues/7",
        "content": "Contact [redacted:email] before rollout.",
        "evidence": [{
            "excerpt": "Contact [redacted:email] before rollout."
        }],
        "candidate_hints": [{
            "title": "Rollout contact was sanitized",
            "body": "Contact [redacted:email] before rollout.",
            "labels": [
                "emission_type:evidence",
                "redaction:v1",
                "redaction_ruleset:fx-sec-001-plus-pii-v1"
            ]
        }],
        "redaction_receipt": {
            "schema_version": 1,
            "engine": "bicameral-stdlib-redaction",
            "engine_version": "1.0.0",
            "ruleset_id": "fx-sec-001-plus-pii-v1",
            "ruleset_digest": "sha256:1111111111111111111111111111111111111111111111111111111111111111",
            "input_digest": "sha256:2222222222222222222222222222222222222222222222222222222222222222",
            "output_digest": "sha256:3333333333333333333333333333333333333333333333333333333333333333",
            "findings": [{
                "category": "pii",
                "action": "tokenized",
                "count": 1
            }],
            "structural_fields_preserved": true,
            "completed_at": "2026-07-17T21:00:00Z"
        }
    })
}

#[tokio::test]
async fn valid_redaction_receipt_is_admitted_without_removed_values() {
    let state = AppState::new(test_audit());
    let app = build_app(state.clone());

    let (status, body) = post_json(app, valid_payload()).await;
    assert_eq!(status, StatusCode::CREATED, "body: {body}");
    assert!(!body.contains("user@example.com"));

    let evidence = state.evidence.read().await;
    assert!(!evidence.is_empty());
    assert!(evidence
        .values()
        .all(|item| !item.excerpt.contains("user@example.com")));
    assert!(evidence
        .values()
        .any(|item| item.excerpt.contains("[redacted:email]")));
}

#[tokio::test]
async fn malformed_redaction_receipt_fails_before_evidence_admission() {
    let state = AppState::new(test_audit());
    let app = build_app(state.clone());
    let mut payload = valid_payload();
    payload["redaction_receipt"]["input_digest"] = serde_json::json!("sha256:not-a-valid-digest");

    let (status, body) = post_json(app, payload).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
    assert!(state.evidence.read().await.is_empty());
    assert!(state.candidates.read().await.is_empty());
}

#[tokio::test]
async fn receipt_cannot_smuggle_tool_or_canonical_authority() {
    let state = AppState::new(test_audit());
    let app = build_app(state.clone());
    let mut payload = valid_payload();
    payload["tool_command"] = serde_json::json!({
        "name": "review.accept_candidate",
        "params": {"target_id": "CAND-1"}
    });
    payload["canonical_decision"] = serde_json::json!({"id": "DEC-FAKE"});

    let (status, body) = post_json(app, payload).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body}");
    assert!(state.evidence.read().await.is_empty());
    assert!(state.candidates.read().await.is_empty());
}
