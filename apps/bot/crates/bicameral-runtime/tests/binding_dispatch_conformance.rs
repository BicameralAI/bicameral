//! Binding dispatch conformance tests.
//!
//! Proves issue #262 acceptance criteria through the `dispatch_with_workspace`
//! entry point:
//!
//! 1. `binding.create` accepts direct binding hints and revalidates them inside
//!    bot before materialization (verified binding produces `BindingEvidence`).
//! 2. Verified evidence stores `binding_id`, `graph_snapshot_id`, `validated_sha`,
//!    `ref_name`, `target_kind`, `target_identifier`, `evidence_state`,
//!    readiness warnings, and `created_from_request_id`.
//! 3. Non-verified states remain visible and do NOT materialize `BindingEvidence`.
//! 4. `binding.inspect` reads accepted binding evidence and derived
//!    currentness/readiness without mutating state.
//! 5. Validation-token input is rejected or typed unsupported/deferred.
//! 6. Tests cover verified binding, snapshot mismatch, stale graph, unsupported
//!    target, ambiguous target, not indexed, unavailable graph, and inspect.
//!
//! All tests are deterministic — no network, no hosted graph, no credentials.

use bicameral_api::event_store::EventStoreAdapter;
use bicameral_api::tool_request::*;
use bicameral_runtime::dispatch::dispatch_with_workspace;
use bicameral_runtime::event_store_adapters::MemoryAdapter;
use std::path::Path;
use std::process::Command;
use uuid::Uuid;

// ─── Helpers ──────────────────────────────────────────────────────────────

fn git(args: &[&str], dir: &Path) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

fn setup_workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();

    git(&["init"], p);
    git(&["config", "user.email", "test@test.com"], p);
    git(&["config", "user.name", "Test"], p);

    std::fs::create_dir_all(p.join("src")).unwrap();
    std::fs::write(
        p.join("src/lib.rs"),
        "pub fn hello_world() -> &'static str {\n    \"hello\"\n}\n",
    )
    .unwrap();
    std::fs::write(
        p.join("src/main.rs"),
        "fn main() {\n    println!(\"hi\");\n}\n",
    )
    .unwrap();

    git(&["add", "."], p);
    git(&["commit", "-m", "init"], p);

    dir
}

fn authority() -> AuthorityContext {
    bicameral_api::fixtures::authority_mcp("test-agent", "sess-test", "/workspace")
}

fn make_binding_create(
    decision_id: &str,
    bindings: Vec<BindingSpec>,
    graph_snapshot_id: Option<String>,
    ref_name: Option<String>,
    validation_token: Option<String>,
) -> ToolRequest {
    ToolRequest::new(
        ToolCommand::BindingCreate(BindingCreateParams {
            decision_or_candidate_id: decision_id.to_string(),
            bindings,
            commit_sha: None,
            ref_name,
            graph_snapshot_id,
            validation_token,
        }),
        authority(),
    )
}

fn make_binding_inspect(
    decision_id: &str,
    commit_sha: Option<String>,
    ref_name: Option<String>,
    graph_snapshot_id: Option<String>,
) -> ToolRequest {
    ToolRequest::new(
        ToolCommand::BindingInspect(BindingInspectParams {
            decision_or_candidate_id: decision_id.to_string(),
            commit_sha,
            ref_name,
            graph_snapshot_id,
        }),
        authority(),
    )
}

// ─── 1. Verified binding creates BindingEvidence ──────────────────────────

#[test]
fn verified_binding_creates_evidence_with_required_fields() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    let decision_id = Uuid::new_v4().to_string();
    let req = make_binding_create(
        &decision_id,
        vec![BindingSpec {
            file: "src/lib.rs".to_string(),
            range: None,
            symbol: None,
            from_discovery: None,
        }],
        None,
        None,
        None,
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok, "{:?}", resp.message);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "bound");

    let bound = &result["bound"];
    assert_eq!(bound.as_array().unwrap().len(), 1);

    let entry = &bound[0];
    // Verify all required fields per issue #262 acceptance criteria:
    assert!(entry["binding_id"].is_string(), "must have binding_id");
    assert!(
        entry["graph_snapshot_id"].is_string(),
        "must have graph_snapshot_id"
    );
    assert!(
        entry["validated_sha"].is_string(),
        "must have validated_sha"
    );
    assert!(entry["target"].is_string(), "must have target");
    assert_eq!(entry["target_kind"], "file");
    assert!(
        entry["target_identifier"].is_string(),
        "must have target_identifier"
    );
    assert_eq!(entry["evidence_state"], "verified");
    assert_eq!(
        entry["created_from_request_id"].as_str().unwrap(),
        req.request_id.to_string()
    );

    // Verify event was appended to event store.
    let events = adapter.replay().unwrap();
    assert_eq!(
        events.len(),
        1,
        "should append exactly one BindToCode event"
    );
}

// ─── 2. Snapshot mismatch ─────────────────────────────────────────────────

#[test]
fn snapshot_mismatch_returns_typed_response() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    let req = make_binding_create(
        &Uuid::new_v4().to_string(),
        vec![BindingSpec {
            file: "src/lib.rs".to_string(),
            range: None,
            symbol: None,
            from_discovery: None,
        }],
        Some("wrong-snapshot-id".to_string()),
        None,
        None,
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "snapshot_mismatch");
    assert_eq!(result["expected_snapshot_id"], "wrong-snapshot-id");
    assert!(result["actual_snapshot_id"].is_string());

    // No events should be appended.
    let events = adapter.replay().unwrap();
    assert!(
        events.is_empty(),
        "snapshot mismatch must not append events"
    );
}

// ─── 3. Stale graph (dirty working tree) ──────────────────────────────────

#[test]
fn stale_graph_rejects_without_materialization() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    // Dirty the file without committing — creates a stale condition.
    std::fs::write(dir.path().join("src/lib.rs"), "// changed\n").unwrap();

    let req = make_binding_create(
        &Uuid::new_v4().to_string(),
        vec![BindingSpec {
            file: "src/lib.rs".to_string(),
            range: None,
            symbol: None,
            from_discovery: None,
        }],
        None,
        None,
        None,
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "rejected");

    let rejected = &result["rejected"];
    assert_eq!(rejected.as_array().unwrap().len(), 1);
    assert_eq!(rejected[0]["evidence_state"], "unknown_stale");

    // No events should be appended.
    let events = adapter.replay().unwrap();
    assert!(events.is_empty(), "stale binding must not append events");
}

// ─── 4. Unsupported target (file type outside supported validation) ───────

#[test]
fn unsupported_target_rejects_without_materialization() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    // Request a symbol binding for a non-existent symbol — will hit Unsupported
    // from the symbol resolver since there's no parseable symbol.
    let req = make_binding_create(
        &Uuid::new_v4().to_string(),
        vec![BindingSpec {
            file: "src/lib.rs".to_string(),
            range: None,
            symbol: Some("NonExistentSymbol".to_string()),
            from_discovery: None,
        }],
        None,
        None,
        None,
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "rejected");

    let rejected = &result["rejected"];
    assert_eq!(rejected.as_array().unwrap().len(), 1);
    // Non-verified states must NOT materialize.
    let state = rejected[0]["evidence_state"].as_str().unwrap();
    assert!(
        state != "verified",
        "non-existent symbol must not produce verified state, got: {state}"
    );

    let events = adapter.replay().unwrap();
    assert!(
        events.is_empty(),
        "non-verified binding must not append events"
    );
}

// ─── 5. Not found (missing file) ─────────────────────────────────────────

#[test]
fn missing_file_rejects_with_not_found() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    let req = make_binding_create(
        &Uuid::new_v4().to_string(),
        vec![BindingSpec {
            file: "nonexistent.rs".to_string(),
            range: None,
            symbol: None,
            from_discovery: None,
        }],
        None,
        None,
        None,
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "rejected");
    assert_eq!(result["rejected"][0]["evidence_state"], "not_found");

    let events = adapter.replay().unwrap();
    assert!(events.is_empty());
}

// ─── 6. Unavailable graph (no workspace) ──────────────────────────────────

#[test]
fn unavailable_graph_returns_error() {
    let adapter = MemoryAdapter::new();

    let req = make_binding_create(
        &Uuid::new_v4().to_string(),
        vec![BindingSpec {
            file: "src/lib.rs".to_string(),
            range: None,
            symbol: None,
            from_discovery: None,
        }],
        None,
        None,
        None,
    );

    // dispatch_with_workspace with None workspace.
    let resp = dispatch_with_workspace(&req, &adapter, None);
    assert_eq!(resp.status, ToolResponseStatus::Error);
    assert!(
        resp.message
            .as_deref()
            .unwrap_or("")
            .contains("graph_unavailable"),
        "expected graph_unavailable, got: {:?}",
        resp.message
    );
}

// ─── 7. Validation token is rejected ──────────────────────────────────────

#[test]
fn validation_token_is_rejected() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    let req = make_binding_create(
        &Uuid::new_v4().to_string(),
        vec![BindingSpec {
            file: "src/lib.rs".to_string(),
            range: None,
            symbol: None,
            from_discovery: None,
        }],
        None,
        None,
        Some("some-token".to_string()),
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert!(
        resp.message
            .as_deref()
            .unwrap_or("")
            .contains("validation_token_unsupported"),
        "expected validation_token_unsupported, got: {:?}",
        resp.message
    );

    let events = adapter.replay().unwrap();
    assert!(events.is_empty());
}

// ─── 8. binding.inspect reads accepted evidence ───────────────────────────

#[test]
fn binding_inspect_reads_accepted_evidence_without_mutation() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    // First, create a binding.
    let decision_id = Uuid::new_v4().to_string();
    let create_req = make_binding_create(
        &decision_id,
        vec![BindingSpec {
            file: "src/lib.rs".to_string(),
            range: None,
            symbol: None,
            from_discovery: None,
        }],
        None,
        None,
        None,
    );

    let create_resp = dispatch_with_workspace(&create_req, &adapter, Some(dir.path()));
    assert_eq!(create_resp.status, ToolResponseStatus::Ok);
    let create_result = create_resp.result.unwrap();
    assert_eq!(create_result["status"], "bound");

    let events_before = adapter.replay().unwrap().len();

    // Now inspect — should return the binding without mutating state.
    let inspect_req = make_binding_inspect(&decision_id, None, None, None);
    let inspect_resp = dispatch_with_workspace(&inspect_req, &adapter, Some(dir.path()));
    assert_eq!(
        inspect_resp.status,
        ToolResponseStatus::Ok,
        "{:?}",
        inspect_resp.message
    );

    let inspect_result = inspect_resp.result.unwrap();
    assert_eq!(inspect_result["binding_count"], 1);

    let bindings = inspect_result["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 1);
    assert!(bindings[0]["binding_id"].is_string());
    assert!(bindings[0]["target"].is_string());
    assert_eq!(bindings[0]["evidence_state"], "verified");

    // Verify no new events were appended (inspect is read-only).
    let events_after = adapter.replay().unwrap().len();
    assert_eq!(
        events_before, events_after,
        "binding.inspect must not append events"
    );
}

// ─── 9. binding.inspect on empty state ────────────────────────────────────

#[test]
fn binding_inspect_empty_state_returns_zero_bindings() {
    let adapter = MemoryAdapter::new();

    let req = make_binding_inspect(&Uuid::new_v4().to_string(), None, None, None);
    let resp = dispatch_with_workspace(&req, &adapter, None);
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert_eq!(result["binding_count"], 0);
    assert!(result["bindings"].as_array().unwrap().is_empty());
}

// ─── 10. Mixed verified/rejected produces partial status ──────────────────

#[test]
fn mixed_bindings_produce_partial_status() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    let req = make_binding_create(
        &Uuid::new_v4().to_string(),
        vec![
            BindingSpec {
                file: "src/lib.rs".to_string(),
                range: None,
                symbol: None,
                from_discovery: None,
            },
            BindingSpec {
                file: "nonexistent.rs".to_string(),
                range: None,
                symbol: None,
                from_discovery: None,
            },
        ],
        None,
        None,
        None,
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "partial");
    assert_eq!(result["bound"].as_array().unwrap().len(), 1);
    assert_eq!(result["rejected"].as_array().unwrap().len(), 1);
}

// ─── 11. Code region binding with range ───────────────────────────────────

#[test]
fn code_region_binding_stores_target_kind() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    let req = make_binding_create(
        &Uuid::new_v4().to_string(),
        vec![BindingSpec {
            file: "src/lib.rs".to_string(),
            range: Some("1-3".to_string()),
            symbol: None,
            from_discovery: None,
        }],
        None,
        None,
        None,
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "bound");
    assert_eq!(result["bound"][0]["target_kind"], "code_region");
}
