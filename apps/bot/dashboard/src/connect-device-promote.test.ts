/**
 * Hosted connection support contracts.
 *
 * Factory #488 Option B makes Cloud `/connect` the sole production pre-pair
 * renderer. Bot keeps only shared status/readiness mappings and the post-handoff
 * `/app` admin surface. `CloudConnectDevice` / `HostedPairingScreen` are no
 * longer production connect-device authority.
 */
import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render } from "@testing-library/svelte";
import HostedConnections from "./pages/HostedConnections.svelte";
import { bridgeStateToConnectStatus } from "./lib/connect-status";
import {
  mapDaemonMcpState,
  mcpViewFromCapabilities,
  isMcpVerified,
  isMcpProgressible,
  setupCommandFromArgv,
  DEFAULT_MCP_SETUP_COMMAND,
} from "./lib/mcp-readiness";

afterEach(() => cleanup());

describe("bridgeStateToConnectStatus", () => {
  it("maps unpaired / pending / paired / update_required without granting authority", () => {
    expect(bridgeStateToConnectStatus({ kind: "unpaired" })).toBe("unavailable");
    expect(
      bridgeStateToConnectStatus({
        kind: "pairing_pending",
        reason: "connecting",
      }),
    ).toBe("checking");
    expect(
      bridgeStateToConnectStatus({
        kind: "pairing_pending",
        reason: "awaiting_confirmation",
      }),
    ).toBe("pending");
    expect(
      bridgeStateToConnectStatus({
        kind: "paired",
        origin: "https://app.example",
        cloudAccountId: "a",
        pairedAt: "",
        expiresAt: "",
      }),
    ).toBe("connected");
    expect(
      bridgeStateToConnectStatus({
        kind: "error",
        errorKind: "unsupported_daemon",
        message: "update",
        retryAfterRepair: true,
      }),
    ).toBe("update_required");
  });
});

describe("post-handoff MCP readiness mapping", () => {
  it("maps daemon snake_case states", () => {
    expect(mapDaemonMcpState("active_verified")).toBe("active_verified");
    expect(mapDaemonMcpState("not_installed")).toBe("not_installed");
    expect(mapDaemonMcpState("unknown")).toBe("not_installed");
  });

  it("picks mcp capability from operator-home list", () => {
    const view = mcpViewFromCapabilities([
      {
        capability_kind: "mcp",
        subject_id: "codex",
        state: "active_verified",
        reason_codes: [],
      },
    ]);
    expect(view.state).toBe("active_verified");
    expect(view.hostId).toBe("codex");
    expect(isMcpVerified(view.state)).toBe(true);
    expect(isMcpProgressible(view.state)).toBe(true);
    expect(view.setupCommand).toBe(DEFAULT_MCP_SETUP_COMMAND);
  });

  it("keeps package-vs-host activation truth distinct", () => {
    expect(isMcpProgressible("installed_pending_host_action")).toBe(true);
    expect(isMcpVerified("installed_pending_host_action")).toBe(false);
    expect(isMcpProgressible("not_installed")).toBe(false);
    expect(isMcpProgressible("repair_required")).toBe(false);
    expect(isMcpProgressible("active_verified")).toBe(true);
  });

  it("uses daemon safe_display_argv when present", () => {
    expect(setupCommandFromArgv(["bicameral", "setup", "mcp"])).toBe(
      "bicameral setup mcp",
    );
    expect(setupCommandFromArgv(null)).toBe(DEFAULT_MCP_SETUP_COMMAND);
    const view = mcpViewFromCapabilities([
      {
        capability_kind: "mcp",
        state: "not_installed",
        safe_display_argv: ["bicameral", "setup", "mcp"],
      },
    ]);
    expect(view.setupCommand).toBe("bicameral setup mcp");
  });
});

describe("HostedConnections grey-out", () => {
  it("renders Drive OAuth and greys unsupported providers", () => {
    const { getByTestId } = render(HostedConnections, {
      props: { drivePhase: "idle" },
    });
    expect(getByTestId("hosted-drive-oauth")).toBeTruthy();
    expect(getByTestId("connection-linear")).toBeTruthy();
    expect(
      getByTestId("connection-linear").classList.contains("greyed"),
    ).toBe(true);
  });
});
