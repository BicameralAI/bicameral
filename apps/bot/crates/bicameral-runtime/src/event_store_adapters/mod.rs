//! Event-store adapter implementations.
//!
//! Both git-backed and drive-folder substrates are in architectural scope.
//! The interface is substrate-neutral per plan section 7.1.

pub mod drive_folder;
pub mod git;
pub mod memory;

pub use drive_folder::DriveFolderAdapter;
pub use git::{GitAdapter, GitAdapterConfig};
pub use memory::MemoryAdapter;

use bicameral_api::event_store::{EventStoreAdapter, SubstrateKind};
use bicameral_config::{BicameralPaths, EventStoreConfig};
use std::sync::Arc;

use crate::atomic_event_store::AtomicEventStoreAdapter;

/// Dual view of one concrete adapter: baseline event-store + atomic batch.
///
/// Both Arcs share the same concrete instance so Memory adapters stay a single
/// process-local ledger (two separate constructions would fork state).
pub struct DualEventStoreAdapter {
    pub event_store: Arc<dyn EventStoreAdapter>,
    pub atomic: Arc<dyn AtomicEventStoreAdapter>,
}

/// Construct the appropriate event-store adapter from configuration and workspace paths.
///
/// Called at daemon/gateway startup to produce the adapter that `AppState`
/// will append to and replay from for the lifetime of the process.
pub fn adapter_from_config(
    config: &EventStoreConfig,
    paths: &BicameralPaths,
) -> Arc<dyn EventStoreAdapter> {
    adapter_from_config_dual(config, paths).event_store
}

/// Construct dual event-store views from the same concrete adapter instance.
///
/// `candidate_selection.commit` requires [`AtomicEventStoreAdapter`]; other
/// ToolRequest paths only need [`EventStoreAdapter`].
pub fn adapter_from_config_dual(
    config: &EventStoreConfig,
    paths: &BicameralPaths,
) -> DualEventStoreAdapter {
    match config.substrate {
        SubstrateKind::Git => {
            let event_dir = config
                .git_path
                .clone()
                .map(|path| {
                    if path.is_absolute() {
                        path
                    } else {
                        paths.workspace_root.join(path)
                    }
                })
                .unwrap_or_else(|| paths.events_dir.clone());
            let adapter_config = GitAdapterConfig {
                ci_merge_gate_configured: config.ci_merge_gate,
            };
            let concrete = Arc::new(GitAdapter::with_config(&event_dir, adapter_config));
            DualEventStoreAdapter {
                event_store: concrete.clone(),
                atomic: concrete,
            }
        }
        SubstrateKind::DriveFolder => {
            let folder = config
                .drive_folder_path
                .clone()
                .expect("event_store.drive_folder_path is required when substrate is drive_folder");
            let folder = if folder.is_absolute() {
                folder
            } else {
                paths.workspace_root.join(folder)
            };
            let concrete = Arc::new(DriveFolderAdapter::new(&folder));
            DualEventStoreAdapter {
                event_store: concrete.clone(),
                atomic: concrete,
            }
        }
        SubstrateKind::Memory => {
            let concrete = Arc::new(MemoryAdapter::new());
            DualEventStoreAdapter {
                event_store: concrete.clone(),
                atomic: concrete,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_api::event_store::{
        EventStoreAdapter, EventStoreEntry, EventType, SubstrateCapability, SubstrateKind,
    };

    #[test]
    fn relative_git_path_is_resolved_against_explicit_workspace_root() {
        let workspace = tempfile::tempdir().unwrap();
        let paths = BicameralPaths::from_workspace_root(workspace.path());
        let config = EventStoreConfig {
            substrate: SubstrateKind::Git,
            git_path: Some("custom/events".into()),
            drive_folder_path: None,
            ci_merge_gate: false,
        };
        let adapter = adapter_from_config(&config, &paths);
        adapter
            .append(EventStoreEntry::new(
                EventType::Ingest,
                serde_json::json!({"test": true}),
                "test".into(),
            ))
            .unwrap();

        assert_eq!(
            std::fs::read_dir(workspace.path().join("custom/events"))
                .unwrap()
                .count(),
            1
        );
    }

    // -- Git capability metadata tests (separate from baseline append/replay) --

    #[test]
    fn git_default_advertises_baseline_pr_capabilities_without_ci_gate() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = GitAdapter::new(dir.path());
        let caps = adapter.capabilities();

        assert!(caps.contains(&SubstrateCapability::DurableReplay));
        assert!(caps.contains(&SubstrateCapability::BranchLocalProposals));
        assert!(caps.contains(&SubstrateCapability::PrNativeReview));
        assert!(caps.contains(&SubstrateCapability::NativeMergeConflictResolution));
        assert!(
            !caps.contains(&SubstrateCapability::CiMergeGateEnforcement),
            "default Git adapter must not claim CiMergeGateEnforcement"
        );
        assert_eq!(caps.len(), 4);
    }

    #[test]
    fn git_with_ci_gate_configured_advertises_all_capabilities() {
        let dir = tempfile::tempdir().unwrap();
        let config = GitAdapterConfig {
            ci_merge_gate_configured: true,
        };
        let adapter = GitAdapter::with_config(dir.path(), config);
        let caps = adapter.capabilities();

        assert!(caps.contains(&SubstrateCapability::DurableReplay));
        assert!(caps.contains(&SubstrateCapability::BranchLocalProposals));
        assert!(caps.contains(&SubstrateCapability::PrNativeReview));
        assert!(caps.contains(&SubstrateCapability::NativeMergeConflictResolution));
        assert!(
            caps.contains(&SubstrateCapability::CiMergeGateEnforcement),
            "CI gate must be advertised when configured"
        );
        assert_eq!(caps.len(), 5);
    }

    #[test]
    fn git_ci_gate_does_not_affect_baseline_capabilities() {
        let dir = tempfile::tempdir().unwrap();
        let without = GitAdapter::new(dir.path());
        let with = GitAdapter::with_config(
            dir.path(),
            GitAdapterConfig {
                ci_merge_gate_configured: true,
            },
        );

        let baseline_caps: std::collections::BTreeSet<_> = without
            .capabilities()
            .into_iter()
            .filter(|c| *c != SubstrateCapability::CiMergeGateEnforcement)
            .collect();
        let with_caps: std::collections::BTreeSet<_> = with
            .capabilities()
            .into_iter()
            .filter(|c| *c != SubstrateCapability::CiMergeGateEnforcement)
            .collect();

        assert_eq!(
            baseline_caps, with_caps,
            "CI gate config must not alter baseline capabilities"
        );
    }

    #[test]
    fn drive_folder_advertises_durable_replay_only() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = DriveFolderAdapter::new(dir.path());
        let caps = adapter.capabilities();

        assert!(caps.contains(&SubstrateCapability::DurableReplay));
        assert!(!caps.contains(&SubstrateCapability::BranchLocalProposals));
        assert!(!caps.contains(&SubstrateCapability::PrNativeReview));
        assert!(!caps.contains(&SubstrateCapability::NativeMergeConflictResolution));
        assert!(!caps.contains(&SubstrateCapability::CiMergeGateEnforcement));
        assert_eq!(caps.len(), 1);
    }

    #[test]
    fn memory_adapter_advertises_no_capabilities() {
        let adapter = MemoryAdapter::new();
        let caps = adapter.capabilities();

        assert!(caps.is_empty());
    }

    #[test]
    fn git_has_strict_superset_of_drive_capabilities() {
        let dir = tempfile::tempdir().unwrap();
        let git = GitAdapter::new(dir.path());
        let drive = DriveFolderAdapter::new(dir.path());

        assert!(git.capabilities().is_superset(&drive.capabilities()));
        assert!(!drive.capabilities().is_superset(&git.capabilities()));
    }

    #[test]
    fn drive_has_strict_superset_of_memory_capabilities() {
        let dir = tempfile::tempdir().unwrap();
        let drive = DriveFolderAdapter::new(dir.path());
        let mem = MemoryAdapter::new();

        assert!(drive.capabilities().is_superset(&mem.capabilities()));
        assert!(!mem.capabilities().is_superset(&drive.capabilities()));
    }

    #[test]
    fn health_check_includes_capabilities() {
        let dir = tempfile::tempdir().unwrap();
        let git = GitAdapter::new(dir.path());
        let health = git.health_check().unwrap();

        assert_eq!(health.capabilities, git.capabilities());
        assert!(health
            .capabilities
            .contains(&SubstrateCapability::DurableReplay));
    }

    #[test]
    fn substrate_kind_unchanged_by_capability_addition() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            GitAdapter::new(dir.path()).substrate_kind(),
            SubstrateKind::Git
        );
        assert_eq!(
            DriveFolderAdapter::new(dir.path()).substrate_kind(),
            SubstrateKind::DriveFolder
        );
        assert_eq!(MemoryAdapter::new().substrate_kind(), SubstrateKind::Memory);
    }

    #[test]
    fn capability_serializes_to_snake_case() {
        let json = serde_json::to_string(&SubstrateCapability::PrNativeReview).unwrap();
        assert_eq!(json, "\"pr_native_review\"");

        let round_tripped: SubstrateCapability = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped, SubstrateCapability::PrNativeReview);
    }

    // -- Git event artifact reviewability tests --

    #[test]
    fn git_appends_create_individual_yaml_files() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = GitAdapter::new(dir.path());

        let entry = bicameral_api::event_store::EventStoreEntry::new(
            bicameral_api::event_store::EventType::PolicyUpdated,
            serde_json::json!({}),
            "reviewability-test".to_string(),
        );
        adapter.append(entry).unwrap();

        let yamls: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|ext| ext.to_str()) == Some("yaml"))
            .collect();
        assert_eq!(yamls.len(), 1, "exactly one YAML event file expected");

        let content = std::fs::read_to_string(yamls[0].path()).unwrap();
        assert!(
            !content.is_empty(),
            "event file must contain serialized content"
        );
        let parsed: bicameral_api::event_store::EventStoreEntry =
            serde_yaml::from_str(&content).unwrap();
        assert_eq!(parsed.actor, "reviewability-test");
    }

    #[test]
    fn git_event_files_are_human_readable_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = GitAdapter::new(dir.path());

        let entry = bicameral_api::event_store::EventStoreEntry::new(
            bicameral_api::event_store::EventType::Ingest,
            serde_json::json!({"test": true}),
            "human-readable-test".to_string(),
        );
        adapter.append(entry).unwrap();

        let yamls: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|ext| ext.to_str()) == Some("yaml"))
            .collect();

        let content = std::fs::read_to_string(yamls[0].path()).unwrap();
        assert!(
            content.contains("human-readable-test"),
            "YAML must contain actor string in readable form"
        );
        assert!(content.contains("ingest"), "YAML must contain event type");
    }

    #[test]
    fn git_no_jsonl_written_on_new_append() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = GitAdapter::new(dir.path());

        let entry = bicameral_api::event_store::EventStoreEntry::new(
            bicameral_api::event_store::EventType::PolicyUpdated,
            serde_json::json!({}),
            "no-jsonl-test".to_string(),
        );
        adapter.append(entry).unwrap();

        assert!(
            !dir.path().join("events.jsonl").exists(),
            "new appends must not create legacy JSONL file"
        );
    }

    #[test]
    fn git_legacy_jsonl_replay_backward_compat() {
        let dir = tempfile::tempdir().unwrap();

        // Write a legacy JSONL file directly.
        let entry = bicameral_api::event_store::EventStoreEntry::new(
            bicameral_api::event_store::EventType::PolicyUpdated,
            serde_json::json!({}),
            "legacy-actor".to_string(),
        );
        let line = serde_json::to_string(&entry).unwrap();
        std::fs::write(dir.path().join("events.jsonl"), format!("{line}\n")).unwrap();

        let adapter = GitAdapter::new(dir.path());
        let replayed = adapter.replay().unwrap();

        assert_eq!(replayed.len(), 1);
        assert_eq!(replayed[0].actor, "legacy-actor");
    }

    #[test]
    fn git_health_warns_on_legacy_layout() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("events.jsonl"), "{}\n").unwrap();

        let adapter = GitAdapter::new(dir.path());
        let health = adapter.health_check().unwrap();

        assert!(
            health.warnings.iter().any(|w| w.contains("Legacy JSONL")),
            "health must warn about legacy layout"
        );
    }

    #[test]
    fn drive_does_not_use_git_pr_semantics() {
        let dir = tempfile::tempdir().unwrap();
        let drive = DriveFolderAdapter::new(dir.path());
        let caps = drive.capabilities();

        assert!(
            !caps.contains(&SubstrateCapability::BranchLocalProposals),
            "Drive must not claim branch-local proposals"
        );
        assert!(
            !caps.contains(&SubstrateCapability::PrNativeReview),
            "Drive must not claim PR-native review"
        );
        assert!(
            !caps.contains(&SubstrateCapability::NativeMergeConflictResolution),
            "Drive must not claim native merge conflict resolution"
        );
        assert!(
            !caps.contains(&SubstrateCapability::CiMergeGateEnforcement),
            "Drive must not claim CI/merge-gate enforcement"
        );
    }
}
