//! Authenticated Google Desktop client-credential bootstrap (bot#1211/cloud#309).
//!
//! The hosted SPA may hand the local daemon its existing signed-in Cloud bearer
//! only on `POST /api/v1/connections/google/start`. This middleware consumes
//! that bearer before the legacy connection handler runs, lets the handler bind
//! the OAuth attempt to the exact public Cloud app config, then performs one
//! authenticated credential fetch against that same Cloud origin. The bearer is
//! never stored. Only a versioned client-credential envelope is persisted in the
//! daemon's configured SecretsStore (OS keyring in production).

use crate::state::AppState;
use axum::body::{to_bytes, Body};
use axum::extract::State;
use axum::http::{Method, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use bicameral_api::dashboard::ConnectionStartOutcome;
use bicameral_config::{GoogleClientCredentialEnvelopeV1, GOOGLE_CLIENT_CREDENTIAL_KEY};
use chrono::Utc;
use serde::Deserialize;
use std::collections::HashSet;
use std::time::Duration;

const GOOGLE_START_PATH: &str = "/api/v1/connections/google/start";
const GOOGLE_CREDENTIAL_PATH: &str = "/api/v1/provider-app-credential/google";
const DEFAULT_CLOUD_ORIGIN: &str = "https://app.bicameral-ai.com";
const BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_BOOTSTRAP_BODY: usize = 16 * 1024;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct GoogleCredentialBootstrapRequest {
    #[serde(default)]
    cloud_access_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GoogleCredentialResponseV1 {
    schema_version: u32,
    provider: String,
    client_type: String,
    client_id: String,
    environment: String,
    config_version: String,
    client_secret: String,
}

impl std::fmt::Debug for GoogleCredentialResponseV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoogleCredentialResponseV1")
            .field("schema_version", &self.schema_version)
            .field("provider", &self.provider)
            .field("client_type", &self.client_type)
            .field("client_id", &self.client_id)
            .field("environment", &self.environment)
            .field("config_version", &self.config_version)
            .field("client_secret", &"[REDACTED]")
            .finish()
    }
}

fn action_needed(message: impl Into<String>) -> Response {
    (
        StatusCode::OK,
        Json(ConnectionStartOutcome::ActionNeeded {
            provider: "google".to_string(),
            message: message.into(),
        }),
    )
        .into_response()
}

fn cloud_origin(state: &AppState) -> String {
    #[cfg(test)]
    if let Some(origin) = state.google_cloud_origin_override.clone() {
        return origin;
    }
    let _ = state;
    std::env::var("BICAMERAL_CLOUD_ORIGIN").unwrap_or_else(|_| DEFAULT_CLOUD_ORIGIN.into())
}

fn explicit_lab_override_ready(state: &AppState) -> bool {
    state.google_oauth_allow_env_override
        && state.google_oauth_config.as_ref().is_some_and(|config| {
            !config.client_id.trim().is_empty() && !config.client_secret.trim().is_empty()
        })
}

async fn read_bootstrap_request(
    request: Request<Body>,
) -> Result<(Request<Body>, GoogleCredentialBootstrapRequest), String> {
    let (parts, body) = request.into_parts();
    let bytes = to_bytes(body, MAX_BOOTSTRAP_BODY)
        .await
        .map_err(|_| "Google credential bootstrap request was unreadable.".to_string())?;
    let bootstrap = if bytes.is_empty() {
        GoogleCredentialBootstrapRequest::default()
    } else {
        serde_json::from_slice::<GoogleCredentialBootstrapRequest>(&bytes).map_err(|_| {
            "Google credential bootstrap request was invalid. Reopen the hosted dashboard and try again."
                .to_string()
        })?
    };
    Ok((Request::from_parts(parts, Body::empty()), bootstrap))
}

async fn fetch_bound_credential(
    state: &AppState,
    cloud_access_token: &str,
) -> Result<GoogleCredentialResponseV1, String> {
    let public = state
        .google_app_config_cache
        .read()
        .await
        .clone()
        .ok_or_else(|| {
            "Google public app configuration was not available for credential binding.".to_string()
        })?;

    let url = format!(
        "{}{}",
        cloud_origin(state).trim_end_matches('/'),
        GOOGLE_CREDENTIAL_PATH
    );
    let client = reqwest::Client::builder()
        .timeout(BOOTSTRAP_TIMEOUT)
        .build()
        .map_err(|_| {
            "Google credential bootstrap transport could not be initialized.".to_string()
        })?;
    let response = client
        .get(url)
        .bearer_auth(cloud_access_token)
        .send()
        .await
        .map_err(|_| {
            "Could not reach the authenticated Google credential bootstrap endpoint.".to_string()
        })?;

    if !response.status().is_success() {
        return Err(match response.status() {
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
                "Cloud session is no longer authorized for Google credential bootstrap. Reopen the hosted dashboard and sign in again.".to_string()
            }
            reqwest::StatusCode::SERVICE_UNAVAILABLE => {
                "Google Desktop credential is not provisioned in the production secret boundary yet.".to_string()
            }
            status => format!(
                "Google credential bootstrap was rejected by Cloud with HTTP {}.",
                status.as_u16()
            ),
        });
    }

    let credential = response
        .json::<GoogleCredentialResponseV1>()
        .await
        .map_err(|_| {
            "Cloud returned a malformed Google credential bootstrap response.".to_string()
        })?;

    if credential.schema_version != 1
        || credential.provider != "google"
        || credential.client_type != "native"
    {
        return Err(
            "Cloud returned an unsupported Google credential bootstrap contract.".to_string(),
        );
    }
    if credential.client_secret.trim().is_empty() {
        return Err("Cloud returned an empty Google Desktop credential.".to_string());
    }
    if credential.client_id != public.client_id
        || credential.environment != public.environment
        || credential.config_version != public.config_version
    {
        return Err(
            "Google credential binding does not match the resolved public app configuration. Reconnect after configuration rotation completes."
                .to_string(),
        );
    }

    Ok(credential)
}

fn persist_credential(
    state: &AppState,
    credential: &GoogleCredentialResponseV1,
) -> Result<(), String> {
    let envelope = GoogleClientCredentialEnvelopeV1::new(
        credential.client_secret.clone(),
        credential.client_id.clone(),
        credential.config_version.clone(),
        Utc::now(),
    )
    .map_err(|_| "Google Desktop credential envelope could not be prepared.".to_string())?;
    let value = envelope
        .to_secret_string()
        .map_err(|_| "Google Desktop credential envelope could not be serialized.".to_string())?;
    state
        .secrets_store
        .write_secret(
            bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
            GOOGLE_CLIENT_CREDENTIAL_KEY,
            &value,
        )
        .map_err(|_| "Google Desktop credential could not be stored in the OS keyring.".to_string())
}

async fn remove_auth_sessions(state: &AppState, session_ids: &[String]) {
    let mut sessions = state.auth_sessions.write().await;
    for session_id in session_ids {
        sessions.remove(session_id);
    }
}

pub async fn google_client_credential_bootstrap_middleware(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if request.method() != Method::POST || request.uri().path() != GOOGLE_START_PATH {
        return next.run(request).await;
    }

    let (request, bootstrap) = match read_bootstrap_request(request).await {
        Ok(value) => value,
        Err(message) => return action_needed(message),
    };
    let cloud_access_token = bootstrap
        .cloud_access_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if cloud_access_token.is_none() && !explicit_lab_override_ready(&state) {
        return action_needed(
            "Cloud session is required to obtain the Google Desktop credential. Reopen the hosted dashboard and sign in again.",
        );
    }

    let Some(cloud_access_token) = cloud_access_token else {
        return next.run(request).await;
    };

    let before: HashSet<String> = state.auth_sessions.read().await.keys().cloned().collect();
    let response = next.run(request).await;
    if !response.status().is_success() {
        return response;
    }

    let new_google_sessions: Vec<String> = state
        .auth_sessions
        .read()
        .await
        .iter()
        .filter(|(id, session)| !before.contains(id.as_str()) && session.provider == "google")
        .map(|(id, _)| id.clone())
        .collect();

    if new_google_sessions.is_empty() {
        return response;
    }

    let credential = match fetch_bound_credential(&state, cloud_access_token).await {
        Ok(credential) => credential,
        Err(message) => {
            remove_auth_sessions(&state, &new_google_sessions).await;
            return action_needed(message);
        }
    };

    {
        let mut sessions = state.auth_sessions.write().await;
        let mut injected = false;
        for session_id in &new_google_sessions {
            let Some(session) = sessions.get_mut(session_id) else {
                continue;
            };
            let Some(config) = session.google_oauth_config.as_mut() else {
                continue;
            };
            if config.client_id != credential.client_id {
                continue;
            }
            config.client_secret = credential.client_secret.clone();
            injected = true;
        }
        if !injected {
            for session_id in &new_google_sessions {
                sessions.remove(session_id);
            }
            return action_needed(
                "Google Desktop credential could not be bound to the in-flight OAuth session. Start the connection again.",
            );
        }
    }

    if let Err(message) = persist_credential(&state, &credential) {
        remove_auth_sessions(&state, &new_google_sessions).await;
        return action_needed(message);
    }

    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use axum::Router;
    use bicameral_audit::store::AuditStore;
    use bicameral_config::{
        GoogleNativeAppConfigV1, InMemorySecretsStore, GOOGLE_CLIENT_CREDENTIAL_KEY,
    };
    use chrono::{Duration as ChronoDuration, Utc};
    use std::sync::Arc;

    fn test_state() -> AppState {
        let dir = tempfile::tempdir().unwrap();
        let audit = AuditStore::open(dir.path()).unwrap();
        AppState::new(audit).with_secrets_store(Arc::new(InMemorySecretsStore::new()))
    }

    fn public_config() -> GoogleNativeAppConfigV1 {
        let now = Utc::now();
        GoogleNativeAppConfigV1 {
            schema_version: 1,
            provider: "google".into(),
            client_type: "native".into(),
            client_id: "desktop.apps.googleusercontent.com".into(),
            environment: "production".into(),
            config_version: "google-production-7".into(),
            scope_set_version: "drive-file-v1".into(),
            scopes: vec!["https://www.googleapis.com/auth/drive.file".into()],
            issued_at: now - ChronoDuration::minutes(1),
            expires_at: now + ChronoDuration::minutes(30),
            enabled: true,
        }
    }

    async fn spawn_credential_server(body: serde_json::Value) -> String {
        let app = Router::new().route(
            GOOGLE_CREDENTIAL_PATH,
            get(move |headers: axum::http::HeaderMap| {
                let body = body.clone();
                async move {
                    let authorized = headers
                        .get(axum::http::header::AUTHORIZATION)
                        .and_then(|value| value.to_str().ok())
                        == Some("Bearer cloud-session-token");
                    if !authorized {
                        return (
                            axum::http::StatusCode::UNAUTHORIZED,
                            [("content-type", "application/json")],
                            "{}".to_string(),
                        );
                    }
                    (
                        axum::http::StatusCode::OK,
                        [("content-type", "application/json")],
                        body.to_string(),
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{address}")
    }

    #[tokio::test]
    async fn authenticated_credential_exact_matches_public_binding_and_persists() {
        let origin = spawn_credential_server(serde_json::json!({
            "schema_version": 1,
            "provider": "google",
            "client_type": "native",
            "client_id": "desktop.apps.googleusercontent.com",
            "environment": "production",
            "config_version": "google-production-7",
            "client_secret": "provider-desktop-secret"
        }))
        .await;
        let mut state = test_state();
        state.google_cloud_origin_override = Some(origin);
        *state.google_app_config_cache.write().await = Some(public_config());

        let credential = fetch_bound_credential(&state, "cloud-session-token")
            .await
            .unwrap();
        assert_eq!(credential.client_id, "desktop.apps.googleusercontent.com");
        assert_eq!(credential.config_version, "google-production-7");
        persist_credential(&state, &credential).unwrap();

        let stored = state
            .secrets_store
            .read_secret(
                bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
                GOOGLE_CLIENT_CREDENTIAL_KEY,
            )
            .unwrap();
        let envelope = GoogleClientCredentialEnvelopeV1::from_secret_string(&stored).unwrap();
        assert!(
            envelope.matches_binding("desktop.apps.googleusercontent.com", "google-production-7")
        );
        assert_eq!(envelope.client_secret, "provider-desktop-secret");
        let debug = format!("{credential:?}");
        assert!(!debug.contains("provider-desktop-secret"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[tokio::test]
    async fn rotated_or_wrong_credential_binding_is_rejected_before_persist() {
        let origin = spawn_credential_server(serde_json::json!({
            "schema_version": 1,
            "provider": "google",
            "client_type": "native",
            "client_id": "desktop.apps.googleusercontent.com",
            "environment": "production",
            "config_version": "google-production-8",
            "client_secret": "wrong-generation-secret"
        }))
        .await;
        let mut state = test_state();
        state.google_cloud_origin_override = Some(origin);
        *state.google_app_config_cache.write().await = Some(public_config());

        let error = fetch_bound_credential(&state, "cloud-session-token")
            .await
            .unwrap_err();
        assert!(error.contains("binding does not match"));
        assert!(state
            .secrets_store
            .read_secret(
                bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
                GOOGLE_CLIENT_CREDENTIAL_KEY,
            )
            .is_none());
    }

    #[tokio::test]
    async fn invalid_cloud_bearer_never_exposes_provider_secret() {
        let origin = spawn_credential_server(serde_json::json!({
            "schema_version": 1,
            "provider": "google",
            "client_type": "native",
            "client_id": "desktop.apps.googleusercontent.com",
            "environment": "production",
            "config_version": "google-production-7",
            "client_secret": "provider-desktop-secret"
        }))
        .await;
        let mut state = test_state();
        state.google_cloud_origin_override = Some(origin);
        *state.google_app_config_cache.write().await = Some(public_config());

        let error = fetch_bound_credential(&state, "wrong-token")
            .await
            .unwrap_err();
        assert!(error.contains("no longer authorized"));
        assert!(!error.contains("provider-desktop-secret"));
    }
}
