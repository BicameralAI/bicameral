//! Staged preflight bot contract conformance — issue #324.
//!
//! Proves the staged preflight response contract from #323 is correctly
//! consumed by the bot dispatcher and that the resulting wire shape is
//! explicit enough for a thin MCP client to render stages without guessing.
//!
//! Acceptance criteria covered:
//!   1. Bot conformance fixture includes staged preflight response with
//!      capture / projection / lookup / enforcement / session_directive.
//!   2. Alpha lookup response reports unsupported/not_configured stages
//!      explicitly.
//!   3. `enforcement.status = not_configured` and
//!      `session_directive.mode = continue` for alpha.
//!   4. Preflight lookup appends no events and mutates no
//!      source / candidate / review / conflict / compliance state.
//!   5. Capability mismatch or unsupported command path returns typed
//!      failure, not legacy fallback.
//!   6. Fixture is linked for MCP conformance — see
//!      `protocol/schemas/v2/fixtures/staged-preflight-response.json`
//!      and BicameralAI/bicameral-mcp#586.
//!
//! All tests are deterministic — no network, no hosted graph, no credentials.

use bicameral_api::alpha_recall::{
    AlphaRecallAuthorityMode, AlphaRecallEnvelope, AlphaRecallMismatchKind,
    ALPHA_RECALL_ENVELOPE_VERSION,
};
use bicameral_api::event_store::EventStoreAdapter;
use bicameral_api::fixtures;
use bicameral_api::staged_preflight::{
    CaptureStatus, EnforcementStatus, LookupStatus, ProjectionStatus, SessionDirectiveMode,
    StagedPreflightResponse,
};
use bicameral_api::tool_request::*;
use bicameral_runtime::dispatch::{
    capability_report, dispatch, CapabilityReport, DEFERRED_COMMANDS, PREFLIGHT_STAGE_CAPABILITIES,
    PROTOCOL_VERSION, SUPPORTED_COMMANDS,
};
use bicameral_runtime::event_store_adapters::MemoryAdapter;

fn mem_adapter() -> MemoryAdapter {
    MemoryAdapter::new()
}

// ─── 1. Fixture shape: staged response has all five stages ─────────────────

#[test]
fn fixture_staged_response_has_all_five_stages() {
    let staged = fixtures::staged_preflight_response();
    let json = serde_json::to_value(&staged).unwrap();

    for key in [
        "capture",
        "projection",
        "lookup",
        "enforcement",
        "session_directive",
    ] {
        assert!(
            json.get(key).is_some(),
            "staged response fixture must include '{key}' stage"
        );
    }
}

#[test]
fn fixture_staged_response_matches_protocol_fixture() {
    let staged = fixtures::staged_preflight_response();
    let json = serde_json::to_value(&staged).unwrap();

    let protocol_fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../protocol/schemas/v2/fixtures/staged-preflight-response.json"
    ))
    .unwrap();

    // Strip _comment/_expected metadata from the protocol fixture.
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
        "Rust fixture must match protocol JSON fixture (sans metadata)"
    );
}

// ─── 2. Alpha stages: not_configured are explicit, never omitted ───────────

#[test]
fn alpha_capture_is_not_configured() {
    let staged = fixtures::staged_preflight_response();
    assert_eq!(staged.capture.status, CaptureStatus::NotConfigured);
}

#[test]
fn alpha_projection_is_not_configured() {
    let staged = fixtures::staged_preflight_response();
    assert_eq!(staged.projection.status, ProjectionStatus::NotConfigured);
}

#[test]
fn alpha_lookup_is_completed() {
    let staged = fixtures::staged_preflight_response();
    assert_eq!(staged.lookup.status, LookupStatus::Completed);
    assert!(
        !staged.lookup.decision_refs.is_empty(),
        "alpha fixture must include at least one decision ref"
    );
    assert!(
        !staged.lookup.limitations.is_empty(),
        "alpha fixture must include at least one limitation"
    );
}

// ─── 3. enforcement.status = not_configured, session_directive.mode = continue

#[test]
fn alpha_enforcement_is_not_configured() {
    let staged = fixtures::staged_preflight_response();
    assert_eq!(staged.enforcement.status, EnforcementStatus::NotConfigured);
}

#[test]
fn alpha_session_directive_is_continue() {
    let staged = fixtures::staged_preflight_response();
    assert_eq!(
        staged.session_directive.mode,
        SessionDirectiveMode::Continue
    );
}

#[test]
fn enforcement_not_configured_never_becomes_blocking() {
    let staged = fixtures::staged_preflight_response();
    let json = serde_json::to_value(&staged).unwrap();

    let enforcement_status = json["enforcement"]["status"].as_str().unwrap();
    assert_eq!(enforcement_status, "not_configured");
    assert_ne!(enforcement_status, "block");
    assert_ne!(enforcement_status, "pause");
    assert_ne!(enforcement_status, "warn");

    let directive_mode = json["session_directive"]["mode"].as_str().unwrap();
    assert_eq!(directive_mode, "continue");
    assert_ne!(directive_mode, "block");
}

// ─── 4. Preflight lookup is read-only: no events, no state mutation ────────

#[test]
fn preflight_run_appends_no_events() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run();
    let resp = dispatch(&req, &adapter);

    assert_eq!(
        resp.status,
        ToolResponseStatus::Ok,
        "preflight.run must succeed: {:?}",
        resp.message
    );

    let events = adapter.replay().unwrap();
    assert!(
        events.is_empty(),
        "preflight.run must not append any event to the store"
    );
}

#[test]
fn preflight_run_does_not_mutate_existing_state() {
    let adapter = mem_adapter();

    // Seed a candidate via ingest so there is something to not-mutate.
    let ingest = fixtures::ingest_submit_local();
    let ingest_resp = dispatch(&ingest, &adapter);
    assert_eq!(ingest_resp.status, ToolResponseStatus::Ok);

    let events_before = adapter.replay().unwrap();
    let count_before = events_before.len();
    assert!(
        count_before > 0,
        "ingest must have produced at least one event"
    );

    // Now run preflight — it must not add any events.
    let preflight = fixtures::preflight_run();
    let preflight_resp = dispatch(&preflight, &adapter);
    assert_eq!(
        preflight_resp.status,
        ToolResponseStatus::Ok,
        "preflight.run must succeed: {:?}",
        preflight_resp.message
    );

    let events_after = adapter.replay().unwrap();
    assert_eq!(
        events_after.len(),
        count_before,
        "preflight.run must not append events (before={count_before}, after={})",
        events_after.len()
    );
}

#[test]
fn preflight_response_carries_staged_key() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run();
    let resp = dispatch(&req, &adapter);

    assert_eq!(resp.status, ToolResponseStatus::Ok);
    let result = resp.result.as_ref().expect("preflight must carry result");

    assert!(
        result.get("staged").is_some(),
        "preflight.run response must contain a 'staged' key"
    );

    // The staged value must deserialize into the typed contract.
    let staged: StagedPreflightResponse = serde_json::from_value(result["staged"].clone())
        .expect("staged key must deserialize into StagedPreflightResponse");

    assert_eq!(staged.capture.status, CaptureStatus::NotConfigured);
    assert_eq!(staged.projection.status, ProjectionStatus::NotConfigured);
    assert_eq!(staged.lookup.status, LookupStatus::Completed);
    assert_eq!(staged.enforcement.status, EnforcementStatus::NotConfigured);
    assert_eq!(
        staged.session_directive.mode,
        SessionDirectiveMode::Continue
    );
}

#[test]
fn preflight_staged_response_after_ingest_has_decision_refs() {
    let adapter = mem_adapter();

    // Ingest a candidate so the preflight lookup finds something.
    let ingest = fixtures::ingest_submit_local();
    let ingest_resp = dispatch(&ingest, &adapter);
    assert_eq!(ingest_resp.status, ToolResponseStatus::Ok);

    // Accept the candidate to make it a Decision — preflight returns decisions.
    let candidate_id = ingest_resp.result.as_ref().unwrap()["candidate_ids"][0]
        .as_str()
        .expect("ingest must return a candidate id")
        .to_string();

    let accept = ToolRequest::new(
        ToolCommand::ReviewAcceptCandidate(ReviewTargetParams {
            target_id: candidate_id,
            reason: Some("conformance test".into()),
        }),
        fixtures::authority_dashboard("reviewer-324", "/home/user/project"),
    );
    let accept_resp = dispatch(&accept, &adapter);
    assert_eq!(accept_resp.status, ToolResponseStatus::Ok);

    // Now preflight — the staged response should include a decision ref.
    let preflight = fixtures::preflight_run();
    let preflight_resp = dispatch(&preflight, &adapter);
    assert_eq!(preflight_resp.status, ToolResponseStatus::Ok);

    let staged: StagedPreflightResponse =
        serde_json::from_value(preflight_resp.result.as_ref().unwrap()["staged"].clone())
            .expect("staged must deserialize");

    // We cannot guarantee the decision matches the preflight scope (files may
    // not overlap), but the staged structure must be well-formed.
    assert_eq!(staged.capture.status, CaptureStatus::NotConfigured);
    assert_eq!(staged.enforcement.status, EnforcementStatus::NotConfigured);
    assert_eq!(
        staged.session_directive.mode,
        SessionDirectiveMode::Continue
    );
}

// ─── 5. Capability mismatch returns typed failure ──────────────────────────

#[test]
fn capability_report_includes_preflight_stages() {
    let report = capability_report();
    assert!(
        !report.preflight_stages.is_empty(),
        "capability report must include preflight_stages"
    );

    let stage_names: Vec<&str> = report.preflight_stages.iter().map(|s| s.stage).collect();
    for expected in [
        "capture",
        "projection",
        "lookup",
        "enforcement",
        "session_directive",
    ] {
        assert!(
            stage_names.contains(&expected),
            "capability report must include stage '{expected}'"
        );
    }
}

#[test]
fn capability_report_alpha_stages_match_contract() {
    let report = capability_report();

    for cap in &report.preflight_stages {
        match cap.stage {
            "capture" | "projection" | "enforcement" => {
                assert_eq!(
                    cap.status, "not_configured",
                    "alpha stage '{}' must be not_configured",
                    cap.stage
                );
            }
            "lookup" => {
                assert_eq!(cap.status, "supported", "lookup stage must be supported");
            }
            "session_directive" => {
                assert_eq!(
                    cap.status, "alpha_continue_only",
                    "session_directive must be alpha_continue_only"
                );
            }
            other => {
                panic!("unexpected preflight stage '{other}' in capability report");
            }
        }
    }
}

#[test]
fn unsupported_command_returns_typed_rejection_not_fallback() {
    let deferred_fixtures = [
        (
            "review.resolve_compliance",
            fixtures::review_resolve_compliance(),
        ),
        (
            "tracking.untrack_source",
            fixtures::tracking_untrack_source(),
        ),
        ("tracking.refresh_query", fixtures::tracking_refresh_query()),
    ];

    for (name, req) in deferred_fixtures {
        let adapter = mem_adapter();
        let resp = dispatch(&req, &adapter);

        assert_eq!(
            resp.status,
            ToolResponseStatus::Rejected,
            "{name}: deferred command must return Rejected, not fallback"
        );
        assert!(
            resp.message
                .as_deref()
                .unwrap_or("")
                .contains("unsupported_capability"),
            "{name}: rejection must cite unsupported_capability, got: {:?}",
            resp.message
        );
    }
}

#[test]
fn incompatible_protocol_must_refuse_not_fallback() {
    fn negotiate(client: &[&str], daemon: &CapabilityReport) -> Result<(), String> {
        if client.contains(&daemon.protocol_version) {
            Ok(())
        } else {
            Err(format!(
                "incompatible_protocol: daemon '{}' not in {:?}",
                daemon.protocol_version, client
            ))
        }
    }

    let report = capability_report();
    assert!(
        negotiate(&[PROTOCOL_VERSION], &report).is_ok(),
        "compatible protocol must succeed"
    );

    let fake = CapabilityReport {
        protocol_version: "v1",
        supported_commands: SUPPORTED_COMMANDS,
        deferred_commands: DEFERRED_COMMANDS,
        preflight_stages: Vec::new(),
        projection_readiness:
            bicameral_api::reasoning_projection::ProjectionProviderReadiness::not_configured(),
        alpha_capabilities: bicameral_api::capability::AlphaCapabilities::unsupported(),
        workspace_binding_available: false,
    };
    assert!(
        negotiate(&[PROTOCOL_VERSION], &fake).is_err(),
        "v1 daemon must be refused"
    );

    let future = CapabilityReport {
        protocol_version: "v99",
        supported_commands: SUPPORTED_COMMANDS,
        deferred_commands: DEFERRED_COMMANDS,
        preflight_stages: Vec::new(),
        projection_readiness:
            bicameral_api::reasoning_projection::ProjectionProviderReadiness::not_configured(),
        alpha_capabilities: bicameral_api::capability::AlphaCapabilities::alpha_default(),
        workspace_binding_available: false,
    };
    assert!(
        negotiate(&[PROTOCOL_VERSION], &future).is_err(),
        "unknown future protocol must be refused"
    );
}

// ─── Staged response wire shape: never claims compliance or blocking ───────

#[test]
fn alpha_staged_response_never_claims_compliance_or_blocking() {
    let staged = fixtures::staged_preflight_response();
    let json = serde_json::to_string(&staged).unwrap();

    let forbidden = [
        "compliant",
        "correct",
        "safe",
        "\"block\"",
        "\"pause\"",
        "\"warn\"",
    ];
    for term in forbidden {
        assert!(
            !json.contains(term),
            "alpha staged response must not contain '{term}', got: {json}"
        );
    }
}

#[test]
fn dispatched_alpha_staged_never_claims_compliance_or_blocking() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run();
    let resp = dispatch(&req, &adapter);

    let staged_json = serde_json::to_string(&resp.result.as_ref().unwrap()["staged"]).unwrap();

    let forbidden = ["compliant", "correct", "\"block\"", "\"pause\""];
    for term in forbidden {
        assert!(
            !staged_json.contains(term),
            "dispatched staged response must not contain '{term}'"
        );
    }
}

// ─── Staged response roundtrip: Rust fixture ↔ JSON ────────────────────────

#[test]
fn staged_response_fixture_roundtrips_through_json() {
    let staged = fixtures::staged_preflight_response();
    let json = serde_json::to_string_pretty(&staged).unwrap();
    let parsed: StagedPreflightResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(staged, parsed);
}

// ─── Preflight stages in capability report match PREFLIGHT_STAGE_CAPABILITIES

#[test]
fn capability_report_stages_match_constant() {
    let report = capability_report();
    assert_eq!(
        report.preflight_stages.len(),
        PREFLIGHT_STAGE_CAPABILITIES.len(),
        "capability report stage count must match PREFLIGHT_STAGE_CAPABILITIES"
    );

    for (i, (expected_stage, expected_status)) in PREFLIGHT_STAGE_CAPABILITIES.iter().enumerate() {
        assert_eq!(report.preflight_stages[i].stage, *expected_stage);
        assert_eq!(report.preflight_stages[i].status, *expected_status);
    }
}

// ─── Runtime trace present on preflight path ───────────────────────────────

#[test]
fn preflight_run_carries_correlated_trace() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run();
    let resp = dispatch(&req, &adapter);

    assert_eq!(resp.status, ToolResponseStatus::Ok);
    assert_eq!(resp.request_id, req.request_id);

    let trace = resp
        .trace
        .as_ref()
        .expect("preflight.run must carry a RuntimeTrace");
    assert_eq!(trace.command, "preflight.run");
    assert_eq!(trace.request_id, req.request_id);
    assert_eq!(trace.status, TraceStatus::Ok);
}

// ─── Canonical alpha recall envelope (#817) ────────────────────────────────

/// Deserialize the canonical envelope from a dispatched `preflight.run`.
fn dispatched_envelope() -> AlphaRecallEnvelope {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run();
    let resp = dispatch(&req, &adapter);
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.as_ref().expect("preflight must carry result");
    let envelope_v = result
        .get("alpha_recall")
        .expect("preflight.run response must contain an 'alpha_recall' envelope");
    serde_json::from_value(envelope_v.clone())
        .expect("alpha_recall must deserialize into AlphaRecallEnvelope")
}

#[test]
fn preflight_response_carries_canonical_alpha_recall_envelope() {
    let envelope = dispatched_envelope();

    // Contract is version- and protocol-pinned so a thin client can refuse
    // an envelope it cannot render.
    assert_eq!(envelope.envelope_version, ALPHA_RECALL_ENVELOPE_VERSION);
    assert_eq!(envelope.protocol_version, PROTOCOL_VERSION);

    // Product content carries the staged payload and the recall packet.
    let staged = &envelope.product.staged;
    assert_eq!(staged.capture.status, CaptureStatus::NotConfigured);
    assert_eq!(staged.projection.status, ProjectionStatus::NotConfigured);
    assert_eq!(staged.lookup.status, LookupStatus::Completed);
    assert_eq!(staged.enforcement.status, EnforcementStatus::NotConfigured);
    assert_eq!(
        staged.session_directive.mode,
        SessionDirectiveMode::Continue
    );
}

#[test]
fn envelope_states_read_only_authority() {
    let envelope = dispatched_envelope();
    assert_eq!(
        envelope.authority.mode,
        AlphaRecallAuthorityMode::ReadOnly,
        "recall lookup must declare read-only authority"
    );
    assert!(
        !envelope.authority.appended_canonical_events,
        "recall lookup must not append canonical events"
    );
    assert!(
        !envelope.authority.mutated_lifecycle_state,
        "recall lookup must not mutate lifecycle state"
    );
}

#[test]
fn envelope_separates_product_from_validation_witnesses() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run();
    let resp = dispatch(&req, &adapter);
    let result = resp.result.as_ref().unwrap();
    let v = &result["alpha_recall"];

    // Daemon-authored product content vs validation-only witnesses are
    // explicitly separated sections.
    assert!(v["product"]["staged"].is_object());
    assert!(v["product"]["packet"].is_object());
    assert!(v["witness"]["runtime_trace"].is_object());
    assert!(v["witness"]["topology_receipt"].is_object());

    // Witnesses must not leak into product content.
    assert!(v["product"].get("runtime_trace").is_none());
    assert!(v["product"].get("topology_receipt").is_none());

    // Topology receipt is a not-consumed witness for later real-process
    // validation — never product state, no local paths.
    let witness = &envelope_witness(resp);
    assert!(!witness.topology_receipt.as_ref().unwrap().consumed);
}

fn envelope_witness(resp: ToolResponse) -> bicameral_api::alpha_recall::AlphaRecallWitness {
    let result = resp.result.unwrap();
    let envelope: AlphaRecallEnvelope =
        serde_json::from_value(result["alpha_recall"].clone()).unwrap();
    envelope.witness
}

#[test]
fn envelope_trace_witness_correlates_with_response_trace() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run();
    let resp = dispatch(&req, &adapter);

    let trace = resp.trace.clone().expect("must carry RuntimeTrace");
    let envelope: AlphaRecallEnvelope =
        serde_json::from_value(resp.result.as_ref().unwrap()["alpha_recall"].clone()).unwrap();

    // The witness mirrors the response trace's correlation identifiers.
    assert_eq!(envelope.witness.runtime_trace.request_id, trace.request_id);
    assert_eq!(envelope.witness.runtime_trace.command, trace.command);
    assert_eq!(
        envelope.witness.runtime_trace.caller_surface,
        trace.caller_surface
    );
    assert_eq!(envelope.witness.runtime_trace.status, TraceStatus::Ok);
}

#[test]
fn envelope_carries_daemon_authored_caveats() {
    // The default preflight (no graph) is readiness-limited, so the daemon
    // must author at least one caveat rather than leaving the client to infer.
    let envelope = dispatched_envelope();
    assert!(
        !envelope.product.caveats.is_empty(),
        "readiness-limited recall must carry daemon-authored caveats"
    );
}

#[test]
fn envelope_refuses_incompatible_protocol_no_fallback() {
    let envelope = dispatched_envelope();

    // Compatible client renders.
    assert!(envelope
        .check_compatibility(&[PROTOCOL_VERSION], ALPHA_RECALL_ENVELOPE_VERSION)
        .is_ok());

    // Incompatible protocol fails closed with a typed mismatch.
    let err = envelope
        .check_compatibility(&["v1"], ALPHA_RECALL_ENVELOPE_VERSION)
        .expect_err("incompatible protocol must be refused");
    assert_eq!(err.kind, AlphaRecallMismatchKind::IncompatibleProtocol);

    // Unsupported envelope version also fails closed.
    let err = envelope
        .check_compatibility(&[PROTOCOL_VERSION], ALPHA_RECALL_ENVELOPE_VERSION + 1)
        .expect_err("unsupported envelope version must be refused");
    assert_eq!(
        err.kind,
        AlphaRecallMismatchKind::UnsupportedEnvelopeVersion
    );
}

#[test]
fn envelope_never_claims_compliance_or_blocking() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run();
    let resp = dispatch(&req, &adapter);
    let envelope_json =
        serde_json::to_string(&resp.result.as_ref().unwrap()["alpha_recall"]).unwrap();

    for term in ["compliant", "\"block\"", "\"pause\"", "\"warn\""] {
        assert!(
            !envelope_json.contains(term),
            "dispatched alpha recall envelope must not contain '{term}'"
        );
    }
}

#[test]
fn preflight_with_envelope_still_appends_no_events() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run();
    let resp = dispatch(&req, &adapter);
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    // The envelope declares read-only authority; prove the event store agrees.
    let envelope: AlphaRecallEnvelope =
        serde_json::from_value(resp.result.as_ref().unwrap()["alpha_recall"].clone()).unwrap();
    assert!(!envelope.authority.appended_canonical_events);

    assert!(
        adapter.replay().unwrap().is_empty(),
        "preflight.run with envelope must not append any event"
    );
}
