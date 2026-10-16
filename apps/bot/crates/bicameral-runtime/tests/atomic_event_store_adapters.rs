//! Production-adapter evidence for the CandidateSelection atomic batch boundary.

use bicameral_api::event_store::{EventStoreAdapter, EventStoreEntry, EventType};
use bicameral_runtime::atomic_event_store::{AtomicEventBatchV1, AtomicEventStoreAdapter};
use bicameral_runtime::event_store_adapters::drive_folder::DriveFolderAdapter;
use bicameral_runtime::event_store_adapters::git::GitAdapter;
use bicameral_runtime::event_store_adapters::memory::MemoryAdapter;
use serde_json::json;
use tempfile::tempdir;
use uuid::Uuid;

fn entries() -> Vec<EventStoreEntry> {
    let mut first = EventStoreEntry::new(
        EventType::AcceptCandidate,
        json!({"candidate_id": Uuid::from_u128(1)}),
        "owner".into(),
    );
    first.id = Uuid::from_u128(101);
    let mut second = EventStoreEntry::new(
        EventType::AcceptCandidate,
        json!({"candidate_id": Uuid::from_u128(2)}),
        "owner".into(),
    );
    second.id = Uuid::from_u128(102);
    vec![first, second]
}

fn batch() -> AtomicEventBatchV1 {
    AtomicEventBatchV1::new(Uuid::from_u128(987), entries()).unwrap()
}

#[test]
fn memory_adapter_materializes_the_complete_batch_under_one_lock() {
    let adapter = MemoryAdapter::new();
    adapter.append_atomic_batch(batch()).unwrap();
    let replayed = adapter.replay().unwrap();
    assert_eq!(replayed.len(), 2);
    assert_eq!(
        replayed.iter().map(|entry| entry.id).collect::<Vec<_>>(),
        vec![Uuid::from_u128(101), Uuid::from_u128(102)]
    );
}

#[test]
fn git_adapter_uses_one_yaml_batch_envelope_and_replays_both_events() {
    let directory = tempdir().unwrap();
    let adapter = GitAdapter::new(directory.path());
    adapter.append_atomic_batch(batch()).unwrap();

    let yaml_files = std::fs::read_dir(directory.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("yaml"))
        .count();
    assert_eq!(
        yaml_files, 1,
        "one atomic batch must be one filesystem artifact"
    );

    let replayed = adapter.replay().unwrap();
    assert_eq!(replayed.len(), 2);
    assert_eq!(
        replayed.iter().map(|entry| entry.id).collect::<Vec<_>>(),
        vec![Uuid::from_u128(101), Uuid::from_u128(102)]
    );
}

#[test]
fn drive_folder_adapter_uses_one_json_batch_envelope_and_replays_both_events() {
    let directory = tempdir().unwrap();
    let adapter = DriveFolderAdapter::new(directory.path());
    adapter.append_atomic_batch(batch()).unwrap();

    let json_files = std::fs::read_dir(directory.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
        .count();
    assert_eq!(
        json_files, 1,
        "one atomic batch must be one filesystem artifact"
    );

    let replayed = adapter.replay().unwrap();
    assert_eq!(replayed.len(), 2);
    assert_eq!(
        replayed.iter().map(|entry| entry.id).collect::<Vec<_>>(),
        vec![Uuid::from_u128(101), Uuid::from_u128(102)]
    );
}
