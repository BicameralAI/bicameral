//! Projection rebuild conformance — proves deleting and rebuilding
//! projection rows preserves the same read output.
//!
//! Per issue #233 acceptance criteria:
//! - Projection can be rebuilt from event-store replay plus current
//!   source/graph indexes.
//! - Tests prove deleting and rebuilding projection rows preserves the
//!   same read output.
//! - Projection does not contain or return compliance/enforcement/
//!   readiness-to-ship claims.

use bicameral_api::candidate::{DecisionCandidate, DecisionLevel};
use bicameral_api::dashboard::SignoffState;
use bicameral_api::event_store::{EventStoreAdapter, EventStoreEntry, EventType, IngestPayload};
use bicameral_api::evidence::SourceEvidence;
use bicameral_api::source::{Source, SourceSnapshot};
use bicameral_runtime::event_store_adapters::MemoryAdapter;
use bicameral_runtime::projection::{
    build_projection, rebuild_projection, MemoryProjectionStore, ProjectionStore,
};
use bicameral_runtime::replay::replay_from_adapter;
use chrono::Utc;
use uuid::Uuid;

fn seed_full_lifecycle() -> MemoryAdapter {
    let adapter = MemoryAdapter::new();

    let source = Source {
        uri: "jira://PROJ-42".to_string(),
        source_type: "jira".to_string(),
        label: Some("Project decision".to_string()),
        source_kind: None,
    };

    let snapshot = SourceSnapshot {
        snapshot_addr: "sha256:lifecycle".to_string(),
        snapshot_ref: "jira://PROJ-42".to_string(),
        content_hash: Some("lifecycle-hash".to_string()),
        content: None,
        capture_method: None,
        captured_at: Utc::now(),
    };

    let evidence = SourceEvidence::new(
        "sha256:lifecycle".to_string(),
        10,
        200,
        "All widgets must use the shared theme provider".to_string(),
        0.95,
    );
    let evidence_id = evidence.id;

    let mut candidate = DecisionCandidate::new(
        "Use shared theme provider".to_string(),
        "All widgets must use the shared theme provider for consistency".to_string(),
        DecisionLevel::Product,
        "jira://PROJ-42".to_string(),
    );
    candidate.evidence_refs = vec![evidence_id];
    let candidate_id = candidate.id;

    let ingest_payload = IngestPayload {
        source,
        snapshot: Some(snapshot),
        evidence: vec![evidence],
        candidate,
        evidence_references: vec![],
        provenance: None,
    };

    adapter
        .append(EventStoreEntry::new(
            EventType::Ingest,
            serde_json::to_value(&ingest_payload).unwrap(),
            "extractor".to_string(),
        ))
        .unwrap();

    let decision_id = Uuid::new_v4();
    adapter
        .append(EventStoreEntry::new(
            EventType::AcceptCandidate,
            serde_json::json!({
                "candidate_id": candidate_id.to_string(),
                "decision_id": decision_id.to_string(),
            }),
            "governance".to_string(),
        ))
        .unwrap();

    adapter
        .append(EventStoreEntry::new(
            EventType::ApproveSignoff,
            serde_json::json!({
                "decision_id": decision_id.to_string(),
            }),
            "tech-lead".to_string(),
        ))
        .unwrap();

    adapter
}

#[test]
fn rebuild_from_replay_preserves_identical_read_output() {
    let adapter = seed_full_lifecycle();
    let replayed = replay_from_adapter(&adapter).unwrap();

    let store = MemoryProjectionStore::new();

    // Build initial projection.
    let first = rebuild_projection(&store, &replayed).unwrap();
    let first_read = store.read().unwrap();

    // Verify content exists.
    assert_eq!(first.decisions.len(), 1);
    assert!(!first.source_evidence.is_empty());
    assert!(!first.ledger_events.is_empty());
    assert!(!first.graph_snapshots.is_empty());

    // Clear and rebuild — must produce identical output.
    store.clear().unwrap();
    let empty = store.read().unwrap();
    assert!(empty.decisions.is_empty(), "clear must empty all rows");

    let second = rebuild_projection(&store, &replayed).unwrap();
    let second_read = store.read().unwrap();

    assert_eq!(
        first, second,
        "rebuild must produce identical projection state"
    );
    assert_eq!(
        first_read, second_read,
        "read after rebuild must match original read"
    );
}

#[test]
fn multiple_rebuilds_are_idempotent() {
    let adapter = seed_full_lifecycle();
    let replayed = replay_from_adapter(&adapter).unwrap();
    let store = MemoryProjectionStore::new();

    let mut previous = rebuild_projection(&store, &replayed).unwrap();
    for _ in 0..5 {
        let current = rebuild_projection(&store, &replayed).unwrap();
        assert_eq!(previous, current, "every rebuild must be identical");
        previous = current;
    }
}

#[test]
fn projection_signoff_state_matches_replayed_decision() {
    let adapter = seed_full_lifecycle();
    let replayed = replay_from_adapter(&adapter).unwrap();
    let proj = build_projection(&replayed);

    for (id, pd) in &proj.decisions {
        let rd = replayed.decisions.get(id).expect("replayed decision");
        assert_eq!(pd.signoff, rd.signoff_state);
        assert_eq!(pd.review_state, rd.review_state);
        assert_eq!(pd.tracking_state, rd.tracking_state);
    }
}

#[test]
fn projection_excludes_compliance_and_enforcement() {
    let adapter = seed_full_lifecycle();
    let replayed = replay_from_adapter(&adapter).unwrap();
    let proj = build_projection(&replayed);

    let json = serde_json::to_string_pretty(&proj).unwrap();
    let forbidden = [
        "compliance_state",
        "compliance_verdict",
        "enforcement",
        "readiness_to_ship",
        "merge_safety",
        "blocking_enforcement",
        "implementation_correctness",
        "feature_readiness",
    ];
    for term in &forbidden {
        assert!(
            !json.contains(term),
            "projection JSON must not contain '{term}'"
        );
    }
}

#[test]
fn projection_decision_signoff_approved() {
    let adapter = seed_full_lifecycle();
    let replayed = replay_from_adapter(&adapter).unwrap();
    let proj = build_projection(&replayed);

    let decision = proj.decisions.values().next().unwrap();
    assert_eq!(decision.signoff, SignoffState::Approved);
    assert_eq!(decision.title, "Use shared theme provider");
}

#[test]
fn projection_source_evidence_links_to_decision() {
    let adapter = seed_full_lifecycle();
    let replayed = replay_from_adapter(&adapter).unwrap();
    let proj = build_projection(&replayed);

    let decision_id = *proj.decisions.keys().next().unwrap();
    for ev in proj.source_evidence.values() {
        assert_eq!(
            ev.decision_id, decision_id,
            "source evidence must link to its Decision"
        );
    }
}

#[test]
fn projection_event_history_preserves_actor_chain() {
    let adapter = seed_full_lifecycle();
    let replayed = replay_from_adapter(&adapter).unwrap();
    let proj = build_projection(&replayed);

    let actors: Vec<&str> = proj
        .ledger_events
        .iter()
        .map(|e| e.actor.as_str())
        .collect();
    assert_eq!(actors, vec!["extractor", "governance", "tech-lead"]);
}
