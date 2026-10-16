//! Progressive preflight query surface.
//!
//! Implements progressive `preflight.run` per ADR-0019:
//!
//! - Source-only preflight remains available when graph data is unavailable.
//! - Diff-scoped graph preflight builds or reuses bounded graph slices.
//! - Responses include `snapshot_scope`, `unknown_scope`, readiness warnings,
//!   and per-result relevance reasons.
//! - Local no-conflict is never reported as global or team-wide consistency.
//! - Preflight query output does not mutate compliance, signoff, review,
//!   binding, or conflict state.

use crate::replay::ReplayedState;
use bicameral_api::candidate::{CandidateState, DecisionLevel};
use bicameral_api::dashboard::ComplianceState;
use bicameral_api::drift::{ContradictionReport, FindingSeverity, FindingStatus};
use bicameral_api::evidence::{
    BindingEvidence, BindingType, GraphReadiness, GraphReadinessView, SnapshotResolution,
};
use bicameral_api::tool_request::{
    LocatorCandidate, LocatorEvidenceState, LocatorGraphReadiness, LocatorResult,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

// ── Query types ───────────────────────────────────────────────────────────

/// Preflight query input aligned with ADR-0019 `preflight.run`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightQuery {
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub symbols: Vec<String>,
    #[serde(default)]
    pub diff_context: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
}

// ── Response types ────────────────────────────────────────────────────────

/// How the decision/candidate is relevant to the preflight query.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreflightRelevance {
    /// Query terms match decision/candidate text.
    SourceTextMatch,
    /// All decisions returned (no file/symbol filter specified).
    FullLedger,
    /// Verified binding target overlaps with query file/symbol.
    BindingTargetMatch,
}

/// A summary of an active contradiction affecting a decision, attached to
/// preflight results. Deduplicated by `report_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveContradiction {
    pub report_id: Uuid,
    pub summary: String,
    pub severity: FindingSeverity,
    pub status: FindingStatus,
    pub related_constraint_ids: Vec<Uuid>,
}

impl ActiveContradiction {
    /// Build from a `ContradictionReport` reference.
    fn from_report(report: &ContradictionReport) -> Self {
        Self {
            report_id: report.id,
            summary: report.summary.clone(),
            severity: report.severity,
            status: report.status,
            related_constraint_ids: report.related_constraint_ids.clone(),
        }
    }
}

/// A relevant decision returned by preflight.
#[derive(Debug, Clone, Serialize)]
pub struct PreflightDecision {
    pub decision_id: Uuid,
    pub title: String,
    pub level: DecisionLevel,
    pub signoff_state: bicameral_api::dashboard::SignoffState,
    pub compliance_state: ComplianceState,
    pub review_state: bicameral_api::candidate::ReviewLifecycleState,
    /// All relevance reasons that matched (deduplicated).
    pub relevance_reasons: Vec<PreflightRelevance>,
    /// Active (open or acknowledged) contradiction findings affecting this
    /// decision. Deduplicated by `report_id`. Empty when no active
    /// contradictions exist for this decision's constraint.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_contradictions: Vec<ActiveContradiction>,
}

/// A relevant candidate returned by preflight.
#[derive(Debug, Clone, Serialize)]
pub struct PreflightCandidate {
    pub candidate_id: Uuid,
    pub title: String,
    pub candidate_state: CandidateState,
}

/// A snapshot considered during preflight.
#[derive(Debug, Clone, Serialize)]
pub struct SnapshotScopeEntry {
    pub name: String,
    pub graph_snapshot_id: Option<String>,
    pub validated_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge_base_sha: Option<String>,
}

/// A scope that could not be resolved locally.
#[derive(Debug, Clone, Serialize)]
pub struct UnknownScopeEntry {
    pub scope: String,
    pub state: String,
    pub detail: String,
}

/// Readiness metadata for the preflight response.
#[derive(Debug, Clone, Serialize)]
pub struct PreflightReadiness {
    pub state: GraphReadiness,
    pub snapshot_scope: Vec<SnapshotScopeEntry>,
    pub unknown_scope: Vec<UnknownScopeEntry>,
}

/// Code-aware locator context enrichment for preflight.
///
/// Carries CodeLocator candidate results as advisory discovery context.
/// All candidates include explicit evidence/readiness caveats. This context
/// is purely informational — it does not assert compliance, safety,
/// no-conflict, or blocking status.
#[derive(Debug, Clone, Serialize)]
pub struct PreflightLocatorContext {
    /// Graph readiness at time of locator invocation.
    pub graph_readiness: LocatorGraphReadiness,
    /// Locator candidates discovered for the query files/symbols.
    /// Each candidate carries its own `evidence_state` caveat.
    pub candidates: Vec<LocatorCandidate>,
    /// Readiness caveats describing limitations of the locator results.
    pub caveats: Vec<String>,
}

/// Progressive preflight response per ADR-0019.
#[derive(Debug, Clone, Serialize)]
pub struct PreflightResponse {
    pub relevant_decisions: Vec<PreflightDecision>,
    pub relevant_candidates: Vec<PreflightCandidate>,
    pub readiness: PreflightReadiness,
    pub warnings: Vec<String>,
    /// Optional code-aware locator context. Present only when CodeLocator
    /// was invoked (workspace available and query contains files/symbols).
    /// All candidates carry evidence/readiness caveats — they are discovery
    /// aids, not compliance or safety assertions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locator_context: Option<PreflightLocatorContext>,
    /// Optional daemon-authored commit-driven freshness read-model (issue
    /// #625). Evidence-only: it never mutates BindingEvidence, Decision state,
    /// signoff, compliance, no-conflict, or merge-safety. Computed by a later
    /// runtime slice; `None` until then.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub freshness: Option<bicameral_api::freshness::PreflightFreshness>,
    /// Optional daemon-authored semantic recall contribution (issue #640).
    /// Advisory read-model: renders graph-verified semantic targets and
    /// approximate hints (kept distinct) plus caveats/next actions. Never
    /// mutates BindingEvidence, Decision state, signoff, compliance,
    /// no-conflict, or merge-safety, and never asserts safe-to-proceed.
    /// Absent (`None`) when no semantic provider is configured, so the no-op
    /// stays quiet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_recall: Option<bicameral_api::semantic_recall::SemanticRecallView>,
}

impl PreflightResponse {
    /// Attach a daemon-authored freshness read-model. Evidence-only.
    pub fn with_freshness(
        mut self,
        freshness: bicameral_api::freshness::PreflightFreshness,
    ) -> Self {
        self.freshness = Some(freshness);
        self
    }

    /// Attach a daemon-authored semantic recall read-model. Advisory,
    /// evidence-only.
    pub fn with_semantic_recall(
        mut self,
        semantic_recall: bicameral_api::semantic_recall::SemanticRecallView,
    ) -> Self {
        self.semantic_recall = Some(semantic_recall);
        self
    }
}

// ── Graph context ─────────────────────────────────────────────────────────

/// Optional graph context for grounded preflight.
///
/// Constructed by the caller from workspace snapshot resolution. When `None`,
/// preflight falls back to source-only mode.
pub struct PreflightGraphContext {
    pub resolution: SnapshotResolution,
}

// ── Source-only preflight ─────────────────────────────────────────────────

/// Execute a source-only preflight query (backward-compatible entry point).
///
/// Equivalent to `query_preflight_progressive(state, query, None)`.
pub fn query_preflight(state: &ReplayedState, query: &PreflightQuery) -> PreflightResponse {
    query_preflight_progressive(state, query, None)
}

// ── Progressive preflight ─────────────────────────────────────────────────

/// Execute a progressive preflight query.
///
/// When `graph_ctx` is `None`, operates in source-only mode: all decisions
/// report `compliance_state = ungrounded`, no snapshot scope, and an
/// unknown_scope entry for graph data.
///
/// When `graph_ctx` is `Some`, additionally matches decisions via their
/// accepted `BindingEvidence` targets. Decisions with verified bindings
/// whose targets overlap with query files/symbols receive
/// `BindingTargetMatch` relevance and preserve their existing compliance
/// state from binding evaluation. Source-only matches without binding
/// evidence remain `ungrounded`.
///
/// Preflight **never mutates** compliance, signoff, review, binding, or
/// conflict state regardless of mode.
pub fn query_preflight_progressive(
    state: &ReplayedState,
    query: &PreflightQuery,
    graph_ctx: Option<&PreflightGraphContext>,
) -> PreflightResponse {
    let mut warnings = Vec::new();
    let mut snapshot_scope = Vec::new();
    let mut unknown_scope = Vec::new();

    // Resolve graph readiness.
    let graph_readiness = match graph_ctx {
        Some(ctx) => {
            let view = GraphReadinessView::from_resolution(&ctx.resolution);
            if let Some(snap) = ctx.resolution.snapshot() {
                snapshot_scope.push(SnapshotScopeEntry {
                    name: "HEAD".to_string(),
                    graph_snapshot_id: Some(snap.graph_snapshot_id.clone()),
                    validated_sha: Some(snap.commit_sha.clone()),
                    ref_name: snap.ref_name.clone(),
                    merge_base_sha: snap.merge_base_sha.clone(),
                });
            }
            view.readiness
        }
        None => {
            unknown_scope.push(UnknownScopeEntry {
                scope: "graph".to_string(),
                state: "not_configured".to_string(),
                detail: "Graph data unavailable. Using source-only preflight.".to_string(),
            });
            GraphReadiness::NotConfigured
        }
    };

    // Always disclose that local preflight cannot see team-wide scope.
    unknown_scope.push(UnknownScopeEntry {
        scope: "team_active_prs".to_string(),
        state: "hosted_required".to_string(),
        detail: "Local preflight cannot see team-wide active PR branches.".to_string(),
    });

    let graph_available = graph_readiness == GraphReadiness::Ready;

    if !query.symbols.is_empty() && !graph_available {
        warnings.push(
            "Symbol grounding is not available in source-only preflight. \
             Symbols are matched against decision text only."
                .to_string(),
        );
    }

    let has_query_terms =
        !query.files.is_empty() || !query.symbols.is_empty() || query.diff_context.is_some();

    let query_terms: Vec<String> = build_query_terms(query);

    // Build binding target index when graph is available.
    let binding_matches: HashSet<Uuid> = if graph_available && has_query_terms {
        find_binding_matches(state, &query.files, &query.symbols)
    } else {
        HashSet::new()
    };

    // Collect decisions with deduplication.
    let mut seen_decision_ids: HashSet<Uuid> = HashSet::new();
    let mut relevant_decisions: Vec<PreflightDecision> = Vec::new();

    for d in state.decisions.values() {
        let mut reasons: Vec<PreflightRelevance> = Vec::new();

        if !has_query_terms {
            reasons.push(PreflightRelevance::FullLedger);
        } else {
            // Source text match.
            let text = format!("{} {}", d.title, d.body).to_lowercase();
            if query_terms.iter().any(|term| text.contains(term)) {
                reasons.push(PreflightRelevance::SourceTextMatch);
            }

            // Binding target match.
            if binding_matches.contains(&d.id) {
                reasons.push(PreflightRelevance::BindingTargetMatch);
            }
        }

        if reasons.is_empty() {
            continue;
        }

        if !seen_decision_ids.insert(d.id) {
            continue;
        }

        // Determine compliance state: source-only results are ungrounded;
        // binding-matched results preserve the existing state from replay
        // (which was set by ComplianceEvaluator if binding evaluation ran).
        let compliance_state = if reasons.contains(&PreflightRelevance::BindingTargetMatch) {
            d.compliance_state
        } else {
            ComplianceState::Ungrounded
        };

        // Attach active contradiction context for this decision.
        // A contradiction is "active" if its status is Open or Acknowledged.
        // We find contradictions whose related_constraint_ids overlap with
        // constraints promoted from this decision.
        let active_contradictions = collect_active_contradictions(state, d.id);

        relevant_decisions.push(PreflightDecision {
            decision_id: d.id,
            title: d.title.clone(),
            level: d.level,
            signoff_state: d.signoff_state,
            compliance_state,
            review_state: d.review_state,
            relevance_reasons: reasons,
            active_contradictions,
        });
    }

    // Deterministic ordering: sort by title, then by decision_id for ties.
    relevant_decisions.sort_by(|a, b| {
        a.title
            .cmp(&b.title)
            .then(a.decision_id.cmp(&b.decision_id))
    });

    // Collect candidates (source-only — bindings do not apply to candidates).
    let mut relevant_candidates: Vec<PreflightCandidate> = state
        .candidates
        .values()
        .filter(|c| {
            if c.candidate_state == CandidateState::Promoted {
                return false;
            }
            if !has_query_terms {
                return true;
            }
            let text = format!("{} {}", c.title, c.description).to_lowercase();
            query_terms.iter().any(|term| text.contains(term))
        })
        .map(|c| PreflightCandidate {
            candidate_id: c.id,
            title: c.title.clone(),
            candidate_state: c.candidate_state,
        })
        .collect();

    relevant_candidates.sort_by(|a, b| a.title.cmp(&b.title));

    // Warnings.
    if !has_query_terms && (!relevant_decisions.is_empty() || !relevant_candidates.is_empty()) {
        warnings.push("No file/symbol/diff filter specified. Returning full ledger.".to_string());
    }

    if has_query_terms && relevant_decisions.is_empty() && relevant_candidates.is_empty() {
        warnings.push("No decisions or candidates match the query terms.".to_string());
    }

    if graph_available {
        warnings
            .push("Local no-conflict result is limited to resolved local snapshots.".to_string());
    }

    PreflightResponse {
        relevant_decisions,
        relevant_candidates,
        readiness: PreflightReadiness {
            state: graph_readiness,
            snapshot_scope,
            unknown_scope,
        },
        warnings,
        locator_context: None,
        freshness: None,
        semantic_recall: None,
    }
}

// ── Binding target matching ───────────────────────────────────────────────

/// Find decisions whose accepted `BindingEvidence` targets overlap with
/// query files or symbols.
fn find_binding_matches(
    state: &ReplayedState,
    files: &[String],
    symbols: &[String],
) -> HashSet<Uuid> {
    let mut matched = HashSet::new();

    let file_set: HashSet<String> = files.iter().map(|f| f.to_lowercase()).collect();
    let file_stems: HashSet<String> = files
        .iter()
        .filter_map(|f| {
            std::path::Path::new(f)
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
        })
        .collect();

    let symbol_set: HashSet<String> = symbols.iter().map(|s| s.to_lowercase()).collect();
    let symbol_stems: HashSet<String> = symbols
        .iter()
        .filter_map(|s| s.rsplit("::").next().map(|n| n.to_lowercase()))
        .collect();

    for (decision_id, bindings) in &state.bindings {
        for binding in bindings {
            if binding_target_matches(binding, &file_set, &file_stems, &symbol_set, &symbol_stems) {
                matched.insert(*decision_id);
                break;
            }
        }
    }

    matched
}

fn binding_target_matches(
    binding: &BindingEvidence,
    file_set: &HashSet<String>,
    file_stems: &HashSet<String>,
    symbol_set: &HashSet<String>,
    symbol_stems: &HashSet<String>,
) -> bool {
    let target_lower = binding.target.to_lowercase();
    match binding.binding_type {
        BindingType::FilePath | BindingType::CodeRegion => {
            file_set.contains(&target_lower)
                || file_stems.contains(&target_lower)
                || file_set.iter().any(|f| target_lower.contains(f.as_str()))
                || file_stems.iter().any(|f| target_lower.contains(f.as_str()))
        }
        BindingType::Symbol => {
            symbol_set.contains(&target_lower)
                || symbol_stems.contains(&target_lower)
                || symbol_set.iter().any(|s| target_lower.contains(s.as_str()))
                || symbol_stems
                    .iter()
                    .any(|s| target_lower.contains(s.as_str()))
        }
        BindingType::Dependency | BindingType::DeploySurface => {
            // For dependency/deploy bindings, check both file and symbol sets.
            file_set.contains(&target_lower) || symbol_set.contains(&target_lower)
        }
    }
}

// ── Query term building ───────────────────────────────────────────────────

fn build_query_terms(query: &PreflightQuery) -> Vec<String> {
    let mut terms = Vec::new();

    for file in &query.files {
        let lower = file.to_lowercase();
        terms.push(lower.clone());
        if let Some(name) = std::path::Path::new(file).file_name() {
            let n = name.to_string_lossy().to_lowercase();
            terms.push(n.clone());
            terms.push(n.replace('_', " "));
        }
        if let Some(stem) = std::path::Path::new(file).file_stem() {
            let s = stem.to_string_lossy().to_lowercase();
            terms.push(s.clone());
            terms.push(s.replace('_', " "));
        }
    }

    for sym in &query.symbols {
        let lower = sym.to_lowercase();
        terms.push(lower.clone());
        terms.push(lower.replace('_', " "));
        if let Some(last) = sym.rsplit("::").next() {
            let l = last.to_lowercase();
            terms.push(l.clone());
            terms.push(l.replace('_', " "));
        }
    }

    if let Some(diff) = &query.diff_context {
        for line in diff.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('+') || trimmed.starts_with('-') {
                let content = trimmed[1..].trim();
                if content.len() >= 4 {
                    terms.push(content.to_lowercase());
                }
                for word in content.split_whitespace() {
                    let w = word.to_lowercase();
                    if w.len() >= 3 {
                        terms.push(w);
                    }
                }
            }
        }
    }

    terms
}

// ── Active contradiction context ──────────────────────────────────────────

/// Collect active (open or acknowledged) contradiction findings that affect
/// the given decision. A contradiction affects a decision when its
/// `related_constraint_ids` contains any catalog constraint whose
/// `decision_id` matches.
///
/// Results are deduplicated by `report_id` and sorted for determinism.
fn collect_active_contradictions(
    state: &ReplayedState,
    decision_id: Uuid,
) -> Vec<ActiveContradiction> {
    // Build the set of constraint IDs belonging to this decision.
    let constraint_ids: HashSet<Uuid> = state
        .catalog
        .values()
        .filter(|c| c.decision_id == decision_id)
        .map(|c| c.id)
        .collect();

    if constraint_ids.is_empty() {
        return Vec::new();
    }

    let mut seen = HashSet::new();
    let mut result: Vec<ActiveContradiction> = state
        .contradictions
        .values()
        .filter(|r| matches!(r.status, FindingStatus::Open | FindingStatus::Acknowledged))
        .filter(|r| {
            r.related_constraint_ids
                .iter()
                .any(|cid| constraint_ids.contains(cid))
        })
        .filter(|r| seen.insert(r.id))
        .map(ActiveContradiction::from_report)
        .collect();

    // Sort by report_id for deterministic output.
    result.sort_by_key(|a| a.report_id);
    result
}

// ── CodeLocator enrichment ────────────────────────────────────────────────

/// Build a `PreflightLocatorContext` from a `LocatorResult`.
///
/// Propagates the locator's graph readiness and candidates with explicit
/// evidence/readiness caveats. Never claims safety, compliance, no-conflict,
/// or blocking status.
///
/// Caveats are derived from:
/// - Graph readiness state (stale, indexing, not configured, failed).
/// - Per-candidate evidence state (approximate, stale, not indexed, etc.).
///
/// When graph data is unavailable or stale, the caveats explicitly say so
/// rather than silently omitting results.
pub fn build_locator_context(locator_result: &LocatorResult) -> PreflightLocatorContext {
    let mut caveats = Vec::new();

    // Graph-level readiness caveats.
    match locator_result.graph_readiness {
        LocatorGraphReadiness::Ready => {}
        LocatorGraphReadiness::Stale => {
            caveats.push(
                "Graph data is stale. Locator candidates may not reflect current code state. \
                 Do not interpret absence of results as no-conflict or safe."
                    .to_string(),
            );
        }
        LocatorGraphReadiness::Indexing => {
            caveats.push(
                "Graph is currently indexing. Results are incomplete. \
                 Do not interpret partial results as safe or conflict-free."
                    .to_string(),
            );
        }
        LocatorGraphReadiness::NotConfigured => {
            caveats.push(
                "Graph is not configured. No code-aware locator candidates available. \
                 Source-only fallback is in effect."
                    .to_string(),
            );
        }
        LocatorGraphReadiness::Failed => {
            caveats.push(
                "Graph snapshot resolution failed. Locator candidates are unavailable. \
                 Do not interpret this as no-conflict or compliant."
                    .to_string(),
            );
        }
    }

    // Per-candidate evidence caveats (deduplicated at type level).
    let has_stale = locator_result
        .candidates
        .iter()
        .any(|c| c.evidence_state == LocatorEvidenceState::UnknownStale);
    let has_not_indexed = locator_result
        .candidates
        .iter()
        .any(|c| c.evidence_state == LocatorEvidenceState::UnknownNotIndexed);
    let has_approximate = locator_result
        .candidates
        .iter()
        .any(|c| c.evidence_state == LocatorEvidenceState::ApproximateCandidate);

    if has_stale {
        caveats.push(
            "Some candidates have stale evidence — code may have changed since last indexing."
                .to_string(),
        );
    }
    if has_not_indexed {
        caveats.push(
            "Some candidates reference files/symbols not yet indexed by the graph.".to_string(),
        );
    }
    if has_approximate {
        caveats.push(
            "Some candidates are approximate — not verified against current graph state."
                .to_string(),
        );
    }

    // Always include the non-authority disclaimer when candidates are present.
    if !locator_result.candidates.is_empty() {
        caveats.push(
            "Locator candidates are advisory discovery context only. \
             They do not assert compliance, safety, no-conflict, or blocking status."
                .to_string(),
        );
    }

    PreflightLocatorContext {
        graph_readiness: locator_result.graph_readiness,
        candidates: locator_result.candidates.clone(),
        caveats,
    }
}

/// Enrich a `PreflightResponse` with optional `LocatorResult` context.
///
/// If `locator_result` is `None`, returns the response unchanged.
/// If `Some`, builds locator context with explicit caveats.
pub fn enrich_with_locator(
    mut response: PreflightResponse,
    locator_result: Option<&LocatorResult>,
) -> PreflightResponse {
    response.locator_context = locator_result.map(build_locator_context);
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_store_adapters::MemoryAdapter;
    use crate::replay::replay_from_adapter;
    use bicameral_api::candidate::DecisionCandidate;
    use bicameral_api::event_store::{
        BindToCodePayload, EventStoreAdapter, EventStoreEntry, EventType, IngestPayload,
    };
    use bicameral_api::evidence::{BindingConfidence, GraphEvidenceState, SourceEvidence};
    use bicameral_api::source::Source;

    fn seed_with_decisions() -> ReplayedState {
        let adapter = MemoryAdapter::new();

        for (uri, title, desc) in [
            (
                "jira://PROJ-1",
                "API gateway rate limiting",
                "All public API endpoints must implement rate limiting",
            ),
            (
                "slack://msg-1",
                "Database migration strategy",
                "Use incremental migration from SQLite to PostgreSQL",
            ),
            (
                "github://pr-1",
                "Auth service architecture",
                "Authentication uses JWT with RS256 in auth_service module",
            ),
        ] {
            let source = Source {
                uri: uri.to_string(),
                source_type: "test".to_string(),
                label: None,
                source_kind: None,
            };
            let evidence = vec![SourceEvidence::new(
                format!("inline:{uri}"),
                0,
                10,
                "test".to_string(),
                0.9,
            )];
            let mut candidate = DecisionCandidate::new(
                title.to_string(),
                desc.to_string(),
                DecisionLevel::Architecture,
                uri.to_string(),
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
        }

        replay_from_adapter(&adapter).unwrap()
    }

    fn seed_with_bindings() -> ReplayedState {
        let adapter = MemoryAdapter::new();

        // Decision 1: bound to src/gateway.rs via FilePath
        let source1 = Source {
            uri: "jira://PROJ-1".to_string(),
            source_type: "test".to_string(),
            label: None,
            source_kind: None,
        };
        let ev1 = vec![SourceEvidence::new(
            "inline:jira://PROJ-1".to_string(),
            0,
            10,
            "test".to_string(),
            0.9,
        )];
        let mut cand1 = DecisionCandidate::new(
            "API gateway rate limiting".to_string(),
            "All public API endpoints must implement rate limiting".to_string(),
            DecisionLevel::Architecture,
            "jira://PROJ-1".to_string(),
        );
        cand1.evidence_refs = ev1.iter().map(|e| e.id).collect();
        let cand1_id = cand1.id;
        adapter
            .append(EventStoreEntry::new(
                EventType::Ingest,
                serde_json::to_value(&IngestPayload {
                    source: source1,
                    snapshot: None,
                    evidence: ev1,
                    candidate: cand1,
                    evidence_references: Vec::new(),
                    provenance: None,
                })
                .unwrap(),
                "test".to_string(),
            ))
            .unwrap();

        let decision1_id = Uuid::new_v4();
        adapter
            .append(EventStoreEntry::new(
                EventType::AcceptCandidate,
                serde_json::json!({
                    "candidate_id": cand1_id.to_string(),
                    "decision_id": decision1_id.to_string(),
                }),
                "test".to_string(),
            ))
            .unwrap();

        // Bind decision1 to src/gateway.rs
        let binding1 = BindingEvidence::new(
            cand1_id,
            BindingType::FilePath,
            "src/gateway.rs".to_string(),
            BindingConfidence::High,
        );
        adapter
            .append(EventStoreEntry::new(
                EventType::BindToCode,
                serde_json::to_value(&BindToCodePayload {
                    decision_id: decision1_id,
                    binding: binding1,
                    graph_snapshot_id: "gs-test-fixture".to_string(),
                    validated_sha: "abc123".to_string(),
                    evidence_state: GraphEvidenceState::Verified,
                    content_hash: None,
                    line_start: None,
                    line_end: None,
                })
                .unwrap(),
                "test".to_string(),
            ))
            .unwrap();

        // Decision 2: bound to auth_service::jwt_verify via Symbol
        let source2 = Source {
            uri: "github://pr-1".to_string(),
            source_type: "test".to_string(),
            label: None,
            source_kind: None,
        };
        let ev2 = vec![SourceEvidence::new(
            "inline:github://pr-1".to_string(),
            0,
            10,
            "test".to_string(),
            0.9,
        )];
        let mut cand2 = DecisionCandidate::new(
            "Auth service architecture".to_string(),
            "Authentication uses JWT with RS256 in auth_service module".to_string(),
            DecisionLevel::Architecture,
            "github://pr-1".to_string(),
        );
        cand2.evidence_refs = ev2.iter().map(|e| e.id).collect();
        let cand2_id = cand2.id;
        adapter
            .append(EventStoreEntry::new(
                EventType::Ingest,
                serde_json::to_value(&IngestPayload {
                    source: source2,
                    snapshot: None,
                    evidence: ev2,
                    candidate: cand2,
                    evidence_references: Vec::new(),
                    provenance: None,
                })
                .unwrap(),
                "test".to_string(),
            ))
            .unwrap();

        let decision2_id = Uuid::new_v4();
        adapter
            .append(EventStoreEntry::new(
                EventType::AcceptCandidate,
                serde_json::json!({
                    "candidate_id": cand2_id.to_string(),
                    "decision_id": decision2_id.to_string(),
                }),
                "test".to_string(),
            ))
            .unwrap();

        // Bind decision2 to auth_service::jwt_verify symbol
        let binding2 = BindingEvidence::new(
            cand2_id,
            BindingType::Symbol,
            "auth_service::jwt_verify".to_string(),
            BindingConfidence::High,
        );
        adapter
            .append(EventStoreEntry::new(
                EventType::BindToCode,
                serde_json::to_value(&BindToCodePayload {
                    decision_id: decision2_id,
                    binding: binding2,
                    graph_snapshot_id: "gs-test-fixture".to_string(),
                    validated_sha: "abc123".to_string(),
                    evidence_state: GraphEvidenceState::Verified,
                    content_hash: None,
                    line_start: None,
                    line_end: None,
                })
                .unwrap(),
                "test".to_string(),
            ))
            .unwrap();

        // Decision 3: no binding
        let source3 = Source {
            uri: "slack://msg-1".to_string(),
            source_type: "test".to_string(),
            label: None,
            source_kind: None,
        };
        let ev3 = vec![SourceEvidence::new(
            "inline:slack://msg-1".to_string(),
            0,
            10,
            "test".to_string(),
            0.9,
        )];
        let mut cand3 = DecisionCandidate::new(
            "Database migration strategy".to_string(),
            "Use incremental migration from SQLite to PostgreSQL".to_string(),
            DecisionLevel::Architecture,
            "slack://msg-1".to_string(),
        );
        cand3.evidence_refs = ev3.iter().map(|e| e.id).collect();
        let cand3_id = cand3.id;
        adapter
            .append(EventStoreEntry::new(
                EventType::Ingest,
                serde_json::to_value(&IngestPayload {
                    source: source3,
                    snapshot: None,
                    evidence: ev3,
                    candidate: cand3,
                    evidence_references: Vec::new(),
                    provenance: None,
                })
                .unwrap(),
                "test".to_string(),
            ))
            .unwrap();

        let decision3_id = Uuid::new_v4();
        adapter
            .append(EventStoreEntry::new(
                EventType::AcceptCandidate,
                serde_json::json!({
                    "candidate_id": cand3_id.to_string(),
                    "decision_id": decision3_id.to_string(),
                }),
                "test".to_string(),
            ))
            .unwrap();

        replay_from_adapter(&adapter).unwrap()
    }

    // ── Source-only preflight tests (backward compat) ─────────────────────

    #[test]
    fn preflight_returns_all_with_no_filter() {
        let state = seed_with_decisions();

        let resp = query_preflight(
            &state,
            &PreflightQuery {
                files: vec![],
                symbols: vec![],
                diff_context: None,
                branch: None,
            },
        );

        assert_eq!(resp.relevant_decisions.len(), 3);
        for d in &resp.relevant_decisions {
            assert_eq!(d.compliance_state, ComplianceState::Ungrounded);
            assert!(d
                .relevance_reasons
                .contains(&PreflightRelevance::FullLedger));
        }
    }

    #[test]
    fn preflight_filters_by_file_terms() {
        let state = seed_with_decisions();

        let resp = query_preflight(
            &state,
            &PreflightQuery {
                files: vec!["src/auth_service.rs".to_string()],
                symbols: vec![],
                diff_context: None,
                branch: None,
            },
        );

        assert!(
            !resp.relevant_decisions.is_empty(),
            "auth_service file should match auth service decision"
        );
        for d in &resp.relevant_decisions {
            assert_eq!(d.compliance_state, ComplianceState::Ungrounded);
            assert!(d
                .relevance_reasons
                .contains(&PreflightRelevance::SourceTextMatch));
        }
    }

    #[test]
    fn preflight_compliance_always_ungrounded() {
        let state = seed_with_decisions();

        let resp = query_preflight(
            &state,
            &PreflightQuery {
                files: vec![],
                symbols: vec![],
                diff_context: None,
                branch: None,
            },
        );

        for d in &resp.relevant_decisions {
            assert_eq!(
                d.compliance_state,
                ComplianceState::Ungrounded,
                "Source-only preflight must report ungrounded compliance"
            );
        }
    }

    #[test]
    fn preflight_no_false_conflict_claims() {
        let state = seed_with_decisions();

        let resp = query_preflight(
            &state,
            &PreflightQuery {
                files: vec!["nonexistent.rs".to_string()],
                symbols: vec![],
                diff_context: None,
                branch: None,
            },
        );

        assert!(
            resp.relevant_decisions.is_empty(),
            "Preflight must not claim false relevance for unrelated files"
        );
    }

    #[test]
    fn preflight_warns_about_symbol_grounding() {
        let state = seed_with_decisions();

        let resp = query_preflight(
            &state,
            &PreflightQuery {
                files: vec![],
                symbols: vec!["auth_service::jwt_verify".to_string()],
                diff_context: None,
                branch: None,
            },
        );

        assert!(
            resp.warnings.iter().any(|w| w.contains("Symbol grounding")),
            "Should warn that symbol grounding is unavailable"
        );
    }

    #[test]
    fn preflight_excludes_promoted_candidates() {
        let state = seed_with_decisions();

        let resp = query_preflight(
            &state,
            &PreflightQuery {
                files: vec![],
                symbols: vec![],
                diff_context: None,
                branch: None,
            },
        );

        assert!(
            resp.relevant_candidates.is_empty(),
            "All candidates were promoted; none should appear in preflight candidates"
        );
    }

    #[test]
    fn preflight_matches_diff_context() {
        let state = seed_with_decisions();

        let diff = "+ implement rate limiting for API gateway\n- old code";

        let resp = query_preflight(
            &state,
            &PreflightQuery {
                files: vec![],
                symbols: vec![],
                diff_context: Some(diff.to_string()),
                branch: None,
            },
        );

        assert!(
            !resp.relevant_decisions.is_empty(),
            "Diff context mentioning rate limiting should match"
        );
    }

    // ── Response shape tests (ADR-0019) ──────────────────────────────────

    #[test]
    fn source_only_includes_readiness_and_unknown_scope() {
        let state = seed_with_decisions();

        let resp = query_preflight(
            &state,
            &PreflightQuery {
                files: vec![],
                symbols: vec![],
                diff_context: None,
                branch: None,
            },
        );

        assert_eq!(resp.readiness.state, GraphReadiness::NotConfigured);
        assert!(resp.readiness.snapshot_scope.is_empty());
        assert!(
            resp.readiness
                .unknown_scope
                .iter()
                .any(|u| u.scope == "graph"),
            "Source-only should report graph as unknown scope"
        );
        assert!(
            resp.readiness
                .unknown_scope
                .iter()
                .any(|u| u.scope == "team_active_prs"),
            "Should always report team_active_prs as unknown scope"
        );
    }

    #[test]
    fn source_only_no_conflict_never_global() {
        let state = seed_with_decisions();

        let resp = query_preflight(
            &state,
            &PreflightQuery {
                files: vec!["nonexistent.rs".to_string()],
                symbols: vec![],
                diff_context: None,
                branch: None,
            },
        );

        // Even with no matches, local no-conflict is not global consistency.
        assert!(
            resp.readiness
                .unknown_scope
                .iter()
                .any(|u| u.scope == "team_active_prs"),
            "Must disclose team scope limitation"
        );
    }

    // ── Progressive (graph-aware) preflight tests ─────────────────────────

    fn make_resolved_ctx() -> PreflightGraphContext {
        use bicameral_api::evidence::{GraphSnapshot, IndexSource};
        PreflightGraphContext {
            resolution: SnapshotResolution::Resolved {
                snapshot: GraphSnapshot {
                    graph_snapshot_id: "gs-test-abc123".to_string(),
                    repo_id: "repo-test".to_string(),
                    commit_sha: "abc123def456".to_string(),
                    index_version: "file-inspect-v1".to_string(),
                    ref_name: Some("refs/heads/feature/preflight".to_string()),
                    merge_base_sha: None,
                    indexed_at: None,
                    index_source: Some(IndexSource::LocalDaemon),
                },
            },
        }
    }

    #[test]
    fn graph_preflight_matches_binding_target_file() {
        let state = seed_with_bindings();
        let ctx = make_resolved_ctx();

        let resp = query_preflight_progressive(
            &state,
            &PreflightQuery {
                files: vec!["src/gateway.rs".to_string()],
                symbols: vec![],
                diff_context: None,
                branch: None,
            },
            Some(&ctx),
        );

        let gateway_decision = resp
            .relevant_decisions
            .iter()
            .find(|d| d.title == "API gateway rate limiting");
        assert!(
            gateway_decision.is_some(),
            "Should match via binding target file"
        );
        let gd = gateway_decision.unwrap();
        assert!(
            gd.relevance_reasons
                .contains(&PreflightRelevance::BindingTargetMatch),
            "Should report BindingTargetMatch relevance"
        );
    }

    #[test]
    fn graph_preflight_matches_binding_target_symbol() {
        let state = seed_with_bindings();
        let ctx = make_resolved_ctx();

        let resp = query_preflight_progressive(
            &state,
            &PreflightQuery {
                files: vec![],
                symbols: vec!["auth_service::jwt_verify".to_string()],
                diff_context: None,
                branch: None,
            },
            Some(&ctx),
        );

        let auth_decision = resp
            .relevant_decisions
            .iter()
            .find(|d| d.title == "Auth service architecture");
        assert!(
            auth_decision.is_some(),
            "Should match via binding target symbol"
        );
        let ad = auth_decision.unwrap();
        assert!(
            ad.relevance_reasons
                .contains(&PreflightRelevance::BindingTargetMatch),
            "Should report BindingTargetMatch relevance"
        );
    }

    #[test]
    fn graph_preflight_deduplicates_source_and_binding_match() {
        let state = seed_with_bindings();
        let ctx = make_resolved_ctx();

        // "gateway" appears in both the decision body text ("rate limiting")
        // AND the binding target ("src/gateway.rs"). The file query term
        // "gateway" matches the decision text "API gateway rate limiting".
        let resp = query_preflight_progressive(
            &state,
            &PreflightQuery {
                files: vec!["src/gateway.rs".to_string()],
                symbols: vec![],
                diff_context: None,
                branch: None,
            },
            Some(&ctx),
        );

        let gateway_decisions: Vec<_> = resp
            .relevant_decisions
            .iter()
            .filter(|d| d.title == "API gateway rate limiting")
            .collect();
        assert_eq!(
            gateway_decisions.len(),
            1,
            "Decision should appear exactly once even with multiple relevance signals"
        );
        let gd = gateway_decisions[0];
        assert!(
            gd.relevance_reasons
                .contains(&PreflightRelevance::SourceTextMatch),
            "Should include SourceTextMatch"
        );
        assert!(
            gd.relevance_reasons
                .contains(&PreflightRelevance::BindingTargetMatch),
            "Should include BindingTargetMatch"
        );
    }

    #[test]
    fn graph_preflight_unbound_decision_stays_ungrounded() {
        let state = seed_with_bindings();
        let ctx = make_resolved_ctx();

        // Query for all decisions (no filter).
        let resp = query_preflight_progressive(
            &state,
            &PreflightQuery {
                files: vec![],
                symbols: vec![],
                diff_context: None,
                branch: None,
            },
            Some(&ctx),
        );

        let db_decision = resp
            .relevant_decisions
            .iter()
            .find(|d| d.title == "Database migration strategy");
        assert!(
            db_decision.is_some(),
            "Unbound decision should still appear"
        );
        let dd = db_decision.unwrap();
        assert_eq!(
            dd.compliance_state,
            ComplianceState::Ungrounded,
            "Unbound decision must report ungrounded compliance"
        );
    }

    #[test]
    fn graph_preflight_includes_snapshot_scope() {
        let state = seed_with_decisions();
        let ctx = make_resolved_ctx();

        let resp = query_preflight_progressive(
            &state,
            &PreflightQuery {
                files: vec![],
                symbols: vec![],
                diff_context: None,
                branch: None,
            },
            Some(&ctx),
        );

        assert_eq!(resp.readiness.state, GraphReadiness::Ready);
        assert_eq!(resp.readiness.snapshot_scope.len(), 1);
        assert_eq!(resp.readiness.snapshot_scope[0].name, "HEAD");
        assert_eq!(
            resp.readiness.snapshot_scope[0]
                .graph_snapshot_id
                .as_deref(),
            Some("gs-test-abc123")
        );
    }

    #[test]
    fn graph_preflight_stale_resolution_degrades_to_not_ready() {
        let state = seed_with_decisions();
        let ctx = PreflightGraphContext {
            resolution: SnapshotResolution::StaleIndex {
                requested_ref: "HEAD".to_string(),
                detail: "index version mismatch".to_string(),
            },
        };

        let resp = query_preflight_progressive(
            &state,
            &PreflightQuery {
                files: vec![],
                symbols: vec![],
                diff_context: None,
                branch: None,
            },
            Some(&ctx),
        );

        assert_eq!(resp.readiness.state, GraphReadiness::Stale);
        assert!(resp.readiness.snapshot_scope.is_empty());
    }

    #[test]
    fn graph_preflight_no_conflict_warns_local_only() {
        let state = seed_with_bindings();
        let ctx = make_resolved_ctx();

        let resp = query_preflight_progressive(
            &state,
            &PreflightQuery {
                files: vec!["nonexistent.rs".to_string()],
                symbols: vec![],
                diff_context: None,
                branch: None,
            },
            Some(&ctx),
        );

        assert!(
            resp.readiness
                .unknown_scope
                .iter()
                .any(|u| u.scope == "team_active_prs"),
            "Graph preflight must still disclose team scope limitation"
        );
        assert!(
            resp.warnings
                .iter()
                .any(|w| w.contains("limited to resolved local snapshots")),
            "Should warn that no-conflict is local only"
        );
    }

    // ── Deterministic ordering tests ──────────────────────────────────────

    #[test]
    fn preflight_ordering_is_deterministic() {
        let state = seed_with_decisions();
        let query = PreflightQuery {
            files: vec![],
            symbols: vec![],
            diff_context: None,
            branch: None,
        };

        let resp1 = query_preflight(&state, &query);
        let resp2 = query_preflight(&state, &query);

        let titles1: Vec<_> = resp1.relevant_decisions.iter().map(|d| &d.title).collect();
        let titles2: Vec<_> = resp2.relevant_decisions.iter().map(|d| &d.title).collect();
        assert_eq!(
            titles1, titles2,
            "Ordering must be deterministic across calls"
        );

        // Verify alphabetical.
        for w in titles1.windows(2) {
            assert!(w[0] <= w[1], "Decisions must be alphabetically ordered");
        }
    }

    // ── Preflight does not mutate state ───────────────────────────────────

    #[test]
    fn preflight_does_not_mutate_state() {
        let state = seed_with_bindings();
        let ctx = make_resolved_ctx();

        // Snapshot state before preflight.
        let decisions_before: Vec<_> = state
            .decisions
            .values()
            .map(|d| (d.id, d.signoff_state, d.compliance_state, d.review_state))
            .collect();
        let bindings_count_before: usize = state.bindings.values().map(|v| v.len()).sum();

        // Run preflight.
        let _resp = query_preflight_progressive(
            &state,
            &PreflightQuery {
                files: vec!["src/gateway.rs".to_string()],
                symbols: vec!["auth_service::jwt_verify".to_string()],
                diff_context: Some("+ add rate limiting".to_string()),
                branch: None,
            },
            Some(&ctx),
        );

        // Verify state unchanged.
        let decisions_after: Vec<_> = state
            .decisions
            .values()
            .map(|d| (d.id, d.signoff_state, d.compliance_state, d.review_state))
            .collect();
        let bindings_count_after: usize = state.bindings.values().map(|v| v.len()).sum();

        assert_eq!(
            decisions_before.len(),
            decisions_after.len(),
            "Decision count must not change"
        );
        assert_eq!(
            bindings_count_before, bindings_count_after,
            "Binding count must not change"
        );
    }

    // ── CodeLocator enrichment tests (issue #439) ─────────────────────────

    use bicameral_api::tool_request::{
        LocatorCandidate, LocatorConfidence, LocatorEvidenceState, LocatorGraphReadiness,
        LocatorReason, LocatorReasonCode, LocatorResult, LocatorSnapshotInfo, LocatorStatus,
        LocatorTargetType,
    };

    fn make_locator_result(
        readiness: LocatorGraphReadiness,
        candidates: Vec<LocatorCandidate>,
    ) -> LocatorResult {
        LocatorResult {
            status: if candidates.is_empty() {
                LocatorStatus::GraphUnavailable
            } else {
                LocatorStatus::Found
            },
            query: Some("test".to_string()),
            graph_readiness: readiness,
            snapshot: Some(LocatorSnapshotInfo {
                graph_snapshot_id: Some("gs-test".to_string()),
                repo_id: Some("repo-test".to_string()),
                commit_sha: Some("abc123".to_string()),
                index_version: Some("v1".to_string()),
                ref_name: None,
            }),
            scope: None,
            candidates,
            warnings: vec![],
            next_actions: vec![],
        }
    }

    fn make_candidate(evidence_state: LocatorEvidenceState) -> LocatorCandidate {
        LocatorCandidate {
            target: "src/example.rs".to_string(),
            target_type: LocatorTargetType::FilePath,
            file: "src/example.rs".to_string(),
            range: None,
            symbol: None,
            evidence_state,
            confidence: LocatorConfidence::Medium,
            reasons: vec![LocatorReason {
                code: LocatorReasonCode::ExactPath,
                detail: "test".to_string(),
            }],
            graph_snapshot_id: Some("gs-test".to_string()),
            validated_sha: Some("abc123".to_string()),
            content_hash: None,
            caveats: vec![],
        }
    }

    #[test]
    fn locator_context_graph_unavailable_produces_caveat() {
        let lr = make_locator_result(LocatorGraphReadiness::NotConfigured, vec![]);
        let ctx = build_locator_context(&lr);

        assert_eq!(ctx.graph_readiness, LocatorGraphReadiness::NotConfigured);
        assert!(ctx.candidates.is_empty());
        assert!(
            ctx.caveats.iter().any(|c| c.contains("not configured")),
            "Must caveat that graph is not configured"
        );
        // Must NOT claim safe, no-conflict, compliant.
        for caveat in &ctx.caveats {
            assert!(!caveat.contains("safe to proceed"));
            assert!(!caveat.contains("no conflict"));
            assert!(!caveat.contains("compliant"));
        }
    }

    #[test]
    fn locator_context_graph_stale_produces_caveat() {
        let candidate = make_candidate(LocatorEvidenceState::UnknownStale);
        let lr = make_locator_result(LocatorGraphReadiness::Stale, vec![candidate]);
        let ctx = build_locator_context(&lr);

        assert_eq!(ctx.graph_readiness, LocatorGraphReadiness::Stale);
        assert!(!ctx.candidates.is_empty());
        assert!(
            ctx.caveats.iter().any(|c| c.contains("stale")),
            "Must caveat stale graph: {:?}",
            ctx.caveats
        );
        assert!(
            ctx.caveats
                .iter()
                .any(|c| c.contains("Do not interpret") && c.contains("no-conflict")),
            "Must explicitly warn against false safety claims"
        );
    }

    #[test]
    fn locator_context_verified_candidate_includes_advisory_caveat() {
        let candidate = make_candidate(LocatorEvidenceState::Verified);
        let lr = make_locator_result(LocatorGraphReadiness::Ready, vec![candidate]);
        let ctx = build_locator_context(&lr);

        assert_eq!(ctx.graph_readiness, LocatorGraphReadiness::Ready);
        assert_eq!(ctx.candidates.len(), 1);
        assert_eq!(
            ctx.candidates[0].evidence_state,
            LocatorEvidenceState::Verified
        );
        // Even verified candidates are advisory only.
        assert!(
            ctx.caveats.iter().any(|c| c.contains("advisory")),
            "Even verified candidates must be labeled advisory"
        );
    }

    #[test]
    fn locator_context_no_false_conflict_or_safety_claim() {
        // Graph failed — must not produce any positive safety/conflict claims.
        let lr = make_locator_result(LocatorGraphReadiness::Failed, vec![]);
        let ctx = build_locator_context(&lr);

        assert!(
            ctx.caveats
                .iter()
                .any(|c| c.contains("Do not interpret this as no-conflict or compliant")),
            "Failed graph must explicitly disclaim safety"
        );
        // No positive claims.
        let full_text = ctx.caveats.join(" ");
        assert!(!full_text.contains("safe to proceed"));
        assert!(!full_text.contains("no conflicts found"));
        assert!(!full_text.contains("all clear"));
    }

    #[test]
    fn enrich_with_locator_none_leaves_response_unchanged() {
        let state = seed_with_decisions();
        let resp = query_preflight(
            &state,
            &PreflightQuery {
                files: vec![],
                symbols: vec![],
                diff_context: None,
                branch: None,
            },
        );
        let enriched = enrich_with_locator(resp.clone(), None);
        assert!(enriched.locator_context.is_none());
    }

    #[test]
    fn enrich_with_locator_some_adds_context() {
        let state = seed_with_decisions();
        let resp = query_preflight(
            &state,
            &PreflightQuery {
                files: vec![],
                symbols: vec![],
                diff_context: None,
                branch: None,
            },
        );
        let candidate = make_candidate(LocatorEvidenceState::ApproximateCandidate);
        let lr = make_locator_result(LocatorGraphReadiness::Ready, vec![candidate]);
        let enriched = enrich_with_locator(resp, Some(&lr));
        assert!(enriched.locator_context.is_some());
        let ctx = enriched.locator_context.unwrap();
        assert_eq!(ctx.candidates.len(), 1);
        assert!(
            ctx.caveats.iter().any(|c| c.contains("approximate")),
            "Must caveat approximate candidates"
        );
    }

    #[test]
    fn preflight_read_only_with_locator_enrichment() {
        // Verify that enrichment does not add events (read-only contract).
        let state = seed_with_bindings();
        let ctx = make_resolved_ctx();

        let decisions_before: Vec<_> = state
            .decisions
            .values()
            .map(|d| (d.id, d.compliance_state))
            .collect();

        let resp = query_preflight_progressive(
            &state,
            &PreflightQuery {
                files: vec!["src/gateway.rs".to_string()],
                symbols: vec![],
                diff_context: None,
                branch: None,
            },
            Some(&ctx),
        );

        // Enrich with locator.
        let candidate = make_candidate(LocatorEvidenceState::Verified);
        let lr = make_locator_result(LocatorGraphReadiness::Ready, vec![candidate]);
        let _enriched = enrich_with_locator(resp, Some(&lr));

        // State unchanged — enrichment is read-only.
        let decisions_after: Vec<_> = state
            .decisions
            .values()
            .map(|d| (d.id, d.compliance_state))
            .collect();
        assert_eq!(decisions_before, decisions_after);
    }

    // ── Contradiction preflight tests (#602) ──────────────────────────────

    use bicameral_api::catalog::{ConstraintScope, ConstraintType};
    use bicameral_api::constraint::{ActorType, Attribution};
    use bicameral_api::drift::{ContradictionReport, FindingSeverity, FindingStatus};
    use bicameral_api::event_store::{
        ClassifyConstraintPayload, PromoteToCanonicalPayload, RaiseContradictionPayload,
        TransitionFindingPayload,
    };

    /// Seed a state with two decisions promoted to catalog constraints, one with
    /// an open contradiction. Returns (adapter, decision1_id, decision2_id,
    /// constraint1_id, constraint2_id, report_id).
    fn seed_with_contradiction() -> (MemoryAdapter, Uuid, Uuid, Uuid, Uuid, Uuid) {
        let adapter = MemoryAdapter::new();

        // Decision 1: ingest + accept + approve + promote + classify.
        let source1 = Source {
            uri: "jira://CONTRA-1".to_string(),
            source_type: "test".to_string(),
            label: None,
            source_kind: None,
        };
        let ev1 = vec![SourceEvidence::new(
            "inline:jira://CONTRA-1".to_string(),
            0,
            10,
            "test".to_string(),
            0.9,
        )];
        let mut cand1 = DecisionCandidate::new(
            "Rate limit policy".to_string(),
            "All API endpoints must have rate limiting".to_string(),
            DecisionLevel::Architecture,
            "jira://CONTRA-1".to_string(),
        );
        cand1.evidence_refs = ev1.iter().map(|e| e.id).collect();
        let cand1_id = cand1.id;
        adapter
            .append(EventStoreEntry::new(
                EventType::Ingest,
                serde_json::to_value(&IngestPayload {
                    source: source1,
                    snapshot: None,
                    evidence: ev1,
                    candidate: cand1,
                    evidence_references: Vec::new(),
                    provenance: None,
                })
                .unwrap(),
                "test".to_string(),
            ))
            .unwrap();

        let decision1_id = Uuid::new_v4();
        adapter
            .append(EventStoreEntry::new(
                EventType::AcceptCandidate,
                serde_json::json!({
                    "candidate_id": cand1_id.to_string(),
                    "decision_id": decision1_id.to_string(),
                }),
                "test".to_string(),
            ))
            .unwrap();
        adapter
            .append(EventStoreEntry::new(
                EventType::ApproveSignoff,
                serde_json::json!({ "decision_id": decision1_id.to_string() }),
                "test".to_string(),
            ))
            .unwrap();

        let promo_key1 = "contra-c1";
        adapter
            .append(EventStoreEntry::new(
                EventType::PromoteToCanonical,
                serde_json::to_value(&PromoteToCanonicalPayload {
                    decision_id: decision1_id,
                    title: "Rate limit policy".to_string(),
                    statement: "All API endpoints must have rate limiting".to_string(),
                    evidence_refs: Vec::new(),
                    affected_surfaces: vec!["api".to_string()],
                    reviewer: Attribution {
                        actor_id: "reviewer".to_string(),
                        actor_type: ActorType::Human,
                        display_name: None,
                    },
                    reason: None,
                    idempotency_key: promo_key1.to_string(),
                })
                .unwrap(),
                "test".to_string(),
            ))
            .unwrap();
        adapter
            .append(EventStoreEntry::new(
                EventType::ClassifyConstraint,
                serde_json::to_value(&ClassifyConstraintPayload {
                    idempotency_key: promo_key1.to_string(),
                    constraint_type: ConstraintType::Security,
                    scope: ConstraintScope::Service,
                    attribution: Attribution {
                        actor_id: "classifier".to_string(),
                        actor_type: ActorType::Human,
                        display_name: None,
                    },
                    affected_surfaces: None,
                })
                .unwrap(),
                "test".to_string(),
            ))
            .unwrap();

        // Decision 2: same flow.
        let source2 = Source {
            uri: "jira://CONTRA-2".to_string(),
            source_type: "test".to_string(),
            label: None,
            source_kind: None,
        };
        let ev2 = vec![SourceEvidence::new(
            "inline:jira://CONTRA-2".to_string(),
            0,
            10,
            "test".to_string(),
            0.9,
        )];
        let mut cand2 = DecisionCandidate::new(
            "Open access policy".to_string(),
            "Public endpoints must not require authentication".to_string(),
            DecisionLevel::Architecture,
            "jira://CONTRA-2".to_string(),
        );
        cand2.evidence_refs = ev2.iter().map(|e| e.id).collect();
        let cand2_id = cand2.id;
        adapter
            .append(EventStoreEntry::new(
                EventType::Ingest,
                serde_json::to_value(&IngestPayload {
                    source: source2,
                    snapshot: None,
                    evidence: ev2,
                    candidate: cand2,
                    evidence_references: Vec::new(),
                    provenance: None,
                })
                .unwrap(),
                "test".to_string(),
            ))
            .unwrap();

        let decision2_id = Uuid::new_v4();
        adapter
            .append(EventStoreEntry::new(
                EventType::AcceptCandidate,
                serde_json::json!({
                    "candidate_id": cand2_id.to_string(),
                    "decision_id": decision2_id.to_string(),
                }),
                "test".to_string(),
            ))
            .unwrap();
        adapter
            .append(EventStoreEntry::new(
                EventType::ApproveSignoff,
                serde_json::json!({ "decision_id": decision2_id.to_string() }),
                "test".to_string(),
            ))
            .unwrap();

        let promo_key2 = "contra-c2";
        adapter
            .append(EventStoreEntry::new(
                EventType::PromoteToCanonical,
                serde_json::to_value(&PromoteToCanonicalPayload {
                    decision_id: decision2_id,
                    title: "Open access policy".to_string(),
                    statement: "Public endpoints must not require authentication".to_string(),
                    evidence_refs: Vec::new(),
                    affected_surfaces: vec!["api".to_string()],
                    reviewer: Attribution {
                        actor_id: "reviewer".to_string(),
                        actor_type: ActorType::Human,
                        display_name: None,
                    },
                    reason: None,
                    idempotency_key: promo_key2.to_string(),
                })
                .unwrap(),
                "test".to_string(),
            ))
            .unwrap();
        adapter
            .append(EventStoreEntry::new(
                EventType::ClassifyConstraint,
                serde_json::to_value(&ClassifyConstraintPayload {
                    idempotency_key: promo_key2.to_string(),
                    constraint_type: ConstraintType::Product,
                    scope: ConstraintScope::Service,
                    attribution: Attribution {
                        actor_id: "classifier".to_string(),
                        actor_type: ActorType::Human,
                        display_name: None,
                    },
                    affected_surfaces: None,
                })
                .unwrap(),
                "test".to_string(),
            ))
            .unwrap();

        // Replay to get constraint IDs.
        let state = replay_from_adapter(&adapter).unwrap();
        let c1_id = state.catalog.get(promo_key1).unwrap().id;
        let c2_id = state.catalog.get(promo_key2).unwrap().id;

        // Raise a contradiction between the two constraints.
        let report_id = Uuid::new_v4();
        let now = chrono::Utc::now();
        let report = ContradictionReport {
            id: report_id,
            summary: "Rate limiting conflicts with open access".to_string(),
            severity: FindingSeverity::High,
            status: FindingStatus::Open,
            related_constraint_ids: vec![c1_id, c2_id],
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

        (adapter, decision1_id, decision2_id, c1_id, c2_id, report_id)
    }

    #[test]
    fn preflight_attaches_open_contradiction_context() {
        let (adapter, decision1_id, _decision2_id, _c1, _c2, report_id) = seed_with_contradiction();
        let state = replay_from_adapter(&adapter).unwrap();

        let resp = query_preflight(
            &state,
            &PreflightQuery {
                files: vec![],
                symbols: vec![],
                diff_context: None,
                branch: None,
            },
        );

        // Both decisions should have the contradiction attached.
        let d1 = resp
            .relevant_decisions
            .iter()
            .find(|d| d.decision_id == decision1_id)
            .expect("decision1 should be in preflight");

        assert_eq!(d1.active_contradictions.len(), 1);
        assert_eq!(d1.active_contradictions[0].report_id, report_id);
        assert_eq!(d1.active_contradictions[0].status, FindingStatus::Open);
        assert_eq!(d1.active_contradictions[0].severity, FindingSeverity::High);

        // Decision signoff is still approved — contradiction does not demote.
        assert_eq!(
            d1.signoff_state,
            bicameral_api::dashboard::SignoffState::Approved
        );
    }

    #[test]
    fn preflight_attaches_acknowledged_contradiction_context() {
        let (adapter, decision1_id, _decision2_id, _c1, _c2, report_id) = seed_with_contradiction();

        // Transition finding: Open → Acknowledged.
        use bicameral_api::drift::ReviewTransitionRequest;
        adapter
            .append(EventStoreEntry::new(
                EventType::TransitionFinding,
                serde_json::to_value(&TransitionFindingPayload {
                    request: ReviewTransitionRequest {
                        report_id,
                        to_status: FindingStatus::Acknowledged,
                        actor: Attribution {
                            actor_id: "owner".to_string(),
                            actor_type: ActorType::Human,
                            display_name: None,
                        },
                        note: Some("Acknowledged conflict".to_string()),
                        at: chrono::Utc::now(),
                    },
                })
                .unwrap(),
                "owner".to_string(),
            ))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();
        let resp = query_preflight(
            &state,
            &PreflightQuery {
                files: vec![],
                symbols: vec![],
                diff_context: None,
                branch: None,
            },
        );

        let d1 = resp
            .relevant_decisions
            .iter()
            .find(|d| d.decision_id == decision1_id)
            .expect("decision1 should be in preflight");

        // Acknowledged contradictions are still "active" and attached.
        assert_eq!(d1.active_contradictions.len(), 1);
        assert_eq!(
            d1.active_contradictions[0].status,
            FindingStatus::Acknowledged
        );
    }

    #[test]
    fn preflight_excludes_resolved_contradiction() {
        let (adapter, decision1_id, _decision2_id, _c1, _c2, report_id) = seed_with_contradiction();

        // Transition: Open → Acknowledged → Resolved.
        use bicameral_api::drift::ReviewTransitionRequest;
        adapter
            .append(EventStoreEntry::new(
                EventType::TransitionFinding,
                serde_json::to_value(&TransitionFindingPayload {
                    request: ReviewTransitionRequest {
                        report_id,
                        to_status: FindingStatus::Acknowledged,
                        actor: Attribution {
                            actor_id: "owner".to_string(),
                            actor_type: ActorType::Human,
                            display_name: None,
                        },
                        note: None,
                        at: chrono::Utc::now(),
                    },
                })
                .unwrap(),
                "owner".to_string(),
            ))
            .unwrap();
        adapter
            .append(EventStoreEntry::new(
                EventType::TransitionFinding,
                serde_json::to_value(&TransitionFindingPayload {
                    request: ReviewTransitionRequest {
                        report_id,
                        to_status: FindingStatus::Resolved,
                        actor: Attribution {
                            actor_id: "owner".to_string(),
                            actor_type: ActorType::Human,
                            display_name: None,
                        },
                        note: Some("Resolved by refactoring".to_string()),
                        at: chrono::Utc::now(),
                    },
                })
                .unwrap(),
                "owner".to_string(),
            ))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();
        let resp = query_preflight(
            &state,
            &PreflightQuery {
                files: vec![],
                symbols: vec![],
                diff_context: None,
                branch: None,
            },
        );

        let d1 = resp
            .relevant_decisions
            .iter()
            .find(|d| d.decision_id == decision1_id)
            .expect("decision1 should be in preflight");

        // Resolved contradictions are terminal — not attached.
        assert!(
            d1.active_contradictions.is_empty(),
            "resolved contradiction should not appear in active_contradictions"
        );
    }

    #[test]
    fn preflight_excludes_dismissed_contradiction() {
        let (adapter, decision1_id, _decision2_id, _c1, _c2, report_id) = seed_with_contradiction();

        // Transition: Open → Dismissed.
        use bicameral_api::drift::ReviewTransitionRequest;
        adapter
            .append(EventStoreEntry::new(
                EventType::TransitionFinding,
                serde_json::to_value(&TransitionFindingPayload {
                    request: ReviewTransitionRequest {
                        report_id,
                        to_status: FindingStatus::Dismissed,
                        actor: Attribution {
                            actor_id: "owner".to_string(),
                            actor_type: ActorType::Human,
                            display_name: None,
                        },
                        note: Some("Not a real conflict".to_string()),
                        at: chrono::Utc::now(),
                    },
                })
                .unwrap(),
                "owner".to_string(),
            ))
            .unwrap();

        let state = replay_from_adapter(&adapter).unwrap();
        let resp = query_preflight(
            &state,
            &PreflightQuery {
                files: vec![],
                symbols: vec![],
                diff_context: None,
                branch: None,
            },
        );

        let d1 = resp
            .relevant_decisions
            .iter()
            .find(|d| d.decision_id == decision1_id)
            .expect("decision1 should be in preflight");

        // Dismissed contradictions are terminal — not attached.
        assert!(
            d1.active_contradictions.is_empty(),
            "dismissed contradiction should not appear in active_contradictions"
        );
    }

    #[test]
    fn preflight_contradiction_context_is_idempotent() {
        let (adapter, _d1, _d2, _c1, _c2, _report_id) = seed_with_contradiction();
        let state = replay_from_adapter(&adapter).unwrap();

        let query = PreflightQuery {
            files: vec![],
            symbols: vec![],
            diff_context: None,
            branch: None,
        };

        // Two calls on the same state must return identical contradiction context.
        let resp1 = query_preflight(&state, &query);
        let resp2 = query_preflight(&state, &query);

        for (d1, d2) in resp1
            .relevant_decisions
            .iter()
            .zip(resp2.relevant_decisions.iter())
        {
            assert_eq!(d1.active_contradictions, d2.active_contradictions);
        }
    }

    #[test]
    fn preflight_approved_signoff_unchanged_by_contradiction() {
        let (adapter, decision1_id, decision2_id, _c1, _c2, _report_id) = seed_with_contradiction();
        let state = replay_from_adapter(&adapter).unwrap();

        // Both decisions remain Approved despite active contradiction.
        let d1 = state.decisions.get(&decision1_id).unwrap();
        let d2 = state.decisions.get(&decision2_id).unwrap();
        assert_eq!(
            d1.signoff_state,
            bicameral_api::dashboard::SignoffState::Approved
        );
        assert_eq!(
            d2.signoff_state,
            bicameral_api::dashboard::SignoffState::Approved
        );
    }
}
