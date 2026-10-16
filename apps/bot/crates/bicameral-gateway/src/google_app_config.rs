//! Cloud-first Google OAuth application configuration bootstrap (bot#1114).
//!
//! Cloud owns Bicameral's Google OAuth application identity/configuration. The
//! local daemon consumes only bounded non-secret app metadata, while keeping
//! PKCE, authorization codes, provider tokens, refresh lifecycle, and
//! ConnectionRegistry authority local.

use bicameral_config::{GoogleNativeAppConfigV1, GoogleOAuthConfig, DEFAULT_GATEWAY_PORT};
use chrono::Utc;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::RwLock;

const DEFAULT_CLOUD_ORIGIN: &str = "https://app.bicameral-ai.com";
const DEFAULT_EXPECTED_ENVIRONMENT: &str = "production";
const GOOGLE_APP_CONFIG_PATH: &str = "/api/v1/provider-app-config/google";
const BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(5);

pub type GoogleAppConfigCache = Arc<RwLock<Option<GoogleNativeAppConfigV1>>>;

pub fn new_google_app_config_cache() -> GoogleAppConfigCache {
    Arc::new(RwLock::new(None))
}

pub fn legacy_env_override_enabled() -> bool {
    std::env::var("BICAMERAL_GOOGLE_OAUTH_ALLOW_ENV_OVERRIDE")
        .ok()
        .is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoogleAppConfigSource {
    Cloud,
    Cache,
    LegacyEnv,
}

#[derive(Debug, Clone)]
pub struct ResolvedGoogleOAuthConfig {
    pub oauth: GoogleOAuthConfig,
    pub source: GoogleAppConfigSource,
    pub config_version: Option<String>,
}

#[derive(Debug, Error)]
pub enum GoogleAppConfigResolveError {
    #[error("Google OAuth application configuration is disabled")]
    Disabled,
    #[error("Google OAuth application configuration is invalid: {0}")]
    Invalid(String),
    #[error("Google OAuth application configuration is unavailable: {0}")]
    Unavailable(String),
}

/// Resolve the effective Google OAuth application config for a new auth attempt.
///
/// Resolution order is intentionally strict:
/// 1. Valid fresh Cloud payload.
/// 2. Still-valid daemon-memory cache only for transient Cloud unavailability.
/// 3. Legacy env configuration only when the caller explicitly opts in.
///
/// Explicit Cloud disablement, malformed 200 responses, schema/environment
/// mismatches, and other semantic rejections never fall back to cache or env.
pub async fn resolve_google_oauth_config(
    cache: &GoogleAppConfigCache,
    legacy_env: Option<&GoogleOAuthConfig>,
    allow_legacy_env: bool,
) -> Result<ResolvedGoogleOAuthConfig, GoogleAppConfigResolveError> {
    let origin = cloud_origin();
    let expected_environment = expected_environment();
    resolve_google_oauth_config_from(
        cache,
        legacy_env,
        allow_legacy_env,
        &origin,
        &expected_environment,
    )
    .await
}

/// Test-only origin-injected resolution (#1135): identical semantics to
/// [`resolve_google_oauth_config`], but against an explicit Cloud origin so
/// deterministic fresh-install tests can run the REAL Cloud-first resolver
/// against a local stub server. Compiled only for test builds.
#[cfg(test)]
pub(crate) async fn resolve_google_oauth_config_from_origin(
    cache: &GoogleAppConfigCache,
    legacy_env: Option<&GoogleOAuthConfig>,
    allow_legacy_env: bool,
    origin: &str,
) -> Result<ResolvedGoogleOAuthConfig, GoogleAppConfigResolveError> {
    let expected_environment = expected_environment();
    resolve_google_oauth_config_from(
        cache,
        legacy_env,
        allow_legacy_env,
        origin,
        &expected_environment,
    )
    .await
}

async fn resolve_google_oauth_config_from(
    cache: &GoogleAppConfigCache,
    legacy_env: Option<&GoogleOAuthConfig>,
    allow_legacy_env: bool,
    origin: &str,
    expected_environment: &str,
) -> Result<ResolvedGoogleOAuthConfig, GoogleAppConfigResolveError> {
    if allow_legacy_env {
        if let Some(config) = legacy_env {
            if !config.client_id.trim().is_empty() {
                return Ok(ResolvedGoogleOAuthConfig {
                    oauth: config.clone(),
                    source: GoogleAppConfigSource::LegacyEnv,
                    config_version: None,
                });
            }
        }
    }

    let url = format!("{}{}", origin.trim_end_matches('/'), GOOGLE_APP_CONFIG_PATH);
    let client = reqwest::Client::builder()
        .timeout(BOOTSTRAP_TIMEOUT)
        .build()
        .map_err(|error| GoogleAppConfigResolveError::Unavailable(error.to_string()))?;

    let response = client.get(&url).send().await;
    match response {
        Ok(response) if response.status().is_success() => {
            let payload = match response.json::<GoogleNativeAppConfigV1>().await {
                Ok(payload) => payload,
                Err(error) => {
                    *cache.write().await = None;
                    return Err(GoogleAppConfigResolveError::Invalid(format!(
                        "Cloud returned malformed app config: {error}"
                    )));
                }
            };
            if let Err(error) = payload.validate_for(expected_environment, Utc::now()) {
                *cache.write().await = None;
                return Err(GoogleAppConfigResolveError::Invalid(error.to_string()));
            }
            let oauth = oauth_from_native_app_config(&payload);
            let config_version = Some(payload.config_version.clone());
            *cache.write().await = Some(payload);
            Ok(ResolvedGoogleOAuthConfig {
                oauth,
                source: GoogleAppConfigSource::Cloud,
                config_version,
            })
        }
        Ok(response) if response.status() == reqwest::StatusCode::GONE => {
            *cache.write().await = None;
            Err(GoogleAppConfigResolveError::Disabled)
        }
        Ok(response)
            if response.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE
                || response.status() == reqwest::StatusCode::BAD_GATEWAY
                || response.status() == reqwest::StatusCode::GATEWAY_TIMEOUT
                || response.status() == reqwest::StatusCode::REQUEST_TIMEOUT
                || response.status().is_server_error() =>
        {
            fallback_after_transient_failure(
                cache,
                legacy_env,
                allow_legacy_env,
                expected_environment,
                format!("Cloud returned HTTP {}", response.status()),
            )
            .await
        }
        Ok(response) => {
            *cache.write().await = None;
            Err(GoogleAppConfigResolveError::Invalid(format!(
                "Cloud rejected app-config bootstrap with HTTP {}",
                response.status()
            )))
        }
        Err(error) => {
            fallback_after_transient_failure(
                cache,
                legacy_env,
                allow_legacy_env,
                expected_environment,
                format!("Cloud bootstrap request failed: {error}"),
            )
            .await
        }
    }
}

async fn fallback_after_transient_failure(
    cache: &GoogleAppConfigCache,
    legacy_env: Option<&GoogleOAuthConfig>,
    allow_legacy_env: bool,
    expected_environment: &str,
    reason: String,
) -> Result<ResolvedGoogleOAuthConfig, GoogleAppConfigResolveError> {
    let cached = cache.read().await.clone();
    if let Some(payload) = cached {
        match payload.validate_for(expected_environment, Utc::now()) {
            Ok(()) => {
                return Ok(ResolvedGoogleOAuthConfig {
                    oauth: oauth_from_native_app_config(&payload),
                    source: GoogleAppConfigSource::Cache,
                    config_version: Some(payload.config_version),
                });
            }
            Err(_) => {
                *cache.write().await = None;
            }
        }
    }

    if allow_legacy_env {
        if let Some(config) = legacy_env {
            if !config.client_id.trim().is_empty() {
                return Ok(ResolvedGoogleOAuthConfig {
                    oauth: config.clone(),
                    source: GoogleAppConfigSource::LegacyEnv,
                    config_version: None,
                });
            }
        }
    }

    Err(GoogleAppConfigResolveError::Unavailable(reason))
}

fn oauth_from_native_app_config(payload: &GoogleNativeAppConfigV1) -> GoogleOAuthConfig {
    GoogleOAuthConfig {
        client_id: payload.client_id.clone(),
        client_secret: String::new(),
        redirect_uri: google_redirect_uri(),
        scopes: payload.scopes.clone(),
    }
}

fn cloud_origin() -> String {
    std::env::var("BICAMERAL_CLOUD_ORIGIN").unwrap_or_else(|_| DEFAULT_CLOUD_ORIGIN.into())
}

fn expected_environment() -> String {
    std::env::var("BICAMERAL_GOOGLE_OAUTH_EXPECTED_ENVIRONMENT")
        .unwrap_or_else(|_| DEFAULT_EXPECTED_ENVIRONMENT.into())
}

fn google_redirect_uri() -> String {
    std::env::var("BICAMERAL_GOOGLE_REDIRECT_URI").unwrap_or_else(|_| default_google_redirect_uri())
}

fn default_google_redirect_uri() -> String {
    format!("http://127.0.0.1:{DEFAULT_GATEWAY_PORT}/api/v1/connections/google/callback")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use axum::Router;
    use chrono::{Duration as ChronoDuration, Utc};
    use tokio::task::JoinHandle;

    fn valid_payload() -> GoogleNativeAppConfigV1 {
        let now = Utc::now();
        GoogleNativeAppConfigV1 {
            schema_version: 1,
            provider: "google".into(),
            client_type: "native".into(),
            client_id: "native.apps.googleusercontent.com".into(),
            environment: DEFAULT_EXPECTED_ENVIRONMENT.into(),
            config_version: "google-production-1".into(),
            scope_set_version: "drive-file-v1".into(),
            scopes: vec!["https://www.googleapis.com/auth/drive.file".into()],
            issued_at: now - ChronoDuration::minutes(1),
            expires_at: now + ChronoDuration::minutes(30),
            enabled: true,
        }
    }

    async fn spawn_app_config_server(
        status: reqwest::StatusCode,
        body: String,
    ) -> (String, JoinHandle<()>) {
        let response_status = axum::http::StatusCode::from_u16(status.as_u16()).unwrap();
        let app = Router::new().route(
            GOOGLE_APP_CONFIG_PATH,
            get(move || {
                let body = body.clone();
                async move {
                    (
                        response_status,
                        [("content-type", "application/json")],
                        body,
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{address}"), handle)
    }

    #[test]
    fn default_redirect_uri_tracks_canonical_gateway_port() {
        assert_eq!(
            default_google_redirect_uri(),
            format!("http://127.0.0.1:{DEFAULT_GATEWAY_PORT}/api/v1/connections/google/callback")
        );
    }

    #[test]
    fn native_payload_becomes_public_client_without_secret() {
        let payload = valid_payload();
        let oauth = oauth_from_native_app_config(&payload);
        assert_eq!(oauth.client_id, payload.client_id);
        assert!(oauth.client_secret.is_empty());
        assert_eq!(oauth.scopes, payload.scopes);
    }

    #[tokio::test]
    async fn live_valid_cloud_response_populates_cache() {
        let payload = valid_payload();
        let body = serde_json::to_string(&payload).unwrap();
        let (origin, server) = spawn_app_config_server(reqwest::StatusCode::OK, body).await;
        let cache = new_google_app_config_cache();

        let resolved = resolve_google_oauth_config_from(
            &cache,
            None,
            false,
            &origin,
            DEFAULT_EXPECTED_ENVIRONMENT,
        )
        .await
        .unwrap();

        assert_eq!(resolved.source, GoogleAppConfigSource::Cloud);
        assert_eq!(resolved.oauth.client_id, payload.client_id);
        assert_eq!(
            resolved.config_version.as_deref(),
            Some("google-production-1")
        );
        assert_eq!(
            cache
                .read()
                .await
                .as_ref()
                .map(|cached| cached.client_id.as_str()),
            Some("native.apps.googleusercontent.com")
        );
        server.abort();
    }

    #[tokio::test]
    async fn live_transient_503_uses_still_valid_cache() {
        let (origin, server) =
            spawn_app_config_server(reqwest::StatusCode::SERVICE_UNAVAILABLE, "{}".into()).await;
        let cache = new_google_app_config_cache();
        *cache.write().await = Some(valid_payload());

        let resolved = resolve_google_oauth_config_from(
            &cache,
            None,
            false,
            &origin,
            DEFAULT_EXPECTED_ENVIRONMENT,
        )
        .await
        .unwrap();

        assert_eq!(resolved.source, GoogleAppConfigSource::Cache);
        assert!(cache.read().await.is_some());
        server.abort();
    }

    #[tokio::test]
    async fn live_410_disable_clears_cache_and_fails_closed() {
        let (origin, server) =
            spawn_app_config_server(reqwest::StatusCode::GONE, "{}".into()).await;
        let cache = new_google_app_config_cache();
        *cache.write().await = Some(valid_payload());

        let error = resolve_google_oauth_config_from(
            &cache,
            None,
            false,
            &origin,
            DEFAULT_EXPECTED_ENVIRONMENT,
        )
        .await
        .unwrap_err();

        assert!(matches!(error, GoogleAppConfigResolveError::Disabled));
        assert!(cache.read().await.is_none());
        server.abort();
    }

    #[tokio::test]
    async fn malformed_200_clears_old_cache_instead_of_masking_rejection() {
        let (origin, server) =
            spawn_app_config_server(reqwest::StatusCode::OK, "{not-json".into()).await;
        let cache = new_google_app_config_cache();
        *cache.write().await = Some(valid_payload());

        let error = resolve_google_oauth_config_from(
            &cache,
            None,
            false,
            &origin,
            DEFAULT_EXPECTED_ENVIRONMENT,
        )
        .await
        .unwrap_err();

        assert!(matches!(error, GoogleAppConfigResolveError::Invalid(_)));
        assert!(cache.read().await.is_none());
        server.abort();
    }

    #[tokio::test]
    async fn wrong_environment_200_clears_old_cache_and_fails_invalid() {
        let mut payload = valid_payload();
        payload.environment = "staging".into();
        let body = serde_json::to_string(&payload).unwrap();
        let (origin, server) = spawn_app_config_server(reqwest::StatusCode::OK, body).await;
        let cache = new_google_app_config_cache();
        *cache.write().await = Some(valid_payload());

        let error = resolve_google_oauth_config_from(
            &cache,
            None,
            false,
            &origin,
            DEFAULT_EXPECTED_ENVIRONMENT,
        )
        .await
        .unwrap_err();

        assert!(matches!(error, GoogleAppConfigResolveError::Invalid(_)));
        assert!(cache.read().await.is_none());
        server.abort();
    }

    #[tokio::test]
    async fn valid_cache_survives_transient_cloud_failure() {
        let cache = new_google_app_config_cache();
        *cache.write().await = Some(valid_payload());
        let resolved = fallback_after_transient_failure(
            &cache,
            None,
            false,
            DEFAULT_EXPECTED_ENVIRONMENT,
            "simulated outage".into(),
        )
        .await
        .unwrap();
        assert_eq!(resolved.source, GoogleAppConfigSource::Cache);
        assert_eq!(
            resolved.oauth.client_id,
            "native.apps.googleusercontent.com"
        );
        assert!(resolved.oauth.client_secret.is_empty());
    }

    #[tokio::test]
    async fn expired_cache_fails_closed() {
        let cache = new_google_app_config_cache();
        let mut payload = valid_payload();
        payload.expires_at = Utc::now() - ChronoDuration::seconds(1);
        *cache.write().await = Some(payload);
        let error = fallback_after_transient_failure(
            &cache,
            None,
            false,
            DEFAULT_EXPECTED_ENVIRONMENT,
            "simulated outage".into(),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, GoogleAppConfigResolveError::Unavailable(_)));
        assert!(cache.read().await.is_none());
    }

    #[tokio::test]
    async fn wrong_environment_cache_fails_closed() {
        let cache = new_google_app_config_cache();
        let mut payload = valid_payload();
        payload.environment = "staging".into();
        *cache.write().await = Some(payload);
        let error = fallback_after_transient_failure(
            &cache,
            None,
            false,
            DEFAULT_EXPECTED_ENVIRONMENT,
            "simulated outage".into(),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, GoogleAppConfigResolveError::Unavailable(_)));
        assert!(cache.read().await.is_none());
    }

    #[tokio::test]
    async fn legacy_fallback_requires_explicit_opt_in() {
        let cache = new_google_app_config_cache();
        let legacy = GoogleOAuthConfig {
            client_id: "legacy-client".into(),
            client_secret: String::new(),
            redirect_uri: "http://localhost/callback".into(),
            scopes: vec!["scope".into()],
        };
        let denied = fallback_after_transient_failure(
            &cache,
            Some(&legacy),
            false,
            DEFAULT_EXPECTED_ENVIRONMENT,
            "simulated outage".into(),
        )
        .await;
        assert!(matches!(
            denied,
            Err(GoogleAppConfigResolveError::Unavailable(_))
        ));

        let allowed = fallback_after_transient_failure(
            &cache,
            Some(&legacy),
            true,
            DEFAULT_EXPECTED_ENVIRONMENT,
            "simulated outage".into(),
        )
        .await
        .unwrap();
        assert_eq!(allowed.source, GoogleAppConfigSource::LegacyEnv);
    }
}
