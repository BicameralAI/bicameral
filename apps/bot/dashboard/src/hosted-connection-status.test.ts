/**
 * Header connection pill vs the disconnect dialog (bicameral#52, bot#1346).
 *
 * After a reload the header showed a green "Connected" pill while the "Local
 * daemon disconnected" dialog was open: the pill read only the pairing state
 * and ignored the daemon-disconnected flag the dialog reads. One function now
 * decides the pill from both.
 */

import { describe, expect, it } from "vitest";
import {
  hostedConnectionStatus,
  bridgeStateToConnectStatus,
} from "./lib/connect-status";
import type { HostedBridgeUIState } from "./lib/hosted-bridge-types";

const paired: HostedBridgeUIState = {
  kind: "paired",
  origin: "https://app.example.test",
  cloudAccountId: "acct-test",
  pairedAt: "2026-01-01T00:00:00Z",
  expiresAt: "2026-01-02T00:00:00Z",
};

describe("hostedConnectionStatus", () => {
  it("shows connected for a paired bridge with a reachable daemon", () => {
    expect(hostedConnectionStatus(paired, false)).toBe("connected");
  });

  it("never shows connected while the disconnect dialog is open", () => {
    expect(hostedConnectionStatus(paired, true)).not.toBe("connected");
    expect(hostedConnectionStatus(paired, true)).toBe("unavailable");
  });

  it("leaves every other bridge state to the pairing mapping", () => {
    const states: HostedBridgeUIState[] = [
      { kind: "unpaired" },
      { kind: "pairing_pending", reason: "connecting" } as HostedBridgeUIState,
    ];
    for (const state of states) {
      expect(hostedConnectionStatus(state, false)).toBe(
        bridgeStateToConnectStatus(state),
      );
    }
  });

  it("does not turn a non-connected state into something it was not", () => {
    expect(hostedConnectionStatus({ kind: "unpaired" }, true)).toBe(
      "unavailable",
    );
  });
});
