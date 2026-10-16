//! Replay engine — reconstructs materialized state from event-store entries.
//!
//! The replay module reads events through `EventStoreAdapter::replay()` and
//! rebuilds Source, SourceSnapshot, SourceEvidence, and DecisionCandidate
//! collections. This ensures restart/replay reconstructs candidates without
//! relying on in-memory state.

use bicameral_api::candidate::TrackingState;
use bicameral_api::candidate::{CandidateState, Decision, DecisionCandidate, ReviewLifecycleState};
use bicameral_api::catalog::{supersede_constraint, CatalogConstraint};
use bicameral_api::constraint::{CanonicalConstraint, CanonicalReviewState};
use bicameral_api::dashboard::{ComplianceState, SignoffState};
use bicameral_api::drift::{transition_finding, ContradictionReport};
use bicameral_api::event_store::{
    BindToCodePayload, ClassifyConstraintPayload, EventStoreAdapter, EventStoreEntry,
    EventStoreError, EventType, IngestPayload, PromoteToCanonicalPayload,
    RaiseContradictionPayload, RaiseCorrectionFindingPayload, RecordSemanticMappingPayload,
    ResolveCompliancePayload, RevalidateBindingPayload, SupersedeConstraintPayload,
    TransitionFindingPayload, UntrackSourcePayload,
};
use bicameral_api::evidence::{BindingEvidence, EvidenceReference, SourceEvidence};
use bicameral_api::evidence_acceptance::EvidenceAcceptedPayload;
use bicameral_api::grounding::CorrectionFinding;
use bicameral_api::semantic::SemanticMapping;
use bicameral_api::source::{Source, SourceSnapshot};
use std::collections::HashMap;
use uuid::Uuid;

/// Materialized state reconstructed from replaying event-store entries.
#[derive(Debug, Default)]
pub struct ReplayedState {
    pub sources: HashMap<String, Source>,
    pub snapshots: HashMap<String, SourceSnapshot>,
    pub evidence: HashMap<Uuid, SourceEvidence>,
    /// Typed evidence references (MCP sessions, tool calls, command output, etc.)
    /// keyed by their UUID. Populated from `IngestPayload.evidence_references`.
    pub evidence_references: HashMap<Uuid, EvidenceReference>,
    pub candidates: HashMap<Uuid, DecisionCandidate>,
    pub decisions: HashMap<Uuid, Decision>,
    /// Binding evidence keyed by decision id.
    pub bindings: HashMap<Uuid, Vec<BindingEvidence>>,
    /// Content hashes recorded at bind time, keyed by `BindingEvidence.id`.
    /// Used by `ComplianceEvaluator` to detect content drift.
    pub binding_content_hashes: HashMap<Uuid, Option<String>>,
    /// Revalidation history keyed by binding id. Each entry accumulates
    /// lineage/revalidation events without mutating prior records.
    pub binding_revalidations: HashMap<Uuid, Vec<RevalidateBindingPayload>>,
    /// Canonical constraints promoted from reviewed decisions, keyed by
    /// `idempotency_key`. A re-emitted promotion with the same key does not
    /// create a second constraint.
    pub canonical_constraints: HashMap<String, CanonicalConstraint>,
    /// Catalog read model: classified canonical constraints, keyed by the
    /// promotion `idempotency_key`. Built from `ClassifyConstraint` (over a
    /// prior `PromoteToCanonical`) and updated by `SupersedeConstraint`.
    pub catalog: HashMap<String, CatalogConstraint>,
    /// Contradiction findings, keyed by report id. Raised via
    /// `RaiseContradiction` (which also writes catalog `conflicts_with` edges)
    /// and transitioned through review via `TransitionFinding`.
    pub contradictions: HashMap<Uuid, ContradictionReport>,
    /// Durable semantic mappings, keyed by mapping id. A re-recorded mapping
    /// with the same id does not create a duplicate.
    pub semantic_mappings: HashMap<Uuid, SemanticMapping>,
    /// Correction-capture findings, keyed by finding id. A re-raised finding
    /// with the same id does not create a duplicate.
    pub correction_findings: HashMap<Uuid, CorrectionFinding>,
    /// Canonical Products keyed by `product_id` (issue #766). Reconstructed
    /// from `CreateProduct` events alone — Cloud is never consulted as canonical
    /// state. Creator roles + Governance Policy Version travel in the record.
    /// Canonical screened-evidence acceptance facts keyed by idempotency key.
    /// This map carries no projection or candidate working state.
    pub accepted_evidence: HashMap<String, EvidenceAcceptedPayload>,
    pub products: HashMap<String, crate::product_creation::ProductRecord>,
    /// Creation idempotency key -> canonical `product_id`. A re-emitted creation
    /// event with an already-seen key does not materialize a second Product.
    pub product_idempotency: HashMap<String, String>,
    pub events: Vec<EventStoreEntry>,
}

/// Replay all events from an adapter and reconstruct materialized state.
pub fn replay_from_adapter(
    adapter: &dyn EventStoreAdapter,
) -> Result<ReplayedState, EventStoreError> {
    let entries = adapter.replay()?;
    let mut state = ReplayedState::default();

    for entry in entries {
        state.events.push(entry.clone());
        match entry.event_type {
            EventType::EvidenceAccepted => {
                let payload: EvidenceAcceptedPayload = serde_json::from_value(entry.payload)
                    .map_err(|e| EventStoreError::Serialization {
                        reason: format!("failed to deserialize EvidenceAcceptedPayload: {e}"),
                    })?;
                state
                    .accepted_evidence
                    .entry(payload.idempotency_key.clone())
                    .or_insert(payload);
            }
            EventType::Ingest | EventType::ExternalIngest => {
                let payload: IngestPayload =
                    serde_json::from_value(entry.payload).map_err(|e| {
                        EventStoreError::Serialization {
                            reason: format!("failed to deserialize IngestPayload: {e}"),
                        }
                    })?;

                state
                    .sources
                    .insert(payload.source.uri.clone(), payload.source);

                if let Some(snap) = payload.snapshot {
                    state.snapshots.insert(snap.snapshot_addr.clone(), snap);
                }

                for ev in payload.evidence {
                    state.evidence.insert(ev.id, ev);
                }

                for er in payload.evidence_references {
                    state.evidence_references.insert(er.id, er);
                }

                state
                    .candidates
                    .insert(payload.candidate.id, payload.candidate);
            }
            EventType::BindToCode => {
                let payload: BindToCodePayload =
                    serde_json::from_value(entry.payload).map_err(|e| {
                        EventStoreError::Serialization {
                            reason: format!("failed to deserialize BindToCodePayload: {e}"),
                        }
                    })?;

                let binding_id = payload.binding.id;
                state
                    .binding_content_hashes
                    .insert(binding_id, payload.content_hash.clone());
                state
                    .bindings
                    .entry(payload.decision_id)
                    .or_default()
                    .push(payload.binding);
            }
            EventType::AcceptCandidate => {
                if entry.payload.get("candidate").is_some() {
                    let payload: bicameral_api::candidate_promotion::CandidatePromotedPayload =
                        serde_json::from_value(entry.payload).map_err(|error| {
                            EventStoreError::Serialization {
                                reason: format!(
                                    "failed to deserialize self-contained promotion payload: {error}"
                                ),
                            }
                        })?;
                    if payload.version
                        != bicameral_api::candidate_promotion::CandidatePromotedPayload::VERSION
                    {
                        return Err(EventStoreError::Serialization {
                            reason: format!(
                                "unsupported candidate promotion payload version: {}",
                                payload.version
                            ),
                        });
                    }
                    let mut candidate = payload.candidate;
                    candidate.candidate_state = CandidateState::Promoted;
                    candidate.review_state = ReviewLifecycleState::NeedsSignoffReview;
                    let mut decision = Decision::from_candidate(&candidate, payload.promoted_by);
                    decision.id = payload.decision_id;
                    decision.promoted_at = entry.timestamp;
                    decision.compliance_state = ComplianceState::Ungrounded;
                    match payload.initial_signoff_state {
                        None | Some(SignoffState::Proposed) => {}
                        Some(SignoffState::Approved) => {
                            decision.signoff_state = SignoffState::Approved;
                            decision.review_state = ReviewLifecycleState::ReadyToMaterialize;
                        }
                        Some(other) => {
                            return Err(EventStoreError::Serialization {
                                reason: format!(
                                    "invalid initial promotion signoff state: {other:?}"
                                ),
                            });
                        }
                    }
                    state.decisions.insert(decision.id, decision);
                    if let Some(existing) = state.candidates.get_mut(&payload.candidate_id) {
                        existing.candidate_state = CandidateState::Promoted;
                        existing.review_state = ReviewLifecycleState::NeedsSignoffReview;
                    }
                } else {
                    let candidate_id: Uuid = entry
                        .payload
                        .get("candidate_id")
                        .and_then(|v| v.as_str())
                        .and_then(|s| Uuid::parse_str(s).ok())
                        .ok_or_else(|| EventStoreError::Serialization {
                            reason: "AcceptCandidate missing candidate_id".to_string(),
                        })?;
                    let decision_id: Uuid = entry
                        .payload
                        .get("decision_id")
                        .and_then(|v| v.as_str())
                        .and_then(|s| Uuid::parse_str(s).ok())
                        .ok_or_else(|| EventStoreError::Serialization {
                            reason: "AcceptCandidate missing decision_id".to_string(),
                        })?;
                    if let Some(candidate) = state.candidates.get_mut(&candidate_id) {
                        candidate.candidate_state = CandidateState::Promoted;
                        candidate.review_state = ReviewLifecycleState::NeedsSignoffReview;
                        let mut decision = Decision::from_candidate(candidate, entry.actor.clone());
                        decision.id = decision_id;
                        decision.promoted_at = entry.timestamp;
                        decision.compliance_state = ComplianceState::Ungrounded;
                        state.decisions.insert(decision_id, decision);
                    }
                }
            }
            EventType::RejectCandidate => {
                let candidate_id: Uuid = entry
                    .payload
                    .get("candidate_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .ok_or_else(|| EventStoreError::Serialization {
                        reason: "RejectCandidate missing candidate_id".to_string(),
                    })?;

                if let Some(candidate) = state.candidates.get_mut(&candidate_id) {
                    candidate.candidate_state = CandidateState::Rejected;
                    candidate.review_state = ReviewLifecycleState::Materialized;
                }
            }
            EventType::ApproveSignoff => {
                let decision_id: Uuid = entry
                    .payload
                    .get("decision_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .ok_or_else(|| EventStoreError::Serialization {
                        reason: "ApproveSignoff missing decision_id".to_string(),
                    })?;

                if let Some(decision) = state.decisions.get_mut(&decision_id) {
                    decision.signoff_state = SignoffState::Approved;
                    decision.review_state = ReviewLifecycleState::ReadyToMaterialize;
                }
            }
            EventType::RejectSignoff => {
                let decision_id: Uuid = entry
                    .payload
                    .get("decision_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .ok_or_else(|| EventStoreError::Serialization {
                        reason: "RejectSignoff missing decision_id".to_string(),
                    })?;

                if let Some(decision) = state.decisions.get_mut(&decision_id) {
                    decision.signoff_state = SignoffState::Rejected;
                    decision.review_state = ReviewLifecycleState::Materialized;
                }
            }
            EventType::SupersedeDecision => {
                let decision_id: Uuid = entry
                    .payload
                    .get("decision_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .ok_or_else(|| EventStoreError::Serialization {
                        reason: "SupersedeDecision missing decision_id".to_string(),
                    })?;

                if let Some(decision) = state.decisions.get_mut(&decision_id) {
                    decision.signoff_state = SignoffState::Superseded;
                    decision.review_state = ReviewLifecycleState::Materialized;
                }
            }
            EventType::ResolveCompliance => {
                let payload: ResolveCompliancePayload = serde_json::from_value(entry.payload)
                    .map_err(|e| EventStoreError::Serialization {
                        reason: format!("failed to deserialize ResolveCompliancePayload: {e}"),
                    })?;

                if let Some(decision) = state.decisions.get_mut(&payload.decision_id) {
                    decision.compliance_state = payload.compliance_state;
                }
            }
            EventType::RevalidateBinding => {
                let payload: RevalidateBindingPayload = serde_json::from_value(entry.payload)
                    .map_err(|e| EventStoreError::Serialization {
                        reason: format!("failed to deserialize RevalidateBindingPayload: {e}"),
                    })?;

                state
                    .binding_revalidations
                    .entry(payload.binding_id)
                    .or_default()
                    .push(payload);
            }
            EventType::PromoteToCanonical => {
                let payload: PromoteToCanonicalPayload = serde_json::from_value(entry.payload)
                    .map_err(|e| EventStoreError::Serialization {
                        reason: format!("failed to deserialize PromoteToCanonicalPayload: {e}"),
                    })?;

                // Idempotent: a re-emitted promotion with the same key does not
                // create a second constraint.
                if !state
                    .canonical_constraints
                    .contains_key(&payload.idempotency_key)
                {
                    let key = payload.idempotency_key.clone();
                    let constraint = CanonicalConstraint {
                        // Deterministic, replay-stable id derived from the stable
                        // promotion key so downstream references survive replay
                        // (a random v4 id would differ each replay).
                        //
                        // INVARIANT (deliberate): identical derivation to the
                        // ClassifyConstraint arm's `CatalogConstraint.id`
                        // (`v5(NAMESPACE_OID, idempotency_key)`), so for a given
                        // key `CanonicalConstraint.id == CatalogConstraint.id` —
                        // they are 1:1 views of one logical constraint. Do not
                        // change one derivation without the other, and never index
                        // both ids in the same `Uuid`-keyed map.
                        id: Uuid::new_v5(&Uuid::NAMESPACE_OID, key.as_bytes()),
                        decision_id: payload.decision_id,
                        title: payload.title,
                        statement: payload.statement,
                        review_state: CanonicalReviewState::Accepted,
                        evidence_refs: payload.evidence_refs,
                        affected_surfaces: payload.affected_surfaces,
                        reviewer: payload.reviewer,
                        reason: payload.reason,
                        promoted_at: entry.timestamp,
                        idempotency_key: key.clone(),
                    };
                    state.canonical_constraints.insert(key, constraint);
                }
            }
            EventType::ClassifyConstraint => {
                let payload: ClassifyConstraintPayload = serde_json::from_value(entry.payload)
                    .map_err(|e| EventStoreError::Serialization {
                        reason: format!("failed to deserialize ClassifyConstraintPayload: {e}"),
                    })?;

                // A constraint cannot enter the catalog without a prior
                // promotion (#481), and classification is idempotent.
                let already = state.catalog.contains_key(&payload.idempotency_key);
                if let Some(canon) = state.canonical_constraints.get(&payload.idempotency_key) {
                    if !already {
                        let affected_surfaces = payload
                            .affected_surfaces
                            .unwrap_or_else(|| canon.affected_surfaces.clone());
                        let constraint = CatalogConstraint {
                            // Deterministic, replay-stable id derived from the
                            // stable promotion key, so downstream references
                            // (conflicts_with, contradiction related ids) survive
                            // replay. A random v4 id would differ each replay.
                            id: Uuid::new_v5(
                                &Uuid::NAMESPACE_OID,
                                payload.idempotency_key.as_bytes(),
                            ),
                            decision_id: canon.decision_id,
                            statement: canon.statement.clone(),
                            constraint_type: payload.constraint_type,
                            scope: payload.scope,
                            review_state: CanonicalReviewState::Accepted,
                            version: 1,
                            affected_surfaces,
                            evidence_refs: canon.evidence_refs.clone(),
                            attribution: payload.attribution,
                            supersedes: None,
                            superseded_by: None,
                            conflicts_with: Vec::new(),
                            created_at: entry.timestamp,
                            updated_at: entry.timestamp,
                        };
                        state.catalog.insert(payload.idempotency_key, constraint);
                    }
                }
            }
            EventType::SupersedeConstraint => {
                let payload: SupersedeConstraintPayload = serde_json::from_value(entry.payload)
                    .map_err(|e| EventStoreError::Serialization {
                        reason: format!("failed to deserialize SupersedeConstraintPayload: {e}"),
                    })?;

                // Both entries must already be in the catalog; else no-op.
                let current = state.catalog.get(&payload.current_idempotency_key).cloned();
                let replacement = state
                    .catalog
                    .get(&payload.replacement_idempotency_key)
                    .cloned();
                if let (Some(current), Some(replacement)) = (current, replacement) {
                    let (superseded, new_replacement) =
                        supersede_constraint(&current, &replacement, entry.timestamp);
                    state
                        .catalog
                        .insert(payload.current_idempotency_key, superseded);
                    state
                        .catalog
                        .insert(payload.replacement_idempotency_key, new_replacement);
                }
            }
            EventType::RaiseContradiction => {
                let payload: RaiseContradictionPayload = serde_json::from_value(entry.payload)
                    .map_err(|e| EventStoreError::Serialization {
                        reason: format!("failed to deserialize RaiseContradictionPayload: {e}"),
                    })?;

                // Idempotent by report id.
                if !state.contradictions.contains_key(&payload.report.id) {
                    let related = payload.report.related_constraint_ids.clone();
                    // Write conflict edges: each related catalog constraint
                    // (located by `.id`) gains the OTHER related ids.
                    for constraint in state.catalog.values_mut() {
                        if related.contains(&constraint.id) {
                            for other in &related {
                                if *other != constraint.id
                                    && !constraint.conflicts_with.contains(other)
                                {
                                    constraint.conflicts_with.push(*other);
                                }
                            }
                        }
                    }
                    state
                        .contradictions
                        .insert(payload.report.id, payload.report);
                }
            }
            EventType::TransitionFinding => {
                let payload: TransitionFindingPayload = serde_json::from_value(entry.payload)
                    .map_err(|e| EventStoreError::Serialization {
                        reason: format!("failed to deserialize TransitionFindingPayload: {e}"),
                    })?;

                // Apply the HITL transition; an invalid transition is ignored
                // and the finding's status is preserved.
                if let Some(report) = state.contradictions.get(&payload.request.report_id) {
                    if let Ok(updated) = transition_finding(report, &payload.request) {
                        state.contradictions.insert(updated.id, updated);
                    }
                }
            }
            EventType::UntrackSource => {
                let payload: UntrackSourcePayload =
                    serde_json::from_value(entry.payload).map_err(|e| {
                        EventStoreError::Serialization {
                            reason: format!("failed to deserialize UntrackSourcePayload: {e}"),
                        }
                    })?;

                if let Some(decision) = state.decisions.get_mut(&payload.decision_id) {
                    decision.tracking_state = TrackingState::Untracked;
                }
            }
            EventType::RecordSemanticMapping => {
                let payload: RecordSemanticMappingPayload = serde_json::from_value(entry.payload)
                    .map_err(|e| {
                    EventStoreError::Serialization {
                        reason: format!("failed to deserialize RecordSemanticMappingPayload: {e}"),
                    }
                })?;

                // Idempotent by mapping id.
                state
                    .semantic_mappings
                    .entry(payload.mapping.id)
                    .or_insert(payload.mapping);
            }
            EventType::RaiseCorrectionFinding => {
                let payload: RaiseCorrectionFindingPayload = serde_json::from_value(entry.payload)
                    .map_err(|e| EventStoreError::Serialization {
                        reason: format!("failed to deserialize RaiseCorrectionFindingPayload: {e}"),
                    })?;

                // Idempotent by finding id.
                state
                    .correction_findings
                    .entry(payload.finding.id)
                    .or_insert(payload.finding);
            }
            EventType::CreateProduct => {
                let payload: bicameral_api::event_store::ProductCreatedPayload =
                    serde_json::from_value(entry.payload).map_err(|e| {
                        EventStoreError::Serialization {
                            reason: format!("failed to deserialize ProductCreatedPayload: {e}"),
                        }
                    })?;

                // Idempotent by creation key and by product_id: replay restores
                // Product identity + creator authority without duplication.
                if state
                    .product_idempotency
                    .contains_key(&payload.idempotency_key)
                    || state.products.contains_key(&payload.product_id)
                {
                    continue;
                }

                let record = crate::product_creation::ProductRecord {
                    product_id: payload.product_id.clone(),
                    creator_cloud_account_id: payload.creator_cloud_account_id,
                    creator_roles: payload.creator_roles,
                    governance_policy_version: payload.governance_policy_version,
                    receipt_id: payload.receipt_id,
                    membership_event_ref: format!("evt_{}", entry.id.simple()),
                    idempotency_key: payload.idempotency_key.clone(),
                    product_display_name: payload.product_display_name,
                    team_display_name: payload.team_display_name,
                    verified_domain: payload.verified_domain,
                    event_store_hint: payload.event_store_hint,
                    required_connection: payload.required_connection,
                    created_at: payload.created_at,
                };

                state
                    .product_idempotency
                    .insert(payload.idempotency_key, payload.product_id.clone());
                state.products.insert(payload.product_id, record);
            }
            _ => {}
        }
    }

    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_store_adapters::MemoryAdapter;
    use bicameral_api::candidate::DecisionLevel;
    use bicameral_api::event_store::{EventStoreEntry, EventType, IngestPayload};
    use bicameral_api::evidence::SourceEvidence;
    use bicameral_api::source::{Source, SourceSnapshot};

    fn make_ingest_entry(source_uri: &str, with_snapshot: bool) -> EventStoreEntry {
        let source = Source {
            uri: source_uri.to_string(),
            source_type: "test".to_string(),
            label: None,
            source_kind: None,
        };
        let snapshot = if with_snapshot {
            Some(SourceSnapshot {
                snapshot_addr: format!("sha256:abc123_{source_uri}"),
                snapshot_ref: source_uri.to_string(),
                captured_at: chrono::Utc::now(),
                content: Some("test content".to_string()),
                content_hash: Some("abc123".to_string()),
                capture_method: None,
            })
        } else {
            None
        };
        let snap_id = snapshot
            .as_ref()
            .map(|s| s.snapshot_addr.clone())
            .unwrap_or_else(|| format!("inline:{source_uri}"));
        let evidence = vec![SourceEvidence::new(
            snap_id,
            0,
            10,
            "test excerpt".to_string(),
            0.9,
        )];
        let mut candidate = DecisionCandidate::new(
            "Test Decision".to_string(),
            "A test".to_string(),
            DecisionLevel::Architecture,
            source_uri.to_string(),
        );
        candidate.evidence_refs = evidence.iter().map(|e| e.id).collect();
        let payload = IngestPayload {
            source,
            snapshot,
            evidence,
            candidate,
            evidence_references: Vec::new(),
            provenance: None,
        };
        EventStoreEntry::new(
            EventType::Ingest,
            serde_json::to_value(&payload).unwrap(),
            "test".to_string(),
        )
    }

    #[test]
    fn replay_reconstructs_candidates_from_adapter() {
        let adapter = MemoryAdapter::new();
        let entry = make_ingest_entry("jira://PROJ-123", true);
        adapter.append(entry).unwrap();

        let state = replay_from_adapter(&adapter).unwrap();
        assert_eq!(state.candidates.len(), 1);
        assert_eq!(state.sources.len(), 1);
        assert_eq!(state.snapshots.len(), 1);
        assert_eq!(state.evidence.len(), 1);
        assert!(state.sources.contains_key("jira://PROJ-123"));
    }

    #[test]
    fn replay_handles_multiple_ingest_events() {
        let adapter = MemoryAdapter::new();
        adapter
            .append(make_ingest_entry("jira://PROJ-1", true))
            .unwrap();
        adapter
            .append(make_ingest_entry("slack://msg-2", false))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();
        assert_eq!(state.candidates.len(), 2);
        assert_eq!(state.sources.len(), 2);
        assert_eq!(state.snapshots.len(), 1, "only one entry had a snapshot");
        assert_eq!(state.evidence.len(), 2);
    }

    #[test]
    fn replay_from_empty_adapter() {
        let adapter = MemoryAdapter::new();
        let state = replay_from_adapter(&adapter).unwrap();
        assert!(state.candidates.is_empty());
        assert!(state.sources.is_empty());
    }

    #[test]
    fn replay_uses_portable_identifiers_not_paths() {
        let adapter = MemoryAdapter::new();
        adapter
            .append(make_ingest_entry("jira://PROJ-1", true))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();
        let source = state.sources.values().next().unwrap();
        assert!(
            !source.uri.contains(".bicameral"),
            "Source URI must be a portable identifier, not a .bicameral path"
        );
        let snap = state.snapshots.values().next().unwrap();
        assert!(
            snap.snapshot_addr.starts_with("sha256:"),
            "Snapshot address must be content-addressed, not a file path"
        );
    }

    #[test]
    fn ingest_payload_is_drive_folder_compatible() {
        // Prove that IngestPayload serializes/deserializes identically regardless
        // of substrate — the DriveFolder adapter stores each event as individual
        // JSON (not JSONL), and the payload must round-trip correctly.
        use crate::event_store_adapters::DriveFolderAdapter;

        let dir = tempfile::tempdir().unwrap();
        let drive_adapter = DriveFolderAdapter::new(dir.path());
        let entry = make_ingest_entry("gdrive://doc-42", true);
        drive_adapter.append(entry).unwrap();

        let state = replay_from_adapter(&drive_adapter).unwrap();
        assert_eq!(state.candidates.len(), 1);
        assert_eq!(state.sources.len(), 1);
        assert!(state.sources.contains_key("gdrive://doc-42"));

        // Verify the adapter reports DriveFolder kind
        assert_eq!(
            drive_adapter.substrate_kind(),
            bicameral_api::event_store::SubstrateKind::DriveFolder
        );
    }

    #[test]
    fn git_adapter_replay_round_trips() {
        use crate::event_store_adapters::GitAdapter;

        let dir = tempfile::tempdir().unwrap();
        let git_adapter = GitAdapter::new(dir.path());
        let entry = make_ingest_entry("github://pr-99", true);
        git_adapter.append(entry).unwrap();

        let state = replay_from_adapter(&git_adapter).unwrap();
        assert_eq!(state.candidates.len(), 1);
        assert_eq!(state.sources.len(), 1);
        assert_eq!(state.snapshots.len(), 1);
        assert_eq!(state.evidence.len(), 1);
    }

    #[test]
    fn replay_reconstructs_bind_to_code_events() {
        use bicameral_api::evidence::{BindingConfidence, BindingEvidence, BindingType};

        let adapter = MemoryAdapter::new();
        adapter
            .append(make_ingest_entry("jira://PROJ-1", true))
            .unwrap();

        let decision_id = uuid::Uuid::new_v4();
        let mut binding = BindingEvidence::new(
            decision_id,
            BindingType::CodeRegion,
            "src.rs:1-5".to_string(),
            BindingConfidence::High,
        );
        binding.graph_snapshot_id = Some("gs-abc123".to_string());

        let bind_payload = BindToCodePayload {
            decision_id,
            binding: binding.clone(),
            graph_snapshot_id: "gs-abc123".to_string(),
            validated_sha: "abc123def".to_string(),
            evidence_state: bicameral_api::evidence::GraphEvidenceState::Verified,
            content_hash: Some("sha256:content".to_string()),
            line_start: Some(1),
            line_end: Some(5),
        };

        adapter
            .append(EventStoreEntry::new(
                EventType::BindToCode,
                serde_json::to_value(&bind_payload).unwrap(),
                "test".to_string(),
            ))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();
        assert_eq!(state.candidates.len(), 1);
        assert_eq!(state.bindings.len(), 1);
        let decision_bindings = state.bindings.get(&decision_id).unwrap();
        assert_eq!(decision_bindings.len(), 1);
        assert_eq!(decision_bindings[0].target, "src.rs:1-5");
        assert_eq!(
            decision_bindings[0].graph_snapshot_id.as_deref(),
            Some("gs-abc123")
        );
    }

    #[test]
    fn replay_groups_multiple_bindings_per_decision() {
        use bicameral_api::evidence::{BindingConfidence, BindingEvidence, BindingType};

        let adapter = MemoryAdapter::new();
        let decision_id = uuid::Uuid::new_v4();

        for target in &["src/a.rs:1-10", "src/b.rs:5-20"] {
            let binding = BindingEvidence::new(
                decision_id,
                BindingType::CodeRegion,
                target.to_string(),
                BindingConfidence::High,
            );
            let payload = BindToCodePayload {
                decision_id,
                binding,
                graph_snapshot_id: "gs-abc".to_string(),
                validated_sha: "abc123".to_string(),
                evidence_state: bicameral_api::evidence::GraphEvidenceState::Verified,
                content_hash: None,
                line_start: None,
                line_end: None,
            };
            adapter
                .append(EventStoreEntry::new(
                    EventType::BindToCode,
                    serde_json::to_value(&payload).unwrap(),
                    "test".to_string(),
                ))
                .unwrap();
        }

        let state = replay_from_adapter(&adapter).unwrap();
        let bindings = state.bindings.get(&decision_id).unwrap();
        assert_eq!(bindings.len(), 2);
    }

    // ── Lifecycle replay tests ──────────────────────────────────

    /// Helper: ingest a candidate and return its UUID.
    fn ingest_and_get_id(adapter: &MemoryAdapter, source_uri: &str) -> Uuid {
        let entry = make_ingest_entry(source_uri, true);
        let payload: IngestPayload = serde_json::from_value(entry.payload.clone()).unwrap();
        let id = payload.candidate.id;
        adapter.append(entry).unwrap();
        id
    }

    /// Helper: emit an AcceptCandidate event and return the generated decision id.
    fn emit_accept(adapter: &MemoryAdapter, candidate_id: Uuid) -> Uuid {
        let decision_id = Uuid::new_v4();
        adapter
            .append(EventStoreEntry::new(
                EventType::AcceptCandidate,
                serde_json::json!({
                    "candidate_id": candidate_id.to_string(),
                    "decision_id": decision_id.to_string(),
                }),
                "test".to_string(),
            ))
            .unwrap();
        decision_id
    }

    #[test]
    fn replay_accept_candidate_creates_decision_proposed() {
        let adapter = MemoryAdapter::new();
        let cand_id = ingest_and_get_id(&adapter, "jira://PROJ-10");
        let dec_id = emit_accept(&adapter, cand_id);

        let state = replay_from_adapter(&adapter).unwrap();
        assert_eq!(state.decisions.len(), 1);
        let decision = state.decisions.get(&dec_id).unwrap();
        assert_eq!(decision.signoff_state, SignoffState::Proposed);
        assert_eq!(decision.candidate_id, cand_id);

        let candidate = state.candidates.get(&cand_id).unwrap();
        assert_eq!(candidate.candidate_state, CandidateState::Promoted);
    }

    #[test]
    fn replay_approve_signoff_transitions_to_approved() {
        let adapter = MemoryAdapter::new();
        let cand_id = ingest_and_get_id(&adapter, "jira://PROJ-11");
        let dec_id = emit_accept(&adapter, cand_id);

        adapter
            .append(EventStoreEntry::new(
                EventType::ApproveSignoff,
                serde_json::json!({ "decision_id": dec_id.to_string() }),
                "test".to_string(),
            ))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();
        let decision = state.decisions.get(&dec_id).unwrap();
        assert_eq!(decision.signoff_state, SignoffState::Approved);
        assert_eq!(
            decision.review_state,
            ReviewLifecycleState::ReadyToMaterialize
        );
    }

    #[test]
    fn replay_reject_candidate_marks_rejected() {
        let adapter = MemoryAdapter::new();
        let cand_id = ingest_and_get_id(&adapter, "jira://PROJ-12");

        adapter
            .append(EventStoreEntry::new(
                EventType::RejectCandidate,
                serde_json::json!({ "candidate_id": cand_id.to_string() }),
                "test".to_string(),
            ))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();
        assert!(state.decisions.is_empty());
        let candidate = state.candidates.get(&cand_id).unwrap();
        assert_eq!(candidate.candidate_state, CandidateState::Rejected);
        assert_eq!(candidate.review_state, ReviewLifecycleState::Materialized);
    }

    #[test]
    fn replay_reject_signoff_marks_rejected() {
        let adapter = MemoryAdapter::new();
        let cand_id = ingest_and_get_id(&adapter, "jira://PROJ-13");
        let dec_id = emit_accept(&adapter, cand_id);

        adapter
            .append(EventStoreEntry::new(
                EventType::RejectSignoff,
                serde_json::json!({ "decision_id": dec_id.to_string() }),
                "test".to_string(),
            ))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();
        let decision = state.decisions.get(&dec_id).unwrap();
        assert_eq!(decision.signoff_state, SignoffState::Rejected);
        assert_eq!(decision.review_state, ReviewLifecycleState::Materialized);
    }

    #[test]
    fn replay_supersede_decision_marks_superseded() {
        let adapter = MemoryAdapter::new();
        let cand_id = ingest_and_get_id(&adapter, "jira://PROJ-14");
        let dec_id = emit_accept(&adapter, cand_id);

        adapter
            .append(EventStoreEntry::new(
                EventType::SupersedeDecision,
                serde_json::json!({ "decision_id": dec_id.to_string() }),
                "test".to_string(),
            ))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();
        let decision = state.decisions.get(&dec_id).unwrap();
        assert_eq!(decision.signoff_state, SignoffState::Superseded);
        assert_eq!(decision.review_state, ReviewLifecycleState::Materialized);
    }

    #[test]
    fn replay_full_lifecycle_sequence() {
        let adapter = MemoryAdapter::new();

        // Ingest two candidates
        let cand_a = ingest_and_get_id(&adapter, "jira://A");
        let cand_b = ingest_and_get_id(&adapter, "jira://B");

        // Accept both
        let dec_a = emit_accept(&adapter, cand_a);
        let dec_b = emit_accept(&adapter, cand_b);

        // Approve A, reject B's signoff
        adapter
            .append(EventStoreEntry::new(
                EventType::ApproveSignoff,
                serde_json::json!({ "decision_id": dec_a.to_string() }),
                "test".to_string(),
            ))
            .unwrap();
        adapter
            .append(EventStoreEntry::new(
                EventType::RejectSignoff,
                serde_json::json!({ "decision_id": dec_b.to_string() }),
                "test".to_string(),
            ))
            .unwrap();

        // Supersede A
        adapter
            .append(EventStoreEntry::new(
                EventType::SupersedeDecision,
                serde_json::json!({ "decision_id": dec_a.to_string() }),
                "test".to_string(),
            ))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();
        assert_eq!(state.decisions.len(), 2);

        let da = state.decisions.get(&dec_a).unwrap();
        assert_eq!(da.signoff_state, SignoffState::Superseded);

        let db = state.decisions.get(&dec_b).unwrap();
        assert_eq!(db.signoff_state, SignoffState::Rejected);

        // Both candidates should be promoted
        assert_eq!(
            state.candidates.get(&cand_a).unwrap().candidate_state,
            CandidateState::Promoted
        );
        assert_eq!(
            state.candidates.get(&cand_b).unwrap().candidate_state,
            CandidateState::Promoted
        );
    }

    #[test]
    fn replay_lifecycle_is_substrate_neutral() {
        use crate::event_store_adapters::GitAdapter;

        let dir = tempfile::tempdir().unwrap();
        let git = GitAdapter::new(dir.path());

        let entry = make_ingest_entry("github://pr-200", true);
        let payload: IngestPayload = serde_json::from_value(entry.payload.clone()).unwrap();
        let cand_id = payload.candidate.id;
        git.append(entry).unwrap();

        let dec_id = Uuid::new_v4();
        git.append(EventStoreEntry::new(
            EventType::AcceptCandidate,
            serde_json::json!({
                "candidate_id": cand_id.to_string(),
                "decision_id": dec_id.to_string(),
            }),
            "test".to_string(),
        ))
        .unwrap();
        git.append(EventStoreEntry::new(
            EventType::ApproveSignoff,
            serde_json::json!({ "decision_id": dec_id.to_string() }),
            "test".to_string(),
        ))
        .unwrap();

        let state = replay_from_adapter(&git).unwrap();
        assert_eq!(state.decisions.len(), 1);
        let d = state.decisions.get(&dec_id).unwrap();
        assert_eq!(d.signoff_state, SignoffState::Approved);
    }

    // ── Canonical promotion replay tests ────────────────────────

    fn emit_promotion(
        adapter: &MemoryAdapter,
        decision_id: Uuid,
        idempotency_key: &str,
    ) -> EventStoreEntry {
        use bicameral_api::constraint::{ActorType, Attribution};
        let payload = PromoteToCanonicalPayload {
            decision_id,
            title: "Boundary rule".to_string(),
            statement: "Core owns promotion mechanics".to_string(),
            evidence_refs: vec![Uuid::new_v4()],
            affected_surfaces: vec!["api/promotion".to_string()],
            reviewer: Attribution {
                actor_id: "rev-1".to_string(),
                actor_type: ActorType::Human,
                display_name: None,
            },
            reason: None,
            idempotency_key: idempotency_key.to_string(),
        };
        let entry = EventStoreEntry::new(
            EventType::PromoteToCanonical,
            serde_json::to_value(&payload).unwrap(),
            "promoter".to_string(),
        );
        adapter.append(entry.clone()).unwrap();
        entry
    }

    #[test]
    fn replay_reconstructs_canonical_constraint() {
        let adapter = MemoryAdapter::new();
        let cand_id = ingest_and_get_id(&adapter, "jira://PROJ-50");
        let dec_id = emit_accept(&adapter, cand_id);
        let emitted = emit_promotion(&adapter, dec_id, "idem-50");

        let state = replay_from_adapter(&adapter).unwrap();
        assert_eq!(state.canonical_constraints.len(), 1);
        let constraint = state.canonical_constraints.get("idem-50").unwrap();
        assert_eq!(constraint.review_state, CanonicalReviewState::Accepted);
        assert_eq!(constraint.decision_id, dec_id);
        assert_eq!(constraint.statement, "Core owns promotion mechanics");
        assert_eq!(constraint.reviewer.actor_id, "rev-1");
        assert_eq!(constraint.evidence_refs.len(), 1);
        // promoted_at is taken from the event timestamp.
        assert_eq!(constraint.promoted_at, emitted.timestamp);
    }

    #[test]
    fn replay_promotion_is_idempotent_by_key() {
        let adapter = MemoryAdapter::new();
        let cand_id = ingest_and_get_id(&adapter, "jira://PROJ-51");
        let dec_id = emit_accept(&adapter, cand_id);
        emit_promotion(&adapter, dec_id, "same-key");
        emit_promotion(&adapter, dec_id, "same-key");

        let state = replay_from_adapter(&adapter).unwrap();
        assert_eq!(
            state.canonical_constraints.len(),
            1,
            "two promotions with the same idempotency_key must yield one constraint"
        );
    }

    #[test]
    fn replay_promotion_substrate_neutral_git() {
        use crate::event_store_adapters::GitAdapter;
        use bicameral_api::constraint::{ActorType, Attribution};

        let dir = tempfile::tempdir().unwrap();
        let git = GitAdapter::new(dir.path());

        let entry = make_ingest_entry("github://pr-50", true);
        let payload: IngestPayload = serde_json::from_value(entry.payload.clone()).unwrap();
        let cand_id = payload.candidate.id;
        git.append(entry).unwrap();

        let dec_id = Uuid::new_v4();
        git.append(EventStoreEntry::new(
            EventType::AcceptCandidate,
            serde_json::json!({
                "candidate_id": cand_id.to_string(),
                "decision_id": dec_id.to_string(),
            }),
            "test".to_string(),
        ))
        .unwrap();

        let promotion = PromoteToCanonicalPayload {
            decision_id: dec_id,
            title: "T".to_string(),
            statement: "S".to_string(),
            evidence_refs: Vec::new(),
            affected_surfaces: Vec::new(),
            reviewer: Attribution {
                actor_id: "rev-git".to_string(),
                actor_type: ActorType::Agent,
                display_name: None,
            },
            reason: None,
            idempotency_key: "idem-git".to_string(),
        };
        git.append(EventStoreEntry::new(
            EventType::PromoteToCanonical,
            serde_json::to_value(&promotion).unwrap(),
            "promoter".to_string(),
        ))
        .unwrap();

        let state = replay_from_adapter(&git).unwrap();
        assert_eq!(state.canonical_constraints.len(), 1);
        let constraint = state.canonical_constraints.get("idem-git").unwrap();
        assert_eq!(constraint.decision_id, dec_id);
        assert_eq!(constraint.reviewer.actor_type, ActorType::Agent);
    }

    // ── Constraint catalog replay tests ─────────────────────────

    fn emit_classify(
        adapter: &MemoryAdapter,
        idempotency_key: &str,
        constraint_type: bicameral_api::catalog::ConstraintType,
        scope: bicameral_api::catalog::ConstraintScope,
    ) {
        use bicameral_api::constraint::{ActorType, Attribution};
        let payload = ClassifyConstraintPayload {
            idempotency_key: idempotency_key.to_string(),
            constraint_type,
            scope,
            attribution: Attribution {
                actor_id: "classifier".to_string(),
                actor_type: ActorType::Human,
                display_name: None,
            },
            affected_surfaces: None,
        };
        adapter
            .append(EventStoreEntry::new(
                EventType::ClassifyConstraint,
                serde_json::to_value(&payload).unwrap(),
                "classifier".to_string(),
            ))
            .unwrap();
    }

    /// Promote a fresh decision and return its promotion idempotency key.
    fn promote(adapter: &MemoryAdapter, source: &str, key: &str) {
        let cand_id = ingest_and_get_id(adapter, source);
        let dec_id = emit_accept(adapter, cand_id);
        emit_promotion(adapter, dec_id, key);
    }

    #[test]
    fn replay_classify_builds_catalog_constraint() {
        use bicameral_api::catalog::{ConstraintScope, ConstraintType};
        let adapter = MemoryAdapter::new();
        promote(&adapter, "jira://CAT-1", "idem-cat-1");
        emit_classify(
            &adapter,
            "idem-cat-1",
            ConstraintType::Security,
            ConstraintScope::Service,
        );

        let state = replay_from_adapter(&adapter).unwrap();
        assert_eq!(state.catalog.len(), 1);
        let c = state.catalog.get("idem-cat-1").unwrap();
        assert_eq!(c.constraint_type, ConstraintType::Security);
        assert_eq!(c.scope, ConstraintScope::Service);
        assert_eq!(c.version, 1);
        assert_eq!(c.review_state, CanonicalReviewState::Accepted);
        // Graph edges carried from the canonical constraint.
        let canon = state.canonical_constraints.get("idem-cat-1").unwrap();
        assert_eq!(c.decision_id, canon.decision_id);
        assert_eq!(c.evidence_refs, canon.evidence_refs);
        assert_eq!(c.statement, canon.statement);
    }

    #[test]
    fn replay_classify_without_promotion_is_noop() {
        use bicameral_api::catalog::{ConstraintScope, ConstraintType};
        let adapter = MemoryAdapter::new();
        // Classify a key that was never promoted.
        emit_classify(
            &adapter,
            "ghost-key",
            ConstraintType::Product,
            ConstraintScope::Global,
        );

        let state = replay_from_adapter(&adapter).unwrap();
        assert!(
            state.catalog.is_empty(),
            "the catalog cannot hold an unpromoted constraint"
        );
    }

    #[test]
    fn replay_classify_is_idempotent() {
        use bicameral_api::catalog::{ConstraintScope, ConstraintType};
        let adapter = MemoryAdapter::new();
        promote(&adapter, "jira://CAT-2", "idem-cat-2");
        emit_classify(
            &adapter,
            "idem-cat-2",
            ConstraintType::Technical,
            ConstraintScope::Feature,
        );
        emit_classify(
            &adapter,
            "idem-cat-2",
            ConstraintType::Compliance,
            ConstraintScope::Global,
        );

        let state = replay_from_adapter(&adapter).unwrap();
        assert_eq!(state.catalog.len(), 1);
        // First classification wins (idempotent).
        assert_eq!(
            state.catalog.get("idem-cat-2").unwrap().constraint_type,
            ConstraintType::Technical
        );
    }

    #[test]
    fn replay_supersede_marks_superseded_and_bumps_version() {
        use bicameral_api::catalog::{ConstraintScope, ConstraintType};
        let adapter = MemoryAdapter::new();
        promote(&adapter, "jira://CAT-3a", "idem-old");
        promote(&adapter, "jira://CAT-3b", "idem-new");
        emit_classify(
            &adapter,
            "idem-old",
            ConstraintType::Product,
            ConstraintScope::Global,
        );
        emit_classify(
            &adapter,
            "idem-new",
            ConstraintType::Product,
            ConstraintScope::Global,
        );

        let payload = SupersedeConstraintPayload {
            current_idempotency_key: "idem-old".to_string(),
            replacement_idempotency_key: "idem-new".to_string(),
        };
        adapter
            .append(EventStoreEntry::new(
                EventType::SupersedeConstraint,
                serde_json::to_value(&payload).unwrap(),
                "governor".to_string(),
            ))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();
        let old = state.catalog.get("idem-old").unwrap();
        let new = state.catalog.get("idem-new").unwrap();
        assert_eq!(old.review_state, CanonicalReviewState::Superseded);
        assert_eq!(old.superseded_by, Some(new.id));
        assert_eq!(new.supersedes, Some(old.id));
        assert_eq!(new.version, 2);
    }

    #[test]
    fn replay_catalog_query_filters_by_type() {
        use bicameral_api::catalog::{
            query_catalog, ConstraintQuery, ConstraintScope, ConstraintType,
        };
        let adapter = MemoryAdapter::new();
        promote(&adapter, "jira://CAT-4a", "k-prod");
        promote(&adapter, "jira://CAT-4b", "k-sec");
        emit_classify(
            &adapter,
            "k-prod",
            ConstraintType::Product,
            ConstraintScope::Global,
        );
        emit_classify(
            &adapter,
            "k-sec",
            ConstraintType::Security,
            ConstraintScope::Service,
        );

        let state = replay_from_adapter(&adapter).unwrap();
        let result = query_catalog(
            &state.catalog,
            &ConstraintQuery {
                types: Some(vec![ConstraintType::Security]),
                ..ConstraintQuery::default()
            },
        );
        assert_eq!(result.total, 1);
        assert_eq!(
            result.constraints[0].constraint_type,
            ConstraintType::Security
        );
    }

    #[test]
    fn replay_catalog_substrate_neutral_git() {
        use crate::event_store_adapters::GitAdapter;
        use bicameral_api::catalog::{ConstraintScope, ConstraintType};
        use bicameral_api::constraint::{ActorType, Attribution};

        let dir = tempfile::tempdir().unwrap();
        let git = GitAdapter::new(dir.path());

        let entry = make_ingest_entry("github://cat-pr", true);
        let payload: IngestPayload = serde_json::from_value(entry.payload.clone()).unwrap();
        let cand_id = payload.candidate.id;
        git.append(entry).unwrap();

        let dec_id = Uuid::new_v4();
        git.append(EventStoreEntry::new(
            EventType::AcceptCandidate,
            serde_json::json!({
                "candidate_id": cand_id.to_string(),
                "decision_id": dec_id.to_string(),
            }),
            "test".to_string(),
        ))
        .unwrap();

        let promotion = PromoteToCanonicalPayload {
            decision_id: dec_id,
            title: "T".to_string(),
            statement: "S".to_string(),
            evidence_refs: Vec::new(),
            affected_surfaces: Vec::new(),
            reviewer: Attribution {
                actor_id: "rev".to_string(),
                actor_type: ActorType::Human,
                display_name: None,
            },
            reason: None,
            idempotency_key: "idem-git-cat".to_string(),
        };
        git.append(EventStoreEntry::new(
            EventType::PromoteToCanonical,
            serde_json::to_value(&promotion).unwrap(),
            "promoter".to_string(),
        ))
        .unwrap();

        let classify = ClassifyConstraintPayload {
            idempotency_key: "idem-git-cat".to_string(),
            constraint_type: ConstraintType::Compliance,
            scope: ConstraintScope::Customer,
            attribution: Attribution {
                actor_id: "classifier".to_string(),
                actor_type: ActorType::Agent,
                display_name: None,
            },
            affected_surfaces: None,
        };
        git.append(EventStoreEntry::new(
            EventType::ClassifyConstraint,
            serde_json::to_value(&classify).unwrap(),
            "classifier".to_string(),
        ))
        .unwrap();

        let state = replay_from_adapter(&git).unwrap();
        assert_eq!(state.catalog.len(), 1);
        let c = state.catalog.get("idem-git-cat").unwrap();
        assert_eq!(c.constraint_type, ConstraintType::Compliance);
        assert_eq!(c.scope, ConstraintScope::Customer);
    }

    // ── Contradiction finding replay tests ──────────────────────

    /// Promote + classify two constraints; return their catalog ids.
    fn classified_pair(adapter: &MemoryAdapter) -> (Uuid, Uuid) {
        use bicameral_api::catalog::{ConstraintScope, ConstraintType};
        promote(adapter, "jira://DRIFT-1", "idem-d1");
        promote(adapter, "jira://DRIFT-2", "idem-d2");
        emit_classify(
            adapter,
            "idem-d1",
            ConstraintType::Security,
            ConstraintScope::Service,
        );
        emit_classify(
            adapter,
            "idem-d2",
            ConstraintType::Product,
            ConstraintScope::Service,
        );
        let state = replay_from_adapter(adapter).unwrap();
        (
            state.catalog.get("idem-d1").unwrap().id,
            state.catalog.get("idem-d2").unwrap().id,
        )
    }

    fn emit_raise(adapter: &MemoryAdapter, id1: Uuid, id2: Uuid) -> Uuid {
        use bicameral_api::drift::{ContradictionReport, FindingSeverity, FindingStatus};
        let report_id = Uuid::new_v4();
        let now = chrono::Utc::now();
        let report = ContradictionReport {
            id: report_id,
            summary: "co-location".to_string(),
            severity: FindingSeverity::High,
            status: FindingStatus::Open,
            related_constraint_ids: vec![id1, id2],
            evidence_refs: Vec::new(),
            suggested_resolution: None,
            owner_recommendation: None,
            detected_at: now,
            updated_at: now,
        };
        adapter
            .append(EventStoreEntry::new(
                EventType::RaiseContradiction,
                serde_json::to_value(&RaiseContradictionPayload { report }).unwrap(),
                "detector".to_string(),
            ))
            .unwrap();
        report_id
    }

    fn emit_transition(
        adapter: &MemoryAdapter,
        report_id: Uuid,
        to: bicameral_api::drift::FindingStatus,
    ) {
        use bicameral_api::constraint::{ActorType, Attribution};
        use bicameral_api::drift::ReviewTransitionRequest;
        let request = ReviewTransitionRequest {
            report_id,
            to_status: to,
            actor: Attribution {
                actor_id: "reviewer".to_string(),
                actor_type: ActorType::Human,
                display_name: None,
            },
            note: None,
            at: chrono::Utc::now(),
        };
        adapter
            .append(EventStoreEntry::new(
                EventType::TransitionFinding,
                serde_json::to_value(&TransitionFindingPayload { request }).unwrap(),
                "reviewer".to_string(),
            ))
            .unwrap();
    }

    #[test]
    fn replay_raise_contradiction_writes_conflict_edges() {
        let adapter = MemoryAdapter::new();
        let (id1, id2) = classified_pair(&adapter);
        let report_id = emit_raise(&adapter, id1, id2);

        let state = replay_from_adapter(&adapter).unwrap();
        assert_eq!(state.contradictions.len(), 1);
        assert!(state.contradictions.contains_key(&report_id));
        // Each related constraint has the other's id in conflicts_with.
        let c1 = state.catalog.get("idem-d1").unwrap();
        let c2 = state.catalog.get("idem-d2").unwrap();
        assert!(c1.conflicts_with.contains(&id2));
        assert!(c2.conflicts_with.contains(&id1));
        assert!(!c1.conflicts_with.contains(&id1), "no self-edge");
    }

    #[test]
    fn replay_transition_finding_updates_status() {
        use bicameral_api::drift::FindingStatus;
        let adapter = MemoryAdapter::new();
        let (id1, id2) = classified_pair(&adapter);
        let report_id = emit_raise(&adapter, id1, id2);
        emit_transition(&adapter, report_id, FindingStatus::Acknowledged);

        let state = replay_from_adapter(&adapter).unwrap();
        assert_eq!(
            state.contradictions.get(&report_id).unwrap().status,
            FindingStatus::Acknowledged
        );
    }

    #[test]
    fn replay_invalid_transition_preserved() {
        use bicameral_api::drift::FindingStatus;
        let adapter = MemoryAdapter::new();
        let (id1, id2) = classified_pair(&adapter);
        let report_id = emit_raise(&adapter, id1, id2);
        // Open -> Resolved is illegal (must go through Acknowledged).
        emit_transition(&adapter, report_id, FindingStatus::Resolved);

        let state = replay_from_adapter(&adapter).unwrap();
        assert_eq!(
            state.contradictions.get(&report_id).unwrap().status,
            FindingStatus::Open,
            "invalid transition leaves status unchanged"
        );
    }

    #[test]
    fn replay_raise_is_idempotent() {
        let adapter = MemoryAdapter::new();
        let (id1, id2) = classified_pair(&adapter);
        let report_id = emit_raise(&adapter, id1, id2);
        // Re-raise the SAME report id.
        use bicameral_api::drift::{ContradictionReport, FindingSeverity, FindingStatus};
        let now = chrono::Utc::now();
        let dup = ContradictionReport {
            id: report_id,
            summary: "dup".to_string(),
            severity: FindingSeverity::Low,
            status: FindingStatus::Open,
            related_constraint_ids: vec![id1, id2],
            evidence_refs: Vec::new(),
            suggested_resolution: None,
            owner_recommendation: None,
            detected_at: now,
            updated_at: now,
        };
        adapter
            .append(EventStoreEntry::new(
                EventType::RaiseContradiction,
                serde_json::to_value(&RaiseContradictionPayload { report: dup }).unwrap(),
                "detector".to_string(),
            ))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();
        assert_eq!(state.contradictions.len(), 1);
        // First raise wins (idempotent): severity High, not the dup's Low.
        assert_eq!(
            state.contradictions.get(&report_id).unwrap().severity,
            FindingSeverity::High
        );
    }

    #[test]
    fn replay_drift_substrate_neutral_git() {
        use crate::event_store_adapters::GitAdapter;
        use bicameral_api::catalog::{ConstraintScope, ConstraintType};
        use bicameral_api::drift::{ContradictionReport, FindingSeverity, FindingStatus};

        let dir = tempfile::tempdir().unwrap();
        let git = GitAdapter::new(dir.path());

        // Promote + classify two constraints on git.
        for (src, key) in [("github://d-1", "g-d1"), ("github://d-2", "g-d2")] {
            let entry = make_ingest_entry(src, true);
            let payload: IngestPayload = serde_json::from_value(entry.payload.clone()).unwrap();
            let cand_id = payload.candidate.id;
            git.append(entry).unwrap();
            let dec_id = Uuid::new_v4();
            git.append(EventStoreEntry::new(
                EventType::AcceptCandidate,
                serde_json::json!({
                    "candidate_id": cand_id.to_string(),
                    "decision_id": dec_id.to_string(),
                }),
                "test".to_string(),
            ))
            .unwrap();
            let promotion = PromoteToCanonicalPayload {
                decision_id: dec_id,
                title: "T".to_string(),
                statement: "S".to_string(),
                evidence_refs: Vec::new(),
                affected_surfaces: vec!["shared".to_string()],
                reviewer: bicameral_api::constraint::Attribution {
                    actor_id: "r".to_string(),
                    actor_type: bicameral_api::constraint::ActorType::Human,
                    display_name: None,
                },
                reason: None,
                idempotency_key: key.to_string(),
            };
            git.append(EventStoreEntry::new(
                EventType::PromoteToCanonical,
                serde_json::to_value(&promotion).unwrap(),
                "promoter".to_string(),
            ))
            .unwrap();
            let classify = ClassifyConstraintPayload {
                idempotency_key: key.to_string(),
                constraint_type: ConstraintType::Security,
                scope: ConstraintScope::Service,
                attribution: bicameral_api::constraint::Attribution {
                    actor_id: "c".to_string(),
                    actor_type: bicameral_api::constraint::ActorType::Agent,
                    display_name: None,
                },
                affected_surfaces: None,
            };
            git.append(EventStoreEntry::new(
                EventType::ClassifyConstraint,
                serde_json::to_value(&classify).unwrap(),
                "classifier".to_string(),
            ))
            .unwrap();
        }

        let mid = replay_from_adapter(&git).unwrap();
        let id1 = mid.catalog.get("g-d1").unwrap().id;
        let id2 = mid.catalog.get("g-d2").unwrap().id;

        let report_id = Uuid::new_v4();
        let now = chrono::Utc::now();
        let report = ContradictionReport {
            id: report_id,
            summary: "co-location".to_string(),
            severity: FindingSeverity::High,
            status: FindingStatus::Open,
            related_constraint_ids: vec![id1, id2],
            evidence_refs: Vec::new(),
            suggested_resolution: None,
            owner_recommendation: None,
            detected_at: now,
            updated_at: now,
        };
        git.append(EventStoreEntry::new(
            EventType::RaiseContradiction,
            serde_json::to_value(&RaiseContradictionPayload { report }).unwrap(),
            "detector".to_string(),
        ))
        .unwrap();

        let state = replay_from_adapter(&git).unwrap();
        assert_eq!(state.contradictions.len(), 1);
        assert!(state
            .catalog
            .get("g-d1")
            .unwrap()
            .conflicts_with
            .contains(&id2));
    }

    // ── Semantic mapping replay tests ───────────────────────────

    fn semantic_mapping_entry(
        mapping_id: Uuid,
        constraint_id: Uuid,
        confidence_bps: u16,
    ) -> EventStoreEntry {
        use bicameral_api::constraint::{ActorType, Attribution, LifecycleReviewState};
        use bicameral_api::semantic::{Provenance, SemanticMapping, SourcePhraseRef};
        let mapping = SemanticMapping {
            id: mapping_id,
            source_phrase: SourcePhraseRef {
                text: "must encrypt PII at rest".to_string(),
                source_id: Some("jira://SEC-1".to_string()),
                locator: None,
            },
            derived_constraint_id: constraint_id,
            provenance: Provenance {
                captured_at: chrono::Utc::now(),
                captured_by: Attribution {
                    actor_id: "mapper".to_string(),
                    actor_type: ActorType::Agent,
                    display_name: None,
                },
                capture_method: "rule-pack".to_string(),
                pipeline_version: None,
                source_hash: None,
            },
            review_state: LifecycleReviewState::Accepted,
            confidence_bps,
            examples: Vec::new(),
        };
        EventStoreEntry::new(
            EventType::RecordSemanticMapping,
            serde_json::to_value(&RecordSemanticMappingPayload { mapping }).unwrap(),
            "mapper".to_string(),
        )
    }

    #[test]
    fn replay_records_semantic_mapping() {
        let adapter = MemoryAdapter::new();
        let mid = Uuid::new_v4();
        let cid = Uuid::new_v4();
        adapter
            .append(semantic_mapping_entry(mid, cid, 9000))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();
        assert_eq!(state.semantic_mappings.len(), 1);
        let m = state.semantic_mappings.get(&mid).unwrap();
        assert_eq!(m.derived_constraint_id, cid);
        assert_eq!(m.confidence_bps, 9000);
    }

    #[test]
    fn replay_semantic_mapping_idempotent_by_id() {
        let adapter = MemoryAdapter::new();
        let mid = Uuid::new_v4();
        let cid = Uuid::new_v4();
        adapter
            .append(semantic_mapping_entry(mid, cid, 9000))
            .unwrap();
        adapter
            .append(semantic_mapping_entry(mid, cid, 1000))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();
        assert_eq!(state.semantic_mappings.len(), 1);
        assert_eq!(
            state.semantic_mappings.get(&mid).unwrap().confidence_bps,
            9000
        );
    }

    #[test]
    fn replay_semantic_mapping_substrate_neutral_git() {
        use crate::event_store_adapters::GitAdapter;
        let dir = tempfile::tempdir().unwrap();
        let git = GitAdapter::new(dir.path());
        let mid = Uuid::new_v4();
        let cid = Uuid::new_v4();
        git.append(semantic_mapping_entry(mid, cid, 8500)).unwrap();

        let state = replay_from_adapter(&git).unwrap();
        assert_eq!(state.semantic_mappings.len(), 1);
        assert_eq!(
            state
                .semantic_mappings
                .get(&mid)
                .unwrap()
                .derived_constraint_id,
            cid
        );
    }

    // ── Correction finding replay tests ─────────────────────────

    fn correction_entry(finding_id: Uuid, decision_id: Uuid) -> EventStoreEntry {
        use bicameral_api::constraint::LifecycleReviewState;
        use bicameral_api::drift::FindingSeverity;
        use bicameral_api::grounding::{CodeRegionRef, CorrectionFinding, SuggestedAction};
        let finding = CorrectionFinding {
            id: finding_id,
            summary: "grounded code region changed".to_string(),
            code_region: CodeRegionRef {
                provider: "github".to_string(),
                repo: Some("BicameralAI/bicameral-bot".to_string()),
                path: "src/lib.rs".to_string(),
                start_line: Some(10),
                end_line: Some(20),
                git_ref: Some("main".to_string()),
                url: None,
            },
            source_doc: None,
            pr: None,
            related_decision_ids: vec![decision_id],
            related_constraint_ids: Vec::new(),
            evidence_refs: Vec::new(),
            confidence_bps: 9000,
            severity: FindingSeverity::High,
            review_state: LifecycleReviewState::Proposed,
            suggested_action: SuggestedAction::UpdateSourceDoc,
            reported_by: None,
            detected_at: chrono::Utc::now(),
        };
        EventStoreEntry::new(
            EventType::RaiseCorrectionFinding,
            serde_json::to_value(&RaiseCorrectionFindingPayload { finding }).unwrap(),
            "detector".to_string(),
        )
    }

    #[test]
    fn replay_raises_correction_finding() {
        let adapter = MemoryAdapter::new();
        let fid = Uuid::new_v4();
        let did = Uuid::new_v4();
        adapter.append(correction_entry(fid, did)).unwrap();

        let state = replay_from_adapter(&adapter).unwrap();
        assert_eq!(state.correction_findings.len(), 1);
        let f = state.correction_findings.get(&fid).unwrap();
        assert_eq!(f.related_decision_ids, vec![did]);
        assert_eq!(
            f.review_state,
            bicameral_api::constraint::LifecycleReviewState::Proposed
        );
    }

    #[test]
    fn replay_correction_finding_idempotent_by_id() {
        let adapter = MemoryAdapter::new();
        let fid = Uuid::new_v4();
        adapter
            .append(correction_entry(fid, Uuid::new_v4()))
            .unwrap();
        adapter
            .append(correction_entry(fid, Uuid::new_v4()))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();
        assert_eq!(state.correction_findings.len(), 1);
    }

    #[test]
    fn replay_correction_finding_substrate_neutral_git() {
        use crate::event_store_adapters::GitAdapter;
        let dir = tempfile::tempdir().unwrap();
        let git = GitAdapter::new(dir.path());
        let fid = Uuid::new_v4();
        git.append(correction_entry(fid, Uuid::new_v4())).unwrap();

        let state = replay_from_adapter(&git).unwrap();
        assert_eq!(state.correction_findings.len(), 1);
        assert!(state.correction_findings.contains_key(&fid));
    }

    #[test]
    fn replay_canonical_constraint_id_is_replay_stable() {
        let adapter = MemoryAdapter::new();
        promote(&adapter, "jira://STABLE-1", "idem-stable");

        // The same event log replayed twice yields the same canonical id, and
        // that id is the deterministic v5 over the promotion key.
        let id1 = replay_from_adapter(&adapter)
            .unwrap()
            .canonical_constraints
            .get("idem-stable")
            .unwrap()
            .id;
        let id2 = replay_from_adapter(&adapter)
            .unwrap()
            .canonical_constraints
            .get("idem-stable")
            .unwrap()
            .id;
        assert_eq!(id1, id2, "canonical constraint id must be replay-stable");
        assert_eq!(id1, Uuid::new_v5(&Uuid::NAMESPACE_OID, b"idem-stable"));
    }

    // ── Binding revalidation replay tests ───────────────────────

    fn make_revalidation_payload(
        decision_id: Uuid,
        binding_id: Uuid,
        outcome: bicameral_api::event_store::RevalidationOutcome,
    ) -> RevalidateBindingPayload {
        use bicameral_api::evidence::BindingType;
        RevalidateBindingPayload {
            decision_id,
            binding_id,
            root_binding_id: binding_id,
            binding_type: BindingType::CodeRegion,
            target: "src/lib.rs:10-20".to_string(),
            original_graph_snapshot_id: "gs-original".to_string(),
            original_validated_sha: "aaa111".to_string(),
            current_graph_snapshot_id: "gs-revalidated".to_string(),
            current_validated_sha: "bbb222".to_string(),
            evidence_state: bicameral_api::evidence::GraphEvidenceState::Verified,
            content_hash: Some("sha256:newcontent".to_string()),
            line_start: Some(10),
            line_end: Some(20),
            outcome,
            symbol_identity: None,
            replacement_binding_id: None,
            governance_id: None,
        }
    }

    // ── Tracking-state replay tests (issue #191, ADR-0022) ───────

    #[test]
    fn replay_accept_candidate_materializes_tracked_decision() {
        let adapter = MemoryAdapter::new();
        let cand_id = ingest_and_get_id(&adapter, "jira://PROJ-191a");
        let dec_id = emit_accept(&adapter, cand_id);

        let state = replay_from_adapter(&adapter).unwrap();
        let decision = state.decisions.get(&dec_id).unwrap();

        assert_eq!(
            decision.tracking_state,
            bicameral_api::candidate::TrackingState::Tracked,
            "promoted Decision must default to Tracked per ADR-0022"
        );
        assert_eq!(
            decision.signoff_state,
            SignoffState::Proposed,
            "freshly promoted Decision must have signoff_state == Proposed"
        );
    }

    #[test]
    fn replay_approve_signoff_leaves_tracking_unchanged() {
        let adapter = MemoryAdapter::new();
        let cand_id = ingest_and_get_id(&adapter, "jira://PROJ-191b");
        let dec_id = emit_accept(&adapter, cand_id);

        adapter
            .append(EventStoreEntry::new(
                EventType::ApproveSignoff,
                serde_json::json!({ "decision_id": dec_id.to_string() }),
                "reviewer".to_string(),
            ))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();
        let decision = state.decisions.get(&dec_id).unwrap();

        assert_eq!(
            decision.signoff_state,
            SignoffState::Approved,
            "signoff must transition to Approved"
        );
        assert_eq!(
            decision.tracking_state,
            bicameral_api::candidate::TrackingState::Tracked,
            "approve_signoff must not toggle tracking_state"
        );
    }

    #[test]
    fn replay_reject_signoff_leaves_tracking_unchanged() {
        let adapter = MemoryAdapter::new();
        let cand_id = ingest_and_get_id(&adapter, "jira://PROJ-191c");
        let dec_id = emit_accept(&adapter, cand_id);

        adapter
            .append(EventStoreEntry::new(
                EventType::RejectSignoff,
                serde_json::json!({ "decision_id": dec_id.to_string() }),
                "reviewer".to_string(),
            ))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();
        let decision = state.decisions.get(&dec_id).unwrap();

        assert_eq!(
            decision.signoff_state,
            SignoffState::Rejected,
            "signoff must transition to Rejected"
        );
        assert_eq!(
            decision.tracking_state,
            bicameral_api::candidate::TrackingState::Tracked,
            "reject_signoff must not toggle tracking_state"
        );
    }

    #[test]
    fn replay_supersede_decision_leaves_tracking_unchanged() {
        let adapter = MemoryAdapter::new();
        let cand_id = ingest_and_get_id(&adapter, "jira://PROJ-191d");
        let dec_id = emit_accept(&adapter, cand_id);

        adapter
            .append(EventStoreEntry::new(
                EventType::SupersedeDecision,
                serde_json::json!({ "decision_id": dec_id.to_string() }),
                "system".to_string(),
            ))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();
        let decision = state.decisions.get(&dec_id).unwrap();

        assert_eq!(decision.signoff_state, SignoffState::Superseded);
        assert_eq!(
            decision.tracking_state,
            bicameral_api::candidate::TrackingState::Tracked,
            "supersede must not toggle tracking_state"
        );
    }

    #[test]
    fn replay_full_lifecycle_preserves_tracking_through_signoff_transitions() {
        let adapter = MemoryAdapter::new();
        let cand_id = ingest_and_get_id(&adapter, "jira://PROJ-191e");
        let dec_id = emit_accept(&adapter, cand_id);

        // Approve signoff
        adapter
            .append(EventStoreEntry::new(
                EventType::ApproveSignoff,
                serde_json::json!({ "decision_id": dec_id.to_string() }),
                "reviewer".to_string(),
            ))
            .unwrap();

        // Then supersede
        adapter
            .append(EventStoreEntry::new(
                EventType::SupersedeDecision,
                serde_json::json!({ "decision_id": dec_id.to_string() }),
                "system".to_string(),
            ))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();
        let decision = state.decisions.get(&dec_id).unwrap();

        assert_eq!(
            decision.signoff_state,
            SignoffState::Superseded,
            "signoff must reflect the final transition"
        );
        assert_eq!(
            decision.tracking_state,
            bicameral_api::candidate::TrackingState::Tracked,
            "tracking must remain Tracked through all signoff transitions"
        );
    }

    #[test]
    fn replay_legacy_decision_without_tracking_field_defaults_to_tracked() {
        let adapter = MemoryAdapter::new();

        // Simulate a legacy AcceptCandidate event whose payload was written
        // before the tracking_state field existed. The Decision constructor
        // + serde default ensures backward compatibility.
        let cand_id = ingest_and_get_id(&adapter, "jira://PROJ-191-legacy");
        let dec_id = emit_accept(&adapter, cand_id);

        let state = replay_from_adapter(&adapter).unwrap();
        let decision = state.decisions.get(&dec_id).unwrap();

        assert_eq!(
            decision.tracking_state,
            bicameral_api::candidate::TrackingState::Tracked,
            "legacy events without tracking_state must default to Tracked"
        );
    }

    // ── Binding revalidation replay tests ───────────────────────

    #[test]
    fn replay_revalidate_binding_accepted() {
        use bicameral_api::event_store::RevalidationOutcome;
        use bicameral_api::evidence::{BindingConfidence, BindingEvidence, BindingType};

        let adapter = MemoryAdapter::new();

        // Set up a decision with a binding
        let cand_id = ingest_and_get_id(&adapter, "jira://PROJ-100");
        let dec_id = emit_accept(&adapter, cand_id);
        let binding_id = Uuid::new_v4();

        let mut binding = BindingEvidence::new(
            dec_id,
            BindingType::CodeRegion,
            "src/lib.rs:10-20".to_string(),
            BindingConfidence::High,
        );
        binding.id = binding_id;
        binding.graph_snapshot_id = Some("gs-original".to_string());

        let bind_payload = BindToCodePayload {
            decision_id: dec_id,
            binding,
            graph_snapshot_id: "gs-original".to_string(),
            validated_sha: "aaa111".to_string(),
            evidence_state: bicameral_api::evidence::GraphEvidenceState::Verified,
            content_hash: Some("sha256:oldcontent".to_string()),
            line_start: Some(10),
            line_end: Some(20),
        };
        adapter
            .append(EventStoreEntry::new(
                EventType::BindToCode,
                serde_json::to_value(&bind_payload).unwrap(),
                "test".to_string(),
            ))
            .unwrap();

        // Emit a confirmed revalidation at a later snapshot
        let reval_payload =
            make_revalidation_payload(dec_id, binding_id, RevalidationOutcome::Confirmed);
        adapter
            .append(EventStoreEntry::new(
                EventType::RevalidateBinding,
                serde_json::to_value(&reval_payload).unwrap(),
                "reviewer-bot".to_string(),
            ))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();

        // Original binding is preserved immutably
        let decision_bindings = state.bindings.get(&dec_id).unwrap();
        assert_eq!(decision_bindings.len(), 1);
        assert_eq!(decision_bindings[0].id, binding_id);

        // Revalidation history accumulated
        let revals = state.binding_revalidations.get(&binding_id).unwrap();
        assert_eq!(revals.len(), 1);
        assert_eq!(revals[0].outcome, RevalidationOutcome::Confirmed);
        assert_eq!(revals[0].current_graph_snapshot_id, "gs-revalidated");
        assert_eq!(revals[0].current_validated_sha, "bbb222");
        assert_eq!(revals[0].original_graph_snapshot_id, "gs-original");
    }

    #[test]
    fn replay_revalidate_binding_replacement_lineage() {
        use bicameral_api::event_store::{RevalidationOutcome, SymbolIdentity};
        use bicameral_api::evidence::{BindingConfidence, BindingEvidence, BindingType};

        let adapter = MemoryAdapter::new();

        let cand_id = ingest_and_get_id(&adapter, "jira://PROJ-200");
        let dec_id = emit_accept(&adapter, cand_id);
        let original_binding_id = Uuid::new_v4();
        let replacement_binding_id = Uuid::new_v4();

        // Original binding (symbol type)
        let mut binding = BindingEvidence::new(
            dec_id,
            BindingType::Symbol,
            "my_module::process_data".to_string(),
            BindingConfidence::High,
        );
        binding.id = original_binding_id;
        binding.graph_snapshot_id = Some("gs-v1".to_string());

        let bind_payload = BindToCodePayload {
            decision_id: dec_id,
            binding,
            graph_snapshot_id: "gs-v1".to_string(),
            validated_sha: "sha-v1".to_string(),
            evidence_state: bicameral_api::evidence::GraphEvidenceState::Verified,
            content_hash: Some("sha256:sym-v1".to_string()),
            line_start: Some(50),
            line_end: Some(80),
        };
        adapter
            .append(EventStoreEntry::new(
                EventType::BindToCode,
                serde_json::to_value(&bind_payload).unwrap(),
                "test".to_string(),
            ))
            .unwrap();

        // Emit a replacement revalidation (symbol moved/renamed)
        let reval_payload = RevalidateBindingPayload {
            decision_id: dec_id,
            binding_id: original_binding_id,
            root_binding_id: original_binding_id,
            binding_type: BindingType::Symbol,
            target: "my_module::transform_data".to_string(),
            original_graph_snapshot_id: "gs-v1".to_string(),
            original_validated_sha: "sha-v1".to_string(),
            current_graph_snapshot_id: "gs-v2".to_string(),
            current_validated_sha: "sha-v2".to_string(),
            evidence_state: bicameral_api::evidence::GraphEvidenceState::Verified,
            content_hash: Some("sha256:sym-v2".to_string()),
            line_start: Some(55),
            line_end: Some(90),
            outcome: RevalidationOutcome::Replaced,
            symbol_identity: Some(SymbolIdentity {
                symbol_name: "transform_data".to_string(),
                qualified_name: Some("my_module::transform_data".to_string()),
                structural_hash: Some("struct-hash-v2".to_string()),
                resolver_identity: Some("tree-sitter-rust@0.21".to_string()),
                parser_metadata: Some("rust/tags.scm".to_string()),
            }),
            replacement_binding_id: Some(replacement_binding_id),
            governance_id: None,
        };
        adapter
            .append(EventStoreEntry::new(
                EventType::RevalidateBinding,
                serde_json::to_value(&reval_payload).unwrap(),
                "revalidation-bot".to_string(),
            ))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();

        // Original binding preserved
        let decision_bindings = state.bindings.get(&dec_id).unwrap();
        assert_eq!(decision_bindings.len(), 1);
        assert_eq!(decision_bindings[0].id, original_binding_id);
        assert_eq!(decision_bindings[0].target, "my_module::process_data");

        // Revalidation history tracks the replacement
        let revals = state
            .binding_revalidations
            .get(&original_binding_id)
            .unwrap();
        assert_eq!(revals.len(), 1);
        assert_eq!(revals[0].outcome, RevalidationOutcome::Replaced);
        assert_eq!(
            revals[0].replacement_binding_id,
            Some(replacement_binding_id)
        );
        assert_eq!(revals[0].target, "my_module::transform_data");

        // Symbol identity is preserved
        let sym = revals[0].symbol_identity.as_ref().unwrap();
        assert_eq!(sym.symbol_name, "transform_data");
        assert_eq!(
            sym.qualified_name.as_deref(),
            Some("my_module::transform_data")
        );
        assert_eq!(sym.structural_hash.as_deref(), Some("struct-hash-v2"));
    }

    #[test]
    fn replay_revalidate_binding_failed_distinguishable() {
        use bicameral_api::event_store::RevalidationOutcome;
        use bicameral_api::evidence::{BindingConfidence, BindingEvidence, BindingType};

        let adapter = MemoryAdapter::new();

        let cand_id = ingest_and_get_id(&adapter, "jira://PROJ-300");
        let dec_id = emit_accept(&adapter, cand_id);
        let binding_id = Uuid::new_v4();

        let mut binding = BindingEvidence::new(
            dec_id,
            BindingType::FilePath,
            "src/old_file.rs".to_string(),
            BindingConfidence::High,
        );
        binding.id = binding_id;
        binding.graph_snapshot_id = Some("gs-init".to_string());

        let bind_payload = BindToCodePayload {
            decision_id: dec_id,
            binding,
            graph_snapshot_id: "gs-init".to_string(),
            validated_sha: "sha-init".to_string(),
            evidence_state: bicameral_api::evidence::GraphEvidenceState::Verified,
            content_hash: None,
            line_start: None,
            line_end: None,
        };
        adapter
            .append(EventStoreEntry::new(
                EventType::BindToCode,
                serde_json::to_value(&bind_payload).unwrap(),
                "test".to_string(),
            ))
            .unwrap();

        // Failed revalidation — file was deleted
        let mut reval_payload =
            make_revalidation_payload(dec_id, binding_id, RevalidationOutcome::Failed);
        reval_payload.evidence_state = bicameral_api::evidence::GraphEvidenceState::NotFound;
        reval_payload.content_hash = None;
        adapter
            .append(EventStoreEntry::new(
                EventType::RevalidateBinding,
                serde_json::to_value(&reval_payload).unwrap(),
                "ci-bot".to_string(),
            ))
            .unwrap();

        // Then a successful re-confirmation later
        let reval_confirmed =
            make_revalidation_payload(dec_id, binding_id, RevalidationOutcome::Confirmed);
        adapter
            .append(EventStoreEntry::new(
                EventType::RevalidateBinding,
                serde_json::to_value(&reval_confirmed).unwrap(),
                "reviewer".to_string(),
            ))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();

        let revals = state.binding_revalidations.get(&binding_id).unwrap();
        assert_eq!(revals.len(), 2);

        // First is failed
        assert_eq!(revals[0].outcome, RevalidationOutcome::Failed);
        assert_eq!(
            revals[0].evidence_state,
            bicameral_api::evidence::GraphEvidenceState::NotFound
        );

        // Second is confirmed — distinguishable from the failed one
        assert_eq!(revals[1].outcome, RevalidationOutcome::Confirmed);
        assert_eq!(
            revals[1].evidence_state,
            bicameral_api::evidence::GraphEvidenceState::Verified
        );
    }

    #[test]
    fn replay_revalidation_substrate_neutral_drive_folder() {
        use crate::event_store_adapters::DriveFolderAdapter;
        use bicameral_api::event_store::RevalidationOutcome;
        use bicameral_api::evidence::{BindingConfidence, BindingEvidence, BindingType};

        let dir = tempfile::tempdir().unwrap();
        let adapter = DriveFolderAdapter::new(dir.path());

        let cand_id = {
            let entry = make_ingest_entry("gdrive://doc-reval", true);
            let payload: IngestPayload = serde_json::from_value(entry.payload.clone()).unwrap();
            let id = payload.candidate.id;
            adapter.append(entry).unwrap();
            id
        };

        let dec_id = Uuid::new_v4();
        adapter
            .append(EventStoreEntry::new(
                EventType::AcceptCandidate,
                serde_json::json!({
                    "candidate_id": cand_id.to_string(),
                    "decision_id": dec_id.to_string(),
                }),
                "test".to_string(),
            ))
            .unwrap();

        let binding_id = Uuid::new_v4();
        let mut binding = BindingEvidence::new(
            dec_id,
            BindingType::CodeRegion,
            "main.rs:1-10".to_string(),
            BindingConfidence::Medium,
        );
        binding.id = binding_id;

        let bind_payload = BindToCodePayload {
            decision_id: dec_id,
            binding,
            graph_snapshot_id: "gs-drive".to_string(),
            validated_sha: "sha-drive".to_string(),
            evidence_state: bicameral_api::evidence::GraphEvidenceState::Verified,
            content_hash: None,
            line_start: Some(1),
            line_end: Some(10),
        };
        adapter
            .append(EventStoreEntry::new(
                EventType::BindToCode,
                serde_json::to_value(&bind_payload).unwrap(),
                "test".to_string(),
            ))
            .unwrap();

        // Revalidation on Drive substrate
        let reval = make_revalidation_payload(dec_id, binding_id, RevalidationOutcome::Confirmed);
        adapter
            .append(EventStoreEntry::new(
                EventType::RevalidateBinding,
                serde_json::to_value(&reval).unwrap(),
                "drive-sync".to_string(),
            ))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();
        assert_eq!(state.decisions.len(), 1);
        let revals = state.binding_revalidations.get(&binding_id).unwrap();
        assert_eq!(revals.len(), 1);
        assert_eq!(revals[0].outcome, RevalidationOutcome::Confirmed);
    }

    #[test]
    fn replay_revalidation_substrate_neutral_git() {
        use crate::event_store_adapters::GitAdapter;
        use bicameral_api::event_store::RevalidationOutcome;
        use bicameral_api::evidence::{BindingConfidence, BindingEvidence, BindingType};

        let dir = tempfile::tempdir().unwrap();
        let adapter = GitAdapter::new(dir.path());

        let entry = make_ingest_entry("github://pr-reval", true);
        let payload: IngestPayload = serde_json::from_value(entry.payload.clone()).unwrap();
        let cand_id = payload.candidate.id;
        adapter.append(entry).unwrap();

        let dec_id = Uuid::new_v4();
        adapter
            .append(EventStoreEntry::new(
                EventType::AcceptCandidate,
                serde_json::json!({
                    "candidate_id": cand_id.to_string(),
                    "decision_id": dec_id.to_string(),
                }),
                "test".to_string(),
            ))
            .unwrap();

        let binding_id = Uuid::new_v4();
        let mut binding = BindingEvidence::new(
            dec_id,
            BindingType::Symbol,
            "crate::foo::bar".to_string(),
            BindingConfidence::High,
        );
        binding.id = binding_id;

        let bind_payload = BindToCodePayload {
            decision_id: dec_id,
            binding,
            graph_snapshot_id: "gs-git".to_string(),
            validated_sha: "sha-git".to_string(),
            evidence_state: bicameral_api::evidence::GraphEvidenceState::Verified,
            content_hash: Some("sha256:git-content".to_string()),
            line_start: Some(100),
            line_end: Some(150),
        };
        adapter
            .append(EventStoreEntry::new(
                EventType::BindToCode,
                serde_json::to_value(&bind_payload).unwrap(),
                "test".to_string(),
            ))
            .unwrap();

        // Revalidation on Git substrate
        let reval = make_revalidation_payload(dec_id, binding_id, RevalidationOutcome::Confirmed);
        adapter
            .append(EventStoreEntry::new(
                EventType::RevalidateBinding,
                serde_json::to_value(&reval).unwrap(),
                "git-bot".to_string(),
            ))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();
        let revals = state.binding_revalidations.get(&binding_id).unwrap();
        assert_eq!(revals.len(), 1);
        assert_eq!(revals[0].outcome, RevalidationOutcome::Confirmed);
    }
}
