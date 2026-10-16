//! Audit receipt persistence.

use crate::receipt::{AuditAction, AuditReceipt};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// File-backed audit receipt store.
///
/// Receipts are stored as individual JSON files in the audit directory,
/// forming a hash-chained sequence.
pub struct AuditStore {
    audit_dir: PathBuf,
    last_hash: Option<String>,
}

impl AuditStore {
    /// Open or create an audit store at the given directory.
    pub fn open(audit_dir: &Path) -> anyhow::Result<Self> {
        std::fs::create_dir_all(audit_dir)?;

        let last_hash = Self::find_last_hash(audit_dir)?;

        Ok(Self {
            audit_dir: audit_dir.to_path_buf(),
            last_hash,
        })
    }

    /// Record a new audit receipt.
    pub fn record(
        &mut self,
        action: AuditAction,
        actor: String,
        target_id: Option<Uuid>,
        summary: String,
    ) -> anyhow::Result<AuditReceipt> {
        let receipt = AuditReceipt::new(action, actor, target_id, summary, self.last_hash.clone());

        let filename = format!("{}.json", receipt.id);
        let path = self.audit_dir.join(&filename);
        let content = serde_json::to_string_pretty(&receipt)?;
        std::fs::write(&path, content)?;

        self.last_hash = Some(receipt.content_hash.clone());

        tracing::debug!(receipt_id = %receipt.id, action = ?receipt.action, "Audit receipt recorded");

        Ok(receipt)
    }

    /// List all receipts in chronological order.
    pub fn list(&self) -> anyhow::Result<Vec<AuditReceipt>> {
        let mut receipts = Vec::new();

        if !self.audit_dir.exists() {
            return Ok(receipts);
        }

        for entry in std::fs::read_dir(&self.audit_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                let content = std::fs::read_to_string(&path)?;
                if let Ok(receipt) = serde_json::from_str::<AuditReceipt>(&content) {
                    receipts.push(receipt);
                }
            }
        }

        receipts.sort_by_key(|a| a.timestamp);
        Ok(receipts)
    }

    fn find_last_hash(audit_dir: &Path) -> anyhow::Result<Option<String>> {
        let mut latest: Option<AuditReceipt> = None;

        if !audit_dir.exists() {
            return Ok(None);
        }

        for entry in std::fs::read_dir(audit_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                let content = std::fs::read_to_string(&path)?;
                if let Ok(receipt) = serde_json::from_str::<AuditReceipt>(&content) {
                    match &latest {
                        None => latest = Some(receipt),
                        Some(l) if receipt.timestamp > l.timestamp => latest = Some(receipt),
                        _ => {}
                    }
                }
            }
        }

        Ok(latest.map(|r| r.content_hash))
    }
}
