<script lang="ts">
  import type { ProjectEntry } from "../lib/project-fixtures";
  import {
    ACCESS_FAILURE_COPY,
    ACCESS_FAILURE_ACTION,
  } from "../lib/project-fixtures";

  type Props = {
    project: ProjectEntry;
    onback?: () => void;
    onconnect?: (providerId: string) => void;
  };

  let { project, onback, onconnect }: Props = $props();

  const failureReason = $derived(project.access_failure ?? "missing_connection");
  const failureCopy = $derived(ACCESS_FAILURE_COPY[failureReason]);
  const actionLabel = $derived(ACCESS_FAILURE_ACTION[failureReason]);

  const providerLabel = $derived(
    project.provider_id === "github"
      ? "GitHub"
      : project.provider_id === "google"
        ? "Google Drive"
        : project.provider_id ?? "provider",
  );

  function handleAction() {
    if (project.provider_id) {
      onconnect?.(project.provider_id);
    }
  }
</script>

<div class="repair-page">
  <button class="back-link" onclick={onback}>
    ← Back to dashboard
  </button>

  <div class="repair-card">
    <h2 class="repair-title">
      {project.name} needs {providerLabel} access
    </h2>

    <div class="repair-detail">
      <div class="detail-row">
        <span class="detail-label">Status</span>
        <span class="detail-value status-warning">{failureCopy}</span>
      </div>
      <div class="detail-row">
        <span class="detail-label">Event store</span>
        <span class="detail-value">{project.event_store_label}</span>
      </div>
      {#if failureReason === "missing_connection"}
        <p class="repair-explanation">
          {providerLabel} not connected. Connect your account to access this project.
        </p>
      {:else if failureReason === "missing_resource_selection"}
        <p class="repair-explanation">
          The project resource has not been configured. Select the repository or folder this project uses.
        </p>
      {:else if failureReason === "insufficient_account_access"}
        <p class="repair-explanation">
          Connected account lacks access to the project resource. Use another account with the required permissions.
        </p>
      {:else if failureReason === "token_expired"}
        <p class="repair-explanation">
          Your connection token has expired. Reconnect to restore access.
        </p>
      {:else if failureReason === "resource_moved_or_deleted"}
        <p class="repair-explanation">
          The project resource has been moved or deleted. Update the project settings to point to the new location.
        </p>
      {/if}
    </div>

    <div class="repair-actions">
      <button class="primary-action" onclick={handleAction}>
        {#if failureReason === "missing_connection"}
          Connect {providerLabel}
        {:else if failureReason === "insufficient_account_access"}
          Use another account
        {:else}
          {actionLabel}
        {/if}
      </button>
    </div>
  </div>
</div>

<style>
  .repair-page {
    max-width: 480px;
    margin: 0 auto;
    padding: 2rem 1.25rem;
  }

  .back-link {
    display: inline-block;
    margin-bottom: 1.5rem;
    padding: 0;
    border: none;
    background: none;
    font-family: inherit;
    font-size: 0.8125rem;
    color: var(--ink-500);
    cursor: pointer;
    transition: color 0.12s;
  }

  .back-link:hover {
    color: var(--ink-900);
  }

  .repair-card {
    background: #fff;
    border: 1px solid var(--line);
    border-radius: 8px;
    padding: 1.5rem;
  }

  .repair-title {
    font-size: 1rem;
    font-weight: 600;
    color: var(--ink-900);
    margin-bottom: 1rem;
  }

  .repair-detail {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    margin-bottom: 1rem;
  }

  .detail-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .detail-label {
    font-size: 0.8125rem;
    color: var(--ink-500);
  }

  .detail-value {
    font-size: 0.8125rem;
    font-weight: 500;
    color: var(--ink-900);
  }

  .status-warning {
    color: var(--warn-600);
  }

  .repair-explanation {
    font-size: 0.8125rem;
    color: var(--ink-700);
    line-height: 1.5;
    margin-top: 0.5rem;
  }

  .repair-actions {
    margin-top: 1.25rem;
  }

  .primary-action {
    display: inline-flex;
    align-items: center;
    padding: 0.5rem 1rem;
    border: none;
    border-radius: 6px;
    background: var(--accent-500);
    color: #fff;
    font-family: inherit;
    font-size: 0.8125rem;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.12s;
  }

  .primary-action:hover {
    background: var(--accent-600);
  }
</style>
