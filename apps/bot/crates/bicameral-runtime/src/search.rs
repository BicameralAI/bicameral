//! Search query surface — deterministic text search over materialized ledger.
//!
//! Implements the source-only `bicameral.search` contract from MCP v0.2 spec
//! §2.2. Searches decisions, candidates, and source evidence excerpts using
//! simple case-insensitive substring matching. No external index infrastructure.

use crate::replay::ReplayedState;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Search query input aligned with spec §2.2 `bicameral.search`.
#[derive(Debug, Clone, Deserialize)]
pub struct SearchQuery {
    pub query: String,
    #[serde(default = "default_scope")]
    pub scope: SearchScope,
    #[serde(default)]
    pub filters: SearchFilters,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_scope() -> SearchScope {
    SearchScope::All
}

fn default_limit() -> usize {
    20
}

/// Scope of the search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchScope {
    Decisions,
    Candidates,
    All,
    /// Binding search scope — reads accepted, bot-owned `BindingEvidence`
    /// materialized in replayed state. Results are read-only recorded
    /// evidence; they never assert verified project truth, compliance,
    /// signoff, no-conflict, or merge safety beyond the recorded evidence.
    Bindings,
}

/// Optional search filters.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SearchFilters {
    #[serde(default)]
    pub level: Option<bicameral_api::candidate::DecisionLevel>,
    #[serde(default)]
    pub signoff_state: Option<bicameral_api::dashboard::SignoffState>,
    #[serde(default)]
    pub compliance_state: Option<bicameral_api::dashboard::ComplianceState>,
}

/// A single search result.
#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    #[serde(rename = "type")]
    pub result_type: SearchResultType,
    pub id: String,
    pub title: String,
    pub snippet: String,
    pub relevance_score: f64,
    /// Source identity (URI) for the evidence backing this result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    /// Snapshot identity anchoring the source evidence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_snapshot_id: Option<String>,
    /// IDs of typed `EvidenceReference` items cited by this result.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_reference_ids: Vec<Uuid>,
    /// Stable human-facing deep-link for inspecting the evidence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inspection_uri: Option<String>,
    /// Informational provenance metadata — which surface/actor submitted this.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<bicameral_api::evidence::SubmissionProvenance>,
}

/// The type of entity found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchResultType {
    Decision,
    Candidate,
    SourceExcerpt,
}

/// A single accepted `BindingEvidence` search result.
///
/// Binding results are read-only recorded evidence over bot-owned,
/// replayed/materialized state. They carry the identity anchors and evidence
/// state recorded at bind time. They are **not** a live re-verification of the
/// code graph and must never be read as verified project truth, compliance,
/// signoff, no-conflict, or merge safety beyond the recorded evidence. Consult
/// `evidence_state`, `readiness_warnings`, and `caveats` before drawing any
/// conclusion.
#[derive(Debug, Clone, Serialize)]
pub struct BindingSearchResult {
    /// Identity of the accepted `BindingEvidence`.
    pub binding_id: String,
    /// Decision this binding is linked to (the replayed grouping key).
    pub decision_id: String,
    /// Candidate the binding evidence originated from.
    pub candidate_id: String,
    /// Binding target string (e.g. `src/main.rs:1-20`).
    pub target: String,
    /// Classification of the binding target (e.g. `file`, `symbol`, `code_region`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_kind: Option<String>,
    /// Canonical identifier for the binding target within the snapshot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_identifier: Option<String>,
    /// Free-form target metadata recorded with the binding evidence. Read-only
    /// recorded data; omitted when no metadata was recorded.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub target_metadata: serde_json::Value,
    /// The typed binding kind (`file_path`, `symbol`, `dependency`, ...).
    pub binding_type: bicameral_api::evidence::BindingType,
    /// Snapshot identity at which the binding was validated (durable anchor).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_snapshot_id: Option<String>,
    /// Commit SHA at which the binding was validated (canonical durable key).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validated_sha: Option<String>,
    /// Provenance ref name at which the binding was resolved (informational).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
    /// Graph evidence state recorded at bind time. Only `Verified` reflects a
    /// proven graph fact; all other states are review-only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_state: Option<bicameral_api::evidence::GraphEvidenceState>,
    /// Human-facing explanation recorded with the binding, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
    /// Readiness/freshness warnings recorded during binding validation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub readiness_warnings: Vec<String>,
    /// Whether the binding scope for this result is unknown or partial — set
    /// when the recorded evidence is not `Verified` or lacks a durable
    /// snapshot/commit anchor.
    pub unknown_or_partial_scope: bool,
    /// Advisory caveats. Always present to prevent over-claiming.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caveats: Vec<String>,
    /// Relevance score for ordering (approximate; lexical match only).
    pub relevance_score: f64,
    /// Stable human-facing deep-link for inspecting the binding evidence.
    pub inspection_uri: String,
}

/// Code-aware context from CodeLocator, included when workspace is available.
///
/// Advisory only — does not assert compliance, safety, no-conflict, or
/// blocking status. Consumers must respect `graph_readiness` and per-candidate
/// `evidence_state` caveats.
#[derive(Debug, Clone, Serialize)]
pub struct SearchCodeContext {
    /// Graph readiness at the time of locator invocation.
    pub graph_readiness: bicameral_api::tool_request::LocatorGraphReadiness,
    /// Locator candidates relevant to the search query.
    pub candidates: Vec<bicameral_api::tool_request::LocatorCandidate>,
    /// Readiness/evidence caveats for the code context.
    pub caveats: Vec<String>,
    /// Daemon-authored semantic recall contribution (issue #640). Renders
    /// graph-verified semantic targets and approximate hints (kept distinct)
    /// plus caveats/next actions. Advisory only — never asserts compliance,
    /// safety, no-conflict, or blocking status. Absent (`None`) when no
    /// semantic provider is configured, so the no-op stays quiet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_recall: Option<bicameral_api::semantic_recall::SemanticRecallView>,
}

/// Search response.
#[derive(Debug, Clone, Serialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    /// Accepted, bot-owned `BindingEvidence` results. Present when the
    /// `bindings` (or `all`) scope is queried. Read-only recorded evidence —
    /// never verified project truth, compliance, signoff, no-conflict, or
    /// merge safety beyond the recorded evidence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<BindingSearchResult>,
    /// When a scope is requested but not yet supported, this field describes
    /// the unsupported scope and reason.
    ///
    /// This is retained for general daemon version-negotiation handling only.
    /// The `bindings` scope is supported and never returns this.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsupported_scope: Option<UnsupportedScope>,
    /// Read-only advisory notes about the scope of the results. Used to make
    /// binding-scope responses explicit (never silently empty) and to surface
    /// unknown/partial-scope conditions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scope_notes: Vec<String>,
    /// Optional code-aware context from CodeLocator. Present when a workspace
    /// is available and the locator produced relevant candidates.
    /// All candidates carry evidence/readiness caveats — they are discovery
    /// aids, not compliance or safety assertions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_context: Option<SearchCodeContext>,
}

/// Typed indicator that a requested search scope is not yet available.
///
/// Retained for general daemon version-negotiation unsupported-capability
/// handling. It is not used for the `bindings` scope, which is supported.
#[derive(Debug, Clone, Serialize)]
pub struct UnsupportedScope {
    pub scope: String,
    pub reason: String,
}

/// Read-only advisory boundary applied to every binding-scope response so the
/// results are never read as verified project truth or merge safety.
const BINDING_SCOPE_BOUNDARY_NOTE: &str =
    "Binding search returns recorded, bot-owned accepted BindingEvidence only. \
     Results are read-only and do not imply complete project truth, compliance, \
     signoff, no-conflict, verified binding state beyond the recorded evidence, \
     or merge safety.";

/// Execute a search over replayed state.
pub fn query_search(state: &ReplayedState, query: &SearchQuery) -> SearchResponse {
    let q = query.query.to_lowercase();
    let mut results = Vec::new();

    if matches!(query.scope, SearchScope::All | SearchScope::Decisions) {
        for decision in state.decisions.values() {
            if let Some(level_filter) = &query.filters.level {
                if decision.level != *level_filter {
                    continue;
                }
            }
            if let Some(signoff_filter) = &query.filters.signoff_state {
                if decision.signoff_state != *signoff_filter {
                    continue;
                }
            }
            if let Some(compliance_filter) = &query.filters.compliance_state {
                if decision.compliance_state != *compliance_filter {
                    continue;
                }
            }

            let title_lower = decision.title.to_lowercase();
            let body_lower = decision.body.to_lowercase();

            let title_match = title_lower.contains(&q);
            let body_match = body_lower.contains(&q);

            if title_match || body_match {
                let score = if title_match { 1.0 } else { 0.7 };
                let snippet = if title_match {
                    decision.title.clone()
                } else {
                    extract_snippet(&decision.body, &q)
                };
                let source_snapshot_id = resolve_decision_snapshot_id(decision, state);
                let inspection_uri = decision
                    .source_id
                    .as_ref()
                    .map(|sid| format!("bicameral://evidence/{}/{}", decision.id, sid));
                results.push(SearchResult {
                    result_type: SearchResultType::Decision,
                    id: decision.id.to_string(),
                    title: decision.title.clone(),
                    snippet,
                    relevance_score: score,
                    source_id: decision.source_id.clone(),
                    source_snapshot_id,
                    evidence_reference_ids: decision.evidence_reference_ids.clone(),
                    inspection_uri,
                    provenance: decision.provenance.clone(),
                });
            }
        }
    }

    if matches!(query.scope, SearchScope::All | SearchScope::Candidates) {
        for candidate in state.candidates.values() {
            if let Some(level_filter) = &query.filters.level {
                if candidate.level != *level_filter {
                    continue;
                }
            }

            let title_lower = candidate.title.to_lowercase();
            let desc_lower = candidate.description.to_lowercase();

            let title_match = title_lower.contains(&q);
            let desc_match = desc_lower.contains(&q);

            if title_match || desc_match {
                let score = if title_match { 0.9 } else { 0.6 };
                let snippet = if title_match {
                    candidate.title.clone()
                } else {
                    extract_snippet(&candidate.description, &q)
                };
                let source_snapshot_id = resolve_candidate_snapshot_id(candidate, state);
                let inspection_uri = Some(format!(
                    "bicameral://evidence/candidate/{}/{}",
                    candidate.id, candidate.source
                ));
                results.push(SearchResult {
                    result_type: SearchResultType::Candidate,
                    id: candidate.id.to_string(),
                    title: candidate.title.clone(),
                    snippet,
                    relevance_score: score,
                    source_id: Some(candidate.source.clone()),
                    source_snapshot_id,
                    evidence_reference_ids: candidate.evidence_reference_ids.clone(),
                    inspection_uri,
                    provenance: candidate.provenance.clone(),
                });
            }
        }

        // Search source evidence excerpts
        for evidence in state.evidence.values() {
            let excerpt_lower = evidence.excerpt.to_lowercase();
            if excerpt_lower.contains(&q) {
                results.push(SearchResult {
                    result_type: SearchResultType::SourceExcerpt,
                    id: evidence.id.to_string(),
                    title: format!("Evidence from {}", evidence.snapshot_id),
                    snippet: evidence.excerpt.clone(),
                    relevance_score: 0.5,
                    source_id: None,
                    source_snapshot_id: Some(evidence.snapshot_id.clone()),
                    evidence_reference_ids: Vec::new(),
                    inspection_uri: Some(format!("bicameral://evidence/source/{}", evidence.id)),
                    provenance: None,
                });
            }
        }
    }

    let mut bindings = Vec::new();
    let mut scope_notes = Vec::new();

    if matches!(query.scope, SearchScope::All | SearchScope::Bindings) {
        // Binding search reads accepted, bot-owned BindingEvidence materialized
        // in replayed state (keyed by decision id). It never reads MCP-owned
        // ledger/search state and never re-verifies the graph live.
        for (decision_id, decision_bindings) in &state.bindings {
            for binding in decision_bindings {
                if let Some(result) = binding_search_result(*decision_id, binding, &q) {
                    bindings.push(result);
                }
            }
        }

        bindings.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        bindings.truncate(query.limit);

        // Binding-scope responses are always explicit: attach the read-only
        // boundary note so an empty result is never silently interpreted as
        // "no bindings exist" or as an unsupported capability.
        scope_notes.push(BINDING_SCOPE_BOUNDARY_NOTE.to_string());

        let partial = bindings
            .iter()
            .filter(|b| b.unknown_or_partial_scope)
            .count();
        if partial > 0 {
            scope_notes.push(format!(
                "{partial} binding result(s) have unknown or partial graph evidence scope \
                 (not Verified, or missing snapshot/commit anchor). Treat those as \
                 review-only, not verified graph evidence."
            ));
        }
    }

    results.sort_by(|a, b| {
        b.relevance_score
            .partial_cmp(&a.relevance_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(query.limit);

    SearchResponse {
        results,
        bindings,
        unsupported_scope: None,
        scope_notes,
        code_context: None,
    }
}

/// Build a [`BindingSearchResult`] for a recorded binding when it matches the
/// (already-lowercased) query. Matching is lexical over the binding's own
/// identity and target metadata — approximate text search, never a verified
/// graph re-check.
fn binding_search_result(
    decision_id: Uuid,
    binding: &bicameral_api::evidence::BindingEvidence,
    q: &str,
) -> Option<BindingSearchResult> {
    let binding_id = binding.id.to_string();
    let decision_id_str = decision_id.to_string();
    let binding_type_str = format!("{:?}", binding.binding_type).to_lowercase();

    // Candidate haystacks paired with a relevance weight for ordering.
    let target_lower = binding.target.to_lowercase();
    let target_id_lower = binding
        .target_identifier
        .as_deref()
        .unwrap_or("")
        .to_lowercase();
    let target_kind_lower = binding.target_kind.as_deref().unwrap_or("").to_lowercase();
    let explanation_lower = binding.explanation.as_deref().unwrap_or("").to_lowercase();

    let mut score = 0.0_f64;
    if target_lower.contains(q) {
        score = score.max(0.85);
    }
    if target_id_lower.contains(q) {
        score = score.max(0.8);
    }
    if binding_id.to_lowercase().contains(q) || decision_id_str.contains(q) {
        score = score.max(0.75);
    }
    if target_kind_lower.contains(q) || binding_type_str.contains(q) {
        score = score.max(0.65);
    }
    if explanation_lower.contains(q) {
        score = score.max(0.55);
    }
    // An empty query matches everything (parity with decision/candidate search).
    if q.is_empty() {
        score = score.max(0.5);
    }

    if score == 0.0 {
        return None;
    }

    let verified = binding
        .evidence_state
        .map(|s| s.allows_binding())
        .unwrap_or(false);
    let anchored = binding.graph_snapshot_id.is_some() && binding.validated_sha.is_some();
    let unknown_or_partial_scope = !verified || !anchored;

    let mut caveats = vec![BINDING_SCOPE_BOUNDARY_NOTE.to_string()];
    if !verified {
        caveats.push(
            "Recorded evidence_state is not Verified — this is review-only graph evidence, \
             not a verified graph fact."
                .to_string(),
        );
    }
    if !anchored {
        caveats.push(
            "Binding lacks a durable snapshot/commit anchor — scope is partial or unknown."
                .to_string(),
        );
    }

    Some(BindingSearchResult {
        binding_id,
        decision_id: decision_id_str,
        candidate_id: binding.candidate_id.to_string(),
        target: binding.target.clone(),
        target_kind: binding.target_kind.clone(),
        target_identifier: binding.target_identifier.clone(),
        target_metadata: binding.metadata.clone(),
        binding_type: binding.binding_type,
        graph_snapshot_id: binding.graph_snapshot_id.clone(),
        validated_sha: binding.validated_sha.clone(),
        ref_name: binding.ref_name.clone(),
        evidence_state: binding.evidence_state,
        explanation: binding.explanation.clone(),
        readiness_warnings: binding.readiness_warnings.clone(),
        unknown_or_partial_scope,
        caveats,
        relevance_score: score,
        inspection_uri: format!("bicameral://binding/{decision_id}/{}", binding.id),
    })
}

/// Enrich a `SearchResponse` with optional CodeLocator context.
///
/// When `locator_result` is `Some`, builds code context with explicit
/// evidence/readiness caveats. The code context is purely advisory —
/// it does not assert compliance, safety, no-conflict, or blocking status.
pub fn enrich_search_with_locator(
    mut response: SearchResponse,
    locator_result: Option<&bicameral_api::tool_request::LocatorResult>,
) -> SearchResponse {
    response.code_context = locator_result.map(|lr| {
        use bicameral_api::tool_request::{LocatorEvidenceState, LocatorGraphReadiness};

        let mut caveats = Vec::new();

        match lr.graph_readiness {
            LocatorGraphReadiness::Ready => {}
            LocatorGraphReadiness::Stale => {
                caveats.push(
                    "Graph data is stale. Code context may not reflect current state. \
                     Do not interpret results as no-conflict or safe."
                        .to_string(),
                );
            }
            LocatorGraphReadiness::Indexing => {
                caveats.push("Graph is indexing. Code context is incomplete.".to_string());
            }
            LocatorGraphReadiness::NotConfigured => {
                caveats.push("Graph not configured. No code-aware context available.".to_string());
            }
            LocatorGraphReadiness::Failed => {
                caveats.push(
                    "Graph resolution failed. Code context unavailable. \
                     Do not interpret this as no-conflict or compliant."
                        .to_string(),
                );
            }
        }

        let has_non_verified = lr
            .candidates
            .iter()
            .any(|c| !matches!(c.evidence_state, LocatorEvidenceState::Verified));
        if has_non_verified {
            caveats.push(
                "Some code candidates are not verified — treat as advisory discovery only."
                    .to_string(),
            );
        }

        if !lr.candidates.is_empty() {
            caveats.push(
                "Code context candidates are advisory. They do not assert compliance, \
                 safety, no-conflict, or blocking status."
                    .to_string(),
            );
        }

        SearchCodeContext {
            graph_readiness: lr.graph_readiness,
            candidates: lr.candidates.clone(),
            caveats,
            semantic_recall: None,
        }
    });
    response
}

/// Enrich a `SearchResponse` with CodeLocator context and a daemon-authored
/// semantic recall view.
///
/// Builds the locator code context exactly as [`enrich_search_with_locator`],
/// then attaches the evidence-only [`SemanticRecallView`] so the search surface
/// renders graph-verified semantic targets and approximate hints distinctly.
/// When `semantic_view` is `None` the code context is unchanged and quiet.
pub fn enrich_search_with_semantic_recall(
    response: SearchResponse,
    locator_result: Option<&bicameral_api::tool_request::LocatorResult>,
    semantic_view: Option<bicameral_api::semantic_recall::SemanticRecallView>,
) -> SearchResponse {
    let mut response = enrich_search_with_locator(response, locator_result);
    if let (Some(ctx), Some(view)) = (response.code_context.as_mut(), semantic_view) {
        ctx.semantic_recall = Some(view);
    }
    response
}

fn resolve_decision_snapshot_id(
    d: &bicameral_api::candidate::Decision,
    state: &ReplayedState,
) -> Option<String> {
    for ev_id in &d.evidence_refs {
        if let Some(ev) = state.evidence.get(ev_id) {
            return Some(ev.snapshot_id.clone());
        }
    }
    for er_id in &d.evidence_reference_ids {
        if let Some(er) = state.evidence_references.get(er_id) {
            return Some(er.snapshot_id.clone());
        }
    }
    None
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

fn extract_snippet(text: &str, query: &str) -> String {
    let lower = text.to_lowercase();
    if let Some(pos) = lower.find(query) {
        let start = pos.saturating_sub(40);
        let end = (pos + query.len() + 40).min(text.len());
        let slice = &text[start..end];
        if start > 0 || end < text.len() {
            format!("...{}...", slice.trim())
        } else {
            slice.to_string()
        }
    } else {
        text.chars().take(80).collect()
    }
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

    fn seed_adapter() -> MemoryAdapter {
        let adapter = MemoryAdapter::new();

        for (uri, title, desc, excerpt) in [
            (
                "jira://PROJ-1",
                "Use microservices",
                "Architecture should use microservices pattern",
                "The system shall use microservices",
            ),
            (
                "slack://msg-1",
                "API rate limits",
                "We need rate limiting on all public APIs",
                "rate limiting is essential for public endpoints",
            ),
            (
                "github://pr-1",
                "Database migration",
                "Switch from SQLite to PostgreSQL for production",
                "PostgreSQL required for scaling",
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
                excerpt.len(),
                excerpt.to_string(),
                0.9,
            )];
            let mut candidate = DecisionCandidate::new(
                title.to_string(),
                desc.to_string(),
                DecisionLevel::Architecture,
                uri.to_string(),
            );
            candidate.evidence_refs = evidence.iter().map(|e| e.id).collect();

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
        }
        adapter
    }

    #[test]
    fn search_finds_candidates_by_title() {
        let adapter = seed_adapter();
        let state = replay_from_adapter(&adapter).unwrap();

        let resp = query_search(
            &state,
            &SearchQuery {
                query: "microservices".to_string(),
                scope: SearchScope::Candidates,
                filters: SearchFilters::default(),
                limit: 20,
            },
        );

        assert!(!resp.results.is_empty());
        assert_eq!(resp.results[0].result_type, SearchResultType::Candidate);
        assert!(resp.results[0].title.contains("microservices"));
    }

    #[test]
    fn search_finds_source_excerpts() {
        let adapter = seed_adapter();
        let state = replay_from_adapter(&adapter).unwrap();

        let resp = query_search(
            &state,
            &SearchQuery {
                query: "PostgreSQL".to_string(),
                scope: SearchScope::All,
                filters: SearchFilters::default(),
                limit: 20,
            },
        );

        let has_excerpt = resp
            .results
            .iter()
            .any(|r| r.result_type == SearchResultType::SourceExcerpt);
        assert!(has_excerpt);
    }

    #[test]
    fn search_respects_limit() {
        let adapter = seed_adapter();
        let state = replay_from_adapter(&adapter).unwrap();

        let resp = query_search(
            &state,
            &SearchQuery {
                query: "".to_string(),
                scope: SearchScope::All,
                filters: SearchFilters::default(),
                limit: 1,
            },
        );

        assert!(resp.results.len() <= 1);
    }

    #[test]
    fn search_finds_decisions_after_promotion() {
        let adapter = seed_adapter();

        // Promote first candidate
        let state_before = replay_from_adapter(&adapter).unwrap();
        let first_cand = state_before.candidates.values().next().unwrap();
        let cand_id = first_cand.id;
        let decision_id = uuid::Uuid::new_v4();

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

        let resp = query_search(
            &state,
            &SearchQuery {
                query: first_cand.title.to_lowercase(),
                scope: SearchScope::Decisions,
                filters: SearchFilters::default(),
                limit: 20,
            },
        );

        assert!(!resp.results.is_empty());
        assert_eq!(resp.results[0].result_type, SearchResultType::Decision);
    }

    #[test]
    fn search_case_insensitive() {
        let adapter = seed_adapter();
        let state = replay_from_adapter(&adapter).unwrap();

        let resp = query_search(
            &state,
            &SearchQuery {
                query: "RATE LIMITS".to_string(),
                scope: SearchScope::Candidates,
                filters: SearchFilters::default(),
                limit: 20,
            },
        );

        assert!(!resp.results.is_empty());
    }

    #[test]
    fn search_results_include_source_evidence_links() {
        let adapter = seed_adapter();
        let state = replay_from_adapter(&adapter).unwrap();

        let resp = query_search(
            &state,
            &SearchQuery {
                query: "microservices".to_string(),
                scope: SearchScope::Candidates,
                filters: SearchFilters::default(),
                limit: 20,
            },
        );

        assert!(!resp.results.is_empty());
        let r = &resp.results[0];
        assert_eq!(r.source_id.as_deref(), Some("jira://PROJ-1"));
        assert_eq!(
            r.source_snapshot_id.as_deref(),
            Some("inline:jira://PROJ-1")
        );
        assert!(r.inspection_uri.is_some());
        assert!(r.inspection_uri.as_ref().unwrap().contains("candidate"));
    }

    #[test]
    fn search_source_excerpt_includes_snapshot_id() {
        let adapter = seed_adapter();
        let state = replay_from_adapter(&adapter).unwrap();

        let resp = query_search(
            &state,
            &SearchQuery {
                query: "PostgreSQL".to_string(),
                scope: SearchScope::All,
                filters: SearchFilters::default(),
                limit: 20,
            },
        );

        let excerpt_result = resp
            .results
            .iter()
            .find(|r| r.result_type == SearchResultType::SourceExcerpt)
            .expect("should find source excerpt");
        assert!(excerpt_result.source_snapshot_id.is_some());
        assert!(excerpt_result.inspection_uri.is_some());
        assert!(excerpt_result
            .inspection_uri
            .as_ref()
            .unwrap()
            .starts_with("bicameral://evidence/source/"));
    }

    #[test]
    fn search_decision_includes_source_evidence_links() {
        let adapter = seed_adapter();

        // Promote first candidate
        let state_before = replay_from_adapter(&adapter).unwrap();
        let first_cand = state_before.candidates.values().next().unwrap();
        let cand_id = first_cand.id;
        let decision_id = uuid::Uuid::new_v4();

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

        let resp = query_search(
            &state,
            &SearchQuery {
                query: first_cand.title.to_lowercase(),
                scope: SearchScope::Decisions,
                filters: SearchFilters::default(),
                limit: 20,
            },
        );

        assert!(!resp.results.is_empty());
        let r = &resp.results[0];
        assert_eq!(r.result_type, SearchResultType::Decision);
        assert!(r.source_id.is_some());
        assert!(r.source_snapshot_id.is_some());
        assert!(r.inspection_uri.is_some());
    }

    /// Insert an accepted, verified, fully-anchored binding into replayed
    /// state under `decision_id`.
    fn seed_verified_binding(state: &mut ReplayedState, decision_id: Uuid, target: &str) -> Uuid {
        use bicameral_api::evidence::{
            BindingConfidence, BindingEvidence, BindingType, GraphEvidenceState,
        };
        let mut binding = BindingEvidence::new(
            Uuid::new_v4(),
            BindingType::FilePath,
            target.to_string(),
            BindingConfidence::High,
        );
        binding.graph_snapshot_id = Some("snap-abc123".to_string());
        binding.validated_sha = Some("deadbeefcafe".to_string());
        binding.ref_name = Some("refs/heads/dev".to_string());
        binding.target_kind = Some("file".to_string());
        binding.target_identifier = Some(target.to_string());
        binding.evidence_state = Some(GraphEvidenceState::Verified);
        binding.explanation = Some("Bound to the health check module".to_string());
        binding.metadata = serde_json::json!({ "language": "rust", "lines": "1-20" });
        let id = binding.id;
        state.bindings.entry(decision_id).or_default().push(binding);
        id
    }

    #[test]
    fn search_bindings_scope_returns_accepted_binding_evidence() {
        let adapter = seed_adapter();
        let mut state = replay_from_adapter(&adapter).unwrap();
        let decision_id = Uuid::new_v4();
        let binding_id = seed_verified_binding(&mut state, decision_id, "src/main.rs:1-20");

        let resp = query_search(
            &state,
            &SearchQuery {
                query: "src/main.rs".to_string(),
                scope: SearchScope::Bindings,
                filters: SearchFilters::default(),
                limit: 20,
            },
        );

        // Supported scope: no unsupported marker, and never a silent empty.
        assert!(resp.unsupported_scope.is_none());
        assert!(!resp.scope_notes.is_empty());
        assert_eq!(resp.bindings.len(), 1);

        let b = &resp.bindings[0];
        assert_eq!(b.binding_id, binding_id.to_string());
        assert_eq!(b.decision_id, decision_id.to_string());
        assert_eq!(b.target, "src/main.rs:1-20");
        assert_eq!(b.target_kind.as_deref(), Some("file"));
        assert_eq!(b.target_identifier.as_deref(), Some("src/main.rs:1-20"));
        assert_eq!(b.target_metadata["language"], "rust");
        assert_eq!(b.graph_snapshot_id.as_deref(), Some("snap-abc123"));
        assert_eq!(b.validated_sha.as_deref(), Some("deadbeefcafe"));
        assert_eq!(
            b.evidence_state,
            Some(bicameral_api::evidence::GraphEvidenceState::Verified)
        );
        // Verified + fully anchored ⇒ scope is neither unknown nor partial.
        assert!(!b.unknown_or_partial_scope);
        // Read-only boundary caveat is always present (never over-claims).
        assert!(b.caveats.iter().any(|c| c.contains("read-only")));
    }

    #[test]
    fn search_bindings_scope_is_never_silently_empty() {
        // No bindings materialized at all: the scope is still supported and
        // returns an explicit read-only scope note — never a silent empty or
        // an unsupported-capability response.
        let adapter = seed_adapter();
        let state = replay_from_adapter(&adapter).unwrap();

        let resp = query_search(
            &state,
            &SearchQuery {
                query: "anything".to_string(),
                scope: SearchScope::Bindings,
                filters: SearchFilters::default(),
                limit: 20,
            },
        );

        assert!(resp.unsupported_scope.is_none());
        assert!(resp.bindings.is_empty());
        assert!(
            !resp.scope_notes.is_empty(),
            "binding scope must always be explicit, never silently empty"
        );
    }

    #[test]
    fn search_bindings_flags_unknown_or_partial_scope() {
        use bicameral_api::evidence::{
            BindingConfidence, BindingEvidence, BindingType, GraphEvidenceState,
        };
        let adapter = seed_adapter();
        let mut state = replay_from_adapter(&adapter).unwrap();
        let decision_id = Uuid::new_v4();

        // Non-verified, unanchored binding: recorded evidence exists but must be
        // surfaced honestly as unknown/partial scope, not verified graph truth.
        let mut binding = BindingEvidence::new(
            Uuid::new_v4(),
            BindingType::Symbol,
            "auth::login".to_string(),
            BindingConfidence::Low,
        );
        binding.target_kind = Some("symbol".to_string());
        binding.evidence_state = Some(GraphEvidenceState::UnknownStale);
        binding.readiness_warnings = vec!["graph snapshot is stale".to_string()];
        state.bindings.entry(decision_id).or_default().push(binding);

        let resp = query_search(
            &state,
            &SearchQuery {
                query: "auth::login".to_string(),
                scope: SearchScope::Bindings,
                filters: SearchFilters::default(),
                limit: 20,
            },
        );

        assert_eq!(resp.bindings.len(), 1);
        let b = &resp.bindings[0];
        assert!(b.unknown_or_partial_scope);
        assert_eq!(b.evidence_state, Some(GraphEvidenceState::UnknownStale));
        assert!(!b.readiness_warnings.is_empty());
        assert!(b.caveats.iter().any(|c| c.contains("not Verified")));
        assert!(resp
            .scope_notes
            .iter()
            .any(|n| n.contains("unknown or partial")));
    }

    #[test]
    fn search_all_scope_includes_bindings_alongside_other_results() {
        let adapter = seed_adapter();
        let mut state = replay_from_adapter(&adapter).unwrap();
        let decision_id = Uuid::new_v4();
        seed_verified_binding(&mut state, decision_id, "microservices/gateway.rs");

        let resp = query_search(
            &state,
            &SearchQuery {
                query: "microservices".to_string(),
                scope: SearchScope::All,
                filters: SearchFilters::default(),
                limit: 20,
            },
        );

        // `all` scope returns candidate/source results AND binding results,
        // keeping approximate text search distinct from binding evidence.
        assert!(!resp.results.is_empty());
        assert_eq!(resp.bindings.len(), 1);
    }

    #[test]
    fn search_bindings_scope_does_not_mutate_state() {
        let adapter = seed_adapter();
        let mut state = replay_from_adapter(&adapter).unwrap();
        let decision_id = Uuid::new_v4();
        seed_verified_binding(&mut state, decision_id, "src/main.rs");
        let before = state.bindings.get(&decision_id).map(|v| v.len());

        let _ = query_search(
            &state,
            &SearchQuery {
                query: "src".to_string(),
                scope: SearchScope::Bindings,
                filters: SearchFilters::default(),
                limit: 20,
            },
        );

        let after = state.bindings.get(&decision_id).map(|v| v.len());
        assert_eq!(before, after);
    }

    #[test]
    fn search_no_results_returns_empty() {
        let adapter = seed_adapter();
        let state = replay_from_adapter(&adapter).unwrap();

        let resp = query_search(
            &state,
            &SearchQuery {
                query: "xyznonexistentquery".to_string(),
                scope: SearchScope::All,
                filters: SearchFilters::default(),
                limit: 20,
            },
        );

        assert!(resp.results.is_empty());
        assert!(resp.unsupported_scope.is_none());
    }

    // ── CodeLocator enrichment tests (issue #439) ─────────────────────────

    use bicameral_api::tool_request::{
        LocatorCandidate, LocatorConfidence, LocatorEvidenceState, LocatorGraphReadiness,
        LocatorReason, LocatorReasonCode, LocatorResult, LocatorSnapshotInfo, LocatorStatus,
        LocatorTargetType,
    };

    fn make_locator_result_for_search(
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

    fn make_search_candidate(evidence_state: LocatorEvidenceState) -> LocatorCandidate {
        LocatorCandidate {
            target: "src/test.rs".to_string(),
            target_type: LocatorTargetType::FilePath,
            file: "src/test.rs".to_string(),
            range: None,
            symbol: None,
            evidence_state,
            confidence: LocatorConfidence::Medium,
            reasons: vec![LocatorReason {
                code: LocatorReasonCode::LexicalMatch,
                detail: "query match".to_string(),
            }],
            graph_snapshot_id: Some("gs-test".to_string()),
            validated_sha: Some("abc123".to_string()),
            content_hash: None,
            caveats: vec![],
        }
    }

    #[test]
    fn search_enrich_with_locator_none_no_code_context() {
        let adapter = seed_adapter();
        let state = replay_from_adapter(&adapter).unwrap();
        let resp = query_search(
            &state,
            &SearchQuery {
                query: "microservices".to_string(),
                scope: SearchScope::All,
                filters: SearchFilters::default(),
                limit: 20,
            },
        );
        let enriched = enrich_search_with_locator(resp, None);
        assert!(enriched.code_context.is_none());
    }

    #[test]
    fn search_enrich_graph_unavailable_produces_caveats() {
        let adapter = seed_adapter();
        let state = replay_from_adapter(&adapter).unwrap();
        let resp = query_search(
            &state,
            &SearchQuery {
                query: "microservices".to_string(),
                scope: SearchScope::All,
                filters: SearchFilters::default(),
                limit: 20,
            },
        );
        let lr = make_locator_result_for_search(LocatorGraphReadiness::NotConfigured, vec![]);
        let enriched = enrich_search_with_locator(resp, Some(&lr));

        let ctx = enriched.code_context.unwrap();
        assert_eq!(ctx.graph_readiness, LocatorGraphReadiness::NotConfigured);
        assert!(ctx.candidates.is_empty());
        assert!(ctx.caveats.iter().any(|c| c.contains("not configured")));
    }

    #[test]
    fn search_enrich_stale_graph_no_false_safety() {
        let adapter = seed_adapter();
        let state = replay_from_adapter(&adapter).unwrap();
        let resp = query_search(
            &state,
            &SearchQuery {
                query: "microservices".to_string(),
                scope: SearchScope::All,
                filters: SearchFilters::default(),
                limit: 20,
            },
        );
        let candidate = make_search_candidate(LocatorEvidenceState::UnknownStale);
        let lr = make_locator_result_for_search(LocatorGraphReadiness::Stale, vec![candidate]);
        let enriched = enrich_search_with_locator(resp, Some(&lr));

        let ctx = enriched.code_context.unwrap();
        assert_eq!(ctx.graph_readiness, LocatorGraphReadiness::Stale);
        assert!(
            ctx.caveats
                .iter()
                .any(|c| c.contains("stale") && c.contains("no-conflict")),
            "Stale graph must warn against safety assumptions"
        );
    }

    #[test]
    fn search_enrich_verified_candidate_carries_advisory_caveat() {
        let adapter = seed_adapter();
        let state = replay_from_adapter(&adapter).unwrap();
        let resp = query_search(
            &state,
            &SearchQuery {
                query: "microservices".to_string(),
                scope: SearchScope::All,
                filters: SearchFilters::default(),
                limit: 20,
            },
        );
        let candidate = make_search_candidate(LocatorEvidenceState::Verified);
        let lr = make_locator_result_for_search(LocatorGraphReadiness::Ready, vec![candidate]);
        let enriched = enrich_search_with_locator(resp, Some(&lr));

        let ctx = enriched.code_context.unwrap();
        assert_eq!(ctx.candidates.len(), 1);
        assert_eq!(
            ctx.candidates[0].evidence_state,
            LocatorEvidenceState::Verified
        );
        assert!(
            ctx.caveats.iter().any(|c| c.contains("advisory")),
            "Verified candidates must still be labeled advisory"
        );
    }

    #[test]
    fn search_source_only_fallback_no_code_context() {
        // When no locator result is provided (source-only), code_context is None.
        let adapter = seed_adapter();
        let state = replay_from_adapter(&adapter).unwrap();
        let resp = query_search(
            &state,
            &SearchQuery {
                query: "rate".to_string(),
                scope: SearchScope::All,
                filters: SearchFilters::default(),
                limit: 20,
            },
        );
        assert!(resp.code_context.is_none());
        assert!(!resp.results.is_empty(), "Source-only search still works");
    }
}
