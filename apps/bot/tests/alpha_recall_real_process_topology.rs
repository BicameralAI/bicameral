//! Disposable real-process topology harness proof — issue #819.
//!
//! Proves the **bot portion** of the narrowed #789 alpha recall topology
//! against a production daemon *process* (spawned `bicameral gateway start`),
//! reached over the loopback HTTP `ToolRequest` boundary — not an in-process or
//! fake-daemon substitute.
//!
//! The real-process tests are marked `#[ignore]` because they spawn a daemon
//! process and bind a loopback socket; they are slow relative to the unit
//! suite. Run them explicitly:
//!
//! ```bash
//! cargo test -p bicameral --test alpha_recall_real_process_topology -- --ignored --nocapture
//! ```
//!
//! The non-ignored test exercises the harness's witness data-model and the
//! client startup-negotiation control so the default `cargo test` still covers
//! the harness contract without spawning a process.
//!
//! Downstream: issue #820 (lifecycle/UserJourney validation) consumes the
//! crash/retry controls and negative-path probes exposed here. This issue does
//! **not** implement #820's validation itself.

mod support;

use support::real_process_topology::{
    negotiate_client_startup, DisposableTopology, TopologyError, DAEMON_PROTOCOL_VERSION,
    TOPOLOGY_RECEIPT_SCHEMA,
};

use bicameral_api::tool_request::ToolResponseStatus;

const PAYMENTS_SOURCE: &str = "meeting://payments/architecture";
const PAYMENTS_TITLE: &str = "Payment retries require idempotency keys";
const PAYMENTS_BODY: &str = "Every payment retry must call apply_idempotency before settlement.";

/// End-to-end proof: a `ToolRequest` reaches a production daemon over loopback
/// and returns the canonical nested `ToolResponse.result` with daemon-authored
/// staged recall/caveats plus a `RuntimeTrace` witness, while the topology
/// receipt witnesses the crossed process boundary.
#[test]
#[ignore = "spawns a production daemon process; run with --ignored"]
fn real_process_recall_returns_staged_recall_and_trace_witness() {
    let mut topology = DisposableTopology::start().expect("start disposable topology");

    // Capability handshake over the real loopback boundary.
    let capabilities = topology.capabilities().expect("capability report");
    assert_eq!(
        capabilities["protocol_version"], DAEMON_PROTOCOL_VERSION,
        "daemon must advertise the expected protocol over loopback"
    );

    // Seed a relevant and an irrelevant accepted Decision through the real path.
    topology
        .seed_accepted_decision(PAYMENTS_SOURCE, PAYMENTS_TITLE, PAYMENTS_BODY)
        .expect("seed relevant decision");
    topology
        .seed_accepted_decision(
            "meeting://profiles/architecture",
            "Profile images require immutable object keys",
            "Profile images must retain immutable object keys across updates.",
        )
        .expect("seed irrelevant decision");

    // Recall over the real loopback boundary.
    let request =
        topology.preflight_request(&["src/payments/idempotency.rs"], &["apply_idempotency"]);
    let response = topology
        .dispatch(&request, "mcp_render_surface -> loopback_http")
        .expect("preflight recall dispatch");

    assert_eq!(
        response.status,
        ToolResponseStatus::Ok,
        "recall must succeed: {:?}",
        response.message
    );

    // RuntimeTrace witness must correlate the surface and command.
    let trace = response
        .trace
        .as_ref()
        .expect("RuntimeTrace witness present");
    assert_eq!(trace.command, "preflight.run");
    assert_eq!(trace.caller_surface, "mcp");
    assert_eq!(trace.request_id, response.request_id);

    // Canonical nested ToolResponse.result with daemon-authored staged recall.
    let result = response.result.as_ref().expect("recall result payload");
    let recall = &result["live_recall"];
    assert!(
        recall.is_object(),
        "live_recall staged recall must be present"
    );
    assert_eq!(recall["outcome"], "results");
    assert_eq!(recall["scoring_eligible"], true);

    // Daemon-authored caveats / readiness limitations are present as an array.
    assert!(
        recall["readiness_limitations"].is_array(),
        "daemon-authored caveats (readiness_limitations) must be present"
    );

    // Staged recall packet: canonical decisions ranked and scoped by the daemon.
    let ranked = recall["ranked_results"]
        .as_array()
        .expect("ranked_results array");
    assert_eq!(ranked.len(), 1, "only the relevant decision should rank");
    assert!(ranked[0]["source_refs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|source_ref| source_ref == PAYMENTS_SOURCE));
    let packet = &recall["packet"];
    assert_eq!(packet["searched_scope"]["level"], "replayable");
    assert_eq!(packet["recall_trace"]["replay_available"], true);

    // Negative assertion: the recall path never claims compliance/merge safety.
    let serialized = serde_json::to_string(result).unwrap().to_lowercase();
    for forbidden in [
        "compliance confirmed",
        "signoff approved",
        "safe to merge",
        "no conflict exists",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "recall result must not claim '{forbidden}'"
        );
    }

    // Topology receipt witnesses the crossed process boundary, non-authoritative.
    let receipt = topology.topology_receipt();
    assert_eq!(receipt.schema, TOPOLOGY_RECEIPT_SCHEMA);
    assert!(
        !receipt.authoritative,
        "receipt is a witness, not product state"
    );
    assert!(
        receipt.disposable,
        "topology must use disposable stores/data"
    );
    assert!(
        receipt.daemon_pid > 0,
        "receipt records the real daemon pid"
    );
    assert!(
        receipt.loopback_addr.starts_with("127.0.0.1:"),
        "daemon must be bound to loopback only"
    );
    assert!(
        receipt
            .hops
            .iter()
            .any(|hop| hop.command == "preflight.run" && hop.trace_present),
        "receipt must witness the traced recall hop over loopback"
    );
    // The receipt serializes cleanly for downstream (#820) attachment.
    serde_json::to_string(&receipt).expect("topology receipt serializes");
}

/// Crash/retry control: the daemon can be crashed and restarted against the
/// same disposable store, and recall recovers. Downstream #820 drives this for
/// lifecycle validation.
#[test]
#[ignore = "spawns a production daemon process; run with --ignored"]
fn real_process_crash_and_retry_recovers_recall() {
    let mut topology = DisposableTopology::start().expect("start disposable topology");
    topology
        .seed_accepted_decision(PAYMENTS_SOURCE, PAYMENTS_TITLE, PAYMENTS_BODY)
        .expect("seed decision");
    assert!(
        topology.daemon_running(),
        "daemon should be running after start"
    );

    // Crash hook: daemon-unavailable is visible while down.
    topology.crash_daemon().expect("crash daemon");
    assert!(
        !topology.daemon_running(),
        "daemon should be down after crash"
    );
    let request =
        topology.preflight_request(&["src/payments/idempotency.rs"], &["apply_idempotency"]);
    let err = topology
        .expect_daemon_unavailable(&request)
        .expect("daemon-unavailable case must be visible");
    assert!(matches!(err, TopologyError::DaemonUnavailable(_)));

    // Retry hook: restart recovers durable recall from the disposable store.
    topology.restart_daemon().expect("restart daemon");
    assert!(
        topology.daemon_running(),
        "daemon should be running after restart"
    );
    let response = topology
        .dispatch(&request, "mcp_render_surface -> loopback_http")
        .expect("recall after restart");
    assert_eq!(response.status, ToolResponseStatus::Ok);
    let recall = &response.result.as_ref().unwrap()["live_recall"];
    assert_eq!(
        recall["ranked_results"].as_array().unwrap().len(),
        1,
        "recall must recover the seeded decision after restart"
    );
}

/// Protocol-drift and shape-mismatch cases are visible (not silently accepted),
/// which downstream #820 validation asserts against.
#[test]
#[ignore = "spawns a production daemon process; run with --ignored"]
fn real_process_protocol_drift_and_shape_mismatch_are_visible() {
    let topology = DisposableTopology::start().expect("start disposable topology");

    // Protocol drift: a client that speaks only a different protocol refuses to
    // start rather than falling back to legacy authority.
    let drift = negotiate_client_startup(&["v1"], DAEMON_PROTOCOL_VERSION);
    assert!(drift.is_err(), "protocol drift must be a visible refusal");
    assert!(drift.unwrap_err().contains("incompatible_protocol"));

    // A compatible client negotiates successfully.
    assert!(negotiate_client_startup(&["v2"], DAEMON_PROTOCOL_VERSION).is_ok());

    // Shape mismatch: a malformed ToolRequest body is rejected with a 4xx,
    // never silently accepted or crashing the daemon.
    let (status, body) = topology
        .dispatch_raw("{\"command\":{\"name\":\"preflight.run\"}}")
        .expect("raw dispatch returns an HTTP status");
    assert!(
        (400..500).contains(&status),
        "shape mismatch must be a visible client error, got {status}: {body}"
    );

    // The daemon survives the malformed request and still serves capabilities.
    let capabilities = topology.capabilities().expect("daemon still healthy");
    assert_eq!(capabilities["protocol_version"], DAEMON_PROTOCOL_VERSION);
}

/// Fast, non-process coverage of the harness contract: the startup-negotiation
/// control and receipt schema constant behave as documented.
#[test]
fn client_startup_negotiation_contract() {
    assert!(negotiate_client_startup(&["v2"], DAEMON_PROTOCOL_VERSION).is_ok());
    let refused = negotiate_client_startup(&["v1", "v0"], DAEMON_PROTOCOL_VERSION);
    assert!(refused.is_err());
    assert!(refused.unwrap_err().contains("incompatible_protocol"));
    assert_eq!(
        TOPOLOGY_RECEIPT_SCHEMA,
        "alpha-recall-real-process-topology-receipt/v1"
    );
}
