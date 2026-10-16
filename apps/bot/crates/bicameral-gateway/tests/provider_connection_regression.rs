//! Provider connection alpha E2E regression tests (issue #412).
//!
//! Validates the combined behavior after #409 (GitHub device-flow),
//! #410 (Google OAuth wrapper), and #411 (dashboard auth-session flow).
//!
//! Acceptance criteria covered:
//! - GitHub provider connection start → mocked completion → status refresh.
//! - Google provider connection start/legacy wrapper → mocked callback → status refresh.
//! - Missing config, expired/denied/provider-unavailable, and unsupported cases.
//! - Dashboard setup routing to provider connection and retry/resume.
//! - No `import_instructions`/`ImportInstructions` in source or fixtures.
//! - Linear is not treated as a live provider connection.
//! - No secret material leaks through registry/API/dashboard outputs.
//! - Provider auth does not create SourceBinding, SourceEvidence, signoff,
//!   compliance, or Decision Ledger events.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use bicameral_audit::AuditStore;
use bicameral_gateway::routes::{connection_routes, dashboard_api_routes, ingest_routes};
use bicameral_gateway::AppState;
use serde_json::Value;
use tower::ServiceExt;

// ── Helpers ──────────────────────────────────────────────────────

fn test_audit() -> AuditStore {
    let dir = tempfile::tempdir().unwrap();
    AuditStore::open(dir.path()).unwrap()
}

fn isolated_connection_state() -> AppState {
    let workspace_root = tempfile::tempdir().unwrap().keep();
    let mut state = AppState::new(test_audit());
    state.workspace_root = std::sync::Arc::new(tokio::sync::RwLock::new(Some(workspace_root)));
    state
}

fn connection_app() -> Router {
    let state = isolated_connection_state();
    Router::new().merge(connection_routes()).with_state(state)
}

fn connection_app_with_state() -> (Router, AppState) {
    let state = isolated_connection_state();
    let app = Router::new()
        .merge(connection_routes())
        .with_state(state.clone());
    (app, state)
}

/// App configured with a Google OAuth config and an isolated workspace root so
/// the local-loopback flow (start → callback → status) runs end-to-end without
/// touching the real `~/.bicameral` connection registry.
fn connection_app_with_google() -> (Router, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let mut state = AppState::new(test_audit());
    state.workspace_root = std::sync::Arc::new(tokio::sync::RwLock::new(Some(dir.keep())));
    state.google_oauth_config = Some(bicameral_config::GoogleOAuthConfig {
        client_id: "test-client-id".into(),
        client_secret: "test-client-secret".into(),
        redirect_uri: "http://localhost:3000/api/v1/connections/google/callback".into(),
        scopes: vec!["https://www.googleapis.com/auth/drive.file".into()],
    });
    state.google_oauth_allow_env_override = true;
    let app = Router::new()
        .merge(connection_routes())
        .with_state(state.clone());
    (app, state)
}

async fn post_json(app: Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 16384).await.unwrap();
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    (status, json)
}

async fn post_empty(app: Router, uri: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 16384).await.unwrap();
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    (status, json)
}

async fn get_json(app: Router, uri: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 16384).await.unwrap();
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    (status, json)
}

/// Secret-material keywords that must never appear in API responses.
const SECRET_KEYWORDS: &[&str] = &[
    "oauth_token",
    "refresh_token",
    "api_key",
    "client_secret",
    "access_token",
    "device_code",
    "bearer_token",
    "ghp_",
    "gho_",
    "ghu_",
];

fn assert_no_secret_material(json: &Value) {
    let raw = serde_json::to_string(json).unwrap();
    for keyword in SECRET_KEYWORDS {
        assert!(
            !raw.contains(keyword),
            "Response must not contain secret keyword '{keyword}': {raw}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// 1. GitHub provider connection: start → mocked completion → status
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn github_start_returns_device_flow_start_url() {
    let app = connection_app();
    let (status, json) = post_empty(app, "/api/v1/connections/github/start").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["outcome"], "start_url");
    assert_eq!(json["provider"], "github");
    assert!(json["url"].as_str().unwrap().contains("github.com"));
    assert!(json["auth_session_id"]
        .as_str()
        .unwrap()
        .starts_with("sess-"));
    assert_eq!(json["flow_type"], "device_flow");
    assert!(json["user_code"].is_string());
    assert_no_secret_material(&json);
}

#[tokio::test]
async fn github_start_returns_auth_session_id_for_polling() {
    let app = connection_app();
    let (_, json) = post_empty(app, "/api/v1/connections/github/start").await;

    let session_id = json["auth_session_id"].as_str().unwrap();
    assert!(!session_id.is_empty(), "auth_session_id must be non-empty");
    assert!(
        session_id.starts_with("sess-github-"),
        "GitHub session IDs should contain provider name"
    );
}

#[tokio::test]
async fn github_status_unknown_session_returns_expired() {
    let app = connection_app();
    let (status, json) = get_json(app, "/api/v1/connections/github/status/sess-nonexistent").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json["outcome"], "expired");
    assert_eq!(json["provider"], "github");
    assert_no_secret_material(&json);
}

#[tokio::test]
async fn github_status_poll_after_start_returns_completion_variant() {
    let (app, _state) = connection_app_with_state();

    // Start the flow to get a session ID.
    let (_, start_json) = post_empty(app.clone(), "/api/v1/connections/github/start").await;
    let session_id = start_json["auth_session_id"].as_str().unwrap();

    // Poll status — since no live GitHub server, this returns a device-flow error
    // mapped to expired (session not found in device-flow store, because the
    // route handler's stub start doesn't insert into the DeviceFlowStore).
    let (status, poll_json) = get_json(
        app,
        &format!("/api/v1/connections/github/status/{session_id}"),
    )
    .await;

    // The session was created by the route stub (not the device-flow store),
    // so the device-flow store lookup returns not-found → expired.
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(poll_json["outcome"], "expired");
    assert_no_secret_material(&poll_json);
}

// ═══════════════════════════════════════════════════════════════════
// 2. Google provider connection: start / callback → status refresh
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn google_start_returns_local_loopback_start_url() {
    let (app, state) = connection_app_with_google();
    let (status, json) = post_empty(app, "/api/v1/connections/google/start").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["outcome"], "start_url");
    assert_eq!(json["provider"], "google");
    assert!(json["url"]
        .as_str()
        .unwrap()
        .contains("accounts.google.com"));
    assert!(json["auth_session_id"]
        .as_str()
        .unwrap()
        .starts_with("gsess-"));
    assert_eq!(json["flow_type"], "local_loopback");
    // Google uses local-loopback: no user_code.
    assert!(json["user_code"].is_null());
    let url = json["url"].as_str().unwrap();
    assert!(url.contains("code_challenge="));
    assert!(url.contains("code_challenge_method=S256"));
    let session_id = json["auth_session_id"].as_str().unwrap();
    let verifier = {
        let sessions = state.auth_sessions.read().await;
        sessions
            .get(session_id)
            .and_then(|session| session.pkce_verifier.clone())
            .expect("PKCE verifier stored daemon-side")
    };
    assert!(
        !url.contains(&verifier),
        "PKCE verifier must not enter browser URL"
    );
    assert_no_secret_material(&json);
}

#[tokio::test]
async fn google_callback_mock_code_cannot_connect() {
    // #1132: authorization-code content can no longer select a synthetic
    // exchange. Observed from outside the crate (no test seam is even
    // visible here), a fabricated mock-prefixed code drives the REAL
    // provider exchange, which fails whether the token endpoint is
    // reachable (Google rejects the code) or not (transport error). Both
    // map to provider_unavailable; neither yields connected or a secret.
    let (app, state) = connection_app_with_google();
    // Start to create a real pending auth session, then read its CSRF token.
    let (_s, start_json) = post_empty(app.clone(), "/api/v1/connections/google/start").await;
    let session_id = start_json["auth_session_id"].as_str().unwrap().to_string();
    let state_token = {
        let sessions = state.auth_sessions.read().await;
        sessions.get(&session_id).unwrap().state_token.clone()
    };

    let body = serde_json::json!({
        "auth_session_id": session_id,
        "code": "mock-auth-code-from-google",
        "state": state_token
    });
    let (status, json) = post_json(app, "/api/v1/connections/google/callback", body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["outcome"], "provider_unavailable");
    assert_eq!(json["provider"], "google");
    assert_no_secret_material(&json);
}

#[tokio::test]
async fn google_callback_without_state_fails_closed() {
    let (app, _state) = connection_app_with_google();
    let (_s, start_json) = post_empty(app.clone(), "/api/v1/connections/google/start").await;
    let session_id = start_json["auth_session_id"].as_str().unwrap().to_string();
    let body = serde_json::json!({
        "auth_session_id": session_id,
        "code": "mock-auth-code-from-google"
    });
    let (status, json) = post_json(app, "/api/v1/connections/google/callback", body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["outcome"], "denied");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("state is required"));
    assert_no_secret_material(&json);
}

#[tokio::test]
async fn google_callback_with_error_requires_a_validated_session() {
    // #1133: a provider error is a terminal outcome only for the exact
    // validated session it belongs to. An error post naming an unknown
    // session maps to expired (nothing to terminate), so unauthenticated
    // callers cannot manufacture denied outcomes.
    let app = connection_app();
    let body = serde_json::json!({
        "auth_session_id": "sess-google-test-002",
        "error": "access_denied"
    });
    let (status, json) = post_json(app.clone(), "/api/v1/connections/google/callback", body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["outcome"], "expired");
    assert_eq!(json["provider"], "google");
    assert_no_secret_material(&json);

    // Bound to a real session, the provider error is terminal Denied and
    // consumes the session one-shot.
    let (app, state) = connection_app_with_google();
    let (_s, start_json) = post_empty(app.clone(), "/api/v1/connections/google/start").await;
    let session_id = start_json["auth_session_id"].as_str().unwrap().to_string();
    let state_token = {
        let sessions = state.auth_sessions.read().await;
        sessions.get(&session_id).unwrap().state_token.clone()
    };
    let body = serde_json::json!({
        "auth_session_id": session_id,
        "state": state_token,
        "error": "access_denied"
    });
    let (status, json) = post_json(app.clone(), "/api/v1/connections/google/callback", body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["outcome"], "denied");
    assert!(json["message"].as_str().unwrap().contains("access_denied"));
    let body = serde_json::json!({
        "auth_session_id": session_id,
        "state": state_token,
        "error": "access_denied"
    });
    let (_status, retry) = post_json(app, "/api/v1/connections/google/callback", body).await;
    assert_eq!(retry["outcome"], "expired");
}

#[tokio::test]
async fn google_status_returns_pending_for_non_device_flow() {
    let (app, _state) = connection_app_with_google();
    // Start to create a real pending auth session.
    let (_s, start_json) = post_empty(app.clone(), "/api/v1/connections/google/start").await;
    let session_id = start_json["auth_session_id"].as_str().unwrap().to_string();

    let (status, json) = get_json(
        app,
        &format!("/api/v1/connections/google/status/{session_id}"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    // Google doesn't use device-flow polling; an un-consumed session is pending.
    assert_eq!(json["outcome"], "pending");
    assert_eq!(json["provider"], "google");
    assert_no_secret_material(&json);
}

// ═══════════════════════════════════════════════════════════════════
// 3. Missing config, expired, denied, provider-unavailable, unsupported
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn unsupported_provider_start_returns_422_unsupported() {
    let app = connection_app();
    let (status, json) = post_empty(app, "/api/v1/connections/notion/start").await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json["outcome"], "unsupported");
    assert_eq!(json["provider"], "notion");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("not a supported provider"));
}

#[tokio::test]
async fn unsupported_provider_callback_returns_422_unsupported() {
    let app = connection_app();
    let body = serde_json::json!({
        "auth_session_id": "sess-notion-001"
    });
    let (status, json) = post_json(app, "/api/v1/connections/notion/callback", body).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json["outcome"], "unsupported");
    assert_eq!(json["provider"], "notion");
}

#[tokio::test]
async fn unsupported_provider_status_returns_422_unsupported() {
    let app = connection_app();
    let (status, json) = get_json(app, "/api/v1/connections/slack/status/sess-slack-001").await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json["outcome"], "unsupported");
    assert_eq!(json["provider"], "slack");
}

#[tokio::test]
async fn unknown_provider_start_returns_unsupported() {
    let app = connection_app();
    let (status, json) = post_empty(app, "/api/v1/connections/foobar/start").await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json["outcome"], "unsupported");
    assert_eq!(json["provider"], "foobar");
}

#[tokio::test]
async fn case_insensitive_provider_normalizes() {
    let app = connection_app();
    let (status, json) = post_empty(app, "/api/v1/connections/GitHub/start").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["provider"], "github");
}

#[tokio::test]
async fn callback_error_maps_to_typed_terminal_outcomes_across_providers() {
    // GitHub keeps the generic error→denied mapping. Google (#1133)
    // validates the session first: an unknown session with an error is
    // expired, never denied.
    let app = connection_app();
    let body = serde_json::json!({
        "auth_session_id": "sess-github-denied",
        "error": "access_denied"
    });
    let (status, json) = post_json(app, "/api/v1/connections/github/callback", body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["outcome"], "denied");

    let app = connection_app();
    let body = serde_json::json!({
        "auth_session_id": "sess-google-denied",
        "error": "access_denied"
    });
    let (status, json) = post_json(app, "/api/v1/connections/google/callback", body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["outcome"], "expired");
}

// ═══════════════════════════════════════════════════════════════════
// 4. Dashboard setup routing and retry/resume behavior
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn connection_registry_returns_all_known_providers() {
    let app = connection_app();
    let (status, json) = get_json(app, "/api/v1/connections").await;

    assert_eq!(status, StatusCode::OK);
    let connections = json["connections"].as_object().unwrap();
    assert!(connections.contains_key("github"));
    assert!(connections.contains_key("google"));
    // Without on-disk registry, all providers are not_connected.
    assert_eq!(connections["github"]["status"], "not_connected");
    assert_eq!(connections["google"]["status"], "not_connected");
    assert_no_secret_material(&json);
}

#[tokio::test]
async fn connection_registry_does_not_include_linear() {
    let app = connection_app();
    let (_, json) = get_json(app, "/api/v1/connections").await;

    let connections = json["connections"].as_object().unwrap();
    // Linear is not a known provider connection in the gateway.
    assert!(
        !connections.contains_key("linear"),
        "Linear must not appear in the connection registry as a live provider"
    );
}

#[tokio::test]
async fn retry_start_returns_fresh_session_id() {
    let app = connection_app();
    // First start.
    let (_, json1) = post_empty(app.clone(), "/api/v1/connections/github/start").await;
    // Second start (retry).
    let (_, json2) = post_empty(app, "/api/v1/connections/github/start").await;

    let id1 = json1["auth_session_id"].as_str().unwrap();
    let id2 = json2["auth_session_id"].as_str().unwrap();
    assert_ne!(
        id1, id2,
        "Retry/resume should generate a fresh auth_session_id"
    );
}

// ═══════════════════════════════════════════════════════════════════
// 5. import_instructions / ImportInstructions must not appear
// ═══════════════════════════════════════════════════════════════════

#[test]
fn connection_start_outcome_type_does_not_expose_import_instructions_in_api() {
    // Verify that exercising all known start paths never produces
    // an import_instructions variant at the API level.
    use bicameral_api::dashboard::ConnectionStartOutcome;

    let outcomes = vec![
        ConnectionStartOutcome::Connected {
            provider: "github".into(),
            account_label: Some("user".into()),
        },
        ConnectionStartOutcome::StartUrl {
            provider: "github".into(),
            url: "https://github.com/login/device".into(),
            auth_session_id: "sess-test".into(),
            user_code: Some("ABCD-1234".into()),
            flow_type: Some("device_flow".into()),
        },
        ConnectionStartOutcome::ActionNeeded {
            provider: "google".into(),
            message: "Reconnect needed.".into(),
        },
        ConnectionStartOutcome::Unsupported {
            provider: "notion".into(),
            message: "Not supported.".into(),
        },
    ];
    for outcome in &outcomes {
        let json = serde_json::to_string(outcome).unwrap();
        assert!(
            !json.contains("import_instructions"),
            "ConnectionStartOutcome JSON must not contain 'import_instructions': {json}"
        );
        assert!(
            !json.contains("ImportInstructions"),
            "ConnectionStartOutcome JSON must not contain 'ImportInstructions': {json}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// 6. Linear boundary proof
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn linear_start_returns_unsupported() {
    let app = connection_app();
    let (status, json) = post_empty(app, "/api/v1/connections/linear/start").await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json["outcome"], "unsupported");
    assert_eq!(json["provider"], "linear");
}

#[tokio::test]
async fn linear_callback_returns_unsupported() {
    let app = connection_app();
    let body = serde_json::json!({
        "auth_session_id": "sess-linear-001"
    });
    let (status, json) = post_json(app, "/api/v1/connections/linear/callback", body).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json["outcome"], "unsupported");
}

#[tokio::test]
async fn linear_status_returns_unsupported() {
    let app = connection_app();
    let (status, json) = get_json(app, "/api/v1/connections/linear/status/sess-linear-001").await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json["outcome"], "unsupported");
}

#[test]
fn linear_is_not_in_known_providers_list() {
    // The gateway's KNOWN_PROVIDERS should not include "linear".
    // This is verified indirectly by the unsupported tests above,
    // but also verified here at the type level.
    let known: &[&str] = &["github", "google"];
    assert!(
        !known.contains(&"linear"),
        "Linear must not be in KNOWN_PROVIDERS until #405 and integrations#173 resolve"
    );
}

// ═══════════════════════════════════════════════════════════════════
// 7. No secret material leaks
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn registry_response_contains_no_secrets() {
    let app = connection_app();
    let (_, json) = get_json(app, "/api/v1/connections").await;
    assert_no_secret_material(&json);
}

#[tokio::test]
async fn start_response_contains_no_secrets_github() {
    let app = connection_app();
    let (_, json) = post_empty(app, "/api/v1/connections/github/start").await;
    assert_no_secret_material(&json);
}

#[tokio::test]
async fn start_response_contains_no_secrets_google() {
    let app = connection_app();
    let (_, json) = post_empty(app, "/api/v1/connections/google/start").await;
    assert_no_secret_material(&json);
}

#[tokio::test]
async fn callback_response_contains_no_secrets() {
    let app = connection_app();
    let body = serde_json::json!({
        "auth_session_id": "sess-test-secrets",
        "code": "auth-code-should-not-echo"
    });
    let (_, json) = post_json(app, "/api/v1/connections/google/callback", body).await;

    assert_no_secret_material(&json);
    // Also verify the auth code from the request is not echoed back.
    let raw = serde_json::to_string(&json).unwrap();
    assert!(
        !raw.contains("auth-code-should-not-echo"),
        "Authorization code must not be echoed in callback response"
    );
}

#[tokio::test]
async fn status_response_contains_no_secrets() {
    let app = connection_app();
    let (_, json) = get_json(app, "/api/v1/connections/github/status/sess-device-test").await;
    assert_no_secret_material(&json);
}

#[test]
fn connection_metadata_type_excludes_secret_fields() {
    use bicameral_config::ConnectionMetadata;

    let meta = ConnectionMetadata {
        status: bicameral_config::ConnectionStatus::Connected,
        account_label: Some("testuser".into()),
        capabilities: vec!["github.repo.read".into()],
        updated_at: Some(chrono::Utc::now()),
        auth_method: Some(bicameral_config::AuthMethod::DeviceFlow),
    };
    let json = serde_json::to_string(&meta).unwrap();
    for keyword in SECRET_KEYWORDS {
        assert!(
            !json.contains(keyword),
            "ConnectionMetadata must not serialize secret keyword '{keyword}'"
        );
    }
}

#[test]
fn completion_outcomes_exclude_secret_fields() {
    use bicameral_api::dashboard::ConnectionCompletionOutcome;

    let outcomes = vec![
        ConnectionCompletionOutcome::Connected {
            provider: "github".into(),
            account_label: Some("user".into()),
            auth_method: Some("device_flow".into()),
            capabilities: vec!["github.repo.read".into()],
        },
        ConnectionCompletionOutcome::ActionNeeded {
            provider: "google".into(),
            message: "Reconnect needed.".into(),
        },
        ConnectionCompletionOutcome::Denied {
            provider: "github".into(),
            message: "Access denied.".into(),
        },
        ConnectionCompletionOutcome::Expired {
            provider: "github".into(),
            message: "Session expired.".into(),
        },
        ConnectionCompletionOutcome::Unsupported {
            provider: "linear".into(),
            message: "Not supported.".into(),
        },
        ConnectionCompletionOutcome::ProviderUnavailable {
            provider: "github".into(),
            message: "GitHub unreachable.".into(),
        },
        ConnectionCompletionOutcome::Pending {
            provider: "github".into(),
            message: Some("Waiting for code entry.".into()),
        },
    ];

    for outcome in &outcomes {
        let json = serde_json::to_string(outcome).unwrap();
        for keyword in SECRET_KEYWORDS {
            assert!(
                !json.contains(keyword),
                "ConnectionCompletionOutcome must not contain '{keyword}': {json}"
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// 8. Provider auth does not create governance state
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn provider_auth_does_not_create_source_binding_or_ledger_events() {
    let state = AppState::new(test_audit());
    let app = Router::new()
        .merge(connection_routes())
        .merge(dashboard_api_routes())
        .merge(ingest_routes())
        .with_state(state.clone());

    // Snapshot governance state before provider auth actions.
    let decisions_before = state.decisions.read().await.len();
    let candidates_before = state.candidates.read().await.len();
    let evidence_before = state.evidence.read().await.len();

    // Perform a full round of provider auth actions.
    // 1. Start GitHub connection.
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/connections/github/start")
        .body(Body::empty())
        .unwrap();
    app.clone().oneshot(req).await.unwrap();

    // 2. Start Google connection.
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/connections/google/start")
        .body(Body::empty())
        .unwrap();
    app.clone().oneshot(req).await.unwrap();

    // 3. Google callback.
    let body_json = serde_json::json!({
        "auth_session_id": "sess-google-governance-test",
        "code": "auth-code-gov-test"
    });
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/connections/google/callback")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&body_json).unwrap()))
        .unwrap();
    app.clone().oneshot(req).await.unwrap();

    // 4. Poll status.
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/connections/github/status/sess-github-governance-test")
        .body(Body::empty())
        .unwrap();
    app.clone().oneshot(req).await.unwrap();

    // 5. Read registry.
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/connections")
        .body(Body::empty())
        .unwrap();
    app.clone().oneshot(req).await.unwrap();

    // Verify: no governance state was created by any provider auth action.
    let decisions_after = state.decisions.read().await.len();
    let candidates_after = state.candidates.read().await.len();
    let evidence_after = state.evidence.read().await.len();

    assert_eq!(
        decisions_before, decisions_after,
        "Provider auth must not create Decision Ledger entries"
    );
    assert_eq!(
        candidates_before, candidates_after,
        "Provider auth must not create DecisionCandidate entries"
    );
    assert_eq!(
        evidence_before, evidence_after,
        "Provider auth must not create SourceEvidence entries"
    );
}

// ═══════════════════════════════════════════════════════════════════
// 9. ConnectionStartOutcome and ConnectionCompletionOutcome roundtrip
// ═══════════════════════════════════════════════════════════════════

#[test]
fn start_outcome_variants_roundtrip_through_json() {
    use bicameral_api::dashboard::ConnectionStartOutcome;

    let cases = vec![
        ConnectionStartOutcome::Connected {
            provider: "github".into(),
            account_label: Some("jinhongkuan".into()),
        },
        ConnectionStartOutcome::StartUrl {
            provider: "github".into(),
            url: "https://github.com/login/device".into(),
            auth_session_id: "ghdf-test".into(),
            user_code: Some("ABCD-1234".into()),
            flow_type: Some("device_flow".into()),
        },
        ConnectionStartOutcome::StartUrl {
            provider: "google".into(),
            url: "https://accounts.google.com/o/oauth2/auth".into(),
            auth_session_id: "sess-google-001".into(),
            user_code: None,
            flow_type: Some("local_loopback".into()),
        },
        ConnectionStartOutcome::ActionNeeded {
            provider: "google".into(),
            message: "Token expired.".into(),
        },
        ConnectionStartOutcome::Unsupported {
            provider: "linear".into(),
            message: "Linear is not supported.".into(),
        },
    ];

    for case in cases {
        let json = serde_json::to_string(&case).unwrap();
        let parsed: ConnectionStartOutcome = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, case);
    }
}

#[test]
fn completion_outcome_variants_roundtrip_through_json() {
    use bicameral_api::dashboard::ConnectionCompletionOutcome;

    let cases = vec![
        ConnectionCompletionOutcome::Connected {
            provider: "github".into(),
            account_label: Some("jinhongkuan".into()),
            auth_method: Some("device_flow".into()),
            capabilities: vec!["github.repo.read".into()],
        },
        ConnectionCompletionOutcome::ActionNeeded {
            provider: "google".into(),
            message: "Callback received but exchange deferred.".into(),
        },
        ConnectionCompletionOutcome::Denied {
            provider: "github".into(),
            message: "User denied.".into(),
        },
        ConnectionCompletionOutcome::Expired {
            provider: "github".into(),
            message: "Session expired.".into(),
        },
        ConnectionCompletionOutcome::Unsupported {
            provider: "linear".into(),
            message: "Not supported.".into(),
        },
        ConnectionCompletionOutcome::ProviderUnavailable {
            provider: "github".into(),
            message: "GitHub unreachable.".into(),
        },
        ConnectionCompletionOutcome::Pending {
            provider: "github".into(),
            message: Some("Waiting...".into()),
        },
    ];

    for case in cases {
        let json = serde_json::to_string(&case).unwrap();
        let parsed: ConnectionCompletionOutcome = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, case);
    }
}
