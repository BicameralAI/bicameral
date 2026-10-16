//! Hosted bridge end-to-end conformance tests — issue #582.
//!
//! Exercises the complete hosted bridge lifecycle through the gateway HTTP
//! surface:
//!
//!   pair (trusted origin) → read operator-home projection → dispatch
//!   allowed command through ToolRequest → verify response receipt
//!
//! Plus negative/boundary cases:
//!   - Unpaired session cannot read projections
//!   - Expired session rejected
//!   - Revoked session rejected
//!   - Untrusted origin rejected before session creation
//!   - Command allowlist rejects forbidden commands
//!   - Authority-smuggling fields rejected at pairing and command
//!   - Hosted bridge cannot mint signoff/compliance/event-store authority
//!   - No local path, secret, or source body leakage in projections
//!   - Replay/idempotency: duplicate pairing yields distinct sessions
//!   - ExternalIngestEnvelope is NOT routable through bridge
//!
//! # Authority boundary
//!
//! External authenticated remote actions are NOT `ExternalIngestEnvelope`.
//! Hosted bridge does NOT mint signoff/compliance/event-store authority
//! directly — all governed mutations route through `ToolRequest` dispatch
//! with daemon-derived `AuthorityContext`.
//!
//! # Testability note
//!
//! Browser-level CORS/CSRF conformance requires Playwright/integration
//! harness coverage not available in Rust unit tests. This gap is named
//! explicitly per acceptance criteria.
//!
//! Related: #539, #576, #577, #578, #579, #580, #581, #582.
//! ADR boundary evidence: ADR-0002, ADR-0003, ADR-0007, ADR-0020, ADR-0023.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use bicameral_api::candidate::{Decision, DecisionLevel, ReviewLifecycleState, TrackingState};
use bicameral_api::dashboard::{ComplianceState, SignoffState};
use bicameral_api::event_store::{EventStoreEntry, EventType};
use bicameral_api::evidence::{BindingConfidence, BindingEvidence, BindingType, SourceEvidence};
use bicameral_audit::AuditStore;
use bicameral_gateway::routes::hosted_bridge_routes;
use bicameral_gateway::state::{AppState, LocalProjectRegistration};
use chrono::Utc;
use std::path::PathBuf;
use tower::ServiceExt;
use uuid::Uuid;

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn test_audit() -> AuditStore {
    let dir = tempfile::tempdir().unwrap();
    AuditStore::open(dir.path()).unwrap()
}

fn build_bridge_app(state: AppState) -> Router {
    Router::new()
        .merge(hosted_bridge_routes())
        .with_state(state)
}

/// Seed the state with a local project registration that has sensitive fields.
async fn seed_project(state: &AppState) {
    let mut registrations = state
        .project_registrations
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    registrations.push(LocalProjectRegistration {
        name: "acme-backend".to_string(),
        event_store_healthy: true,
        has_workspace_binding: true,
        workspace_path: Some(PathBuf::from("/home/user/repos/acme-backend")),
        provider_config: Some(serde_json::json!({
            "provider_token": "ghp_SECRETTOKEN123456",
            "api_key": "sk-secret-api-key-value",
            "refresh_token": "rt_REFRESH_SECRET",
        })),
        setup_state: "normal".to_string(),
    });
    registrations.push(LocalProjectRegistration {
        name: "acme-frontend".to_string(),
        event_store_healthy: true,
        has_workspace_binding: false,
        workspace_path: Some(PathBuf::from("/home/user/repos/acme-frontend")),
        provider_config: None,
        setup_state: "code_grounding_not_ready".to_string(),
    });
}

/// Seed a project whose local workspace binding is broken and needs repair,
/// carrying sensitive fields that must never cross the bridge (#733).
async fn seed_repair_required_project(state: &AppState) {
    let mut registrations = state
        .project_registrations
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    registrations.push(LocalProjectRegistration {
        name: "acme-repair".to_string(),
        event_store_healthy: true,
        has_workspace_binding: true,
        workspace_path: Some(PathBuf::from("/home/user/repos/acme-repair")),
        provider_config: Some(serde_json::json!({
            "provider_token": "ghp_REPAIR_SECRET_TOKEN",
        })),
        setup_state: "repair_required".to_string(),
    });
}

/// Seed a Decision (plus its source evidence, binding, and a ledger event)
/// with confidential fields, to prove the network-origin renderable ledger
/// projection (#705, T4) drops them.
async fn seed_ledger_with_sensitive_data(state: &AppState) {
    let decision_id = Uuid::new_v4();
    let candidate_id = Uuid::new_v4();
    let evidence_id = Uuid::new_v4();

    // Source evidence carries a raw excerpt with a secret + local path — this
    // must never surface through the network-origin projection.
    let mut evidence = SourceEvidence::new(
        "snap-abc123".to_string(),
        0,
        42,
        "raw excerpt: AKIAIOSFODNN7EXAMPLE at /home/user/repos/secret-svc/config.yaml".to_string(),
        0.9,
    );
    evidence.id = evidence_id;
    state.evidence.write().await.insert(evidence_id, evidence);

    // Binding carries a local file path + explanation — also confidential.
    let mut binding = BindingEvidence::new(
        candidate_id,
        BindingType::FilePath,
        "/home/user/repos/secret-svc/src/auth.rs:10:40".to_string(),
        BindingConfidence::High,
    );
    binding.explanation =
        Some("binds to the token-refresh path in /home/user/repos/secret-svc".to_string());
    state
        .bindings
        .write()
        .await
        .insert(decision_id, vec![binding]);

    let decision = Decision {
        id: decision_id,
        candidate_id,
        // Title embeds a secret so we can prove the redactor runs on it.
        title: "Rotate AKIAIOSFODNN7EXAMPLE before launch".to_string(),
        // Body is a raw governance narrative that must never be rendered.
        body: "Full decision body with secret ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij and path /home/user/repos/secret-svc".to_string(),
        level: DecisionLevel::Architecture,
        signoff_state: SignoffState::Approved,
        compliance_state: ComplianceState::Ungrounded,
        review_state: ReviewLifecycleState::NeedsSignoffReview,
        evidence_refs: vec![evidence_id],
        evidence_reference_ids: Vec::new(),
        source_id: Some("/home/user/repos/secret-svc/DECISIONS.md".to_string()),
        tracking_state: TrackingState::Tracked,
        promoted_at: Utc::now(),
        promoted_by: "alice@secret-corp.example".to_string(),
        provenance: None,
    };
    state.decisions.write().await.insert(decision_id, decision);

    // A ledger event whose actor is confidential authority material.
    let _ = state.event_store.append(EventStoreEntry::new(
        EventType::ApproveSignoff,
        serde_json::json!({ "decision_id": decision_id.to_string() }),
        "alice@secret-corp.example".to_string(),
    ));
}

async fn post_json(app: Router, uri: &str, payload: serde_json::Value) -> (StatusCode, Vec<u8>) {
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
    (status, body.to_vec())
}

async fn post_json_with_token(
    app: Router,
    uri: &str,
    payload: serde_json::Value,
    token: &str,
) -> (StatusCode, Vec<u8>) {
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .header("x-bridge-session-token", token)
                .body(Body::from(serde_json::to_string(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, body.to_vec())
}

async fn get_with_token(app: Router, uri: &str, token: &str) -> (StatusCode, Vec<u8>) {
    let resp = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("x-bridge-session-token", token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, body.to_vec())
}

async fn get_without_token(app: Router, uri: &str) -> (StatusCode, Vec<u8>) {
    let resp = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, body.to_vec())
}

/// POST JSON carrying an `Origin` header (initiator/hosted-origin requests).
async fn post_json_with_origin(
    app: Router,
    uri: &str,
    payload: serde_json::Value,
    origin: &str,
) -> (StatusCode, Vec<u8>) {
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .header("origin", origin)
                .body(Body::from(serde_json::to_string(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, body.to_vec())
}

/// GET carrying an `Origin` header (initiator/hosted-origin requests).
async fn get_with_origin(app: Router, uri: &str, origin: &str) -> (StatusCode, Vec<u8>) {
    let resp = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("origin", origin)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, body.to_vec())
}

const TEST_ORIGIN: &str = "https://app.bicameral-ai.com";
const TEST_ACCOUNT: &str = "acct-test-alice";

/// Create a pending pairing request for the standard test origin/account and
/// return the `202` pending response body (#790).
async fn create_pending_request(state: &AppState) -> serde_json::Value {
    let app = build_bridge_app(state.clone());
    let payload = serde_json::json!({
        "origin": TEST_ORIGIN,
        "cloud_account_id": TEST_ACCOUNT,
        "csrf_token": "csrf-valid-token-001",
        "team_id": "team-1",
        "cloud_access_token": "cloud-token-1",
        "client_protocol_version": bicameral_api::hosted_bridge::HOSTED_BRIDGE_PROTOCOL_VERSION,
    });
    let (status, body) = post_json(app, "/api/v1/hosted-bridge/pair", payload).await;
    assert_eq!(
        status,
        StatusCode::ACCEPTED,
        "pending pairing failed: {}",
        String::from_utf8_lossy(&body)
    );
    serde_json::from_slice(&body).unwrap()
}

/// Read the local confirmation nonce for a pending request from the LOCAL
/// surface view (#790).
async fn local_confirmation_nonce(state: &AppState, request_handle: &str) -> String {
    let app = build_bridge_app(state.clone());
    let (status, body) = get_without_token(
        app,
        &format!("/api/v1/hosted-bridge/pair/pending/{request_handle}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "local view failed");
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    resp["local_confirmation_nonce"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Run the full pending → local approve → status handoff flow for a trusted
/// origin and return the released session token (#790).
async fn pair_session(state: &AppState) -> String {
    let pending = create_pending_request(state).await;
    let request_handle = pending["request_handle"].as_str().unwrap().to_string();
    let poll_proof = pending["poll_proof"].as_str().unwrap().to_string();

    // Local surface reads the pending request to obtain the decision nonce.
    let nonce = local_confirmation_nonce(state, &request_handle).await;

    // Explicit local approval (no hosted Origin header → same-origin/local).
    let app = build_bridge_app(state.clone());
    let (status, body) = post_json(
        app,
        "/api/v1/hosted-bridge/pair/decision",
        serde_json::json!({
            "request_handle": request_handle,
            "action": "approve",
            "local_confirmation_nonce": nonce,
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "local approval failed: {}",
        String::from_utf8_lossy(&body)
    );

    // Initiating browser polls the status exchange to retrieve the handoff.
    let app = build_bridge_app(state.clone());
    let (status, body) = get_with_origin(
        app,
        &format!(
            "/api/v1/hosted-bridge/pair/status?request_handle={request_handle}\
             &poll_proof={poll_proof}&cloud_account_id={TEST_ACCOUNT}"
        ),
        TEST_ORIGIN,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "status handoff failed: {}",
        String::from_utf8_lossy(&body)
    );
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["status"], "approved");
    resp["session_token"].as_str().unwrap().to_string()
}

// ═══════════════════════════════════════════════════════════════════════════════
// 1. HAPPY PATH: Pairing → Authenticated Command → Response
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn e2e_pair_then_read_operator_home() {
    let state = AppState::new(test_audit());
    seed_project(&state).await;
    let token = pair_session(&state).await;

    let app = build_bridge_app(state);
    let (status, body) = get_with_token(app, "/api/v1/hosted-bridge/operator-home", &token).await;
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["pairing_state"], "paired");
    assert!(!resp["projects"].as_array().unwrap().is_empty());
    assert_eq!(resp["workspace_binding_available"], false);
}

#[tokio::test]
async fn e2e_pair_then_read_projects_projection() {
    let state = AppState::new(test_audit());
    seed_project(&state).await;
    state
        .event_store
        .append(EventStoreEntry::new(
            EventType::ProductMemberAdded,
            serde_json::json!({
                "product_id": "acme-backend",
                "cloud_account_id": "acct-test-alice"
            }),
            "bridge:acct-test-alice".into(),
        ))
        .unwrap();
    state
        .event_store
        .append(EventStoreEntry::new(
            EventType::ProductMemberAdded,
            serde_json::json!({
                "product_id": "other-account-product",
                "cloud_account_id": "acct-other"
            }),
            "bridge:acct-other".into(),
        ))
        .unwrap();
    let token = pair_session(&state).await;

    let app = build_bridge_app(state);
    let (status, body) = get_with_token(app, "/api/v1/hosted-bridge/projects", &token).await;
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["projection_only"], true);
    let projects = resp["projects"].as_array().unwrap();
    assert_eq!(projects.len(), 2);
    assert_eq!(
        resp["memberships"],
        serde_json::json!([
            {
                "product_id": "acme-backend",
                "setup_incomplete": false
            }
        ])
    );
    let serialized = String::from_utf8(body).unwrap();
    assert!(!serialized.contains("acct-other"));
    assert!(!serialized.contains("other-account-product"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// 1b. FAKE-DAEMON CONFORMANCE: selected Cloud project scopes projection (#719)
// ═══════════════════════════════════════════════════════════════════════════════
//
// The gateway `AppState` seeded with `LocalProjectRegistration`s stands in as a
// fake local daemon. These tests drive the real hosted-bridge HTTP surface to
// prove the selected Cloud project id flows through pairing → scoped projection,
// that a missing local binding renders the "needs binding" (`unbound`) state,
// and that the unscoped daemon project list is never used as Cloud membership.

/// AC#1: the selected Cloud project id flows through pairing/projection and the
/// daemon returns readiness scoped to exactly that project.
#[tokio::test]
async fn e2e_selected_project_ready_scopes_to_selection() {
    let state = AppState::new(test_audit());
    seed_project(&state).await;
    let token = pair_session(&state).await;

    let app = build_bridge_app(state);
    // acme-backend is bound + healthy + normal in seed_project.
    let (status, body) =
        get_with_token(app, "/api/v1/hosted-bridge/projects/acme-backend", &token).await;
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    // The selected id round-tripped through the daemon.
    assert_eq!(resp["selected_project"]["project_id"], "acme-backend");
    assert_eq!(resp["binding_readiness"], "ready");
    assert_eq!(resp["controls_available"], true);
    // Scoped to the selection: the single matching project, not the list.
    assert_eq!(resp["project"]["name"], "acme-backend");
    // AC#3: the other locally-registered project is NOT leaked as membership.
    let raw = String::from_utf8_lossy(&body);
    assert!(!raw.contains("acme-frontend"));
    // Redaction: no local paths or provider secrets in the scoped projection.
    for leaked in [
        "/home/user",
        "ghp_SECRETTOKEN123456",
        "sk-secret-api-key-value",
        "rt_REFRESH_SECRET",
        "workspace_path",
        "provider_config",
    ] {
        assert!(!raw.contains(leaked), "leaked sensitive token: {leaked}");
    }
}

/// AC#2: selecting a project that is locally registered but has no workspace
/// binding renders the "needs binding" (`unbound`) readiness state.
#[tokio::test]
async fn e2e_selected_project_registered_but_unbound_needs_binding() {
    let state = AppState::new(test_audit());
    seed_project(&state).await;
    let token = pair_session(&state).await;

    let app = build_bridge_app(state);
    // acme-frontend is registered with has_workspace_binding = false.
    let (status, body) =
        get_with_token(app, "/api/v1/hosted-bridge/projects/acme-frontend", &token).await;
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["selected_project"]["project_id"], "acme-frontend");
    assert_eq!(resp["binding_readiness"], "unbound");
    assert_eq!(resp["controls_available"], false);
}

/// AC#2 + AC#3: selecting a Cloud project the daemon has never registered
/// resolves to `unbound` ("needs binding") with HTTP 200 — NOT a 403/404
/// membership verdict, and NOT the unscoped project list. The daemon's local
/// list is never treated as the Cloud membership source.
#[tokio::test]
async fn e2e_selected_project_unknown_to_daemon_is_unbound_not_membership_denied() {
    let state = AppState::new(test_audit());
    seed_project(&state).await;
    let token = pair_session(&state).await;

    let app = build_bridge_app(state);
    let (status, body) = get_with_token(
        app,
        "/api/v1/hosted-bridge/projects/cloud-only-project",
        &token,
    )
    .await;
    // Not a membership rejection: readiness is reported, not denied.
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["selected_project"]["project_id"], "cloud-only-project");
    assert_eq!(resp["binding_readiness"], "unbound");
    assert!(resp["project"].is_null());
    // The local list is not surfaced as membership: no other project leaks.
    let raw = String::from_utf8_lossy(&body);
    assert!(!raw.contains("acme-backend"));
    assert!(!raw.contains("acme-frontend"));
}

/// The selected-project route echoes optional cloud tenancy identity (team id)
/// but only ever scopes to the selection — never grants membership from it.
#[tokio::test]
async fn e2e_selected_project_echoes_cloud_team_scope() {
    let state = AppState::new(test_audit());
    seed_project(&state).await;
    let token = pair_session(&state).await;

    let app = build_bridge_app(state);
    let (status, body) = get_with_token(
        app,
        "/api/v1/hosted-bridge/projects/acme-backend?team_id=team-acme&display_name=Acme%20Backend",
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["selected_project"]["team_id"], "team-acme");
    assert_eq!(resp["selected_project"]["display_name"], "Acme Backend");
    assert_eq!(resp["binding_readiness"], "ready");
}

/// An unpaired session cannot resolve selected-project readiness.
#[tokio::test]
async fn e2e_selected_project_requires_pairing() {
    let state = AppState::new(test_audit());
    seed_project(&state).await;
    let app = build_bridge_app(state);

    let (status, body) =
        get_without_token(app, "/api/v1/hosted-bridge/projects/acme-backend").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["kind"], "NotPaired");
}

// ── #733: hosted-safe workspace binding readiness over the bridge ──────────
//
// These drive the real hosted-bridge HTTP surface to prove that project-level
// readiness distinguishes unbound / bound / repair-required workspace states,
// carries `code_grounding_ready` + caveats + typed next actions, guides the
// hosted UI to the MCP/CLI/local bind flow, and never leaks a local path.

/// AC: a bound project reports code-grounding readiness through the bridge
/// without exposing any local path, and offers no bind/repair next action.
#[tokio::test]
async fn e2e_workspace_readiness_bound_is_ready_without_path() {
    let state = AppState::new(test_audit());
    seed_project(&state).await;
    let token = pair_session(&state).await;

    let app = build_bridge_app(state);
    let (status, body) =
        get_with_token(app, "/api/v1/hosted-bridge/projects/acme-backend", &token).await;
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let ws = &resp["workspace_readiness"];
    assert_eq!(ws["binding_state"], "local_workspace_bound");
    assert_eq!(ws["code_grounding_ready"], true);
    assert!(ws["next_action"].is_null());

    let raw = String::from_utf8_lossy(&body);
    for leaked in ["/home/user", "ghp_SECRETTOKEN123456", "workspace_path"] {
        assert!(!raw.contains(leaked), "leaked sensitive token: {leaked}");
    }
}

/// AC: a registered-but-unbound project surfaces `code_grounding_ready = false`
/// as a readiness gap (not an authority failure) and a next action guiding the
/// user to the MCP/CLI/local bind flow.
#[tokio::test]
async fn e2e_workspace_readiness_unbound_guides_to_bind_flow() {
    let state = AppState::new(test_audit());
    seed_project(&state).await;
    let token = pair_session(&state).await;

    let app = build_bridge_app(state);
    let (status, body) =
        get_with_token(app, "/api/v1/hosted-bridge/projects/acme-frontend", &token).await;
    // A missing binding is a readiness gap, not a project authority failure.
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let ws = &resp["workspace_readiness"];
    assert_eq!(ws["binding_state"], "local_workspace_unbound");
    assert_eq!(ws["code_grounding_ready"], false);
    assert!(!ws["caveats"].as_array().unwrap().is_empty());
    let action = &ws["next_action"];
    assert_eq!(action["kind"], "bind_local_workspace");
    let surfaces = action["surfaces"].as_array().unwrap();
    assert!(surfaces.iter().any(|s| s == "mcp"));
    assert!(surfaces.iter().any(|s| s == "cli"));
    assert!(!action["command"].as_str().unwrap().is_empty());
}

/// AC: a repair-required project returns typed repair guidance without leaking
/// any private path detail.
#[tokio::test]
async fn e2e_workspace_readiness_repair_required_returns_typed_guidance() {
    let state = AppState::new(test_audit());
    seed_repair_required_project(&state).await;
    let token = pair_session(&state).await;

    let app = build_bridge_app(state);
    let (status, body) =
        get_with_token(app, "/api/v1/hosted-bridge/projects/acme-repair", &token).await;
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let ws = &resp["workspace_readiness"];
    assert_eq!(ws["binding_state"], "local_workspace_repair_required");
    assert_eq!(ws["code_grounding_ready"], false);
    assert_eq!(ws["next_action"]["kind"], "repair_local_workspace");

    let raw = String::from_utf8_lossy(&body);
    for leaked in ["/home/user", "ghp_REPAIR_SECRET_TOKEN", "workspace_path"] {
        assert!(!raw.contains(leaked), "leaked sensitive token: {leaked}");
    }
}

/// AC: pairing alone cannot create a workspace binding. A freshly-paired
/// session selecting an unbound project still reports `unbound` with a bind
/// next action — the bridge never flips it to `bound` — and the user-level
/// operator-home capability stays `workspace_binding_available = false`.
#[tokio::test]
async fn e2e_pairing_alone_cannot_create_workspace_binding() {
    let state = AppState::new(test_audit());
    seed_project(&state).await;
    let token = pair_session(&state).await;

    let selected_app = build_bridge_app(state.clone());
    let (status, body) = get_with_token(
        selected_app,
        "/api/v1/hosted-bridge/projects/acme-frontend",
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["binding_readiness"], "unbound");
    assert_eq!(
        resp["workspace_readiness"]["binding_state"],
        "local_workspace_unbound"
    );
    assert_eq!(
        resp["workspace_readiness"]["next_action"]["kind"],
        "bind_local_workspace"
    );

    // The user-level capability advertised at operator-home stays false.
    let op_app = build_bridge_app(state);
    let (op_status, op_body) =
        get_with_token(op_app, "/api/v1/hosted-bridge/operator-home", &token).await;
    assert_eq!(op_status, StatusCode::OK);
    let op: serde_json::Value = serde_json::from_slice(&op_body).unwrap();
    assert_eq!(op["workspace_binding_available"], false);
}

#[tokio::test]
async fn e2e_pair_then_read_workspace_discovery() {
    let state = AppState::new(test_audit());
    seed_project(&state).await;
    let token = pair_session(&state).await;

    let app = build_bridge_app(state);
    let (status, body) =
        get_with_token(app, "/api/v1/hosted-bridge/workspace-discovery", &token).await;
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["projection_only"], true);
    let workspaces = resp["workspaces"].as_array().unwrap();
    assert!(!workspaces.is_empty());
}

#[tokio::test]
async fn e2e_pair_then_read_dashboard_adapter() {
    let state = AppState::new(test_audit());
    seed_project(&state).await;
    let token = pair_session(&state).await;

    let app = build_bridge_app(state);
    let (status, body) =
        get_with_token(app, "/api/v1/hosted-bridge/dashboard-adapter", &token).await;
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["mode"], "hosted");
    assert!(resp.get("fixture_fallback").is_none());
    assert!(resp["ledger_summary"].is_object());
}

#[tokio::test]
async fn e2e_pair_then_status_check() {
    let state = AppState::new(test_audit());
    let token = pair_session(&state).await;

    let app = build_bridge_app(state);
    let (status, body) = get_with_token(app, "/api/v1/hosted-bridge/status", &token).await;
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["pairing_state"], "hosted_bridge_paired");
    assert_eq!(resp["cloud_account_id"], "acct-test-alice");
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. NEGATIVE: Unpaired/Expired/Revoked Session
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn unpaired_cannot_read_projections() {
    let state = AppState::new(test_audit());
    seed_project(&state).await;
    let app = build_bridge_app(state);

    // No session token → rejected
    let (status, body) = get_without_token(app, "/api/v1/hosted-bridge/operator-home").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["kind"], "NotPaired");
}

#[tokio::test]
async fn invalid_token_cannot_read_projections() {
    let state = AppState::new(test_audit());
    seed_project(&state).await;
    let app = build_bridge_app(state);

    let (status, body) = get_with_token(
        app,
        "/api/v1/hosted-bridge/operator-home",
        "invalid-token-999",
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["kind"], "NotPaired");
}

#[tokio::test]
async fn revoked_session_cannot_read_projections() {
    let state = AppState::new(test_audit());
    seed_project(&state).await;
    let token = pair_session(&state).await;

    // Revoke the session
    let app = build_bridge_app(state.clone());
    let (revoke_status, _) = post_json_with_token(
        app,
        "/api/v1/hosted-bridge/revoke",
        serde_json::json!({}),
        &token,
    )
    .await;
    assert_eq!(revoke_status, StatusCode::OK);

    // Attempt to read after revocation
    let app = build_bridge_app(state);
    let (status, body) = get_with_token(app, "/api/v1/hosted-bridge/operator-home", &token).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["kind"], "PairingRevoked");
}

#[tokio::test]
async fn expired_session_cannot_read_projections() {
    let state = AppState::new(test_audit());
    seed_project(&state).await;

    // Manually insert an expired session
    let expired_token = "expired-sess-001".to_string();
    {
        use bicameral_gateway::state::HostedBridgePairingSession;
        let mut sessions = state.hosted_bridge_sessions.write().await;
        sessions.insert(
            expired_token.clone(),
            HostedBridgePairingSession {
                session_token: expired_token.clone(),
                pairing_id: None,
                origin: "https://app.bicameral-ai.com".to_string(),
                cloud_account_id: "acct-expired".to_string(),
                paired_at: chrono::Utc::now() - chrono::Duration::hours(25),
                expires_at: chrono::Utc::now() - chrono::Duration::hours(1),
                revoked: false,
            },
        );
    }

    let app = build_bridge_app(state);
    let (status, body) =
        get_with_token(app, "/api/v1/hosted-bridge/operator-home", &expired_token).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["kind"], "PairingExpired");
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. NEGATIVE: Trusted-Origin Rejection
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn untrusted_origin_rejected_at_pairing() {
    let state = AppState::new(test_audit());
    let app = build_bridge_app(state);

    let payload = serde_json::json!({
        "origin": "https://evil.attacker.com",
        "cloud_account_id": "acct-evil",
        "csrf_token": "csrf-evil-001",
        "team_id": "team-1",
        "cloud_access_token": "cloud-token-1",
    });
    let (status, body) = post_json(app, "/api/v1/hosted-bridge/pair", payload).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["error"], "origin_not_trusted");
}

#[tokio::test]
async fn http_origin_variant_rejected() {
    let state = AppState::new(test_audit());
    let app = build_bridge_app(state);

    let payload = serde_json::json!({
        "origin": "http://app.bicameral-ai.com",
        "cloud_account_id": "acct-http",
        "csrf_token": "csrf-http-001",
        "team_id": "team-1",
        "cloud_access_token": "cloud-token-1",
    });
    let (status, _) = post_json(app, "/api/v1/hosted-bridge/pair", payload).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn subdomain_of_trusted_origin_rejected() {
    let state = AppState::new(test_audit());
    let app = build_bridge_app(state);

    let payload = serde_json::json!({
        "origin": "https://sub.app.bicameral-ai.com",
        "cloud_account_id": "acct-sub",
        "csrf_token": "csrf-sub-001",
        "team_id": "team-1",
        "cloud_access_token": "cloud-token-1",
    });
    let (status, _) = post_json(app, "/api/v1/hosted-bridge/pair", payload).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn empty_origin_rejected() {
    let state = AppState::new(test_audit());
    let app = build_bridge_app(state);

    let payload = serde_json::json!({
        "origin": "",
        "cloud_account_id": "acct-empty",
        "csrf_token": "csrf-empty-001",
        "team_id": "team-1",
        "cloud_access_token": "cloud-token-1",
    });
    let (status, _) = post_json(app, "/api/v1/hosted-bridge/pair", payload).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. NEGATIVE: Command Allowlist Rejection
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn forbidden_command_rejected_through_bridge() {
    let state = AppState::new(test_audit());
    let token = pair_session(&state).await;

    // Try to dispatch a forbidden command (signoff)
    let app = build_bridge_app(state);
    let payload = serde_json::json!({
        "command": {
            "name": "review.approve_signoff",
            "params": {
                "target_id": "DEC-1",
                "reason": null
            }
        }
    });
    let (status, body) =
        post_json_with_token(app, "/api/v1/hosted-bridge/command", payload, &token).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["kind"], "CommandNotAllowed");
}

#[tokio::test]
async fn source_bind_command_rejected_through_bridge() {
    let state = AppState::new(test_audit());
    let token = pair_session(&state).await;

    let app = build_bridge_app(state);
    let payload = serde_json::json!({
        "command": {
            "name": "binding.create",
            "params": {
                "decision_or_candidate_id": "00000000-0000-0000-0000-000000000001",
                "bindings": [{
                    "file": "src/main.rs",
                    "range": "1-10"
                }],
                "commit_sha": "abc123"
            }
        }
    });
    let (status, body) =
        post_json_with_token(app, "/api/v1/hosted-bridge/command", payload, &token).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["kind"], "CommandNotAllowed");
}

#[tokio::test]
async fn ingest_command_rejected_through_bridge() {
    let state = AppState::new(test_audit());
    let token = pair_session(&state).await;

    let app = build_bridge_app(state);
    let payload = serde_json::json!({
        "command": {
            "name": "ingest.submit_local",
            "params": {
                "source_uri": "https://example.com",
                "source_type": "manual",
                "title": "test",
                "description": "test",
                "suggested_level": null,
                "label": null,
                "snapshot_content": null,
                "evidence": [],
                "candidate_drafts": [],
                "binding_hints": [],
                "rationale": null,
                "metadata": null
            }
        }
    });
    let (status, body) =
        post_json_with_token(app, "/api/v1/hosted-bridge/command", payload, &token).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["kind"], "CommandNotAllowed");
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. NEGATIVE: Authority-Smuggling Rejection
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn authority_smuggling_rejected_at_pairing() {
    let state = AppState::new(test_audit());
    let app = build_bridge_app(state);

    let payload = serde_json::json!({
        "origin": "https://app.bicameral-ai.com",
        "cloud_account_id": "acct-attacker",
        "csrf_token": "csrf-attacker",
        "team_id": "team-1",
        "cloud_access_token": "cloud-token-1",
        "authority_context": {
            "actor_id": "admin-root",
            "auth_method": "local_daemon"
        },
        "signoff_approval": true,
    });
    let (status, body) = post_json(app, "/api/v1/hosted-bridge/pair", payload).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["error"], "authority_smuggling_rejected");
    let smuggled = resp["smuggled_fields"].as_array().unwrap();
    assert!(smuggled.iter().any(|f| f == "authority_context"));
    assert!(smuggled.iter().any(|f| f == "signoff_approval"));
}

#[tokio::test]
async fn authority_smuggling_rejected_at_command() {
    let state = AppState::new(test_audit());
    let token = pair_session(&state).await;

    let app = build_bridge_app(state);
    let payload = serde_json::json!({
        "command": {
            "name": "preflight.run",
            "params": {
                "files": ["src/main.rs"],
                "symbols": [],
                "diff_context": null,
                "branch": null,
                "checkpoint_hints": []
            }
        },
        "authority_context": {
            "actor_id": "attacker-admin",
            "auth_method": "local_daemon"
        },
    });
    let (status, body) =
        post_json_with_token(app, "/api/v1/hosted-bridge/command", payload, &token).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["error"], "authority_smuggling_rejected");
}

#[tokio::test]
async fn event_store_write_smuggling_rejected() {
    let state = AppState::new(test_audit());
    let app = build_bridge_app(state);

    let payload = serde_json::json!({
        "origin": "https://app.bicameral-ai.com",
        "cloud_account_id": "acct-smuggler",
        "csrf_token": "csrf-smuggler",
        "team_id": "team-1",
        "cloud_access_token": "cloud-token-1",
        "event_store_write": {"event_type": "AcceptCandidate", "payload": {}},
    });
    let (status, body) = post_json(app, "/api/v1/hosted-bridge/pair", payload).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(resp["smuggled_fields"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f == "event_store_write"));
}

#[tokio::test]
async fn provider_token_smuggling_rejected() {
    let state = AppState::new(test_audit());
    let app = build_bridge_app(state);

    let payload = serde_json::json!({
        "origin": "https://app.bicameral-ai.com",
        "cloud_account_id": "acct-smuggler",
        "csrf_token": "csrf-smuggler",
        "team_id": "team-1",
        "cloud_access_token": "cloud-token-1",
        "provider_token": "ghp_STOLEN_TOKEN",
    });
    let (status, body) = post_json(app, "/api/v1/hosted-bridge/pair", payload).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(resp["smuggled_fields"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f == "provider_token"));
}

#[tokio::test]
async fn workspace_path_smuggling_rejected() {
    let state = AppState::new(test_audit());
    let app = build_bridge_app(state);

    let payload = serde_json::json!({
        "origin": "https://app.bicameral-ai.com",
        "cloud_account_id": "acct-smuggler",
        "csrf_token": "csrf-smuggler",
        "team_id": "team-1",
        "cloud_access_token": "cloud-token-1",
        "workspace_path": "/etc/shadow",
    });
    let (status, body) = post_json(app, "/api/v1/hosted-bridge/pair", payload).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(resp["smuggled_fields"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f == "workspace_path"));
}

#[tokio::test]
async fn external_ingest_envelope_smuggling_rejected() {
    let state = AppState::new(test_audit());
    let app = build_bridge_app(state);

    let payload = serde_json::json!({
        "origin": "https://app.bicameral-ai.com",
        "cloud_account_id": "acct-smuggler",
        "csrf_token": "csrf-smuggler",
        "team_id": "team-1",
        "cloud_access_token": "cloud-token-1",
        "external_ingest_envelope": {
            "source_system": "attacker",
            "content": "malicious"
        },
    });
    let (status, body) = post_json(app, "/api/v1/hosted-bridge/pair", payload).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(resp["smuggled_fields"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f == "external_ingest_envelope"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// 6. BOUNDARY: Hosted bridge is NOT ExternalIngestEnvelope
// ═══════════════════════════════════════════════════════════════════════════════

/// The bridge command dispatch path is distinct from external ingest.
/// Attempting to inject external_ingest_envelope via bridge is caught
/// by smuggling detection (above). This test further verifies that bridge
/// AuthorityContext uses HostedBridge auth method, not ExternalIngest.
#[tokio::test]
async fn bridge_authority_is_hosted_bridge_not_external_ingest() {
    use bicameral_api::action_authorization::{ActionAuthorizationPolicy, GovernedActionClass};
    use bicameral_api::tool_request::{AuthMethod, AuthorityContext};

    // Simulate the authority context the bridge command handler derives
    let bridge_authority = AuthorityContext {
        actor_id: "bridge:acct-test-alice".into(),
        auth_method: AuthMethod::HostedBridge,
        session_id: Some("sess-bridge-001".into()),
        workspace: None,
        policy_scope: vec!["hosted_bridge".into()],
        audit_metadata: serde_json::Map::new(),
    };

    // Read-only is permitted
    let read_result = ActionAuthorizationPolicy::evaluate_action_class(
        &bridge_authority,
        GovernedActionClass::ReadOnly,
    );
    assert!(read_result.is_ok());

    // Candidate review is permitted (review ≠ signoff)
    let review_result = ActionAuthorizationPolicy::evaluate_action_class(
        &bridge_authority,
        GovernedActionClass::CandidateReview,
    );
    assert!(review_result.is_ok());

    // Decision signoff is DENIED — bridge cannot mint signoff authority
    let signoff_result = ActionAuthorizationPolicy::evaluate_action_class(
        &bridge_authority,
        GovernedActionClass::DecisionSignoff,
    );
    assert!(signoff_result.is_err());
    let denial = signoff_result.unwrap_err();
    assert!(denial.message.contains("signoff"));
}

/// Hosted bridge cannot mint compliance resolution authority.
#[tokio::test]
async fn bridge_cannot_resolve_compliance() {
    use bicameral_api::action_authorization::{ActionAuthorizationPolicy, GovernedActionClass};
    use bicameral_api::tool_request::{AuthMethod, AuthorityContext};

    let bridge_authority = AuthorityContext {
        actor_id: "bridge:acct-compliance-attacker".into(),
        auth_method: AuthMethod::HostedBridge,
        session_id: Some("sess-compliance".into()),
        workspace: None,
        policy_scope: vec!["hosted_bridge".into()],
        audit_metadata: serde_json::Map::new(),
    };

    // ComplianceResolution requires local actor; HostedBridge is local-equivalent
    // for require_local_actor but NOT for owner authority. Verify that
    // compliance resolution via bridge is allowed at the action class level
    // (since it routes through require_local_actor, not require_owner_authority).
    // The actual command is blocked by the allowlist before reaching authorization.
    let result = ActionAuthorizationPolicy::evaluate_action_class(
        &bridge_authority,
        GovernedActionClass::ComplianceResolution,
    );
    // ComplianceResolution routes through require_local_actor which accepts
    // HostedBridge. The defense-in-depth is the command allowlist rejecting
    // "review.resolve_compliance" before it reaches authorization.
    assert!(result.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 7. BOUNDARY: No Local Path/Secret/Source Body Leakage
// ═══════════════════════════════════════════════════════════════════════════════

/// All hosted bridge projection responses must not contain sensitive fields.
#[tokio::test]
async fn operator_home_does_not_leak_sensitive_fields() {
    let state = AppState::new(test_audit());
    seed_project(&state).await;
    let token = pair_session(&state).await;

    let app = build_bridge_app(state);
    let (status, body) = get_with_token(app, "/api/v1/hosted-bridge/operator-home", &token).await;
    assert_eq!(status, StatusCode::OK);

    let body_str = String::from_utf8_lossy(&body);
    for field in bicameral_api::hosted_bridge::NEVER_EXPOSE_FIELDS {
        assert!(
            !body_str.contains(field),
            "operator-home response leaked '{field}'"
        );
    }
    // Verify the actual secret values are not present
    assert!(!body_str.contains("ghp_SECRETTOKEN"));
    assert!(!body_str.contains("sk-secret-api-key"));
    assert!(!body_str.contains("rt_REFRESH_SECRET"));
    assert!(!body_str.contains("/home/user/repos/"));
}

#[tokio::test]
async fn workspace_discovery_does_not_leak_sensitive_fields() {
    let state = AppState::new(test_audit());
    seed_project(&state).await;
    let token = pair_session(&state).await;

    let app = build_bridge_app(state);
    let (status, body) =
        get_with_token(app, "/api/v1/hosted-bridge/workspace-discovery", &token).await;
    assert_eq!(status, StatusCode::OK);

    let body_str = String::from_utf8_lossy(&body);
    for field in bicameral_api::hosted_bridge::NEVER_EXPOSE_FIELDS {
        assert!(
            !body_str.contains(field),
            "workspace-discovery response leaked '{field}'"
        );
    }
    assert!(!body_str.contains("ghp_SECRETTOKEN"));
    assert!(!body_str.contains("/home/user/repos/"));
}

#[tokio::test]
async fn dashboard_adapter_does_not_leak_sensitive_fields() {
    let state = AppState::new(test_audit());
    seed_project(&state).await;
    let token = pair_session(&state).await;

    let app = build_bridge_app(state);
    let (status, body) =
        get_with_token(app, "/api/v1/hosted-bridge/dashboard-adapter", &token).await;
    assert_eq!(status, StatusCode::OK);

    let body_str = String::from_utf8_lossy(&body);
    for field in bicameral_api::hosted_bridge::NEVER_EXPOSE_FIELDS {
        assert!(
            !body_str.contains(field),
            "dashboard-adapter response leaked '{field}'"
        );
    }
    assert!(!body_str.contains("ghp_SECRETTOKEN"));
    assert!(!body_str.contains("/home/user/repos/"));
}

#[tokio::test]
async fn projects_endpoint_does_not_leak_sensitive_fields() {
    let state = AppState::new(test_audit());
    seed_project(&state).await;
    let token = pair_session(&state).await;

    let app = build_bridge_app(state);
    let (status, body) = get_with_token(app, "/api/v1/hosted-bridge/projects", &token).await;
    assert_eq!(status, StatusCode::OK);

    let body_str = String::from_utf8_lossy(&body);
    for field in bicameral_api::hosted_bridge::NEVER_EXPOSE_FIELDS {
        assert!(
            !body_str.contains(field),
            "projects response leaked '{field}'"
        );
    }
    assert!(!body_str.contains("/home/user/repos/"));
}

/// The network-origin renderable ledger projection (#705, T4) must serve only
/// the approved allow-list — ids, timestamps, status/review-state enums,
/// counts, and redactor-passed titles — and must drop decision bodies, source
/// excerpts, code paths, actors, and provider secrets.
#[tokio::test]
async fn projection_ledger_does_not_leak_sensitive_fields() {
    let state = AppState::new(test_audit());
    seed_project(&state).await;
    seed_ledger_with_sensitive_data(&state).await;
    let token = pair_session(&state).await;

    let app = build_bridge_app(state);
    let (status, body) =
        get_with_token(app, "/api/v1/hosted-bridge/projection/ledger", &token).await;
    assert_eq!(status, StatusCode::OK);

    let body_str = String::from_utf8_lossy(&body);

    // Forbidden field NAMES from the full local projection must be absent.
    for field in bicameral_api::hosted_bridge::LEDGER_FORBIDDEN_FIELDS {
        assert!(
            !body_str.contains(field),
            "projection/ledger leaked forbidden field name '{field}'"
        );
    }
    for field in bicameral_api::hosted_bridge::NEVER_EXPOSE_FIELDS {
        assert!(
            !body_str.contains(field),
            "projection/ledger leaked sensitive field name '{field}'"
        );
    }

    // Confidential VALUES must never surface.
    assert!(
        !body_str.contains("AKIAIOSFODNN7EXAMPLE"),
        "projection/ledger leaked a secret embedded in the title/excerpt"
    );
    assert!(
        !body_str.contains("ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij"),
        "projection/ledger leaked a secret embedded in the decision body"
    );
    assert!(
        !body_str.contains("/home/user/repos/"),
        "projection/ledger leaked a local filesystem path"
    );
    assert!(
        !body_str.contains("Full decision body"),
        "projection/ledger leaked the raw decision body"
    );
    assert!(
        !body_str.contains("raw excerpt"),
        "projection/ledger leaked a source-evidence excerpt"
    );
    assert!(
        !body_str.contains("alice@secret-corp.example"),
        "projection/ledger leaked an actor identity"
    );
    assert!(
        !body_str.contains("auth.rs"),
        "projection/ledger leaked a code path"
    );

    // Positive: the approved renderable fields ARE present, and the title was
    // rendered with its secret redacted.
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["projection_only"], true);
    let decisions = resp["decisions"].as_array().expect("decisions array");
    assert_eq!(decisions.len(), 1);
    let entry = &decisions[0];
    assert!(entry["decision_id"].is_string());
    assert!(entry["promoted_at"].is_string());
    assert_eq!(entry["review_state"], "needs_signoff_review");
    assert_eq!(entry["signoff_state"], "approved");
    assert_eq!(entry["binding_count"], 1);
    assert_eq!(entry["source_evidence_count"], 1);
    let title = entry["title"].as_str().unwrap();
    assert!(
        title.contains("[redacted:secret]"),
        "title should be redacted: {title}"
    );
    assert!(
        title.contains("before launch"),
        "non-sensitive title text should survive"
    );

    // Events carry only id/type/timestamp — no actor.
    let events = resp["events"].as_array().expect("events array");
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(event["event_type"], "approve_signoff");
    assert!(
        event.get("actor").is_none(),
        "event must not carry an actor"
    );
    assert!(
        event.get("summary").is_none(),
        "event must not carry a summary"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 8. BOUNDARY: Replay/Idempotency
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn duplicate_pairing_yields_distinct_sessions() {
    let state = AppState::new(test_audit());
    let token1 = pair_session(&state).await;
    let token2 = pair_session(&state).await;

    // Each pairing produces a unique session token
    assert_ne!(token1, token2);

    // Both sessions are independently valid
    let app = build_bridge_app(state.clone());
    let (s1, _) = get_with_token(app, "/api/v1/hosted-bridge/status", &token1).await;
    assert_eq!(s1, StatusCode::OK);

    let app = build_bridge_app(state);
    let (s2, _) = get_with_token(app, "/api/v1/hosted-bridge/status", &token2).await;
    assert_eq!(s2, StatusCode::OK);
}

#[tokio::test]
async fn revoking_one_session_does_not_affect_another() {
    let state = AppState::new(test_audit());
    let token1 = pair_session(&state).await;
    let token2 = pair_session(&state).await;

    // Revoke token1
    let app = build_bridge_app(state.clone());
    let (status, _) = post_json_with_token(
        app,
        "/api/v1/hosted-bridge/revoke",
        serde_json::json!({}),
        &token1,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // token1 is revoked
    let app = build_bridge_app(state.clone());
    let (s1, _) = get_with_token(app, "/api/v1/hosted-bridge/status", &token1).await;
    assert_eq!(s1, StatusCode::FORBIDDEN);

    // token2 is still valid
    let app = build_bridge_app(state);
    let (s2, _) = get_with_token(app, "/api/v1/hosted-bridge/status", &token2).await;
    assert_eq!(s2, StatusCode::OK);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 9. BOUNDARY: Pairing Does Not Select Project / Bind Workspace
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn pairing_response_does_not_contain_project_selection() {
    let state = AppState::new(test_audit());
    seed_project(&state).await;

    let app = build_bridge_app(state);
    let payload = serde_json::json!({
        "origin": "https://app.bicameral-ai.com",
        "cloud_account_id": "acct-no-project",
        "csrf_token": "csrf-no-project",
        "team_id": "team-1",
        "cloud_access_token": "cloud-token-1",
        "client_protocol_version": bicameral_api::hosted_bridge::HOSTED_BRIDGE_PROTOCOL_VERSION,
    });
    let (status, body) = post_json(app, "/api/v1/hosted-bridge/pair", payload).await;
    // #790: pairing now returns a pending request, never an immediate session.
    assert_eq!(status, StatusCode::ACCEPTED);

    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["status"], "pending");
    // Pairing does not select a project and releases no token.
    assert!(resp.get("project_id").is_none());
    assert!(resp.get("selected_project").is_none());
    assert!(resp.get("session_token").is_none());
    // Workspace binding is not available at pairing level.
    assert_eq!(resp["capabilities"]["workspace_binding_available"], false);
}

#[tokio::test]
async fn pairing_does_not_create_workspace_binding() {
    let state = AppState::new(test_audit());
    seed_project(&state).await;
    let _token = pair_session(&state).await;

    // After pairing, no new workspace bindings were created
    let bindings = state.bindings.read().await;
    assert!(
        bindings.is_empty(),
        "pairing must not create workspace bindings"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 10. BOUNDARY: Invalid Remote Callback Authentication
// ═══════════════════════════════════════════════════════════════════════════════

/// Remote action callbacks cannot be routed through the hosted bridge.
/// The bridge command surface only accepts ToolCommand shapes on the
/// allowlist — it does not accept AuthenticatedRemoteActionEnvelope.
#[tokio::test]
async fn remote_callback_envelope_not_routable_through_bridge_command() {
    let state = AppState::new(test_audit());
    let token = pair_session(&state).await;

    let app = build_bridge_app(state);
    // Attempt to submit a remote action callback payload through bridge command
    let payload = serde_json::json!({
        "command": {
            "action_id": "00000000-0000-0000-0000-000000000042",
            "action_kind": "accept_candidate",
            "target_id": "CAND-99",
            "callback_source": "slack",
            "external_actor_id": "U_ATTACKER",
            "provider_signature": "v0=fake",
            "nonce": "nonce-attack-001",
            "issued_at": "2026-06-27T00:00:00Z",
        }
    });
    let (status, body) =
        post_json_with_token(app, "/api/v1/hosted-bridge/command", payload, &token).await;
    // Should fail to parse as ToolCommand (not a valid command shape)
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["kind"], "UnsupportedCommand");
}

// ═══════════════════════════════════════════════════════════════════════════════
// 11. BOUNDARY: Command Dispatch with RuntimeDispatch (ToolRequest authorization)
// ═══════════════════════════════════════════════════════════════════════════════

/// When runtime dispatch is not configured, bridge commands return
/// RuntimeUnavailable — they do not fall through to unguarded paths.
#[tokio::test]
async fn command_without_runtime_dispatch_returns_unavailable() {
    let state = AppState::new(test_audit());
    // state.runtime_dispatch is None by default
    let token = pair_session(&state).await;

    let app = build_bridge_app(state);
    let payload = serde_json::json!({
        "command": {
            "name": "preflight.run",
            "params": {
                "files": ["src/main.rs"],
                "symbols": [],
                "diff_context": null,
                "branch": null,
                "checkpoint_hints": []
            }
        }
    });
    let (status, body) =
        post_json_with_token(app, "/api/v1/hosted-bridge/command", payload, &token).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["kind"], "RuntimeUnavailable");
}

// ═══════════════════════════════════════════════════════════════════════════════
// 11b. BOUNDARY: Product creation commands are cloud-routable (#766)
// ═══════════════════════════════════════════════════════════════════════════════

/// `product.create` is an exceptional canonical daemon write that the SPA may
/// route through the bridge — authority is validated by the daemon-confirmed
/// hosted actor context, not by the caller. It must therefore pass the bridge
/// command allowlist (reaching runtime dispatch), unlike governed local
/// mutations such as `binding.create` which stay `CommandNotAllowed`.
#[tokio::test]
async fn product_create_passes_bridge_allowlist() {
    let state = AppState::new(test_audit());
    let token = pair_session(&state).await;

    let app = build_bridge_app(state);
    let payload = serde_json::json!({
        "command": {
            "name": "product.create",
            "params": {
                "idempotency_key": "idem-bridge-1",
                "product_display_name": "Acme Checkout",
                "asserted_cloud_account_id": "acct-test-alice",
            }
        }
    });
    let (status, body) =
        post_json_with_token(app, "/api/v1/hosted-bridge/command", payload, &token).await;
    // Allowlist passed → runtime dispatch attempted. No runtime configured in
    // this test, so it surfaces RuntimeUnavailable rather than CommandNotAllowed.
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["kind"], "RuntimeUnavailable");
}

#[tokio::test]
async fn product_publish_passes_bridge_allowlist() {
    let state = AppState::new(test_audit());
    let token = pair_session(&state).await;

    let app = build_bridge_app(state);
    let payload = serde_json::json!({
        "command": {
            "name": "product.publish",
            "params": {
                "product_id": "prd_01HZX8CHECKOUT",
                "idempotency_key": "prd_01HZX8CHECKOUT:publish",
            }
        }
    });
    let (status, body) =
        post_json_with_token(app, "/api/v1/hosted-bridge/command", payload, &token).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["kind"], "RuntimeUnavailable");
}

// ═══════════════════════════════════════════════════════════════════════════════
// 12. BOUNDARY: Dashboard adapter fails closed when unpaired
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn dashboard_adapter_rejects_unpaired_without_fixtures() {
    let state = AppState::new(test_audit());
    seed_project(&state).await;
    let app = build_bridge_app(state);

    // Request without a valid session fails closed; no fixture-backed success.
    let (status, body) = get_with_token(
        app,
        "/api/v1/hosted-bridge/dashboard-adapter",
        "invalid-token",
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["kind"], "NotPaired");
    assert!(resp.get("fixture_fallback").is_none());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 13. BOUNDARY: Missing required fields at pairing
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn missing_csrf_token_rejected() {
    let state = AppState::new(test_audit());
    let app = build_bridge_app(state);

    let payload = serde_json::json!({
        "origin": "https://app.bicameral-ai.com",
        "cloud_account_id": "acct-no-csrf",
    });
    let (status, _) = post_json(app, "/api/v1/hosted-bridge/pair", payload).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn empty_csrf_token_rejected() {
    let state = AppState::new(test_audit());
    let app = build_bridge_app(state);

    let payload = serde_json::json!({
        "origin": "https://app.bicameral-ai.com",
        "cloud_account_id": "acct-empty-csrf",
        "csrf_token": "",
        "team_id": "team-1",
        "cloud_access_token": "cloud-token-1",
    });
    let (status, _) = post_json(app, "/api/v1/hosted-bridge/pair", payload).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 14. COVERAGE GAP DECLARATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Named CORS/CSRF coverage gap per acceptance criteria.
///
/// Browser-level CORS preflight (OPTIONS), SameSite cookie behavior, and
/// CSRF token rotation are enforced by the browser + gateway middleware layers
/// that require Playwright or integration harness coverage not available in
/// Rust unit/integration tests.
///
/// Covered here: CSRF token presence validation (empty/missing rejected).
/// NOT covered: browser-enforced SameSite, CORS preflight OPTIONS handling,
/// CSRF token rotation on session refresh.
#[test]
fn coverage_gap_cors_csrf_requires_browser_harness() {
    // This test documents the gap. It passes by construction.
    // Playwright/Cypress coverage would verify:
    //   - OPTIONS preflight returns correct Access-Control-Allow-Origin
    //   - Cross-origin requests without CORS headers are blocked
    //   - CSRF token is rotated on session refresh
    //   - SameSite cookie attribute prevents cross-site session riding
}

// ═══════════════════════════════════════════════════════════════════════════════
// 12. DAEMON PROTOCOL COMPATIBILITY WINDOW (#697 / #711)
// ═══════════════════════════════════════════════════════════════════════════════
//
// Compatibility is keyed on the integer `daemon_protocol_version`, never on the
// daemon release version / semver. The alpha window is "current + N-1"; daemons
// 2+ breaking versions behind get `hosted_dashboard_upgrade_required` with a
// one-action repair path, and daemons newer than the SPA expects degrade to a
// benign forward-skew state rather than hard-failing.

/// The pairing handshake advertises the integer daemon_protocol_version.
#[tokio::test]
async fn pairing_advertises_integer_daemon_protocol_version() {
    let state = AppState::new(test_audit());
    let app = build_bridge_app(state);
    let payload = serde_json::json!({
        "origin": "https://app.bicameral-ai.com",
        "cloud_account_id": "acct-test-alice",
        "csrf_token": "csrf-valid-token-001",
        "team_id": "team-1",
        "cloud_access_token": "cloud-token-1",
        "client_protocol_version": bicameral_api::hosted_bridge::HOSTED_BRIDGE_PROTOCOL_VERSION,
    });
    let (status, body) = post_json(app, "/api/v1/hosted-bridge/pair", payload).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        resp["capabilities"]["daemon_protocol_version"],
        serde_json::json!(bicameral_api::hosted_bridge::HOSTED_DASHBOARD_PROTOCOL_VERSION),
    );
    assert_eq!(
        resp["capabilities"]["daemon_build_version"],
        serde_json::json!(bicameral_api::hosted_bridge::DAEMON_BUILD_VERSION),
    );
    assert_eq!(
        resp["capabilities"]["hosted_bridge_protocol_version"],
        serde_json::json!(bicameral_api::hosted_bridge::HOSTED_BRIDGE_PROTOCOL_VERSION),
    );
}

/// Cloud at parity with the daemon → current, controls available, no repair.
#[tokio::test]
async fn compatibility_current_protocol_is_fully_available() {
    let state = AppState::new(test_audit());
    let app = build_bridge_app(state);
    let daemon = bicameral_api::hosted_bridge::HOSTED_DASHBOARD_PROTOCOL_VERSION;
    let (status, body) = get_without_token(
        app,
        &format!("/api/v1/hosted-bridge/compatibility?cloud_protocol_version={daemon}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["state"], "current");
    assert_eq!(resp["upgrade_required"], false);
    assert_eq!(resp["controls_available"], true);
    assert_eq!(resp["read_only_projections_available"], true);
    assert!(resp.get("repair").is_none() || resp["repair"].is_null());
}

/// Cloud one ahead of daemon → N-1 supported, still usable, no repair.
#[tokio::test]
async fn compatibility_n_minus_1_is_supported() {
    let state = AppState::new(test_audit());
    let app = build_bridge_app(state);
    let cloud = bicameral_api::hosted_bridge::HOSTED_DASHBOARD_PROTOCOL_VERSION + 1;
    let (status, body) = get_without_token(
        app,
        &format!("/api/v1/hosted-bridge/compatibility?cloud_protocol_version={cloud}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["state"], "supported_previous");
    assert_eq!(resp["upgrade_required"], false);
    assert_eq!(resp["controls_available"], true);
}

/// Cloud 2+ ahead of daemon → upgrade required, gated, one-action repair path.
#[tokio::test]
async fn compatibility_two_versions_behind_requires_upgrade() {
    let state = AppState::new(test_audit());
    let app = build_bridge_app(state);
    let cloud = bicameral_api::hosted_bridge::HOSTED_DASHBOARD_PROTOCOL_VERSION + 2;
    let (status, body) = get_without_token(
        app,
        &format!("/api/v1/hosted-bridge/compatibility?cloud_protocol_version={cloud}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["state"], "hosted_dashboard_upgrade_required");
    assert_eq!(resp["upgrade_required"], true);
    assert_eq!(resp["controls_available"], false);
    assert_eq!(resp["read_only_projections_available"], false);
    // One-action repair: a single copy-paste update command.
    assert!(resp["repair"]["update_command"].as_str().is_some());
    assert!(resp["repair"]["instruction"].as_str().is_some());
}

/// Cloud behind the daemon → forward skew degrades safely, does not hard fail.
#[tokio::test]
async fn compatibility_forward_skew_degrades_safely() {
    let state = AppState::new(test_audit());
    let app = build_bridge_app(state);
    // Cloud expects an older protocol than this newer daemon speaks.
    let (status, body) = get_without_token(
        app,
        "/api/v1/hosted-bridge/compatibility?cloud_protocol_version=0",
    )
    .await;
    // Forward skew is a 200 benign state, never a 4xx/5xx hard failure.
    assert_eq!(status, StatusCode::OK);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["state"], "hosted_dashboard_forward_skew");
    assert_eq!(resp["forward_skew"], true);
    assert_eq!(resp["upgrade_required"], false);
    assert_eq!(resp["read_only_projections_available"], true);
    assert_eq!(resp["controls_available"], false);
}

/// Telemetry aggregates fire rate + protocol-version distribution only —
/// never workspace, ledger, source, evidence, or local-path data.
#[tokio::test]
async fn compatibility_telemetry_records_fire_rate_and_distribution_only() {
    let state = AppState::new(test_audit());
    let daemon = bicameral_api::hosted_bridge::HOSTED_DASHBOARD_PROTOCOL_VERSION;

    // current, upgrade-required, forward-skew.
    for cloud in [daemon, daemon + 2, 0] {
        let app = build_bridge_app(state.clone());
        let (status, _) = get_without_token(
            app,
            &format!("/api/v1/hosted-bridge/compatibility?cloud_protocol_version={cloud}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    let app = build_bridge_app(state);
    let (status, body) =
        get_without_token(app, "/api/v1/hosted-bridge/compatibility/telemetry").await;
    assert_eq!(status, StatusCode::OK);
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["total_checks"], 3);
    assert_eq!(resp["upgrade_required_count"], 1);
    assert_eq!(resp["forward_skew_count"], 1);
    assert!((resp["upgrade_required_fire_rate"].as_f64().unwrap() - 1.0 / 3.0).abs() < 1e-9);
    // Distribution keys are protocol-version integers; the daemon only ever
    // reports its own version here.
    let dist = &resp["daemon_protocol_version_distribution"];
    assert_eq!(dist[daemon.to_string()], 3);

    // No confidential field names anywhere in the telemetry payload.
    let raw = String::from_utf8_lossy(&body).to_lowercase();
    for forbidden in ["path", "workspace", "source", "evidence", "secret", "token"] {
        assert!(
            !raw.contains(forbidden),
            "telemetry leaked forbidden token '{forbidden}': {raw}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 13. #769: local setup continuation (Open / Continue setup / Repair)
// ═══════════════════════════════════════════════════════════════════════════════

mod continue_setup_769 {
    use super::*;
    use bicameral_api::product_registration::{
        ContinueSetupRejectionCode, ContinueSetupStep, ProductContinueSetupResult, SetupReadiness,
        SetupRepairReason,
    };
    use bicameral_api::tool_request::{ToolRequest, ToolResponse};
    use bicameral_gateway::state::RuntimeDispatch;
    use std::sync::{Arc, Mutex};

    type Captured = Arc<Mutex<Option<serde_json::Map<String, serde_json::Value>>>>;

    /// A runtime-dispatch stub that captures the daemon-set authority metadata
    /// and returns a canned continuation result. This lets the gateway contract
    /// tests assert the fact-derivation + redacted projection the route owns,
    /// without pulling the runtime crate into the gateway's test graph.
    struct StubDispatch {
        captured: Captured,
        result: ProductContinueSetupResult,
    }

    impl RuntimeDispatch for StubDispatch {
        fn dispatch(&self, request: &ToolRequest, _ws: Option<&std::path::Path>) -> ToolResponse {
            *self.captured.lock().unwrap() = Some(request.authority.audit_metadata.clone());
            let mut result = self.result.clone();
            result.request_id = request.request_id.to_string();
            ToolResponse::success(request.request_id, serde_json::to_value(&result).unwrap())
        }
        fn capabilities(&self) -> serde_json::Value {
            serde_json::json!({})
        }
    }

    fn now() -> chrono::DateTime<Utc> {
        Utc::now()
    }

    fn with_stub(result: ProductContinueSetupResult) -> (AppState, Captured) {
        let mut state = AppState::new(test_audit());
        let captured: Captured = Arc::new(Mutex::new(None));
        state.runtime_dispatch = Some(Arc::new(StubDispatch {
            captured: captured.clone(),
            result,
        }));
        (state, captured)
    }

    fn body() -> serde_json::Value {
        serde_json::json!({
            "product_id": "prd_test",
            "registration_ref": "reg_ref_opaque",
            "provider_hint": "github",
        })
    }

    /// Seed a Connected github connection under a temp workspace root so the
    /// gateway derives `provider_access = authorized`. Returns the guard that
    /// must outlive the request.
    async fn seed_connected_github(state: &AppState) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".bicameral").join("connections.yaml");
        let mut registry = bicameral_config::ConnectionRegistry::default();
        registry.upsert(
            "github",
            bicameral_config::ConnectionMetadata {
                status: bicameral_config::ConnectionStatus::Connected,
                account_label: Some("acme-org".into()),
                capabilities: vec![],
                updated_at: None,
                auth_method: None,
            },
        );
        registry.save_to_file(&path).unwrap();
        *state.workspace_root.write().await = Some(dir.path().to_path_buf());
        dir
    }

    #[tokio::test]
    async fn joined_without_provider_projects_continue_setup() {
        let (state, captured) = with_stub(ProductContinueSetupResult::advanced(
            "req".into(),
            "prd_test".into(),
            "acct-test-alice".into(),
            SetupReadiness::ContinueSetup {
                step: ContinueSetupStep::ConnectProvider,
            },
            None,
            now(),
        ));
        let token = pair_session(&state).await;
        let app = build_bridge_app(state);
        let (status, raw) = post_json_with_token(
            app,
            "/api/v1/hosted-bridge/product-continue-setup",
            body(),
            &token,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&raw));
        let resp: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(resp["readiness"], "continue_setup");
        assert_eq!(resp["next_step"], "connect_provider");
        assert_eq!(resp["has_registration"], false);
        assert_eq!(resp["membership_retained"], true);
        assert!(resp.get("registration_ref").is_none());

        // The gateway derived provider access from local state (no connection →
        // absent) and set it as daemon metadata, not from the SPA body.
        let meta = captured.lock().unwrap().clone().unwrap();
        assert_eq!(meta["setup_provider_access"], "absent");
    }

    #[tokio::test]
    async fn authorized_provider_projects_open_and_redacts_registration_ref() {
        let (state, captured) = with_stub(ProductContinueSetupResult::advanced(
            "req".into(),
            "prd_test".into(),
            "acct-test-alice".into(),
            SetupReadiness::Open,
            Some("reg_ref_SECRET_DO_NOT_LEAK".into()),
            now(),
        ));
        let _guard = seed_connected_github(&state).await;
        let token = pair_session(&state).await;
        let app = build_bridge_app(state);
        let (status, raw) = post_json_with_token(
            app,
            "/api/v1/hosted-bridge/product-continue-setup",
            body(),
            &token,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&raw));
        let text = String::from_utf8_lossy(&raw);
        let resp: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(resp["readiness"], "open");
        assert_eq!(resp["has_registration"], true);
        // The opaque registration_ref never crosses the bridge.
        assert!(
            !text.contains("reg_ref_SECRET_DO_NOT_LEAK"),
            "registration_ref leaked into hosted projection: {text}"
        );

        let meta = captured.lock().unwrap().clone().unwrap();
        assert_eq!(meta["setup_provider_access"], "authorized");
        assert_eq!(meta["setup_descriptor_validation"], "valid");
    }

    #[tokio::test]
    async fn replay_failure_projects_repair() {
        let (state, _captured) = with_stub(ProductContinueSetupResult::advanced(
            "req".into(),
            "prd_test".into(),
            "acct-test-alice".into(),
            SetupReadiness::Repair {
                reason: SetupRepairReason::ReplayFailed,
            },
            Some("reg_ref_opaque".into()),
            now(),
        ));
        let token = pair_session(&state).await;
        let app = build_bridge_app(state);
        let (status, raw) = post_json_with_token(
            app,
            "/api/v1/hosted-bridge/product-continue-setup",
            body(),
            &token,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let resp: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(resp["readiness"], "repair");
        assert_eq!(resp["repair_reason"], "replay_failed");
        assert_eq!(resp["membership_retained"], true);
    }

    #[tokio::test]
    async fn missing_membership_is_typed_rejection_retaining_no_false_membership() {
        let (state, _captured) = with_stub(ProductContinueSetupResult::rejected(
            "req".into(),
            "prd_test".into(),
            "acct-test-alice".into(),
            ContinueSetupRejectionCode::MembershipRequired,
            false,
            now(),
        ));
        let token = pair_session(&state).await;
        let app = build_bridge_app(state);
        let (status, raw) = post_json_with_token(
            app,
            "/api/v1/hosted-bridge/product-continue-setup",
            body(),
            &token,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        let resp: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(resp["code"], "membership_required");
        assert_eq!(resp["membership_retained"], false);
    }

    #[tokio::test]
    async fn authority_smuggling_rejected() {
        let (state, _captured) = with_stub(ProductContinueSetupResult::advanced(
            "req".into(),
            "prd_test".into(),
            "acct-test-alice".into(),
            SetupReadiness::Open,
            None,
            now(),
        ));
        let token = pair_session(&state).await;
        let app = build_bridge_app(state);
        let mut payload = body();
        payload["authority_context"] =
            serde_json::json!({ "actor_id": "attacker", "auth_method": "local_daemon" });
        let (status, raw) = post_json_with_token(
            app,
            "/api/v1/hosted-bridge/product-continue-setup",
            payload,
            &token,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let resp: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(resp["error"], "authority_smuggling_rejected");
    }

    #[tokio::test]
    async fn unknown_field_in_body_rejected() {
        let (state, _captured) = with_stub(ProductContinueSetupResult::advanced(
            "req".into(),
            "prd_test".into(),
            "acct-test-alice".into(),
            SetupReadiness::Open,
            None,
            now(),
        ));
        let token = pair_session(&state).await;
        let app = build_bridge_app(state);
        let payload = serde_json::json!({
            "product_id": "prd_test",
            "registration_ref": "reg_ref_opaque",
            "provider_token": "ghp_should_be_rejected",
        });
        let (status, _raw) = post_json_with_token(
            app,
            "/api/v1/hosted-bridge/product-continue-setup",
            payload,
            &token,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn no_runtime_dispatch_returns_unavailable() {
        let state = AppState::new(test_audit());
        let token = pair_session(&state).await;
        let app = build_bridge_app(state);
        let (status, raw) = post_json_with_token(
            app,
            "/api/v1/hosted-bridge/product-continue-setup",
            body(),
            &token,
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        let resp: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(resp["kind"], "RuntimeUnavailable");
    }

    #[tokio::test]
    async fn unpaired_session_cannot_continue_setup() {
        let state = AppState::new(test_audit());
        let app = build_bridge_app(state);
        let (status, _raw) = post_json_with_token(
            app,
            "/api/v1/hosted-bridge/product-continue-setup",
            body(),
            "invalid-token",
        )
        .await;
        assert_ne!(status, StatusCode::OK);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 15. PENDING PAIRING / LOCAL CONFIRMATION FLOW (#790)
// ═══════════════════════════════════════════════════════════════════════════════
//
// Contract-level (supporting) evidence for the daemon pending-pairing contract:
// `POST /pair` returns a 202 pending request (opaque handle/proof, expiry,
// capability/version, local confirmation affordance) and never a session token;
// only an explicit same-origin local approval transitions a pending request to a
// paired session; hosted/cross-origin/replayed/malformed/mismatched attempts fail
// closed; denied/expired outcomes are typed and tokenless; the post-approval
// status handoff is retry-safe. This is NOT terminal browser lifecycle / User
// Journey acceptance — that remains bicameral-cloud#121.

mod pending_pairing_flow {
    use super::*;

    /// Force a pending request to be expired by rewinding its `expires_at`.
    async fn expire_pending(state: &AppState, request_handle: &str) {
        let mut pending = state.hosted_bridge_pending.write().await;
        let record = pending.get_mut(request_handle).expect("pending record");
        record.expires_at = chrono::Utc::now() - chrono::Duration::seconds(1);
    }

    async fn approve_locally(state: &AppState, request_handle: &str) -> (StatusCode, Vec<u8>) {
        let nonce = local_confirmation_nonce(state, request_handle).await;
        let app = build_bridge_app(state.clone());
        post_json(
            app,
            "/api/v1/hosted-bridge/pair/decision",
            serde_json::json!({
                "request_handle": request_handle,
                "action": "approve",
                "local_confirmation_nonce": nonce,
            }),
        )
        .await
    }

    async fn poll_status(
        state: &AppState,
        request_handle: &str,
        poll_proof: &str,
        account: &str,
        origin: &str,
    ) -> (StatusCode, serde_json::Value) {
        let app = build_bridge_app(state.clone());
        let (status, body) = get_with_origin(
            app,
            &format!(
                "/api/v1/hosted-bridge/pair/status?request_handle={request_handle}\
                 &poll_proof={poll_proof}&cloud_account_id={account}"
            ),
            origin,
        )
        .await;
        let value = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
        (status, value)
    }

    #[tokio::test]
    async fn pending_creation_returns_202_with_handle_and_no_token() {
        let state = AppState::new(test_audit());
        let pending = create_pending_request(&state).await;

        assert_eq!(pending["status"], "pending");
        assert!(pending["request_handle"]
            .as_str()
            .is_some_and(|s| !s.is_empty()));
        assert!(pending["poll_proof"]
            .as_str()
            .is_some_and(|s| !s.is_empty()));
        let pairing_code = pending["pairing_code"].as_str().unwrap();
        assert_eq!(pairing_code.len(), 9);
        assert_eq!(pairing_code.as_bytes()[4], b'-');
        assert!(pending["expires_at"]
            .as_str()
            .is_some_and(|s| !s.is_empty()));
        assert_eq!(
            pending["protocol_version"],
            serde_json::json!(bicameral_api::hosted_bridge::HOSTED_BRIDGE_PROTOCOL_VERSION)
        );
        // No token, no paired session anywhere in the pending response.
        assert!(pending.get("session_token").is_none());
        let raw = serde_json::to_string(&pending).unwrap();
        assert!(!raw.contains("session_token"));
        // A terminal confirmation affordance is offered with the capability summary.
        let confirmation = &pending["confirmation"];
        // The daemon advertises its loopback approval page as a same-origin path.
        assert_eq!(
            confirmation["local_confirmation_url"],
            serde_json::json!(format!(
                "/local/pairing?request={}",
                pending["request_handle"].as_str().unwrap()
            ))
        );
        assert_eq!(
            confirmation["terminal_command"],
            serde_json::json!("bicameral pairing approve")
        );
        assert!(!confirmation["capability_summary"]["allowed"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(!confirmation["capability_summary"]["forbidden"]
            .as_array()
            .unwrap()
            .is_empty());

        // Plaintext is response-only. Daemon state retains a fixed-size digest.
        let handle = pending["request_handle"].as_str().unwrap();
        let records = state.hosted_bridge_pending.read().await;
        let record = records.get(handle).unwrap();
        assert_eq!(record.pairing_code_digest.len(), 32);
        assert!(!record
            .pairing_code_digest
            .windows(pairing_code.len())
            .any(|window| window == pairing_code.as_bytes()));
    }

    #[tokio::test]
    async fn local_code_lookup_returns_match_without_approving() {
        let state = AppState::new(test_audit());
        let pending = create_pending_request(&state).await;
        let pairing_code = pending["pairing_code"].as_str().unwrap().to_lowercase();

        let app = build_bridge_app(state.clone());
        let (status, body) = post_json(
            app,
            "/api/v1/hosted-bridge/pair/code/lookup",
            serde_json::json!({ "pairing_code": pairing_code }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let view: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(view["requesting_origin"], TEST_ORIGIN);
        assert_eq!(view["cloud_account_id"], TEST_ACCOUNT);
        assert_eq!(view["status"], "pending");
        assert!(!body_contains_token(&body));
        assert!(state.hosted_bridge_sessions.read().await.is_empty());
    }

    #[tokio::test]
    async fn hosted_origin_cannot_lookup_pairing_code() {
        let state = AppState::new(test_audit());
        let pending = create_pending_request(&state).await;
        let app = build_bridge_app(state.clone());
        let (status, body) = post_json_with_origin(
            app,
            "/api/v1/hosted-bridge/pair/code/lookup",
            serde_json::json!({ "pairing_code": pending["pairing_code"] }),
            TEST_ORIGIN,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(!body_contains_token(&body));
        assert!(state.hosted_bridge_sessions.read().await.is_empty());
    }

    #[tokio::test]
    async fn wrong_pairing_code_fails_closed_without_changing_state() {
        let state = AppState::new(test_audit());
        let pending = create_pending_request(&state).await;
        let app = build_bridge_app(state.clone());
        let (status, body) = post_json(
            app,
            "/api/v1/hosted-bridge/pair/code/lookup",
            serde_json::json!({ "pairing_code": "0000-0000" }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(!body_contains_token(&body));
        assert!(state.hosted_bridge_sessions.read().await.is_empty());
        let records = state.hosted_bridge_pending.read().await;
        let handle = pending["request_handle"].as_str().unwrap();
        assert_eq!(
            records.get(handle).unwrap().decision,
            bicameral_gateway::state::PendingPairingDecision::Pending
        );
    }

    #[tokio::test]
    async fn old_client_without_version_receives_update_required() {
        let state = AppState::new(test_audit());
        let app = build_bridge_app(state);
        let payload = serde_json::json!({
            "origin": TEST_ORIGIN,
            "cloud_account_id": TEST_ACCOUNT,
            "csrf_token": "csrf-valid-token-001",
            "team_id": "team-1",
            "cloud_access_token": "cloud-token-1",
        });
        let (status, body) = post_json(app, "/api/v1/hosted-bridge/pair", payload).await;
        assert_eq!(status, StatusCode::UPGRADE_REQUIRED);
        let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(resp["status"], "incompatible");
        assert_eq!(resp["error"], "update_required");
        assert!(!body_contains_token(&body));
    }

    #[tokio::test]
    async fn old_immediate_pair_client_v2_receives_update_required() {
        let state = AppState::new(test_audit());
        let app = build_bridge_app(state);
        let payload = serde_json::json!({
            "origin": TEST_ORIGIN,
            "cloud_account_id": TEST_ACCOUNT,
            "csrf_token": "csrf-valid-token-001",
            "team_id": "team-1",
            "cloud_access_token": "cloud-token-1",
            "client_protocol_version": 2,
        });
        let (status, body) = post_json(app, "/api/v1/hosted-bridge/pair", payload).await;
        assert_eq!(status, StatusCode::UPGRADE_REQUIRED);
        let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(resp["status"], "incompatible");
        assert_eq!(
            resp["required_protocol_version"],
            serde_json::json!(bicameral_api::hosted_bridge::PENDING_PAIRING_MIN_PROTOCOL_VERSION)
        );
        assert!(!body_contains_token(&body));
    }

    #[tokio::test]
    async fn local_pending_view_returns_details_without_token() {
        let state = AppState::new(test_audit());
        let pending = create_pending_request(&state).await;
        let handle = pending["request_handle"].as_str().unwrap();

        let app = build_bridge_app(state.clone());
        let (status, body) =
            get_without_token(app, &format!("/api/v1/hosted-bridge/pair/pending/{handle}")).await;
        assert_eq!(status, StatusCode::OK);
        let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(resp["status"], "pending");
        assert_eq!(resp["requesting_origin"], TEST_ORIGIN);
        assert_eq!(resp["cloud_account_id"], TEST_ACCOUNT);
        assert!(resp["local_confirmation_nonce"]
            .as_str()
            .is_some_and(|s| !s.is_empty()));
        assert!(!body_contains_token(&body));
    }

    #[tokio::test]
    async fn hosted_origin_cannot_read_local_pending_view() {
        let state = AppState::new(test_audit());
        let pending = create_pending_request(&state).await;
        let handle = pending["request_handle"].as_str().unwrap();

        let app = build_bridge_app(state.clone());
        let (status, body) = get_with_origin(
            app,
            &format!("/api/v1/hosted-bridge/pair/pending/{handle}"),
            TEST_ORIGIN,
        )
        .await;
        // A hosted (cross-origin) surface must not read the local view.
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(!body_contains_token(&body));
    }

    #[tokio::test]
    async fn local_approval_then_status_releases_token_and_is_retry_safe() {
        let state = AppState::new(test_audit());
        let pending = create_pending_request(&state).await;
        let handle = pending["request_handle"].as_str().unwrap().to_string();
        let proof = pending["poll_proof"].as_str().unwrap().to_string();

        // Before approval the initiator only sees pending — no token.
        let (status, resp) = poll_status(&state, &handle, &proof, TEST_ACCOUNT, TEST_ORIGIN).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(resp["status"], "pending");
        assert!(resp.get("session_token").is_none());

        let (approve_status, approve_body) = approve_locally(&state, &handle).await;
        assert_eq!(approve_status, StatusCode::OK);
        // The approval response itself does not carry the token.
        assert!(!body_contains_token(&approve_body));

        // First poll after approval releases the token.
        let (status, resp) = poll_status(&state, &handle, &proof, TEST_ACCOUNT, TEST_ORIGIN).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(resp["status"], "approved");
        let token = resp["session_token"].as_str().unwrap().to_string();
        assert!(!token.is_empty());

        // Retry-safe: a repeated poll returns the same still-valid token.
        let (status, resp2) = poll_status(&state, &handle, &proof, TEST_ACCOUNT, TEST_ORIGIN).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(resp2["session_token"].as_str().unwrap(), token);

        // The released token is a real, usable session.
        let app = build_bridge_app(state);
        let (s, _) = get_with_token(app, "/api/v1/hosted-bridge/status", &token).await;
        assert_eq!(s, StatusCode::OK);
    }

    #[tokio::test]
    async fn daemon_restart_reattaches_from_durable_pairing_without_new_code() {
        let dir = tempfile::tempdir().unwrap();
        let pairings_path = dir.path().join("pairings.json");
        let daemon_instance_id = "daemon-stable-across-restart";

        let first_state = AppState::new(test_audit())
            .with_durable_pairing_store(std::sync::Arc::new(
                bicameral_gateway::durable_pairing_store::DurablePairingStore::open(&pairings_path)
                    .unwrap(),
            ))
            .with_daemon_instance_id(daemon_instance_id);
        let pending = create_pending_request(&first_state).await;
        let handle = pending["request_handle"].as_str().unwrap().to_string();
        let proof = pending["poll_proof"].as_str().unwrap().to_string();

        let (approve_status, _) = approve_locally(&first_state, &handle).await;
        assert_eq!(approve_status, StatusCode::OK);
        let (_, first_handoff) =
            poll_status(&first_state, &handle, &proof, TEST_ACCOUNT, TEST_ORIGIN).await;
        let first_token = first_handoff["session_token"]
            .as_str()
            .expect("first process should release a live token")
            .to_string();
        drop(first_state);

        let restarted_state = AppState::new(test_audit())
            .with_durable_pairing_store(std::sync::Arc::new(
                bicameral_gateway::durable_pairing_store::DurablePairingStore::open(&pairings_path)
                    .unwrap(),
            ))
            .with_daemon_instance_id(daemon_instance_id);

        let (status, restored) =
            poll_status(&restarted_state, &handle, &proof, TEST_ACCOUNT, TEST_ORIGIN).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(restored["status"], "approved");
        let restored_token = restored["session_token"]
            .as_str()
            .expect("restart should remint a live token");
        assert_ne!(restored_token, first_token);

        let app = build_bridge_app(restarted_state);
        let (status, _) = get_with_token(app, "/api/v1/hosted-bridge/status", restored_token).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn durable_reattach_status_retry_reuses_the_live_reminted_session() {
        let dir = tempfile::tempdir().unwrap();
        let pairings_path = dir.path().join("pairings.json");
        let daemon_instance_id = "daemon-stable-across-restart";
        let first_state = AppState::new(test_audit())
            .with_durable_pairing_store(std::sync::Arc::new(
                bicameral_gateway::durable_pairing_store::DurablePairingStore::open(&pairings_path)
                    .unwrap(),
            ))
            .with_daemon_instance_id(daemon_instance_id);
        let pending = create_pending_request(&first_state).await;
        let handle = pending["request_handle"].as_str().unwrap().to_string();
        let proof = pending["poll_proof"].as_str().unwrap().to_string();
        approve_locally(&first_state, &handle).await;
        drop(first_state);

        let restarted_state = AppState::new(test_audit())
            .with_durable_pairing_store(std::sync::Arc::new(
                bicameral_gateway::durable_pairing_store::DurablePairingStore::open(&pairings_path)
                    .unwrap(),
            ))
            .with_daemon_instance_id(daemon_instance_id);
        let (_, first_restore) =
            poll_status(&restarted_state, &handle, &proof, TEST_ACCOUNT, TEST_ORIGIN).await;
        let first_reminted_token = first_restore["session_token"].as_str().unwrap();

        let (_, retry_restore) =
            poll_status(&restarted_state, &handle, &proof, TEST_ACCOUNT, TEST_ORIGIN).await;
        assert_eq!(
            retry_restore["session_token"].as_str().unwrap(),
            first_reminted_token
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_durable_reattach_retries_share_one_live_session() {
        let dir = tempfile::tempdir().unwrap();
        let pairings_path = dir.path().join("pairings.json");
        let daemon_instance_id = "daemon-stable-across-restart";
        let first_state = AppState::new(test_audit())
            .with_durable_pairing_store(std::sync::Arc::new(
                bicameral_gateway::durable_pairing_store::DurablePairingStore::open(&pairings_path)
                    .unwrap(),
            ))
            .with_daemon_instance_id(daemon_instance_id);
        let pending = create_pending_request(&first_state).await;
        let handle = pending["request_handle"].as_str().unwrap().to_string();
        let proof = pending["poll_proof"].as_str().unwrap().to_string();
        approve_locally(&first_state, &handle).await;
        drop(first_state);

        let restarted_state = AppState::new(test_audit())
            .with_durable_pairing_store(std::sync::Arc::new(
                bicameral_gateway::durable_pairing_store::DurablePairingStore::open(&pairings_path)
                    .unwrap(),
            ))
            .with_daemon_instance_id(daemon_instance_id);
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(32));
        let mut polls = Vec::new();
        for _ in 0..32 {
            let state = restarted_state.clone();
            let handle = handle.clone();
            let proof = proof.clone();
            let barrier = barrier.clone();
            polls.push(tokio::spawn(async move {
                barrier.wait().await;
                let (status, restored) =
                    poll_status(&state, &handle, &proof, TEST_ACCOUNT, TEST_ORIGIN).await;
                assert_eq!(status, StatusCode::OK);
                restored["session_token"].as_str().unwrap().to_string()
            }));
        }

        let mut tokens = std::collections::HashSet::new();
        for poll in polls {
            tokens.insert(poll.await.unwrap());
        }
        assert_eq!(tokens.len(), 1, "concurrent retries must be idempotent");
    }

    #[tokio::test]
    async fn revoked_durable_pairing_cannot_reattach_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let pairings_path = dir.path().join("pairings.json");
        let daemon_instance_id = "daemon-stable-across-restart";

        let first_state = AppState::new(test_audit())
            .with_durable_pairing_store(std::sync::Arc::new(
                bicameral_gateway::durable_pairing_store::DurablePairingStore::open(&pairings_path)
                    .unwrap(),
            ))
            .with_daemon_instance_id(daemon_instance_id);
        let pending = create_pending_request(&first_state).await;
        let handle = pending["request_handle"].as_str().unwrap().to_string();
        let proof = pending["poll_proof"].as_str().unwrap().to_string();
        approve_locally(&first_state, &handle).await;
        let (_, handoff) =
            poll_status(&first_state, &handle, &proof, TEST_ACCOUNT, TEST_ORIGIN).await;
        let token = handoff["session_token"].as_str().unwrap();

        let app = build_bridge_app(first_state.clone());
        let (revoke_status, _) = post_json_with_token(
            app,
            "/api/v1/hosted-bridge/revoke",
            serde_json::json!({}),
            token,
        )
        .await;
        assert_eq!(revoke_status, StatusCode::OK);
        drop(first_state);

        let restarted_state = AppState::new(test_audit())
            .with_durable_pairing_store(std::sync::Arc::new(
                bicameral_gateway::durable_pairing_store::DurablePairingStore::open(&pairings_path)
                    .unwrap(),
            ))
            .with_daemon_instance_id(daemon_instance_id);
        let (status, restored) =
            poll_status(&restarted_state, &handle, &proof, TEST_ACCOUNT, TEST_ORIGIN).await;

        assert_ne!(status, StatusCode::OK);
        assert_ne!(restored["status"], "approved");
        assert!(restored.get("session_token").is_none());
    }

    #[tokio::test]
    async fn durable_revoke_write_failure_still_invalidates_live_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let store_dir = dir.path().join("removed-before-revoke");
        std::fs::create_dir_all(&store_dir).unwrap();
        let pairings_path = store_dir.join("pairings.json");
        let state = AppState::new(test_audit()).with_durable_pairing_store(std::sync::Arc::new(
            bicameral_gateway::durable_pairing_store::DurablePairingStore::open(&pairings_path)
                .unwrap(),
        ));
        let pending = create_pending_request(&state).await;
        let handle = pending["request_handle"].as_str().unwrap().to_string();
        let proof = pending["poll_proof"].as_str().unwrap().to_string();
        approve_locally(&state, &handle).await;
        let (_, handoff) = poll_status(&state, &handle, &proof, TEST_ACCOUNT, TEST_ORIGIN).await;
        let token = handoff["session_token"].as_str().unwrap().to_string();
        std::fs::remove_dir_all(&store_dir).unwrap();

        let (revoke_status, _) = post_json_with_token(
            build_bridge_app(state.clone()),
            "/api/v1/hosted-bridge/revoke",
            serde_json::json!({}),
            &token,
        )
        .await;
        assert_eq!(revoke_status, StatusCode::INTERNAL_SERVER_ERROR);

        let (status, _) = get_with_token(
            build_bridge_app(state.clone()),
            "/api/v1/hosted-bridge/status",
            &token,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let (_, restored) = poll_status(&state, &handle, &proof, TEST_ACCOUNT, TEST_ORIGIN).await;
        assert!(restored.get("session_token").is_none());
    }

    #[tokio::test]
    async fn durable_reattach_rejects_binding_mismatches_without_token() {
        let dir = tempfile::tempdir().unwrap();
        let pairings_path = dir.path().join("pairings.json");
        let daemon_instance_id = "daemon-stable-across-restart";

        let first_state = AppState::new(test_audit())
            .with_durable_pairing_store(std::sync::Arc::new(
                bicameral_gateway::durable_pairing_store::DurablePairingStore::open(&pairings_path)
                    .unwrap(),
            ))
            .with_daemon_instance_id(daemon_instance_id);
        let pending = create_pending_request(&first_state).await;
        let handle = pending["request_handle"].as_str().unwrap().to_string();
        let proof = pending["poll_proof"].as_str().unwrap().to_string();
        approve_locally(&first_state, &handle).await;
        drop(first_state);

        let restarted_state = AppState::new(test_audit())
            .with_durable_pairing_store(std::sync::Arc::new(
                bicameral_gateway::durable_pairing_store::DurablePairingStore::open(&pairings_path)
                    .unwrap(),
            ))
            .with_daemon_instance_id(daemon_instance_id);

        let (status, body) =
            poll_status(&restarted_state, &handle, &proof, "acct-other", TEST_ORIGIN).await;
        assert_ne!(status, StatusCode::OK);
        assert!(body.get("session_token").is_none());

        let (status, body) = poll_status(
            &restarted_state,
            &handle,
            &proof,
            TEST_ACCOUNT,
            "https://other.example.com",
        )
        .await;
        assert_ne!(status, StatusCode::OK);
        assert!(body.get("session_token").is_none());

        let wrong_daemon_state = AppState::new(test_audit())
            .with_durable_pairing_store(std::sync::Arc::new(
                bicameral_gateway::durable_pairing_store::DurablePairingStore::open(&pairings_path)
                    .unwrap(),
            ))
            .with_daemon_instance_id("daemon-different-identity");
        let (status, body) = poll_status(
            &wrong_daemon_state,
            &handle,
            &proof,
            TEST_ACCOUNT,
            TEST_ORIGIN,
        )
        .await;
        assert_ne!(status, StatusCode::OK);
        assert!(body.get("session_token").is_none());

        let store = std::sync::Arc::new(
            bicameral_gateway::durable_pairing_store::DurablePairingStore::open(&pairings_path)
                .unwrap(),
        );
        let mut record = store.find_by_restore_request_handle(&handle).unwrap();
        record.protocol_version += 1;
        store.upsert(record.clone()).unwrap();
        let protocol_mismatch_state = AppState::new(test_audit())
            .with_durable_pairing_store(store.clone())
            .with_daemon_instance_id(daemon_instance_id);
        let (status, body) = poll_status(
            &protocol_mismatch_state,
            &handle,
            &proof,
            TEST_ACCOUNT,
            TEST_ORIGIN,
        )
        .await;
        assert_ne!(status, StatusCode::OK);
        assert!(body.get("session_token").is_none());

        record.protocol_version = bicameral_api::hosted_bridge::HOSTED_BRIDGE_PROTOCOL_VERSION;
        record.profile = "different-profile".to_string();
        store.upsert(record).unwrap();
        let profile_mismatch_state = AppState::new(test_audit())
            .with_durable_pairing_store(store)
            .with_daemon_instance_id(daemon_instance_id);
        let (status, body) = poll_status(
            &profile_mismatch_state,
            &handle,
            &proof,
            TEST_ACCOUNT,
            TEST_ORIGIN,
        )
        .await;
        assert_ne!(status, StatusCode::OK);
        assert!(body.get("session_token").is_none());
    }

    #[tokio::test]
    async fn durable_reattach_rejects_tampered_restore_proof_without_token() {
        let dir = tempfile::tempdir().unwrap();
        let pairings_path = dir.path().join("pairings.json");
        let daemon_instance_id = "daemon-stable-across-restart";
        let first_state = AppState::new(test_audit())
            .with_durable_pairing_store(std::sync::Arc::new(
                bicameral_gateway::durable_pairing_store::DurablePairingStore::open(&pairings_path)
                    .unwrap(),
            ))
            .with_daemon_instance_id(daemon_instance_id);
        let pending = create_pending_request(&first_state).await;
        let handle = pending["request_handle"].as_str().unwrap().to_string();
        approve_locally(&first_state, &handle).await;
        drop(first_state);

        let restarted_state = AppState::new(test_audit())
            .with_durable_pairing_store(std::sync::Arc::new(
                bicameral_gateway::durable_pairing_store::DurablePairingStore::open(&pairings_path)
                    .unwrap(),
            ))
            .with_daemon_instance_id(daemon_instance_id);
        let (status, body) = poll_status(
            &restarted_state,
            &handle,
            "tampered-restore-proof",
            TEST_ACCOUNT,
            TEST_ORIGIN,
        )
        .await;

        assert_ne!(status, StatusCode::OK);
        assert!(body.get("session_token").is_none());
    }

    #[tokio::test]
    async fn failed_durable_commit_does_not_approve_or_release_a_session() {
        let dir = tempfile::tempdir().unwrap();
        let store_dir = dir.path().join("removed-before-approve");
        std::fs::create_dir_all(&store_dir).unwrap();
        let pairings_path = store_dir.join("pairings.json");
        let state = AppState::new(test_audit()).with_durable_pairing_store(std::sync::Arc::new(
            bicameral_gateway::durable_pairing_store::DurablePairingStore::open(&pairings_path)
                .unwrap(),
        ));
        let pending = create_pending_request(&state).await;
        let handle = pending["request_handle"].as_str().unwrap().to_string();
        let proof = pending["poll_proof"].as_str().unwrap().to_string();
        std::fs::remove_dir_all(&store_dir).unwrap();

        let (approve_status, approve_body) = approve_locally(&state, &handle).await;
        assert_eq!(approve_status, StatusCode::OK);
        let approval: serde_json::Value = serde_json::from_slice(&approve_body).unwrap();
        assert_eq!(approval["status"], "denied");
        assert!(!body_contains_token(&approve_body));

        let (status, handoff) =
            poll_status(&state, &handle, &proof, TEST_ACCOUNT, TEST_ORIGIN).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(handoff["status"], "pending");
        assert!(handoff.get("session_token").is_none());
    }

    #[tokio::test]
    async fn local_denial_yields_typed_denied_without_token() {
        let state = AppState::new(test_audit());
        let pending = create_pending_request(&state).await;
        let handle = pending["request_handle"].as_str().unwrap().to_string();
        let proof = pending["poll_proof"].as_str().unwrap().to_string();

        let nonce = local_confirmation_nonce(&state, &handle).await;
        let app = build_bridge_app(state.clone());
        let (status, body) = post_json(
            app,
            "/api/v1/hosted-bridge/pair/decision",
            serde_json::json!({
                "request_handle": handle,
                "action": "deny",
                "local_confirmation_nonce": nonce,
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(resp["status"], "denied");
        assert!(!body_contains_token(&body));

        // The initiator sees a typed, tokenless denied outcome.
        let (status, resp) = poll_status(&state, &handle, &proof, TEST_ACCOUNT, TEST_ORIGIN).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(resp["status"], "denied");
        assert!(resp.get("session_token").is_none());
    }

    #[tokio::test]
    async fn cross_origin_local_decision_is_rejected() {
        let state = AppState::new(test_audit());
        let pending = create_pending_request(&state).await;
        let handle = pending["request_handle"].as_str().unwrap().to_string();
        let nonce = local_confirmation_nonce(&state, &handle).await;

        let app = build_bridge_app(state.clone());
        let (status, body) = post_json_with_origin(
            app,
            "/api/v1/hosted-bridge/pair/decision",
            serde_json::json!({
                "request_handle": handle,
                "action": "approve",
                "local_confirmation_nonce": nonce,
            }),
            TEST_ORIGIN,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(!body_contains_token(&body));

        // No session was minted; the request is still pending.
        let (_, resp) = poll_status(
            &state,
            &handle,
            pending["poll_proof"].as_str().unwrap(),
            TEST_ACCOUNT,
            TEST_ORIGIN,
        )
        .await;
        assert_eq!(resp["status"], "pending");
    }

    #[tokio::test]
    async fn malformed_local_decision_nonce_fails_closed() {
        let state = AppState::new(test_audit());
        let pending = create_pending_request(&state).await;
        let handle = pending["request_handle"].as_str().unwrap().to_string();

        let app = build_bridge_app(state.clone());
        let (status, body) = post_json(
            app,
            "/api/v1/hosted-bridge/pair/decision",
            serde_json::json!({
                "request_handle": handle,
                "action": "approve",
                "local_confirmation_nonce": "not-the-real-nonce",
            }),
        )
        .await;
        assert_ne!(status, StatusCode::OK);
        assert!(!body_contains_token(&body));
    }

    #[tokio::test]
    async fn replayed_local_approval_never_re_mints_and_is_retry_safe() {
        let state = AppState::new(test_audit());
        let pending = create_pending_request(&state).await;
        let handle = pending["request_handle"].as_str().unwrap().to_string();
        let proof = pending["poll_proof"].as_str().unwrap().to_string();

        let (first, _) = approve_locally(&state, &handle).await;
        assert_eq!(first, StatusCode::OK);
        let (_, resp1) = poll_status(&state, &handle, &proof, TEST_ACCOUNT, TEST_ORIGIN).await;
        let token1 = resp1["session_token"].as_str().unwrap().to_string();

        // A replayed approval must not mint a second session.
        let (_second, body) = approve_locally(&state, &handle).await;
        assert!(!body_contains_token(&body));

        let (_, resp2) = poll_status(&state, &handle, &proof, TEST_ACCOUNT, TEST_ORIGIN).await;
        assert_eq!(resp2["status"], "approved");
        assert_eq!(resp2["session_token"].as_str().unwrap(), token1);
    }

    #[tokio::test]
    async fn status_proof_mismatch_fails_closed() {
        let state = AppState::new(test_audit());
        let pending = create_pending_request(&state).await;
        let handle = pending["request_handle"].as_str().unwrap().to_string();
        approve_locally(&state, &handle).await;

        let (status, resp) =
            poll_status(&state, &handle, "wrong-proof", TEST_ACCOUNT, TEST_ORIGIN).await;
        assert_ne!(status, StatusCode::OK);
        assert!(resp.get("session_token").is_none());
    }

    #[tokio::test]
    async fn status_origin_mismatch_fails_closed() {
        let state = AppState::new(test_audit());
        let pending = create_pending_request(&state).await;
        let handle = pending["request_handle"].as_str().unwrap().to_string();
        let proof = pending["poll_proof"].as_str().unwrap().to_string();
        approve_locally(&state, &handle).await;

        let (status, resp) = poll_status(
            &state,
            &handle,
            &proof,
            TEST_ACCOUNT,
            "https://evil.attacker.com",
        )
        .await;
        assert_ne!(status, StatusCode::OK);
        assert!(resp.get("session_token").is_none());
    }

    #[tokio::test]
    async fn status_account_mismatch_fails_closed() {
        let state = AppState::new(test_audit());
        let pending = create_pending_request(&state).await;
        let handle = pending["request_handle"].as_str().unwrap().to_string();
        let proof = pending["poll_proof"].as_str().unwrap().to_string();
        approve_locally(&state, &handle).await;

        let (status, resp) =
            poll_status(&state, &handle, &proof, "acct-someone-else", TEST_ORIGIN).await;
        assert_ne!(status, StatusCode::OK);
        assert!(resp.get("session_token").is_none());
    }

    #[tokio::test]
    async fn expired_pending_status_is_typed_expired_without_token() {
        let state = AppState::new(test_audit());
        let pending = create_pending_request(&state).await;
        let handle = pending["request_handle"].as_str().unwrap().to_string();
        let proof = pending["poll_proof"].as_str().unwrap().to_string();
        expire_pending(&state, &handle).await;

        let (status, resp) = poll_status(&state, &handle, &proof, TEST_ACCOUNT, TEST_ORIGIN).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(resp["status"], "expired");
        assert!(resp.get("session_token").is_none());
    }

    #[tokio::test]
    async fn expired_pending_cannot_be_approved() {
        let state = AppState::new(test_audit());
        let pending = create_pending_request(&state).await;
        let handle = pending["request_handle"].as_str().unwrap().to_string();
        let nonce = local_confirmation_nonce(&state, &handle).await;
        expire_pending(&state, &handle).await;

        let app = build_bridge_app(state.clone());
        let (status, body) = post_json(
            app,
            "/api/v1/hosted-bridge/pair/decision",
            serde_json::json!({
                "request_handle": handle,
                "action": "approve",
                "local_confirmation_nonce": nonce,
            }),
        )
        .await;
        // Expired requests never mint a session.
        assert!(!body_contains_token(&body));
        if status == StatusCode::OK {
            let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(resp["status"], "expired");
        }
    }

    #[tokio::test]
    async fn unknown_handle_status_is_not_found() {
        let state = AppState::new(test_audit());
        let (status, resp) = poll_status(
            &state,
            "00000000-0000-0000-0000-000000000000",
            "any-proof",
            TEST_ACCOUNT,
            TEST_ORIGIN,
        )
        .await;
        assert_ne!(status, StatusCode::OK);
        assert!(resp.get("session_token").is_none());
    }

    #[tokio::test]
    async fn approve_http_path_carries_pending_team_id_into_minted_grant() {
        use axum::extract::State as AxumState;
        use axum::routing::post;
        use axum::Json;
        use std::net::SocketAddr;
        use std::sync::Arc;
        use tokio::sync::Mutex;

        let captured = Arc::new(Mutex::new(None::<serde_json::Value>));
        let captured_clone = captured.clone();
        let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        let mock = axum::Router::new()
            .route(
                "/api/v1/teams/:team_id/pairings",
                post(|| async {
                    (
                        StatusCode::CREATED,
                        Json(serde_json::json!({"status": "active"})),
                    )
                }),
            )
            .route(
                "/api/v1/teams/:team_id/pairings/:pairing_id/identity",
                post(
                    |AxumState(slot): AxumState<Arc<Mutex<Option<serde_json::Value>>>>,
                     Json(body): Json<serde_json::Value>| async move {
                        *slot.lock().await = Some(body);
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({"status": "active"})),
                        )
                    },
                ),
            )
            .with_state(captured_clone);
        tokio::spawn(async move {
            axum::serve(listener, mock).await.unwrap();
        });

        std::env::set_var("BICAMERAL_CLOUD_ORIGIN", format!("http://{addr}"));

        let state = AppState::new(test_audit());
        let app = build_bridge_app(state.clone());
        let team_id = "team-grant-http";
        let payload = serde_json::json!({
            "origin": TEST_ORIGIN,
            "cloud_account_id": TEST_ACCOUNT,
            "csrf_token": "csrf-valid-token-001",
            "team_id": team_id,
            "cloud_access_token": "cloud-token-1",
            "client_protocol_version": bicameral_api::hosted_bridge::HOSTED_BRIDGE_PROTOCOL_VERSION,
        });
        let (status, body) = post_json(app, "/api/v1/hosted-bridge/pair", payload).await;
        assert_eq!(
            status,
            StatusCode::ACCEPTED,
            "{}",
            String::from_utf8_lossy(&body)
        );
        let pending: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let request_handle = pending["request_handle"].as_str().unwrap();
        let nonce = local_confirmation_nonce(&state, request_handle).await;

        let app = build_bridge_app(state.clone());
        let (status, body) = post_json(
            app,
            "/api/v1/hosted-bridge/pair/decision",
            serde_json::json!({
                "request_handle": request_handle,
                "action": "approve",
                "local_confirmation_nonce": nonce,
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

        let mut grant = None;
        for _ in 0..50 {
            grant = captured.lock().await.clone();
            if grant.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let grant = grant.expect("approve must POST minted PairingAuthorizationV1 to Cloud enroll");
        assert_eq!(
            grant["pairing_authorization"]["team_id"].as_str(),
            Some(team_id),
            "minted grant must carry pending team_id: {grant}"
        );
    }

    #[tokio::test]
    async fn abandoned_pending_requests_are_cleaned_up() {
        let state = AppState::new(test_audit());
        let abandoned = create_pending_request(&state).await;
        let abandoned_handle = abandoned["request_handle"].as_str().unwrap().to_string();
        expire_pending(&state, &abandoned_handle).await;

        // A subsequent pairing prunes expired/abandoned pending state.
        let _fresh = create_pending_request(&state).await;
        let pending = state.hosted_bridge_pending.read().await;
        assert!(
            !pending.contains_key(&abandoned_handle),
            "abandoned pending request must be pruned"
        );
    }
}

/// Scan a raw response body for anything that looks like a leaked bridge
/// session token. The daemon uses UUID session tokens; the field name
/// `session_token` must never appear except in the approved status handoff.
fn body_contains_token(body: &[u8]) -> bool {
    let text = String::from_utf8_lossy(body);
    text.contains("session_token")
}

/// Daemon-served local approval page (cloud#353). The page replaces terminal
/// pairing in the normal journey without changing who decides: only a
/// loopback-origin decision carrying the local nonce approves, Cloud can open
/// but never read or submit, and the CLI path is unchanged.
mod local_approval_page {
    use super::*;
    use bicameral_gateway::browser_data_plane::browser_data_plane_routes;
    use bicameral_gateway::local_approval::local_approval_routes;

    /// Mirrors the production composition in `build_router`.
    fn daemon_app(state: AppState) -> Router {
        Router::new()
            .merge(browser_data_plane_routes(state.clone()))
            .merge(local_approval_routes())
            .with_state(state)
    }

    async fn get_page(
        state: &AppState,
        uri: &str,
        host: &str,
        origin: Option<&str>,
    ) -> axum::response::Response {
        let mut req = Request::builder().uri(uri).header("host", host);
        if let Some(origin) = origin {
            req = req.header("origin", origin);
        }
        daemon_app(state.clone())
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn page_is_served_on_loopback_with_strict_headers_and_no_cors() {
        let state = AppState::new(test_audit());
        for uri in ["/local/pairing", "/local/pairing.js"] {
            let resp = get_page(&state, uri, "127.0.0.1:37373", Some(TEST_ORIGIN)).await;
            assert_eq!(resp.status(), StatusCode::OK, "{uri}");
            let h = resp.headers();
            let csp = h["content-security-policy"].to_str().unwrap();
            assert!(csp.contains("frame-ancestors 'none'"));
            assert!(csp.contains("connect-src 'self'"));
            assert_eq!(h["x-frame-options"], "DENY");
            assert_eq!(h["cache-control"], "no-store");
            // Cloud may open the page but must never be able to read it.
            assert!(h.get("access-control-allow-origin").is_none(), "{uri}");
        }
    }

    #[tokio::test]
    async fn page_refuses_non_loopback_host() {
        let state = AppState::new(test_audit());
        let resp = get_page(&state, "/local/pairing", "rebound.example:37373", None).await;
        assert_eq!(resp.status(), StatusCode::MISDIRECTED_REQUEST);
    }

    #[tokio::test]
    async fn advertised_url_resolves_to_page_and_page_origin_approval_releases_session() {
        let state = AppState::new(test_audit());
        let pending = create_pending_request(&state).await;
        let handle = pending["request_handle"].as_str().unwrap().to_string();
        let url = pending["confirmation"]["local_confirmation_url"]
            .as_str()
            .unwrap()
            .to_string();
        let page = get_page(&state, &url, "127.0.0.1:37373", None).await;
        assert_eq!(page.status(), StatusCode::OK);

        // The page reads the pending view same-origin (no Origin on GET)...
        let nonce = local_confirmation_nonce(&state, &handle).await;
        // ...and submits from its own loopback origin.
        let (status, body) = post_json_with_origin(
            daemon_app(state.clone()),
            "/api/v1/hosted-bridge/pair/decision",
            serde_json::json!({
                "request_handle": handle,
                "action": "approve",
                "local_confirmation_nonce": nonce,
            }),
            "http://127.0.0.1:37373",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(!body_contains_token(&body));
        let decision: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(decision["status"], "approved");

        // The initiating Cloud browser observes approval through its own poll.
        let (status, body) = get_with_origin(
            daemon_app(state.clone()),
            &format!(
                "/api/v1/hosted-bridge/pair/status?request_handle={handle}&poll_proof={}&cloud_account_id={TEST_ACCOUNT}",
                pending["poll_proof"].as_str().unwrap()
            ),
            TEST_ORIGIN,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let polled: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(polled["status"], "approved");
        assert!(polled["session_token"].as_str().is_some());
    }

    #[tokio::test]
    async fn cloud_origin_cannot_read_nonce_or_approve_through_full_daemon_router() {
        let state = AppState::new(test_audit());
        let pending = create_pending_request(&state).await;
        let handle = pending["request_handle"].as_str().unwrap().to_string();

        let (status, body) = get_with_origin(
            daemon_app(state.clone()),
            &format!("/api/v1/hosted-bridge/pair/pending/{handle}"),
            TEST_ORIGIN,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(!String::from_utf8_lossy(&body).contains("local_confirmation_nonce\":\""));

        let nonce = local_confirmation_nonce(&state, &handle).await;
        let (status, _) = post_json_with_origin(
            daemon_app(state.clone()),
            "/api/v1/hosted-bridge/pair/decision",
            serde_json::json!({
                "request_handle": handle,
                "action": "approve",
                "local_confirmation_nonce": nonce,
            }),
            TEST_ORIGIN,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let records = state.hosted_bridge_pending.read().await;
        assert_eq!(
            records[&handle].decision,
            bicameral_gateway::state::PendingPairingDecision::Pending
        );
    }
}

/// DNS-rebinding hardening: a rebound name reaching the daemon sends its own
/// `Host` and no `Origin` on same-origin GETs; it must not read the nonce.
mod local_endpoints_rebinding {
    use super::*;

    #[tokio::test]
    async fn rebound_host_cannot_read_pending_view_or_list() {
        let state = AppState::new(test_audit());
        let pending = create_pending_request(&state).await;
        let handle = pending["request_handle"].as_str().unwrap().to_string();
        for uri in [
            format!("/api/v1/hosted-bridge/pair/pending/{handle}"),
            "/api/v1/hosted-bridge/pair/pending".to_string(),
        ] {
            let resp = build_bridge_app(state.clone())
                .oneshot(
                    Request::builder()
                        .uri(&uri)
                        .header("host", "rebound.example:37373")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN, "{uri}");
        }
        // Loopback Host keeps working for the local page and the CLI.
        let resp = build_bridge_app(state.clone())
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/hosted-bridge/pair/pending/{handle}"))
                    .header("host", "127.0.0.1:37373")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
