<script lang="ts">
  import {
    createWorkspace,
    fetchConnectionRegistry,
    startConnection,
    pollConnectionStatus,
    type CreateWorkspaceResponse,
  } from "../lib/api";
  import {
    type Connection,
    type DiscoveredProject,
    type EventStoreKind,
  } from "../lib/connection-fixtures";
  import type {
    ConnectionMetadata,
    ConnectionStartOutcome,
  } from "../lib/types";
  import {
    type ProviderSetupState,
    type SharedDriveEntry,
    type GoogleDriveReadiness,
    type GitHubAppReadiness,
    SETUP_FIXTURES,
  } from "../lib/provider-setup-fixtures";

  let { oncreated, ongosettings }: { oncreated: () => void; ongosettings: () => void } = $props();

  type SetupTab = "create" | "join";

  let activeTab = $state<SetupTab>("create");

  // ── Create flow state ──────────────────────────────────────────
  let projectName = $state("");
  let selectedEventStore = $state<EventStoreKind>("github");
  let creating = $state(false);
  let createResult = $state<CreateWorkspaceResponse | null>(null);

  // ── Inline connection state ────────────────────────────────────
  let connecting = $state(false);
  let connectingProvider = $state<string | null>(null);
  let connectionError = $state<string | null>(null);

  // ── Provider resource readiness ────────────────────────────────
  let githubSetup = $state<ProviderSetupState>(SETUP_FIXTURES.github.not_connected);
  let driveSetup = $state<ProviderSetupState>(SETUP_FIXTURES.google_drive.not_connected);
  let selectedDriveId = $state<string | null>(null);

  const GITHUB_RESOURCE_DEFERRED: ProviderSetupState = {
    provider_id: "github",
    connection_status: "connected",
    resource_readiness: {
      state: "action_needed",
      message: "GitHub project resource discovery is not available in this daemon build.",
    } as GitHubAppReadiness,
  };

  const DRIVE_RESOURCE_DEFERRED: ProviderSetupState = {
    provider_id: "google",
    connection_status: "connected",
    resource_readiness: {
      state: "action_needed",
      eligible_shared_drives: [],
      domain_valid: true,
      message: "Google Drive project discovery is not available in this daemon build.",
    } as GoogleDriveReadiness,
  };

  // ── Join flow state ────────────────────────────────────────────
  let joinProvider = $state<EventStoreKind>("github");
  let directLink = $state("");
  let searching = $state(false);
  let discoveredProjects = $state<DiscoveredProject[]>([]);
  let hasSearched = $state(false);
  let selectedProjectId = $state<string | null>(null);
  let joining = $state(false);
  let joinError = $state<string | null>(null);

  // ── Connection state (live registry) ─────
  let liveConnections = $state<Record<string, ConnectionMetadata>>({});
  let connectionsLoaded = $state(false);

async function refreshConnections() {
  try {
    const registry = await fetchConnectionRegistry();
    liveConnections = registry.connections;
  } catch {
    liveConnections = {};
  } finally {
    connectionsLoaded = true;
  }
  updateProviderSetupStates();
}

  $effect(() => {
    refreshConnections();
  });

  function updateProviderSetupStates() {
    const ghMeta = liveConnections["github"];
    const gdMeta = liveConnections["google"];

    if (ghMeta) {
      if (ghMeta.status === "connected") {
        githubSetup = GITHUB_RESOURCE_DEFERRED;
      } else if (ghMeta.status === "action_needed") {
        githubSetup = SETUP_FIXTURES.github.action_needed;
      } else if (ghMeta.status === "unsupported") {
        githubSetup = SETUP_FIXTURES.github.unsupported;
      } else {
        githubSetup = SETUP_FIXTURES.github.not_connected;
      }
    }

    if (gdMeta) {
      if (gdMeta.status === "connected") { driveSetup = DRIVE_RESOURCE_DEFERRED;
      } else if (gdMeta.status === "action_needed") {
        driveSetup = SETUP_FIXTURES.google_drive.action_needed;
      } else if (gdMeta.status === "unsupported") {
        driveSetup = SETUP_FIXTURES.google_drive.unsupported;
      } else {
        driveSetup = SETUP_FIXTURES.google_drive.not_connected;
      }
    }
  }

  function connectionFor(providerId: string): Connection | undefined {
    const meta = liveConnections[providerId];
    if (!meta) return undefined;
    return {
      provider_id: providerId,
      status: meta.status,
      account_label: meta.account_label,
      capabilities: meta.capabilities,
      updated_at: meta.updated_at,
      auth_method: meta.auth_method,
    };
  }

  function isConnected(providerId: string): boolean {
    const meta = liveConnections[providerId];
    return meta?.status === "connected";
  }

  function providerLabel(kind: EventStoreKind): string {
    return kind === "github" ? "GitHub" : "Google Drive";
  }

  function eventStoreDescription(kind: EventStoreKind): string {
    return kind === "github"
      ? "Install the Bicameral GitHub App to use GitHub as the project event store."
      : "Use a shared drive as the project event store.";
  }

  function connectActionLabel(kind: EventStoreKind): string {
    return kind === "github" ? "Install Bicameral GitHub App" : "Connect Google Drive";
  }

  function readinessLabel(r: string): string {
    switch (r) {
      case "ready": return "Ready";
      case "needs_access": return "Needs access";
      case "incomplete": return "Incomplete";
      case "offline": return "Offline";
      default: return r;
    }
  }

  function eventStoreLabel(es: EventStoreKind): string {
    return es === "github" ? "GitHub" : "Google Drive";
  }

  function formatDate(iso?: string): string {
    if (!iso) return "—";
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

  // ── Inline connect flow ────────────────────────────────────────

  async function handleInlineConnect(kind: EventStoreKind) {
    const providerId = kind === "google_drive" ? "google" : "github";
    connecting = true;
    connectingProvider = providerId;
    connectionError = null;

    try {
      const outcome: ConnectionStartOutcome = await startConnection(providerId);

      if (outcome.outcome === "connected") {
        // Already connected
        await refreshConnections();
      } else if (outcome.outcome === "start_url") {
        // Open auth URL — in production this opens a popup/redirect
        // For fixture-backed flow, simulate completion after delay
        window.open(outcome.url, "_blank", "width=600,height=700");
        // Poll for completion
        const sessionId = outcome.auth_session_id;
        await pollUntilConnected(providerId, sessionId);
      } else if (outcome.outcome === "action_needed") {
        connectionError = outcome.message;
        if (providerId === "github") {
          githubSetup = SETUP_FIXTURES.github.action_needed;
        } else {
          driveSetup = SETUP_FIXTURES.google_drive.action_needed;
        }
      } else if (outcome.outcome === "unsupported") {
        connectionError = outcome.message;
        if (providerId === "github") {
          githubSetup = SETUP_FIXTURES.github.unsupported;
        } else {
          driveSetup = SETUP_FIXTURES.google_drive.unsupported;
        }
      }
    } catch (e: unknown) {
      connectionError = e instanceof Error ? e.message : "Connection failed. Please try again.";
      if (providerId === "github") {
        githubSetup = SETUP_FIXTURES.github.reconnect_after_error;
      } else {
        driveSetup = SETUP_FIXTURES.google_drive.reconnect_after_error;
      }
    } finally {
      connecting = false;
      connectingProvider = null;
    }
  }

  async function pollUntilConnected(providerId: string, sessionId: string) {
    const maxAttempts = 60;
    for (let i = 0; i < maxAttempts; i++) {
      await new Promise((r) => setTimeout(r, 2000));
      try {
        const result = await pollConnectionStatus(providerId, sessionId);
        if (result.outcome === "connected") {
          await refreshConnections();
          return;
        } else if (result.outcome === "denied" || result.outcome === "expired") {
          connectionError = result.message;
          return;
        } else if (result.outcome === "action_needed") {
          connectionError = result.message;
          return;
        }
        // "pending" — keep polling
      } catch {
        // Network error during poll — continue
      }
    }
    connectionError = "Connection timed out. Please try again.";
  }

  // ── Provider resource readiness helpers ─────────────────────────

  function currentSetup(kind: EventStoreKind): ProviderSetupState {
    return kind === "github" ? githubSetup : driveSetup;
  }

  function isResourceReady(kind: EventStoreKind): boolean {
    const setup = currentSetup(kind);
    if (setup.connection_status !== "connected") return false;
    const readiness = setup.resource_readiness;
    if (readiness.state === "ready") return true;
    // For Drive with pending_selection, check if a drive is selected
    if (kind === "google_drive" && readiness.state === "pending_selection") {
      return selectedDriveId !== null;
    }
    return false;
  }

  function driveReadiness(): GoogleDriveReadiness | null {
    if (driveSetup.resource_readiness && "eligible_shared_drives" in driveSetup.resource_readiness) {
      return driveSetup.resource_readiness as GoogleDriveReadiness;
    }
    return null;
  }

  function githubReadiness(): GitHubAppReadiness | null {
    if (githubSetup.resource_readiness && !("eligible_shared_drives" in githubSetup.resource_readiness)) {
      return githubSetup.resource_readiness as GitHubAppReadiness;
    }
    return null;
  }

  function driveProjectPath(driveName: string): string {
    const name = projectName || "<project-name>";
    return `${driveName}/.bicameral/${name}/`;
  }

  function handleDriveSelect(driveId: string) {
    selectedDriveId = driveId;
  }

  // ── Create flow ────────────────────────────────────────────────

  let createProviderConnected = $derived(isConnected(
    selectedEventStore === "google_drive" ? "google" : "github"
  ));

  let createEnabled = $derived(
    createProviderConnected && isResourceReady(selectedEventStore) && projectName.trim().length > 0
  );

  async function handleCreate() {
    creating = true;
    createResult = null;
    try {
      const res = await createWorkspace({
        workspace_name: projectName || undefined,
      });
      createResult = res;
      if (res.success) {
        oncreated();
      }
    } catch (e: unknown) {
      createResult = {
        success: false,
        config_path: null,
        errors: [e instanceof Error ? e.message : "Unknown error"],
      };
    } finally {
      creating = false;
    }
  }

  // ── Join flow ──────────────────────────────────────────────────

  let joinProviderConnected = $derived(isConnected(
    joinProvider === "google_drive" ? "google" : "github"
  ));

  function handleFindProjects() {
    searching = true;
    hasSearched = false;
    selectedProjectId = null;
    joinError = null;
    // No hosted project-discovery endpoint exists in this slice; return an honest empty state.
    setTimeout(() => {
      discoveredProjects = [];
      hasSearched = true;
      searching = false;
    }, 600);
  }

  function handleJoinProject() {
    if (!selectedProjectId) return;
    const project = discoveredProjects.find((p) => p.id === selectedProjectId);
    if (!project) return;

    // Validate provider access
    const provId = project.event_store === "google_drive" ? "google" : "github";
    if (!isConnected(provId)) {
      joinError = `${providerLabel(project.event_store)} is not connected.`;
      return;
    }
    // Validate event-store marker readiness
    if (project.readiness !== "ready") {
      joinError = `Project "${project.name}" is not ready (${readinessLabel(project.readiness).toLowerCase()}).`;
      return;
    }

    joining = true;
    joinError = null;
    // Simulate join
    setTimeout(() => {
      joining = false;
      oncreated();
    }, 800);
  }
</script>

<div class="setup-wizard">
  <div class="setup-card">
    <h1>Welcome to Bicameral</h1>
    <p class="subtitle">
      Set up a project to start capturing decisions and grounding them in code.
    </p>

    <div class="tab-bar">
      <button
        class="tab"
        class:active={activeTab === "create"}
        onclick={() => (activeTab = "create")}
      >
        Create
      </button>
      <button
        class="tab"
        class:active={activeTab === "join"}
        onclick={() => (activeTab = "join")}
      >
        Join
      </button>
    </div>

    <!-- ── Create tab ──────────────────────────────────────────── -->
    {#if activeTab === "create"}
      <div class="tab-content">
        <label class="field">
          <span>Project name</span>
          <input
            type="text"
            bind:value={projectName}
            placeholder="e.g. acme-checkout"
          />
        </label>

        <div class="event-store-section">
          <span class="section-label">Event store</span>
          <div class="event-store-options">
            {#each (["github", "google_drive"] as EventStoreKind[]) as es}
              {@const provId = es === "google_drive" ? "google" : "github"}
              {@const conn = connectionFor(provId)}
              {@const connected = conn?.status === "connected"}
              {@const setup = currentSetup(es)}
              {@const isUnsupported = setup.connection_status === "unsupported"}
              <label class="event-store-option" class:selected={selectedEventStore === es} class:unsupported={isUnsupported}>
                <input
                  type="radio"
                  name="event-store"
                  value={es}
                  bind:group={selectedEventStore}
                  disabled={isUnsupported}
                />
                <div class="option-body">
                  <span class="option-label">{providerLabel(es)}</span>
                  <span class="option-desc">{eventStoreDescription(es)}</span>
                  <span class="conn-status" class:connected class:disconnected={!connected && !isUnsupported} class:unsupported={isUnsupported}>
                    {#if isUnsupported}
                      Not available
                    {:else if connected}
                      Connected
                    {:else if setup.connection_status === "action_needed"}
                      Action needed
                    {:else}
                      Not connected
                    {/if}
                  </span>
                </div>
              </label>
            {/each}
          </div>
        </div>

        <!-- Provider resource readiness section -->
        {#if createProviderConnected && selectedEventStore === "github"}
          {@const ghReady = githubReadiness()}
          {#if ghReady}
            <div class="readiness-section">
              <span class="readiness-badge" class:ready={ghReady.state === "ready"} class:warn={ghReady.state === "action_needed"}>
                {ghReady.state === "ready" ? "Ready" : "Action needed"}
              </span>
              <span class="readiness-message">{ghReady.message}</span>
            </div>
          {/if}
        {/if}

        {#if createProviderConnected && selectedEventStore === "google_drive"}
          {@const dr = driveReadiness()}
          {#if dr}
            {#if !dr.domain_valid}
              <div class="callout callout-error">
                <p>{dr.message}</p>
              </div>
            {:else if dr.eligible_shared_drives.length === 1}
              <div class="readiness-section">
                <span class="readiness-badge ready">Ready</span>
                <span class="readiness-message">
                  Shared drive: {dr.eligible_shared_drives[0].name}
                </span>
              </div>
              <div class="callout callout-info">
                <p class="path-convention">
                  Project files will be stored at: <code>{driveProjectPath(dr.eligible_shared_drives[0].name)}</code>
                </p>
              </div>
            {:else if dr.eligible_shared_drives.length > 1}
              <div class="drive-selector">
                <span class="section-label">Select shared drive</span>
                <div class="drive-options">
                  {#each dr.eligible_shared_drives as drive}
                    <label class="drive-option" class:selected={selectedDriveId === drive.id}>
                      <input
                        type="radio"
                        name="shared-drive"
                        value={drive.id}
                        checked={selectedDriveId === drive.id}
                        onchange={() => handleDriveSelect(drive.id)}
                      />
                      <span class="drive-name">{drive.name}</span>
                      <span class="drive-domain">{drive.domain}</span>
                    </label>
                  {/each}
                </div>
              </div>
              {#if selectedDriveId}
                {@const selectedDrive = dr.eligible_shared_drives.find(d => d.id === selectedDriveId)}
                {#if selectedDrive}
                  <div class="callout callout-info">
                    <p class="path-convention">
                      Project files will be stored at: <code>{driveProjectPath(selectedDrive.name)}</code>
                    </p>
                  </div>
                {/if}
              {/if}
            {:else}
              <div class="callout callout-warn">
                <p>No eligible shared drives found. Team projects require at least one shared drive.</p>
              </div>
            {/if}
          {/if}
        {/if}

        <!-- Inline connect notice and action -->
        {#if !createProviderConnected}
          {@const setup = currentSetup(selectedEventStore)}
          {#if setup.connection_status === "unsupported"}
            <div class="callout callout-error">
              <p>{setup.resource_readiness.message}</p>
            </div>
          {:else}
            <p class="connect-notice">
              {connectActionLabel(selectedEventStore)} to continue.
            </p>
            {#if connectionError}
              <div class="error-box">
                <p>{connectionError}</p>
              </div>
            {/if}
          {/if}
        {/if}

        <div class="actions-row">
          {#if createProviderConnected && createEnabled}
            <button class="primary" onclick={handleCreate} disabled={creating}>
              {creating ? "Creating..." : "Create project"}
            </button>
          {:else if createProviderConnected && !createEnabled}
            <button class="primary" disabled>
              Create project
            </button>
          {:else if currentSetup(selectedEventStore).connection_status !== "unsupported"}
            <button
              class="primary"
              onclick={() => handleInlineConnect(selectedEventStore)}
              disabled={connecting}
            >
              {#if connecting && connectingProvider === (selectedEventStore === "google_drive" ? "google" : "github")}
                Connecting...
              {:else}
                {connectActionLabel(selectedEventStore)}
              {/if}
            </button>
          {/if}
        </div>

        {#if createResult && !createResult.success}
          <div class="error-box">
            {#each createResult.errors as err}
              <p>{err}</p>
            {/each}
          </div>
        {/if}

        {#if createResult?.success}
          <div class="success-box">
            <p>Project created at <code>{createResult.config_path}</code></p>
          </div>
        {/if}
      </div>
    {/if}

    <!-- ── Join tab ────────────────────────────────────────────── -->
    {#if activeTab === "join"}
      <div class="tab-content">
        <div class="provider-selector">
          <span class="section-label">Provider</span>
          <div class="provider-options">
            {#each (["github", "google_drive"] as EventStoreKind[]) as es}
              <label class="provider-option" class:selected={joinProvider === es}>
                <input
                  type="radio"
                  name="join-provider"
                  value={es}
                  bind:group={joinProvider}
                />
                <span>{providerLabel(es)}</span>
              </label>
            {/each}
          </div>
        </div>

        {#if !joinProviderConnected}
          <p class="connect-notice">
            {connectActionLabel(joinProvider)} to discover projects.
          </p>
          {#if connectionError}
            <div class="error-box">
              <p>{connectionError}</p>
            </div>
          {/if}
          <div class="actions-row">
            <button
              class="primary"
              onclick={() => handleInlineConnect(joinProvider)}
              disabled={connecting}
            >
              {#if connecting && connectingProvider === (joinProvider === "google_drive" ? "google" : "github")}
                Connecting...
              {:else}
                {connectActionLabel(joinProvider)}
              {/if}
            </button>
          </div>
        {:else}
          <div class="actions-row">
            <button
              class="secondary"
              onclick={handleFindProjects}
              disabled={searching}
            >
              {searching ? "Searching..." : "Find projects"}
            </button>
          </div>

          {#if hasSearched}
            {#if discoveredProjects.length === 0}
              <p class="empty-state">No Bicameral projects found for {providerLabel(joinProvider)}.</p>
            {:else}
              <div class="discovered-list">
                {#each discoveredProjects as project}
                  <label
                    class="discovered-row"
                    class:selected={selectedProjectId === project.id}
                  >
                    <input
                      type="radio"
                      name="discovered-project"
                      value={project.id}
                      bind:group={selectedProjectId}
                    />
                    <div class="project-info">
                      <span class="project-name">{project.name}</span>
                      <span class="project-meta">
                        {eventStoreLabel(project.event_store)} · {project.provider_resource}
                      </span>
                    </div>
                    <div class="project-status">
                      <span class="readiness" class:ready={project.readiness === "ready"}
                        class:warn={project.readiness === "needs_access" || project.readiness === "incomplete"}
                        class:off={project.readiness === "offline"}>
                        {readinessLabel(project.readiness)}
                      </span>
                      <span class="last-updated">{formatDate(project.last_updated)}</span>
                    </div>
                  </label>
                {/each}
              </div>
            {/if}
          {/if}

          <label class="field direct-link-field">
            <span>Or enter a repo or Drive link directly</span>
            <input
              type="text"
              bind:value={directLink}
              placeholder={joinProvider === "github"
                ? "e.g. https://github.com/org/repo"
                : "e.g. https://drive.google.com/drive/folders/..."}
            />
          </label>

          {#if joinError}
            <div class="error-box">
              <p>{joinError}</p>
            </div>
          {/if}

          <div class="actions-row">
            <button
              class="primary"
              onclick={handleJoinProject}
              disabled={joining || (!selectedProjectId && !directLink)}
            >
              {joining ? "Joining..." : "Join project"}
            </button>
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .setup-wizard {
    display: flex;
    justify-content: center;
    align-items: flex-start;
    min-height: 80vh;
    padding: 3rem 1rem;
  }

  .setup-card {
    max-width: 560px;
    width: 100%;
  }

  h1 {
    font-size: 1.5rem;
    font-weight: 700;
    margin-bottom: 0.5rem;
  }

  .subtitle {
    color: var(--ink-500);
    font-size: 0.9rem;
    margin-bottom: 1.5rem;
  }

  /* ── Tabs ──────────────────────────────────────────────────── */

  .tab-bar {
    display: flex;
    gap: 0;
    border-bottom: 1px solid var(--line);
    margin-bottom: 1.25rem;
  }

  .tab {
    padding: 0.5rem 1.25rem;
    border: none;
    background: none;
    font-size: 0.875rem;
    font-family: inherit;
    color: var(--ink-500);
    cursor: pointer;
    border-bottom: 2px solid transparent;
    transition: all 0.12s;
  }

  .tab:hover {
    color: var(--ink-900);
  }

  .tab.active {
    color: var(--accent-500);
    font-weight: 600;
    border-bottom-color: var(--accent-500);
  }

  .tab-content {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  /* ── Fields ────────────────────────────────────────────────── */

  .field {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }

  .field span {
    font-size: 0.8rem;
    color: var(--ink-500);
  }

  input[type="text"] {
    padding: 0.5rem 0.75rem;
    border: 1px solid var(--line-strong);
    border-radius: 6px;
    font-size: 0.85rem;
    font-family: inherit;
  }

  input[type="text"]:focus {
    outline: none;
    border-color: var(--accent-500);
    box-shadow: 0 0 0 2px rgba(67, 56, 202, 0.1);
  }

  /* ── Event store selection ─────────────────────────────────── */

  .section-label {
    font-size: 0.8rem;
    color: var(--ink-500);
    display: block;
    margin-bottom: 0.5rem;
  }

  .event-store-options {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .event-store-option {
    display: flex;
    align-items: flex-start;
    gap: 0.75rem;
    padding: 0.75rem 1rem;
    border: 1px solid var(--line);
    border-radius: 8px;
    cursor: pointer;
    background: #fff;
    transition: border-color 0.12s;
  }

  .event-store-option:hover {
    border-color: var(--accent-200);
  }

  .event-store-option.selected {
    border-color: var(--accent-500);
    background: var(--memory-50);
  }

  .event-store-option.unsupported {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .event-store-option input[type="radio"] {
    margin-top: 0.15rem;
  }

  .option-body {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }

  .option-label {
    font-size: 0.875rem;
    font-weight: 600;
    color: var(--ink-900);
  }

  .option-desc {
    font-size: 0.8rem;
    color: var(--ink-500);
  }

  .conn-status {
    font-size: 0.75rem;
    font-weight: 500;
    margin-top: 0.15rem;
  }

  .conn-status.connected {
    color: var(--ok-600);
  }

  .conn-status.disconnected {
    color: var(--danger-600);
  }

  .conn-status.unsupported {
    color: var(--ink-500);
  }

  .connect-notice {
    font-size: 0.82rem;
    color: var(--warn-600);
    background: var(--warn-50);
    border: 1px solid #fde68a;
    border-radius: 6px;
    padding: 0.5rem 0.75rem;
  }

  /* ── Provider readiness ────────────────────────────────────── */

  .readiness-section {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 0;
  }

  .readiness-badge {
    font-size: 0.72rem;
    font-weight: 600;
    padding: 0.15rem 0.5rem;
    border-radius: 9999px;
    text-transform: uppercase;
    letter-spacing: 0.02em;
  }

  .readiness-badge.ready {
    color: var(--ok-600);
    background: var(--ok-50);
  }

  .readiness-badge.warn {
    color: var(--warn-600);
    background: var(--warn-50);
  }

  .readiness-message {
    font-size: 0.8rem;
    color: var(--ink-700);
  }

  /* ── Callouts ──────────────────────────────────────────────── */

  .callout {
    padding: 0.6rem 0.85rem;
    border-radius: 6px;
    font-size: 0.8rem;
  }

  .callout p {
    margin: 0;
  }

  .callout-info {
    background: var(--accent-50);
    border: 1px solid var(--accent-200);
    color: var(--accent-700);
  }

  .callout-warn {
    background: var(--warn-50);
    border: 1px solid #fde68a;
    color: var(--warn-600);
  }

  .callout-error {
    background: var(--danger-50);
    border: 1px solid #fecaca;
    color: var(--danger-600);
  }

  .path-convention code {
    background: rgba(0, 0, 0, 0.06);
    padding: 0.1em 0.35em;
    border-radius: 3px;
    font-size: 0.82em;
  }

  /* ── Drive selector ────────────────────────────────────────── */

  .drive-selector {
    display: flex;
    flex-direction: column;
  }

  .drive-options {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }

  .drive-option {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 0.75rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    cursor: pointer;
    font-size: 0.82rem;
    background: #fff;
    transition: border-color 0.12s;
  }

  .drive-option:hover {
    border-color: var(--accent-200);
  }

  .drive-option.selected {
    border-color: var(--accent-500);
    background: var(--memory-50);
  }

  .drive-name {
    font-weight: 500;
    color: var(--ink-900);
  }

  .drive-domain {
    font-size: 0.72rem;
    color: var(--ink-500);
    margin-left: auto;
  }

  /* ── Provider selector (Join) ──────────────────────────────── */

  .provider-options {
    display: flex;
    gap: 0.5rem;
  }

  .provider-option {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 1rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    cursor: pointer;
    font-size: 0.85rem;
    background: #fff;
    transition: border-color 0.12s;
  }

  .provider-option:hover {
    border-color: var(--accent-200);
  }

  .provider-option.selected {
    border-color: var(--accent-500);
    background: var(--memory-50);
    font-weight: 600;
  }

  .provider-option input[type="radio"] {
    display: none;
  }

  /* ── Discovered projects ───────────────────────────────────── */

  .discovered-list {
    display: flex;
    flex-direction: column;
    gap: 0;
    border: 1px solid var(--line);
    border-radius: 8px;
    overflow: hidden;
  }

  .discovered-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.65rem 1rem;
    cursor: pointer;
    background: #fff;
    border-bottom: 1px solid var(--canvas-muted);
    transition: background 0.1s;
  }

  .discovered-row:last-child {
    border-bottom: none;
  }

  .discovered-row:hover {
    background: var(--canvas-muted);
  }

  .discovered-row.selected {
    background: var(--memory-50);
  }

  .discovered-row input[type="radio"] {
    flex-shrink: 0;
  }

  .project-info {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    flex: 1;
    min-width: 0;
  }

  .project-name {
    font-size: 0.85rem;
    font-weight: 600;
    color: var(--ink-900);
  }

  .project-meta {
    font-size: 0.75rem;
    color: var(--ink-500);
  }

  .project-status {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 0.1rem;
    flex-shrink: 0;
  }

  .readiness {
    font-size: 0.75rem;
    font-weight: 500;
    padding: 0.1rem 0.5rem;
    border-radius: 9999px;
  }

  .readiness.ready {
    color: var(--ok-600);
    background: var(--ok-50);
  }

  .readiness.warn {
    color: var(--warn-600);
    background: var(--warn-50);
  }

  .readiness.off {
    color: var(--ink-500);
    background: var(--canvas-muted);
  }

  .last-updated {
    font-size: 0.7rem;
    color: var(--ink-400);
  }

  .empty-state {
    font-size: 0.82rem;
    color: var(--ink-500);
    padding: 0.75rem 0;
  }

  .direct-link-field {
    margin-top: 0.5rem;
  }

  /* ── Buttons ───────────────────────────────────────────────── */

  .actions-row {
    display: flex;
    gap: 0.5rem;
  }

  button.primary {
    padding: 0.5rem 1.25rem;
    background: var(--accent-500);
    color: #fff;
    border: none;
    border-radius: 6px;
    font-size: 0.85rem;
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

  button.secondary {
    padding: 0.5rem 1.25rem;
    background: #fff;
    color: var(--accent-500);
    border: 1px solid var(--accent-500);
    border-radius: 6px;
    font-size: 0.85rem;
    font-family: inherit;
    cursor: pointer;
    transition: all 0.12s;
  }

  button.secondary:hover:not(:disabled) {
    background: var(--memory-50);
  }

  button.secondary:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  /* ── Result boxes ──────────────────────────────────────────── */

  .error-box {
    padding: 0.75rem 1rem;
    background: var(--danger-50);
    border: 1px solid #fecaca;
    border-radius: 6px;
    color: var(--danger-600);
    font-size: 0.82rem;
  }

  .success-box {
    padding: 0.75rem 1rem;
    background: var(--ok-50);
    border: 1px solid #bbf7d0;
    border-radius: 6px;
    color: var(--ok-600);
    font-size: 0.82rem;
  }

  code {
    background: var(--canvas-muted);
    padding: 0.1em 0.35em;
    border-radius: 3px;
    font-size: 0.82em;
  }
</style>
