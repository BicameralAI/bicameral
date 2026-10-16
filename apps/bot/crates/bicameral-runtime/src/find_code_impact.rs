//! Planned code-impact discovery runtime for `decision.find_code_impact`.
//!
//! Implements the binding lifecycle transition:
//! `BindingDiscoveryRequested → BindingSearchPlanned → BindingCandidatesReady`.
//!
//! Given an accepted Decision, extracts search context (summary, rationale,
//! source evidence, existing bindings, caller hints), generates bounded
//! `code.locate` queries, executes them against `GraphSnapshotService`, and
//! aggregates results into a typed `FindCodeImpactResult`.
//!
//! Authority boundary:
//! - Does NOT materialize `BindingEvidence`.
//! - Does NOT assert compliance, signoff, safety, or blocking.
//! - Does NOT call an LLM or treat model-generated claims as verified.
//! - Planner output is bounded by max queries/files.

use bicameral_api::candidate::Decision;
use bicameral_api::evidence::BindingEvidence;
use bicameral_api::tool_request::{
    BindingDiscoveryPhase, CodeImpactCandidate, CodeImpactCaveat, CodeImpactCaveatKind,
    CodeLocateParams, FindCodeImpactHintKind, FindCodeImpactNextAction, FindCodeImpactParams,
    FindCodeImpactResult, FindCodeImpactStatus, LocatorEvidenceState, LocatorGraphReadiness,
    LocatorSnapshotInfo, LocatorStatus, SearchPlanSummary,
};

use crate::code_locator;
use crate::graph_snapshot_service::GraphSnapshotService;

// ---------------------------------------------------------------------------
// Planner constants
// ---------------------------------------------------------------------------

/// Maximum number of `code.locate` queries executed per discovery run.
const MAX_QUERIES: usize = 10;

/// Maximum number of candidate results retained from all queries combined.
const MAX_CANDIDATES: usize = 50;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Execute planned code-impact discovery for an accepted Decision.
///
/// `decision` — the accepted Decision from the replayed ledger.
/// `bindings` — existing accepted bindings for the Decision (may be empty).
/// `params` — the `FindCodeImpactParams` from the ToolRequest.
/// `svc` — the graph snapshot service for code.locate calls.
pub fn find_code_impact(
    decision: &Decision,
    bindings: &[BindingEvidence],
    params: &FindCodeImpactParams,
    svc: &dyn GraphSnapshotService,
) -> FindCodeImpactResult {
    // Phase 1: Plan — extract search scopes from Decision context.
    let plan = plan_queries(decision, bindings, params);

    if plan.queries.is_empty() {
        return FindCodeImpactResult {
            status: FindCodeImpactStatus::NotFound,
            decision_id: params.decision_id.clone(),
            phase: BindingDiscoveryPhase::SearchPlanned,
            graph_readiness: LocatorGraphReadiness::NotConfigured,
            snapshot: None,
            search_plan: Some(SearchPlanSummary {
                strategy: plan.strategy.clone(),
                queries_executed: 0,
                hint_seeded: plan.hint_seeded,
            }),
            candidates: vec![],
            caveats: vec![CodeImpactCaveat {
                kind: CodeImpactCaveatKind::LimitedSourceContext,
                detail: "no search queries could be derived from Decision context".into(),
            }],
            next_actions: vec![FindCodeImpactNextAction {
                action: "provide_hints".into(),
                description: "Retry with file/symbol/keyword hints to seed discovery".into(),
                command_hint: Some("decision.find_code_impact with hints".into()),
            }],
            semantic_recall: None,
        };
    }

    // Phase 2: Execute — run bounded code.locate calls.
    let mut all_candidates: Vec<CodeImpactCandidate> = Vec::new();
    let mut last_readiness = LocatorGraphReadiness::NotConfigured;
    let mut last_snapshot: Option<LocatorSnapshotInfo> = None;
    let mut queries_executed: usize = 0;
    let mut graph_unavailable = false;
    let mut graph_stale = false;

    let limit = params.limit.min(MAX_CANDIDATES);

    for locate_params in &plan.queries {
        let result = code_locator::locate(locate_params, svc);
        queries_executed += 1;

        last_readiness = result.graph_readiness;
        if let Some(ref snap) = result.snapshot {
            last_snapshot = Some(snap.clone());
        }

        match result.status {
            LocatorStatus::GraphUnavailable | LocatorStatus::GraphNotConfigured => {
                graph_unavailable = true;
                break;
            }
            LocatorStatus::Found | LocatorStatus::Ambiguous => {
                if result.graph_readiness == LocatorGraphReadiness::Stale {
                    graph_stale = true;
                }
                for candidate in result.candidates {
                    if all_candidates.len() >= limit {
                        break;
                    }
                    all_candidates.push(CodeImpactCandidate {
                        target: candidate.target,
                        target_type: candidate.target_type,
                        file: candidate.file,
                        range: candidate.range,
                        symbol: candidate.symbol,
                        evidence_state: candidate.evidence_state,
                        confidence: candidate.confidence,
                        reasons: candidate.reasons,
                        graph_snapshot_id: candidate.graph_snapshot_id,
                        validated_sha: candidate.validated_sha,
                        content_hash: candidate.content_hash,
                    });
                }
            }
            LocatorStatus::NotFound | LocatorStatus::Unsupported | LocatorStatus::Failed => {
                // Continue to next query
            }
        }

        if all_candidates.len() >= limit {
            break;
        }
    }

    // Phase 3: Aggregate — build result from collected candidates.
    if graph_unavailable && all_candidates.is_empty() {
        return FindCodeImpactResult {
            status: FindCodeImpactStatus::GraphUnavailable,
            decision_id: params.decision_id.clone(),
            phase: BindingDiscoveryPhase::SearchPlanned,
            graph_readiness: last_readiness,
            snapshot: last_snapshot,
            search_plan: Some(SearchPlanSummary {
                strategy: plan.strategy.clone(),
                queries_executed,
                hint_seeded: plan.hint_seeded,
            }),
            candidates: vec![],
            caveats: vec![],
            next_actions: vec![FindCodeImpactNextAction {
                action: "refresh_graph".into(),
                description: "Run graph.refresh_snapshot to index the workspace".into(),
                command_hint: Some("graph.refresh_snapshot".into()),
            }],
            semantic_recall: None,
        };
    }

    // Determine status from candidates
    let status = determine_status(&all_candidates, graph_stale);

    // Build caveats
    let mut caveats: Vec<CodeImpactCaveat> = Vec::new();
    if graph_stale {
        caveats.push(CodeImpactCaveat {
            kind: CodeImpactCaveatKind::GraphStale,
            detail: "graph snapshot may not reflect current HEAD".into(),
        });
    }
    if all_candidates.len() >= limit {
        caveats.push(CodeImpactCaveat {
            kind: CodeImpactCaveatKind::Truncated,
            detail: format!("results truncated at limit={limit}"),
        });
    }
    if has_approximate_candidates(&all_candidates) {
        caveats.push(CodeImpactCaveat {
            kind: CodeImpactCaveatKind::ApproximateCandidates,
            detail: "some candidates are approximate and not graph-verified".into(),
        });
    }

    // Build next actions
    let next_actions = build_next_actions(&status, &all_candidates);

    FindCodeImpactResult {
        status,
        decision_id: params.decision_id.clone(),
        phase: BindingDiscoveryPhase::CandidatesReady,
        graph_readiness: last_readiness,
        snapshot: last_snapshot,
        search_plan: Some(SearchPlanSummary {
            strategy: plan.strategy.clone(),
            queries_executed,
            hint_seeded: plan.hint_seeded,
        }),
        candidates: all_candidates,
        caveats,
        next_actions,
        semantic_recall: None,
    }
}

// ---------------------------------------------------------------------------
// Planner — deterministic query extraction
// ---------------------------------------------------------------------------

struct SearchPlan {
    strategy: String,
    hint_seeded: bool,
    queries: Vec<CodeLocateParams>,
}

/// Extract bounded code.locate queries from Decision context and caller hints.
fn plan_queries(
    decision: &Decision,
    bindings: &[BindingEvidence],
    params: &FindCodeImpactParams,
) -> SearchPlan {
    let mut queries: Vec<CodeLocateParams> = Vec::new();
    let hint_seeded = !params.hints.is_empty();

    // Strategy 1: Caller-provided hints seed explicit queries.
    let file_hints: Vec<String> = params
        .hints
        .iter()
        .filter(|h| h.kind == FindCodeImpactHintKind::File)
        .map(|h| h.value.clone())
        .collect();

    let symbol_hints: Vec<String> = params
        .hints
        .iter()
        .filter(|h| h.kind == FindCodeImpactHintKind::Symbol)
        .map(|h| h.value.clone())
        .collect();

    let keyword_hints: Vec<String> = params
        .hints
        .iter()
        .filter(|h| h.kind == FindCodeImpactHintKind::Keyword)
        .map(|h| h.value.clone())
        .collect();

    // Build hint-seeded query if file/symbol hints are available.
    if !file_hints.is_empty() || !symbol_hints.is_empty() {
        let query_text = if !keyword_hints.is_empty() {
            Some(keyword_hints.join(" "))
        } else {
            None
        };

        queries.push(make_locate_params(
            query_text,
            Some(params.decision_id.clone()),
            file_hints.clone(),
            symbol_hints.clone(),
            vec![],
            params,
        ));
    }

    // Strategy 2: Keyword-only query from hints (when no file/symbol hints).
    if !keyword_hints.is_empty() && file_hints.is_empty() && symbol_hints.is_empty() {
        queries.push(make_locate_params(
            Some(keyword_hints.join(" ")),
            Some(params.decision_id.clone()),
            vec![],
            vec![],
            vec![],
            params,
        ));
    }

    // Strategy 3: Existing bindings — locate their targets for currentness.
    if !bindings.is_empty() && queries.len() < MAX_QUERIES {
        let binding_targets: Vec<String> = bindings.iter().map(|b| b.target.clone()).collect();
        let binding_ids: Vec<String> = bindings.iter().map(|b| b.id.to_string()).collect();

        queries.push(make_locate_params_with_bindings(
            None,
            Some(params.decision_id.clone()),
            binding_targets,
            vec![],
            binding_ids,
            params,
        ));
    }

    // Strategy 4: Decision title keywords (deterministic extraction).
    if queries.len() < MAX_QUERIES {
        let title_query = extract_keywords_from_text(&decision.title);
        if !title_query.is_empty() {
            queries.push(make_locate_params(
                Some(title_query),
                Some(params.decision_id.clone()),
                vec![],
                vec![],
                vec![],
                params,
            ));
        }
    }

    // Strategy 5: Decision body keywords (limited extraction, deduplicated).
    if queries.len() < MAX_QUERIES {
        let body_query = extract_keywords_from_text(&decision.body);
        let title_query = extract_keywords_from_text(&decision.title);
        if !body_query.is_empty() && body_query != title_query {
            queries.push(make_locate_params(
                Some(body_query),
                Some(params.decision_id.clone()),
                vec![],
                vec![],
                vec![],
                params,
            ));
        }
    }

    // Enforce MAX_QUERIES bound.
    queries.truncate(MAX_QUERIES);

    let strategy = if hint_seeded {
        "hint_seeded".to_string()
    } else if !bindings.is_empty() {
        "existing_bindings".to_string()
    } else {
        "source_context".to_string()
    };

    SearchPlan {
        strategy,
        hint_seeded,
        queries,
    }
}

fn make_locate_params(
    query: Option<String>,
    decision_id: Option<String>,
    files: Vec<String>,
    symbols: Vec<String>,
    changed_files: Vec<String>,
    impact_params: &FindCodeImpactParams,
) -> CodeLocateParams {
    CodeLocateParams {
        query,
        decision_id,
        candidate_id: None,
        source_evidence_ids: vec![],
        binding_ids: vec![],
        files,
        symbols,
        changed_files,
        diff_context: None,
        workspace_id: impact_params.workspace_id.clone(),
        commit_sha: impact_params.commit_sha.clone(),
        ref_name: impact_params.ref_name.clone(),
        graph_snapshot_id: None,
        base_ref: None,
        base_sha: None,
        head_ref: None,
        head_sha: None,
        scope_kind: None,
        limit: impact_params.limit.min(MAX_CANDIDATES),
    }
}

fn make_locate_params_with_bindings(
    query: Option<String>,
    decision_id: Option<String>,
    files: Vec<String>,
    symbols: Vec<String>,
    binding_ids: Vec<String>,
    impact_params: &FindCodeImpactParams,
) -> CodeLocateParams {
    CodeLocateParams {
        query,
        decision_id,
        candidate_id: None,
        source_evidence_ids: vec![],
        binding_ids,
        files,
        symbols,
        changed_files: vec![],
        diff_context: None,
        workspace_id: impact_params.workspace_id.clone(),
        commit_sha: impact_params.commit_sha.clone(),
        ref_name: impact_params.ref_name.clone(),
        graph_snapshot_id: None,
        base_ref: None,
        base_sha: None,
        head_ref: None,
        head_sha: None,
        scope_kind: None,
        limit: impact_params.limit.min(MAX_CANDIDATES),
    }
}

/// Extract meaningful keywords from Decision text for lexical search.
fn extract_keywords_from_text(text: &str) -> String {
    const MAX_KEYWORDS: usize = 6;
    const STOPWORDS: &[&str] = &[
        "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
        "do", "does", "did", "will", "would", "could", "should", "may", "might", "shall", "can",
        "to", "of", "in", "for", "on", "with", "at", "by", "from", "as", "into", "over", "through",
        "during", "before", "after", "above", "below", "between", "under", "again", "further",
        "then", "once", "here", "there", "when", "where", "why", "how", "all", "each", "every",
        "both", "few", "more", "most", "other", "some", "such", "no", "nor", "not", "only", "own",
        "same", "so", "than", "too", "very", "just", "because", "but", "and", "or", "if", "while",
        "that", "this", "it", "its", "they", "them", "their", "we", "us", "our", "you", "your",
        "he", "him", "his", "she", "her",
    ];

    let words: Vec<&str> = text
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter(|w| w.len() > 2)
        .filter(|w| !STOPWORDS.contains(&w.to_lowercase().as_str()))
        .take(MAX_KEYWORDS)
        .collect();

    words.join(" ")
}

// ---------------------------------------------------------------------------
// Result aggregation helpers
// ---------------------------------------------------------------------------

fn determine_status(candidates: &[CodeImpactCandidate], graph_stale: bool) -> FindCodeImpactStatus {
    if candidates.is_empty() {
        return FindCodeImpactStatus::NotFound;
    }

    if graph_stale {
        return FindCodeImpactStatus::GraphStale;
    }

    let has_verified = candidates
        .iter()
        .any(|c| c.evidence_state == LocatorEvidenceState::Verified);

    if has_verified {
        FindCodeImpactStatus::Found
    } else {
        FindCodeImpactStatus::NeedsHumanSelection
    }
}

fn has_approximate_candidates(candidates: &[CodeImpactCandidate]) -> bool {
    candidates
        .iter()
        .any(|c| c.evidence_state == LocatorEvidenceState::ApproximateCandidate)
}

fn build_next_actions(
    status: &FindCodeImpactStatus,
    candidates: &[CodeImpactCandidate],
) -> Vec<FindCodeImpactNextAction> {
    let mut actions = Vec::new();

    match status {
        FindCodeImpactStatus::Found => {
            if candidates.iter().any(|c| c.allows_binding()) {
                actions.push(FindCodeImpactNextAction {
                    action: "binding_create".into(),
                    description: "Create binding evidence for verified candidate(s)".into(),
                    command_hint: Some("binding.create".into()),
                });
            }
        }
        FindCodeImpactStatus::NeedsHumanSelection => {
            actions.push(FindCodeImpactNextAction {
                action: "select_candidate".into(),
                description: "Choose candidate region(s) for binding".into(),
                command_hint: None,
            });
            actions.push(FindCodeImpactNextAction {
                action: "narrow_search".into(),
                description: "Provide additional hints to refine search".into(),
                command_hint: Some("decision.find_code_impact with hints".into()),
            });
        }
        FindCodeImpactStatus::NotFound => {
            actions.push(FindCodeImpactNextAction {
                action: "refresh_graph".into(),
                description: "Run graph.refresh_snapshot to index the workspace".into(),
                command_hint: Some("graph.refresh_snapshot".into()),
            });
            actions.push(FindCodeImpactNextAction {
                action: "provide_hints".into(),
                description: "Retry with file/symbol/keyword hints".into(),
                command_hint: Some("decision.find_code_impact with hints".into()),
            });
        }
        FindCodeImpactStatus::GraphUnavailable => {
            actions.push(FindCodeImpactNextAction {
                action: "refresh_graph".into(),
                description: "Run graph.refresh_snapshot to index the workspace".into(),
                command_hint: Some("graph.refresh_snapshot".into()),
            });
        }
        FindCodeImpactStatus::GraphStale => {
            actions.push(FindCodeImpactNextAction {
                action: "refresh_graph".into(),
                description: "Re-index workspace for latest state".into(),
                command_hint: Some("graph.refresh_snapshot".into()),
            });
        }
        FindCodeImpactStatus::Unsupported => {
            actions.push(FindCodeImpactNextAction {
                action: "keep_source_only".into(),
                description: "Decision is not eligible for code-impact discovery".into(),
                command_hint: None,
            });
        }
    }

    actions
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_api::candidate::{Decision, DecisionLevel, ReviewLifecycleState, TrackingState};
    use bicameral_api::dashboard::{ComplianceState, SignoffState};
    use bicameral_api::evidence::{
        BindingConfidence, BindingEvidence, BindingType, CodeSourceCurrentness,
        CodeSourceRefreshResult, FileInspectionResult, GraphEvidenceState, GraphReadiness,
        GraphReadinessView, GraphSnapshot, SnapshotResolution, SymbolLanguage,
        SymbolResolutionResult,
    };
    use bicameral_api::tool_request::{FindCodeImpactHint, FindCodeImpactHintKind};
    use chrono::Utc;
    use uuid::Uuid;

    use crate::graph_snapshot_service::{GraphSnapshotService, GraphSnapshotServiceError};

    // -- Mock GraphSnapshotService --------------------------------------------

    struct MockGraphService {
        head_snapshot: Option<GraphSnapshot>,
    }

    impl MockGraphService {
        fn available() -> Self {
            Self {
                head_snapshot: Some(test_snapshot()),
            }
        }
        fn unavailable() -> Self {
            Self {
                head_snapshot: None,
            }
        }
    }

    impl GraphSnapshotService for MockGraphService {
        fn resolve_snapshot(&self, _requested_ref: &str) -> SnapshotResolution {
            match &self.head_snapshot {
                Some(snap) => SnapshotResolution::Resolved {
                    snapshot: snap.clone(),
                },
                None => SnapshotResolution::Failed {
                    requested_ref: "HEAD".to_string(),
                    detail: "mock: graph unavailable".to_string(),
                },
            }
        }

        fn resolve_head(&self) -> Result<GraphSnapshot, GraphSnapshotServiceError> {
            self.head_snapshot
                .clone()
                .ok_or_else(|| GraphSnapshotServiceError::WorkspaceNotConfigured("mock".into()))
        }

        fn check_readiness(&self, _resolutions: &[&SnapshotResolution]) -> GraphReadiness {
            if self.head_snapshot.is_some() {
                GraphReadiness::Ready
            } else {
                GraphReadiness::Failed
            }
        }

        fn readiness_view(&self) -> GraphReadinessView {
            GraphReadinessView {
                readiness: if self.head_snapshot.is_some() {
                    GraphReadiness::Ready
                } else {
                    GraphReadiness::Failed
                },
                graph_snapshot_id: self
                    .head_snapshot
                    .as_ref()
                    .map(|s| s.graph_snapshot_id.clone()),
                detail: None,
                scope_coverage: None,
            }
        }

        fn inspect_file(
            &self,
            _snapshot: &GraphSnapshot,
            path: &str,
            _line_start: Option<u32>,
            _line_end: Option<u32>,
        ) -> FileInspectionResult {
            FileInspectionResult {
                path: path.to_string(),
                evidence_state: GraphEvidenceState::Verified,
                content_hash: Some(format!("hash-{path}")),
                line_start: None,
                line_end: None,
                graph_snapshot_id: self
                    .head_snapshot
                    .as_ref()
                    .map(|s| s.graph_snapshot_id.clone())
                    .unwrap_or_default(),
                detail: None,
            }
        }

        fn resolve_symbols(
            &self,
            _snapshot: &GraphSnapshot,
            _rel_path: &str,
        ) -> SymbolResolutionResult {
            SymbolResolutionResult {
                graph_snapshot_id: self
                    .head_snapshot
                    .as_ref()
                    .map(|s| s.graph_snapshot_id.clone())
                    .unwrap_or_default(),
                language: SymbolLanguage::Unsupported,
                occurrences: vec![],
                evidence_state: GraphEvidenceState::Unsupported,
                detail: Some("mock: no symbol resolution".to_string()),
            }
        }

        fn evaluate_currentness(
            &self,
            _decision_id: Uuid,
            _binding: &BindingEvidence,
        ) -> CodeSourceRefreshResult {
            CodeSourceRefreshResult {
                decision_id: Uuid::nil(),
                binding_id: Uuid::nil(),
                graph_readiness: GraphReadiness::Failed,
                currentness: CodeSourceCurrentness::GraphUnavailable,
                evaluated_snapshot_id: None,
                detail: Some("mock".to_string()),
            }
        }

        fn evaluate_decision_currentness(
            &self,
            _decision_id: Uuid,
            _bindings: &[BindingEvidence],
        ) -> Vec<CodeSourceRefreshResult> {
            vec![]
        }

        fn currentness_for_readiness(&self, _readiness: GraphReadiness) -> CodeSourceCurrentness {
            CodeSourceCurrentness::GraphUnavailable
        }
    }

    // -- Test helpers ----------------------------------------------------------

    fn test_snapshot() -> GraphSnapshot {
        GraphSnapshot {
            graph_snapshot_id: "snap-test-001".to_string(),
            repo_id: "test-repo".to_string(),
            commit_sha: "abc123def".to_string(),
            index_version: "file-inspect-v1".to_string(),
            ref_name: Some("HEAD".to_string()),
            merge_base_sha: None,
            indexed_at: None,
            index_source: None,
        }
    }

    fn make_decision(title: &str, body: &str) -> Decision {
        Decision {
            id: Uuid::new_v4(),
            candidate_id: Uuid::new_v4(),
            title: title.to_string(),
            body: body.to_string(),
            level: DecisionLevel::Product,
            signoff_state: SignoffState::Proposed,
            compliance_state: ComplianceState::Ungrounded,
            review_state: ReviewLifecycleState::Proposed,
            evidence_refs: vec![],
            evidence_reference_ids: vec![],
            source_id: Some("test-source".to_string()),
            tracking_state: TrackingState::Tracked,
            promoted_at: Utc::now(),
            promoted_by: "test-actor".to_string(),
            provenance: None,
        }
    }

    fn make_binding(file_target: &str) -> BindingEvidence {
        BindingEvidence {
            id: Uuid::new_v4(),
            candidate_id: Uuid::new_v4(),
            binding_type: BindingType::CodeRegion,
            target: file_target.to_string(),
            confidence: BindingConfidence::High,
            explanation: None,
            bound_at: Utc::now(),
            metadata: serde_json::Value::Null,
            graph_snapshot_id: Some("gs-test".into()),
            validated_sha: Some("abc123".into()),
            ref_name: None,
            target_kind: Some("file".into()),
            target_identifier: Some(file_target.into()),
            evidence_state: Some(GraphEvidenceState::Verified),
            readiness_warnings: vec![],
            created_from_request_id: None,
        }
    }

    // -- Tests -----------------------------------------------------------------

    #[test]
    fn planner_extracts_queries_from_hints() {
        let decision = make_decision("Add health check endpoint", "Implement /healthz route");
        let params = FindCodeImpactParams {
            decision_id: decision.id.to_string(),
            hints: vec![
                FindCodeImpactHint {
                    kind: FindCodeImpactHintKind::File,
                    value: "src/routes/healthz.rs".into(),
                },
                FindCodeImpactHint {
                    kind: FindCodeImpactHintKind::Symbol,
                    value: "HealthCheckHandler".into(),
                },
            ],
            workspace_id: None,
            ref_name: None,
            commit_sha: None,
            limit: 20,
        };

        let plan = plan_queries(&decision, &[], &params);
        assert!(plan.hint_seeded);
        assert!(!plan.queries.is_empty());
        assert_eq!(plan.strategy, "hint_seeded");
        assert!(plan.queries[0]
            .files
            .contains(&"src/routes/healthz.rs".to_string()));
        assert!(plan.queries[0]
            .symbols
            .contains(&"HealthCheckHandler".to_string()));
    }

    #[test]
    fn planner_extracts_queries_from_decision_context() {
        let decision = make_decision(
            "Implement OAuth device flow",
            "Add device code grant type for CLI authentication",
        );
        let params = FindCodeImpactParams {
            decision_id: decision.id.to_string(),
            hints: vec![],
            workspace_id: None,
            ref_name: None,
            commit_sha: None,
            limit: 20,
        };

        let plan = plan_queries(&decision, &[], &params);
        assert!(!plan.hint_seeded);
        assert!(!plan.queries.is_empty());
        assert_eq!(plan.strategy, "source_context");
        assert!(plan.queries.iter().any(|q| q.query.is_some()));
    }

    #[test]
    fn planner_uses_existing_bindings() {
        let decision = make_decision("Add auth route", "Routes for auth");
        let binding = make_binding("src/auth.rs:1-20");
        let params = FindCodeImpactParams {
            decision_id: decision.id.to_string(),
            hints: vec![],
            workspace_id: None,
            ref_name: None,
            commit_sha: None,
            limit: 20,
        };

        let plan = plan_queries(&decision, &[binding], &params);
        assert_eq!(plan.strategy, "existing_bindings");
        assert!(plan
            .queries
            .iter()
            .any(|q| q.files.contains(&"src/auth.rs:1-20".to_string())));
    }

    #[test]
    fn planner_bounded_by_max_queries() {
        let decision = make_decision(
            "Very long title with many keywords here for testing bounds",
            "Body with different keywords that expand queries beyond limit",
        );
        let params = FindCodeImpactParams {
            decision_id: decision.id.to_string(),
            hints: vec![
                FindCodeImpactHint {
                    kind: FindCodeImpactHintKind::File,
                    value: "a.rs".into(),
                },
                FindCodeImpactHint {
                    kind: FindCodeImpactHintKind::Keyword,
                    value: "keyword1".into(),
                },
            ],
            workspace_id: None,
            ref_name: None,
            commit_sha: None,
            limit: 20,
        };

        let plan = plan_queries(&decision, &[], &params);
        assert!(plan.queries.len() <= MAX_QUERIES);
    }

    #[test]
    fn find_code_impact_with_available_graph() {
        let decision = make_decision("Add healthz endpoint", "Implement /healthz");
        let params = FindCodeImpactParams {
            decision_id: decision.id.to_string(),
            hints: vec![FindCodeImpactHint {
                kind: FindCodeImpactHintKind::File,
                value: "src/routes.rs".into(),
            }],
            workspace_id: None,
            ref_name: None,
            commit_sha: None,
            limit: 20,
        };

        let svc = MockGraphService::available();
        let result = find_code_impact(&decision, &[], &params, &svc);

        assert_eq!(result.decision_id, decision.id.to_string());
        assert_eq!(result.phase, BindingDiscoveryPhase::CandidatesReady);
        assert_eq!(result.status, FindCodeImpactStatus::Found);
        assert!(!result.candidates.is_empty());
        assert!(result.search_plan.is_some());
        let plan = result.search_plan.unwrap();
        assert!(plan.queries_executed > 0);
        assert!(plan.hint_seeded);
    }

    #[test]
    fn find_code_impact_graph_unavailable() {
        let decision = make_decision("Some decision", "Body text");
        let params = FindCodeImpactParams {
            decision_id: decision.id.to_string(),
            hints: vec![FindCodeImpactHint {
                kind: FindCodeImpactHintKind::File,
                value: "src/something.rs".into(),
            }],
            workspace_id: None,
            ref_name: None,
            commit_sha: None,
            limit: 20,
        };

        let svc = MockGraphService::unavailable();
        let result = find_code_impact(&decision, &[], &params, &svc);

        assert_eq!(result.status, FindCodeImpactStatus::GraphUnavailable);
        assert!(result.candidates.is_empty());
        assert!(result
            .next_actions
            .iter()
            .any(|a| a.action == "refresh_graph"));
    }

    #[test]
    fn find_code_impact_multi_locate_aggregates() {
        let decision = make_decision("Implement OAuth flow", "Add OAuth device grant");
        let binding = make_binding("src/oauth.rs:5-30");
        let params = FindCodeImpactParams {
            decision_id: decision.id.to_string(),
            hints: vec![
                FindCodeImpactHint {
                    kind: FindCodeImpactHintKind::File,
                    value: "src/auth.rs".into(),
                },
                FindCodeImpactHint {
                    kind: FindCodeImpactHintKind::Symbol,
                    value: "AuthProvider".into(),
                },
            ],
            workspace_id: None,
            ref_name: None,
            commit_sha: None,
            limit: 20,
        };

        let svc = MockGraphService::available();
        let result = find_code_impact(&decision, &[binding], &params, &svc);

        assert!(result.candidates.len() >= 1);
        assert_eq!(result.phase, BindingDiscoveryPhase::CandidatesReady);
        let plan = result.search_plan.unwrap();
        assert!(plan.queries_executed >= 2);
    }

    #[test]
    fn find_code_impact_no_context_returns_not_found_with_caveat() {
        let decision = make_decision("", "");
        let params = FindCodeImpactParams {
            decision_id: decision.id.to_string(),
            hints: vec![],
            workspace_id: None,
            ref_name: None,
            commit_sha: None,
            limit: 20,
        };

        let svc = MockGraphService::available();
        let result = find_code_impact(&decision, &[], &params, &svc);

        assert_eq!(result.status, FindCodeImpactStatus::NotFound);
        assert!(result
            .caveats
            .iter()
            .any(|c| c.kind == CodeImpactCaveatKind::LimitedSourceContext));
    }

    #[test]
    fn extract_keywords_filters_stopwords() {
        let text = "the quick brown fox jumps over the lazy dog";
        let keywords = extract_keywords_from_text(text);
        assert!(!keywords.contains("the"));
        assert!(!keywords.contains("over"));
        assert!(keywords.contains("quick"));
        assert!(keywords.contains("brown"));
    }

    #[test]
    fn extract_keywords_respects_max_limit() {
        let text = "word1 word2 word3 word4 word5 word6 word7 word8 word9 word10";
        let keywords = extract_keywords_from_text(text);
        let count = keywords.split_whitespace().count();
        assert!(count <= 6);
    }

    #[test]
    fn empty_hints_no_bindings_uses_decision_text() {
        let decision = make_decision(
            "Configure database connection pool",
            "Set up PostgreSQL pool",
        );
        let params = FindCodeImpactParams {
            decision_id: decision.id.to_string(),
            hints: vec![],
            workspace_id: None,
            ref_name: None,
            commit_sha: None,
            limit: 20,
        };

        let svc = MockGraphService::available();
        let result = find_code_impact(&decision, &[], &params, &svc);

        assert_ne!(result.status, FindCodeImpactStatus::GraphUnavailable);
        assert!(result.search_plan.is_some());
    }

    #[test]
    fn rejected_decision_still_produces_result() {
        let mut decision = make_decision("Rejected thing", "Body");
        decision.signoff_state = SignoffState::Rejected;
        let params = FindCodeImpactParams {
            decision_id: decision.id.to_string(),
            hints: vec![FindCodeImpactHint {
                kind: FindCodeImpactHintKind::File,
                value: "src/foo.rs".into(),
            }],
            workspace_id: None,
            ref_name: None,
            commit_sha: None,
            limit: 20,
        };

        let svc = MockGraphService::available();
        let result = find_code_impact(&decision, &[], &params, &svc);
        // The function itself doesn't check signoff — dispatch layer handles that
        assert_eq!(result.phase, BindingDiscoveryPhase::CandidatesReady);
    }
}
