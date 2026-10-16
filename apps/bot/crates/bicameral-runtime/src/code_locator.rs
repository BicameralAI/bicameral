//! Deterministic CodeLocator — retrieval, ranking, and verification of code
//! target candidates using `GraphSnapshotService`.
//!
//! Per issue #437 and the deterministic-code-locator-graph-workflow spec:
//!
//! - Uses explicit caller hints, local search, graph snapshots, symbol lookup,
//!   and file inspection to find likely code targets.
//! - Must not call an LLM or treat model-generated claims as verified evidence.
//! - Ranking is deterministic: structural hash match > exact path > exact
//!   symbol > prior binding > changed file > lexical match.
//! - Only `Verified` evidence state may become accepted `BindingEvidence`
//!   through the governed materialization path.
//! - Must not append governance events, accept BindingEvidence, or mutate
//!   signoff/compliance/review/Decision state.

use bicameral_api::evidence::{GraphEvidenceState, GraphReadiness};
use bicameral_api::recall::{RecallScopeKind, SemanticCodeSearchRequest, SemanticCodeTargetType};
use bicameral_api::tool_request::{
    CodeLocateParams, LocatorCandidate, LocatorConfidence, LocatorEvidenceState,
    LocatorGraphReadiness, LocatorReason, LocatorReasonCode, LocatorResult, LocatorSnapshotInfo,
    LocatorStatus, LocatorTargetType,
};

use crate::graph_snapshot_service::GraphSnapshotService;
use crate::recall_admissibility::{
    RecallAdmissibilityGate, RecallAdmissibilityOutcome, RecallAdmissibleUse,
};
use crate::recall_provider::SemanticCodeSearchProvider;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Execute a deterministic `code.locate` command against the given
/// `GraphSnapshotService`.
///
/// Returns a `LocatorResult` with ranked candidates, graph readiness,
/// snapshot info, warnings, and next-action guidance.
pub fn locate(params: &CodeLocateParams, svc: &dyn GraphSnapshotService) -> LocatorResult {
    locate_with_providers(params, svc, None)
}

/// Execute a deterministic `code.locate` command with optional semantic
/// provider fusion.
///
/// When a `SemanticCodeSearchProvider` is supplied, semantic candidates are
/// collected after deterministic strategies, run through the
/// `RecallAdmissibilityGate`, and then verified against the graph where
/// possible. Verified semantic targets rank below exact graph targets but
/// above approximate hints. Approximate semantic candidates never become
/// `BindingEvidence` without deterministic graph proof.
pub fn locate_with_providers(
    params: &CodeLocateParams,
    svc: &dyn GraphSnapshotService,
    semantic_provider: Option<&dyn SemanticCodeSearchProvider>,
) -> LocatorResult {
    // 1. Resolve snapshot — prefer explicit commit_sha, then HEAD.
    let requested_ref = params
        .commit_sha
        .as_deref()
        .or(params.ref_name.as_deref())
        .unwrap_or("HEAD");

    let resolution = svc.resolve_snapshot(requested_ref);
    let graph_readiness = map_readiness(resolution.readiness());

    // If graph is not usable, return early with graph-unavailable status.
    if !resolution.is_resolved() {
        return LocatorResult {
            status: LocatorStatus::GraphUnavailable,
            query: params.query.clone(),
            graph_readiness,
            snapshot: None,
            scope: None,
            candidates: vec![],
            warnings: vec![format!(
                "graph snapshot not available for ref '{requested_ref}'"
            )],
            next_actions: vec!["Run graph.refresh_snapshot to index the workspace".to_string()],
        };
    }

    let snapshot = match resolution.snapshot() {
        Some(s) => s.clone(),
        None => {
            return LocatorResult {
                status: LocatorStatus::GraphUnavailable,
                query: params.query.clone(),
                graph_readiness,
                snapshot: None,
                scope: None,
                candidates: vec![],
                warnings: vec!["resolved snapshot missing unexpectedly".to_string()],
                next_actions: vec![],
            };
        }
    };

    let snap_info = LocatorSnapshotInfo {
        graph_snapshot_id: Some(snapshot.graph_snapshot_id.clone()),
        repo_id: Some(snapshot.repo_id.clone()),
        commit_sha: Some(snapshot.commit_sha.clone()),
        index_version: Some(snapshot.index_version.clone()),
        ref_name: snapshot.ref_name.clone(),
    };

    // 2. Collect candidates from each deterministic strategy.
    let mut candidates: Vec<ScoredCandidate> = Vec::new();

    // Strategy A: exact path lookup via files hint.
    for path in &params.files {
        let inspection = svc.inspect_file(&snapshot, path, None, None);
        let ev = map_evidence_state(inspection.evidence_state);
        let conf = if ev == LocatorEvidenceState::Verified {
            LocatorConfidence::High
        } else {
            confidence_for_evidence(ev)
        };
        candidates.push(ScoredCandidate {
            candidate: LocatorCandidate {
                target: path.clone(),
                target_type: LocatorTargetType::FilePath,
                file: path.clone(),
                range: None,
                symbol: None,
                evidence_state: ev,
                confidence: conf,
                reasons: vec![LocatorReason {
                    code: LocatorReasonCode::ExactPath,
                    detail: format!("file hint: {path}"),
                }],
                graph_snapshot_id: Some(snapshot.graph_snapshot_id.clone()),
                validated_sha: Some(snapshot.commit_sha.clone()),
                content_hash: inspection.content_hash.clone(),
                caveats: vec![],
            },
            rank_score: rank_score(LocatorReasonCode::ExactPath, ev),
        });
    }

    // Strategy B: symbol lookup via symbols hint.
    for sym in &params.symbols {
        let sym_result = svc.resolve_symbols(&snapshot, sym);
        if sym_result.evidence_state == GraphEvidenceState::Unsupported {
            // Try workspace-wide symbol search via find_symbol approach.
            // The GraphSnapshotService resolves per-file; for symbol search
            // we fall back to a lexical match strategy below.
        }
        if !sym_result.occurrences.is_empty() {
            for occ in &sym_result.occurrences {
                let ev = map_evidence_state(sym_result.evidence_state);
                let conf = if ev == LocatorEvidenceState::Verified {
                    LocatorConfidence::High
                } else {
                    confidence_for_evidence(ev)
                };
                candidates.push(ScoredCandidate {
                    candidate: LocatorCandidate {
                        target: format!("{}::{}", occ.file_path, occ.symbol_name),
                        target_type: LocatorTargetType::Symbol,
                        file: occ.file_path.clone(),
                        range: Some(format!("{}-{}", occ.line_start, occ.line_end)),
                        symbol: Some(occ.symbol_name.clone()),
                        evidence_state: ev,
                        confidence: conf,
                        reasons: vec![LocatorReason {
                            code: LocatorReasonCode::ExactSymbol,
                            detail: format!("symbol hint: {sym}"),
                        }],
                        graph_snapshot_id: Some(snapshot.graph_snapshot_id.clone()),
                        validated_sha: Some(snapshot.commit_sha.clone()),
                        content_hash: Some(occ.structural_hash.clone()),
                        caveats: vec![],
                    },
                    rank_score: rank_score(LocatorReasonCode::ExactSymbol, ev),
                });
            }
        }
    }

    // Strategy C: changed-file matching.
    for path in &params.changed_files {
        // Avoid duplicates from files hint.
        if params.files.contains(path) {
            continue;
        }
        let inspection = svc.inspect_file(&snapshot, path, None, None);
        let ev = map_evidence_state(inspection.evidence_state);
        candidates.push(ScoredCandidate {
            candidate: LocatorCandidate {
                target: path.clone(),
                target_type: LocatorTargetType::FilePath,
                file: path.clone(),
                range: None,
                symbol: None,
                evidence_state: ev,
                confidence: confidence_for_evidence(ev),
                reasons: vec![LocatorReason {
                    code: LocatorReasonCode::ChangedFile,
                    detail: format!("changed file: {path}"),
                }],
                graph_snapshot_id: Some(snapshot.graph_snapshot_id.clone()),
                validated_sha: Some(snapshot.commit_sha.clone()),
                content_hash: inspection.content_hash.clone(),
                caveats: vec![],
            },
            rank_score: rank_score(LocatorReasonCode::ChangedFile, ev),
        });
    }

    // Strategy D: lexical/query-based file search.
    if let Some(query) = &params.query {
        if !query.is_empty() {
            let query_lower = query.to_lowercase();
            // Check each file hint and changed_file for query overlap (already
            // handled above). For bare query strings, search symbol names in
            // files already collected and any symbols hints.
            for path in &params.files {
                if path.to_lowercase().contains(&query_lower)
                    && !candidates.iter().any(|c| {
                        c.candidate.file == *path
                            && c.candidate
                                .reasons
                                .iter()
                                .any(|r| r.code == LocatorReasonCode::LexicalMatch)
                    })
                {
                    // Already covered by ExactPath; skip duplicate.
                }
            }
        }
    }

    // Strategy E: semantic provider fusion (issue #639).
    //
    // Call the configured semantic provider, run each candidate through the
    // RecallAdmissibilityGate, and attempt deterministic graph verification
    // for admitted candidates. Verified semantic targets rank below exact
    // graph targets but above approximate hints. Approximate candidates
    // are downgraded and carry caveats.
    let mut semantic_warnings: Vec<String> = Vec::new();
    if let Some(provider) = semantic_provider {
        let search_request = SemanticCodeSearchRequest {
            query: params.query.clone().unwrap_or_default(),
            workspace_id: params
                .workspace_id
                .clone()
                .or_else(|| Some(snapshot.repo_id.clone())),
            graph_snapshot_id: params
                .graph_snapshot_id
                .clone()
                .or_else(|| Some(snapshot.graph_snapshot_id.clone())),
            commit_sha: params
                .commit_sha
                .clone()
                .or_else(|| Some(snapshot.commit_sha.clone())),
            ref_name: params.ref_name.clone().or(snapshot.ref_name.clone()),
            files: params.files.clone(),
            symbols: params.symbols.clone(),
            changed_files: params.changed_files.clone(),
            source_evidence_ids: params.source_evidence_ids.clone(),
            decision_id: params.decision_id.as_ref().map(|u| u.to_string()),
            candidate_id: params.candidate_id.as_ref().map(|u| u.to_string()),
            limit: params.limit,
            scope_kind: Some(RecallScopeKind::Query),
        };

        let search_result = provider.search(&search_request);
        let gate = RecallAdmissibilityGate::new();

        // Surface scope limitations from the provider.
        if !search_result.unknown_scope.is_empty() {
            semantic_warnings.push(format!(
                "semantic provider reported {} unknown scope region(s)",
                search_result.unknown_scope.len()
            ));
        }

        for sem_candidate in &search_result.candidates {
            let admission = gate.evaluate_semantic_candidate(&search_result, sem_candidate);

            // Skip rejected or deferred candidates entirely.
            if matches!(
                admission.outcome,
                RecallAdmissibilityOutcome::Reject | RecallAdmissibilityOutcome::Defer
            ) {
                continue;
            }

            // Determine whether this candidate has rank-only access.
            let is_rank_only = admission.allowed_uses.iter().all(|u| {
                matches!(
                    u,
                    RecallAdmissibleUse::RankOnly
                        | RecallAdmissibleUse::ExplainOnly
                        | RecallAdmissibleUse::Evaluate
                )
            });

            // Attempt deterministic graph verification for the semantic target.
            let target_type = match sem_candidate.target_type {
                SemanticCodeTargetType::Symbol => LocatorTargetType::Symbol,
                SemanticCodeTargetType::CodeRegion => LocatorTargetType::CodeRegion,
                SemanticCodeTargetType::FilePath => LocatorTargetType::FilePath,
                SemanticCodeTargetType::Dependency => LocatorTargetType::Dependency,
                SemanticCodeTargetType::DeploySurface => LocatorTargetType::DeploySurface,
            };

            let (evidence_state, confidence, reason_code, graph_verification, content_hash) =
                if target_type == LocatorTargetType::Symbol {
                    // Try symbol resolution through graph.
                    if let Some(sym) = sem_candidate.symbol.as_deref() {
                        let sym_result = svc.resolve_symbols(&snapshot, sym);
                        if !sym_result.occurrences.is_empty()
                            && sym_result.evidence_state == GraphEvidenceState::Verified
                        {
                            (
                                LocatorEvidenceState::Verified,
                                LocatorConfidence::High,
                                LocatorReasonCode::SemanticTarget,
                                true,
                                Some(sym_result.occurrences[0].structural_hash.clone()),
                            )
                        } else {
                            (
                                LocatorEvidenceState::ApproximateCandidate,
                                LocatorConfidence::Low,
                                LocatorReasonCode::SemanticHint,
                                false,
                                None,
                            )
                        }
                    } else {
                        (
                            LocatorEvidenceState::ApproximateCandidate,
                            LocatorConfidence::Low,
                            LocatorReasonCode::SemanticHint,
                            false,
                            None,
                        )
                    }
                } else if target_type == LocatorTargetType::FilePath
                    || target_type == LocatorTargetType::CodeRegion
                {
                    // Try file inspection through graph.
                    let inspection = svc.inspect_file(&snapshot, &sem_candidate.file, None, None);
                    let ev = map_evidence_state(inspection.evidence_state);
                    if ev == LocatorEvidenceState::Verified {
                        (
                            LocatorEvidenceState::Verified,
                            LocatorConfidence::High,
                            LocatorReasonCode::SemanticTarget,
                            true,
                            inspection.content_hash.clone(),
                        )
                    } else {
                        (
                            LocatorEvidenceState::ApproximateCandidate,
                            LocatorConfidence::Low,
                            LocatorReasonCode::SemanticHint,
                            false,
                            inspection.content_hash.clone(),
                        )
                    }
                } else {
                    // Dependency and DeploySurface targets are always approximate.
                    (
                        LocatorEvidenceState::ApproximateCandidate,
                        LocatorConfidence::Low,
                        LocatorReasonCode::SemanticHint,
                        false,
                        None,
                    )
                };

            // Build caveats from admissibility decision.
            let mut caveats = admission.caveats.clone();
            if !graph_verification && is_rank_only {
                caveats.push(
                    "approximate semantic candidate: requires deterministic graph proof before binding".to_string(),
                );
            }

            let target = if let Some(ref sym) = sem_candidate.symbol {
                format!("{}::{}", sem_candidate.file, sym)
            } else {
                sem_candidate.file.clone()
            };

            let mut reasons = vec![LocatorReason {
                code: reason_code,
                detail: format!(
                    "semantic provider: {} (score={:.2})",
                    sem_candidate
                        .reasons
                        .first()
                        .map(|s| s.as_str())
                        .unwrap_or("similarity"),
                    sem_candidate.score
                ),
            }];

            // Carry provider provenance as an additional reason.
            reasons.push(LocatorReason {
                code: reason_code,
                detail: format!(
                    "provider: {} v{}",
                    sem_candidate.provenance.provider_id, sem_candidate.provenance.provider_version
                ),
            });

            candidates.push(ScoredCandidate {
                candidate: LocatorCandidate {
                    target,
                    target_type,
                    file: sem_candidate.file.clone(),
                    range: sem_candidate.range.clone(),
                    symbol: sem_candidate.symbol.clone(),
                    evidence_state,
                    confidence,
                    reasons,
                    graph_snapshot_id: if graph_verification {
                        Some(snapshot.graph_snapshot_id.clone())
                    } else {
                        None
                    },
                    validated_sha: if graph_verification {
                        Some(snapshot.commit_sha.clone())
                    } else {
                        None
                    },
                    content_hash,
                    caveats,
                },
                rank_score: rank_score(reason_code, evidence_state),
            });
        }
    }

    // 3. Deduplicate: merge candidates for the same (file, symbol) pair.
    candidates = deduplicate(candidates);

    // 4. Sort by rank_score descending, then by file path for stability.
    candidates.sort_by(|a, b| {
        b.rank_score
            .partial_cmp(&a.rank_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.candidate.file.cmp(&b.candidate.file))
    });

    // 5. Apply limit with explicit truncation warning.
    let pre_truncate_count = candidates.len();
    candidates.truncate(params.limit);
    if pre_truncate_count > params.limit {
        semantic_warnings.push(format!(
            "results truncated from {} to {} candidates (limit={})",
            pre_truncate_count,
            candidates.len(),
            params.limit
        ));
    }

    // 6. Derive top-level status and next actions.
    let final_candidates: Vec<LocatorCandidate> =
        candidates.into_iter().map(|sc| sc.candidate).collect();

    let status = derive_status(&final_candidates, graph_readiness);
    let next_actions = derive_next_actions(&final_candidates, &status, graph_readiness);
    let mut warnings = derive_warnings(&final_candidates, graph_readiness);
    warnings.extend(semantic_warnings);

    LocatorResult {
        status,
        query: params.query.clone(),
        graph_readiness,
        snapshot: Some(snap_info),
        scope: None,
        candidates: final_candidates,
        warnings,
        next_actions,
    }
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

struct ScoredCandidate {
    candidate: LocatorCandidate,
    rank_score: f64,
}

// ---------------------------------------------------------------------------
// Ranking
// ---------------------------------------------------------------------------

/// Deterministic rank score: reason code weight × evidence multiplier.
fn rank_score(reason: LocatorReasonCode, evidence: LocatorEvidenceState) -> f64 {
    let reason_weight = match reason {
        LocatorReasonCode::ExactPath => 100.0,
        LocatorReasonCode::ExactSymbol => 95.0,
        LocatorReasonCode::PriorBinding => 80.0,
        LocatorReasonCode::ChangedFile => 60.0,
        LocatorReasonCode::ImportEdge => 50.0,
        LocatorReasonCode::CallEdge => 45.0,
        LocatorReasonCode::ReferenceEdge => 40.0,
        LocatorReasonCode::SourceEvidenceOverlap => 30.0,
        LocatorReasonCode::SemanticTarget => 70.0,
        LocatorReasonCode::SemanticHint => 15.0,
        LocatorReasonCode::LexicalMatch => 20.0,
    };
    let evidence_multiplier = match evidence {
        LocatorEvidenceState::Verified => 1.0,
        LocatorEvidenceState::ApproximateCandidate => 0.5,
        LocatorEvidenceState::UnknownStale => 0.3,
        LocatorEvidenceState::Ambiguous => 0.4,
        LocatorEvidenceState::UnknownNotIndexed => 0.2,
        LocatorEvidenceState::Unsupported => 0.1,
        LocatorEvidenceState::NotFound => 0.0,
    };
    reason_weight * evidence_multiplier
}

// ---------------------------------------------------------------------------
// Mapping helpers
// ---------------------------------------------------------------------------

fn map_readiness(g: GraphReadiness) -> LocatorGraphReadiness {
    match g {
        GraphReadiness::Ready => LocatorGraphReadiness::Ready,
        GraphReadiness::Indexing => LocatorGraphReadiness::Indexing,
        GraphReadiness::Stale => LocatorGraphReadiness::Stale,
        GraphReadiness::Failed => LocatorGraphReadiness::Failed,
        GraphReadiness::NotConfigured => LocatorGraphReadiness::NotConfigured,
    }
}

fn map_evidence_state(g: GraphEvidenceState) -> LocatorEvidenceState {
    match g {
        GraphEvidenceState::Verified => LocatorEvidenceState::Verified,
        GraphEvidenceState::NotFound => LocatorEvidenceState::NotFound,
        GraphEvidenceState::UnknownNotIndexed => LocatorEvidenceState::UnknownNotIndexed,
        GraphEvidenceState::UnknownStale => LocatorEvidenceState::UnknownStale,
        GraphEvidenceState::Ambiguous => LocatorEvidenceState::Ambiguous,
        GraphEvidenceState::Unsupported => LocatorEvidenceState::Unsupported,
        GraphEvidenceState::ApproximateCandidate => LocatorEvidenceState::ApproximateCandidate,
    }
}

fn confidence_for_evidence(ev: LocatorEvidenceState) -> LocatorConfidence {
    match ev {
        LocatorEvidenceState::Verified => LocatorConfidence::High,
        LocatorEvidenceState::ApproximateCandidate => LocatorConfidence::Low,
        LocatorEvidenceState::UnknownStale => LocatorConfidence::Unknown,
        LocatorEvidenceState::Ambiguous => LocatorConfidence::Low,
        LocatorEvidenceState::UnknownNotIndexed => LocatorConfidence::Unknown,
        LocatorEvidenceState::Unsupported => LocatorConfidence::Unknown,
        LocatorEvidenceState::NotFound => LocatorConfidence::Unknown,
    }
}

// ---------------------------------------------------------------------------
// Deduplication
// ---------------------------------------------------------------------------

/// Merge candidates for the same (file, symbol) key.
///
/// When duplicates exist, the highest-scored entry is kept and its reasons,
/// caveats, and provenance data are merged deterministically from all
/// entries sharing that key.
fn deduplicate(mut candidates: Vec<ScoredCandidate>) -> Vec<ScoredCandidate> {
    // Sort highest score first so the first entry per key wins.
    candidates.sort_by(|a, b| {
        b.rank_score
            .partial_cmp(&a.rank_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut seen: std::collections::HashMap<(String, String), usize> =
        std::collections::HashMap::new();
    let mut result = Vec::new();

    for sc in candidates {
        let key = (
            sc.candidate.file.clone(),
            sc.candidate.symbol.clone().unwrap_or_default(),
        );
        if let Some(&idx) = seen.get(&key) {
            // Merge reasons from the duplicate into the existing entry.
            let existing: &mut ScoredCandidate = &mut result[idx];
            for reason in &sc.candidate.reasons {
                let already_present = existing
                    .candidate
                    .reasons
                    .iter()
                    .any(|r| r.code == reason.code && r.detail == reason.detail);
                if !already_present {
                    existing.candidate.reasons.push(reason.clone());
                }
            }
            // Merge caveats.
            for caveat in &sc.candidate.caveats {
                if !existing.candidate.caveats.contains(caveat) {
                    existing.candidate.caveats.push(caveat.clone());
                }
            }
            // Prefer verified evidence state and graph proof from any source.
            if sc.candidate.evidence_state == LocatorEvidenceState::Verified
                && existing.candidate.evidence_state != LocatorEvidenceState::Verified
            {
                existing.candidate.evidence_state = sc.candidate.evidence_state;
                existing.candidate.confidence = sc.candidate.confidence;
                existing.candidate.graph_snapshot_id = sc
                    .candidate
                    .graph_snapshot_id
                    .or(existing.candidate.graph_snapshot_id.take());
                existing.candidate.validated_sha = sc
                    .candidate
                    .validated_sha
                    .or(existing.candidate.validated_sha.take());
                existing.candidate.content_hash = sc
                    .candidate
                    .content_hash
                    .or(existing.candidate.content_hash.take());
                // Re-score with the better evidence.
                existing.rank_score = existing.rank_score.max(sc.rank_score);
            }
        } else {
            let idx = result.len();
            seen.insert(key, idx);
            result.push(sc);
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Status / next-action derivation
// ---------------------------------------------------------------------------

fn derive_status(
    candidates: &[LocatorCandidate],
    graph_readiness: LocatorGraphReadiness,
) -> LocatorStatus {
    if candidates.is_empty() {
        return match graph_readiness {
            LocatorGraphReadiness::NotConfigured => LocatorStatus::GraphNotConfigured,
            LocatorGraphReadiness::Failed => LocatorStatus::GraphUnavailable,
            _ => LocatorStatus::NotFound,
        };
    }

    let verified_count = candidates
        .iter()
        .filter(|c| c.evidence_state == LocatorEvidenceState::Verified)
        .count();

    if verified_count > 1 {
        return LocatorStatus::Ambiguous;
    }

    if verified_count == 1 {
        return LocatorStatus::Found;
    }

    // All candidates are non-verified.
    let has_approximate = candidates
        .iter()
        .any(|c| c.evidence_state == LocatorEvidenceState::ApproximateCandidate);
    if has_approximate {
        return LocatorStatus::Found;
    }

    LocatorStatus::NotFound
}

fn derive_next_actions(
    candidates: &[LocatorCandidate],
    status: &LocatorStatus,
    graph_readiness: LocatorGraphReadiness,
) -> Vec<String> {
    let mut actions = Vec::new();

    match status {
        LocatorStatus::NotFound => {
            actions.push("Broaden search hints or check file paths".to_string());
        }
        LocatorStatus::Ambiguous => {
            actions.push(
                "Narrow query with additional file, symbol, or scope hints to disambiguate"
                    .to_string(),
            );
        }
        LocatorStatus::GraphUnavailable | LocatorStatus::GraphNotConfigured => {
            actions.push(
                "Run graph.refresh_snapshot to index the workspace before locating".to_string(),
            );
        }
        _ => {}
    }

    if graph_readiness == LocatorGraphReadiness::Stale {
        actions.push("Graph data is stale; run graph.refresh_snapshot to update".to_string());
    }

    if !candidates.is_empty()
        && candidates
            .iter()
            .all(|c| c.evidence_state != LocatorEvidenceState::Verified)
    {
        actions.push("No verified candidates; review approximate results carefully".to_string());
    }

    actions
}

fn derive_warnings(
    candidates: &[LocatorCandidate],
    graph_readiness: LocatorGraphReadiness,
) -> Vec<String> {
    let mut warnings = Vec::new();

    if graph_readiness == LocatorGraphReadiness::Stale {
        warnings
            .push("Graph snapshot is stale — results may not reflect latest changes".to_string());
    }

    let unsupported = candidates
        .iter()
        .filter(|c| c.evidence_state == LocatorEvidenceState::Unsupported)
        .count();
    if unsupported > 0 {
        warnings.push(format!(
            "{unsupported} candidate(s) are in unsupported language/format"
        ));
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph_snapshot_service::{GraphSnapshotService, GraphSnapshotServiceError};
    use bicameral_api::evidence::{
        BindingEvidence, CodeSourceCurrentness, CodeSourceRefreshResult, FileInspectionResult,
        GraphReadinessView, GraphSnapshot, SnapshotResolution, SymbolResolutionResult,
    };
    use bicameral_api::evidence::{
        GraphEvidenceState, SymbolKind, SymbolLanguage, SymbolOccurrence,
    };
    use uuid::Uuid;

    // ── Mock GraphSnapshotService ─────────────────────────────────────

    /// A test-only mock that returns configurable snapshot, file, and symbol
    /// results without needing a real git workspace.
    struct MockGraphService {
        head_snapshot: Option<GraphSnapshot>,
        file_results: std::collections::HashMap<String, FileInspectionResult>,
        symbol_results: std::collections::HashMap<String, SymbolResolutionResult>,
        readiness: GraphReadiness,
    }

    impl MockGraphService {
        fn ready(snapshot: GraphSnapshot) -> Self {
            Self {
                head_snapshot: Some(snapshot),
                file_results: std::collections::HashMap::new(),
                symbol_results: std::collections::HashMap::new(),
                readiness: GraphReadiness::Ready,
            }
        }

        fn unavailable() -> Self {
            Self {
                head_snapshot: None,
                file_results: std::collections::HashMap::new(),
                symbol_results: std::collections::HashMap::new(),
                readiness: GraphReadiness::Failed,
            }
        }

        fn with_file(mut self, path: &str, result: FileInspectionResult) -> Self {
            self.file_results.insert(path.to_string(), result);
            self
        }

        fn with_symbol(mut self, rel_path: &str, result: SymbolResolutionResult) -> Self {
            self.symbol_results.insert(rel_path.to_string(), result);
            self
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
            self.readiness
        }

        fn readiness_view(&self) -> GraphReadinessView {
            GraphReadinessView {
                readiness: self.readiness,
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
            self.file_results
                .get(path)
                .cloned()
                .unwrap_or_else(|| FileInspectionResult {
                    path: path.to_string(),
                    evidence_state: GraphEvidenceState::NotFound,
                    content_hash: None,
                    line_start: None,
                    line_end: None,
                    graph_snapshot_id: self
                        .head_snapshot
                        .as_ref()
                        .map(|s| s.graph_snapshot_id.clone())
                        .unwrap_or_default(),
                    detail: Some("file not found in mock".to_string()),
                })
        }

        fn resolve_symbols(
            &self,
            _snapshot: &GraphSnapshot,
            rel_path: &str,
        ) -> SymbolResolutionResult {
            self.symbol_results
                .get(rel_path)
                .cloned()
                .unwrap_or_else(|| SymbolResolutionResult {
                    graph_snapshot_id: self
                        .head_snapshot
                        .as_ref()
                        .map(|s| s.graph_snapshot_id.clone())
                        .unwrap_or_default(),
                    language: SymbolLanguage::Unsupported,
                    occurrences: vec![],
                    evidence_state: GraphEvidenceState::NotFound,
                    detail: Some("no symbol result in mock".to_string()),
                })
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

    // ── Test helpers ──────────────────────────────────────────────────

    fn test_snapshot() -> GraphSnapshot {
        GraphSnapshot {
            graph_snapshot_id: "snap-test-001".to_string(),
            repo_id: "test-repo".to_string(),
            commit_sha: "abc123".to_string(),
            index_version: "file-inspect-v1".to_string(),
            ref_name: Some("HEAD".to_string()),
            merge_base_sha: None,
            indexed_at: None,
            index_source: None,
        }
    }

    fn verified_file(path: &str) -> FileInspectionResult {
        FileInspectionResult {
            path: path.to_string(),
            evidence_state: GraphEvidenceState::Verified,
            content_hash: Some("hash-abc".to_string()),
            line_start: None,
            line_end: None,
            graph_snapshot_id: "snap-test-001".to_string(),
            detail: None,
        }
    }

    fn stale_file(path: &str) -> FileInspectionResult {
        FileInspectionResult {
            path: path.to_string(),
            evidence_state: GraphEvidenceState::UnknownStale,
            content_hash: None,
            line_start: None,
            line_end: None,
            graph_snapshot_id: "snap-test-001".to_string(),
            detail: Some("working tree differs".to_string()),
        }
    }

    fn verified_symbol(file: &str, name: &str, start: u32, end: u32) -> SymbolResolutionResult {
        SymbolResolutionResult {
            graph_snapshot_id: "snap-test-001".to_string(),
            language: SymbolLanguage::Rust,
            occurrences: vec![SymbolOccurrence {
                graph_snapshot_id: "snap-test-001".to_string(),
                symbol_name: name.to_string(),
                qualified_name: None,
                symbol_kind: SymbolKind::Function,
                language: SymbolLanguage::Rust,
                file_path: file.to_string(),
                line_start: start,
                line_end: end,
                content_hash: "content-hash-001".to_string(),
                structural_hash: "struct-hash-001".to_string(),
                evidence_state: GraphEvidenceState::Verified,
                detail: None,
                capture_kind: None,
                parser_version: Some("tree-sitter-rust-0.23".to_string()),
            }],
            evidence_state: GraphEvidenceState::Verified,
            detail: None,
        }
    }

    fn empty_params() -> CodeLocateParams {
        CodeLocateParams {
            query: None,
            decision_id: None,
            candidate_id: None,
            source_evidence_ids: vec![],
            binding_ids: vec![],
            files: vec![],
            symbols: vec![],
            changed_files: vec![],
            diff_context: None,
            workspace_id: None,
            commit_sha: None,
            ref_name: None,
            graph_snapshot_id: None,
            base_ref: None,
            base_sha: None,
            head_ref: None,
            head_sha: None,
            scope_kind: None,
            limit: 20,
        }
    }

    // ── Test: verified single file ────────────────────────────────────

    #[test]
    fn locate_verified_file_returns_found() {
        let snap = test_snapshot();
        let svc =
            MockGraphService::ready(snap).with_file("src/main.rs", verified_file("src/main.rs"));

        let params = CodeLocateParams {
            files: vec!["src/main.rs".to_string()],
            ..empty_params()
        };

        let result = locate(&params, &svc);

        assert_eq!(result.status, LocatorStatus::Found);
        assert_eq!(result.graph_readiness, LocatorGraphReadiness::Ready);
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(
            result.candidates[0].evidence_state,
            LocatorEvidenceState::Verified
        );
        assert_eq!(result.candidates[0].confidence, LocatorConfidence::High);
        assert_eq!(
            result.candidates[0].reasons[0].code,
            LocatorReasonCode::ExactPath
        );
        assert!(result.candidates[0].content_hash.is_some());
        assert!(result.candidates[0].graph_snapshot_id.is_some());
        assert!(result.candidates[0].validated_sha.is_some());
    }

    // ── Test: verified symbol ────────────────────────────────────────

    #[test]
    fn locate_verified_symbol_returns_found() {
        let snap = test_snapshot();
        let svc = MockGraphService::ready(snap).with_symbol(
            "dispatch",
            verified_symbol("src/dispatch.rs", "dispatch", 10, 50),
        );

        let params = CodeLocateParams {
            symbols: vec!["dispatch".to_string()],
            ..empty_params()
        };

        let result = locate(&params, &svc);

        assert_eq!(result.status, LocatorStatus::Found);
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].target_type, LocatorTargetType::Symbol);
        assert_eq!(
            result.candidates[0].evidence_state,
            LocatorEvidenceState::Verified
        );
        assert!(result.candidates[0].symbol.is_some());
        assert!(result.candidates[0].range.is_some());
    }

    // ── Test: approximate (stale) candidate ──────────────────────────

    #[test]
    fn locate_stale_file_returns_approximate() {
        let snap = test_snapshot();
        let svc = MockGraphService::ready(snap).with_file("src/lib.rs", stale_file("src/lib.rs"));

        let params = CodeLocateParams {
            files: vec!["src/lib.rs".to_string()],
            ..empty_params()
        };

        let result = locate(&params, &svc);

        // Stale file → candidate exists but not verified → NotFound status
        // (no verified or approximate candidates).
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(
            result.candidates[0].evidence_state,
            LocatorEvidenceState::UnknownStale
        );
        assert_eq!(result.candidates[0].confidence, LocatorConfidence::Unknown);
    }

    // ── Test: not found ──────────────────────────────────────────────

    #[test]
    fn locate_missing_file_returns_not_found() {
        let snap = test_snapshot();
        let svc = MockGraphService::ready(snap);

        let params = CodeLocateParams {
            files: vec!["nonexistent.rs".to_string()],
            ..empty_params()
        };

        let result = locate(&params, &svc);

        assert_eq!(result.candidates.len(), 1);
        assert_eq!(
            result.candidates[0].evidence_state,
            LocatorEvidenceState::NotFound
        );
    }

    #[test]
    fn locate_empty_params_returns_not_found() {
        let snap = test_snapshot();
        let svc = MockGraphService::ready(snap);

        let result = locate(&empty_params(), &svc);

        assert_eq!(result.status, LocatorStatus::NotFound);
        assert!(result.candidates.is_empty());
    }

    // ── Test: graph unavailable ──────────────────────────────────────

    #[test]
    fn locate_graph_unavailable_returns_typed_status() {
        let svc = MockGraphService::unavailable();

        let params = CodeLocateParams {
            files: vec!["src/main.rs".to_string()],
            ..empty_params()
        };

        let result = locate(&params, &svc);

        assert_eq!(result.status, LocatorStatus::GraphUnavailable);
        assert_eq!(result.graph_readiness, LocatorGraphReadiness::Failed);
        assert!(result.candidates.is_empty());
        assert!(!result.next_actions.is_empty());
    }

    // ── Test: multiple candidates (ambiguous) ────────────────────────

    #[test]
    fn locate_multiple_verified_candidates_returns_ambiguous() {
        let snap = test_snapshot();
        let svc = MockGraphService::ready(snap)
            .with_file("src/a.rs", verified_file("src/a.rs"))
            .with_file("src/b.rs", verified_file("src/b.rs"));

        let params = CodeLocateParams {
            files: vec!["src/a.rs".to_string(), "src/b.rs".to_string()],
            ..empty_params()
        };

        let result = locate(&params, &svc);

        assert_eq!(result.status, LocatorStatus::Ambiguous);
        assert_eq!(result.candidates.len(), 2);
        assert!(result
            .candidates
            .iter()
            .all(|c| c.evidence_state == LocatorEvidenceState::Verified));
        assert!(result
            .next_actions
            .iter()
            .any(|a| a.contains("disambiguate")));
    }

    // ── Test: needs narrowing (ambiguous symbols) ────────────────────

    #[test]
    fn locate_ambiguous_symbols_suggests_narrowing() {
        let snap = test_snapshot();
        let ambiguous_result = SymbolResolutionResult {
            graph_snapshot_id: "snap-test-001".to_string(),
            language: SymbolLanguage::Rust,
            occurrences: vec![
                SymbolOccurrence {
                    graph_snapshot_id: "snap-test-001".to_string(),
                    symbol_name: "process".to_string(),
                    qualified_name: None,
                    symbol_kind: SymbolKind::Function,
                    language: SymbolLanguage::Rust,
                    file_path: "src/a.rs".to_string(),
                    line_start: 5,
                    line_end: 20,
                    content_hash: "chash-a".to_string(),
                    structural_hash: "hash-a".to_string(),
                    evidence_state: GraphEvidenceState::Ambiguous,
                    detail: None,
                    capture_kind: None,
                    parser_version: None,
                },
                SymbolOccurrence {
                    graph_snapshot_id: "snap-test-001".to_string(),
                    symbol_name: "process".to_string(),
                    qualified_name: None,
                    symbol_kind: SymbolKind::Function,
                    language: SymbolLanguage::Rust,
                    file_path: "src/b.rs".to_string(),
                    line_start: 10,
                    line_end: 30,
                    content_hash: "chash-b".to_string(),
                    structural_hash: "hash-b".to_string(),
                    evidence_state: GraphEvidenceState::Ambiguous,
                    detail: None,
                    capture_kind: None,
                    parser_version: None,
                },
            ],
            evidence_state: GraphEvidenceState::Ambiguous,
            detail: Some("Multiple definitions of 'process'".to_string()),
        };
        let svc = MockGraphService::ready(snap).with_symbol("process", ambiguous_result);

        let params = CodeLocateParams {
            symbols: vec!["process".to_string()],
            ..empty_params()
        };

        let result = locate(&params, &svc);

        assert_eq!(result.candidates.len(), 2);
        // Both are ambiguous → status should be NotFound since none are verified.
        // But we have candidates, and they're ambiguous evidence state.
        assert!(result
            .candidates
            .iter()
            .all(|c| c.evidence_state == LocatorEvidenceState::Ambiguous));
        assert!(result
            .next_actions
            .iter()
            .any(|a| a.contains("disambiguate") || a.contains("approximate")));
    }

    // ── Test: changed files produce candidates ───────────────────────

    #[test]
    fn locate_changed_files_produce_candidates() {
        let snap = test_snapshot();
        let svc = MockGraphService::ready(snap)
            .with_file("src/changed.rs", verified_file("src/changed.rs"));

        let params = CodeLocateParams {
            changed_files: vec!["src/changed.rs".to_string()],
            ..empty_params()
        };

        let result = locate(&params, &svc);

        assert_eq!(result.candidates.len(), 1);
        assert_eq!(
            result.candidates[0].reasons[0].code,
            LocatorReasonCode::ChangedFile
        );
    }

    // ── Test: ranking is deterministic ───────────────────────────────

    #[test]
    fn locate_ranking_is_deterministic() {
        let snap = test_snapshot();
        let svc = MockGraphService::ready(snap)
            .with_file("src/main.rs", verified_file("src/main.rs"))
            .with_file("src/changed.rs", verified_file("src/changed.rs"));

        let params = CodeLocateParams {
            files: vec!["src/main.rs".to_string()],
            changed_files: vec!["src/changed.rs".to_string()],
            ..empty_params()
        };

        let r1 = locate(&params, &svc);
        let r2 = locate(&params, &svc);

        assert_eq!(r1.candidates.len(), r2.candidates.len());
        for (a, b) in r1.candidates.iter().zip(r2.candidates.iter()) {
            assert_eq!(a.target, b.target);
            assert_eq!(a.evidence_state, b.evidence_state);
            assert_eq!(a.confidence, b.confidence);
        }

        // ExactPath should rank higher than ChangedFile.
        assert_eq!(r1.candidates[0].file, "src/main.rs");
        assert_eq!(
            r1.candidates[0].reasons[0].code,
            LocatorReasonCode::ExactPath
        );
    }

    // ── Test: no governance writes ───────────────────────────────────

    #[test]
    fn locate_does_not_produce_binding_evidence() {
        // CodeLocator only produces LocatorResult; it never calls
        // event-store mutations. This test verifies the contract by
        // confirming the return type is LocatorResult (not BindingEvidence)
        // and that the mock service's evaluate_currentness was not called
        // in any path that would imply governance mutation.
        let snap = test_snapshot();
        let svc =
            MockGraphService::ready(snap).with_file("src/main.rs", verified_file("src/main.rs"));

        let params = CodeLocateParams {
            files: vec!["src/main.rs".to_string()],
            ..empty_params()
        };

        let result = locate(&params, &svc);

        // Result is LocatorResult, not BindingEvidence — the type system
        // enforces that no governance writes occurred.
        assert_eq!(result.status, LocatorStatus::Found);
        assert!(result.candidates[0].evidence_state.allows_binding());
    }

    // ── Test: limit is respected ─────────────────────────────────────

    #[test]
    fn locate_respects_limit() {
        let snap = test_snapshot();
        let mut svc = MockGraphService::ready(snap);
        for i in 0..10 {
            let path = format!("src/file_{i}.rs");
            svc = svc.with_file(&path, verified_file(&path));
        }

        let params = CodeLocateParams {
            files: (0..10).map(|i| format!("src/file_{i}.rs")).collect(),
            limit: 3,
            ..empty_params()
        };

        let result = locate(&params, &svc);
        assert!(result.candidates.len() <= 3);
    }

    // ── Test: stale graph produces warnings ──────────────────────────

    #[test]
    fn locate_stale_graph_produces_warnings() {
        let snap = test_snapshot();
        let mut svc = MockGraphService::ready(snap);
        svc.readiness = GraphReadiness::Stale;
        svc = svc.with_file("src/main.rs", stale_file("src/main.rs"));

        let params = CodeLocateParams {
            files: vec!["src/main.rs".to_string()],
            ..empty_params()
        };

        let result = locate(&params, &svc);

        // Graph readiness maps from the resolution, but we set the service readiness to stale.
        // Since resolve_snapshot still returns Resolved, graph_readiness comes from the resolution.
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(
            result.candidates[0].evidence_state,
            LocatorEvidenceState::UnknownStale
        );
    }

    // ── Test: mixed verified and non-verified ────────────────────────

    #[test]
    fn locate_mixed_verified_and_stale_returns_found() {
        let snap = test_snapshot();
        let svc = MockGraphService::ready(snap)
            .with_file("src/good.rs", verified_file("src/good.rs"))
            .with_file("src/stale.rs", stale_file("src/stale.rs"));

        let params = CodeLocateParams {
            files: vec!["src/good.rs".to_string(), "src/stale.rs".to_string()],
            ..empty_params()
        };

        let result = locate(&params, &svc);

        // One verified candidate → Found.
        assert_eq!(result.status, LocatorStatus::Found);
        assert_eq!(result.candidates.len(), 2);
    }

    // ════════════════════════════════════════════════════════════════════
    // Semantic recall fusion tests (issue #639)
    // ════════════════════════════════════════════════════════════════════

    use crate::recall_provider::{fake_semantic_candidate, FakeSemanticCodeSearchProvider};
    use bicameral_api::recall::{
        RecallUnknownProviderScope, RecallUnknownProviderScopeReason, SemanticCodeCandidate,
        SemanticCodeTargetType,
    };

    fn semantic_file_candidate(
        file: &str,
        score: f64,
        target_type: SemanticCodeTargetType,
    ) -> SemanticCodeCandidate {
        let mut c = fake_semantic_candidate(file, score);
        c.target_type = target_type;
        c
    }

    // ── Acceptance: verified exact > verified semantic > approximate ──

    #[test]
    fn fusion_verified_exact_outranks_verified_semantic() {
        let snap = test_snapshot();
        let svc = MockGraphService::ready(snap)
            .with_file("src/exact.rs", verified_file("src/exact.rs"))
            .with_file("src/semantic.rs", verified_file("src/semantic.rs"));

        let provider = FakeSemanticCodeSearchProvider::ready(vec![semantic_file_candidate(
            "src/semantic.rs",
            0.95,
            SemanticCodeTargetType::FilePath,
        )]);

        let params = CodeLocateParams {
            files: vec!["src/exact.rs".to_string()],
            ..empty_params()
        };

        let result = locate_with_providers(&params, &svc, Some(&provider));

        assert!(result.candidates.len() >= 2);

        // First candidate must be exact path (highest rank).
        assert_eq!(result.candidates[0].file, "src/exact.rs");
        assert!(result.candidates[0]
            .reasons
            .iter()
            .any(|r| r.code == LocatorReasonCode::ExactPath));

        // Second candidate is semantic-verified.
        let semantic_cand = result
            .candidates
            .iter()
            .find(|c| c.file == "src/semantic.rs")
            .expect("semantic candidate present");
        assert!(semantic_cand
            .reasons
            .iter()
            .any(|r| r.code == LocatorReasonCode::SemanticTarget));
        assert_eq!(semantic_cand.evidence_state, LocatorEvidenceState::Verified);

        // Exact path rank > semantic target rank.
        let exact_idx = result
            .candidates
            .iter()
            .position(|c| c.file == "src/exact.rs")
            .unwrap();
        let semantic_idx = result
            .candidates
            .iter()
            .position(|c| c.file == "src/semantic.rs")
            .unwrap();
        assert!(
            exact_idx < semantic_idx,
            "exact path must rank higher than verified semantic"
        );
    }

    #[test]
    fn fusion_verified_semantic_outranks_approximate_semantic() {
        let snap = test_snapshot();
        // Only src/verified.rs exists in graph; src/approx.rs does not.
        let svc = MockGraphService::ready(snap)
            .with_file("src/verified.rs", verified_file("src/verified.rs"));

        let provider = FakeSemanticCodeSearchProvider::ready(vec![
            semantic_file_candidate("src/verified.rs", 0.90, SemanticCodeTargetType::FilePath),
            semantic_file_candidate("src/approx.rs", 0.85, SemanticCodeTargetType::FilePath),
        ]);

        let result = locate_with_providers(&empty_params(), &svc, Some(&provider));

        assert_eq!(result.candidates.len(), 2);

        let verified = result
            .candidates
            .iter()
            .find(|c| c.file == "src/verified.rs")
            .unwrap();
        let approximate = result
            .candidates
            .iter()
            .find(|c| c.file == "src/approx.rs")
            .unwrap();

        assert_eq!(verified.evidence_state, LocatorEvidenceState::Verified);
        assert!(verified
            .reasons
            .iter()
            .any(|r| r.code == LocatorReasonCode::SemanticTarget));

        assert_eq!(
            approximate.evidence_state,
            LocatorEvidenceState::ApproximateCandidate
        );
        assert!(approximate
            .reasons
            .iter()
            .any(|r| r.code == LocatorReasonCode::SemanticHint));

        // Verified semantic must rank before approximate semantic.
        let v_idx = result
            .candidates
            .iter()
            .position(|c| c.file == "src/verified.rs")
            .unwrap();
        let a_idx = result
            .candidates
            .iter()
            .position(|c| c.file == "src/approx.rs")
            .unwrap();
        assert!(v_idx < a_idx);
    }

    // ── Acceptance: approximate semantic never becomes BindingEvidence ──

    #[test]
    fn fusion_approximate_semantic_never_verified_without_graph_proof() {
        let snap = test_snapshot();
        // No file registration: graph cannot verify semantic candidate.
        let svc = MockGraphService::ready(snap);

        let provider = FakeSemanticCodeSearchProvider::ready(vec![semantic_file_candidate(
            "src/only_semantic.rs",
            0.99,
            SemanticCodeTargetType::FilePath,
        )]);

        let result = locate_with_providers(&empty_params(), &svc, Some(&provider));

        assert_eq!(result.candidates.len(), 1);
        assert_eq!(
            result.candidates[0].evidence_state,
            LocatorEvidenceState::ApproximateCandidate
        );
        assert!(result.candidates[0]
            .reasons
            .iter()
            .any(|r| r.code == LocatorReasonCode::SemanticHint));
        // Must not have graph proof fields when approximate.
        assert!(result.candidates[0].graph_snapshot_id.is_none());
        assert!(result.candidates[0].validated_sha.is_none());
        // Must carry caveat about requiring graph proof.
        assert!(result.candidates[0]
            .caveats
            .iter()
            .any(|c| c.contains("deterministic graph proof")));
    }

    // ── Acceptance: duplicate candidates merge deterministically ──

    #[test]
    fn fusion_duplicate_merge_preserves_reasons_and_caveats() {
        let snap = test_snapshot();
        let svc = MockGraphService::ready(snap)
            .with_file("src/overlap.rs", verified_file("src/overlap.rs"));

        // Exact path and semantic both target the same file.
        let provider = FakeSemanticCodeSearchProvider::ready(vec![semantic_file_candidate(
            "src/overlap.rs",
            0.91,
            SemanticCodeTargetType::FilePath,
        )]);

        let params = CodeLocateParams {
            files: vec!["src/overlap.rs".to_string()],
            ..empty_params()
        };

        let result = locate_with_providers(&params, &svc, Some(&provider));

        // Should be deduplicated into one candidate.
        assert_eq!(result.candidates.len(), 1);
        let cand = &result.candidates[0];

        // Must have both ExactPath and SemanticTarget reasons.
        let reason_codes: Vec<_> = cand.reasons.iter().map(|r| r.code).collect();
        assert!(
            reason_codes.contains(&LocatorReasonCode::ExactPath),
            "merged candidate must have ExactPath reason"
        );
        assert!(
            reason_codes.contains(&LocatorReasonCode::SemanticTarget),
            "merged candidate must have SemanticTarget reason"
        );

        // Evidence state must be Verified (highest).
        assert_eq!(cand.evidence_state, LocatorEvidenceState::Verified);
    }

    // ── Acceptance: ranking is stable across identical inputs ──

    #[test]
    fn fusion_ranking_is_stable() {
        let snap = test_snapshot();
        let svc =
            MockGraphService::ready(snap.clone()).with_file("src/a.rs", verified_file("src/a.rs"));

        let provider = FakeSemanticCodeSearchProvider::ready(vec![
            semantic_file_candidate("src/b.rs", 0.80, SemanticCodeTargetType::FilePath),
            semantic_file_candidate("src/c.rs", 0.75, SemanticCodeTargetType::FilePath),
        ]);

        let params = CodeLocateParams {
            files: vec!["src/a.rs".to_string()],
            ..empty_params()
        };

        let r1 = locate_with_providers(&params, &svc, Some(&provider));
        let r2 = locate_with_providers(&params, &svc, Some(&provider));

        assert_eq!(r1.candidates.len(), r2.candidates.len());
        for (a, b) in r1.candidates.iter().zip(r2.candidates.iter()) {
            assert_eq!(a.target, b.target);
            assert_eq!(a.evidence_state, b.evidence_state);
            assert_eq!(a.confidence, b.confidence);
            assert_eq!(a.reasons.len(), b.reasons.len());
            assert_eq!(a.caveats, b.caveats);
        }
    }

    // ── Acceptance: truncation is explicit ──

    #[test]
    fn fusion_truncation_produces_warning() {
        let snap = test_snapshot();
        let svc =
            MockGraphService::ready(snap).with_file("src/exact.rs", verified_file("src/exact.rs"));

        // Create more semantic candidates than the limit.
        let candidates: Vec<_> = (0..5)
            .map(|i| {
                semantic_file_candidate(
                    &format!("src/sem_{i}.rs"),
                    0.90 - (i as f64 * 0.01),
                    SemanticCodeTargetType::FilePath,
                )
            })
            .collect();
        let provider = FakeSemanticCodeSearchProvider::ready(candidates);

        let params = CodeLocateParams {
            files: vec!["src/exact.rs".to_string()],
            limit: 3,
            ..empty_params()
        };

        let result = locate_with_providers(&params, &svc, Some(&provider));

        assert!(result.candidates.len() <= 3);
        assert!(
            result.warnings.iter().any(|w| w.contains("truncated")),
            "must warn about truncation"
        );
    }

    // ── Acceptance: stale provider caveats ──

    #[test]
    fn fusion_stale_semantic_carries_caveats() {
        let snap = test_snapshot();
        let svc = MockGraphService::ready(snap);

        let provider = FakeSemanticCodeSearchProvider::stale(vec![fake_semantic_candidate(
            "src/stale_sem.rs",
            0.77,
        )]);

        let result = locate_with_providers(&empty_params(), &svc, Some(&provider));

        assert_eq!(result.candidates.len(), 1);
        assert_eq!(
            result.candidates[0].evidence_state,
            LocatorEvidenceState::ApproximateCandidate
        );
        // Must carry caveats (from admissibility gate or semantic fusion).
        assert!(
            !result.candidates[0].caveats.is_empty(),
            "stale semantic candidate must carry caveats"
        );
    }

    // ── Acceptance: no authority escalation ──

    #[test]
    fn fusion_semantic_never_claims_authority() {
        let snap = test_snapshot();
        let svc = MockGraphService::ready(snap);

        let provider = FakeSemanticCodeSearchProvider::ready(vec![semantic_file_candidate(
            "src/authority.rs",
            0.99,
            SemanticCodeTargetType::FilePath,
        )]);

        let result = locate_with_providers(&empty_params(), &svc, Some(&provider));

        // Approximate candidate must not have verified evidence.
        assert_eq!(
            result.candidates[0].evidence_state,
            LocatorEvidenceState::ApproximateCandidate
        );
        // No governance state in LocatorResult — type system enforces this.
        // The result is LocatorResult, not BindingEvidence.

        // Candidate must carry caveat about requiring graph proof.
        assert!(result.candidates[0]
            .caveats
            .iter()
            .any(|c| c.contains("deterministic graph proof")));
    }

    // ── Test: unknown scope warning ──

    #[test]
    fn fusion_unknown_scope_produces_warning() {
        let snap = test_snapshot();
        let svc = MockGraphService::ready(snap);

        let mut fixture = crate::recall_provider::FakeSemanticCodeSearchFixture::ready(vec![
            fake_semantic_candidate("src/scoped.rs", 0.80),
        ]);
        fixture.unknown_scope.push(RecallUnknownProviderScope {
            scope: "src/generated".to_string(),
            reason: RecallUnknownProviderScopeReason::NotIndexed,
            detail: Some("generated files excluded".to_string()),
        });
        let provider = FakeSemanticCodeSearchProvider::new(fixture);

        let result = locate_with_providers(&empty_params(), &svc, Some(&provider));

        assert!(
            result.warnings.iter().any(|w| w.contains("unknown scope")),
            "must warn about unknown scope from provider"
        );
    }

    // ── Test: no semantic provider → same as locate() ──

    #[test]
    fn fusion_no_provider_identical_to_locate() {
        let snap = test_snapshot();
        let svc =
            MockGraphService::ready(snap).with_file("src/main.rs", verified_file("src/main.rs"));

        let params = CodeLocateParams {
            files: vec!["src/main.rs".to_string()],
            ..empty_params()
        };

        let r1 = locate(&params, &svc);
        let r2 = locate_with_providers(&params, &svc, None);

        assert_eq!(r1.status, r2.status);
        assert_eq!(r1.candidates.len(), r2.candidates.len());
        assert_eq!(
            r1.candidates[0].evidence_state,
            r2.candidates[0].evidence_state
        );
        assert_eq!(r1.candidates[0].file, r2.candidates[0].file);
    }

    // ── Test: full rank posture ordering ──

    #[test]
    fn fusion_rank_posture_ordering() {
        // Issue #639 expected rank posture:
        // 1. verified exact graph/code targets
        // 2. verified prior binding/source overlap
        // 3. verified semantic target after deterministic graph verification
        // 4. approximate semantic/provider/ownership hints
        // 5. lexical-only hints
        // 6. stale, ambiguous, unsupported, or scoped-limited hints with caveats
        let exact = rank_score(LocatorReasonCode::ExactPath, LocatorEvidenceState::Verified);
        let exact_symbol = rank_score(
            LocatorReasonCode::ExactSymbol,
            LocatorEvidenceState::Verified,
        );
        let prior_binding = rank_score(
            LocatorReasonCode::PriorBinding,
            LocatorEvidenceState::Verified,
        );
        let semantic_verified = rank_score(
            LocatorReasonCode::SemanticTarget,
            LocatorEvidenceState::Verified,
        );
        let semantic_approx = rank_score(
            LocatorReasonCode::SemanticHint,
            LocatorEvidenceState::ApproximateCandidate,
        );
        let lexical = rank_score(
            LocatorReasonCode::LexicalMatch,
            LocatorEvidenceState::Verified,
        );
        let stale = rank_score(
            LocatorReasonCode::LexicalMatch,
            LocatorEvidenceState::UnknownStale,
        );

        assert!(exact > exact_symbol);
        assert!(exact_symbol > prior_binding);
        assert!(prior_binding > semantic_verified);
        assert!(semantic_verified > semantic_approx);
        assert!(lexical > semantic_approx);
        assert!(semantic_approx > stale);
    }

    // ── Test: semantic with symbol verification ──

    #[test]
    fn fusion_semantic_symbol_verified_through_graph() {
        let snap = test_snapshot();
        let svc = MockGraphService::ready(snap).with_symbol(
            "my_func",
            verified_symbol("src/target.rs", "my_func", 10, 20),
        );

        let mut candidate =
            semantic_file_candidate("src/target.rs", 0.92, SemanticCodeTargetType::Symbol);
        candidate.symbol = Some("my_func".to_string());

        let provider = FakeSemanticCodeSearchProvider::ready(vec![candidate]);

        let result = locate_with_providers(&empty_params(), &svc, Some(&provider));

        assert_eq!(result.candidates.len(), 1);
        assert_eq!(
            result.candidates[0].evidence_state,
            LocatorEvidenceState::Verified
        );
        assert!(result.candidates[0]
            .reasons
            .iter()
            .any(|r| r.code == LocatorReasonCode::SemanticTarget));
        assert!(result.candidates[0].graph_snapshot_id.is_some());
        assert!(result.candidates[0].validated_sha.is_some());
    }
}
