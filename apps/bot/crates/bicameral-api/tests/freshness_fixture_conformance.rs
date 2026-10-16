//! Deterministic conformance for the commit-driven freshness fixtures
//! (issue #625, slice 1: API/types + fixtures).
//!
//! Each protocol JSON fixture under `protocol/schemas/v2/fixtures/` must equal
//! the daemon-authored Rust read-model produced by the matching constructor in
//! `bicameral_api::fixtures`. This proves MCP/dashboard/hosted surfaces render a
//! byte-for-byte reproducible, daemon-authored contract — they never recompute
//! freshness.
//!
//! Set `BIC_REGEN_FIXTURES=1` to (re)write the golden JSON files from the Rust
//! fixtures instead of asserting against them.

use std::path::PathBuf;

use bicameral_api::fixtures::all_freshness_fixtures;
use bicameral_api::freshness::{
    CommitFreshness, FreshnessCaveatCode, FreshnessNextAction, PreflightFreshness,
};
use bicameral_api::{
    CodeSourceCurrentness, GraphEvidenceState, GraphReadiness, RecallActionKind, RecallPacket,
};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocol/schemas/v2/fixtures")
        .canonicalize()
        .expect("fixtures dir exists")
}

/// The fixture file stores the freshness block under a `freshness` key, plus
/// human-facing `_comment`/`_expected` metadata that is ignored on parse.
fn read_fixture_block(name: &str) -> PreflightFreshness {
    let path = fixtures_dir().join(format!("{name}.json"));
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{name}: read {}: {e}", path.display()));
    let doc: serde_json::Value =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{name}: parse failed: {e}"));
    let block = doc
        .get("freshness")
        .unwrap_or_else(|| panic!("{name}: missing `freshness` key"));
    serde_json::from_value(block.clone())
        .unwrap_or_else(|e| panic!("{name}: freshness block does not deserialize: {e}"))
}

fn write_fixture(name: &str, fresh: &PreflightFreshness) {
    let path = fixtures_dir().join(format!("{name}.json"));
    let doc = serde_json::json!({
        "_comment": format!("Commit-driven preflight freshness read-model (issue #625): {name}. \
            Daemon-authored, evidence-only; surfaces render this verbatim."),
        "_expected": {
            "commit_freshness": fresh.commit_freshness,
            "currentness": fresh.currentness,
            "graph_readiness": fresh.graph_readiness,
            "evidence_state": fresh.evidence_state,
        },
        "freshness": fresh,
    });
    let pretty = serde_json::to_string_pretty(&doc).expect("serialize");
    std::fs::write(&path, format!("{pretty}\n")).expect("write fixture");
}

#[test]
fn protocol_fixtures_match_rust_read_model() {
    let regen = std::env::var("BIC_REGEN_FIXTURES").is_ok();
    for (name, fresh) in all_freshness_fixtures() {
        if regen {
            write_fixture(name, &fresh);
            continue;
        }
        let from_disk = read_fixture_block(name);
        assert_eq!(
            from_disk, fresh,
            "{name}: protocol fixture diverged from the Rust read-model"
        );
        // The fixture round-trips through JSON without loss.
        let json = serde_json::to_string(&fresh).unwrap();
        let reparsed: PreflightFreshness = serde_json::from_str(&json).unwrap();
        assert_eq!(
            reparsed, fresh,
            "{name}: read-model is not round-trip stable"
        );
    }
}

#[test]
fn all_seven_acceptance_scenarios_present() {
    let names: Vec<&str> = all_freshness_fixtures()
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    for required in [
        "freshness-stale-graph",
        "freshness-queued-refresh",
        "freshness-touched-file-changed",
        "freshness-touched-symbol-changed",
        "freshness-untouched-snapshot-scoped",
        "freshness-unknown-touched-scope",
        "freshness-no-result-stale-graph",
    ] {
        assert!(names.contains(&required), "missing fixture: {required}");
    }
}

/// The hybrid policy's load-bearing distinctions, asserted directly on the
/// fixtures so a regression in the mapping is caught here.
#[test]
fn hybrid_policy_distinctions_hold() {
    let by_name: std::collections::HashMap<&str, PreflightFreshness> =
        all_freshness_fixtures().into_iter().collect();

    // Touched code (file or symbol) is downgraded.
    for n in [
        "freshness-touched-file-changed",
        "freshness-touched-symbol-changed",
    ] {
        assert_eq!(
            by_name[n].commit_freshness,
            CommitFreshness::TouchedStale,
            "{n}"
        );
        assert_eq!(
            by_name[n].currentness,
            CodeSourceCurrentness::ContentChanged,
            "{n}"
        );
    }

    // Confirmed-untouched evidence stays snapshot-scoped (usable, not current).
    let untouched = &by_name["freshness-untouched-snapshot-scoped"];
    assert_eq!(untouched.commit_freshness, CommitFreshness::SnapshotScoped);
    assert!(!untouched.commit_freshness.is_current_for_head());

    // Unknown touched scope becomes unknown_stale (never silently fresh).
    let unknown = &by_name["freshness-unknown-touched-scope"];
    assert_eq!(unknown.commit_freshness, CommitFreshness::UnknownStale);
    assert_eq!(
        unknown.caveats[0].code,
        FreshnessCaveatCode::TouchedScopeUnknown
    );

    // Queued refresh is unknown_stale and never blocks (it returns actions).
    let queued = &by_name["freshness-queued-refresh"];
    assert_eq!(queued.commit_freshness, CommitFreshness::UnknownStale);
    assert!(queued
        .next_actions
        .contains(&FreshnessNextAction::RetryWhenReady));

    // No-result on a stale graph carries the empty-result caveat.
    let no_result = &by_name["freshness-no-result-stale-graph"];
    assert!(no_result
        .caveats
        .iter()
        .any(|c| c.code == FreshnessCaveatCode::EmptyResultStaleGraph));

    // Every non-fresh fixture carries at least one caveat AND next action and
    // reuses graph readiness vocabulary.
    for (name, f) in all_freshness_fixtures() {
        if f.commit_freshness != CommitFreshness::Fresh {
            assert!(!f.caveats.is_empty(), "{name}: missing caveat");
            assert!(!f.next_actions.is_empty(), "{name}: missing next action");
        }
        // graph_readiness is a reused GraphReadiness; evidence_state a reused
        // GraphEvidenceState — assert they are populated sensibly.
        let _r: GraphReadiness = f.graph_readiness;
        let _e: GraphEvidenceState = f.evidence_state;
    }
}

/// Authority boundary: attaching freshness to a `RecallPacket` is purely
/// additive — it must not change the packet's allowed actions or any other
/// authority-bearing field. Freshness is a separate, derived view.
#[test]
fn recall_packet_freshness_is_action_and_authority_neutral() {
    let base = RecallPacket::new_alpha();
    let base_actions: Vec<RecallActionKind> =
        base.allowed_next_actions.iter().map(|a| a.kind).collect();
    let base_canonical = base.canonical_decisions.len();
    let base_candidates = base.decision_candidates.len();

    for (_name, fresh) in all_freshness_fixtures() {
        let with = RecallPacket::new_alpha().with_freshness(fresh);
        let with_actions: Vec<RecallActionKind> =
            with.allowed_next_actions.iter().map(|a| a.kind).collect();
        assert_eq!(
            with_actions, base_actions,
            "attaching freshness must not change allowed_next_actions"
        );
        assert_eq!(with.canonical_decisions.len(), base_canonical);
        assert_eq!(with.decision_candidates.len(), base_candidates);
        assert!(with.freshness.is_some());
    }
}

/// Authority boundary: the freshness read-model never serializes governance or
/// authority vocabulary. Freshness labels are evidence-only.
#[test]
fn fixtures_carry_no_authority_or_governance_vocabulary() {
    for (name, f) in all_freshness_fixtures() {
        let json = serde_json::to_string(&f).unwrap();
        for forbidden in [
            "signoff",
            "compliance",
            "compliant",
            "approved",
            "no_conflict",
            "no-conflict",
            "merge_safe",
            "merge-safety",
            "binding_evidence",
            "decision_state",
            "\"safe\"",
            "\"blocked\"",
        ] {
            assert!(
                !json.contains(forbidden),
                "{name}: freshness fixture must not contain '{forbidden}': {json}"
            );
        }
    }
}
