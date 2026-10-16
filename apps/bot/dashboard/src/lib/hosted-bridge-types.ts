/** Hosted bridge pairing and session types for the dashboard UI.
 *
 * Mirrors the Rust types in `crates/bicameral-api/src/hosted_bridge.rs`.
 * The hosted bridge is user/operator-level pairing between a remote
 * browser origin and the local Bicameral daemon. It is not project
 * membership, workspace binding, source authority, signoff, or
 * event-store authority.
 */

import type {
  SignoffState,
  ReviewLifecycleState,
  TrackingState,
  GraphEvidenceState,
} from "./types";

// ── Bridge pairing state ───────────────────────────────────────────

export type HostedBridgePairingState =
  | "hosted_bridge_unpaired"
  | "hosted_bridge_paired"
  | "hosted_bridge_revoked";

// ── Error kinds ────────────────────────────────────────────────────

export type HostedBridgeErrorKind =
  | "not_paired"
  | "pairing_revoked"
  | "pairing_expired"
  | "origin_mismatch"
  | "origin_not_trusted"
  | "wrong_daemon"
  | "project_not_found"
  | "account_mismatch"
  | "unsupported_command"
  | "authority_smuggling_rejected"
  | "daemon_unavailable"
  | "unsupported_daemon"
  | "missing_cloud_session"
  | "missing_team_id"
  | "missing_cloud_access_token";

// ── Pending pairing / local-confirmation contract (#790) ───────────
//
// Mirrors the Rust `PendingPairingOutcome`, `PairingCapabilitySummary`,
// `LocalConfirmationAffordance`, `HostedBridgePairingPendingResponse`,
// `PendingPairingLocalView`, `PairingDecisionAction`,
// `LocalPairingDecisionRequest`, `LocalPairingDecisionResponse`,
// `HostedBridgePairingStatusResponse`, and
// `HostedBridgePairingUpdateRequiredResponse` in
// `crates/bicameral-api/src/hosted_bridge.rs`.
//
// A valid hosted `POST /pair` returns `202` with a pending outcome, an opaque
// request handle, an opaque poll proof, an expiry, and capability/version info
// — never a session or token. A token is only ever released in the approved
// status-poll response body.

/** Transient pending-pairing request/UI outcome — not canonical state. */
export type PendingPairingOutcome =
  | "pending"
  | "approved"
  | "denied"
  | "expired"
  | "incompatible";

/** Allowed vs forbidden capability advertisement for the confirmation UI. */
export type PairingCapabilitySummary = {
  bridge_supported: boolean;
  workspace_binding_available: boolean;
  protocol_version: number;
  allowed: string[];
  forbidden: string[];
};

/** Locally served confirmation affordance handed to the initiating browser. */
export type LocalConfirmationAffordance = {
  local_confirmation_url?: string;
  terminal_command: string;
  message: string;
  requesting_origin: string;
  cloud_account_id: string;
  capability_summary: PairingCapabilitySummary;
};

/** `202` pending response body — never carries a session or token. */
export type HostedBridgePairingPendingResponse = {
  status: "pending";
  request_handle: string;
  poll_proof: string;
  pairing_code: string;
  expires_at: string;
  protocol_version: number;
  capabilities: PairingCapabilitySummary;
  confirmation: LocalConfirmationAffordance;
};

/** Local (same-origin) view of a pending request; never carries a token. */
export type PendingPairingLocalView = {
  status: PendingPairingOutcome;
  request_handle: string;
  requesting_origin: string;
  cloud_account_id: string;
  capability_summary: PairingCapabilitySummary;
  expires_at: string;
  local_confirmation_nonce: string;
};

export type PairingDecisionAction = "approve" | "deny";

/** Same-origin local decision request. */
export type LocalPairingDecisionRequest = {
  request_handle: string;
  action: PairingDecisionAction;
  local_confirmation_nonce: string;
};

/** Local decision response — typed outcome, never a token. */
export type LocalPairingDecisionResponse = {
  status: PendingPairingOutcome;
  request_handle: string;
};

/**
 * Approved-only status-poll response. The `session_token` is present ONLY when
 * `status === "approved"` and the linked session is live; every other outcome
 * omits it. This is the single path on which a token may cross to the browser.
 */
export type HostedBridgePairingStatusResponse = {
  status: PendingPairingOutcome;
  request_handle: string;
  session_token?: string;
  expires_at?: string;
};

/** Typed update-required outcome for incompatible consumers/daemons. */
export type HostedBridgePairingUpdateRequiredResponse = {
  status: "incompatible";
  error: "update_required";
  required_protocol_version: number;
  client_protocol_version: number;
  daemon_protocol_version: number;
};

// ── Pairing pending reason ─────────────────────────────────────────

export type PairingPendingReason =
  | "awaiting_confirmation"
  | "connecting"
  | "handshake";

// ── Composite UI state ─────────────────────────────────────────────

export type HostedBridgeUIState =
  | { kind: "unpaired" }
  | { kind: "pairing_pending"; reason: PairingPendingReason; message?: string }
  | {
      kind: "confirmation";
      origin: string;
      cloudAccountId: string;
      requestHandle?: string;
      expiresAt?: string;
    }
  | {
      kind: "paired";
      origin: string;
      cloudAccountId: string;
      pairedAt: string;
      expiresAt: string;
    }
  | { kind: "expired"; reason: string; revokedAt: string }
  | { kind: "revoked"; reason: string; revokedAt: string }
  | { kind: "error"; errorKind: HostedBridgeErrorKind; message: string; retryAfterRepair: boolean };

// ── Session info (shown in session shell) ──────────────────────────

export type HostedBridgeSessionInfo = {
  pairingState: HostedBridgePairingState;
  origin: string;
  cloudAccountId: string;
  pairedAt: string;
  expiresAt: string;
};

// ── Confirmation screen actions ────────────────────────────────────

export type ConfirmationAction = "connect" | "cancel";

// ── Allowed / not-allowed action lists for confirmation ────────────

export const BRIDGE_ALLOWED_ACTIONS: readonly string[] = [
  "List locally registered Bicameral projects",
  "Show setup and readiness status",
  "Request project registration or repair",
  "Request governed actions after local policy checks",
];

export const BRIDGE_NOT_ALLOWED_ACTIONS: readonly string[] = [
  "Provider secrets",
  "Filesystem access",
  "Workspace or code access",
  "Decision approval without policy checks",
];

// ── Hosted dashboard data adapter types (#616) ─────────────────────

export type DashboardDataMode = "hosted";

export type SafeWorkspaceMetadata = {
  project_name: string;
  event_store_healthy: boolean;
  has_workspace_binding: boolean;
  setup_state: string;
  code_grounding_ready: boolean;
  decision_count: number;
  candidate_count: number;
  event_store_kind?: string;
};

export type HostedLedgerSummary = {
  decision_count: number;
  candidate_count: number;
  needs_signoff_count: number;
  stale_source_count: number;
  bound_count: number;
  last_promoted_at?: string;
};

export type HostedDashboardAdapterResponse = {
  mode: DashboardDataMode;
  workspace?: SafeWorkspaceMetadata;
  ledger_summary?: HostedLedgerSummary;
  projects: Array<{
    name: string;
    event_store_healthy: boolean;
    has_workspace_binding: boolean;
    setup_state: string;
  }>;
  status_message?: string;
};

// ── Renderable ledger projection (#705, T4) ────────────────────────
//
// Mirrors the Rust `HostedLedgerProjection` / `RenderableLedgerEntry` /
// `RenderableLedgerEvent` in `crates/bicameral-api/src/hosted_bridge.rs`.
//
// This is the positive allow-list approved in #698 item 5: the ONLY fields
// safe to render on a network origin. Confidential fields the local
// `ProjectionState` carries — decision bodies, source-evidence excerpts, code
// paths, binding explanations, actors (`promoted_by`), `source_id`,
// provenance, provider secrets, raw event payloads — must never appear here.
// Hosted/cloud/browser code is a renderer only; it computes no authority
// (ADR-0020 / ADR-0025). Titles are passed through the daemon-side sensitive-
// pattern redactor before crossing the boundary, so a rendered `title` may
// contain `[redacted:secret|phi|pan]` spans.

export type DecisionLevel = "product" | "architecture" | "implementation";

export type LedgerSourceFreshness = "fresh" | "stale" | "offline" | "unknown";

/** A single render-safe ledger row for a network-origin hosted dashboard. */
export type RenderableLedgerEntry = {
  decision_id: string;
  candidate_id: string;
  /** Redactor-passed title; may contain `[redacted:<class>]` spans. */
  title: string;
  level: DecisionLevel;
  signoff_state: SignoffState;
  review_state: ReviewLifecycleState;
  tracking_state: TrackingState;
  source_freshness: LedgerSourceFreshness;
  evidence_state: GraphEvidenceState;
  promoted_at: string;
  source_evidence_count: number;
  binding_count: number;
};

/** A render-safe ledger event: opaque id, lifecycle type, timestamp only. */
export type RenderableLedgerEvent = {
  id: string;
  event_type: string;
  timestamp: string;
};

export type HostedLedgerProjection = {
  summary: HostedLedgerSummary;
  decisions: RenderableLedgerEntry[];
  events: RenderableLedgerEvent[];
  projection_only: boolean;
};

/**
 * The exact renderable field set for a ledger entry. Kept in lockstep with the
 * Rust `RENDERABLE_LEDGER_FIELDS` allow-list; conformance tests assert entries
 * carry only these keys.
 */
export const RENDERABLE_LEDGER_ENTRY_FIELDS: readonly string[] = [
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

/**
 * Confidential field names that must never appear in the renderable ledger
 * projection. Mirrors the Rust `LEDGER_FORBIDDEN_FIELDS` list; conformance
 * tests assert none of these surface in any fixture or entry.
 */
export const LEDGER_FORBIDDEN_FIELDS: readonly string[] = [
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

// ── Workspace binding readiness projection (bicameral-bot#733 / #734) ──
//
// Mirrors the Rust `WorkspaceBindingState`, `WorkspaceBindingSourceSurface`,
// `ProjectSafeDisplayMetadata`, `WorkspaceReadinessActionKind`,
// `WorkspaceReadinessNextAction`, `HostedWorkspaceBindingReadiness`,
// `DaemonBindingReadiness`, `SelectedProjectRef`, and
// `SelectedProjectSessionResponse` in `crates/bicameral-api/src/*`.
//
// This is the daemon-owned readiness projection the hosted dashboard renders
// for a selected project. The hosted surface is a RENDERER ONLY: it never
// binds a workspace, never computes authority, and the projection deliberately
// carries no local absolute path, provider token, source body, graph index, or
// event-store body. Binding is performed exclusively on a LOCAL surface
// (MCP / CLI / local dashboard); the hosted bridge is never a bind surface.

/** Daemon-owned durable workspace binding state (mirrors Rust `WorkspaceBindingState`). */
export type WorkspaceBindingState =
  | "local_workspace_unbound"
  | "local_workspace_bound"
  | "local_workspace_repair_required";

/**
 * Local surface that can perform a workspace bind/repair (mirrors Rust
 * `WorkspaceBindingSourceSurface`). The hosted bridge is intentionally NOT a
 * member — it never binds.
 */
export type WorkspaceBindingSourceSurface = "mcp" | "cli" | "dashboard";

/** Redacted, hosted-safe project display metadata (mirrors `ProjectSafeDisplayMetadata`). */
export type ProjectSafeDisplayMetadata = {
  display_name: string;
  project_slug?: string;
  candidate_label?: string;
};

/** Typed next-action affordance kind (mirrors `WorkspaceReadinessActionKind`). */
export type WorkspaceReadinessActionKind =
  | "bind_local_workspace"
  | "repair_local_workspace";

/**
 * Hosted-safe next-action affordance for workspace binding readiness (mirrors
 * `WorkspaceReadinessNextAction`). Renderer-only: it tells the dashboard which
 * LOCAL flow to guide the user to. `command` is a generic, non-path hint (e.g.
 * `bicameral workspace bind`); it is never an absolute path or secret.
 */
export type WorkspaceReadinessNextAction = {
  kind: WorkspaceReadinessActionKind;
  surfaces: WorkspaceBindingSourceSurface[];
  command: string;
  instruction: string;
};

/**
 * Hosted-safe workspace binding readiness projection for a single project
 * (mirrors `HostedWorkspaceBindingReadiness`). A missing/broken binding is a
 * code-grounding readiness gap (`code_grounding_ready = false`), NOT a project
 * authority failure. Carries no local path, provider token, source body, graph
 * index, or event-store body.
 */
export type HostedWorkspaceBindingReadiness = {
  project_id: string;
  display: ProjectSafeDisplayMetadata;
  binding_state: WorkspaceBindingState;
  code_grounding_ready: boolean;
  caveats?: string[];
  next_action?: WorkspaceReadinessNextAction;
};

/**
 * Coarse daemon-reported binding readiness for a selected project (mirrors Rust
 * `DaemonBindingReadiness`). It is NOT a compliance or signoff state.
 */
export type DaemonBindingReadiness =
  | "unpaired"
  | "upgrade_required"
  | "unbound"
  | "degraded"
  | "ready"
  | "unknown";

/** Safe, opaque reference to the selected hosted project (mirrors `SelectedProjectRef`). */
export type SelectedProjectRef = {
  project_id: string;
  team_id?: string;
  display_name?: string;
};

/**
 * Project-scoped session response for a selected hosted project (mirrors
 * `SelectedProjectSessionResponse`). `workspace_readiness` is the daemon-owned
 * projection this feature renders; it is `undefined` only when the session is
 * not paired and local readiness cannot be resolved.
 */
export type SelectedProjectSessionResponse = {
  pairing_state: HostedBridgePairingState;
  selected_project: SelectedProjectRef;
  binding_readiness: DaemonBindingReadiness;
  project?: {
    name: string;
    event_store_healthy: boolean;
    has_workspace_binding: boolean;
    setup_state: string;
  };
  controls_available: boolean;
  workspace_readiness?: HostedWorkspaceBindingReadiness;
  resolved_at: string;
  status_message?: string;
};

/**
 * Confidential fields that must NEVER appear in a hosted workspace-readiness
 * projection. Conformance tests assert no fixture or response carries these,
 * and that no value looks like a local absolute path.
 */
export const WORKSPACE_READINESS_FORBIDDEN_FIELDS: readonly string[] = [
  "candidate_path",
  "workspace_path",
  "workspace_root",
  "local_path",
  "file_path",
  "absolute_path",
  "provider_token",
  "access_token",
  "refresh_token",
  "client_secret",
  "source_body",
  "graph_index",
  "event_store_body",
];

// ── Daemon protocol compatibility window (#697 / #711) ─────────────
//
// Mirrors the Rust `HostedDashboardCompatibility` / `...State` /
// `HostedDashboardUpgradeRepair` in `crates/bicameral-api/src/hosted_bridge.rs`.
//
// Compatibility between the always-current cloud SPA and the (possibly older)
// local daemon is keyed strictly on the integer `daemon_protocol_version` —
// never on release version / semver. The alpha window is "current + N-1";
// daemons 2+ protocol versions behind are gated with an upgrade-required state
// that carries a one-action repair path, and daemons newer than the SPA expects
// degrade to a benign forward-skew ("dashboard catching up") state.

/** The Bot-owned hosted-bridge wire-contract version this SPA expects. */
export const HOSTED_BRIDGE_PROTOCOL_VERSION = 4;

/**
 * Minimum hosted-bridge protocol version that supports the local-confirmation
 * pending-pairing flow (#790). A consumer below this floor is incompatible and
 * receives a typed `update_required` outcome from `POST /pair` instead of a
 * paired session. Mirrors the Rust `PENDING_PAIRING_MIN_PROTOCOL_VERSION`.
 */
export const PENDING_PAIRING_MIN_PROTOCOL_VERSION = 4;

/** Legacy name retained for existing dashboard callers. */
export const HOSTED_DASHBOARD_PROTOCOL_VERSION = HOSTED_BRIDGE_PROTOCOL_VERSION;

/** Alpha support-window depth: current plus N-1 (one previous protocol). */
export const HOSTED_DASHBOARD_SUPPORT_WINDOW = 1;

export type HostedDashboardCompatibilityState =
  | "current"
  | "supported_previous"
  | "hosted_dashboard_upgrade_required"
  | "hosted_dashboard_forward_skew";

/** One-action repair path shown when the daemon is too old. */
export type HostedDashboardUpgradeRepair = {
  auto_update_available: boolean;
  update_command: string;
  instruction: string;
};

export type HostedDashboardCompatibility = {
  cloud_expected_protocol_version: number;
  daemon_protocol_version: number;
  state: HostedDashboardCompatibilityState;
  protocol_versions_behind: number;
  upgrade_required: boolean;
  forward_skew: boolean;
  controls_available: boolean;
  read_only_projections_available: boolean;
  repair?: HostedDashboardUpgradeRepair;
  note?: string;
};

/**
 * Evaluate hosted-dashboard compatibility client-side from protocol-version
 * integers. Mirrors the Rust `evaluate_hosted_dashboard_compatibility` so the
 * SPA can render an accurate compatibility state even when it cannot reach a
 * compatible daemon (e.g. forward skew / catching-up). This computes NO
 * authority — it only selects which UI state to render (renderer-only).
 */
export function evaluateHostedDashboardCompatibility(
  cloudExpectedProtocolVersion: number,
  daemonProtocolVersion: number,
): HostedDashboardCompatibility {
  const behind = cloudExpectedProtocolVersion - daemonProtocolVersion;

  if (behind < 0) {
    return {
      cloud_expected_protocol_version: cloudExpectedProtocolVersion,
      daemon_protocol_version: daemonProtocolVersion,
      state: "hosted_dashboard_forward_skew",
      protocol_versions_behind: behind,
      upgrade_required: false,
      forward_skew: true,
      controls_available: false,
      read_only_projections_available: true,
      note:
        "The local daemon is newer than this dashboard expects. The dashboard " +
        "is catching up; some controls are hidden until it updates.",
    };
  }
  if (behind === 0) {
    return {
      cloud_expected_protocol_version: cloudExpectedProtocolVersion,
      daemon_protocol_version: daemonProtocolVersion,
      state: "current",
      protocol_versions_behind: 0,
      upgrade_required: false,
      forward_skew: false,
      controls_available: true,
      read_only_projections_available: true,
    };
  }
  if (behind <= HOSTED_DASHBOARD_SUPPORT_WINDOW) {
    return {
      cloud_expected_protocol_version: cloudExpectedProtocolVersion,
      daemon_protocol_version: daemonProtocolVersion,
      state: "supported_previous",
      protocol_versions_behind: behind,
      upgrade_required: false,
      forward_skew: false,
      controls_available: true,
      read_only_projections_available: true,
      note:
        "The local daemon is one protocol version behind but still supported. " +
        "Update when convenient to stay current.",
    };
  }
  return {
    cloud_expected_protocol_version: cloudExpectedProtocolVersion,
    daemon_protocol_version: daemonProtocolVersion,
    state: "hosted_dashboard_upgrade_required",
    protocol_versions_behind: behind,
    upgrade_required: true,
    forward_skew: false,
    controls_available: false,
    read_only_projections_available: false,
    repair: {
      auto_update_available: false,
      update_command: "bicameral daemon update",
      instruction:
        "Run this command to update your local Bicameral daemon, then reconnect.",
    },
    note:
      "The local daemon is too far behind to use with the hosted dashboard. " +
      "Update the daemon to continue.",
  };
}
