import { afterEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render } from "@testing-library/svelte";
import HostedDashboard from "./pages/HostedDashboard.svelte";

const pairedState = {
  kind: "paired" as const,
  origin: "https://app.bicameral-ai.com",
  cloudAccountId: "acct-test",
  pairedAt: "2026-08-11T19:00:00Z",
  expiresAt: "2026-08-11T20:00:00Z",
};

afterEach(() => cleanup());

describe("HostedDashboard post-handoff ownership (#1092) + Settings host IA", () => {
  it("enters the paired admin shell even when MCP is not installed", () => {
    const { getByTestId, queryByText, queryByTestId } = render(HostedDashboard, {
      props: {
        bridgeState: pairedState,
        mcpState: "not_installed",
        mcpHostId: "claude",
        mcpReasons: ["package_missing"],
        mcpSetupCommand: "bicameral setup mcp",
        projects: [],
      },
    });

    expect(getByTestId("paired-shell")).toBeTruthy();
    // Products home: status strip only — no permanent MCP essay.
    expect(queryByTestId("paired-mcp-readiness")).toBeNull();
    expect(queryByTestId("mcp-readiness-home")).toBeNull();
    expect(getByTestId("host-status-mcp")).toBeTruthy();
    expect(getByTestId("host-status-mcp").getAttribute("data-mcp-state")).toBe(
      "not_installed",
    );
    expect(queryByText("Continue to Products")).toBeNull();
    expect(queryByTestId("mcp-continue-products")).toBeNull();
    expect(queryByText("Install MCP")).toBeNull();
  });

  it("opens Settings → Agent host for MCP detail from the host strip", async () => {
    const { getByTestId, getByText, queryByTestId } = render(HostedDashboard, {
      props: {
        bridgeState: pairedState,
        mcpState: "installed_pending_host_action",
        mcpHostId: "claude",
        mcpReasons: ["activation_receipt_required"],
        mcpSetupCommand: "bicameral setup mcp",
        projects: [],
      },
    });

    expect(getByTestId("host-status-mcp").textContent).toMatch(/MCP needs host/i);
    await fireEvent.click(getByTestId("host-status-mcp"));

    expect(getByTestId("hosted-settings")).toBeTruthy();
    expect(getByTestId("settings-agent-host")).toBeTruthy();
    expect(getByTestId("settings-mcp-card")).toBeTruthy();
    expect(getByText("Pending host action")).toBeTruthy();
    expect(getByText("bicameral setup mcp")).toBeTruthy();
    expect(queryByTestId("mcp-continue-products")).toBeNull();
  });

  it("distinguishes pending host action from verified activation in the strip", () => {
    const pending = render(HostedDashboard, {
      props: {
        bridgeState: pairedState,
        mcpState: "installed_pending_host_action",
        mcpHostId: "claude",
        projects: [],
      },
    });
    expect(pending.getByTestId("host-status-mcp").textContent).toMatch(
      /MCP needs host/i,
    );
    expect(pending.queryByTestId("mcp-continue-products")).toBeNull();
    cleanup();

    const verified = render(HostedDashboard, {
      props: {
        bridgeState: pairedState,
        mcpState: "active_verified",
        mcpHostId: "codex",
        projects: [],
      },
    });
    expect(verified.getByTestId("paired-shell")).toBeTruthy();
    expect(verified.getByTestId("host-status-mcp").textContent).toMatch(
      /MCP ready/i,
    );
    expect(verified.queryByTestId("mcp-continue-products")).toBeNull();
  });

  it("keeps Products reachable in every MCP readiness state", () => {
    const states = [
      "not_installed",
      "prerequisite_missing",
      "installed_pending_host_action",
      "active_verified",
      "repair_required",
    ] as const;

    for (const mcpState of states) {
      const { getByTestId, queryByTestId } = render(HostedDashboard, {
        props: { bridgeState: pairedState, mcpState, projects: [] },
      });
      expect(getByTestId("paired-shell")).toBeTruthy();
      expect(getByTestId("product-ledger-main")).toBeTruthy();
      expect(queryByTestId("mcp-continue-products")).toBeNull();
      cleanup();
    }
  });

  it("routes create-without-storage into Settings → Storage", async () => {
    const { getByTestId } = render(HostedDashboard, {
      props: {
        bridgeState: pairedState,
        mcpState: "not_installed",
        eventSubstrateConnected: false,
        projects: [],
      },
    });

    await fireEvent.click(getByTestId("empty-create"));
    expect(getByTestId("hosted-settings")).toBeTruthy();
    expect(getByTestId("settings-storage")).toBeTruthy();
  });

  it("does not render install/gateway/pair/MCP curriculum when unpaired", () => {
    const { getByTestId, queryByTestId, queryByText } = render(HostedDashboard, {
      props: {
        bridgeState: { kind: "unpaired" },
        mcpState: "not_installed",
        projects: [],
      },
    });

    // Leave placeholder only — pair bootstrap is Cloud /connect, MCP is Settings.
    expect(getByTestId("hosted-handoff-leaving")).toBeTruthy();
    expect(queryByTestId("hosted-pairing-screen")).toBeNull();
    expect(queryByTestId("hosted-connect-device")).toBeNull();
    expect(queryByText("Install MCP")).toBeNull();
    expect(queryByText("Step 1 of 4")).toBeNull();
  });
});
