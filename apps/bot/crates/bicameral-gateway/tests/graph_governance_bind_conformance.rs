//! Gateway bind-route conformance tests for the graph-fact vs governance-fact
//! boundary (ADR-0010 / ADR-0015, issue #123).
//!
//! Proves that the gateway `/api/v1/bind` route:
//!
//! - Accepts only `GraphEvidenceState::Verified` evidence as binding
//!   materialization requests.
//! - Rejects every non-verified `GraphEvidenceState` variant with HTTP 422.
//! - Does not mutate ledger state when evidence is non-verified.
//!
//! Uses the gateway's in-memory `AppState` — no SurrealDB dependency.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use bicameral_api::evidence::{BindingConfidence, BindingType, GraphEvidenceState};
use bicameral_audit::store::AuditStore;
use bicameral_gateway::state::AppState;
use tower::ServiceExt;
use uuid::Uuid;

/// All non-verified states per ADR-0010.
const NON_VERIFIED_STATES: [GraphEvidenceState; 6] = [
    GraphEvidenceState::NotFound,
    GraphEvidenceState::UnknownStale,
    GraphEvidenceState::UnknownNotIndexed,
    GraphEvidenceState::Ambiguous,
    GraphEvidenceState::Unsupported,
    GraphEvidenceState::ApproximateCandidate,
];

fn test_state() -> AppState {
    let dir = tempfile::tempdir().unwrap();
    let audit = AuditStore::open(dir.path()).unwrap();
    AppState::new(audit)
}

fn build_router(state: AppState) -> axum::Router {
    // Re-create the same router the gateway uses.
    axum::Router::new()
        .merge(bicameral_gateway::routes::health_routes())
        .merge(bicameral_gateway::routes::ingest_routes())
        .merge(bicameral_gateway::routes::review_routes())
        .merge(bicameral_gateway::routes::status_routes())
        .merge(bicameral_gateway::routes::dashboard_api_routes())
        .merge(bicameral_gateway::routes::query_routes())
        .merge(bicameral_gateway::routes::bind_routes())
        .merge(bicameral_gateway::routes::compliance_routes())
        .with_state(state)
}

/// Ingest a candidate and accept it to produce a decision. Returns the
/// decision UUID.
async fn ingest_and_accept(state: &AppState) -> Uuid {
    // Ingest
    let app = build_router(state.clone());
    let body = serde_json::json!({
        "title": "Conformance test decision",
        "description": "For bind conformance",
        "level": "architecture",
        "source": "test://conformance",
        "evidence": [{"span_start": 0, "span_end": 10, "excerpt": "test excerpt"}]
    });
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/ingest")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Get candidate id
    let cand_id = {
        let cands = state.candidates.read().await;
        *cands.keys().next().unwrap()
    };

    // Accept → create decision
    let app = build_router(state.clone());
    let cmd_body = serde_json::json!({
        "target_id": cand_id.to_string(),
        "command": "accept_candidate"
    });
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/dashboard/command")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&cmd_body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    // Get decision id
    let decisions = state.decisions.read().await;
    *decisions.keys().next().unwrap()
}

/// Build a bind request body with the given evidence state.
///
/// Sends a `BindingProposal` (graph fact) to the gateway, which is the
/// accepted-evidence materialization boundary.
fn bind_request_body(decision_id: Uuid, evidence_state: GraphEvidenceState) -> String {
    let proposal = serde_json::json!({
        "candidate_id": decision_id.to_string(),
        "binding_type": BindingType::CodeRegion,
        "target": "src/lib.rs:10-20",
        "confidence": BindingConfidence::High,
        "graph_snapshot_id": format!("gs-conformance-{}", Uuid::new_v4()),
        "validated_sha": "abc123def456",
        "content_hash": "sha256:test-content",
    });

    let body = serde_json::json!({
        "decision_id": decision_id.to_string(),
        "proposal": proposal,
        "evidence_state": evidence_state,
        "line_start": 10,
        "line_end": 20,
    });
    serde_json::to_string(&body).unwrap()
}

// ═══════════════════════════════════════════════════════════════════════════
// Gateway bind route: verified evidence accepted
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn gateway_bind_accepts_verified_evidence() {
    let state = test_state();
    let decision_id = ingest_and_accept(&state).await;

    let app = build_router(state.clone());
    let body = bind_request_body(decision_id, GraphEvidenceState::Verified);
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/bind")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "Verified evidence must be accepted by the gateway"
    );

    let resp_body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
    assert_eq!(json["status"], "bound");

    // Binding should be materialized in state
    let bindings = state.bindings.read().await;
    assert!(
        bindings.contains_key(&decision_id),
        "Verified bind must materialize in state"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Gateway bind route: non-verified evidence rejected
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn gateway_bind_rejects_not_found() {
    let state = test_state();
    let decision_id = ingest_and_accept(&state).await;

    let app = build_router(state.clone());
    let body = bind_request_body(decision_id, GraphEvidenceState::NotFound);
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/bind")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let resp_body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
    assert_eq!(json["status"], "rejected");

    // No bindings in state
    let bindings = state.bindings.read().await;
    assert!(
        !bindings.contains_key(&decision_id),
        "Rejected bind must not materialize"
    );
}

#[tokio::test]
async fn gateway_bind_rejects_unknown_stale() {
    let state = test_state();
    let decision_id = ingest_and_accept(&state).await;

    let app = build_router(state.clone());
    let body = bind_request_body(decision_id, GraphEvidenceState::UnknownStale);
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/bind")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn gateway_bind_rejects_unknown_not_indexed() {
    let state = test_state();
    let decision_id = ingest_and_accept(&state).await;

    let app = build_router(state.clone());
    let body = bind_request_body(decision_id, GraphEvidenceState::UnknownNotIndexed);
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/bind")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn gateway_bind_rejects_ambiguous() {
    let state = test_state();
    let decision_id = ingest_and_accept(&state).await;

    let app = build_router(state.clone());
    let body = bind_request_body(decision_id, GraphEvidenceState::Ambiguous);
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/bind")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn gateway_bind_rejects_unsupported() {
    let state = test_state();
    let decision_id = ingest_and_accept(&state).await;

    let app = build_router(state.clone());
    let body = bind_request_body(decision_id, GraphEvidenceState::Unsupported);
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/bind")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn gateway_bind_rejects_approximate_candidate() {
    let state = test_state();
    let decision_id = ingest_and_accept(&state).await;

    let app = build_router(state.clone());
    let body = bind_request_body(decision_id, GraphEvidenceState::ApproximateCandidate);
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/bind")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

// ═══════════════════════════════════════════════════════════════════════════
// Exhaustive sweep: every non-verified state is rejected
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn gateway_bind_rejects_all_non_verified_states_exhaustively() {
    for evidence_state in &NON_VERIFIED_STATES {
        let state = test_state();
        let decision_id = ingest_and_accept(&state).await;

        let app = build_router(state.clone());
        let body = bind_request_body(decision_id, *evidence_state);
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/bind")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "Expected 422 for {evidence_state:?}, got {}",
            resp.status()
        );

        // Ensure no ledger mutation occurred
        let bindings = state.bindings.read().await;
        assert!(
            !bindings.contains_key(&decision_id),
            "Non-verified state {evidence_state:?} must not create bindings"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Gateway bind route: missing decision returns 404
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn gateway_bind_returns_404_for_unknown_decision() {
    let state = test_state();
    let bogus_decision = Uuid::new_v4();

    let app = build_router(state);
    let body = bind_request_body(bogus_decision, GraphEvidenceState::Verified);
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/bind")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "Bind to unknown decision must return 404"
    );
}
