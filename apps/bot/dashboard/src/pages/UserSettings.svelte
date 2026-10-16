<script lang="ts">
  import ConnectionCard from "../lib/ConnectionCard.svelte";
  import type { Connection } from "../lib/connection-fixtures";
  import { fetchConnectionRegistry } from "../lib/api";
  import type { ConnectionMetadata, ConnectionStatus } from "../lib/types";

  type Props = {
    onclose: () => void;
  };

  let { onclose }: Props = $props();

  type Tab = "connections";

  let activeTab = $state<Tab>("connections");
  let connections = $state<Connection[]>([]);
  let loading = $state(true);
  let loadError = $state<string | null>(null);

  /** Always surface alpha event-store + common providers even if registry omits them. */
  const CATALOG_PLACEHOLDERS: Connection[] = [
    {
      provider_id: "google",
      status: "not_connected" as ConnectionStatus,
      capabilities: [],
    },
    {
      provider_id: "github",
      status: "not_connected" as ConnectionStatus,
      capabilities: [],
    },
    {
      provider_id: "linear",
      status: "unsupported" as ConnectionStatus,
      capabilities: [],
    },
  ];

  function metadataToConnection(
    providerId: string,
    meta: ConnectionMetadata,
  ): Connection {
    return {
      provider_id: providerId,
      status: meta.status,
      account_label: meta.account_label,
      capabilities: meta.capabilities,
      updated_at: meta.updated_at,
      auth_method: meta.auth_method,
    };
  }

  function mergeCatalog(live: Connection[]): Connection[] {
    const byId = new Map(live.map((c) => [c.provider_id, c]));
    for (const placeholder of CATALOG_PLACEHOLDERS) {
      if (!byId.has(placeholder.provider_id)) {
        byId.set(placeholder.provider_id, placeholder);
      }
    }
    // Alpha substrate first
    const order = ["google", "github", "linear"];
    const ordered: Connection[] = [];
    for (const id of order) {
      const row = byId.get(id);
      if (row) {
        ordered.push(row);
        byId.delete(id);
      }
    }
    for (const rest of byId.values()) ordered.push(rest);
    return ordered;
  }

  async function loadConnections() {
    loading = true;
    loadError = null;
    try {
      const registry = await fetchConnectionRegistry();
      const live: Connection[] = Object.entries(
        registry.connections,
      ).map(([id, meta]) => metadataToConnection(id, meta));
      connections = mergeCatalog(live);
    } catch {
      connections = mergeCatalog([]);
      loadError = "Could not load live connection status.";
    } finally {
      loading = false;
    }
  }

  function handleStatusChange() {
    loadConnections();
  }

  function handleBackdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) {
      onclose();
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      onclose();
    }
  }

  $effect(() => {
    loadConnections();
  });
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="modal-backdrop" onclick={handleBackdropClick}>
  <div class="modal-panel" role="dialog" aria-label="User Settings">
    <div class="modal-header">
      <h1>User Settings</h1>
      <button class="close-btn" onclick={onclose} aria-label="Close">✕</button>
    </div>

    <div class="modal-body">
      <nav class="tab-nav">
        <button
          class:active={activeTab === "connections"}
          onclick={() => (activeTab = "connections")}
        >
          Connections
        </button>
      </nav>

      <div class="tab-content">
        {#if activeTab === "connections"}
          <div class="connections-panel">
            <p class="panel-desc">
              Manage provider connections for your Bicameral account.
              Connections are user-level and shared across all projects.
            </p>
            {#if loadError}
              <div class="load-warning">{loadError}</div>
            {/if}
            {#if loading}
              <p class="loading-text">Loading connections...</p>
            {:else}
              {#each connections as conn (conn.provider_id)}
                <ConnectionCard
                  connection={conn}
                  onstatuschange={handleStatusChange}
                />
              {/each}
            {/if}
          </div>
        {/if}
      </div>
    </div>
  </div>
</div>

<style>
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.3);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .modal-panel {
    background: #fff;
    width: 560px;
    max-width: 90vw;
    max-height: 85vh;
    display: flex;
    flex-direction: column;
    border: 1px solid var(--line);
    box-shadow: 0 8px 30px rgba(0, 0, 0, 0.12);
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 1rem 1.25rem;
    border-bottom: 1px solid var(--line);
  }

  .modal-header h1 {
    font-size: 1rem;
    font-weight: 600;
    color: var(--ink-900);
  }

  .close-btn {
    background: none;
    border: none;
    font-size: 1rem;
    color: var(--ink-500);
    cursor: pointer;
    padding: 0.25rem;
    line-height: 1;
  }

  .close-btn:hover {
    color: var(--ink-900);
  }

  .modal-body {
    flex: 1;
    overflow-y: auto;
    padding: 0;
  }

  .tab-nav {
    display: flex;
    gap: 0;
    padding: 0 1.25rem;
    border-bottom: 1px solid var(--line);
  }

  .tab-nav button {
    padding: 0.625rem 1rem;
    border: none;
    background: none;
    color: var(--ink-500);
    font-size: 0.8125rem;
    font-family: inherit;
    cursor: pointer;
    border-bottom: 2px solid transparent;
    margin-bottom: -1px;
    transition: all 0.12s;
  }

  .tab-nav button:hover {
    color: var(--ink-900);
  }

  .tab-nav button.active {
    color: var(--accent-500);
    border-bottom-color: var(--accent-500);
    font-weight: 600;
  }

  .tab-content {
    padding: 1.25rem;
  }

  .connections-panel {
    max-width: 100%;
  }

  .panel-desc {
    font-size: 0.8125rem;
    color: var(--ink-500);
    margin-bottom: 1rem;
    line-height: 1.5;
  }

  .load-warning {
    font-size: 0.8125rem;
    color: var(--warn-600);
    background: var(--warn-50);
    padding: 0.375rem 0.5rem;
    border-radius: 4px;
    margin-bottom: 0.75rem;
  }

  .loading-text {
    font-size: 0.8125rem;
    color: var(--ink-500);
    padding: 1rem 0;
  }
</style>
