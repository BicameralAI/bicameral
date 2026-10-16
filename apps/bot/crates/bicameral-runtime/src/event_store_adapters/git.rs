//! Git-backed event-store adapter.
//!
//! Stores accepted governance events as individual YAML files under an
//! `events/` directory, matching the ADR-0001 event-first layout:
//!
//! ```text
//! <root>/events/<timestamp>-<event_type>.yaml
//! ```
//!
//! Each event file is a PR-reviewable text artifact. Replay reads and
//! timestamp-sorts all `.yaml` files in the events directory.
//!
//! **Backward compatibility:** if no `events/` directory exists but a legacy
//! `events.jsonl` file is present, replay reads from the JSONL file instead.
//! New appends always write to the event-first layout.

use bicameral_api::event_store::{
    EventStoreAdapter, EventStoreEntry, EventStoreError, SubstrateCapability, SubstrateHealth,
    SubstrateKind,
};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::atomic_event_store::{AtomicEventBatchV1, AtomicEventStoreAdapter};

/// Configuration for the Git-backed event store adapter.
///
/// Controls which optional capabilities the adapter advertises beyond the
/// baseline append/replay/health contract and the always-present Git
/// capabilities (branch-local proposals, PR-native review, merge conflict
/// resolution).
#[derive(Debug, Clone, Default)]
pub struct GitAdapterConfig {
    /// Whether CI or merge-gate enforcement is available on this repository.
    ///
    /// Only set to `true` when the repository actually has a merge boundary
    /// (e.g. GitHub branch protection with required checks). Per ADR-0001 §6,
    /// an adapter must not claim CI/merge blocking unless the substrate
    /// provides it.
    pub ci_merge_gate_configured: bool,
}

/// Git-backed event store adapter.
///
/// Events are stored as individual YAML files under `<root>/events/`,
/// producing PR-reviewable artifacts per ADR-0001. The caller is responsible
/// for `git add`/`commit`/`push` to share with other team members.
pub struct GitAdapter {
    root: PathBuf,
    config: GitAdapterConfig,
}

impl GitAdapter {
    /// Create an adapter with default config (no CI/merge-gate enforcement).
    pub fn new(repo_path: &Path) -> Self {
        Self {
            root: repo_path.to_path_buf(),
            config: GitAdapterConfig::default(),
        }
    }

    /// Create an adapter with explicit capability configuration.
    pub fn with_config(repo_path: &Path, config: GitAdapterConfig) -> Self {
        Self {
            root: repo_path.to_path_buf(),
            config,
        }
    }

    fn legacy_jsonl_path(&self) -> PathBuf {
        self.root.join("events.jsonl")
    }

    /// Build a deterministic filename for an event entry.
    ///
    /// Format: `<rfc3339-timestamp>-<event_type>-<uuid-prefix>.yaml`
    /// The UUID prefix disambiguates events with identical timestamps and types.
    fn event_filename(entry: &EventStoreEntry) -> String {
        let ts = entry.timestamp.format("%Y%m%dT%H%M%S%.3fZ");
        let etype = serde_json::to_value(entry.event_type)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| "unknown".to_string());
        let id_prefix = &entry.id.to_string()[..8];
        format!("{ts}-{etype}-{id_prefix}.yaml")
    }

    fn batch_filename(batch: &AtomicEventBatchV1) -> String {
        format!("batch-{}.yaml", batch.batch_id)
    }

    /// Read events from the event-first YAML directory layout.
    fn replay_from_events_dir(&self) -> Result<Vec<EventStoreEntry>, EventStoreError> {
        let dir = &self.root;
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        let read_dir = std::fs::read_dir(dir).map_err(|e| EventStoreError::Io {
            reason: e.to_string(),
        })?;

        for dir_entry in read_dir {
            let dir_entry = dir_entry.map_err(|e| EventStoreError::Io {
                reason: e.to_string(),
            })?;
            let path = dir_entry.path();

            if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
                continue;
            }

            let content = std::fs::read_to_string(&path).map_err(|e| EventStoreError::Io {
                reason: e.to_string(),
            })?;

            if let Ok(batch) = serde_yaml::from_str::<AtomicEventBatchV1>(&content) {
                batch.validate()?;
                entries.extend(batch.entries);
                continue;
            }

            let entry: EventStoreEntry =
                serde_yaml::from_str(&content).map_err(|e| EventStoreError::Serialization {
                    reason: format!("failed to deserialize {}: {e}", path.display()),
                })?;
            entries.push(entry);
        }

        entries.sort_by_key(|e| e.timestamp);
        Ok(entries)
    }

    /// Read events from the legacy single-file JSONL format.
    fn replay_from_legacy_jsonl(&self) -> Result<Vec<EventStoreEntry>, EventStoreError> {
        let path = self.legacy_jsonl_path();
        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = std::fs::read_to_string(&path).map_err(|e| EventStoreError::Io {
            reason: e.to_string(),
        })?;

        let mut entries = Vec::new();
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let entry: EventStoreEntry =
                serde_json::from_str(line).map_err(|e| EventStoreError::Serialization {
                    reason: e.to_string(),
                })?;
            entries.push(entry);
        }

        Ok(entries)
    }

    /// Count YAML event files in the events directory.
    fn yaml_event_count(&self) -> usize {
        let dir = &self.root;
        if !dir.exists() {
            return 0;
        }
        std::fs::read_dir(dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().and_then(|ext| ext.to_str()) == Some("yaml"))
                    .count()
            })
            .unwrap_or(0)
    }
}

impl EventStoreAdapter for GitAdapter {
    fn append(&self, entry: EventStoreEntry) -> Result<(), EventStoreError> {
        std::fs::create_dir_all(&self.root).map_err(|e| EventStoreError::Io {
            reason: e.to_string(),
        })?;

        let filename = Self::event_filename(&entry);
        let path = self.root.join(filename);

        // YAML event-first layout (ADR-0001). This is not
        // serialize_event_store_entry JSON. Git is not a v2 proof substrate
        // until append writes those exact bytes (bot#1193 / factory #901 row 1).
        let content =
            serde_yaml::to_string(&entry).map_err(|e| EventStoreError::Serialization {
                reason: e.to_string(),
            })?;

        std::fs::write(path, content).map_err(|e| EventStoreError::Io {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    fn replay(&self) -> Result<Vec<EventStoreEntry>, EventStoreError> {
        // Prefer event-first YAML layout if any .yaml files exist;
        // fall back to legacy JSONL.
        if self.root.exists() && self.yaml_event_count() > 0 {
            return self.replay_from_events_dir();
        }

        self.replay_from_legacy_jsonl()
    }

    fn substrate_kind(&self) -> SubstrateKind {
        SubstrateKind::Git
    }

    fn health_check(&self) -> Result<SubstrateHealth, EventStoreError> {
        let yaml_count = self.yaml_event_count();
        let entry_count = if yaml_count > 0 {
            yaml_count
        } else if self.legacy_jsonl_path().exists() {
            std::fs::read_to_string(self.legacy_jsonl_path())
                .map(|c| c.lines().filter(|l| !l.trim().is_empty()).count())
                .unwrap_or(0)
        } else {
            0
        };

        let mut warnings = Vec::new();
        if yaml_count == 0 && self.legacy_jsonl_path().exists() {
            warnings.push(
                "Legacy JSONL layout detected — consider migrating to event-first YAML".to_string(),
            );
        }

        Ok(SubstrateHealth {
            kind: SubstrateKind::Git,
            reachable: true,
            last_synced: None,
            entry_count,
            freshness: None,
            warnings,
            capabilities: self.capabilities(),
        })
    }

    fn capabilities(&self) -> BTreeSet<SubstrateCapability> {
        let mut caps = BTreeSet::from([
            SubstrateCapability::DurableReplay,
            SubstrateCapability::BranchLocalProposals,
            SubstrateCapability::PrNativeReview,
            SubstrateCapability::NativeMergeConflictResolution,
        ]);
        if self.config.ci_merge_gate_configured {
            caps.insert(SubstrateCapability::CiMergeGateEnforcement);
        }
        caps
    }
}

impl AtomicEventStoreAdapter for GitAdapter {
    fn append_atomic_batch(&self, batch: AtomicEventBatchV1) -> Result<(), EventStoreError> {
        batch.validate()?;
        std::fs::create_dir_all(&self.root).map_err(|error| EventStoreError::Io {
            reason: error.to_string(),
        })?;
        let filename = Self::batch_filename(&batch);
        let final_path = self.root.join(&filename);
        if final_path.exists() {
            return Err(EventStoreError::Conflict {
                reason: format!("atomic event batch {} already exists", batch.batch_id),
            });
        }
        let content =
            serde_yaml::to_string(&batch).map_err(|error| EventStoreError::Serialization {
                reason: error.to_string(),
            })?;
        let temporary_path = self.root.join(format!(".tmp-{filename}"));
        std::fs::write(&temporary_path, content).map_err(|error| EventStoreError::Io {
            reason: error.to_string(),
        })?;
        std::fs::rename(&temporary_path, &final_path).map_err(|error| EventStoreError::Io {
            reason: format!("atomic batch rename failed: {error}"),
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod v2_proof_bytes {
    use super::*;
    use bicameral_api::event_store::{serialize_event_store_entry, EventStoreEntry, EventType};
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    #[test]
    fn append_writes_yaml_not_serialize_event_store_entry_json() {
        let dir = std::env::temp_dir().join(format!(
            "bicameral-git-v2-proof-{}",
            Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let adapter = GitAdapter::new(&dir);
        let entry = EventStoreEntry {
            id: Uuid::parse_str("0198d2b4-8a5c-7f22-9e31-9bb014cc8200").unwrap(),
            event_type: EventType::CreateProduct,
            payload: serde_json::json!({"product_id": "prd_git"}),
            timestamp: Utc.with_ymd_and_hms(2026, 7, 12, 8, 59, 0).unwrap(),
            actor: "actor-1".into(),
            governance_result_id: None,
        };
        adapter.append(entry.clone()).expect("append");
        let written = std::fs::read(dir.join(GitAdapter::event_filename(&entry))).expect("read");
        let json = serialize_event_store_entry(&entry).expect("json");
        assert_ne!(
            written.as_slice(),
            json.as_slice(),
            "Git YAML append must not be treated as v2 event_digest bytes"
        );
        let text = String::from_utf8(written).expect("utf8");
        assert!(
            !text.trim_start().starts_with('{'),
            "Git append is YAML, not JSON"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
