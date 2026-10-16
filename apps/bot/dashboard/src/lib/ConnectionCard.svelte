<script lang="ts">
  import type { Connection } from "./connection-fixtures";
  import type {
    ConnectionStartOutcome,
    ConnectionCompletionOutcome,
  } from "./types";
  import { startConnection, pollConnectionStatus } from "./api";

  type Props = {
    connection: Connection;
    onstatuschange?: () => void;
  };

  let { connection, onstatuschange }: Props = $props();

  let startOutcome = $state<ConnectionStartOutcome | null>(null);
  let completionOutcome = $state<ConnectionCompletionOutcome | null>(null);
  let startError = $state<string | null>(null);
  let starting = $state(false);
  let polling = $state(false);

  let pollTimer: ReturnType<typeof setInterval> | null = null;

  const POLL_INTERVAL_MS = 3000;
  const MAX_POLL_ATTEMPTS = 60;

  const PROVIDER_LABELS: Record<string, string> = {
    github: "GitHub",
    google: "Google Drive",
    linear: "Linear",
  };

  const PROVIDER_DESCRIPTIONS: Record<string, string> = {
    github: "Repositories, pull requests, issues, and commits.",
    google: "Alpha Product event store (Drive folder) plus docs and shared folders.",
    linear: "Teams, issues, and project tracking.",
  };

  const STATUS_CONFIG: Record<
    string,
    { label: string; className: string; action: string }
  > = {
    connected: {
      label: "Connected",
      className: "status-connected",
      action: "Manage",
    },
    action_needed: {
      label: "Action needed",
      className: "status-action-needed",
      action: "Reconnect",
    },
    not_connected: {
      label: "Not connected",
      className: "status-not-connected",
      action: "Connect",
    },
    unsupported: {
      label: "Unsupported",
      className: "status-unsupported",
      action: "",
    },
  };

  const statusInfo = $derived(
    STATUS_CONFIG[connection.status] ?? STATUS_CONFIG["not_connected"],
  );
  const providerLabel = $derived(
    PROVIDER_LABELS[connection.provider_id] ?? connection.provider_id,
  );
  const providerDescription = $derived(
    PROVIDER_DESCRIPTIONS[connection.provider_id] ?? "",
  );

  const isUnsupported = $derived(connection.status === "unsupported");

  function stopPolling() {
    if (pollTimer !== null) {
      clearInterval(pollTimer);
      pollTimer = null;
    }
    polling = false;
  }

  function startPolling(provider: string, sessionId: string) {
    polling = true;
    let attempts = 0;

    pollTimer = setInterval(async () => {
      attempts++;
      if (attempts > MAX_POLL_ATTEMPTS) {
        stopPolling();
        completionOutcome = {
          outcome: "expired",
          provider,
          message: "Polling timed out. Please try again.",
        };
        return;
      }

      try {
        const result = await pollConnectionStatus(provider, sessionId);
        if (result.outcome !== "pending") {
          stopPolling();
          completionOutcome = result;
          if (result.outcome === "connected") {
            onstatuschange?.();
          }
        }
      } catch {
        stopPolling();
        completionOutcome = {
          outcome: "provider_unavailable",
          provider,
          message: "Failed to check connection status.",
        };
      }
    }, POLL_INTERVAL_MS);
  }

  async function handleStartConnection() {
    starting = true;
    startError = null;
    startOutcome = null;
    completionOutcome = null;
    stopPolling();

    try {
      const outcome = await startConnection(connection.provider_id);
      startOutcome = outcome;

      if (
        outcome.outcome === "start_url" &&
        outcome.flow_type === "device_flow"
      ) {
        startPolling(outcome.provider, outcome.auth_session_id);
      } else if (outcome.outcome === "connected") {
        onstatuschange?.();
      }
    } catch (e) {
      startError = e instanceof Error ? e.message : "Connection failed.";
    } finally {
      starting = false;
    }
  }
</script>

<div class="connection-card">
  <div class="card-header">
    <div class="provider-icon">{providerLabel.charAt(0)}</div>
    <div class="provider-info">
      <span class="provider-name">{providerLabel}</span>
      <span class="provider-desc">{providerDescription}</span>
    </div>
    <span class="status-badge {statusInfo.className}">{statusInfo.label}</span>
  </div>

  {#if connection.status === "connected"}
    <div class="card-details">
      {#if connection.account_label}
        <div class="detail-row">
          <span class="detail-label">Account</span>
          <span class="detail-value">{connection.account_label}</span>
        </div>
      {/if}
      {#if connection.capabilities.length > 0}
        <div class="detail-row">
          <span class="detail-label">Capabilities</span>
          <span class="detail-value caps"
            >{connection.capabilities.join(", ")}</span
          >
        </div>
      {/if}
      {#if connection.updated_at}
        <div class="detail-row">
          <span class="detail-label">Last verified</span>
          <span class="detail-value"
            >{new Date(connection.updated_at).toLocaleDateString()}</span
          >
        </div>
      {/if}
    </div>
  {:else if connection.status === "action_needed"}
    <div class="card-details">
      {#if connection.account_label}
        <div class="detail-row">
          <span class="detail-label">Account</span>
          <span class="detail-value">{connection.account_label}</span>
        </div>
      {/if}
      <div class="action-hint">
        Your connection needs attention. Reconnect to restore access.
      </div>
    </div>
  {:else if isUnsupported}
    <div class="card-details">
      <div class="action-hint">
        {providerLabel} is not yet available as a live connection.
      </div>
    </div>
  {:else}
    <div class="card-details">
      <div class="action-hint">
        Connect your {providerLabel} account to use it with Bicameral.
      </div>
    </div>
  {/if}

  {#if startOutcome}
    <div class="outcome-panel">
      {#if startOutcome.outcome === "connected"}
        <div class="outcome-msg outcome-ok">
          Connected{startOutcome.account_label ? ` as ${startOutcome.account_label}` : ""}.
        </div>
      {:else if startOutcome.outcome === "start_url"}
        <div class="outcome-msg outcome-info">
          <a href={startOutcome.url} target="_blank" rel="noopener noreferrer"
            >Open {providerLabel} authorization</a
          >
          {#if startOutcome.user_code}
            <span class="user-code">Code: {startOutcome.user_code}</span>
          {/if}
          {#if polling}
            <span class="poll-indicator">Waiting for authorization...</span>
          {/if}
        </div>
      {:else if startOutcome.outcome === "action_needed"}
        <div class="outcome-msg outcome-warn">{startOutcome.message}</div>
      {:else if startOutcome.outcome === "unsupported"}
        <div class="outcome-msg outcome-err">{startOutcome.message}</div>
      {/if}
    </div>
  {/if}

  {#if completionOutcome}
    <div class="outcome-panel">
      {#if completionOutcome.outcome === "connected"}
        <div class="outcome-msg outcome-ok">
          Connected{completionOutcome.account_label ? ` as ${completionOutcome.account_label}` : ""}.
        </div>
      {:else if completionOutcome.outcome === "action_needed"}
        <div class="outcome-msg outcome-warn">{completionOutcome.message}</div>
      {:else if completionOutcome.outcome === "denied"}
        <div class="outcome-msg outcome-err">{completionOutcome.message}</div>
      {:else if completionOutcome.outcome === "expired"}
        <div class="outcome-msg outcome-err">{completionOutcome.message}</div>
      {:else if completionOutcome.outcome === "provider_unavailable"}
        <div class="outcome-msg outcome-err">{completionOutcome.message}</div>
      {:else if completionOutcome.outcome === "pending"}
        <div class="outcome-msg outcome-info">
          {completionOutcome.message ?? "Waiting for authorization..."}
        </div>
      {/if}
    </div>
  {/if}

  {#if startError}
    <div class="outcome-panel">
      <div class="outcome-msg outcome-err">{startError}</div>
    </div>
  {/if}

  {#if !isUnsupported}
    <div class="card-footer">
      <button
        class="card-action"
        disabled={starting || polling}
        onclick={handleStartConnection}
      >
        {#if starting}
          Starting…
        {:else if polling}
          Polling…
        {:else}
          {statusInfo.action}
        {/if}
      </button>
      {#if connection.auth_method}
        <span class="auth-method">via {connection.auth_method}</span>
      {/if}
    </div>
  {/if}
</div>

<style>
  .connection-card {
    border: 1px solid var(--line);
    background: #fff;
    padding: 1rem 1.25rem;
    margin-bottom: 0.75rem;
  }

  .card-header {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-bottom: 0.75rem;
  }

  .provider-icon {
    width: 2rem;
    height: 2rem;
    border-radius: 6px;
    background: var(--canvas-muted);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 0.875rem;
    font-weight: 700;
    color: var(--ink-700);
    flex-shrink: 0;
  }

  .provider-info {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
  }

  .provider-name {
    font-size: 0.875rem;
    font-weight: 600;
    color: var(--ink-900);
  }

  .provider-desc {
    font-size: 0.75rem;
    color: var(--ink-500);
  }

  .status-badge {
    font-size: 0.75rem;
    font-weight: 500;
    padding: 0.2rem 0.5rem;
    border-radius: 9999px;
    flex-shrink: 0;
  }

  .status-connected {
    background: var(--ok-50);
    color: var(--ok-600);
  }

  .status-action-needed {
    background: var(--warn-50);
    color: var(--warn-600);
  }

  .status-not-connected {
    background: var(--canvas-muted);
    color: var(--ink-500);
  }

  .status-unsupported {
    background: var(--canvas-muted);
    color: var(--ink-400);
  }

  .card-details {
    padding: 0.5rem 0;
    border-top: 1px solid var(--canvas-muted);
  }

  .detail-row {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    padding: 0.25rem 0;
    font-size: 0.8125rem;
  }

  .detail-label {
    color: var(--ink-500);
  }

  .detail-value {
    color: var(--ink-900);
  }

  .detail-value.caps {
    font-family: "JetBrains Mono", monospace;
    font-size: 0.75rem;
  }

  .action-hint {
    font-size: 0.8125rem;
    color: var(--ink-500);
    padding: 0.25rem 0;
  }

  .card-footer {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding-top: 0.75rem;
    border-top: 1px solid var(--canvas-muted);
  }

  .card-action {
    padding: 0.375rem 0.875rem;
    border: 1px solid var(--line-strong);
    background: #fff;
    color: var(--ink-700);
    font-size: 0.8125rem;
    font-family: inherit;
    cursor: pointer;
  }

  .card-action:disabled {
    cursor: not-allowed;
    opacity: 0.6;
  }

  .card-action:not(:disabled):hover {
    background: var(--canvas-muted);
  }

  .auth-method {
    font-size: 0.75rem;
    color: var(--ink-400);
  }

  .outcome-panel {
    padding: 0.5rem 0;
    border-top: 1px solid var(--canvas-muted);
  }

  .outcome-msg {
    font-size: 0.8125rem;
    padding: 0.375rem 0.5rem;
    border-radius: 4px;
  }

  .outcome-ok {
    background: var(--ok-50);
    color: var(--ok-600);
  }

  .outcome-info {
    background: var(--accent-50);
    color: var(--accent-700);
  }

  .outcome-warn {
    background: var(--warn-50);
    color: var(--warn-600);
  }

  .outcome-err {
    background: var(--danger-50);
    color: var(--danger-600);
  }

  .user-code {
    display: inline-block;
    margin-left: 0.5rem;
    font-family: "JetBrains Mono", monospace;
    font-size: 0.75rem;
    background: var(--canvas-muted);
    padding: 0.1rem 0.375rem;
    border-radius: 3px;
  }

  .poll-indicator {
    display: block;
    margin-top: 0.375rem;
    font-size: 0.75rem;
    color: var(--ink-500);
    font-style: italic;
  }
</style>
