<script lang="ts">
  import {
    fetchConfig,
    type WorkspaceConfig,
  } from "../lib/api";
  import type { SourceBinding } from "../lib/connection-fixtures";

  type SettingsTab = "project" | "sources" | "channels";

  let activeTab = $state<SettingsTab>("project");

  let config = $state<WorkspaceConfig | null>(null);
  let loading = $state(true);
  let loadError = $state<string | null>(null);

  async function load() {
    loading = true;
    loadError = null;
    try {
      const res = await fetchConfig();
      if (res.config) {
        config = res.config;
      } else if (res.errors.length > 0) {
        loadError = res.errors.join("; ");
      }
    } catch (e: unknown) {
      loadError = e instanceof Error ? e.message : "Failed to load config";
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    load();
  });

  const projectName = $derived(config?.workspace_name ?? "Untitled project");

  const eventStoreLabel = $derived(
    config?.event_store?.substrate === "drive_folder"
      ? "Google Drive"
      : config?.event_store?.substrate === "git"
        ? "Git (repository-backed)"
        : config?.event_store?.substrate ?? "Not configured",
  );

  const needsAccessRepair = $derived(
    !config?.event_store?.substrate,
  );

  const sourceBindings: SourceBinding[] = [];

  function statusClass(status: string): string {
    switch (status) {
      case "healthy":
        return "badge-healthy";
      case "action_needed":
        return "badge-warning";
      case "paused":
        return "badge-paused";
      case "revoked":
        return "badge-revoked";
      default:
        return "";
    }
  }

  function statusLabel(status: string): string {
    switch (status) {
      case "healthy":
        return "Healthy";
      case "action_needed":
        return "Action needed";
      case "paused":
        return "Paused";
      case "revoked":
        return "Revoked";
      default:
        return status;
    }
  }

</script>

<div class="project-settings">
  <h1>Project Settings</h1>

  <nav class="tab-bar">
    <button
      class:active={activeTab === "project"}
      onclick={() => (activeTab = "project")}
    >
      Project
    </button>
    <button
      class:active={activeTab === "sources"}
      onclick={() => (activeTab = "sources")}
    >
      Sources
    </button>
    <button
      class:active={activeTab === "channels"}
      onclick={() => (activeTab = "channels")}
    >
      Channels
    </button>
  </nav>

  {#if loading}
    <p class="loading">Loading project settings...</p>
  {:else if loadError}
    <div class="error-box">
      <p>{loadError}</p>
    </div>
  {:else}
    {#if activeTab === "project"}
      <section class="settings-section">
        <h2>Identity</h2>
        <dl class="info-grid">
          <dt>Project name</dt>
          <dd>{projectName}</dd>
          <dt>Event store</dt>
          <dd>{eventStoreLabel}</dd>
        </dl>

        {#if needsAccessRepair}
          <div class="repair-box">
            <p>Local access needs configuration.</p>
            <button class="repair-btn">Configure event store</button>
          </div>
        {/if}
      </section>

    {:else if activeTab === "sources"}
      <section class="settings-section">
        <h2>Approved Sources</h2>
        {#if sourceBindings.length === 0}
          <p class="empty-state">No source bindings configured.</p>
        {:else}
          <div class="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>Resource</th>
                  <th>Ingestion owner</th>
                  <th>Approver</th>
                  <th>Status</th>
                  <th>Dedupe identity</th>
                </tr>
              </thead>
              <tbody>
                {#each sourceBindings as binding (binding.id)}
                  <tr>
                    <td>
                      <span class="resource-label">{binding.resource.display_label}</span>
                      <span class="resource-kind">{binding.resource.kind}</span>
                    </td>
                    <td>{binding.ingestion_owner.operator_id}</td>
                    <td>{binding.approved_by}</td>
                    <td>
                      <span class="status-badge {statusClass(binding.status)}">
                        {statusLabel(binding.status)}
                      </span>
                    </td>
                    <td><code>{binding.dedupe_key}</code></td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}
      </section>

    {:else if activeTab === "channels"}
      <section class="settings-section">
        <h2>Channels</h2>
        <p class="empty-state">
          Channel route configuration is not available in this release.
        </p>
        <p class="settings-note">
          Channels can be browsed in the connector catalog, but durable project
          routing is deferred until owner workflow, storage, authority, edit
          lifecycle, and recovery behavior are defined.
        </p>
      </section>
    {/if}
  {/if}
</div>

<style>
  .project-settings {
    max-width: 800px;
  }

  h1 {
    font-size: 1.125rem;
    font-weight: 600;
    margin-bottom: 1rem;
    color: var(--ink-900);
  }

  h2 {
    font-size: 0.875rem;
    font-weight: 600;
    margin-bottom: 0.75rem;
    color: var(--ink-700);
  }

  .tab-bar {
    display: flex;
    gap: 0;
    border-bottom: 1px solid var(--line);
    margin-bottom: 1.25rem;
  }

  .tab-bar button {
    padding: 0.5rem 1rem;
    border: none;
    background: none;
    color: var(--ink-500);
    font-size: 0.8125rem;
    font-family: inherit;
    cursor: pointer;
    border-bottom: 2px solid transparent;
    transition: all 0.12s;
  }

  .tab-bar button:hover {
    color: var(--ink-900);
  }

  .tab-bar button.active {
    color: var(--accent-500);
    border-bottom-color: var(--accent-500);
    font-weight: 600;
  }

  .settings-section {
    padding: 1rem 1.25rem;
    border: 1px solid var(--line);
    background: #fff;
  }

  .info-grid {
    display: grid;
    grid-template-columns: 140px 1fr;
    gap: 0.5rem 1rem;
    font-size: 0.8125rem;
  }

  .info-grid dt {
    color: var(--ink-500);
    font-weight: 500;
  }

  .info-grid dd {
    color: var(--ink-900);
  }

  .repair-box {
    margin-top: 1rem;
    padding: 0.75rem 1rem;
    background: var(--warn-50);
    border: 1px solid #fde68a;
    font-size: 0.8125rem;
    color: var(--warn-600);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
  }

  .repair-btn {
    padding: 0.375rem 0.75rem;
    background: var(--warn-500);
    color: #fff;
    border: none;
    font-size: 0.8125rem;
    font-family: inherit;
    cursor: pointer;
    white-space: nowrap;
    transition: background 0.12s;
  }

  .repair-btn:hover {
    background: var(--warn-600);
  }

  .table-wrap {
    overflow-x: auto;
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.8125rem;
  }

  th {
    text-align: left;
    padding: 0.5rem 0.75rem;
    font-weight: 600;
    color: var(--ink-500);
    border-bottom: 1px solid var(--line);
    white-space: nowrap;
  }

  td {
    padding: 0.5rem 0.75rem;
    color: var(--ink-900);
    border-bottom: 1px solid var(--canvas-muted);
    vertical-align: top;
  }

  .resource-label {
    font-weight: 500;
  }

  .resource-kind {
    display: inline-block;
    margin-left: 0.375rem;
    padding: 0.125rem 0.375rem;
    font-size: 0.6875rem;
    background: var(--canvas-muted);
    color: var(--ink-500);
    border-radius: 2px;
  }

  code {
    font-size: 0.75rem;
    padding: 0.125rem 0.375rem;
    background: var(--canvas-muted);
    color: var(--ink-700);
  }

  .status-badge {
    display: inline-block;
    padding: 0.125rem 0.5rem;
    font-size: 0.75rem;
    font-weight: 500;
    border-radius: 2px;
  }

  .badge-healthy {
    background: var(--ok-50);
    color: var(--ok-600);
  }

  .badge-warning {
    background: var(--warn-50);
    color: var(--warn-600);
  }

  .badge-paused {
    background: var(--canvas-muted);
    color: var(--ink-500);
  }

  .badge-revoked {
    background: var(--danger-50);
    color: var(--danger-600);
  }

  .loading {
    color: var(--ink-500);
    font-size: 0.875rem;
  }

  .error-box {
    margin-top: 1rem;
    padding: 0.75rem 1rem;
    background: var(--danger-50);
    border: 1px solid #fecaca;
    color: var(--danger-600);
    font-size: 0.8125rem;
  }

  .empty-state {
    color: var(--ink-500);
    font-size: 0.8125rem;
    padding: 1rem 0;
  }

  .settings-note {
    color: var(--ink-700);
    font-size: 0.8125rem;
    line-height: 1.5;
    max-width: 42rem;
  }
</style>
