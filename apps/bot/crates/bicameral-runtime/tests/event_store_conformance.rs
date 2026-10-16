//! EventStoreAdapter conformance test harness.
//!
//! Runs identical append/replay assertions against every `EventStoreAdapter`
//! implementation that can be instantiated without network or external
//! credentials.  The goal is to verify that each adapter honours the baseline
//! contract defined in `bicameral-api::event_store::EventStoreAdapter`.
//!
//! # Assertion categories
//!
//! ## Baseline contract (every adapter MUST pass)
//!
//! | Test helper                              | What it checks                                  |
//! |------------------------------------------|--------------------------------------------------|
//! | `assert_accepted_event_append`           | Append succeeds and replay returns the entry     |
//! | `assert_deterministic_replay_ordering`   | Multiple events replay in timestamp order        |
//! | `assert_replay_into_materialized_state`  | replay_from_adapter rebuilds equivalent state    |
//! | `assert_identity_preservation`           | actor, timestamp, event id, type survive round-trip |
//! | `assert_health_freshness`                | health_check returns valid SubstrateHealth       |
//! | `assert_empty_replay`                    | Empty store replays to empty vec                 |
//! | `assert_duplicate_event_id_behavior`     | Duplicate UUIDs are accepted (append-only log)   |
//! | `assert_malformed_payload_in_replay`     | Non-Ingest events with minimal payloads replay   |
//!
//! ## Optional substrate-capability assertions
//!
//! | Test helper                              | What it checks                                  |
//! |------------------------------------------|--------------------------------------------------|
//! | `assert_capabilities_consistent`         | Declared capabilities match substrate kind       |
//! | `assert_health_includes_capabilities`    | health.capabilities == adapter.capabilities()    |
//! | `assert_durable_replay_after_reopen`     | DurableReplay adapters survive re-instantiation  |

// ---------------------------------------------------------------------------
// Conformance helpers — reusable across any EventStoreAdapter
// ---------------------------------------------------------------------------

mod conformance {
    use bicameral_api::candidate::{DecisionCandidate, DecisionLevel};
    use bicameral_api::event_store::{
        EventStoreAdapter, EventStoreEntry, EventType, IngestPayload, SubstrateCapability,
        SubstrateKind,
    };
    use bicameral_api::evidence::SourceEvidence;
    use bicameral_api::source::{Source, SourceSnapshot};
    use bicameral_runtime::replay::replay_from_adapter;
    use chrono::{Duration, Utc};
    use uuid::Uuid;

    // -- fixture builders ---------------------------------------------------

    /// Build a minimal `EventStoreEntry` with the given type, actor, optional
    /// fixed UUID, and timestamp offset (seconds from "now").
    ///
    /// Uses `PolicyUpdated` as the default "safe" event type — the replay engine
    /// ignores it via the wildcard arm, so bare `{}` payloads never trigger
    /// deserialization errors.
    pub fn make_entry(
        event_type: EventType,
        actor: &str,
        id: Option<Uuid>,
        ts_offset_secs: i64,
    ) -> EventStoreEntry {
        let base = Utc::now() + Duration::seconds(ts_offset_secs);
        EventStoreEntry {
            id: id.unwrap_or_else(Uuid::new_v4),
            event_type,
            payload: serde_json::json!({}),
            timestamp: base,
            actor: actor.to_string(),
            governance_result_id: None,
        }
    }

    /// Build a complete `Ingest` entry with a well-formed `IngestPayload` so
    /// that `replay_from_adapter` can deserialize and materialize state.
    pub fn make_ingest_entry(actor: &str, ts_offset_secs: i64) -> EventStoreEntry {
        let candidate_id = Uuid::new_v4();
        let evidence_id = Uuid::new_v4();
        let snapshot_addr = format!("sha256:{}", Uuid::new_v4());

        let mut payload = IngestPayload {
            source: Source {
                uri: format!("test://source/{}", Uuid::new_v4()),
                source_type: "test".to_string(),
                label: Some("conformance test source".to_string()),
                source_kind: None,
            },
            snapshot: Some(SourceSnapshot {
                snapshot_addr: snapshot_addr.clone(),
                snapshot_ref: "test-ref".to_string(),
                captured_at: Utc::now(),
                content: Some("test content".to_string()),
                content_hash: Some("deadbeef".to_string()),
                capture_method: None,
            }),
            evidence: vec![SourceEvidence::new(
                snapshot_addr,
                0,
                12,
                "test content".to_string(),
                0.9,
            )],
            candidate: DecisionCandidate::new(
                "Test decision".to_string(),
                "Conformance harness test candidate".to_string(),
                DecisionLevel::Implementation,
                "test://source".to_string(),
            ),
            evidence_references: Vec::new(),
            provenance: None,
        };
        payload.candidate.id = candidate_id;
        payload.candidate.evidence_refs = vec![evidence_id];

        let base = Utc::now() + Duration::seconds(ts_offset_secs);
        EventStoreEntry {
            id: Uuid::new_v4(),
            event_type: EventType::Ingest,
            payload: serde_json::to_value(&payload).expect("serialize IngestPayload"),
            timestamp: base,
            actor: actor.to_string(),
            governance_result_id: None,
        }
    }

    // -- Baseline contract assertions ---------------------------------------

    /// Append a single event and replay it; verify the entry is returned.
    pub fn assert_accepted_event_append(adapter: &dyn EventStoreAdapter) {
        // Use PolicyUpdated — the replay engine's wildcard arm ignores it,
        // so a bare `{}` payload is safe.
        let entry = make_entry(EventType::PolicyUpdated, "conformance-bot", None, 0);
        let id = entry.id;

        adapter.append(entry).expect("append must succeed");

        let replayed = adapter.replay().expect("replay must succeed");
        assert!(
            replayed.iter().any(|e| e.id == id),
            "replayed events must contain the appended entry"
        );
    }

    /// Append events with increasing timestamps; verify replay returns them in
    /// timestamp order regardless of insertion concurrency.
    pub fn assert_deterministic_replay_ordering(adapter: &dyn EventStoreAdapter) {
        let mut ids = Vec::new();
        for i in 0..5 {
            let entry = make_entry(
                EventType::PolicyUpdated,
                &format!("actor-{i}"),
                None,
                i * 10,
            );
            ids.push(entry.id);
            adapter.append(entry).expect("append must succeed");
        }

        let replayed = adapter.replay().expect("replay must succeed");
        let our: Vec<_> = replayed.iter().filter(|e| ids.contains(&e.id)).collect();

        assert_eq!(our.len(), 5, "all five entries must be replayed");

        for window in our.windows(2) {
            assert!(
                window[0].timestamp <= window[1].timestamp,
                "replay must be in timestamp order: {:?} should come before {:?}",
                window[0].timestamp,
                window[1].timestamp,
            );
        }
    }

    /// Append Ingest events, replay via `replay_from_adapter`, and verify the
    /// materialized `ReplayedState` contains the expected candidates, sources,
    /// snapshots, and evidence.
    ///
    /// **Important:** this assertion must run on a fresh adapter (no prior bare
    /// entries) because `replay_from_adapter` deserializes every `Ingest` event.
    pub fn assert_replay_into_materialized_state(adapter: &dyn EventStoreAdapter) {
        let entry = make_ingest_entry("replay-test-bot", 0);
        let payload: IngestPayload =
            serde_json::from_value(entry.payload.clone()).expect("deserialize fixture");
        let expected_candidate_id = payload.candidate.id;
        let expected_source_uri = payload.source.uri.clone();

        adapter.append(entry).expect("append must succeed");

        let state = replay_from_adapter(adapter).expect("replay_from_adapter must succeed");

        assert!(
            state.candidates.contains_key(&expected_candidate_id),
            "materialized state must contain the ingested candidate"
        );
        assert!(
            state.sources.contains_key(&expected_source_uri),
            "materialized state must contain the ingested source"
        );
        assert!(
            !state.events.is_empty(),
            "materialized state must have at least one event"
        );
    }

    /// Verify actor, timestamp, event ID, and event type survive the
    /// append → replay round-trip.
    pub fn assert_identity_preservation(adapter: &dyn EventStoreAdapter) {
        let fixed_id = Uuid::new_v4();
        let actor = "identity-test-actor";
        let entry = make_entry(EventType::PolicyUpdated, actor, Some(fixed_id), 100);
        let original_ts = entry.timestamp;
        let original_type = entry.event_type;

        adapter.append(entry).expect("append must succeed");

        let replayed = adapter.replay().expect("replay must succeed");
        let found = replayed
            .iter()
            .find(|e| e.id == fixed_id)
            .expect("must find entry by ID");

        assert_eq!(found.actor, actor, "actor must be preserved");
        assert_eq!(found.timestamp, original_ts, "timestamp must be preserved");
        assert_eq!(
            found.event_type, original_type,
            "event_type must be preserved"
        );
        assert_eq!(found.id, fixed_id, "event id must be preserved");
    }

    /// Health check returns a valid `SubstrateHealth` with a matching kind and
    /// non-negative entry count.
    pub fn assert_health_freshness(adapter: &dyn EventStoreAdapter) {
        let health = adapter.health_check().expect("health_check must succeed");

        assert_eq!(
            health.kind,
            adapter.substrate_kind(),
            "health.kind must match adapter.substrate_kind()"
        );
        assert!(health.reachable, "adapter must report as reachable");
    }

    /// An adapter with no events should replay to an empty vec.
    pub fn assert_empty_replay(adapter: &dyn EventStoreAdapter) {
        let replayed = adapter.replay().expect("replay must succeed");
        assert!(
            replayed.is_empty(),
            "empty adapter must replay to empty vec"
        );
    }

    /// Appending two entries with the same UUID must not error.
    ///
    /// Substrate-specific behavior:
    /// - **JSONL-based** adapters (Git, Memory) keep both entries in the log.
    /// - **File-per-event** adapters (DriveFolder) overwrite the file, so only
    ///   the last write survives.  This is documented, not a bug.
    ///
    /// The assertion therefore checks: (a) neither append returns an error, and
    /// (b) at least one entry with the shared id is present in replay.
    pub fn assert_duplicate_event_id_behavior(adapter: &dyn EventStoreAdapter) {
        let shared_id = Uuid::new_v4();
        let e1 = make_entry(EventType::PolicyUpdated, "dup-actor-1", Some(shared_id), 0);
        let e2 = make_entry(EventType::PolicyUpdated, "dup-actor-2", Some(shared_id), 1);

        adapter.append(e1).expect("first append must succeed");
        adapter
            .append(e2)
            .expect("second append (same id) must succeed");

        let replayed = adapter.replay().expect("replay must succeed");
        let dups: Vec<_> = replayed.iter().filter(|e| e.id == shared_id).collect();
        assert!(
            !dups.is_empty(),
            "at least one entry with the duplicate UUID must survive replay"
        );

        // JSONL-based adapters preserve both; file-per-event adapters keep the last.
        match adapter.substrate_kind() {
            SubstrateKind::Git | SubstrateKind::Memory => {
                assert!(
                    dups.len() >= 2,
                    "JSONL-based adapter should keep all entries with the same UUID (found {})",
                    dups.len()
                );
            }
            SubstrateKind::DriveFolder => {
                // Overwrite is expected — the last writer wins.
                assert_eq!(
                    dups.len(),
                    1,
                    "file-per-event adapter should keep only the last entry"
                );
                assert_eq!(dups[0].actor, "dup-actor-2");
            }
        }
    }

    /// Events with non-Ingest types and minimal `{}` payloads should survive
    /// the append → replay round-trip without deserialization errors.
    /// This validates that the replay engine's wildcard arm handles events
    /// that carry domain-specific or opaque payloads.
    pub fn assert_malformed_payload_in_replay(adapter: &dyn EventStoreAdapter) {
        let legacy_types = [
            EventType::DecisionAccepted,
            EventType::DecisionRejected,
            EventType::EvidenceAttached,
            EventType::ReviewCompleted,
            EventType::PolicyUpdated,
        ];

        let mut ids = Vec::new();
        for (i, et) in legacy_types.iter().enumerate() {
            let entry = make_entry(*et, "legacy-test", None, i as i64);
            ids.push(entry.id);
            adapter.append(entry).expect("append must succeed");
        }

        let replayed = adapter.replay().expect("replay must succeed");
        for id in &ids {
            assert!(
                replayed.iter().any(|e| &e.id == id),
                "legacy/opaque event {id} must survive round-trip"
            );
        }
    }

    // -- Optional substrate-capability assertions ---------------------------

    /// Verify that declared capabilities are consistent with the substrate kind.
    /// Memory adapters should not claim DurableReplay; Git and DriveFolder
    /// adapters should declare it.
    pub fn assert_capabilities_consistent(adapter: &dyn EventStoreAdapter) {
        let caps = adapter.capabilities();
        let kind = adapter.substrate_kind();

        match kind {
            SubstrateKind::Memory => {
                assert!(
                    !caps.contains(&SubstrateCapability::DurableReplay),
                    "Memory adapter must not claim DurableReplay"
                );
            }
            SubstrateKind::Git => {
                assert!(
                    caps.contains(&SubstrateCapability::DurableReplay),
                    "Git adapter must declare DurableReplay"
                );
            }
            SubstrateKind::DriveFolder => {
                assert!(
                    caps.contains(&SubstrateCapability::DurableReplay),
                    "DriveFolder adapter must declare DurableReplay"
                );
            }
        }
    }

    /// `health_check().capabilities` must equal `adapter.capabilities()`.
    pub fn assert_health_includes_capabilities(adapter: &dyn EventStoreAdapter) {
        let health = adapter.health_check().expect("health_check must succeed");
        assert_eq!(
            health.capabilities,
            adapter.capabilities(),
            "health.capabilities must match adapter.capabilities()"
        );
    }

    /// For adapters that claim `DurableReplay`, data must survive
    /// re-instantiation from the same path.  Caller must provide a closure
    /// that re-opens the adapter from the same backing path.
    pub fn assert_durable_replay_after_reopen<F>(adapter: &dyn EventStoreAdapter, reopen: F)
    where
        F: FnOnce() -> Box<dyn EventStoreAdapter>,
    {
        if !adapter
            .capabilities()
            .contains(&SubstrateCapability::DurableReplay)
        {
            return;
        }

        let entry = make_entry(EventType::PolicyUpdated, "durability-test", None, 0);
        let id = entry.id;
        adapter.append(entry).expect("append must succeed");

        let reopened = reopen();
        let replayed = reopened.replay().expect("replay after reopen must succeed");
        assert!(
            replayed.iter().any(|e| e.id == id),
            "DurableReplay adapter must retain events across re-instantiation"
        );
    }

    // -- Convenience runners ------------------------------------------------

    /// Run all baseline contract assertions that do NOT require
    /// `replay_from_adapter` (and thus tolerate mixed event types in the log).
    pub fn run_baseline_suite(adapter: &dyn EventStoreAdapter) {
        assert_empty_replay(adapter);
        assert_accepted_event_append(adapter);
        assert_deterministic_replay_ordering(adapter);
        assert_identity_preservation(adapter);
        assert_health_freshness(adapter);
        assert_duplicate_event_id_behavior(adapter);
        assert_malformed_payload_in_replay(adapter);
    }

    /// Run the materialized-state replay assertion on a *fresh* adapter so
    /// `replay_from_adapter` only sees well-formed `Ingest` payloads.
    pub fn run_materialized_replay_suite(adapter: &dyn EventStoreAdapter) {
        assert_replay_into_materialized_state(adapter);
    }

    /// Run all optional substrate-capability assertions.
    pub fn run_capability_suite(adapter: &dyn EventStoreAdapter) {
        assert_capabilities_consistent(adapter);
        assert_health_includes_capabilities(adapter);
    }
}

// ===========================================================================
// Per-adapter test modules
// ===========================================================================

/// MemoryAdapter — baseline + capability + materialized-replay assertions.
mod memory_adapter {
    use super::conformance;
    use bicameral_runtime::event_store_adapters::MemoryAdapter;

    #[test]
    fn baseline_contract() {
        let adapter = MemoryAdapter::new();
        conformance::run_baseline_suite(&adapter);
    }

    #[test]
    fn materialized_replay() {
        let adapter = MemoryAdapter::new();
        conformance::run_materialized_replay_suite(&adapter);
    }

    #[test]
    fn capability_assertions() {
        let adapter = MemoryAdapter::new();
        conformance::run_capability_suite(&adapter);
    }
}

/// GitAdapter — baseline + capability + materialized-replay + durability.
mod git_adapter {
    use super::conformance;
    use bicameral_runtime::event_store_adapters::GitAdapter;

    #[test]
    fn baseline_contract() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = GitAdapter::new(dir.path());
        conformance::run_baseline_suite(&adapter);
    }

    #[test]
    fn materialized_replay() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = GitAdapter::new(dir.path());
        conformance::run_materialized_replay_suite(&adapter);
    }

    #[test]
    fn capability_assertions() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = GitAdapter::new(dir.path());
        conformance::run_capability_suite(&adapter);
    }

    #[test]
    fn durable_replay_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let adapter = GitAdapter::new(&path);

        conformance::assert_durable_replay_after_reopen(&adapter, move || {
            Box::new(GitAdapter::new(&path))
        });
    }
}

/// DriveFolderAdapter — baseline + capability + materialized-replay + durability.
mod drive_folder_adapter {
    use super::conformance;
    use bicameral_runtime::event_store_adapters::DriveFolderAdapter;

    #[test]
    fn baseline_contract() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = DriveFolderAdapter::new(dir.path());
        conformance::run_baseline_suite(&adapter);
    }

    #[test]
    fn materialized_replay() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = DriveFolderAdapter::new(dir.path());
        conformance::run_materialized_replay_suite(&adapter);
    }

    #[test]
    fn capability_assertions() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = DriveFolderAdapter::new(dir.path());
        conformance::run_capability_suite(&adapter);
    }

    #[test]
    fn durable_replay_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let adapter = DriveFolderAdapter::new(&path);

        conformance::assert_durable_replay_after_reopen(&adapter, move || {
            Box::new(DriveFolderAdapter::new(&path))
        });
    }
}
