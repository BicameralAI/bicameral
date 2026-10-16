//! Drive-folder adapter hardening tests.
//!
//! Covers freshness/offline/stale state, conflict detection, replay
//! determinism with tie-breaking, temp-file skipping, and capability
//! boundary assertions. No external Google credentials required.

use bicameral_api::event_store::{
    EventStoreAdapter, EventStoreEntry, EventType, FreshnessState, SubstrateCapability,
    SubstrateKind,
};
use bicameral_runtime::event_store_adapters::DriveFolderAdapter;
use chrono::{Duration, Utc};
use std::path::Path;
use uuid::Uuid;

fn make_entry(event_type: EventType, actor: &str, ts_offset_secs: i64) -> EventStoreEntry {
    let base = Utc::now() + Duration::seconds(ts_offset_secs);
    EventStoreEntry {
        id: Uuid::new_v4(),
        event_type,
        payload: serde_json::json!({}),
        timestamp: base,
        actor: actor.to_string(),
        governance_result_id: None,
    }
}

// ---------------------------------------------------------------------------
// Freshness state
// ---------------------------------------------------------------------------

#[test]
fn freshness_is_offline_when_folder_missing() {
    let adapter = DriveFolderAdapter::new(Path::new("/nonexistent/drive-folder"));
    assert_eq!(adapter.freshness(), FreshnessState::Offline);
}

#[test]
fn freshness_is_unknown_when_no_sync_marker() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = DriveFolderAdapter::new(dir.path());
    assert_eq!(adapter.freshness(), FreshnessState::Unknown);
}

#[test]
fn freshness_is_fresh_after_mark_synced() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = DriveFolderAdapter::new(dir.path());
    adapter.mark_synced().unwrap();
    assert_eq!(adapter.freshness(), FreshnessState::Fresh);
}

#[test]
fn freshness_is_stale_when_marker_old() {
    let dir = tempfile::tempdir().unwrap();
    // Write an old timestamp directly.
    std::fs::create_dir_all(dir.path()).unwrap();
    let old_ts = Utc::now() - Duration::seconds(3600);
    std::fs::write(
        dir.path().join(".bicameral-sync-marker"),
        old_ts.to_rfc3339(),
    )
    .unwrap();

    let adapter = DriveFolderAdapter::new(dir.path());
    assert_eq!(adapter.freshness(), FreshnessState::Stale);
}

#[test]
fn custom_stale_threshold_is_respected() {
    let dir = tempfile::tempdir().unwrap();
    // Marker is 5 seconds old.
    let ts = Utc::now() - Duration::seconds(5);
    std::fs::create_dir_all(dir.path()).unwrap();
    std::fs::write(dir.path().join(".bicameral-sync-marker"), ts.to_rfc3339()).unwrap();

    // With a 10-second threshold, it should be fresh.
    let adapter = DriveFolderAdapter::with_stale_threshold(dir.path(), 10);
    assert_eq!(adapter.freshness(), FreshnessState::Fresh);

    // With a 2-second threshold, it should be stale.
    let adapter = DriveFolderAdapter::with_stale_threshold(dir.path(), 2);
    assert_eq!(adapter.freshness(), FreshnessState::Stale);
}

#[test]
fn last_synced_returns_none_when_no_marker() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = DriveFolderAdapter::new(dir.path());
    assert!(adapter.last_synced().is_none());
}

#[test]
fn last_synced_returns_timestamp_after_mark() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = DriveFolderAdapter::new(dir.path());
    adapter.mark_synced().unwrap();
    let ts = adapter.last_synced().unwrap();
    let age = Utc::now().signed_duration_since(ts);
    assert!(age.num_seconds() < 5, "last_synced should be recent");
}

// ---------------------------------------------------------------------------
// Health check
// ---------------------------------------------------------------------------

#[test]
fn health_check_includes_freshness_state() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = DriveFolderAdapter::new(dir.path());

    let health = adapter.health_check().unwrap();
    assert_eq!(health.freshness, Some(FreshnessState::Unknown));
    assert!(!health.warnings.is_empty());

    adapter.mark_synced().unwrap();
    let health = adapter.health_check().unwrap();
    assert_eq!(health.freshness, Some(FreshnessState::Fresh));
    assert!(health.warnings.is_empty());
}

#[test]
fn health_check_offline_reports_warnings() {
    let adapter = DriveFolderAdapter::new(Path::new("/nonexistent/drive-folder"));
    let health = adapter.health_check().unwrap();

    assert!(!health.reachable);
    assert_eq!(health.freshness, Some(FreshnessState::Offline));
    assert!(health.warnings.iter().any(|w| w.contains("not reachable")));
}

#[test]
fn health_check_stale_reports_warning() {
    let dir = tempfile::tempdir().unwrap();
    let old_ts = Utc::now() - Duration::seconds(3600);
    std::fs::create_dir_all(dir.path()).unwrap();
    std::fs::write(
        dir.path().join(".bicameral-sync-marker"),
        old_ts.to_rfc3339(),
    )
    .unwrap();

    let adapter = DriveFolderAdapter::new(dir.path());
    let health = adapter.health_check().unwrap();
    assert_eq!(health.freshness, Some(FreshnessState::Stale));
    assert!(health.warnings.iter().any(|w| w.contains("stale")));
}

#[test]
fn health_check_last_synced_populated() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = DriveFolderAdapter::new(dir.path());
    adapter.mark_synced().unwrap();

    let health = adapter.health_check().unwrap();
    assert!(health.last_synced.is_some());
}

// ---------------------------------------------------------------------------
// Capability boundary
// ---------------------------------------------------------------------------

#[test]
fn drive_does_not_advertise_pr_review() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = DriveFolderAdapter::new(dir.path());
    let caps = adapter.capabilities();

    assert!(caps.contains(&SubstrateCapability::DurableReplay));
    assert!(!caps.contains(&SubstrateCapability::PrNativeReview));
    assert!(!caps.contains(&SubstrateCapability::NativeMergeConflictResolution));
    assert!(!caps.contains(&SubstrateCapability::CiMergeGateEnforcement));
    assert!(!caps.contains(&SubstrateCapability::BranchLocalProposals));
    assert_eq!(caps.len(), 1, "Only DurableReplay should be advertised");
}

#[test]
fn substrate_kind_is_drive_folder() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = DriveFolderAdapter::new(dir.path());
    assert_eq!(adapter.substrate_kind(), SubstrateKind::DriveFolder);
}

// ---------------------------------------------------------------------------
// Replay determinism
// ---------------------------------------------------------------------------

#[test]
fn replay_is_deterministic_by_timestamp_then_uuid() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = DriveFolderAdapter::new(dir.path());

    // Create entries with the same timestamp but different UUIDs.
    let ts = Utc::now();
    let mut entries: Vec<EventStoreEntry> = (0..5)
        .map(|i| EventStoreEntry {
            id: Uuid::new_v4(),
            event_type: EventType::PolicyUpdated,
            payload: serde_json::json!({}),
            timestamp: ts,
            actor: format!("actor-{i}"),
            governance_result_id: None,
        })
        .collect();

    for e in &entries {
        adapter.append(e.clone()).unwrap();
    }

    // Sort expected by (timestamp, id).
    entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp).then_with(|| a.id.cmp(&b.id)));

    let replayed = adapter.replay().unwrap();
    let replayed_ids: Vec<_> = replayed.iter().map(|e| e.id).collect();
    let expected_ids: Vec<_> = entries.iter().map(|e| e.id).collect();
    assert_eq!(
        replayed_ids, expected_ids,
        "UUID tie-breaking must be deterministic"
    );
}

#[test]
fn replay_is_deterministic_across_reopen() {
    let dir = tempfile::tempdir().unwrap();

    let adapter1 = DriveFolderAdapter::new(dir.path());
    for i in 0..10 {
        let entry = make_entry(EventType::PolicyUpdated, &format!("actor-{i}"), i);
        adapter1.append(entry).unwrap();
    }

    let replay1: Vec<_> = adapter1.replay().unwrap().iter().map(|e| e.id).collect();

    let adapter2 = DriveFolderAdapter::new(dir.path());
    let replay2: Vec<_> = adapter2.replay().unwrap().iter().map(|e| e.id).collect();

    assert_eq!(
        replay1, replay2,
        "Replay must be identical across re-instantiation"
    );
}

// ---------------------------------------------------------------------------
// Conflict detection
// ---------------------------------------------------------------------------

#[test]
fn duplicate_uuid_overwrites_last_writer_wins() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = DriveFolderAdapter::new(dir.path());

    let shared_id = Uuid::new_v4();
    let e1 = EventStoreEntry {
        id: shared_id,
        event_type: EventType::PolicyUpdated,
        payload: serde_json::json!({}),
        timestamp: Utc::now(),
        actor: "first-writer".to_string(),
        governance_result_id: None,
    };
    let e2 = EventStoreEntry {
        id: shared_id,
        event_type: EventType::PolicyUpdated,
        payload: serde_json::json!({}),
        timestamp: Utc::now() + Duration::seconds(1),
        actor: "second-writer".to_string(),
        governance_result_id: None,
    };

    adapter.append(e1).unwrap();
    adapter.append(e2).unwrap();

    let replayed = adapter.replay().unwrap();
    let dups: Vec<_> = replayed.iter().filter(|e| e.id == shared_id).collect();
    assert_eq!(dups.len(), 1, "File-per-event means last writer wins");
    assert_eq!(dups[0].actor, "second-writer");
}

// ---------------------------------------------------------------------------
// Temp-file handling
// ---------------------------------------------------------------------------

#[test]
fn replay_skips_temp_files() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = DriveFolderAdapter::new(dir.path());

    // Write a normal event.
    let entry = make_entry(EventType::PolicyUpdated, "normal", 0);
    adapter.append(entry).unwrap();

    // Write a leftover temp file.
    let tmp_content =
        serde_json::to_string_pretty(&make_entry(EventType::PolicyUpdated, "temp-leftover", 1))
            .unwrap();
    std::fs::write(dir.path().join(".tmp-leftover.json"), &tmp_content).unwrap();

    let replayed = adapter.replay().unwrap();
    assert_eq!(
        replayed.len(),
        1,
        "Temp files must be skipped during replay"
    );
    assert_eq!(replayed[0].actor, "normal");
}

#[test]
fn health_check_excludes_temp_files_from_count() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = DriveFolderAdapter::new(dir.path());

    let entry = make_entry(EventType::PolicyUpdated, "counted", 0);
    adapter.append(entry).unwrap();

    std::fs::write(dir.path().join(".tmp-uncounted.json"), "{}").unwrap();

    let health = adapter.health_check().unwrap();
    assert_eq!(health.entry_count, 1, "Temp files should not be counted");
}

// ---------------------------------------------------------------------------
// Empty state
// ---------------------------------------------------------------------------

#[test]
fn empty_folder_replays_empty() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = DriveFolderAdapter::new(dir.path());
    let replayed = adapter.replay().unwrap();
    assert!(replayed.is_empty());
}

#[test]
fn missing_folder_replays_empty() {
    let adapter = DriveFolderAdapter::new(Path::new("/nonexistent/drive-folder"));
    let replayed = adapter.replay().unwrap();
    assert!(replayed.is_empty());
}

// ---------------------------------------------------------------------------
// Atomic-ish write
// ---------------------------------------------------------------------------

#[test]
fn append_creates_folder_if_needed() {
    let dir = tempfile::tempdir().unwrap();
    let subfolder = dir.path().join("nested").join("drive");
    let adapter = DriveFolderAdapter::new(&subfolder);

    let entry = make_entry(EventType::PolicyUpdated, "create-folder-test", 0);
    adapter.append(entry).unwrap();

    assert!(subfolder.exists());
    let replayed = adapter.replay().unwrap();
    assert_eq!(replayed.len(), 1);
}

// ---------------------------------------------------------------------------
// Non-event JSON files are skipped gracefully
// ---------------------------------------------------------------------------

#[test]
fn replay_skips_non_event_json_files() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = DriveFolderAdapter::new(dir.path());

    let entry = make_entry(EventType::PolicyUpdated, "real-event", 0);
    adapter.append(entry).unwrap();

    // Write a JSON file that is not a valid EventStoreEntry.
    std::fs::write(
        dir.path().join("not-an-event.json"),
        r#"{"something": "else"}"#,
    )
    .unwrap();

    let replayed = adapter.replay().unwrap();
    assert_eq!(replayed.len(), 1);
    assert_eq!(replayed[0].actor, "real-event");
}

// ---------------------------------------------------------------------------
// Two-instance shared folder: cross-instance event sharing
// ---------------------------------------------------------------------------

#[test]
fn two_instances_share_events_through_same_folder() {
    let shared_dir = tempfile::tempdir().unwrap();

    // Instance A appends an event (simulates accept/promote).
    let instance_a = DriveFolderAdapter::new(shared_dir.path());
    let entry_a = make_entry(EventType::AcceptCandidate, "instance-a", 0);
    let entry_a_id = entry_a.id;
    instance_a.append(entry_a).unwrap();

    // Instance B points at the same folder, replays, and sees the Decision.
    let instance_b = DriveFolderAdapter::new(shared_dir.path());
    let replayed = instance_b.replay().unwrap();

    assert_eq!(replayed.len(), 1, "Instance B must see Instance A's event");
    assert_eq!(replayed[0].id, entry_a_id);
    assert_eq!(replayed[0].actor, "instance-a");
    assert_eq!(replayed[0].event_type, EventType::AcceptCandidate);
}

#[test]
fn two_instances_see_each_others_appends() {
    let shared_dir = tempfile::tempdir().unwrap();

    let instance_a = DriveFolderAdapter::new(shared_dir.path());
    let instance_b = DriveFolderAdapter::new(shared_dir.path());

    // A writes first event.
    let entry_a = make_entry(EventType::AcceptCandidate, "team-member-a", -2);
    instance_a.append(entry_a).unwrap();

    // B writes second event.
    let entry_b = make_entry(EventType::AcceptCandidate, "team-member-b", -1);
    instance_b.append(entry_b).unwrap();

    // Both instances can replay all events in timestamp order.
    let replayed_a = instance_a.replay().unwrap();
    let replayed_b = instance_b.replay().unwrap();

    assert_eq!(replayed_a.len(), 2);
    assert_eq!(replayed_b.len(), 2);
    assert_eq!(replayed_a[0].actor, "team-member-a");
    assert_eq!(replayed_a[1].actor, "team-member-b");
    assert_eq!(replayed_b[0].actor, "team-member-a");
    assert_eq!(replayed_b[1].actor, "team-member-b");
}
