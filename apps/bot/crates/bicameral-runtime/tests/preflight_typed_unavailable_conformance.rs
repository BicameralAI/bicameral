//! Typed unavailable/deferred/readiness-limited preflight conformance — issue #393.
//!
//! Proves the alpha preflight cut line: `preflight.run` returns a clear typed
//! response when code-aware or enforcement-like behavior is unavailable, and
//! any available source-only/search output remains bot-authored and read-only.
//!
//! Acceptance criteria covered:
//!   1. `preflight.run` returns typed unsupported/deferred/readiness-limited
//!      state for unavailable alpha capability.
//!   2. Response does not imply compliance, blocking authority, merge safety,
//!      or global no-conflict proof.
//!   3. MCP/dashboard/API surfaces render the daemon-authored response rather
//!      than deriving their own applicability verdict (availability field is
//!      always present and typed).
//!   4. Existing search/history behavior remains available for accepted
//!      decisions.
//!   5. Tests cover typed unavailable response and no ledger/review/signoff
//!      mutation.
//!
//! All tests are deterministic — no network, no hosted graph, no credentials.

use bicameral_api::event_store::EventStoreAdapter;
use bicameral_api::fixtures;
use bicameral_api::staged_preflight::{
    CaptureStatus, EnforcementStatus, LookupStatus, PreflightAvailability, ProjectionStatus,
    SessionDirectiveMode, StagedPreflightResponse,
};
use bicameral_api::tool_request::*;
use bicameral_runtime::dispatch::{dispatch, DEFERRED_COMMANDS};
use bicameral_runtime::event_store_adapters::MemoryAdapter;

fn mem_adapter() -> MemoryAdapter {
    MemoryAdapter::new()
}

// ─── 1. Typed availability is always present in staged response ────────────

#[test]
fn preflight_staged_response_carries_availability_on_all_stages() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run();
    let resp = dispatch(&req, &adapter);

    assert_eq!(resp.status, ToolResponseStatus::Ok);
    let result = resp.result.as_ref().expect("preflight must carry result");
    let staged_v = &result["staged"];

    // Every stage must have an `availability` field.
    for stage in [
        "capture",
        "projection",
        "lookup",
        "enforcement",
        "session_directive",
    ] {
        assert!(
            staged_v[stage].get("availability").is_some(),
            "staged.{stage} must include 'availability' field"
        );
    }
}

#[test]
fn preflight_alpha_availability_is_typed_not_inferred() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run();
    let resp = dispatch(&req, &adapter);

    let staged: StagedPreflightResponse =
        serde_json::from_value(resp.result.as_ref().unwrap()["staged"].clone())
            .expect("staged must deserialize");

    // Unsupported stages are explicitly typed.
    assert_eq!(
        staged.capture.availability,
        PreflightAvailability::Unsupported
    );
    assert_eq!(
        staged.enforcement.availability,
        PreflightAvailability::Unsupported
    );

    // Deferred stages are explicitly typed.
    assert_eq!(
        staged.projection.availability,
        PreflightAvailability::Deferred
    );

    // Session directive is always available in alpha.
    assert_eq!(
        staged.session_directive.availability,
        PreflightAvailability::Available
    );
}

#[test]
fn preflight_lookup_availability_reflects_readiness() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run();
    let resp = dispatch(&req, &adapter);

    let staged: StagedPreflightResponse =
        serde_json::from_value(resp.result.as_ref().unwrap()["staged"].clone())
            .expect("staged must deserialize");

    // Without workspace/graph, lookup is readiness-limited (source-only).
    assert_eq!(
        staged.lookup.availability,
        PreflightAvailability::ReadinessLimited
    );
    // Lookup still completes — source-only results are returned.
    assert_eq!(staged.lookup.status, LookupStatus::Completed);
}

// ─── 2. Response never implies compliance, blocking, merge safety ──────────

#[test]
fn typed_unavailable_response_never_implies_compliance() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run();
    let resp = dispatch(&req, &adapter);

    let staged_json = serde_json::to_string(&resp.result.as_ref().unwrap()["staged"]).unwrap();

    let forbidden = [
        "compliant",
        "correct",
        "safe",
        "merge_safe",
        "no_conflict",
        "\"block\"",
        "\"pause\"",
    ];
    for term in forbidden {
        assert!(
            !staged_json.contains(term),
            "typed unavailable response must not contain '{term}', got: {staged_json}"
        );
    }
}

#[test]
fn typed_unavailable_never_claims_blocking_authority() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run();
    let resp = dispatch(&req, &adapter);

    let staged: StagedPreflightResponse =
        serde_json::from_value(resp.result.as_ref().unwrap()["staged"].clone())
            .expect("staged must deserialize");

    // Enforcement is unsupported — it must not produce block/pause/warn.
    assert_eq!(staged.enforcement.status, EnforcementStatus::NotConfigured);
    assert_eq!(
        staged.enforcement.availability,
        PreflightAvailability::Unsupported
    );

    // Session directive must not produce block/pause/warn.
    assert_eq!(
        staged.session_directive.mode,
        SessionDirectiveMode::Continue
    );
}

// ─── 3. Surfaces render daemon-authored response (availability is present) ─

#[test]
fn availability_field_serializes_as_snake_case_string() {
    let staged = StagedPreflightResponse::alpha_from_lookup(vec![], vec![], None);
    let v: serde_json::Value = serde_json::to_value(&staged).unwrap();

    // Check all availability fields serialize to expected string values.
    assert_eq!(v["capture"]["availability"], "unsupported");
    assert_eq!(v["projection"]["availability"], "deferred");
    assert_eq!(v["lookup"]["availability"], "available");
    assert_eq!(v["enforcement"]["availability"], "unsupported");
    assert_eq!(v["session_directive"]["availability"], "available");
}

#[test]
fn readiness_limited_availability_serializes_correctly() {
    let staged = StagedPreflightResponse::alpha_readiness_limited(
        vec!["dec-001".into()],
        vec!["graph: stale".into()],
        None,
    );
    let v: serde_json::Value = serde_json::to_value(&staged).unwrap();

    assert_eq!(v["lookup"]["availability"], "readiness_limited");
    assert_eq!(v["lookup"]["status"], "completed");
}

// ─── 4. Existing search/history behavior remains available ─────────────────

#[test]
fn search_query_still_works_after_preflight_changes() {
    let adapter = mem_adapter();

    // Ingest a candidate.
    let ingest = fixtures::ingest_submit_local();
    let ingest_resp = dispatch(&ingest, &adapter);
    assert_eq!(ingest_resp.status, ToolResponseStatus::Ok);

    // Search must still work.
    let search = fixtures::search_query();
    let search_resp = dispatch(&search, &adapter);
    assert_eq!(
        search_resp.status,
        ToolResponseStatus::Ok,
        "search.query must remain functional: {:?}",
        search_resp.message
    );
}

#[test]
fn history_list_still_works_after_preflight_changes() {
    let adapter = mem_adapter();

    // Ingest a candidate.
    let ingest = fixtures::ingest_submit_local();
    let ingest_resp = dispatch(&ingest, &adapter);
    assert_eq!(ingest_resp.status, ToolResponseStatus::Ok);

    // Accept it to create a decision with history.
    let candidate_id = ingest_resp.result.as_ref().unwrap()["candidate_ids"][0]
        .as_str()
        .expect("ingest must return candidate id")
        .to_string();

    let accept = ToolRequest::new(
        ToolCommand::ReviewAcceptCandidate(ReviewTargetParams {
            target_id: candidate_id,
            reason: Some("conformance".into()),
        }),
        fixtures::authority_dashboard("reviewer-393", "/home/user/project"),
    );
    let accept_resp = dispatch(&accept, &adapter);
    assert_eq!(accept_resp.status, ToolResponseStatus::Ok);

    // History must still work.
    let history = fixtures::history_list();
    let history_resp = dispatch(&history, &adapter);
    assert_eq!(
        history_resp.status,
        ToolResponseStatus::Ok,
        "history.list must remain functional: {:?}",
        history_resp.message
    );
}

// ─── 5. No ledger/review/signoff mutation from preflight ───────────────────

#[test]
fn preflight_typed_unavailable_appends_no_events() {
    let adapter = mem_adapter();
    let req = fixtures::preflight_run();
    let resp = dispatch(&req, &adapter);

    assert_eq!(resp.status, ToolResponseStatus::Ok);
    let events = adapter.replay().unwrap();
    assert!(
        events.is_empty(),
        "preflight.run with typed unavailable must not append events"
    );
}

#[test]
fn preflight_does_not_mutate_review_or_signoff_state() {
    let adapter = mem_adapter();

    // Ingest + accept to create decision.
    let ingest = fixtures::ingest_submit_local();
    let ingest_resp = dispatch(&ingest, &adapter);
    assert_eq!(ingest_resp.status, ToolResponseStatus::Ok);

    let candidate_id = ingest_resp.result.as_ref().unwrap()["candidate_ids"][0]
        .as_str()
        .unwrap()
        .to_string();

    let accept = ToolRequest::new(
        ToolCommand::ReviewAcceptCandidate(ReviewTargetParams {
            target_id: candidate_id,
            reason: Some("conformance".into()),
        }),
        fixtures::authority_dashboard("reviewer-393", "/home/user/project"),
    );
    dispatch(&accept, &adapter);

    let events_before = adapter.replay().unwrap();
    let count_before = events_before.len();

    // Run preflight — it must not add any events or mutate state.
    let preflight = fixtures::preflight_run();
    let preflight_resp = dispatch(&preflight, &adapter);
    assert_eq!(preflight_resp.status, ToolResponseStatus::Ok);

    let events_after = adapter.replay().unwrap();
    assert_eq!(
        events_after.len(),
        count_before,
        "preflight must not mutate ledger/review/signoff (events before={count_before}, after={})",
        events_after.len()
    );
}

#[test]
fn deferred_commands_still_return_typed_rejection() {
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
            "{name}: deferred command must return Rejected"
        );
        assert!(
            resp.message
                .as_deref()
                .unwrap_or("")
                .contains("unsupported_capability"),
            "{name}: rejection must cite unsupported_capability"
        );

        // Must not append events.
        let events = adapter.replay().unwrap();
        assert!(
            events.is_empty(),
            "{name}: deferred command must not append events"
        );
    }
}

// ─── Wire shape: availability classification is exhaustive ─────────────────

#[test]
fn all_four_availability_variants_serialize_distinctly() {
    let variants = [
        (PreflightAvailability::Available, "\"available\""),
        (PreflightAvailability::Unsupported, "\"unsupported\""),
        (PreflightAvailability::Deferred, "\"deferred\""),
        (
            PreflightAvailability::ReadinessLimited,
            "\"readiness_limited\"",
        ),
    ];

    for (variant, expected_json) in variants {
        let json = serde_json::to_string(&variant).unwrap();
        assert_eq!(
            json, expected_json,
            "PreflightAvailability::{variant:?} must serialize to {expected_json}"
        );
    }
}
