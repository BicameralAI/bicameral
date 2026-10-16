//! Operator-local Product registration persistence and event-store routing.
//!
//! A Product's canonical event ledger stores Product facts. It cannot also be
//! the locator needed to find that ledger after a daemon restart. This module
//! owns the separate operator-control-plane mapping from canonical Product id
//! to a validated local event-store route.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bicameral_api::event_store::{EventStoreAdapter, SubstrateKind};
use bicameral_api::product_creation::{ProductCreateParams, ProductCreationResult};
use bicameral_api::product_registration::{
    ConnectionMetadata, ProjectRegistration, RegisteredProvider, RegisteredSubstrate,
};
use bicameral_config::{BicameralPaths, EventStoreConfig};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::event_store_adapters::{DriveFolderAdapter, GitAdapter, GitAdapterConfig};

const REGISTRY_SCHEMA_VERSION: u32 = 1;

/// Operator-private descriptor used to open one Product's event store.
///
/// Unlike bridge-safe [`ProjectRegistration`], this type may carry a local
/// path. It is persisted only in the operator registry and is never returned
/// through hosted projections or operational logs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "substrate", rename_all = "snake_case", deny_unknown_fields)]
pub enum EventStoreRoute {
    Git {
        events_dir: PathBuf,
        #[serde(default)]
        ci_merge_gate_configured: bool,
    },
    DriveFolder {
        folder: PathBuf,
    },
    /// Real Google Drive REST substrate (#1087 resolver step 5): the
    /// validated folder id routes to the EXACT production Drive API
    /// adapter over the OS-keyring credential envelope. Records with this
    /// variant are authored only by governed provider-resolution paths
    /// (a caller-supplied folder id is routing input, never resource
    /// evidence); this layer merely reopens them, including after daemon
    /// restart.
    GoogleDriveApi {
        folder_id: String,
    },
}

impl EventStoreRoute {
    /// Resolve the route for the daemon's explicitly configured workspace.
    pub fn from_config(config: &EventStoreConfig, paths: &BicameralPaths) -> Option<Self> {
        match config.substrate {
            SubstrateKind::Git => Some(Self::Git {
                events_dir: config.git_path.as_ref().map_or_else(
                    || paths.events_dir.clone(),
                    |path| resolve_workspace_path(path, &paths.workspace_root),
                ),
                ci_merge_gate_configured: config.ci_merge_gate,
            }),
            SubstrateKind::DriveFolder => {
                config
                    .drive_folder_path
                    .clone()
                    .map(|folder| Self::DriveFolder {
                        folder: resolve_workspace_path(&folder, &paths.workspace_root),
                    })
            }
            // Memory is process-local and cannot be reopened after restart, so
            // it must never be persisted as a durable Product route.
            SubstrateKind::Memory => None,
        }
    }

    pub(crate) fn open(&self) -> Arc<dyn EventStoreAdapter> {
        self.open_dual().event_store
    }

    /// Open the route for a canonical create, retaining the provider-confirmed
    /// append seam when the substrate is the real Google Drive API.
    ///
    /// Only [`EventStoreRoute::GoogleDriveApi`] yields a [`ConfirmedAppend`]
    /// handle; Git and synced-folder routes return `None` and stay non-v2
    /// (they do not write the same canonical JSON bytes with provider readback).
    pub(crate) fn open_for_create(
        &self,
    ) -> (
        Arc<dyn EventStoreAdapter>,
        Option<Arc<dyn crate::drive_api_event_store::ConfirmedAppend>>,
    ) {
        match self {
            Self::GoogleDriveApi { folder_id } => {
                match crate::google_drive_credential_route::open_google_drive_api_adapter(folder_id)
                {
                    Ok(concrete) => (
                        concrete.clone() as Arc<dyn EventStoreAdapter>,
                        Some(concrete as Arc<dyn crate::drive_api_event_store::ConfirmedAppend>),
                    ),
                    Err(_) => (self.open(), None),
                }
            }
            _ => (self.open(), None),
        }
    }

    /// Open the Product ledger with the atomic batch capability required by
    /// `candidate_selection.commit`.
    pub fn open_atomic(&self) -> Arc<dyn crate::atomic_event_store::AtomicEventStoreAdapter> {
        self.open_dual().atomic
    }

    fn open_dual(&self) -> crate::event_store_adapters::DualEventStoreAdapter {
        match self {
            Self::Git {
                events_dir,
                ci_merge_gate_configured,
            } => {
                let concrete = Arc::new(GitAdapter::with_config(
                    events_dir,
                    GitAdapterConfig {
                        ci_merge_gate_configured: *ci_merge_gate_configured,
                    },
                ));
                crate::event_store_adapters::DualEventStoreAdapter {
                    event_store: concrete.clone(),
                    atomic: concrete,
                }
            }
            Self::DriveFolder { folder } => {
                let concrete = Arc::new(DriveFolderAdapter::new(folder));
                crate::event_store_adapters::DualEventStoreAdapter {
                    event_store: concrete.clone(),
                    atomic: concrete,
                }
            }
            Self::GoogleDriveApi { folder_id } => {
                crate::drive_api_transport::open_google_drive_api_route(folder_id)
            }
        }
    }
}

fn resolve_workspace_path(path: &Path, workspace_root: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    }
}

/// One durable operator-local Product route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectRouteRecord {
    pub registration: ProjectRegistration,
    pub event_store: EventStoreRoute,
    /// Daemon-signed, provider-confirmed `ProductCreationCommitProof` (v2) for
    /// the canonical `CreateProduct` event, recorded at operator-scoped create
    /// and returned on `product.publish` (bot#1087). Absent for routes that
    /// predate the proof producer or that use a non-v2 substrate; it carries
    /// no credential or local path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_event_commit_proof:
        Option<bicameral_api::product_creation::ProductCreationCommitProof>,
}

impl ProjectRouteRecord {
    /// Build the operator route paired with a successful canonical creation.
    pub fn from_creation(
        result: &ProductCreationResult,
        params: &ProductCreateParams,
        event_store: EventStoreRoute,
    ) -> Self {
        let provider = match params
            .event_store_hint
            .as_ref()
            .and_then(|hint| hint.provider.as_deref())
        {
            Some("github") => RegisteredProvider::Github,
            Some("google") => RegisteredProvider::Google,
            _ => RegisteredProvider::None,
        };
        let substrate = match event_store {
            EventStoreRoute::Git { .. } => RegisteredSubstrate::Git,
            // The Google REST route reports the drive-folder substrate
            // family, matching the adapter's own substrate_kind; a
            // dedicated registered-substrate taxonomy entry belongs to
            // the remaining #1087 create slice that authors such records.
            EventStoreRoute::DriveFolder { .. } | EventStoreRoute::GoogleDriveApi { .. } => {
                RegisteredSubstrate::DriveFolder
            }
        };
        let registered_at = chrono::DateTime::parse_from_rfc3339(&result.created_at)
            .map(|value| value.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());
        Self {
            registration: ProjectRegistration {
                product_id: result.product_id.clone(),
                cloud_account_id: result.creator_cloud_account_id.clone(),
                registration_ref: format!("creation:{}", result.receipt_id),
                connection: ConnectionMetadata {
                    provider,
                    substrate,
                    account_label: None,
                    descriptor_ref: format!("local:{}", result.product_id),
                },
                governance_policy_version: result.governance_policy_version.clone(),
                registered_at,
            },
            event_store,
            first_event_commit_proof: None,
        }
    }

    /// Build a route while migrating a canonical Product already present in
    /// the daemon's explicitly configured event store.
    pub fn from_product_record(
        record: &crate::product_creation::ProductRecord,
        event_store: EventStoreRoute,
    ) -> Self {
        Self::from_creation(
            &record.to_result(),
            &ProductCreateParams {
                idempotency_key: record.idempotency_key.clone(),
                product_display_name: record.product_display_name.clone(),
                event_store_descriptor: None,
                asserted_cloud_account_id: Some(record.creator_cloud_account_id.clone()),
                team_display_name: record.team_display_name.clone(),
                verified_domain: record.verified_domain.clone(),
                event_store_hint: record.event_store_hint.clone(),
                required_connection: record.required_connection.clone(),
            },
            event_store,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryFile {
    schema_version: u32,
    record: ProjectRouteRecord,
}

/// Registry persistence or routing failure.
#[derive(Debug, Error)]
pub enum ProjectRegistryError {
    #[error("invalid Product id for operator registry")]
    InvalidProductId,
    #[error("conflicting route already exists for Product {product_id}")]
    ConflictingRoute { product_id: String },
    #[error("Product {product_id} is not registered with this daemon")]
    NotRegistered { product_id: String },
    #[error("unsupported registry schema version {version}")]
    UnsupportedSchema { version: u32 },
    #[error("operator registry I/O failed: {0}")]
    Io(String),
    #[error("operator registry record is invalid: {0}")]
    Serialization(String),
}

/// Persistence boundary for operator-local Product routes.
pub trait ProjectRegistrationStore: Send + Sync {
    fn get(
        &self,
        product_id: &str,
        cloud_account_id: &str,
    ) -> Result<Option<ProjectRouteRecord>, ProjectRegistryError>;
    fn list(&self) -> Result<Vec<ProjectRouteRecord>, ProjectRegistryError>;
    fn write(&self, record: &ProjectRouteRecord) -> Result<(), ProjectRegistryError>;

    fn find_product(
        &self,
        product_id: &str,
    ) -> Result<Option<ProjectRouteRecord>, ProjectRegistryError> {
        let mut matching = self
            .list()?
            .into_iter()
            .filter(|record| record.registration.product_id == product_id);
        let Some(first) = matching.next() else {
            return Ok(None);
        };
        if matching.any(|record| record.event_store != first.event_store) {
            return Err(ProjectRegistryError::ConflictingRoute {
                product_id: product_id.to_string(),
            });
        }
        Ok(Some(first))
    }
}

/// File-backed operator registry, normally rooted at `~/.bicameral/projects/`.
pub struct FileProjectRegistrationStore {
    root: PathBuf,
    write_lock: Mutex<()>,
}

/// In-memory registry used by deterministic runtime tests.
#[derive(Default)]
pub struct MemoryProjectRegistrationStore {
    records: Mutex<std::collections::BTreeMap<(String, String), ProjectRouteRecord>>,
}

impl MemoryProjectRegistrationStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ProjectRegistrationStore for MemoryProjectRegistrationStore {
    fn get(
        &self,
        product_id: &str,
        cloud_account_id: &str,
    ) -> Result<Option<ProjectRouteRecord>, ProjectRegistryError> {
        Ok(self
            .records
            .lock()
            .map_err(|error| ProjectRegistryError::Io(error.to_string()))?
            .get(&(product_id.to_string(), cloud_account_id.to_string()))
            .cloned())
    }

    fn list(&self) -> Result<Vec<ProjectRouteRecord>, ProjectRegistryError> {
        Ok(self
            .records
            .lock()
            .map_err(|error| ProjectRegistryError::Io(error.to_string()))?
            .values()
            .cloned()
            .collect())
    }

    fn write(&self, record: &ProjectRouteRecord) -> Result<(), ProjectRegistryError> {
        FileProjectRegistrationStore::validate_product_id(&record.registration.product_id)?;
        let mut records = self
            .records
            .lock()
            .map_err(|error| ProjectRegistryError::Io(error.to_string()))?;
        if records.values().any(|existing| {
            existing.registration.product_id == record.registration.product_id
                && existing.event_store != record.event_store
        }) {
            return Err(ProjectRegistryError::ConflictingRoute {
                product_id: record.registration.product_id.clone(),
            });
        }
        let key = (
            record.registration.product_id.clone(),
            record.registration.cloud_account_id.clone(),
        );
        match records.get(&key) {
            Some(existing)
                if existing.registration.cloud_account_id
                    == record.registration.cloud_account_id
                    && existing.event_store == record.event_store =>
            {
                Ok(())
            }
            Some(_) => Err(ProjectRegistryError::ConflictingRoute {
                product_id: record.registration.product_id.clone(),
            }),
            None => {
                records.insert(key, record.clone());
                Ok(())
            }
        }
    }
}

impl FileProjectRegistrationStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            write_lock: Mutex::new(()),
        }
    }

    fn validate_product_id(product_id: &str) -> Result<(), ProjectRegistryError> {
        if product_id.is_empty()
            || !product_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        {
            return Err(ProjectRegistryError::InvalidProductId);
        }
        Ok(())
    }

    fn record_path(
        &self,
        product_id: &str,
        cloud_account_id: &str,
    ) -> Result<PathBuf, ProjectRegistryError> {
        Self::validate_product_id(product_id)?;
        Self::validate_product_id(cloud_account_id)?;
        Ok(self
            .root
            .join(format!("{product_id}--{cloud_account_id}.json")))
    }

    fn read_path(path: &Path) -> Result<ProjectRouteRecord, ProjectRegistryError> {
        let bytes = std::fs::read(path)
            .map_err(|error| ProjectRegistryError::Io(format!("{}: {error}", path.display())))?;
        let file: RegistryFile = serde_json::from_slice(&bytes)
            .map_err(|error| ProjectRegistryError::Serialization(error.to_string()))?;
        if file.schema_version != REGISTRY_SCHEMA_VERSION {
            return Err(ProjectRegistryError::UnsupportedSchema {
                version: file.schema_version,
            });
        }
        Ok(file.record)
    }

    fn ensure_root(&self) -> Result<(), ProjectRegistryError> {
        std::fs::create_dir_all(&self.root)
            .map_err(|error| ProjectRegistryError::Io(error.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.root, std::fs::Permissions::from_mode(0o700))
                .map_err(|error| ProjectRegistryError::Io(error.to_string()))?;
        }
        Ok(())
    }

    fn existing_route_matches(
        path: &Path,
        record: &ProjectRouteRecord,
    ) -> Result<bool, ProjectRegistryError> {
        let existing = Self::read_path(path)?;
        Ok(
            existing.registration.product_id == record.registration.product_id
                && existing.registration.cloud_account_id == record.registration.cloud_account_id
                && existing.event_store == record.event_store,
        )
    }
}

impl ProjectRegistrationStore for FileProjectRegistrationStore {
    fn get(
        &self,
        product_id: &str,
        cloud_account_id: &str,
    ) -> Result<Option<ProjectRouteRecord>, ProjectRegistryError> {
        let path = self.record_path(product_id, cloud_account_id)?;
        if !path.exists() {
            return Ok(None);
        }
        Self::read_path(&path).map(Some)
    }

    fn list(&self) -> Result<Vec<ProjectRouteRecord>, ProjectRegistryError> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let mut paths = std::fs::read_dir(&self.root)
            .map_err(|error| ProjectRegistryError::Io(error.to_string()))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| ProjectRegistryError::Io(error.to_string()))?;
        paths.retain(|path| path.extension().and_then(|value| value.to_str()) == Some("json"));
        paths.sort();
        paths.iter().map(|path| Self::read_path(path)).collect()
    }

    fn write(&self, record: &ProjectRouteRecord) -> Result<(), ProjectRegistryError> {
        let _guard = self
            .write_lock
            .lock()
            .map_err(|error| ProjectRegistryError::Io(error.to_string()))?;
        let product_id = &record.registration.product_id;
        let path = self.record_path(product_id, &record.registration.cloud_account_id)?;
        self.ensure_root()?;

        if self
            .find_product(product_id)?
            .is_some_and(|existing| existing.event_store != record.event_store)
        {
            return Err(ProjectRegistryError::ConflictingRoute {
                product_id: product_id.clone(),
            });
        }

        if path.exists() {
            return if Self::existing_route_matches(&path, record)? {
                Ok(())
            } else {
                Err(ProjectRegistryError::ConflictingRoute {
                    product_id: product_id.clone(),
                })
            };
        }

        let encoded = serde_json::to_vec_pretty(&RegistryFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            record: record.clone(),
        })
        .map_err(|error| ProjectRegistryError::Serialization(error.to_string()))?;
        let temp_path = self.root.join(format!(
            ".{product_id}.{}.tmp",
            uuid::Uuid::new_v4().simple()
        ));

        #[cfg(unix)]
        let mut temp = {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temp_path)
        };
        #[cfg(not(unix))]
        let mut temp = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path);

        use std::io::Write;
        let result = (|| {
            let temp = temp
                .as_mut()
                .map_err(|error| ProjectRegistryError::Io(error.to_string()))?;
            temp.write_all(&encoded)
                .and_then(|_| temp.sync_all())
                .map_err(|error| ProjectRegistryError::Io(error.to_string()))?;
            match std::fs::hard_link(&temp_path, &path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if Self::existing_route_matches(&path, record)? {
                        Ok(())
                    } else {
                        Err(ProjectRegistryError::ConflictingRoute {
                            product_id: product_id.clone(),
                        })
                    }
                }
                Err(error) => Err(ProjectRegistryError::Io(error.to_string())),
            }
        })();
        let _ = std::fs::remove_file(&temp_path);
        result
    }
}

/// Resolves canonical Product ids through the operator registry.
pub struct ProjectEventStoreRouter {
    registrations: Arc<dyn ProjectRegistrationStore>,
}

impl ProjectEventStoreRouter {
    pub fn new(registrations: Arc<dyn ProjectRegistrationStore>) -> Self {
        Self { registrations }
    }

    pub fn adapter_for(
        &self,
        product_id: &str,
    ) -> Result<Arc<dyn EventStoreAdapter>, ProjectRegistryError> {
        let record = self
            .registrations
            .find_product(product_id)?
            .ok_or_else(|| ProjectRegistryError::NotRegistered {
                product_id: product_id.to_string(),
            })?;
        Ok(record.event_store.open())
    }

    /// Resolve the Product's atomic event-store adapter for selection commit.
    pub fn atomic_adapter_for(
        &self,
        product_id: &str,
    ) -> Result<Arc<dyn crate::atomic_event_store::AtomicEventStoreAdapter>, ProjectRegistryError>
    {
        let record = self
            .registrations
            .find_product(product_id)?
            .ok_or_else(|| ProjectRegistryError::NotRegistered {
                product_id: product_id.to_string(),
            })?;
        Ok(record.event_store.open_atomic())
    }

    pub fn store(&self) -> &dyn ProjectRegistrationStore {
        self.registrations.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_api::product_registration::{
        ConnectionMetadata, RegisteredProvider, RegisteredSubstrate,
    };

    fn record(product_id: &str, account: &str, events_dir: PathBuf) -> ProjectRouteRecord {
        ProjectRouteRecord {
            registration: ProjectRegistration {
                product_id: product_id.into(),
                cloud_account_id: account.into(),
                registration_ref: format!("reg:{product_id}:{account}"),
                connection: ConnectionMetadata {
                    provider: RegisteredProvider::Github,
                    substrate: RegisteredSubstrate::Git,
                    account_label: None,
                    descriptor_ref: format!("local:{product_id}"),
                },
                governance_policy_version: "gpv_1".into(),
                registered_at: chrono::Utc::now(),
            },
            event_store: EventStoreRoute::Git {
                events_dir,
                ci_merge_gate_configured: false,
            },
            first_event_commit_proof: None,
        }
    }

    #[test]
    fn registry_is_account_scoped_for_one_product() {
        let store = MemoryProjectRegistrationStore::new();
        let route = PathBuf::from("/private/product/events");
        store
            .write(&record("prd_shared", "acct_a", route.clone()))
            .unwrap();
        store.write(&record("prd_shared", "acct_b", route)).unwrap();

        assert!(store.get("prd_shared", "acct_a").unwrap().is_some());
        assert!(store.get("prd_shared", "acct_b").unwrap().is_some());
        assert_eq!(store.list().unwrap().len(), 2);
    }

    #[test]
    fn conflicting_route_for_same_tuple_fails_closed() {
        let store = MemoryProjectRegistrationStore::new();
        store
            .write(&record(
                "prd_conflict",
                "acct_a",
                PathBuf::from("/private/a/events"),
            ))
            .unwrap();
        let error = store
            .write(&record(
                "prd_conflict",
                "acct_a",
                PathBuf::from("/private/b/events"),
            ))
            .unwrap_err();

        assert!(matches!(
            error,
            ProjectRegistryError::ConflictingRoute { .. }
        ));
    }

    #[test]
    fn conflicting_route_across_accounts_for_one_product_fails_closed() {
        let store = MemoryProjectRegistrationStore::new();
        store
            .write(&record(
                "prd_conflict",
                "acct_a",
                PathBuf::from("/private/a/events"),
            ))
            .unwrap();
        let error = store
            .write(&record(
                "prd_conflict",
                "acct_b",
                PathBuf::from("/private/b/events"),
            ))
            .unwrap_err();

        assert!(matches!(
            error,
            ProjectRegistryError::ConflictingRoute { .. }
        ));
    }

    #[test]
    fn traversal_identifiers_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let store = FileProjectRegistrationStore::new(root.path());
        let error = store
            .write(&record(
                "../escape",
                "acct_a",
                PathBuf::from("/private/product/events"),
            ))
            .unwrap_err();
        assert!(matches!(error, ProjectRegistryError::InvalidProductId));
    }

    #[cfg(unix)]
    #[test]
    fn file_registry_uses_private_permissions_and_leaves_no_temp_file() {
        use std::os::unix::fs::PermissionsExt;

        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("projects");
        let store = FileProjectRegistrationStore::new(&root);
        store
            .write(&record(
                "prd_private",
                "acct_a",
                PathBuf::from("/private/product/events"),
            ))
            .unwrap();

        assert_eq!(
            std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let files = std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(files.len(), 1);
        assert_eq!(
            std::fs::metadata(&files[0]).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(files[0].file_name().unwrap(), "prd_private--acct_a.json");
    }

    #[test]
    fn configured_relative_route_is_anchored_to_workspace_not_process_cwd() {
        let workspace = tempfile::tempdir().unwrap();
        let paths = BicameralPaths::from_workspace_root(workspace.path());
        let config = EventStoreConfig {
            substrate: SubstrateKind::Git,
            git_path: Some(PathBuf::from("custom/events")),
            drive_folder_path: None,
            ci_merge_gate: false,
        };

        assert_eq!(
            EventStoreRoute::from_config(&config, &paths),
            Some(EventStoreRoute::Git {
                events_dir: workspace.path().join("custom/events"),
                ci_merge_gate_configured: false,
            })
        );
    }
}
