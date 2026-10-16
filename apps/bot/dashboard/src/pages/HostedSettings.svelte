<script lang="ts">
  /**
   * Operator Settings — logo-led connection list (Layout A).
   * Storage = Google Drive. Agent host = Claude / Codex MCP rows.
   *
   * When reached from the post-pair onboarding handoff, the same governed
   * Drive component is wrapped in a focused final-step presentation. Normal
   * Settings keeps the compact list.
   */
  import { GoogleDriveOAuthFlow, type DriveOAuthPhase } from "../ui";
  import McpReadinessCard from "../ui/McpReadinessCard.svelte";
  import type { McpReadinessState } from "../lib/mcp-readiness";

  export type SettingsSection = "storage" | "agent-host";

  type Props = {
    focusSection?: SettingsSection;
    drivePhase?: DriveOAuthPhase;
    driveAccountLabel?: string;
    driveSessionId?: string | null;
    driveOnboarding?: boolean;
    mcpState?: McpReadinessState;
    mcpHostId?: string;
    mcpReasons?: string[];
    mcpSetupCommand?: string;
    ondrivestart?: () => void;
    ondriveopenauth?: () => void;
    ondrivecancel?: () => void;
    ondriveskip?: () => void;
    onrefreshmcp?: () => void;
    oncopymcp?: () => void;
  };

  let {
    focusSection = "storage",
    drivePhase = "idle",
    driveAccountLabel = "",
    driveSessionId = null,
    driveOnboarding = false,
    mcpState = "not_installed",
    mcpHostId = "claude",
    mcpReasons = [],
    mcpSetupCommand = "bicameral setup mcp",
    ondrivestart,
    ondriveopenauth,
    ondrivecancel,
    ondriveskip,
    onrefreshmcp,
    oncopymcp,
  }: Props = $props();

  let storageEl = $state<HTMLElement | null>(null);
  let agentEl = $state<HTMLElement | null>(null);

  $effect(() => {
    const el = driveOnboarding
      ? storageEl
      : focusSection === "agent-host"
        ? agentEl
        : storageEl;
    if (el && typeof el.scrollIntoView === "function") {
      el.scrollIntoView({ behavior: "smooth", block: "start" });
    }
  });
</script>

<div class="hosted-settings" class:onboarding={driveOnboarding} data-testid="hosted-settings">
  {#if driveOnboarding}
    <section class="drive-onboarding-intro" data-testid="drive-onboarding-intro" aria-labelledby="drive-onboarding-title">
      <p class="onboarding-kicker">Final step</p>
      <h2 id="drive-onboarding-title">Connect Google Drive</h2>
      <p class="onboarding-copy">
        Connect your Google Drive so Bicameral can use your authorized Drive connection for its event store and memory data. Google will show the requested permissions before anything is authorized.
      </p>
      <ul class="onboarding-benefits">
        <li>Your Bicameral event store stays in your Google Drive.</li>
        <li>The connection uses Google OAuth and only the permissions you approve.</li>
        <li>This is optional now. You can connect later from Settings.</li>
      </ul>
    </section>
  {/if}

  <!--
    Layout A (normal Settings): stacked connection rows — logo | name | status | action.
    Focused onboarding reuses the exact same Drive OAuth component/controller.
  -->
  <div class="settings-group" data-testid="settings-storage" bind:this={storageEl} id="settings-storage">
    {#if !driveOnboarding}<p class="group-label">Storage</p>{/if}
    <GoogleDriveOAuthFlow
      phase={drivePhase}
      accountLabel={driveAccountLabel}
      sessionId={driveSessionId}
      onstart={ondrivestart}
      onopenauth={ondriveopenauth}
      oncancel={ondrivecancel}
      data-testid="settings-drive-oauth"
    />
    {#if driveOnboarding}
      <button
        type="button"
        class="skip-drive"
        data-testid="drive-onboarding-skip"
        onclick={() => ondriveskip?.()}
      >
        Skip for now
      </button>
    {/if}
  </div>

  {#if !driveOnboarding}
    <div
      class="settings-group"
      class:focused={focusSection === "agent-host"}
      data-testid="settings-agent-host"
      bind:this={agentEl}
      id="settings-agent-host"
    >
      <div class="group-head">
        <p class="group-label">Agent host</p>
        <button
          type="button"
          class="btn refresh-btn"
          data-testid="settings-mcp-refresh"
          onclick={() => onrefreshmcp?.()}
        >
          Refresh
        </button>
      </div>
      <McpReadinessCard
        state={mcpState}
        subjectId={mcpHostId}
        reasonCodes={mcpReasons}
        setupCommand={mcpSetupCommand}
        oncopy={oncopymcp}
        data-testid="settings-mcp-card"
      />
    </div>
  {/if}
</div>

<style>
  .hosted-settings {
    display: flex;
    flex-direction: column;
    gap: 1.25rem;
    max-width: 40rem;
  }
  .hosted-settings.onboarding {
    gap: 1rem;
  }
  .drive-onboarding-intro {
    padding: 1.25rem 1.25rem 1.1rem;
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background: var(--surface);
  }
  .onboarding-kicker {
    margin: 0 0 0.35rem;
    font-size: var(--text-xs);
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--ink-500);
  }
  .drive-onboarding-intro h2 {
    margin: 0;
    font-size: 1.35rem;
    color: var(--ink-900);
  }
  .onboarding-copy {
    margin: 0.65rem 0 0;
    max-width: 36rem;
    font-size: var(--text-sm);
    line-height: 1.55;
    color: var(--ink-600);
  }
  .onboarding-benefits {
    margin: 0.9rem 0 0;
    padding-left: 1.2rem;
    display: grid;
    gap: 0.35rem;
    font-size: var(--text-sm);
    line-height: 1.45;
    color: var(--ink-700);
  }
  .settings-group {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    scroll-margin-top: 1rem;
  }
  .group-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
  }
  .group-label {
    margin: 0;
    font-size: 0.6875rem;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--ink-500);
  }
  .skip-drive {
    align-self: center;
    appearance: none;
    border: 0;
    background: transparent;
    padding: 0.45rem 0.7rem;
    color: var(--ink-600);
    font: inherit;
    font-size: var(--text-sm);
    text-decoration: underline;
    text-underline-offset: 0.18em;
    cursor: pointer;
  }
  .skip-drive:hover {
    color: var(--ink-900);
  }
  .refresh-btn {
    min-height: 1.5rem;
    padding: 0 0.5rem;
    font-size: var(--text-xs);
  }
</style>
