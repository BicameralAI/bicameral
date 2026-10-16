//! Workspace binding proposal protocol — transient `WorkspaceBindingProposal`
//! vocabulary plus the `workspace.bind` request/response/error contract for
//! local `ToolRequest` dispatch (bicameral-bot#731).
//!
//! # What this is
//!
//! MCP-assisted local workspace binding lets an edge surface (the MCP thin
//! client, in alpha) *propose* that the local daemon bind an already-registered
//! project to a local code folder for grounding and preflight. The daemon
//! validates and materializes the binding; the surface never writes it.
//!
//! - **[`WorkspaceBindingProposal`]** is the transient DTO a surface builds to
//!   ask the daemon to bind a candidate folder. It is not a durable lifecycle
//!   state and not accepted evidence.
//! - **[`WorkspaceBindRequest`] / [`WorkspaceBindResponse`] /
//!   [`WorkspaceBindErrorResponse`]** are the `workspace.bind` command contract
//!   dispatched through the local `ToolRequest` path.
//! - **[`WorkspaceBindingState`]** is the durable lifecycle state, kept to the
//!   three states owned by the registration/runtime specs:
//!   `local_workspace_unbound`, `local_workspace_bound`,
//!   `local_workspace_repair_required`.
//!
//! # Authority boundary
//!
//! A `WorkspaceBindingProposal` is **not**:
//!
//! - **project identity** — identity still comes from explicit
//!   `ProjectRegistration`, never from ambient filesystem discovery;
//! - **`BindingEvidence`** — it never links a decision to a code symbol/range;
//! - **source authority** — it is not a `SourceBinding` or `SourceEvidence`;
//! - **cloud authority** — it grants no hosted/cloud membership or authority.
//!
//! Edge surfaces (MCP) **propose**; the local daemon **validates and
//! materializes** binding. Bridge/hosted surfaces may render binding readiness
//! but must never receive the local `candidate_path` or any local absolute path.
//!
//! # Field exposure
//!
//! | Field | Local-only | May cross the hosted bridge |
//! |---|---|---|
//! | [`WorkspaceBindingProposal::candidate_path`] | yes | **no** |
//! | [`WorkspaceBindingProposal::project_id`] | | yes (opaque id) |
//! | [`ProjectSafeDisplayMetadata`] | | yes (redacted display only) |
//! | [`WorkspaceBindingState`] | | yes (readiness) |
//!
//! The hosted-safe projection is [`WorkspaceBindingReadinessView`]; it carries
//! the durable state and redacted display metadata and **never** the
//! `candidate_path`. See [`WORKSPACE_BINDING_LOCAL_ONLY_FIELDS`].
//!
//! # Non-goals (bicameral-bot#731)
//!
//! Daemon persistence, MCP UI prompts, hosted dashboard UI, and any
//! browser/cloud folder selection are out of scope. This module defines the
//! stable protocol contract and fixtures so downstream bot, hosted-dashboard,
//! and MCP workers can align before daemon persistence lands.
//!
//! Related: `docs/specs/workspace-binding-proposal-protocol.md`,
//! `docs/specs/explicit-project-registration-hosted-local-discovery.md`,
//! `docs/specs/cloud-hosted-dashboard-local-daemon-runtime-architecture.md`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ── Durable state ──────────────────────────────────────────────────────────

/// Durable local workspace binding lifecycle state.
///
/// These are the only durable workspace states. `WorkspaceBindingProposal` is a
/// transient protocol object, not a state in this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceBindingState {
    /// Project is registered but has no local workspace binding yet.
    LocalWorkspaceUnbound,
    /// Project has a materialized local workspace binding.
    LocalWorkspaceBound,
    /// A previously usable binding is broken and must be repaired before use.
    LocalWorkspaceRepairRequired,
}

// ── Source surface ─────────────────────────────────────────────────────────

/// Local surface that authored a [`WorkspaceBindingProposal`].
///
/// In alpha, MCP is the proposing surface (`source_surface: mcp`). Other local
/// surfaces are listed for forward-compatibility; none grant authority — the
/// daemon still validates and materializes the binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceBindingSourceSurface {
    /// MCP thin-client session proposed the binding.
    Mcp,
    /// Local CLI invocation proposed the binding.
    Cli,
    /// Local review dashboard/TUI proposed the binding.
    Dashboard,
}

// ── Project-safe display metadata ──────────────────────────────────────────

/// Redacted, project-safe display metadata for a binding proposal.
///
/// SAFE to cross the hosted bridge: contains no local filesystem path, provider
/// secret, source body, or event-store body. Any folder hint here must be a
/// non-path label (e.g. a basename), never an absolute path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProjectSafeDisplayMetadata {
    /// Human-readable project display name. No path/remote/secret leakage.
    pub display_name: String,

    /// Optional short project slug/label. Safe, opaque display token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_slug: Option<String>,

    /// Optional non-path folder label to help a human recognize the candidate
    /// (e.g. the folder basename only). MUST NOT be an absolute path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_label: Option<String>,
}

// ── Transient proposal ─────────────────────────────────────────────────────

/// Transient proposal for an edge surface to ask the daemon to bind a candidate
/// folder to an already-registered project.
///
/// This is not a durable state and not accepted evidence. See the module docs
/// for the authority boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceBindingProposal {
    /// Opaque, already-registered project identifier the binding targets.
    /// Never a filesystem path (see [`ProjectRegistration`] semantics in the
    /// registration spec).
    pub project_id: String,

    /// Redacted display metadata safe for hosted-bridge exposure.
    pub display: ProjectSafeDisplayMetadata,

    /// **LOCAL-ONLY.** Absolute candidate folder path for daemon/MCP local use.
    /// MUST NOT cross the hosted bridge or appear in any hosted-safe view.
    pub candidate_path: String,

    /// Which local surface authored this proposal (`mcp` in alpha).
    pub source_surface: WorkspaceBindingSourceSurface,

    /// Human-readable reason the surface is proposing this candidate.
    pub reason: String,

    /// Surface confidence that the candidate is correct, in `[0.0, 1.0]`.
    pub confidence: f32,
}

impl WorkspaceBindingProposal {
    /// Project a hosted-safe readiness view for this proposal, dropping the
    /// local-only [`candidate_path`](WorkspaceBindingProposal::candidate_path).
    ///
    /// Use this (never the raw proposal) when reporting binding readiness across
    /// the hosted bridge.
    pub fn hosted_safe_view(
        &self,
        state: WorkspaceBindingState,
        bind_available: bool,
    ) -> WorkspaceBindingReadinessView {
        WorkspaceBindingReadinessView {
            project_id: self.project_id.clone(),
            state,
            display: self.display.clone(),
            bind_available,
            status_message: None,
        }
    }
}

// ── workspace.bind request ─────────────────────────────────────────────────

/// `workspace.bind` request — the command contract dispatched through the local
/// `ToolRequest` path once daemon persistence lands.
///
/// Carrying the transient [`WorkspaceBindingProposal`] plus explicit operator
/// confirmation and optional optimistic preconditions. The request is local
/// authority; it is never routed through the hosted bridge or external ingest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceBindRequest {
    /// The proposal to validate and materialize.
    pub proposal: WorkspaceBindingProposal,

    /// Explicit operator confirmation that binding may proceed. When `false`
    /// (or omitted) the daemon returns
    /// [`WorkspaceBindErrorKind::ConfirmationMissing`].
    #[serde(default)]
    pub confirmed: bool,

    /// Minimum daemon workspace-binding capability version the caller requires.
    /// When the daemon advertises a lower version the request fails with
    /// [`WorkspaceBindErrorKind::DaemonCapabilityMismatch`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_daemon_capability: Option<u32>,

    /// Optional durable state the caller believes currently holds. Advisory
    /// optimistic-concurrency hint; the daemon remains authoritative.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_current_state: Option<WorkspaceBindingState>,
}

// ── workspace.bind response ────────────────────────────────────────────────

/// `workspace.bind` success response — the daemon validated and materialized
/// (or already holds) a binding for the project.
///
/// Carries only project-safe fields; the local `candidate_path` is never echoed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceBindResponse {
    /// The project the binding was materialized for.
    pub project_id: String,

    /// Durable state after the successful bind (`local_workspace_bound`).
    pub state: WorkspaceBindingState,

    /// Redacted display metadata safe for hosted-bridge exposure.
    pub display: ProjectSafeDisplayMetadata,

    /// Optional human-readable status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

// ── workspace.bind error ───────────────────────────────────────────────────

/// Typed `workspace.bind` failure classifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceBindErrorKind {
    /// The proposal targets a project other than the one in session scope.
    WrongProject,
    /// The target project is not registered locally.
    UnregisteredProject,
    /// The candidate path failed the daemon's local safety policy.
    UnsafePath,
    /// The project already has a materialized workspace binding.
    AlreadyBound,
    /// Explicit operator confirmation was missing.
    ConfirmationMissing,
    /// The daemon does not advertise the required workspace-binding capability.
    DaemonCapabilityMismatch,
    /// The project binding is broken and must be repaired before binding.
    RepairRequired,
}

/// `workspace.bind` error response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceBindErrorResponse {
    /// Error classification.
    pub error: WorkspaceBindErrorKind,

    /// The project the request targeted.
    pub project_id: String,

    /// Human-readable error message.
    pub message: String,

    /// Durable state the daemon reports for the project after the failed
    /// attempt. Never mutated by a rejected request.
    pub state: WorkspaceBindingState,

    /// Whether the caller may retry after local repair/confirmation.
    pub retry_after_repair: bool,
}

// ── Hosted-safe readiness projection ───────────────────────────────────────

/// Hosted-safe projection of workspace binding readiness for a project.
///
/// SAFE to cross the hosted bridge: carries the durable [`WorkspaceBindingState`]
/// and redacted [`ProjectSafeDisplayMetadata`], and **never** the local
/// `candidate_path` or any absolute path. It is a readiness projection, not an
/// authority object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceBindingReadinessView {
    /// The project this readiness view describes.
    pub project_id: String,

    /// Durable workspace binding state.
    pub state: WorkspaceBindingState,

    /// Redacted display metadata safe for hosted-bridge exposure.
    pub display: ProjectSafeDisplayMetadata,

    /// Whether a bind proposal/flow may be offered to the operator.
    pub bind_available: bool,

    /// Optional human-readable status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
}

/// Field names on [`WorkspaceBindingProposal`] / [`WorkspaceBindRequest`] /
/// [`WorkspaceResolveRequest`] that are **local-only** and must never cross
/// the hosted bridge.
pub const WORKSPACE_BINDING_LOCAL_ONLY_FIELDS: &[&str] = &["candidate_path"];

// ── workspace.resolve request / response (SessionStart routing, mcp#831) ──

/// `workspace.resolve` request — local-only reverse lookup from a host folder
/// hint to at most one already-materialized [`LocalWorkspaceBinding`].
///
/// The path is a **routing hint**, never Product identity. The daemon scans
/// operator-local records and returns a unique bound `product_id` or a typed
/// miss. This command does not create a Product or a binding.
///
/// `candidate_path` is local-only and must never cross the hosted bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceResolveRequest {
    /// **LOCAL-ONLY.** Absolute host-reported workspace / cwd folder. Exact
    /// canonical path match against stored bindings; parent/child paths do
    /// not match. MUST NOT cross the hosted bridge.
    pub candidate_path: String,
}

/// `workspace.resolve` success — exactly one `local_workspace_bound` record
/// matches the candidate folder.
///
/// Carries only project-safe fields. The local `candidate_path` is never echoed.
/// `product_id` is the registered project id used as ToolRequest Product
/// routing (ADR-0023). This response is not Product identity and does not
/// materialize a binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceResolveResponse {
    /// Canonical Product / project id to attach to a subsequent ToolRequest.
    pub product_id: String,

    /// Durable state of the unique match (`local_workspace_bound`).
    pub state: WorkspaceBindingState,

    /// Redacted display metadata; never an absolute path.
    pub display: ProjectSafeDisplayMetadata,
}

/// Typed `workspace.resolve` failure classifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceResolveErrorKind {
    /// No stored binding matches the candidate folder.
    Unbound,
    /// The unique matching binding is broken and must be repaired.
    RepairRequired,
    /// Two or more stored bindings match the same canonical folder.
    Ambiguous,
    /// The candidate path failed the local safety policy (empty, relative).
    UnsafePath,
}

/// `workspace.resolve` error response. Never invents a `product_id`.
/// Never echoes `candidate_path`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceResolveErrorResponse {
    /// Error classification.
    pub error: WorkspaceResolveErrorKind,

    /// Human-readable error message.
    pub message: String,

    /// Durable state the daemon reports after the failed lookup.
    pub state: WorkspaceBindingState,

    /// Number of stored records that matched the canonical path (0, or 2+).
    pub match_count: u32,
}

// ── Deterministic protocol-level classifier ────────────────────────────────

/// Daemon-provided facts the protocol classifier evaluates a request against.
///
/// These are inputs the daemon already knows locally. Providing them as an
/// explicit struct keeps [`classify_bind_request`] pure (no filesystem, no
/// persistence) so downstream bot / hosted-dashboard / MCP workers can reuse the
/// exact outcome mapping before daemon persistence lands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DaemonBindingFacts {
    /// Whether the target project is registered locally.
    pub project_registered: bool,

    /// The project id the daemon considers current for this session, if scoped.
    /// A mismatch with the proposal yields [`WorkspaceBindErrorKind::WrongProject`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_project_id: Option<String>,

    /// Current durable state for the project.
    pub current_state: WorkspaceBindingState,

    /// Daemon-advertised workspace-binding capability version.
    pub daemon_capability: u32,

    /// Whether the candidate path passed the daemon's local safety policy.
    pub candidate_path_safe: bool,
}

/// Classify a [`WorkspaceBindRequest`] against daemon facts into a typed
/// success response or a typed error.
///
/// This is a pure, deterministic protocol-level mapping. It does **not** touch
/// the filesystem, persist state, or materialize a binding — daemon persistence
/// is out of scope (bicameral-bot#731). It exists so every downstream worker
/// derives the same outcome from the same inputs.
pub fn classify_bind_request(
    request: &WorkspaceBindRequest,
    facts: &DaemonBindingFacts,
) -> Result<WorkspaceBindResponse, WorkspaceBindErrorResponse> {
    let proposal = &request.proposal;
    let project_id = proposal.project_id.clone();

    let error = |kind: WorkspaceBindErrorKind, message: &str, retry_after_repair: bool| {
        Err(WorkspaceBindErrorResponse {
            error: kind,
            project_id: project_id.clone(),
            message: message.to_string(),
            state: facts.current_state,
            retry_after_repair,
        })
    };

    if let Some(required) = request.required_daemon_capability {
        if facts.daemon_capability < required {
            return error(
                WorkspaceBindErrorKind::DaemonCapabilityMismatch,
                "Daemon does not advertise the required workspace-binding capability.",
                true,
            );
        }
    }

    if let Some(session_project) = facts.session_project_id.as_deref() {
        if session_project != proposal.project_id {
            return error(
                WorkspaceBindErrorKind::WrongProject,
                "Proposal targets a project other than the one in session scope.",
                false,
            );
        }
    }

    if !facts.project_registered {
        return error(
            WorkspaceBindErrorKind::UnregisteredProject,
            "Target project is not registered locally; register it before binding.",
            true,
        );
    }

    if facts.current_state == WorkspaceBindingState::LocalWorkspaceRepairRequired {
        return error(
            WorkspaceBindErrorKind::RepairRequired,
            "Project binding is broken and must be repaired before binding.",
            true,
        );
    }

    if facts.current_state == WorkspaceBindingState::LocalWorkspaceBound {
        return error(
            WorkspaceBindErrorKind::AlreadyBound,
            "Project already has a materialized workspace binding.",
            false,
        );
    }

    if !facts.candidate_path_safe {
        return error(
            WorkspaceBindErrorKind::UnsafePath,
            "Candidate path failed the local safety policy.",
            true,
        );
    }

    if !request.confirmed {
        return error(
            WorkspaceBindErrorKind::ConfirmationMissing,
            "Explicit operator confirmation is required to bind a workspace.",
            true,
        );
    }

    Ok(WorkspaceBindResponse {
        project_id,
        state: WorkspaceBindingState::LocalWorkspaceBound,
        display: proposal.display.clone(),
        message: Some("Workspace binding materialized.".to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proposal() -> WorkspaceBindingProposal {
        WorkspaceBindingProposal {
            project_id: "proj_ada".into(),
            display: ProjectSafeDisplayMetadata {
                display_name: "Ada Service".into(),
                project_slug: Some("ada-service".into()),
                candidate_label: Some("ada-service".into()),
            },
            candidate_path: "/home/alex/code/ada-service".into(),
            source_surface: WorkspaceBindingSourceSurface::Mcp,
            reason: "Open editor workspace matches the registered project.".into(),
            confidence: 0.82,
        }
    }

    fn request(confirmed: bool) -> WorkspaceBindRequest {
        WorkspaceBindRequest {
            proposal: proposal(),
            confirmed,
            required_daemon_capability: None,
            expected_current_state: Some(WorkspaceBindingState::LocalWorkspaceUnbound),
        }
    }

    fn facts() -> DaemonBindingFacts {
        DaemonBindingFacts {
            project_registered: true,
            session_project_id: Some("proj_ada".into()),
            current_state: WorkspaceBindingState::LocalWorkspaceUnbound,
            daemon_capability: 1,
            candidate_path_safe: true,
        }
    }

    #[test]
    fn state_serialization_matches_durable_vocabulary() {
        assert_eq!(
            serde_json::to_value(WorkspaceBindingState::LocalWorkspaceUnbound).unwrap(),
            serde_json::json!("local_workspace_unbound")
        );
        assert_eq!(
            serde_json::to_value(WorkspaceBindingState::LocalWorkspaceBound).unwrap(),
            serde_json::json!("local_workspace_bound")
        );
        assert_eq!(
            serde_json::to_value(WorkspaceBindingState::LocalWorkspaceRepairRequired).unwrap(),
            serde_json::json!("local_workspace_repair_required")
        );
    }

    #[test]
    fn source_surface_mcp_serializes_snake_case() {
        assert_eq!(
            serde_json::to_value(WorkspaceBindingSourceSurface::Mcp).unwrap(),
            serde_json::json!("mcp")
        );
    }

    #[test]
    fn proposal_round_trips() {
        let p = proposal();
        let json = serde_json::to_string(&p).unwrap();
        let back: WorkspaceBindingProposal = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn confirmed_request_binds() {
        let resp = classify_bind_request(&request(true), &facts()).unwrap();
        assert_eq!(resp.state, WorkspaceBindingState::LocalWorkspaceBound);
        assert_eq!(resp.project_id, "proj_ada");
    }

    #[test]
    fn missing_confirmation_is_typed_error() {
        let err = classify_bind_request(&request(false), &facts()).unwrap_err();
        assert_eq!(err.error, WorkspaceBindErrorKind::ConfirmationMissing);
        assert_eq!(err.state, WorkspaceBindingState::LocalWorkspaceUnbound);
    }

    #[test]
    fn unsafe_path_is_typed_error() {
        let mut f = facts();
        f.candidate_path_safe = false;
        let err = classify_bind_request(&request(true), &f).unwrap_err();
        assert_eq!(err.error, WorkspaceBindErrorKind::UnsafePath);
    }

    #[test]
    fn unregistered_project_is_typed_error() {
        let mut f = facts();
        f.project_registered = false;
        f.session_project_id = None;
        let err = classify_bind_request(&request(true), &f).unwrap_err();
        assert_eq!(err.error, WorkspaceBindErrorKind::UnregisteredProject);
    }

    #[test]
    fn wrong_project_is_typed_error() {
        let mut f = facts();
        f.session_project_id = Some("proj_other".into());
        let err = classify_bind_request(&request(true), &f).unwrap_err();
        assert_eq!(err.error, WorkspaceBindErrorKind::WrongProject);
    }

    #[test]
    fn already_bound_is_typed_error() {
        let mut f = facts();
        f.current_state = WorkspaceBindingState::LocalWorkspaceBound;
        let err = classify_bind_request(&request(true), &f).unwrap_err();
        assert_eq!(err.error, WorkspaceBindErrorKind::AlreadyBound);
    }

    #[test]
    fn repair_required_is_typed_error() {
        let mut f = facts();
        f.current_state = WorkspaceBindingState::LocalWorkspaceRepairRequired;
        let err = classify_bind_request(&request(true), &f).unwrap_err();
        assert_eq!(err.error, WorkspaceBindErrorKind::RepairRequired);
        assert_eq!(
            err.state,
            WorkspaceBindingState::LocalWorkspaceRepairRequired
        );
    }

    #[test]
    fn capability_mismatch_is_typed_error() {
        let mut req = request(true);
        req.required_daemon_capability = Some(2);
        let err = classify_bind_request(&req, &facts()).unwrap_err();
        assert_eq!(err.error, WorkspaceBindErrorKind::DaemonCapabilityMismatch);
    }

    #[test]
    fn hosted_safe_view_drops_candidate_path() {
        let view = proposal().hosted_safe_view(WorkspaceBindingState::LocalWorkspaceUnbound, true);
        let json = serde_json::to_value(&view).unwrap();
        assert!(json.get("candidate_path").is_none());
        // No string value in the hosted-safe view is an absolute path.
        assert!(!json_contains_absolute_path(&json));
    }

    #[test]
    fn resolve_error_kind_serializes_snake_case() {
        assert_eq!(
            serde_json::to_value(WorkspaceResolveErrorKind::Unbound).unwrap(),
            serde_json::json!("unbound")
        );
        assert_eq!(
            serde_json::to_value(WorkspaceResolveErrorKind::RepairRequired).unwrap(),
            serde_json::json!("repair_required")
        );
        assert_eq!(
            serde_json::to_value(WorkspaceResolveErrorKind::Ambiguous).unwrap(),
            serde_json::json!("ambiguous")
        );
        assert_eq!(
            serde_json::to_value(WorkspaceResolveErrorKind::UnsafePath).unwrap(),
            serde_json::json!("unsafe_path")
        );
    }

    #[test]
    fn resolve_success_never_echoes_candidate_path() {
        let resp = WorkspaceResolveResponse {
            product_id: "proj_ada".into(),
            state: WorkspaceBindingState::LocalWorkspaceBound,
            display: ProjectSafeDisplayMetadata {
                display_name: "Ada Service".into(),
                project_slug: Some("ada-service".into()),
                candidate_label: Some("ada-service".into()),
            },
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("candidate_path").is_none());
        assert!(!json_contains_absolute_path(&json));
        assert_eq!(json["product_id"], "proj_ada");
    }

    #[test]
    fn resolve_error_never_echoes_candidate_path_or_product_id() {
        let err = WorkspaceResolveErrorResponse {
            error: WorkspaceResolveErrorKind::Ambiguous,
            message: "Multiple products are bound to this folder.".into(),
            state: WorkspaceBindingState::LocalWorkspaceBound,
            match_count: 2,
        };
        let json = serde_json::to_value(&err).unwrap();
        assert!(json.get("candidate_path").is_none());
        assert!(json.get("product_id").is_none());
        assert!(!json_contains_absolute_path(&json));
    }

    fn json_contains_absolute_path(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::String(s) => s.starts_with('/'),
            serde_json::Value::Array(items) => items.iter().any(json_contains_absolute_path),
            serde_json::Value::Object(map) => map.values().any(json_contains_absolute_path),
            _ => false,
        }
    }
}
