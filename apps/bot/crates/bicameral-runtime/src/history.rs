//! History query surface — reads replayed/materialized decision state.
//!
//! Implements the source-only `bicameral.history` contract from MCP v0.2 spec
//! §2.2. Returns decisions, candidates, and optionally events from the
//! replayed event store. No grounding or compliance claims are made.

use crate::replay::ReplayedState;
use bicameral_api::candidate::Decision;
use bicameral_api::dashboard::ComplianceState;
use bicameral_api::event_store::EventStoreEntry;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// History query input aligned with spec §2.2 `bicameral.history`.
#[derive(Debug, Clone, Deserialize)]
pub struct HistoryQuery {
    #[serde(default)]
    pub decision_id: Option<Uuid>,
    #[serde(default)]
    pub include_events: bool,
    #[serde(default)]
    pub include_candidates: bool,
    #[serde(default)]
    pub since: Option<DateTime<Utc>>,
}

/// A single decision entry in the history response.
#[derive(Debug, Clone, Serialize)]
pub struct HistoryDecision {
    pub decision_id: Uuid,
    pub candidate_id: Uuid,
    pub title: String,
    pub body: String,
    pub level: bicameral_api::candidate::DecisionLevel,
    pub signoff_state: bicameral_api::dashboard::SignoffState,
    pub compliance_state: ComplianceState,
    pub review_state: bicameral_api::candidate::ReviewLifecycleState,
    pub source_id: Option<String>,
    /// Snapshot identity anchoring the source evidence for this decision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_snapshot_id: Option<String>,
    /// IDs of typed `EvidenceReference` items cited by this decision.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub evidence_reference_ids: Vec<Uuid>,
    /// Stable human-facing deep-link for inspecting this decision's evidence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inspection_uri: Option<String>,
    /// Informational provenance metadata — which surface/actor submitted this.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<bicameral_api::evidence::SubmissionProvenance>,
    pub promoted_at: DateTime<Utc>,
    pub promoted_by: String,
}

impl HistoryDecision {
    /// Construct from a Decision and the replayed state for evidence lookups.
    pub fn from_decision(d: &Decision, state: &ReplayedState) -> Self {
        let source_snapshot_id = Self::resolve_snapshot_id(d, state);
        let inspection_uri = d
            .source_id
            .as_ref()
            .map(|sid| format!("bicameral://evidence/{}/{}", d.id, sid));

        Self {
            decision_id: d.id,
            candidate_id: d.candidate_id,
            title: d.title.clone(),
            body: d.body.clone(),
            level: d.level,
            signoff_state: d.signoff_state,
            compliance_state: d.compliance_state,
            review_state: d.review_state,
            source_id: d.source_id.clone(),
            source_snapshot_id,
            evidence_reference_ids: d.evidence_reference_ids.clone(),
            inspection_uri,
            provenance: d.provenance.clone(),
            promoted_at: d.promoted_at,
            promoted_by: d.promoted_by.clone(),
        }
    }

    fn resolve_snapshot_id(d: &Decision, state: &ReplayedState) -> Option<String> {
        // Try evidence_refs first (legacy SourceEvidence span pointers).
        for ev_id in &d.evidence_refs {
            if let Some(ev) = state.evidence.get(ev_id) {
                return Some(ev.snapshot_id.clone());
            }
        }
        // Fall back to typed EvidenceReference items.
        for er_id in &d.evidence_reference_ids {
            if let Some(er) = state.evidence_references.get(er_id) {
                return Some(er.snapshot_id.clone());
            }
        }
        None
    }
}

/// A single candidate entry in the history response.
#[derive(Debug, Clone, Serialize)]
pub struct HistoryCandidate {
    pub candidate_id: Uuid,
    pub title: String,
    pub description: String,
    pub level: bicameral_api::candidate::DecisionLevel,
    pub candidate_state: bicameral_api::candidate::CandidateState,
    pub source: String,
    /// Snapshot identity anchoring the source evidence for this candidate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_snapshot_id: Option<String>,
    /// IDs of typed `EvidenceReference` items cited by this candidate.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub evidence_reference_ids: Vec<Uuid>,
    /// Stable human-facing deep-link for inspecting this candidate's evidence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inspection_uri: Option<String>,
    /// Informational provenance metadata — which surface/actor submitted this.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<bicameral_api::evidence::SubmissionProvenance>,
}

/// History response aligned with spec §2.2.
#[derive(Debug, Clone, Serialize)]
pub struct HistoryResponse {
    pub decisions: Vec<HistoryDecision>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates: Option<Vec<HistoryCandidate>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events: Option<Vec<EventStoreEntry>>,
}

/// Query history from replayed state.
pub fn query_history(state: &ReplayedState, query: &HistoryQuery) -> HistoryResponse {
    let mut decisions: Vec<HistoryDecision> = if let Some(id) = query.decision_id {
        state
            .decisions
            .get(&id)
            .map(|d| vec![HistoryDecision::from_decision(d, state)])
            .unwrap_or_default()
    } else {
        state
            .decisions
            .values()
            .map(|d| HistoryDecision::from_decision(d, state))
            .collect()
    };

    if let Some(since) = query.since {
        decisions.retain(|d| d.promoted_at >= since);
    }

    decisions.sort_by_key(|d| std::cmp::Reverse(d.promoted_at));

    let candidates = if query.include_candidates {
        let mut cands: Vec<HistoryCandidate> = state
            .candidates
            .values()
            .map(|c| {
                let source_snapshot_id = resolve_candidate_snapshot_id(c, state);
                let inspection_uri = Some(format!(
                    "bicameral://evidence/candidate/{}/{}",
                    c.id, c.source
                ));
                HistoryCandidate {
                    candidate_id: c.id,
                    title: c.title.clone(),
                    description: c.description.clone(),
                    level: c.level,
                    candidate_state: c.candidate_state,
                    source: c.source.clone(),
                    source_snapshot_id,
                    evidence_reference_ids: c.evidence_reference_ids.clone(),
                    inspection_uri,
                    provenance: c.provenance.clone(),
                }
            })
            .collect();
        cands.sort_by(|a, b| a.title.cmp(&b.title));
        Some(cands)
    } else {
        None
    };

    let events = if query.include_events {
        let mut evts = state.events.clone();
        if let Some(since) = query.since {
            evts.retain(|e| e.timestamp >= since);
        }
        Some(evts)
    } else {
        None
    };

    HistoryResponse {
        decisions,
        candidates,
        events,
    }
}

fn resolve_candidate_snapshot_id(
    c: &bicameral_api::candidate::DecisionCandidate,
    state: &ReplayedState,
) -> Option<String> {
    for ev_id in &c.evidence_refs {
        if let Some(ev) = state.evidence.get(ev_id) {
            return Some(ev.snapshot_id.clone());
        }
    }
    for er_id in &c.evidence_reference_ids {
        if let Some(er) = state.evidence_references.get(er_id) {
            return Some(er.snapshot_id.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_store_adapters::MemoryAdapter;
    use crate::replay::replay_from_adapter;
    use bicameral_api::candidate::{DecisionCandidate, DecisionLevel};
    use bicameral_api::event_store::{
        EventStoreAdapter, EventStoreEntry, EventType, IngestPayload,
    };
    use bicameral_api::evidence::SourceEvidence;
    use bicameral_api::source::Source;

    fn seed_adapter_with_decision() -> (MemoryAdapter, Uuid, Uuid) {
        let adapter = MemoryAdapter::new();

        let source = Source {
            uri: "jira://TEST-1".to_string(),
            source_type: "jira".to_string(),
            label: None,
            source_kind: None,
        };
        let evidence = vec![SourceEvidence::new(
            "inline:jira://TEST-1".to_string(),
            0,
            10,
            "test excerpt".to_string(),
            0.9,
        )];
        let mut candidate = DecisionCandidate::new(
            "Architecture Decision".to_string(),
            "We should use microservices".to_string(),
            DecisionLevel::Architecture,
            "jira://TEST-1".to_string(),
        );
        candidate.evidence_refs = evidence.iter().map(|e| e.id).collect();
        let cand_id = candidate.id;

        let payload = IngestPayload {
            source,
            snapshot: None,
            evidence,
            candidate,
            evidence_references: Vec::new(),
            provenance: None,
        };
        adapter
            .append(EventStoreEntry::new(
                EventType::Ingest,
                serde_json::to_value(&payload).unwrap(),
                "test".to_string(),
            ))
            .unwrap();

        let decision_id = Uuid::new_v4();
        adapter
            .append(EventStoreEntry::new(
                EventType::AcceptCandidate,
                serde_json::json!({
                    "candidate_id": cand_id.to_string(),
                    "decision_id": decision_id.to_string(),
                }),
                "test".to_string(),
            ))
            .unwrap();

        (adapter, cand_id, decision_id)
    }

    #[test]
    fn history_returns_all_decisions() {
        let (adapter, _, _) = seed_adapter_with_decision();
        let state = replay_from_adapter(&adapter).unwrap();

        let resp = query_history(
            &state,
            &HistoryQuery {
                decision_id: None,
                include_events: false,
                include_candidates: false,
                since: None,
            },
        );

        assert_eq!(resp.decisions.len(), 1);
        assert_eq!(resp.decisions[0].title, "Architecture Decision");
        assert_eq!(
            resp.decisions[0].compliance_state,
            ComplianceState::Ungrounded
        );
        assert!(resp.candidates.is_none());
        assert!(resp.events.is_none());
    }

    #[test]
    fn history_filters_by_decision_id() {
        let (adapter, _, decision_id) = seed_adapter_with_decision();
        let state = replay_from_adapter(&adapter).unwrap();

        let resp = query_history(
            &state,
            &HistoryQuery {
                decision_id: Some(decision_id),
                include_events: false,
                include_candidates: false,
                since: None,
            },
        );
        assert_eq!(resp.decisions.len(), 1);

        let resp_miss = query_history(
            &state,
            &HistoryQuery {
                decision_id: Some(Uuid::new_v4()),
                include_events: false,
                include_candidates: false,
                since: None,
            },
        );
        assert!(resp_miss.decisions.is_empty());
    }

    #[test]
    fn history_includes_candidates_when_requested() {
        let (adapter, _, _) = seed_adapter_with_decision();
        let state = replay_from_adapter(&adapter).unwrap();

        let resp = query_history(
            &state,
            &HistoryQuery {
                decision_id: None,
                include_events: false,
                include_candidates: true,
                since: None,
            },
        );
        assert!(resp.candidates.is_some());
        assert_eq!(resp.candidates.unwrap().len(), 1);
    }

    #[test]
    fn history_includes_events_when_requested() {
        let (adapter, _, _) = seed_adapter_with_decision();
        let state = replay_from_adapter(&adapter).unwrap();

        let resp = query_history(
            &state,
            &HistoryQuery {
                decision_id: None,
                include_events: true,
                include_candidates: false,
                since: None,
            },
        );
        assert!(resp.events.is_some());
        assert_eq!(resp.events.unwrap().len(), 2);
    }

    #[test]
    fn history_decision_includes_source_evidence_links() {
        let (adapter, _, _) = seed_adapter_with_decision();
        let state = replay_from_adapter(&adapter).unwrap();

        let resp = query_history(
            &state,
            &HistoryQuery {
                decision_id: None,
                include_events: false,
                include_candidates: false,
                since: None,
            },
        );

        assert_eq!(resp.decisions.len(), 1);
        let d = &resp.decisions[0];
        assert_eq!(d.source_id.as_deref(), Some("jira://TEST-1"));
        assert_eq!(
            d.source_snapshot_id.as_deref(),
            Some("inline:jira://TEST-1")
        );
        assert!(d.inspection_uri.is_some());
        assert!(d
            .inspection_uri
            .as_ref()
            .unwrap()
            .starts_with("bicameral://evidence/"));
    }

    #[test]
    fn history_candidate_includes_source_evidence_links() {
        let (adapter, _, _) = seed_adapter_with_decision();
        let state = replay_from_adapter(&adapter).unwrap();

        let resp = query_history(
            &state,
            &HistoryQuery {
                decision_id: None,
                include_events: false,
                include_candidates: true,
                since: None,
            },
        );

        let cands = resp.candidates.unwrap();
        assert_eq!(cands.len(), 1);
        let c = &cands[0];
        assert_eq!(c.source, "jira://TEST-1");
        assert_eq!(
            c.source_snapshot_id.as_deref(),
            Some("inline:jira://TEST-1")
        );
        assert!(!c.evidence_reference_ids.is_empty() || c.source_snapshot_id.is_some());
        assert!(c.inspection_uri.is_some());
        assert!(c.inspection_uri.as_ref().unwrap().contains("candidate"));
    }

    #[test]
    fn history_with_evidence_references_survives_replay() {
        use bicameral_api::evidence::EvidenceReference;
        use bicameral_api::source::SourceKind;

        let adapter = MemoryAdapter::new();
        let er = EvidenceReference::new(
            "snapshot-abc".to_string(),
            SourceKind::McpSession,
            "MCP session excerpt".to_string(),
        );
        let er_id = er.id;

        let source = Source {
            uri: "mcp://session-1".to_string(),
            source_type: "mcp".to_string(),
            label: None,
            source_kind: Some(SourceKind::McpSession),
        };
        let mut candidate = DecisionCandidate::new(
            "MCP Decision".to_string(),
            "From MCP session".to_string(),
            DecisionLevel::Architecture,
            "mcp://session-1".to_string(),
        );
        candidate.evidence_reference_ids = vec![er_id];
        let cand_id = candidate.id;

        let payload = IngestPayload {
            source,
            snapshot: None,
            evidence: Vec::new(),
            candidate,
            evidence_references: vec![er],
            provenance: None,
        };
        adapter
            .append(EventStoreEntry::new(
                EventType::Ingest,
                serde_json::to_value(&payload).unwrap(),
                "test".to_string(),
            ))
            .unwrap();

        let decision_id = Uuid::new_v4();
        adapter
            .append(EventStoreEntry::new(
                EventType::AcceptCandidate,
                serde_json::json!({
                    "candidate_id": cand_id.to_string(),
                    "decision_id": decision_id.to_string(),
                }),
                "test".to_string(),
            ))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();

        // Verify evidence_references stored in replay state
        assert!(state.evidence_references.contains_key(&er_id));

        let resp = query_history(
            &state,
            &HistoryQuery {
                decision_id: None,
                include_events: false,
                include_candidates: true,
                since: None,
            },
        );

        assert_eq!(resp.decisions.len(), 1);
        let d = &resp.decisions[0];
        assert_eq!(d.evidence_reference_ids, vec![er_id]);
        assert_eq!(d.source_snapshot_id.as_deref(), Some("snapshot-abc"));
        assert!(d.inspection_uri.is_some());

        let cands = resp.candidates.unwrap();
        assert_eq!(cands.len(), 1);
        let c = &cands[0];
        assert_eq!(c.evidence_reference_ids, vec![er_id]);
        assert_eq!(c.source_snapshot_id.as_deref(), Some("snapshot-abc"));
    }

    #[test]
    fn history_no_results_returns_empty() {
        let adapter = MemoryAdapter::new();
        let state = replay_from_adapter(&adapter).unwrap();

        let resp = query_history(
            &state,
            &HistoryQuery {
                decision_id: None,
                include_events: false,
                include_candidates: true,
                since: None,
            },
        );

        assert!(resp.decisions.is_empty());
        assert_eq!(resp.candidates.unwrap().len(), 0);
    }

    #[test]
    fn history_surfaces_provenance_from_tool_request() {
        let adapter = MemoryAdapter::new();
        let provenance = bicameral_api::evidence::SubmissionProvenance {
            submitting_surface: Some("tool_request".to_string()),
            source_kind: Some("jira".to_string()),
            actor_id: Some("agent-1".to_string()),
            session_id: Some("sess-1".to_string()),
            request_id: Some("req-1".to_string()),
            source_uri: Some("jira://TEST-2".to_string()),
            submitted_at: chrono::Utc::now(),
        };
        let mut candidate = DecisionCandidate::new(
            "Provenance test".to_string(),
            "test body".to_string(),
            DecisionLevel::Implementation,
            "jira://TEST-2".to_string(),
        );
        candidate.provenance = Some(provenance.clone());
        let cand_id = candidate.id;

        let source = Source {
            uri: "jira://TEST-2".to_string(),
            source_type: "jira".to_string(),
            label: None,
            source_kind: None,
        };
        let payload = IngestPayload {
            source,
            snapshot: None,
            evidence: Vec::new(),
            candidate,
            evidence_references: Vec::new(),
            provenance: Some(provenance),
        };
        adapter
            .append(EventStoreEntry::new(
                EventType::Ingest,
                serde_json::to_value(&payload).unwrap(),
                "test".to_string(),
            ))
            .unwrap();

        let decision_id = Uuid::new_v4();
        adapter
            .append(EventStoreEntry::new(
                EventType::AcceptCandidate,
                serde_json::json!({
                    "candidate_id": cand_id.to_string(),
                    "decision_id": decision_id.to_string(),
                }),
                "test".to_string(),
            ))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();
        let resp = query_history(
            &state,
            &HistoryQuery {
                decision_id: None,
                include_events: false,
                include_candidates: true,
                since: None,
            },
        );

        // Decision should have provenance
        assert_eq!(resp.decisions.len(), 1);
        let d = &resp.decisions[0];
        assert!(d.provenance.is_some());
        let dp = d.provenance.as_ref().unwrap();
        assert_eq!(dp.actor_id.as_deref(), Some("agent-1"));

        // Candidate should have provenance
        let cands = resp.candidates.unwrap();
        assert_eq!(cands.len(), 1);
        let c = &cands[0];
        assert!(c.provenance.is_some());
        assert_eq!(
            c.provenance.as_ref().unwrap().submitting_surface.as_deref(),
            Some("tool_request")
        );
    }

    #[test]
    fn history_degrades_gracefully_without_provenance() {
        let (adapter, _, _) = seed_adapter_with_decision();
        let state = replay_from_adapter(&adapter).unwrap();

        let resp = query_history(
            &state,
            &HistoryQuery {
                decision_id: None,
                include_events: false,
                include_candidates: true,
                since: None,
            },
        );

        // Without provenance, field should be None
        assert_eq!(resp.decisions.len(), 1);
        assert!(resp.decisions[0].provenance.is_none());

        let cands = resp.candidates.unwrap();
        assert_eq!(cands.len(), 1);
        assert!(cands[0].provenance.is_none());
    }
}
