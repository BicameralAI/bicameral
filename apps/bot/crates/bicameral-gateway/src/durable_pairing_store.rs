//! Alpha durable pairing relationship store (operator-local file).
//!
//! Survives daemon process restart. Not Cloud multi-instance durability.
//! Bound metadata: cloud_account_id + origin + protocol/profile + daemon_instance_id.

use bicameral_api::hosted_bridge::HOSTED_BRIDGE_PROTOCOL_VERSION;
use bicameral_api::pairing_authorization::PAIRING_AUTHORIZATION_PROFILE;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Filename under the durable pairing directory.
pub const DURABLE_PAIRINGS_FILE: &str = "pairings.json";
/// Filename for Alpha TOFU daemon instance identity.
pub const DAEMON_INSTANCE_FILE: &str = "daemon-instance-id";

/// Bound relationship metadata for a durable opaque `pairing_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurablePairingRecord {
    pub pairing_id: String,
    pub cloud_account_id: String,
    pub approved_origin: String,
    pub protocol_version: u32,
    pub profile: String,
    pub daemon_instance_id: String,
    pub team_id: Option<String>,
    pub created_at: String,
    pub revoked: bool,
    /// Browser-held opaque handle that can locate this relationship on restore.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restore_request_handle: Option<String>,
    /// SHA-256 of the browser-held restore proof. The bearer proof is never persisted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restore_poll_proof_sha256: Option<String>,
    /// Lifetime applied to each ephemeral session minted from this relationship.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_ttl_seconds: Option<u64>,
    /// Last enrollment attempt outcome (never silent success).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enrollment_status: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct DurablePairingFile {
    pairings: HashMap<String, DurablePairingRecord>,
}

/// File-backed durable pairing map. Alpha: single-process operator-local.
pub struct DurablePairingStore {
    path: PathBuf,
    inner: Mutex<DurablePairingFile>,
}

impl DurablePairingStore {
    /// Open or create the store at `path` (JSON file).
    pub fn open(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let inner = if path.exists() {
            let raw = fs::read_to_string(&path)?;
            if raw.trim().is_empty() {
                DurablePairingFile::default()
            } else {
                serde_json::from_str(&raw).map_err(|e| {
                    io::Error::new(io::ErrorKind::InvalidData, format!("pairings.json: {e}"))
                })?
            }
        } else {
            DurablePairingFile::default()
        };
        Ok(Self {
            path,
            inner: Mutex::new(inner),
        })
    }

    /// In-memory store for tests (still requires a path for persist calls).
    pub fn in_memory() -> Self {
        Self {
            path: PathBuf::from(":memory:"),
            inner: Mutex::new(DurablePairingFile::default()),
        }
    }

    pub fn get(&self, pairing_id: &str) -> Option<DurablePairingRecord> {
        self.inner.lock().ok()?.pairings.get(pairing_id).cloned()
    }

    pub fn find_by_restore_request_handle(
        &self,
        request_handle: &str,
    ) -> Option<DurablePairingRecord> {
        self.inner
            .lock()
            .ok()?
            .pairings
            .values()
            .find(|record| record.restore_request_handle.as_deref() == Some(request_handle))
            .cloned()
    }

    pub fn upsert(&self, record: DurablePairingRecord) -> io::Result<()> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| io::Error::other("pairings lock poisoned"))?;
        let pairing_id = record.pairing_id.clone();
        let previous = guard.pairings.insert(pairing_id.clone(), record);
        if let Err(err) = self.persist_locked(&guard) {
            match previous {
                Some(previous) => {
                    guard.pairings.insert(pairing_id, previous);
                }
                None => {
                    guard.pairings.remove(&pairing_id);
                }
            }
            return Err(err);
        }
        Ok(())
    }

    pub fn set_enrollment_status(&self, pairing_id: &str, status: &str) -> io::Result<()> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| io::Error::other("pairings lock poisoned"))?;
        if let Some(record) = guard.pairings.get_mut(pairing_id) {
            record.enrollment_status = Some(status.to_string());
        }
        self.persist_locked(&guard)
    }

    pub fn revoke(&self, pairing_id: &str) -> io::Result<bool> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| io::Error::other("pairings lock poisoned"))?;
        let Some(record) = guard.pairings.get_mut(pairing_id) else {
            return Ok(false);
        };
        record.revoked = true;
        self.persist_locked(&guard)?;
        Ok(true)
    }

    fn persist_locked(&self, file: &DurablePairingFile) -> io::Result<()> {
        if self.path == Path::new(":memory:") {
            return Ok(());
        }
        let raw = serde_json::to_string_pretty(file)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, raw)?;
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

/// Resolve `~/.bicameral/pairings/pairings.json` when operator home exists.
pub fn default_durable_pairings_path() -> Option<PathBuf> {
    bicameral_config::BicameralPaths::operator_home()
        .map(|h| h.join("pairings").join(DURABLE_PAIRINGS_FILE))
}

/// Load or mint the Alpha TOFU daemon instance identity.
pub fn load_or_mint_daemon_instance_id(path: impl AsRef<Path>) -> io::Result<String> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if path.exists() {
        let raw = fs::read_to_string(path)?;
        let id = raw.trim().to_string();
        if !id.is_empty() {
            return Ok(id);
        }
    }
    let id = format!("daemon-{}", uuid::Uuid::new_v4());
    fs::write(path, &id)?;
    Ok(id)
}

/// Default path for daemon instance identity.
pub fn default_daemon_instance_path() -> Option<PathBuf> {
    bicameral_config::BicameralPaths::operator_home().map(|h| h.join(DAEMON_INSTANCE_FILE))
}

/// Inputs captured when local approval commits a durable relationship.
pub struct NewDurablePairingRecord<'a> {
    pub pairing_id: String,
    pub cloud_account_id: String,
    pub approved_origin: String,
    pub daemon_instance_id: String,
    pub team_id: Option<String>,
    pub restore_request_handle: String,
    pub restore_poll_proof: &'a str,
    pub session_ttl_seconds: u64,
    pub now: chrono::DateTime<chrono::Utc>,
}

/// Build a new durable record at LOCAL_APPROVED.
pub fn new_durable_record(input: NewDurablePairingRecord<'_>) -> DurablePairingRecord {
    DurablePairingRecord {
        pairing_id: input.pairing_id,
        cloud_account_id: input.cloud_account_id,
        approved_origin: input.approved_origin,
        protocol_version: HOSTED_BRIDGE_PROTOCOL_VERSION,
        profile: PAIRING_AUTHORIZATION_PROFILE.to_string(),
        daemon_instance_id: input.daemon_instance_id,
        team_id: input.team_id,
        created_at: bicameral_api::format_instant_z(input.now),
        revoked: false,
        restore_request_handle: Some(input.restore_request_handle),
        restore_poll_proof_sha256: Some(format!(
            "{:x}",
            Sha256::digest(input.restore_poll_proof.as_bytes())
        )),
        session_ttl_seconds: Some(input.session_ttl_seconds),
        enrollment_status: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn persists_across_open() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("pairings.json");
        let store = DurablePairingStore::open(&path).unwrap();
        let record = new_durable_record(NewDurablePairingRecord {
            pairing_id: "pairing-abc".into(),
            cloud_account_id: "acct-1".into(),
            approved_origin: "https://app.bicameral-ai.com".into(),
            daemon_instance_id: "daemon-1".into(),
            team_id: Some("team-1".into()),
            restore_request_handle: "restore-handle-1".into(),
            restore_poll_proof: "restore-proof-1",
            session_ttl_seconds: 3600,
            now: chrono::Utc::now(),
        });
        store.upsert(record.clone()).unwrap();
        drop(store);

        let reopened = DurablePairingStore::open(&path).unwrap();
        let loaded = reopened.get("pairing-abc").unwrap();
        assert_eq!(loaded.cloud_account_id, "acct-1");
        assert_eq!(loaded.daemon_instance_id, "daemon-1");
        assert!(!loaded.revoked);
    }

    #[test]
    fn daemon_instance_id_stable() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("daemon-instance-id");
        let first = load_or_mint_daemon_instance_id(&path).unwrap();
        let second = load_or_mint_daemon_instance_id(&path).unwrap();
        assert_eq!(first, second);
        assert!(first.starts_with("daemon-"));
    }
}
