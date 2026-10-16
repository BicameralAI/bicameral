<script lang="ts">
  import {
    fetchConfig,
    fetchConnectionRegistry,
    updateConfig,
    type WorkspaceConfig,
    type UpdateConfigResponse,
  } from "../lib/api";
  import type { Connection, SourceBinding } from "../lib/connection-fixtures";
  import type { ProjectionSetupState } from "../lib/types";

  let config = $state<WorkspaceConfig | null>(null);
  let loading = $state(true);
  let saving = $state(false);
  let loadError = $state<string | null>(null);
  let saveResult = $state<UpdateConfigResponse | null>(null);

  // Editable fields
  let workspaceName = $state("");
  let gatewayHost = $state("127.0.0.1");
  let gatewayPort = $state(37373);
  let substrate = $state("git");
  let gitPath = $state("");
  let driveFolderPath = $state("");
  let ciMergeGate = $state(false);
  let connections = $state<Connection[]>([]);
  let sourceBindings = $state<SourceBinding[]>([]);

  function populateFrom(cfg: WorkspaceConfig) {
    workspaceName = cfg.workspace_name ?? "";
    gatewayHost = cfg.gateway?.host ?? "127.0.0.1";
    gatewayPort = cfg.gateway?.port ?? 37373;
    substrate = cfg.event_store?.substrate ?? "git";
    gitPath = cfg.event_store?.git_path ?? "";
    driveFolderPath = cfg.event_store?.drive_folder_path ?? "";
    ciMergeGate = cfg.event_store?.ci_merge_gate ?? false;
  }

  async function load() {
    loading = true;
    loadError = null;
    try {
      const res = await fetchConfig();
      if (res.config) {
        config = res.config;
        populateFrom(res.config);
      } else if (res.errors.length > 0) {
        loadError = res.errors.join("; ");
      }
    } catch (e: unknown) {
      loadError = e instanceof Error ? e.message : "Failed load config";
    }

    try {
      const registry = await fetchConnectionRegistry();
      connections = Object.entries(registry.connections).map(([provider_id, meta]) => ({
        provider_id,
        status: meta.status,
        account_label: meta.account_label,
        capabilities: meta.capabilities,
        updated_at: meta.updated_at,
        auth_method: meta.auth_method,
      }));
    } catch {
      connections = [];
    }

    sourceBindings = [];
    loading = false;
  }

  async function handleSave() {
    saving = true;
    saveResult = null;
    try {
      const res = await updateConfig({
        workspace_name: workspaceName || undefined,
        gateway_host: gatewayHost,
        gateway_port: gatewayPort,
        substrate,
        git_path: gitPath || undefined,
        drive_folder_path: driveFolderPath || undefined,
        ci_merge_gate: ciMergeGate,
      });
      saveResult = res;
      if (res.success) {
        await load();
      }
    } catch (e: unknown) {
      saveResult = {
        success: false,
        errors: [e instanceof Error ? e.message : "Unknown error"],
      };
    } finally {
      saving = false;
    }
  }

  // Load on mount
  $effect(() => {
    load();
  });

  const isLoopback = $derived(
    gatewayHost === "127.0.0.1" ||
      gatewayHost === "::1" ||
      gatewayHost === "localhost",
  );

  // ── Connection helpers ─────────────────────────────────────────

  function providerDisplayName(id: string): string {
    switch (id) {
      case "github": return "GitHub";
      case "google": return "Google Drive";
      default: return id;
    }
  }

  function statusLabel(status: string): string {
    switch (status) {
      case "connected": return "Connected";
      case "action_needed": return "Action needed";
      case "not_connected": return "Not connected";
      default: return status;
    }
  }

  function connectLabel(id: string): string {
    return `Connect ${providerDisplayName(id)}`;
  }

  function formatConnectionDate(iso?: string): string {
    if (!iso) return "";
    try {
      return new Date(iso).toLocaleDateString(undefined, {
        year: "numeric",
        month: "short",
        day: "numeric",
      });
    } catch {
      return iso;
    }
  }

  // ── Projection setup helpers ────────────────────────────────────

  function projectionStateLabel(state: ProjectionSetupState): string {
    switch (state) {
      case "configured": return "Configured";
      case "projection_deferred": return "Deferred";
      case "provider_not_configured": return "Not configured";
      case "provider_unavailable": return "Unavailable";
    }
  }

  function projectionStateClassName(state: ProjectionSetupState): string {
    switch (state) {
      case "configured": return "proj-configured";
      case "projection_deferred": return "proj-deferred";
      case "provider_not_configured": return "proj-not-configured";
      case "provider_unavailable": return "proj-unavailable";
    }
  }
</script>

<div class="settings-page">
  <h1>Settings</h1>

  <!-- ── Connections section ──────────────────────────────────── -->
  <section>
    <h2>Connections</h2>
    <div class="connections-grid">
      {#each connections as conn}
        <div class="connection-card" class:connected={conn.status === "connected"}
          class:action-needed={conn.status === "action_needed"}
          class:disconnected={conn.status === "not_connected"}>
          <div class="card-header">
            <span class="provider-name">{providerDisplayName(conn.provider_id)}</span>
            <span class="status-badge"
              class:status-connected={conn.status === "connected"}
              class:status-action={conn.status === "action_needed"}
              class:status-disconnected={conn.status === "not_connected"}>
              {statusLabel(conn.status)}
            </span>
          </div>
          {#if conn.account_label}
            <span class="account-label">{conn.account_label}</span>
          {/if}
          {#if conn.updated_at}
            <span class="updated-at">{formatConnectionDate(conn.updated_at)}</span>
          {/if}
          <div class="card-actions">
            {#if conn.status === "connected"}
              <button class="link-btn">Reconnect</button>
              <button class="link-btn danger">Disconnect</button>
            {:else}
              <button class="connect-btn">{connectLabel(conn.provider_id)}</button>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  </section>

  <!-- ── Sources / Projection Setup section ───────────────────── -->
  <section>
    <h2>Sources / Projection</h2>
    <div class="sources-list">
      {#each sourceBindings as binding}
        <div class="source-row">
          <div class="source-header">
            <span class="source-label">{binding.resource.display_label}</span>
            <span class="source-mode">{binding.mode}</span>
          </div>
          {#if binding.projection_setup}
            <div class="projection-state">
              <span class="projection-badge {projectionStateClassName(binding.projection_setup.state)}">
                {projectionStateLabel(binding.projection_setup.state)}
              </span>
              <span class="projection-message">{binding.projection_setup.message}</span>
            </div>
            {#if binding.projection_setup.repair_hint}
              <div class="projection-repair-hint">
                {binding.projection_setup.repair_hint}
              </div>
            {/if}
          {/if}
        </div>
      {/each}
    </div>
  </section>

  <!-- ── Gateway / Workspace section ─────────────────────────── -->
  <h2 class="section-divider">Gateway / Workspace</h2>

  {#if loading}
    <p class="loading">Loading configuration...</p>
  {:else if loadError}
    <div class="error-box">
      <p>{loadError}</p>
    </div>
  {:else}
    <form
      onsubmit={(e) => {
        e.preventDefault();
        handleSave();
      }}
    >
      <section>
        <h2>Workspace</h2>
        <label class="field">
          <span>Workspace name</span>
          <input type="text" bind:value={workspaceName} placeholder="my-team" />
        </label>
      </section>

      <section>
        <h2>Gateway</h2>
        <div class="field-row">
          <label class="field">
            <span>Host</span>
            <input type="text" bind:value={gatewayHost} />
          </label>
          <label class="field field-narrow">
            <span>Port</span>
            <input type="number" bind:value={gatewayPort} min="1" max="65535" />
          </label>
        </div>
        {#if !isLoopback}
          <div class="warning-box">
            Non-loopback host detected. v0.1 has no authentication — only
            loopback addresses (127.0.0.1, ::1, localhost) are safe for
            unauthenticated use.
          </div>
        {/if}
      </section>

      <section>
        <h2>Event Store</h2>
        <label class="field">
          <span>Substrate</span>
          <select bind:value={substrate}>
            <option value="git">Git (repository-backed)</option>
            <option value="drive_folder">Drive / basic-store (synced folder)</option
            >
          </select>
        </label>

        {#if substrate === "git"}
          <label class="field">
            <span>Git path (relative to workspace root)</span>
            <input
              type="text"
              bind:value={gitPath}
              placeholder=".bicameral/events"
            />
          </label>
          <label class="field-checkbox">
            <input type="checkbox" bind:checked={ciMergeGate} />
            <span>CI merge-gate enforcement</span>
          </label>
        {/if}

        {#if substrate === "drive_folder"}
          <label class="field">
            <span>Drive folder path</span>
            <input
              type="text"
              bind:value={driveFolderPath}
              placeholder="/path/to/shared/folder"
            />
          </label>
        {/if}
      </section>

      <div class="form-actions">
        <button type="submit" class="primary" disabled={saving}>
          {saving ? "Saving..." : "Save settings"}
        </button>
      </div>
    </form>

    {#if saveResult && !saveResult.success}
      <div class="error-box">
        {#each saveResult.errors as err}
          <p>{err}</p>
        {/each}
      </div>
    {/if}

    {#if saveResult?.success}
      <div class="success-box">
        <p>Settings saved.</p>
      </div>
    {/if}
  {/if}
</div>

<style>
  .settings-page {
    max-width: 640px;
  }

  h1 {
    font-size: 1.125rem;
    font-weight: 600;
    margin-bottom: 1.5rem;
    color: var(--ink-900);
  }

  /* ── Connections ────────────────────────────────────────────── */

  .connections-grid {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .connection-card {
    padding: 0.75rem 1rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: #fff;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .card-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .provider-name {
    font-size: 0.875rem;
    font-weight: 600;
    color: var(--ink-900);
  }

  .status-badge {
    font-size: 0.75rem;
    font-weight: 500;
    padding: 0.1rem 0.5rem;
    border-radius: 9999px;
  }

  .status-connected {
    color: var(--ok-600);
    background: var(--ok-50);
  }

  .status-action {
    color: var(--warn-600);
    background: var(--warn-50);
  }

  .status-disconnected {
    color: var(--ink-500);
    background: var(--canvas-muted);
  }

  .account-label {
    font-size: 0.8rem;
    color: var(--ink-500);
  }

  .updated-at {
    font-size: 0.75rem;
    color: var(--ink-400);
  }

  .card-actions {
    display: flex;
    gap: 0.75rem;
    margin-top: 0.25rem;
  }

  .connect-btn {
    padding: 0.35rem 0.75rem;
    background: var(--accent-500);
    color: #fff;
    border: none;
    border-radius: 4px;
    font-size: 0.8rem;
    font-family: inherit;
    cursor: pointer;
    transition: background 0.12s;
  }

  .connect-btn:hover {
    background: var(--accent-600);
  }

  .link-btn {
    background: none;
    border: none;
    color: var(--accent-500);
    font-size: 0.8rem;
    font-family: inherit;
    cursor: pointer;
    padding: 0;
  }

  .link-btn:hover {
    text-decoration: underline;
  }

  .link-btn.danger {
    color: var(--danger-600);
  }

  .section-divider {
    font-size: 0.875rem;
    font-weight: 600;
    margin-top: 1.5rem;
    margin-bottom: 0.75rem;
    color: var(--ink-700);
  }

  h2 {
    font-size: 0.875rem;
    font-weight: 600;
    margin-bottom: 0.75rem;
    color: var(--ink-700);
  }

  section {
    margin-bottom: 1.5rem;
    padding: 1rem 1.25rem;
    border: 1px solid var(--line);
    background: #fff;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    margin-bottom: 0.75rem;
  }

  .field span {
    font-size: 0.8125rem;
    color: var(--ink-500);
  }

  .field-row {
    display: flex;
    gap: 1rem;
  }

  .field-narrow {
    max-width: 120px;
  }

  input[type="text"],
  input[type="number"],
  select {
    padding: 0.5rem 0.75rem;
    border: 1px solid var(--line-strong);
    font-size: 0.8125rem;
    font-family: inherit;
    background: #fff;
  }

  input:focus,
  select:focus {
    outline: none;
    border-color: var(--accent-500);
    box-shadow: 0 0 0 2px rgba(8, 145, 178, 0.1);
  }

  .field-checkbox {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 0.75rem;
    font-size: 0.8125rem;
    color: var(--ink-700);
  }

  .form-actions {
    margin-top: 0.5rem;
  }

  button.primary {
    padding: 0.5rem 1.25rem;
    background: var(--accent-500);
    color: #fff;
    border: none;
    font-size: 0.875rem;
    font-family: inherit;
    cursor: pointer;
    transition: background 0.12s;
  }

  button.primary:hover:not(:disabled) {
    background: var(--accent-600);
  }

  button.primary:disabled {
    opacity: 0.6;
    cursor: not-allowed;
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

  .success-box {
    margin-top: 1rem;
    padding: 0.75rem 1rem;
    background: var(--ok-50);
    border: 1px solid #bbf7d0;
    color: var(--ok-600);
    font-size: 0.8125rem;
  }

  .warning-box {
    padding: 0.75rem 1rem;
    background: var(--warn-50);
    border: 1px solid #fde68a;
    color: var(--warn-600);
    font-size: 0.8125rem;
    margin-bottom: 0.75rem;
  }

  /* ── Sources / Projection ───────────────────────────────────── */

  .sources-list {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .source-row {
    padding: 0.75rem 1rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--canvas-muted);
  }

  .source-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 0.375rem;
  }

  .source-label {
    font-size: 0.8125rem;
    font-weight: 600;
    color: var(--ink-900);
  }

  .source-mode {
    font-size: 0.75rem;
    color: var(--ink-500);
    text-transform: capitalize;
  }

  .projection-state {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-top: 0.25rem;
  }

  .projection-badge {
    font-size: 0.6875rem;
    font-weight: 500;
    padding: 0.1rem 0.5rem;
    border-radius: 9999px;
    white-space: nowrap;
  }

  .proj-configured {
    color: var(--ok-600);
    background: var(--ok-50);
  }

  .proj-deferred {
    color: var(--warn-600);
    background: var(--warn-50);
  }

  .proj-not-configured {
    color: var(--ink-500);
    background: var(--canvas-muted);
  }

  .proj-unavailable {
    color: var(--danger-600);
    background: var(--danger-50);
  }

  .projection-message {
    font-size: 0.75rem;
    color: var(--ink-700);
  }

  .projection-repair-hint {
    font-size: 0.75rem;
    color: var(--warn-600);
    background: var(--warn-50);
    border: 1px solid #fde68a;
    padding: 0.375rem 0.625rem;
    border-radius: 4px;
    margin-top: 0.375rem;
  }
</style>
