<script lang="ts">
  /**
   * Hosted Project Shell — wraps existing bot dashboard surfaces in a
   * hosted tab-navigation shell for a ready project.
   *
   * Reuses IngestionGate, LedgerView, Channels, and read-only
   * ProjectSettings from the existing bot dashboard. Does not invent
   * hosted-only Decision, Activity, Source, compliance, signoff, or
   * code-evidence shapes.
   *
   * Governed mutations are disabled until the hosted bridge command
   * policy/receipt path is implemented and proven.
   *
   * Spec ref: docs/specs/alpha-hosted-dashboard-ui.md
   * Issue: #620
   */
  import type { HostedBridgeSessionInfo } from "../lib/hosted-bridge-types";
  import type { ProjectEntry } from "../lib/project-fixtures";
  import HostedLedgerView from "./HostedLedgerView.svelte";
  import Channels from "./Channels.svelte";

  type HostedProjectTab = "activity" | "decisions" | "channels" | "settings";

  type Props = {
    session: HostedBridgeSessionInfo;
    project: ProjectEntry;
  sessionToken?: string;
    governedActionsEnabled?: boolean;
    onback?: () => void;
    onuselocal?: () => void;
  };

  let {
    session,
    project,
  sessionToken,
    governedActionsEnabled = false,
    onback,
    onuselocal,
  }: Props = $props();

  let activeTab = $state<HostedProjectTab>("activity");

  const isExpiringSoon = $derived(() => {
    try {
      const remaining = new Date(session.expiresAt).getTime() - Date.now();
      return remaining > 0 && remaining < 3600_000;
    } catch {
      return false;
    }
  });
</script>

<div class="hosted-project-shell" data-testid="hosted-project-shell">
  <div class="shell-header" data-testid="shell-header">
    <div class="header-left">
      {#if onback}
        <button class="back-btn" onclick={onback} data-testid="back-btn">
          &larr;
        </button>
      {/if}
      <span class="project-label">Project:</span>
      <span class="project-name" data-testid="project-name">{project.name}</span>
    </div>
    <div class="header-right">
      <span
        class="hosted-badge"
        class:expiring={isExpiringSoon()}
        data-testid="hosted-badge"
      >
        Paired hosted view
      </span>
      {#if onuselocal}
        <button class="local-link" onclick={onuselocal}>
          Open local dashboard
        </button>
      {/if}
    </div>
  </div>

  <nav class="tab-bar" data-testid="tab-bar">
    <button
      class="tab"
      class:active={activeTab === "activity"}
      onclick={() => (activeTab = "activity")}
      data-testid="tab-activity"
    >
      Activity
    </button>
    <button
      class="tab"
      class:active={activeTab === "decisions"}
      onclick={() => (activeTab = "decisions")}
      data-testid="tab-decisions"
    >
      Decisions
    </button>
    <button
      class="tab"
      class:active={activeTab === "channels"}
      onclick={() => (activeTab = "channels")}
      data-testid="tab-channels"
    >
      Channels
    </button>
    <button
      class="tab"
      class:active={activeTab === "settings"}
      onclick={() => (activeTab = "settings")}
      data-testid="tab-settings"
    >
      Settings
    </button>
  </nav>

  {#if !governedActionsEnabled}
    <div class="governed-notice" data-testid="governed-actions-disabled">
      Governed mutations are disabled in hosted mode. Use the local
      dashboard for review actions until hosted bridge command policy is
      implemented.
    </div>
  {/if}

  <div class="tab-content" data-testid="tab-content">
    {#if activeTab === "activity"}
      <div class="hosted-empty" data-testid="hosted-activity-deferred">
<h2>Activity</h2>
<p>Hosted Activity is waiting on a network-origin source/candidate projection. No fabricated tool submissions are shown.</p>
</div>
    {:else if activeTab === "decisions"}
      <HostedLedgerView {sessionToken} />
    {:else if activeTab === "channels"}
      <Channels />
    {:else if activeTab === "settings"}
      <div class="settings-readonly" data-testid="settings-readonly">
        <h2>Project Settings</h2>
        <p class="settings-notice">
          Settings are read-only in hosted mode. Use the local dashboard
          to modify project configuration.
        </p>
        <dl class="settings-summary" data-testid="settings-summary">
          <dt>Project</dt>
          <dd>{project.name}</dd>
          <dt>Event store</dt>
          <dd>{project.event_store_label}</dd>
          <dt>Status</dt>
          <dd>
            <span class="readiness-badge {project.readiness}">
              {project.readiness === "ready" ? "Ready" : project.readiness === "offline" ? "Offline" : "Needs access"}
            </span>
          </dd>
        </dl>
      </div>
    {/if}
  </div>
</div>

<style>
  .hosted-project-shell {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }

  .shell-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.625rem 1.25rem;
    background: var(--surface);
    border-bottom: 1px solid var(--line);
    gap: 1rem;
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .header-right {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .back-btn {
    background: none;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    padding: 0.25rem 0.5rem;
    font-size: var(--text-base);
    cursor: pointer;
    color: var(--ink-500);
    font-family: var(--font-sans);
    line-height: 1;
    transition:
      background var(--dur-fast) var(--ease-out),
      color var(--dur-fast) var(--ease-out);
  }

  .back-btn:hover {
    background: var(--canvas-muted);
    color: var(--ink-900);
  }

  .project-label {
    font-size: var(--text-sm);
    color: var(--ink-500);
  }

  .project-name {
    font-size: 0.9375rem;
    font-weight: 600;
    color: var(--ink-900);
  }

  .hosted-badge {
    font-size: var(--text-xs);
    font-weight: 600;
    line-height: 1.5;
    padding: 2px var(--sp-2);
    border-radius: var(--radius-sm);
    background: var(--accent-50);
    color: var(--accent-600);
    border: 1px solid var(--accent-200);
    white-space: nowrap;
  }

  .hosted-badge.expiring {
    background: var(--warn-50);
    color: var(--warn-600);
    border-color: #fde68a;
  }

  .local-link {
    background: none;
    border: none;
    color: var(--accent-600);
    font-size: 0.75rem;
    font-family: var(--font-sans);
    cursor: pointer;
    padding: 0;
    text-decoration: underline;
    text-decoration-color: transparent;
    transition: text-decoration-color var(--dur-fast) var(--ease-out);
  }

  .local-link:hover {
    text-decoration-color: var(--accent-600);
  }

  /* ── Tab bar ────────────────────────────────────────────────── */

  .tab-bar {
    display: flex;
    gap: 0;
    background: var(--surface);
    border-bottom: 1px solid var(--line);
    padding: 0 1.25rem;
  }

  .tab {
    padding: 0.625rem 1rem;
    border: none;
    background: none;
    font-family: var(--font-sans);
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--ink-500);
    cursor: pointer;
    border-bottom: 2px solid transparent;
    transition:
      color var(--dur-fast) var(--ease-out),
      background var(--dur-fast) var(--ease-out),
      border-color var(--dur-fast) var(--ease-out);
  }

  .tab:hover {
    color: var(--ink-900);
    background: var(--canvas-muted);
  }

  .tab.active {
    color: var(--accent-600);
    font-weight: 600;
    border-bottom-color: var(--accent-500);
  }

  /* ── Governed actions notice ─────────────────────────────────── */

  .governed-notice {
    padding: 0.5rem 1.25rem;
    background: var(--canvas-muted);
    border-bottom: 1px solid var(--line);
    font-size: 0.75rem;
    color: var(--ink-500);
  }

  /* ── Tab content ─────────────────────────────────────────────── */

  .tab-content {
    flex: 1;
    padding: 1.25rem;
    overflow: auto;
    background: var(--canvas);
  }

  /* ── Settings (read-only) ────────────────────────────────────── */

  .settings-readonly {
    max-width: 600px;
  }

  .settings-readonly h2 {
    font-size: var(--text-md);
    font-weight: 600;
    color: var(--ink-900);
    margin-bottom: 0.75rem;
  }

  .settings-notice {
    font-size: var(--text-sm);
    color: var(--ink-500);
    margin-bottom: 1.25rem;
    padding: 0.625rem 1rem;
    background: var(--canvas-muted);
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
  }

  .settings-summary {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0.5rem 1rem;
    font-size: var(--text-sm);
  }

  .settings-summary dt {
    font-weight: 500;
    color: var(--ink-500);
  }

  .settings-summary dd {
    color: var(--ink-900);
    margin: 0;
  }

  .readiness-badge {
    display: inline-block;
    font-size: var(--text-xs);
    font-weight: 600;
    line-height: 1.5;
    padding: 2px var(--sp-2);
    border-radius: var(--radius-sm);
    border: 1px solid transparent;
  }

  .readiness-badge.ready {
    color: var(--ok-600);
    background: var(--ok-50);
    border-color: #a7f3d0;
  }

  .readiness-badge.needs_access {
    color: var(--warn-600);
    background: var(--warn-50);
    border-color: #fde68a;
  }

  .readiness-badge.offline {
    color: var(--ink-500);
    background: var(--canvas-muted);
    border-color: var(--line);
  }

  /* ── Responsive ─────────────────────────────────────────────── */

  @media (max-width: 640px) {
    .shell-header {
      flex-direction: column;
      align-items: flex-start;
      gap: 0.5rem;
    }

    .tab-bar {
      overflow-x: auto;
    }

    .tab {
      white-space: nowrap;
    }
  }
</style>
