//! Preflight lookup checkpoint hint conformance — issue #551.
//!
//! Proves the alpha preflight lookup request shape with optional
//! `checkpoint_hint` metadata. Checkpoint labels (`pre_work`,
//! `mid_session`, `pre_write`, `manual_lookup`) are inert hints only.
//!
//! Acceptance criteria covered:
//!   1. Request shape: `PreflightRunParams` accepts optional `checkpoint_hint`.
//!   2. Hint metadata: all four variants serialize/deserialize correctly.
//!   3. Read-only contract: lookup with any hint creates no canonical mutation
//!      (no `SourceSnapshot`, `SourceEvidence`, `DecisionCandidate`, `Decision`,
//!      `BindingEvidence`, signoff, compliance, or enforcement state).
//!   4. Hint is echoed in the staged response `lookup.checkpoint_hint` field.
//!   5. Omitting the hint produces the same behavior as providing one.
//!   6. Backward compatibility: requests without `checkpoint_hint` deserialize.
//!
//! All tests are deterministic — no network, no hosted graph, no credentials.

use bicameral_api::event_store::EventStoreAdapter;
use bicameral_api::fixtures;
use bicameral_api::staged_preflight::StagedPreflightResponse;
use bicameral_api::tool_request::*;
use bicameral_runtime::dispatch::dispatch;
use bicameral_runtime::event_store_adapters::MemoryAdapter;

fn mem_adapter() -> MemoryAdapter {
    MemoryAdapter::new()
}

// ─── 1. Request shape: checkpoint_hint is accepted ─────────────────────────

#[test]
fn preflight_run_accepts_checkpoint_hint() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run_with_checkpoint_hint();
    let resp = dispatch(&req, &adapter);
    assert_eq!(
        resp.status,
        ToolResponseStatus::Ok,
        "preflight.run with checkpoint_hint must succeed, got: {:?}",
        resp.message
    );
}

#[test]
fn preflight_run_params_with_hint_roundtrip() {
    let params = PreflightRunParams {
        files: vec!["src/lib.rs".into()],
        symbols: vec![],
        diff_context: None,
        branch: None,
        checkpoint_hint: Some(CheckpointHint::MidSession),
        managed_planning: None,
    };
    let json = serde_json::to_string(&params).unwrap();
    let parsed: PreflightRunParams = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.checkpoint_hint, Some(CheckpointHint::MidSession));
    assert_eq!(parsed.files, vec!["src/lib.rs"]);
}

#[test]
fn preflight_run_params_without_hint_roundtrip() {
    let params = PreflightRunParams {
        files: vec![],
        symbols: vec![],
        diff_context: None,
        branch: None,
        checkpoint_hint: None,
        managed_planning: None,
    };
    let json = serde_json::to_string(&params).unwrap();
    let parsed: PreflightRunParams = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.checkpoint_hint, None);
}

// ─── 2. Hint metadata: all variants serialize/deserialize ──────────────────

#[test]
fn all_checkpoint_hint_variants_roundtrip() {
    for hint in [
        CheckpointHint::PreWork,
        CheckpointHint::MidSession,
        CheckpointHint::PreWrite,
        CheckpointHint::ManualLookup,
    ] {
        let json = serde_json::to_string(&hint).unwrap();
        let parsed: CheckpointHint = serde_json::from_str(&json).unwrap();
        assert_eq!(hint, parsed, "roundtrip failed for {json}");
    }
}

#[test]
fn checkpoint_hint_serializes_as_snake_case() {
    assert_eq!(
        serde_json::to_string(&CheckpointHint::PreWork).unwrap(),
        "\"pre_work\""
    );
    assert_eq!(
        serde_json::to_string(&CheckpointHint::MidSession).unwrap(),
        "\"mid_session\""
    );
    assert_eq!(
        serde_json::to_string(&CheckpointHint::PreWrite).unwrap(),
        "\"pre_write\""
    );
    assert_eq!(
        serde_json::to_string(&CheckpointHint::ManualLookup).unwrap(),
        "\"manual_lookup\""
    );
}

// ─── 3. Read-only contract: no mutation regardless of hint ─────────────────

#[test]
fn preflight_with_hint_appends_no_events() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run_with_checkpoint_hint();
    let resp = dispatch(&req, &adapter);
    assert_eq!(resp.status, ToolResponseStatus::Ok);
    let events = adapter.replay().unwrap();
    assert!(
        events.is_empty(),
        "preflight.run with checkpoint_hint must not append events (read-only)"
    );
}

#[test]
fn preflight_with_each_hint_variant_appends_no_events() {
    for hint in [
        CheckpointHint::PreWork,
        CheckpointHint::MidSession,
        CheckpointHint::PreWrite,
        CheckpointHint::ManualLookup,
    ] {
        let adapter = mem_adapter();
        let req = ToolRequest::new(
            ToolCommand::PreflightRun(PreflightRunParams {
                files: vec!["src/main.rs".into()],
                symbols: vec![],
                diff_context: None,
                branch: None,
                checkpoint_hint: Some(hint),
                managed_planning: None,
            }),
            fixtures::authority_mcp("agent-test", "sess-hint-001", "/repo"),
        );
        let resp = dispatch(&req, &adapter);
        assert_eq!(
            resp.status,
            ToolResponseStatus::Ok,
            "preflight.run with {hint:?} must succeed"
        );
        let events = adapter.replay().unwrap();
        assert!(
            events.is_empty(),
            "preflight.run with {hint:?} must not create events"
        );
    }
}

#[test]
fn preflight_lookup_cannot_create_source_snapshot() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run_with_checkpoint_hint();
    let _resp = dispatch(&req, &adapter);
    let events = adapter.replay().unwrap();
    for event in &events {
        let json = serde_json::to_string(&event.payload).unwrap();
        assert!(
            !json.contains("SourceSnapshot"),
            "preflight lookup must not create SourceSnapshot"
        );
    }
}

#[test]
fn preflight_lookup_cannot_create_source_evidence() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run_with_checkpoint_hint();
    let _resp = dispatch(&req, &adapter);
    let events = adapter.replay().unwrap();
    for event in &events {
        let json = serde_json::to_string(&event.payload).unwrap();
        assert!(
            !json.contains("SourceEvidence"),
            "preflight lookup must not create SourceEvidence"
        );
    }
}

#[test]
fn preflight_lookup_cannot_create_decision_candidate() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run_with_checkpoint_hint();
    let _resp = dispatch(&req, &adapter);
    let events = adapter.replay().unwrap();
    for event in &events {
        let json = serde_json::to_string(&event.payload).unwrap();
        assert!(
            !json.contains("DecisionCandidate"),
            "preflight lookup must not create DecisionCandidate"
        );
    }
}

#[test]
fn preflight_lookup_cannot_create_decision() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run_with_checkpoint_hint();
    let _resp = dispatch(&req, &adapter);
    let state = bicameral_runtime::replay::replay_from_adapter(&adapter).unwrap();
    assert!(
        state.decisions.is_empty(),
        "preflight lookup must not create Decision"
    );
}

#[test]
fn preflight_lookup_cannot_create_binding_evidence() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run_with_checkpoint_hint();
    let _resp = dispatch(&req, &adapter);
    let events = adapter.replay().unwrap();
    for event in &events {
        let json = serde_json::to_string(&event.payload).unwrap();
        assert!(
            !json.contains("BindingEvidence"),
            "preflight lookup must not create BindingEvidence"
        );
    }
}

#[test]
fn preflight_lookup_cannot_create_signoff_state() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run_with_checkpoint_hint();
    let _resp = dispatch(&req, &adapter);
    let state = bicameral_runtime::replay::replay_from_adapter(&adapter).unwrap();
    // No decisions means no signoff state changes possible.
    assert!(
        state.decisions.is_empty(),
        "preflight lookup must not create signoff state"
    );
}

#[test]
fn preflight_lookup_cannot_create_compliance_state() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run_with_checkpoint_hint();
    let _resp = dispatch(&req, &adapter);
    let state = bicameral_runtime::replay::replay_from_adapter(&adapter).unwrap();
    assert!(
        state.decisions.is_empty(),
        "preflight lookup must not create compliance state"
    );
}

#[test]
fn preflight_lookup_cannot_create_enforcement_state() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run_with_checkpoint_hint();
    let resp = dispatch(&req, &adapter);
    let result = resp.result.expect("preflight result must be present");
    let staged: StagedPreflightResponse = serde_json::from_value(result["staged"].clone()).unwrap();
    assert_eq!(
        staged.enforcement.status,
        bicameral_api::staged_preflight::EnforcementStatus::NotConfigured,
        "alpha preflight must not create enforcement state"
    );
}

// ─── 4. Hint is echoed in staged response ──────────────────────────────────

#[test]
fn checkpoint_hint_echoed_in_staged_lookup() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run_with_checkpoint_hint();
    let resp = dispatch(&req, &adapter);
    let result = resp.result.expect("preflight result must be present");
    let staged: StagedPreflightResponse = serde_json::from_value(result["staged"].clone()).unwrap();
    assert_eq!(
        staged.lookup.checkpoint_hint,
        Some(CheckpointHint::PreWork),
        "staged response must echo the checkpoint_hint from the request"
    );
}

#[test]
fn no_checkpoint_hint_means_none_in_staged_lookup() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run();
    let resp = dispatch(&req, &adapter);
    let result = resp.result.expect("preflight result must be present");
    let staged: StagedPreflightResponse = serde_json::from_value(result["staged"].clone()).unwrap();
    assert_eq!(
        staged.lookup.checkpoint_hint, None,
        "staged response must have no checkpoint_hint when request omits it"
    );
}

#[test]
fn checkpoint_hint_none_omitted_from_json() {
    let staged = StagedPreflightResponse::alpha_from_lookup(vec![], vec![], None);
    let v: serde_json::Value = serde_json::to_value(&staged).unwrap();
    assert!(
        v["lookup"].get("checkpoint_hint").is_none(),
        "checkpoint_hint: None must be omitted from serialized JSON"
    );
}

#[test]
fn checkpoint_hint_present_in_json_when_set() {
    let staged =
        StagedPreflightResponse::alpha_from_lookup(vec![], vec![], Some(CheckpointHint::PreWrite));
    let v: serde_json::Value = serde_json::to_value(&staged).unwrap();
    assert_eq!(
        v["lookup"]["checkpoint_hint"], "pre_write",
        "checkpoint_hint must serialize when present"
    );
}

// ─── 5. Identical behavior regardless of hint ──────────────────────────────

#[test]
fn preflight_results_identical_with_and_without_hint() {
    let adapter = mem_adapter();

    // Run without hint.
    let req_no_hint = ToolRequest::new(
        ToolCommand::PreflightRun(PreflightRunParams {
            files: vec!["src/main.rs".into()],
            symbols: vec![],
            diff_context: None,
            branch: None,
            checkpoint_hint: None,
            managed_planning: None,
        }),
        fixtures::authority_mcp("agent-test", "sess-cmp-001", "/repo"),
    );
    let resp_no_hint = dispatch(&req_no_hint, &adapter);

    // Run with hint.
    let req_with_hint = ToolRequest::new(
        ToolCommand::PreflightRun(PreflightRunParams {
            files: vec!["src/main.rs".into()],
            symbols: vec![],
            diff_context: None,
            branch: None,
            checkpoint_hint: Some(CheckpointHint::MidSession),
            managed_planning: None,
        }),
        fixtures::authority_mcp("agent-test", "sess-cmp-002", "/repo"),
    );
    let resp_with_hint = dispatch(&req_with_hint, &adapter);

    // Both must succeed.
    assert_eq!(resp_no_hint.status, ToolResponseStatus::Ok);
    assert_eq!(resp_with_hint.status, ToolResponseStatus::Ok);

    let result_no = resp_no_hint.result.unwrap();
    let result_with = resp_with_hint.result.unwrap();

    // Core preflight fields must match (decisions, candidates, readiness).
    assert_eq!(
        result_no["relevant_decisions"],
        result_with["relevant_decisions"]
    );
    assert_eq!(
        result_no["relevant_candidates"],
        result_with["relevant_candidates"]
    );
    assert_eq!(result_no["readiness"], result_with["readiness"]);

    // Staged response fields match except for the echoed hint.
    let staged_no: StagedPreflightResponse =
        serde_json::from_value(result_no["staged"].clone()).unwrap();
    let staged_with: StagedPreflightResponse =
        serde_json::from_value(result_with["staged"].clone()).unwrap();

    assert_eq!(staged_no.capture, staged_with.capture);
    assert_eq!(staged_no.projection, staged_with.projection);
    assert_eq!(staged_no.enforcement, staged_with.enforcement);
    assert_eq!(staged_no.session_directive, staged_with.session_directive);
    assert_eq!(staged_no.lookup.status, staged_with.lookup.status);
    assert_eq!(
        staged_no.lookup.availability,
        staged_with.lookup.availability
    );
    assert_eq!(
        staged_no.lookup.decision_refs,
        staged_with.lookup.decision_refs
    );
    assert_eq!(staged_no.lookup.limitations, staged_with.lookup.limitations);
}

// ─── 6. Backward compatibility ─────────────────────────────────────────────

#[test]
fn legacy_request_without_checkpoint_hint_deserializes() {
    let json = r#"{
        "files": ["src/main.rs"],
        "symbols": [],
        "diff_context": null,
        "branch": null
    }"#;
    let params: PreflightRunParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.checkpoint_hint, None);
    assert_eq!(params.files, vec!["src/main.rs"]);
}

#[test]
fn request_with_checkpoint_hint_deserializes() {
    let json = r#"{
        "files": [],
        "symbols": [],
        "checkpoint_hint": "pre_work"
    }"#;
    let params: PreflightRunParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.checkpoint_hint, Some(CheckpointHint::PreWork));
}

#[test]
fn tool_command_preflight_with_hint_roundtrip() {
    let cmd = ToolCommand::PreflightRun(PreflightRunParams {
        files: vec!["src/api.rs".into()],
        symbols: vec!["handle_request".into()],
        diff_context: None,
        branch: Some("feature/lookup".into()),
        checkpoint_hint: Some(CheckpointHint::ManualLookup),
        managed_planning: None,
    });
    let json = serde_json::to_string_pretty(&cmd).unwrap();
    let parsed: ToolCommand = serde_json::from_str(&json).unwrap();
    match parsed {
        ToolCommand::PreflightRun(p) => {
            assert_eq!(p.checkpoint_hint, Some(CheckpointHint::ManualLookup));
            assert_eq!(p.files, vec!["src/api.rs"]);
            assert_eq!(p.symbols, vec!["handle_request"]);
            assert_eq!(p.branch, Some("feature/lookup".into()));
        }
        _ => panic!("expected PreflightRun"),
    }
}

#[test]
fn staged_response_without_hint_matches_legacy_protocol_fixture() {
    let staged = fixtures::staged_preflight_response();
    let json = serde_json::to_value(&staged).unwrap();

    let protocol_fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../protocol/schemas/v2/fixtures/staged-preflight-response.json"
    ))
    .unwrap();

    let expected = {
        let mut v = protocol_fixture.clone();
        if let Some(obj) = v.as_object_mut() {
            obj.remove("_comment");
            obj.remove("_expected");
        }
        v
    };

    assert_eq!(
        json, expected,
        "staged response without hint must match legacy protocol fixture"
    );
}

// ─── 7. Staged response with hint fixture ──────────────────────────────────

#[test]
fn staged_response_with_hint_fixture_roundtrip() {
    let staged = fixtures::staged_preflight_response_with_hint();
    let json = serde_json::to_string_pretty(&staged).unwrap();
    let parsed: StagedPreflightResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(staged, parsed);
    assert_eq!(staged.lookup.checkpoint_hint, Some(CheckpointHint::PreWork));
}
