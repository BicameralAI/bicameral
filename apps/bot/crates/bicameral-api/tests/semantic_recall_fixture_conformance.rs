//! Deterministic conformance for the semantic recall contribution fixtures
//! (issue #640).
//!
//! `code.locate` already fuses semantic-provider candidates into its ranked
//! result, distinguishing graph-verified targets (`SemanticTarget`) from
//! approximate hints (`SemanticHint`). This suite proves that the daemon-
//! authored [`SemanticRecallView`] read-model — which every surface
//! (`search.query`, `decision.find_code_impact`, `preflight.run`, the
//! `RecallPacket`/projection, and the dashboard) renders verbatim — keeps those
//! two tiers distinct, surfaces the required scope/provider states, and never
//! implies authority it does not have.
//!
//! Each protocol JSON fixture under `protocol/schemas/v2/fixtures/` must equal
//! the matching constructor in `bicameral_api::fixtures`, so surfaces render a
//! byte-for-byte reproducible, daemon-authored contract — they never recompute
//! the view.
//!
//! Set `BIC_REGEN_FIXTURES=1` to (re)write the golden JSON files from the Rust
//! fixtures instead of asserting against them.

use std::path::PathBuf;

use bicameral_api::dashboard::{
    CandidateReviewSection, CodeEvidenceDetailSection, ComplianceDeferredSection, ComplianceState,
    DecisionDetail, SignoffSection, SignoffState,
};
use bicameral_api::fixtures::all_semantic_recall_fixtures;
use bicameral_api::semantic_recall::{
    SemanticEvidenceTier, SemanticRecallCaveatCode, SemanticRecallNextAction, SemanticRecallView,
};
use bicameral_api::tool_request::FindCodeImpactResult;
use bicameral_api::{RecallActionKind, RecallPacket, RecallProviderStatus};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocol/schemas/v2/fixtures")
        .canonicalize()
        .expect("fixtures dir exists")
}

/// The fixture file stores the view under a `semantic_recall` key, plus
/// human-facing `_comment`/`_expected` metadata that is ignored on parse.
fn read_fixture_block(name: &str) -> SemanticRecallView {
    let path = fixtures_dir().join(format!("{name}.json"));
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{name}: read {}: {e}", path.display()));
    let doc: serde_json::Value =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{name}: parse failed: {e}"));
    let block = doc
        .get("semantic_recall")
        .unwrap_or_else(|| panic!("{name}: missing `semantic_recall` key"));
    serde_json::from_value(block.clone())
        .unwrap_or_else(|e| panic!("{name}: semantic_recall block does not deserialize: {e}"))
}

fn write_fixture(name: &str, view: &SemanticRecallView) {
    let path = fixtures_dir().join(format!("{name}.json"));
    let doc = serde_json::json!({
        "_comment": format!("Semantic recall contribution read-model (issue #640): {name}. \
            Daemon-authored, evidence-only; surfaces render this verbatim. Approximate hints and \
            graph-verified targets are kept in separate collections and never collapsed."),
        "_expected": {
            "provider_status": view.provider_status,
            "verified_target_count": view.verified_targets.len(),
            "approximate_hint_count": view.approximate_hints.len(),
            "no_result_in_searched_scope": view.no_result_in_searched_scope,
            "caveat_codes": view.caveats.iter().map(|c| c.code).collect::<Vec<_>>(),
            "next_actions": view.next_actions,
        },
        "semantic_recall": view,
    });
    let pretty = serde_json::to_string_pretty(&doc).expect("serialize");
    std::fs::write(&path, format!("{pretty}\n")).expect("write fixture");
}

#[test]
fn protocol_fixtures_match_rust_read_model() {
    let regen = std::env::var("BIC_REGEN_FIXTURES").is_ok();
    for (name, view) in all_semantic_recall_fixtures() {
        if regen {
            write_fixture(name, &view);
            continue;
        }
        let from_disk = read_fixture_block(name);
        assert_eq!(
            from_disk, view,
            "{name}: protocol fixture diverged from the Rust read-model"
        );
        // The fixture round-trips through JSON without loss.
        let json = serde_json::to_string(&view).unwrap();
        let reparsed: SemanticRecallView = serde_json::from_str(&json).unwrap();
        assert_eq!(
            reparsed, view,
            "{name}: read-model is not round-trip stable"
        );
    }
}

#[test]
fn all_required_acceptance_states_present() {
    let names: Vec<&str> = all_semantic_recall_fixtures()
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    for required in [
        "semantic-recall-searched-scope",
        "semantic-recall-unknown-scope",
        "semantic-recall-stale-index",
        "semantic-recall-unsupported-provider",
        "semantic-recall-no-result-in-searched-scope",
    ] {
        assert!(names.contains(&required), "missing fixture: {required}");
    }
}

/// The load-bearing distinctions of the read-model, asserted directly on the
/// fixtures so a regression in the mapping is caught here.
#[test]
fn approximate_and_verified_never_collapse() {
    let by_name: std::collections::HashMap<&str, SemanticRecallView> =
        all_semantic_recall_fixtures().into_iter().collect();

    // Searched scope: verified targets and approximate hints are present and
    // kept in separate collections, each tagged with its own tier.
    let searched = &by_name["semantic-recall-searched-scope"];
    assert_eq!(searched.verified_targets.len(), 1);
    assert_eq!(searched.approximate_hints.len(), 1);
    assert!(searched
        .verified_targets
        .iter()
        .all(|e| e.tier == SemanticEvidenceTier::GraphVerified));
    assert!(searched
        .approximate_hints
        .iter()
        .all(|e| e.tier == SemanticEvidenceTier::ApproximateHint));
    // Presence of an approximate hint always raises the advisory caveat and a
    // verify-with-graph-proof routing hint.
    assert!(searched
        .caveats
        .iter()
        .any(|c| c.code == SemanticRecallCaveatCode::ApproximateHintsPresent));
    assert!(searched
        .next_actions
        .contains(&SemanticRecallNextAction::VerifyWithGraphProof));
    assert!(!searched.no_result_in_searched_scope);

    // Unknown scope: a partial-coverage caveat is raised and the uncovered
    // scope is disclosed verbatim.
    let unknown = &by_name["semantic-recall-unknown-scope"];
    assert!(!unknown.unknown_scope.is_empty());
    assert!(unknown
        .caveats
        .iter()
        .any(|c| c.code == SemanticRecallCaveatCode::UnknownScope));

    // Stale index: any returned hint is approximate and a refresh action is
    // offered (never blocks).
    let stale = &by_name["semantic-recall-stale-index"];
    assert_eq!(stale.provider_status, RecallProviderStatus::Stale);
    assert!(stale
        .caveats
        .iter()
        .any(|c| c.code == SemanticRecallCaveatCode::StaleIndex));
    assert!(stale
        .next_actions
        .contains(&SemanticRecallNextAction::RefreshSemanticIndex));
    assert!(stale.verified_targets.is_empty());

    // Unsupported provider: an unsupported caveat is raised and there is no
    // verified or approximate contribution.
    let unsupported = &by_name["semantic-recall-unsupported-provider"];
    assert_eq!(
        unsupported.provider_status,
        RecallProviderStatus::Unsupported
    );
    assert!(unsupported
        .caveats
        .iter()
        .any(|c| c.code == SemanticRecallCaveatCode::ProviderUnsupported));
    assert!(unsupported.verified_targets.is_empty());
    assert!(unsupported.approximate_hints.is_empty());

    // No-result-in-searched-scope: empty result is scoped to the search, not an
    // all-clear, and carries the scoped-empty caveat.
    let no_result = &by_name["semantic-recall-no-result-in-searched-scope"];
    assert!(no_result.no_result_in_searched_scope);
    assert!(no_result.verified_targets.is_empty());
    assert!(no_result.approximate_hints.is_empty());
    assert!(no_result
        .caveats
        .iter()
        .any(|c| c.code == SemanticRecallCaveatCode::NoResultInSearchedScope));
}

/// Authority boundary: the read-model never serializes governance or authority
/// vocabulary. Semantic recall labels are evidence-only and advisory.
#[test]
fn fixtures_carry_no_authority_or_governance_vocabulary() {
    for (name, view) in all_semantic_recall_fixtures() {
        let json = serde_json::to_string(&view).unwrap().to_lowercase();
        for forbidden in [
            "complete recall",
            "no conflict",
            "no-conflict",
            "no_conflict",
            "compliance",
            "compliant",
            "signoff",
            "merge safe",
            "merge-safe",
            "merge_safe",
            "enforcement",
            "safe to proceed",
            "safe-to-proceed",
            "\"safe\"",
            "\"blocked\"",
        ] {
            assert!(
                !json.contains(forbidden),
                "{name}: semantic recall fixture must not contain '{forbidden}': {json}"
            );
        }
    }
}

/// Authority boundary: attaching a semantic recall view to a `RecallPacket` is
/// purely additive — it must not change the packet's allowed actions or any
/// other authority-bearing field. The view is a separate, derived read-model.
#[test]
fn recall_packet_semantic_recall_is_action_and_authority_neutral() {
    let base = RecallPacket::new_alpha();
    let base_actions: Vec<RecallActionKind> =
        base.allowed_next_actions.iter().map(|a| a.kind).collect();
    let base_canonical = base.canonical_decisions.len();
    let base_candidates = base.decision_candidates.len();

    for (_name, view) in all_semantic_recall_fixtures() {
        let with = RecallPacket::new_alpha().with_semantic_recall(view);
        let with_actions: Vec<RecallActionKind> =
            with.allowed_next_actions.iter().map(|a| a.kind).collect();
        assert_eq!(
            with_actions, base_actions,
            "attaching semantic recall must not change allowed_next_actions"
        );
        assert_eq!(with.canonical_decisions.len(), base_canonical);
        assert_eq!(with.decision_candidates.len(), base_candidates);
        assert!(with.semantic_recall.is_some());
    }
}

/// Quiet no-op: a `RecallPacket` with no semantic provider configured omits the
/// field entirely from the wire form.
#[test]
fn recall_packet_without_semantic_recall_is_quiet() {
    let base = RecallPacket::new_alpha();
    assert!(base.semantic_recall.is_none());
    let json = serde_json::to_value(&base).unwrap();
    assert!(
        json.get("semantic_recall").is_none(),
        "absent semantic recall must be omitted from the wire form"
    );
}

/// Code-impact surface (`decision.find_code_impact`): the view rides on the
/// result additively and the field is omitted entirely when absent (quiet
/// no-op), so the existing no-provider path stays deterministic.
#[test]
fn find_code_impact_surface_renders_view_additively() {
    let mut result: FindCodeImpactResult =
        bicameral_api::fixtures::find_code_impact_result_approximate();
    // No provider configured by default → quiet no-op (field omitted).
    assert!(result.semantic_recall.is_none());
    let quiet = serde_json::to_value(&result).unwrap();
    assert!(quiet.get("semantic_recall").is_none());

    // Attaching the view is additive and serializes within the surface.
    let (_, view) = all_semantic_recall_fixtures()
        .into_iter()
        .find(|(n, _)| *n == "semantic-recall-searched-scope")
        .unwrap();
    let candidates_before = result.candidates.len();
    result.semantic_recall = Some(view.clone());
    assert_eq!(
        result.candidates.len(),
        candidates_before,
        "attaching the view must not change canonical candidates"
    );
    let rendered = serde_json::to_value(&result).unwrap();
    assert!(rendered.get("semantic_recall").is_some());
    let reparsed: FindCodeImpactResult = serde_json::from_value(rendered).unwrap();
    assert_eq!(reparsed.semantic_recall, Some(view));
}

/// Dashboard surface (`DecisionDetail`): the view renders in a section
/// **separate** from `code_evidence` so approximate hints never collapse into
/// verified graph evidence, and the field is omitted entirely when absent.
#[test]
fn dashboard_decision_detail_renders_view_separate_from_code_evidence() {
    let mut detail = DecisionDetail {
        decision_id: "DEC-640".into(),
        candidate_id: "CAND-640".into(),
        title: "Render semantic recall caveats".into(),
        body: "Body".into(),
        level: bicameral_api::candidate::DecisionLevel::Implementation,
        promoted_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
        promoted_by: "tester".into(),
        tracking_state: bicameral_api::candidate::TrackingState::Tracked,
        candidate_review: CandidateReviewSection {
            review_state: bicameral_api::candidate::ReviewLifecycleState::Materialized,
            source_id: None,
            source_freshness: bicameral_api::source::SourceFreshness::Fresh,
            freshness_warning: None,
        },
        signoff: SignoffSection {
            state: SignoffState::Proposed,
            allowed_commands: vec![],
        },
        code_evidence: CodeEvidenceDetailSection {
            evidence_state: bicameral_api::evidence::GraphEvidenceState::UnknownNotIndexed,
            has_bindings: false,
            bindings: vec![],
        },
        compliance: ComplianceDeferredSection {
            state: ComplianceState::Ungrounded,
            deferred: true,
        },
        provenance: None,
        semantic_recall: None,
    };

    // Quiet no-op when no provider is configured.
    let quiet = serde_json::to_value(&detail).unwrap();
    assert!(quiet.get("semantic_recall").is_none());

    let (_, view) = all_semantic_recall_fixtures()
        .into_iter()
        .find(|(n, _)| *n == "semantic-recall-searched-scope")
        .unwrap();
    detail.semantic_recall = Some(view);
    let rendered = serde_json::to_value(&detail).unwrap();
    // The semantic recall section is distinct from the code_evidence section.
    assert!(rendered.get("code_evidence").is_some());
    let sem = rendered
        .get("semantic_recall")
        .expect("semantic recall section present");
    // Verified targets and approximate hints are rendered under separate keys.
    assert!(sem.get("verified_targets").is_some());
    assert!(sem.get("approximate_hints").is_some());
    let reparsed: DecisionDetail = serde_json::from_value(rendered).unwrap();
    assert!(reparsed.semantic_recall.is_some());
}
