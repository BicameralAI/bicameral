<script lang="ts">
  /**
   * Workspace Binding Card — hosted dashboard binding CTA and readiness view
   * (bicameral-bot#734).
   *
   * Renders the daemon-owned workspace binding readiness (#733) for a selected
   * hosted project. Presentational and renderer-only: it never binds a
   * workspace and shows no local absolute path, provider token, source body,
   * graph index, or event-store body. Binding happens on a LOCAL surface — the
   * hosted dashboard only guides the user there (MCP-assisted bind is the
   * primary alpha path; CLI is a fallback when the daemon surface offers it).
   *
   * Spec ref: docs/specs/cloud-hosted-dashboard-local-daemon-runtime-architecture.md
   *           docs/specs/hosted-bridge-user-level-pairing.md
   * Issue: #734
   */
  import type { WorkspaceBindingView } from "../lib/workspace-binding-readiness";

  type Props = {
    view: WorkspaceBindingView;
    /** Retry a failed readiness fetch. */
    onretry?: () => void;
    /** Manually refresh readiness (in addition to background polling). */
    onrefresh?: () => void;
    /** Open the now-bound project (shown only in the bound state). */
    onopen?: () => void;
  };

  let { view, onretry, onrefresh, onopen }: Props = $props();
</script>

<div
  class="binding-card"
  data-testid="workspace-binding-card"
  data-state={view.kind}
>
  {#if view.kind === "loading"}
    <div class="binding-loading" data-testid="binding-loading">
      <span class="spinner" aria-hidden="true"></span>
      <span>Checking workspace readiness…</span>
    </div>
  {:else if view.kind === "error"}
    <div class="binding-block error" data-testid="binding-error" role="alert">
      <h2 class="binding-title">Could not check workspace readiness</h2>
      <p class="binding-body">{view.message}</p>
      {#if view.retryable && onretry}
        <button class="primary-action" onclick={onretry} data-testid="binding-retry">
          Retry
        </button>
      {/if}
    </div>
  {:else if view.kind === "unsupported"}
    <div
      class="binding-block unsupported"
      data-testid="binding-unsupported"
      role="alert"
    >
      <h2 class="binding-title">Workspace binding needs a daemon update</h2>
      <p class="binding-body">{view.message}</p>
      {#if view.repair}
        <div class="repair-hint" data-testid="binding-upgrade">
          <p class="repair-instruction">{view.repair.instruction}</p>
          <code class="command" data-testid="binding-upgrade-command"
            >{view.repair.update_command}</code
          >
        </div>
      {/if}
      <!-- Fails closed: no bind controls are offered in the unsupported state. -->
    </div>
  {:else if view.kind === "bound"}
    <div class="binding-block bound" data-testid="binding-bound">
      <span class="ready-badge" data-testid="binding-ready-badge">Bound</span>
      <h2 class="binding-title">{view.projectName} is bound to a local workspace</h2>
      <p class="binding-body">{view.message}</p>
      {#if onopen}
        <button class="primary-action" onclick={onopen} data-testid="binding-open">
          Open project
        </button>
      {/if}
    </div>
  {:else if view.kind === "repair_required"}
    <div class="binding-block repair" data-testid="binding-repair">
      <span class="repair-badge">Repair needed</span>
      <h2 class="binding-title">{view.projectName} needs its workspace repaired</h2>
      {#each view.caveats as caveat}
        <p class="binding-body">{caveat}</p>
      {/each}
      <p class="binding-instruction">{view.instruction}</p>
      <div class="surface-steps">
        {#if view.mcpAvailable}
          <p class="surface-step" data-testid="repair-mcp">
            Open the Bicameral MCP client in this repository and approve the
            repair.
          </p>
        {/if}
        {#if view.cliAvailable}
          <p class="surface-step" data-testid="repair-cli-fallback">
            Or run this on the machine running the daemon:
            <code class="command">{view.command}</code>
          </p>
        {/if}
      </div>
    </div>
  {:else if view.kind === "unbound"}
    <div class="binding-block unbound" data-testid="binding-cta">
      <span class="next-step-badge" data-testid="binding-next-step-badge">
        Next step
      </span>
      <h2 class="binding-title">Bind {view.projectName} to your local code</h2>
      <p class="binding-body">
        This project is registered. Bind it to a local workspace to unlock
        code-grounded decisions and reviews.
      </p>

      {#if view.mcpAvailable}
        <div class="primary-path" data-testid="binding-mcp-primary">
          <span class="path-label">Recommended</span>
          <p class="surface-step">
            Open the Bicameral MCP client in this repository and approve the
            binding when prompted. Binding runs locally on the machine running
            the daemon — the hosted dashboard cannot bind a workspace for you.
          </p>
        </div>
      {/if}

      {#if view.cliAvailable}
        <div class="fallback-path" data-testid="binding-cli-fallback">
          <span class="path-label">CLI fallback</span>
          <p class="surface-step">
            Prefer the terminal? Run this on the machine running the daemon:
          </p>
          <code class="command" data-testid="binding-command">{view.command}</code>
        </div>
      {/if}

      <div class="binding-footer">
        {#if view.polling}
          <span class="polling" data-testid="binding-polling">
            <span class="spinner small" aria-hidden="true"></span>
            Waiting for the daemon to report the workspace is bound…
          </span>
        {/if}
        {#if onrefresh}
          <button
            class="ghost-action"
            onclick={onrefresh}
            data-testid="binding-refresh"
          >
            Refresh
          </button>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .binding-card {
    max-width: 560px;
    margin: 0 auto;
  }

  .binding-block {
    background: #fff;
    border: 1px solid var(--line);
    border-radius: var(--radius-md, 8px);
    padding: 1.5rem;
  }

  .binding-block.unbound {
    border-color: var(--accent-200);
    background: var(--accent-50);
  }

  .binding-block.error,
  .binding-block.unsupported,
  .binding-block.repair {
    border-color: #fde68a;
    background: var(--warn-50);
  }

  .binding-title {
    font-size: 1rem;
    font-weight: 600;
    color: var(--ink-900);
    margin: 0.5rem 0 0.5rem;
  }

  .binding-body {
    font-size: 0.8125rem;
    color: var(--ink-700);
    line-height: 1.5;
    margin: 0 0 0.5rem;
  }

  .binding-instruction {
    font-size: 0.8125rem;
    color: var(--ink-900);
    line-height: 1.5;
    margin: 0.25rem 0 0.75rem;
  }

  .next-step-badge,
  .ready-badge,
  .repair-badge {
    display: inline-block;
    font-size: var(--text-xs, 0.6875rem);
    font-weight: 600;
    line-height: 1.5;
    padding: 2px var(--sp-2, 0.5rem);
    border-radius: var(--radius-sm, 4px);
    border: 1px solid transparent;
  }

  .next-step-badge {
    color: var(--accent-600);
    background: var(--accent-100, #e0e7ff);
    border-color: var(--accent-200);
  }

  .ready-badge {
    color: var(--ok-600);
    background: var(--ok-50);
    border-color: #a7f3d0;
  }

  .repair-badge {
    color: var(--warn-600);
    background: var(--warn-50);
    border-color: #fde68a;
  }

  .primary-path,
  .fallback-path,
  .repair-hint {
    border: 1px solid var(--line);
    border-radius: var(--radius-sm, 4px);
    padding: 0.75rem 1rem;
    margin-top: 0.75rem;
    background: #fff;
  }

  .path-label {
    display: block;
    font-size: var(--text-xs, 0.6875rem);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--ink-500);
    margin-bottom: 0.25rem;
  }

  .surface-step {
    font-size: 0.8125rem;
    color: var(--ink-700);
    line-height: 1.5;
    margin: 0;
  }

  .command {
    display: inline-block;
    margin-top: 0.5rem;
    padding: 0.25rem 0.5rem;
    font-family: var(--font-mono, monospace);
    font-size: 0.75rem;
    background: var(--canvas-muted, #f3f4f6);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm, 4px);
    color: var(--ink-900);
  }

  .binding-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
    margin-top: 1rem;
  }

  .polling {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.75rem;
    color: var(--ink-500);
  }

  .binding-loading {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 1.5rem;
    font-size: 0.8125rem;
    color: var(--ink-500);
  }

  .spinner {
    width: 14px;
    height: 14px;
    border: 2px solid var(--line);
    border-top-color: var(--accent-500);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  .spinner.small {
    width: 12px;
    height: 12px;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .primary-action {
    display: inline-flex;
    align-items: center;
    padding: 0.5rem 1rem;
    border: none;
    border-radius: var(--radius-sm, 6px);
    background: var(--accent-500);
    color: #fff;
    font-family: inherit;
    font-size: 0.8125rem;
    font-weight: 600;
    cursor: pointer;
    margin-top: 0.5rem;
  }

  .primary-action:hover {
    background: var(--accent-600);
  }

  .ghost-action {
    background: none;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm, 6px);
    padding: 0.375rem 0.75rem;
    font-family: inherit;
    font-size: 0.75rem;
    color: var(--ink-700);
    cursor: pointer;
  }

  .ghost-action:hover {
    background: var(--canvas-muted, #f3f4f6);
  }
</style>
