//! Daemon-local content-addressed storage for screened evidence bodies.
//!
//! Evidence bytes are written before the canonical `EvidenceAccepted` event is
//! appended. Writes are idempotent by SHA-256 digest. A failed event append may
//! leave an unreferenced blob; retrying the same ingest reuses the blob and can
//! safely append the missing canonical fact.

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use thiserror::Error;
use uuid::Uuid;

const LOCATOR_SCHEME: &str = "sha256";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEvidenceBody {
    pub digest: String,
    pub locator: String,
    pub byte_length: usize,
}

#[derive(Debug, Error)]
pub enum EvidenceRepositoryError {
    #[error("evidence repository I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid evidence locator: {0}")]
    InvalidLocator(String),
    #[error("stored evidence digest mismatch for {locator}")]
    DigestMismatch { locator: String },
    #[error("evidence repository lock poisoned")]
    LockPoisoned,
}

pub trait EvidenceRepository: Send + Sync {
    fn put(&self, bytes: &[u8]) -> Result<StoredEvidenceBody, EvidenceRepositoryError>;
    fn read(&self, locator: &str) -> Result<Vec<u8>, EvidenceRepositoryError>;
}

fn digest_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn locator_for_digest(digest: &str) -> String {
    format!("{LOCATOR_SCHEME}/{}/{}", &digest[..2], digest)
}

fn digest_from_locator(locator: &str) -> Result<&str, EvidenceRepositoryError> {
    let mut parts = locator.split('/');
    let scheme = parts.next();
    let prefix = parts.next();
    let digest = parts.next();
    if scheme != Some(LOCATOR_SCHEME)
        || prefix.is_none()
        || digest.is_none()
        || parts.next().is_some()
    {
        return Err(EvidenceRepositoryError::InvalidLocator(locator.to_string()));
    }
    let prefix = prefix.unwrap();
    let digest = digest.unwrap();
    if digest.len() != 64
        || prefix.len() != 2
        || !digest.starts_with(prefix)
        || !digest
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(EvidenceRepositoryError::InvalidLocator(locator.to_string()));
    }
    Ok(digest)
}

#[derive(Debug, Clone)]
pub struct FileEvidenceRepository {
    root: PathBuf,
}

impl FileEvidenceRepository {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path_for_locator(&self, locator: &str) -> Result<PathBuf, EvidenceRepositoryError> {
        digest_from_locator(locator)?;
        Ok(self.root.join(locator))
    }

    fn verify_existing(
        &self,
        path: &Path,
        locator: &str,
        expected_digest: &str,
    ) -> Result<usize, EvidenceRepositoryError> {
        let mut bytes = Vec::new();
        fs::File::open(path)?.read_to_end(&mut bytes)?;
        if digest_hex(&bytes) != expected_digest {
            return Err(EvidenceRepositoryError::DigestMismatch {
                locator: locator.to_string(),
            });
        }
        Ok(bytes.len())
    }
}

impl EvidenceRepository for FileEvidenceRepository {
    fn put(&self, bytes: &[u8]) -> Result<StoredEvidenceBody, EvidenceRepositoryError> {
        let digest = digest_hex(bytes);
        let locator = locator_for_digest(&digest);
        let target = self.path_for_locator(&locator)?;
        if target.exists() {
            let byte_length = self.verify_existing(&target, &locator, &digest)?;
            return Ok(StoredEvidenceBody {
                digest: format!("sha256:{digest}"),
                locator,
                byte_length,
            });
        }

        let parent = target
            .parent()
            .ok_or_else(|| EvidenceRepositoryError::InvalidLocator(locator.clone()))?;
        fs::create_dir_all(parent)?;
        let temp = parent.join(format!(".{}.tmp", Uuid::new_v4()));
        let write_result = (|| -> Result<(), std::io::Error> {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            fs::rename(&temp, &target)?;
            if let Ok(dir) = fs::File::open(parent) {
                let _ = dir.sync_all();
            }
            Ok(())
        })();
        if let Err(error) = write_result {
            let _ = fs::remove_file(&temp);
            if target.exists() {
                let byte_length = self.verify_existing(&target, &locator, &digest)?;
                return Ok(StoredEvidenceBody {
                    digest: format!("sha256:{digest}"),
                    locator,
                    byte_length,
                });
            }
            return Err(EvidenceRepositoryError::Io(error));
        }

        Ok(StoredEvidenceBody {
            digest: format!("sha256:{digest}"),
            locator,
            byte_length: bytes.len(),
        })
    }

    fn read(&self, locator: &str) -> Result<Vec<u8>, EvidenceRepositoryError> {
        let expected_digest = digest_from_locator(locator)?;
        let path = self.path_for_locator(locator)?;
        let bytes = fs::read(path)?;
        if digest_hex(&bytes) != expected_digest {
            return Err(EvidenceRepositoryError::DigestMismatch {
                locator: locator.to_string(),
            });
        }
        Ok(bytes)
    }
}

#[derive(Debug, Default)]
pub struct MemoryEvidenceRepository {
    blobs: Mutex<HashMap<String, Vec<u8>>>,
}

impl MemoryEvidenceRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

impl EvidenceRepository for MemoryEvidenceRepository {
    fn put(&self, bytes: &[u8]) -> Result<StoredEvidenceBody, EvidenceRepositoryError> {
        let digest = digest_hex(bytes);
        let locator = locator_for_digest(&digest);
        let mut blobs = self
            .blobs
            .lock()
            .map_err(|_| EvidenceRepositoryError::LockPoisoned)?;
        if let Some(existing) = blobs.get(&locator) {
            if digest_hex(existing) != digest {
                return Err(EvidenceRepositoryError::DigestMismatch {
                    locator: locator.clone(),
                });
            }
        } else {
            blobs.insert(locator.clone(), bytes.to_vec());
        }
        Ok(StoredEvidenceBody {
            digest: format!("sha256:{digest}"),
            locator,
            byte_length: bytes.len(),
        })
    }

    fn read(&self, locator: &str) -> Result<Vec<u8>, EvidenceRepositoryError> {
        let expected_digest = digest_from_locator(locator)?;
        let blobs = self
            .blobs
            .lock()
            .map_err(|_| EvidenceRepositoryError::LockPoisoned)?;
        let bytes = blobs
            .get(locator)
            .cloned()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
        if digest_hex(&bytes) != expected_digest {
            return Err(EvidenceRepositoryError::DigestMismatch {
                locator: locator.to_string(),
            });
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_repository_is_idempotent_and_verifies_reads() {
        let dir = tempfile::tempdir().unwrap();
        let repo = FileEvidenceRepository::new(dir.path());
        let first = repo.put(b"screened evidence").unwrap();
        let second = repo.put(b"screened evidence").unwrap();
        assert_eq!(first, second);
        assert_eq!(repo.read(&first.locator).unwrap(), b"screened evidence");
        assert!(!first
            .locator
            .contains(dir.path().to_string_lossy().as_ref()));
    }

    #[test]
    fn file_repository_rejects_tampered_blob() {
        let dir = tempfile::tempdir().unwrap();
        let repo = FileEvidenceRepository::new(dir.path());
        let stored = repo.put(b"original").unwrap();
        let path = dir.path().join(&stored.locator);
        fs::write(path, b"tampered").unwrap();
        assert!(matches!(
            repo.read(&stored.locator),
            Err(EvidenceRepositoryError::DigestMismatch { .. })
        ));
    }

    #[test]
    fn invalid_locator_cannot_escape_repository_root() {
        let dir = tempfile::tempdir().unwrap();
        let repo = FileEvidenceRepository::new(dir.path());
        assert!(matches!(
            repo.read("sha256/../secret"),
            Err(EvidenceRepositoryError::InvalidLocator(_))
        ));
    }
}
