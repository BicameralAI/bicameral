//! Code-source refresh — wires code evidence refresh to GraphStore readiness.
//!
//! Per issue #194 and ADR-0010/ADR-0022:
//!
//! - Code sources are behaviorally Sources but have a dedicated graph
//!   implementation substrate. Refreshing code evidence means
//!   resolving/revalidating graph snapshots.
//! - The refresh path delegates to `GraphReadiness`, `SnapshotResolution`,
//!   and `FileInspector` — the existing GraphStore/GraphSnapshot/GraphSlice
//!   readiness concepts.
//! - Accepted `BindingEvidence` remains immutable. Refresh produces derived
//!   `CodeSourceCurrentness` state, not signoff/compliance mutation.
//! - Stale/unknown graph states return typed readiness or evidence states,
//!   never blocking governance results in V1.

use bicameral_api::evidence::{
    BindingEvidence, CodeSourceCurrentness, CodeSourceRefreshResult, GraphEvidenceState,
    GraphReadiness,
};
use uuid::Uuid;

use crate::grounding::{FileInspector, SnapshotResolver};

/// Evaluate code-source currentness for a single binding against the
/// current workspace graph state.
///
/// This is a read-model query. It inspects the binding target against the
/// current `GraphSnapshot` and returns a derived `CodeSourceCurrentness`
/// without mutating the binding, signoff, compliance, or ledger state.
///
/// The function delegates to `SnapshotResolver` for snapshot resolution
/// and `FileInspector` for file-level evidence inspection — the same
/// GraphStore readiness concepts used by `bind_to_code` and
/// `ComplianceEvaluator`.
pub fn evaluate_binding_currentness(
    resolver: &SnapshotResolver,
    decision_id: Uuid,
    binding: &BindingEvidence,
) -> CodeSourceRefreshResult {
    let resolution = resolver.resolve_ref("HEAD");
    let readiness = resolution.readiness();

    match readiness {
        GraphReadiness::Ready => {
            let snapshot = match resolution.snapshot() {
                Some(s) => s,
                None => {
                    return CodeSourceRefreshResult {
                        binding_id: binding.id,
                        decision_id,
                        graph_readiness: readiness,
                        currentness: CodeSourceCurrentness::GraphUnavailable,
                        evaluated_snapshot_id: None,
                        detail: Some(
                            "Resolution reported Ready but no snapshot available".to_string(),
                        ),
                    };
                }
            };

            let inspector = FileInspector::new(resolver.workspace_root(), snapshot.clone());

            let (file, line_start, line_end) = parse_binding_target(&binding.target);
            let inspection = inspector.inspect_file(&file, line_start, line_end);

            let currentness = match inspection.evidence_state {
                GraphEvidenceState::Verified => {
                    if let (Some(original_hash), Some(current_hash)) = (
                        &binding
                            .metadata
                            .get("content_hash")
                            .and_then(|v| v.as_str()),
                        &inspection.content_hash,
                    ) {
                        if *original_hash == current_hash.as_str() {
                            CodeSourceCurrentness::Current
                        } else {
                            CodeSourceCurrentness::ContentChanged
                        }
                    } else {
                        // No content hash to compare — file exists, treat as current
                        CodeSourceCurrentness::Current
                    }
                }
                GraphEvidenceState::NotFound => CodeSourceCurrentness::TargetMissing,
                _ => CodeSourceCurrentness::GraphUnavailable,
            };

            CodeSourceRefreshResult {
                binding_id: binding.id,
                decision_id,
                graph_readiness: readiness,
                currentness,
                evaluated_snapshot_id: Some(snapshot.graph_snapshot_id.clone()),
                detail: inspection.detail.clone(),
            }
        }
        GraphReadiness::NotConfigured => CodeSourceRefreshResult {
            binding_id: binding.id,
            decision_id,
            graph_readiness: readiness,
            currentness: CodeSourceCurrentness::GraphNotConfigured,
            evaluated_snapshot_id: None,
            detail: Some("Local graph support is not configured".to_string()),
        },
        GraphReadiness::Stale | GraphReadiness::Indexing | GraphReadiness::Failed => {
            CodeSourceRefreshResult {
                binding_id: binding.id,
                decision_id,
                graph_readiness: readiness,
                currentness: CodeSourceCurrentness::GraphUnavailable,
                evaluated_snapshot_id: None,
                detail: Some(format!("Graph readiness: {:?}", readiness)),
            }
        }
    }
}

/// Evaluate code-source currentness for all bindings of a Decision.
///
/// Returns one `CodeSourceRefreshResult` per binding. None of these results
/// mutate the original bindings or the Decision's signoff/compliance state.
pub fn evaluate_decision_bindings(
    resolver: &SnapshotResolver,
    decision_id: Uuid,
    bindings: &[BindingEvidence],
) -> Vec<CodeSourceRefreshResult> {
    bindings
        .iter()
        .map(|b| evaluate_binding_currentness(resolver, decision_id, b))
        .collect()
}

/// Maps `GraphReadiness` to `CodeSourceCurrentness` for cases where the
/// graph cannot provide verified evidence.
///
/// This is the typed readiness-to-currentness mapping that prevents stale
/// or unavailable graph states from being interpreted as governance results.
pub fn currentness_for_readiness(readiness: GraphReadiness) -> CodeSourceCurrentness {
    match readiness {
        GraphReadiness::Ready => {
            // Ready means the graph is available — actual currentness depends
            // on file inspection, not readiness alone.
            CodeSourceCurrentness::Current
        }
        GraphReadiness::NotConfigured => CodeSourceCurrentness::GraphNotConfigured,
        GraphReadiness::Stale | GraphReadiness::Indexing | GraphReadiness::Failed => {
            CodeSourceCurrentness::GraphUnavailable
        }
    }
}

/// Parse a binding target string into (file_path, line_start, line_end).
///
/// Binding targets use the format `path` or `path:start-end`.
fn parse_binding_target(target: &str) -> (String, Option<u32>, Option<u32>) {
    if let Some((path, range)) = target.rsplit_once(':') {
        if let Some((start_s, end_s)) = range.split_once('-') {
            if let (Ok(start), Ok(end)) = (start_s.parse::<u32>(), end_s.parse::<u32>()) {
                return (path.to_string(), Some(start), Some(end));
            }
        }
    }
    (target.to_string(), None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── currentness_for_readiness mapping ────────────────────────────

    #[test]
    fn stale_readiness_maps_to_graph_unavailable() {
        assert_eq!(
            currentness_for_readiness(GraphReadiness::Stale),
            CodeSourceCurrentness::GraphUnavailable
        );
    }

    #[test]
    fn indexing_readiness_maps_to_graph_unavailable() {
        assert_eq!(
            currentness_for_readiness(GraphReadiness::Indexing),
            CodeSourceCurrentness::GraphUnavailable
        );
    }

    #[test]
    fn failed_readiness_maps_to_graph_unavailable() {
        assert_eq!(
            currentness_for_readiness(GraphReadiness::Failed),
            CodeSourceCurrentness::GraphUnavailable
        );
    }

    #[test]
    fn not_configured_readiness_maps_to_graph_not_configured() {
        assert_eq!(
            currentness_for_readiness(GraphReadiness::NotConfigured),
            CodeSourceCurrentness::GraphNotConfigured
        );
    }

    #[test]
    fn ready_readiness_maps_to_current() {
        assert_eq!(
            currentness_for_readiness(GraphReadiness::Ready),
            CodeSourceCurrentness::Current
        );
    }

    // ── parse_binding_target ─────────────────────────────────────────

    #[test]
    fn parse_file_only_target() {
        let (file, start, end) = parse_binding_target("src/lib.rs");
        assert_eq!(file, "src/lib.rs");
        assert_eq!(start, None);
        assert_eq!(end, None);
    }

    #[test]
    fn parse_file_with_range_target() {
        let (file, start, end) = parse_binding_target("src/lib.rs:10-20");
        assert_eq!(file, "src/lib.rs");
        assert_eq!(start, Some(10));
        assert_eq!(end, Some(20));
    }

    #[test]
    fn parse_file_with_invalid_range_falls_back() {
        let (file, start, end) = parse_binding_target("src/lib.rs:abc-def");
        assert_eq!(file, "src/lib.rs:abc-def");
        assert_eq!(start, None);
        assert_eq!(end, None);
    }

    // ── CodeSourceCurrentness never implies governance ───────────────

    #[test]
    fn all_non_ready_readiness_states_produce_non_blocking_currentness() {
        let non_ready = [
            GraphReadiness::Stale,
            GraphReadiness::Indexing,
            GraphReadiness::Failed,
            GraphReadiness::NotConfigured,
        ];
        for readiness in non_ready {
            let currentness = currentness_for_readiness(readiness);
            assert!(
                currentness == CodeSourceCurrentness::GraphUnavailable
                    || currentness == CodeSourceCurrentness::GraphNotConfigured,
                "GraphReadiness::{readiness:?} must produce non-blocking currentness, got {currentness:?}"
            );
        }
    }
}
