<script lang="ts">
  import type { ProjectEntry } from "../lib/project-fixtures";
  import { ACCESS_FAILURE_COPY } from "../lib/project-fixtures";
  import {
    DISCOVERY_EMPTY_TITLE,
    DISCOVERY_EMPTY_DESCRIPTION,
  } from "../lib/operator-home-fixtures";
  import type { ProductJoinFailureView } from "../lib/product-join";
  import type { Snippet } from "svelte";
  import type { McpReadinessState } from "../lib/mcp-readiness";
  import type { SettingsSection } from "./HostedSettings.svelte";
  import {
    ROW_ACTION_LABEL,
    YOUR_PRODUCTS_HEADING,
    availableHeading,
    groupRows,
    rowAction,
    type RowAction,
  } from "../lib/operator-home-model";

  type Props = {
    projects?: ProjectEntry[];
    /** Open a ready, joined Product. */
    onopen?: (project: ProjectEntry) => void;
    /** Resume incomplete local registration/provider setup for a joined Product. */
    oncontinuesetup?: (project: ProjectEntry) => void;
    /** Repair a broken local dependency for a joined Product. */
    onrepair?: (project: ProjectEntry) => void;
    /** Create a new Product (canonical daemon creation/publication). */
    oncreate?: () => void;
    /** Request a same-domain per-Product join for an available row. */
    onjoin?: (project: ProjectEntry) => void;
    /** Id of the row whose join is currently in flight (transient UI only). */
    joiningId?: string | null;
    /** Per-row typed join failure, keyed by project id. */
    joinFailures?: Record<string, ProductJoinFailureView>;
    /** Navigate to Products home. */
    onproducts?: () => void;
    /** Navigate to operator Settings (Storage + Agent host). */
    onsettings?: () => void;
    /** Deep-link into a Settings section from the host status footer. */
    onopensettings?: (section: SettingsSection) => void;
    /** @deprecated Prefer onsettings — transitional callers only. */
    onconnections?: () => void;
    /** Event-store substrate connected (alpha: Google Drive). Gates Create CTA copy. */
    eventSubstrateConnected?: boolean;
    /** MCP readiness for rail footer only (does not gate Products). */
    mcpState?: McpReadinessState;
    /** Which primary rail item is active. */
    activeNav?: "products" | "settings";
    /** When set, replaces the Products ledger main (Settings shell). */
    children?: Snippet;
  };

  let {
    projects = [],
    onopen,
    oncontinuesetup,
    onrepair,
    oncreate,
    onjoin,
    joiningId = null,
    joinFailures = {},
    onproducts,
    onsettings,
    onopensettings,
    onconnections,
    eventSubstrateConnected = false,
    mcpState = "not_installed",
    activeNav = "products",
    children,
  }: Props = $props();

  /** Langfuse-style collapsible rail (icon strip when collapsed). */
  let railCollapsed = $state(false);

  const groups = $derived(groupRows(projects));
  const isEmpty = $derived(projects.length === 0);
  const availableTitle = $derived(availableHeading(groups.available));

  const mcpFooter = $derived.by(() => {
    switch (mcpState) {
      case "active_verified":
        return { label: "MCP ready", tone: "ok" as const, off: false };
      case "installed_pending_host_action":
        return { label: "MCP needs host", tone: "warn" as const, off: false };
      case "prerequisite_missing":
        return { label: "MCP needs setup", tone: "warn" as const, off: false };
      case "repair_required":
        return { label: "MCP needs repair", tone: "bad" as const, off: false };
      case "not_installed":
      default:
        return { label: "MCP not set up", tone: "muted" as const, off: true };
    }
  });

  function openSettings(section: SettingsSection = "storage") {
    if (onopensettings) onopensettings(section);
    else if (onsettings) onsettings();
    else onconnections?.();
  }

  function dispatchMemberAction(project: ProjectEntry, action: RowAction) {
    if (action === "open") onopen?.(project);
    else if (action === "continue_setup") oncontinuesetup?.(project);
    else if (action === "repair") onrepair?.(project);
  }

  function readinessLabel(project: ProjectEntry): string {
    if (project.readiness === "ready") {
      return project.setup_incomplete ? "Setup incomplete" : "Ready";
    }
    if (project.readiness === "offline") return "Offline";
    if (project.access_failure) {
      return ACCESS_FAILURE_COPY[project.access_failure];
    }
    return "Needs access";
  }

  function readinessClass(project: ProjectEntry): string {
    if (project.readiness === "ready") {
      return project.setup_incomplete ? "needs-access" : "ready";
    }
    if (project.readiness === "offline") return "offline";
    return "needs-access";
  }
</script>

<div class="operator-home" class:rail-collapsed={railCollapsed}>
  <nav class="navigation-rail" aria-label="Workspace" data-collapsed={railCollapsed}>
    <div class="rail-toolbar">
      <button
        type="button"
        class="rail-collapse-btn"
        data-testid="nav-collapse"
        aria-label={railCollapsed ? "Expand navigation" : "Collapse navigation"}
        aria-expanded={!railCollapsed}
        title={railCollapsed ? "Expand" : "Collapse"}
        onclick={() => {
          railCollapsed = !railCollapsed;
        }}
      >
        <!-- Panel-left / collapse icon (Langfuse-style) -->
        <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
          <rect
            x="1.5"
            y="2"
            width="13"
            height="12"
            rx="1.5"
            fill="none"
            stroke="currentColor"
            stroke-width="1.25"
          />
          <path d="M6 2v12" stroke="currentColor" stroke-width="1.25" />
        </svg>
      </button>
    </div>
    {#if !railCollapsed}
      <p class="rail-label">ADMIN</p>
    {/if}
    <ul class="rail-items">
      <li
        class="rail-item"
        class:active={activeNav === "products"}
        aria-current={activeNav === "products" ? "page" : undefined}
      >
        {#if activeNav === "products"}
          <span aria-hidden="true">▦</span
          >{#if !railCollapsed}<span>Products</span>{/if}
        {:else}
          <button
            type="button"
            class="rail-btn"
            data-testid="nav-products"
            title="Products"
            onclick={() => onproducts?.()}
          >
            <span aria-hidden="true">▦</span
            >{#if !railCollapsed}<span>Products</span>{/if}
          </button>
        {/if}
      </li>
      <li
        class="rail-item"
        class:active={activeNav === "settings"}
        aria-current={activeNav === "settings" ? "page" : undefined}
      >
        {#if activeNav === "settings"}
          <span aria-hidden="true">⚙</span
          >{#if !railCollapsed}<span>Settings</span>{/if}
        {:else}
          <button
            type="button"
            class="rail-btn"
            data-testid="nav-settings"
            title="Settings"
            onclick={() => openSettings("storage")}
          >
            <span aria-hidden="true">⚙</span
            >{#if !railCollapsed}<span>Settings</span>{/if}
          </button>
        {/if}
      </li>
    </ul>
    <div class="rail-connection" data-testid="host-status">
      {#if !railCollapsed}
        <p class="rail-label">HOST</p>
      {/if}
      <button
        type="button"
        class="status-line"
        data-testid="host-status-drive"
        title={eventSubstrateConnected
          ? "Drive connected"
          : "Drive not connected"}
        onclick={() => openSettings("storage")}
      >
        <span
          class="connection-dot"
          class:off={!eventSubstrateConnected}
          aria-hidden="true"
        ></span>
        {#if !railCollapsed}
          <span
            >{eventSubstrateConnected
              ? "Drive connected"
              : "Drive not connected"}</span
          >
        {/if}
      </button>
      <button
        type="button"
        class="status-line"
        data-testid="host-status-mcp"
        data-mcp-state={mcpState}
        data-tone={mcpFooter.tone}
        title={mcpFooter.label}
        onclick={() => openSettings("agent-host")}
      >
        <span
          class="connection-dot"
          class:off={mcpFooter.off}
          class:warn={mcpFooter.tone === "warn"}
          class:bad={mcpFooter.tone === "bad"}
          aria-hidden="true"
        ></span>
        {#if !railCollapsed}
          <span>{mcpFooter.label}</span>
        {/if}
      </button>
      {#if !railCollapsed}
        <p
          class="sync-current"
          class:warn={!eventSubstrateConnected || mcpFooter.tone !== "ok"}
        >
          {#if eventSubstrateConnected && mcpFooter.tone === "ok"}
            HOST READY
          {:else if !eventSubstrateConnected}
            CONNECT STORAGE
          {:else}
            AGENT HOST OPTIONAL
          {/if}
        </p>
      {/if}
    </div>
  </nav>

  <!-- Content page header (Langfuse "Home" row): title size ~18px, not a second fat app bar -->
  <div class="workspace-topbar" data-testid="workspace-topbar">
    <h1 class="page-heading">
      {activeNav === "settings" ? "Settings" : "Products"}
    </h1>
    <div class="topbar-trailing">
      {#if activeNav === "products" && !children && !isEmpty}
        <button
          class="btn-dark"
          data-testid="create-product"
          onclick={oncreate}
          title={eventSubstrateConnected
            ? "Create product"
            : "Connect storage in Settings first (alpha event store)"}
        >
          Create product
        </button>
      {/if}
    </div>
  </div>

  <main class="product-ledger-main" data-testid="product-ledger-main">
    {#if children}
      {@render children()}
    {:else}
    {#if isEmpty}
      <div class="empty-panel" data-testid="empty-state">
        <div class="empty-icon" aria-hidden="true">○</div>
        <h2 class="empty-title">{DISCOVERY_EMPTY_TITLE}</h2>
        <p class="empty-description" data-testid="empty-description">
          {DISCOVERY_EMPTY_DESCRIPTION}
        </p>
        <div class="empty-actions">
          <button
            class="btn-dark"
            data-testid="empty-create"
            onclick={oncreate}
          >
            Create product
          </button>
        </div>
      </div>
    {:else}
      {#if groups.members.length > 0}
        <section class="product-group" data-testid="your-products-section">
          <h2 class="group-label" id="your-products-heading">
            {YOUR_PRODUCTS_HEADING}
          </h2>
          <div class="panel-frame table-frame" data-testid="project-table">
            <table class="project-table" aria-label={YOUR_PRODUCTS_HEADING}>
              <colgroup>
                <col class="product-column" />
                <col class="store-column" />
                <col class="status-column" />
                <col class="updated-column" />
                <col class="action-column" />
              </colgroup>
              <thead>
                <tr>
                  <th scope="col">Product</th>
                  <th scope="col">Event store</th>
                  <th scope="col">Status</th>
                  <th scope="col">Updated</th>
                  <th scope="col" aria-label="Action"></th>
                </tr>
              </thead>
              <tbody>
                {#each groups.members as project (project.id)}
                  {@const action = rowAction(project)}
                  <tr
                    data-testid="project-row"
                    data-membership="member"
                    data-readiness={project.readiness}
                  >
                    <th scope="row" class="project-name" data-label="Product">
                      {project.name}
                    </th>
                    <td class="col-store" data-label="Event store">
                      {project.event_store_label}
                    </td>
                    <td class="col-status" data-label="Status">
                      <span class="readiness-badge {readinessClass(project)}">
                        {readinessLabel(project)}
                      </span>
                    </td>
                    <td class="col-updated" data-label="Updated" aria-label="Update time unavailable">—</td>
                    <td class="col-action" data-label="Action">
                      {#if action}
                        <button
                          class="row-action"
                          class:repair={action === "repair"}
                          data-testid="row-action"
                          data-action={action}
                          onclick={() => dispatchMemberAction(project, action)}
                        >
                          {ROW_ACTION_LABEL[action]}
                        </button>
                      {/if}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        </section>
      {/if}

      {#if groups.available.length > 0}
        <section class="product-group" data-testid="available-section">
          <h2 class="group-label" id="available-products-heading" data-testid="available-heading">
            {availableTitle}
          </h2>
          <div class="panel-frame table-frame" data-testid="available-table">
            <table class="project-table" aria-label={availableTitle}>
              <colgroup>
                <col class="product-column" />
                <col class="store-column" />
                <col class="status-column" />
                <col class="updated-column" />
                <col class="action-column" />
              </colgroup>
              <thead>
                <tr>
                  <th scope="col">Product</th>
                  <th scope="col">Event store</th>
                  <th scope="col">Status</th>
                  <th scope="col">Updated</th>
                  <th scope="col" aria-label="Action"></th>
                </tr>
              </thead>
              <tbody>
                {#each groups.available as project (project.id)}
                  {@const joining = joiningId === project.id}
                  {@const failure = joinFailures[project.id]}
                  <tr
                    class:has-failure={!!failure}
                    data-testid="project-row"
                    data-membership="available"
                  >
                    <th scope="row" class="project-name" data-label="Product">
                      {project.name}
                    </th>
                    <td class="col-store" data-label="Event store">
                      {project.event_store_label}
                    </td>
                    <td class="col-status" data-label="Status">
                      {#if failure}
                        <span
                          class="join-failure"
                          data-testid="join-failure"
                          data-failure-code={failure.code}
                          role="alert"
                        >
                          {failure.message}
                        </span>
                      {:else}
                        <span class="available-status">Available</span>
                      {/if}
                    </td>
                    <td class="col-updated" data-label="Updated" aria-label="Update time unavailable">—</td>
                    <td class="col-action" data-label="Action">
                      {#if failure && failure.retryable}
                        <button
                          class="row-action join"
                          data-testid="join-retry"
                          data-action="join"
                          aria-label={`Retry joining ${project.name}`}
                          onclick={() => onjoin?.(project)}
                        >
                          Try again
                        </button>
                      {:else if !failure}
                        <button
                          class="row-action join"
                          data-testid="join-action"
                          data-action="join"
                          data-joining={joining}
                          aria-label={`Join ${project.name}`}
                          aria-busy={joining}
                          disabled={joining}
                          onclick={() => onjoin?.(project)}
                        >
                          {joining ? "Joining…" : ROW_ACTION_LABEL.join}
                        </button>
                      {/if}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        </section>
      {/if}
    {/if}
    {/if}
  </main>
</div>

<style>
  /* Shell: Langfuse / shadcn density tokens from app.css (--shell-*). */
  .operator-home {
    display: grid;
    grid-template-columns: var(--shell-sidebar-w) minmax(0, 1fr);
    grid-template-rows: 2.75rem minmax(0, 1fr); /* page title row (Langfuse "Home") */
    min-height: calc(100vh - var(--shell-topbar-h, 2.5rem));
    background: var(--canvas);
  }

  .operator-home.rail-collapsed {
    grid-template-columns: 3rem minmax(0, 1fr);
  }

  .rail-label,
  .sync-current {
    font-family: var(--font-mono);
  }

  .connection-dot {
    width: 6px;
    height: 6px;
    flex: 0 0 6px;
    border-radius: 50%;
    background: var(--ok-500);
  }

  .navigation-rail {
    grid-column: 1;
    grid-row: 1 / 3;
    position: relative;
    display: flex;
    min-height: 0;
    flex-direction: column;
    padding: 0.375rem 0.375rem 0.75rem;
    border-right: 1px solid var(--line);
    background: var(--surface);
  }

  .rail-toolbar {
    display: flex;
    align-items: center;
    min-height: 1.75rem;
    margin-bottom: 0.25rem;
    padding: 0 0.125rem;
  }

  .rail-collapse-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 1.75rem;
    height: 1.75rem;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--ink-500);
    cursor: pointer;
  }

  .rail-collapse-btn:hover {
    background: var(--canvas-muted);
    color: var(--ink-900);
  }

  .rail-label {
    padding: 0.25rem 0.5rem 0;
    color: var(--ink-400);
    font-size: 0.625rem;
    font-weight: 500;
    letter-spacing: 0.06em;
  }

  .rail-items {
    display: grid;
    gap: 1px;
    margin-top: 0.25rem;
    list-style: none;
  }

  .rail-item {
    display: grid;
    grid-template-columns: 14px 1fr;
    align-items: center;
    gap: 0.5rem;
    min-height: var(--shell-nav-item-h);
    padding: 0 0.5rem;
    color: var(--ink-700);
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
  }

  .rail-collapsed .rail-item,
  .rail-collapsed .rail-btn {
    grid-template-columns: 1fr;
    justify-items: center;
    padding: 0;
    gap: 0;
  }

  .rail-btn {
    display: grid;
    grid-template-columns: 14px 1fr;
    align-items: center;
    gap: 0.5rem;
    width: 100%;
    min-height: var(--shell-nav-item-h);
    padding: 0 0.5rem;
    margin: 0;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
    border-radius: var(--radius-sm);
  }

  .rail-btn:hover {
    background: var(--canvas-muted);
  }

  .connection-dot.off {
    background: var(--ink-400);
  }

  .rail-item > :first-child {
    color: var(--ink-400);
    font-size: 0.75rem;
  }

  .rail-item.active {
    position: relative;
    background: var(--canvas-muted);
    color: var(--ink-900);
    font-weight: 600;
  }

  .rail-item.active::before {
    content: "";
    position: absolute;
    inset: 3px auto 3px 0;
    width: 2px;
    border-radius: 1px;
    background: var(--accent-500);
  }

  .rail-item.active > :first-child {
    color: var(--accent-600);
  }

  .rail-connection {
    margin-top: auto;
    padding-top: 0.5rem;
    border-top: 1px solid var(--line);
  }

  .status-line {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    margin: 1px 0 0;
    padding: 0.25rem 0.5rem;
    width: 100%;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--ink-700);
    font: inherit;
    font-size: 0.6875rem;
    font-weight: 500;
    text-align: left;
    cursor: pointer;
  }

  .status-line:hover {
    background: var(--canvas-muted);
  }

  .status-line:first-of-type {
    margin-top: 0.375rem;
  }

  .connection-dot.warn {
    background: var(--warn-500);
  }

  .connection-dot.bad {
    background: var(--danger-500);
  }

  .sync-current {
    margin: 0.25rem 0 0 1.25rem;
    color: var(--ok-600);
    font-size: 0.5625rem;
    font-weight: 500;
    letter-spacing: 0.04em;
  }

  .sync-current.warn {
    color: var(--ink-500);
  }

  .workspace-topbar {
    grid-column: 2;
    grid-row: 1;
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-width: 0;
    height: 2.75rem;
    padding: 0 var(--shell-main-pad-x);
    border-bottom: 1px solid var(--line);
    background: var(--canvas);
  }

  /* Langfuse "Home" title: ~18px / 600, not a heavy app chrome bar. */
  .page-heading {
    margin: 0;
    font-size: 1.125rem;
    font-weight: 600;
    line-height: 1.25;
    letter-spacing: -0.02em;
    color: var(--ink-900);
  }

  .topbar-trailing {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    min-width: 0;
  }

  .product-ledger-main {
    grid-column: 2;
    grid-row: 2;
    min-width: 0;
    padding: var(--shell-main-pad-y) var(--shell-main-pad-x) var(--sp-6);
    background: var(--canvas);
    overflow: auto;
  }

  .product-group + .product-group {
    margin-top: var(--sp-4);
  }

  .empty-icon {
    color: var(--ink-400);
    font-size: 1.5rem;
    line-height: 1;
  }

  .table-frame {
    /* panel-frame supplies border; keep alias for tests / mobile overrides */
  }

  .project-table {
    width: 100%;
    border-collapse: collapse;
    table-layout: fixed;
  }

  .product-column { width: 31%; }
  .store-column { width: 29%; }
  .status-column { width: 15%; }
  .updated-column { width: 13%; }
  .action-column { width: 12%; }

  .project-table thead {
    background: var(--canvas-muted);
  }

  .project-table thead th {
    height: 32px;
    padding: 0 0.75rem;
    border-bottom: 1px solid var(--line);
    color: var(--ink-500);
    font-size: 0.625rem;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-align: left;
    text-transform: uppercase;
  }

  .project-table tbody tr {
    border-bottom: 1px solid var(--line);
    background: var(--surface);
    transition: background var(--dur-fast) var(--ease-out);
  }

  .project-table tbody tr:hover {
    background: var(--canvas-muted);
  }

  .project-table tbody tr:last-child {
    border-bottom: none;
  }

  .project-table tbody th,
  .project-table tbody td {
    height: 40px;
    padding: 0.375rem 0.75rem;
    color: var(--ink-500);
    font-size: var(--text-sm);
    text-align: left;
    vertical-align: middle;
  }

  .project-table tbody th.project-name {
    color: var(--ink-900);
    font-size: var(--text-sm);
    font-weight: 600;
  }

  .col-updated {
    color: var(--ink-400);
    font-size: var(--text-xs);
  }

  .col-action {
    text-align: right !important;
  }

  .readiness-badge,
  .available-status {
    display: inline-flex;
    min-height: 22px;
    align-items: center;
    padding: 0 var(--sp-2);
    border: 1px solid currentColor;
    border-radius: var(--radius-sm);
    background: var(--surface);
    font-size: var(--text-xs);
    font-weight: 600;
    white-space: nowrap;
  }

  .readiness-badge.ready {
    color: var(--ok-600);
  }

  .readiness-badge.needs-access {
    color: var(--warn-600);
  }

  .readiness-badge.offline {
    color: var(--ink-500);
  }

  .available-status {
    color: var(--accent-700);
  }

  .row-action {
    min-width: 64px;
    height: 28px;
    padding: 0 0.625rem;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-md);
    background: var(--surface);
    color: var(--ink-700);
    font-family: inherit;
    font-size: var(--text-sm);
    font-weight: 600;
    cursor: pointer;
    transition:
      background var(--dur-fast) var(--ease-out),
      border-color var(--dur-fast) var(--ease-out),
      color var(--dur-fast) var(--ease-out);
  }

  .row-action:hover {
    border-color: var(--ink-400);
    background: var(--canvas-muted);
    color: var(--ink-900);
  }

  .row-action.repair,
  .row-action.join {
    border-color: var(--ink-900);
    background: var(--ink-900);
    color: #fff;
  }

  .row-action.repair:hover,
  .row-action.join:hover {
    border-color: var(--ink-700);
    background: var(--ink-700);
  }

  .row-action:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .join-failure {
    display: inline-block;
    color: var(--danger-600);
    font-size: var(--text-xs);
    line-height: 1.35;
  }

  .project-table tbody tr.has-failure th,
  .project-table tbody tr.has-failure td {
    height: 48px;
  }

  @media (max-width: 900px) {
    .product-ledger-main {
      padding-right: var(--sp-4);
      padding-left: var(--sp-4);
    }
  }

  @media (max-width: 720px) {
    /* Keep HOST + Settings reachable on narrow: dock the rail as a bottom bar.
       Dual footer IA (Storage + Agent host) must not disappear on mobile. */
    .operator-home {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: var(--shell-topbar-h) minmax(0, 1fr) auto;
      min-height: 100vh;
    }

    .workspace-topbar {
      grid-column: 1;
      grid-row: 1;
      display: flex;
      padding: 0 var(--sp-3);
    }

    .navigation-rail {
      grid-column: 1;
      grid-row: 3;
      position: sticky;
      bottom: 0;
      z-index: 5;
      flex-direction: row;
      flex-wrap: wrap;
      align-items: center;
      gap: var(--sp-2) var(--sp-3);
      min-height: 0;
      padding: var(--sp-2) var(--sp-3) calc(var(--sp-2) + env(safe-area-inset-bottom, 0px));
      border-right: none;
      border-top: 1px solid var(--line);
      background: var(--surface);
    }

    .rail-items {
      display: flex;
      flex: 1 1 auto;
      gap: var(--sp-1);
      margin-top: 0;
      padding: 0;
    }

    .rail-item {
      min-height: 32px;
      padding: 0 var(--sp-2);
      font-size: var(--text-sm);
    }

    .rail-btn {
      min-height: 32px;
      padding: 0 var(--sp-2);
      margin: 0;
    }

    .rail-connection {
      display: flex;
      flex: 1 1 100%;
      flex-wrap: wrap;
      align-items: center;
      gap: var(--sp-1) var(--sp-2);
      margin-top: 0;
      padding-top: var(--sp-2);
      border-top: 1px solid var(--line);
    }

    .rail-connection .rail-label {
      padding: 0 var(--sp-1) 0 0;
    }

    .status-line,
    .status-line:first-of-type {
      width: auto;
      margin: 0;
      padding: var(--sp-2);
      font-size: 0.6875rem;
    }

    .sync-current {
      margin: 0 0 0 var(--sp-1);
      font-size: 0.5rem;
    }

    .product-ledger-main {
      grid-column: 1;
      grid-row: 2;
      padding: var(--sp-4) var(--sp-4) var(--sp-5);
    }

    .table-frame {
      overflow: visible;
      border: none;
      border-radius: 0;
      background: transparent;
    }

    .project-table,
    .project-table tbody {
      display: block;
      width: 100%;
    }

    .project-table colgroup {
      display: none;
    }

    .project-table thead {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip: rect(0 0 0 0);
      clip-path: inset(50%);
      white-space: nowrap;
    }

    .project-table tbody {
      display: grid;
      gap: var(--sp-3);
    }

    .project-table tbody tr {
      display: grid;
      grid-template-columns: minmax(0, 1fr) auto;
      gap: var(--sp-2) var(--sp-4);
      min-height: 0;
      padding: var(--sp-4);
      border: 1px solid var(--line);
      border-radius: var(--radius-lg);
      background: var(--surface);
    }

    .project-table tbody tr:hover {
      background: var(--surface);
    }

    .project-table tbody th,
    .project-table tbody td,
    .project-table tbody tr.has-failure th,
    .project-table tbody tr.has-failure td {
      display: block;
      width: auto;
      height: auto;
      padding: 0;
    }

    .project-table tbody th.project-name {
      grid-column: 1;
      grid-row: 1;
      align-self: center;
    }

    .project-table tbody td::before {
      content: attr(data-label);
      display: block;
      margin-bottom: 3px;
      color: var(--ink-400);
      font-family: var(--font-mono);
      font-size: 0.5625rem;
      font-weight: 500;
      letter-spacing: 0.03em;
      text-transform: uppercase;
    }

    .project-table tbody .col-store,
    .project-table tbody .col-status,
    .project-table tbody .col-updated {
      grid-column: 1;
    }

    .project-table tbody .col-action {
      grid-column: 2;
      grid-row: 1;
      align-self: start;
    }

    .project-table tbody .col-action::before {
      content: none;
    }

    .readiness-badge,
    .available-status {
      min-width: 0;
    }
  }

  @media (max-width: 460px) {
    .project-table tbody tr {
      grid-template-columns: minmax(0, 1fr);
    }

    .project-table tbody .col-action {
      grid-column: 1;
      grid-row: auto;
      justify-self: start;
      margin-top: var(--sp-1);
    }
  }
</style>
