//! V1 reviewable-context projection — rebuildable read model for Ledger View.
//!
//! Per ADR-0022 and the `v1-reviewable-context-projection` spec:
//!
//! - The event store remains the canonical authority for accepted governance
//!   events.
//! - This projection is disposable and rebuildable from event-store replay
//!   plus current source/graph indexes.
//! - The projection materializes Decisions, source evidence, code evidence,
//!   graph snapshot provenance, graph readiness, and ledger event history.
//! - The projection must NOT materialize or expose compliance verdicts,
//!   implementation correctness, feature readiness, merge safety, blocking
//!   enforcement, or policy-expanded authority.

pub use bicameral_api::projection::{
    ProjectedCodeEvidence, ProjectedDecision, ProjectedGraphReadiness, ProjectedGraphSnapshot,
    ProjectedLedgerEvent, ProjectedSourceEvidence, ProjectionState,
};

use bicameral_api::candidate::ReviewLifecycleState;
use bicameral_api::event_store::{EventStoreEntry, EventType};
use bicameral_api::evidence::GraphEvidenceState;
use bicameral_api::source::SourceFreshness;

use crate::replay::ReplayedState;

// ── Projection store trait ───────────────────────────────────────

/// Substrate-neutral projection store trait.
///
/// Implementations may use in-memory storage (for deterministic tests),
/// SurrealDB, or other local stores. The projection is always rebuildable
/// from event-store replay — the store is disposable.
pub trait ProjectionStore: Send + Sync {
    /// Replace the entire projection state.
    fn store(&self, state: ProjectionState) -> Result<(), ProjectionError>;

    /// Read the entire projection state.
    fn read(&self) -> Result<ProjectionState, ProjectionError>;

    /// Clear all projection rows.
    fn clear(&self) -> Result<(), ProjectionError>;
}

/// Errors from projection operations.
#[derive(Debug, thiserror::Error)]
pub enum ProjectionError {
    #[error("projection store I/O error: {reason}")]
    Io { reason: String },
}

// ── In-memory projection store (deterministic local tests) ───────

/// In-memory projection store for deterministic local tests.
///
/// This satisfies the E2E acceptance slice without requiring SurrealDB.
/// Future implementations may use embedded SurrealDB or other local stores.
pub struct MemoryProjectionStore {
    state: std::sync::Mutex<ProjectionState>,
}

impl MemoryProjectionStore {
    pub fn new() -> Self {
        Self {
            state: std::sync::Mutex::new(ProjectionState::default()),
        }
    }
}

impl Default for MemoryProjectionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectionStore for MemoryProjectionStore {
    fn store(&self, state: ProjectionState) -> Result<(), ProjectionError> {
        let mut guard = self.state.lock().map_err(|e| ProjectionError::Io {
            reason: e.to_string(),
        })?;
        *guard = state;
        Ok(())
    }

    fn read(&self) -> Result<ProjectionState, ProjectionError> {
        let guard = self.state.lock().map_err(|e| ProjectionError::Io {
            reason: e.to_string(),
        })?;
        Ok(guard.clone())
    }

    fn clear(&self) -> Result<(), ProjectionError> {
        let mut guard = self.state.lock().map_err(|e| ProjectionError::Io {
            reason: e.to_string(),
        })?;
        *guard = ProjectionState::default();
        Ok(())
    }
}

// ── Projection builder ───────────────────────────────────────────

/// Build the V1 reviewable-context projection from replayed event-store state.
///
/// Maps `ReplayedState` (the canonical event-store replay) into the
/// projection record families defined by the `v1-reviewable-context-projection`
/// spec. The projection contains only signoff/review state, evidence, graph
/// readiness, and event history — no compliance, enforcement, or readiness
/// claims.
pub fn build_projection(replayed: &ReplayedState) -> ProjectionState {
    let mut state = ProjectionState::default();

    // Decisions — signoff/review state only.
    for (id, d) in &replayed.decisions {
        let source_freshness = if d.review_state == ReviewLifecycleState::StaleSourcePending {
            SourceFreshness::Stale
        } else {
            SourceFreshness::Fresh
        };

        state.decisions.insert(
            *id,
            ProjectedDecision {
                id: d.id,
                candidate_id: d.candidate_id,
                title: d.title.clone(),
                body: d.body.clone(),
                level: d.level,
                signoff: d.signoff_state,
                review_state: d.review_state,
                tracking_state: d.tracking_state,
                source_id: d.source_id.clone(),
                source_freshness,
                promoted_at: d.promoted_at,
                promoted_by: d.promoted_by.clone(),
                provenance: d.provenance.clone(),
            },
        );
    }

    // Source evidence — linked to Decisions via evidence_refs.
    for (id, d) in &replayed.decisions {
        for ev_id in &d.evidence_refs {
            if let Some(ev) = replayed.evidence.get(ev_id) {
                state.source_evidence.insert(
                    ev.id,
                    ProjectedSourceEvidence {
                        id: ev.id,
                        decision_id: *id,
                        snapshot_id: ev.snapshot_id.clone(),
                        excerpt: ev.excerpt.clone(),
                        span_start: ev.span_start,
                        span_end: ev.span_end,
                        confidence: ev.confidence,
                        captured_at: ev.captured_at,
                    },
                );
            }
        }
    }

    // Code evidence — from accepted bindings.
    for (decision_id, bindings) in &replayed.bindings {
        for binding in bindings {
            let (file, start_line, end_line) = parse_binding_target(&binding.target);
            state.code_evidence.insert(
                binding.id,
                ProjectedCodeEvidence {
                    id: binding.id,
                    decision_id: *decision_id,
                    file,
                    start_line,
                    end_line,
                    binding_type: format!("{:?}", binding.binding_type),
                    explanation: binding.explanation.clone(),
                    evidence_state: GraphEvidenceState::Verified,
                },
            );
        }
    }

    // Graph snapshots — from source snapshots in replayed state.
    for (addr, snap) in &replayed.snapshots {
        state.graph_snapshots.insert(
            addr.clone(),
            ProjectedGraphSnapshot {
                snapshot_addr: snap.snapshot_addr.clone(),
                snapshot_ref: snap.snapshot_ref.clone(),
                captured_at: Some(snap.captured_at),
            },
        );
    }

    // Graph readiness — per Decision, based on binding presence.
    for id in replayed.decisions.keys() {
        let has_bindings = replayed
            .bindings
            .get(id)
            .map(|bs| !bs.is_empty())
            .unwrap_or(false);
        let evidence_state = if has_bindings {
            GraphEvidenceState::Verified
        } else {
            GraphEvidenceState::UnknownNotIndexed
        };
        state.graph_readiness.insert(
            *id,
            ProjectedGraphReadiness {
                decision_id: *id,
                evidence_state,
                has_bindings,
            },
        );
    }

    // Ledger events — event history for the visible read model.
    for entry in &replayed.events {
        let summary = summarize_event(entry);
        state.ledger_events.push(ProjectedLedgerEvent {
            id: entry.id,
            event_type: entry.event_type,
            timestamp: entry.timestamp,
            actor: entry.actor.clone(),
            summary,
        });
    }

    state
}

/// Rebuild the projection: clear the store, then build and store fresh state.
///
/// This is the core idempotency operation: deleting all projection rows
/// and rebuilding from event-store replay must produce the same read output.
pub fn rebuild_projection(
    store: &dyn ProjectionStore,
    replayed: &ReplayedState,
) -> Result<ProjectionState, ProjectionError> {
    store.clear()?;
    let state = build_projection(replayed);
    store.store(state.clone())?;
    Ok(state)
}

// ── Helpers ──────────────────────────────────────────────────────

/// Parse a `BindingEvidence.target` into (file, start_line, end_line).
fn parse_binding_target(target: &str) -> (String, u32, u32) {
    let parts: Vec<&str> = target.rsplitn(3, ':').collect();
    match parts.len() {
        3 => {
            let end = parts[0].parse().unwrap_or(0);
            let start = parts[1].parse().unwrap_or(0);
            let file = parts[2].to_string();
            (file, start, end)
        }
        2 => {
            let line = parts[0].parse().unwrap_or(0);
            let file = parts[1].to_string();
            (file, line, line)
        }
        _ => (target.to_string(), 0, 0),
    }
}

/// Produce a human-readable summary line for a ledger event.
fn summarize_event(entry: &EventStoreEntry) -> String {
    match entry.event_type {
        EventType::Ingest => format!("Candidate ingested by {}", entry.actor),
        EventType::AcceptCandidate => format!("Candidate accepted → Decision by {}", entry.actor),
        EventType::RejectCandidate => format!("Candidate rejected by {}", entry.actor),
        EventType::ApproveSignoff => format!("Signoff approved by {}", entry.actor),
        EventType::RejectSignoff => format!("Signoff rejected by {}", entry.actor),
        EventType::BindToCode => format!("Bound to code by {}", entry.actor),
        EventType::SupersedeDecision => format!("Decision superseded by {}", entry.actor),
        EventType::ResolveCompliance => format!("Compliance resolved by {}", entry.actor),
        EventType::RevalidateBinding => format!("Binding revalidated by {}", entry.actor),
        EventType::UntrackSource => format!("Source untracked by {}", entry.actor),
        EventType::ExternalIngest => {
            format!("External adapter evidence ingested by {}", entry.actor)
        }
        EventType::EvidenceAccepted => {
            format!("Screened external evidence accepted by {}", entry.actor)
        }
        EventType::EditCandidate => format!("Candidate edited by {}", entry.actor),
        EventType::PromoteToCanonical => {
            format!(
                "Decision promoted to canonical constraint by {}",
                entry.actor
            )
        }
        EventType::ClassifyConstraint => {
            format!("Constraint classified by {}", entry.actor)
        }
        EventType::SupersedeConstraint => {
            format!("Constraint superseded by {}", entry.actor)
        }
        EventType::RaiseContradiction => {
            format!("Contradiction finding raised by {}", entry.actor)
        }
        EventType::TransitionFinding => {
            format!("Finding status transitioned by {}", entry.actor)
        }
        EventType::RecordSemanticMapping => {
            format!("Semantic mapping recorded by {}", entry.actor)
        }
        EventType::RaiseCorrectionFinding => {
            format!("Correction finding raised by {}", entry.actor)
        }
        EventType::RequestCorrection => {
            format!("Correction request submitted by {}", entry.actor)
        }
        EventType::CreateProduct => format!("Product created by {}", entry.actor),
        EventType::ProductMemberAdded => {
            format!("Product Member auto-join granted by {}", entry.actor)
        }
        EventType::ProductAutoJoinPolicySet => {
            format!("Product auto-join policy set by {}", entry.actor)
        }
        EventType::ProjectRegistrationWritten => {
            format!("Project registration written by {}", entry.actor)
        }
        EventType::DecisionAccepted => format!("Decision accepted (legacy) by {}", entry.actor),
        EventType::DecisionRejected => format!("Decision rejected (legacy) by {}", entry.actor),
        EventType::EvidenceAttached => format!("Evidence attached (legacy) by {}", entry.actor),
        EventType::ReviewCompleted => format!("Review completed (legacy) by {}", entry.actor),
        EventType::PolicyUpdated => format!("Policy updated (legacy) by {}", entry.actor),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_store_adapters::MemoryAdapter;
    use crate::replay::replay_from_adapter;
    use bicameral_api::candidate::{DecisionCandidate, DecisionLevel};
    use bicameral_api::dashboard::SignoffState;
    use bicameral_api::event_store::{
        EventStoreAdapter, EventStoreEntry, EventType, IngestPayload,
    };
    use bicameral_api::evidence::SourceEvidence;
    use bicameral_api::source::{Source, SourceSnapshot};
    use chrono::Utc;
    use uuid::Uuid;

    /// Seed an event store with a complete ingest→accept→approve flow.
    fn seed_full_flow() -> (MemoryAdapter, Uuid, Uuid) {
        let adapter = MemoryAdapter::new();

        let source = Source {
            uri: "jira://TEST-100".to_string(),
            source_type: "jira".to_string(),
            label: None,
            source_kind: None,
        };

        let snapshot = SourceSnapshot {
            snapshot_addr: "sha256:abc123".to_string(),
            snapshot_ref: "jira://TEST-100".to_string(),
            content_hash: Some("abc123".to_string()),
            content: None,
            capture_method: None,
            captured_at: Utc::now(),
        };

        let evidence = SourceEvidence::new(
            "sha256:abc123".to_string(),
            0,
            100,
            "Test excerpt for projection".to_string(),
            0.9,
        );
        let evidence_id = evidence.id;

        let mut candidate = DecisionCandidate::new(
            "Test Decision".to_string(),
            "Implement widget foo".to_string(),
            DecisionLevel::Product,
            "jira://TEST-100".to_string(),
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
                "test-actor".to_string(),
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
                "test-actor".to_string(),
            ))
            .unwrap();

        adapter
            .append(EventStoreEntry::new(
                EventType::ApproveSignoff,
                serde_json::json!({
                    "decision_id": decision_id.to_string(),
                }),
                "reviewer".to_string(),
            ))
            .unwrap();

        (adapter, candidate_id, decision_id)
    }

    #[test]
    fn build_projection_contains_decisions_and_evidence() {
        let (adapter, _cid, did) = seed_full_flow();
        let replayed = replay_from_adapter(&adapter).unwrap();
        let proj = build_projection(&replayed);

        assert_eq!(proj.decisions.len(), 1);
        let pd = proj.decisions.get(&did).unwrap();
        assert_eq!(pd.title, "Test Decision");
        assert_eq!(pd.signoff, SignoffState::Approved);
        assert_eq!(
            pd.tracking_state,
            bicameral_api::candidate::TrackingState::Tracked
        );

        assert!(!proj.source_evidence.is_empty());
        assert!(!proj.ledger_events.is_empty());
    }

    #[test]
    fn projection_does_not_expose_compliance_or_enforcement() {
        let (adapter, _cid, _did) = seed_full_flow();
        let replayed = replay_from_adapter(&adapter).unwrap();
        let proj = build_projection(&replayed);

        let serialized = serde_json::to_string(&proj).unwrap();
        assert!(
            !serialized.contains("\"compliance_state\""),
            "projection must not expose compliance_state"
        );
        assert!(
            !serialized.contains("\"enforcement\""),
            "projection must not expose enforcement"
        );
    }

    #[test]
    fn rebuild_projection_preserves_read_output() {
        let (adapter, _cid, _did) = seed_full_flow();
        let replayed = replay_from_adapter(&adapter).unwrap();

        let store = MemoryProjectionStore::new();

        let first = rebuild_projection(&store, &replayed).unwrap();
        let first_read = store.read().unwrap();
        assert_eq!(first, first_read);

        let second = rebuild_projection(&store, &replayed).unwrap();
        let second_read = store.read().unwrap();

        assert_eq!(first, second, "rebuild must produce identical projection");
        assert_eq!(
            first_read, second_read,
            "read after rebuild must match original"
        );
    }

    #[test]
    fn clear_removes_all_projection_rows() {
        let (adapter, _cid, _did) = seed_full_flow();
        let replayed = replay_from_adapter(&adapter).unwrap();

        let store = MemoryProjectionStore::new();
        rebuild_projection(&store, &replayed).unwrap();

        let before = store.read().unwrap();
        assert!(!before.decisions.is_empty());

        store.clear().unwrap();
        let after = store.read().unwrap();
        assert!(after.decisions.is_empty());
        assert!(after.source_evidence.is_empty());
        assert!(after.ledger_events.is_empty());
    }

    #[test]
    fn projection_contains_graph_readiness_per_decision() {
        let (adapter, _cid, did) = seed_full_flow();
        let replayed = replay_from_adapter(&adapter).unwrap();
        let proj = build_projection(&replayed);

        assert!(proj.graph_readiness.contains_key(&did));
        let gr = proj.graph_readiness.get(&did).unwrap();
        assert_eq!(gr.decision_id, did);
        assert!(!gr.has_bindings);
    }

    #[test]
    fn projection_contains_graph_snapshots() {
        let (adapter, _cid, _did) = seed_full_flow();
        let replayed = replay_from_adapter(&adapter).unwrap();
        let proj = build_projection(&replayed);

        assert!(proj.graph_snapshots.contains_key("sha256:abc123"));
    }

    #[test]
    fn projection_event_history_covers_full_flow() {
        let (adapter, _cid, _did) = seed_full_flow();
        let replayed = replay_from_adapter(&adapter).unwrap();
        let proj = build_projection(&replayed);

        assert_eq!(proj.ledger_events.len(), 3);
        assert!(proj.ledger_events[0].summary.contains("ingested"));
        assert!(proj.ledger_events[1].summary.contains("accepted"));
        assert!(proj.ledger_events[2].summary.contains("approved"));
    }

    #[test]
    fn parse_binding_target_splits_correctly() {
        assert_eq!(
            parse_binding_target("src/lib.rs:10:20"),
            ("src/lib.rs".to_string(), 10, 20)
        );
        assert_eq!(
            parse_binding_target("src/lib.rs:42"),
            ("src/lib.rs".to_string(), 42, 42)
        );
        assert_eq!(
            parse_binding_target("src/lib.rs"),
            ("src/lib.rs".to_string(), 0, 0)
        );
    }
}
