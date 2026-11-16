//! Daemon-side provisioning of Google Drive Product storage (ARCH-0007
//! Decision 2, #50).
//!
//! On an explicit owner action the daemon finds or creates the Bicameral
//! Shared Drive, creates one ledger folder in it, and shares that folder
//! with the owner's Google Workspace domain. That domain grant is the only
//! permission change this module makes: it never adds individual accounts,
//! never shares the Shared Drive itself, never shares outside the owner's
//! domain or by public link, and never touches any other file.
//!
//! Authority boundary: provisioning yields a folder candidate only. The
//! caller still validates the folder through the Drive metadata probe
//! before it can become `Ready`, and Drive access is never Product
//! membership.
//!
//! Known limitation: every provisioning attempt creates a new ledger
//! folder. An attempt that is abandoned before `product.create` leaves an
//! empty folder behind in the Shared Drive; nothing cleans those up.

use crate::google_resource_selection::{
    probe_error_state, validate_folder_metadata, DriveResourceProbe, SelectionOutcome,
};
use bicameral_api::provider_resource::{ProviderResourceDescriptor, ProviderResourceState};
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

/// Name of the Shared Drive that holds Bicameral Product ledgers. A name
/// is only a lookup hint: Shared Drive names are not unique, so a match is
/// reused only when it is unambiguous and the caller can write and share
/// in it.
pub const BICAMERAL_SHARED_DRIVE_NAME: &str = "Bicameral";

/// Secrets-store key for the non-secret provisioning hints. Stored next to
/// the Google connection credential because that store is the daemon's
/// durable, already-injected operator-local state; the value carries no
/// token material.
pub const DRIVE_STORAGE_HINTS_KEY: &str = "drive_storage_hints_v1";

/// Safe facts about the connected Google account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveAccount {
    pub email: String,
    pub can_create_drives: bool,
}

/// Safe facts about one Shared Drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedDriveFacts {
    pub id: String,
    pub name: String,
    pub can_add_children: bool,
    pub can_share: bool,
}

impl SharedDriveFacts {
    fn usable(&self) -> bool {
        self.can_add_children && self.can_share
    }
}

/// Typed provisioning failures. Never carries token material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriveProvisionError {
    /// Credential rejected by the provider (401).
    CredentialRejected,
    /// The provider understood the request and refused it (403): admin
    /// policy, missing capability, or a sharing restriction.
    Refused,
    /// `drives.create` answered 409: a drive already exists for this
    /// request id.
    AlreadyExists,
    /// Provider returned an error outside the classes above.
    Provider { safe_message: String },
    /// Transport-level failure reaching the provider.
    Network { safe_message: String },
}

pub type ProvisionFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, DriveProvisionError>> + Send + 'a>>;

/// Injected Drive transport for provisioning.
///
/// The live implementation calls the Drive REST API; tests inject
/// deterministic doubles. The access token flows through this boundary
/// transiently and must never be logged or stored by implementations.
pub trait DriveStorageProvisioner: Send + Sync {
    /// `about.get`: account email and whether it may create Shared Drives.
    fn account<'a>(&'a self, access_token: &'a str) -> ProvisionFuture<'a, DriveAccount>;

    /// `drives.get`: `None` when the drive is gone or not visible.
    fn get_shared_drive<'a>(
        &'a self,
        access_token: &'a str,
        drive_id: &'a str,
    ) -> ProvisionFuture<'a, Option<SharedDriveFacts>>;

    /// `drives.list` filtered to an exact name.
    fn find_shared_drives_by_name<'a>(
        &'a self,
        access_token: &'a str,
        name: &'a str,
    ) -> ProvisionFuture<'a, Vec<SharedDriveFacts>>;

    /// `drives.create` with an idempotency `requestId`.
    fn create_shared_drive<'a>(
        &'a self,
        access_token: &'a str,
        name: &'a str,
        request_id: &'a str,
    ) -> ProvisionFuture<'a, SharedDriveFacts>;

    /// `files.create` for one folder at the root of a Shared Drive.
    /// Returns the new folder id.
    fn create_folder<'a>(
        &'a self,
        access_token: &'a str,
        drive_id: &'a str,
        name: &'a str,
    ) -> ProvisionFuture<'a, String>;

    /// `permissions.create` on a folder: `type=domain`, `role=writer`,
    /// no discovery, no notification email.
    fn share_folder_with_domain<'a>(
        &'a self,
        access_token: &'a str,
        folder_id: &'a str,
        domain: &'a str,
    ) -> ProvisionFuture<'a, ()>;
}

// ── Live Drive REST transport ─────────────────────────────────────

/// Canonical Drive v3 REST base.
pub const GOOGLE_DRIVE_API_BASE: &str = "https://www.googleapis.com/drive/v3";

const DRIVE_CAPABILITY_FIELDS: &str = "id,name,capabilities(canAddChildren,canShare)";

/// Live Drive REST provisioner.
pub struct HttpDriveStorageProvisioner {
    client: reqwest::Client,
    base_url: String,
}

impl HttpDriveStorageProvisioner {
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_base_url(client, GOOGLE_DRIVE_API_BASE)
    }

    /// Point the transport at another origin. Production always uses
    /// [`GOOGLE_DRIVE_API_BASE`]; contract tests target a local stub.
    pub fn with_base_url(client: reqwest::Client, base_url: impl Into<String>) -> Self {
        Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
        }
    }

    async fn send(
        &self,
        request: reqwest::RequestBuilder,
        access_token: &str,
    ) -> Result<reqwest::Response, DriveProvisionError> {
        request
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|_| DriveProvisionError::Network {
                safe_message: "Could not reach Google Drive.".into(),
            })
    }
}

fn status_error(status: u16) -> DriveProvisionError {
    match status {
        401 => DriveProvisionError::CredentialRejected,
        403 => DriveProvisionError::Refused,
        409 => DriveProvisionError::AlreadyExists,
        status => DriveProvisionError::Provider {
            safe_message: format!("Google Drive returned status {status}."),
        },
    }
}

async fn read_json(response: reqwest::Response) -> Result<serde_json::Value, DriveProvisionError> {
    response
        .json()
        .await
        .map_err(|_| DriveProvisionError::Provider {
            safe_message: "Google Drive returned an unreadable response.".into(),
        })
}

fn drive_facts(json: &serde_json::Value) -> Result<SharedDriveFacts, DriveProvisionError> {
    let id = json["id"].as_str().unwrap_or_default();
    if id.is_empty() {
        return Err(DriveProvisionError::Provider {
            safe_message: "Google Drive returned a shared drive without an id.".into(),
        });
    }
    Ok(SharedDriveFacts {
        id: id.to_string(),
        name: json["name"].as_str().unwrap_or_default().to_string(),
        can_add_children: json["capabilities"]["canAddChildren"]
            .as_bool()
            .unwrap_or(false),
        can_share: json["capabilities"]["canShare"].as_bool().unwrap_or(false),
    })
}

impl DriveStorageProvisioner for HttpDriveStorageProvisioner {
    fn account<'a>(&'a self, access_token: &'a str) -> ProvisionFuture<'a, DriveAccount> {
        Box::pin(async move {
            let request = self
                .client
                .get(format!("{}/about", self.base_url))
                .query(&[("fields", "user(emailAddress),canCreateDrives")]);
            let response = self.send(request, access_token).await?;
            if response.status().as_u16() != 200 {
                return Err(status_error(response.status().as_u16()));
            }
            let json = read_json(response).await?;
            Ok(DriveAccount {
                email: json["user"]["emailAddress"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                can_create_drives: json["canCreateDrives"].as_bool().unwrap_or(false),
            })
        })
    }

    fn get_shared_drive<'a>(
        &'a self,
        access_token: &'a str,
        drive_id: &'a str,
    ) -> ProvisionFuture<'a, Option<SharedDriveFacts>> {
        Box::pin(async move {
            let request = self
                .client
                .get(format!("{}/drives/{}", self.base_url, drive_id))
                .query(&[("fields", DRIVE_CAPABILITY_FIELDS)]);
            let response = self.send(request, access_token).await?;
            match response.status().as_u16() {
                200 => Ok(Some(drive_facts(&read_json(response).await?)?)),
                404 => Ok(None),
                status => Err(status_error(status)),
            }
        })
    }

    fn find_shared_drives_by_name<'a>(
        &'a self,
        access_token: &'a str,
        name: &'a str,
    ) -> ProvisionFuture<'a, Vec<SharedDriveFacts>> {
        Box::pin(async move {
            // Drive query strings quote with `'`; escape `\` and `'`.
            let escaped = name.replace('\\', "\\\\").replace('\'', "\\'");
            let fields = format!("drives({DRIVE_CAPABILITY_FIELDS})");
            let request = self
                .client
                .get(format!("{}/drives", self.base_url))
                .query(&[
                    ("q", format!("name = '{escaped}'").as_str()),
                    ("pageSize", "100"),
                    ("fields", fields.as_str()),
                ]);
            let response = self.send(request, access_token).await?;
            if response.status().as_u16() != 200 {
                return Err(status_error(response.status().as_u16()));
            }
            let json = read_json(response).await?;
            json["drives"]
                .as_array()
                .map(|drives| drives.iter().map(drive_facts).collect())
                .unwrap_or_else(|| Ok(Vec::new()))
        })
    }

    fn create_shared_drive<'a>(
        &'a self,
        access_token: &'a str,
        name: &'a str,
        request_id: &'a str,
    ) -> ProvisionFuture<'a, SharedDriveFacts> {
        Box::pin(async move {
            let request = self
                .client
                .post(format!("{}/drives", self.base_url))
                .query(&[
                    ("requestId", request_id),
                    ("fields", DRIVE_CAPABILITY_FIELDS),
                ])
                .json(&serde_json::json!({ "name": name }));
            let response = self.send(request, access_token).await?;
            if response.status().as_u16() != 200 {
                return Err(status_error(response.status().as_u16()));
            }
            drive_facts(&read_json(response).await?)
        })
    }

    fn create_folder<'a>(
        &'a self,
        access_token: &'a str,
        drive_id: &'a str,
        name: &'a str,
    ) -> ProvisionFuture<'a, String> {
        Box::pin(async move {
            let request = self
                .client
                .post(format!("{}/files", self.base_url))
                .query(&[("supportsAllDrives", "true"), ("fields", "id")])
                .json(&serde_json::json!({
                    "name": name,
                    "mimeType": crate::google_resource_selection::DRIVE_FOLDER_MIME_TYPE,
                    "parents": [drive_id],
                }));
            let response = self.send(request, access_token).await?;
            if response.status().as_u16() != 200 {
                return Err(status_error(response.status().as_u16()));
            }
            let json = read_json(response).await?;
            match json["id"].as_str().filter(|id| !id.is_empty()) {
                Some(id) => Ok(id.to_string()),
                None => Err(DriveProvisionError::Provider {
                    safe_message: "Google Drive returned a folder without an id.".into(),
                }),
            }
        })
    }

    fn share_folder_with_domain<'a>(
        &'a self,
        access_token: &'a str,
        folder_id: &'a str,
        domain: &'a str,
    ) -> ProvisionFuture<'a, ()> {
        Box::pin(async move {
            let request = self
                .client
                .post(format!("{}/files/{}/permissions", self.base_url, folder_id))
                .query(&[
                    ("supportsAllDrives", "true"),
                    ("sendNotificationEmail", "false"),
                    ("fields", "id"),
                ])
                .json(&serde_json::json!({
                    "type": "domain",
                    "role": "writer",
                    "domain": domain,
                    "allowFileDiscovery": false,
                }));
            let response = self.send(request, access_token).await?;
            if response.status().as_u16() != 200 {
                return Err(status_error(response.status().as_u16()));
            }
            Ok(())
        })
    }
}

// ── Durable routing hints ─────────────────────────────────────────

/// Per-account provisioning hints. Routing data only: a stored drive id is
/// re-checked with `drives.get` before every use, and neither field grants
/// anything.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DriveStorageHint {
    /// Shared Drive this daemon last provisioned into for the account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_drive_id: Option<String>,
    /// Idempotency id of a `drives.create` that has been sent but whose
    /// result is not recorded yet. Written before the call so a retry can
    /// never create a second drive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_request_id: Option<String>,
}

type DriveStorageHints = BTreeMap<String, DriveStorageHint>;

fn load_hints(secrets: &dyn bicameral_config::SecretsStore) -> DriveStorageHints {
    secrets
        .read_secret(
            bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
            DRIVE_STORAGE_HINTS_KEY,
        )
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Read the hint for one Google account (case-insensitive email).
pub fn load_hint(secrets: &dyn bicameral_config::SecretsStore, account: &str) -> DriveStorageHint {
    load_hints(secrets)
        .remove(&account.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Persist the hint for one Google account.
pub fn store_hint(
    secrets: &dyn bicameral_config::SecretsStore,
    account: &str,
    hint: &DriveStorageHint,
) -> anyhow::Result<()> {
    let mut hints = load_hints(secrets);
    hints.insert(account.to_ascii_lowercase(), hint.clone());
    let raw = serde_json::to_string(&hints)
        .map_err(|_| anyhow::anyhow!("drive storage hint serialization failed"))?;
    secrets.write_secret(
        bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
        DRIVE_STORAGE_HINTS_KEY,
        &raw,
    )
}

// ── Orchestration ─────────────────────────────────────────────────

/// A typed non-ready result: wire state plus an operator-safe message.
type Refusal = (ProviderResourceState, String);

fn refusal(state: ProviderResourceState, message: impl Into<String>) -> Refusal {
    (state, message.into())
}

fn failed((state, message): Refusal) -> SelectionOutcome {
    SelectionOutcome::Failed { state, message }
}

/// Map a transport failure to a typed non-ready result. `refused` is the
/// step-specific message for a 403.
fn provision_error_refusal(error: &DriveProvisionError, refused: &str) -> Refusal {
    match error {
        DriveProvisionError::CredentialRejected => refusal(
            ProviderResourceState::ActionNeeded,
            "Google rejected the connection credential. Reconnect Google and retry.",
        ),
        DriveProvisionError::Refused => refusal(ProviderResourceState::ActionNeeded, refused),
        DriveProvisionError::AlreadyExists => refusal(
            ProviderResourceState::NotReady,
            "Google Drive reported a conflicting request. Try again.",
        ),
        DriveProvisionError::Provider { safe_message }
        | DriveProvisionError::Network { safe_message } => {
            refusal(ProviderResourceState::NotReady, safe_message.clone())
        }
    }
}

const CANNOT_CREATE_DRIVES_MESSAGE: &str =
    "Bicameral needs a Google Workspace account that can create shared drives. This account \
     cannot. Ask your Workspace admin to allow shared drive creation, or connect a Workspace \
     account that has it.";

const HINT_WRITE_FAILED_MESSAGE: &str =
    "Bicameral could not record Drive storage setup state on this device. Nothing was created. \
     Try again.";

/// Sharing domain of an account email: the part after the last `@`.
pub fn sharing_domain(email: &str) -> Option<String> {
    let (_, domain) = email.rsplit_once('@')?;
    let domain = domain.trim().to_ascii_lowercase();
    (!domain.is_empty() && domain.contains('.')).then_some(domain)
}

fn ledger_folder_name(now: chrono::DateTime<chrono::Utc>) -> String {
    format!("Bicameral ledger {}", now.format("%Y-%m-%d %H:%M:%S UTC"))
}

/// Resolve the Shared Drive to provision into: the stored drive, else one
/// unambiguous usable name match, else a newly created drive.
async fn resolve_shared_drive(
    provisioner: &dyn DriveStorageProvisioner,
    secrets: &dyn bicameral_config::SecretsStore,
    access_token: &str,
    account: &DriveAccount,
) -> Result<SharedDriveFacts, Refusal> {
    const DRIVE_REFUSED: &str =
        "Google Drive refused access to the Bicameral shared drive. Check your Workspace \
         shared drive permissions and retry.";
    let mut hint = load_hint(secrets, &account.email);

    // 1. The drive this daemon recorded, when it still exists and is usable.
    if let Some(drive_id) = hint.shared_drive_id.clone() {
        match provisioner.get_shared_drive(access_token, &drive_id).await {
            Ok(Some(drive)) if drive.usable() => return Ok(drive),
            Ok(_) => {}
            Err(error) => return Err(provision_error_refusal(&error, DRIVE_REFUSED)),
        }
    }

    // 2. A name match, only when it is unambiguous and usable. Listing lags
    // creation by several seconds, so this never proves absence.
    let matches = provisioner
        .find_shared_drives_by_name(access_token, BICAMERAL_SHARED_DRIVE_NAME)
        .await
        .map_err(|error| provision_error_refusal(&error, DRIVE_REFUSED))?;
    if let [only] = matches.as_slice() {
        if only.name == BICAMERAL_SHARED_DRIVE_NAME && only.usable() {
            hint.shared_drive_id = Some(only.id.clone());
            hint.create_request_id = None;
            // Best effort: the hint is only a shortcut for the next attempt.
            let _ = store_hint(secrets, &account.email, &hint);
            return Ok(only.clone());
        }
    }

    // 3. Create. The request id is durable BEFORE the call, so a retry
    // after a lost response repeats the same idempotent request.
    let mut reused_request = hint.create_request_id.is_some();
    loop {
        let request_id = match hint.create_request_id.clone() {
            Some(request_id) => request_id,
            None => {
                let request_id = uuid::Uuid::new_v4().to_string();
                hint.shared_drive_id = None;
                hint.create_request_id = Some(request_id.clone());
                if store_hint(secrets, &account.email, &hint).is_err() {
                    return Err(refusal(
                        ProviderResourceState::NotReady,
                        HINT_WRITE_FAILED_MESSAGE,
                    ));
                }
                request_id
            }
        };
        match provisioner
            .create_shared_drive(access_token, BICAMERAL_SHARED_DRIVE_NAME, &request_id)
            .await
        {
            Ok(drive) => {
                hint.shared_drive_id = Some(drive.id.clone());
                hint.create_request_id = None;
                let _ = store_hint(secrets, &account.email, &hint);
                return Ok(drive);
            }
            // An earlier attempt with this request id already created the
            // drive but its id was never recorded.
            Err(DriveProvisionError::AlreadyExists) if reused_request => {
                if matches.is_empty() {
                    // Not listed yet: keep the request id and let the owner
                    // retry once Google's listing catches up.
                    return Err(refusal(
                        ProviderResourceState::NotReady,
                        "Google is still finishing the Bicameral shared drive. Try again in a \
                         minute.",
                    ));
                }
                // The earlier drive cannot be told apart from other drives
                // with the same name; start a fresh idempotent request.
                hint.create_request_id = None;
                reused_request = false;
            }
            Err(DriveProvisionError::Refused) => {
                return Err(refusal(
                    ProviderResourceState::ActionNeeded,
                    CANNOT_CREATE_DRIVES_MESSAGE,
                ));
            }
            Err(error) => return Err(provision_error_refusal(&error, DRIVE_REFUSED)),
        }
    }
}

/// Provision Product ledger storage and validate it.
///
/// Fail-closed at every step; only daemon-side validation of the folder
/// this call created, inside the drive it resolved, can produce `Ready`.
pub async fn provision_ledger_folder(
    provisioner: &dyn DriveStorageProvisioner,
    probe: &dyn DriveResourceProbe,
    secrets: &dyn bicameral_config::SecretsStore,
    access_token: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> SelectionOutcome {
    match provision_and_validate(provisioner, probe, secrets, access_token, now).await {
        Ok(descriptor) => SelectionOutcome::Ready(descriptor),
        Err(refusal) => failed(refusal),
    }
}

async fn provision_and_validate(
    provisioner: &dyn DriveStorageProvisioner,
    probe: &dyn DriveResourceProbe,
    secrets: &dyn bicameral_config::SecretsStore,
    access_token: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<ProviderResourceDescriptor, Refusal> {
    let account = provisioner.account(access_token).await.map_err(|error| {
        provision_error_refusal(
            &error,
            "Google Drive refused to describe the connected account. Reconnect Google and \
             retry.",
        )
    })?;
    // No fallback: without Shared Drive creation (admin policy, or a
    // personal account) nothing is created or shared.
    if !account.can_create_drives {
        return Err(refusal(
            ProviderResourceState::ActionNeeded,
            CANNOT_CREATE_DRIVES_MESSAGE,
        ));
    }
    let Some(domain) = sharing_domain(&account.email) else {
        return Err(refusal(
            ProviderResourceState::NotReady,
            "Google Drive did not report an account email Bicameral can share with.",
        ));
    };

    let drive = resolve_shared_drive(provisioner, secrets, access_token, &account).await?;

    let folder_id = provisioner
        .create_folder(access_token, &drive.id, &ledger_folder_name(now))
        .await
        .map_err(|error| {
            provision_error_refusal(
                &error,
                "Google Drive refused to create the ledger folder in the Bicameral shared \
                 drive.",
            )
        })?;

    // The single permission change this system makes (ARCH-0007
    // Decision 2): the ledger folder, the owner's own domain, nothing else.
    provisioner
        .share_folder_with_domain(access_token, &folder_id, &domain)
        .await
        .map_err(|error| {
            provision_error_refusal(
                &error,
                &format!(
                    "Google Drive refused to share the ledger folder with {domain}. Your \
                     Workspace sharing settings may block it; ask your Workspace admin."
                ),
            )
        })?;

    let metadata = probe
        .get_file_metadata(access_token, &folder_id)
        .await
        .map_err(|error| probe_error_state(&error))?;
    if metadata.drive_id.as_deref() != Some(drive.id.as_str()) {
        return Err(refusal(
            ProviderResourceState::NotReady,
            "The ledger folder is not in the Bicameral shared drive.",
        ));
    }
    validate_folder_metadata(&folder_id, &metadata)
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use std::sync::Mutex;

    /// One recorded provisioner call, in order.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum Call {
        Account,
        GetDrive(String),
        FindDrives(String),
        CreateDrive { name: String, request_id: String },
        CreateFolder { drive_id: String, name: String },
        ShareDomain { folder_id: String, domain: String },
    }

    /// Scripted provisioner double that records every call.
    pub(crate) struct ScriptedProvisioner {
        pub(crate) account: Result<DriveAccount, DriveProvisionError>,
        /// Drives visible to `drives.get`.
        pub(crate) drives: Mutex<Vec<SharedDriveFacts>>,
        /// Result of `drives.list` by name.
        pub(crate) listed: Mutex<Vec<SharedDriveFacts>>,
        /// Queued `drives.create` results; when empty a drive
        /// `created-drive-N` is produced.
        pub(crate) create_results: Mutex<Vec<Result<SharedDriveFacts, DriveProvisionError>>>,
        pub(crate) folder: Result<String, DriveProvisionError>,
        pub(crate) share: Result<(), DriveProvisionError>,
        pub(crate) calls: Mutex<Vec<Call>>,
    }

    pub(crate) fn drive(id: &str) -> SharedDriveFacts {
        SharedDriveFacts {
            id: id.into(),
            name: BICAMERAL_SHARED_DRIVE_NAME.into(),
            can_add_children: true,
            can_share: true,
        }
    }

    impl ScriptedProvisioner {
        pub(crate) fn workspace() -> Self {
            Self {
                account: Ok(DriveAccount {
                    email: "Owner@Example.com".into(),
                    can_create_drives: true,
                }),
                drives: Mutex::new(Vec::new()),
                listed: Mutex::new(Vec::new()),
                create_results: Mutex::new(Vec::new()),
                folder: Ok("ledger-folder-1".into()),
                share: Ok(()),
                calls: Mutex::new(Vec::new()),
            }
        }

        pub(crate) fn calls(&self) -> Vec<Call> {
            self.calls.lock().unwrap().clone()
        }

        fn record(&self, call: Call) {
            self.calls.lock().unwrap().push(call);
        }
    }

    impl DriveStorageProvisioner for ScriptedProvisioner {
        fn account<'a>(&'a self, _access_token: &'a str) -> ProvisionFuture<'a, DriveAccount> {
            self.record(Call::Account);
            let result = self.account.clone();
            Box::pin(async move { result })
        }

        fn get_shared_drive<'a>(
            &'a self,
            _access_token: &'a str,
            drive_id: &'a str,
        ) -> ProvisionFuture<'a, Option<SharedDriveFacts>> {
            self.record(Call::GetDrive(drive_id.into()));
            let found = self
                .drives
                .lock()
                .unwrap()
                .iter()
                .find(|drive| drive.id == drive_id)
                .cloned();
            Box::pin(async move { Ok(found) })
        }

        fn find_shared_drives_by_name<'a>(
            &'a self,
            _access_token: &'a str,
            name: &'a str,
        ) -> ProvisionFuture<'a, Vec<SharedDriveFacts>> {
            self.record(Call::FindDrives(name.into()));
            let listed = self.listed.lock().unwrap().clone();
            Box::pin(async move { Ok(listed) })
        }

        fn create_shared_drive<'a>(
            &'a self,
            _access_token: &'a str,
            name: &'a str,
            request_id: &'a str,
        ) -> ProvisionFuture<'a, SharedDriveFacts> {
            self.record(Call::CreateDrive {
                name: name.into(),
                request_id: request_id.into(),
            });
            let mut queued = self.create_results.lock().unwrap();
            let result = if queued.is_empty() {
                let created = self
                    .calls
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|call| matches!(call, Call::CreateDrive { .. }))
                    .count();
                Ok(drive(&format!("created-drive-{created}")))
            } else {
                queued.remove(0)
            };
            if let Ok(created) = &result {
                self.drives.lock().unwrap().push(created.clone());
            }
            Box::pin(async move { result })
        }

        fn create_folder<'a>(
            &'a self,
            _access_token: &'a str,
            drive_id: &'a str,
            name: &'a str,
        ) -> ProvisionFuture<'a, String> {
            self.record(Call::CreateFolder {
                drive_id: drive_id.into(),
                name: name.into(),
            });
            let result = self.folder.clone();
            Box::pin(async move { result })
        }

        fn share_folder_with_domain<'a>(
            &'a self,
            _access_token: &'a str,
            folder_id: &'a str,
            domain: &'a str,
        ) -> ProvisionFuture<'a, ()> {
            self.record(Call::ShareDomain {
                folder_id: folder_id.into(),
                domain: domain.into(),
            });
            let result = self.share.clone();
            Box::pin(async move { result })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{drive, Call, ScriptedProvisioner};
    use super::*;
    use crate::google_resource_selection::{
        DriveFileMetadata, DriveProbeFuture, DRIVE_FOLDER_MIME_TYPE,
    };
    use bicameral_config::{InMemorySecretsStore, SecretsStore};
    use std::sync::Mutex;

    const ACCOUNT: &str = "Owner@Example.com";

    /// Probe double reporting the folder inside `drive_id`.
    struct FolderProbe {
        drive_id: Option<String>,
        can_add_children: bool,
    }

    impl FolderProbe {
        fn in_drive(drive_id: &str) -> Self {
            Self {
                drive_id: Some(drive_id.into()),
                can_add_children: true,
            }
        }
    }

    impl DriveResourceProbe for FolderProbe {
        fn get_file_metadata<'a>(
            &'a self,
            _access_token: &'a str,
            file_id: &'a str,
        ) -> DriveProbeFuture<'a> {
            let metadata = DriveFileMetadata {
                id: file_id.into(),
                name: "Bicameral ledger".into(),
                mime_type: DRIVE_FOLDER_MIME_TYPE.into(),
                trashed: false,
                drive_id: self.drive_id.clone(),
                can_add_children: self.can_add_children,
                can_list_children: Some(true),
            };
            Box::pin(async move { Ok(metadata) })
        }
    }

    async fn run(
        provisioner: &ScriptedProvisioner,
        probe: &FolderProbe,
        secrets: &dyn SecretsStore,
    ) -> SelectionOutcome {
        provision_ledger_folder(provisioner, probe, secrets, "token", chrono::Utc::now()).await
    }

    fn share_calls(provisioner: &ScriptedProvisioner) -> Vec<Call> {
        provisioner
            .calls()
            .into_iter()
            .filter(|call| matches!(call, Call::ShareDomain { .. }))
            .collect()
    }

    fn create_drive_calls(provisioner: &ScriptedProvisioner) -> Vec<String> {
        provisioner
            .calls()
            .into_iter()
            .filter_map(|call| match call {
                Call::CreateDrive { request_id, .. } => Some(request_id),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn fresh_account_creates_drive_folder_and_one_domain_share() {
        let provisioner = ScriptedProvisioner::workspace();
        let secrets = InMemorySecretsStore::new();
        let outcome = run(
            &provisioner,
            &FolderProbe::in_drive("created-drive-1"),
            &secrets,
        )
        .await;

        let SelectionOutcome::Ready(descriptor) = outcome else {
            panic!("expected Ready, got {outcome:?}");
        };
        assert_eq!(descriptor.folder_id, "ledger-folder-1");
        assert_eq!(descriptor.drive_id.as_deref(), Some("created-drive-1"));

        let calls = provisioner.calls();
        assert!(matches!(calls[0], Call::Account));
        assert!(calls.iter().any(|call| matches!(
            call,
            Call::CreateFolder { drive_id, name }
                if drive_id == "created-drive-1" && name.starts_with("Bicameral ledger ")
        )));
        // The only permission call: the owner's domain, on the folder.
        assert_eq!(
            share_calls(&provisioner),
            vec![Call::ShareDomain {
                folder_id: "ledger-folder-1".into(),
                domain: "example.com".into(),
            }]
        );

        // The drive id is recorded and the pending request id is cleared.
        let hint = load_hint(&secrets, ACCOUNT);
        assert_eq!(hint.shared_drive_id.as_deref(), Some("created-drive-1"));
        assert_eq!(hint.create_request_id, None);
    }

    #[tokio::test]
    async fn account_that_cannot_create_drives_is_refused_and_nothing_is_created() {
        let mut provisioner = ScriptedProvisioner::workspace();
        provisioner.account = Ok(DriveAccount {
            email: "owner@personal.example".into(),
            can_create_drives: false,
        });
        let secrets = InMemorySecretsStore::new();
        let outcome = run(&provisioner, &FolderProbe::in_drive("x"), &secrets).await;

        assert_eq!(
            outcome,
            SelectionOutcome::Failed {
                state: ProviderResourceState::ActionNeeded,
                message: CANNOT_CREATE_DRIVES_MESSAGE.into(),
            }
        );
        assert_eq!(provisioner.calls(), vec![Call::Account]);
        assert_eq!(
            load_hint(&secrets, "owner@personal.example"),
            Default::default()
        );
    }

    #[tokio::test]
    async fn stored_drive_id_is_reused_without_listing_or_creating() {
        let provisioner = ScriptedProvisioner::workspace();
        provisioner
            .drives
            .lock()
            .unwrap()
            .push(drive("stored-drive"));
        let secrets = InMemorySecretsStore::new();
        store_hint(
            &secrets,
            ACCOUNT,
            &DriveStorageHint {
                shared_drive_id: Some("stored-drive".into()),
                create_request_id: None,
            },
        )
        .unwrap();

        let outcome = run(
            &provisioner,
            &FolderProbe::in_drive("stored-drive"),
            &secrets,
        )
        .await;
        assert!(matches!(outcome, SelectionOutcome::Ready(_)));
        let calls = provisioner.calls();
        assert!(calls.contains(&Call::GetDrive("stored-drive".into())));
        assert!(!calls.iter().any(|call| matches!(call, Call::FindDrives(_))));
        assert!(create_drive_calls(&provisioner).is_empty());
    }

    #[tokio::test]
    async fn vanished_stored_drive_falls_through_to_a_new_drive() {
        let provisioner = ScriptedProvisioner::workspace();
        let secrets = InMemorySecretsStore::new();
        store_hint(
            &secrets,
            ACCOUNT,
            &DriveStorageHint {
                shared_drive_id: Some("deleted-drive".into()),
                create_request_id: None,
            },
        )
        .unwrap();

        let outcome = run(
            &provisioner,
            &FolderProbe::in_drive("created-drive-1"),
            &secrets,
        )
        .await;
        assert!(matches!(outcome, SelectionOutcome::Ready(_)));
        assert_eq!(create_drive_calls(&provisioner).len(), 1);
        assert_eq!(
            load_hint(&secrets, ACCOUNT).shared_drive_id.as_deref(),
            Some("created-drive-1")
        );
    }

    #[tokio::test]
    async fn request_id_is_persisted_before_create_and_reused_on_retry() {
        // First attempt: create fails at the transport after the request id
        // was recorded.
        let provisioner = ScriptedProvisioner::workspace();
        provisioner
            .create_results
            .lock()
            .unwrap()
            .push(Err(DriveProvisionError::Network {
                safe_message: "Could not reach Google Drive.".into(),
            }));
        let secrets = InMemorySecretsStore::new();
        let first = run(
            &provisioner,
            &FolderProbe::in_drive("created-drive-2"),
            &secrets,
        )
        .await;
        assert!(matches!(first, SelectionOutcome::Failed { .. }));
        let pending = load_hint(&secrets, ACCOUNT)
            .create_request_id
            .expect("request id is durable before the create call");
        assert_eq!(create_drive_calls(&provisioner), vec![pending.clone()]);
        assert!(share_calls(&provisioner).is_empty());

        // Retry: the same request id is sent again.
        let second = run(
            &provisioner,
            &FolderProbe::in_drive("created-drive-2"),
            &secrets,
        )
        .await;
        assert!(matches!(second, SelectionOutcome::Ready(_)));
        assert_eq!(
            create_drive_calls(&provisioner),
            vec![pending.clone(), pending]
        );
        assert_eq!(load_hint(&secrets, ACCOUNT).create_request_id, None);
    }

    #[tokio::test]
    async fn conflict_on_a_reused_request_id_waits_for_listing_instead_of_duplicating() {
        let provisioner = ScriptedProvisioner::workspace();
        provisioner
            .create_results
            .lock()
            .unwrap()
            .push(Err(DriveProvisionError::AlreadyExists));
        let secrets = InMemorySecretsStore::new();
        store_hint(
            &secrets,
            ACCOUNT,
            &DriveStorageHint {
                shared_drive_id: None,
                create_request_id: Some("req-earlier".into()),
            },
        )
        .unwrap();

        let outcome = run(&provisioner, &FolderProbe::in_drive("x"), &secrets).await;
        assert!(matches!(
            outcome,
            SelectionOutcome::Failed {
                state: ProviderResourceState::NotReady,
                ..
            }
        ));
        // One create attempt, with the earlier id, and the id is kept.
        assert_eq!(create_drive_calls(&provisioner), vec!["req-earlier"]);
        assert_eq!(
            load_hint(&secrets, ACCOUNT).create_request_id.as_deref(),
            Some("req-earlier")
        );
        assert!(share_calls(&provisioner).is_empty());
    }

    #[tokio::test]
    async fn single_usable_name_match_is_reused() {
        let provisioner = ScriptedProvisioner::workspace();
        *provisioner.listed.lock().unwrap() = vec![drive("existing-drive")];
        let secrets = InMemorySecretsStore::new();

        let outcome = run(
            &provisioner,
            &FolderProbe::in_drive("existing-drive"),
            &secrets,
        )
        .await;
        assert!(matches!(outcome, SelectionOutcome::Ready(_)));
        assert!(create_drive_calls(&provisioner).is_empty());
        assert_eq!(
            load_hint(&secrets, ACCOUNT).shared_drive_id.as_deref(),
            Some("existing-drive")
        );
    }

    #[tokio::test]
    async fn ambiguous_or_unusable_name_matches_create_a_new_drive() {
        // Two drives with the same name.
        let provisioner = ScriptedProvisioner::workspace();
        *provisioner.listed.lock().unwrap() = vec![drive("drive-a"), drive("drive-b")];
        let secrets = InMemorySecretsStore::new();
        let outcome = run(
            &provisioner,
            &FolderProbe::in_drive("created-drive-1"),
            &secrets,
        )
        .await;
        assert!(matches!(outcome, SelectionOutcome::Ready(_)));
        assert_eq!(create_drive_calls(&provisioner).len(), 1);

        // One match the caller cannot share in.
        let provisioner = ScriptedProvisioner::workspace();
        let mut read_only = drive("someone-elses-drive");
        read_only.can_share = false;
        *provisioner.listed.lock().unwrap() = vec![read_only];
        let secrets = InMemorySecretsStore::new();
        let outcome = run(
            &provisioner,
            &FolderProbe::in_drive("created-drive-1"),
            &secrets,
        )
        .await;
        assert!(matches!(outcome, SelectionOutcome::Ready(_)));
        assert_eq!(create_drive_calls(&provisioner).len(), 1);
    }

    #[tokio::test]
    async fn refused_domain_share_is_not_ready() {
        let mut provisioner = ScriptedProvisioner::workspace();
        provisioner.share = Err(DriveProvisionError::Refused);
        let secrets = InMemorySecretsStore::new();
        let outcome = run(
            &provisioner,
            &FolderProbe::in_drive("created-drive-1"),
            &secrets,
        )
        .await;
        let SelectionOutcome::Failed { state, message } = outcome else {
            panic!("a refused share must not be Ready");
        };
        assert_eq!(state, ProviderResourceState::ActionNeeded);
        assert!(message.contains("example.com"));
    }

    #[tokio::test]
    async fn folder_outside_the_provisioned_drive_is_not_ready() {
        for probe in [
            FolderProbe::in_drive("some-other-drive"),
            FolderProbe {
                drive_id: None,
                can_add_children: true,
            },
        ] {
            let provisioner = ScriptedProvisioner::workspace();
            let secrets = InMemorySecretsStore::new();
            let outcome = run(&provisioner, &probe, &secrets).await;
            assert!(matches!(
                outcome,
                SelectionOutcome::Failed {
                    state: ProviderResourceState::NotReady,
                    ..
                }
            ));
        }
    }

    #[tokio::test]
    async fn unwritable_folder_fails_validation() {
        let provisioner = ScriptedProvisioner::workspace();
        let secrets = InMemorySecretsStore::new();
        let probe = FolderProbe {
            drive_id: Some("created-drive-1".into()),
            can_add_children: false,
        };
        let outcome = run(&provisioner, &probe, &secrets).await;
        assert!(matches!(
            outcome,
            SelectionOutcome::Failed {
                state: ProviderResourceState::ActionNeeded,
                ..
            }
        ));
    }

    /// Store whose writes fail: the request id cannot be made durable.
    struct ReadOnlyStore;
    impl SecretsStore for ReadOnlyStore {
        fn read_secret(&self, _s: &str, _k: &str) -> Option<String> {
            None
        }
        fn write_secret(&self, _s: &str, _k: &str, _v: &str) -> anyhow::Result<()> {
            anyhow::bail!("write refused")
        }
        fn delete_secret(&self, _s: &str, _k: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn create_is_not_sent_when_the_request_id_cannot_be_recorded() {
        let provisioner = ScriptedProvisioner::workspace();
        let outcome = run(&provisioner, &FolderProbe::in_drive("x"), &ReadOnlyStore).await;
        assert!(matches!(outcome, SelectionOutcome::Failed { .. }));
        assert!(create_drive_calls(&provisioner).is_empty());
    }

    #[test]
    fn sharing_domain_is_the_lowercased_email_domain() {
        assert_eq!(
            sharing_domain("Owner@Example.com").as_deref(),
            Some("example.com")
        );
        assert_eq!(sharing_domain("no-at-sign"), None);
        assert_eq!(sharing_domain("trailing@"), None);
        assert_eq!(sharing_domain("user@localhost"), None);
    }

    #[test]
    fn hints_are_per_account_and_carry_no_token_material() {
        let secrets = InMemorySecretsStore::new();
        store_hint(
            &secrets,
            "A@Example.com",
            &DriveStorageHint {
                shared_drive_id: Some("drive-a".into()),
                create_request_id: None,
            },
        )
        .unwrap();
        assert_eq!(
            load_hint(&secrets, "a@example.com")
                .shared_drive_id
                .as_deref(),
            Some("drive-a")
        );
        assert_eq!(load_hint(&secrets, "b@example.com"), Default::default());
        let raw = secrets
            .read_secret(
                bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
                DRIVE_STORAGE_HINTS_KEY,
            )
            .unwrap();
        assert!(!raw.contains("token"));
    }

    // ── Live transport contract (local stub, no network) ──────────

    #[derive(Debug, Clone)]
    struct Recorded {
        method: String,
        path: String,
        query: String,
        authorization: String,
        body: String,
    }

    async fn spawn_drive_stub(
        responses: Vec<(u16, serde_json::Value)>,
    ) -> (String, std::sync::Arc<Mutex<Vec<Recorded>>>) {
        use axum::extract::Request;
        let recorded = std::sync::Arc::new(Mutex::new(Vec::<Recorded>::new()));
        let queue = std::sync::Arc::new(Mutex::new(responses));
        let sink = recorded.clone();
        let app = axum::Router::new().fallback(move |request: Request| {
            let sink = sink.clone();
            let queue = queue.clone();
            async move {
                let (parts, body) = request.into_parts();
                let bytes = axum::body::to_bytes(body, 65536).await.unwrap();
                sink.lock().unwrap().push(Recorded {
                    method: parts.method.to_string(),
                    path: parts.uri.path().to_string(),
                    query: parts.uri.query().unwrap_or_default().to_string(),
                    authorization: parts
                        .headers
                        .get("authorization")
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or_default()
                        .to_string(),
                    body: String::from_utf8(bytes.to_vec()).unwrap(),
                });
                let (status, json) = queue.lock().unwrap().remove(0);
                (
                    axum::http::StatusCode::from_u16(status).unwrap(),
                    [("content-type", "application/json")],
                    json.to_string(),
                )
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{address}/drive/v3"), recorded)
    }

    fn query_pairs(query: &str) -> BTreeMap<String, String> {
        reqwest::Url::parse(&format!("http://stub.invalid/?{query}"))
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect()
    }

    #[tokio::test]
    async fn live_transport_sends_the_documented_requests() {
        let drive_json = serde_json::json!({
            "id": "drive-1",
            "name": "Bicameral",
            "capabilities": {"canAddChildren": true, "canShare": true}
        });
        let (base, recorded) = spawn_drive_stub(vec![
            (
                200,
                serde_json::json!({"user": {"emailAddress": "o@example.com"}, "canCreateDrives": true}),
            ),
            (200, drive_json.clone()),
            (404, serde_json::json!({})),
            (200, serde_json::json!({"drives": [drive_json.clone()]})),
            (200, drive_json.clone()),
            (409, serde_json::json!({})),
            (200, serde_json::json!({"id": "folder-1"})),
            (200, serde_json::json!({"id": "perm-1"})),
            (403, serde_json::json!({})),
        ])
        .await;
        let live = HttpDriveStorageProvisioner::with_base_url(reqwest::Client::new(), base);

        let account = live.account("tok").await.unwrap();
        assert_eq!(account.email, "o@example.com");
        assert!(account.can_create_drives);
        assert_eq!(
            live.get_shared_drive("tok", "drive-1").await.unwrap(),
            Some(drive("drive-1"))
        );
        assert_eq!(live.get_shared_drive("tok", "gone").await.unwrap(), None);
        assert_eq!(
            live.find_shared_drives_by_name("tok", "Bicameral")
                .await
                .unwrap(),
            vec![drive("drive-1")]
        );
        assert_eq!(
            live.create_shared_drive("tok", "Bicameral", "req-1")
                .await
                .unwrap(),
            drive("drive-1")
        );
        assert_eq!(
            live.create_shared_drive("tok", "Bicameral", "req-1").await,
            Err(DriveProvisionError::AlreadyExists)
        );
        assert_eq!(
            live.create_folder("tok", "drive-1", "Bicameral ledger x")
                .await
                .unwrap(),
            "folder-1"
        );
        live.share_folder_with_domain("tok", "folder-1", "example.com")
            .await
            .unwrap();
        assert_eq!(
            live.share_folder_with_domain("tok", "folder-1", "example.com")
                .await,
            Err(DriveProvisionError::Refused)
        );

        let recorded = recorded.lock().unwrap().clone();
        assert!(recorded
            .iter()
            .all(|request| request.authorization == "Bearer tok"));

        // about
        assert_eq!(
            (recorded[0].method.as_str(), recorded[0].path.as_str()),
            ("GET", "/drive/v3/about")
        );
        assert_eq!(
            query_pairs(&recorded[0].query)["fields"],
            "user(emailAddress),canCreateDrives"
        );
        // drives.get
        assert_eq!(
            (recorded[1].method.as_str(), recorded[1].path.as_str()),
            ("GET", "/drive/v3/drives/drive-1")
        );
        // drives.list by exact name
        assert_eq!(
            (recorded[3].method.as_str(), recorded[3].path.as_str()),
            ("GET", "/drive/v3/drives")
        );
        assert_eq!(query_pairs(&recorded[3].query)["q"], "name = 'Bicameral'");
        // drives.create
        assert_eq!(
            (recorded[4].method.as_str(), recorded[4].path.as_str()),
            ("POST", "/drive/v3/drives")
        );
        assert_eq!(query_pairs(&recorded[4].query)["requestId"], "req-1");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&recorded[4].body).unwrap(),
            serde_json::json!({"name": "Bicameral"})
        );
        // files.create for the folder
        assert_eq!(
            (recorded[6].method.as_str(), recorded[6].path.as_str()),
            ("POST", "/drive/v3/files")
        );
        assert_eq!(query_pairs(&recorded[6].query)["supportsAllDrives"], "true");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&recorded[6].body).unwrap(),
            serde_json::json!({
                "name": "Bicameral ledger x",
                "mimeType": DRIVE_FOLDER_MIME_TYPE,
                "parents": ["drive-1"],
            })
        );
        // permissions.create: the domain, on the folder, no discovery.
        assert_eq!(
            (recorded[7].method.as_str(), recorded[7].path.as_str()),
            ("POST", "/drive/v3/files/folder-1/permissions")
        );
        let share_query = query_pairs(&recorded[7].query);
        assert_eq!(share_query["supportsAllDrives"], "true");
        assert_eq!(share_query["sendNotificationEmail"], "false");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&recorded[7].body).unwrap(),
            serde_json::json!({
                "type": "domain",
                "role": "writer",
                "domain": "example.com",
                "allowFileDiscovery": false,
            })
        );
    }
}
