/**
 * Map HostedBridgeUIState → CloudConnectDevice ConnectStatus.
 * Pairing code / MCP readiness are owned by the parent, not bridge kind alone.
 */

import type { HostedBridgeUIState } from "./hosted-bridge-types";

/** Mirrors CloudConnectDevice ConnectStatus (keep in lockstep). */
export type ConnectStatus =
  | "unavailable"
  | "checking"
  | "pending"
  | "connected"
  | "update_required";

export function bridgeStateToConnectStatus(
  state: HostedBridgeUIState,
): ConnectStatus {
  switch (state.kind) {
    case "paired":
      return "connected";
    case "pairing_pending":
      return state.reason === "connecting" ? "checking" : "pending";
    case "error":
      if (state.errorKind === "unsupported_daemon") return "update_required";
      return "unavailable";
    case "expired":
    case "revoked":
    case "unpaired":
    case "confirmation":
    default:
      return "unavailable";
  }
}

/**
 * The header pill. A paired bridge is only "connected" while the daemon is
 * reachable: when the disconnect dialog is open the pill must not contradict it
 * (bicameral#52). Every other state is left to the pairing mapping.
 */
export function hostedConnectionStatus(
  state: HostedBridgeUIState,
  daemonDisconnected: boolean,
): ConnectStatus {
  const status = bridgeStateToConnectStatus(state);
  return daemonDisconnected && status === "connected" ? "unavailable" : status;
}
