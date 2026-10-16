<script lang="ts">
  /**
   * Local evaluation harness for v0.1.17 create + connections UX (bot#1025 / T-B).
   * Open: npm run dev → http://127.0.0.1:5173/?eval=create-product-ux
   *
   * Not production routing. Uses design-system tokens from app.css.
   */
  import ConnectionCard from "../lib/ConnectionCard.svelte";
  import type { Connection } from "../lib/connection-fixtures";
  import HostedProjectRegistration from "./HostedProjectRegistration.svelte";

  type View = "create" | "connections";

  let view = $state<View>("create");
  let driveConnected = $state(false);
  let lastSubmit = $state<string | null>(null);

  const driveConnection = $derived<Connection>({
    provider_id: "google",
    status: driveConnected ? "connected" : "not_connected",
    account_label: driveConnected ? "eval@bicameral-ai.com" : undefined,
    capabilities: driveConnected ? ["event_store", "drive_folder"] : [],
  });

  const connections = $derived<Connection[]>([
    driveConnection,
    {
      provider_id: "github",
      status: "not_connected",
      capabilities: [],
    },
    {
      provider_id: "linear",
      status: "unsupported",
      capabilities: [],
    },
  ]);

  function openConnections() {
    view = "connections";
  }

  function toggleDrive() {
    driveConnected = !driveConnected;
  }
</script>

<div class="eval-shell" data-testid="create-product-ux-eval">
  <header class="eval-header">
    <div>
      <p class="eyebrow">v0.1.17 local eval · bot#1025</p>
      <h1>Create Product — connection gate</h1>
      <p class="lede">
        Create stays disabled until Google Drive is connected. Use the
        exclamation to open Connections. Toggle Drive below to simulate connect.
      </p>
    </div>
    <div class="eval-tools">
      <label class="toggle">
        <input
          type="checkbox"
          checked={driveConnected}
          onchange={toggleDrive}
          data-testid="eval-toggle-drive"
        />
        Simulate Drive connected
      </label>
      <nav class="tabs" aria-label="Eval views">
        <button
          type="button"
          class:active={view === "create"}
          onclick={() => (view = "create")}
          data-testid="eval-tab-create"
        >
          Create
        </button>
        <button
          type="button"
          class:active={view === "connections"}
          onclick={() => (view = "connections")}
          data-testid="eval-tab-connections"
        >
          Connections
        </button>
      </nav>
    </div>
  </header>

  <div class="layout">
    <aside class="sidebar" aria-label="Shell preview">
      <p class="nav-section">WORKSPACE</p>
      <button type="button" class="nav-item" disabled title="Facade until T-D">
        Overview
      </button>
      <button type="button" class="nav-item" disabled title="Facade until T-D">
        Activity
      </button>
      <button type="button" class="nav-item active">Products</button>
      <button type="button" class="nav-item" disabled title="Facade until T-D">
        Settings
      </button>
      <p class="nav-note">
        Grey items are intentional facade chrome (T-D rework). Products +
        Connections paths are under evaluation.
      </p>
    </aside>

    <main>
      {#if view === "create"}
        <HostedProjectRegistration
          eventSubstrateConnected={driveConnected}
          eventSubstrateLabel="Google Drive"
          onrequestconnect={openConnections}
          onsubmitcreate={(input) => {
            lastSubmit = JSON.stringify(input);
          }}
          oncancel={() => {
            lastSubmit = null;
          }}
        />
        {#if lastSubmit}
          <p class="submit-echo" data-testid="eval-submit-echo">
            Submit fired (eval only): <code>{lastSubmit}</code>
          </p>
        {/if}
      {:else}
        <section class="connections">
          <h2>Connections</h2>
          <p class="panel-desc">
            Operator-level connections. Alpha Product event substrate is
            <strong>Google Drive</strong>.
          </p>
          {#each connections as conn (conn.provider_id)}
            <ConnectionCard connection={conn} />
          {/each}
          <p class="hint">
            Live OAuth is not wired in this harness. Use “Simulate Drive
            connected” in the header, then return to Create.
          </p>
          <button
            type="button"
            class="btn secondary"
            onclick={() => (view = "create")}
            data-testid="eval-back-create"
          >
            Back to Create
          </button>
        </section>
      {/if}
    </main>
  </div>
</div>

<style>
  .eval-shell {
    min-height: 100vh;
    background: var(--canvas);
    color: var(--ink-900);
    font-family: var(--font-sans);
  }

  .eval-header {
    display: flex;
    justify-content: space-between;
    gap: var(--sp-6);
    padding: var(--sp-6) var(--sp-8);
    border-bottom: 1px solid var(--line);
    background: var(--surface);
  }

  .eyebrow {
    margin: 0 0 var(--sp-1);
    font-size: var(--text-xs);
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--ink-500);
  }

  h1 {
    margin: 0 0 var(--sp-2);
    font-size: var(--text-lg);
    font-weight: 600;
  }

  .lede {
    margin: 0;
    max-width: 40rem;
    font-size: var(--text-base);
    color: var(--ink-700);
  }

  .eval-tools {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: var(--sp-3);
  }

  .toggle {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    font-size: var(--text-sm);
    color: var(--ink-700);
  }

  .tabs {
    display: flex;
    gap: var(--sp-1);
    padding: var(--sp-1);
    background: var(--canvas-muted);
    border-radius: var(--radius-md);
  }

  .tabs button {
    border: 0;
    background: transparent;
    padding: var(--sp-2) var(--sp-3);
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
    color: var(--ink-700);
    cursor: pointer;
  }

  .tabs button.active {
    background: var(--surface);
    color: var(--ink-900);
    box-shadow: var(--shadow-1);
    font-weight: 600;
  }

  .layout {
    display: grid;
    grid-template-columns: 220px 1fr;
    min-height: calc(100vh - 120px);
  }

  .sidebar {
    padding: var(--sp-5);
    border-right: 1px solid var(--line);
    background: var(--canvas-muted);
  }

  .nav-section {
    margin: 0 0 var(--sp-3);
    font-size: var(--text-xs);
    color: var(--ink-500);
    letter-spacing: 0.06em;
  }

  .nav-item {
    display: block;
    width: 100%;
    text-align: left;
    border: 0;
    background: transparent;
    padding: var(--sp-2) var(--sp-3);
    margin-bottom: var(--sp-1);
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
    color: var(--ink-700);
  }

  .nav-item.active {
    background: var(--accent-50);
    color: var(--accent-700);
    font-weight: 600;
  }

  .nav-item:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .nav-note {
    margin-top: var(--sp-5);
    font-size: var(--text-xs);
    color: var(--ink-500);
    line-height: 1.4;
  }

  main {
    padding: var(--sp-6) var(--sp-8);
  }

  .connections h2 {
    margin: 0 0 var(--sp-2);
    font-size: var(--text-md);
  }

  .panel-desc,
  .hint {
    font-size: var(--text-base);
    color: var(--ink-700);
  }

  .hint {
    margin: var(--sp-4) 0;
    padding: var(--sp-3);
    background: var(--warn-50);
    border-radius: var(--radius-md);
    border: 1px solid var(--line);
  }

  .submit-echo {
    margin-top: var(--sp-4);
    font-size: var(--text-sm);
    color: var(--ok-600);
  }

  .btn.secondary {
    margin-top: var(--sp-3);
    border: 1px solid var(--line);
    background: var(--surface);
    border-radius: var(--radius-md);
    padding: var(--sp-2) var(--sp-4);
    font-size: var(--text-sm);
    cursor: pointer;
  }
</style>
