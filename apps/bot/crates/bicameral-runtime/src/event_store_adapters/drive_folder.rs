//! Drive-folder-backed event-store adapter.
//!
//! Compatible with Google Drive sync: events are stored as individual JSON
//! files in a configured local folder. The folder can be synced via Drive
//! Desktop or similar tools. Explicit freshness/offline/stale state tracking
//! is included so the runtime knows when data may be stale.
//!
//! # Substrate path distinction
//!
//! This adapter implements the **local synced-folder** path: it reads and
//! writes files on the local filesystem and relies on an external sync agent
//! (Drive Desktop, rclone, etc.) to propagate changes to Google Drive. It
//! does not call the Google Drive REST API directly.
//!
//! A future `DriveApiAdapter` may call the Drive API for real-time freshness,
//! conflict detection via Drive revision IDs, and server-side ordering. That
//! adapter would implement the same `EventStoreAdapter` trait and declare the
//! same baseline capabilities, but could add richer freshness metadata.
//!
//! # Capability boundary
//!
//! Drive/basic-store substrates satisfy the baseline Decision Ledger contract:
//! durable append, deterministic replay, audit, health/freshness reporting,
//! and `DurableReplay`. They do **not** advertise:
//!
//! - `PrNativeReview` — no PR review mechanism.
//! - `NativeMergeConflictResolution` — no branch/merge model.
//! - `CiMergeGateEnforcement` — no CI pipeline.
//! - `BranchLocalProposals` — no branch isolation.
//!
//! Advisory enforcement should surface as dashboard warnings, queued actions,
//! or paused review states instead of pretending to be CI.

use bicameral_api::event_store::{
    EventStoreAdapter, EventStoreEntry, EventStoreError, FreshnessState, SubstrateCapability,
    SubstrateHealth, SubstrateKind,
};
use chrono::{DateTime, Utc};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::atomic_event_store::{AtomicEventBatchV1, AtomicEventStoreAdapter};

/// Default staleness threshold in seconds (15 minutes).
const DEFAULT_STALE_THRESHOLD_SECS: i64 = 900;

/// Sync marker filename written by `mark_synced` and read for freshness.
const SYNC_MARKER_FILENAME: &str = ".bicameral-sync-marker";

/// Drive-folder-backed event store adapter.
///
/// Each event is stored as a separate JSON file named by UUID. This avoids
/// JSONL merge conflicts when multiple team members sync through Drive.
///
/// This is the **local synced-folder** variant. It depends on an external sync
/// agent to propagate changes to/from Google Drive. See module docs for the
/// distinction between this and a future `DriveApiAdapter`.
pub struct DriveFolderAdapter {
    folder_path: PathBuf,
    stale_threshold_secs: i64,
}

impl DriveFolderAdapter {
    pub fn new(folder_path: &Path) -> Self {
        Self {
            folder_path: folder_path.to_path_buf(),
            stale_threshold_secs: DEFAULT_STALE_THRESHOLD_SECS,
        }
    }

    /// Create an adapter with a custom staleness threshold (for testing).
    pub fn with_stale_threshold(folder_path: &Path, stale_threshold_secs: i64) -> Self {
        Self {
            folder_path: folder_path.to_path_buf(),
            stale_threshold_secs,
        }
    }

    /// Write a sync marker to indicate this folder is actively synced.
    ///
    /// The marker contains an RFC 3339 timestamp. Freshness is derived by
    /// comparing this timestamp against the current time.
    pub fn mark_synced(&self) -> Result<(), EventStoreError> {
        let marker = self.folder_path.join(SYNC_MARKER_FILENAME);
        std::fs::create_dir_all(&self.folder_path).map_err(|e| EventStoreError::Io {
            reason: e.to_string(),
        })?;
        let content = Utc::now().to_rfc3339();
        std::fs::write(marker, content).map_err(|e| EventStoreError::Io {
            reason: e.to_string(),
        })?;
        Ok(())
    }

    /// Read the sync marker timestamp, if present and parseable.
    pub fn last_synced(&self) -> Option<DateTime<Utc>> {
        let marker = self.folder_path.join(SYNC_MARKER_FILENAME);
        let content = std::fs::read_to_string(marker).ok()?;
        content.trim().parse::<DateTime<Utc>>().ok()
    }

    /// Assess the current freshness state of the local folder.
    pub fn freshness(&self) -> FreshnessState {
        if !self.folder_path.exists() {
            return FreshnessState::Offline;
        }
        match self.last_synced() {
            Some(ts) => {
                let age = Utc::now().signed_duration_since(ts);
                if age.num_seconds() <= self.stale_threshold_secs {
                    FreshnessState::Fresh
                } else {
                    FreshnessState::Stale
                }
            }
            None => FreshnessState::Unknown,
        }
    }

    /// Check for write conflicts by detecting if the target file already
    /// exists. In a synced-folder environment, an existing file with the same
    /// UUID means a concurrent writer produced a duplicate event — this is
    /// surfaced as a warning rather than an error because Drive sync may
    /// legitimately deliver events from other team members.
    fn check_write_conflict(&self, entry_id: &uuid::Uuid) -> Option<String> {
        let path = self.folder_path.join(format!("{}.json", entry_id));
        if path.exists() {
            Some(format!(
                "Event file {}.json already exists — concurrent write or duplicate",
                entry_id
            ))
        } else {
            None
        }
    }
}

impl EventStoreAdapter for DriveFolderAdapter {
    fn append(&self, entry: EventStoreEntry) -> Result<(), EventStoreError> {
        std::fs::create_dir_all(&self.folder_path).map_err(|e| EventStoreError::Io {
            reason: e.to_string(),
        })?;

        // Detect potential write conflict (existing file with same UUID).
        // In the synced-folder model this is informational — we overwrite
        // because the last-writer-wins semantic is documented and tested.
        if let Some(warning) = self.check_write_conflict(&entry.id) {
            tracing::warn!(event_id = %entry.id, "{}", warning);
        }

        let filename = format!("{}.json", entry.id);
        let path = self.folder_path.join(&filename);
        let content =
            serde_json::to_string_pretty(&entry).map_err(|e| EventStoreError::Serialization {
                reason: e.to_string(),
            })?;

        // Atomic-ish write: write to a temp file then rename to reduce the
        // window where a partial write could be synced.
        let tmp_path = self.folder_path.join(format!(".tmp-{}", filename));
        std::fs::write(&tmp_path, &content).map_err(|e| EventStoreError::Io {
            reason: e.to_string(),
        })?;
        std::fs::rename(&tmp_path, &path).map_err(|e| EventStoreError::Io {
            reason: format!("rename to final path failed: {}", e),
        })?;

        Ok(())
    }

    fn replay(&self) -> Result<Vec<EventStoreEntry>, EventStoreError> {
        if !self.folder_path.exists() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        let mut skipped = 0usize;

        let read_dir = std::fs::read_dir(&self.folder_path).map_err(|e| EventStoreError::Io {
            reason: e.to_string(),
        })?;

        for dir_entry in read_dir {
            let dir_entry = dir_entry.map_err(|e| EventStoreError::Io {
                reason: e.to_string(),
            })?;
            let path = dir_entry.path();

            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            // Skip temp files from interrupted writes.
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with(".tmp-") {
                    skipped += 1;
                    continue;
                }
            }

            let content = std::fs::read_to_string(&path).map_err(|e| EventStoreError::Io {
                reason: e.to_string(),
            })?;

            if let Ok(batch) = serde_json::from_str::<AtomicEventBatchV1>(&content) {
                batch.validate()?;
                entries.extend(batch.entries);
                continue;
            }

            match serde_json::from_str::<EventStoreEntry>(&content) {
                Ok(entry) => entries.push(entry),
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "Skipping non-event JSON file during replay"
                    );
                    skipped += 1;
                }
            }
        }

        if skipped > 0 {
            tracing::info!(skipped, "Skipped non-event files during replay");
        }

        // Deterministic replay: sort by timestamp, then by UUID for
        // tie-breaking (ensures identical ordering across machines).
        entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp).then_with(|| a.id.cmp(&b.id)));
        Ok(entries)
    }

    fn substrate_kind(&self) -> SubstrateKind {
        SubstrateKind::DriveFolder
    }

    fn health_check(&self) -> Result<SubstrateHealth, EventStoreError> {
        let reachable = self.folder_path.exists();
        let freshness = self.freshness();
        let last_synced = self.last_synced();

        let entry_count = if reachable {
            std::fs::read_dir(&self.folder_path)
                .map(|rd| {
                    rd.filter_map(|e| e.ok())
                        .filter(|e| {
                            let path = e.path();
                            path.extension().and_then(|ext| ext.to_str()) == Some("json")
                                && !path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .is_some_and(|n| n.starts_with(".tmp-"))
                        })
                        .count()
                })
                .unwrap_or(0)
        } else {
            0
        };

        let mut warnings = Vec::new();
        match freshness {
            FreshnessState::Stale => {
                let age_desc = last_synced
                    .map(|ts| {
                        let age = Utc::now().signed_duration_since(ts);
                        format!("{} seconds ago", age.num_seconds())
                    })
                    .unwrap_or_else(|| "unknown".to_string());
                warnings.push(format!(
                    "Drive sync marker is stale (last synced {}). Data may not reflect remote state.",
                    age_desc
                ));
            }
            FreshnessState::Unknown => {
                warnings.push(
                    "No sync marker found — freshness cannot be determined. \
                     Call mark_synced() after a successful sync."
                        .to_string(),
                );
            }
            FreshnessState::Offline => {
                warnings.push("Drive folder is not reachable or does not exist.".to_string());
            }
            FreshnessState::Fresh => {}
        }

        Ok(SubstrateHealth {
            kind: SubstrateKind::DriveFolder,
            reachable,
            last_synced,
            entry_count,
            freshness: Some(freshness),
            warnings,
            capabilities: self.capabilities(),
        })
    }

    fn capabilities(&self) -> BTreeSet<SubstrateCapability> {
        // Baseline Decision Ledger contract only — no PR, merge, or CI capabilities.
        BTreeSet::from([SubstrateCapability::DurableReplay])
    }
}

impl AtomicEventStoreAdapter for DriveFolderAdapter {
    fn append_atomic_batch(&self, batch: AtomicEventBatchV1) -> Result<(), EventStoreError> {
        batch.validate()?;
        std::fs::create_dir_all(&self.folder_path).map_err(|error| EventStoreError::Io {
            reason: error.to_string(),
        })?;
        let filename = format!("batch-{}.json", batch.batch_id);
        let final_path = self.folder_path.join(&filename);
        if final_path.exists() {
            return Err(EventStoreError::Conflict {
                reason: format!("atomic event batch {} already exists", batch.batch_id),
            });
        }
        let content = serde_json::to_string_pretty(&batch).map_err(|error| {
            EventStoreError::Serialization {
                reason: error.to_string(),
            }
        })?;
        let temporary_path = self.folder_path.join(format!(".tmp-{filename}"));
        std::fs::write(&temporary_path, content).map_err(|error| EventStoreError::Io {
            reason: error.to_string(),
        })?;
        std::fs::rename(&temporary_path, &final_path).map_err(|error| EventStoreError::Io {
            reason: format!("atomic batch rename failed: {error}"),
        })?;
        Ok(())
    }
}
