//! Conformance for semantic recall rendering on the runtime surfaces
//! (`search.query` and `preflight.run`) — issue #640.
//!
//! The daemon-authored [`SemanticRecallView`] read-model is rendered verbatim
//! by these surfaces. This suite proves the view rides on each surface
//! additively (it never changes canonical search results or preflight
//! readiness/decisions), keeps graph-verified targets and approximate hints in
//! separate collections, and stays a quiet, omitted no-op when no semantic
//! provider is configured.
//!
//! All tests are deterministic — no network, no hosted graph, no credentials.

use bicameral_api::fixtures::{all_semantic_recall_fixtures, locator_result_ready_verified};
use bicameral_runtime::preflight::{query_preflight, PreflightQuery};
use bicameral_runtime::replay::ReplayedState;
use bicameral_runtime::search::{
    enrich_search_with_locator, enrich_search_with_semantic_recall, query_search, SearchQuery,
    SearchScope,
};

fn searched_scope_view() -> bicameral_api::semantic_recall::SemanticRecallView {
    all_semantic_recall_fixtures()
        .into_iter()
        .find(|(n, _)| *n == "semantic-recall-searched-scope")
        .map(|(_, v)| v)
        .expect("searched-scope fixture present")
}

fn empty_state() -> ReplayedState {
    ReplayedState::default()
}

fn search_query() -> SearchQuery {
    SearchQuery {
        query: "rate limiting".to_string(),
        scope: SearchScope::All,
        filters: Default::default(),
        limit: 50,
    }
}

/// search.query: attaching the semantic recall view is additive — it does not
/// change the canonical search results — and renders verified targets and
/// approximate hints in separate collections.
#[test]
fn search_surface_renders_view_additively_and_distinctly() {
    let state = empty_state();
    let base = query_search(&state, &search_query());
    let base_result_count = base.results.len();

    let locator = locator_result_ready_verified();
    let view = searched_scope_view();
    let enriched = enrich_search_with_semantic_recall(
        query_search(&state, &search_query()),
        Some(&locator),
        Some(view.clone()),
    );

    assert_eq!(
        enriched.results.len(),
        base_result_count,
        "attaching semantic recall must not change canonical search results"
    );
    let code_context = enriched
        .code_context
        .expect("code context present when locator result is supplied");
    let sem = code_context
        .semantic_recall
        .as_ref()
        .expect("semantic recall view attached");
    assert_eq!(sem.verified_targets.len(), view.verified_targets.len());
    assert_eq!(sem.approximate_hints.len(), view.approximate_hints.len());
    assert!(!sem.verified_targets.is_empty());
    assert!(!sem.approximate_hints.is_empty());

    // The view serializes within the surface under its own key, distinct from
    // the locator `candidates` list.
    let json = serde_json::to_value(&code_context).unwrap();
    let rendered = json
        .get("semantic_recall")
        .expect("semantic_recall key rendered");
    assert!(rendered.get("verified_targets").is_some());
    assert!(rendered.get("approximate_hints").is_some());
}

/// search.query: with no semantic provider the view is absent and the field is
/// omitted entirely from the wire form (quiet, deterministic no-op).
#[test]
fn search_surface_quiet_when_no_provider() {
    let state = empty_state();
    let locator = locator_result_ready_verified();
    let enriched =
        enrich_search_with_locator(query_search(&state, &search_query()), Some(&locator));
    let code_context = enriched.code_context.expect("code context present");
    assert!(code_context.semantic_recall.is_none());
    let json = serde_json::to_value(&code_context).unwrap();
    assert!(
        json.get("semantic_recall").is_none(),
        "absent semantic recall must be omitted from the wire form"
    );
}

/// preflight.run: attaching the semantic recall view is additive — it does not
/// change readiness, relevant decisions, candidates, or warnings.
#[test]
fn preflight_surface_renders_view_additively() {
    let state = empty_state();
    let query = PreflightQuery {
        files: vec!["crates/bicameral-api/src/recall.rs".to_string()],
        symbols: vec![],
        diff_context: None,
        branch: None,
    };
    let base = query_preflight(&state, &query);
    assert!(base.semantic_recall.is_none());
    let quiet = serde_json::to_value(&base).unwrap();
    assert!(
        quiet.get("semantic_recall").is_none(),
        "quiet no-op when no provider configured"
    );

    let view = searched_scope_view();
    let base_readiness = serde_json::to_value(&base.readiness).unwrap();
    let base_decisions = base.relevant_decisions.len();
    let base_candidates = base.relevant_candidates.len();
    let base_warnings = base.warnings.clone();

    let with = query_preflight(&state, &query).with_semantic_recall(view.clone());
    assert_eq!(
        serde_json::to_value(&with.readiness).unwrap(),
        base_readiness,
        "semantic recall must not change preflight readiness"
    );
    assert_eq!(with.relevant_decisions.len(), base_decisions);
    assert_eq!(with.relevant_candidates.len(), base_candidates);
    assert_eq!(with.warnings, base_warnings);

    let sem = with.semantic_recall.as_ref().expect("view attached");
    assert_eq!(*sem, view);
    let json = serde_json::to_value(&with).unwrap();
    assert!(json.get("semantic_recall").is_some());
}
