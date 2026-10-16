//! Graph status and refresh snapshot dispatch conformance tests.
//!
//! Proves:
//!
//! - `graph.status` returns `GraphStatusResult` with typed readiness without
//!   performing indexing or mutating governance state.
//! - `graph.refresh_snapshot` returns `GraphRefreshResult` with typed status,
//!   target resolution, and next actions.
//! - Both commands handle workspace-not-configured (no workspace root)
//!   gracefully with typed `NotConfigured` responses.
//! - When a workspace is provided, the `GraphSnapshotService` trait is used
//!   to resolve readiness and snapshot identity.
//! - Neither command appends governance events to the event store.
//!
//! Coverage:
//! - Ready state (workspace with resolvable HEAD)
//! - Not configured state (no workspace root)
//! - Stale / failed states (ref not available)
//! - Unsupported state surface (refresh_snapshot)

use std::path::{Path, PathBuf};

use bicameral_api::event_store::EventStoreAdapter;
use bicameral_api::tool_request::*;
use bicameral_runtime::dispatch::dispatch_with_workspace;
use bicameral_runtime::event_store_adapters::MemoryAdapter;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn mem_adapter() -> MemoryAdapter {
    MemoryAdapter::new()
}

/// Resolve the workspace root (the repo checkout root) from
/// `CARGO_MANIFEST_DIR` which points at `crates/bicameral-runtime/`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn graph_status_request(ref_name: Option<&str>) -> ToolRequest {
    ToolRequest::new(
        ToolCommand::GraphStatus(GraphStatusParams {
            workspace_id: None,
            ref_name: ref_name.map(|s| s.to_string()),
            commit_sha: None,
            graph_snapshot_id: None,
        }),
        AuthorityContext {
            actor_id: "test-actor".into(),
            auth_method: AuthMethod::Cli,
            session_id: Some("test-session".into()),
            workspace: None,
            policy_scope: vec![],
            audit_metadata: serde_json::Map::new(),
        },
    )
}

fn graph_refresh_request(ref_name: Option<&str>) -> ToolRequest {
    ToolRequest::new(
        ToolCommand::GraphRefreshSnapshot(GraphRefreshParams {
            workspace_id: None,
            ref_name: ref_name.map(|s| s.to_string()),
            commit_sha: None,
            base_ref: None,
            base_sha: None,
            head_ref: None,
            head_sha: None,
            scope_kind: None,
            files: vec![],
            symbols: vec![],
            allow_index: AllowIndex::Bounded,
            trigger: RefreshTrigger::Manual,
        }),
        AuthorityContext {
            actor_id: "test-actor".into(),
            auth_method: AuthMethod::Cli,
            session_id: Some("test-session".into()),
            workspace: None,
            policy_scope: vec![],
            audit_metadata: serde_json::Map::new(),
        },
    )
}

// ---------------------------------------------------------------------------
// graph.status tests
// ---------------------------------------------------------------------------

#[test]
fn graph_status_not_configured_without_workspace() {
    let adapter = mem_adapter();
    let req = graph_status_request(Some("HEAD"));
    let resp = dispatch_with_workspace(&req, &adapter, None);

    assert_eq!(resp.status, ToolResponseStatus::Ok);
    let result: GraphStatusResult = serde_json::from_value(resp.result.unwrap()).unwrap();
    assert_eq!(result.readiness, LocatorGraphReadiness::NotConfigured);
    assert!(result.stale_detail.is_some());
    assert!(result
        .next_actions
        .contains(&"configure_workspace".to_string()));
}

#[test]
fn graph_status_not_configured_does_not_append_events() {
    let adapter = mem_adapter();
    let req = graph_status_request(Some("HEAD"));
    let _ = dispatch_with_workspace(&req, &adapter, None);
    let events = adapter.replay().unwrap();
    assert!(
        events.is_empty(),
        "graph.status must not append governance events"
    );
}

#[test]
fn graph_status_ready_with_git_workspace() {
    let adapter = mem_adapter();
    let req = graph_status_request(Some("HEAD"));

    // Use the bicameral-bot repo itself as a valid git workspace
    let ws = workspace_root();
    let resp = dispatch_with_workspace(&req, &adapter, Some(&ws));

    assert_eq!(resp.status, ToolResponseStatus::Ok);
    let result: GraphStatusResult = serde_json::from_value(resp.result.unwrap()).unwrap();
    // A valid git repo with a resolvable HEAD should be Ready
    assert_eq!(result.readiness, LocatorGraphReadiness::Ready);
    assert!(result.snapshot.is_some());
}

#[test]
fn graph_status_ready_does_not_append_events() {
    let adapter = mem_adapter();
    let req = graph_status_request(Some("HEAD"));
    let ws = workspace_root();
    let _ = dispatch_with_workspace(&req, &adapter, Some(&ws));
    let events = adapter.replay().unwrap();
    assert!(
        events.is_empty(),
        "graph.status must not append governance events even with workspace"
    );
}

#[test]
fn graph_status_stale_for_unavailable_ref() {
    let adapter = mem_adapter();
    // Request a ref that definitely doesn't exist
    let req = graph_status_request(Some("refs/heads/nonexistent-branch-xyz-999"));
    let ws = workspace_root();
    let resp = dispatch_with_workspace(&req, &adapter, Some(&ws));

    assert_eq!(resp.status, ToolResponseStatus::Ok);
    let result: GraphStatusResult = serde_json::from_value(resp.result.unwrap()).unwrap();
    // Unavailable ref should not be Ready
    assert_ne!(result.readiness, LocatorGraphReadiness::Ready);
    assert!(result.stale_detail.is_some());
}

#[test]
fn graph_status_default_ref_is_head() {
    let adapter = mem_adapter();
    // No ref_name specified — should default to HEAD
    let req = graph_status_request(None);
    let ws = workspace_root();
    let resp = dispatch_with_workspace(&req, &adapter, Some(&ws));

    assert_eq!(resp.status, ToolResponseStatus::Ok);
    let result: GraphStatusResult = serde_json::from_value(resp.result.unwrap()).unwrap();
    assert_eq!(result.readiness, LocatorGraphReadiness::Ready);
}

// ---------------------------------------------------------------------------
// graph.refresh_snapshot tests
// ---------------------------------------------------------------------------

#[test]
fn graph_refresh_not_configured_without_workspace() {
    let adapter = mem_adapter();
    let req = graph_refresh_request(Some("HEAD"));
    let resp = dispatch_with_workspace(&req, &adapter, None);

    assert_eq!(resp.status, ToolResponseStatus::Ok);
    let result: GraphRefreshResult = serde_json::from_value(resp.result.unwrap()).unwrap();
    assert_eq!(result.status, GraphRefreshStatus::NotConfigured);
    assert!(result.target.is_none());
    assert!(result
        .next_actions
        .contains(&"configure_workspace".to_string()));
}

#[test]
fn graph_refresh_not_configured_does_not_append_events() {
    let adapter = mem_adapter();
    let req = graph_refresh_request(Some("HEAD"));
    let _ = dispatch_with_workspace(&req, &adapter, None);
    let events = adapter.replay().unwrap();
    assert!(
        events.is_empty(),
        "graph.refresh_snapshot must not append governance events"
    );
}

#[test]
fn graph_refresh_ready_with_git_workspace() {
    let adapter = mem_adapter();
    let req = graph_refresh_request(Some("HEAD"));
    let ws = workspace_root();
    let resp = dispatch_with_workspace(&req, &adapter, Some(&ws));

    assert_eq!(resp.status, ToolResponseStatus::Ok);
    let result: GraphRefreshResult = serde_json::from_value(resp.result.unwrap()).unwrap();
    assert_eq!(result.status, GraphRefreshStatus::Ready);
    assert!(result.target.is_some());
    assert!(result.snapshot.is_some());

    let target = result.target.unwrap();
    assert!(target.is_current_for_target);
    assert!(target.resolved_sha.is_some());
    assert!(target.current_head_sha.is_some());
}

#[test]
fn graph_refresh_ready_does_not_append_events() {
    let adapter = mem_adapter();
    let req = graph_refresh_request(Some("HEAD"));
    let ws = workspace_root();
    let _ = dispatch_with_workspace(&req, &adapter, Some(&ws));
    let events = adapter.replay().unwrap();
    assert!(
        events.is_empty(),
        "graph.refresh_snapshot must not append governance events even when ready"
    );
}

#[test]
fn graph_refresh_stale_for_unavailable_ref() {
    let adapter = mem_adapter();
    let req = graph_refresh_request(Some("refs/heads/nonexistent-branch-xyz-999"));
    let ws = workspace_root();
    let resp = dispatch_with_workspace(&req, &adapter, Some(&ws));

    assert_eq!(resp.status, ToolResponseStatus::Ok);
    let result: GraphRefreshResult = serde_json::from_value(resp.result.unwrap()).unwrap();
    // Should not be Ready for an unavailable ref
    assert_ne!(result.status, GraphRefreshStatus::Ready);
    assert!(result.target.is_some());
}

#[test]
fn graph_refresh_default_ref_is_head() {
    let adapter = mem_adapter();
    let req = graph_refresh_request(None);
    let ws = workspace_root();
    let resp = dispatch_with_workspace(&req, &adapter, Some(&ws));

    assert_eq!(resp.status, ToolResponseStatus::Ok);
    let result: GraphRefreshResult = serde_json::from_value(resp.result.unwrap()).unwrap();
    assert_eq!(result.status, GraphRefreshStatus::Ready);
}

#[test]
fn graph_refresh_snapshot_has_typed_target() {
    let adapter = mem_adapter();
    let req = graph_refresh_request(Some("HEAD"));
    let ws = workspace_root();
    let resp = dispatch_with_workspace(&req, &adapter, Some(&ws));

    let result: GraphRefreshResult = serde_json::from_value(resp.result.unwrap()).unwrap();
    let target = result.target.unwrap();
    assert_eq!(target.requested_ref, Some("HEAD".to_string()));
}

// ---------------------------------------------------------------------------
// Capability advertisement
// ---------------------------------------------------------------------------

#[test]
fn graph_commands_in_supported_commands() {
    use bicameral_runtime::dispatch::SUPPORTED_COMMANDS;
    assert!(
        SUPPORTED_COMMANDS.contains(&"graph.status"),
        "graph.status must be in SUPPORTED_COMMANDS"
    );
    assert!(
        SUPPORTED_COMMANDS.contains(&"graph.refresh_snapshot"),
        "graph.refresh_snapshot must be in SUPPORTED_COMMANDS"
    );
}

#[test]
fn graph_commands_not_in_deferred_commands() {
    use bicameral_runtime::dispatch::DEFERRED_COMMANDS;
    assert!(
        !DEFERRED_COMMANDS.contains(&"graph.status"),
        "graph.status must not be in DEFERRED_COMMANDS"
    );
    assert!(
        !DEFERRED_COMMANDS.contains(&"graph.refresh_snapshot"),
        "graph.refresh_snapshot must not be in DEFERRED_COMMANDS"
    );
}
