//! Binding through CodeLocator and GraphSnapshotService conformance tests.
//!
//! Issue #438 acceptance criteria:
//!
//! 1. `binding.create` accepts only verified candidates with matching snapshot
//!    expectations.
//! 2. Snapshot mismatch returns typed rejection/next action.
//! 3. Approximate, ambiguous, stale, unindexed, unsupported, and not_found
//!    locator candidates do NOT materialize `BindingEvidence`.
//! 4. `binding.inspect` returns current / content_changed / target_missing /
//!    graph_unavailable / graph_not_configured as appropriate.
//! 5. Tests prove signoff and compliance do not mutate.
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
) -> ToolRequest {
    ToolRequest::new(
        ToolCommand::BindingCreate(BindingCreateParams {
            decision_or_candidate_id: decision_id.to_string(),
            bindings,
            commit_sha: None,
            ref_name,
            graph_snapshot_id,
            validation_token: None,
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

// ═══════════════════════════════════════════════════════════════════════════
// Acceptance criterion 1: binding.create accepts only verified candidates
// with matching snapshot expectations.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn verified_file_via_graph_service_produces_binding_evidence() {
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
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "bound");
    assert_eq!(result["bound"].as_array().unwrap().len(), 1);

    let entry = &result["bound"][0];
    assert_eq!(entry["evidence_state"], "verified");
    assert!(entry["graph_snapshot_id"].is_string());
    assert!(entry["validated_sha"].is_string());

    // Event was materialized.
    let events = adapter.replay().unwrap();
    assert_eq!(events.len(), 1);
}

#[test]
fn verified_code_region_via_graph_service_produces_binding_evidence() {
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
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "bound");
    assert_eq!(result["bound"][0]["target_kind"], "code_region");
    assert_eq!(result["bound"][0]["evidence_state"], "verified");
}

#[test]
fn matching_snapshot_expectation_allows_binding() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    // First, create a binding to discover the actual snapshot ID.
    let first_req = make_binding_create(
        &Uuid::new_v4().to_string(),
        vec![BindingSpec {
            file: "src/lib.rs".to_string(),
            range: None,
            symbol: None,
            from_discovery: None,
        }],
        None,
        None,
    );

    let first_resp = dispatch_with_workspace(&first_req, &adapter, Some(dir.path()));
    let first_result = first_resp.result.unwrap();
    let actual_snapshot_id = first_result["graph_snapshot_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Now create with the matching expected snapshot ID — should succeed.
    let adapter2 = MemoryAdapter::new();
    let req = make_binding_create(
        &Uuid::new_v4().to_string(),
        vec![BindingSpec {
            file: "src/main.rs".to_string(),
            range: None,
            symbol: None,
            from_discovery: None,
        }],
        Some(actual_snapshot_id),
        None,
    );

    let resp = dispatch_with_workspace(&req, &adapter2, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "bound");
}

// ═══════════════════════════════════════════════════════════════════════════
// Acceptance criterion 2: Snapshot mismatch returns typed rejection/next
// action.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn snapshot_mismatch_returns_typed_rejection_with_next_action() {
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
        Some("stale-snapshot-id-from-earlier".to_string()),
        None,
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "snapshot_mismatch");
    assert_eq!(
        result["expected_snapshot_id"],
        "stale-snapshot-id-from-earlier"
    );
    assert!(
        result["actual_snapshot_id"].is_string(),
        "must surface actual snapshot for re-resolve"
    );
    assert!(
        result["next_action"].is_string(),
        "must include next_action guidance"
    );

    // No events appended.
    let events = adapter.replay().unwrap();
    assert!(
        events.is_empty(),
        "snapshot mismatch must not append events"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Acceptance criterion 3: Non-verified candidates do NOT materialize
// BindingEvidence.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn not_found_candidate_does_not_materialize() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    let req = make_binding_create(
        &Uuid::new_v4().to_string(),
        vec![BindingSpec {
            file: "does_not_exist.rs".to_string(),
            range: None,
            symbol: None,
            from_discovery: None,
        }],
        None,
        None,
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "rejected");
    assert_eq!(result["rejected"][0]["evidence_state"], "not_found");

    let events = adapter.replay().unwrap();
    assert!(events.is_empty(), "not_found must not materialize");
}

#[test]
fn stale_candidate_does_not_materialize() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    // Dirty the file to create a stale condition (uncommitted change).
    std::fs::write(dir.path().join("src/lib.rs"), "// modified after commit\n").unwrap();

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
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "rejected");
    assert_eq!(result["rejected"][0]["evidence_state"], "unknown_stale");

    let events = adapter.replay().unwrap();
    assert!(events.is_empty(), "stale candidate must not materialize");
}

#[test]
fn unsupported_symbol_does_not_materialize() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    // Request a symbol binding for a non-existent symbol.
    let req = make_binding_create(
        &Uuid::new_v4().to_string(),
        vec![BindingSpec {
            file: "src/lib.rs".to_string(),
            range: None,
            symbol: Some("NoSuchSymbol".to_string()),
            from_discovery: None,
        }],
        None,
        None,
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "rejected");

    let state = result["rejected"][0]["evidence_state"].as_str().unwrap();
    assert!(
        state != "verified",
        "non-existent symbol must not produce verified state, got: {state}"
    );

    let events = adapter.replay().unwrap();
    assert!(
        events.is_empty(),
        "unresolvable symbol must not materialize"
    );
}

#[test]
fn graph_unavailable_does_not_materialize() {
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
    );

    // No workspace — graph is unavailable.
    let resp = dispatch_with_workspace(&req, &adapter, None);
    assert_eq!(resp.status, ToolResponseStatus::Error);
    assert!(resp
        .message
        .as_deref()
        .unwrap_or("")
        .contains("graph_unavailable"));

    let events = adapter.replay().unwrap();
    assert!(events.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// Acceptance criterion 4: binding.inspect returns current / content_changed
// / target_missing / graph_unavailable / graph_not_configured.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn inspect_returns_current_when_file_unchanged() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

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
    );

    dispatch_with_workspace(&create_req, &adapter, Some(dir.path()));

    let inspect_req = make_binding_inspect(&decision_id, None, None, None);
    let inspect_resp = dispatch_with_workspace(&inspect_req, &adapter, Some(dir.path()));
    assert_eq!(inspect_resp.status, ToolResponseStatus::Ok);

    let result = inspect_resp.result.unwrap();
    assert_eq!(result["bindings"][0]["currentness"], "current");
}

#[test]
fn inspect_returns_content_changed_when_file_modified() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

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
    );

    dispatch_with_workspace(&create_req, &adapter, Some(dir.path()));

    // Modify the file and commit — content hash changes.
    std::fs::write(
        dir.path().join("src/lib.rs"),
        "pub fn goodbye_world() -> &'static str {\n    \"bye\"\n}\n",
    )
    .unwrap();
    git(&["add", "src/lib.rs"], dir.path());
    git(&["commit", "-m", "modify lib"], dir.path());

    let inspect_req = make_binding_inspect(&decision_id, None, None, None);
    let inspect_resp = dispatch_with_workspace(&inspect_req, &adapter, Some(dir.path()));
    assert_eq!(inspect_resp.status, ToolResponseStatus::Ok);

    let result = inspect_resp.result.unwrap();
    assert_eq!(result["bindings"][0]["currentness"], "content_changed");
}

#[test]
fn inspect_returns_target_missing_when_file_deleted() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    let decision_id = Uuid::new_v4().to_string();
    let create_req = make_binding_create(
        &decision_id,
        vec![BindingSpec {
            file: "src/main.rs".to_string(),
            range: None,
            symbol: None,
            from_discovery: None,
        }],
        None,
        None,
    );

    dispatch_with_workspace(&create_req, &adapter, Some(dir.path()));

    // Delete the file and commit.
    std::fs::remove_file(dir.path().join("src/main.rs")).unwrap();
    git(&["add", "src/main.rs"], dir.path());
    git(&["commit", "-m", "remove main"], dir.path());

    let inspect_req = make_binding_inspect(&decision_id, None, None, None);
    let inspect_resp = dispatch_with_workspace(&inspect_req, &adapter, Some(dir.path()));
    assert_eq!(inspect_resp.status, ToolResponseStatus::Ok);

    let result = inspect_resp.result.unwrap();
    assert_eq!(result["bindings"][0]["currentness"], "target_missing");
}

#[test]
fn inspect_returns_graph_unavailable_without_workspace() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

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
    );

    dispatch_with_workspace(&create_req, &adapter, Some(dir.path()));

    // Inspect without workspace — should be graph_unavailable.
    let inspect_req = make_binding_inspect(&decision_id, None, None, None);
    let inspect_resp = dispatch_with_workspace(&inspect_req, &adapter, None);
    assert_eq!(inspect_resp.status, ToolResponseStatus::Ok);

    let result = inspect_resp.result.unwrap();
    assert_eq!(result["bindings"][0]["currentness"], "graph_unavailable");
}

// ═══════════════════════════════════════════════════════════════════════════
// Acceptance criterion 5: Tests prove signoff and compliance do not mutate.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn binding_create_does_not_mutate_signoff_or_compliance() {
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
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    // Verify only BindToCode events — no signoff, compliance, or review
    // mutations.
    let events = adapter.replay().unwrap();
    for event in &events {
        let event_type = event
            .payload
            .get("event_type")
            .or_else(|| event.payload.get("type"));
        if let Some(et) = event_type {
            let type_str = et.as_str().unwrap_or("");
            assert!(
                !type_str.contains("signoff")
                    && !type_str.contains("compliance")
                    && !type_str.contains("review"),
                "binding.create must not produce signoff/compliance/review events, got: {}",
                type_str
            );
        }
    }
}

#[test]
fn binding_inspect_does_not_mutate_signoff_or_compliance() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

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
    );

    dispatch_with_workspace(&create_req, &adapter, Some(dir.path()));
    let events_before = adapter.replay().unwrap().len();

    // Inspect — must not add any events.
    let inspect_req = make_binding_inspect(&decision_id, None, None, None);
    dispatch_with_workspace(&inspect_req, &adapter, Some(dir.path()));

    let events_after = adapter.replay().unwrap().len();
    assert_eq!(
        events_before, events_after,
        "binding.inspect must not append any events (read-only)"
    );
}

#[test]
fn binding_inspect_after_content_change_does_not_mutate_original_binding() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

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
    );

    let create_resp = dispatch_with_workspace(&create_req, &adapter, Some(dir.path()));
    let create_result = create_resp.result.unwrap();
    let original_binding_id = create_result["bound"][0]["binding_id"]
        .as_str()
        .unwrap()
        .to_string();
    let original_validated_sha = create_result["bound"][0]["validated_sha"]
        .as_str()
        .unwrap()
        .to_string();

    // Modify file and commit.
    std::fs::write(
        dir.path().join("src/lib.rs"),
        "pub fn new_function() -> bool {\n    true\n}\n",
    )
    .unwrap();
    git(&["add", "src/lib.rs"], dir.path());
    git(&["commit", "-m", "change"], dir.path());

    let events_before = adapter.replay().unwrap().len();

    // Inspect after change.
    let inspect_req = make_binding_inspect(&decision_id, None, None, None);
    let inspect_resp = dispatch_with_workspace(&inspect_req, &adapter, Some(dir.path()));
    let inspect_result = inspect_resp.result.unwrap();

    // Original binding fields must be preserved — immutable.
    let binding = &inspect_result["bindings"][0];
    assert_eq!(binding["binding_id"], original_binding_id);
    assert_eq!(binding["validated_sha"], original_validated_sha);
    assert_eq!(binding["evidence_state"], "verified");

    // Currentness reflects the change but doesn't rewrite the binding.
    assert_eq!(binding["currentness"], "content_changed");

    // No new events.
    let events_after = adapter.replay().unwrap().len();
    assert_eq!(
        events_before, events_after,
        "inspect must not mutate event store"
    );
}
