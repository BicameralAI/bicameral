//! Production Google Drive REST transport for the Drive event-store
//! adapter core (#1086 residual slice).
//!
//! Composes the deterministic [`crate::drive_api_event_store`] core with:
//!
//! - the daemon credential boundary from #1084: access tokens come from the
//!   OS-keyring canonical envelope (`connection.google/oauth_token`), with
//!   expiry-driven refresh that re-persists the rotated envelope;
//! - the validated folder authority from #1085: the adapter is constructed
//!   from an explicitly validated Drive folder id and every request carries
//!   `supportsAllDrives=true` so My Drive and shared-drive folders behave
//!   identically;
//! - provider-side canonical identity binding: every event object is
//!   created with the `appProperties` marker
//!   [`CANONICAL_EVENT_MARKER_KEY`] = `<event filename>`, and both retry
//!   discovery and replay listing filter on that marker, so an unrelated
//!   same-name Drive object can never masquerade as (or inject itself into)
//!   the canonical Bicameral event stream. Exact byte readback remains the
//!   confirmation contract on top of the marker.
//!
//! The core trait is synchronous; HTTP work runs on short-lived scoped
//! worker threads so callers inside a tokio runtime never block or nest
//! runtimes. Access tokens flow through requests transiently and never
//! appear in errors, logs, or returned metadata.

use crate::drive_api_event_store::{
    DriveApiAdapter, DriveApiTransport, DriveRemoteObject, DriveTransportError,
    DriveTransportErrorKind,
};
use bicameral_api::event_store::EventStoreError;
use bicameral_config::{GoogleOAuthConfig, SecretsStore, StoredOAuthToken};
use std::sync::{Arc, OnceLock};

/// `appProperties` key marking an object as a canonical Bicameral event.
/// The value is the exact canonical event filename (`<uuid>.json`).
pub const CANONICAL_EVENT_MARKER_KEY: &str = "bicameralCanonicalEvent";

/// Default Google API host (Drive REST + media download).
const DEFAULT_API_BASE: &str = "https://www.googleapis.com";

/// Default Google OAuth token endpoint used for refresh grants.
const DEFAULT_TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";

/// Deterministic near-expiry refresh skew (#1134). A stored access token
/// that expires within this window is refreshed proactively, so a request
/// never departs with a token that would die in flight.
pub const REFRESH_SKEW_SECONDS: i64 = 60;

/// Run blocking HTTP work on a short-lived scoped thread.
///
/// The transport trait is synchronous while the daemon runs inside tokio;
/// `reqwest::blocking` refuses to run on an async runtime thread, so every
/// provider call executes on its own plain OS thread and joins before
/// returning. Event appends are low-frequency, so per-call threads are an
/// acceptable cost for correctness.
fn run_blocking<T: Send>(work: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        scope
            .spawn(work)
            .join()
            .expect("drive transport worker thread panicked")
    })
}

// ── Access-token provider (#1084 composition) ─────────────────────

/// Source of a currently-usable Google access token.
///
/// Implementations own refresh/rotation; the transport never sees refresh
/// tokens and never stores what it is given.
pub trait DriveAccessTokenProvider: Send + Sync {
    fn access_token(&self) -> Result<String, DriveTransportError>;
}

/// Keyring-backed token provider composing the #1084 credential seam.
///
/// Reads the canonical `connection.google/oauth_token` envelope (legacy
/// `google_drive` alias accepted during the compatibility window). When the
/// access token is expired and a refresh token plus OAuth config are
/// available, performs a `refresh_token` grant and re-persists the rotated
/// envelope to the canonical key before returning the fresh access token.
/// Fails closed with `Authentication` when no usable credential exists.
pub struct KeyringGoogleAccessTokenProvider {
    secrets_store: Arc<dyn SecretsStore>,
    oauth_config: Option<GoogleOAuthConfig>,
    token_endpoint: String,
    client: OnceLock<reqwest::blocking::Client>,
    /// Serializes refresh so concurrent callers produce exactly one
    /// effective refresh generation (#1101/#1134): the winner refreshes
    /// and persists; waiters re-read the persisted envelope instead of
    /// issuing a second grant.
    refresh_lock: std::sync::Mutex<()>,
}

impl KeyringGoogleAccessTokenProvider {
    pub fn new(
        secrets_store: Arc<dyn SecretsStore>,
        oauth_config: Option<GoogleOAuthConfig>,
    ) -> Self {
        Self {
            secrets_store,
            oauth_config,
            token_endpoint: DEFAULT_TOKEN_ENDPOINT.to_string(),
            client: OnceLock::new(),
            refresh_lock: std::sync::Mutex::new(()),
        }
    }

    /// Test seam: point the refresh grant at a deterministic endpoint.
    pub fn with_token_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.token_endpoint = endpoint.into();
        self
    }

    fn read_stored_token(&self) -> Option<StoredOAuthToken> {
        let read = |source_id: &str| {
            self.secrets_store
                .read_secret(
                    source_id,
                    bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
                )
                .and_then(|value| bicameral_config::parse_stored_oauth_token(&value))
                .filter(|token| token.is_usable())
        };
        read(bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID).or_else(|| {
            read(bicameral_config::connections::legacy_aliases::GOOGLE_DRIVE_SOURCE_AUTH)
        })
    }

    /// Perform a `refresh_token` grant and persist the rotated envelope.
    fn refresh(
        &self,
        refresh_token: &str,
        config: &GoogleOAuthConfig,
    ) -> Result<String, DriveTransportError> {
        let endpoint = self.token_endpoint.clone();
        // Native/public clients never send an empty pseudo-secret (#1134);
        // only an explicitly configured confidential client includes one.
        let mut params = vec![
            ("client_id", config.client_id.clone()),
            ("refresh_token", refresh_token.to_string()),
            ("grant_type", "refresh_token".to_string()),
        ];
        if !config.client_secret.is_empty() {
            params.push(("client_secret", config.client_secret.clone()));
        }

        let json: serde_json::Value = run_blocking(|| {
            let client = self.client.get_or_init(reqwest::blocking::Client::new);
            let response = client
                .post(&endpoint)
                .form(&params)
                .send()
                .map_err(|_| unavailable("Could not reach the Google token endpoint."))?;
            match response.status().as_u16() {
                200 => response
                    .json()
                    .map_err(|_| invalid("Google token endpoint returned an unreadable response.")),
                400 | 401 | 403 => Err(auth_error(
                    "Google rejected the stored refresh credential. Reconnect Google.",
                )),
                status => Err(unavailable(format!(
                    "Google token endpoint returned status {status}."
                ))),
            }
        })?;

        let access_token = json["access_token"]
            .as_str()
            .filter(|token| !token.is_empty())
            .ok_or_else(|| invalid("Google refresh response did not contain an access token."))?
            .to_string();

        // Google typically omits refresh_token on refresh grants; keep the
        // existing one so future rotations stay possible.
        let rotated_refresh = json["refresh_token"]
            .as_str()
            .filter(|token| !token.is_empty())
            .map(String::from)
            .or_else(|| Some(refresh_token.to_string()));

        let envelope = bicameral_config::OAuthTokenEnvelopeV1::from_token_exchange(
            access_token.clone(),
            rotated_refresh,
            json["expires_in"].as_u64(),
            json["token_type"].as_str().map(String::from),
            json["scope"].as_str().map(String::from),
            chrono::Utc::now(),
        )
        // Preserve the minting client identity through rotation so the
        // NEXT refresh after a restart still has it.
        .with_client_id(Some(config.client_id.clone()));
        let secret_value = envelope
            .to_secret_string()
            .map_err(|_| invalid("Rotated token envelope could not be prepared."))?;
        self.secrets_store
            .write_secret(
                bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
                bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
                &secret_value,
            )
            .map_err(|_| {
                auth_error("Rotated Google credential could not be stored in the OS keyring.")
            })?;

        Ok(access_token)
    }
}

impl KeyringGoogleAccessTokenProvider {
    /// Whether the stored envelope needs a refresh: already expired or
    /// expiring within the deterministic [`REFRESH_SKEW_SECONDS`] window.
    fn needs_refresh(envelope: &bicameral_config::OAuthTokenEnvelopeV1) -> bool {
        envelope.access_token.is_empty()
            || envelope.access_token_expired(
                chrono::Utc::now() + chrono::Duration::seconds(REFRESH_SKEW_SECONDS),
            )
    }

    /// Resolve the refresh client identity: an explicitly injected config
    /// wins (legacy confidential override); otherwise the non-secret
    /// client identity recorded in the envelope at mint time is used as a
    /// native/public client (no client_secret exists, so none is ever
    /// sent). This is what makes a durable Google route reopenable after
    /// daemon restart with no injected client environment (#1087 step 5).
    fn effective_refresh_config(
        &self,
        envelope: &bicameral_config::OAuthTokenEnvelopeV1,
    ) -> Option<GoogleOAuthConfig> {
        if let Some(config) = &self.oauth_config {
            if !config.client_id.trim().is_empty() {
                return Some(config.clone());
            }
        }
        envelope
            .client_id
            .as_ref()
            .filter(|client_id| !client_id.trim().is_empty())
            .map(|client_id| GoogleOAuthConfig {
                client_id: client_id.clone(),
                client_secret: String::new(),
                redirect_uri: String::new(),
                scopes: Vec::new(),
            })
    }
}

impl DriveAccessTokenProvider for KeyringGoogleAccessTokenProvider {
    fn access_token(&self) -> Result<String, DriveTransportError> {
        let Some(stored) = self.read_stored_token() else {
            return Err(auth_error(
                "No usable Google credential is stored. Connect Google first.",
            ));
        };

        match &stored {
            StoredOAuthToken::Envelope(envelope) => {
                if !Self::needs_refresh(envelope) {
                    return Ok(envelope.access_token.clone());
                }
                // Single effective refresh generation (#1101/#1134): only
                // one caller refreshes; the rest re-read the envelope the
                // winner persisted before it became usable.
                let _generation = self
                    .refresh_lock
                    .lock()
                    .map_err(|_| auth_error("Google credential refresh state is poisoned."))?;
                if let Some(StoredOAuthToken::Envelope(current)) = self.read_stored_token() {
                    if !Self::needs_refresh(&current) {
                        return Ok(current.access_token.clone());
                    }
                }
                match (
                    stored.refresh_token(),
                    self.effective_refresh_config(envelope),
                ) {
                    (Some(refresh_token), Some(config)) => self.refresh(refresh_token, &config),
                    (Some(_), None) => Err(auth_error(
                        "Google credential is expired and no refresh client identity is \
                         available. Reconnect Google.",
                    )),
                    (None, _) => Err(auth_error(
                        "Google credential is expired and no refresh token was stored. \
                         Reconnect Google.",
                    )),
                }
            }
            // Legacy plain tokens carry no expiry metadata; pass them
            // through and let provider 401s surface as Authentication.
            StoredOAuthToken::LegacyPlain { .. } => Ok(stored
                .access_token()
                .expect("usable legacy token has an access token")
                .to_string()),
        }
    }
}

// ── REST transport ────────────────────────────────────────────────

/// Live Google Drive REST implementation of [`DriveApiTransport`].
pub struct HttpDriveApiTransport {
    token_provider: Arc<dyn DriveAccessTokenProvider>,
    api_base: String,
    client: OnceLock<reqwest::blocking::Client>,
}

impl HttpDriveApiTransport {
    pub fn new(token_provider: Arc<dyn DriveAccessTokenProvider>) -> Self {
        Self {
            token_provider,
            api_base: DEFAULT_API_BASE.to_string(),
            client: OnceLock::new(),
        }
    }

    /// Test seam: point every request at a deterministic host.
    pub fn with_api_base(mut self, api_base: impl Into<String>) -> Self {
        self.api_base = api_base.into();
        self
    }

    /// Escape a value for embedding in a Drive `q` single-quoted string.
    fn escape_query_value(value: &str) -> String {
        value.replace('\\', "\\\\").replace('\'', "\\'")
    }

    fn get_json(
        &self,
        url: String,
        access_token: String,
    ) -> Result<serde_json::Value, DriveTransportError> {
        run_blocking(move || {
            let client = self.client.get_or_init(reqwest::blocking::Client::new);
            let response = client
                .get(&url)
                .bearer_auth(&access_token)
                .send()
                .map_err(|_| unavailable("Could not reach Google Drive."))?;
            let status = response.status().as_u16();
            if !(200..=299).contains(&status) {
                let body = response.text().unwrap_or_default();
                map_status_with_body(status, &body)?;
                return Err(invalid("Google Drive returned an unexpected status."));
            }
            response
                .json()
                .map_err(|_| invalid("Google Drive returned an unreadable response."))
        })
    }

    fn objects_from_files(value: &serde_json::Value) -> Vec<DriveRemoteObject> {
        value["files"]
            .as_array()
            .map(|files| {
                files
                    .iter()
                    // Drive v3 has no key-only `appProperties has { key=... }`
                    // filter (it 400s "Invalid Value"), so canonical events are
                    // discriminated client-side by the marker appProperty the
                    // daemon itself wrote on create.
                    .filter(|file| {
                        file["appProperties"]
                            .get(CANONICAL_EVENT_MARKER_KEY)
                            .and_then(serde_json::Value::as_str)
                            .is_some_and(|value| !value.is_empty())
                    })
                    .map(|file| DriveRemoteObject {
                        file_id: file["id"].as_str().unwrap_or_default().to_string(),
                        name: file["name"].as_str().unwrap_or_default().to_string(),
                        version: file["headRevisionId"]
                            .as_str()
                            .or_else(|| file["version"].as_str())
                            .map(String::from),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn list_with_query(&self, query: &str) -> Result<Vec<DriveRemoteObject>, DriveTransportError> {
        let access_token = self.token_provider.access_token()?;
        let mut objects = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut url = format!(
                "{}/drive/v3/files?q={}&supportsAllDrives=true&includeItemsFromAllDrives=true\
                 &fields=nextPageToken,files(id,name,headRevisionId,appProperties)&pageSize=1000",
                self.api_base,
                urlencode(query),
            );
            if let Some(token) = &page_token {
                url.push_str(&format!("&pageToken={}", urlencode(token)));
            }
            let json = self.get_json(url, access_token.clone())?;
            objects.extend(Self::objects_from_files(&json));
            match json["nextPageToken"].as_str() {
                Some(token) if !token.is_empty() => page_token = Some(token.to_string()),
                _ => break,
            }
        }
        Ok(objects)
    }
}

impl DriveApiTransport for HttpDriveApiTransport {
    fn find_children_by_name(
        &self,
        folder_id: &str,
        name: &str,
    ) -> Result<Vec<DriveRemoteObject>, DriveTransportError> {
        // The marker filter is the provider-side canonical identity
        // binding: only objects this daemon stamped as canonical events
        // are discovery candidates, so a foreign same-name object cannot
        // be adopted on retry. Duplicates among stamped objects still
        // surface (and conflict in the core).
        let query = format!(
            "'{}' in parents and name = '{}' and trashed = false and \
             appProperties has {{ key='{}' and value='{}' }}",
            Self::escape_query_value(folder_id),
            Self::escape_query_value(name),
            CANONICAL_EVENT_MARKER_KEY,
            Self::escape_query_value(name),
        );
        self.list_with_query(&query)
    }

    fn create_json_file(
        &self,
        folder_id: &str,
        name: &str,
        body: &[u8],
    ) -> Result<DriveRemoteObject, DriveTransportError> {
        let access_token = self.token_provider.access_token()?;
        let metadata = serde_json::json!({
            "name": name,
            "parents": [folder_id],
            "mimeType": "application/json",
            "appProperties": { CANONICAL_EVENT_MARKER_KEY: name },
        });

        // Drive multipart upload uses multipart/related, which reqwest's
        // form-data helper does not produce; build the body manually.
        let boundary = format!("bicameral-{}", uuid::Uuid::new_v4());
        let mut payload = Vec::new();
        payload.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        payload.extend_from_slice(b"Content-Type: application/json; charset=UTF-8\r\n\r\n");
        payload.extend_from_slice(metadata.to_string().as_bytes());
        payload.extend_from_slice(format!("\r\n--{boundary}\r\n").as_bytes());
        payload.extend_from_slice(b"Content-Type: application/json\r\n\r\n");
        payload.extend_from_slice(body);
        payload.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

        let url = format!(
            "{}/upload/drive/v3/files?uploadType=multipart&supportsAllDrives=true\
             &fields=id,name,headRevisionId",
            self.api_base
        );

        let json: serde_json::Value = run_blocking(move || {
            let client = self.client.get_or_init(reqwest::blocking::Client::new);
            let response = client
                .post(&url)
                .bearer_auth(&access_token)
                .header(
                    "Content-Type",
                    format!("multipart/related; boundary={boundary}"),
                )
                .body(payload)
                .send()
                .map_err(|_| unavailable("Could not reach Google Drive."))?;
            let status = response.status().as_u16();
            if !(200..=299).contains(&status) {
                let body = response.text().unwrap_or_default();
                map_status_with_body(status, &body)?;
                return Err(invalid("Google Drive returned an unexpected status."));
            }
            response
                .json()
                .map_err(|_| invalid("Google Drive returned an unreadable upload response."))
        })?;

        let file_id = json["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| invalid("Google Drive upload response did not contain a file id."))?;
        Ok(DriveRemoteObject {
            file_id: file_id.to_string(),
            name: json["name"].as_str().unwrap_or(name).to_string(),
            version: json["headRevisionId"].as_str().map(String::from),
        })
    }

    fn download_file(&self, file_id: &str) -> Result<Vec<u8>, DriveTransportError> {
        let access_token = self.token_provider.access_token()?;
        let url = format!(
            "{}/drive/v3/files/{}?alt=media&supportsAllDrives=true",
            self.api_base, file_id
        );
        run_blocking(move || {
            let client = self.client.get_or_init(reqwest::blocking::Client::new);
            let response = client
                .get(&url)
                .bearer_auth(&access_token)
                .send()
                .map_err(|_| unavailable("Could not reach Google Drive."))?;
            let status = response.status().as_u16();
            map_status(status)?;
            response
                .bytes()
                .map(|bytes| bytes.to_vec())
                .map_err(|_| invalid("Google Drive returned an unreadable download body."))
        })
    }

    fn list_children(
        &self,
        folder_id: &str,
    ) -> Result<Vec<DriveRemoteObject>, DriveTransportError> {
        // Folder-bounded, marker-bounded replay: only canonical Bicameral
        // event objects inside the validated folder participate, so an
        // unrelated JSON file cannot inject events into replay. Drive v3 has no
        // key-only appProperties filter, so the folder+trashed query runs
        // server-side and `objects_from_files` keeps only marker-bearing files.
        let query = format!(
            "'{}' in parents and trashed = false",
            Self::escape_query_value(folder_id),
        );
        self.list_with_query(&query)
    }

    fn probe_folder(&self, folder_id: &str) -> Result<(), DriveTransportError> {
        let access_token = self.token_provider.access_token()?;
        let url = format!(
            "{}/drive/v3/files/{}?supportsAllDrives=true\
             &fields=id,mimeType,trashed,capabilities/canAddChildren",
            self.api_base, folder_id
        );
        let json = self.get_json(url, access_token)?;
        if json["mimeType"].as_str() != Some("application/vnd.google-apps.folder") {
            return Err(invalid(
                "The configured event-store resource is not a folder.",
            ));
        }
        if json["trashed"].as_bool() == Some(true) {
            return Err(DriveTransportError::new(
                DriveTransportErrorKind::NotFound,
                "The configured event-store folder is in the trash.",
            ));
        }
        if json["capabilities"]["canAddChildren"].as_bool() != Some(true) {
            return Err(auth_error(
                "The current credential cannot create files in the event-store folder.",
            ));
        }
        Ok(())
    }
}

/// Compose the production Drive event store from validated authority.
///
/// `folder_id` must come from the daemon/provider validated resource
/// selection (#1085); `secrets_store` is the production credential
/// boundary (#1084). Production hosted paths must never substitute a
/// memory, synced-folder, fixture, or browser-authored source here.
pub fn production_drive_event_store(
    folder_id: impl Into<String>,
    secrets_store: Arc<dyn SecretsStore>,
    oauth_config: Option<GoogleOAuthConfig>,
) -> Result<DriveApiAdapter, EventStoreError> {
    let provider = Arc::new(KeyringGoogleAccessTokenProvider::new(
        secrets_store,
        oauth_config,
    ));
    let transport = Arc::new(HttpDriveApiTransport::new(provider));
    DriveApiAdapter::new(folder_id, transport)
}

/// Atomic candidate batches are not yet governed on the Google Drive API
/// substrate; the capability fails closed with a typed error instead of
/// pretending transactional multi-object semantics the provider cannot
/// give (#1087 keeps ownership of any future atomic contract).
impl crate::atomic_event_store::AtomicEventStoreAdapter for DriveApiAdapter {
    fn append_atomic_batch(
        &self,
        _batch: crate::atomic_event_store::AtomicEventBatchV1,
    ) -> Result<(), EventStoreError> {
        Err(EventStoreError::Io {
            reason: "atomic_batch_unsupported: atomic candidate batches are not yet governed \
                     on the Google Drive API substrate"
                .to_string(),
        })
    }
}

/// Fail-closed stand-in for a durable route whose record cannot be opened
/// (e.g. a corrupt folder id). Every operation returns `Unreachable`; no
/// silent fallback substrate is ever substituted (#1087 resolver step 6).
pub struct UnopenableEventStore {
    reason: String,
}

impl bicameral_api::event_store::EventStoreAdapter for UnopenableEventStore {
    fn append(
        &self,
        _entry: bicameral_api::event_store::EventStoreEntry,
    ) -> Result<(), EventStoreError> {
        Err(EventStoreError::Unreachable {
            reason: self.reason.clone(),
        })
    }

    fn replay(&self) -> Result<Vec<bicameral_api::event_store::EventStoreEntry>, EventStoreError> {
        Err(EventStoreError::Unreachable {
            reason: self.reason.clone(),
        })
    }

    fn substrate_kind(&self) -> bicameral_api::event_store::SubstrateKind {
        bicameral_api::event_store::SubstrateKind::DriveFolder
    }

    fn health_check(&self) -> Result<bicameral_api::event_store::SubstrateHealth, EventStoreError> {
        Err(EventStoreError::Unreachable {
            reason: self.reason.clone(),
        })
    }

    fn capabilities(
        &self,
    ) -> std::collections::BTreeSet<bicameral_api::event_store::SubstrateCapability> {
        std::collections::BTreeSet::new()
    }
}

impl crate::atomic_event_store::AtomicEventStoreAdapter for UnopenableEventStore {
    fn append_atomic_batch(
        &self,
        _batch: crate::atomic_event_store::AtomicEventBatchV1,
    ) -> Result<(), EventStoreError> {
        Err(EventStoreError::Unreachable {
            reason: self.reason.clone(),
        })
    }
}

/// Open the durable Google Drive API route (#1087 resolver step 5 — the
/// smallest governed composition): the EXACT production adapter over the
/// OS-keyring credential envelope, reachable through every ordinary
/// daemon route consumer (startup registration hydration/health,
/// `product.continue_setup` replay, publish/append routing). Refresh
/// identity comes from an explicitly configured legacy env client when
/// present, otherwise from the non-secret client identity recorded in
/// the stored envelope, so the route reopens after restart with no
/// injected client environment. Endpoints are the canonical production
/// constants only: durable route records carry no endpoint overrides, so
/// registry-file tampering cannot redirect token or Drive traffic.
pub fn open_google_drive_api_route(
    folder_id: &str,
) -> crate::event_store_adapters::DualEventStoreAdapter {
    match production_drive_event_store(
        folder_id,
        Arc::new(bicameral_config::OsKeyringSecretsStore::new()),
        GoogleOAuthConfig::from_env(),
    ) {
        Ok(adapter) => {
            let concrete = Arc::new(adapter);
            crate::event_store_adapters::DualEventStoreAdapter {
                event_store: concrete.clone(),
                atomic: concrete,
            }
        }
        Err(error) => {
            let unopenable = Arc::new(UnopenableEventStore {
                reason: format!("google_drive_api route is unopenable: {error}"),
            });
            crate::event_store_adapters::DualEventStoreAdapter {
                event_store: unopenable.clone(),
                atomic: unopenable,
            }
        }
    }
}

fn auth_error(message: impl Into<String>) -> DriveTransportError {
    DriveTransportError::new(DriveTransportErrorKind::Authentication, message)
}

fn unavailable(message: impl Into<String>) -> DriveTransportError {
    DriveTransportError::new(DriveTransportErrorKind::Unavailable, message)
}

fn invalid(message: impl Into<String>) -> DriveTransportError {
    DriveTransportError::new(DriveTransportErrorKind::InvalidResponse, message)
}

/// Map a non-success Drive response to a typed error, surfacing Google's own
/// `error.message` (never request material) so operators see why a call was
/// rejected instead of a bare status code (#1264 real-execution finding).
fn map_status_with_body(status: u16, body: &str) -> Result<(), DriveTransportError> {
    if (200..=299).contains(&status) {
        return Ok(());
    }
    let detail = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value["error"]["message"].as_str().map(str::to_string))
        .filter(|message| !message.trim().is_empty());
    let base = match map_status(status) {
        Err(error) => error,
        Ok(()) => unreachable!("non-success status must map to an error"),
    };
    match detail {
        Some(message) => Err(DriveTransportError::new(
            base.kind,
            format!(
                "{} Google said: {}",
                base.safe_message.trim_end(),
                message.trim()
            ),
        )),
        None => Err(base),
    }
}

fn map_status(status: u16) -> Result<(), DriveTransportError> {
    match status {
        200..=299 => Ok(()),
        401 | 403 => Err(auth_error(
            "Google rejected the connection credential for this Drive request.",
        )),
        404 => Err(DriveTransportError::new(
            DriveTransportErrorKind::NotFound,
            "The requested Drive resource does not exist or is not accessible.",
        )),
        429 => Err(unavailable("Google Drive is rate limiting requests.")),
        500..=599 => Err(unavailable("Google Drive is currently unavailable.")),
        status => Err(invalid(format!("Google Drive returned status {status}."))),
    }
}

fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod http_transport_tests {
    use super::*;

    #[test]
    fn objects_from_files_keeps_only_marker_bearing_files() {
        // Drive cannot filter by a key-only appProperty server-side (#1264),
        // so a folder that also holds unrelated JSON must not leak into replay.
        let value = serde_json::json!({
            "files": [
                {
                    "id": "evt-1",
                    "name": "00000000-0000-0000-0000-000000000001.json",
                    "headRevisionId": "v1",
                    "appProperties": { CANONICAL_EVENT_MARKER_KEY: "00000000-0000-0000-0000-000000000001.json" }
                },
                { "id": "other", "name": "notes.json" },
                { "id": "empty-marker", "name": "x.json", "appProperties": { CANONICAL_EVENT_MARKER_KEY: "" } }
            ]
        });
        let objects = HttpDriveApiTransport::objects_from_files(&value);
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].file_id, "evt-1");
    }

    #[test]
    fn map_status_with_body_surfaces_google_error_message() {
        let body = r#"{"error":{"code":400,"message":"Invalid Value"}}"#;
        let err = map_status_with_body(400, body).unwrap_err();
        assert!(
            err.safe_message.contains("Invalid Value"),
            "{}",
            err.safe_message
        );
        // A body without a message still yields the typed status error.
        let bare = map_status_with_body(400, "not json").unwrap_err();
        assert!(
            bare.safe_message.contains("status 400"),
            "{}",
            bare.safe_message
        );
    }
}
