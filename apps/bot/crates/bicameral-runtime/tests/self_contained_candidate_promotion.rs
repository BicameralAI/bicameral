use bicameral_api::candidate::{DecisionCandidate, DecisionLevel};
use bicameral_api::candidate_promotion::CandidatePromotedPayload;
use bicameral_api::event_store::{EventStoreAdapter, EventStoreEntry, EventType};
use bicameral_runtime::event_store_adapters::MemoryAdapter;
use bicameral_runtime::replay::replay_from_adapter;
use serde_json::Map;
use uuid::Uuid;

#[test]
fn promotion_replays_decision_without_candidate_history() {
    let adapter = MemoryAdapter::new();
    let mut candidate = DecisionCandidate::new(
        "Use durable local projection state".to_string(),
        "Projection requests and candidates remain daemon-local until promotion.".to_string(),
        DecisionLevel::Architecture,
        "github://BicameralAI/bicameral-bot/issues/856".to_string(),
    );
    candidate.tags = vec!["projection".to_string(), "sqlite".to_string()];
    let candidate_id = candidate.id;
    let decision_id = Uuid::new_v4();
    let promoted = CandidatePromotedPayload::new(
        candidate.clone(),
        decision_id,
        Some("Reviewed and accepted".to_string()),
        "reviewer:test",
        Map::new(),
    );
    let entry = EventStoreEntry::new(
        EventType::AcceptCandidate,
        serde_json::to_value(promoted).unwrap(),
        "reviewer:test".to_string(),
    );
    adapter.append(entry).unwrap();

    let replayed = replay_from_adapter(&adapter).unwrap();
    assert!(replayed.candidates.is_empty());
    let decision = replayed.decisions.get(&decision_id).unwrap();
    assert_eq!(decision.candidate_id, candidate_id);
    assert_eq!(decision.title, candidate.title);
    assert_eq!(decision.body, candidate.description);
    assert_eq!(decision.evidence_refs, candidate.evidence_refs);
    assert_eq!(decision.provenance, candidate.provenance);
}

#[test]
fn unsupported_promotion_payload_version_fails_closed() {
    let adapter = MemoryAdapter::new();
    let candidate = DecisionCandidate::new(
        "Versioned promotion".to_string(),
        "Unknown payload versions must not be interpreted optimistically.".to_string(),
        DecisionLevel::Implementation,
        "test://promotion-version".to_string(),
    );
    let mut promoted =
        CandidatePromotedPayload::new(candidate, Uuid::new_v4(), None, "reviewer:test", Map::new());
    promoted.version = CandidatePromotedPayload::VERSION + 1;
    adapter
        .append(EventStoreEntry::new(
            EventType::AcceptCandidate,
            serde_json::to_value(promoted).unwrap(),
            "reviewer:test".to_string(),
        ))
        .unwrap();

    let error = replay_from_adapter(&adapter).unwrap_err().to_string();
    assert!(error.contains("unsupported candidate promotion payload version"));
}
