//! Bot-MCP thin-client runtime conformance — issue #265.
//!
//! Terminal cross-surface contract check proving that a **thin MCP client**
//! (stdio transport + tool-call mapping + `AuthorityContext` assembly, owning
//! no ledger/graph/binding/governance/source/event-store authority — see
//! `docs/specs/bot-mcp-data-flow-runtime-architecture.md`, ADR-0002) can map
//! its expected tool calls to canonical `ToolRequest` commands and receive
//! usable `ToolResponse` results from the bot daemon dispatcher without any
//! MCP-owned authority fallback.
//!
//! Acceptance criteria covered:
//!   1. Compatible daemon startup/capability handshake succeeds; incompatible
//!      protocol startup is refused.
//!   2. Expected MCP tool mappings reach canonical bot commands for ingest,
//!      preflight, binding, review, search, and history.
//!   3. Unsupported/deferred commands return typed responses.
//!   4. No legacy MCP handler fallback is needed/accepted for V1 paths.
//!   6. Runtime trace correlation is present on ≥1 accepted and ≥1 rejected
//!      path. (Criterion 5 — migration notes — lives in
//!      `docs/specs/bot-mcp-thin-client-migration-notes.md`.)
//!
//! Scope boundary (verification-only): this file adds **no** lifecycle
//! behavior. It exercises the already-merged dispatcher (#258), ingest (#259),
//! history/search source links (#260), preflight (#261), binding (#262), and
//! runtime trace (#263) slices through their public entry points.
//!
//! All tests are deterministic — no network, no hosted graph, no credentials.

use std::collections::HashSet;

use bicameral_api::event_store::EventStoreAdapter;
use bicameral_api::fixtures;
use bicameral_api::tool_request::*;
use bicameral_runtime::dispatch::{
    capability_report, dispatch, CapabilityReport, DEFERRED_COMMANDS, PREFLIGHT_STAGE_CAPABILITIES,
    PROTOCOL_VERSION, SUPPORTED_COMMANDS,
};
use bicameral_runtime::event_store_adapters::MemoryAdapter;

// ─── Thin-client handshake model ────────────────────────────────────────────
//
// The bot daemon advertises a capability report; the thin MCP client decides
// whether to start. The decision logic lives in the client, so we model it here
// and drive it with the real `capability_report()` the daemon returns. This is
// test-local modeling, not product behavior.

/// Outcome of a thin-client startup negotiation against a daemon protocol.
fn negotiate_startup(
    client_supported_protocols: &[&str],
    daemon: &CapabilityReport,
) -> Result<(), String> {
    if client_supported_protocols.contains(&daemon.protocol_version) {
        Ok(())
    } else {
        Err(format!(
            "incompatible_protocol: daemon protocol '{}' not in client-supported {:?}; \
             refusing startup (no legacy handler fallback)",
            daemon.protocol_version, client_supported_protocols
        ))
    }
}

/// A daemon capability report with an overridden protocol version, used to
/// simulate an incompatible daemon without mutating the real one.
fn report_with_protocol(protocol: &'static str) -> CapabilityReport {
    CapabilityReport {
        protocol_version: protocol,
        supported_commands: SUPPORTED_COMMANDS,
        deferred_commands: DEFERRED_COMMANDS,
        preflight_stages: PREFLIGHT_STAGE_CAPABILITIES
            .iter()
            .map(
                |(stage, status)| bicameral_runtime::dispatch::PreflightStageCapability {
                    stage,
                    status,
                },
            )
            .collect(),
        projection_readiness:
            bicameral_api::reasoning_projection::ProjectionProviderReadiness::not_configured(),
        alpha_capabilities: bicameral_api::capability::AlphaCapabilities::alpha_default(),
        workspace_binding_available: false,
    }
}

fn mem_adapter() -> MemoryAdapter {
    MemoryAdapter::new()
}

#[test]
fn mcp_selected_product_is_explicit_tool_request_routing_context() {
    let request = fixtures::history_list().for_product("prd_mcp_alpha");
    let encoded = serde_json::to_value(request).unwrap();

    assert_eq!(encoded["product_id"], "prd_mcp_alpha");
    assert_ne!(encoded["authority"]["workspace"], "prd_mcp_alpha");
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum McpParityExpectation {
    Exposed,
    HiddenOrDeferred,
}

#[derive(Debug, Clone, Copy)]
struct McpCommandParityFixture {
    /// Canonical daemon command name from `ToolCommand::command_name()`.
    canonical_command: &'static str,
    /// MCP-side fixture or expected tool-route family this command belongs to.
    ///
    /// Keep this aligned with `bicameral-mcp` thin-client ToolRequest fixtures
    /// such as `tests/test_toolrequest_thin_client.py`.
    mcp_expectation: &'static str,
    expectation: McpParityExpectation,
}

/// Bot-side mirror of the MCP alpha command parity fixture.
///
/// This table is intentionally command-name based: it is a cross-repo fixture
/// contract, not runtime routing behavior. If the daemon registry changes, the
/// failing messages below should point workers to update both this table and
/// MCP's thin-client expected surface in the same PR chain.
fn mcp_alpha_command_parity_fixture() -> Vec<McpCommandParityFixture> {
    vec![
        McpCommandParityFixture {
            canonical_command: "ingest.submit_local",
            mcp_expectation: "bicameral.ingest",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "ingest.submit_managed",
            mcp_expectation: "bicameral.ingest.managed",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "history.list",
            mcp_expectation: "bicameral.history",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "search.query",
            mcp_expectation: "bicameral.search",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "review.accept_candidate",
            mcp_expectation: "bicameral.review",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "review.reject_candidate",
            mcp_expectation: "bicameral.review",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "candidate.hide",
            mcp_expectation: "bicameral.review",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "candidate.suppress",
            mcp_expectation: "bicameral.review",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "review.approve_signoff",
            mcp_expectation: "bicameral.review",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "review.reject_signoff",
            mcp_expectation: "bicameral.review",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "binding.create",
            mcp_expectation: "bicameral.bind",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "binding.inspect",
            mcp_expectation: "bicameral.bind",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "evidence.refresh",
            mcp_expectation: "bicameral.evidence.refresh",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "preflight.run",
            mcp_expectation: "bicameral.preflight",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "code.locate",
            mcp_expectation: "bicameral.code.locate",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "graph.status",
            mcp_expectation: "bicameral.graph.status",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "graph.refresh_snapshot",
            mcp_expectation: "bicameral.graph.refresh_snapshot",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "decision.find_code_impact",
            mcp_expectation: "bicameral.decision.find_code_impact",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "recall.inspect_evidence",
            mcp_expectation: "bicameral.recall.inspect_evidence",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "recall.expand_scope",
            mcp_expectation: "bicameral.recall.expand_scope",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "recall.promote_decision_candidate",
            mcp_expectation: "bicameral.recall.promote_decision_candidate",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "review.transition_finding",
            mcp_expectation: "bicameral.review.transition_finding",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "review.resolve_compliance",
            mcp_expectation: "defer: compliance resolution is not alpha-supported",
            expectation: McpParityExpectation::HiddenOrDeferred,
        },
        McpCommandParityFixture {
            canonical_command: "tracking.untrack_source",
            mcp_expectation: "defer: tracking mutation is not alpha-supported",
            expectation: McpParityExpectation::HiddenOrDeferred,
        },
        McpCommandParityFixture {
            canonical_command: "tracking.refresh_query",
            mcp_expectation: "defer: tracking query is not alpha-supported",
            expectation: McpParityExpectation::HiddenOrDeferred,
        },
        McpCommandParityFixture {
            canonical_command: "privacy.erase_subject",
            mcp_expectation: "defer: privacy erasure requires explicit gate",
            expectation: McpParityExpectation::HiddenOrDeferred,
        },
        McpCommandParityFixture {
            canonical_command: "recall.request_correction",
            mcp_expectation: "bicameral.recall.request_correction",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "workspace.bind",
            mcp_expectation: "bicameral.workspace.bind",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "workspace.resolve",
            mcp_expectation:
                "bicameral.workspace.resolve + prework-run (local-only; hosted bridge forbidden)",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "product.create",
            mcp_expectation: "defer: hosted-bridge canonical write, not an MCP alpha tool",
            expectation: McpParityExpectation::HiddenOrDeferred,
        },
        McpCommandParityFixture {
            canonical_command: "product.publish",
            mcp_expectation: "defer: hosted-bridge canonical write, not an MCP alpha tool",
            expectation: McpParityExpectation::HiddenOrDeferred,
        },
        McpCommandParityFixture {
            canonical_command: "candidate_selection.prepare",
            mcp_expectation: "bicameral.candidate_selection.prepare",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "candidate_selection.commit",
            mcp_expectation: "bicameral.candidate_selection.commit",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "candidate_session.heartbeat",
            mcp_expectation: "bicameral.candidate_session.heartbeat",
            expectation: McpParityExpectation::Exposed,
        },
        McpCommandParityFixture {
            canonical_command: "candidate_session.end",
            mcp_expectation: "bicameral.candidate_session.end",
            expectation: McpParityExpectation::Exposed,
        },
    ]
}

// ─── 1. Capability handshake ────────────────────────────────────────────────

#[test]
fn handshake_succeeds_for_compatible_daemon() {
    let daemon = capability_report();

    // The real daemon advertises the canonical protocol version.
    assert_eq!(daemon.protocol_version, PROTOCOL_VERSION);

    // A thin client that supports the canonical protocol negotiates startup.
    assert!(
        negotiate_startup(&[PROTOCOL_VERSION], &daemon).is_ok(),
        "compatible protocol must allow startup"
    );

    // The advertised command surface is the canonical lifecycle set.
    for cmd in [
        "ingest.submit_local",
        "preflight.run",
        "binding.create",
        "binding.inspect",
        "review.accept_candidate",
        "review.reject_candidate",
        "review.approve_signoff",
        "review.reject_signoff",
        "history.list",
        "search.query",
    ] {
        assert!(
            daemon.supported_commands.contains(&cmd),
            "handshake must advertise canonical command '{cmd}'"
        );
    }
}

#[test]
fn handshake_refused_for_incompatible_daemon_protocol() {
    // Thin client only speaks v2.
    let client = [PROTOCOL_VERSION];

    // Older daemon protocol → refuse, do not fall back to legacy handlers.
    let old = report_with_protocol("v1");
    let err = negotiate_startup(&client, &old).expect_err("v1 daemon must be refused");
    assert!(err.contains("incompatible_protocol"));

    // Newer/unknown daemon protocol → also refuse (fail-closed).
    let future = report_with_protocol("v3");
    assert!(
        negotiate_startup(&client, &future).is_err(),
        "unknown future daemon protocol must be refused"
    );
}

// ─── 2. MCP tool → canonical command mapping ────────────────────────────────
//
// Each MCP tool the thin client exposes maps to one or more canonical
// `ToolRequest` commands. We assert the mapping by dispatching the canonical
// fixture and confirming it routes to the expected canonical command (via the
// runtime trace `command` field) without an Error/panic — i.e. the bot's own
// handler was reached, not a legacy MCP handler.

/// (mcp_tool, canonical_command, fixture).
fn tool_command_mappings() -> Vec<(&'static str, &'static str, ToolRequest)> {
    vec![
        (
            "bicameral.ingest",
            "ingest.submit_local",
            fixtures::ingest_submit_local(),
        ),
        (
            "bicameral.preflight",
            "preflight.run",
            fixtures::preflight_run(),
        ),
        (
            "bicameral.bind",
            "binding.create",
            fixtures::binding_create(),
        ),
        (
            "bicameral.bind",
            "binding.inspect",
            fixtures::binding_inspect(),
        ),
        (
            "bicameral.review",
            "review.accept_candidate",
            fixtures::review_accept_candidate(),
        ),
        (
            "bicameral.review",
            "review.reject_candidate",
            fixtures::review_reject_candidate(),
        ),
        (
            "bicameral.review",
            "review.approve_signoff",
            fixtures::review_approve_signoff(),
        ),
        (
            "bicameral.review",
            "review.reject_signoff",
            fixtures::review_reject_signoff(),
        ),
        ("bicameral.search", "search.query", fixtures::search_query()),
        (
            "bicameral.history",
            "history.list",
            fixtures::history_list(),
        ),
    ]
}

#[test]
fn mcp_tool_names_map_to_canonical_commands() {
    for (mcp_tool, canonical, req) in tool_command_mappings() {
        // The fixture's command must serialize to the canonical command name.
        assert_eq!(
            req.command.command_name(),
            canonical,
            "{mcp_tool}: fixture command must be canonical '{canonical}'"
        );

        let adapter = mem_adapter();
        let resp = dispatch(&req, &adapter);

        // Routing reached the canonical bot command (proved via the trace),
        // not a silent MCP-owned fallback.
        let trace = resp
            .trace
            .as_ref()
            .unwrap_or_else(|| panic!("{mcp_tool}: response must carry a RuntimeTrace"));
        assert_eq!(
            trace.command, canonical,
            "{mcp_tool}: trace must record canonical command '{canonical}'"
        );
        assert_eq!(
            resp.request_id, req.request_id,
            "{mcp_tool}: response must correlate request_id"
        );
    }
}

// ─── MCP `bicameral.search` → canonical `search.query` (bindings scope) ─────
//
// Issue #134: the thin MCP client maps `bicameral.search` to exactly one
// canonical `search.query` ToolRequest, including for the `bindings` scope.
// The daemon reads accepted, bot-owned BindingEvidence from replayed state and
// returns valid binding results — never a legacy MCP-owned handler, never a
// typed unsupported-capability answer, never a silent empty.

#[test]
fn mcp_search_maps_to_single_canonical_search_query_for_bindings_scope() {
    use bicameral_api::evidence::{
        BindingConfidence, BindingEvidence, BindingType, GraphEvidenceState,
    };

    // The thin client owns no ledger/binding authority: it emits one canonical
    // `search.query` ToolRequest with scope `bindings`.
    let req = fixtures::search_query_bindings();
    assert_eq!(
        req.command.command_name(),
        "search.query",
        "bicameral.search must map to the single canonical search.query command"
    );

    // Seed accepted, bot-owned BindingEvidence via a canonical BindToCode event.
    let adapter = mem_adapter();
    let decision_id = uuid::Uuid::new_v4();
    let mut binding = BindingEvidence::new(
        uuid::Uuid::new_v4(),
        BindingType::FilePath,
        "src/main.rs".to_string(),
        BindingConfidence::High,
    );
    binding.graph_snapshot_id = Some("snap-134".to_string());
    binding.validated_sha = Some("abc123def456".to_string());
    binding.target_kind = Some("file".to_string());
    binding.target_identifier = Some("src/main.rs".to_string());
    binding.evidence_state = Some(GraphEvidenceState::Verified);
    let binding_id = binding.id;
    let payload = bicameral_api::event_store::BindToCodePayload {
        decision_id,
        binding,
        graph_snapshot_id: "snap-134".to_string(),
        validated_sha: "abc123def456".to_string(),
        evidence_state: GraphEvidenceState::Verified,
        content_hash: Some("h134".to_string()),
        line_start: Some(1),
        line_end: Some(20),
    };
    adapter
        .append(bicameral_api::event_store::EventStoreEntry::new(
            bicameral_api::event_store::EventType::BindToCode,
            serde_json::to_value(&payload).unwrap(),
            "test".to_string(),
        ))
        .unwrap();

    let resp = dispatch(&req, &adapter);

    // Routing reached the canonical bot command, not an MCP fallback.
    assert_eq!(
        resp.status,
        ToolResponseStatus::Ok,
        "msg: {:?}",
        resp.message
    );
    assert_eq!(
        resp.trace.as_ref().map(|t| t.command.as_str()),
        Some("search.query")
    );

    let result = resp.result.expect("binding search must carry a result");
    let bindings = result["bindings"]
        .as_array()
        .expect("result must include a bindings array");
    assert_eq!(
        bindings.len(),
        1,
        "accepted binding evidence must be returned"
    );
    let b = &bindings[0];
    // The MCP client receives every required binding field.
    assert_eq!(b["binding_id"], binding_id.to_string());
    assert_eq!(b["decision_id"], decision_id.to_string());
    assert_eq!(b["target_kind"], "file");
    assert_eq!(b["target_identifier"], "src/main.rs");
    assert_eq!(b["graph_snapshot_id"], "snap-134");
    assert_eq!(b["validated_sha"], "abc123def456");
    assert_eq!(b["evidence_state"], "verified");
    assert!(b.get("unknown_or_partial_scope").is_some());

    // Never a typed unsupported answer, never a silent empty.
    assert!(
        !resp
            .message
            .as_deref()
            .unwrap_or("")
            .contains("unsupported_scope"),
        "bindings scope must not be answered as unsupported"
    );
    assert!(
        result["scope_notes"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false),
        "binding scope response must carry an explicit read-only scope note"
    );
}

// ─── Accepted V1 paths reach canonical commands with usable responses ───────

#[test]
fn read_and_ingest_paths_return_usable_results() {
    // Source-only read/ingest paths succeed on a fresh ledger without any
    // workspace, graph, or legacy handler.
    for (name, req) in [
        ("ingest.submit_local", fixtures::ingest_submit_local()),
        ("preflight.run", fixtures::preflight_run()),
        ("history.list", fixtures::history_list()),
        ("search.query", fixtures::search_query()),
    ] {
        let adapter = mem_adapter();
        let resp = dispatch(&req, &adapter);
        assert_eq!(
            resp.status,
            ToolResponseStatus::Ok,
            "{name}: accepted V1 path must return Ok, got {:?}",
            resp.message
        );
        assert!(
            resp.result.is_some(),
            "{name}: accepted path must carry a usable result payload"
        );
    }
}

#[test]
fn binding_create_routes_to_bot_graph_handler() {
    // Without a workspace the bot's binding handler reports graph_unavailable —
    // a *typed* daemon response proving the canonical binding command was
    // reached, never a legacy MCP binding handler that fabricates a result.
    let adapter = mem_adapter();
    let resp = dispatch(&fixtures::binding_create(), &adapter);
    assert_ne!(
        resp.status,
        ToolResponseStatus::Ok,
        "binding.create without workspace cannot verify; must not falsely succeed"
    );
    assert!(
        resp.message
            .as_deref()
            .unwrap_or("")
            .contains("graph_unavailable"),
        "binding.create must return typed graph_unavailable, got: {:?}",
        resp.message
    );
    assert_eq!(
        resp.trace.as_ref().map(|t| t.command.as_str()),
        Some("binding.create"),
        "trace must record the canonical binding.create command"
    );
}

#[test]
fn binding_inspect_is_read_only_and_routes_to_bot() {
    // binding.inspect is a pure read: on an empty ledger it returns a usable,
    // empty projection (no graph access required, no mutation).
    let adapter = mem_adapter();
    let resp = dispatch(&fixtures::binding_inspect(), &adapter);
    assert_eq!(
        resp.status,
        ToolResponseStatus::Ok,
        "binding.inspect read must succeed, got: {:?}",
        resp.message
    );
    assert_eq!(
        resp.trace.as_ref().map(|t| t.command.as_str()),
        Some("binding.inspect"),
        "trace must record the canonical binding.inspect command"
    );
    assert!(
        adapter.replay().unwrap().is_empty(),
        "binding.inspect must not append any event"
    );
}

#[test]
fn ingest_then_accept_is_accepted_v1_path_without_fallback() {
    // A real accepted V1 lifecycle slice: ingest a candidate, then promote it
    // through the canonical review command. No legacy MCP handler participates.
    let adapter = mem_adapter();

    let ingest = fixtures::ingest_submit_local();
    let ingest_resp = dispatch(&ingest, &adapter);
    assert_eq!(
        ingest_resp.status,
        ToolResponseStatus::Ok,
        "ingest must succeed: {:?}",
        ingest_resp.message
    );
    let candidate_id = ingest_resp.result.as_ref().unwrap()["candidate_ids"][0]
        .as_str()
        .expect("ingest must return at least one candidate id")
        .to_string();

    let accept = ToolRequest::new(
        ToolCommand::ReviewAcceptCandidate(ReviewTargetParams {
            target_id: candidate_id.clone(),
            reason: Some("conformance: accepted V1 path".into()),
        }),
        fixtures::authority_dashboard("reviewer-265", "/home/user/project"),
    );
    let accept_resp = dispatch(&accept, &adapter);
    assert_eq!(
        accept_resp.status,
        ToolResponseStatus::Ok,
        "review.accept_candidate must succeed: {:?}",
        accept_resp.message
    );
    assert_eq!(
        accept_resp.result.as_ref().unwrap()["target_id"]
            .as_str()
            .unwrap(),
        candidate_id,
        "accept must echo the promoted candidate id"
    );

    // The promotion is materialized through the canonical event store, not an
    // MCP-side cache: replayed history surfaces exactly one Decision.
    let history = dispatch(&fixtures::history_list(), &adapter);
    let decisions = history.result.as_ref().unwrap()["decisions"]
        .as_array()
        .unwrap();
    assert_eq!(
        decisions.len(),
        1,
        "accepted candidate must materialize exactly one canonical Decision"
    );
}

// ─── 3. Unsupported / deferred commands return typed responses ──────────────

#[test]
fn deferred_commands_return_typed_unsupported() {
    let deferred = [
        (
            "review.resolve_compliance",
            fixtures::review_resolve_compliance(),
        ),
        (
            "tracking.untrack_source",
            fixtures::tracking_untrack_source(),
        ),
        ("tracking.refresh_query", fixtures::tracking_refresh_query()),
        ("privacy.erase_subject", fixtures::privacy_erase_subject()),
    ];

    for (name, req) in deferred {
        // Each deferred command is declared deferred (and not supported) in the
        // capability handshake — so a thin client knows the typed contract up
        // front rather than discovering it via a silent failure.
        assert!(
            DEFERRED_COMMANDS.contains(&name),
            "{name} must be advertised as deferred"
        );
        assert!(
            !SUPPORTED_COMMANDS.contains(&name),
            "{name} must not be advertised as supported"
        );

        let adapter = mem_adapter();
        let resp = dispatch(&req, &adapter);
        assert_eq!(
            resp.status,
            ToolResponseStatus::Rejected,
            "{name}: deferred command must return typed Rejected, got {:?}",
            resp.status
        );
        assert!(
            resp.message
                .as_deref()
                .unwrap_or("")
                .contains("unsupported_capability"),
            "{name}: rejection must cite unsupported_capability, got: {:?}",
            resp.message
        );
        // Even the rejected path carries a correlated trace.
        assert_eq!(
            resp.trace.as_ref().map(|t| t.status),
            Some(TraceStatus::Rejected),
            "{name}: rejected path must carry a Rejected trace"
        );
    }
}

// ─── 4. No legacy MCP handler fallback ──────────────────────────────────────

#[test]
fn legacy_mcp_payload_is_rejected_not_accepted() {
    // A v0.2-shaped payload (mcp_version / direct_ledger_write metadata) is
    // rejected with a typed reason — never silently accepted by a legacy path.
    let adapter = mem_adapter();
    let resp = dispatch(&fixtures::negative_legacy_mcp_payload(), &adapter);
    assert_eq!(
        resp.status,
        ToolResponseStatus::Rejected,
        "legacy MCP payload must be rejected, not accepted via fallback"
    );
    assert!(
        resp.message
            .as_deref()
            .unwrap_or("")
            .contains("legacy_mcp_payload"),
        "rejection must cite legacy_mcp_payload, got: {:?}",
        resp.message
    );

    // And no event was written — no legacy direct-ledger write occurred.
    assert!(
        adapter.replay().unwrap().is_empty(),
        "rejected legacy payload must not append any canonical event"
    );
}

#[test]
fn external_adapter_payload_does_not_borrow_local_authority() {
    // External-source shapes must enter through the bot-owned external gateway,
    // not local ToolRequest authority — the dispatcher rejects them.
    let adapter = mem_adapter();
    let resp = dispatch(&fixtures::negative_external_adapter_payload(), &adapter);
    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert!(
        resp.message
            .as_deref()
            .unwrap_or("")
            .contains("external_adapter_payload"),
        "rejection must cite external_adapter_payload, got: {:?}",
        resp.message
    );
}

// ─── 6. Runtime trace correlation across accepted and rejected paths ────────

#[test]
fn runtime_trace_correlates_accepted_and_rejected_paths() {
    let adapter = mem_adapter();

    // Accepted path.
    let accepted = fixtures::history_list();
    let accepted_id = accepted.request_id;
    let accepted_resp = dispatch(&accepted, &adapter);
    assert_eq!(accepted_resp.status, ToolResponseStatus::Ok);
    let accepted_trace = accepted_resp
        .trace
        .as_ref()
        .expect("accepted path must carry a trace");
    assert_eq!(accepted_trace.request_id, accepted_id);
    assert_eq!(accepted_trace.command, "history.list");
    assert_eq!(accepted_trace.status, TraceStatus::Ok);
    assert_eq!(accepted_trace.caller_surface, "mcp");
    assert!(accepted_trace.failure_reason.is_none());

    // Rejected path.
    let rejected = fixtures::review_resolve_compliance();
    let rejected_id = rejected.request_id;
    let rejected_resp = dispatch(&rejected, &adapter);
    assert_eq!(rejected_resp.status, ToolResponseStatus::Rejected);
    let rejected_trace = rejected_resp
        .trace
        .as_ref()
        .expect("rejected path must carry a trace");
    assert_eq!(rejected_trace.request_id, rejected_id);
    assert_eq!(rejected_trace.command, "review.resolve_compliance");
    assert_eq!(rejected_trace.status, TraceStatus::Rejected);
    assert!(
        rejected_trace.failure_reason.is_some(),
        "rejected trace must record a failure reason"
    );

    // Traces are operational receipts only — they never carry canonical IDs for
    // a rejected command.
    assert!(rejected_trace.touched_ids.is_empty());
}

// ─── Registry completeness: every command is typed (no silent gaps) ─────────

#[test]
fn capability_surface_partitions_every_lifecycle_command() {
    let requests = fixtures::one_request_per_command();
    let mut names = HashSet::new();

    for cmd in SUPPORTED_COMMANDS {
        assert!(
            !DEFERRED_COMMANDS.contains(cmd),
            "{cmd} cannot be both supported and deferred"
        );
    }

    for req in &requests {
        let canonical = req.command.command_name();
        names.insert(canonical);
        let class = req.command.capability_class();
        let supported = SUPPORTED_COMMANDS.contains(&canonical);
        let deferred = DEFERRED_COMMANDS.contains(&canonical);
        assert!(
            supported ^ deferred,
            "{canonical} must appear in exactly one advertisement list"
        );
        assert_eq!(
            class,
            if supported {
                CommandCapabilityClass::Supported
            } else {
                CommandCapabilityClass::Deferred
            },
            "{canonical} capability_class must agree with advertisement lists"
        );
    }

    assert_eq!(
        names.len(),
        35,
        "one_request_per_command must cover every ToolCommand variant in the \
         advertised surface (SUPPORTED + DEFERRED)"
    );
    assert_eq!(
        SUPPORTED_COMMANDS.len() + DEFERRED_COMMANDS.len(),
        names.len(),
        "advertised command lists must cover the full command surface"
    );
}

#[test]
fn mcp_alpha_parity_fixture_covers_every_daemon_command() {
    let requests = fixtures::one_request_per_command();
    let fixture = mcp_alpha_command_parity_fixture();
    let fixture_names = fixture
        .iter()
        .map(|entry| entry.canonical_command)
        .collect::<HashSet<_>>();

    for req in &requests {
        let canonical = req.command.command_name();
        assert!(
            fixture_names.contains(canonical),
            "{canonical} is missing from the bot-side MCP alpha parity fixture. \
             Update mcp_alpha_command_parity_fixture() and the matching \
             bicameral-mcp thin-client expectations in the same issue chain."
        );
    }

    assert_eq!(
        fixture_names.len(),
        fixture.len(),
        "MCP alpha parity fixture must not contain duplicate command entries"
    );
    assert_eq!(
        fixture.len(),
        requests.len(),
        "MCP alpha parity fixture must cover the full ToolCommand surface"
    );
}

#[test]
fn mcp_alpha_parity_fixture_matches_daemon_capability_report() {
    let report = capability_report();

    for entry in mcp_alpha_command_parity_fixture() {
        let supported = report.supported_commands.contains(&entry.canonical_command);
        let deferred = report.deferred_commands.contains(&entry.canonical_command);

        assert!(
            supported ^ deferred,
            "{} must appear in exactly one daemon capability list before MCP \
             parity can be asserted",
            entry.canonical_command
        );

        match entry.expectation {
            McpParityExpectation::Exposed => {
                assert!(
                    supported,
                    "{} is expected to be exposed through MCP route '{}', but \
                     the daemon does not advertise it as supported. Update the \
                     daemon registry or change the MCP fixture expectation with \
                     an explicit issue/RFQ.",
                    entry.canonical_command, entry.mcp_expectation
                );
            }
            McpParityExpectation::HiddenOrDeferred => {
                assert!(
                    deferred,
                    "{} is expected to be hidden/deferred for MCP ('{}'), but \
                     the daemon does not advertise it as deferred. If this \
                     command becomes alpha-supported, update bicameral-mcp \
                     thin-client expected tools and this fixture together.",
                    entry.canonical_command, entry.mcp_expectation
                );
            }
        }
    }
}

#[test]
fn mcp_alpha_parity_fixture_names_all_deferred_commands_to_hide() {
    let hidden_or_deferred = mcp_alpha_command_parity_fixture()
        .into_iter()
        .filter(|entry| entry.expectation == McpParityExpectation::HiddenOrDeferred)
        .map(|entry| entry.canonical_command)
        .collect::<HashSet<_>>();

    for command in DEFERRED_COMMANDS {
        assert!(
            hidden_or_deferred.contains(command),
            "{command} is deferred by the daemon but missing from the MCP \
             hide/defer parity fixture. Add it before changing MCP exposure."
        );
    }
}

#[test]
fn recall_request_correction_is_advertised_supported() {
    assert!(
        SUPPORTED_COMMANDS.contains(&"recall.request_correction"),
        "recall.request_correction must be advertised as supported (RFQ #663)"
    );
    assert!(
        !DEFERRED_COMMANDS.contains(&"recall.request_correction"),
        "recall.request_correction must not be advertised as deferred"
    );

    let cmd = ToolCommand::RecallRequestCorrection(
        bicameral_api::recall::RecallRequestCorrectionParams {
            packet_id: uuid::Uuid::nil(),
            selected_item_ids: vec![uuid::Uuid::nil()],
            correction_kind: bicameral_api::recall::CorrectionKind::SourceContradiction,
            rationale: "test".into(),
            approval_proof: bicameral_api::recall::ApprovalProof {
                kind: "signoff_token".into(),
                value: "tok".into(),
                actor_id: "user-owner-01".into(),
                provided_at: None,
            },
        },
    );
    assert_eq!(cmd.capability_class(), CommandCapabilityClass::Supported);
}

// ─── Canonical alpha recall envelope: thin client renders, never computes ───
//
// The daemon authors the canonical `ToolResponse.result` alpha recall envelope
// (#817). A thin MCP client deserializes and renders it; it must not compute
// recall, synthesize success, own freshness, or derive caveats locally.

#[test]
fn thin_client_consumes_daemon_authored_alpha_recall_envelope() {
    use bicameral_api::alpha_recall::{AlphaRecallAuthorityMode, AlphaRecallEnvelope};

    let adapter = mem_adapter();
    let req = fixtures::preflight_run();
    let resp = dispatch(&req, &adapter);
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    // The client's only job: parse the daemon-authored envelope from the result.
    let envelope: AlphaRecallEnvelope =
        serde_json::from_value(resp.result.as_ref().unwrap()["alpha_recall"].clone())
            .expect("thin client must be able to parse the canonical envelope");

    // The envelope is daemon-authored: read-only authority and daemon-authored
    // caveats are present, so the client renders rather than infers.
    assert_eq!(envelope.authority.mode, AlphaRecallAuthorityMode::ReadOnly);
    assert!(
        !envelope.product.caveats.is_empty(),
        "daemon authors caveats; the thin client must not synthesize them"
    );

    // The witness trace correlates with the response's RuntimeTrace command,
    // proving the client is rendering the daemon's canonical command output.
    let trace = resp.trace.as_ref().expect("response must carry a trace");
    assert_eq!(envelope.witness.runtime_trace.command, trace.command);
    assert_eq!(envelope.witness.runtime_trace.command, "preflight.run");
}

#[test]
fn thin_client_refuses_incompatible_envelope_without_fallback() {
    use bicameral_api::alpha_recall::{
        AlphaRecallEnvelope, AlphaRecallMismatchKind, ALPHA_RECALL_ENVELOPE_VERSION,
    };

    let adapter = mem_adapter();
    let resp = dispatch(&fixtures::preflight_run(), &adapter);
    let envelope: AlphaRecallEnvelope =
        serde_json::from_value(resp.result.as_ref().unwrap()["alpha_recall"].clone()).unwrap();

    // A client speaking the canonical protocol/version renders the envelope.
    assert!(envelope
        .check_compatibility(&[PROTOCOL_VERSION], ALPHA_RECALL_ENVELOPE_VERSION)
        .is_ok());

    // A client that speaks only a legacy protocol fails closed with a typed
    // mismatch — never a fabricated fallback render.
    let err = envelope
        .check_compatibility(&["v1"], ALPHA_RECALL_ENVELOPE_VERSION)
        .expect_err("legacy-only client must refuse the envelope");
    assert_eq!(err.kind, AlphaRecallMismatchKind::IncompatibleProtocol);
}

// ─── Nested ToolResponse.result shape: MCP consumes the canonical envelope ──
//
// #818 repair: the daemon returns the canonical envelope nested inside
// `ToolResponse.result` under `alpha_recall`, alongside the preserved sibling
// keys. A thin MCP client extracts it via the canonical
// `AlphaRecallEnvelope::from_tool_result` contract rather than guessing at the
// shape, and any missing/malformed daemon content is a visible failure — never
// a silent success or a legacy MCP-local fallback.

/// The committed MCP-consumable golden fixture and its stable contract
/// invariants. The bot-side dispatch must remain faithful to it.
const ALPHA_RECALL_FIXTURE: &str =
    include_str!("../../../protocol/schemas/v2/fixtures/alpha-recall-envelope.json");

#[test]
fn dispatched_result_carries_canonical_nested_shape() {
    use bicameral_api::alpha_recall::ALPHA_RECALL_RESULT_KEY;

    let adapter = mem_adapter();
    let resp = dispatch(&fixtures::preflight_run(), &adapter);
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.as_ref().expect("preflight must carry a result");

    // The canonical envelope is nested under `alpha_recall`, and the preserved
    // sibling keys remain present so the nested result shape is stable for the
    // MCP repair to consume.
    for key in ["staged", "live_recall", ALPHA_RECALL_RESULT_KEY] {
        assert!(
            result.get(key).is_some(),
            "preflight.run result must nest the '{key}' key"
        );
    }
}

#[test]
fn thin_client_extracts_envelope_via_canonical_contract() {
    use bicameral_api::alpha_recall::AlphaRecallEnvelope;

    let adapter = mem_adapter();
    let resp = dispatch(&fixtures::preflight_run(), &adapter);

    // The thin client uses the canonical extractor, not a bespoke path.
    let envelope = AlphaRecallEnvelope::from_tool_result(resp.result.as_ref())
        .expect("canonical nested result must yield the daemon envelope");

    // It correlates with the response trace, proving it renders the daemon's
    // canonical command output rather than deriving its own.
    let trace = resp.trace.as_ref().expect("response must carry a trace");
    assert_eq!(envelope.witness.runtime_trace.command, trace.command);
    assert_eq!(envelope.witness.runtime_trace.request_id, trace.request_id);
}

#[test]
fn missing_or_malformed_daemon_content_is_visible_failure_not_silent_success() {
    use bicameral_api::alpha_recall::{
        AlphaRecallEnvelope, AlphaRecallMismatchKind, ALPHA_RECALL_RESULT_KEY,
    };

    let adapter = mem_adapter();
    let resp = dispatch(&fixtures::preflight_run(), &adapter);
    let mut result = resp.result.clone().expect("preflight must carry a result");

    // Drop the canonical envelope: a daemon result without it must fail closed,
    // never render a success with missing daemon-owned content.
    result
        .as_object_mut()
        .unwrap()
        .remove(ALPHA_RECALL_RESULT_KEY);
    let err = AlphaRecallEnvelope::from_tool_result(Some(&result))
        .expect_err("result missing the envelope must fail closed");
    assert_eq!(err.kind, AlphaRecallMismatchKind::MissingEnvelope);

    // Replace the envelope with a legacy MCP-local shape: it is rejected as a
    // malformed envelope, never coerced into a fabricated render.
    result.as_object_mut().unwrap().insert(
        ALPHA_RECALL_RESULT_KEY.into(),
        serde_json::json!({ "mcp_local_recall": true, "score": 0.99 }),
    );
    let err = AlphaRecallEnvelope::from_tool_result(Some(&result))
        .expect_err("legacy MCP-local shape must fail closed");
    assert_eq!(err.kind, AlphaRecallMismatchKind::MalformedEnvelope);
}

#[test]
fn dispatched_envelope_is_faithful_to_committed_fixture() {
    use bicameral_api::alpha_recall::{
        AlphaRecallAuthorityMode, AlphaRecallEnvelope, ALPHA_RECALL_ENVELOPE_VERSION,
    };

    let adapter = mem_adapter();
    let resp = dispatch(&fixtures::preflight_run(), &adapter);
    let envelope = AlphaRecallEnvelope::from_tool_result(resp.result.as_ref()).unwrap();

    // The committed fixture's `_expected` invariants pin the contract the MCP
    // repair consumes; live dispatch must honor them.
    let fixture: serde_json::Value = serde_json::from_str(ALPHA_RECALL_FIXTURE).unwrap();
    let expected = &fixture["_expected"];

    assert_eq!(
        envelope.envelope_version,
        expected["envelope_version"].as_u64().unwrap() as u32
    );
    assert_eq!(envelope.envelope_version, ALPHA_RECALL_ENVELOPE_VERSION);
    assert_eq!(
        envelope.protocol_version,
        expected["protocol_version"].as_str().unwrap()
    );
    assert_eq!(envelope.authority.mode, AlphaRecallAuthorityMode::ReadOnly);
    assert_eq!(
        envelope.authority.appended_canonical_events,
        expected["appended_canonical_events"].as_bool().unwrap()
    );
    assert_eq!(
        envelope
            .witness
            .topology_receipt
            .as_ref()
            .map(|t| t.consumed),
        Some(expected["topology_receipt_consumed"].as_bool().unwrap())
    );
    assert_eq!(
        envelope.witness.runtime_trace.command,
        expected["runtime_trace_command"].as_str().unwrap()
    );

    // The daemon authors caveats (readiness-limited lookup); the client renders
    // them and must never synthesize its own.
    assert!(!envelope.product.caveats.is_empty());

    // Read-only lookup: no event was appended.
    assert!(
        adapter.replay().unwrap().is_empty(),
        "nested-result recall lookup must not append any canonical event"
    );
}
