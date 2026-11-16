//! Daemon-owned Google Drive ledger-folder setup and validation (#1085,
//! ARCH-0007 Decision 2).
//!
//! Implements the provider-owned resource-selection seam required before
//! canonical `product.create` with a Google descriptor. On an explicit
//! owner action the daemon provisions the folder itself with its stored
//! `drive` credential (see [`crate::google_drive_provisioning`]): it finds
//! or creates the Bicameral Shared Drive, creates one ledger folder, shares
//! that folder with the owner's Workspace domain, and validates it with a
//! fail-closed Drive `files.get`. There is no Picker and no browser-supplied
//! folder id: Google's native Picker only supports `drive.file`, which
//! cannot serve cross-operator Join.
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
/// `product.create` (#1264). Thirty minutes covers a real operator setting
/// up storage and then returning to the create dialog; a real run at ten
/// minutes expired twice before Create.
pub const SELECTION_SESSION_TTL_SECONDS: i64 = 1_800;

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
    /// Resource does not exist or is not accessible to the connected
    /// Google account under the granted `drive` scope.
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

/// Terminal or pending outcome of one setup attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionOutcome {
    /// Provisioning is in flight. A request that is dropped mid-flight
    /// leaves the session here until the TTL purges it: fail-closed, it
    /// can never validate a create.
    Pending,
    Ready(ProviderResourceDescriptor),
    Failed {
        state: ProviderResourceState,
        message: String,
    },
}

/// A daemon-memory resource-selection session.
///
/// Creation-attempt context only: never persisted, never registry state,
/// and never exposed beyond the redacted projection. It holds no
/// credential, authorization code, or OAuth state.
#[derive(Debug, Clone)]
pub struct ResourceSelectionSession {
    pub session_id: String,
    pub created_at: DateTime<Utc>,
    pub outcome: SelectionOutcome,
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
            message: "Setting up Google Drive storage.".into(),
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
            "The ledger item is not a folder.".into(),
        ));
    }
    if metadata.trashed {
        return Err((
            ProviderResourceState::NotReady,
            "The ledger folder is in the trash.".into(),
        ));
    }
    if !metadata.can_add_children {
        return Err((
            ProviderResourceState::ActionNeeded,
            "Bicameral cannot create files in the ledger folder.".into(),
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
            "The ledger folder does not exist or is not accessible.".into(),
        ),
        DriveProbeError::Provider { safe_message } | DriveProbeError::Network { safe_message } => {
            (ProviderResourceState::NotReady, safe_message.clone())
        }
    }
}

// ── Desktop credential binding for Google token refresh (#1264) ────

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
use bicameral_api::provider_resource::ResourceSelectionStartOutcome;

/// Routes for daemon-owned Google ledger-folder setup.
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
            "/api/v1/setup/google/resource-selection/:session_id",
            get(resource_selection_status),
        )
}

fn start_action_needed(message: &str) -> (StatusCode, Json<ResourceSelectionStartOutcome>) {
    (
        StatusCode::OK,
        Json(ResourceSelectionStartOutcome::ActionNeeded {
            provider: "google".into(),
            message: message.into(),
        }),
    )
}

/// `POST /setup/google/resource-selection/start`: provision and validate
/// the Product ledger folder.
///
/// Requires the stored Google connection credential to record the `drive`
/// grant; without it the call returns `action_needed` and reaches no
/// provider. Provisioning runs inside this request, so the session is
/// terminal by the time the response is sent; the SPA reads the redacted
/// result through the status route. Each call creates a new ledger folder
/// (see the limitation noted in [`crate::google_drive_provisioning`]).
async fn resource_selection_start(
    State(state): State<AppState>,
) -> (StatusCode, Json<ResourceSelectionStartOutcome>) {
    use crate::routes::GoogleDriveAccessTokenError;

    let access_token = match crate::routes::google_drive_access_token(&state).await {
        Ok(token) => token,
        Err(GoogleDriveAccessTokenError::NotConnected) => {
            return start_action_needed("Connect Google before setting up Drive storage.");
        }
        Err(GoogleDriveAccessTokenError::MissingDriveGrant) => {
            return start_action_needed(crate::routes::GOOGLE_DRIVE_SCOPE_NOT_GRANTED_MESSAGE);
        }
        Err(GoogleDriveAccessTokenError::Rejected) => {
            return start_action_needed(
                "Google rejected the connection credential. Reconnect Google and retry.",
            );
        }
        Err(GoogleDriveAccessTokenError::Unavailable) => {
            return start_action_needed(
                "Bicameral could not refresh the Google credential. Check the connection and \
                 retry.",
            );
        }
    };

    let session_id = format!("rsel-{}", uuid::Uuid::new_v4());
    let now = Utc::now();
    {
        let mut sessions = state.resource_selection_sessions.write().await;
        // Expired sessions are dropped opportunistically so abandoned
        // attempts cannot accumulate.
        sessions.retain(|_, session| !session.is_expired(now));
        sessions.insert(
            session_id.clone(),
            ResourceSelectionSession {
                session_id: session_id.clone(),
                created_at: now,
                outcome: SelectionOutcome::Pending,
            },
        );
    }

    let outcome = crate::google_drive_provisioning::provision_ledger_folder(
        state.drive_storage_provisioner.as_ref(),
        state.drive_resource_probe.as_ref(),
        state.secrets_store.as_ref(),
        &access_token,
        now,
    )
    .await;

    {
        let mut sessions = state.resource_selection_sessions.write().await;
        if let Some(entry) = sessions.get_mut(&session_id) {
            entry.outcome = outcome;
        }
    }

    (
        StatusCode::OK,
        Json(ResourceSelectionStartOutcome::Started {
            provider: "google".into(),
            selection_session_id: session_id,
        }),
    )
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

    fn test_session(now: DateTime<Utc>) -> ResourceSelectionSession {
        ResourceSelectionSession {
            session_id: "rsel-1".into(),
            created_at: now,
            outcome: SelectionOutcome::Pending,
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
            message: "The ledger folder is in the trash.".into(),
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

    use crate::google_drive_provisioning::test_support::{Call, ScriptedProvisioner};
    use crate::google_drive_provisioning::DriveAccount;
    use axum::body::Body;
    use axum::http::Request;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tower::ServiceExt;

    const DRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive";

    /// Probe double that reports every folder inside `drive_id` and counts
    /// calls.
    struct CountingProbe {
        drive_id: String,
        calls: AtomicUsize,
    }

    impl CountingProbe {
        fn in_drive(drive_id: &str) -> Arc<Self> {
            Arc::new(Self {
                drive_id: drive_id.into(),
                calls: AtomicUsize::new(0),
            })
        }
    }

    impl DriveResourceProbe for CountingProbe {
        fn get_file_metadata<'a>(
            &'a self,
            _access_token: &'a str,
            file_id: &'a str,
        ) -> DriveProbeFuture<'a> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut metadata = folder_metadata();
            metadata.id = file_id.into();
            metadata.drive_id = Some(self.drive_id.clone());
            Box::pin(async move { Ok(metadata) })
        }
    }

    fn store_google_credential(
        state: &AppState,
        scope: Option<&str>,
        expires_in_seconds: Option<u64>,
        obtained_at: DateTime<Utc>,
    ) {
        let envelope = bicameral_config::OAuthTokenEnvelopeV1::from_token_exchange(
            "stored-access-token".into(),
            Some("stored-refresh-token".into()),
            expires_in_seconds,
            Some("Bearer".into()),
            scope.map(String::from),
            obtained_at,
        );
        state
            .secrets_store
            .write_secret(
                bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
                bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
                &envelope.to_secret_string().unwrap(),
            )
            .unwrap();
    }

    fn setup_app(
        provisioner: Arc<ScriptedProvisioner>,
        probe: Arc<CountingProbe>,
    ) -> (Router, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let audit = bicameral_audit::store::AuditStore::open(dir.path()).unwrap();
        std::mem::forget(dir);
        let state = AppState::new(audit)
            .with_drive_resource_probe(probe)
            .with_drive_storage_provisioner(provisioner);
        let app = Router::new()
            .merge(resource_selection_routes())
            .with_state(state.clone());
        (app, state)
    }

    async fn request_json(
        app: &Router,
        method: &str,
        uri: &str,
    ) -> (StatusCode, serde_json::Value) {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 16384).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    async fn start(app: &Router) -> serde_json::Value {
        let (status, json) =
            request_json(app, "POST", "/api/v1/setup/google/resource-selection/start").await;
        assert_eq!(status, StatusCode::OK);
        json
    }

    async fn status_of(app: &Router, session_id: &str) -> (StatusCode, serde_json::Value) {
        request_json(
            app,
            "GET",
            &format!("/api/v1/setup/google/resource-selection/{session_id}"),
        )
        .await
    }

    #[tokio::test]
    async fn start_provisions_shares_validates_and_projects_ready() {
        let provisioner = Arc::new(ScriptedProvisioner::workspace());
        let probe = CountingProbe::in_drive("created-drive-1");
        let (app, state) = setup_app(provisioner.clone(), probe.clone());
        store_google_credential(&state, Some(DRIVE_SCOPE), Some(3600), Utc::now());

        let started = start(&app).await;
        assert_eq!(started["outcome"], "started");
        assert!(
            started.get("url").is_none(),
            "no browser URL: nothing to pick"
        );
        let session_id = started["selection_session_id"].as_str().unwrap();

        let (status, projection) = status_of(&app, session_id).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(projection["state"], "ready");
        assert_eq!(projection["resource"]["folder_id"], "ledger-folder-1");
        assert_eq!(projection["resource"]["drive_id"], "created-drive-1");
        let wire = projection.to_string();
        assert!(!wire.contains("stored-access-token"));
        assert!(!wire.contains("stored-refresh-token"));

        // The one permission change: the owner's domain, on the folder.
        let shares: Vec<Call> = provisioner
            .calls()
            .into_iter()
            .filter(|call| matches!(call, Call::ShareDomain { .. }))
            .collect();
        assert_eq!(
            shares,
            vec![Call::ShareDomain {
                folder_id: "ledger-folder-1".into(),
                domain: "example.com".into(),
            }]
        );
        assert_eq!(probe.calls.load(Ordering::SeqCst), 1);

        // Downstream contract is unchanged: product.create is stamped for
        // exactly the folder the daemon provisioned and validated.
        assert_eq!(
            validated_event_store_folder(&state, &create_command(Some("ledger-folder-1"))).await,
            Some("ledger-folder-1".to_string())
        );
        assert_eq!(
            validated_event_store_folder(&state, &create_command(Some("pasted-folder"))).await,
            None
        );
    }

    #[tokio::test]
    async fn start_without_a_google_credential_reaches_no_provider() {
        let provisioner = Arc::new(ScriptedProvisioner::workspace());
        let probe = CountingProbe::in_drive("created-drive-1");
        let (app, state) = setup_app(provisioner.clone(), probe.clone());

        let started = start(&app).await;
        assert_eq!(started["outcome"], "action_needed");
        assert!(provisioner.calls().is_empty());
        assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
        assert!(state.resource_selection_sessions.read().await.is_empty());
    }

    #[tokio::test]
    async fn start_without_the_drive_grant_requires_reconnect_and_reaches_no_provider() {
        for scope in [
            Some("https://www.googleapis.com/auth/drive.file"),
            Some("https://www.googleapis.com/auth/drive.readonly"),
            None,
        ] {
            let provisioner = Arc::new(ScriptedProvisioner::workspace());
            let probe = CountingProbe::in_drive("created-drive-1");
            let (app, state) = setup_app(provisioner.clone(), probe.clone());
            store_google_credential(&state, scope, Some(3600), Utc::now());

            let started = start(&app).await;
            assert_eq!(started["outcome"], "action_needed", "scope {scope:?}");
            assert_eq!(
                started["message"],
                crate::routes::GOOGLE_DRIVE_SCOPE_NOT_GRANTED_MESSAGE
            );
            assert!(provisioner.calls().is_empty());
            assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
            assert!(state.resource_selection_sessions.read().await.is_empty());
        }
    }

    #[tokio::test]
    async fn account_that_cannot_create_shared_drives_is_refused_without_fallback() {
        let mut scripted = ScriptedProvisioner::workspace();
        scripted.account = Ok(DriveAccount {
            email: "owner@personal.example".into(),
            can_create_drives: false,
        });
        let provisioner = Arc::new(scripted);
        let probe = CountingProbe::in_drive("created-drive-1");
        let (app, state) = setup_app(provisioner.clone(), probe.clone());
        store_google_credential(&state, Some(DRIVE_SCOPE), Some(3600), Utc::now());

        let started = start(&app).await;
        assert_eq!(started["outcome"], "started");
        let session_id = started["selection_session_id"].as_str().unwrap();
        let (_, projection) = status_of(&app, session_id).await;
        assert_eq!(projection["state"], "action_needed");
        assert!(projection["message"]
            .as_str()
            .unwrap()
            .contains("Google Workspace account that can create shared drives"));
        assert!(projection.get("resource").is_none());
        // Nothing was created or shared, and nothing can be stamped.
        assert_eq!(provisioner.calls(), vec![Call::Account]);
        assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            validated_event_store_folder(&state, &create_command(Some("ledger-folder-1"))).await,
            None
        );
    }

    /// Local Google token-endpoint stub recording form bodies.
    async fn spawn_token_stub(
        status: StatusCode,
        body: serde_json::Value,
    ) -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
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

    fn legacy_env_config() -> bicameral_config::GoogleOAuthConfig {
        bicameral_config::GoogleOAuthConfig {
            client_id: "test-client-id".into(),
            client_secret: "test-client-secret".into(),
            redirect_uri: "http://localhost:3000/api/v1/connections/google/callback".into(),
            scopes: vec![DRIVE_SCOPE.into()],
        }
    }

    async fn app_with_expired_credential(
        token_status: StatusCode,
        token_body: serde_json::Value,
    ) -> (
        Router,
        Arc<ScriptedProvisioner>,
        Arc<std::sync::Mutex<Vec<String>>>,
    ) {
        let provisioner = Arc::new(ScriptedProvisioner::workspace());
        let probe = CountingProbe::in_drive("created-drive-1");
        let dir = tempfile::tempdir().unwrap();
        let audit = bicameral_audit::store::AuditStore::open(dir.path()).unwrap();
        std::mem::forget(dir);
        let (endpoint, recorded) = spawn_token_stub(token_status, token_body).await;
        let mut state = AppState::new(audit)
            .with_drive_resource_probe(probe)
            .with_drive_storage_provisioner(provisioner.clone());
        state.google_oauth_config = Some(legacy_env_config());
        state.google_oauth_allow_env_override = true;
        state.google_token_endpoint_override = Some(endpoint);
        store_google_credential(
            &state,
            Some(DRIVE_SCOPE),
            Some(3600),
            Utc::now() - chrono::Duration::hours(2),
        );
        let app = Router::new()
            .merge(resource_selection_routes())
            .with_state(state);
        (app, provisioner, recorded)
    }

    #[tokio::test]
    async fn expired_access_token_is_refreshed_before_provisioning() {
        let (app, provisioner, recorded) = app_with_expired_credential(
            StatusCode::OK,
            serde_json::json!({
                "access_token": "refreshed-access-token",
                "expires_in": 3599,
                "token_type": "Bearer"
            }),
        )
        .await;

        let started = start(&app).await;
        assert_eq!(started["outcome"], "started");
        let session_id = started["selection_session_id"].as_str().unwrap();
        let (_, projection) = status_of(&app, session_id).await;
        assert_eq!(projection["state"], "ready");

        let forms = recorded.lock().unwrap().clone();
        assert_eq!(forms.len(), 1);
        assert!(forms[0].contains("grant_type=refresh_token"));
        assert!(forms[0].contains("refresh_token=stored-refresh-token"));
        assert!(!provisioner.calls().is_empty());
    }

    #[tokio::test]
    async fn rejected_or_narrowed_refresh_requires_reconnect_and_reaches_no_provider() {
        for (status, body) in [
            (
                StatusCode::BAD_REQUEST,
                serde_json::json!({"error": "invalid_grant"}),
            ),
            (
                StatusCode::OK,
                serde_json::json!({
                    "access_token": "narrow-access-token",
                    "expires_in": 3599,
                    "scope": "https://www.googleapis.com/auth/drive.file"
                }),
            ),
        ] {
            let (app, provisioner, _recorded) = app_with_expired_credential(status, body).await;
            let started = start(&app).await;
            assert_eq!(started["outcome"], "action_needed");
            assert!(provisioner.calls().is_empty());
        }
    }

    #[tokio::test]
    async fn unknown_or_expired_session_is_not_found() {
        let provisioner = Arc::new(ScriptedProvisioner::workspace());
        let probe = CountingProbe::in_drive("created-drive-1");
        let (app, state) = setup_app(provisioner, probe);

        let (status, projection) = status_of(&app, "rsel-missing").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(projection["state"], "not_ready");

        let mut expired =
            test_session(Utc::now() - chrono::Duration::seconds(SELECTION_SESSION_TTL_SECONDS + 1));
        expired.outcome = SelectionOutcome::Ready(ready_descriptor("folder-abc"));
        state
            .resource_selection_sessions
            .write()
            .await
            .insert(expired.session_id.clone(), expired);
        let (status, projection) = status_of(&app, "rsel-1").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(projection.get("resource").is_none());
    }

    #[tokio::test]
    async fn picker_callback_surface_is_gone() {
        let provisioner = Arc::new(ScriptedProvisioner::workspace());
        let probe = CountingProbe::in_drive("created-drive-1");
        let (app, state) = setup_app(provisioner.clone(), probe);
        store_google_credential(&state, Some(DRIVE_SCOPE), Some(3600), Utc::now());

        // A browser redirect carrying a folder id can no longer complete
        // anything: the path only resolves as an unknown session id.
        let (status, _) = request_json(
            &app,
            "GET",
            "/api/v1/setup/google/resource-selection/callback?state=s&code=c&picked_file_ids=folder-x",
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = request_json(
            &app,
            "POST",
            "/api/v1/setup/google/resource-selection/callback",
        )
        .await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
        assert!(provisioner.calls().is_empty());
        assert_eq!(
            validated_event_store_folder(&state, &create_command(Some("folder-x"))).await,
            None
        );
    }
}
