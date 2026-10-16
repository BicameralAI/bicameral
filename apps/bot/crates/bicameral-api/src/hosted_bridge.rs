//! Hosted bridge user-level pairing protocol types.
//!
//! Defines the request/response contract for the hosted bridge route family
//! on the bot gateway. The bridge is user/operator-level: it pairs a remote
//! browser origin to the local daemon runtime without granting project
//! membership, workspace binding, source authority, signoff, or event-store
//! authority.
//!
//! # Authority boundary
//!
//! The hosted bridge is **not** the same contract as `ExternalIngestEnvelope`
//! (evidence-only) or local `ToolRequest` (local actor authority). It is a
//! browser-origin pairing/session surface that checks origin, CSRF, replay,
//! and expiry before forwarding governed actions to local policy.
//!
//! Bridge pairing does **not**:
//! - Create `DecisionCandidate`, `Decision`, or `SourceEvidence`
//! - Emit review, signoff, or compliance commands
//! - Write accepted events or create canonical Decision Ledger entries
//! - Grant workspace binding or filesystem access
//! - Accept provider tokens directly
//! - Bypass action authorization policy (ADR-0020)
//!
//! # State model
//!
//! ```text
//! User-level (bridge pairing):
//!   hosted_bridge_unpaired -> hosted_bridge_paired -> hosted_bridge_revoked
//!
//! Project-level (after pairing):
//!   unregistered -> registered_project -> normal_project_running
//!
//! Workspace-level (after project registration):
//!   local_workspace_unbound -> local_workspace_bound
//! ```
//!
//! Related: `docs/specs/hosted-bridge-user-level-pairing.md`, ADR-0002, ADR-0020.

use crate::candidate::{DecisionLevel, ReviewLifecycleState, TrackingState};
use crate::dashboard::SignoffState;
use crate::event_store::EventType;
use crate::evidence::GraphEvidenceState;
use crate::source::SourceFreshness;
use crate::workspace_binding::{
    ProjectSafeDisplayMetadata, WorkspaceBindingSourceSurface, WorkspaceBindingState,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

// ── Bridge pairing state ───────────────────────────────────────────────────

/// User-level hosted bridge pairing state.
///
/// This is the top-level state for the bridge connection between a remote
/// browser origin and the local daemon. It is orthogonal to project
/// registration and workspace binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostedBridgePairingState {
    /// No active pairing. Hosted web can see only pairing/setup affordances.
    Unpaired,
    /// Active pairing. Hosted web is paired through a checked origin, local
    /// session, and capability handshake.
    Paired,
    /// Prior pairing revoked or expired. Must not be reused.
    Revoked,
}

// ── Capability discovery ───────────────────────────────────────────────────

/// Hosted bridge capability metadata advertised during pairing handshake.
///
/// This is compatibility/readiness metadata only. It does not grant authority
/// or mutate governance state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedBridgeCapability {
    /// Whether the daemon supports hosted bridge pairing at all.
    pub bridge_supported: bool,

    /// Release/build version of the daemon binary (diagnostic only).
    ///
    /// This changes with ordinary Bot releases and MUST NOT be used as the
    /// hosted-bridge compatibility key.
    #[serde(default)]
    pub daemon_build_version: String,

    /// Version of the Bot-owned hosted-bridge wire contract.
    ///
    /// This changes only when the bridge contract changes incompatibly and is
    /// the compatibility key Cloud must compare.
    #[serde(default)]
    pub hosted_bridge_protocol_version: u32,

    /// Protocol version label for the bridge route family (display/diagnostic).
    ///
    /// This is a human-facing label and is NOT the compatibility key. Hosted /
    /// cloud compatibility is decided on the integer `daemon_protocol_version`
    /// below, never on this string or on any release/semver value (#697).
    pub bridge_protocol_version: String,

    /// Integer hosted-dashboard protocol version this daemon speaks.
    ///
    /// This is the sole compatibility key between the always-current cloud SPA
    /// and the (possibly older) local daemon. Defaults to `0` when absent so
    /// legacy daemons that never advertised it read as "oldest protocol" and
    /// are handled by the support-window rules (#697 / #711).
    #[serde(default)]
    pub daemon_protocol_version: u32,

    /// Whether project listing is available after pairing.
    pub project_list_available: bool,

    /// Whether workspace binding is available (always false at user-level).
    pub workspace_binding_available: bool,

    /// Human-readable note about bridge readiness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

// ── Pairing request/response ───────────────────────────────────────────────

/// Hosted bridge pairing request — sent by the hosted web to initiate pairing.
///
/// The local daemon validates the origin, CSRF token, and account linkage
/// before accepting the pairing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedBridgePairingRequest {
    /// Remote origin requesting pairing (e.g. "https://app.bicameral-ai.com").
    pub origin: String,

    /// Cloud account identifier requesting pairing.
    pub cloud_account_id: String,

    /// CSRF token for anti-forgery protection.
    pub csrf_token: String,

    /// Requested bridge session TTL in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_ttl_seconds: Option<u64>,
}

/// Hosted bridge pairing response — returned by the local daemon after
/// successful pairing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedBridgePairingResponse {
    /// The new pairing state (should be `Paired`).
    pub state: HostedBridgePairingState,

    /// Session token for subsequent bridge requests.
    pub session_token: String,

    /// Session expiry timestamp (ISO-8601).
    pub expires_at: String,

    /// Capabilities available through this pairing.
    pub capabilities: HostedBridgeCapability,
}

// ── Pending pairing / local-confirmation flow (#790) ───────────────────────
//
// The hosted bridge no longer issues a session token directly from
// `POST /pair`. A valid hosted request instead creates a short-lived,
// daemon-memory **pending request** and returns a `202` pending outcome with
// an opaque handle/proof, expiry, capability/version info, and a same-origin
// local confirmation affordance. Only an explicit local approval on the
// locally served confirmation surface may transition the pending request to a
// paired session. Hosted/cross-origin/replayed/mismatched attempts fail
// closed. See `docs/specs/hosted-bridge-user-level-pairing.md`.
//
// Authority boundary: pending pairing is transient daemon-local request state.
// It is NOT canonical Decision / project / workspace / provider / source /
// signoff / event-store state, and it is never persisted to Cloud.

/// Typed outcome of a pending pairing request as it moves through the
/// local-confirmation lifecycle (#790).
///
/// ```text
/// pending ── local approve ──▶ approved   (session token released, retry-safe)
///    │
///    ├── local deny ─────────▶ denied     (terminal, never reveals a token)
///    └── expiry ─────────────▶ expired     (terminal, never reveals a token)
///
/// incompatible: the consumer/daemon is too old for the confirmation flow.
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingPairingOutcome {
    /// Request created and awaiting an explicit local approve/deny decision.
    Pending,
    /// Locally approved. Only in this state may a bridge session token be
    /// released to the initiating browser through the status exchange.
    Approved,
    /// Locally denied. Terminal — no token is ever released.
    Denied,
    /// Expired before a local decision was made. Terminal — no token.
    Expired,
    /// The requesting consumer/daemon is incompatible with the confirmation
    /// protocol and must update. No pending request is created.
    Incompatible,
}

impl PendingPairingOutcome {
    /// Stable string tag for logs, telemetry, and dashboard copy.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Expired => "expired",
            Self::Incompatible => "incompatible",
        }
    }

    /// Whether this outcome is terminal (no further transitions possible).
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Denied | Self::Expired | Self::Incompatible)
    }
}

/// A concise allowed/forbidden capability summary rendered on the local
/// confirmation surface before the operator approves or denies (#790).
///
/// Renderer-only. It carries no authority and no secrets — just the human
/// summary of what the requesting origin is and is not granted by pairing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingCapabilitySummary {
    /// What a paired session is allowed to do (user-level projections and
    /// policy-checked requests).
    pub allowed: Vec<String>,
    /// What pairing explicitly does NOT grant.
    pub forbidden: Vec<String>,
}

/// Build the canonical allowed/forbidden capability summary for the local
/// confirmation surface, mirroring the hosted-bridge spec UX shape (#790).
pub fn pairing_capability_summary() -> PairingCapabilitySummary {
    PairingCapabilitySummary {
        allowed: vec![
            "List locally registered Bicameral projects".to_string(),
            "Show setup / readiness status".to_string(),
            "Request project registration or repair".to_string(),
            "Request governed actions after local policy checks".to_string(),
        ],
        forbidden: vec![
            "Provider secrets or tokens".to_string(),
            "Local filesystem or workspace/code access".to_string(),
            "Decision approval, signoff, or compliance without policy checks".to_string(),
            "Direct Decision Ledger or event-store writes".to_string(),
        ],
    }
}

/// Number of Crockford Base32 symbols in a hosted pairing code.
pub const PAIRING_CODE_SYMBOL_COUNT: usize = 8;

/// Normalize a human-entered pairing code to its ungrouped uppercase form.
///
/// Hyphens and ASCII whitespace are presentation-only. Ambiguous Crockford
/// symbols (`I`, `L`, `O`, `U`) are rejected rather than silently rewritten,
/// keeping browser display and terminal entry deterministic.
pub fn normalize_pairing_code(input: &str) -> Option<String> {
    const ALPHABET: &str = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

    let normalized: String = input
        .chars()
        .filter(|ch| *ch != '-' && !ch.is_ascii_whitespace())
        .map(|ch| ch.to_ascii_uppercase())
        .collect();

    if normalized.len() != PAIRING_CODE_SYMBOL_COUNT
        || !normalized.chars().all(|ch| ALPHABET.contains(ch))
    {
        return None;
    }
    Some(normalized)
}

/// Local confirmation affordance returned alongside a pending pairing request
/// (#790, #865).
///
/// Directs the operator to the daemon-served local approval page
/// (`local_confirmation_url`, a same-origin path on the daemon) and to the
/// terminal fallback. Opening the page grants nothing: only an explicit
/// decision submitted from the loopback origin with the local nonce does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalConfirmationAffordance {
    /// Same-origin, locally served confirmation path (relative to the daemon
    /// origin), e.g. `/local/pairing?request=<handle>`. Absent when the daemon
    /// exposes terminal confirmation only (older daemons).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_confirmation_url: Option<String>,

    /// Canonical local command for securely entering the browser-displayed
    /// pairing code. The code itself is deliberately not interpolated here.
    pub terminal_command: String,

    /// Short human-readable instruction for the initiating browser to show
    /// (e.g. "Approve this connection in your local Bicameral").
    pub message: String,

    /// The requesting Cloud origin, echoed for display on both surfaces.
    pub requesting_origin: String,

    /// The requesting Cloud account identifier, echoed for display.
    pub cloud_account_id: String,

    /// Allowed / forbidden capability summary to render before decision.
    pub capability_summary: PairingCapabilitySummary,
}

/// Response returned by `POST /pair` for a valid hosted request under the
/// local-confirmation flow (#790). HTTP `202 Accepted`.
///
/// It contains an opaque request handle and poll proof, expiry, capability /
/// version info, and the local confirmation affordance. It NEVER contains a
/// bridge session token or a `paired` state — a token is released only after
/// an explicit local approval, through the status exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedBridgePairingPendingResponse {
    /// Always [`PendingPairingOutcome::Pending`] for a freshly created request.
    pub status: PendingPairingOutcome,

    /// Opaque handle identifying the pending request. Not a session token and
    /// not authority-bearing; used to poll status and to load the local
    /// confirmation surface.
    pub request_handle: String,

    /// Opaque proof the initiating browser must present (bound to its
    /// origin/account) to poll status. Not a session token.
    pub poll_proof: String,

    /// Short-lived code displayed only to the initiating browser. The
    /// operator pastes it into a local terminal; the daemon stores only its
    /// digest. Matching this code locates a request but never approves it.
    pub pairing_code: String,

    /// When the pending request expires (ISO-8601). Abandoned requests are
    /// cleaned up after this instant.
    pub expires_at: String,

    /// Hosted-bridge protocol/capability version this daemon speaks.
    pub protocol_version: u32,

    /// Capability/version envelope advertised to the initiating browser.
    pub capabilities: HostedBridgeCapability,

    /// Same-origin local confirmation affordance.
    pub confirmation: LocalConfirmationAffordance,
}

/// Explicit local decision on a pending pairing request (#790).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairingDecisionAction {
    /// Approve the pending request and release a session to the initiator.
    Approve,
    /// Deny the pending request. Terminal; no token is ever released.
    Deny,
}

/// Request body for the LOCAL-only pairing decision endpoint (#790).
///
/// The `local_confirmation_nonce` is only obtainable from the locally served
/// confirmation surface (itself gated to same-origin/local access), so a
/// hosted or cross-origin caller cannot forge an approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalPairingDecisionRequest {
    /// The pending request being decided.
    pub request_handle: String,

    /// Approve or deny.
    pub action: PairingDecisionAction,

    /// Same-origin local confirmation nonce proving the decision originated
    /// from the locally served surface. Never exposed to the hosted origin.
    pub local_confirmation_nonce: String,
}

/// Local-surface view of a pending request, rendered by the locally served
/// confirmation component (#790).
///
/// Served only to same-origin/local callers. Carries the nonce the surface
/// submits back with the decision. Never carries a session token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingPairingLocalView {
    /// Current status of the pending request.
    pub status: PendingPairingOutcome,

    /// The pending request handle.
    pub request_handle: String,

    /// Requesting Cloud origin (for display).
    pub requesting_origin: String,

    /// Requesting Cloud account identifier (for display).
    pub cloud_account_id: String,

    /// Allowed / forbidden capability summary to render before decision.
    pub capability_summary: PairingCapabilitySummary,

    /// When the pending request expires (ISO-8601).
    pub expires_at: String,

    /// Same-origin local confirmation nonce the surface submits with the
    /// approve/deny decision. Never leaves the local surface.
    pub local_confirmation_nonce: String,
}

/// Request body for the LOCAL-only pairing-code lookup endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalPairingCodeLookupRequest {
    /// Human-entered code copied from the initiating browser. Formatting and
    /// ASCII case are normalized by the daemon before the digest comparison.
    pub pairing_code: String,
}

/// Safe summary used by local terminal surfaces to report pending requests.
/// It intentionally omits the pairing code, poll proof, and decision nonce.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingPairingSummary {
    pub request_handle: String,
    pub requesting_origin: String,
    pub cloud_account_id: String,
    pub expires_at: String,
}

/// LOCAL-only list response for terminal/CLI discovery of pending requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingPairingListResponse {
    pub requests: Vec<PendingPairingSummary>,
}

/// Result of a local pairing decision (#790). Never contains a session token —
/// the initiating browser retrieves the session only via the status exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalPairingDecisionResponse {
    /// The resulting pending-request outcome (approved / denied / expired).
    pub status: PendingPairingOutcome,

    /// The decided request handle.
    pub request_handle: String,

    /// Human-readable confirmation message.
    pub message: String,
}

/// Hosted status-exchange response for a polling initiator (#790).
///
/// Returns the typed pending-request outcome bound to the initiator's
/// origin/account and poll proof. Only an `approved` outcome carries the
/// bridge session token and expiry; every other outcome omits the token
/// entirely (fail-closed, no leakage). Retrieval is retry-safe: repeated polls
/// of an approved request return the same session while it remains valid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedBridgePairingStatusResponse {
    /// Typed outcome for this pending request.
    pub status: PendingPairingOutcome,

    /// Bridge session token — present ONLY when `status == Approved` and the
    /// session is still valid. `None` in every other case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,

    /// Bridge session expiry (ISO-8601) — present ONLY alongside a released
    /// session token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,

    /// Capabilities for the released session — present ONLY when approved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<HostedBridgeCapability>,

    /// Human-readable status message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Typed "update required" outcome returned to an incompatible old
/// immediate-pair consumer/daemon (#790).
///
/// Old consumers must fail closed with a deterministic, actionable outcome
/// rather than silently bypassing local confirmation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedBridgePairingUpdateRequiredResponse {
    /// Always [`PendingPairingOutcome::Incompatible`].
    pub status: PendingPairingOutcome,

    /// Typed error kind for consumers that branch on `error`.
    pub error: HostedBridgeErrorKind,

    /// The minimum hosted-bridge protocol version that supports the
    /// local-confirmation pending-pairing flow.
    pub required_protocol_version: u32,

    /// The protocol version the consumer declared (0 when it declared none).
    pub client_protocol_version: u32,

    /// The daemon's own hosted-bridge protocol version.
    pub daemon_protocol_version: u32,

    /// One-action repair path (reused from the compatibility window).
    pub repair: HostedDashboardUpgradeRepair,

    /// Human-readable message.
    pub message: String,
}

// ── Operator-home session ──────────────────────────────────────────────────

/// Project readiness entry — projection data (not authority) available after
/// pairing.
///
/// This is a non-canonical view of locally registered projects. It does not
/// grant project membership or selection authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectReadinessEntry {
    /// Project display name.
    pub name: String,

    /// Whether the project's event-store is accessible and healthy.
    pub event_store_healthy: bool,

    /// Whether the project has at least one workspace binding.
    pub has_workspace_binding: bool,

    /// Setup state summary (e.g. "normal", "repair_required").
    pub setup_state: String,
}

impl ProjectReadinessEntry {
    /// Derive the daemon-owned durable [`WorkspaceBindingState`] this readiness
    /// entry projects (bicameral-bot#733).
    ///
    /// Prefers real daemon-owned state where the daemon reports it: a
    /// repair-flavoured `setup_state` maps to
    /// [`WorkspaceBindingState::LocalWorkspaceRepairRequired`]; otherwise the
    /// entry is [`LocalWorkspaceBound`](WorkspaceBindingState::LocalWorkspaceBound)
    /// when it has a workspace binding and
    /// [`LocalWorkspaceUnbound`](WorkspaceBindingState::LocalWorkspaceUnbound)
    /// when it does not. A missing binding is a code-grounding readiness gap,
    /// never a project authority failure.
    pub fn workspace_binding_state(&self) -> WorkspaceBindingState {
        if self.setup_state == "repair_required"
            || self.setup_state == "local_workspace_repair_required"
        {
            WorkspaceBindingState::LocalWorkspaceRepairRequired
        } else if self.has_workspace_binding {
            WorkspaceBindingState::LocalWorkspaceBound
        } else {
            WorkspaceBindingState::LocalWorkspaceUnbound
        }
    }

    /// Project a hosted-safe [`HostedWorkspaceBindingReadiness`] for this entry
    /// (bicameral-bot#733).
    ///
    /// Redaction: uses only the safe display name; the entry never carries a
    /// local path, provider token, source body, graph index, or event-store
    /// body, so neither does the projection.
    pub fn workspace_binding_readiness(&self) -> HostedWorkspaceBindingReadiness {
        let display = ProjectSafeDisplayMetadata {
            display_name: self.name.clone(),
            project_slug: None,
            candidate_label: None,
        };
        project_workspace_binding_readiness(&self.name, &display, self.workspace_binding_state())
    }
}

/// Operator-home session response — the paired bridge view of local state.
///
/// Available after `hosted_bridge_paired`. Lists locally registered projects
/// as projection data (not authority). Workspace binding is reported as
/// unavailable until explicitly requested inside a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorHomeSessionResponse {
    /// Current pairing state.
    pub pairing_state: HostedBridgePairingState,

    /// Locally registered projects (projection only, not authority).
    pub projects: Vec<ProjectReadinessEntry>,
    /// Daemon-owned, operator-scoped capability readiness. This projection is
    /// read-only and contains no Product, pairing-session, or credential keys.
    #[serde(default)]
    pub capability_readiness: Vec<crate::capability_readiness::DaemonCapabilityReadinessV1>,

    /// Whether workspace binding is available at this level.
    /// Always `false` — workspace binding requires explicit request inside a
    /// project.
    pub workspace_binding_available: bool,

    /// Human-readable operator-home status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
}

// ── Selected hosted project (scoped session / projection) — #719 ───────────

/// Safe, opaque reference to the hosted Cloud project the SPA has selected.
///
/// Mirrors the "selected project" identity from the Cloud project discovery
/// contract (bicameral-cloud#64). Carries only cloud-coordination identity that
/// is safe for network-origin exposure — never a local filesystem path,
/// provider secret, repository remote, or source body.
///
/// Authority boundary: the daemon uses this only to *scope* its readiness
/// projection to the selected project. It is NEVER treated as the Cloud
/// membership source — cloud team membership is owned by the cloud tier
/// (ADR-0006); the daemon must not infer membership from, nor grant it via,
/// its local project list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedProjectRef {
    /// Stable cloud-visible id for the daemon-exposed project. Opaque; never a
    /// filesystem path. Resolved against the daemon's locally-registered project
    /// identifiers (the registration name in alpha; a dedicated opaque id is
    /// deferred — see the discovery contract).
    pub project_id: String,

    /// Owning cloud team id (tenancy boundary). Optional at the daemon surface,
    /// which need not know the cloud team to scope local readiness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,

    /// Human label for the selected project. No path/remote/secret leakage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

/// Daemon-reported binding readiness for a *selected* hosted project.
///
/// Coarse readiness describing whether the SPA can safely enable project
/// actions against the paired daemon for the selected project. Composes pairing
/// state and daemon-reported local binding/health. It is **not** a compliance
/// or signoff state.
///
/// The vocabulary mirrors the Cloud discovery contract's `binding_readiness`
/// ladder (bicameral-cloud#64) so the always-current cloud SPA and the local
/// daemon speak the same states.
///
/// [`DaemonBindingReadiness::Unbound`] is the "needs binding" readiness state
/// (bicameral-bot#719): a compatible pairing exists but the daemon has no local
/// workspace binding for the selected project, so only reads are offered and an
/// expose/bind flow is surfaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonBindingReadiness {
    /// No active daemon pairing for the selection. Show pairing/install CTA.
    Unpaired,
    /// Paired daemon protocol incompatible (too old). Block mutable controls;
    /// show upgrade repair hint.
    UpgradeRequired,
    /// Compatible pairing, but the daemon has no local binding for the selected
    /// project yet — the "needs binding" state. Reads only; offer expose/bind.
    Unbound,
    /// Bound but the daemon reports degraded local readiness. Enable reads; warn
    /// on mutable controls.
    Degraded,
    /// Bound and the daemon reports ready. Enable allowlisted dashboard actions.
    Ready,
    /// Readiness not yet resolved. Resolve on select; show neutral state.
    Unknown,
}

impl DaemonBindingReadiness {
    /// Whether allowlisted mutable dashboard controls may be offered for the
    /// selected project. Only a fully [`Ready`](DaemonBindingReadiness::Ready)
    /// selection permits controls; every other state is read-only.
    pub fn controls_available(self) -> bool {
        matches!(self, DaemonBindingReadiness::Ready)
    }
}

/// Project-scoped session response for a selected hosted Cloud project (#719).
///
/// Returned once the SPA selects a project (after user-level pairing). It
/// echoes the selected project identity that this projection is scoped to —
/// proving the selected Cloud project id flows through pairing → projection —
/// and reports the daemon's binding readiness for **exactly that project**,
/// never the unscoped project list.
///
/// Redaction: `project` is a [`ProjectReadinessEntry`] projection (safe fields
/// only); no local path, provider secret, or source body is included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedProjectSessionResponse {
    /// Current user-level pairing state.
    pub pairing_state: HostedBridgePairingState,

    /// The selected project identity this projection is scoped to. Echoed back
    /// so the caller can confirm the selection round-tripped through the daemon.
    pub selected_project: SelectedProjectRef,

    /// Daemon-reported binding readiness for the selected project.
    pub binding_readiness: DaemonBindingReadiness,

    /// Project-scoped safe readiness projection for the selected project.
    /// `None` when the daemon has no locally-registered project matching the
    /// selection (readiness is then [`DaemonBindingReadiness::Unbound`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<ProjectReadinessEntry>,

    /// Whether allowlisted mutable controls may be offered (only when `Ready`).
    pub controls_available: bool,

    /// Hosted-safe workspace binding readiness for the selected project
    /// (bicameral-bot#733): durable binding state, `code_grounding_ready`,
    /// caveats, and a typed next action guiding the hosted UI to the
    /// MCP/CLI/local bind flow. `None` only when the session is not paired and
    /// local readiness cannot be resolved. Carries no local path, provider
    /// token, source body, graph index, or event-store body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_readiness: Option<HostedWorkspaceBindingReadiness>,

    /// ISO-8601 time this selection's readiness was resolved.
    pub resolved_at: String,

    /// Human-readable status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
}

/// Resolve a selected hosted project against the daemon's locally-registered
/// project projections, producing a project-scoped readiness response (#719).
///
/// # Authority boundary
///
/// `local_projects` is matched only to *scope* the projection to the selected
/// project. It is NEVER treated as the Cloud membership source — cloud team
/// membership is owned by the cloud tier (bicameral-cloud#64, ADR-0006). A
/// selected project with no matching local binding resolves to
/// [`DaemonBindingReadiness::Unbound`] ("needs binding"): never an error, and
/// never by inventing or denying membership. The daemon reports only local
/// readiness for the *already-selected* project; it does not decide who may
/// discover which projects.
///
/// Matching uses the selected `project_id` against the daemon's local project
/// identifiers (the registration name in alpha; see [`SelectedProjectRef`]).
pub fn resolve_selected_project_readiness(
    pairing_state: HostedBridgePairingState,
    selected: &SelectedProjectRef,
    local_projects: &[ProjectReadinessEntry],
    resolved_at: &str,
) -> SelectedProjectSessionResponse {
    // Only a live pairing can resolve local project readiness.
    if pairing_state != HostedBridgePairingState::Paired {
        return SelectedProjectSessionResponse {
            pairing_state,
            selected_project: selected.clone(),
            binding_readiness: DaemonBindingReadiness::Unpaired,
            project: None,
            controls_available: false,
            workspace_readiness: None,
            resolved_at: resolved_at.to_string(),
            status_message: Some(
                "No active hosted bridge pairing; re-pair to resolve project readiness."
                    .to_string(),
            ),
        };
    }

    // Scope to the selected project only. The local list is a scoping lookup,
    // NOT a membership source (see authority boundary above).
    let matched = local_projects
        .iter()
        .find(|p| p.name == selected.project_id);

    let (binding_readiness, project, workspace_readiness) = match matched {
        // Daemon has no local binding for the selected project → needs binding.
        // This is a code-grounding readiness gap, not a project authority
        // failure: still project a hosted-safe "needs binding" readiness view
        // (bicameral-bot#733) rather than denying the selection.
        None => {
            let display = ProjectSafeDisplayMetadata {
                display_name: selected
                    .display_name
                    .clone()
                    .unwrap_or_else(|| selected.project_id.clone()),
                project_slug: None,
                candidate_label: None,
            };
            let readiness = project_workspace_binding_readiness(
                &selected.project_id,
                &display,
                WorkspaceBindingState::LocalWorkspaceUnbound,
            );
            (DaemonBindingReadiness::Unbound, None, Some(readiness))
        }
        Some(entry) => {
            let readiness = if !entry.has_workspace_binding {
                DaemonBindingReadiness::Unbound
            } else if !entry.event_store_healthy || entry.setup_state != "normal" {
                DaemonBindingReadiness::Degraded
            } else {
                DaemonBindingReadiness::Ready
            };
            (
                readiness,
                Some(entry.clone()),
                Some(entry.workspace_binding_readiness()),
            )
        }
    };

    let status_message = Some(
        match binding_readiness {
            DaemonBindingReadiness::Unbound => {
                "Selected project has no local workspace binding on this daemon. \
                 Expose or bind the project to enable actions."
            }
            DaemonBindingReadiness::Degraded => {
                "Selected project is bound but the daemon reports degraded local readiness."
            }
            DaemonBindingReadiness::Ready => "Selected project is bound and ready.",
            _ => "Selected project readiness resolved.",
        }
        .to_string(),
    );

    SelectedProjectSessionResponse {
        pairing_state,
        selected_project: selected.clone(),
        controls_available: binding_readiness.controls_available(),
        binding_readiness,
        project,
        workspace_readiness,
        resolved_at: resolved_at.to_string(),
        status_message,
    }
}

// ── Workspace binding readiness projection (bicameral-bot#733) ──────────────

/// Typed next-action affordance the hosted UI renders to move a project toward
/// code-grounding readiness (bicameral-bot#733).
///
/// Renderer-only. The hosted surface CANNOT itself perform the action — pairing
/// alone grants no workspace binding authority. The action tells the dashboard
/// which local flow to guide the user to (MCP/CLI/local daemon).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceReadinessActionKind {
    /// Guide the user to bind a local workspace through the MCP/CLI/local
    /// daemon flow. The hosted surface never binds directly.
    BindLocalWorkspace,
    /// Guide the user to repair a broken local workspace binding through the
    /// MCP/CLI/local daemon flow.
    RepairLocalWorkspace,
}

/// A hosted-safe next-action affordance for workspace binding readiness
/// (bicameral-bot#733).
///
/// Renderer-only and privacy-preserving: it carries a typed action kind, the
/// local surfaces that can perform it, a generic non-path copy-paste command
/// hint, and short human copy. It NEVER carries a local absolute path, provider
/// token, source body, graph index, or event-store body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceReadinessNextAction {
    /// What the hosted UI should guide the user to do.
    pub kind: WorkspaceReadinessActionKind,

    /// Local surfaces that can perform the action (MCP / CLI / local
    /// dashboard). The hosted bridge is not among them.
    pub surfaces: Vec<WorkspaceBindingSourceSurface>,

    /// Generic, non-path copy-paste command hint (e.g. `bicameral workspace
    /// bind`). Never an absolute path or secret.
    pub command: String,

    /// Short human-readable instruction. No path/secret leakage.
    pub instruction: String,
}

/// Hosted-safe workspace binding readiness projection for a single registered
/// project (bicameral-bot#733).
///
/// Projects the daemon-owned durable [`WorkspaceBindingState`]
/// (`local_workspace_unbound` / `local_workspace_bound` /
/// `local_workspace_repair_required`) into a readiness view the hosted
/// dashboard can render. It is a non-authoritative projection: it reports
/// readiness and safe next actions but grants no binding authority and never
/// creates a binding.
///
/// Redaction: carries only the opaque `project_id`, redacted
/// [`ProjectSafeDisplayMetadata`], the durable state enum, a derived
/// `code_grounding_ready` flag, human-readable caveats, and an optional typed
/// next action. It NEVER carries a local absolute path, provider token, source
/// body, graph index, or event-store body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedWorkspaceBindingReadiness {
    /// Opaque project identifier this readiness view describes.
    pub project_id: String,

    /// Redacted display metadata safe for hosted-bridge exposure.
    pub display: ProjectSafeDisplayMetadata,

    /// Daemon-owned durable workspace binding state being projected.
    pub binding_state: WorkspaceBindingState,

    /// Whether the project is ready for code-grounding operations. A missing
    /// or broken workspace binding is a code-grounding readiness gap
    /// (`code_grounding_ready = false`), NOT a project authority failure.
    pub code_grounding_ready: bool,

    /// Human-readable caveats explaining any readiness gap. Empty when ready.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caveats: Vec<String>,

    /// Typed next action guiding the hosted UI toward readiness. `None` when the
    /// project is already bound and ready.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_action: Option<WorkspaceReadinessNextAction>,
}

/// Local surfaces that can perform a workspace bind/repair: MCP thin client,
/// local CLI, and the local review dashboard/TUI. The hosted bridge is
/// intentionally excluded — it never binds.
fn workspace_bind_surfaces() -> Vec<WorkspaceBindingSourceSurface> {
    vec![
        WorkspaceBindingSourceSurface::Mcp,
        WorkspaceBindingSourceSurface::Cli,
    ]
}

/// Project a daemon-owned [`WorkspaceBindingState`] into a hosted-safe
/// [`HostedWorkspaceBindingReadiness`] (bicameral-bot#733).
///
/// Pure and deterministic: it derives `code_grounding_ready`, caveats, and the
/// typed next action from the state alone. It never touches the filesystem,
/// creates a binding, or grants authority — an unbound or repair-required
/// project yields a "needs action" readiness view, never an error.
pub fn project_workspace_binding_readiness(
    project_id: &str,
    display: &ProjectSafeDisplayMetadata,
    binding_state: WorkspaceBindingState,
) -> HostedWorkspaceBindingReadiness {
    let (code_grounding_ready, caveats, next_action) = match binding_state {
        WorkspaceBindingState::LocalWorkspaceBound => (true, Vec::new(), None),
        WorkspaceBindingState::LocalWorkspaceUnbound => (
            false,
            vec![
                "No local workspace binding on this daemon; code grounding is \
                 unavailable until the project is bound."
                    .to_string(),
            ],
            Some(WorkspaceReadinessNextAction {
                kind: WorkspaceReadinessActionKind::BindLocalWorkspace,
                surfaces: workspace_bind_surfaces(),
                command: "bicameral workspace bind".to_string(),
                instruction: "Bind this project to a local code folder from the MCP client or \
                              CLI on the machine running the daemon. The hosted dashboard cannot \
                              bind a workspace."
                    .to_string(),
            }),
        ),
        WorkspaceBindingState::LocalWorkspaceRepairRequired => (
            false,
            vec![
                "The local workspace binding is broken and must be repaired before code \
                 grounding can resume."
                    .to_string(),
            ],
            Some(WorkspaceReadinessNextAction {
                kind: WorkspaceReadinessActionKind::RepairLocalWorkspace,
                surfaces: workspace_bind_surfaces(),
                command: "bicameral workspace repair".to_string(),
                instruction: "Repair the local workspace binding from the MCP client or CLI on \
                              the machine running the daemon."
                    .to_string(),
            }),
        ),
    };

    HostedWorkspaceBindingReadiness {
        project_id: project_id.to_string(),
        display: display.clone(),
        binding_state,
        code_grounding_ready,
        caveats,
        next_action,
    }
}

// ── Error responses ────────────────────────────────────────────────────────

/// Hosted bridge error kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostedBridgeErrorKind {
    /// Pairing has not been established or is expired.
    NotPaired,
    /// Pairing was revoked.
    PairingRevoked,
    /// Pairing session expired.
    PairingExpired,
    /// Origin does not match the paired origin.
    OriginMismatch,
    /// Origin is not on the trusted origin allowlist (#613).
    OriginNotTrusted,
    /// The daemon instance does not match the expected daemon.
    WrongDaemon,
    /// The project does not exist or is not registered locally.
    ProjectNotFound,
    /// The cloud account does not match the paired account.
    AccountMismatch,
    /// The bridge command is not supported by this daemon version.
    UnsupportedCommand,
    /// The command is not allowed through the cloud-routable bridge.
    CommandNotAllowed,
    /// Authority-smuggling attempt detected and rejected.
    AuthoritySmugglingRejected,
    /// The runtime dispatcher is not available (daemon not fully initialized).
    RuntimeUnavailable,
    /// The consumer/daemon speaks a hosted-bridge protocol version older than
    /// the minimum that supports the local-confirmation pending-pairing flow
    /// (#790). Old immediate-pair consumers receive this typed outcome instead
    /// of a paired session; they must update before pairing can proceed.
    UpdateRequired,
    /// The referenced pending pairing request does not exist (unknown handle),
    /// or has already been consumed/cleaned up.
    PendingRequestNotFound,
    /// A local pairing decision (approve/deny) was attempted from a
    /// non-same-origin/non-local surface. Only the locally served confirmation
    /// surface may decide a pending request; hosted/cross-origin attempts fail
    /// closed (#790).
    LocalConfirmationRequired,
    /// The supplied pending-request proof (poll proof or local confirmation
    /// nonce) did not match the stored value. Fails closed without revealing
    /// any token (#790).
    PairingProofMismatch,
}

/// Hosted bridge error response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedBridgeErrorResponse {
    /// Error classification.
    pub error: HostedBridgeErrorKind,

    /// Human-readable error message.
    pub message: String,

    /// Whether the client should retry after re-pairing.
    pub retry_after_repair: bool,
}

// ── Revoked/expired response ───────────────────────────────────────────────

/// Response when a bridge request arrives with a revoked or expired session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedBridgeRevokedResponse {
    /// The terminal state.
    pub state: HostedBridgePairingState,

    /// Reason for revocation/expiry.
    pub reason: String,

    /// When the pairing was revoked or expired (ISO-8601).
    pub revoked_at: String,

    /// Whether re-pairing is available.
    pub re_pair_available: bool,
}

// ── Authority-smuggling rejection ──────────────────────────────────────────

/// Fields that are forbidden in hosted bridge payloads.
///
/// Any payload containing these fields through the bridge route must be
/// rejected — they represent authority-smuggling attempts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoritySmugglingPayload {
    /// The raw payload that was rejected.
    pub raw_payload: serde_json::Value,

    /// Which forbidden fields were detected.
    pub forbidden_fields: Vec<String>,

    /// The rejection response.
    pub rejection: HostedBridgeErrorResponse,
}

/// List of field names that are always forbidden in hosted bridge payloads.
///
/// If any of these appear in a bridge request body, the request is rejected
/// with `AuthoritySmugglingRejected`.
pub const BRIDGE_FORBIDDEN_FIELDS: &[&str] = &[
    "authority_context",
    "tool_command",
    "review_command",
    "signoff_approval",
    "compliance_resolution",
    "decision_write",
    "event_store_write",
    "source_binding",
    "external_ingest_envelope",
    "provider_token",
    "workspace_path",
    "policy_grant",
    "no_conflict_claim",
    "canonical_decision",
];

// ── Compatibility envelope ──────────────────────────────────────────────────

/// Protocol compatibility outcome between a hosted cloud surface and a local
/// daemon.
///
/// Determines whether the daemon version can service cloud-routed requests at
/// the requested protocol version. The compatibility window is bounded:
/// current-version and previous-minor are supported; older versions are blocked
/// and newer versions degrade mutable controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityOutcome {
    /// Fully compatible — daemon protocol matches the cloud surface version.
    FullyCompatible,
    /// Compatible with degraded features — daemon is one minor version behind.
    DegradedCompatible,
    /// Blocked — daemon version is too old for the cloud surface.
    Blocked,
    /// Newer daemon — cloud surface cannot drive all daemon capabilities.
    /// Mutable controls are blocked; read-only projections may still work.
    NewerDaemonDegraded,
}

/// Protocol compatibility envelope for a daemon/cloud handshake.
///
/// Captures the protocol version pair and the resulting compatibility outcome.
/// Fixtures model the bounded compatibility window without expanding beyond
/// current + previous daemon minor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolCompatibilityEnvelope {
    /// Protocol version advertised by the cloud surface.
    pub cloud_protocol_version: String,

    /// Protocol version advertised by the local daemon.
    pub daemon_protocol_version: String,

    /// Compatibility outcome for this version pair.
    pub outcome: CompatibilityOutcome,

    /// Whether mutable controls (command dispatch) are available.
    pub mutable_controls_available: bool,

    /// Whether read-only projections are available.
    pub read_only_projections_available: bool,

    /// Human-readable compatibility note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

// ── Hosted-dashboard daemon protocol compatibility window (#697 / #711) ─────

/// The hosted-dashboard bridge protocol version this daemon build speaks.
///
/// A monotonically increasing integer bumped ONLY on a breaking change to the
/// hosted-bridge protocol / read-projection contract — never on a release or
/// semver bump. Compatibility between the always-current cloud-served SPA and a
/// locally-installed (possibly older) daemon is keyed on THIS value, not on the
/// daemon's release version / semver (#697, ADR-0026).
///
/// Version 3 (#790) is a breaking change: `POST /pair` no longer issues a
/// session token directly. Instead it creates a transient pending request that
/// requires an explicit same-origin local approval before a session is
/// released. Consumers/daemons that speak an older version (immediate-pair)
/// are incompatible with the confirmation flow and must update.
///
/// Version 4 (#865) adds a browser-displayed, terminal-entered pairing code.
/// Code matching only locates the request; an explicit local decision remains
/// the sole session-minting transition.
pub const HOSTED_BRIDGE_PROTOCOL_VERSION: u32 = 4;

/// Minimum hosted-bridge protocol version that supports the local-confirmation
/// pending-pairing flow (#790).
///
/// This is a security floor distinct from the rolling N-1 read-projection
/// support window: a consumer/daemon below this version cannot participate in
/// local confirmation and receives a typed `update_required` outcome from the
/// pairing endpoint rather than a paired session. It must never regress below
/// the version that introduced local confirmation.
pub const PENDING_PAIRING_MIN_PROTOCOL_VERSION: u32 = 4;

/// Daemon release/build identifier. Diagnostic only; never a protocol key.
pub const DAEMON_BUILD_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Legacy name retained for one compatibility window.
pub const HOSTED_DASHBOARD_PROTOCOL_VERSION: u32 = HOSTED_BRIDGE_PROTOCOL_VERSION;

/// Alpha support-window depth: how many breaking protocol versions behind the
/// cloud SPA's expected version a daemon may be and still be served.
///
/// `1` means "current plus N-1": the current protocol version and exactly one
/// previous are accepted. A daemon `HOSTED_DASHBOARD_SUPPORT_WINDOW + 1` (i.e.
/// 2+) breaking versions behind is gated with `hosted_dashboard_upgrade_required`.
pub const HOSTED_DASHBOARD_SUPPORT_WINDOW: u32 = 1;

/// Copy-paste command a Member can run to update their local daemon to a
/// hosted-dashboard-compatible protocol version.
///
/// This is the one-action repair path (ADR-0026): it names no workspace,
/// ledger, source, evidence, or local path.
pub const HOSTED_DASHBOARD_UPGRADE_COMMAND: &str = "bicameral daemon update";

/// Outcome of comparing the cloud SPA's expected hosted-dashboard protocol
/// version against the local daemon's advertised `daemon_protocol_version`.
///
/// Keyed strictly on protocol-version integers (never release/semver). The
/// alpha window is "current + N-1"; older daemons are gated and newer daemons
/// degrade to a benign catching-up state rather than hard-failing (#697).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostedDashboardCompatibilityState {
    /// Daemon speaks exactly the protocol the cloud SPA expects.
    Current,
    /// Daemon is one breaking protocol version behind (N-1) — still inside the
    /// alpha support window; served with a compatibility notice.
    SupportedPrevious,
    /// Daemon is two or more breaking protocol versions behind — gated with a
    /// one-action upgrade repair path. This is the explicit
    /// `hosted_dashboard_upgrade_required` state from #697.
    #[serde(rename = "hosted_dashboard_upgrade_required")]
    UpgradeRequired,
    /// Daemon is newer than the cloud SPA expects — forward skew. The dashboard
    /// is "catching up"; it degrades safely (read-only) rather than failing.
    #[serde(rename = "hosted_dashboard_forward_skew")]
    ForwardSkew,
}

impl HostedDashboardCompatibilityState {
    /// Stable string tag used in telemetry and dashboard copy.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::SupportedPrevious => "supported_previous",
            Self::UpgradeRequired => "hosted_dashboard_upgrade_required",
            Self::ForwardSkew => "hosted_dashboard_forward_skew",
        }
    }
}

/// One-action repair path shown when the daemon is too old.
///
/// Renderer-only, non-confidential: carries only a generic install/update
/// command and human copy — never a workspace, ledger, source, path, or secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedDashboardUpgradeRepair {
    /// Whether the daemon can update itself in one click (auto-update).
    pub auto_update_available: bool,

    /// A single copy-paste command that updates the local daemon.
    pub update_command: String,

    /// Short, human-readable one-action instruction.
    pub instruction: String,
}

/// Result of a hosted-dashboard daemon protocol compatibility check.
///
/// Non-confidential handshake/readiness metadata only — it grants no authority
/// and carries no workspace/ledger/source/path data. Hosted/cloud/browser use
/// this purely to decide which UI state to render (renderer-only; ADR-0025).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedDashboardCompatibility {
    /// Protocol version the cloud-served SPA expects.
    pub cloud_expected_protocol_version: u32,

    /// Protocol version the local daemon advertises.
    pub daemon_protocol_version: u32,

    /// Compatibility state for this version pair.
    pub state: HostedDashboardCompatibilityState,

    /// How many breaking protocol versions the daemon is behind the cloud SPA
    /// (0 = current, negative = daemon is ahead / forward skew).
    pub protocol_versions_behind: i64,

    /// Whether the daemon must be upgraded before the dashboard will serve it.
    pub upgrade_required: bool,

    /// Whether the daemon is newer than the cloud SPA expects (forward skew).
    pub forward_skew: bool,

    /// Whether mutable controls may be shown. Renderer-only always, but controls
    /// are additionally hidden outside the fully-`Current` state.
    pub controls_available: bool,

    /// Whether read-only projections may be rendered. Available in every state
    /// except `UpgradeRequired` (a too-old daemon is gated entirely).
    pub read_only_projections_available: bool,

    /// One-action repair path — present only when `upgrade_required`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repair: Option<HostedDashboardUpgradeRepair>,

    /// Human-readable compatibility note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Evaluate hosted-dashboard compatibility from protocol-version integers.
///
/// Compatibility is keyed strictly on `daemon_protocol_version` vs the cloud
/// SPA's expected protocol version — never on release version / semver (#697):
///
/// - daemon ahead of cloud → `ForwardSkew` (benign; read-only, controls hidden)
/// - daemon == cloud → `Current` (full)
/// - daemon exactly N-1 behind → `SupportedPrevious` (inside window; read + controls)
/// - daemon 2+ behind → `UpgradeRequired` (gated; one-action repair path)
pub fn evaluate_hosted_dashboard_compatibility(
    cloud_expected_protocol_version: u32,
    daemon_protocol_version: u32,
) -> HostedDashboardCompatibility {
    let behind = cloud_expected_protocol_version as i64 - daemon_protocol_version as i64;
    let window = HOSTED_DASHBOARD_SUPPORT_WINDOW as i64;

    let mut repair = None;
    let mut note = None;
    let (
        state,
        controls_available,
        read_only_projections_available,
        upgrade_required,
        forward_skew,
    ) = if behind < 0 {
        note = Some(
            "The local daemon is newer than this dashboard expects. The dashboard is \
                 catching up; some controls are hidden until it updates."
                .to_string(),
        );
        (
            HostedDashboardCompatibilityState::ForwardSkew,
            false,
            true,
            false,
            true,
        )
    } else if behind == 0 {
        (
            HostedDashboardCompatibilityState::Current,
            true,
            true,
            false,
            false,
        )
    } else if behind <= window {
        note = Some(
            "The local daemon is one protocol version behind but still supported. Update \
                 when convenient to stay current."
                .to_string(),
        );
        (
            HostedDashboardCompatibilityState::SupportedPrevious,
            true,
            true,
            false,
            false,
        )
    } else {
        note = Some(
            "The local daemon is too far behind to use with the hosted dashboard. Update \
                 the daemon to continue."
                .to_string(),
        );
        repair = Some(HostedDashboardUpgradeRepair {
            auto_update_available: false,
            update_command: HOSTED_DASHBOARD_UPGRADE_COMMAND.to_string(),
            instruction: "Run this command to update your local Bicameral daemon, then reconnect."
                .to_string(),
        });
        (
            HostedDashboardCompatibilityState::UpgradeRequired,
            false,
            false,
            true,
            false,
        )
    };

    HostedDashboardCompatibility {
        cloud_expected_protocol_version,
        daemon_protocol_version,
        state,
        protocol_versions_behind: behind,
        upgrade_required,
        forward_skew,
        controls_available,
        read_only_projections_available,
        repair,
        note,
    }
}

/// Non-confidential aggregate telemetry for hosted-dashboard compatibility
/// checks.
///
/// Records ONLY protocol-version integers and outcome counts — never a
/// workspace, ledger, source, evidence, provider secret, or local path
/// (ADR-0025 / #711). Safe to expose to operators and to hosted cloud.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedDashboardCompatibilityTelemetry {
    /// Total compatibility handshakes observed.
    pub total_checks: u64,

    /// How many produced `hosted_dashboard_upgrade_required`.
    pub upgrade_required_count: u64,

    /// How many produced forward-skew.
    pub forward_skew_count: u64,

    /// Observed daemon protocol-version distribution: version → count.
    pub daemon_protocol_version_distribution: BTreeMap<u32, u64>,
}

impl HostedDashboardCompatibilityTelemetry {
    /// Fold a single compatibility outcome into the aggregate.
    pub fn record(&mut self, compat: &HostedDashboardCompatibility) {
        self.total_checks += 1;
        match compat.state {
            HostedDashboardCompatibilityState::UpgradeRequired => {
                self.upgrade_required_count += 1;
            }
            HostedDashboardCompatibilityState::ForwardSkew => {
                self.forward_skew_count += 1;
            }
            _ => {}
        }
        *self
            .daemon_protocol_version_distribution
            .entry(compat.daemon_protocol_version)
            .or_insert(0) += 1;
    }

    /// Upgrade-required fire rate in `[0.0, 1.0]` (0.0 before any checks).
    pub fn upgrade_required_fire_rate(&self) -> f64 {
        if self.total_checks == 0 {
            0.0
        } else {
            self.upgrade_required_count as f64 / self.total_checks as f64
        }
    }
}

// ── Pending-pairing protocol compatibility (#790) ──────────────────────────

/// Outcome of checking a pairing consumer's declared protocol version against
/// the local-confirmation security floor (#790).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingProtocolCompatibility {
    /// The consumer is confirmation-capable; a pending request may be created.
    Compatible,
    /// The consumer is too old for the confirmation flow and must update.
    UpdateRequired {
        /// The version the consumer declared (0 when none was declared).
        client_protocol_version: u32,
    },
}

/// Evaluate whether a pairing consumer is compatible with the local-confirmation
/// pending-pairing flow (#790).
///
/// Keyed strictly on the declared hosted-bridge protocol version. A consumer
/// that declares no version reads as `0` (oldest / immediate-pair) and is
/// treated as incompatible — fail closed rather than silently bypassing
/// confirmation. Any version at or above
/// [`PENDING_PAIRING_MIN_PROTOCOL_VERSION`] is confirmation-capable.
pub fn evaluate_pairing_protocol_compatibility(
    client_protocol_version: Option<u32>,
) -> PairingProtocolCompatibility {
    let declared = client_protocol_version.unwrap_or(0);
    if declared >= PENDING_PAIRING_MIN_PROTOCOL_VERSION {
        PairingProtocolCompatibility::Compatible
    } else {
        PairingProtocolCompatibility::UpdateRequired {
            client_protocol_version: declared,
        }
    }
}

/// Build the typed `update_required` response for an incompatible pairing
/// consumer (#790).
pub fn pairing_update_required_response(
    client_protocol_version: u32,
) -> HostedBridgePairingUpdateRequiredResponse {
    HostedBridgePairingUpdateRequiredResponse {
        status: PendingPairingOutcome::Incompatible,
        error: HostedBridgeErrorKind::UpdateRequired,
        required_protocol_version: PENDING_PAIRING_MIN_PROTOCOL_VERSION,
        client_protocol_version,
        daemon_protocol_version: HOSTED_BRIDGE_PROTOCOL_VERSION,
        repair: HostedDashboardUpgradeRepair {
            auto_update_available: false,
            update_command: HOSTED_DASHBOARD_UPGRADE_COMMAND.to_string(),
            instruction: "Update to a Bicameral hosted-bridge client that supports local \
                          pairing confirmation, then reconnect."
                .to_string(),
        },
        message: format!(
            "This hosted-bridge client speaks protocol v{client_protocol_version}, but local \
             pairing confirmation requires v{PENDING_PAIRING_MIN_PROTOCOL_VERSION} or newer. \
             The daemon will not issue a session to an immediate-pair client."
        ),
    }
}

/// Whether an `Origin` header value denotes a same-origin/local confirmation
/// surface allowed to decide a pending pairing request (#790).
///
/// The locally served confirmation surface runs on the daemon's own loopback
/// origin. A same-origin browser fetch either omits `Origin` (some same-origin
/// GETs) or sends a loopback origin. Any non-loopback origin — including every
/// trusted hosted origin — is rejected, so hosted/cross-origin callers cannot
/// approve a pending request.
pub fn is_local_confirmation_origin(origin: Option<&str>) -> bool {
    match origin {
        // A missing Origin header means a same-origin request from the locally
        // served surface (or a non-browser local caller). Allowed.
        None => true,
        Some(o) => {
            let o = o.trim();
            if o.is_empty() {
                return true;
            }
            // Accept only loopback http origins, with or without a port.
            for host in ["127.0.0.1", "localhost", "[::1]"] {
                let base = format!("http://{host}");
                if o == base || o.starts_with(&format!("{base}:")) {
                    return true;
                }
            }
            false
        }
    }
}

// ── Cloud-routable command allowlist ────────────────────────────────────────

/// A cloud-routable bridge command family.
///
/// Defines whether a named command family is allowed or forbidden through the
/// hosted bridge. Allowed commands are dashboard-intent read/projection
/// operations. Forbidden commands are authority-bearing mutations (signoff,
/// Decision Ledger writes, source-binding, compliance outcomes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeCommandFamily {
    /// Command family name (e.g. "list_projects", "signoff").
    pub name: String,

    /// Whether this command family is allowed through the bridge.
    pub allowed: bool,

    /// Reason for the allow/deny decision.
    pub reason: String,
}

/// Cloud-routable command allowlist constants.
///
/// Commands allowed through the hosted bridge are dashboard-intent operations
/// that read projections or trigger non-authority actions. Commands that write
/// to the Decision Ledger, perform signoff, mutate source bindings, or resolve
/// compliance outcomes are forbidden.
pub const BRIDGE_ALLOWED_COMMANDS: &[&str] = &[
    "list_projects",
    "get_project_status",
    "get_decision_projection",
    "get_ledger_summary",
    "get_workspace_readiness",
];

pub const BRIDGE_FORBIDDEN_COMMANDS: &[&str] = &[
    "signoff",
    "decision_write",
    "source_bind",
    "compliance_resolve",
    "event_store_write",
    "review_approve",
    "review_reject",
    "workspace_bind",
    "provider_configure",
];

// ── Team-visible workspace discovery ───────────────────────────────────────

/// Minimal team-visible workspace discovery payload.
///
/// Exposes only the fields safe for team-visible workspace discovery through
/// the hosted bridge. Rich metadata (filesystem paths, provider secrets,
/// source evidence bodies, Decision Ledger event payloads) are excluded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamVisibleWorkspaceDiscovery {
    /// Project name.
    pub project_name: String,

    /// Whether the event store is healthy.
    pub event_store_healthy: bool,

    /// Whether the project has a workspace binding.
    pub has_workspace_binding: bool,

    /// Setup state (e.g. "normal", "repair_required", "code_grounding_not_ready").
    pub setup_state: String,

    /// Whether the workspace is ready for code grounding operations.
    pub code_grounding_ready: bool,
}

// ── Cloud-routable ToolCommand allowlist ────────────────────────────────────

/// ToolCommand names that are cloud-routable through the hosted bridge.
///
/// Only read-only, projection, and advisory commands may be dispatched through
/// the bridge. Commands that write to the Decision Ledger, perform signoff,
/// mutate source bindings, resolve compliance, refresh graph indexes, perform
/// unrestricted ingest, or access local filesystem are forbidden.
pub const CLOUD_ROUTABLE_TOOL_COMMANDS: &[&str] = &[
    "preflight.run",
    "history.list",
    "search.query",
    "code.locate",
    "graph.status",
    "binding.inspect",
    "decision.find_code_impact",
    "recall.inspect_evidence",
    "recall.expand_scope",
    "tracking.refresh_query",
    // Daemon-canonical hosted Product creation (#766). Unlike other cloud-routed
    // commands these are canonical writes, but they are authorized against the
    // daemon-confirmed hosted actor context (paired session + bound Cloud
    // account) rather than caller-supplied authority, and the daemon remains the
    // sole canonical writer. Cloud never becomes the canonical Product writer:
    // `product.publish` only derives a non-canonical directory row from the
    // committed creation receipt.
    "product.create",
    "product.publish",
];

/// ToolCommand names explicitly forbidden through the hosted bridge.
///
/// Owner-only, signoff, compliance, graph refresh, unrestricted ingest,
/// local filesystem, and source/event-store write authority commands.
pub const CLOUD_FORBIDDEN_TOOL_COMMANDS: &[&str] = &[
    "ingest.submit_local",
    "ingest.submit_managed",
    "review.accept_candidate",
    "review.reject_candidate",
    "review.approve_signoff",
    "review.reject_signoff",
    "review.resolve_compliance",
    "binding.create",
    // Daemon-owned workspace binding (#732): binding persistence is a
    // local-daemon authority. The hosted bridge may only project readiness, so
    // routing `workspace.bind` / `workspace.resolve` through the cloud fails
    // closed (#747 / mcp#831, ADR-0005). Reverse-lookup carries a local
    // candidate_path hint and must stay operator-local.
    "workspace.bind",
    "workspace.resolve",
    "evidence.refresh",
    "tracking.untrack_source",
    "graph.refresh_snapshot",
    "recall.promote_decision_candidate",
    "recall.request_correction",
];

/// Check whether a ToolCommand name is cloud-routable through the hosted bridge.
pub fn is_cloud_routable_command(command_name: &str) -> bool {
    CLOUD_ROUTABLE_TOOL_COMMANDS.contains(&command_name)
}

// ── Trusted origin allowlist (#613) ─────────────────────────────────────────

/// Default trusted origins for hosted bridge pairing.
///
/// Only origins on this allowlist (or operator-configured custom origins) may
/// initiate or maintain a hosted bridge pairing session. Invalid origins are
/// rejected with typed errors before session creation or command routing.
///
/// **Do not** put local Vite (`http://127.0.0.1:5173`) here — that would ship
/// to every install. Use `BICAMERAL_DEV_TRUST_VITE=1` or
/// `BICAMERAL_HOSTED_BRIDGE_EXTRA_ORIGINS` for local dashboard smoke only.
pub const DEFAULT_TRUSTED_ORIGINS: &[&str] = &[
    "https://app.bicameral-ai.com",
    "https://staging.bicameral-ai.com",
];

/// Vite default loopback origins used only when `BICAMERAL_DEV_TRUST_VITE` is set.
pub const DEV_VITE_TRUSTED_ORIGINS: &[&str] = &["http://127.0.0.1:5173", "http://localhost:5173"];

/// Opt-in: trust local Vite `:5173` for hosted-bridge pairing (local smoke only).
///
/// Accepted values: `1`, `true`, `yes` (case-insensitive).
pub const DEV_TRUST_VITE_ENV: &str = "BICAMERAL_DEV_TRUST_VITE";

/// Comma-separated extra trusted origins (exact scheme+host+port).
///
/// Example: `http://127.0.0.1:5173,http://localhost:5173`
pub const EXTRA_TRUSTED_ORIGINS_ENV: &str = "BICAMERAL_HOSTED_BRIDGE_EXTRA_ORIGINS";

/// Origin validation outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OriginValidationOutcome {
    /// Origin is trusted — pairing/routing may proceed.
    Trusted,
    /// Origin is not on the allowlist.
    Rejected { origin: String, reason: String },
}

fn env_flag_enabled(name: &str) -> bool {
    match std::env::var(name) {
        Ok(raw) => {
            let t = raw.trim();
            t == "1" || t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("yes")
        }
        Err(_) => false,
    }
}

/// Resolve opt-in extra trusted origins from the process environment.
///
/// - `BICAMERAL_DEV_TRUST_VITE=1` → Vite loopback on `:5173`
/// - `BICAMERAL_HOSTED_BRIDGE_EXTRA_ORIGINS` → comma-separated exact origins
///
/// Defaults stay production-only. Gateway seeds `AppState.custom_trusted_origins`
/// from this at startup.
pub fn extra_trusted_origins_from_env() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();

    if env_flag_enabled(DEV_TRUST_VITE_ENV) {
        for origin in DEV_VITE_TRUSTED_ORIGINS {
            out.push((*origin).to_string());
        }
    }

    if let Ok(raw) = std::env::var(EXTRA_TRUSTED_ORIGINS_ENV) {
        for part in raw.split(',') {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                continue;
            }
            if !out.iter().any(|existing| existing == trimmed) {
                out.push(trimmed.to_string());
            }
        }
    }

    out
}

/// Validate an origin against the trusted origin allowlist.
///
/// Checks the supplied `origin` against `DEFAULT_TRUSTED_ORIGINS` plus any
/// operator-configured `custom_origins` (including env-seeded dev origins).
/// Origins must be exact string matches (scheme + host + optional port). Empty
/// or whitespace-only origins are always rejected.
pub fn validate_origin(origin: &str, custom_origins: &[String]) -> OriginValidationOutcome {
    let trimmed = origin.trim();
    if trimmed.is_empty() {
        return OriginValidationOutcome::Rejected {
            origin: origin.to_string(),
            reason: "Origin must not be empty.".to_string(),
        };
    }

    // Check default allowlist.
    if DEFAULT_TRUSTED_ORIGINS.contains(&trimmed) {
        return OriginValidationOutcome::Trusted;
    }

    // Check operator-configured custom origins.
    if custom_origins.iter().any(|c| c.as_str() == trimmed) {
        return OriginValidationOutcome::Trusted;
    }

    OriginValidationOutcome::Rejected {
        origin: trimmed.to_string(),
        reason: format!(
            "Origin '{}' is not on the trusted origin allowlist. \
             Allowed origins: {:?}. For local Vite smoke set \
             BICAMERAL_DEV_TRUST_VITE=1 (or BICAMERAL_HOSTED_BRIDGE_EXTRA_ORIGINS).",
            trimmed, DEFAULT_TRUSTED_ORIGINS,
        ),
    }
}

// ── Safe workspace/project metadata projection (#614) ──────────────────────

/// Safe team-visible workspace metadata for hosted discovery.
///
/// Contains only the fields that are safe for team-visible exposure through
/// the hosted bridge. The following are explicitly excluded:
/// - Local filesystem paths (`workspace_path`)
/// - Provider secrets or tokens
/// - `SourceEvidence` bodies
/// - `Decision Ledger` event payloads
/// - Authority-bearing fields (signoff state, compliance state)
///
/// This is a projection/read-model only — not canonical authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafeWorkspaceMetadata {
    /// Project display name.
    pub project_name: String,

    /// Whether the project's event-store is accessible and healthy.
    pub event_store_healthy: bool,

    /// Whether the project has at least one workspace binding.
    pub has_workspace_binding: bool,

    /// Setup state summary (e.g. "normal", "repair_required",
    /// "code_grounding_not_ready").
    pub setup_state: String,

    /// Whether code grounding is ready (derived from workspace binding state).
    pub code_grounding_ready: bool,

    /// Number of accepted Decisions (count only, no bodies or payloads).
    #[serde(default)]
    pub decision_count: u32,

    /// Number of pending candidates (count only, no bodies or payloads).
    #[serde(default)]
    pub candidate_count: u32,

    /// Event store kind label (e.g. "github", "google_drive", "memory").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_store_kind: Option<String>,
}

/// Fields that must NEVER appear in any hosted bridge response.
///
/// Used by leakage tests to verify that safe projections do not
/// accidentally expose sensitive data.
pub const NEVER_EXPOSE_FIELDS: &[&str] = &[
    "workspace_path",
    "provider_config",
    "provider_token",
    "provider_secret",
    "api_key",
    "access_token",
    "refresh_token",
    "source_evidence_body",
    "event_payload",
    "local_path",
    "filesystem_path",
    "home_dir",
];

// ── Hosted-mode dashboard data adapter (#616) ───────────────────────────────

/// Hosted dashboard data mode.
///
/// The shipped dashboard reads from the bridge-gated hosted adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DashboardDataMode {
    /// Hosted mode: dashboard reads through bridge-gated adapter.
    Hosted,
}

/// Hosted dashboard adapter response envelope.
///
/// Wraps bridge-safe projection data for hosted dashboard consumption.
/// Canonical mutations are still routed through daemon/ToolRequest paths;
/// the adapter only provides read-model data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostedDashboardAdapterResponse {
    /// Data mode used for this response.
    pub mode: DashboardDataMode,

    /// Safe workspace metadata (no sensitive fields).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<SafeWorkspaceMetadata>,

    /// Safe ledger summary (decision/candidate counts, no bodies).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ledger_summary: Option<HostedLedgerSummary>,

    /// Project list (safe projection).
    #[serde(default)]
    pub projects: Vec<ProjectReadinessEntry>,

    /// Human-readable adapter status message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
}

/// Safe ledger summary for hosted dashboard consumption.
///
/// Contains aggregate counts and high-level status only. No Decision
/// bodies, SourceEvidence excerpts, event payloads, or authority-bearing
/// fields are included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedLedgerSummary {
    /// Total accepted Decisions.
    pub decision_count: u32,

    /// Total pending candidates.
    pub candidate_count: u32,

    /// Decisions needing signoff review.
    pub needs_signoff_count: u32,

    /// Decisions with stale source evidence.
    pub stale_source_count: u32,

    /// Decisions with verified code bindings.
    pub bound_count: u32,

    /// Most recent Decision promotion timestamp (ISO-8601), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_promoted_at: Option<String>,
}

// ── T4 renderable ledger allow-list (#705, approves #698 item 5) ─────────────
//
// The hosted dashboard data plane is browser ↔ local daemon (direct), where the
// browser runs on a network origin (`app.bicameral-ai.com`) and the daemon holds
// all Confidential data. Per ADR-0025 the hosted tier is a *renderer* that never
// holds Confidential data, and per ADR-0020 the local daemon stays the sole
// canonical authority. The types below pin the exact, minimal, positive
// allow-list of ledger fields that are safe to render on that network origin,
// approved in #698 item 5:
//
//   ids, timestamps, status/review-state enums, counts, and redacted titles.
//
// Everything else — decision bodies, source-evidence excerpts/bodies, code file
// paths, binding explanations, `source_id`, `promoted_by` actor identity,
// provenance, provider secrets, and raw event-store payloads — stays daemon-side
// and must never cross the boundary. The full local `ProjectionState`
// (`crate::projection`) carries those confidential fields and therefore must not
// be served on a network origin; the daemon projects into these types first.

/// A single render-safe ledger row for a network-origin hosted dashboard.
///
/// This is the positive allow-list approved in #698 item 5 (T4). Only opaque
/// ids, lifecycle/review-state enums, a timestamp, aggregate counts, and a
/// redactor-passed `title` may cross the browser boundary (ADR-0020 / ADR-0025).
///
/// Intentionally excluded (must stay daemon-side): decision `body`,
/// source-evidence `excerpt`/bodies, code file paths, binding `explanation`,
/// `source_id`, `promoted_by` actor identity, and `provenance`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderableLedgerEntry {
    /// Opaque Decision id.
    pub decision_id: Uuid,
    /// Opaque originating candidate id.
    pub candidate_id: Uuid,
    /// Decision title after the sensitive-pattern redactor has run. Never a
    /// raw decision body — titles are passed through the redactor before
    /// crossing the boundary (#698 item 5).
    pub title: String,
    /// Decision level enum.
    pub level: DecisionLevel,
    /// Signoff state enum (review-state, not authority material).
    pub signoff_state: SignoffState,
    /// Review lifecycle state enum.
    pub review_state: ReviewLifecycleState,
    /// Tracking state enum.
    pub tracking_state: TrackingState,
    /// Source freshness enum (fresh/stale), derived — not a source body.
    pub source_freshness: SourceFreshness,
    /// Graph evidence state enum (verified / unknown), derived — not code.
    pub evidence_state: GraphEvidenceState,
    /// Promotion timestamp.
    pub promoted_at: DateTime<Utc>,
    /// Count of source-evidence items (count only, no bodies/excerpts).
    pub source_evidence_count: u32,
    /// Count of code bindings (count only, no paths/explanations).
    pub binding_count: u32,
}

/// A render-safe ledger event row for a network-origin hosted dashboard.
///
/// Only the opaque event id, the lifecycle event-type enum, and the timestamp
/// cross the boundary. The human-readable summary and the actor are dropped:
/// actor identity is signoff/authority material outside the approved allow-list
/// (#698 item 5, ADR-0020).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderableLedgerEvent {
    /// Opaque event id.
    pub id: Uuid,
    /// Lifecycle event-type enum.
    pub event_type: EventType,
    /// Event timestamp.
    pub timestamp: DateTime<Utc>,
}

/// Network-origin renderable ledger projection (#705, T4).
///
/// This is the *named projection path* that pins the exact renderable field set
/// for hosted ledger/detail data. The daemon builds it from canonical state and
/// redacts before it crosses the browser boundary (ADR-0020 / ADR-0025); the
/// hosted SPA renders it verbatim and must not derive additional confidential
/// detail client-side. It is a non-authoritative read model — it never carries
/// or materializes governance state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostedLedgerProjection {
    /// Aggregate counts (no bodies).
    pub summary: HostedLedgerSummary,
    /// Per-Decision render-safe rows (allow-listed fields only).
    #[serde(default)]
    pub decisions: Vec<RenderableLedgerEntry>,
    /// Ledger event rows (id / type / timestamp only).
    #[serde(default)]
    pub events: Vec<RenderableLedgerEvent>,
    /// Always `true` — marks this as a non-authoritative read projection.
    pub projection_only: bool,
}

/// The exact set of Decision field names approved for network-origin rendering
/// (#698 item 5). Any field not in this list must stay daemon-side. Leakage
/// tests pin the allow-list against this constant.
pub const RENDERABLE_LEDGER_FIELDS: &[&str] = &[
    "decision_id",
    "candidate_id",
    "title",
    "level",
    "signoff_state",
    "review_state",
    "tracking_state",
    "source_freshness",
    "evidence_state",
    "promoted_at",
    "source_evidence_count",
    "binding_count",
];

/// Confidential field names that must never appear in a renderable ledger
/// projection (#698 item 5). These are the decision/evidence fields the full
/// local `ProjectionState` carries but the network-origin projection drops.
/// Leakage tests assert none of these appear in the serialized payload.
pub const LEDGER_FORBIDDEN_FIELDS: &[&str] = &[
    "body",
    "excerpt",
    "explanation",
    "promoted_by",
    "provenance",
    "source_id",
    "snapshot_id",
    "span_start",
    "span_end",
    "compliance_state",
];

/// Check whether a JSON payload contains any forbidden authority-smuggling fields.
pub fn detect_smuggled_fields(payload: &serde_json::Value) -> Vec<String> {
    let mut found = Vec::new();
    if let Some(obj) = payload.as_object() {
        for &field in BRIDGE_FORBIDDEN_FIELDS {
            if obj.contains_key(field) {
                found.push(field.to_string());
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Origin validation tests (#613) ─────────────────────────────

    #[test]
    fn trusted_default_origins_accepted() {
        for origin in DEFAULT_TRUSTED_ORIGINS {
            assert_eq!(
                validate_origin(origin, &[]),
                OriginValidationOutcome::Trusted,
                "Default origin {} should be trusted",
                origin,
            );
        }
    }

    #[test]
    fn custom_origin_accepted() {
        let custom = vec!["https://custom.example.com".to_string()];
        assert_eq!(
            validate_origin("https://custom.example.com", &custom),
            OriginValidationOutcome::Trusted,
        );
    }

    #[test]
    fn unknown_origin_rejected() {
        let result = validate_origin("https://evil.example.com", &[]);
        assert!(matches!(result, OriginValidationOutcome::Rejected { .. }));
    }

    #[test]
    fn empty_origin_rejected() {
        let result = validate_origin("", &[]);
        assert!(matches!(result, OriginValidationOutcome::Rejected { .. }));
    }

    #[test]
    fn whitespace_origin_rejected() {
        let result = validate_origin("   ", &[]);
        assert!(matches!(result, OriginValidationOutcome::Rejected { .. }));
    }

    #[test]
    fn origin_must_be_exact_match() {
        // Subdomain of a trusted origin is not trusted.
        let result = validate_origin("https://sub.app.bicameral-ai.com", &[]);
        assert!(matches!(result, OriginValidationOutcome::Rejected { .. }));
    }

    #[test]
    fn http_origin_rejected() {
        // http:// variant of a trusted origin is not trusted.
        let result = validate_origin("http://app.bicameral-ai.com", &[]);
        assert!(matches!(result, OriginValidationOutcome::Rejected { .. }));
    }

    #[test]
    fn vite_loopback_rejected_without_dev_opt_in() {
        // Production default must not trust local Vite.
        let result = validate_origin("http://127.0.0.1:5173", &[]);
        assert!(matches!(result, OriginValidationOutcome::Rejected { .. }));
        let result = validate_origin("http://localhost:5173", &[]);
        assert!(matches!(result, OriginValidationOutcome::Rejected { .. }));
    }

    #[test]
    fn vite_loopback_accepted_via_custom_origins() {
        let custom: Vec<String> = DEV_VITE_TRUSTED_ORIGINS
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        assert_eq!(
            validate_origin("http://127.0.0.1:5173", &custom),
            OriginValidationOutcome::Trusted,
        );
        assert_eq!(
            validate_origin("http://localhost:5173", &custom),
            OriginValidationOutcome::Trusted,
        );
    }

    #[test]
    fn extra_trusted_origins_from_env_dev_vite_flag() {
        // Isolate from ambient shell env for the duration of this test.
        let prev_dev = std::env::var(DEV_TRUST_VITE_ENV).ok();
        let prev_extra = std::env::var(EXTRA_TRUSTED_ORIGINS_ENV).ok();
        std::env::remove_var(DEV_TRUST_VITE_ENV);
        std::env::remove_var(EXTRA_TRUSTED_ORIGINS_ENV);

        assert!(extra_trusted_origins_from_env().is_empty());

        std::env::set_var(DEV_TRUST_VITE_ENV, "1");
        let origins = extra_trusted_origins_from_env();
        assert!(origins.contains(&"http://127.0.0.1:5173".to_string()));
        assert!(origins.contains(&"http://localhost:5173".to_string()));

        // restore
        match prev_dev {
            Some(v) => std::env::set_var(DEV_TRUST_VITE_ENV, v),
            None => std::env::remove_var(DEV_TRUST_VITE_ENV),
        }
        match prev_extra {
            Some(v) => std::env::set_var(EXTRA_TRUSTED_ORIGINS_ENV, v),
            None => std::env::remove_var(EXTRA_TRUSTED_ORIGINS_ENV),
        }
    }

    // ── Safe metadata leakage tests (#614) ─────────────────────────

    #[test]
    fn safe_workspace_metadata_excludes_sensitive_fields() {
        let meta = SafeWorkspaceMetadata {
            project_name: "test-project".to_string(),
            event_store_healthy: true,
            has_workspace_binding: true,
            setup_state: "normal".to_string(),
            code_grounding_ready: true,
            decision_count: 5,
            candidate_count: 2,
            event_store_kind: Some("github".to_string()),
        };
        let serialized = serde_json::to_string(&meta).unwrap();
        for field in NEVER_EXPOSE_FIELDS {
            assert!(
                !serialized.contains(field),
                "SafeWorkspaceMetadata must not contain '{}'",
                field,
            );
        }
    }

    #[test]
    fn project_readiness_entry_excludes_sensitive_fields() {
        let entry = ProjectReadinessEntry {
            name: "test".to_string(),
            event_store_healthy: true,
            has_workspace_binding: false,
            setup_state: "code_grounding_not_ready".to_string(),
        };
        let serialized = serde_json::to_string(&entry).unwrap();
        for field in NEVER_EXPOSE_FIELDS {
            assert!(
                !serialized.contains(field),
                "ProjectReadinessEntry must not contain '{}'",
                field,
            );
        }
    }

    // ── Smuggling detection tests ──────────────────────────────────

    #[test]
    fn detect_smuggled_fields_catches_authority_context() {
        let payload = serde_json::json!({
            "origin": "https://app.bicameral-ai.com",
            "authority_context": {"actor_id": "attacker"},
        });
        let found = detect_smuggled_fields(&payload);
        assert!(found.contains(&"authority_context".to_string()));
    }

    #[test]
    fn detect_smuggled_fields_clean_payload() {
        let payload = serde_json::json!({
            "origin": "https://app.bicameral-ai.com",
            "cloud_account_id": "acct-123",
            "csrf_token": "tok",
        });
        let found = detect_smuggled_fields(&payload);
        assert!(found.is_empty());
    }

    // ── T4 renderable ledger allow-list tests (#705, #698 item 5) ───

    fn sample_ledger_entry() -> RenderableLedgerEntry {
        RenderableLedgerEntry {
            decision_id: Uuid::nil(),
            candidate_id: Uuid::nil(),
            title: "Adopt gateway authority boundary".to_string(),
            level: DecisionLevel::Architecture,
            signoff_state: SignoffState::Proposed,
            review_state: ReviewLifecycleState::Proposed,
            tracking_state: TrackingState::Tracked,
            source_freshness: SourceFreshness::Fresh,
            evidence_state: GraphEvidenceState::UnknownNotIndexed,
            promoted_at: Utc::now(),
            source_evidence_count: 2,
            binding_count: 1,
        }
    }

    /// The renderable ledger entry serializes to exactly the approved
    /// allow-list of field names — no confidential fields structurally present.
    #[test]
    fn renderable_ledger_entry_pins_allow_list() {
        let value = serde_json::to_value(sample_ledger_entry()).unwrap();
        let obj = value.as_object().unwrap();
        let mut keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
        keys.sort_unstable();
        let mut expected: Vec<&str> = RENDERABLE_LEDGER_FIELDS.to_vec();
        expected.sort_unstable();
        assert_eq!(
            keys, expected,
            "renderable ledger field set drifted from allow-list"
        );
    }

    /// Forbidden confidential field names never appear in the serialized
    /// renderable ledger projection.
    #[test]
    fn renderable_ledger_projection_excludes_forbidden_fields() {
        let projection = HostedLedgerProjection {
            summary: HostedLedgerSummary {
                decision_count: 1,
                candidate_count: 0,
                needs_signoff_count: 0,
                stale_source_count: 0,
                bound_count: 1,
                last_promoted_at: None,
            },
            decisions: vec![sample_ledger_entry()],
            events: vec![RenderableLedgerEvent {
                id: Uuid::nil(),
                event_type: EventType::AcceptCandidate,
                timestamp: Utc::now(),
            }],
            projection_only: true,
        };
        let serialized = serde_json::to_string(&projection).unwrap();
        for field in LEDGER_FORBIDDEN_FIELDS {
            assert!(
                !serialized.contains(field),
                "HostedLedgerProjection must not contain forbidden field '{field}'",
            );
        }
        for field in NEVER_EXPOSE_FIELDS {
            assert!(
                !serialized.contains(field),
                "HostedLedgerProjection must not contain sensitive field '{field}'",
            );
        }
    }

    /// The renderable event row carries only id/type/timestamp — no actor or
    /// human-readable summary (actor identity is authority material).
    #[test]
    fn renderable_ledger_event_excludes_actor_and_summary() {
        let event = RenderableLedgerEvent {
            id: Uuid::nil(),
            event_type: EventType::ApproveSignoff,
            timestamp: Utc::now(),
        };
        let value = serde_json::to_value(event).unwrap();
        let obj = value.as_object().unwrap();
        let mut keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["event_type", "id", "timestamp"]);
    }

    // ── Hosted-dashboard daemon protocol compatibility (#697 / #711) ────────

    #[test]
    fn compat_current_protocol_is_fully_available() {
        let c = evaluate_hosted_dashboard_compatibility(2, 2);
        assert_eq!(c.state, HostedDashboardCompatibilityState::Current);
        assert_eq!(c.protocol_versions_behind, 0);
        assert!(!c.upgrade_required);
        assert!(!c.forward_skew);
        assert!(c.controls_available);
        assert!(c.read_only_projections_available);
        assert!(c.repair.is_none());
    }

    #[test]
    fn compat_previous_protocol_n_minus_1_is_supported() {
        let c = evaluate_hosted_dashboard_compatibility(2, 1);
        assert_eq!(
            c.state,
            HostedDashboardCompatibilityState::SupportedPrevious
        );
        assert_eq!(c.protocol_versions_behind, 1);
        assert!(!c.upgrade_required);
        assert!(c.controls_available);
        assert!(c.read_only_projections_available);
        assert!(c.repair.is_none());
    }

    #[test]
    fn compat_two_versions_behind_requires_upgrade_with_one_action_repair() {
        let c = evaluate_hosted_dashboard_compatibility(3, 1);
        assert_eq!(c.state, HostedDashboardCompatibilityState::UpgradeRequired);
        assert_eq!(c.state.as_str(), "hosted_dashboard_upgrade_required");
        assert_eq!(c.protocol_versions_behind, 2);
        assert!(c.upgrade_required);
        assert!(!c.forward_skew);
        // A too-old daemon is gated entirely: no controls, no read-only reads.
        assert!(!c.controls_available);
        assert!(!c.read_only_projections_available);
        // The repair path is a single copy-paste command (one-action).
        let repair = c.repair.expect("upgrade-required must carry a repair path");
        assert_eq!(repair.update_command, HOSTED_DASHBOARD_UPGRADE_COMMAND);
        assert!(!repair.instruction.is_empty());
    }

    #[test]
    fn compat_newer_daemon_is_forward_skew_and_degrades_safely() {
        let c = evaluate_hosted_dashboard_compatibility(2, 3);
        assert_eq!(c.state, HostedDashboardCompatibilityState::ForwardSkew);
        assert_eq!(c.protocol_versions_behind, -1);
        assert!(!c.upgrade_required);
        assert!(c.forward_skew);
        // Forward skew never hard-fails: read-only projections still render,
        // mutable controls are hidden until the dashboard catches up.
        assert!(!c.controls_available);
        assert!(c.read_only_projections_available);
        assert!(c.repair.is_none());
    }

    #[test]
    fn compat_never_keys_on_release_semver_only_protocol_ints() {
        // Two daemons with identical protocol version are equally compatible
        // regardless of any release/semver difference the caller might track.
        let a = evaluate_hosted_dashboard_compatibility(2, 2);
        let b = evaluate_hosted_dashboard_compatibility(2, 2);
        assert_eq!(a.state, b.state);
        assert_eq!(a, b);
    }

    #[test]
    fn compat_roundtrips_through_serde() {
        for daemon in 0u32..=4 {
            let c = evaluate_hosted_dashboard_compatibility(2, daemon);
            let json = serde_json::to_string(&c).unwrap();
            let rt: HostedDashboardCompatibility = serde_json::from_str(&json).unwrap();
            assert_eq!(c, rt);
        }
    }

    #[test]
    fn telemetry_records_fire_rate_and_version_distribution_only() {
        let mut t = HostedDashboardCompatibilityTelemetry::default();
        // current, N-1, upgrade-required, forward-skew, upgrade-required again
        t.record(&evaluate_hosted_dashboard_compatibility(2, 2));
        t.record(&evaluate_hosted_dashboard_compatibility(2, 1));
        t.record(&evaluate_hosted_dashboard_compatibility(3, 1));
        t.record(&evaluate_hosted_dashboard_compatibility(2, 3));
        t.record(&evaluate_hosted_dashboard_compatibility(4, 1));

        assert_eq!(t.total_checks, 5);
        assert_eq!(t.upgrade_required_count, 2);
        assert_eq!(t.forward_skew_count, 1);
        assert!((t.upgrade_required_fire_rate() - 0.4).abs() < 1e-9);

        // Distribution keys are protocol-version integers only — never a
        // workspace, ledger, source, or path.
        assert_eq!(t.daemon_protocol_version_distribution.get(&1), Some(&3));
        assert_eq!(t.daemon_protocol_version_distribution.get(&2), Some(&1));
        assert_eq!(t.daemon_protocol_version_distribution.get(&3), Some(&1));

        // The whole telemetry payload serializes to protocol ints + counts.
        let json = serde_json::to_string(&t).unwrap();
        assert!(!json.contains("path"));
        assert!(!json.contains("workspace"));
        assert!(!json.contains("source"));
    }

    #[test]
    fn telemetry_fire_rate_is_zero_before_any_checks() {
        let t = HostedDashboardCompatibilityTelemetry::default();
        assert_eq!(t.total_checks, 0);
        assert_eq!(t.upgrade_required_fire_rate(), 0.0);
    }

    // ── Selected hosted project readiness resolver (#719) ──────────────────

    fn local_projects_fixture() -> Vec<ProjectReadinessEntry> {
        vec![
            ProjectReadinessEntry {
                name: "my-service".into(),
                event_store_healthy: true,
                has_workspace_binding: true,
                setup_state: "normal".into(),
            },
            ProjectReadinessEntry {
                name: "half-bound".into(),
                event_store_healthy: false,
                has_workspace_binding: true,
                setup_state: "repair_required".into(),
            },
            ProjectReadinessEntry {
                name: "unbound-local".into(),
                event_store_healthy: true,
                has_workspace_binding: false,
                setup_state: "code_grounding_not_ready".into(),
            },
        ]
    }

    fn selected(id: &str) -> SelectedProjectRef {
        SelectedProjectRef {
            project_id: id.into(),
            team_id: Some("team-acme".into()),
            display_name: Some("Selected".into()),
        }
    }

    // AC#1: the selected Cloud project id flows through to the scoped projection.
    #[test]
    fn selected_project_id_is_echoed_and_scopes_projection() {
        let resp = resolve_selected_project_readiness(
            HostedBridgePairingState::Paired,
            &selected("my-service"),
            &local_projects_fixture(),
            "2026-06-25T12:00:00Z",
        );
        assert_eq!(resp.selected_project.project_id, "my-service");
        assert_eq!(resp.binding_readiness, DaemonBindingReadiness::Ready);
        assert!(resp.controls_available);
        // Scoped to the selection only — the single matching project, not the list.
        let project = resp.project.expect("scoped project present");
        assert_eq!(project.name, "my-service");
    }

    // AC#2: a missing local binding renders the "needs binding" readiness state.
    #[test]
    fn selected_project_with_no_local_match_is_unbound() {
        let resp = resolve_selected_project_readiness(
            HostedBridgePairingState::Paired,
            &selected("not-registered-locally"),
            &local_projects_fixture(),
            "2026-06-25T12:00:00Z",
        );
        assert_eq!(resp.binding_readiness, DaemonBindingReadiness::Unbound);
        assert!(!resp.controls_available);
        // No local project is fabricated for an unmatched selection.
        assert!(resp.project.is_none());
    }

    // AC#2: a locally-registered but unbound project also renders `unbound`.
    #[test]
    fn selected_project_locally_registered_but_unbound_is_unbound() {
        let resp = resolve_selected_project_readiness(
            HostedBridgePairingState::Paired,
            &selected("unbound-local"),
            &local_projects_fixture(),
            "2026-06-25T12:00:00Z",
        );
        assert_eq!(resp.binding_readiness, DaemonBindingReadiness::Unbound);
        assert!(!resp.controls_available);
        let project = resp.project.expect("scoped project present");
        assert!(!project.has_workspace_binding);
    }

    // Bound but unhealthy/not-normal → degraded (reads allowed, controls off).
    #[test]
    fn selected_project_bound_but_unhealthy_is_degraded() {
        let resp = resolve_selected_project_readiness(
            HostedBridgePairingState::Paired,
            &selected("half-bound"),
            &local_projects_fixture(),
            "2026-06-25T12:00:00Z",
        );
        assert_eq!(resp.binding_readiness, DaemonBindingReadiness::Degraded);
        assert!(!resp.controls_available);
    }

    // AC#3: the unscoped local list is never the membership source. Selecting a
    // project the daemon does not know does not deny "membership" (no error /
    // 403 semantics) and does not leak the other local projects.
    #[test]
    fn selected_project_does_not_use_local_list_as_membership() {
        let resp = resolve_selected_project_readiness(
            HostedBridgePairingState::Paired,
            &selected("cloud-only-project"),
            &local_projects_fixture(),
            "2026-06-25T12:00:00Z",
        );
        // Not treated as a non-member: resolves to a readiness state, not a
        // rejection, and carries no other project's data.
        assert_eq!(resp.binding_readiness, DaemonBindingReadiness::Unbound);
        assert!(resp.project.is_none());
        let json = serde_json::to_string(&resp).unwrap();
        assert!(!json.contains("my-service"));
        assert!(!json.contains("half-bound"));
        assert!(!json.contains("unbound-local"));
    }

    // Unpaired sessions cannot resolve local readiness.
    #[test]
    fn selected_project_unpaired_reports_unpaired() {
        let resp = resolve_selected_project_readiness(
            HostedBridgePairingState::Unpaired,
            &selected("my-service"),
            &local_projects_fixture(),
            "2026-06-25T12:00:00Z",
        );
        assert_eq!(resp.binding_readiness, DaemonBindingReadiness::Unpaired);
        assert!(!resp.controls_available);
        assert!(resp.project.is_none());
    }

    // Redaction: the scoped response never carries sensitive local fields.
    #[test]
    fn selected_project_response_has_no_forbidden_fields() {
        let resp = resolve_selected_project_readiness(
            HostedBridgePairingState::Paired,
            &selected("my-service"),
            &local_projects_fixture(),
            "2026-06-25T12:00:00Z",
        );
        let json = serde_json::to_string(&resp).unwrap();
        for field in NEVER_EXPOSE_FIELDS {
            assert!(
                !json.contains(field),
                "scoped selected-project response leaked forbidden field: {field}"
            );
        }
    }

    // ── Workspace binding readiness projection (bicameral-bot#733) ─────────

    fn display() -> ProjectSafeDisplayMetadata {
        ProjectSafeDisplayMetadata {
            display_name: "Ada Service".into(),
            project_slug: Some("ada-service".into()),
            candidate_label: None,
        }
    }

    // AC#1: the three durable workspace states each project a distinct
    // hosted-safe readiness view.
    #[test]
    fn readiness_distinguishes_the_three_workspace_states() {
        let unbound = project_workspace_binding_readiness(
            "ada",
            &display(),
            WorkspaceBindingState::LocalWorkspaceUnbound,
        );
        let bound = project_workspace_binding_readiness(
            "ada",
            &display(),
            WorkspaceBindingState::LocalWorkspaceBound,
        );
        let repair = project_workspace_binding_readiness(
            "ada",
            &display(),
            WorkspaceBindingState::LocalWorkspaceRepairRequired,
        );

        assert_eq!(
            unbound.binding_state,
            WorkspaceBindingState::LocalWorkspaceUnbound
        );
        assert_eq!(
            bound.binding_state,
            WorkspaceBindingState::LocalWorkspaceBound
        );
        assert_eq!(
            repair.binding_state,
            WorkspaceBindingState::LocalWorkspaceRepairRequired
        );
        // Distinct readiness across the three states.
        assert!(!unbound.code_grounding_ready);
        assert!(bound.code_grounding_ready);
        assert!(!repair.code_grounding_ready);
    }

    // AC#5: a missing workspace binding is a code-grounding readiness gap, not a
    // project authority failure — the projection is a normal readiness view
    // (never an error) with `code_grounding_ready = false` and a bind action.
    #[test]
    fn missing_binding_is_code_grounding_readiness_not_authority_failure() {
        let r = project_workspace_binding_readiness(
            "ada",
            &display(),
            WorkspaceBindingState::LocalWorkspaceUnbound,
        );
        assert!(!r.code_grounding_ready);
        assert!(!r.caveats.is_empty());
        let action = r.next_action.expect("unbound offers a bind next action");
        assert_eq!(
            action.kind,
            WorkspaceReadinessActionKind::BindLocalWorkspace
        );
    }

    // AC#4: unbound projects receive a next action guiding the user to the
    // MCP/CLI/local bind flow.
    #[test]
    fn unbound_next_action_guides_to_mcp_cli_local_bind() {
        let r = project_workspace_binding_readiness(
            "ada",
            &display(),
            WorkspaceBindingState::LocalWorkspaceUnbound,
        );
        let action = r.next_action.unwrap();
        assert!(action
            .surfaces
            .contains(&WorkspaceBindingSourceSurface::Mcp));
        assert!(action
            .surfaces
            .contains(&WorkspaceBindingSourceSurface::Cli));
        assert!(!action.command.is_empty());
        assert!(!action.instruction.is_empty());
    }

    // AC#4: bound projects report readiness (ready, no action) without exposing
    // any local path.
    #[test]
    fn bound_reports_ready_without_next_action_or_path() {
        let r = project_workspace_binding_readiness(
            "ada",
            &display(),
            WorkspaceBindingState::LocalWorkspaceBound,
        );
        assert!(r.code_grounding_ready);
        assert!(r.caveats.is_empty());
        assert!(r.next_action.is_none());
        let json = serde_json::to_string(&r).unwrap();
        for field in NEVER_EXPOSE_FIELDS {
            assert!(!json.contains(field), "bound readiness leaked '{field}'");
        }
    }

    // AC#4: repair-required projects return typed repair guidance without
    // exposing private path details.
    #[test]
    fn repair_required_returns_typed_repair_guidance_without_path() {
        let r = project_workspace_binding_readiness(
            "ada",
            &display(),
            WorkspaceBindingState::LocalWorkspaceRepairRequired,
        );
        assert!(!r.code_grounding_ready);
        let action = r.next_action.as_ref().expect("repair offers a next action");
        assert_eq!(
            action.kind,
            WorkspaceReadinessActionKind::RepairLocalWorkspace
        );
        let json = serde_json::to_string(&r).unwrap();
        for field in NEVER_EXPOSE_FIELDS {
            assert!(!json.contains(field), "repair readiness leaked '{field}'");
        }
        // No absolute-path-looking content in the guidance.
        assert!(!json.contains("/home/"));
        assert!(!json.contains("/Users/"));
    }

    // ProjectReadinessEntry derives the daemon-owned state and hosted-safe view.
    #[test]
    fn project_entry_derives_workspace_state_and_readiness() {
        let repair = ProjectReadinessEntry {
            name: "half-bound".into(),
            event_store_healthy: false,
            has_workspace_binding: true,
            setup_state: "repair_required".into(),
        };
        assert_eq!(
            repair.workspace_binding_state(),
            WorkspaceBindingState::LocalWorkspaceRepairRequired
        );
        assert_eq!(
            repair
                .workspace_binding_readiness()
                .next_action
                .unwrap()
                .kind,
            WorkspaceReadinessActionKind::RepairLocalWorkspace
        );

        let unbound = ProjectReadinessEntry {
            name: "unbound-local".into(),
            event_store_healthy: true,
            has_workspace_binding: false,
            setup_state: "code_grounding_not_ready".into(),
        };
        assert_eq!(
            unbound.workspace_binding_state(),
            WorkspaceBindingState::LocalWorkspaceUnbound
        );
        assert!(!unbound.workspace_binding_readiness().code_grounding_ready);
    }

    // AC#3 / capability boundary: user-level `workspace_binding_available` stays
    // false, and pairing alone cannot create a binding — a paired session over
    // an unbound project still resolves to `unbound` with a bind next action,
    // never `bound`.
    #[test]
    fn pairing_alone_cannot_create_workspace_binding() {
        // User-level capability is structurally false at pairing.
        let cap = HostedBridgeCapability {
            bridge_supported: true,
            daemon_build_version: DAEMON_BUILD_VERSION.into(),
            hosted_bridge_protocol_version: HOSTED_BRIDGE_PROTOCOL_VERSION,
            bridge_protocol_version: "v1".into(),
            daemon_protocol_version: HOSTED_DASHBOARD_PROTOCOL_VERSION,
            project_list_available: true,
            workspace_binding_available: false,
            note: None,
        };
        assert!(!cap.workspace_binding_available);

        // A freshly-paired session selecting an unbound project does not gain a
        // binding: readiness stays unbound and the projected state is unbound.
        let resp = resolve_selected_project_readiness(
            HostedBridgePairingState::Paired,
            &selected("unbound-local"),
            &local_projects_fixture(),
            "2026-06-25T12:00:00Z",
        );
        assert_eq!(resp.binding_readiness, DaemonBindingReadiness::Unbound);
        let ws = resp
            .workspace_readiness
            .expect("paired session carries workspace readiness");
        assert_eq!(
            ws.binding_state,
            WorkspaceBindingState::LocalWorkspaceUnbound
        );
        assert!(!ws.code_grounding_ready);
        assert_eq!(
            ws.next_action.unwrap().kind,
            WorkspaceReadinessActionKind::BindLocalWorkspace
        );
    }

    // The selected-project response carries workspace readiness scoped to the
    // selection, and an unpaired session carries none.
    #[test]
    fn selected_project_response_includes_scoped_workspace_readiness() {
        let ready = resolve_selected_project_readiness(
            HostedBridgePairingState::Paired,
            &selected("my-service"),
            &local_projects_fixture(),
            "2026-06-25T12:00:00Z",
        );
        let ws = ready.workspace_readiness.expect("readiness present");
        assert_eq!(ws.binding_state, WorkspaceBindingState::LocalWorkspaceBound);
        assert!(ws.code_grounding_ready);

        let unpaired = resolve_selected_project_readiness(
            HostedBridgePairingState::Unpaired,
            &selected("my-service"),
            &local_projects_fixture(),
            "2026-06-25T12:00:00Z",
        );
        assert!(unpaired.workspace_readiness.is_none());
    }

    #[test]
    fn capability_advertises_distinct_build_and_protocol_versions() {
        let value = serde_json::to_value(crate::fixtures::hosted_bridge_capability_discovery())
            .expect("capability serializes");

        assert_eq!(
            value["daemon_build_version"],
            serde_json::json!(env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(
            value["hosted_bridge_protocol_version"],
            serde_json::json!(HOSTED_BRIDGE_PROTOCOL_VERSION)
        );
    }

    // ── Pending pairing / local-confirmation contract (#790) ────────

    #[test]
    fn protocol_version_advanced_to_pending_pairing_floor() {
        assert_eq!(HOSTED_BRIDGE_PROTOCOL_VERSION, 4);
        assert_eq!(PENDING_PAIRING_MIN_PROTOCOL_VERSION, 4);
    }

    #[test]
    fn pairing_code_normalization_is_strict_and_format_tolerant() {
        assert_eq!(normalize_pairing_code("7k2m-pq8r"), Some("7K2MPQ8R".into()));
        assert_eq!(
            normalize_pairing_code(" 7K2M PQ8R "),
            Some("7K2MPQ8R".into())
        );
        assert_eq!(normalize_pairing_code("O000-0000"), None);
        assert_eq!(normalize_pairing_code("TOO-SHORT"), None);
    }

    #[test]
    fn current_client_is_compatible() {
        assert!(matches!(
            evaluate_pairing_protocol_compatibility(Some(HOSTED_BRIDGE_PROTOCOL_VERSION)),
            PairingProtocolCompatibility::Compatible
        ));
    }

    #[test]
    fn old_and_absent_clients_require_update() {
        for declared in [None, Some(0), Some(1), Some(2), Some(3)] {
            match evaluate_pairing_protocol_compatibility(declared) {
                PairingProtocolCompatibility::UpdateRequired {
                    client_protocol_version,
                } => {
                    assert_eq!(client_protocol_version, declared.unwrap_or(0));
                }
                PairingProtocolCompatibility::Compatible => {
                    panic!("declared {declared:?} must require update")
                }
            }
        }
    }

    #[test]
    fn update_required_response_is_typed_and_tokenless() {
        let resp = pairing_update_required_response(2);
        assert_eq!(resp.status, PendingPairingOutcome::Incompatible);
        assert_eq!(resp.error, HostedBridgeErrorKind::UpdateRequired);
        assert_eq!(
            resp.required_protocol_version,
            PENDING_PAIRING_MIN_PROTOCOL_VERSION
        );
        assert_eq!(resp.client_protocol_version, 2);
        assert_eq!(resp.daemon_protocol_version, HOSTED_BRIDGE_PROTOCOL_VERSION);
        let json = serde_json::to_string(&resp).unwrap();
        assert!(!json.contains("session_token"));
    }

    #[test]
    fn local_and_missing_origins_are_local_confirmation() {
        assert!(is_local_confirmation_origin(None));
        assert!(is_local_confirmation_origin(Some("")));
        assert!(is_local_confirmation_origin(Some("http://127.0.0.1:8765")));
        assert!(is_local_confirmation_origin(Some("http://localhost:3000")));
        assert!(is_local_confirmation_origin(Some("http://[::1]:9000")));
    }

    #[test]
    fn hosted_and_cross_origins_are_not_local_confirmation() {
        assert!(!is_local_confirmation_origin(Some(
            "https://app.bicameral-ai.com"
        )));
        assert!(!is_local_confirmation_origin(Some(
            "https://evil.attacker.com"
        )));
        assert!(!is_local_confirmation_origin(Some(
            "http://localhost.evil.com"
        )));
    }

    #[test]
    fn pending_outcome_terminality() {
        assert!(PendingPairingOutcome::Denied.is_terminal());
        assert!(PendingPairingOutcome::Expired.is_terminal());
        assert!(PendingPairingOutcome::Incompatible.is_terminal());
        assert!(!PendingPairingOutcome::Pending.is_terminal());
        assert!(!PendingPairingOutcome::Approved.is_terminal());
    }

    #[test]
    fn capability_summary_lists_allowed_and_forbidden() {
        let summary = pairing_capability_summary();
        assert!(!summary.allowed.is_empty());
        assert!(!summary.forbidden.is_empty());
        // Forbidden capabilities must name the authority boundaries.
        let forbidden = summary.forbidden.join(" ").to_lowercase();
        assert!(forbidden.contains("secret") || forbidden.contains("token"));
        assert!(forbidden.contains("filesystem") || forbidden.contains("workspace"));
    }
}
