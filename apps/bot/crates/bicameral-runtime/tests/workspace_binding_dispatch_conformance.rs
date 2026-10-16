//! Workspace binding dispatch conformance (bicameral-bot#732).
//!
//! Proves the daemon-owned `workspace.bind` command routes through the local
//! `ToolRequest` dispatcher and that surfaces (MCP/CLI/dashboard) cannot bypass
//! daemon validation:
//!
//! 1. A confirmed bind for a registered project through a safe candidate path
//!    materializes durable operator-local `LocalWorkspaceBinding` state.
//! 2. Missing confirmation, unregistered project, and unsafe paths return typed
//!    rejections and never mutate stored state (no persisted record appears).
//! 3. Rebinding an already-bound project returns `already_bound` without
//!    overwriting the stored binding.
//! 4. A hosted-bridge actor is rejected by authority before any binding logic —
//!    the local candidate_path never crosses the bridge.
//! 5. Without the daemon's operator-local binding context, `workspace.bind`
//!    returns `unsupported_capability`; a surface cannot self-serve a binding.
//! 6. The success payload never carries the local candidate_path.
//!
//! All tests are deterministic — no network, no hosted graph, no credentials.

use bicameral_api::tool_request::*;
use bicameral_api::workspace_binding::{
    ProjectSafeDisplayMetadata, WorkspaceBindRequest, WorkspaceBindingProposal,
    WorkspaceBindingSourceSurface, WorkspaceBindingState, WorkspaceResolveRequest,
};
use bicameral_runtime::dispatch::{
    capability_report, capability_report_with_workspace_binding, dispatch_with_workspace,
    dispatch_with_workspace_binding, WorkspaceBindingDispatchContext,
};
use bicameral_runtime::event_store_adapters::MemoryAdapter;
use bicameral_runtime::workspace_binding::{
    FileWorkspaceBindingStore, StaticWorkspaceBindingRegistry, WorkspaceBindingStore,
};
use std::path::Path;

const PROJECT_ID: &str = "proj_ada";

fn proposal(
    candidate_path: &str,
    surface: WorkspaceBindingSourceSurface,
) -> WorkspaceBindingProposal {
    proposal_for(PROJECT_ID, candidate_path, surface)
}

fn proposal_for(
    project_id: &str,
    candidate_path: &str,
    surface: WorkspaceBindingSourceSurface,
) -> WorkspaceBindingProposal {
    WorkspaceBindingProposal {
        project_id: project_id.into(),
        display: ProjectSafeDisplayMetadata {
            display_name: "Ada Service".into(),
            project_slug: Some("ada-service".into()),
            candidate_label: Some("ada-service".into()),
        },
        candidate_path: candidate_path.into(),
        source_surface: surface,
        reason: "Open editor workspace matches the registered project.".into(),
        confidence: 0.92,
    }
}

fn bind_request(
    candidate_path: &str,
    confirmed: bool,
    surface: WorkspaceBindingSourceSurface,
) -> WorkspaceBindRequest {
    WorkspaceBindRequest {
        proposal: proposal(candidate_path, surface),
        confirmed,
        required_daemon_capability: None,
        expected_current_state: Some(WorkspaceBindingState::LocalWorkspaceUnbound),
    }
}

fn mcp_request(candidate_path: &str, confirmed: bool) -> ToolRequest {
    let authority = bicameral_api::fixtures::authority_mcp("agent-732", "sess-732", "/workspace");
    ToolRequest::new(
        ToolCommand::WorkspaceBind(bind_request(
            candidate_path,
            confirmed,
            WorkspaceBindingSourceSurface::Mcp,
        )),
        authority,
    )
}

fn hosted_bridge_authority() -> AuthorityContext {
    AuthorityContext {
        actor_id: "bridge-actor".into(),
        auth_method: AuthMethod::HostedBridge,
        session_id: Some("bridge-sess".into()),
        workspace: None,
        policy_scope: vec!["default".into()],
        audit_metadata: serde_json::Map::new(),
    }
}

/// Registry with `proj_ada` registered and no session scoping.
fn registry() -> StaticWorkspaceBindingRegistry {
    StaticWorkspaceBindingRegistry::new([PROJECT_ID.to_string()], None)
}

/// Count persisted binding record files under a store root.
fn persisted_count(root: &Path) -> usize {
    match std::fs::read_dir(root) {
        Ok(entries) => entries.filter_map(Result::ok).count(),
        Err(_) => 0,
    }
}

#[test]
fn confirmed_bind_for_registered_project_materializes_state() {
    let ws = tempfile::tempdir().unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    let adapter = MemoryAdapter::new();
    let registry = registry();
    let store = FileWorkspaceBindingStore::new(store_dir.path());
    let ctx = WorkspaceBindingDispatchContext {
        registry: &registry,
        store: &store,
    };

    let req = mcp_request(&ws.path().to_string_lossy(), true);
    let resp = dispatch_with_workspace_binding(&req, &adapter, None, None, &ctx);

    assert_eq!(
        resp.status,
        ToolResponseStatus::Ok,
        "message: {:?}",
        resp.message
    );
    let result = resp.result.expect("success carries a result payload");
    assert_eq!(result["status"], "bound");
    assert_eq!(result["outcome"]["project_id"], PROJECT_ID);
    assert_eq!(result["outcome"]["state"], "local_workspace_bound");

    // Durable operator-local record persisted through the daemon store.
    let stored = store.load(PROJECT_ID).unwrap().expect("binding persisted");
    assert_eq!(stored.state, WorkspaceBindingState::LocalWorkspaceBound);
    assert_eq!(stored.candidate_path, ws.path().to_string_lossy());
}

#[test]
fn one_operator_daemon_binds_two_registered_projects_without_process_workspace() {
    let ws_a = tempfile::tempdir().unwrap();
    let ws_b = tempfile::tempdir().unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    let adapter = MemoryAdapter::new();
    let registry = StaticWorkspaceBindingRegistry::new(
        ["proj_ada".to_string(), "proj_babbage".to_string()],
        None,
    );
    let store = FileWorkspaceBindingStore::new(store_dir.path());
    let ctx = WorkspaceBindingDispatchContext {
        registry: &registry,
        store: &store,
    };

    for (project_id, workspace) in [("proj_ada", &ws_a), ("proj_babbage", &ws_b)] {
        let request = ToolRequest::new(
            ToolCommand::WorkspaceBind(WorkspaceBindRequest {
                proposal: proposal_for(
                    project_id,
                    &workspace.path().to_string_lossy(),
                    WorkspaceBindingSourceSurface::Mcp,
                ),
                confirmed: true,
                required_daemon_capability: None,
                expected_current_state: Some(WorkspaceBindingState::LocalWorkspaceUnbound),
            }),
            bicameral_api::fixtures::authority_mcp(
                "agent-908",
                "session-908",
                "/unrelated/process/cwd",
            ),
        );

        let response = dispatch_with_workspace_binding(&request, &adapter, None, None, &ctx);
        assert_eq!(
            response.status,
            ToolResponseStatus::Ok,
            "{project_id}: {:?}",
            response.message
        );
    }

    assert_eq!(
        store.load("proj_ada").unwrap().unwrap().candidate_path,
        ws_a.path().to_string_lossy()
    );
    assert_eq!(
        store.load("proj_babbage").unwrap().unwrap().candidate_path,
        ws_b.path().to_string_lossy()
    );
}

#[test]
fn missing_confirmation_is_typed_and_never_persists() {
    let ws = tempfile::tempdir().unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    let adapter = MemoryAdapter::new();
    let registry = registry();
    let store = FileWorkspaceBindingStore::new(store_dir.path());
    let ctx = WorkspaceBindingDispatchContext {
        registry: &registry,
        store: &store,
    };

    let req = mcp_request(&ws.path().to_string_lossy(), false);
    let resp = dispatch_with_workspace_binding(&req, &adapter, None, None, &ctx);

    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    let result = resp
        .result
        .expect("typed rejection carries structured payload");
    assert_eq!(result["error"], "confirmation_missing");
    assert_eq!(
        persisted_count(store_dir.path()),
        0,
        "no record must be written"
    );
}

#[test]
fn unregistered_project_is_rejected_without_mutation() {
    let ws = tempfile::tempdir().unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    let adapter = MemoryAdapter::new();
    let empty_registry = StaticWorkspaceBindingRegistry::new(Vec::<String>::new(), None);
    let store = FileWorkspaceBindingStore::new(store_dir.path());
    let ctx = WorkspaceBindingDispatchContext {
        registry: &empty_registry,
        store: &store,
    };

    let req = mcp_request(&ws.path().to_string_lossy(), true);
    let resp = dispatch_with_workspace_binding(&req, &adapter, None, None, &ctx);

    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert_eq!(resp.result.unwrap()["error"], "unregistered_project");
    assert_eq!(persisted_count(store_dir.path()), 0);
}

#[test]
fn unsafe_nonexistent_path_is_rejected_without_mutation() {
    let store_dir = tempfile::tempdir().unwrap();
    let adapter = MemoryAdapter::new();
    let registry = registry();
    let store = FileWorkspaceBindingStore::new(store_dir.path());
    let ctx = WorkspaceBindingDispatchContext {
        registry: &registry,
        store: &store,
    };

    let req = mcp_request("/nonexistent/path/for-732", true);
    let resp = dispatch_with_workspace_binding(&req, &adapter, None, None, &ctx);

    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert_eq!(resp.result.unwrap()["error"], "unsafe_path");
    assert_eq!(persisted_count(store_dir.path()), 0);
}

#[test]
fn rebinding_bound_project_returns_already_bound() {
    let ws = tempfile::tempdir().unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    let adapter = MemoryAdapter::new();
    let registry = registry();
    let store = FileWorkspaceBindingStore::new(store_dir.path());
    let ctx = WorkspaceBindingDispatchContext {
        registry: &registry,
        store: &store,
    };

    // First bind succeeds.
    let first = dispatch_with_workspace_binding(
        &mcp_request(&ws.path().to_string_lossy(), true),
        &adapter,
        None,
        None,
        &ctx,
    );
    assert_eq!(first.status, ToolResponseStatus::Ok);

    // Second bind is rejected as already-bound; stored record is unchanged.
    let second = dispatch_with_workspace_binding(
        &mcp_request(&ws.path().to_string_lossy(), true),
        &adapter,
        None,
        None,
        &ctx,
    );
    assert_eq!(second.status, ToolResponseStatus::Rejected);
    let result = second.result.unwrap();
    assert_eq!(result["error"], "already_bound");
    assert_eq!(result["state"], "local_workspace_bound");
    assert_eq!(persisted_count(store_dir.path()), 1);
}

#[test]
fn hosted_bridge_actor_cannot_bind() {
    let ws = tempfile::tempdir().unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    let adapter = MemoryAdapter::new();
    let registry = registry();
    let store = FileWorkspaceBindingStore::new(store_dir.path());
    let ctx = WorkspaceBindingDispatchContext {
        registry: &registry,
        store: &store,
    };

    let req = ToolRequest::new(
        ToolCommand::WorkspaceBind(bind_request(
            &ws.path().to_string_lossy(),
            true,
            WorkspaceBindingSourceSurface::Mcp,
        )),
        hosted_bridge_authority(),
    );
    let resp = dispatch_with_workspace_binding(&req, &adapter, None, None, &ctx);

    // Authority denial happens before any binding logic runs.
    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert!(
        resp.message
            .as_deref()
            .unwrap_or_default()
            .contains("unauthorized"),
        "expected an authority rejection, got: {:?}",
        resp.message
    );
    assert_eq!(persisted_count(store_dir.path()), 0);
}

#[test]
fn workspace_bind_without_daemon_context_is_unsupported() {
    let ws = tempfile::tempdir().unwrap();
    let adapter = MemoryAdapter::new();

    // The pure dispatch entrypoint has no operator-local binding store, so a
    // surface routing workspace.bind through it cannot self-serve a binding.
    let req = mcp_request(&ws.path().to_string_lossy(), true);
    let resp = dispatch_with_workspace(&req, &adapter, None);

    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert!(
        resp.message
            .as_deref()
            .unwrap_or_default()
            .contains("unsupported_capability"),
        "expected unsupported_capability, got: {:?}",
        resp.message
    );
}

#[test]
fn success_payload_never_carries_candidate_path() {
    let ws = tempfile::tempdir().unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    let adapter = MemoryAdapter::new();
    let registry = registry();
    let store = FileWorkspaceBindingStore::new(store_dir.path());
    let ctx = WorkspaceBindingDispatchContext {
        registry: &registry,
        store: &store,
    };

    let req = mcp_request(&ws.path().to_string_lossy(), true);
    let resp = dispatch_with_workspace_binding(&req, &adapter, None, None, &ctx);
    let json = serde_json::to_value(&resp).unwrap();

    assert!(!contains_key_deep(&json, "candidate_path"));
    assert!(!contains_absolute_path(&json["result"]["outcome"]));
}

// ─── Capability discovery availability transition (bicameral-bot#747) ──────
//
// Capability discovery must advertise workspace binding as available only when
// the daemon can actually route it. `workspace.bind` stays a
// protocol-recognized command in `SUPPORTED_COMMANDS`, but the
// `workspace_binding_available` flag is the truthful routability signal that
// mirrors whether the dispatch context (operator-local store) is present.

#[test]
fn capability_report_advertises_binding_available_when_routable() {
    let report = capability_report_with_workspace_binding(
        bicameral_api::reasoning_projection::ProjectionProviderReadiness::not_configured(),
        true,
    );
    assert!(
        report.workspace_binding_available,
        "capability discovery must advertise workspace binding when routable"
    );
    assert!(
        report.supported_commands.contains(&"workspace.bind"),
        "workspace.bind must remain in supported/routable command metadata"
    );

    // The availability flag must match a runtime that actually routes a bind:
    // a dispatch context (operator-local store) is present.
    let ws = tempfile::tempdir().unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    let adapter = MemoryAdapter::new();
    let registry = registry();
    let store = FileWorkspaceBindingStore::new(store_dir.path());
    let ctx = WorkspaceBindingDispatchContext {
        registry: &registry,
        store: &store,
    };
    let resp = dispatch_with_workspace_binding(
        &mcp_request(&ws.path().to_string_lossy(), true),
        &adapter,
        None,
        None,
        &ctx,
    );
    assert_eq!(
        resp.status,
        ToolResponseStatus::Ok,
        "an available capability must actually route a bind: {:?}",
        resp.message
    );
}

#[test]
fn capability_report_advertises_binding_unavailable_when_not_routable() {
    // The default/pure-runtime report has no operator-local binding store.
    let report = capability_report();
    assert!(
        !report.workspace_binding_available,
        "capability discovery must not advertise workspace binding when the \
         daemon cannot route it"
    );
    // The command remains recognized at the protocol level even when it is not
    // currently routable; only the availability flag flips.
    assert!(
        report.supported_commands.contains(&"workspace.bind"),
        "workspace.bind stays a recognized command; only availability flips"
    );

    // The unavailable flag must match a runtime that fails closed: dispatching
    // without a context returns unsupported_capability.
    let ws = tempfile::tempdir().unwrap();
    let adapter = MemoryAdapter::new();
    let resp = dispatch_with_workspace(
        &mcp_request(&ws.path().to_string_lossy(), true),
        &adapter,
        None,
    );
    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert!(
        resp.message
            .as_deref()
            .unwrap_or_default()
            .contains("unsupported_capability"),
        "an unavailable capability must fail closed, got: {:?}",
        resp.message
    );
}

fn resolve_request(candidate_path: &str) -> ToolRequest {
    let authority = bicameral_api::fixtures::authority_mcp("agent-831", "sess-831", "/workspace");
    ToolRequest::new(
        ToolCommand::WorkspaceResolve(WorkspaceResolveRequest {
            candidate_path: candidate_path.into(),
        }),
        authority,
    )
}

#[test]
fn resolve_unique_bound_path_returns_product_id() {
    let ws = tempfile::tempdir().unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    let adapter = MemoryAdapter::new();
    let registry = registry();
    let store = FileWorkspaceBindingStore::new(store_dir.path());
    let ctx = WorkspaceBindingDispatchContext {
        registry: &registry,
        store: &store,
    };

    let bound = dispatch_with_workspace_binding(
        &mcp_request(&ws.path().to_string_lossy(), true),
        &adapter,
        None,
        None,
        &ctx,
    );
    assert_eq!(bound.status, ToolResponseStatus::Ok);

    let resp = dispatch_with_workspace_binding(
        &resolve_request(&ws.path().to_string_lossy()),
        &adapter,
        None,
        None,
        &ctx,
    );
    assert_eq!(resp.status, ToolResponseStatus::Ok, "{:?}", resp.message);
    let outcome = &resp.result.unwrap()["outcome"];
    assert_eq!(outcome["product_id"], PROJECT_ID);
    assert_eq!(outcome["state"], "local_workspace_bound");
    assert!(outcome.get("candidate_path").is_none());
}

#[test]
fn resolve_unbound_path_is_typed_unbound() {
    let ws = tempfile::tempdir().unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    let adapter = MemoryAdapter::new();
    let registry = registry();
    let store = FileWorkspaceBindingStore::new(store_dir.path());
    let ctx = WorkspaceBindingDispatchContext {
        registry: &registry,
        store: &store,
    };

    let resp = dispatch_with_workspace_binding(
        &resolve_request(&ws.path().to_string_lossy()),
        &adapter,
        None,
        None,
        &ctx,
    );
    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert_eq!(resp.result.unwrap()["error"], "unbound");
}

#[test]
fn resolve_does_not_match_child_folder() {
    let parent = tempfile::tempdir().unwrap();
    let child = parent.path().join("nested");
    std::fs::create_dir(&child).unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    let adapter = MemoryAdapter::new();
    let registry = registry();
    let store = FileWorkspaceBindingStore::new(store_dir.path());
    let ctx = WorkspaceBindingDispatchContext {
        registry: &registry,
        store: &store,
    };

    let bound = dispatch_with_workspace_binding(
        &mcp_request(&parent.path().to_string_lossy(), true),
        &adapter,
        None,
        None,
        &ctx,
    );
    assert_eq!(bound.status, ToolResponseStatus::Ok);

    let resp = dispatch_with_workspace_binding(
        &resolve_request(&child.to_string_lossy()),
        &adapter,
        None,
        None,
        &ctx,
    );
    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert_eq!(resp.result.unwrap()["error"], "unbound");
}

#[test]
fn hosted_bridge_actor_cannot_resolve() {
    let ws = tempfile::tempdir().unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    let adapter = MemoryAdapter::new();
    let registry = registry();
    let store = FileWorkspaceBindingStore::new(store_dir.path());
    let ctx = WorkspaceBindingDispatchContext {
        registry: &registry,
        store: &store,
    };

    let req = ToolRequest::new(
        ToolCommand::WorkspaceResolve(WorkspaceResolveRequest {
            candidate_path: ws.path().to_string_lossy().into_owned(),
        }),
        hosted_bridge_authority(),
    );
    let resp = dispatch_with_workspace_binding(&req, &adapter, None, None, &ctx);
    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert!(
        resp.message
            .as_deref()
            .unwrap_or_default()
            .contains("unauthorized"),
        "expected an authority rejection, got: {:?}",
        resp.message
    );
}

#[test]
fn workspace_resolve_without_daemon_context_is_unsupported() {
    let ws = tempfile::tempdir().unwrap();
    let adapter = MemoryAdapter::new();
    let resp = dispatch_with_workspace(
        &resolve_request(&ws.path().to_string_lossy()),
        &adapter,
        None,
    );
    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert!(
        resp.message
            .as_deref()
            .unwrap_or_default()
            .contains("unsupported_capability"),
        "expected unsupported_capability, got: {:?}",
        resp.message
    );
}

fn contains_key_deep(value: &serde_json::Value, key: &str) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            map.contains_key(key) || map.values().any(|v| contains_key_deep(v, key))
        }
        serde_json::Value::Array(items) => items.iter().any(|v| contains_key_deep(v, key)),
        _ => false,
    }
}

fn contains_absolute_path(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(s) => s.starts_with('/'),
        serde_json::Value::Array(items) => items.iter().any(contains_absolute_path),
        serde_json::Value::Object(map) => map.values().any(contains_absolute_path),
        _ => false,
    }
}
