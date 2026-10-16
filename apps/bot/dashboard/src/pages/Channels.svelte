<script lang="ts">
  /**
   * Channels — connector catalog rendered from descriptor-shaped data.
   *
   * Channels are project communication and review routes (e.g. Slack,
   * Devin, Linear). They are distinct from managed evidence Sources,
   * which are approved ingestion bindings that feed the governance
   * runtime (see SourceBinding in connection-fixtures.ts).
   *
   * Shows available connectors as cards with availability, status,
   * trust tier, modes, category, and setup readiness.
   * Selecting a card opens the connector detail/setup surface.
   *
   * Boundary: this page renders descriptor data only. It does not
   * perform live connector wiring, store secrets, or add backend routes.
   * Integrations descriptors declare requirements; the operator runtime
   * receives any secret/token values.
   */
  import { CONNECTOR_DESCRIPTORS } from "../lib/connector-descriptors";
  import type { ConnectorDescriptor } from "../lib/connector-types";
  import ConnectorSetup from "./ConnectorSetup.svelte";

  let selectedId = $state<string | null>(null);
  let setupConnector = $state<ConnectorDescriptor | null>(null);

  function selectConnector(id: string) {
    selectedId = selectedId === id ? null : id;
  }

  function selected(): ConnectorDescriptor | undefined {
    return CONNECTOR_DESCRIPTORS.find((c) => c.id === selectedId);
  }

  function openSetup(connector: ConnectorDescriptor) {
    setupConnector = connector;
  }

  function closeSetup() {
    setupConnector = null;
  }

  function statusLabel(s: string): string {
    switch (s) {
      case "live-ready":
        return "Flip-ready";
      case "beta":
        return "Beta";
      case "candidate":
        return "Candidate";
      case "scoped":
        return "Scoped";
      default:
        return s;
    }
  }

  function modeLabel(m: string): string {
    switch (m) {
      case "webhook":
        return "Webhook";
      case "active":
        return "Active poll";
      case "passive":
        return "Passive";
      default:
        return m;
    }
  }

  function categoryLabel(c: string): string {
    switch (c) {
      case "communication":
        return "Communication";
      case "developer-ai":
        return "Developer AI";
      case "project-tracker":
        return "Project Tracker";
      default:
        return c;
    }
  }
</script>

{#if setupConnector}
  <ConnectorSetup connector={setupConnector} onback={closeSetup} />
{:else}
  <section>
    <header class="page-header" id="channels">
      <h1>Channels</h1>
      <p class="subtitle">
        Communication, review, and connector routes for your project.
      </p>
      <p class="ia-note">
        Channels carry notifications and review context. Managed evidence
        Sources — approved ingestion bindings that feed the governance
        runtime — are configured separately under Project Settings.
      </p>
    </header>

    <div class="channels-layout">
      <div class="card-grid">
        {#each CONNECTOR_DESCRIPTORS as connector (connector.id)}
          <button
            class="connector-card"
            class:selected={selectedId === connector.id}
            class:unavailable={!connector.available}
            onclick={() => selectConnector(connector.id)}
          >
            <div class="card-top">
              <span class="connector-logo" class:slack={connector.icon === "slack"} class:devin={connector.icon === "devin"} class:linear={connector.icon === "linear"}>
                {#if connector.icon === "slack"}
                  <span></span><span></span><span></span><span></span>
                {:else if connector.icon === "linear"}
                  <svg viewBox="0 0 100 100" width="16" height="16" fill="currentColor"><path d="M1.22 61.5a48.8 48.8 0 0 1-1.2-7.7L37.3 91a49 49 0 0 1-7.7-1.2L1.2 61.5ZM0 50.5a50 50 0 0 0 .4 4.5L46 96a50 50 0 0 0 4.5.4L0 45.9v4.6ZM0 39.2 60.8 100a49 49 0 0 0 7-2.3L2.3 32.2A49 49 0 0 0 0 39.2ZM5 25.4 74.6 95a49 49 0 0 0 5.5-4.2L9.2 20A49 49 0 0 0 5 25.4ZM14 15l71 71a50 50 0 0 0 4-4.7L18.7 10.9A50 50 0 0 0 14 15ZM24.3 7.3l68.4 68.4a49 49 0 0 0 2.9-6L31.3 4.4a49 49 0 0 0-7 2.9ZM37.3 1.9 91.7 56.3A48.5 48.5 0 0 0 93 50c0-1.7-.1-3.4-.3-5.1L42.4.7A49 49 0 0 0 37.3 2Z"/></svg>
                {:else}
                  D
                {/if}
              </span>
              <span class="connector-name">{connector.name}</span>
              <span
                class="availability-dot"
                class:available={connector.available}
                title={connector.available ? "Available" : "Coming soon"}
              ></span>
            </div>

            <p class="connector-desc">{connector.summary ?? connector.description}</p>

            <div class="card-meta">
              <span class="meta-badge category">{categoryLabel(connector.category)}</span>
              <span class="meta-badge status">{connector.status_copy ?? statusLabel(connector.status)}</span>
            </div>

            <div class="card-footer">
              {#if connector.available}
                <span class="modes">
                  {#each connector.modes as mode}
                    <span class="mode-tag">{modeLabel(mode)}</span>
                  {/each}
                </span>
              {:else}
                <span class="mode-tag disabled">Unavailable</span>
              {/if}
            </div>
          </button>
        {/each}
      </div>

      {#if selected()}
        {@const conn = selected()!}
        <aside class="detail-panel">
          <div class="detail-heading">
            <span class="detail-logo" class:slack={conn.icon === "slack"} class:devin={conn.icon === "devin"} class:linear={conn.icon === "linear"}>
              {#if conn.icon === "slack"}
                <span></span><span></span><span></span><span></span>
              {:else if conn.icon === "linear"}
                <svg viewBox="0 0 100 100" width="20" height="20" fill="currentColor"><path d="M1.22 61.5a48.8 48.8 0 0 1-1.2-7.7L37.3 91a49 49 0 0 1-7.7-1.2L1.2 61.5ZM0 50.5a50 50 0 0 0 .4 4.5L46 96a50 50 0 0 0 4.5.4L0 45.9v4.6ZM0 39.2 60.8 100a49 49 0 0 0 7-2.3L2.3 32.2A49 49 0 0 0 0 39.2ZM5 25.4 74.6 95a49 49 0 0 0 5.5-4.2L9.2 20A49 49 0 0 0 5 25.4ZM14 15l71 71a50 50 0 0 0 4-4.7L18.7 10.9A50 50 0 0 0 14 15ZM24.3 7.3l68.4 68.4a49 49 0 0 0 2.9-6L31.3 4.4a49 49 0 0 0-7 2.9ZM37.3 1.9 91.7 56.3A48.5 48.5 0 0 0 93 50c0-1.7-.1-3.4-.3-5.1L42.4.7A49 49 0 0 0 37.3 2Z"/></svg>
              {:else}
                D
              {/if}
            </span>
            <div>
              <h2>{conn.name}</h2>
              <p class="detail-kicker">{categoryLabel(conn.category)}</p>
            </div>
          </div>
          <p class="detail-desc">{conn.description}</p>

          <div class="connector-summary">
            <h3>What it does</h3>
            <p>{conn.summary ?? conn.description}</p>
          </div>

          {#if conn.live_readiness}
            <div class="readiness-note">
              <h3>Status</h3>
              <p>{conn.live_readiness}</p>
            </div>
          {/if}

          {#if conn.resources && conn.resources.length > 0}
            <div class="resource-list">
              {#each conn.resources as resource}
                <a href={resource.url} target="_blank" rel="noopener">{resource.label}</a>
              {/each}
            </div>
          {/if}

          {#if conn.available}
            <button class="setup-btn" onclick={() => openSetup(conn)}>
              Set up {conn.name} →
            </button>
          {:else}
            <button class="setup-btn disabled" disabled>
              Coming soon
            </button>
          {/if}

          <p class="boundary-note">
            Secrets stay in your local setup path. Bicameral does not save them
            to the Decision Ledger.
          </p>
        </aside>
      {/if}
    </div>
  </section>
{/if}

<style>
  section {
    max-width: 1100px;
  }

  .page-header {
    margin-bottom: 1.5rem;
  }

  .page-header h1 {
    font-size: var(--text-lg);
    font-weight: 700;
    color: var(--ink-900);
  }

  .subtitle {
    font-size: var(--text-sm);
    color: var(--ink-500);
    margin-top: 0.25rem;
  }

  .ia-note {
    font-size: 0.75rem;
    color: var(--ink-700);
    margin-top: 0.375rem;
    line-height: 1.5;
  }

  .channels-layout {
    display: flex;
    gap: 1.5rem;
    align-items: flex-start;
  }

  .card-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: 1rem;
    flex: 1;
  }

  .connector-card {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding: 1rem;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--surface);
    cursor: pointer;
    text-align: left;
    font-family: var(--font-sans);
    transition:
      border-color var(--dur-fast) var(--ease-out),
      box-shadow var(--dur-fast) var(--ease-out);
  }

  .connector-card:hover {
    border-color: var(--accent-200);
  }

  .connector-card.selected {
    border-color: var(--accent-500);
    box-shadow: 0 0 0 3px var(--accent-50);
  }

  .connector-card.unavailable {
    opacity: 0.6;
  }

  .card-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }

  .connector-logo,
  .detail-logo {
    width: 2rem;
    height: 2rem;
    display: inline-grid;
    place-items: center;
    flex-shrink: 0;
    border-radius: var(--radius-lg);
    background: var(--ink-900);
    color: var(--surface);
    font-weight: 700;
  }

  .connector-logo.linear,
  .detail-logo.linear {
    background: #5e6ad2;
    color: #fff;
  }

  .connector-logo.slack,
  .detail-logo.slack {
    grid-template-columns: repeat(2, 0.45rem);
    grid-template-rows: repeat(2, 0.45rem);
    gap: 0.14rem;
    border: 1px solid var(--line);
    background: var(--surface);
  }

  .connector-logo.slack span,
  .detail-logo.slack span {
    width: 0.45rem;
    height: 0.45rem;
    border-radius: 999px;
  }

  .connector-logo.slack span:nth-child(1),
  .detail-logo.slack span:nth-child(1) {
    background: #36c5f0;
  }

  .connector-logo.slack span:nth-child(2),
  .detail-logo.slack span:nth-child(2) {
    background: #2eb67d;
  }

  .connector-logo.slack span:nth-child(3),
  .detail-logo.slack span:nth-child(3) {
    background: #ecb22e;
  }

  .connector-logo.slack span:nth-child(4),
  .detail-logo.slack span:nth-child(4) {
    background: #e01e5a;
  }

  .connector-name {
    font-size: 0.9375rem;
    font-weight: 600;
    color: var(--ink-900);
    margin-right: auto;
  }

  .availability-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--line-strong);
  }

  .availability-dot.available {
    background: var(--ok-500);
  }

  .connector-desc {
    font-size: 0.75rem;
    color: var(--ink-700);
    line-height: 1.4;
  }

  .card-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 0.375rem;
    margin-top: 0.25rem;
  }

  .meta-badge {
    font-size: 0.6875rem;
    padding: 0.125rem 0.5rem;
    border-radius: 9999px;
    font-weight: 500;
  }

  .meta-badge.category {
    background: var(--canvas-muted);
    color: var(--ink-700);
  }

  .meta-badge.status {
    background: var(--accent-50);
    color: var(--accent-700);
  }

  .card-footer {
    display: flex;
    gap: 0.5rem;
    flex-wrap: wrap;
    margin-top: auto;
    padding-top: 0.375rem;
    border-top: 1px solid var(--canvas-muted);
  }

  .mode-tag {
    font-size: 0.6875rem;
    padding: 0.0625rem 0.375rem;
    border-radius: 3px;
    background: var(--canvas-muted);
    border: 1px solid var(--line);
    color: var(--ink-700);
  }

  .mode-tag.disabled {
    color: var(--ink-500);
    background: var(--canvas-muted);
  }

  /* Detail panel */
  .detail-panel {
    width: 340px;
    flex-shrink: 0;
    padding: 1.25rem;
    background: #fff;
    border: 1px solid var(--line);
    border-radius: 0.5rem;
    position: sticky;
    top: 1rem;
  }

  .detail-heading {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-bottom: 0.75rem;
  }

  .detail-logo {
    width: 2.5rem;
    height: 2.5rem;
  }

  .detail-logo.slack {
    grid-template-columns: repeat(2, 0.5rem);
    grid-template-rows: repeat(2, 0.5rem);
  }

  .detail-logo.slack span {
    width: 0.5rem;
    height: 0.5rem;
  }

  .detail-panel h2 {
    font-size: 1.125rem;
    font-weight: 700;
    color: var(--ink-900);
  }

  .detail-kicker {
    font-size: 0.75rem;
    color: var(--ink-500);
  }

  .detail-desc {
    font-size: 0.8125rem;
    color: var(--ink-700);
    margin-bottom: 1rem;
    line-height: 1.4;
  }

  .connector-summary {
    padding: 0.75rem;
    background: var(--canvas-muted);
    border: 1px solid var(--line);
    border-radius: 0.375rem;
    margin-bottom: 0.75rem;
  }

  .connector-summary h3 {
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--ink-700);
    margin-bottom: 0.375rem;
  }

  .connector-summary p {
    font-size: 0.6875rem;
    color: var(--ink-700);
    line-height: 1.5;
  }

  .readiness-note {
    padding: 0.75rem;
    background: var(--accent-50);
    border: 1px solid var(--accent-200);
    border-radius: 0.375rem;
    margin-bottom: 0.75rem;
  }

  .readiness-note h3 {
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--accent-700);
    margin-bottom: 0.375rem;
  }

  .readiness-note p {
    font-size: 0.6875rem;
    color: var(--accent-600);
    line-height: 1.5;
  }

  .resource-list {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    margin-bottom: 0.75rem;
  }

  .resource-list a {
    padding: 0.25rem 0.625rem;
    border: 1px solid var(--accent-200);
    border-radius: 999px;
    background: var(--accent-50);
    color: var(--accent-500);
    font-size: 0.75rem;
    text-decoration: none;
  }

  .setup-btn {
    display: block;
    width: 100%;
    padding: 0.5rem;
    margin-bottom: 0.75rem;
    border: 1px solid var(--accent-500);
    border-radius: 0.375rem;
    background: var(--accent-500);
    color: #fff;
    font-size: 0.8125rem;
    font-weight: 500;
    font-family: inherit;
    cursor: pointer;
    transition: all 0.15s;
    text-align: center;
  }

  .setup-btn:hover {
    background: var(--accent-600);
  }

  .setup-btn.disabled,
  .setup-btn.disabled:hover {
    border-color: var(--line-strong);
    background: var(--canvas-muted);
    color: var(--ink-500);
    cursor: not-allowed;
  }

  .boundary-note {
    font-size: 0.6875rem;
    color: var(--ink-500);
    font-style: italic;
    line-height: 1.4;
    padding-top: 0.5rem;
    border-top: 1px solid var(--canvas-muted);
  }
</style>
