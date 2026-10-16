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
