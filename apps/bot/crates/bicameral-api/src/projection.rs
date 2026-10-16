//! V1 reviewable-context projection types — rebuildable read model.
//!
//! Per ADR-0022 and the `v1-reviewable-context-projection` spec:
//!
//! - The event store remains the canonical authority.
//! - This projection is disposable and rebuildable from event-store replay.
//! - The projection must NOT expose compliance verdicts, implementation
//!   correctness, feature readiness, merge safety, or blocking enforcement.

use crate::candidate::{DecisionLevel, ReviewLifecycleState, TrackingState};
use crate::dashboard::SignoffState;
use crate::event_store::EventType;
use crate::evidence::GraphEvidenceState;
use crate::source::SourceFreshness;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// A Decision row in the projection — signoff/review state only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectedDecision {
    pub id: Uuid,
    pub candidate_id: Uuid,
    pub title: String,
    pub body: String,
    pub level: DecisionLevel,
    pub signoff: SignoffState,
    pub review_state: ReviewLifecycleState,
    pub tracking_state: TrackingState,
    pub source_id: Option<String>,
    pub source_freshness: SourceFreshness,
    pub promoted_at: DateTime<Utc>,
    pub promoted_by: String,
    /// Informational submission provenance carried from the Decision (#389), so
    /// the projection-backed detail endpoint surfaces it with the same fidelity
    /// as the in-memory path (decision-retrieval surface parity, #387). Never
    /// creates authority; absent (`None`) when unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<crate::evidence::SubmissionProvenance>,
}

/// Source evidence row — cited evidence span and snapshot provenance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectedSourceEvidence {
    pub id: Uuid,
    pub decision_id: Uuid,
    pub snapshot_id: String,
    pub excerpt: String,
    pub span_start: usize,
    pub span_end: usize,
    pub confidence: f64,
    pub captured_at: DateTime<Utc>,
}

/// Code evidence row — accepted bindings and advisory code hints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectedCodeEvidence {
    pub id: Uuid,
    pub decision_id: Uuid,
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    pub binding_type: String,
    pub explanation: Option<String>,
    pub evidence_state: GraphEvidenceState,
}

/// Graph snapshot row — snapshot identity and provenance metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectedGraphSnapshot {
    pub snapshot_addr: String,
    pub snapshot_ref: String,
    pub captured_at: Option<DateTime<Utc>>,
}

/// Graph readiness row — indexing/staleness/readiness state per Decision.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectedGraphReadiness {
    pub decision_id: Uuid,
    pub evidence_state: GraphEvidenceState,
    pub has_bindings: bool,
}

/// Ledger event row — replay/event history for the visible read model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectedLedgerEvent {
    pub id: Uuid,
    pub event_type: EventType,
    pub timestamp: DateTime<Utc>,
    pub actor: String,
    pub summary: String,
}

/// The complete projection state — all record families.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ProjectionState {
    pub decisions: HashMap<Uuid, ProjectedDecision>,
    pub source_evidence: HashMap<Uuid, ProjectedSourceEvidence>,
    pub code_evidence: HashMap<Uuid, ProjectedCodeEvidence>,
    pub graph_snapshots: HashMap<String, ProjectedGraphSnapshot>,
    pub graph_readiness: HashMap<Uuid, ProjectedGraphReadiness>,
    pub ledger_events: Vec<ProjectedLedgerEvent>,
}
