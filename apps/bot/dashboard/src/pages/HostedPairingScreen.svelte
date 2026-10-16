<script lang="ts">
  /**
   * Production pairing surface (bot#1030 / #1035).
   * Maps HostedBridgeUIState + pending code + MCP readiness into ConnectStatus.
   * LocalConfirmationScreen still owns the same-origin Approve/Deny path when
   * bridgeState.kind === "confirmation" (HostedDashboard routes that separately).
   */
  import type { HostedBridgeUIState } from "../lib/hosted-bridge-types";
  import type { McpReadinessState } from "../lib/mcp-readiness";
  import { bridgeStateToConnectStatus } from "../lib/connect-status";
  import CloudConnectDevice from "./CloudConnectDevice.svelte";

  type Props = {
    state: HostedBridgeUIState;
    pairingCode?: string | null;
    mcpState?: McpReadinessState;
    mcpHostId?: string;
    mcpReasons?: string[];
    /** Operator-facing setup command; prefer daemon safe_display_argv. */
    setupCommand?: string;
    accountLabel?: string;
    onpair?: () => void;
    onauthrestart?: () => void;
    onuselocal?: () => void;
    oncontinue?: () => void;
    onrefreshmcp?: () => void;
  };

  let {
    state,
    pairingCode = null,
    mcpState = "not_installed",
    mcpHostId = "claude",
    mcpReasons = [],
    setupCommand = "bicameral setup mcp",
    accountLabel = "hosted",
    onpair,
    onauthrestart,
    onuselocal: _onuselocal,
    oncontinue,
    onrefreshmcp,
  }: Props = $props();

  const connectStatus = $derived(bridgeStateToConnectStatus(state));

  function handleCheck() {
    onpair?.();
  }

  function handleRetry() {
    if (state.kind === "error" && state.errorKind === "missing_cloud_session") {
      onauthrestart?.();
      return;
    }
    onpair?.();
  }

  function handleCopy(
    kind: "install" | "update" | "gateway" | "pairing" | "setup",
  ) {
    const map: Record<string, string> = {
      install: "curl -fsSL https://install.bicameral-ai.com/install.sh | sh",
      update: "bicameral update",
      gateway: "bicameral gateway start",
      pairing: pairingCode ?? "",
      setup: setupCommand,
    };
    const text = map[kind] ?? "";
    if (text) void navigator.clipboard?.writeText(text).catch(() => {});
  }
</script>

<div class="hosted-pairing-wrap" data-testid="hosted-pairing-screen">
  <CloudConnectDevice
    {accountLabel}
    status={connectStatus}
    {pairingCode}
    {mcpState}
    {mcpHostId}
    {mcpReasons}
    {setupCommand}
    oncheck={handleCheck}
    onretry={handleRetry}
    oncontinue={oncontinue}
    oncopy={handleCopy}
    data-testid="hosted-connect-device"
  />
  {#if connectStatus === "connected" && mcpState !== "active_verified" && onrefreshmcp}
    <p class="refresh-hint">
      <button type="button" class="link-btn" onclick={onrefreshmcp}>
        Refresh MCP status
      </button>
    </p>
  {/if}
</div>

<style>
  .hosted-pairing-wrap {
    flex: 1;
    display: flex;
    flex-direction: column;
  }
  .refresh-hint {
    text-align: center;
    margin: -1rem 0 1.5rem;
    font-size: var(--text-sm);
  }
  .link-btn {
    background: none;
    border: none;
    color: var(--accent-600);
    font: inherit;
    cursor: pointer;
    text-decoration: underline;
  }
</style>
