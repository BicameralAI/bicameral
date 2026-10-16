//! Graph readiness view — surfaces graph state honestly without mutating
//! Decision lifecycle.
//!
//! Per ADR-0010, ADR-0011, ADR-0015, and ADR-0022:
//!
//! - Graph readiness and evidence states are graph/view state, not Decision
//!   lifecycle events.
//! - Non-verified `GraphEvidenceState` values must not directly change
//!   signoff, compliance, review state, or ledger materialization.
//! - Unresolved graph grounding must not produce `collision_pending`.
//! - Surfaces can still show stale/not_configured/indexing/failed/ambiguous
//!   states honestly via `GraphReadinessView`.
//! - Signoff state is never an input to graph readiness or compliance
//!   determination. Signoff alone cannot prove compliance, readiness,
//!   or enforcement (issue #193).
//!
//! This module provides the boundary guard functions that enforce these
//! invariants.

use bicameral_api::dashboard::{ComplianceState, SignoffState};
use bicameral_api::evidence::{
    GraphEvidenceState, GraphReadiness, GraphReadinessView, GraphScopeCoverage,
};

/// Returns `true` when a `GraphEvidenceState` is verified — the only state
/// that may support binding materialization, compliance conclusions, or
/// blocking governance results.
pub fn is_verified_evidence(state: GraphEvidenceState) -> bool {
    state == GraphEvidenceState::Verified
}

/// Returns `true` when a `GraphEvidenceState` is a non-verified graph state
/// that must never directly mutate Decision lifecycle (signoff, compliance,
/// review state, ledger materialization) or produce `collision_pending`.
pub fn is_non_verified_graph_state(state: GraphEvidenceState) -> bool {
    !is_verified_evidence(state)
}

/// Asserts the graph-readiness boundary invariant: a non-verified graph
/// evidence state must not produce any lifecycle-mutating outcome.
///
/// Returns `None` for non-verified states (no lifecycle mutation allowed)
/// and `Some(state)` for `Verified` (may proceed to governance evaluation).
pub fn lifecycle_gate(evidence_state: GraphEvidenceState) -> Option<GraphEvidenceState> {
    if is_verified_evidence(evidence_state) {
        Some(evidence_state)
    } else {
        None
    }
}

/// Maps a `GraphReadiness` to the `ComplianceState` that a Ledger View
/// should show when graph readiness prevents verified evidence.
///
/// Only `Ready` readiness can eventually produce `Reflected`/`Drifted`/etc.
/// All other readiness states produce `Pending` (graph not yet available)
/// or `Ungrounded` (no graph configured).
pub fn compliance_for_readiness(readiness: GraphReadiness) -> ComplianceState {
    match readiness {
        GraphReadiness::Ready => {
            // Ready means the graph is available — actual compliance depends
            // on binding evaluation, not on readiness alone. Return Pending
            // as a safe default; the ComplianceEvaluator determines the real
            // state.
            ComplianceState::Pending
        }
        GraphReadiness::Indexing | GraphReadiness::Stale | GraphReadiness::Failed => {
            ComplianceState::Pending
        }
        GraphReadiness::NotConfigured => ComplianceState::Ungrounded,
    }
}

/// Asserts that a given `SignoffState` was NOT produced by unresolved graph
/// grounding. Per ADR-0011, only reviewed/verified/policy-accepted conflict
/// evidence may trigger `CollisionPending`. Unresolved graph grounding
/// (stale, not_configured, failed, ambiguous, approximate) must never produce
/// `CollisionPending`.
///
/// Returns `true` if the signoff state is safe (not collision_pending), or
/// if collision_pending was produced by verified evidence. Returns `false`
/// if `CollisionPending` was produced without verified evidence.
pub fn validate_signoff_not_from_graph_grounding(
    signoff: SignoffState,
    evidence_state: Option<GraphEvidenceState>,
) -> bool {
    if signoff != SignoffState::CollisionPending {
        return true;
    }
    // CollisionPending is only valid when produced by verified conflict evidence.
    matches!(evidence_state, Some(GraphEvidenceState::Verified))
}

/// Build a `GraphReadinessView` for a workspace scope.
///
/// This is the canonical way to create view-layer graph readiness state
/// for dashboard or surface display.
pub fn build_readiness_view(
    readiness: GraphReadiness,
    graph_snapshot_id: Option<String>,
    detail: Option<String>,
) -> GraphReadinessView {
    GraphReadinessView {
        readiness,
        graph_snapshot_id,
        detail,
        scope_coverage: None,
    }
}

/// Build a `GraphReadinessView` with explicit scope coverage.
///
/// Use this when the caller can determine whether the graph snapshot
/// covers the entire worktree (`Full`) or only a subset of indexed
/// file types (`Partial`). Partial scope must not be treated as full
/// branch coverage.
pub fn build_readiness_view_with_scope(
    readiness: GraphReadiness,
    graph_snapshot_id: Option<String>,
    detail: Option<String>,
    scope_coverage: GraphScopeCoverage,
) -> GraphReadinessView {
    GraphReadinessView {
        readiness,
        graph_snapshot_id,
        detail,
        scope_coverage: Some(scope_coverage),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_api::evidence::SnapshotResolution;

    // ── Acceptance criterion: non-verified states never mutate lifecycle ──

    #[test]
    fn stale_evidence_does_not_pass_lifecycle_gate() {
        assert!(lifecycle_gate(GraphEvidenceState::UnknownStale).is_none());
    }

    #[test]
    fn not_indexed_evidence_does_not_pass_lifecycle_gate() {
        assert!(lifecycle_gate(GraphEvidenceState::UnknownNotIndexed).is_none());
    }

    #[test]
    fn ambiguous_evidence_does_not_pass_lifecycle_gate() {
        assert!(lifecycle_gate(GraphEvidenceState::Ambiguous).is_none());
    }

    #[test]
    fn approximate_candidate_does_not_pass_lifecycle_gate() {
        assert!(lifecycle_gate(GraphEvidenceState::ApproximateCandidate).is_none());
    }

    #[test]
    fn unsupported_evidence_does_not_pass_lifecycle_gate() {
        assert!(lifecycle_gate(GraphEvidenceState::Unsupported).is_none());
    }

    #[test]
    fn not_found_evidence_does_not_pass_lifecycle_gate() {
        assert!(lifecycle_gate(GraphEvidenceState::NotFound).is_none());
    }

    #[test]
    fn verified_evidence_passes_lifecycle_gate() {
        assert_eq!(
            lifecycle_gate(GraphEvidenceState::Verified),
            Some(GraphEvidenceState::Verified)
        );
    }

    // ── Acceptance criterion: unresolved graph grounding never produces
    //    collision_pending ──

    #[test]
    fn stale_evidence_must_not_produce_collision_pending() {
        assert!(
            !validate_signoff_not_from_graph_grounding(
                SignoffState::CollisionPending,
                Some(GraphEvidenceState::UnknownStale),
            ),
            "Stale graph evidence must not produce collision_pending"
        );
    }

    #[test]
    fn not_indexed_evidence_must_not_produce_collision_pending() {
        assert!(
            !validate_signoff_not_from_graph_grounding(
                SignoffState::CollisionPending,
                Some(GraphEvidenceState::UnknownNotIndexed),
            ),
            "UnknownNotIndexed graph evidence must not produce collision_pending"
        );
    }

    #[test]
    fn ambiguous_evidence_must_not_produce_collision_pending() {
        assert!(
            !validate_signoff_not_from_graph_grounding(
                SignoffState::CollisionPending,
                Some(GraphEvidenceState::Ambiguous),
            ),
            "Ambiguous graph evidence must not produce collision_pending"
        );
    }

    #[test]
    fn approximate_candidate_must_not_produce_collision_pending() {
        assert!(
            !validate_signoff_not_from_graph_grounding(
                SignoffState::CollisionPending,
                Some(GraphEvidenceState::ApproximateCandidate),
            ),
            "ApproximateCandidate must not produce collision_pending"
        );
    }

    #[test]
    fn no_evidence_must_not_produce_collision_pending() {
        assert!(
            !validate_signoff_not_from_graph_grounding(SignoffState::CollisionPending, None),
            "Absent graph evidence must not produce collision_pending"
        );
    }

    #[test]
    fn verified_evidence_may_produce_collision_pending() {
        assert!(
            validate_signoff_not_from_graph_grounding(
                SignoffState::CollisionPending,
                Some(GraphEvidenceState::Verified),
            ),
            "Verified conflict evidence is allowed to produce collision_pending"
        );
    }

    #[test]
    fn non_collision_signoff_is_always_valid() {
        for signoff in [
            SignoffState::Proposed,
            SignoffState::Approved,
            SignoffState::Rejected,
            SignoffState::Superseded,
        ] {
            assert!(
                validate_signoff_not_from_graph_grounding(signoff, None),
                "Non-collision signoff states are always valid"
            );
        }
    }

    // ── Acceptance criterion: surfaces can still show graph readiness states ──

    #[test]
    fn stale_readiness_surfaces_honestly() {
        let view = build_readiness_view(
            GraphReadiness::Stale,
            None,
            Some("Index version mismatch".to_string()),
        );
        assert_eq!(view.readiness, GraphReadiness::Stale);
        assert_eq!(view.detail.as_deref(), Some("Index version mismatch"));
    }

    #[test]
    fn not_configured_readiness_surfaces_honestly() {
        let view = build_readiness_view(
            GraphReadiness::NotConfigured,
            None,
            Some("Local graph support is unavailable".to_string()),
        );
        assert_eq!(view.readiness, GraphReadiness::NotConfigured);
    }

    #[test]
    fn indexing_readiness_surfaces_honestly() {
        let view = build_readiness_view(GraphReadiness::Indexing, None, None);
        assert_eq!(view.readiness, GraphReadiness::Indexing);
    }

    #[test]
    fn failed_readiness_surfaces_honestly() {
        let view = build_readiness_view(
            GraphReadiness::Failed,
            None,
            Some("Indexing failed".to_string()),
        );
        assert_eq!(view.readiness, GraphReadiness::Failed);
    }

    #[test]
    fn ready_readiness_surfaces_with_snapshot() {
        let view = build_readiness_view(GraphReadiness::Ready, Some("gs-abc123".to_string()), None);
        assert_eq!(view.readiness, GraphReadiness::Ready);
        assert_eq!(view.graph_snapshot_id.as_deref(), Some("gs-abc123"));
    }

    // ── Acceptance criterion: compliance_for_readiness mapping ──

    #[test]
    fn stale_readiness_maps_to_pending_compliance() {
        assert_eq!(
            compliance_for_readiness(GraphReadiness::Stale),
            ComplianceState::Pending
        );
    }

    #[test]
    fn not_configured_readiness_maps_to_ungrounded_compliance() {
        assert_eq!(
            compliance_for_readiness(GraphReadiness::NotConfigured),
            ComplianceState::Ungrounded
        );
    }

    #[test]
    fn indexing_readiness_maps_to_pending_compliance() {
        assert_eq!(
            compliance_for_readiness(GraphReadiness::Indexing),
            ComplianceState::Pending
        );
    }

    #[test]
    fn failed_readiness_maps_to_pending_compliance() {
        assert_eq!(
            compliance_for_readiness(GraphReadiness::Failed),
            ComplianceState::Pending
        );
    }

    // ── GraphReadinessView construction ──

    #[test]
    fn readiness_view_from_stale_index_resolution() {
        let resolution = SnapshotResolution::StaleIndex {
            requested_ref: "main".to_string(),
            detail: "index version mismatch".to_string(),
        };
        let view = GraphReadinessView::from_resolution(&resolution);
        assert_eq!(view.readiness, GraphReadiness::Stale);
        assert!(view.graph_snapshot_id.is_none());
        assert_eq!(view.detail.as_deref(), Some("index version mismatch"));
    }

    #[test]
    fn readiness_view_from_not_available_resolution() {
        let resolution = SnapshotResolution::RefNotAvailable {
            requested_ref: "feature/x".to_string(),
            detail: "ref not found locally".to_string(),
        };
        let view = GraphReadinessView::from_resolution(&resolution);
        assert_eq!(view.readiness, GraphReadiness::Failed);
        assert!(view.graph_snapshot_id.is_none());
    }

    #[test]
    fn readiness_view_from_hosted_required_resolution() {
        let resolution = SnapshotResolution::HostedRequired {
            requested_ref: "pr/42".to_string(),
            detail: "requires team graph".to_string(),
        };
        let view = GraphReadinessView::from_resolution(&resolution);
        assert_eq!(view.readiness, GraphReadiness::NotConfigured);
    }

    // ── Exhaustive: every non-verified GraphEvidenceState is classified ──

    #[test]
    fn all_non_verified_states_are_classified() {
        let non_verified = [
            GraphEvidenceState::NotFound,
            GraphEvidenceState::UnknownNotIndexed,
            GraphEvidenceState::UnknownStale,
            GraphEvidenceState::Ambiguous,
            GraphEvidenceState::Unsupported,
            GraphEvidenceState::ApproximateCandidate,
        ];
        for state in non_verified {
            assert!(
                is_non_verified_graph_state(state),
                "{state:?} should be non-verified"
            );
            assert!(
                lifecycle_gate(state).is_none(),
                "{state:?} must not pass lifecycle gate"
            );
        }
    }

    // ── Cross-check: existing compliance evaluator behavior ──

    #[test]
    fn compliance_evaluator_maps_non_verified_to_pending() {
        // This test validates that the ComplianceEvaluator's existing behavior
        // (mapping non-verified evidence states to Pending compliance) is
        // consistent with the graph-readiness boundary.
        use crate::compliance::evaluate_without_inspector;
        use bicameral_api::evidence::{BindingConfidence, BindingEvidence, BindingType};
        use uuid::Uuid;

        let decision_id = Uuid::new_v4();
        let binding = BindingEvidence::new(
            decision_id,
            BindingType::FilePath,
            "src/lib.rs".to_string(),
            BindingConfidence::High,
        );

        let verdict = evaluate_without_inspector(decision_id, &[binding]);

        // evaluate_without_inspector returns Pending, never CollisionPending
        assert_eq!(verdict.compliance_state, ComplianceState::Pending);
        assert_ne!(
            verdict.compliance_state,
            ComplianceState::Reflected,
            "Non-verified evidence must not produce Reflected compliance"
        );

        // The binding verdicts should use UnknownStale (graph not available)
        for bv in &verdict.binding_verdicts {
            assert_eq!(bv.evidence_state, GraphEvidenceState::UnknownStale);
        }
    }

    // ── Cross-check: bind_to_code rejects non-verified evidence ──

    #[test]
    fn bind_to_code_rejects_all_non_verified_states() {
        // This validates the existing bind_to_code gate: only Verified
        // evidence produces a Bound result. Non-verified states produce
        // Rejected, never lifecycle mutation.
        use crate::bind_to_code::{bind_to_code, BindToCodeRequest, BindToCodeResult};
        use crate::grounding::FileInspector;
        use bicameral_api::evidence::GraphSnapshot;
        use chrono::Utc;
        use uuid::Uuid;

        let dir = tempfile::tempdir().unwrap();
        let snap = GraphSnapshot {
            graph_snapshot_id: "gs-test".to_string(),
            repo_id: "test/repo".to_string(),
            commit_sha: "abc123".to_string(),
            index_version: "file-inspect-v1".to_string(),
            ref_name: None,
            merge_base_sha: None,
            indexed_at: Some(Utc::now()),
            index_source: None,
        };
        let inspector = FileInspector::new(dir.path(), snap.clone());
        let req = BindToCodeRequest {
            decision_id: Uuid::new_v4(),
            file: "nonexistent.rs".to_string(),
            line_start: None,
            line_end: None,
            symbol: None,
            explanation: None,
            validation_token: None,
        };

        let result = bind_to_code(&inspector, &snap, &req, None);

        // The file doesn't exist → NotFound → Rejected (not Bound)
        match result {
            BindToCodeResult::Rejected { evidence_state, .. } => {
                assert_ne!(
                    evidence_state,
                    GraphEvidenceState::Verified,
                    "Non-existent file must not produce Verified evidence"
                );
            }
            BindToCodeResult::Bound { .. } => {
                panic!("Non-existent file must not produce a Bound result");
            }
            BindToCodeResult::SnapshotMismatch { .. } => {
                panic!("No snapshot mismatch expected in this test");
            }
            BindToCodeResult::ValidationTokenUnsupported { .. } => {
                panic!("No validation token was provided in this test");
            }
        }
    }

    // ── ReviewLifecycleState boundary check ──

    #[test]
    fn graph_readiness_states_never_produce_collision_pending_review() {
        // Graph readiness states (stale, not_configured, indexing, failed)
        // must never result in CollisionPending review state. They should
        // produce at most NeedsGroundingReview or leave review unchanged.
        let non_ready_states = [
            GraphReadiness::Stale,
            GraphReadiness::NotConfigured,
            GraphReadiness::Indexing,
            GraphReadiness::Failed,
        ];

        for readiness in non_ready_states {
            let compliance = compliance_for_readiness(readiness);
            // Compliance should be Pending or Ungrounded, never something
            // that would trigger collision_pending
            assert!(
                compliance == ComplianceState::Pending || compliance == ComplianceState::Ungrounded,
                "GraphReadiness::{readiness:?} produced unexpected compliance: {compliance:?}"
            );
        }
    }

    // ── Signoff-independence invariant tests (issue #193, ADR-0022) ──

    #[test]
    fn signoff_alone_does_not_determine_graph_readiness() {
        // Graph readiness is derived from GraphReadiness / GraphEvidenceState,
        // not from SignoffState. compliance_for_readiness and lifecycle_gate
        // accept only graph state — signoff is never an input.
        //
        // This test asserts the structural boundary: the readiness helpers
        // in this module have no SignoffState parameter.
        for readiness in [
            GraphReadiness::Ready,
            GraphReadiness::Stale,
            GraphReadiness::NotConfigured,
            GraphReadiness::Indexing,
            GraphReadiness::Failed,
        ] {
            let compliance = compliance_for_readiness(readiness);
            // All results are valid graph-derived compliance states.
            assert!(
                matches!(
                    compliance,
                    ComplianceState::Pending | ComplianceState::Ungrounded
                ),
                "compliance_for_readiness must derive from graph state, not signoff"
            );
        }
    }

    #[test]
    fn signoff_approved_with_non_verified_evidence_does_not_pass_lifecycle_gate() {
        // Even when signoff is Approved, non-verified graph evidence must
        // still be blocked by the lifecycle gate. Signoff does not override
        // graph evidence requirements (ADR-0022).
        let non_verified = [
            GraphEvidenceState::UnknownStale,
            GraphEvidenceState::UnknownNotIndexed,
            GraphEvidenceState::Ambiguous,
            GraphEvidenceState::ApproximateCandidate,
            GraphEvidenceState::Unsupported,
            GraphEvidenceState::NotFound,
        ];

        for evidence in non_verified {
            // lifecycle_gate does not accept signoff — only evidence state.
            // Regardless of signoff approval, non-verified evidence is blocked.
            assert!(
                lifecycle_gate(evidence).is_none(),
                "Non-verified evidence {evidence:?} must not pass lifecycle gate \
                 regardless of signoff state"
            );
        }
    }
}
