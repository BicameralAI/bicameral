/** Central hosted-mode bridge client — replaces fixture-driven pairing
 * with live daemon calls and classifies daemon errors into UI states.
 *
 * Network failures → daemon_unavailable error state.
 * HTTP 500 from hosted-bridge routes → daemon_unavailable repair state.
 * Successful responses → mapped to HostedBridgeUIState.
 *
 * Issue: #648
 */

import type {
  HostedBridgeErrorKind,
  HostedBridgeUIState,
} from "./hosted-bridge-types";
import type { ProjectEntry } from "./project-fixtures";
import {
  requestHostedBridgePairing,
  fetchHostedBridgeStatus,
  pollHostedBridgePairStatus,
  fetchWorkspaceDiscovery,
  type HostedBridgePairResponse,
  type HostedBridgeStatusResponse,
  type HostedSetupProjection,
} from "./api";

// ── Error classification ───────────────────────────────────────────

function isDaemonNetworkError(err: unknown): boolean {
  if (err instanceof TypeError) return true;
  if (err instanceof Error && err.message.includes("fetch")) return true;
  return false;
}

function daemonUnavailableState(): HostedBridgeUIState {
  return {
    kind: "error",
    errorKind: "daemon_unavailable",
    // A network-level failure here can mean the daemon is down OR that the
    // browser cannot reach it over the direct data plane. The direct pull
    // depends on Private Network Access (Chromium-only); on Safari/Firefox the
    // request fails the same way even when the daemon is healthy (#698 item 3).
    // Surface both possibilities rather than only "daemon down".
    message:
      "Local Bicameral daemon is not reachable. Check that it is running. " +
      "Note: the hosted dashboard requires a Chromium-based browser " +
      "(Chrome, Edge, or Brave); Safari and Firefox are not yet supported.",
    retryAfterRepair: true,
  };
}

const PAIR_INIT_RECOVERY_KINDS: ReadonlySet<HostedBridgeErrorKind> = new Set([
  "missing_team_id",
  "missing_cloud_access_token",
]);

/** Typed POST /pair 400s that must return the operator to Cloud /connect. */
export function isPairInitRecoveryError(state: HostedBridgeUIState): boolean {
  return state.kind === "error" && PAIR_INIT_RECOVERY_KINDS.has(state.errorKind);
}

function pairInitKindFromBody(body: unknown): HostedBridgeErrorKind | null {
  const kind =
    body && typeof body === "object" && "kind" in body
      ? String((body as { kind?: unknown }).kind)
      : "";
  if (kind === "MissingTeamId") return "missing_team_id";
  if (kind === "MissingCloudAccessToken") return "missing_cloud_access_token";
  return null;
}

function pairInitRecoveryState(errorKind: HostedBridgeErrorKind): HostedBridgeUIState {
  return {
    kind: "error",
    errorKind,
    message: "Pair initiation is missing Cloud tenancy. Return to Connect and retry.",
    retryAfterRepair: true,
  };
}

function serverErrorState(status: number): HostedBridgeUIState {
  return {
    kind: "error",
    errorKind: "daemon_unavailable",
    message: `Daemon returned an error (HTTP ${status}). The daemon may need repair or restart.`,
    retryAfterRepair: true,
  };
}

// ── Response → UI state mapping ────────────────────────────────────

export type BridgeClientResult = {
  state: HostedBridgeUIState;
  sessionToken?: string;
};

export type PairClientResult = BridgeClientResult & {
  pairingCode?: string;
  requestHandle?: string;
  pollProof?: string;
};

function mapPairResponseToState(res: HostedBridgePairResponse): PairClientResult {
  if (res.error === "update_required" || res.kind === "update_required") {
    return {
      state: {
        kind: "error",
        errorKind: "unsupported_daemon",
        message:
          res.message ??
          "This daemon is incompatible with the hosted dashboard. Update Bicameral and retry.",
        retryAfterRepair: true,
      },
    };
  }
  if (res.state === "paired" && res.session_token && res.expires_at) {
    return {
      state: {
        kind: "paired",
        origin: res.origin ?? "",
        cloudAccountId: "",
        pairedAt: new Date().toISOString(),
        expiresAt: res.expires_at,
      },
      sessionToken: res.session_token,
    };
  }
  // #790 pending path: code + handle + proof, never a session token.
  if (res.status === "pending" || res.pairing_code || res.request_handle) {
    return {
      state: {
        kind: "pairing_pending",
        reason: "awaiting_confirmation",
        message:
          res.confirmation?.message ??
          res.message ??
          "Enter the pairing code on your local machine.",
      },
      pairingCode: res.pairing_code,
      requestHandle: res.request_handle,
      pollProof: res.poll_proof,
    };
  }
  return {
    state: {
      kind: "pairing_pending",
      reason: "awaiting_confirmation",
      message: res.message ?? "Waiting for daemon to confirm pairing.",
    },
  };
}

function mapStatusResponseToState(
  res: HostedBridgeStatusResponse,
): HostedBridgeUIState {
  if (res.pairing_state === "hosted_bridge_paired") {
    return {
      kind: "paired",
      origin: res.origin ?? "",
      cloudAccountId: res.cloud_account_id ?? "",
      pairedAt: res.paired_at ?? "",
      expiresAt: res.expires_at ?? "",
    };
  }
  if (res.pairing_state === "hosted_bridge_revoked") {
    return {
      kind: "revoked",
      reason: res.error ?? "Pairing was revoked.",
      revokedAt: new Date().toISOString(),
    };
  }
  if (res.kind === "expired" || res.pairing_state === "expired") {
    return {
      kind: "expired",
      reason: res.error ?? "Pairing session has expired.",
      revokedAt: new Date().toISOString(),
    };
  }
  if (res.error) {
    return {
      kind: "error",
      errorKind: (res.kind ?? "daemon_unavailable") as import("./hosted-bridge-types").HostedBridgeErrorKind,
      message: res.error,
      retryAfterRepair: true,
    };
  }
  return { kind: "unpaired" };
}

// ── Live bridge client ─────────────────────────────────────────────

/** Call daemon /api/v1/hosted-bridge/pair and classify response. */
export async function pairWithDaemon(
  origin: string,
  cloudAccountId: string,
  csrfToken: string,
  teamId: string,
  cloudAccessToken: string,
): Promise<PairClientResult> {
  try {
    const res = await requestHostedBridgePairing({
      origin,
      cloud_account_id: cloudAccountId,
      csrf_token: csrfToken,
      team_id: teamId,
      cloud_access_token: cloudAccessToken,
    });
    return mapPairResponseToState(res);
  } catch (err: unknown) {
    if (isDaemonNetworkError(err)) {
      return { state: daemonUnavailableState() };
    }
    const status =
      err && typeof err === "object" && "status" in err
        ? Number((err as { status: number }).status)
        : 500;
    const body =
      err && typeof err === "object" && "body" in err
        ? (err as { body?: unknown }).body
        : undefined;
    const pairInitKind = status === 400 ? pairInitKindFromBody(body) : null;
    if (pairInitKind) {
      return { state: pairInitRecoveryState(pairInitKind) };
    }
    if (status === 409 || status === 426) {
      return {
        state: {
          kind: "error",
          errorKind: "unsupported_daemon",
          message:
            err instanceof Error
              ? err.message
              : "Update required to pair with this daemon.",
          retryAfterRepair: true,
        },
      };
    }
    return { state: serverErrorState(status || 500) };
  }
}

/**
 * Poll GET /api/v1/hosted-bridge/pair/status until approved/denied/expired.
 * Releases session_token only on approved.
 */
export async function pollPendingPairStatus(opts: {
  requestHandle: string;
  pollProof: string;
  cloudAccountId: string;
}): Promise<PairClientResult> {
  try {
    const res = await pollHostedBridgePairStatus(opts);
    if (res.status === "approved" && res.session_token) {
      return {
        state: {
          kind: "paired",
          origin: typeof window !== "undefined" ? window.location.origin : "",
          cloudAccountId: opts.cloudAccountId,
          pairedAt: new Date().toISOString(),
          expiresAt: res.expires_at ?? "",
        },
        sessionToken: res.session_token,
      };
    }
    if (res.status === "denied") {
      return {
        state: {
          kind: "revoked",
          reason: res.message ?? "Pairing was denied on the local machine.",
          revokedAt: new Date().toISOString(),
        },
      };
    }
    if (res.status === "expired") {
      return {
        state: {
          kind: "expired",
          reason: res.message ?? "Pairing request expired.",
          revokedAt: new Date().toISOString(),
        },
      };
    }
    if (res.status === "incompatible") {
      return {
        state: {
          kind: "error",
          errorKind: "unsupported_daemon",
          message: res.message ?? "Update required.",
          retryAfterRepair: true,
        },
      };
    }
    return {
      state: {
        kind: "pairing_pending",
        reason: "awaiting_confirmation",
        message: res.message ?? "Waiting for local approval…",
      },
      requestHandle: opts.requestHandle,
      pollProof: opts.pollProof,
    };
  } catch (err: unknown) {
    if (isDaemonNetworkError(err)) {
      return { state: daemonUnavailableState() };
    }
    return { state: serverErrorState(500) };
  }
}

/** Poll daemon bridge status and classify response. */
export async function pollBridgeStatus(
  sessionToken: string,
): Promise<BridgeClientResult> {
  try {
    const res = await fetchHostedBridgeStatus(sessionToken);
    return { state: mapStatusResponseToState(res) };
  } catch (err: unknown) {
    if (isDaemonNetworkError(err)) {
      return { state: daemonUnavailableState() };
    }
    return { state: serverErrorState(500) };
  }
}

/** Fetch project readiness from daemon workspace-discovery endpoint. */
export async function fetchDaemonProjects(
  sessionToken: string,
): Promise<{ projects: ProjectEntry[]; error?: HostedBridgeUIState }> {
  try {
    const res = await fetchWorkspaceDiscovery(sessionToken);
    const projects: ProjectEntry[] = res.workspaces.map((ws, idx) => ({
      id: `hosted_${idx}_${ws.project_name.replace(/\s+/g, "_")}`,
      name: ws.project_name,
      event_store_kind: (ws.event_store_kind === "google_drive" ? "google_drive" : "github") as ProjectEntry["event_store_kind"],
      event_store_label: ws.event_store_kind === "google_drive" ? "Google Drive" : "GitHub event store",
      // A joined Product is ready once its event store is healthy. A
      // `LocalWorkspaceBinding` is optional and independently addable (bot#769):
      // its absence must NOT downgrade readiness or block `Open`.
      readiness: ws.event_store_healthy ? "ready" as const : "needs_access" as const,
      access_failure: !ws.event_store_healthy ? "missing_connection" as const : undefined,
    }));
    return { projects };
  } catch (err: unknown) {
    if (isDaemonNetworkError(err)) {
      return { projects: [], error: daemonUnavailableState() };
    }
    return { projects: [], error: serverErrorState(500) };
  }
}

/**
 * Apply a daemon-authoritative setup readiness projection (bot#769) onto a
 * Product row. Pure and orthogonal to membership: it only sets the readiness /
 * `setup_incomplete` axes `rowAction` consumes, and never rolls membership back
 * — a joined Product stays a member (`Continue setup` / `Repair`), never `Join`.
 *
 *   open           → ready, setup complete            → `Open`
 *   continue_setup → ready, setup incomplete          → `Continue setup`
 *   repair         → needs_access                     → `Repair`
 */
export function applySetupProjection(
  project: ProjectEntry,
  projection: HostedSetupProjection,
): ProjectEntry {
  switch (projection.readiness) {
    case "open":
      return { ...project, readiness: "ready", setup_incomplete: false };
    case "continue_setup":
      return { ...project, readiness: "ready", setup_incomplete: true };
    case "repair":
      return {
        ...project,
        readiness: "needs_access",
        setup_incomplete: false,
        access_failure: project.access_failure ?? "missing_connection",
      };
  }
}

/** Classify a raw fetch error from any hosted-bridge display/projection route. */
export function classifyHostedRouteError(
  err: unknown,
  status?: number,
): HostedBridgeUIState {
  if (isDaemonNetworkError(err)) {
    return daemonUnavailableState();
  }
  if (status !== undefined && status >= 500) {
    return serverErrorState(status);
  }
  return daemonUnavailableState();
}
