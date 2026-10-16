//! Reasoning Projection contract tests — mandatory daemon-owned projection.
//!
//! Verifies acceptance criteria from issue #431:
//!
//! 1. Local ingest no longer creates a reviewable candidate directly from
//!    caller fields without daemon projection.
//! 2. External ingest no longer materializes advisory candidate_hints
//!    directly into candidates on alpha path.
//! 3. Implementation returns honest alpha projection outcome
//!    (projected, no_candidate, deferred, hard_rejection).
//! 4. Evidence-only or provider-unavailable flows do not create generic
//!    fallback candidates.
//! 5. Projection outcome is returned in response payload.

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

async fn post_json(app: Router, uri: &str, payload: serde_json::Value) -> (StatusCode, String) {
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), 1_048_576)
        .await
        .unwrap();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

// ── External ingest: candidate_projected outcome ──────────────────

#[tokio::test]
async fn external_ingest_projects_candidate_from_hints() {
    let state = AppState::new(test_audit());
    let app = build_app(state);

    let payload = serde_json::json!({
        "source_system": "jira",
        "source_uri": "https://jira.example.com/PROJ-100",
        "content": "Team decided to use PostgreSQL for the user store",
        "evidence": [{
            "excerpt": "decided to use PostgreSQL",
            "confidence": 0.9
        }],
        "candidate_hints": [{
            "title": "Use PostgreSQL for user store",
            "body": "Architecture decision: PostgreSQL for the user data store",
            "level": "architecture",
            "labels": ["database"]
        }]
    });

    let (status, body) = post_json(app, "/api/v1/external-ingest", payload).await;
    assert_eq!(status, StatusCode::CREATED, "body: {body}");

    let resp: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(resp["projection_outcome"], "candidate_projected");
    assert_eq!(resp["candidate_ids"].as_array().unwrap().len(), 1);
    assert!(!resp["evidence_ids"].as_array().unwrap().is_empty());

    let result = &resp["advisory_results"][0];
    assert_eq!(result["action"], "accepted_as_candidate");
}

// ── External ingest: no_candidate outcome ─────────────────────────

#[tokio::test]
async fn external_ingest_no_candidate_on_evidence_only() {
    let state = AppState::new(test_audit());
    let app = build_app(state);

    let payload = serde_json::json!({
        "source_system": "slack",
        "source_uri": "https://slack.example.com/C999/p111",
        "content": "Some discussion about lunch options",
        "evidence": [{
            "excerpt": "let's get tacos",
            "confidence": 0.7
        }]
    });

    let (status, body) = post_json(app, "/api/v1/external-ingest", payload).await;
    assert_eq!(status, StatusCode::CREATED, "body: {body}");

    let resp: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(resp["projection_outcome"], "no_candidate");
    assert!(resp["candidate_ids"].as_array().unwrap().is_empty());
    // Evidence still captured even though no candidates materialized.
    assert!(!resp["evidence_ids"].as_array().unwrap().is_empty());
}

// ── No fallback candidate manufactured ────────────────────────────

#[tokio::test]
async fn external_ingest_no_fallback_candidate_from_evidence() {
    let state = AppState::new(test_audit());
    let app = build_app(state);

    // Evidence exists but no hints/title/description → no generic fallback.
    let payload = serde_json::json!({
        "source_system": "github",
        "source_uri": "https://github.com/example/repo/issues/42",
        "content": "Issue #42: Fix the flaky test in CI",
        "evidence": [
            {
                "excerpt": "Fix the flaky test in CI",
                "confidence": 0.6
            },
            {
                "excerpt": "Sometimes fails on the nightly runner",
                "confidence": 0.5
            }
        ]
    });

    let (status, body) = post_json(app, "/api/v1/external-ingest", payload).await;
    assert_eq!(status, StatusCode::CREATED, "body: {body}");

    let resp: serde_json::Value = serde_json::from_str(&body).unwrap();
    // Must NOT create a generic "External evidence from github" fallback.
    assert!(
        resp["candidate_ids"].as_array().unwrap().is_empty(),
        "Evidence-only ingest must not manufacture generic fallback candidates"
    );
    assert_eq!(resp["projection_outcome"], "no_candidate");
    // Evidence is still captured.
    assert_eq!(resp["evidence_ids"].as_array().unwrap().len(), 2);
}

// ── Multiple hints projected ──────────────────────────────────────

#[tokio::test]
async fn external_ingest_projects_multiple_candidates_from_hints() {
    let state = AppState::new(test_audit());
    let app = build_app(state);

    let payload = serde_json::json!({
        "source_system": "linear",
        "source_uri": "https://linear.app/team/ABC-123",
        "content": "Sprint planning: we decided to use event sourcing for order processing and introduce a GraphQL gateway for mobile clients",
        "evidence": [{
            "excerpt": "we decided to use event sourcing for order processing and introduce a GraphQL gateway for mobile clients",
            "confidence": 0.85
        }],
        "candidate_hints": [
            {
                "title": "Migrate to event sourcing",
                "body": "We decided to use event sourcing for the order processing domain",
                "level": "architecture",
                "labels": ["backend"]
            },
            {
                "title": "Add GraphQL gateway",
                "body": "We decided to introduce a GraphQL gateway for mobile clients",
                "level": "architecture",
                "labels": ["api", "mobile"]
            }
        ]
    });

    let (status, body) = post_json(app, "/api/v1/external-ingest", payload).await;
    assert_eq!(status, StatusCode::CREATED, "body: {body}");

    let resp: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(resp["projection_outcome"], "candidate_projected");
    assert_eq!(
        resp["candidate_ids"].as_array().unwrap().len(),
        2,
        "Two hints should produce two projected candidates"
    );
}

// ── Projection outcome field presence ─────────────────────────────

#[tokio::test]
async fn external_ingest_response_includes_projection_outcome() {
    let state = AppState::new(test_audit());
    let app = build_app(state);

    let payload = serde_json::json!({
        "source_system": "slack",
        "source_uri": "https://slack.example.com/C555/p999",
        "content": "Routine standup notes",
        "evidence": [{
            "excerpt": "standup notes",
            "confidence": 0.5
        }]
    });

    let (status, body) = post_json(app, "/api/v1/external-ingest", payload).await;
    assert_eq!(status, StatusCode::CREATED, "body: {body}");

    let resp: serde_json::Value = serde_json::from_str(&body).unwrap();
    // projection_outcome must always be present in the response.
    assert!(
        resp.get("projection_outcome").is_some(),
        "projection_outcome field must be present in external ingest response"
    );
}
