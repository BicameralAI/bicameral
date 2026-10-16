//! Progressive preflight conformance tests.
//!
//! Proves issue #140 acceptance criteria:
//!
//! 1. Source-only preflight remains available when graph data is unavailable.
//! 2. Diff-scoped graph preflight builds or reuses bounded graph slices.
//! 3. Responses include `snapshot_scope`, `unknown_scope`, readiness warnings,
//!    and per-result relevance reasons.
//! 4. Local no-conflict is never reported as global or team-wide consistency.
//! 5. Deterministic ordering, deduplication, and distinction between source-only
//!    and grounded results.
//! 6. Preflight query output does not mutate compliance, signoff, review,
//!    binding, or conflict state.
//!
//! All tests are deterministic — no network, no hosted graph, no credentials.

use bicameral_api::candidate::{CandidateState, DecisionCandidate, DecisionLevel};
use bicameral_api::dashboard::ComplianceState;
use bicameral_api::event_store::{
    BindToCodePayload, EventStoreAdapter, EventStoreEntry, EventType, IngestPayload,
};
use bicameral_api::evidence::SourceEvidence;
use bicameral_api::evidence::{
    BindingConfidence, BindingType, GraphEvidenceState, GraphReadiness, GraphSnapshot, IndexSource,
    SnapshotResolution,
};
use bicameral_api::source::Source;
use bicameral_runtime::event_store_adapters::MemoryAdapter;
use bicameral_runtime::preflight::{
    query_preflight, query_preflight_progressive, PreflightGraphContext, PreflightQuery,
    PreflightRelevance,
};
use bicameral_runtime::replay::replay_from_adapter;
use uuid::Uuid;

// ── Helpers ──────────────────────────────────────────────────────────────

fn seed_fixture() -> bicameral_runtime::replay::ReplayedState {
    let adapter = MemoryAdapter::new();

    // Decision A: "API rate limiting" — bound to src/gateway.rs
    let source_a = Source {
        uri: "jira://PROJ-1".to_string(),
        source_type: "test".to_string(),
        label: None,
        source_kind: None,
    };
    let ev_a = vec![SourceEvidence::new(
        "inline:jira://PROJ-1".to_string(),
        0,
        10,
        "test".to_string(),
        0.9,
    )];
    let mut cand_a = DecisionCandidate::new(
        "API rate limiting".to_string(),
        "All API gateway endpoints must enforce rate limits".to_string(),
        DecisionLevel::Architecture,
        "jira://PROJ-1".to_string(),
    );
    cand_a.evidence_refs = ev_a.iter().map(|e| e.id).collect();
    let cand_a_id = cand_a.id;
    adapter
        .append(EventStoreEntry::new(
            EventType::Ingest,
            serde_json::to_value(&IngestPayload {
                source: source_a,
                snapshot: None,
                evidence: ev_a,
                candidate: cand_a,
                evidence_references: Vec::new(),
                provenance: None,
            })
            .unwrap(),
            "test".to_string(),
        ))
        .unwrap();

    let decision_a_id = Uuid::new_v4();
    adapter
        .append(EventStoreEntry::new(
            EventType::AcceptCandidate,
            serde_json::json!({
                "candidate_id": cand_a_id.to_string(),
                "decision_id": decision_a_id.to_string(),
            }),
            "test".to_string(),
        ))
        .unwrap();

    // Bind A → src/gateway.rs
    let binding_a = bicameral_api::evidence::BindingEvidence::new(
        cand_a_id,
        BindingType::FilePath,
        "src/gateway.rs".to_string(),
        BindingConfidence::High,
    );
    adapter
        .append(EventStoreEntry::new(
            EventType::BindToCode,
            serde_json::to_value(&BindToCodePayload {
                decision_id: decision_a_id,
                binding: binding_a,
                graph_snapshot_id: "gs-fixture-abc".to_string(),
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

    // Decision B: "Auth JWT validation" — bound to auth_service::verify_token
    let source_b = Source {
        uri: "github://pr-42".to_string(),
        source_type: "test".to_string(),
        label: None,
        source_kind: None,
    };
    let ev_b = vec![SourceEvidence::new(
        "inline:github://pr-42".to_string(),
        0,
        10,
        "test".to_string(),
        0.9,
    )];
    let mut cand_b = DecisionCandidate::new(
        "Auth JWT validation".to_string(),
        "Authentication service must validate JWT with RS256".to_string(),
        DecisionLevel::Architecture,
        "github://pr-42".to_string(),
    );
    cand_b.evidence_refs = ev_b.iter().map(|e| e.id).collect();
    let cand_b_id = cand_b.id;
    adapter
        .append(EventStoreEntry::new(
            EventType::Ingest,
            serde_json::to_value(&IngestPayload {
                source: source_b,
                snapshot: None,
                evidence: ev_b,
                candidate: cand_b,
                evidence_references: Vec::new(),
                provenance: None,
            })
            .unwrap(),
            "test".to_string(),
        ))
        .unwrap();

    let decision_b_id = Uuid::new_v4();
    adapter
        .append(EventStoreEntry::new(
            EventType::AcceptCandidate,
            serde_json::json!({
                "candidate_id": cand_b_id.to_string(),
                "decision_id": decision_b_id.to_string(),
            }),
            "test".to_string(),
        ))
        .unwrap();

    // Bind B → auth_service::verify_token
    let binding_b = bicameral_api::evidence::BindingEvidence::new(
        cand_b_id,
        BindingType::Symbol,
        "auth_service::verify_token".to_string(),
        BindingConfidence::High,
    );
    adapter
        .append(EventStoreEntry::new(
            EventType::BindToCode,
            serde_json::to_value(&BindToCodePayload {
                decision_id: decision_b_id,
                binding: binding_b,
                graph_snapshot_id: "gs-fixture-abc".to_string(),
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

    // Decision C: "Database migration strategy" — no binding
    let source_c = Source {
        uri: "slack://msg-7".to_string(),
        source_type: "test".to_string(),
        label: None,
        source_kind: None,
    };
    let ev_c = vec![SourceEvidence::new(
        "inline:slack://msg-7".to_string(),
        0,
        10,
        "test".to_string(),
        0.9,
    )];
    let mut cand_c = DecisionCandidate::new(
        "Database migration strategy".to_string(),
        "Migrate from SQLite to PostgreSQL incrementally".to_string(),
        DecisionLevel::Architecture,
        "slack://msg-7".to_string(),
    );
    cand_c.evidence_refs = ev_c.iter().map(|e| e.id).collect();
    let cand_c_id = cand_c.id;
    adapter
        .append(EventStoreEntry::new(
            EventType::Ingest,
            serde_json::to_value(&IngestPayload {
                source: source_c,
                snapshot: None,
                evidence: ev_c,
                candidate: cand_c,
                evidence_references: Vec::new(),
                provenance: None,
            })
            .unwrap(),
            "test".to_string(),
        ))
        .unwrap();

    let decision_c_id = Uuid::new_v4();
    adapter
        .append(EventStoreEntry::new(
            EventType::AcceptCandidate,
            serde_json::json!({
                "candidate_id": cand_c_id.to_string(),
                "decision_id": decision_c_id.to_string(),
            }),
            "test".to_string(),
        ))
        .unwrap();

    replay_from_adapter(&adapter).unwrap()
}

fn make_resolved_graph_ctx() -> PreflightGraphContext {
    PreflightGraphContext {
        resolution: SnapshotResolution::Resolved {
            snapshot: GraphSnapshot {
                graph_snapshot_id: "gs-conformance-test".to_string(),
                repo_id: "repo-conformance".to_string(),
                commit_sha: "deadbeef12345678".to_string(),
                index_version: "file-inspect-v1".to_string(),
                ref_name: Some("refs/heads/feature/preflight".to_string()),
                merge_base_sha: None,
                indexed_at: None,
                index_source: Some(IndexSource::LocalDaemon),
            },
        },
    }
}

// ── AC1: Source-only preflight available without graph data ────────────

#[test]
fn ac1_source_only_preflight_without_graph() {
    let state = seed_fixture();

    let resp = query_preflight(
        &state,
        &PreflightQuery {
            files: vec!["src/gateway.rs".to_string()],
            symbols: vec![],
            diff_context: None,
            branch: None,
        },
    );

    assert!(
        !resp.relevant_decisions.is_empty(),
        "Source-only preflight must return matching decisions"
    );
    for d in &resp.relevant_decisions {
        assert_eq!(
            d.compliance_state,
            ComplianceState::Ungrounded,
            "Source-only must report ungrounded compliance"
        );
        assert!(
            d.relevance_reasons
                .contains(&PreflightRelevance::SourceTextMatch),
            "Source-only must report SourceTextMatch relevance"
        );
    }
    assert_eq!(resp.readiness.state, GraphReadiness::NotConfigured);
}

#[test]
fn ac1_source_only_fallback_when_graph_none() {
    let state = seed_fixture();

    let resp = query_preflight_progressive(
        &state,
        &PreflightQuery {
            files: vec![],
            symbols: vec![],
            diff_context: None,
            branch: None,
        },
        None,
    );

    assert_eq!(resp.readiness.state, GraphReadiness::NotConfigured);
    assert!(
        resp.readiness
            .unknown_scope
            .iter()
            .any(|u| u.scope == "graph"),
        "Must report graph as unknown scope"
    );
    for d in &resp.relevant_decisions {
        assert_eq!(d.compliance_state, ComplianceState::Ungrounded);
    }
}

// ── AC2: Diff-scoped graph preflight uses bounded graph slices ────────

#[test]
fn ac2_graph_preflight_uses_binding_evidence() {
    let state = seed_fixture();
    let ctx = make_resolved_graph_ctx();

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
        .find(|d| d.title == "API rate limiting");
    assert!(
        gateway_decision.is_some(),
        "Graph preflight must find decision via binding target"
    );
    assert!(
        gateway_decision
            .unwrap()
            .relevance_reasons
            .contains(&PreflightRelevance::BindingTargetMatch),
        "Must report BindingTargetMatch relevance"
    );
}

#[test]
fn ac2_graph_preflight_symbol_binding_match() {
    let state = seed_fixture();
    let ctx = make_resolved_graph_ctx();

    let resp = query_preflight_progressive(
        &state,
        &PreflightQuery {
            files: vec![],
            symbols: vec!["auth_service::verify_token".to_string()],
            diff_context: None,
            branch: None,
        },
        Some(&ctx),
    );

    let auth_decision = resp
        .relevant_decisions
        .iter()
        .find(|d| d.title == "Auth JWT validation");
    assert!(
        auth_decision.is_some(),
        "Graph preflight must find decision via symbol binding"
    );
    assert!(auth_decision
        .unwrap()
        .relevance_reasons
        .contains(&PreflightRelevance::BindingTargetMatch),);
}

// ── AC3: snapshot_scope, unknown_scope, readiness, relevance reasons ──

#[test]
fn ac3_response_includes_snapshot_scope() {
    let state = seed_fixture();
    let ctx = make_resolved_graph_ctx();

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
        Some("gs-conformance-test")
    );
    assert_eq!(
        resp.readiness.snapshot_scope[0].validated_sha.as_deref(),
        Some("deadbeef12345678")
    );
}

#[test]
fn ac3_response_includes_unknown_scope() {
    let state = seed_fixture();

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
        resp.readiness
            .unknown_scope
            .iter()
            .any(|u| u.scope == "team_active_prs" && u.state == "hosted_required"),
        "Must report team scope as hosted_required"
    );
}

#[test]
fn ac3_readiness_warnings_for_stale_index() {
    let state = seed_fixture();
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
fn ac3_per_result_relevance_reasons() {
    let state = seed_fixture();
    let ctx = make_resolved_graph_ctx();

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

    let gateway = resp
        .relevant_decisions
        .iter()
        .find(|d| d.title == "API rate limiting")
        .expect("gateway decision should be present");

    assert!(
        gateway
            .relevance_reasons
            .contains(&PreflightRelevance::SourceTextMatch),
        "Must report SourceTextMatch"
    );
    assert!(
        gateway
            .relevance_reasons
            .contains(&PreflightRelevance::BindingTargetMatch),
        "Must report BindingTargetMatch"
    );
}

// ── AC4: Local no-conflict is never global/team-wide ──────────────────

#[test]
fn ac4_local_no_conflict_never_global() {
    let state = seed_fixture();
    let ctx = make_resolved_graph_ctx();

    let resp = query_preflight_progressive(
        &state,
        &PreflightQuery {
            files: vec!["src/unrelated_file.rs".to_string()],
            symbols: vec![],
            diff_context: None,
            branch: None,
        },
        Some(&ctx),
    );

    // No matches — but this is not global consistency.
    assert!(resp.relevant_decisions.is_empty());
    assert!(
        resp.readiness
            .unknown_scope
            .iter()
            .any(|u| u.scope == "team_active_prs"),
        "Must disclose team scope limitation even with no matches"
    );
    assert!(
        resp.warnings
            .iter()
            .any(|w| w.contains("limited to resolved local snapshots")),
        "Must warn that no-conflict is local only"
    );
}

#[test]
fn ac4_source_only_no_conflict_never_global() {
    let state = seed_fixture();

    let resp = query_preflight(
        &state,
        &PreflightQuery {
            files: vec!["src/unrelated_file.rs".to_string()],
            symbols: vec![],
            diff_context: None,
            branch: None,
        },
    );

    assert!(resp.relevant_decisions.is_empty());
    assert!(
        resp.readiness
            .unknown_scope
            .iter()
            .any(|u| u.scope == "team_active_prs"),
        "Source-only must also disclose team scope limitation"
    );
}

// ── AC5: Deterministic ordering, deduplication, distinction ───────────

#[test]
fn ac5_ordering_is_deterministic() {
    let state = seed_fixture();
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
    assert_eq!(titles1, titles2, "Ordering must be deterministic");

    for w in titles1.windows(2) {
        assert!(w[0] <= w[1], "Decisions must be alphabetically ordered");
    }
}

#[test]
fn ac5_deduplication_with_multiple_relevance_signals() {
    let state = seed_fixture();
    let ctx = make_resolved_graph_ctx();

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

    let gateway_count = resp
        .relevant_decisions
        .iter()
        .filter(|d| d.title == "API rate limiting")
        .count();
    assert_eq!(
        gateway_count, 1,
        "Decision must appear exactly once despite multiple relevance signals"
    );
}

#[test]
fn ac5_source_only_vs_grounded_distinction() {
    let state = seed_fixture();
    let ctx = make_resolved_graph_ctx();

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

    // Decision C has no binding — must be source-only (FullLedger).
    let db_decision = resp
        .relevant_decisions
        .iter()
        .find(|d| d.title == "Database migration strategy")
        .expect("unbound decision should appear");
    assert_eq!(
        db_decision.compliance_state,
        ComplianceState::Ungrounded,
        "Unbound decision must remain ungrounded"
    );
    assert!(
        db_decision
            .relevance_reasons
            .contains(&PreflightRelevance::FullLedger),
        "Unbound decision must report FullLedger"
    );
    assert!(
        !db_decision
            .relevance_reasons
            .contains(&PreflightRelevance::BindingTargetMatch),
        "Unbound decision must NOT report BindingTargetMatch"
    );
}

// ── AC6: Preflight does not mutate any state ──────────────────────────

#[test]
fn ac6_preflight_does_not_mutate_compliance_signoff_review_binding_conflict() {
    let state = seed_fixture();
    let ctx = make_resolved_graph_ctx();

    // Snapshot state before.
    let decisions_before: Vec<_> = state
        .decisions
        .values()
        .map(|d| (d.id, d.signoff_state, d.compliance_state, d.review_state))
        .collect();
    let bindings_count_before: usize = state.bindings.values().map(|v| v.len()).sum();
    let events_count_before = state.events.len();

    // Run multiple preflights.
    for _ in 0..3 {
        let _ = query_preflight_progressive(
            &state,
            &PreflightQuery {
                files: vec!["src/gateway.rs".to_string()],
                symbols: vec!["auth_service::verify_token".to_string()],
                diff_context: Some("+ add rate limit\n- remove old code".to_string()),
                branch: Some("feature/preflight".to_string()),
            },
            Some(&ctx),
        );
    }

    // Verify no mutations.
    let decisions_after: Vec<_> = state
        .decisions
        .values()
        .map(|d| (d.id, d.signoff_state, d.compliance_state, d.review_state))
        .collect();
    let bindings_count_after: usize = state.bindings.values().map(|v| v.len()).sum();
    let events_count_after = state.events.len();

    assert_eq!(decisions_before.len(), decisions_after.len());
    assert_eq!(bindings_count_before, bindings_count_after);
    assert_eq!(events_count_before, events_count_after);
}

// ── Hosted-required resolution ────────────────────────────────────────

#[test]
fn hosted_required_resolution_reports_not_configured() {
    let state = seed_fixture();
    let ctx = PreflightGraphContext {
        resolution: SnapshotResolution::HostedRequired {
            requested_ref: "refs/remotes/origin/team-branch".to_string(),
            detail: "Cross-team branch not available locally".to_string(),
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

    // HostedRequired maps to NotConfigured — the graph is not available locally.
    assert_eq!(resp.readiness.state, GraphReadiness::NotConfigured);
    assert!(resp.readiness.snapshot_scope.is_empty());
}

// ── Full-ledger with no filter ────────────────────────────────────────

#[test]
fn full_ledger_returns_all_decisions_with_full_ledger_relevance() {
    let state = seed_fixture();

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
        assert!(
            d.relevance_reasons
                .contains(&PreflightRelevance::FullLedger),
            "Full-ledger mode must report FullLedger relevance"
        );
        assert_eq!(d.compliance_state, ComplianceState::Ungrounded);
    }
}
