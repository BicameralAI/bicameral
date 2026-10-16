//! GitHub OAuth device-flow implementation.
//!
//! Implements the GitHub device authorization grant (RFC 8628) through the
//! provider connection flow. The dashboard calls `POST /connections/github/start`
//! to get a `user_code` + `verification_uri`, shows them to the operator, and
//! polls `GET /connections/github/status/:session_id` until the session reaches
//! a terminal state.
//!
//! **Security invariants:**
//! - Access tokens are never included in HTTP responses.
//! - Device codes are held only in the server-side session store.
//! - All token material is persisted through `ConnectionRegistry` + on-disk
//!   secrets file; no token ever appears in logs or API responses.

use bicameral_api::dashboard::{
    AuthSessionId, ConnectionCompletionOutcome, ConnectionStartOutcome,
};
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Environment variable for the GitHub OAuth App client ID.
pub const GITHUB_CLIENT_ID_ENV: &str = "BICAMERAL_GITHUB_CLIENT_ID";

/// Default OAuth scopes requested for GitHub device flow.
const DEFAULT_SCOPES: &str = "repo read:org read:user";

// ── GitHub API response types ────────────────────────────────────

/// GitHub device-flow code response from `POST https://github.com/login/device/code`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// GitHub token response from `POST https://github.com/login/oauth/access_token`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct TokenResponse {
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub error_description: Option<String>,
}

/// Minimal GitHub user info for account_label.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct GitHubUser {
    pub login: String,
}

// ── Session store ────────────────────────────────────────────────

/// In-memory state for one device-flow session.
#[derive(Debug, Clone)]
pub struct DeviceFlowSession {
    pub provider: String,
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub client_id: String,
    pub expires_at: chrono::DateTime<Utc>,
    pub interval_secs: u64,
    pub terminal: Option<ConnectionCompletionOutcome>,
}

/// Thread-safe store for active device-flow sessions.
#[derive(Debug, Clone, Default)]
pub struct DeviceFlowStore {
    sessions: Arc<RwLock<HashMap<AuthSessionId, DeviceFlowSession>>>,
}

impl DeviceFlowStore {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn insert(&self, session_id: AuthSessionId, session: DeviceFlowSession) {
        self.sessions.write().await.insert(session_id, session);
    }

    pub async fn get(&self, session_id: &str) -> Option<DeviceFlowSession> {
        self.sessions.read().await.get(session_id).cloned()
    }

    pub async fn set_terminal(&self, session_id: &str, outcome: ConnectionCompletionOutcome) {
        if let Some(session) = self.sessions.write().await.get_mut(session_id) {
            session.terminal = Some(outcome);
        }
    }
}

// ── Errors ───────────────────────────────────────────────────────

/// Errors from the GitHub device-flow implementation.
#[derive(Debug, thiserror::Error)]
pub enum GitHubDeviceFlowError {
    #[error("GitHub client ID not configured (set {GITHUB_CLIENT_ID_ENV})")]
    ClientIdNotConfigured,
    #[error("HTTP request failed: {0}")]
    HttpError(String),
    #[error("Failed to parse GitHub response: {0}")]
    ParseError(String),
    #[error("Device flow session not found: {0}")]
    SessionNotFound(String),
    #[error("Device flow session expired")]
    SessionExpired,
}

// ── GitHub HTTP helpers ──────────────────────────────────────────

/// Request a device code from GitHub.
pub async fn request_device_code(
    http: &reqwest::Client,
    client_id: &str,
    scope: &str,
    device_code_url: &str,
) -> Result<DeviceCodeResponse, GitHubDeviceFlowError> {
    let resp = http
        .post(device_code_url)
        .header("Accept", "application/json")
        .form(&[("client_id", client_id), ("scope", scope)])
        .send()
        .await
        .map_err(|e| GitHubDeviceFlowError::HttpError(e.to_string()))?;

    let body = resp
        .text()
        .await
        .map_err(|e| GitHubDeviceFlowError::HttpError(e.to_string()))?;

    serde_json::from_str(&body)
        .map_err(|e| GitHubDeviceFlowError::ParseError(format!("{e}: {body}")))
}

/// Poll GitHub for an access token.
pub async fn poll_access_token(
    http: &reqwest::Client,
    client_id: &str,
    device_code: &str,
    token_url: &str,
) -> Result<TokenResponse, GitHubDeviceFlowError> {
    let resp = http
        .post(token_url)
        .header("Accept", "application/json")
        .form(&[
            ("client_id", client_id),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])
        .send()
        .await
        .map_err(|e| GitHubDeviceFlowError::HttpError(e.to_string()))?;

    let body = resp
        .text()
        .await
        .map_err(|e| GitHubDeviceFlowError::HttpError(e.to_string()))?;

    serde_json::from_str(&body)
        .map_err(|e| GitHubDeviceFlowError::ParseError(format!("{e}: {body}")))
}

/// Fetch the authenticated GitHub user's login name.
pub async fn get_authenticated_user(
    http: &reqwest::Client,
    access_token: &str,
    api_url: &str,
) -> Result<GitHubUser, GitHubDeviceFlowError> {
    let resp = http
        .get(api_url)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("User-Agent", "bicameral-bot")
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| GitHubDeviceFlowError::HttpError(e.to_string()))?;

    let body = resp
        .text()
        .await
        .map_err(|e| GitHubDeviceFlowError::HttpError(e.to_string()))?;

    serde_json::from_str(&body)
        .map_err(|e| GitHubDeviceFlowError::ParseError(format!("{e}: {body}")))
}

// ── GitHub endpoint URLs ─────────────────────────────────────────

/// Production GitHub device-code endpoint.
pub const GITHUB_DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
/// Production GitHub token endpoint.
pub const GITHUB_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
/// Production GitHub user endpoint.
pub const GITHUB_USER_URL: &str = "https://api.github.com/user";

/// Configurable endpoints and client identity for GitHub API calls.
///
/// In production, `client_id` is read from the `BICAMERAL_GITHUB_CLIENT_ID`
/// env var. Tests override all fields to point at a mock server.
#[derive(Debug, Clone)]
pub struct GitHubEndpoints {
    pub device_code_url: String,
    pub token_url: String,
    pub user_url: String,
    /// GitHub OAuth App client ID. When `None`, `start_device_flow` reads
    /// from `BICAMERAL_GITHUB_CLIENT_ID`.
    pub client_id: Option<String>,
}

impl Default for GitHubEndpoints {
    fn default() -> Self {
        Self {
            device_code_url: GITHUB_DEVICE_CODE_URL.into(),
            token_url: GITHUB_TOKEN_URL.into(),
            user_url: GITHUB_USER_URL.into(),
            client_id: None,
        }
    }
}

impl GitHubEndpoints {
    /// Resolve the client ID: explicit field, then env var.
    pub fn resolve_client_id(&self) -> Result<String, GitHubDeviceFlowError> {
        if let Some(ref id) = self.client_id {
            return Ok(id.clone());
        }
        std::env::var(GITHUB_CLIENT_ID_ENV)
            .map_err(|_| GitHubDeviceFlowError::ClientIdNotConfigured)
    }
}

// ── Device flow orchestration ────────────────────────────────────

/// Initiate a GitHub device-flow session.
///
/// Returns `ConnectionStartOutcome::StartUrl` with the verification URI
/// and user code for the operator to complete in their browser.
pub async fn start_device_flow(
    store: &DeviceFlowStore,
    http: &reqwest::Client,
    endpoints: &GitHubEndpoints,
) -> Result<ConnectionStartOutcome, GitHubDeviceFlowError> {
    let client_id = endpoints.resolve_client_id()?;

    let device_resp =
        request_device_code(http, &client_id, DEFAULT_SCOPES, &endpoints.device_code_url).await?;

    let session_id: AuthSessionId = format!("ghdf-{}", uuid::Uuid::new_v4());
    let expires_at = Utc::now() + chrono::Duration::seconds(device_resp.expires_in as i64);

    let session = DeviceFlowSession {
        provider: "github".into(),
        device_code: device_resp.device_code,
        user_code: device_resp.user_code.clone(),
        verification_uri: device_resp.verification_uri.clone(),
        client_id,
        expires_at,
        interval_secs: device_resp.interval,
        terminal: None,
    };

    store.insert(session_id.clone(), session).await;

    Ok(ConnectionStartOutcome::StartUrl {
        provider: "github".into(),
        url: device_resp.verification_uri,
        auth_session_id: session_id,
        user_code: Some(device_resp.user_code),
        flow_type: Some("device_flow".into()),
    })
}

/// Poll a GitHub device-flow session.
///
/// On success, persists the token to the connection secrets file and updates
/// the `ConnectionRegistry`. No token material appears in the returned outcome.
pub async fn poll_device_flow(
    store: &DeviceFlowStore,
    http: &reqwest::Client,
    endpoints: &GitHubEndpoints,
    session_id: &str,
) -> Result<ConnectionCompletionOutcome, GitHubDeviceFlowError> {
    let session = store
        .get(session_id)
        .await
        .ok_or_else(|| GitHubDeviceFlowError::SessionNotFound(session_id.into()))?;

    // Return cached terminal state if already resolved.
    if let Some(ref terminal) = session.terminal {
        return Ok(terminal.clone());
    }

    // Check server-side expiry.
    if Utc::now() >= session.expires_at {
        let outcome = ConnectionCompletionOutcome::Expired {
            provider: "github".into(),
            message: "Device-flow session expired. Please start a new connection.".into(),
        };
        store.set_terminal(session_id, outcome.clone()).await;
        return Ok(outcome);
    }

    // Poll GitHub for token.
    let token_resp = poll_access_token(
        http,
        &session.client_id,
        &session.device_code,
        &endpoints.token_url,
    )
    .await?;

    // Handle GitHub error codes (RFC 8628 §3.5).
    if let Some(ref error) = token_resp.error {
        return match error.as_str() {
            "authorization_pending" => Ok(ConnectionCompletionOutcome::Pending {
                provider: "github".into(),
                message: Some("Waiting for user to enter the code on GitHub.".into()),
            }),
            "slow_down" => Ok(ConnectionCompletionOutcome::Pending {
                provider: "github".into(),
                message: Some(
                    "Polling too fast. The dashboard should increase the interval.".into(),
                ),
            }),
            "expired_token" => {
                let outcome = ConnectionCompletionOutcome::Expired {
                    provider: "github".into(),
                    message: "The device code has expired. Please start a new connection.".into(),
                };
                store.set_terminal(session_id, outcome.clone()).await;
                Ok(outcome)
            }
            "access_denied" => {
                let outcome = ConnectionCompletionOutcome::Denied {
                    provider: "github".into(),
                    message: "The user denied the authorization request.".into(),
                };
                store.set_terminal(session_id, outcome.clone()).await;
                Ok(outcome)
            }
            other => {
                let outcome = ConnectionCompletionOutcome::ProviderUnavailable {
                    provider: "github".into(),
                    message: format!(
                        "GitHub returned an unexpected error: {other}{}",
                        token_resp
                            .error_description
                            .as_deref()
                            .map(|d| format!(" — {d}"))
                            .unwrap_or_default()
                    ),
                };
                store.set_terminal(session_id, outcome.clone()).await;
                Ok(outcome)
            }
        };
    }

    // Success: we received an access token.
    if let Some(ref access_token) = token_resp.access_token {
        let account_label = get_authenticated_user(http, access_token, &endpoints.user_url)
            .await
            .map(|u| u.login)
            .ok();

        let scopes: Vec<String> = token_resp
            .scope
            .as_deref()
            .unwrap_or("")
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| format!("github.{}", s.trim()))
            .collect();

        persist_github_token(access_token)?;
        update_github_registry(account_label.as_deref(), &scopes)?;

        let outcome = ConnectionCompletionOutcome::Connected {
            provider: "github".into(),
            account_label,
            auth_method: Some("device_flow".into()),
            capabilities: scopes,
        };
        store.set_terminal(session_id, outcome.clone()).await;
        Ok(outcome)
    } else {
        Ok(ConnectionCompletionOutcome::Pending {
            provider: "github".into(),
            message: Some("Unexpected response from GitHub (no token, no error).".into()),
        })
    }
}

// ── Persistence helpers ──────────────────────────────────────────

/// Persist the GitHub access token to the operator-level secrets file.
///
/// Writes to `~/.bicameral/connection-secrets.json`. The file stores
/// `{ "connection.github.oauth_token": "<token>" }`. Token material
/// is never logged or returned in API responses.
fn persist_github_token(token: &str) -> Result<(), GitHubDeviceFlowError> {
    let secrets_path = bicameral_config::BicameralPaths::operator_home()
        .map(|h| h.join("connection-secrets.json"))
        .ok_or_else(|| {
            GitHubDeviceFlowError::HttpError("Cannot resolve operator home directory".into())
        })?;

    let mut secrets: HashMap<String, String> = if secrets_path.exists() {
        let content = std::fs::read_to_string(&secrets_path)
            .map_err(|e| GitHubDeviceFlowError::HttpError(format!("Read secrets: {e}")))?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        HashMap::new()
    };

    secrets.insert("connection.github.oauth_token".into(), token.into());

    if let Some(parent) = secrets_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| GitHubDeviceFlowError::HttpError(format!("Create dir: {e}")))?;
    }

    let content = serde_json::to_string_pretty(&secrets)
        .map_err(|e| GitHubDeviceFlowError::HttpError(format!("Serialize secrets: {e}")))?;
    std::fs::write(&secrets_path, content)
        .map_err(|e| GitHubDeviceFlowError::HttpError(format!("Write secrets: {e}")))?;

    Ok(())
}

/// Update the `ConnectionRegistry` on disk with GitHub connection metadata.
fn update_github_registry(
    account_label: Option<&str>,
    capabilities: &[String],
) -> Result<(), GitHubDeviceFlowError> {
    let registry_path = bicameral_config::BicameralPaths::connections_file().ok_or_else(|| {
        GitHubDeviceFlowError::HttpError("Cannot resolve connections file path".into())
    })?;

    let mut registry =
        bicameral_config::ConnectionRegistry::load_from_file(&registry_path).unwrap_or_default();

    registry.upsert(
        "github",
        bicameral_config::ConnectionMetadata {
            status: bicameral_config::ConnectionStatus::Connected,
            account_label: account_label.map(String::from),
            capabilities: capabilities.to_vec(),
            updated_at: Some(Utc::now()),
            auth_method: Some(bicameral_config::AuthMethod::DeviceFlow),
        },
    );

    registry
        .save_to_file(&registry_path)
        .map_err(|e| GitHubDeviceFlowError::HttpError(format!("Save registry: {e}")))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use tokio::net::TcpListener;

    /// Start a mock HTTP server that responds to GitHub device-flow endpoints.
    async fn start_mock_server(
        device_code_body: serde_json::Value,
        token_body: serde_json::Value,
        user_body: serde_json::Value,
    ) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let dc = Arc::new(device_code_body);
        let tk = Arc::new(token_body);
        let usr = Arc::new(user_body);

        let handle = tokio::spawn(async move {
            let dc = dc.clone();
            let tk = tk.clone();
            let usr = usr.clone();

            let make_svc = axum::Router::new()
                .route(
                    "/login/device/code",
                    axum::routing::post({
                        let dc = dc.clone();
                        move || {
                            let dc = dc.clone();
                            async move { axum::Json(dc.as_ref().clone()) }
                        }
                    }),
                )
                .route(
                    "/login/oauth/access_token",
                    axum::routing::post({
                        let tk = tk.clone();
                        move || {
                            let tk = tk.clone();
                            async move { axum::Json(tk.as_ref().clone()) }
                        }
                    }),
                )
                .route(
                    "/user",
                    axum::routing::get({
                        let usr = usr.clone();
                        move || {
                            let usr = usr.clone();
                            async move { axum::Json(usr.as_ref().clone()) }
                        }
                    }),
                );

            axum::serve(listener, make_svc).await.unwrap();
        });

        (addr, handle)
    }

    fn mock_endpoints(addr: SocketAddr) -> GitHubEndpoints {
        GitHubEndpoints {
            device_code_url: format!("http://{addr}/login/device/code"),
            token_url: format!("http://{addr}/login/oauth/access_token"),
            user_url: format!("http://{addr}/user"),
            client_id: Some("test-client-id".into()),
        }
    }

    #[tokio::test]
    async fn start_device_flow_returns_start_url() {
        let device_code_body = serde_json::json!({
            "device_code": "test-device-code",
            "user_code": "ABCD-1234",
            "verification_uri": "https://github.com/login/device",
            "expires_in": 900,
            "interval": 5
        });
        let (addr, _handle) = start_mock_server(
            device_code_body,
            serde_json::json!({}),
            serde_json::json!({}),
        )
        .await;

        let store = DeviceFlowStore::new();
        let http = reqwest::Client::new();
        let endpoints = mock_endpoints(addr);

        let result = start_device_flow(&store, &http, &endpoints).await.unwrap();

        match result {
            ConnectionStartOutcome::StartUrl {
                provider,
                url,
                auth_session_id,
                user_code,
                flow_type,
            } => {
                assert_eq!(provider, "github");
                assert_eq!(url, "https://github.com/login/device");
                assert!(auth_session_id.starts_with("ghdf-"));
                assert_eq!(user_code.as_deref(), Some("ABCD-1234"));
                assert_eq!(flow_type.as_deref(), Some("device_flow"));
                assert!(store.get(&auth_session_id).await.is_some());
            }
            other => panic!("Expected StartUrl, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn start_device_flow_fails_without_client_id() {
        let store = DeviceFlowStore::new();
        let http = reqwest::Client::new();
        // No client_id in endpoints and env var not set → ClientIdNotConfigured.
        let endpoints = GitHubEndpoints {
            client_id: None,
            ..GitHubEndpoints::default()
        };

        let result = start_device_flow(&store, &http, &endpoints).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GitHubDeviceFlowError::ClientIdNotConfigured
        ));
    }

    #[tokio::test]
    async fn poll_returns_pending_when_authorization_pending() {
        let device_code_body = serde_json::json!({
            "device_code": "dc-pending",
            "user_code": "PEND-0001",
            "verification_uri": "https://github.com/login/device",
            "expires_in": 900,
            "interval": 5
        });
        let token_body = serde_json::json!({
            "error": "authorization_pending",
            "error_description": "The authorization request is still pending."
        });
        let (addr, _handle) =
            start_mock_server(device_code_body, token_body, serde_json::json!({})).await;

        let store = DeviceFlowStore::new();
        let http = reqwest::Client::new();
        let endpoints = mock_endpoints(addr);

        let outcome = start_device_flow(&store, &http, &endpoints).await.unwrap();
        let session_id = match outcome {
            ConnectionStartOutcome::StartUrl {
                auth_session_id, ..
            } => auth_session_id,
            _ => panic!("Expected StartUrl"),
        };

        let poll_result = poll_device_flow(&store, &http, &endpoints, &session_id)
            .await
            .unwrap();
        match poll_result {
            ConnectionCompletionOutcome::Pending { provider, .. } => {
                assert_eq!(provider, "github");
            }
            other => panic!("Expected Pending, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn poll_returns_denied_when_access_denied() {
        let device_code_body = serde_json::json!({
            "device_code": "dc-denied",
            "user_code": "DENY-0001",
            "verification_uri": "https://github.com/login/device",
            "expires_in": 900,
            "interval": 5
        });
        let token_body = serde_json::json!({
            "error": "access_denied",
            "error_description": "The user has denied your application access."
        });
        let (addr, _handle) =
            start_mock_server(device_code_body, token_body, serde_json::json!({})).await;

        let store = DeviceFlowStore::new();
        let http = reqwest::Client::new();
        let endpoints = mock_endpoints(addr);

        let outcome = start_device_flow(&store, &http, &endpoints).await.unwrap();
        let session_id = match outcome {
            ConnectionStartOutcome::StartUrl {
                auth_session_id, ..
            } => auth_session_id,
            _ => panic!("Expected StartUrl"),
        };

        let poll_result = poll_device_flow(&store, &http, &endpoints, &session_id)
            .await
            .unwrap();
        match poll_result {
            ConnectionCompletionOutcome::Denied {
                provider, message, ..
            } => {
                assert_eq!(provider, "github");
                assert!(message.contains("denied"));
            }
            other => panic!("Expected Denied, got {other:?}"),
        }

        // Subsequent polls return cached terminal state.
        let poll_again = poll_device_flow(&store, &http, &endpoints, &session_id)
            .await
            .unwrap();
        assert!(matches!(
            poll_again,
            ConnectionCompletionOutcome::Denied { .. }
        ));
    }

    #[tokio::test]
    async fn poll_returns_expired_when_token_expired() {
        let device_code_body = serde_json::json!({
            "device_code": "dc-expired",
            "user_code": "EXPR-0001",
            "verification_uri": "https://github.com/login/device",
            "expires_in": 900,
            "interval": 5
        });
        let token_body = serde_json::json!({
            "error": "expired_token",
            "error_description": "The device code has expired."
        });
        let (addr, _handle) =
            start_mock_server(device_code_body, token_body, serde_json::json!({})).await;

        let store = DeviceFlowStore::new();
        let http = reqwest::Client::new();
        let endpoints = mock_endpoints(addr);

        let outcome = start_device_flow(&store, &http, &endpoints).await.unwrap();
        let session_id = match outcome {
            ConnectionStartOutcome::StartUrl {
                auth_session_id, ..
            } => auth_session_id,
            _ => panic!("Expected StartUrl"),
        };

        let poll_result = poll_device_flow(&store, &http, &endpoints, &session_id)
            .await
            .unwrap();
        match poll_result {
            ConnectionCompletionOutcome::Expired {
                provider, message, ..
            } => {
                assert_eq!(provider, "github");
                assert!(message.contains("expired"));
            }
            other => panic!("Expected Expired, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn poll_returns_connected_on_success() {
        let device_code_body = serde_json::json!({
            "device_code": "dc-success",
            "user_code": "SUCC-0001",
            "verification_uri": "https://github.com/login/device",
            "expires_in": 900,
            "interval": 5
        });
        let token_body = serde_json::json!({
            "access_token": "gho_test_token_abc123",
            "token_type": "bearer",
            "scope": "repo,read:org,read:user"
        });
        let user_body = serde_json::json!({
            "login": "testuser"
        });
        let (addr, _handle) = start_mock_server(device_code_body, token_body, user_body).await;

        let tmp = tempfile::tempdir().unwrap();
        // Point HOME at temp dir so persist helpers write there.
        std::env::set_var("HOME", tmp.path());

        let store = DeviceFlowStore::new();
        let http = reqwest::Client::new();
        let endpoints = mock_endpoints(addr);

        let outcome = start_device_flow(&store, &http, &endpoints).await.unwrap();
        let session_id = match outcome {
            ConnectionStartOutcome::StartUrl {
                auth_session_id, ..
            } => auth_session_id,
            _ => panic!("Expected StartUrl"),
        };

        let poll_result = poll_device_flow(&store, &http, &endpoints, &session_id)
            .await
            .unwrap();
        match poll_result {
            ConnectionCompletionOutcome::Connected {
                provider,
                account_label,
                auth_method,
                capabilities,
            } => {
                assert_eq!(provider, "github");
                assert_eq!(account_label.as_deref(), Some("testuser"));
                assert_eq!(auth_method.as_deref(), Some("device_flow"));
                assert!(!capabilities.is_empty());
            }
            other => panic!("Expected Connected, got {other:?}"),
        }

        // Verify registry was updated.
        let registry_path = tmp.path().join(".bicameral").join("connections.yaml");
        assert!(registry_path.exists(), "Registry file should be created");
        let registry =
            bicameral_config::ConnectionRegistry::load_from_file(&registry_path).unwrap();
        assert_eq!(
            registry.status("github"),
            bicameral_config::ConnectionStatus::Connected
        );

        // Verify secrets file was created.
        let secrets_path = tmp
            .path()
            .join(".bicameral")
            .join("connection-secrets.json");
        assert!(secrets_path.exists(), "Secrets file should be created");
        let secrets_content = std::fs::read_to_string(&secrets_path).unwrap();
        assert!(secrets_content.contains("connection.github.oauth_token"));
    }

    #[tokio::test]
    async fn poll_returns_not_found_for_unknown_session() {
        let store = DeviceFlowStore::new();
        let http = reqwest::Client::new();
        let endpoints = GitHubEndpoints::default();

        let result = poll_device_flow(&store, &http, &endpoints, "nonexistent-session").await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GitHubDeviceFlowError::SessionNotFound(_)
        ));
    }

    #[tokio::test]
    async fn poll_returns_expired_when_session_clock_expired() {
        let device_code_body = serde_json::json!({
            "device_code": "dc-clock-expired",
            "user_code": "CLKX-0001",
            "verification_uri": "https://github.com/login/device",
            "expires_in": 900,
            "interval": 5
        });
        let (addr, _handle) = start_mock_server(
            device_code_body,
            serde_json::json!({}),
            serde_json::json!({}),
        )
        .await;

        let store = DeviceFlowStore::new();
        let http = reqwest::Client::new();
        let endpoints = mock_endpoints(addr);

        let outcome = start_device_flow(&store, &http, &endpoints).await.unwrap();
        let session_id = match outcome {
            ConnectionStartOutcome::StartUrl {
                auth_session_id, ..
            } => auth_session_id,
            _ => panic!("Expected StartUrl"),
        };

        // Manually expire the session.
        {
            let mut sessions = store.sessions.write().await;
            if let Some(session) = sessions.get_mut(&session_id) {
                session.expires_at = Utc::now() - chrono::Duration::seconds(1);
            }
        }

        let poll_result = poll_device_flow(&store, &http, &endpoints, &session_id)
            .await
            .unwrap();
        match poll_result {
            ConnectionCompletionOutcome::Expired { provider, .. } => {
                assert_eq!(provider, "github");
            }
            other => panic!("Expected Expired, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn poll_provider_unavailable_on_unexpected_error() {
        let device_code_body = serde_json::json!({
            "device_code": "dc-unavail",
            "user_code": "UNAV-0001",
            "verification_uri": "https://github.com/login/device",
            "expires_in": 900,
            "interval": 5
        });
        let token_body = serde_json::json!({
            "error": "server_error",
            "error_description": "Something went wrong on GitHub's end."
        });
        let (addr, _handle) =
            start_mock_server(device_code_body, token_body, serde_json::json!({})).await;

        let store = DeviceFlowStore::new();
        let http = reqwest::Client::new();
        let endpoints = mock_endpoints(addr);

        let outcome = start_device_flow(&store, &http, &endpoints).await.unwrap();
        let session_id = match outcome {
            ConnectionStartOutcome::StartUrl {
                auth_session_id, ..
            } => auth_session_id,
            _ => panic!("Expected StartUrl"),
        };

        let poll_result = poll_device_flow(&store, &http, &endpoints, &session_id)
            .await
            .unwrap();
        match poll_result {
            ConnectionCompletionOutcome::ProviderUnavailable {
                provider, message, ..
            } => {
                assert_eq!(provider, "github");
                assert!(message.contains("server_error"));
            }
            other => panic!("Expected ProviderUnavailable, got {other:?}"),
        }
    }

    #[test]
    fn no_secret_in_start_url_response() {
        let outcome = ConnectionStartOutcome::StartUrl {
            provider: "github".into(),
            url: "https://github.com/login/device".into(),
            auth_session_id: "ghdf-test".into(),
            user_code: Some("ABCD-1234".into()),
            flow_type: Some("device_flow".into()),
        };

        let json = serde_json::to_string(&outcome).unwrap();
        for secret_key in &[
            "oauth_token",
            "refresh_token",
            "api_key",
            "client_secret",
            "device_code",
            "access_token",
        ] {
            assert!(
                !json.contains(secret_key),
                "StartUrl response must not contain: {secret_key}"
            );
        }
    }

    #[test]
    fn no_secret_in_connected_outcome() {
        let outcome = ConnectionCompletionOutcome::Connected {
            provider: "github".into(),
            account_label: Some("testuser".into()),
            auth_method: Some("device_flow".into()),
            capabilities: vec!["github.repo".into()],
        };

        let json = serde_json::to_string(&outcome).unwrap();
        for secret_key in &[
            "oauth_token",
            "refresh_token",
            "api_key",
            "client_secret",
            "device_code",
            "gho_",
        ] {
            assert!(
                !json.contains(secret_key),
                "Connected outcome must not contain: {secret_key}"
            );
        }
    }
}
