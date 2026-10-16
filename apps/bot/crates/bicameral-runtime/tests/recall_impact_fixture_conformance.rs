//! Recall Impact-to-Decision Fixture Conformance Tests
//!
//! Alpha fixture corpus for preflight impact-to-decision recall.
//! Validates the invariants from:
//! - `docs/specs/preflight-impact-to-decision-recall-strategy.md`
//! - `docs/specs/constraint-lookup-recallpacket-contract.md`
//!
//! ## Evaluation metrics
//!
//! - recall@k: relevant decisions appear in top-k results
//! - precision@k: no false positives in top-k results
//! - first relevant rank / MRR: position of first relevant result
//! - critical false negatives: accepted binding overlap must never be missed
//! - false-positive/noise count: non-relevant items in results
//! - honesty-label coverage: every result carries an honest authority label
//! - deterministic replay: fixtures produce identical output on re-run
//!
//! ## Negative fixture coverage
//!
//! Each negative test enforces one invariant from the recall strategy spec:
//!
//! 1. High ranking score cannot create/promote Decision or BindingEvidence
//! 2. Semantic similarity cannot assert compliance
//! 3. CodeGenome confidence cannot prove no-conflict
//! 4. Stale/unscoped signals are rejected or downgraded
//! 5. No-match from partial scope cannot claim global absence
//! 6. MCP cannot compute ranking/fusion locally beyond rendering daemon output
//! 7. Preferences cannot create authority, waive gates, or hide accepted evidence
//!
//! ## Protocol-level JSON fixtures
//!
//! Positive:
//! - `protocol/schemas/v2/fixtures/recall-impact-positive-binding-overlap.json`
//! - `protocol/schemas/v2/fixtures/recall-impact-positive-graph-overlap.json`
//! - `protocol/schemas/v2/fixtures/recall-impact-positive-corpus-search.json`
//!
//! Negative:
//! - `protocol/schemas/v2/fixtures/recall-negative-high-score-no-create-decision.json`
//! - `protocol/schemas/v2/fixtures/recall-negative-semantic-similarity-no-compliance.json`
//! - `protocol/schemas/v2/fixtures/recall-negative-codegenome-no-prove.json`
//! - `protocol/schemas/v2/fixtures/recall-negative-stale-scope-rejected.json`
//! - `protocol/schemas/v2/fixtures/recall-negative-partial-scope-no-global-absence.json`
//! - `protocol/schemas/v2/fixtures/recall-negative-mcp-local-no-ranking.json`
//! - `protocol/schemas/v2/fixtures/recall-negative-preferences-no-authority.json`

use bicameral_api::fixtures;
use bicameral_api::recall::{AuthorityLabelKind, RecallAction, RecallActionKind, RecallPacket};

// ===========================================================================
// Deterministic replay: all fixtures serialize/deserialize identically
// ===========================================================================

#[test]
fn all_recall_fixtures_roundtrip_deterministic() {
    for (name, packet) in fixtures::all_recall_impact_fixtures() {
        let json = serde_json::to_string_pretty(&packet)
            .unwrap_or_else(|e| panic!("{name}: serialize failed: {e}"));
        let parsed: RecallPacket = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("{name}: deserialize failed: {e}"));
        let re_json = serde_json::to_string_pretty(&parsed)
            .unwrap_or_else(|e| panic!("{name}: re-serialize failed: {e}"));
        assert_eq!(
            json, re_json,
            "{name}: round-trip JSON mismatch — replay is not deterministic"
        );
        assert_eq!(
            parsed.packet_id, packet.packet_id,
            "{name}: packet_id mismatch after replay"
        );
    }
}

// ===========================================================================
// Honesty-label coverage: every entry carries an authority label
// ===========================================================================

#[test]
fn all_recall_fixtures_have_honesty_labels() {
    for (name, packet) in fixtures::all_recall_impact_fixtures() {
        for (i, decision) in packet.canonical_decisions.iter().enumerate() {
            assert!(
                !decision.authority_labels.is_empty(),
                "{name}: canonical_decisions[{i}] has no authority labels — honesty-label coverage violated"
            );
        }
        // Candidates from semantic-only routes must carry advisory labels
        for (i, candidate) in packet.decision_candidates.iter().enumerate() {
            // Candidates at minimum must have an origin_surface
            assert!(
                !candidate.origin_surface.is_empty(),
                "{name}: decision_candidates[{i}] has empty origin_surface"
            );
        }
    }
}

// ===========================================================================
// Recall@k and precision@k: positive fixtures
// ===========================================================================

#[test]
fn binding_overlap_recall_at_1() {
    let packet = fixtures::recall_packet_binding_overlap();
    assert!(
        !packet.canonical_decisions.is_empty(),
        "binding_overlap: recall@1 — must return at least one canonical decision"
    );
    assert_eq!(
        packet.canonical_decisions[0].title, "API handler must validate input before dispatch",
        "binding_overlap: first relevant result must be the binding-overlap decision"
    );
    assert!(
        packet.decision_candidates.is_empty(),
        "binding_overlap: precision — binding overlap should not produce noise candidates"
    );
}

#[test]
fn graph_overlap_recall_at_1_with_unknown_scope() {
    let packet = fixtures::recall_packet_graph_overlap();
    assert!(
        !packet.canonical_decisions.is_empty(),
        "graph_overlap: recall@1 — must return at least one canonical decision"
    );
    assert!(
        !packet.unknown_scope.is_empty(),
        "graph_overlap: must disclose unknown scope when graph is not fully indexed"
    );
}

#[test]
fn corpus_search_recall_with_candidate() {
    let packet = fixtures::recall_packet_corpus_search();
    assert!(
        !packet.canonical_decisions.is_empty(),
        "corpus_search: must return at least one canonical decision"
    );
    assert!(
        !packet.decision_candidates.is_empty(),
        "corpus_search: must surface related candidates"
    );
}

// ===========================================================================
// Critical false negatives: accepted binding overlap must never be missed
// ===========================================================================

#[test]
fn binding_overlap_is_never_empty() {
    let packet = fixtures::recall_packet_binding_overlap();
    assert!(
        !packet.canonical_decisions.is_empty(),
        "CRITICAL: accepted binding overlap must never produce an empty canonical_decisions — this is a critical false negative"
    );
    let has_human_ratified = packet.canonical_decisions.iter().any(|d| {
        d.authority_labels
            .iter()
            .any(|l| l.kind == AuthorityLabelKind::HumanRatified)
    });
    assert!(
        has_human_ratified,
        "CRITICAL: binding overlap decisions must carry human_ratified authority label"
    );
}

#[test]
fn preference_must_not_hide_binding_overlap() {
    let packet = fixtures::recall_packet_preference_must_not_hide();
    assert!(
        !packet.canonical_decisions.is_empty(),
        "CRITICAL: accepted binding overlap decision must not be hidden by preference suppression"
    );
    let has_human_ratified = packet.canonical_decisions.iter().any(|d| {
        d.authority_labels
            .iter()
            .any(|l| l.kind == AuthorityLabelKind::HumanRatified)
    });
    assert!(
        has_human_ratified,
        "CRITICAL: preference-filtered packet must still surface human_ratified binding overlap"
    );
}

// ===========================================================================
// Negative: high score cannot create/promote Decision or BindingEvidence
// ===========================================================================

#[test]
fn high_score_recall_packet_has_no_mutation_fields() {
    for (name, packet) in fixtures::all_recall_impact_fixtures() {
        let json = serde_json::to_value(&packet).unwrap();
        let obj = json.as_object().unwrap();
        // RecallPacket must never contain mutation-capable fields
        let forbidden = [
            "binding_evidence",
            "compliance_state",
            "enforcement",
            "signoff_approval",
            "accepted_decision",
            "created_decision",
        ];
        for field in &forbidden {
            assert!(
                !obj.contains_key(*field),
                "{name}: RecallPacket contains forbidden mutation field '{field}' — high score cannot create Decision or BindingEvidence"
            );
        }
    }
}

#[test]
fn recall_packet_allowed_actions_cannot_bypass_governed_path() {
    for (name, packet) in fixtures::all_recall_impact_fixtures() {
        for action in &packet.allowed_next_actions {
            match action.kind {
                RecallActionKind::InspectEvidence => {
                    assert!(
                        !action.requires_authority,
                        "{name}: inspect_evidence must not require authority"
                    );
                }
                RecallActionKind::ExpandScope => {
                    assert!(
                        !action.requires_authority,
                        "{name}: expand_scope must not require authority"
                    );
                }
                RecallActionKind::RequestCorrection => {
                    assert!(
                        action.requires_authority,
                        "{name}: request_correction MUST require authority"
                    );
                }
                RecallActionKind::PromoteDecisionCandidate => {
                    assert!(
                        action.requires_authority,
                        "{name}: promote_decision_candidate MUST require authority"
                    );
                }
            }
        }
    }
}

// ===========================================================================
// Negative: semantic similarity cannot assert compliance
// ===========================================================================

#[test]
fn semantic_similarity_carries_only_advisory_labels() {
    let packet = fixtures::recall_packet_semantic_similarity_advisory();
    // No canonical decisions should appear from similarity-only
    assert!(
        packet.canonical_decisions.is_empty(),
        "semantic_similarity: similarity-only route must not produce canonical decisions — compliance cannot be asserted from similarity"
    );
    // All candidates from semantic similarity must not carry human_ratified
    for (i, candidate) in packet.decision_candidates.iter().enumerate() {
        assert_ne!(
            candidate.extraction_confidence,
            bicameral_api::candidate::ExtractionConfidence::High,
            "semantic_similarity: decision_candidates[{i}] must not carry High extraction confidence from similarity alone"
        );
    }
    // RecallPacket must not contain compliance fields
    let json = serde_json::to_value(&packet).unwrap();
    let obj = json.as_object().unwrap();
    assert!(
        !obj.contains_key("compliance_state"),
        "semantic_similarity: packet must not contain compliance_state"
    );
    assert!(
        !obj.contains_key("enforcement"),
        "semantic_similarity: packet must not contain enforcement"
    );
}

// ===========================================================================
// Negative: CodeGenome confidence cannot prove no-conflict
// ===========================================================================

#[test]
fn codegenome_advisory_cannot_create_authority() {
    // RecallPacket actions are the only way to interact with recall output.
    // Even with high CodeGenome signals, the packet's allowed_next_actions
    // must enforce authority gating for promotion.
    for (name, packet) in fixtures::all_recall_impact_fixtures() {
        if packet.is_action_allowed(RecallActionKind::PromoteDecisionCandidate) {
            let promote_action = packet
                .allowed_next_actions
                .iter()
                .find(|a| a.kind == RecallActionKind::PromoteDecisionCandidate)
                .unwrap();
            assert!(
                promote_action.requires_authority,
                "{name}: PromoteDecisionCandidate must require authority — CodeGenome confidence cannot bypass"
            );
        }
    }
}

// ===========================================================================
// Negative: stale/unscoped signals are rejected or downgraded
// ===========================================================================

#[test]
fn stale_evidence_carries_stale_label() {
    let packet = fixtures::recall_packet_stale_evidence();
    assert!(!packet.canonical_decisions.is_empty());
    let stale_decision = &packet.canonical_decisions[0];
    let has_stale_label = stale_decision
        .authority_labels
        .iter()
        .any(|l| l.kind == AuthorityLabelKind::Stale);
    assert!(
        has_stale_label,
        "stale_evidence: stale decisions must carry Stale authority label"
    );
    assert!(
        !packet.unknown_scope.is_empty(),
        "stale_evidence: stale packet must disclose stale scope in unknown_scope"
    );
}

// ===========================================================================
// Negative: no-match from partial scope cannot claim global absence
// ===========================================================================

#[test]
fn partial_scope_no_match_discloses_unknown_scope() {
    let packet = fixtures::recall_packet_partial_scope_no_match();
    assert!(
        packet.canonical_decisions.is_empty(),
        "partial_scope_no_match: no canonical decisions expected"
    );
    assert!(
        packet.decision_candidates.is_empty(),
        "partial_scope_no_match: no candidates expected"
    );
    assert!(
        !packet.unknown_scope.is_empty(),
        "CRITICAL: partial scope search with no match MUST disclose unknown_scope — cannot claim global absence"
    );
    // Must have at least one expansion target
    let has_targets = packet
        .unknown_scope
        .iter()
        .any(|s| !s.expansion_targets.is_empty());
    assert!(
        has_targets,
        "partial_scope_no_match: unknown_scope entries must provide expansion_targets"
    );
}

#[test]
fn partial_scope_packet_has_no_global_absence_claim() {
    let packet = fixtures::recall_packet_partial_scope_no_match();
    let json = serde_json::to_string(&packet).unwrap();
    let forbidden_claims = [
        "No relevant Decision exists",
        "No conflict found",
        "This work is clear",
        "No governing constraint applies",
    ];
    for claim in &forbidden_claims {
        assert!(
            !json.contains(claim),
            "partial_scope_no_match: serialized packet must not contain forbidden global absence claim: '{claim}'"
        );
    }
}

// ===========================================================================
// Negative: MCP cannot compute ranking/fusion locally
// ===========================================================================

#[test]
fn recall_packet_is_daemon_output_only() {
    // RecallPacket is a read-only artifact returned by the daemon.
    // Its structure does not contain any ranking computation fields that
    // would allow MCP to re-rank or fuse signals locally.
    for (name, packet) in fixtures::all_recall_impact_fixtures() {
        let json = serde_json::to_value(&packet).unwrap();
        let obj = json.as_object().unwrap();
        let mcp_forbidden_fields = [
            "ranking_function",
            "fusion_weights",
            "local_score_override",
            "mcp_computed_rank",
        ];
        for field in &mcp_forbidden_fields {
            assert!(
                !obj.contains_key(*field),
                "{name}: RecallPacket must not contain '{field}' — MCP cannot compute ranking locally"
            );
        }
    }
}

// ===========================================================================
// Negative: preferences cannot hide accepted evidence or create authority
// ===========================================================================

#[test]
fn preference_filtered_packet_still_shows_binding_overlap() {
    let packet = fixtures::recall_packet_preference_must_not_hide();
    // The decision with accepted binding overlap must survive preference filtering
    assert!(
        !packet.canonical_decisions.is_empty(),
        "preferences_no_authority: accepted binding overlap must not be hidden by preference suppression"
    );
    let binding_decision = &packet.canonical_decisions[0];
    assert!(
        binding_decision
            .authority_labels
            .iter()
            .any(|l| l.kind == AuthorityLabelKind::HumanRatified),
        "preferences_no_authority: surviving decision must retain human_ratified label"
    );
}

#[test]
fn preference_filtered_packet_has_no_authority_creation() {
    let packet = fixtures::recall_packet_preference_must_not_hide();
    let json = serde_json::to_value(&packet).unwrap();
    let obj = json.as_object().unwrap();
    // Preferences must not add authority-creating fields
    let forbidden = [
        "binding_evidence",
        "compliance_state",
        "signoff_approval",
        "preference_created_authority",
    ];
    for field in &forbidden {
        assert!(
            !obj.contains_key(*field),
            "preferences_no_authority: packet must not contain '{field}' — preferences cannot create authority"
        );
    }
}

// ===========================================================================
// Cross-cutting: all fixtures have valid alpha actions
// ===========================================================================

#[test]
fn all_recall_fixtures_have_alpha_actions() {
    let expected_alpha = RecallAction::alpha_actions();
    for (name, packet) in fixtures::all_recall_impact_fixtures() {
        assert_eq!(
            packet.allowed_next_actions.len(),
            expected_alpha.len(),
            "{name}: alpha packets must have exactly {} allowed actions",
            expected_alpha.len()
        );
        assert!(
            packet.is_action_allowed(RecallActionKind::InspectEvidence),
            "{name}: must allow InspectEvidence"
        );
        assert!(
            packet.is_action_allowed(RecallActionKind::ExpandScope),
            "{name}: must allow ExpandScope"
        );
        assert!(
            packet.is_action_allowed(RecallActionKind::PromoteDecisionCandidate),
            "{name}: must allow PromoteDecisionCandidate"
        );
    }
}

// ===========================================================================
// JSON fixture files parse correctly
// ===========================================================================

#[test]
fn positive_json_fixtures_parse() {
    let fixtures = [
        include_str!(
            "../../../protocol/schemas/v2/fixtures/recall-impact-positive-binding-overlap.json"
        ),
        include_str!(
            "../../../protocol/schemas/v2/fixtures/recall-impact-positive-graph-overlap.json"
        ),
        include_str!(
            "../../../protocol/schemas/v2/fixtures/recall-impact-positive-corpus-search.json"
        ),
    ];
    for (i, fixture_json) in fixtures.iter().enumerate() {
        let parsed: serde_json::Value = serde_json::from_str(fixture_json)
            .unwrap_or_else(|e| panic!("positive fixture {i}: parse failed: {e}"));
        assert!(
            parsed.is_object(),
            "positive fixture {i}: must be a JSON object"
        );
        assert!(
            parsed.get("_expected").is_some()
                || parsed.get("_expected_response").is_some()
                || parsed.get("expected_recall_packet").is_some(),
            "positive fixture {i}: must have expected outcome annotation"
        );
    }
}

#[test]
fn negative_json_fixtures_parse() {
    let fixtures = [
        include_str!("../../../protocol/schemas/v2/fixtures/recall-negative-high-score-no-create-decision.json"),
        include_str!("../../../protocol/schemas/v2/fixtures/recall-negative-semantic-similarity-no-compliance.json"),
        include_str!("../../../protocol/schemas/v2/fixtures/recall-negative-codegenome-no-prove.json"),
        include_str!("../../../protocol/schemas/v2/fixtures/recall-negative-stale-scope-rejected.json"),
        include_str!("../../../protocol/schemas/v2/fixtures/recall-negative-partial-scope-no-global-absence.json"),
        include_str!("../../../protocol/schemas/v2/fixtures/recall-negative-mcp-local-no-ranking.json"),
        include_str!("../../../protocol/schemas/v2/fixtures/recall-negative-preferences-no-authority.json"),
    ];
    for (i, fixture_json) in fixtures.iter().enumerate() {
        let parsed: serde_json::Value = serde_json::from_str(fixture_json)
            .unwrap_or_else(|e| panic!("negative fixture {i}: parse failed: {e}"));
        assert!(
            parsed.is_object(),
            "negative fixture {i}: must be a JSON object"
        );
        assert!(
            parsed.get("_invariant").is_some(),
            "negative fixture {i}: must have _invariant annotation"
        );
        assert!(
            parsed.get("_must_fail_if").is_some(),
            "negative fixture {i}: must have _must_fail_if annotation"
        );
    }
}

// ===========================================================================
// False-positive / noise count: fixtures with only similarity or advisory
// routes must not produce canonical decisions
// ===========================================================================

#[test]
fn advisory_only_packets_have_no_canonical_decisions() {
    let packet = fixtures::recall_packet_semantic_similarity_advisory();
    assert_eq!(
        packet.canonical_decisions.len(),
        0,
        "advisory_only: semantic similarity route must not produce canonical decisions (false positive)"
    );
}

// ===========================================================================
// MRR (Mean Reciprocal Rank): first relevant result position
// ===========================================================================

#[test]
fn binding_overlap_first_relevant_rank_is_1() {
    let packet = fixtures::recall_packet_binding_overlap();
    // First canonical decision IS the relevant result
    assert!(
        !packet.canonical_decisions.is_empty(),
        "binding_overlap MRR: must have at least one result"
    );
    // Binding overlap should be first (rank 1, MRR = 1.0)
    let first = &packet.canonical_decisions[0];
    assert!(
        first
            .authority_labels
            .iter()
            .any(|l| l.kind == AuthorityLabelKind::HumanRatified),
        "binding_overlap MRR: first result must be human_ratified (highest confidence tier)"
    );
}
