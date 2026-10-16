//! MCP v0.2 integration tests — exercises bot-side contract routes for
//! ingest, preflight, bind, review, history, and search.
//!
//! Validates response shapes against `docs/specs/mcp-integration-v02.md`.
//! The bot owns persistence, grounding, and materialized projections; MCP is
//! a thin local tool surface that consumes these.
//!
//! Known gaps requiring the separate `bicameral-mcp` repo:
//! - End-to-end MCP client round-trip (MCP SDK → bot → response).
//! - Cross-repo grounding and hosted code-graph queries.
//! - SymbolDB import/caller/callee graph traversal.
//! - Historical grounding and time-series drift analysis.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use bicameral_audit::AuditStore;
use bicameral_gateway::routes;
use bicameral_gateway::state::AppState;
use serde_json::Value;
use tower::ServiceExt;

// ── Helpers ──────────────────────────────────────────────────────

fn test_state() -> AppState {
    let dir = tempfile::tempdir().unwrap();
    let audit = AuditStore::open(dir.path()).unwrap();
    AppState::new(audit)
}

fn full_app(state: AppState) -> Router {
    Router::new()
        .merge(routes::health_routes())
        .merge(routes::ingest_routes())
        .merge(routes::review_routes())
        .merge(routes::status_routes())
        .merge(routes::dashboard_api_routes())
        .merge(routes::query_routes())
        .merge(routes::bind_routes())
        .merge(routes::compliance_routes())
        .with_state(state)
}

async fn post_json(app: Router, uri: &str, body: &Value) -> (StatusCode, Value) {
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let parsed: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, parsed)
}

async fn get_json(app: Router, uri: &str) -> (StatusCode, Value) {
    let resp = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let parsed: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, parsed)
}

/// Ingest a candidate and return the response body.
async fn ingest(app: &Router, title: &str, desc: &str, level: &str) -> Value {
    let body = serde_json::json!({
        "title": title,
        "description": desc,
        "level": level,
        "source": format!("https://example.com/{}", title.to_lowercase().replace(' ', "-")),
        "source_type": "github",
        "label": title,
        "snapshot_content": format!("Snapshot for {}: {}", title, desc),
        "evidence": [{
            "span_start": 0,
            "span_end": desc.len(),
            "excerpt": desc,
            "confidence": 0.9
        }]
    });
    let (status, parsed) = post_json(app.clone(), "/api/v1/ingest", &body).await;
    assert_eq!(status, StatusCode::CREATED, "Ingest failed: {:?}", parsed);
    parsed
}

/// Accept a candidate to promote it to a Decision.
async fn accept_candidate(app: &Router, candidate_id: &str) -> Value {
    let cmd = serde_json::json!({
        "target_id": candidate_id,
        "command": "accept_candidate"
    });
    let (status, parsed) = post_json(app.clone(), "/api/v1/dashboard/command", &cmd).await;
    assert_eq!(
        status,
        StatusCode::ACCEPTED,
        "accept_candidate failed: {:?}",
        parsed
    );
    parsed
}

// ── 1. Ingest contract tests ─────────────────────────────────────

#[tokio::test]
async fn ingest_response_matches_v02_spec() {
    let state = test_state();
    let app = full_app(state);

    let resp = ingest(
        &app,
        "JWT token validation",
        "All auth endpoints must validate JWT tokens before processing",
        "architecture",
    )
    .await;

    // Spec §2.2 `bicameral.ingest` output fields
    assert!(resp["source_id"].is_string(), "Missing source_id");
    assert!(
        resp["snapshot_id"].is_string() || resp["snapshot_id"].is_null(),
        "snapshot_id should be string or null"
    );
    assert!(resp["evidence_ids"].is_array(), "Missing evidence_ids");
    assert!(
        !resp["evidence_ids"].as_array().unwrap().is_empty(),
        "evidence_ids should not be empty"
    );
    assert!(resp["candidate_ids"].is_array(), "Missing candidate_ids");
    assert!(
        !resp["candidate_ids"].as_array().unwrap().is_empty(),
        "candidate_ids should not be empty"
    );

    // governance_results per spec
    let gov = &resp["governance_results"];
    assert!(gov.is_array(), "Missing governance_results");
    let first = &gov[0];
    assert!(first["candidate_id"].is_string());
    assert_eq!(first["action"].as_str().unwrap(), "accepted_as_candidate");
    assert!(first["reason"].is_string());

    // review_state and candidate_state match spec enums
    let review_state = first["review_state"].as_str().unwrap();
    assert!(
        [
            "advisory",
            "proposed",
            "needs_signoff_review",
            "ready_to_materialize"
        ]
        .contains(&review_state),
        "Unexpected review_state: {}",
        review_state
    );
    let candidate_state = first["candidate_state"].as_str().unwrap();
    assert!(
        ["proposed", "rejected"].contains(&candidate_state),
        "Unexpected candidate_state: {}",
        candidate_state
    );
}

#[tokio::test]
async fn ingest_rejects_empty_evidence() {
    let state = test_state();
    let app = full_app(state);

    let body = serde_json::json!({
        "title": "Empty evidence",
        "description": "Should be rejected",
        "level": "product",
        "source": "test",
        "evidence": []
    });
    let (status, parsed) = post_json(app, "/api/v1/ingest", &body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(parsed["reason"].is_string(), "Rejection should have reason");
}

#[tokio::test]
async fn ingest_does_not_create_local_artifacts() {
    let state = test_state();
    let app = full_app(state.clone());

    ingest(&app, "Artifact check", "No local artifacts", "product").await;

    // The implementation uses in-memory state only — no filesystem writes.
    // Verify no MCP-owned persistence was created.
    assert!(
        !std::path::Path::new(".bicameral/mcp").exists(),
        "MCP-owned persistence directory should not exist"
    );
    assert!(
        !std::path::Path::new(".mcp").exists(),
        "MCP artifact directory should not exist"
    );
}

// ── 2. Preflight contract tests ──────────────────────────────────

#[tokio::test]
async fn preflight_response_matches_v02_spec() {
    let state = test_state();
    let app = full_app(state.clone());

    // Seed: ingest + promote to decision
    let resp = ingest(
        &app,
        "Deploy pipeline gating",
        "All deploy pipelines must pass integration tests before release",
        "architecture",
    )
    .await;
    let cand_id = resp["candidate_ids"][0].as_str().unwrap();
    accept_candidate(&app, cand_id).await;

    // Preflight with matching file context
    let pf_body = serde_json::json!({
        "files": ["src/deploy_pipeline.rs"],
        "symbols": [],
        "branch": "main"
    });
    let (status, parsed) = post_json(app, "/api/v1/preflight", &pf_body).await;
    assert_eq!(status, StatusCode::OK);

    // Spec §2.2 `bicameral.preflight` output structure
    assert!(
        parsed["relevant_decisions"].is_array(),
        "Missing relevant_decisions"
    );
    assert!(
        parsed["relevant_candidates"].is_array(),
        "Missing relevant_candidates"
    );
    assert!(parsed["warnings"].is_array(), "Missing warnings");

    let decisions = parsed["relevant_decisions"].as_array().unwrap();
    assert!(
        !decisions.is_empty(),
        "Should find the deploy pipeline decision"
    );

    let d = &decisions[0];
    assert!(d["decision_id"].is_string(), "Missing decision_id");
    assert!(d["title"].is_string(), "Missing title");
    assert!(d["level"].is_string(), "Missing level");
    assert!(d["signoff_state"].is_string(), "Missing signoff_state");
    assert!(
        d["compliance_state"].is_string(),
        "Missing compliance_state"
    );
    assert!(d["review_state"].is_string(), "Missing review_state");
    assert!(
        d["relevance_reasons"].is_array(),
        "Missing relevance_reasons"
    );

    // ADR-0019: response must include readiness metadata.
    assert!(
        parsed["readiness"].is_object(),
        "Missing readiness in response"
    );
    assert!(
        parsed["readiness"]["state"].is_string(),
        "Missing readiness.state"
    );
    assert!(
        parsed["readiness"]["snapshot_scope"].is_array(),
        "Missing readiness.snapshot_scope"
    );
    assert!(
        parsed["readiness"]["unknown_scope"].is_array(),
        "Missing readiness.unknown_scope"
    );

    // Source-only preflight must report ungrounded (no graph available)
    assert_eq!(
        d["compliance_state"].as_str().unwrap(),
        "ungrounded",
        "Source-only preflight must report ungrounded compliance"
    );
}

#[tokio::test]
async fn preflight_no_false_relevance() {
    let state = test_state();
    let app = full_app(state.clone());

    let resp = ingest(
        &app,
        "Database schema migration",
        "Use PostgreSQL for production database",
        "architecture",
    )
    .await;
    let cand_id = resp["candidate_ids"][0].as_str().unwrap();
    accept_candidate(&app, cand_id).await;

    // Preflight with completely unrelated file
    let pf_body = serde_json::json!({
        "files": ["src/completely_unrelated_widget.rs"]
    });
    let (status, parsed) = post_json(app, "/api/v1/preflight", &pf_body).await;
    assert_eq!(status, StatusCode::OK);

    let decisions = parsed["relevant_decisions"].as_array().unwrap();
    assert!(
        decisions.is_empty(),
        "Must not claim false relevance for unrelated files"
    );
}

#[tokio::test]
async fn preflight_full_ledger_when_no_filters() {
    let state = test_state();
    let app = full_app(state.clone());

    let resp = ingest(
        &app,
        "API versioning",
        "All public APIs must be versioned",
        "product",
    )
    .await;
    let cand_id = resp["candidate_ids"][0].as_str().unwrap();
    accept_candidate(&app, cand_id).await;

    // Preflight with no file/symbol/diff filter → full ledger
    let pf_body = serde_json::json!({});
    let (status, parsed) = post_json(app, "/api/v1/preflight", &pf_body).await;
    assert_eq!(status, StatusCode::OK);

    let decisions = parsed["relevant_decisions"].as_array().unwrap();
    assert!(
        !decisions.is_empty(),
        "Full ledger should include all decisions"
    );

    let warnings = parsed["warnings"].as_array().unwrap();
    assert!(
        warnings
            .iter()
            .any(|w| w.as_str().unwrap().contains("full ledger")
                || w.as_str().unwrap().contains("No file")),
        "Should warn about returning full ledger"
    );
}

#[tokio::test]
async fn preflight_symbol_grounding_warning() {
    let state = test_state();
    let app = full_app(state);

    let pf_body = serde_json::json!({
        "symbols": ["foo::bar::Baz"]
    });
    let (status, parsed) = post_json(app, "/api/v1/preflight", &pf_body).await;
    assert_eq!(status, StatusCode::OK);

    let warnings = parsed["warnings"].as_array().unwrap();
    assert!(
        warnings
            .iter()
            .any(|w| w.as_str().unwrap().contains("Symbol grounding")),
        "Should warn that symbol grounding is unavailable"
    );
}

// ── 3. Bind contract tests ───────────────────────────────────────

#[tokio::test]
async fn bind_verified_evidence_materializes() {
    let state = test_state();
    let app = full_app(state.clone());

    // Seed a decision
    let resp = ingest(
        &app,
        "Error handling strategy",
        "All service endpoints must use typed error responses",
        "architecture",
    )
    .await;
    let cand_id = resp["candidate_ids"][0].as_str().unwrap();
    accept_candidate(&app, cand_id).await;

    let decision_id = state
        .decisions
        .read()
        .await
        .keys()
        .next()
        .unwrap()
        .to_string();

    // Build a BindingProposal (graph fact from runtime prevalidation)
    let cand_uuid: uuid::Uuid = cand_id.parse().unwrap();
    let proposal = serde_json::json!({
        "candidate_id": cand_uuid.to_string(),
        "binding_type": "code_region",
        "target": "src/handlers/errors.rs:10-25",
        "confidence": "high",
        "graph_snapshot_id": "snap-test-001",
        "validated_sha": "abc123def456",
        "content_hash": "sha256:deadbeef",
    });

    let bind_body = serde_json::json!({
        "decision_id": decision_id,
        "proposal": proposal,
        "evidence_state": "verified",
        "line_start": 10,
        "line_end": 25
    });

    let (status, parsed) = post_json(app.clone(), "/api/v1/bind", &bind_body).await;
    assert_eq!(status, StatusCode::CREATED, "Bind failed: {:?}", parsed);

    // Spec §2.2 `bicameral.bind` output for verified binding
    assert_eq!(parsed["status"].as_str().unwrap(), "bound");
    assert!(parsed["binding_id"].is_string(), "Missing binding_id");
    assert!(parsed["decision_id"].is_string(), "Missing decision_id");
    assert!(parsed["target"].is_string(), "Missing target");
    assert!(
        parsed["graph_snapshot_id"].is_string(),
        "Missing graph_snapshot_id"
    );
    assert!(parsed["validated_sha"].is_string(), "Missing validated_sha");

    // Verify binding was materialized in state
    let bindings = state.bindings.read().await;
    let dec_uuid = uuid::Uuid::parse_str(&decision_id).unwrap();
    assert!(
        bindings.contains_key(&dec_uuid),
        "Binding should be materialized"
    );
    assert_eq!(bindings[&dec_uuid].len(), 1);
}

#[tokio::test]
async fn bind_rejects_non_verified_evidence() {
    let state = test_state();
    let app = full_app(state.clone());

    // Seed a decision
    let resp = ingest(
        &app,
        "Logging standard",
        "All services must use structured logging",
        "implementation",
    )
    .await;
    let cand_id = resp["candidate_ids"][0].as_str().unwrap();
    accept_candidate(&app, cand_id).await;

    let decision_id = state
        .decisions
        .read()
        .await
        .keys()
        .next()
        .unwrap()
        .to_string();

    let cand_uuid: uuid::Uuid = cand_id.parse().unwrap();
    let proposal = serde_json::json!({
        "candidate_id": cand_uuid.to_string(),
        "binding_type": "code_region",
        "target": "src/logging.rs:1-50",
        "confidence": "high",
        "graph_snapshot_id": "snap-test-002",
        "validated_sha": "abc123",
    });

    let bind_body = serde_json::json!({
        "decision_id": decision_id,
        "proposal": proposal,
        "evidence_state": "not_found",
    });

    let (status, parsed) = post_json(app, "/api/v1/bind", &bind_body).await;
    // Non-verified evidence should be rejected
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(parsed["status"].as_str().unwrap(), "rejected");
    assert!(
        parsed["evidence_state"].is_string(),
        "Rejection should include evidence_state"
    );
    assert!(
        parsed["detail"].is_string(),
        "Rejection should include detail"
    );
}

#[tokio::test]
async fn bind_returns_404_for_unknown_decision() {
    let state = test_state();
    let app = full_app(state);

    let fake_decision_id = uuid::Uuid::new_v4();
    let proposal = serde_json::json!({
        "candidate_id": uuid::Uuid::new_v4().to_string(),
        "binding_type": "file_path",
        "target": "src/foo.rs",
        "confidence": "high",
        "graph_snapshot_id": "snap-none",
        "validated_sha": "000000",
    });

    let bind_body = serde_json::json!({
        "decision_id": fake_decision_id.to_string(),
        "proposal": proposal,
        "evidence_state": "verified",
    });

    let (status, _) = post_json(app, "/api/v1/bind", &bind_body).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "Bind to unknown decision should return 404"
    );
}

// ── 4. Review contract tests ─────────────────────────────────────

#[tokio::test]
async fn review_submit_and_approve_lifecycle() {
    let state = test_state();
    let app = full_app(state.clone());

    // Ingest
    let resp = ingest(
        &app,
        "Rate limiting",
        "All public endpoints must enforce rate limits",
        "architecture",
    )
    .await;
    let cand_id = resp["candidate_ids"][0].as_str().unwrap();
    let cand_uuid: uuid::Uuid = cand_id.parse().unwrap();

    // Submit review command (ReviewCommand expects id, kind, candidate_id, issued_by, issued_at)
    let review_cmd = serde_json::json!({
        "id": uuid::Uuid::new_v4().to_string(),
        "candidate_id": cand_id,
        "kind": "submit",
        "issued_by": "test-reviewer",
        "issued_at": chrono::Utc::now().to_rfc3339()
    });
    let (status, parsed) = post_json(app.clone(), "/api/v1/review", &review_cmd).await;
    assert_eq!(status, StatusCode::OK, "Review submit failed: {:?}", parsed);

    // Spec §2.2 `bicameral.review` output fields
    assert!(parsed["candidate_id"].is_string(), "Missing candidate_id");
    assert!(parsed["status"].is_string(), "Missing status");
    assert_eq!(parsed["status"].as_str().unwrap(), "in_review");

    // Approve
    let approve_cmd = serde_json::json!({
        "id": uuid::Uuid::new_v4().to_string(),
        "candidate_id": cand_id,
        "kind": "approve",
        "issued_by": "test-reviewer",
        "issued_at": chrono::Utc::now().to_rfc3339()
    });
    let (status, parsed) = post_json(app.clone(), "/api/v1/review", &approve_cmd).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "Review approve failed: {:?}",
        parsed
    );
    assert_eq!(parsed["status"].as_str().unwrap(), "approved");

    // Get review state
    let (status, parsed) = get_json(app, &format!("/api/v1/review/{}", cand_uuid)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(parsed["status"].as_str().unwrap(), "approved");
    assert!(parsed["history"].is_array(), "Missing review history");
    assert!(
        parsed["history"].as_array().unwrap().len() >= 2,
        "History should contain submit + approve"
    );
}

#[tokio::test]
async fn review_reject_lifecycle() {
    let state = test_state();
    let app = full_app(state.clone());

    let resp = ingest(
        &app,
        "Rejected decision",
        "This will be rejected",
        "product",
    )
    .await;
    let cand_id = resp["candidate_ids"][0].as_str().unwrap();

    let reject_cmd = serde_json::json!({
        "id": uuid::Uuid::new_v4().to_string(),
        "candidate_id": cand_id,
        "kind": "reject",
        "issued_by": "test-reviewer",
        "issued_at": chrono::Utc::now().to_rfc3339()
    });
    let (status, parsed) = post_json(app, "/api/v1/review", &reject_cmd).await;
    assert_eq!(status, StatusCode::OK, "Review reject failed: {:?}", parsed);
    assert_eq!(parsed["status"].as_str().unwrap(), "rejected");
}

#[tokio::test]
async fn review_returns_404_for_unknown_candidate() {
    let state = test_state();
    let app = full_app(state);

    let cmd = serde_json::json!({
        "id": uuid::Uuid::new_v4().to_string(),
        "candidate_id": uuid::Uuid::new_v4().to_string(),
        "kind": "submit",
        "issued_by": "test",
        "issued_at": chrono::Utc::now().to_rfc3339()
    });
    let (status, _) = post_json(app, "/api/v1/review", &cmd).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ── 5. History contract tests ────────────────────────────────────

#[tokio::test]
async fn history_response_matches_v02_spec() {
    let state = test_state();
    let app = full_app(state.clone());

    // Ingest + promote
    let resp = ingest(
        &app,
        "Caching layer design",
        "Use Redis for session caching with 15-minute TTL",
        "architecture",
    )
    .await;
    let cand_id = resp["candidate_ids"][0].as_str().unwrap();
    accept_candidate(&app, cand_id).await;

    // Query history with all options
    let (status, parsed) = get_json(
        app,
        "/api/v1/history?include_candidates=true&include_events=true",
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Spec §2.2 `bicameral.history` output structure
    assert!(parsed["decisions"].is_array(), "Missing decisions");
    let decisions = parsed["decisions"].as_array().unwrap();
    assert_eq!(decisions.len(), 1);

    let d = &decisions[0];
    assert!(d["decision_id"].is_string(), "Missing decision_id");
    assert!(d["candidate_id"].is_string(), "Missing candidate_id");
    assert!(d["title"].is_string(), "Missing title");
    assert!(d["body"].is_string(), "Missing body");
    assert!(d["level"].is_string(), "Missing level");
    assert!(d["signoff_state"].is_string(), "Missing signoff_state");
    assert!(
        d["compliance_state"].is_string(),
        "Missing compliance_state"
    );
    assert!(d["review_state"].is_string(), "Missing review_state");
    assert!(d["promoted_at"].is_string(), "Missing promoted_at");
    assert!(d["promoted_by"].is_string(), "Missing promoted_by");
    assert_eq!(d["title"].as_str().unwrap(), "Caching layer design");

    // Candidates included
    assert!(
        parsed["candidates"].is_array(),
        "Missing candidates when include_candidates=true"
    );
    let candidates = parsed["candidates"].as_array().unwrap();
    assert!(!candidates.is_empty());
    let c = &candidates[0];
    assert!(c["candidate_id"].is_string());
    assert!(c["title"].is_string());
    assert!(c["level"].is_string());
    assert!(c["candidate_state"].is_string());

    // Events included
    assert!(
        parsed["events"].is_array(),
        "Missing events when include_events=true"
    );
}

#[tokio::test]
async fn history_empty_without_promoted_decisions() {
    let state = test_state();
    let app = full_app(state);

    let (status, parsed) = get_json(app, "/api/v1/history").await;
    assert_eq!(status, StatusCode::OK);
    assert!(parsed["decisions"].as_array().unwrap().is_empty());
    // candidates and events omitted by default
    assert!(parsed.get("candidates").is_none() || parsed["candidates"].is_null());
    assert!(parsed.get("events").is_none() || parsed["events"].is_null());
}

#[tokio::test]
async fn history_filter_by_decision_id() {
    let state = test_state();
    let app = full_app(state.clone());

    // Create two decisions
    let resp1 = ingest(&app, "Decision Alpha", "First decision", "product").await;
    accept_candidate(&app, resp1["candidate_ids"][0].as_str().unwrap()).await;

    let resp2 = ingest(&app, "Decision Beta", "Second decision", "architecture").await;
    accept_candidate(&app, resp2["candidate_ids"][0].as_str().unwrap()).await;

    let decision_id = state
        .decisions
        .read()
        .await
        .values()
        .find(|d| d.title == "Decision Alpha")
        .unwrap()
        .id
        .to_string();

    let (status, parsed) =
        get_json(app, &format!("/api/v1/history?decision_id={}", decision_id)).await;
    assert_eq!(status, StatusCode::OK);

    let decisions = parsed["decisions"].as_array().unwrap();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0]["title"].as_str().unwrap(), "Decision Alpha");
}

// ── 6. Search contract tests ─────────────────────────────────────

#[tokio::test]
async fn search_response_matches_v02_spec() {
    let state = test_state();
    let app = full_app(state.clone());

    // Ingest a candidate and promote to decision
    let resp = ingest(
        &app,
        "Microservice architecture",
        "Use microservices for horizontal scalability",
        "architecture",
    )
    .await;
    let cand_id = resp["candidate_ids"][0].as_str().unwrap();
    accept_candidate(&app, cand_id).await;

    // Ingest another candidate (not promoted)
    ingest(
        &app,
        "GraphQL adoption",
        "Consider GraphQL for the API gateway microservice layer",
        "product",
    )
    .await;

    let search_body = serde_json::json!({
        "query": "microservice",
        "scope": "all",
        "limit": 10
    });
    let (status, parsed) = post_json(app, "/api/v1/search", &search_body).await;
    assert_eq!(status, StatusCode::OK);

    // Spec §2.2 `bicameral.search` output structure
    assert!(parsed["results"].is_array(), "Missing results");
    let results = parsed["results"].as_array().unwrap();
    assert!(
        !results.is_empty(),
        "Should find results matching 'microservice'"
    );

    for r in results {
        assert!(r["type"].is_string(), "Missing type");
        assert!(r["id"].is_string(), "Missing id");
        assert!(r["title"].is_string(), "Missing title");
        assert!(r["snippet"].is_string(), "Missing snippet");
        assert!(r["relevance_score"].is_f64(), "Missing relevance_score");

        let rtype = r["type"].as_str().unwrap();
        assert!(
            ["decision", "candidate", "source_excerpt"].contains(&rtype),
            "Unexpected result type: {}",
            rtype
        );
    }

    // Should find both the decision and the candidate
    let types: Vec<&str> = results
        .iter()
        .map(|r| r["type"].as_str().unwrap())
        .collect();
    assert!(
        types.contains(&"decision"),
        "Should find the promoted decision"
    );
    assert!(
        types.contains(&"candidate"),
        "Should find the unpromoted candidate"
    );
}

#[tokio::test]
async fn search_scope_filter() {
    let state = test_state();
    let app = full_app(state.clone());

    let resp = ingest(
        &app,
        "Observability pipeline",
        "Implement distributed tracing with OpenTelemetry",
        "architecture",
    )
    .await;
    let cand_id = resp["candidate_ids"][0].as_str().unwrap();
    accept_candidate(&app, cand_id).await;

    // Search only decisions
    let search_body = serde_json::json!({
        "query": "observability",
        "scope": "decisions"
    });
    let (status, parsed) = post_json(app.clone(), "/api/v1/search", &search_body).await;
    assert_eq!(status, StatusCode::OK);

    let results = parsed["results"].as_array().unwrap();
    for r in results {
        assert_eq!(
            r["type"].as_str().unwrap(),
            "decision",
            "Scope=decisions should only return decisions"
        );
    }

    // Search only candidates
    let search_body = serde_json::json!({
        "query": "observability",
        "scope": "candidates"
    });
    let (status, parsed) = post_json(app, "/api/v1/search", &search_body).await;
    assert_eq!(status, StatusCode::OK);

    let results = parsed["results"].as_array().unwrap();
    for r in results {
        let rtype = r["type"].as_str().unwrap();
        assert!(
            rtype == "candidate" || rtype == "source_excerpt",
            "Scope=candidates should not return decisions, got: {}",
            rtype
        );
    }
}

#[tokio::test]
async fn search_empty_for_no_match() {
    let state = test_state();
    let app = full_app(state);

    let body = serde_json::json!({
        "query": "xyzzy_nonexistent_term_12345"
    });
    let (status, parsed) = post_json(app, "/api/v1/search", &body).await;
    assert_eq!(status, StatusCode::OK);
    assert!(parsed["results"].as_array().unwrap().is_empty());
}

// ── 7. End-to-end lifecycle test ─────────────────────────────────

/// Exercises the full MCP v0.2 lifecycle:
/// ingest → accept → bind → review → history → search → preflight
#[tokio::test]
async fn full_mcp_v02_lifecycle() {
    let state = test_state();
    let app = full_app(state.clone());

    // Step 1: Ingest
    let ingest_resp = ingest(
        &app,
        "Authentication middleware",
        "All API routes must pass through the auth middleware before processing",
        "architecture",
    )
    .await;
    let cand_id = ingest_resp["candidate_ids"][0].as_str().unwrap();
    assert!(!cand_id.is_empty());

    // Step 2: Accept → promotes candidate to Decision
    accept_candidate(&app, cand_id).await;

    let decision_id = state
        .decisions
        .read()
        .await
        .keys()
        .next()
        .unwrap()
        .to_string();

    // Step 3: Bind verified proposal to the Decision
    let cand_uuid: uuid::Uuid = cand_id.parse().unwrap();
    let proposal = serde_json::json!({
        "candidate_id": cand_uuid.to_string(),
        "binding_type": "code_region",
        "target": "src/middleware/auth.rs:15-45",
        "confidence": "high",
        "graph_snapshot_id": "snap-lifecycle-001",
        "validated_sha": "a1b2c3d4e5f6",
        "content_hash": "sha256:lifecycle_hash",
    });

    let bind_body = serde_json::json!({
        "decision_id": decision_id,
        "proposal": proposal,
        "evidence_state": "verified",
        "line_start": 15,
        "line_end": 45
    });
    let (status, bind_resp) = post_json(app.clone(), "/api/v1/bind", &bind_body).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(bind_resp["status"].as_str().unwrap(), "bound");

    // Step 4: Review — submit for the candidate
    let review_cmd = serde_json::json!({
        "id": uuid::Uuid::new_v4().to_string(),
        "candidate_id": cand_id,
        "kind": "submit",
        "issued_by": "lifecycle-test",
        "issued_at": chrono::Utc::now().to_rfc3339()
    });
    let (status, _) = post_json(app.clone(), "/api/v1/review", &review_cmd).await;
    assert_eq!(status, StatusCode::OK);

    // Step 5: History — verify decision appears
    let (status, history_resp) = get_json(
        app.clone(),
        "/api/v1/history?include_candidates=true&include_events=true",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let decisions = history_resp["decisions"].as_array().unwrap();
    assert_eq!(decisions.len(), 1);
    assert_eq!(
        decisions[0]["title"].as_str().unwrap(),
        "Authentication middleware"
    );

    // Step 6: Search — find the decision by content
    let search_body = serde_json::json!({
        "query": "auth middleware",
        "scope": "all"
    });
    let (status, search_resp) = post_json(app.clone(), "/api/v1/search", &search_body).await;
    assert_eq!(status, StatusCode::OK);
    let results = search_resp["results"].as_array().unwrap();
    assert!(
        !results.is_empty(),
        "Search should find the auth middleware decision"
    );

    // Step 7: Preflight — verify decision surfaces for relevant files
    let pf_body = serde_json::json!({
        "files": ["src/middleware/auth.rs"]
    });
    let (status, pf_resp) = post_json(app, "/api/v1/preflight", &pf_body).await;
    assert_eq!(status, StatusCode::OK);

    let pf_decisions = pf_resp["relevant_decisions"].as_array().unwrap();
    assert!(
        !pf_decisions.is_empty(),
        "Preflight should surface auth middleware decision for auth.rs"
    );
    assert_eq!(
        pf_decisions[0]["compliance_state"].as_str().unwrap(),
        "ungrounded",
        "Source-only preflight compliance must be ungrounded"
    );

    // Verify no MCP-owned persistence was created during the entire lifecycle
    assert!(
        !std::path::Path::new(".bicameral/mcp").exists(),
        "MCP-owned persistence should not be created"
    );
}

// ── 8. Compliance route integration ──────────────────────────────

#[tokio::test]
async fn compliance_evaluate_after_bind() {
    let state = test_state();
    let app = full_app(state.clone());

    // Ingest + promote
    let resp = ingest(
        &app,
        "Input validation",
        "All user inputs must be validated before processing",
        "architecture",
    )
    .await;
    let cand_id = resp["candidate_ids"][0].as_str().unwrap();
    accept_candidate(&app, cand_id).await;

    let decision_id = state
        .decisions
        .read()
        .await
        .keys()
        .next()
        .unwrap()
        .to_string();

    // Bind evidence
    let cand_uuid: uuid::Uuid = cand_id.parse().unwrap();
    let binding = serde_json::json!({
        "id": uuid::Uuid::new_v4().to_string(),
        "candidate_id": cand_uuid.to_string(),
        "binding_type": "code_region",
        "target": "src/validation.rs:1-100",
        "confidence": "high",
        "bound_at": chrono::Utc::now().to_rfc3339(),
    });

    let bind_body = serde_json::json!({
        "decision_id": decision_id,
        "binding": binding,
        "graph_snapshot_id": "snap-compliance-001",
        "validated_sha": "deadbeef",
        "evidence_state": "verified",
    });
    post_json(app.clone(), "/api/v1/bind", &bind_body).await;

    // Evaluate compliance — gateway has no graph, so should be pending
    let (status, parsed) = get_json(
        app.clone(),
        &format!("/api/v1/compliance/decision/{}", decision_id),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(parsed["compliance_state"].is_string());

    let cs = parsed["compliance_state"].as_str().unwrap();
    assert!(
        cs == "pending" || cs == "ungrounded",
        "Gateway without graph should report pending or ungrounded, got: {}",
        cs
    );
    assert!(parsed["binding_count"].is_u64());
    assert!(parsed["binding_verdicts"].is_array());
}

#[tokio::test]
async fn compliance_ungrounded_without_bindings() {
    let state = test_state();
    let app = full_app(state.clone());

    let resp = ingest(
        &app,
        "Unbound decision",
        "No bindings will be created",
        "product",
    )
    .await;
    let cand_id = resp["candidate_ids"][0].as_str().unwrap();
    accept_candidate(&app, cand_id).await;

    let decision_id = state
        .decisions
        .read()
        .await
        .keys()
        .next()
        .unwrap()
        .to_string();

    let (status, parsed) =
        get_json(app, &format!("/api/v1/compliance/decision/{}", decision_id)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        parsed["compliance_state"].as_str().unwrap(),
        "ungrounded",
        "Decision with no bindings should be ungrounded"
    );
    assert_eq!(parsed["binding_count"].as_u64().unwrap(), 0);
}

// ── MCP/tool evidence as Source + EvidenceReference tests ────────

/// Ingest an MCP session as a Source with evidence references and verify
/// the response includes both span evidence and typed evidence reference IDs.
#[tokio::test]
async fn mcp_session_ingest_creates_source_with_kind() {
    let state = test_state();
    let app = full_app(state.clone());

    let body = serde_json::json!({
        "title": "Cache invalidation policy from MCP session",
        "description": "Agent observed cache TTL must be ≤60s",
        "source": "mcp://session/abc-123",
        "source_type": "mcp_session",
        "source_kind": "mcp_session",
        "label": "MCP session abc-123",
        "snapshot_content": "Turn 3: User said cache TTL must be 60s or less.\nTurn 5: tool_call list_caches returned [{\"name\":\"api\",\"ttl\":300}]",
        "evidence": [{
            "excerpt": "cache TTL must be 60s or less",
            "span_start": 18,
            "span_end": 47,
            "confidence": 0.85
        }],
        "evidence_references": [
            {
                "source_kind": "mcp_session",
                "excerpt": "User said cache TTL must be 60s or less",
                "pointer": "turn:3",
                "metadata": { "turn_index": 3, "role": "user" }
            },
            {
                "source_kind": "mcp_session",
                "excerpt": "tool_call list_caches returned [{\"name\":\"api\",\"ttl\":300}]",
                "pointer": "tool_call:list_caches",
                "metadata": { "turn_index": 5, "tool_name": "list_caches" }
            }
        ]
    });

    let (status, parsed) = post_json(app.clone(), "/api/v1/ingest", &body).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "MCP ingest failed: {:?}",
        parsed
    );

    // Response includes both span-based evidence and typed evidence references
    assert!(!parsed["evidence_ids"].as_array().unwrap().is_empty());
    let ref_ids = parsed["evidence_reference_ids"].as_array().unwrap();
    assert_eq!(ref_ids.len(), 2, "Expected 2 evidence reference IDs");

    // Source in state carries source_kind
    let sources = state.sources.read().await;
    let source = sources.get("mcp://session/abc-123").unwrap();
    assert_eq!(source.source_type, "mcp_session");
    assert!(source.source_kind.is_some());

    // Evidence references in state carry correct source_kind and pointer
    let ev_refs = state.evidence_references.read().await;
    assert_eq!(ev_refs.len(), 2);
    for ev_ref in ev_refs.values() {
        assert_eq!(
            ev_ref.source_kind,
            bicameral_api::source::SourceKind::McpSession
        );
        assert!(ev_ref.pointer.is_some());
    }
}

/// Ingest a command run as a Source and verify evidence references point at
/// command output.
#[tokio::test]
async fn command_run_ingest_creates_source_with_kind() {
    let state = test_state();
    let app = full_app(state.clone());

    let body = serde_json::json!({
        "title": "Deploy requires staging approval",
        "description": "deploy.sh output shows staging gate is mandatory",
        "source": "cmd://deploy-check/run-42",
        "source_type": "command_run",
        "source_kind": "command_run",
        "label": "deploy.sh --check",
        "snapshot_content": "$ deploy.sh --check\nERROR: staging approval required before production deploy\nexit code: 1",
        "evidence": [{
            "excerpt": "staging approval required before production deploy",
            "confidence": 0.95
        }],
        "evidence_references": [{
            "source_kind": "command_run",
            "excerpt": "ERROR: staging approval required before production deploy",
            "pointer": "stderr:line:2",
            "metadata": { "exit_code": 1, "command": "deploy.sh --check" }
        }]
    });

    let (status, parsed) = post_json(app, "/api/v1/ingest", &body).await;
    assert_eq!(status, StatusCode::CREATED);

    let ref_ids = parsed["evidence_reference_ids"].as_array().unwrap();
    assert_eq!(ref_ids.len(), 1);

    let ev_refs = state.evidence_references.read().await;
    let ev_ref = ev_refs.values().next().unwrap();
    assert_eq!(
        ev_ref.source_kind,
        bicameral_api::source::SourceKind::CommandRun
    );
    assert_eq!(ev_ref.pointer.as_deref(), Some("stderr:line:2"));
    assert_eq!(ev_ref.metadata.as_ref().unwrap()["exit_code"], 1);
}

/// MCP evidence enters as advisory candidate context — not direct Decision
/// authority. Ingesting MCP evidence produces a DecisionCandidate (proposed),
/// not a Decision.
#[tokio::test]
async fn mcp_evidence_enters_as_advisory_candidate_not_decision() {
    let state = test_state();
    let app = full_app(state.clone());

    let body = serde_json::json!({
        "title": "API rate limit from MCP session",
        "description": "Agent session suggests 100 req/s limit",
        "source": "mcp://session/xyz-456",
        "source_type": "mcp_session",
        "source_kind": "mcp_session",
        "snapshot_content": "Turn 1: discussing rate limits",
        "evidence": [{ "excerpt": "100 req/s limit", "confidence": 0.7 }],
        "evidence_references": [{
            "source_kind": "mcp_session",
            "excerpt": "100 req/s limit",
            "pointer": "turn:1"
        }]
    });

    let (status, _) = post_json(app, "/api/v1/ingest", &body).await;
    assert_eq!(status, StatusCode::CREATED);

    // Candidate exists (advisory context)
    let candidates = state.candidates.read().await;
    assert_eq!(candidates.len(), 1);
    let cand = candidates.values().next().unwrap();
    assert_eq!(
        cand.candidate_state,
        bicameral_api::candidate::CandidateState::Proposed
    );

    // No Decision exists — MCP evidence is advisory, not direct authority
    let decisions = state.decisions.read().await;
    assert!(
        decisions.is_empty(),
        "MCP evidence must not directly create Decisions"
    );
}

/// Promoted Decisions preserve the evidence chain back to the MCP/tool
/// SourceSnapshot through evidence_reference_ids.
#[tokio::test]
async fn promoted_decision_preserves_mcp_evidence_chain() {
    let state = test_state();
    let app = full_app(state.clone());

    // 1. Ingest MCP evidence
    let body = serde_json::json!({
        "title": "Auth tokens must expire",
        "description": "MCP session evidence: tokens expire after 24h",
        "source": "mcp://session/auth-review",
        "source_type": "mcp_session",
        "source_kind": "mcp_session",
        "snapshot_content": "Turn 2: Auth tokens must expire within 24 hours",
        "evidence": [{ "excerpt": "tokens expire after 24h", "confidence": 0.9 }],
        "evidence_references": [
            {
                "source_kind": "mcp_session",
                "excerpt": "Auth tokens must expire within 24 hours",
                "pointer": "turn:2",
                "metadata": { "turn_index": 2 }
            }
        ]
    });

    let (status, resp) = post_json(app.clone(), "/api/v1/ingest", &body).await;
    assert_eq!(status, StatusCode::CREATED);

    let cand_id = resp["candidate_ids"][0].as_str().unwrap();
    let ref_ids: Vec<String> = resp["evidence_reference_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(ref_ids.len(), 1);

    // 2. Promote candidate → Decision
    accept_candidate(&app, cand_id).await;

    // 3. Verify Decision preserves evidence_reference_ids
    let decisions = state.decisions.read().await;
    assert_eq!(decisions.len(), 1);
    let decision = decisions.values().next().unwrap();

    assert!(
        !decision.evidence_refs.is_empty(),
        "Decision should preserve span-evidence refs"
    );
    assert!(
        !decision.evidence_reference_ids.is_empty(),
        "Decision should preserve typed evidence reference IDs"
    );

    // Evidence references in state are still accessible by the IDs
    let ev_refs = state.evidence_references.read().await;
    for ref_id in &decision.evidence_reference_ids {
        let ev_ref = ev_refs
            .get(ref_id)
            .expect("Evidence reference must exist in state");
        assert_eq!(
            ev_ref.source_kind,
            bicameral_api::source::SourceKind::McpSession
        );
    }

    // Source snapshot is preserved
    let snapshots = state.snapshots.read().await;
    assert!(!snapshots.is_empty(), "SourceSnapshot must be preserved");
}

/// Backward compatibility: ingest without source_kind or evidence_references
/// still works (all new fields are optional).
#[tokio::test]
async fn ingest_without_source_kind_is_backward_compatible() {
    let state = test_state();
    let app = full_app(state.clone());

    let body = serde_json::json!({
        "title": "Legacy ingest",
        "description": "No source_kind or evidence_references",
        "source": "https://example.com/legacy",
        "evidence": [{ "excerpt": "legacy evidence", "confidence": 0.8 }]
    });

    let (status, parsed) = post_json(app, "/api/v1/ingest", &body).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "Legacy ingest failed: {:?}",
        parsed
    );

    // Source in state has no source_kind
    let sources = state.sources.read().await;
    let source = sources.get("https://example.com/legacy").unwrap();
    assert!(source.source_kind.is_none());

    // No evidence references created
    let ev_refs = state.evidence_references.read().await;
    assert!(ev_refs.is_empty());

    // Candidate still has empty evidence_reference_ids
    let candidates = state.candidates.read().await;
    let cand = candidates.values().next().unwrap();
    assert!(cand.evidence_reference_ids.is_empty());
}

// ── Alpha cut-line: deferred commands across surfaces ────────────
//
// Issue #290 / #382: verify that deferred commands return typed JSON with
// HTTP 202, not silent acceptance, crashes, or empty bodies.

#[tokio::test]
async fn alpha_cutline_dashboard_deferred_commands_return_typed_json() {
    let deferred_commands = ["resolve_compliance", "untrack_decision", "refresh_source"];

    for cmd_name in &deferred_commands {
        let cmd = serde_json::json!({
            "target_id": "00000000-0000-4000-8000-000000000001",
            "command": cmd_name
        });
        let app_clone = full_app(test_state());
        let (status, body) = post_json(app_clone, "/api/v1/dashboard/command", &cmd).await;
        assert_eq!(
            status,
            StatusCode::ACCEPTED,
            "dashboard command '{cmd_name}' must return 202 ACCEPTED with typed deferred JSON"
        );
        assert_eq!(
            body["status"], "deferred",
            "{cmd_name}: status must be 'deferred'"
        );
        assert_eq!(
            body["reason"], "not_supported_in_alpha",
            "{cmd_name}: reason must be 'not_supported_in_alpha'"
        );
        assert_eq!(
            body["command"], *cmd_name,
            "{cmd_name}: command field must echo the command name"
        );
    }
}
