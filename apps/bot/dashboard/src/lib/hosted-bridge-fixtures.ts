/** Fixture data for hosted bridge pairing UI tests and development.
 *
 * Follows the same fixture pattern as `provider-setup-fixtures.ts`.
 */

import type {
  HostedBridgeUIState,
  HostedBridgeSessionInfo,
  HostedDashboardAdapterResponse,
  HostedLedgerProjection,
} from "./hosted-bridge-types";

// ── UI state fixtures ──────────────────────────────────────────────

export const HOSTED_BRIDGE_FIXTURES: Record<string, HostedBridgeUIState> = {
  unpaired: {
    kind: "unpaired",
  },

  pairing_pending_awaiting: {
    kind: "pairing_pending",
    reason: "awaiting_confirmation",
    message: "Waiting for local Bicameral daemon to confirm pairing.",
  },

  pairing_pending_connecting: {
    kind: "pairing_pending",
    reason: "connecting",
    message: "Connecting to local daemon...",
  },

  pairing_pending_handshake: {
    kind: "pairing_pending",
    reason: "handshake",
    message: "Completing pairing handshake...",
  },

  confirmation: {
    kind: "confirmation",
    origin: "https://app.bicameral-ai.com",
    cloudAccountId: "acct_test_12345",
  },

  paired: {
    kind: "paired",
    origin: "https://app.bicameral-ai.com",
    cloudAccountId: "acct_test_12345",
    pairedAt: "2026-06-25T10:00:00Z",
    expiresAt: "2026-06-26T10:00:00Z",
  },

  expired: {
    kind: "expired",
    reason: "Pairing session has expired.",
    revokedAt: "2026-06-26T10:00:00Z",
  },

  revoked: {
    kind: "revoked",
    reason: "Pairing was revoked by the operator.",
    revokedAt: "2026-06-26T08:30:00Z",
  },

  error_daemon_unavailable: {
    kind: "error",
    errorKind: "daemon_unavailable",
    message:
      "Local Bicameral daemon is not reachable. Check that it is running.",
    retryAfterRepair: true,
  },

  error_wrong_daemon: {
    kind: "error",
    errorKind: "wrong_daemon",
    message: "The daemon at this address serves a different Bicameral instance.",
    retryAfterRepair: true,
  },

  error_wrong_account: {
    kind: "error",
    errorKind: "account_mismatch",
    message: "The cloud account does not match the daemon's operator account.",
    retryAfterRepair: true,
  },

  error_unsupported_daemon: {
    kind: "error",
    errorKind: "unsupported_daemon",
    message:
      "This daemon version does not support hosted bridge pairing. Update to the latest version.",
    retryAfterRepair: false,
  },

  error_origin_mismatch: {
    kind: "error",
    errorKind: "origin_mismatch",
    message: "Browser origin does not match the paired origin.",
    retryAfterRepair: true,
  },

  error_origin_not_trusted: {
    kind: "error",
    errorKind: "origin_not_trusted",
    message:
      "Origin is not on the trusted origin allowlist. Contact your administrator to add it.",
    retryAfterRepair: false,
  },

  error_missing_cloud_session: {
    kind: "error",
    errorKind: "missing_cloud_session",
    message:
      "Cloud session is missing. Reopen the hosted dashboard and sign in again.",
    retryAfterRepair: false,
  },
};

// ── Session info fixtures (for session shell view) ─────────────────

export const SESSION_FIXTURES: Record<string, HostedBridgeSessionInfo> = {
  active: {
    pairingState: "hosted_bridge_paired",
    origin: "https://app.bicameral-ai.com",
    cloudAccountId: "acct_test_12345",
    pairedAt: "2026-06-25T10:00:00Z",
    expiresAt: "2026-06-26T10:00:00Z",
  },

  expiring_soon: {
    pairingState: "hosted_bridge_paired",
    origin: "https://app.bicameral-ai.com",
    cloudAccountId: "acct_test_67890",
    pairedAt: "2026-06-26T08:00:00Z",
    expiresAt: "2026-06-26T09:00:00Z",
  },
};

// ── Dashboard adapter fixtures (#616) ──────────────────────────────

export const DASHBOARD_ADAPTER_FIXTURES: Record<
  string,
  HostedDashboardAdapterResponse
> = {
  hosted_with_data: {
    mode: "hosted",
    workspace: {
      project_name: "my-service",
      event_store_healthy: true,
      has_workspace_binding: true,
      setup_state: "normal",
      code_grounding_ready: true,
      decision_count: 5,
      candidate_count: 2,
      event_store_kind: "github",
    },
    ledger_summary: {
      decision_count: 5,
      candidate_count: 2,
      needs_signoff_count: 1,
      stale_source_count: 0,
      bound_count: 3,
      last_promoted_at: "2026-06-25T14:30:00Z",
    },
    projects: [
      {
        name: "my-service",
        event_store_healthy: true,
        has_workspace_binding: true,
        setup_state: "normal",
      },
    ],
    status_message: "Hosted dashboard adapter serving data.",
  },

  hosted_empty: {
    mode: "hosted",
    ledger_summary: {
      decision_count: 0,
      candidate_count: 0,
      needs_signoff_count: 0,
      stale_source_count: 0,
      bound_count: 0,
    },
    projects: [],
    status_message: "No projects registered.",
  },
};

// ── Renderable ledger projection fixtures (#705, T4) ───────────────
//
// These deliberately model what a network-origin render should look like:
// only allow-listed fields, opaque ids, and a title that has ALREADY been
// through the daemon-side redactor (note the `[redacted:secret]` span). No
// decision bodies, excerpts, code paths, actors, or secrets appear — the
// conformance tests enforce that.

export const LEDGER_PROJECTION_FIXTURES: Record<
  string,
  HostedLedgerProjection
> = {
  with_data: {
    summary: {
      decision_count: 2,
      candidate_count: 1,
      needs_signoff_count: 1,
      stale_source_count: 1,
      bound_count: 1,
      last_promoted_at: "2026-06-25T14:30:00Z",
    },
    decisions: [
      {
        decision_id: "3f1c2b7a-0000-4000-8000-000000000001",
        candidate_id: "3f1c2b7a-0000-4000-8000-0000000000a1",
        title: "Rotate [redacted:secret] before launch",
        level: "architecture",
        signoff_state: "approved",
        review_state: "needs_signoff_review",
        tracking_state: "tracked",
        source_freshness: "fresh",
        evidence_state: "verified",
        promoted_at: "2026-06-25T14:30:00Z",
        source_evidence_count: 3,
        binding_count: 1,
      },
      {
        decision_id: "3f1c2b7a-0000-4000-8000-000000000002",
        candidate_id: "3f1c2b7a-0000-4000-8000-0000000000a2",
        title: "Adopt event-store snapshotting",
        level: "product",
        signoff_state: "proposed",
        review_state: "advisory",
        tracking_state: "tracked",
        source_freshness: "stale",
        evidence_state: "unknown_not_indexed",
        promoted_at: "2026-06-24T09:15:00Z",
        source_evidence_count: 0,
        binding_count: 0,
      },
    ],
    events: [
      {
        id: "3f1c2b7a-0000-4000-8000-0000000000e1",
        event_type: "approve_signoff",
        timestamp: "2026-06-25T14:30:00Z",
      },
      {
        id: "3f1c2b7a-0000-4000-8000-0000000000e2",
        event_type: "accept_candidate",
        timestamp: "2026-06-24T09:15:00Z",
      },
    ],
    projection_only: true,
  },

  empty: {
    summary: {
      decision_count: 0,
      candidate_count: 0,
      needs_signoff_count: 0,
      stale_source_count: 0,
      bound_count: 0,
    },
    decisions: [],
    events: [],
    projection_only: true,
  },
};
