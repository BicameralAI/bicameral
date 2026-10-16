//! Durable daemon-owned capability-readiness store.

use bicameral_api::capability_readiness::{
    DaemonCapabilityReadinessV1, McpReadinessObservation, ReadinessReductionError,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const STORE_FILE: &str = "readiness-v1.json";

#[derive(Debug, thiserror::Error)]
pub enum CapabilityReadinessStoreError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Reduction(#[from] ReadinessReductionError),
    #[error("activation correlation was already consumed by different evidence")]
    ReceiptAlreadyConsumed,
    #[error("capability readiness store lock is unavailable")]
    LockUnavailable,
    #[error("unsupported capability readiness store schema version {0}")]
    UnsupportedSchema(u8),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadinessEnvelope {
    schema_version: u8,
    records: BTreeMap<String, DaemonCapabilityReadinessV1>,
    consumed_correlations: BTreeMap<String, String>,
}

impl Default for ReadinessEnvelope {
    fn default() -> Self {
        Self {
            schema_version: 1,
            records: BTreeMap::new(),
            consumed_correlations: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileCapabilityReadinessStore {
    root: PathBuf,
    state: Arc<Mutex<ReadinessEnvelope>>,
}

impl FileCapabilityReadinessStore {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, CapabilityReadinessStoreError> {
        let root = root.into();
        create_private_dir(&root)?;
        let path = root.join(STORE_FILE);
        let state: ReadinessEnvelope = if path.exists() {
            let bytes = std::fs::read(path)?;
            serde_json::from_slice(&bytes)?
        } else {
            ReadinessEnvelope::default()
        };
        if state.schema_version != 1 {
            return Err(CapabilityReadinessStoreError::UnsupportedSchema(
                state.schema_version,
            ));
        }
        Ok(Self {
            root,
            state: Arc::new(Mutex::new(state)),
        })
    }

    pub fn submit(
        &self,
        observation: McpReadinessObservation,
    ) -> Result<DaemonCapabilityReadinessV1, CapabilityReadinessStoreError> {
        let readiness = observation.reduce()?;
        let mut guard = self
            .state
            .lock()
            .map_err(|_| CapabilityReadinessStoreError::LockUnavailable)?;
        let mut next = guard.clone();

        if let Some(activation) = &observation.activation {
            match next.consumed_correlations.get(&activation.correlation_id) {
                Some(digest) if digest != &activation.receipt_digest => {
                    return Err(CapabilityReadinessStoreError::ReceiptAlreadyConsumed);
                }
                Some(_) => {}
                None => {
                    next.consumed_correlations.insert(
                        activation.correlation_id.clone(),
                        activation.receipt_digest.clone(),
                    );
                }
            }
        }

        next.records.retain(|_, existing| {
            existing.subject_id != readiness.subject_id
                || existing.artifact_generation == readiness.artifact_generation
        });
        next.records
            .insert(readiness_key(&readiness), readiness.clone());
        persist_private_json(&self.root.join(STORE_FILE), &next)?;
        *guard = next;
        Ok(readiness)
    }

    pub fn list(&self) -> Result<Vec<DaemonCapabilityReadinessV1>, CapabilityReadinessStoreError> {
        let guard = self
            .state
            .lock()
            .map_err(|_| CapabilityReadinessStoreError::LockUnavailable)?;
        Ok(guard.records.values().cloned().collect())
    }
}

fn readiness_key(readiness: &DaemonCapabilityReadinessV1) -> String {
    format!(
        "mcp:{}:{}",
        readiness.subject_id.as_str(),
        readiness.artifact_generation
    )
}

fn persist_private_json(
    path: &Path,
    envelope: &ReadinessEnvelope,
) -> Result<(), CapabilityReadinessStoreError> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "readiness path has no parent")
    })?;
    create_private_dir(parent)?;
    let temporary = parent.join(format!(".{STORE_FILE}.tmp-{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(envelope)?;

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(&temporary, &bytes)?;
    }
    std::fs::rename(&temporary, path)?;
    Ok(())
}

fn create_private_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_api::capability_readiness::{
        CapabilityReadinessState, DeveloperHostId, McpReadinessObservation,
        VerifiedActivationEvidence,
    };

    #[test]
    fn active_readiness_survives_store_restart() {
        let root = tempfile::tempdir().unwrap();
        let artifact = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let observation = McpReadinessObservation {
            subject_id: DeveloperHostId::Codex,
            artifact_generation: artifact.into(),
            package_installed: true,
            registration_verified: true,
            managed_hook_verified: true,
            missing_prerequisites: Vec::new(),
            drift_reason_codes: Vec::new(),
            activation: Some(VerifiedActivationEvidence {
                correlation_id: "codex-session-start-1".into(),
                receipt_digest:
                    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
                artifact_generation: artifact.into(),
                verified_at: "2026-07-30T00:00:00Z".into(),
                expires_at: "2026-07-30T01:00:00Z".into(),
            }),
            host_permission: None,
            observed_at: "2026-07-30T00:05:00Z".into(),
        };

        let store = FileCapabilityReadinessStore::open(root.path()).unwrap();
        let submitted = store.submit(observation).unwrap();
        assert_eq!(submitted.state, CapabilityReadinessState::ActiveVerified);
        drop(store);

        let restarted = FileCapabilityReadinessStore::open(root.path()).unwrap();
        assert_eq!(restarted.list().unwrap(), vec![submitted]);
    }

    #[test]
    fn consumed_activation_correlation_cannot_authorize_different_receipt() {
        let root = tempfile::tempdir().unwrap();
        let artifact = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let observation = |receipt_digest: &str| McpReadinessObservation {
            subject_id: DeveloperHostId::Codex,
            artifact_generation: artifact.into(),
            package_installed: true,
            registration_verified: true,
            managed_hook_verified: true,
            missing_prerequisites: Vec::new(),
            drift_reason_codes: Vec::new(),
            activation: Some(VerifiedActivationEvidence {
                correlation_id: "single-use-correlation".into(),
                receipt_digest: receipt_digest.into(),
                artifact_generation: artifact.into(),
                verified_at: "2026-07-30T00:00:00Z".into(),
                expires_at: "2026-07-30T01:00:00Z".into(),
            }),
            host_permission: None,
            observed_at: "2026-07-30T00:05:00Z".into(),
        };
        let store = FileCapabilityReadinessStore::open(root.path()).unwrap();
        store
            .submit(observation(
                "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            ))
            .unwrap();

        let error = store
            .submit(observation(
                "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            ))
            .unwrap_err();

        assert!(matches!(
            error,
            CapabilityReadinessStoreError::ReceiptAlreadyConsumed
        ));
    }

    #[test]
    fn observing_new_artifact_generation_invalidates_prior_active_projection() {
        let root = tempfile::tempdir().unwrap();
        let store = FileCapabilityReadinessStore::open(root.path()).unwrap();
        let active_artifact =
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        store
            .submit(McpReadinessObservation {
                subject_id: DeveloperHostId::Claude,
                artifact_generation: active_artifact.into(),
                package_installed: true,
                registration_verified: true,
                managed_hook_verified: true,
                missing_prerequisites: Vec::new(),
                drift_reason_codes: Vec::new(),
                activation: Some(VerifiedActivationEvidence {
                    correlation_id: "old-generation".into(),
                    receipt_digest:
                        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                            .into(),
                    artifact_generation: active_artifact.into(),
                    verified_at: "2026-07-30T00:00:00Z".into(),
                    expires_at: "2026-07-30T01:00:00Z".into(),
                }),
                host_permission: None,
                observed_at: "2026-07-30T00:05:00Z".into(),
            })
            .unwrap();

        let current =
            store
                .submit(McpReadinessObservation {
                    subject_id: DeveloperHostId::Claude,
                    artifact_generation:
                        "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                            .into(),
                    package_installed: true,
                    registration_verified: true,
                    managed_hook_verified: true,
                    missing_prerequisites: Vec::new(),
                    drift_reason_codes: Vec::new(),
                    activation: None,
                    host_permission: None,
                    observed_at: "2026-07-30T00:10:00Z".into(),
                })
                .unwrap();

        assert_eq!(
            current.state,
            CapabilityReadinessState::InstalledPendingHostAction
        );
        assert_eq!(store.list().unwrap(), vec![current]);
    }
}
