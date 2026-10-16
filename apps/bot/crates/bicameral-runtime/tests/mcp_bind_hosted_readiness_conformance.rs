//! Cross-repo conformance: MCP bind → hosted readiness (bicameral-bot#735).
//!
//! This is the completion-validation leaf of the workspace-binding alpha chain.
//! It proves the **composed** MCP-assisted-bind → daemon → hosted-bridge
//! readiness lifecycle and its failure paths end-to-end, against the merged
//! product surfaces (no fixtures/fake-daemon shortcut for the daemon leg):
//!
//! - MCP path (bicameral-mcp#706 / #708): the merged capability-gated
//!   `workspace.bind` user path. The bot side models the thin-client mapping as
//!   an `AuthorityContext`-carrying `ToolRequest` routed through the daemon
//!   dispatcher (see `bot_mcp_thin_client_conformance.rs`,
//!   `workspace_binding_dispatch_conformance.rs`).
//! - Daemon (bicameral-bot#731 protocol, #732 dispatch/materialization, #747
//!   capability discovery): `classify_bind_request` / `bind_workspace` /
//!   `dispatch_with_workspace_binding`, the sole binding authority.
//! - Hosted bridge readiness (bicameral-bot#733):
//!   `resolve_selected_project_readiness` /
//!   `project_workspace_binding_readiness` →
//!   `SelectedProjectSessionResponse.workspace_readiness`.
//! - Hosted dashboard CTA (bicameral-bot#734, PR #741): the renderer-only bind
//!   CTA / polling gate. That leg is TypeScript and is covered by the dashboard
//!   Vitest suite (`dashboard/src/workspace-binding-readiness.test.ts`); this
//!   harness proves the daemon-owned readiness projection the CTA consumes.
//!
//! # git-remote agreement is evidence, not authority
//!
//! A candidate local workspace's git remote is **evidence for proposal /
//! conformance only**. `project_id` remains the authority key; daemon
//! validation and materialization remain required; the hosted/cloud tier never
//! gains binding authority and never persists a local absolute path. The
//! remote-agreement classifier modeled here ([`normalize_git_remote`] /
//! [`classify_remote_agreement`]) is deliberately **test-local**: it models the
//! proposal-time evidence a proposing surface (MCP) would weigh before asking
//! for confirmation. It is not a new product authority surface (see the PR
//! body's domain-entity-candidate + factory-learning-signal notes).
//!
//! Acceptance criteria proven here (bicameral-bot#735):
//!
//! 1. git-remote agreement for readiness:
//!    - matching candidate remote ⇒ hosted + MCP + daemon readiness agree the
//!      bind can proceed after confirmation, and it materializes;
//!    - contradictory candidate remote (normalizes to a *different* registered
//!      project) ⇒ fail closed: no binding is materialized and readiness stays
//!      unbound;
//!    - SSH and HTTPS remotes normalize equal.
//! 2. Authority invariant: `project_id` is the authority key; a matching remote
//!    cannot substitute for registration, cannot re-route binding to another
//!    project, and no hosted-safe projection carries a local path/remote.
//! 3. Composed lifecycle + failure paths: happy path, cancelled/rejected path
//!    leaves the project unbound, and an unsupported daemon capability fails
//!    closed with upgrade/repair guidance.
//!
//! All tests are deterministic — no network, no hosted graph, no credentials.

use bicameral_api::fixtures;
use bicameral_api::hosted_bridge::{
    resolve_selected_project_readiness, DaemonBindingReadiness, HostedBridgePairingState,
    ProjectReadinessEntry, SelectedProjectRef, WorkspaceReadinessActionKind,
};
use bicameral_api::tool_request::*;
use bicameral_api::workspace_binding::{
    ProjectSafeDisplayMetadata, WorkspaceBindRequest, WorkspaceBindingProposal,
    WorkspaceBindingSourceSurface, WorkspaceBindingState,
};
use bicameral_runtime::dispatch::{
    capability_report_with_workspace_binding, dispatch_with_workspace,
    dispatch_with_workspace_binding, WorkspaceBindingDispatchContext,
};
use bicameral_runtime::event_store_adapters::MemoryAdapter;
use bicameral_runtime::workspace_binding::{
    FileWorkspaceBindingStore, StaticWorkspaceBindingRegistry, WorkspaceBindingStore,
    DAEMON_WORKSPACE_BINDING_CAPABILITY,
};

// ─── git-remote agreement evidence model (test-local, non-authoritative) ────
//
// Models the proposal-time evidence a proposing surface (MCP) weighs before
// asking the operator to confirm a bind. This is NOT product authority: the
// daemon still validates against `project_id`, and the remote never grants a
// binding. See the module header.

/// Normalize a git remote URL to a stable `owner/repo` identifier.
///
/// SSH (`git@github.com:owner/repo.git`) and HTTPS
/// (`https://github.com/owner/repo`) forms normalize to the same
/// `owner/repo` string, matching the issue's stated rule
/// (`BicameralAI/ada-service`). Case is preserved; agreement comparison is
/// case-insensitive because GitHub owners/repos are case-insensitive.
fn normalize_git_remote(remote: &str) -> String {
    let trimmed = remote.trim();
    let trimmed = trimmed.strip_suffix('/').unwrap_or(trimmed);
    let without_suffix = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    for prefix in [
        "git@github.com:",
        "ssh://git@github.com/",
        "https://github.com/",
        "http://github.com/",
    ] {
        if let Some(rest) = without_suffix.strip_prefix(prefix) {
            return rest.to_string();
        }
    }
    without_suffix.to_string()
}

/// Whether two git remotes refer to the same repository.
fn remotes_agree(a: &str, b: &str) -> bool {
    normalize_git_remote(a).eq_ignore_ascii_case(&normalize_git_remote(b))
}

/// Proposal-time agreement verdict between a candidate workspace's git remote
/// and the set of registered projects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteAgreement {
    /// Candidate remote matches the *selected* project's registered source ref.
    Matching,
    /// Candidate remote matches a *different* registered project — the operator
    /// almost certainly selected the wrong project. Fail closed.
    Contradictory,
    /// Candidate remote matches no registered project's source ref. No
    /// corroborating evidence either way; do not auto-proceed.
    Unknown,
}

/// A registered project's identity: opaque `project_id` (the authority key) and
/// its declared git source ref (evidence only).
struct RegisteredProject {
    project_id: &'static str,
    source_ref: &'static str,
}

const PROJECTS: &[RegisteredProject] = &[
    RegisteredProject {
        project_id: "proj_ada",
        source_ref: "git@github.com:BicameralAI/ada-service.git",
    },
    RegisteredProject {
        project_id: "proj_beacon",
        source_ref: "https://github.com/BicameralAI/beacon-service",
    },
];

fn source_ref_for(project_id: &str) -> &'static str {
    PROJECTS
        .iter()
        .find(|p| p.project_id == project_id)
        .expect("known registered project")
        .source_ref
}

/// Classify the git-remote evidence for binding `selected_project_id` to a
/// candidate workspace whose git remote is `candidate_remote`.
fn classify_remote_agreement(selected_project_id: &str, candidate_remote: &str) -> RemoteAgreement {
    if remotes_agree(candidate_remote, source_ref_for(selected_project_id)) {
        return RemoteAgreement::Matching;
    }
    if PROJECTS
        .iter()
        .any(|p| remotes_agree(candidate_remote, p.source_ref))
    {
        return RemoteAgreement::Contradictory;
    }
    RemoteAgreement::Unknown
}

// ─── Composed lifecycle helpers ─────────────────────────────────────────────

fn proposal(project_id: &str, candidate_path: &str) -> WorkspaceBindingProposal {
    WorkspaceBindingProposal {
        project_id: project_id.into(),
        display: ProjectSafeDisplayMetadata {
            display_name: "Ada Service".into(),
            project_slug: Some("ada-service".into()),
            candidate_label: Some("ada-service".into()),
        },
        candidate_path: candidate_path.into(),
        source_surface: WorkspaceBindingSourceSurface::Mcp,
        reason: "Open editor workspace git remote matches the registered project.".into(),
        confidence: 0.92,
    }
}

fn bind_request(
    project_id: &str,
    candidate_path: &str,
    confirmed: bool,
    required_daemon_capability: Option<u32>,
) -> WorkspaceBindRequest {
    WorkspaceBindRequest {
        proposal: proposal(project_id, candidate_path),
        confirmed,
        required_daemon_capability,
        expected_current_state: Some(WorkspaceBindingState::LocalWorkspaceUnbound),
    }
}

/// Build the MCP-authored `ToolRequest` (bicameral-mcp#706/#708 path, modeled
/// on the bot side as an `McpSession` `AuthorityContext`).
fn mcp_bind(
    project_id: &str,
    candidate_path: &str,
    confirmed: bool,
    required_daemon_capability: Option<u32>,
) -> ToolRequest {
    ToolRequest::new(
        ToolCommand::WorkspaceBind(bind_request(
            project_id,
            candidate_path,
            confirmed,
            required_daemon_capability,
        )),
        fixtures::authority_mcp("agent-735", "sess-735", "/workspace"),
    )
}

/// Registry with both projects registered, session-scoped to `session_project`.
fn registry(session_project: Option<&str>) -> StaticWorkspaceBindingRegistry {
    StaticWorkspaceBindingRegistry::new(
        PROJECTS.iter().map(|p| p.project_id.to_string()),
        session_project.map(str::to_string),
    )
}

/// Reflect the daemon-owned durable binding store into the hosted-bridge
/// readiness projection for `project_id` (bicameral-bot#733), exactly as the
/// gateway would after a paired session selects the project. This is the
/// daemon → hosted-readiness leg of the composed lifecycle.
fn hosted_readiness(
    store: &dyn WorkspaceBindingStore,
    project_id: &str,
) -> bicameral_api::hosted_bridge::SelectedProjectSessionResponse {
    let bound = store
        .load(project_id)
        .expect("store read")
        .map(|b| b.state == WorkspaceBindingState::LocalWorkspaceBound)
        .unwrap_or(false);

    let local_projects = vec![ProjectReadinessEntry {
        name: project_id.to_string(),
        event_store_healthy: true,
        has_workspace_binding: bound,
        setup_state: "normal".to_string(),
    }];

    let selected = SelectedProjectRef {
        project_id: project_id.to_string(),
        team_id: None,
        display_name: None,
    };
    resolve_selected_project_readiness(
        HostedBridgePairingState::Paired,
        &selected,
        &local_projects,
        "2026-07-09T00:00:00Z",
    )
}

/// Assert a serialized hosted-safe projection carries no local path or remote.
fn assert_no_path_or_remote_leak(value: &serde_json::Value) {
    fn walk(v: &serde_json::Value) {
        match v {
            serde_json::Value::String(s) => {
                assert!(
                    !s.starts_with('/'),
                    "hosted-safe view leaked absolute path: {s}"
                );
                assert!(
                    !s.contains("github.com") && !s.contains("git@"),
                    "hosted-safe view leaked a git remote: {s}"
                );
            }
            serde_json::Value::Array(items) => items.iter().for_each(walk),
            serde_json::Value::Object(map) => {
                assert!(
                    !map.contains_key("candidate_path") && !map.contains_key("workspace_path"),
                    "hosted-safe view exposed a local-only path field"
                );
                map.values().for_each(walk);
            }
            _ => {}
        }
    }
    walk(value);
}

// ═══════════════════════════════════════════════════════════════════════════
// AC#1a — matching remote: composed readiness agrees, binding materializes
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn matching_remote_proceeds_and_materializes_through_composed_lifecycle() {
    let ws = tempfile::tempdir().unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    let adapter = MemoryAdapter::new();
    let registry = registry(Some("proj_ada"));
    let store = FileWorkspaceBindingStore::new(store_dir.path());
    let ctx = WorkspaceBindingDispatchContext {
        registry: &registry,
        store: &store,
    };

    // Candidate workspace's git remote (HTTPS form) matches proj_ada's SSH ref.
    let candidate_remote = "https://github.com/BicameralAI/ada-service";
    assert_eq!(
        classify_remote_agreement("proj_ada", candidate_remote),
        RemoteAgreement::Matching,
    );

    // Before binding: hosted readiness says unbound + offers the #734 bind CTA.
    let before = hosted_readiness(&store, "proj_ada");
    assert_eq!(before.binding_readiness, DaemonBindingReadiness::Unbound);
    let ws_before = before.workspace_readiness.as_ref().unwrap();
    assert_eq!(
        ws_before.binding_state,
        WorkspaceBindingState::LocalWorkspaceUnbound
    );
    assert!(!ws_before.code_grounding_ready);
    let action = ws_before
        .next_action
        .as_ref()
        .expect("unbound offers a CTA");
    assert_eq!(
        action.kind,
        WorkspaceReadinessActionKind::BindLocalWorkspace
    );
    assert!(action
        .surfaces
        .contains(&WorkspaceBindingSourceSurface::Mcp));
    assert!(action
        .surfaces
        .contains(&WorkspaceBindingSourceSurface::Cli));

    // Matching evidence ⇒ the surface confirms; the daemon validates + binds.
    let resp = dispatch_with_workspace_binding(
        &mcp_bind("proj_ada", &ws.path().to_string_lossy(), true, None),
        &adapter,
        None,
        None,
        &ctx,
    );
    assert_eq!(
        resp.status,
        ToolResponseStatus::Ok,
        "message: {:?}",
        resp.message
    );
    let result = resp.result.as_ref().expect("success payload");
    assert_eq!(result["status"], "bound");
    assert_eq!(result["outcome"]["state"], "local_workspace_bound");

    // Daemon materialized the durable operator-local record.
    let stored = store.load("proj_ada").unwrap().expect("binding persisted");
    assert_eq!(stored.state, WorkspaceBindingState::LocalWorkspaceBound);

    // After binding: hosted readiness agrees bound + code-grounding ready, and
    // the #734 CTA disappears — all three readiness views agree.
    let after = hosted_readiness(&store, "proj_ada");
    assert_eq!(after.binding_readiness, DaemonBindingReadiness::Ready);
    let ws_after = after.workspace_readiness.as_ref().unwrap();
    assert_eq!(
        ws_after.binding_state,
        WorkspaceBindingState::LocalWorkspaceBound
    );
    assert!(ws_after.code_grounding_ready);
    assert!(ws_after.next_action.is_none());

    // No hosted-safe response carries a local path or git remote.
    assert_no_path_or_remote_leak(&serde_json::to_value(&after).unwrap());
    let dispatch_json = serde_json::to_value(&resp).unwrap();
    assert!(!contains_key_deep(&dispatch_json, "candidate_path"));
}

// ═══════════════════════════════════════════════════════════════════════════
// AC#1b — contradictory remote: fail closed, no binding materialized
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn contradictory_remote_fails_closed_and_never_materializes() {
    let ws = tempfile::tempdir().unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    let adapter = MemoryAdapter::new();
    let registry = registry(Some("proj_ada"));
    let store = FileWorkspaceBindingStore::new(store_dir.path());
    let ctx = WorkspaceBindingDispatchContext {
        registry: &registry,
        store: &store,
    };

    // Operator selected proj_ada, but the candidate workspace's remote points
    // at proj_beacon — a *different* registered project. Evidence contradicts.
    let candidate_remote = "git@github.com:BicameralAI/beacon-service.git";
    assert_eq!(
        classify_remote_agreement("proj_ada", candidate_remote),
        RemoteAgreement::Contradictory,
    );

    // Fail closed: the proposing surface withholds confirmation on
    // contradictory evidence (it surfaces repair/reject instead of binding).
    let withheld = !matches!(
        classify_remote_agreement("proj_ada", candidate_remote),
        RemoteAgreement::Matching
    );
    assert!(
        withheld,
        "contradictory evidence must withhold confirmation"
    );

    let resp = dispatch_with_workspace_binding(
        &mcp_bind("proj_ada", &ws.path().to_string_lossy(), !withheld, None),
        &adapter,
        None,
        None,
        &ctx,
    );
    // Daemon rejects the unconfirmed request; nothing is materialized.
    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert_eq!(resp.result.unwrap()["error"], "confirmation_missing");
    assert!(
        store.load("proj_ada").unwrap().is_none(),
        "no binding materialized"
    );

    // Hosted readiness stays unbound and keeps offering the bind/repair CTA.
    let after = hosted_readiness(&store, "proj_ada");
    assert_eq!(after.binding_readiness, DaemonBindingReadiness::Unbound);
    let ws_after = after.workspace_readiness.as_ref().unwrap();
    assert!(!ws_after.code_grounding_ready);
    assert!(ws_after.next_action.is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// AC#2 — authority invariant: project_id is authority, remote is evidence only
// ═══════════════════════════════════════════════════════════════════════════

/// Even with a *matching* candidate remote, a contradictory remote cannot
/// re-route binding to the project it points at when the daemon session is
/// scoped to another project: `project_id` authority (`WrongProject`) wins.
#[test]
fn matching_remote_cannot_reroute_binding_across_session_scope() {
    let ws = tempfile::tempdir().unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    let adapter = MemoryAdapter::new();
    // Session is scoped to proj_ada.
    let registry = registry(Some("proj_ada"));
    let store = FileWorkspaceBindingStore::new(store_dir.path());
    let ctx = WorkspaceBindingDispatchContext {
        registry: &registry,
        store: &store,
    };

    // The candidate remote matches proj_beacon, and the surface tries to bind
    // proj_beacon (confirmed) — but the session authority is proj_ada.
    let candidate_remote = "https://github.com/BicameralAI/beacon-service";
    assert_eq!(
        classify_remote_agreement("proj_beacon", candidate_remote),
        RemoteAgreement::Matching,
    );

    let resp = dispatch_with_workspace_binding(
        &mcp_bind("proj_beacon", &ws.path().to_string_lossy(), true, None),
        &adapter,
        None,
        None,
        &ctx,
    );
    // Authority key (project_id vs session scope) rejects it; remote evidence
    // does not grant a binding.
    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert_eq!(resp.result.unwrap()["error"], "wrong_project");
    assert!(store.load("proj_beacon").unwrap().is_none());
    assert!(store.load("proj_ada").unwrap().is_none());
}

/// A matching remote cannot substitute for project registration: an
/// unregistered project fails closed even when the candidate remote agrees.
#[test]
fn matching_remote_does_not_substitute_for_registration() {
    let ws = tempfile::tempdir().unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    let adapter = MemoryAdapter::new();
    // Empty registry: no project is registered.
    let empty = StaticWorkspaceBindingRegistry::new(Vec::<String>::new(), None);
    let store = FileWorkspaceBindingStore::new(store_dir.path());
    let ctx = WorkspaceBindingDispatchContext {
        registry: &empty,
        store: &store,
    };

    // The remote agrees with proj_ada's ref, but proj_ada is not registered.
    assert_eq!(
        classify_remote_agreement("proj_ada", "git@github.com:BicameralAI/ada-service.git"),
        RemoteAgreement::Matching,
    );

    let resp = dispatch_with_workspace_binding(
        &mcp_bind("proj_ada", &ws.path().to_string_lossy(), true, None),
        &adapter,
        None,
        None,
        &ctx,
    );
    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert_eq!(resp.result.unwrap()["error"], "unregistered_project");
    assert!(store.load("proj_ada").unwrap().is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// AC#3 — composed failure paths: cancel/reject, unsupported capability
// ═══════════════════════════════════════════════════════════════════════════

/// Cancelled/rejected path leaves the project unbound (issue AC).
#[test]
fn cancelled_confirmation_leaves_project_unbound() {
    let ws = tempfile::tempdir().unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    let adapter = MemoryAdapter::new();
    let registry = registry(Some("proj_ada"));
    let store = FileWorkspaceBindingStore::new(store_dir.path());
    let ctx = WorkspaceBindingDispatchContext {
        registry: &registry,
        store: &store,
    };

    // Matching evidence, but the operator cancels at the confirmation prompt.
    let resp = dispatch_with_workspace_binding(
        &mcp_bind("proj_ada", &ws.path().to_string_lossy(), false, None),
        &adapter,
        None,
        None,
        &ctx,
    );
    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert_eq!(resp.result.unwrap()["error"], "confirmation_missing");

    let after = hosted_readiness(&store, "proj_ada");
    assert_eq!(after.binding_readiness, DaemonBindingReadiness::Unbound);
    assert!(
        !after
            .workspace_readiness
            .as_ref()
            .unwrap()
            .code_grounding_ready
    );
}

/// Unsupported daemon capability fails closed with retry-after-repair guidance,
/// and materializes nothing; the hosted CTA/upgrade path still applies (#734).
#[test]
fn unsupported_daemon_capability_fails_closed() {
    let ws = tempfile::tempdir().unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    let adapter = MemoryAdapter::new();
    let registry = registry(Some("proj_ada"));
    let store = FileWorkspaceBindingStore::new(store_dir.path());
    let ctx = WorkspaceBindingDispatchContext {
        registry: &registry,
        store: &store,
    };

    // The caller requires a newer daemon workspace-binding capability than the
    // daemon advertises.
    let required = DAEMON_WORKSPACE_BINDING_CAPABILITY + 1;
    let resp = dispatch_with_workspace_binding(
        &mcp_bind(
            "proj_ada",
            &ws.path().to_string_lossy(),
            true,
            Some(required),
        ),
        &adapter,
        None,
        None,
        &ctx,
    );
    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    let result = resp.result.unwrap();
    assert_eq!(result["error"], "daemon_capability_mismatch");
    assert!(store.load("proj_ada").unwrap().is_none());

    let after = hosted_readiness(&store, "proj_ada");
    assert_eq!(after.binding_readiness, DaemonBindingReadiness::Unbound);
}

/// A surface cannot self-serve a binding: routing `workspace.bind` through a
/// runtime with no operator-local binding context fails closed with
/// `unsupported_capability`, and capability discovery (#747) reports it
/// unavailable.
#[test]
fn surface_cannot_self_serve_without_daemon_binding_context() {
    let ws = tempfile::tempdir().unwrap();
    let adapter = MemoryAdapter::new();

    let resp = dispatch_with_workspace(
        &mcp_bind("proj_ada", &ws.path().to_string_lossy(), true, None),
        &adapter,
        None,
    );
    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert!(resp
        .message
        .as_deref()
        .unwrap_or_default()
        .contains("unsupported_capability"));

    // Capability discovery must not advertise a binding it cannot route.
    let report = capability_report_with_workspace_binding(
        bicameral_api::reasoning_projection::ProjectionProviderReadiness::not_configured(),
        false,
    );
    assert!(!report.workspace_binding_available);
    assert!(report.supported_commands.contains(&"workspace.bind"));
}

// ═══════════════════════════════════════════════════════════════════════════
// git-remote normalization: SSH and HTTPS forms compare equal
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ssh_and_https_remotes_normalize_equal() {
    let ssh = "git@github.com:BicameralAI/ada-service.git";
    let https = "https://github.com/BicameralAI/ada-service";
    assert_eq!(normalize_git_remote(ssh), "BicameralAI/ada-service");
    assert_eq!(normalize_git_remote(https), "BicameralAI/ada-service");
    assert!(remotes_agree(ssh, https));

    // Trailing slash and ssh:// scheme forms also normalize equal.
    assert!(remotes_agree(
        ssh,
        "https://github.com/BicameralAI/ada-service/"
    ));
    assert!(remotes_agree(
        ssh,
        "ssh://git@github.com/BicameralAI/ada-service.git"
    ));

    // A different repository does not agree.
    assert!(!remotes_agree(
        ssh,
        "https://github.com/BicameralAI/beacon-service"
    ));
}

#[test]
fn unknown_remote_is_neither_matching_nor_contradictory() {
    // A remote matching no registered project's source ref yields Unknown, so
    // the surface does not auto-proceed on unrecognized evidence.
    assert_eq!(
        classify_remote_agreement("proj_ada", "https://github.com/BicameralAI/unrelated-repo"),
        RemoteAgreement::Unknown,
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
