//! External adapter ingest gateway contract tests.
//!
//! Verifies the acceptance criteria from issue #264:
//!
//! 1. External ingest rejects local actor authority, review commands,
//!    signoff approval, compliance resolution, and policy grants.
//! 2. Accepted adapter ingest creates source/evidence records compatible
//!    with the V1 advisory golden path.
//! 3. Tests cover accepted adapter ingest, rejection/refusal, and
//!    authority-injection attempts.

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

/// Helper: POST JSON to a route and return (status, body string).
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&body).to_string())
}

fn valid_redaction_receipt() -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "engine": "bicameral-stdlib-redaction",
        "engine_version": "1.0.0",
        "ruleset_id": "fx-sec-001-plus-pii-v1",
        "ruleset_digest": format!("sha256:{}", "1".repeat(64)),
        "input_digest": format!("sha256:{}", "2".repeat(64)),
        "output_digest": format!("sha256:{}", "3".repeat(64)),
        "findings": [{"category": "pii", "action": "tokenized", "count": 1}],
        "structural_fields_preserved": true,
        "completed_at": "2026-07-19T00:00:00Z"
    })
}

#[tokio::test]
async fn external_ingest_capabilities_advertise_strict_v2_contract() {
    let app = build_app(AppState::new(test_audit()));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/external-ingest/capabilities")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let capability: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(capability["protocol_version"], 2);
    assert_eq!(
        capability["supported_protocol_versions"],
        serde_json::json!([2])
    );
    assert_eq!(capability["delivery_endpoint"], "/api/v2/external-ingest");
    assert_eq!(capability["redaction_receipt_required"], true);
}

#[tokio::test]
async fn external_ingest_v2_requires_receipt_and_accepts_receipted_delivery() {
    let payload = serde_json::json!({
        "source_system": "github",
        "source_uri": "https://github.com/BicameralAI/example/issues/7",
        "content": "sanitized evidence",
        "evidence": [{"excerpt": "sanitized evidence"}]
    });

    let (status, body) = post_json(
        build_app(AppState::new(test_audit())),
        "/api/v2/external-ingest",
        payload.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
    assert!(body.contains("requires redaction_receipt"));

    let mut receipted = payload;
    receipted["redaction_receipt"] = valid_redaction_receipt();
    let (status, body) = post_json(
        build_app(AppState::new(test_audit())),
        "/api/v2/external-ingest",
        receipted,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "body: {body}");
}

// ── Happy path: accepted external adapter ingest ─────────────────

#[tokio::test]
async fn external_ingest_accepts_valid_envelope() {
    let state = AppState::new(test_audit());
    let app = build_app(state);

    let payload = serde_json::json!({
        "source_system": "jira",
        "source_uri": "https://jira.example.com/PROJ-42",
        "content": "The service must implement rate limiting on public endpoints",
        "evidence": [
            {
                "excerpt": "rate limiting on public endpoints",
                "span_start": 30,
                "span_end": 62,
                "confidence": 0.85
            }
        ],
        "candidate_hints": [
            {
                "title": "Implement rate limiting",
                "body": "Public endpoints need rate limiting per ADR-0015",
                "level": "architecture",
                "labels": ["security", "api"]
            }
        ]
    });

    let (status, body) = post_json(app, "/api/v1/external-ingest", payload).await;
    assert_eq!(status, StatusCode::CREATED, "body: {body}");

    let resp: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(resp["source_id"], "https://jira.example.com/PROJ-42");
    assert!(resp["snapshot_id"].is_string());
    assert!(!resp["evidence_ids"].as_array().unwrap().is_empty());
    assert!(!resp["candidate_ids"].as_array().unwrap().is_empty());
    assert!(!resp["advisory_results"].as_array().unwrap().is_empty());

    let result = &resp["advisory_results"][0];
    assert_eq!(result["action"], "accepted_as_candidate");
    assert_eq!(result["review_state"], "proposed");
    assert_eq!(result["candidate_state"], "proposed");
}

#[tokio::test]
async fn external_ingest_accepts_evidence_only_no_hints() {
    let state = AppState::new(test_audit());
    let app = build_app(state);

    let payload = serde_json::json!({
        "source_system": "slack",
        "source_uri": "https://slack.example.com/messages/C123/p456",
        "content": "Routine implementation status update with no proposed decision",
        "evidence": [
            {
                "excerpt": "Routine implementation status update",
                "confidence": 0.9
            }
        ]
    });

    let (status, body) = post_json(app, "/api/v1/external-ingest", payload).await;
    assert_eq!(status, StatusCode::CREATED, "body: {body}");

    let resp: serde_json::Value = serde_json::from_str(&body).unwrap();
    // Evidence-only ingest without candidate hints: daemon projection finds
    // no decision-like claim, so no candidates are materialized (per spec).
    assert!(resp["candidate_ids"].as_array().unwrap().is_empty());
    assert_eq!(resp["projection_outcome"], "no_candidate");
}

#[tokio::test]
async fn external_ingest_creates_snapshot_with_content_hash() {
    let state = AppState::new(test_audit());
    let app = build_app(state.clone());

    let content = "Architecture decision: use event sourcing";
    let payload = serde_json::json!({
        "source_system": "notion",
        "source_uri": "https://notion.so/page-123",
        "content": content,
        "evidence": [
            { "excerpt": "use event sourcing" }
        ]
    });

    let (status, body) = post_json(app, "/api/v1/external-ingest", payload).await;
    assert_eq!(status, StatusCode::CREATED, "body: {body}");

    let resp: serde_json::Value = serde_json::from_str(&body).unwrap();
    let snapshot_id = resp["snapshot_id"].as_str().unwrap();
    assert!(snapshot_id.starts_with("sha256:"));

    // Verify snapshot is materialized in state
    let snapshots = state.snapshots.read().await;
    let snap = snapshots.get(snapshot_id).unwrap();
    assert_eq!(snap.content.as_deref(), Some(content));
}

// ── Rejection: empty evidence ────────────────────────────────────

#[tokio::test]
async fn external_ingest_rejects_empty_evidence_and_no_hints() {
    let state = AppState::new(test_audit());
    let app = build_app(state);

    let payload = serde_json::json!({
        "source_system": "github",
        "source_uri": "https://github.com/org/repo/issues/1",
        "content": "Some content"
    });

    let (status, body) = post_json(app, "/api/v1/external-ingest", payload).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "body: {body}");

    let resp: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(resp["reason"], "empty_evidence");
}

// ── Rejection: authority injection attempts ──────────────────────

#[tokio::test]
async fn external_ingest_rejects_authority_context_injection() {
    let state = AppState::new(test_audit());
    let app = build_app(state);

    let payload = serde_json::json!({
        "source_system": "jira",
        "source_uri": "https://jira.example.com/PROJ-1",
        "content": "Decision content",
        "evidence": [{ "excerpt": "something" }],
        "authority": {
            "actor_id": "attacker",
            "auth_method": "local_daemon"
        }
    });

    let (status, body) = post_json(app, "/api/v1/external-ingest", payload).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body}");

    let resp: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(resp["reason"], "authority_injection");
}

#[tokio::test]
async fn external_ingest_rejects_actor_id_injection() {
    let state = AppState::new(test_audit());
    let app = build_app(state);

    let payload = serde_json::json!({
        "source_system": "slack",
        "source_uri": "https://slack.example.com/msg/1",
        "content": "Content",
        "evidence": [{ "excerpt": "ex" }],
        "actor_id": "fake-admin"
    });

    let (status, body) = post_json(app, "/api/v1/external-ingest", payload).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body}");

    let resp: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(resp["reason"], "authority_injection");
}

#[tokio::test]
async fn external_ingest_rejects_session_id_injection() {
    let state = AppState::new(test_audit());
    let app = build_app(state);

    let payload = serde_json::json!({
        "source_system": "github",
        "source_uri": "https://github.com/org/repo/pr/5",
        "content": "PR content",
        "evidence": [{ "excerpt": "excerpt" }],
        "session_id": "hijacked-session-123"
    });

    let (status, body) = post_json(app, "/api/v1/external-ingest", payload).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body}");

    let resp: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(resp["reason"], "authority_injection");
}

// ── Rejection: review command injection ──────────────────────────

#[tokio::test]
async fn external_ingest_rejects_review_command_injection() {
    let state = AppState::new(test_audit());
    let app = build_app(state);

    let payload = serde_json::json!({
        "source_system": "jira",
        "source_uri": "https://jira.example.com/PROJ-2",
        "content": "Content",
        "evidence": [{ "excerpt": "ex" }],
        "review_command": "approve_signoff"
    });

    let (status, body) = post_json(app, "/api/v1/external-ingest", payload).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body}");

    let resp: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(resp["reason"], "review_command_injection");
}

#[tokio::test]
async fn external_ingest_rejects_tool_command_injection() {
    let state = AppState::new(test_audit());
    let app = build_app(state);

    let payload = serde_json::json!({
        "source_system": "linear",
        "source_uri": "https://linear.app/team/issue/ENG-1",
        "content": "Issue content",
        "evidence": [{ "excerpt": "ex" }],
        "tool_command": {
            "name": "review.accept_candidate",
            "params": { "target_id": "CAND-1" }
        }
    });

    let (status, body) = post_json(app, "/api/v1/external-ingest", payload).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body}");

    let resp: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(resp["reason"], "review_command_injection");
}

// ── Rejection: signoff injection ─────────────────────────────────

#[tokio::test]
async fn external_ingest_rejects_signoff_approval_injection() {
    let state = AppState::new(test_audit());
    let app = build_app(state);

    let payload = serde_json::json!({
        "source_system": "slack",
        "source_uri": "https://slack.example.com/msg/2",
        "content": "Content",
        "evidence": [{ "excerpt": "ex" }],
        "approve_signoff": { "target_id": "DEC-1" }
    });

    let (status, body) = post_json(app, "/api/v1/external-ingest", payload).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body}");

    let resp: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(resp["reason"], "signoff_injection");
}

// ── Rejection: compliance resolution injection ───────────────────

#[tokio::test]
async fn external_ingest_rejects_compliance_resolution_injection() {
    let state = AppState::new(test_audit());
    let app = build_app(state);

    let payload = serde_json::json!({
        "source_system": "github",
        "source_uri": "https://github.com/org/repo/issues/3",
        "content": "Content",
        "evidence": [{ "excerpt": "ex" }],
        "resolve_compliance": { "target_id": "DEC-1", "verdict": "reflected" }
    });

    let (status, body) = post_json(app, "/api/v1/external-ingest", payload).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body}");

    let resp: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(resp["reason"], "compliance_injection");
}

// ── Rejection: policy grant injection ────────────────────────────

#[tokio::test]
async fn external_ingest_rejects_policy_grant_injection() {
    let state = AppState::new(test_audit());
    let app = build_app(state);

    let payload = serde_json::json!({
        "source_system": "notion",
        "source_uri": "https://notion.so/page-456",
        "content": "Content",
        "evidence": [{ "excerpt": "ex" }],
        "policy_grant": { "scope": "all", "level": "admin" }
    });

    let (status, body) = post_json(app, "/api/v1/external-ingest", payload).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body}");

    let resp: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(resp["reason"], "policy_grant_injection");
}

#[tokio::test]
async fn external_ingest_rejects_governance_override_injection() {
    let state = AppState::new(test_audit());
    let app = build_app(state);

    let payload = serde_json::json!({
        "source_system": "github",
        "source_uri": "https://github.com/org/repo/pr/10",
        "content": "Content",
        "evidence": [{ "excerpt": "ex" }],
        "governance_override": { "action": "force_accept" }
    });

    let (status, body) = post_json(app, "/api/v1/external-ingest", payload).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body}");

    let resp: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(resp["reason"], "policy_grant_injection");
}

// ── Rejection: invalid payload ───────────────────────────────────

#[tokio::test]
async fn external_ingest_rejects_invalid_json() {
    let state = AppState::new(test_audit());
    let app = build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/external-ingest")
                .header("content-type", "application/json")
                .body(Body::from("not valid json"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn external_ingest_rejects_missing_required_fields() {
    let state = AppState::new(test_audit());
    let app = build_app(state);

    // Missing source_system, source_uri, content
    let payload = serde_json::json!({
        "evidence": [{ "excerpt": "something" }]
    });

    let (status, body) = post_json(app, "/api/v1/external-ingest", payload).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");

    let resp: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(resp["reason"], "invalid_payload");
}

// ── Compound injection: multiple forbidden fields ────────────────

#[tokio::test]
async fn external_ingest_rejects_combined_authority_and_review() {
    let state = AppState::new(test_audit());
    let app = build_app(state);

    // Contains both authority injection AND review command
    let payload = serde_json::json!({
        "source_system": "jira",
        "source_uri": "https://jira.example.com/PROJ-99",
        "content": "Decision content",
        "evidence": [{ "excerpt": "ex" }],
        "authority": { "actor_id": "attacker", "auth_method": "local_daemon" },
        "review_command": "approve_signoff",
        "canonical_decision": { "id": "DEC-FAKE" }
    });

    let (status, _body) = post_json(app, "/api/v1/external-ingest", payload).await;
    // Should be rejected (first detected forbidden field wins)
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ── V1 golden path compatibility ─────────────────────────────────

#[tokio::test]
async fn external_ingest_materializes_source_evidence_candidate() {
    let state = AppState::new(test_audit());
    let app = build_app(state.clone());

    let payload = serde_json::json!({
        "source_system": "linear",
        "source_uri": "https://linear.app/team/issue/ENG-42",
        "content": "We need to migrate the auth service to OAuth2",
        "evidence": [
            {
                "excerpt": "migrate the auth service to OAuth2",
                "span_start": 11,
                "span_end": 45,
                "confidence": 0.92
            }
        ],
        "candidate_hints": [
            {
                "title": "Migrate auth to OAuth2",
                "body": "Auth service should use OAuth2 per security review",
                "level": "architecture"
            }
        ]
    });

    let (status, _) = post_json(app, "/api/v1/external-ingest", payload).await;
    assert_eq!(status, StatusCode::CREATED);

    // Verify source is materialized
    let sources = state.sources.read().await;
    assert!(sources.contains_key("https://linear.app/team/issue/ENG-42"));

    // Verify evidence is materialized
    let evidence = state.evidence.read().await;
    assert!(!evidence.is_empty());
    let ev = evidence.values().next().unwrap();
    assert_eq!(ev.excerpt, "migrate the auth service to OAuth2");
    assert_eq!(ev.span_start, 11);
    assert_eq!(ev.span_end, 45);

    // Verify candidate is materialized with review state
    let candidates = state.candidates.read().await;
    assert!(!candidates.is_empty());
    let cand = candidates.values().next().unwrap();
    assert_eq!(cand.title, "Migrate auth to OAuth2");

    // Verify review state exists for the candidate
    let reviews = state.reviews.read().await;
    assert!(reviews.contains_key(&cand.id));
}

// ── Regression: improved invalid-payload error messages ──────────

#[tokio::test]
async fn external_ingest_invalid_payload_names_expected_fields() {
    let state = AppState::new(test_audit());
    let app = build_app(state);

    // Payload missing required `source_uri` and `content` fields.
    let payload = serde_json::json!({
        "source_system": "slack"
    });

    let (status, body) = post_json(app, "/api/v1/external-ingest", payload).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");

    let resp: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(resp["reason"], "invalid_payload");

    let msg = resp["message"].as_str().unwrap();
    // Error must name the expected fields so the integration author knows
    // what to fix without reading the full spec.
    assert!(
        msg.contains("source_uri"),
        "Error should name expected field 'source_uri': {msg}"
    );
    assert!(
        msg.contains("content"),
        "Error should name expected field 'content': {msg}"
    );
}

#[tokio::test]
async fn external_ingest_common_alias_produces_actionable_hint() {
    let state = AppState::new(test_audit());
    let app = build_app(state);

    // Payload uses common alias fields instead of canonical names.
    let payload = serde_json::json!({
        "source_system": "jira",
        "source_ref": "https://jira.example.com/PROJ-789",
        "body": "Sprint planning: auth must support SAML SSO",
        "evidence": [
            { "title": "SAML SSO requirement" }
        ]
    });

    let (status, body) = post_json(app, "/api/v1/external-ingest", payload).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");

    let resp: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(resp["reason"], "invalid_payload");

    let msg = resp["message"].as_str().unwrap();
    // Must include actionable field mapping hints.
    assert!(
        msg.contains("source_ref") && msg.contains("source_uri"),
        "Error should hint 'source_ref' → 'source_uri': {msg}"
    );
    assert!(
        msg.contains("'body' is not accepted") && msg.contains("'content'"),
        "Error should hint 'body' → 'content': {msg}"
    );
    assert!(
        msg.contains("evidence[]") && msg.contains("'excerpt'"),
        "Error should hint evidence alias → 'excerpt': {msg}"
    );
}
