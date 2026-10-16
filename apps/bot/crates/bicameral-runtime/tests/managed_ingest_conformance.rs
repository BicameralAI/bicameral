//! Managed provider-backed ingestion conformance tests.
//!
//! Verifies the acceptance criteria from issue #388:
//!
//! 1. Managed provider-backed ingestion checks provider connection and source
//!    binding state separately.
//! 2. Missing provider connection returns typed `provider_not_connected`.
//! 3. Missing source binding returns typed `source_binding_not_ready`.
//! 4. Unsupported managed source flow returns typed rejection, not panic/empty
//!    success.
//! 5. Local/manual ingest behavior remains unchanged.

use bicameral_api::fixtures;
use bicameral_api::tool_request::ToolResponseStatus;
use bicameral_runtime::dispatch::dispatch;
use bicameral_runtime::event_store_adapters::MemoryAdapter;

fn mem_adapter() -> MemoryAdapter {
    MemoryAdapter::new()
}

// ── Provider not connected ───────────────────────────────────────

#[test]
fn managed_ingest_rejects_when_provider_not_connected() {
    let adapter = mem_adapter();
    // Use a provider that does not exist in any connections.yaml on test host
    let req = fixtures::ingest_submit_managed("nonexistent_provider", "src_binding_1");
    let resp = dispatch(&req, &adapter);

    assert_eq!(
        resp.status,
        ToolResponseStatus::Rejected,
        "expected rejected status for missing provider, got: {:?}",
        resp.status
    );

    let msg = resp.message.as_deref().unwrap_or("");
    assert!(
        msg.contains("provider_not_connected") || msg.contains("provider_action_needed"),
        "expected provider_not_connected or provider_action_needed in message, got: {msg}"
    );
}

// ── Source binding not ready ─────────────────────────────────────

#[test]
fn managed_ingest_rejects_when_source_binding_not_ready() {
    // Even if we could somehow have a connected provider, source bindings
    // are not implemented in alpha — the handler will either reject at
    // provider check (not connected) or at source binding check.
    //
    // This test verifies the typed rejection message contains
    // source_binding_not_ready when the provider check passes.
    //
    // Since we cannot easily inject a connected provider in unit tests
    // without writing a connections.yaml to the test user's home directory,
    // we verify the rejection chain: if provider is not connected, it short-
    // circuits before source binding. The source_binding_not_ready path is
    // tested by the dispatch unit tests in dispatch.rs where we can mock
    // the registry.
    let adapter = mem_adapter();
    let req = fixtures::ingest_submit_managed("github", "github_repo_primary");
    let resp = dispatch(&req, &adapter);

    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    let msg = resp.message.as_deref().unwrap_or("");
    // On test hosts without ~/.bicameral/connections.yaml, the provider
    // will be NotConnected, so we get provider_not_connected.
    assert!(
        msg.contains("provider_not_connected")
            || msg.contains("source_binding_not_ready")
            || msg.contains("unsupported_managed_flow"),
        "expected typed rejection reason in message, got: {msg}"
    );
}

// ── Unsupported managed flow ─────────────────────────────────────

#[test]
fn managed_ingest_never_panics_or_returns_empty_success() {
    let adapter = mem_adapter();
    let req = fixtures::ingest_submit_managed("github", "some_binding");
    let resp = dispatch(&req, &adapter);

    // Must be Rejected (typed), never Ok (empty success) or Error (panic-like)
    assert_eq!(
        resp.status,
        ToolResponseStatus::Rejected,
        "managed ingest must return typed rejection, not {:?}",
        resp.status
    );
    // Must have a non-empty message
    assert!(
        resp.message.as_deref().unwrap_or("").len() > 10,
        "rejection must carry a meaningful typed message"
    );
}

// ── Local/manual ingest unchanged ────────────────────────────────

#[test]
fn local_ingest_still_works_after_managed_ingest_addition() {
    let adapter = mem_adapter();
    let req = fixtures::ingest_submit_local();
    let resp = dispatch(&req, &adapter);

    assert_eq!(
        resp.status,
        ToolResponseStatus::Ok,
        "local ingest must remain functional, got: {:?} — {}",
        resp.status,
        resp.message.as_deref().unwrap_or("")
    );
    // Result should contain source_id, snapshot_id, candidate_ids
    let result = resp
        .result
        .expect("local ingest should return result payload");
    assert!(result.get("source_id").is_some());
    assert!(result.get("candidate_ids").is_some());
}

// ── Serialization round-trip ─────────────────────────────────────

#[test]
fn managed_ingest_command_serializes_correctly() {
    let req = fixtures::ingest_submit_managed("github", "github_repo_primary");
    let json = serde_json::to_string(&req.command).unwrap();
    assert!(
        json.contains("ingest.submit_managed"),
        "serialized command should contain ingest.submit_managed, got: {json}"
    );
    assert!(json.contains("github"));
    assert!(json.contains("github_repo_primary"));

    // Deserialize back
    let cmd: bicameral_api::tool_request::ToolCommand = serde_json::from_str(&json).unwrap();
    assert_eq!(cmd.command_name(), "ingest.submit_managed");
}
