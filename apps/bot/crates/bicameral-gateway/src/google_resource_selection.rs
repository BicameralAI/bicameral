//! Daemon-owned Google Drive folder selection and validation (#1085).
//!
//! Implements the provider-owned resource-selection seam required before
//! canonical `product.create` with a Google descriptor: a system-browser
//! Picker/OAuth start URL, an opaque daemon-authored selection session,
//! callback binding of `picked_file_ids` to that exact session, credential
//! exchange/rotation through the existing connection-secret path, and
//! fail-closed Drive `files.get` validation of exactly one selected
//! folder.
//!
//! Authority boundary: folder validation grants resource readiness only.
//! It does not create a Product, append an event, create SourceBinding,
//! approve ingestion, grant membership, or publish Cloud state. The
//! hosted SPA never receives Google access/refresh tokens; it sees only
//! the redacted [`ResourceSelectionProjection`].

use bicameral_api::provider_resource::{
    ProviderResourceDescriptor, ProviderResourceState, ResourceSelectionProjection,
};
use chrono::{DateTime, Utc};
use std::future::Future;
use std::pin::Pin;

/// Drive folder MIME type required for readiness.
pub const DRIVE_FOLDER_MIME_TYPE: &str = "application/vnd.google-apps.folder";

/// Lifetime of a selection session before it expires.
///
/// Bounds how long a daemon-validated folder may be carried into
/// `product.create` (#1264) and how long a pending Picker attempt stays
/// claimable. Thirty minutes covers a real operator walking through the
/// Google account chooser, consent, and Picker and then returning to the
/// create dialog; a real run at ten minutes expired twice before Create.
pub const SELECTION_SESSION_TTL_SECONDS: i64 = 1_800;

/// Loopback path Google redirects to for the Picker/resource-selection
/// flow. Distinct from the provider-Connection callback: the browser GET
/// on this path resolves opaque state against `resource_selection_sessions`
/// only, never `auth_sessions`.
pub const RESOURCE_SELECTION_CALLBACK_PATH: &str =
    "/api/v1/setup/google/resource-selection/callback";

/// Rewrite the resolved Connection redirect URI to the resource-selection
/// callback on the SAME loopback origin (#1135 review fix): the redirect
/// URI actually sent to Google must identify this flow's callback, and the
/// later token exchange posts the identical pinned value. Only the path is
/// substituted so the canonical daemon port (37373 by default) and any
/// environment-specific origin survive.
fn selection_redirect_uri(connection_redirect_uri: &str) -> String {
    match connection_redirect_uri.find("/api/") {
        Some(index) => format!(
            "{}{}",
            &connection_redirect_uri[..index],
            RESOURCE_SELECTION_CALLBACK_PATH
        ),
        None => format!(
            "http://127.0.0.1:{}{}",
            bicameral_config::DEFAULT_GATEWAY_PORT,
            RESOURCE_SELECTION_CALLBACK_PATH
        ),
    }
}

// ── Drive metadata probe ──────────────────────────────────────────

/// Safe subset of Drive `files.get` metadata used for validation.
///
/// Contains no secret material and no file content. `Debug` is derived
/// deliberately: every field is already a safe provider fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveFileMetadata {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub trashed: bool,
    /// Present when the file lives in a shared drive.
    pub drive_id: Option<String>,
    pub can_add_children: bool,
    pub can_list_children: Option<bool>,
}

/// Typed failures from the Drive metadata probe. Never carries token
/// material; provider detail is reduced to a safe operator message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriveProbeError {
    /// Credential rejected by the provider (401/403 auth class).
    CredentialRejected,
    /// Resource does not exist or is not accessible under `drive.file`.
    NotAccessible,
    /// Provider returned an error outside the auth/not-found classes.
    Provider { safe_message: String },
    /// Transport-level failure reaching the provider.
    Network { safe_message: String },
}

pub type DriveProbeFuture<'a> =
    Pin<Box<dyn Future<Output = Result<DriveFileMetadata, DriveProbeError>> + Send + 'a>>;

/// Injected Drive `files.get` transport.
///
/// The live implementation calls the Drive REST API with
/// `supportsAllDrives=true` and a narrow `fields` projection; tests
/// inject deterministic doubles. The access token flows through this
/// boundary transiently and must never be logged or stored by
/// implementations.
pub trait DriveResourceProbe: Send + Sync {
    fn get_file_metadata<'a>(
        &'a self,
        access_token: &'a str,
        file_id: &'a str,
    ) -> DriveProbeFuture<'a>;
}

/// Live Drive REST probe.
pub struct HttpDriveResourceProbe {
    client: reqwest::Client,
}

impl HttpDriveResourceProbe {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

impl DriveResourceProbe for HttpDriveResourceProbe {
    fn get_file_metadata<'a>(
        &'a self,
        access_token: &'a str,
        file_id: &'a str,
    ) -> DriveProbeFuture<'a> {
        Box::pin(async move {
            let url = format!(
                "https://www.googleapis.com/drive/v3/files/{}?supportsAllDrives=true&fields=id,name,mimeType,trashed,driveId,capabilities/canAddChildren,capabilities/canListChildren",
                file_id
            );
            let response = self
                .client
                .get(&url)
                .bearer_auth(access_token)
                .send()
                .await
                .map_err(|_| DriveProbeError::Network {
                    safe_message: "Could not reach Google Drive.".into(),
                })?;

            match response.status().as_u16() {
                200 => {}
                401 | 403 => return Err(DriveProbeError::CredentialRejected),
                404 => return Err(DriveProbeError::NotAccessible),
                status => {
                    return Err(DriveProbeError::Provider {
                        safe_message: format!("Google Drive returned status {status}."),
                    })
                }
            }

            let json: serde_json::Value =
                response
                    .json()
                    .await
                    .map_err(|_| DriveProbeError::Provider {
                        safe_message: "Google Drive returned an unreadable response.".into(),
                    })?;

            Ok(DriveFileMetadata {
                id: json["id"].as_str().unwrap_or_default().to_string(),
                name: json["name"].as_str().unwrap_or_default().to_string(),
                mime_type: json["mimeType"].as_str().unwrap_or_default().to_string(),
                trashed: json["trashed"].as_bool().unwrap_or(false),
                drive_id: json["driveId"].as_str().map(String::from),
                can_add_children: json["capabilities"]["canAddChildren"]
                    .as_bool()
                    .unwrap_or(false),
                can_list_children: json["capabilities"]["canListChildren"].as_bool(),
            })
        })
    }
}

// ── Selection session state ───────────────────────────────────────

/// Terminal or pending outcome of one selection attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionOutcome {
    Pending,
    /// In-flight completion claim (#1135, review comment 5323636602):
    /// exactly one validated callback atomically transitions
    /// `Pending -> Completing` and becomes the sole party allowed to
    /// perform token exchange, secret persistence, and Drive validation.
    /// Concurrent duplicates observe this state and never reach a
    /// provider. If the claimant dies mid-completion the session stays
    /// `Completing` until the TTL purges it — fail-closed: nothing false
    /// is stored and a single-use authorization code is never exchanged
    /// twice.
    Completing,
    Ready(ProviderResourceDescriptor),
    Failed {
        state: ProviderResourceState,
        message: String,
    },
}

/// A daemon-memory resource-selection session.
///
/// Creation-attempt context only: never persisted, never registry state,
/// and never exposed beyond the redacted projection.
///
/// #1135: the session pins the EXACT Google application config and PKCE
/// verifier bound when the Picker flow started, so the callback can never
/// re-resolve a rotated client identity mid-flight and the code exchange
/// is always PKCE-bound to this attempt.
#[derive(Debug, Clone)]
pub struct ResourceSelectionSession {
    pub session_id: String,
    pub state_token: String,
    pub created_at: DateTime<Utc>,
    pub outcome: SelectionOutcome,
    /// Exact resolved application config bound at start (#1135).
    /// Daemon-memory only; never returned to the browser or Cloud.
    pub google_oauth_config: bicameral_config::GoogleOAuthConfig,
    /// Ephemeral RFC 7636 verifier for this selection attempt (#1135).
    /// Never persisted, logged, returned to the browser, or sent to Cloud.
    pub pkce_verifier: String,
}

impl ResourceSelectionSession {
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        now >= self.created_at + chrono::Duration::seconds(SELECTION_SESSION_TTL_SECONDS)
    }
}

/// Project a session into the safe wire shape.
pub fn selection_projection(outcome: &SelectionOutcome) -> ResourceSelectionProjection {
    match outcome {
        SelectionOutcome::Pending => ResourceSelectionProjection {
            provider: "google".into(),
            state: ProviderResourceState::PendingSelection,
            message: "Waiting for folder selection in the browser.".into(),
            resource: None,
        },
        // The in-flight claim projects as the existing non-terminal wire
        // state: safe for polling, introduces no new wire vocabulary, and
        // carries no session/code/token material.
        SelectionOutcome::Completing => ResourceSelectionProjection {
            provider: "google".into(),
            state: ProviderResourceState::PendingSelection,
            message: "Folder selection is completing.".into(),
            resource: None,
        },
        SelectionOutcome::Ready(descriptor) => ResourceSelectionProjection {
            provider: "google".into(),
            state: ProviderResourceState::Ready,
            message: "Folder validated for Bicameral event storage.".into(),
            resource: Some(descriptor.clone()),
        },
        SelectionOutcome::Failed { state, message } => ResourceSelectionProjection {
            provider: "google".into(),
            state: *state,
            message: message.clone(),
            resource: None,
        },
    }
}

// ── Fail-closed validation ────────────────────────────────────────

/// Validate probed folder metadata against the readiness contract.
///
/// Fail-closed: readiness requires an accessible, non-trashed folder the
/// current credential can add children to. Everything else maps to a
/// typed non-ready state with a safe message.
pub fn validate_folder_metadata(
    selected_id: &str,
    metadata: &DriveFileMetadata,
) -> Result<ProviderResourceDescriptor, (ProviderResourceState, String)> {
    if metadata.id != selected_id {
        return Err((
            ProviderResourceState::NotReady,
            "Google Drive returned metadata for a different resource.".into(),
        ));
    }
    if metadata.mime_type != DRIVE_FOLDER_MIME_TYPE {
        return Err((
            ProviderResourceState::NotReady,
            "The selected item is not a folder.".into(),
        ));
    }
    if metadata.trashed {
        return Err((
            ProviderResourceState::NotReady,
            "The selected folder is in the trash.".into(),
        ));
    }
    if !metadata.can_add_children {
        return Err((
            ProviderResourceState::ActionNeeded,
            "Bicameral cannot create files in the selected folder. Choose a folder you can edit."
                .into(),
        ));
    }
    Ok(ProviderResourceDescriptor {
        provider: "google".into(),
        resource_kind: "drive_folder".into(),
        folder_id: metadata.id.clone(),
        folder_display_name: metadata.name.clone(),
        drive_id: metadata.drive_id.clone(),
        mime_type: metadata.mime_type.clone(),
        can_add_children: metadata.can_add_children,
        can_list_children: metadata.can_list_children,
    })
}

/// Map a probe error to a typed non-ready selection state.
pub fn probe_error_state(error: &DriveProbeError) -> (ProviderResourceState, String) {
    match error {
        DriveProbeError::CredentialRejected => (
            ProviderResourceState::ActionNeeded,
            "Google rejected the connection credential. Reconnect Google and retry.".into(),
        ),
        DriveProbeError::NotAccessible => (
            ProviderResourceState::NotReady,
            "The selected folder does not exist or is not accessible.".into(),
        ),
        DriveProbeError::Provider { safe_message } | DriveProbeError::Network { safe_message } => {
            (ProviderResourceState::NotReady, safe_message.clone())
        }
    }
}

/// Build the system-browser Picker/OAuth URL for folder selection.
///
/// Retains `drive.file` (no broad Drive scope), requests refreshable
/// semantics, uses fresh daemon-generated PKCE S256 (#1135), and
/// constrains the Picker to exactly one selectable folder
/// (`trigger_onepick=true`, folder selection enabled; multi-select is
/// never enabled).
pub fn picker_authorization_url(
    config: &bicameral_config::GoogleOAuthConfig,
    state_token: &str,
    code_challenge: &str,
) -> String {
    let base = config.authorization_url_with_pkce(state_token, Some(code_challenge));
    format!("{base}&trigger_onepick=true&allow_folder_selection=true")
}

// ── Desktop credential binding for the Picker exchange (#1264) ─────

/// Read the Desktop client credential envelope that a successful Connect
/// persisted in the OS keyring, and return its secret only when the envelope
/// is bound to exactly this `client_id` and `config_version`.
///
/// Never logs or returns the secret elsewhere; a missing, unparsable, or
/// rotated envelope yields `None` so the caller fails closed instead of
/// attempting a code exchange Google would reject.
pub(crate) fn stored_desktop_credential(
    secrets: &dyn bicameral_config::SecretsStore,
    client_id: &str,
    config_version: Option<&str>,
) -> Option<String> {
    let raw = secrets.read_secret(
        bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
        bicameral_config::GOOGLE_CLIENT_CREDENTIAL_KEY,
    )?;
    let envelope =
        bicameral_config::GoogleClientCredentialEnvelopeV1::from_secret_string(&raw).ok()?;
    let config_version = config_version.unwrap_or_default();
    if !envelope.matches_binding(client_id, config_version)
        || envelope.client_secret.trim().is_empty()
    {
        return None;
    }
    Some(envelope.client_secret)
}

// ── Descriptor validation for operator-scoped create (#1264) ──────

/// Return the Google Drive folder id carried by a `product.create` command
/// when, and only when, this daemon holds a live `Ready` selection session
/// for exactly that folder.
///
/// This is the only way a hosted `product.create` descriptor becomes
/// daemon-validated: the browser can transport the descriptor it read from
/// the redacted projection, but a folder the daemon never validated, a
/// selection that expired, a failed selection, or a different folder all
/// resolve to `None` and the runtime fails closed.
pub async fn validated_event_store_folder(
    state: &AppState,
    command: &bicameral_api::tool_request::ToolCommand,
) -> Option<String> {
    use bicameral_api::product_creation::ProductEventStoreDescriptor;
    use bicameral_api::tool_request::ToolCommand;

    let ToolCommand::ProductCreate(params) = command else {
        return None;
    };
    let Some(ProductEventStoreDescriptor::Google { folder_id }) = &params.event_store_descriptor
    else {
        return None;
    };
    let now = Utc::now();
    let sessions = state.resource_selection_sessions.read().await;
    let validated = sessions.values().any(|session| {
        !session.is_expired(now)
            && matches!(
                &session.outcome,
                SelectionOutcome::Ready(descriptor)
                    if descriptor.provider == "google" && descriptor.folder_id == *folder_id
            )
    });
    validated.then(|| folder_id.clone())
}

// ── HTTP surface ──────────────────────────────────────────────────

use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use bicameral_api::provider_resource::{
    ResourceSelectionCallbackRequest, ResourceSelectionStartOutcome,
};

/// Routes for daemon-owned Google folder selection.
///
/// Mounted under the existing `/api/v1/setup/` browser-data-plane prefix;
/// the hosted SPA drives them but never receives token material.
pub fn resource_selection_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/setup/google/resource-selection/start",
            post(resource_selection_start),
        )
        .route(
            "/api/v1/setup/google/resource-selection/callback",
            post(resource_selection_callback).get(resource_selection_loopback_callback),
        )
        .route(
            "/api/v1/setup/google/resource-selection/:session_id",
            get(resource_selection_status),
        )
}

/// Query shape of the real Google browser redirect for the Picker flow.
/// Values are consumed in memory and never reflected into the browser
/// response, logs, projections, or durable evidence. `scope` is accepted
/// as provider-informational and dropped.
#[derive(Debug, serde::Deserialize)]
struct SelectionLoopbackQuery {
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    scope: Option<String>,
    /// Comma-separated selected file ids appended by the Picker surface.
    #[serde(default)]
    picked_file_ids: Option<String>,
}

/// Receive the provider browser redirect for the native Picker/resource
/// selection flow (#1135 review fix).
///
/// Google redirects the browser here with an HTTP GET query. The opaque
/// CSRF `state` maps back to the transient daemon-owned
/// `resource_selection_sessions` entry — never `auth_sessions` — so
/// Picker completion cannot cross into ConnectionRegistry authority. The
/// selection session id, PKCE verifier, authorization code, and token
/// material never appear in the browser output.
async fn resource_selection_loopback_callback(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<SelectionLoopbackQuery>,
) -> axum::response::Response {
    let Some(state_token) = query.state.as_deref().filter(|value| !value.is_empty()) else {
        return selection_loopback_page(StatusCode::BAD_REQUEST, false);
    };

    // Resolve the opaque state to a live selection session, purging
    // expired entries so an aged attempt can never resurrect a callback
    // (same discipline as the Connection loopback, #1133).
    let session_id = {
        let now = Utc::now();
        let mut sessions = state.resource_selection_sessions.write().await;
        sessions.retain(|_, session| !session.is_expired(now));
        sessions
            .iter()
            .find(|(_, session)| session.state_token == state_token)
            .map(|(session_id, _)| session_id.clone())
    };
    let Some(session_id) = session_id else {
        return selection_loopback_page(StatusCode::BAD_REQUEST, false);
    };

    let picked_file_ids = query
        .picked_file_ids
        .as_deref()
        .map(|raw| {
            raw.split(',')
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let body = ResourceSelectionCallbackRequest {
        selection_session_id: session_id,
        code: query.code,
        state: query.state,
        picked_file_ids,
        error: query.error,
    };
    let (status, projection) = complete_resource_selection(&state, &body).await;

    let ready = matches!(projection.state, ProviderResourceState::Ready);
    let browser_status = if ready {
        StatusCode::OK
    } else if status.is_success() {
        StatusCode::BAD_REQUEST
    } else {
        status
    };
    selection_loopback_page(browser_status, ready)
}

/// Static, reflection-free completion page for the Picker loopback.
fn selection_loopback_page(status: StatusCode, ready: bool) -> axum::response::Response {
    use axum::response::IntoResponse;

    let (title, message) = if ready {
        (
            "Google Drive folder selected",
            "The folder was validated for Bicameral event storage. You can close this tab and return to Bicameral.",
        )
    } else {
        (
            "Google Drive folder selection not completed",
            "The folder selection was not completed. Return to Bicameral and try again.",
        )
    };
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{title}</title></head><body><main><h1>{title}</h1><p>{message}</p></main></body></html>"
    );
    (
        status,
        [
            ("cache-control", "no-store"),
            (
                "content-security-policy",
                "default-src 'none'; style-src 'none'; frame-ancestors 'none'",
            ),
            ("referrer-policy", "no-referrer"),
            ("x-content-type-options", "nosniff"),
        ],
        axum::response::Html(body),
    )
        .into_response()
}

/// `POST /setup/google/resource-selection/start` — begin folder selection.
///
/// #1135: application config resolves through the same governed
/// Cloud-first resolver as provider Connection. A normal fresh install
/// needs no manually injected Google client id/secret; the legacy env
/// path stays behind the same explicit override policy. The resolved
/// config and a fresh PKCE verifier are pinned into the session.
async fn resource_selection_start(
    State(state): State<AppState>,
) -> (StatusCode, Json<ResourceSelectionStartOutcome>) {
    let resolved = match crate::routes::resolve_google_oauth_config_for(&state).await {
        Ok(resolved) => resolved,
        Err(error) => {
            return (
                StatusCode::OK,
                Json(ResourceSelectionStartOutcome::ActionNeeded {
                    provider: "google".into(),
                    message: format!(
                        "Google OAuth application configuration is unavailable: {error}. Retry after Cloud/operator configuration is restored."
                    ),
                }),
            );
        }
    };
    let mut config = resolved.oauth;
    // The URI Google redirects to must be this flow's own GET callback;
    // the pinned config keeps the rewritten value so the token exchange
    // posts the exact redirect_uri that authorized (#1135 review fix).
    config.redirect_uri = selection_redirect_uri(&config.redirect_uri);
    // #1264: the Picker flow exchanges its authorization code exactly like
    // the Connect flow, and Google's Desktop client requires the provider
    // credential at that exchange (#1211/#1226). Connect binds it from the
    // authenticated Cloud bootstrap and persists the envelope in the OS
    // keyring; folder selection reuses that persisted envelope only when its
    // binding matches the resolved configuration. Without a bound envelope
    // the exchange stays secretless and Google's typed rejection surfaces
    // through the existing fail-closed projection (`action_needed`).
    if config.client_secret.trim().is_empty() {
        if let Some(secret) = stored_desktop_credential(
            state.secrets_store.as_ref(),
            &config.client_id,
            resolved.config_version.as_deref(),
        ) {
            config.client_secret = secret;
        }
    }

    let session_id = format!("rsel-{}", uuid::Uuid::new_v4());
    let state_token = uuid::Uuid::new_v4().to_string();
    let pkce_verifier = crate::google_oauth::generate_pkce_verifier();
    let pkce_challenge = crate::google_oauth::pkce_s256_challenge(&pkce_verifier);
    let url = picker_authorization_url(&config, &state_token, &pkce_challenge);

    {
        let mut sessions = state.resource_selection_sessions.write().await;
        // Expired sessions are dropped opportunistically so abandoned
        // selections cannot accumulate.
        let now = Utc::now();
        sessions.retain(|_, session| !session.is_expired(now));
        sessions.insert(
            session_id.clone(),
            ResourceSelectionSession {
                session_id: session_id.clone(),
                state_token,
                created_at: now,
                outcome: SelectionOutcome::Pending,
                google_oauth_config: config,
                pkce_verifier,
            },
        );
    }

    (
        StatusCode::OK,
        Json(ResourceSelectionStartOutcome::StartUrl {
            provider: "google".into(),
            url,
            selection_session_id: session_id,
        }),
    )
}

/// `POST /setup/google/resource-selection/callback` — bind the Picker
/// redirect to its exact session and validate the selected folder.
async fn resource_selection_callback(
    State(state): State<AppState>,
    Json(body): Json<ResourceSelectionCallbackRequest>,
) -> (StatusCode, Json<ResourceSelectionProjection>) {
    let (status, projection) = complete_resource_selection(&state, &body).await;
    (status, Json(projection))
}

/// Shared internal completion path for the POST callback and the native
/// GET loopback adapter. Fail-closed at every step; #1135 review fix:
/// only a `Pending` session can be advanced, so a validated terminal
/// callback is one-shot and the callback capability is permanently
/// ineligible for another provider completion (polling still receives the
/// safe stored projection within the TTL).
async fn complete_resource_selection(
    state: &AppState,
    body: &ResourceSelectionCallbackRequest,
) -> (StatusCode, ResourceSelectionProjection) {
    // 1. Lookup, expiry, state validation, and the completion CLAIM run in
    // ONE atomic critical section (#1135, review comment 5323636602):
    // exactly one callback can transition `Pending -> Completing` and
    // becomes the sole party allowed to perform token exchange, secret
    // writes, or Drive validation. Concurrent duplicate GET/POST callbacks
    // observe `Completing` (or the terminal outcome) and never reach a
    // provider, so a single-use authorization code is exchanged at most
    // once. A mismatched state is rejected BEFORE the claim, so an
    // unauthenticated caller can neither claim nor kill the legitimate
    // pending attempt.
    let session = {
        let mut sessions = state.resource_selection_sessions.write().await;
        let now = Utc::now();
        if sessions
            .get(&body.selection_session_id)
            .is_some_and(|entry| entry.is_expired(now))
        {
            // Expiry stays fail-closed: the aged entry is purged and can
            // never be claimed or completed.
            sessions.remove(&body.selection_session_id);
        }
        let Some(entry) = sessions.get_mut(&body.selection_session_id) else {
            return (
                StatusCode::NOT_FOUND,
                ResourceSelectionProjection {
                    provider: "google".into(),
                    state: ProviderResourceState::NotReady,
                    message: "Selection session not found or expired.".into(),
                    resource: None,
                },
            );
        };

        if body.state.as_deref() != Some(entry.state_token.as_str()) {
            return (
                StatusCode::OK,
                ResourceSelectionProjection {
                    provider: "google".into(),
                    state: ProviderResourceState::NotReady,
                    message: "Selection state mismatch.".into(),
                    resource: None,
                },
            );
        }

        match &entry.outcome {
            // Sole claimant: acquire completion authority atomically.
            SelectionOutcome::Pending => {
                entry.outcome = SelectionOutcome::Completing;
                entry.clone()
            }
            // In-flight or terminal: return the stored safe projection
            // with zero provider side effects (one-shot completion).
            other => {
                let projection = selection_projection(other);
                return (StatusCode::OK, projection);
            }
        }
    };

    // 2. Only the claimant reaches provider side effects, outside the lock.
    let outcome = resolve_selection_outcome(state, body, &session).await;

    // 3. Commit the terminal outcome. By construction only the claimant
    // can be here and only `Completing` is replaced (defense in depth:
    // nothing else can transition the entry while the claim is held).
    {
        let mut sessions = state.resource_selection_sessions.write().await;
        if let Some(entry) = sessions.get_mut(&body.selection_session_id) {
            if matches!(entry.outcome, SelectionOutcome::Completing) {
                entry.outcome = outcome.clone();
            } else {
                let projection = selection_projection(&entry.outcome);
                return (StatusCode::OK, projection);
            }
        }
    }

    (StatusCode::OK, selection_projection(&outcome))
}

/// Resolve the terminal outcome for a bound callback. Fail-closed at
/// every step; only daemon-side provider validation can produce `Ready`.
async fn resolve_selection_outcome(
    state: &AppState,
    body: &ResourceSelectionCallbackRequest,
    session: &ResourceSelectionSession,
) -> SelectionOutcome {
    if let Some(error) = &body.error {
        return SelectionOutcome::Failed {
            state: ProviderResourceState::ActionNeeded,
            message: format!("Google returned an authorization error: {error}."),
        };
    }
    if body.picked_file_ids.is_empty() {
        return SelectionOutcome::Failed {
            state: ProviderResourceState::NotReady,
            message: "No folder was selected.".into(),
        };
    }
    if body.picked_file_ids.len() > 1 {
        return SelectionOutcome::Failed {
            state: ProviderResourceState::NotReady,
            message: "Select exactly one folder.".into(),
        };
    }
    let selected_id = body.picked_file_ids[0].clone();

    let Some(code) = body.code.as_deref().filter(|c| !c.is_empty()) else {
        return SelectionOutcome::Failed {
            state: ProviderResourceState::ActionNeeded,
            message: "Authorization code is missing from the Picker redirect.".into(),
        };
    };

    // #1135: consume the EXACT application config and PKCE verifier pinned
    // at start. Re-resolving here could switch client identity mid-flight
    // after a Cloud config rotation; new attempts pick up new config.
    // Credential exchange/rotation flows through the shared #1132 seam;
    // a failed exchange or keyring write means the credential is
    // unavailable and readiness fails closed.
    let token_data = match crate::routes::exchange_and_persist_google_token(
        state,
        &session.google_oauth_config,
        code,
        &session.pkce_verifier,
    )
    .await
    {
        Ok(data) => data,
        Err(crate::routes::GoogleTokenPersistError::Exchange(message)) => {
            return SelectionOutcome::Failed {
                state: ProviderResourceState::ActionNeeded,
                message: format!("Google token exchange failed: {message}"),
            };
        }
        Err(crate::routes::GoogleTokenPersistError::UnusableBundle) => {
            return SelectionOutcome::Failed {
                state: ProviderResourceState::ActionNeeded,
                message: "Google returned an unusable token bundle.".into(),
            };
        }
        Err(crate::routes::GoogleTokenPersistError::Persist(message)) => {
            return SelectionOutcome::Failed {
                state: ProviderResourceState::ActionNeeded,
                message,
            };
        }
    };

    let metadata = match state
        .drive_resource_probe
        .get_file_metadata(&token_data.access_token, &selected_id)
        .await
    {
        Ok(metadata) => metadata,
        Err(error) => {
            let (resource_state, message) = probe_error_state(&error);
            return SelectionOutcome::Failed {
                state: resource_state,
                message,
            };
        }
    };

    match validate_folder_metadata(&selected_id, &metadata) {
        Ok(descriptor) => SelectionOutcome::Ready(descriptor),
        Err((resource_state, message)) => SelectionOutcome::Failed {
            state: resource_state,
            message,
        },
    }
}

/// `GET /setup/google/resource-selection/:session_id` — safe projection.
async fn resource_selection_status(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> (StatusCode, Json<ResourceSelectionProjection>) {
    let session = {
        let sessions = state.resource_selection_sessions.read().await;
        sessions.get(&session_id).cloned()
    };
    match session {
        Some(session) if !session.is_expired(Utc::now()) => {
            (StatusCode::OK, Json(selection_projection(&session.outcome)))
        }
        _ => (
            StatusCode::NOT_FOUND,
            Json(ResourceSelectionProjection {
                provider: "google".into(),
                state: ProviderResourceState::NotReady,
                message: "Selection session not found or expired.".into(),
                resource: None,
            }),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder_metadata() -> DriveFileMetadata {
        DriveFileMetadata {
            id: "folder-123".into(),
            name: "Bicameral Events".into(),
            mime_type: DRIVE_FOLDER_MIME_TYPE.into(),
            trashed: false,
            drive_id: None,
            can_add_children: true,
            can_list_children: Some(true),
        }
    }

    #[test]
    fn valid_folder_produces_redacted_descriptor() {
        let descriptor = validate_folder_metadata("folder-123", &folder_metadata()).unwrap();
        assert_eq!(descriptor.provider, "google");
        assert_eq!(descriptor.resource_kind, "drive_folder");
        assert_eq!(descriptor.folder_id, "folder-123");
        assert_eq!(descriptor.mime_type, DRIVE_FOLDER_MIME_TYPE);
        assert!(descriptor.can_add_children);
    }

    #[test]
    fn shared_drive_folder_retains_drive_id_as_safe_metadata() {
        let mut metadata = folder_metadata();
        metadata.drive_id = Some("shared-drive-9".into());
        let descriptor = validate_folder_metadata("folder-123", &metadata).unwrap();
        assert_eq!(descriptor.drive_id.as_deref(), Some("shared-drive-9"));
    }

    #[test]
    fn non_folder_fails_closed() {
        let mut metadata = folder_metadata();
        metadata.mime_type = "application/pdf".into();
        let (state, _) = validate_folder_metadata("folder-123", &metadata).unwrap_err();
        assert_eq!(state, ProviderResourceState::NotReady);
    }

    #[test]
    fn trashed_folder_fails_closed() {
        let mut metadata = folder_metadata();
        metadata.trashed = true;
        let (state, _) = validate_folder_metadata("folder-123", &metadata).unwrap_err();
        assert_eq!(state, ProviderResourceState::NotReady);
    }

    #[test]
    fn cannot_add_children_fails_closed_as_action_needed() {
        let mut metadata = folder_metadata();
        metadata.can_add_children = false;
        let (state, _) = validate_folder_metadata("folder-123", &metadata).unwrap_err();
        assert_eq!(state, ProviderResourceState::ActionNeeded);
    }

    #[test]
    fn mismatched_resource_identity_fails_closed() {
        let (state, _) = validate_folder_metadata("folder-OTHER", &folder_metadata()).unwrap_err();
        assert_eq!(state, ProviderResourceState::NotReady);
    }

    #[test]
    fn picker_url_constrains_scope_and_selection() {
        let config = bicameral_config::GoogleOAuthConfig {
            client_id: "client-id".into(),
            client_secret: String::new(),
            redirect_uri: "http://localhost:3000/cb".into(),
            scopes: vec!["https://www.googleapis.com/auth/drive.file".into()],
        };
        let verifier = crate::google_oauth::generate_pkce_verifier();
        let challenge = crate::google_oauth::pkce_s256_challenge(&verifier);
        let url = picker_authorization_url(&config, "state-1", &challenge);
        assert!(url.contains("drive.file"));
        assert!(!url.contains("drive.readonly"));
        assert!(url.contains("trigger_onepick=true"));
        assert!(url.contains("allow_folder_selection=true"));
        assert!(!url.contains("multiselect"));
        assert!(url.contains("access_type=offline"));
        assert!(url.contains("prompt=consent"));
        // #1135: fresh PKCE S256 on every attempt; the verifier itself
        // never appears in the browser URL.
        assert!(url.contains("code_challenge="));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(!url.contains(&verifier));
    }

    fn test_session(now: DateTime<Utc>) -> ResourceSelectionSession {
        ResourceSelectionSession {
            session_id: "rsel-1".into(),
            state_token: "state-1".into(),
            created_at: now,
            outcome: SelectionOutcome::Pending,
            google_oauth_config: test_google_config(),
            pkce_verifier: crate::google_oauth::generate_pkce_verifier(),
        }
    }

    fn ready_descriptor(folder_id: &str) -> ProviderResourceDescriptor {
        ProviderResourceDescriptor {
            provider: "google".into(),
            resource_kind: "drive_folder".into(),
            folder_id: folder_id.into(),
            folder_display_name: "Bicameral Events".into(),
            drive_id: None,
            mime_type: DRIVE_FOLDER_MIME_TYPE.into(),
            can_add_children: true,
            can_list_children: Some(true),
        }
    }

    fn create_command(folder_id: Option<&str>) -> bicameral_api::tool_request::ToolCommand {
        bicameral_api::tool_request::ToolCommand::ProductCreate(
            bicameral_api::product_creation::ProductCreateParams {
                idempotency_key: "k".into(),
                product_display_name: "P".into(),
                event_store_descriptor: folder_id.map(|id| {
                    bicameral_api::product_creation::ProductEventStoreDescriptor::Google {
                        folder_id: id.into(),
                    }
                }),
                asserted_cloud_account_id: Some("acct".into()),
                team_display_name: None,
                verified_domain: None,
                event_store_hint: None,
                required_connection: None,
            },
        )
    }

    async fn state_with_session(session: ResourceSelectionSession) -> AppState {
        let dir = tempfile::tempdir().unwrap();
        let audit = bicameral_audit::store::AuditStore::open(dir.path()).unwrap();
        let state = AppState::new(audit);
        state
            .resource_selection_sessions
            .write()
            .await
            .insert(session.session_id.clone(), session);
        // Keep the tempdir alive for the test body by leaking it; audit
        // stores in these unit tests are never read back.
        std::mem::forget(dir);
        state
    }

    #[tokio::test]
    async fn validated_folder_requires_a_live_ready_session_for_the_exact_folder() {
        let now = Utc::now();
        let mut session = test_session(now);
        session.outcome = SelectionOutcome::Ready(ready_descriptor("folder-abc"));
        let state = state_with_session(session).await;

        assert_eq!(
            validated_event_store_folder(&state, &create_command(Some("folder-abc"))).await,
            Some("folder-abc".to_string())
        );
        // A different folder the daemon never validated is not stamped.
        assert_eq!(
            validated_event_store_folder(&state, &create_command(Some("folder-other"))).await,
            None
        );
        // No descriptor at all is never stamped.
        assert_eq!(
            validated_event_store_folder(&state, &create_command(None)).await,
            None
        );
    }

    #[tokio::test]
    async fn validated_folder_ignores_pending_failed_and_expired_sessions() {
        let now = Utc::now();
        let pending = test_session(now);
        let state = state_with_session(pending).await;
        assert_eq!(
            validated_event_store_folder(&state, &create_command(Some("folder-abc"))).await,
            None
        );

        let mut failed = test_session(now);
        failed.session_id = "rsel-failed".into();
        failed.outcome = SelectionOutcome::Failed {
            state: ProviderResourceState::NotReady,
            message: "Select exactly one folder.".into(),
        };
        let state = state_with_session(failed).await;
        assert_eq!(
            validated_event_store_folder(&state, &create_command(Some("folder-abc"))).await,
            None
        );

        let mut expired =
            test_session(now - chrono::Duration::seconds(SELECTION_SESSION_TTL_SECONDS + 1));
        expired.session_id = "rsel-expired".into();
        expired.outcome = SelectionOutcome::Ready(ready_descriptor("folder-abc"));
        let state = state_with_session(expired).await;
        assert_eq!(
            validated_event_store_folder(&state, &create_command(Some("folder-abc"))).await,
            None,
            "a stale selection must not validate a create"
        );
    }

    #[tokio::test]
    async fn non_create_commands_are_never_stamped() {
        let now = Utc::now();
        let mut session = test_session(now);
        session.outcome = SelectionOutcome::Ready(ready_descriptor("folder-abc"));
        let state = state_with_session(session).await;
        let publish = bicameral_api::tool_request::ToolCommand::ProductPublish(
            bicameral_api::product_creation::ProductPublishParams {
                product_id: "prd_x".into(),
                idempotency_key: "k".into(),
            },
        );
        assert_eq!(validated_event_store_folder(&state, &publish).await, None);
    }

    #[test]
    fn stored_desktop_credential_requires_a_binding_matched_envelope() {
        use bicameral_config::SecretsStore;
        let store = bicameral_config::InMemorySecretsStore::new();
        // Absent envelope: fail closed.
        assert_eq!(
            stored_desktop_credential(&store, "client-a", Some("1")),
            None
        );

        let envelope = bicameral_config::GoogleClientCredentialEnvelopeV1::new(
            "desktop-secret",
            "client-a",
            "1",
            Utc::now(),
        )
        .unwrap();
        store
            .write_secret(
                bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
                bicameral_config::GOOGLE_CLIENT_CREDENTIAL_KEY,
                &envelope.to_secret_string().unwrap(),
            )
            .unwrap();

        assert_eq!(
            stored_desktop_credential(&store, "client-a", Some("1")).as_deref(),
            Some("desktop-secret")
        );
        // Rotated client id or config version: fail closed.
        assert_eq!(
            stored_desktop_credential(&store, "client-b", Some("1")),
            None
        );
        assert_eq!(
            stored_desktop_credential(&store, "client-a", Some("2")),
            None
        );
    }

    #[test]
    fn session_expiry_is_bounded() {
        let now = Utc::now();
        let session = test_session(now);
        assert!(!session.is_expired(now));
        assert!(
            session.is_expired(now + chrono::Duration::seconds(SELECTION_SESSION_TTL_SECONDS + 1))
        );
    }

    #[test]
    fn probe_error_states_are_typed_and_safe() {
        let (state, message) = probe_error_state(&DriveProbeError::CredentialRejected);
        assert_eq!(state, ProviderResourceState::ActionNeeded);
        assert!(!message.contains("token"));
        let (state, _) = probe_error_state(&DriveProbeError::NotAccessible);
        assert_eq!(state, ProviderResourceState::NotReady);
    }

    // ── Endpoint tests ────────────────────────────────────────────

    use axum::body::Body;
    use axum::http::Request;
    use std::sync::Arc;
    use tower::ServiceExt;

    /// Deterministic probe double returning a fixed result.
    struct FixedProbe(Result<DriveFileMetadata, DriveProbeError>);

    impl DriveResourceProbe for FixedProbe {
        fn get_file_metadata<'a>(
            &'a self,
            _access_token: &'a str,
            _file_id: &'a str,
        ) -> DriveProbeFuture<'a> {
            let result = self.0.clone();
            Box::pin(async move { result })
        }
    }

    fn test_google_config() -> bicameral_config::GoogleOAuthConfig {
        bicameral_config::GoogleOAuthConfig {
            client_id: "test-client-id".into(),
            client_secret: "test-client-secret".into(),
            redirect_uri: "http://localhost:3000/api/v1/setup/google/resource-selection/callback"
                .into(),
            scopes: vec!["https://www.googleapis.com/auth/drive.file".into()],
        }
    }

    /// Spawn a local Google token-endpoint stub (the #1132 test seam),
    /// recording form bodies.
    async fn spawn_token_stub(
        status: StatusCode,
        body: serde_json::Value,
    ) -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
        use axum::routing::post;
        let recorded: Arc<std::sync::Mutex<Vec<String>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = recorded.clone();
        let response_body = body.to_string();
        let app = Router::new().route(
            "/token",
            post(move |request_body: String| {
                let sink = sink.clone();
                let response_body = response_body.clone();
                async move {
                    sink.lock().unwrap().push(request_body);
                    (
                        status,
                        [("content-type", "application/json")],
                        response_body,
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{address}/token"), recorded)
    }

    fn stub_token_response() -> serde_json::Value {
        serde_json::json!({
            "access_token": "stub-access-token",
            "refresh_token": "stub-refresh-token",
            "expires_in": 3599,
            "token_type": "Bearer",
            "scope": "https://www.googleapis.com/auth/drive.file"
        })
    }

    async fn selection_app_with_recorder(
        probe: Arc<dyn DriveResourceProbe>,
    ) -> (Router, AppState, Arc<std::sync::Mutex<Vec<String>>>) {
        let (endpoint, recorded) = spawn_token_stub(StatusCode::OK, stub_token_response()).await;
        let dir = tempfile::tempdir().unwrap();
        let audit = bicameral_audit::store::AuditStore::open(dir.path()).unwrap();
        let mut state = AppState::new(audit).with_drive_resource_probe(probe);
        state.google_oauth_config = Some(test_google_config());
        state.google_oauth_allow_env_override = true;
        state.google_token_endpoint_override = Some(endpoint);
        let app = Router::new()
            .merge(resource_selection_routes())
            .with_state(state.clone());
        (app, state, recorded)
    }

    async fn selection_app(probe: Arc<dyn DriveResourceProbe>) -> (Router, AppState) {
        let (app, state, _recorded) = selection_app_with_recorder(probe).await;
        (app, state)
    }

    /// Drive the REAL browser GET loopback shape and return
    /// (status, headers, body).
    async fn get_loopback(
        app: &Router,
        query: &str,
    ) -> (StatusCode, axum::http::HeaderMap, String) {
        let req = Request::builder()
            .method("GET")
            .uri(format!(
                "/api/v1/setup/google/resource-selection/callback{query}"
            ))
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
        (status, headers, String::from_utf8(bytes.to_vec()).unwrap())
    }

    async fn post_json(
        app: &Router,
        uri: &str,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let req = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 16384).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    async fn get_json(app: &Router, uri: &str) -> (StatusCode, serde_json::Value) {
        let req = Request::builder()
            .method("GET")
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 16384).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    /// Start a selection and return (session_id, state_token).
    async fn start_selection(app: &Router) -> (String, String) {
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/setup/google/resource-selection/start")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 16384).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["outcome"], "start_url");
        let url = json["url"].as_str().unwrap();
        assert!(url.contains("trigger_onepick=true"));
        assert!(url.contains("allow_folder_selection=true"));
        assert!(url.contains("drive.file"));
        let state_token = url
            .split("state=")
            .nth(1)
            .unwrap_or("")
            .split('&')
            .next()
            .unwrap_or("")
            .to_string();
        (
            json["selection_session_id"].as_str().unwrap().to_string(),
            state_token,
        )
    }

    fn callback_body(session_id: &str, state_token: &str, picked: &[&str]) -> serde_json::Value {
        serde_json::json!({
            "selection_session_id": session_id,
            "code": "picker-code-1",
            "state": state_token,
            "picked_file_ids": picked,
        })
    }

    #[tokio::test]
    async fn happy_path_selects_validates_and_rotates_credential() {
        let (app, state) = selection_app(Arc::new(FixedProbe(Ok(folder_metadata())))).await;
        let (session_id, state_token) = start_selection(&app).await;

        let (status, json) = post_json(
            &app,
            "/api/v1/setup/google/resource-selection/callback",
            callback_body(&session_id, &state_token, &["folder-123"]),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["state"], "ready");
        assert_eq!(json["resource"]["folder_id"], "folder-123");
        assert_eq!(json["resource"]["resource_kind"], "drive_folder");

        // No token material anywhere in the projection.
        let raw = json.to_string();
        assert!(!raw.contains("access_token"));
        assert!(!raw.contains("picker-code"));
        assert!(!raw.contains("refresh"));

        // Credential rotation flowed through the #1084 canonical seam.
        let stored = state
            .secrets_store
            .read_secret(
                bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
                bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
            )
            .expect("Picker exchange must persist the canonical envelope");
        assert!(matches!(
            bicameral_config::parse_stored_oauth_token(&stored),
            Some(bicameral_config::StoredOAuthToken::Envelope(_))
        ));

        // Poll returns the same safe readiness.
        let (status, json) = get_json(
            &app,
            &format!("/api/v1/setup/google/resource-selection/{session_id}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["state"], "ready");
    }

    #[tokio::test]
    async fn wrong_state_is_rejected_without_consuming_the_session() {
        let (app, _state) = selection_app(Arc::new(FixedProbe(Ok(folder_metadata())))).await;
        let (session_id, _state_token) = start_selection(&app).await;

        let (status, json) = post_json(
            &app,
            "/api/v1/setup/google/resource-selection/callback",
            callback_body(&session_id, "wrong-state-token", &["folder-123"]),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["state"], "not_ready");

        // The session remains pending; the mismatched caller neither
        // advanced nor killed it.
        let (_, json) = get_json(
            &app,
            &format!("/api/v1/setup/google/resource-selection/{session_id}"),
        )
        .await;
        assert_eq!(json["state"], "pending_selection");
    }

    #[tokio::test]
    async fn unknown_session_is_not_found() {
        let (app, _state) = selection_app(Arc::new(FixedProbe(Ok(folder_metadata())))).await;
        let (status, json) = post_json(
            &app,
            "/api/v1/setup/google/resource-selection/callback",
            callback_body("rsel-unknown", "any", &["folder-123"]),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["state"], "not_ready");

        let (status, _) =
            get_json(&app, "/api/v1/setup/google/resource-selection/rsel-unknown").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn zero_or_multiple_picks_fail_closed() {
        let (app, _state) = selection_app(Arc::new(FixedProbe(Ok(folder_metadata())))).await;

        let (session_id, state_token) = start_selection(&app).await;
        let (_, json) = post_json(
            &app,
            "/api/v1/setup/google/resource-selection/callback",
            callback_body(&session_id, &state_token, &[]),
        )
        .await;
        assert_eq!(json["state"], "not_ready");

        let (session_id, state_token) = start_selection(&app).await;
        let (_, json) = post_json(
            &app,
            "/api/v1/setup/google/resource-selection/callback",
            callback_body(&session_id, &state_token, &["folder-1", "folder-2"]),
        )
        .await;
        assert_eq!(json["state"], "not_ready");
    }

    #[tokio::test]
    async fn missing_code_or_config_fails_closed_as_action_needed() {
        let (app, _state) = selection_app(Arc::new(FixedProbe(Ok(folder_metadata())))).await;
        let (session_id, state_token) = start_selection(&app).await;
        let mut body = callback_body(&session_id, &state_token, &["folder-123"]);
        body.as_object_mut().unwrap().remove("code");
        let (_, json) = post_json(
            &app,
            "/api/v1/setup/google/resource-selection/callback",
            body,
        )
        .await;
        assert_eq!(json["state"], "action_needed");

        // Start with no legacy config and no reachable Cloud is
        // action_needed, never a URL (resolver fails closed).
        let dir = tempfile::tempdir().unwrap();
        let audit = bicameral_audit::store::AuditStore::open(dir.path()).unwrap();
        let mut state = AppState::new(audit);
        state.google_oauth_config = None;
        state.google_cloud_origin_override = Some("http://127.0.0.1:9".into());
        let app = Router::new()
            .merge(resource_selection_routes())
            .with_state(state);
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/setup/google/resource-selection/start")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), 16384).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["outcome"], "action_needed");
    }

    #[tokio::test]
    async fn provider_error_param_fails_closed() {
        let (app, _state) = selection_app(Arc::new(FixedProbe(Ok(folder_metadata())))).await;
        let (session_id, state_token) = start_selection(&app).await;
        let mut body = callback_body(&session_id, &state_token, &["folder-123"]);
        body.as_object_mut()
            .unwrap()
            .insert("error".into(), "access_denied".into());
        let (_, json) = post_json(
            &app,
            "/api/v1/setup/google/resource-selection/callback",
            body,
        )
        .await;
        assert_eq!(json["state"], "action_needed");
    }

    #[tokio::test]
    async fn invalid_resources_never_project_ready() {
        // Non-folder.
        let mut metadata = folder_metadata();
        metadata.mime_type = "application/pdf".into();
        let (app, _s) = selection_app(Arc::new(FixedProbe(Ok(metadata)))).await;
        let (sid, tok) = start_selection(&app).await;
        let (_, json) = post_json(
            &app,
            "/api/v1/setup/google/resource-selection/callback",
            callback_body(&sid, &tok, &["folder-123"]),
        )
        .await;
        assert_eq!(json["state"], "not_ready");

        // Trashed folder.
        let mut metadata = folder_metadata();
        metadata.trashed = true;
        let (app, _s) = selection_app(Arc::new(FixedProbe(Ok(metadata)))).await;
        let (sid, tok) = start_selection(&app).await;
        let (_, json) = post_json(
            &app,
            "/api/v1/setup/google/resource-selection/callback",
            callback_body(&sid, &tok, &["folder-123"]),
        )
        .await;
        assert_eq!(json["state"], "not_ready");

        // Insufficient capability.
        let mut metadata = folder_metadata();
        metadata.can_add_children = false;
        let (app, _s) = selection_app(Arc::new(FixedProbe(Ok(metadata)))).await;
        let (sid, tok) = start_selection(&app).await;
        let (_, json) = post_json(
            &app,
            "/api/v1/setup/google/resource-selection/callback",
            callback_body(&sid, &tok, &["folder-123"]),
        )
        .await;
        assert_eq!(json["state"], "action_needed");

        // Inaccessible resource.
        let (app, _s) =
            selection_app(Arc::new(FixedProbe(Err(DriveProbeError::NotAccessible)))).await;
        let (sid, tok) = start_selection(&app).await;
        let (_, json) = post_json(
            &app,
            "/api/v1/setup/google/resource-selection/callback",
            callback_body(&sid, &tok, &["folder-123"]),
        )
        .await;
        assert_eq!(json["state"], "not_ready");

        // Credential rejected by provider.
        let (app, _s) = selection_app(Arc::new(FixedProbe(Err(
            DriveProbeError::CredentialRejected,
        ))))
        .await;
        let (sid, tok) = start_selection(&app).await;
        let (_, json) = post_json(
            &app,
            "/api/v1/setup/google/resource-selection/callback",
            callback_body(&sid, &tok, &["folder-123"]),
        )
        .await;
        assert_eq!(json["state"], "action_needed");
    }

    /// A secrets store whose writes fail: the credential is unavailable,
    /// so readiness must fail closed before any provider validation.
    struct FailingWriteStore;
    impl bicameral_config::SecretsStore for FailingWriteStore {
        fn read_secret(&self, _s: &str, _k: &str) -> Option<String> {
            None
        }
        fn write_secret(&self, _s: &str, _k: &str, _v: &str) -> anyhow::Result<()> {
            Err(anyhow::anyhow!("simulated keyring unavailable"))
        }
        fn delete_secret(&self, _s: &str, _k: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn credential_unavailable_fails_closed() {
        let (endpoint, _recorded) = spawn_token_stub(StatusCode::OK, stub_token_response()).await;
        let dir = tempfile::tempdir().unwrap();
        let audit = bicameral_audit::store::AuditStore::open(dir.path()).unwrap();
        let mut state = AppState::new(audit)
            .with_secrets_store(Arc::new(FailingWriteStore))
            .with_drive_resource_probe(Arc::new(FixedProbe(Ok(folder_metadata()))));
        state.google_oauth_config = Some(test_google_config());
        state.google_oauth_allow_env_override = true;
        state.google_token_endpoint_override = Some(endpoint);
        let app = Router::new()
            .merge(resource_selection_routes())
            .with_state(state.clone());

        let (session_id, state_token) = start_selection(&app).await;
        let (_, json) = post_json(
            &app,
            "/api/v1/setup/google/resource-selection/callback",
            callback_body(&session_id, &state_token, &["folder-123"]),
        )
        .await;
        assert_eq!(json["state"], "action_needed");
        // No secret material and no descriptor in the failure.
        let raw = json.to_string();
        assert!(json["resource"].is_null());
        assert!(!raw.contains("picker-code"));
    }

    #[tokio::test]
    async fn expired_session_cannot_be_advanced_or_polled() {
        let (app, state) = selection_app(Arc::new(FixedProbe(Ok(folder_metadata())))).await;
        let (session_id, state_token) = start_selection(&app).await;

        // Force expiry.
        {
            let mut sessions = state.resource_selection_sessions.write().await;
            let session = sessions.get_mut(&session_id).unwrap();
            session.created_at =
                Utc::now() - chrono::Duration::seconds(SELECTION_SESSION_TTL_SECONDS + 5);
        }

        let (status, _) = post_json(
            &app,
            "/api/v1/setup/google/resource-selection/callback",
            callback_body(&session_id, &state_token, &["folder-123"]),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, _) = get_json(
            &app,
            &format!("/api/v1/setup/google/resource-selection/{session_id}"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    // ── #1135 Cloud-first fresh-install composition ───────────────

    fn cloud_app_config_payload(client_id: &str) -> serde_json::Value {
        let now = Utc::now();
        serde_json::json!({
            "schema_version": 1,
            "provider": "google",
            "client_type": "native",
            "client_id": client_id,
            "environment": "production",
            "config_version": "google-production-1",
            "scope_set_version": "drive-file-v1",
            "scopes": ["https://www.googleapis.com/auth/drive.file"],
            "issued_at": (now - chrono::Duration::minutes(1)).to_rfc3339(),
            "expires_at": (now + chrono::Duration::minutes(30)).to_rfc3339(),
            "enabled": true,
        })
    }

    /// Serve a mutable Cloud app-config payload at the governed path.
    async fn spawn_cloud_config_stub(
        payload: serde_json::Value,
    ) -> (String, Arc<std::sync::Mutex<serde_json::Value>>) {
        use axum::routing::get;
        let current = Arc::new(std::sync::Mutex::new(payload));
        let source = current.clone();
        let app = Router::new().route(
            "/api/v1/provider-app-config/google",
            get(move || {
                let source = source.clone();
                async move {
                    (
                        StatusCode::OK,
                        [("content-type", "application/json")],
                        source.lock().unwrap().to_string(),
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{address}"), current)
    }

    /// #1135 fresh-install integration: Cloud app-config resolution →
    /// Picker start (NO injected client id/secret anywhere) → PKCE/state
    /// → callback → real-transport token exchange (stub endpoint, no
    /// client_secret on the wire) → envelope persisted → probe → Ready.
    #[tokio::test]
    async fn fresh_install_composes_cloud_config_pkce_and_readiness() {
        let (cloud_origin, _payload) =
            spawn_cloud_config_stub(cloud_app_config_payload("cloud-native-client")).await;
        let (token_endpoint, recorded) =
            spawn_token_stub(StatusCode::OK, stub_token_response()).await;

        let dir = tempfile::tempdir().unwrap();
        let audit = bicameral_audit::store::AuditStore::open(dir.path()).unwrap();
        let mut state = AppState::new(audit)
            .with_drive_resource_probe(Arc::new(FixedProbe(Ok(folder_metadata()))));
        // Fresh install: NO legacy env config, NO override, Cloud only.
        state.google_oauth_config = None;
        state.google_oauth_allow_env_override = false;
        state.google_cloud_origin_override = Some(cloud_origin);
        state.google_token_endpoint_override = Some(token_endpoint);
        let app = Router::new()
            .merge(resource_selection_routes())
            .with_state(state.clone());

        let (session_id, state_token) = start_selection(&app).await;

        // The start URL is PKCE-bound to the Cloud-published client.
        // (start_selection already asserted drive.file + one-pick.)
        let (status, json) = post_json(
            &app,
            "/api/v1/setup/google/resource-selection/callback",
            callback_body(&session_id, &state_token, &["folder-123"]),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["state"], "ready");

        // Native/public exchange: Cloud client id on the wire, no
        // client_secret, PKCE verifier present.
        let bodies = recorded.lock().unwrap().clone();
        assert_eq!(bodies.len(), 1);
        assert!(bodies[0].contains("client_id=cloud-native-client"));
        assert!(!bodies[0].contains("client_secret"));
        assert!(bodies[0].contains("code_verifier="));

        // The canonical envelope was persisted daemon-side only.
        assert!(state
            .secrets_store
            .read_secret(
                bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
                bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
            )
            .is_some());
    }

    /// #1135: app-config rotation after start does not switch the
    /// in-flight OAuth client identity; the callback consumes the exact
    /// pinned config, and a NEW attempt uses the rotated config.
    #[tokio::test]
    async fn config_rotation_mid_flight_keeps_pinned_identity() {
        let (cloud_origin, payload) =
            spawn_cloud_config_stub(cloud_app_config_payload("client-generation-1")).await;
        let (token_endpoint, recorded) =
            spawn_token_stub(StatusCode::OK, stub_token_response()).await;

        let dir = tempfile::tempdir().unwrap();
        let audit = bicameral_audit::store::AuditStore::open(dir.path()).unwrap();
        let mut state = AppState::new(audit)
            .with_drive_resource_probe(Arc::new(FixedProbe(Ok(folder_metadata()))));
        state.google_oauth_config = None;
        state.google_oauth_allow_env_override = false;
        state.google_cloud_origin_override = Some(cloud_origin);
        state.google_token_endpoint_override = Some(token_endpoint);
        let app = Router::new()
            .merge(resource_selection_routes())
            .with_state(state.clone());

        let (session_id, state_token) = start_selection(&app).await;

        // Cloud rotates the client identity while the browser flow is
        // in flight.
        *payload.lock().unwrap() = cloud_app_config_payload("client-generation-2");

        let (_, json) = post_json(
            &app,
            "/api/v1/setup/google/resource-selection/callback",
            callback_body(&session_id, &state_token, &["folder-123"]),
        )
        .await;
        assert_eq!(json["state"], "ready");

        // The exchange used the PINNED generation-1 identity.
        let bodies = recorded.lock().unwrap().clone();
        assert_eq!(bodies.len(), 1);
        assert!(bodies[0].contains("client_id=client-generation-1"));

        // A new attempt resolves the rotated generation-2 identity.
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/setup/google/resource-selection/start")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), 16384).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["outcome"], "start_url");
        assert!(json["url"]
            .as_str()
            .unwrap()
            .contains("client_id=client-generation-2"));
    }

    // ── #1135 review fix: real GET loopback + one-shot semantics ──

    #[test]
    fn selection_redirect_uri_substitutes_only_the_path() {
        assert_eq!(
            selection_redirect_uri("http://127.0.0.1:37373/api/v1/connections/google/callback"),
            format!("http://127.0.0.1:37373{RESOURCE_SELECTION_CALLBACK_PATH}")
        );
        // Environment-specific origins survive; only the path changes.
        assert_eq!(
            selection_redirect_uri("http://127.0.0.1:7411/api/v1/connections/google/callback"),
            format!("http://127.0.0.1:7411{RESOURCE_SELECTION_CALLBACK_PATH}")
        );
        // A shapeless value falls back to the canonical loopback origin.
        assert_eq!(
            selection_redirect_uri("not-a-uri"),
            format!(
                "http://127.0.0.1:{}{RESOURCE_SELECTION_CALLBACK_PATH}",
                bicameral_config::DEFAULT_GATEWAY_PORT
            )
        );
    }

    /// End-to-end deterministic regression in the ACTUAL browser query
    /// shape: Google GET-redirects to the selection callback, the opaque
    /// state resolves the resource-selection session, the pinned
    /// config/verifier drive the real-transport exchange, the probe
    /// validates the folder, and a static safe page is returned.
    #[tokio::test]
    async fn real_get_loopback_completes_selection_end_to_end() {
        let (app, state, recorded) =
            selection_app_with_recorder(Arc::new(FixedProbe(Ok(folder_metadata())))).await;
        let (session_id, state_token) = start_selection(&app).await;

        // The redirect URI sent to Google identifies THIS callback.
        {
            let sessions = state.resource_selection_sessions.read().await;
            let session = sessions.get(&session_id).unwrap();
            assert!(session
                .google_oauth_config
                .redirect_uri
                .ends_with(RESOURCE_SELECTION_CALLBACK_PATH));
        }

        let (status, headers, html) = get_loopback(
            &app,
            &format!("?state={state_token}&code=picker-code-1&scope=drive.file&picked_file_ids=folder-123"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(html.contains("Google Drive folder selected"));

        // Static safe page: protective headers, zero reflection.
        assert_eq!(headers["cache-control"], "no-store");
        assert_eq!(headers["referrer-policy"], "no-referrer");
        assert_eq!(headers["x-content-type-options"], "nosniff");
        assert!(headers.contains_key("content-security-policy"));
        assert!(!html.contains(&session_id));
        assert!(!html.contains(&state_token));
        assert!(!html.contains("picker-code-1"));
        assert!(!html.contains("folder-123"));
        assert!(!html.contains("stub-access-token"));

        // The exchange ran once over the real transport with the pinned
        // verifier and the selection redirect URI.
        let bodies = recorded.lock().unwrap().clone();
        assert_eq!(bodies.len(), 1);
        assert!(bodies[0].contains("code_verifier="));
        assert!(bodies[0].contains("resource-selection%2Fcallback"));

        // The canonical envelope was persisted daemon-side.
        assert!(state
            .secrets_store
            .read_secret(
                bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
                bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
            )
            .is_some());

        // Polling still projects the safe terminal readiness.
        let (status, json) = get_json(
            &app,
            &format!("/api/v1/setup/google/resource-selection/{session_id}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["state"], "ready");
    }

    /// #1135 review fix: terminal sessions are one-shot. Replays after
    /// Ready — via GET and via POST — return the stored projection and
    /// never re-run the exchange or the probe.
    #[tokio::test]
    async fn replay_after_ready_is_one_shot() {
        let (app, _state, recorded) =
            selection_app_with_recorder(Arc::new(FixedProbe(Ok(folder_metadata())))).await;
        let (session_id, state_token) = start_selection(&app).await;

        let (status, _, html) = get_loopback(
            &app,
            &format!("?state={state_token}&code=picker-code-1&picked_file_ids=folder-123"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(html.contains("folder selected"));
        assert_eq!(recorded.lock().unwrap().len(), 1);

        // Duplicate GET replay: stored projection, no second exchange.
        let (status, _, _) = get_loopback(
            &app,
            &format!("?state={state_token}&code=picker-code-2&picked_file_ids=folder-999"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(recorded.lock().unwrap().len(), 1);

        // Direct POST replay: same one-shot behavior.
        let (status, json) = post_json(
            &app,
            "/api/v1/setup/google/resource-selection/callback",
            callback_body(&session_id, &state_token, &["folder-999"]),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["state"], "ready");
        assert_eq!(
            json["resource"]["folder_id"], "folder-123",
            "replay must return the stored outcome, not process folder-999"
        );
        assert_eq!(recorded.lock().unwrap().len(), 1);
    }

    /// #1135 review fix: provider-denied terminal completion is one-shot
    /// too — a Failed session cannot be re-processed into Ready.
    #[tokio::test]
    async fn replay_after_provider_denied_is_one_shot() {
        let (app, _state, recorded) =
            selection_app_with_recorder(Arc::new(FixedProbe(Ok(folder_metadata())))).await;
        let (session_id, state_token) = start_selection(&app).await;

        let (status, _, html) =
            get_loopback(&app, &format!("?state={state_token}&error=access_denied")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(html.contains("not completed"));
        assert_eq!(recorded.lock().unwrap().len(), 0);

        // Replay with a would-be-valid completion: the terminal Failed
        // outcome stands and no exchange runs.
        let (_, json) = post_json(
            &app,
            "/api/v1/setup/google/resource-selection/callback",
            callback_body(&session_id, &state_token, &["folder-123"]),
        )
        .await;
        assert_eq!(json["state"], "action_needed");
        assert_eq!(recorded.lock().unwrap().len(), 0);
    }

    /// GET with unknown or expired state fails closed without reflection
    /// and cannot advance anything; expired entries are purged.
    #[tokio::test]
    async fn get_loopback_unknown_or_expired_state_fails_closed() {
        let (app, state, recorded) =
            selection_app_with_recorder(Arc::new(FixedProbe(Ok(folder_metadata())))).await;

        let (status, _, html) = get_loopback(&app, "?state=unknown-state&code=secret-code").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(!html.contains("unknown-state"));
        assert!(!html.contains("secret-code"));
        assert!(recorded.lock().unwrap().is_empty());

        // Expired session: purged by the scan, never resurrected.
        let (session_id, state_token) = start_selection(&app).await;
        {
            let mut sessions = state.resource_selection_sessions.write().await;
            let session = sessions.get_mut(&session_id).unwrap();
            session.created_at =
                Utc::now() - chrono::Duration::seconds(SELECTION_SESSION_TTL_SECONDS + 5);
        }
        let (status, _, _) = get_loopback(
            &app,
            &format!("?state={state_token}&code=picker-code-1&picked_file_ids=folder-123"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(!state
            .resource_selection_sessions
            .read()
            .await
            .contains_key(&session_id));
        assert!(recorded.lock().unwrap().is_empty());
    }

    /// Probe double that counts invocations and parks each call until the
    /// test releases the gate — lets a claimant be held deterministically
    /// mid-completion while duplicates race it.
    struct GatedCountingProbe {
        count: Arc<std::sync::atomic::AtomicUsize>,
        gate: Arc<tokio::sync::Notify>,
    }

    impl DriveResourceProbe for GatedCountingProbe {
        fn get_file_metadata<'a>(
            &'a self,
            _access_token: &'a str,
            _file_id: &'a str,
        ) -> DriveProbeFuture<'a> {
            Box::pin(async move {
                self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                self.gate.notified().await;
                Ok(folder_metadata())
            })
        }
    }

    /// Secrets store decorator counting oauth-key writes, so the
    /// concurrency regression can prove exactly one secret mutation.
    struct CountingSecretsStore {
        inner: bicameral_config::InMemorySecretsStore,
        writes: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl bicameral_config::SecretsStore for CountingSecretsStore {
        fn read_secret(&self, source_id: &str, key: &str) -> Option<String> {
            self.inner.read_secret(source_id, key)
        }
        fn write_secret(&self, source_id: &str, key: &str, value: &str) -> anyhow::Result<()> {
            self.writes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.inner.write_secret(source_id, key, value)
        }
        fn delete_secret(&self, source_id: &str, key: &str) -> anyhow::Result<()> {
            self.inner.delete_secret(source_id, key)
        }
    }

    /// #1135 review comment 5323636602: provider completion itself is
    /// one-shot. Two valid callbacks race the same Pending session; the
    /// claimant is parked deterministically inside the Drive probe while
    /// the duplicate runs to completion. The duplicate must execute ZERO
    /// provider side effects, and exactly one terminal result must stand.
    #[tokio::test]
    async fn concurrent_duplicate_callbacks_execute_provider_side_effects_once() {
        use std::sync::atomic::Ordering::SeqCst;

        let probe_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let gate = Arc::new(tokio::sync::Notify::new());
        let probe = Arc::new(GatedCountingProbe {
            count: probe_count.clone(),
            gate: gate.clone(),
        });

        let (endpoint, recorded) = spawn_token_stub(StatusCode::OK, stub_token_response()).await;
        let writes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let dir = tempfile::tempdir().unwrap();
        let audit = bicameral_audit::store::AuditStore::open(dir.path()).unwrap();
        let mut state = AppState::new(audit)
            .with_secrets_store(Arc::new(CountingSecretsStore {
                inner: bicameral_config::InMemorySecretsStore::new(),
                writes: writes.clone(),
            }))
            .with_drive_resource_probe(probe);
        state.google_oauth_config = Some(test_google_config());
        state.google_oauth_allow_env_override = true;
        state.google_token_endpoint_override = Some(endpoint);
        let app = Router::new()
            .merge(resource_selection_routes())
            .with_state(state.clone());

        let (session_id, state_token) = start_selection(&app).await;

        // Claimant: acquires the completion claim, exchanges the code,
        // persists the envelope, then parks inside the Drive probe.
        let claimant_app = app.clone();
        let claimant_session = session_id.clone();
        let claimant_state = state_token.clone();
        let claimant = tokio::spawn(async move {
            post_json(
                &claimant_app,
                "/api/v1/setup/google/resource-selection/callback",
                callback_body(&claimant_session, &claimant_state, &["folder-123"]),
            )
            .await
        });

        // Deterministically wait until the claimant is inside the probe.
        for _ in 0..400 {
            if probe_count.load(SeqCst) == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(
            probe_count.load(SeqCst),
            1,
            "claimant must be parked in the probe"
        );
        assert_eq!(recorded.lock().unwrap().len(), 1);
        assert_eq!(writes.load(SeqCst), 1);

        // Valid duplicate POST while the claim is held: safe non-terminal
        // projection, no second exchange, no second probe, no write.
        let (status, json) = post_json(
            &app,
            "/api/v1/setup/google/resource-selection/callback",
            callback_body(&session_id, &state_token, &["folder-999"]),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["state"], "pending_selection");
        assert!(json["resource"].is_null());

        // Valid duplicate GET loopback while the claim is held: safe
        // non-terminal page, still zero additional provider effects.
        let (status, _, html) = get_loopback(
            &app,
            &format!("?state={state_token}&code=picker-code-2&picked_file_ids=folder-999"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(html.contains("not completed"));

        assert_eq!(
            recorded.lock().unwrap().len(),
            1,
            "token endpoint hit count == 1"
        );
        assert_eq!(probe_count.load(SeqCst), 1, "drive validation count == 1");
        assert_eq!(writes.load(SeqCst), 1, "no duplicate secret mutation");

        // Release the claimant: exactly one stable terminal result.
        gate.notify_one();
        let (status, json) = claimant.await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["state"], "ready");
        assert_eq!(json["resource"]["folder_id"], "folder-123");

        assert_eq!(recorded.lock().unwrap().len(), 1);
        assert_eq!(probe_count.load(SeqCst), 1);
        assert_eq!(writes.load(SeqCst), 1);

        // The terminal outcome is the claimant's and cannot be overwritten
        // by a late duplicate carrying different picks.
        let (_, json) = get_json(
            &app,
            &format!("/api/v1/setup/google/resource-selection/{session_id}"),
        )
        .await;
        assert_eq!(json["state"], "ready");
        let (_, json) = post_json(
            &app,
            "/api/v1/setup/google/resource-selection/callback",
            callback_body(&session_id, &state_token, &["folder-999"]),
        )
        .await;
        assert_eq!(json["state"], "ready");
        assert_eq!(json["resource"]["folder_id"], "folder-123");
        assert_eq!(recorded.lock().unwrap().len(), 1);
        assert_eq!(probe_count.load(SeqCst), 1);
        assert_eq!(writes.load(SeqCst), 1);
    }

    /// A mismatched state arriving while the claim is held neither claims
    /// nor kills the in-flight attempt.
    #[tokio::test]
    async fn state_mismatch_during_completing_cannot_claim_or_kill() {
        use std::sync::atomic::Ordering::SeqCst;

        let probe_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let gate = Arc::new(tokio::sync::Notify::new());
        let probe = Arc::new(GatedCountingProbe {
            count: probe_count.clone(),
            gate: gate.clone(),
        });
        let (endpoint, recorded) = spawn_token_stub(StatusCode::OK, stub_token_response()).await;
        let dir = tempfile::tempdir().unwrap();
        let audit = bicameral_audit::store::AuditStore::open(dir.path()).unwrap();
        let mut state = AppState::new(audit).with_drive_resource_probe(probe);
        state.google_oauth_config = Some(test_google_config());
        state.google_oauth_allow_env_override = true;
        state.google_token_endpoint_override = Some(endpoint);
        let app = Router::new()
            .merge(resource_selection_routes())
            .with_state(state.clone());

        let (session_id, state_token) = start_selection(&app).await;
        let claimant_app = app.clone();
        let claimant_session = session_id.clone();
        let claimant_state = state_token.clone();
        let claimant = tokio::spawn(async move {
            post_json(
                &claimant_app,
                "/api/v1/setup/google/resource-selection/callback",
                callback_body(&claimant_session, &claimant_state, &["folder-123"]),
            )
            .await
        });
        for _ in 0..400 {
            if probe_count.load(SeqCst) == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        let (_, json) = post_json(
            &app,
            "/api/v1/setup/google/resource-selection/callback",
            callback_body(&session_id, "attacker-guess", &["folder-999"]),
        )
        .await;
        assert_eq!(json["state"], "not_ready");
        assert_eq!(recorded.lock().unwrap().len(), 1);

        gate.notify_one();
        let (_, json) = claimant.await.unwrap();
        assert_eq!(json["state"], "ready");
    }

    /// A mismatched/unknown state on the GET path cannot kill the
    /// legitimate pending attempt: the real browser round trip still
    /// completes afterwards.
    #[tokio::test]
    async fn get_state_mismatch_does_not_kill_pending_attempt() {
        let (app, _state, recorded) =
            selection_app_with_recorder(Arc::new(FixedProbe(Ok(folder_metadata())))).await;
        let (_session_id, state_token) = start_selection(&app).await;

        let (status, _, _) = get_loopback(
            &app,
            "?state=attacker-guess&code=x&picked_file_ids=folder-123",
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(recorded.lock().unwrap().is_empty());

        let (status, _, html) = get_loopback(
            &app,
            &format!("?state={state_token}&code=picker-code-1&picked_file_ids=folder-123"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(html.contains("folder selected"));
    }
}
