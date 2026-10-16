<script lang="ts">
  /**
   * Hosted Dashboard — paired hosted admin surface.
   *
   * Factory #488 Option B: Cloud `/connect` is sole pre-pair. Bot `/app` is
   * post-handoff product only. Host configuration (Storage + Agent host) lives
   * under Settings with a host status strip — not a Products home essay.
   *
   * PRD: docs/prd/hosted-operator-host-configuration.md
   * Spec refs: docs/specs/alpha-hosted-dashboard-ui.md, #1092, #955, #1038.
   */
  import type {
    HostedBridgeUIState,
    HostedDashboardCompatibility,
  } from "../lib/hosted-bridge-types";
  import type { ProjectEntry } from "../lib/project-fixtures";
  import type { McpReadinessState } from "../lib/mcp-readiness";
  import type { DriveOAuthPhase } from "../ui";
  import OperatorHome from "./OperatorHome.svelte";
  import HostedSettings from "./HostedSettings.svelte";
  import type { SettingsSection } from "./HostedSettings.svelte";
  import HostedProjectRegistration from "./HostedProjectRegistration.svelte";
  import HostedProjectShell from "./HostedProjectShell.svelte";
  import {
    getPendingHostedCreateInput,
    type RegistrationOutcome,
  } from "../lib/hosted-project-registration";
  import type {
    ProductJoinFailureView,
    ProductJoinOutcome,
  } from "../lib/product-join";
  import type { HostedSetupProjection } from "../lib/api";
  import { applySetupProjection } from "../lib/hosted-bridge-client";
  import WorkspaceBindingGate from "./WorkspaceBindingGate.svelte";
  import HostedCompatibilityNotice from "./HostedCompatibilityNotice.svelte";

  type Props = {
    bridgeState: HostedBridgeUIState;
    sessionToken?: string;
    projects?: ProjectEntry[];
    pairingCode?: string | null;
    mcpState?: McpReadinessState;
    mcpHostId?: string;
    mcpReasons?: string[];
    /** Clipboard command for MCP setup (daemon argv when available). */
    mcpSetupCommand?: string;
    accountLabel?: string;
    /** Alpha event store (Google Drive) connected. */
    eventSubstrateConnected?: boolean;
    drivePhase?: DriveOAuthPhase;
    driveAccountLabel?: string;
    driveSessionId?: string | null;
    /** Daemon protocol compatibility result (#711); absent = assume current. */
    compatibility?: HostedDashboardCompatibility;
    /** Legacy pairing callbacks retained while callers migrate to Cloud-owned pre-pair. */
    onpair?: () => void;
    onconfirm?: () => void;
    oncancel?: () => void;
    onrepair?: () => void;
    onauthrestart?: () => void;
    onuselocal?: () => void;
    oncontinueproducts?: () => void;
    onrefreshmcp?: () => void;
    /** Re-run the compatibility handshake after the user updates the daemon. */
    oncompatretry?: () => void;
    /** In-flight state for daemon-canonical Product creation (bot#799). */
    registrationBusy?: boolean;
    /** Result of the last create/publication attempt. */
    registrationResult?: RegistrationOutcome | null;
    oncreateproject?: (input: {
      displayName: string;
      projectRef?: string;
    }) => void;
    /** Clear any prior create result (on open/cancel/done). */
    onregistrationreset?: () => void;
    /** Google Drive folder selection for Product create (#1264). */
    driveFolderPhase?: "idle" | "starting" | "picking" | "ready" | "failed";
    driveFolderName?: string;
    driveFolderMessage?: string;
    ondrivefolderselect?: () => void;
    ondrivefolderreset?: () => void;
    onjoinproduct?: (project: ProjectEntry) => Promise<ProductJoinOutcome>;
    oncontinuesetup?: (
      project: ProjectEntry,
    ) => Promise<HostedSetupProjection | null>;
    ondrivestart?: () => void;
    ondriveopenauth?: () => void;
    ondrivecancel?: () => void;
  };

  let {
    bridgeState,
    sessionToken,
    projects = [],
    pairingCode: _pairingCode = null,
    mcpState = "not_installed",
    mcpHostId = "claude",
    mcpReasons = [],
    mcpSetupCommand = "bicameral setup mcp",
    accountLabel: _accountLabel = "hosted",
    eventSubstrateConnected = false,
    drivePhase = "idle",
    driveAccountLabel = "",
    driveSessionId = null,
    compatibility,
    onpair: _onpair,
    onconfirm: _onconfirm,
    oncancel: _oncancel,
    onrepair: _onrepair,
    onauthrestart: _onauthrestart,
    onuselocal: _onuselocal,
    oncontinueproducts: _oncontinueproducts,
    onrefreshmcp,
    oncompatretry,
    registrationBusy = false,
    registrationResult = null,
    oncreateproject,
    onregistrationreset,
    driveFolderPhase = "idle",
    driveFolderName = "",
    driveFolderMessage = "",
    ondrivefolderselect,
    ondrivefolderreset,
    onjoinproduct,
    oncontinuesetup,
    ondrivestart,
    ondriveopenauth,
    ondrivecancel,
  }: Props = $props();

  let upgradeRequired = $derived(compatibility?.upgrade_required ?? false);
  let compatBanner = $derived(
    compatibility &&
      (compatibility.state === "hosted_dashboard_forward_skew" ||
        compatibility.state === "supported_previous")
      ? compatibility
      : null,
  );

  type HostedView =
    | "operator-home"
    | "settings"
    | "project"
    | "binding";

  // Cloud /connect hands off to `/app?onboarding=drive` after the user confirms
  // the explicit Device Connected step. Land on Drive once, then drop the URL
  // hint so reloads stay normal.
  function consumeDriveOnboardingHint(): boolean {
    if (typeof window === "undefined") return false;
    const url = new URL(window.location.href);
    if (url.searchParams.get("onboarding") !== "drive") return false;
    url.searchParams.delete("onboarding");
    window.history.replaceState(window.history.state, "", url.toString());
    return true;
  }
  const landOnDriveStep = consumeDriveOnboardingHint();
  let driveOnboarding = $state(landOnDriveStep);

  let activeView = $state<HostedView>(
    landOnDriveStep ? "settings" : "operator-home",
  );

  // Daemon-authored Drive truth finishes onboarding: go to the product home.
  // Leaving the Drive step ends onboarding, so a later connect never redirects.
  $effect(() => {
    if (driveOnboarding && activeView !== "settings") {
      driveOnboarding = false;
      return;
    }
    if (driveOnboarding && eventSubstrateConnected) {
      driveOnboarding = false;
      if (activeView === "settings") activeView = "operator-home";
    }
  });
  let settingsFocus = $state<SettingsSection>("storage");
  let selectedProject = $state<ProjectEntry | null>(null);
  let bindingProject = $state<ProjectEntry | null>(null);
  let registrationOpen = $state(false);
  let recoveryAttemptedFor = $state<string | null>(null);

  let joiningId = $state<string | null>(null);
  let joinFailures = $state<Record<string, ProductJoinFailureView>>({});

  function openSettings(section: SettingsSection = "storage") {
    registrationOpen = false;
    settingsFocus = section;
    activeView = "settings";
  }

  function skipDriveOnboarding() {
    // If an OAuth session is already in flight, leaving onboarding should stop
    // its browser-side watcher rather than letting a skipped step keep polling.
    ondrivecancel?.();
    driveOnboarding = false;
    settingsFocus = "storage";
    activeView = "operator-home";
  }

  function handleOpen(project: ProjectEntry) {
    registrationOpen = false;
    selectedProject = project;
    activeView = "project";
  }

  async function handleContinueOrRepair(project: ProjectEntry) {
    registrationOpen = false;
    const projection = await oncontinuesetup?.(project);
    if (projection) {
      const advanced = applySetupProjection(project, projection);
      if (projection.readiness === "open") {
        handleOpen(advanced);
        return;
      }
      bindingProject = advanced;
      activeView = "binding";
      return;
    }
    bindingProject = project;
    activeView = "binding";
  }

  async function handleJoin(project: ProjectEntry) {
    if (joiningId) return;
    joiningId = project.id;
    const { [project.id]: _cleared, ...rest } = joinFailures;
    joinFailures = rest;
    const outcome = await onjoinproduct?.(project);
    joiningId = null;
    if (!outcome) return;
    if (outcome.status === "granted") {
      const joined = projects.find((p) => p.id === project.id);
      if (joined) {
        if (joined.readiness === "ready" && !joined.setup_incomplete) {
          handleOpen(joined);
        } else {
          handleContinueOrRepair(joined);
        }
      }
    } else {
      joinFailures = { ...joinFailures, [project.id]: outcome.failure };
    }
  }

  function handleBackToHome() {
    registrationOpen = false;
    selectedProject = null;
    bindingProject = null;
    activeView = "operator-home";
  }

  function handleBindingBound() {
    if (bindingProject) {
      selectedProject = { ...bindingProject, readiness: "ready" };
      bindingProject = null;
      activeView = "project";
    }
  }

  function openCreate() {
    if (!eventSubstrateConnected) {
      openSettings("storage");
      return;
    }
    onregistrationreset?.();
    registrationOpen = true;
    activeView = "operator-home";
  }

  function closeRegistration() {
    onregistrationreset?.();
    registrationOpen = false;
  }

  async function copyMcpSetupCommand() {
    try {
      await navigator.clipboard?.writeText(mcpSetupCommand);
    } catch {
      // Copy is a convenience only; readiness/setup authority remains daemon-side.
    }
  }

  $effect(() => {
    if (registrationOpen && registrationResult?.status === "recorded") {
      registrationOpen = false;
    }
  });

  $effect(() => {
    if (
      bridgeState.kind === "paired" &&
      oncreateproject &&
      recoveryAttemptedFor !== bridgeState.cloudAccountId
    ) {
      recoveryAttemptedFor = bridgeState.cloudAccountId;
      const pending = getPendingHostedCreateInput(bridgeState.cloudAccountId);
      if (pending) {
        registrationOpen = true;
        oncreateproject(pending);
      }
    }
  });

  /**
   * PRD: unpaired /app never renders install/gateway/pair/MCP curriculum.
   * Pair bootstrap is Cloud HTML at /connect only. Leave once from /app —
   * never from other paths (Vite SPA catch-all would loop).
   */
  $effect(() => {
    if (typeof window === "undefined") return;
    if (bridgeState.kind === "paired") return;
    if (!window.location.pathname.startsWith("/app")) return;
    window.location.replace("/connect");
  });
</script>

<div class="hosted-dashboard" data-testid="hosted-dashboard">
  {#if bridgeState.kind === "paired"}
    <div class="paired-shell" data-testid="paired-shell">
      {#if upgradeRequired && compatibility}
        <HostedCompatibilityNotice {compatibility} onretry={oncompatretry} />
      {:else if compatBanner}
        <HostedCompatibilityNotice
          compatibility={compatBanner}
          onretry={oncompatretry}
        />
      {/if}

      {#if activeView === "project" && selectedProject}
        <HostedProjectShell
          session={{
            pairingState: "hosted_bridge_paired",
            origin: bridgeState.origin,
            cloudAccountId: bridgeState.cloudAccountId,
            pairedAt: bridgeState.pairedAt,
            expiresAt: bridgeState.expiresAt,
          }}
          project={selectedProject}
          {sessionToken}
          governedActionsEnabled={false}
          onback={handleBackToHome}
          onuselocal={_onuselocal}
        />
      {:else if activeView === "binding" && bindingProject}
        <WorkspaceBindingGate
          project={bindingProject}
          {sessionToken}
          {compatibility}
          onback={handleBackToHome}
          onbound={handleBindingBound}
        />
      {:else if activeView === "settings"}
        <OperatorHome
          {projects}
          onproducts={handleBackToHome}
          onsettings={() => openSettings("storage")}
          onopensettings={openSettings}
          {eventSubstrateConnected}
          {mcpState}
          activeNav="settings"
        >
          <HostedSettings
            focusSection={settingsFocus}
            {drivePhase}
            driveAccountLabel={driveAccountLabel}
            driveSessionId={driveSessionId}
            {driveOnboarding}
            {mcpState}
            {mcpHostId}
            {mcpReasons}
            {mcpSetupCommand}
            ondrivestart={ondrivestart}
            ondriveopenauth={ondriveopenauth}
            ondrivecancel={ondrivecancel}
            ondriveskip={skipDriveOnboarding}
            onrefreshmcp={onrefreshmcp}
            oncopymcp={copyMcpSetupCommand}
          />
        </OperatorHome>
      {:else}
        {#if registrationOpen}
          <HostedProjectRegistration
            busy={registrationBusy}
            result={registrationResult}
            eventSubstrateConnected={eventSubstrateConnected}
            folderPhase={driveFolderPhase}
            folderName={driveFolderName}
            folderMessage={driveFolderMessage}
            onselectfolder={() => ondrivefolderselect?.()}
            onresetfolder={() => ondrivefolderreset?.()}
            onsubmitcreate={(input) => oncreateproject?.(input)}
            oncancel={closeRegistration}
            onrequestconnect={() => {
              closeRegistration();
              openSettings("storage");
            }}
          />
        {/if}
        <OperatorHome
          {projects}
          onopen={handleOpen}
          oncontinuesetup={handleContinueOrRepair}
          onrepair={handleContinueOrRepair}
          oncreate={openCreate}
          onjoin={handleJoin}
          {joiningId}
          {joinFailures}
          onsettings={() => openSettings("storage")}
          onopensettings={openSettings}
          {eventSubstrateConnected}
          {mcpState}
          activeNav="products"
        />
      {/if}
    </div>
  {:else}
    <!-- No UI: effect sends /app → Cloud /connect. Do not mount
         CloudConnectDevice (install/gateway/pair/MCP rail) here. -->
    <div
      class="handoff-leaving"
      data-testid="hosted-handoff-leaving"
      aria-busy="true"
      aria-label="Opening Cloud device connection"
    ></div>
  {/if}
</div>

<style>
  .hosted-dashboard {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 100vh;
    background: var(--canvas);
    color: var(--ink-900);
  }

  .paired-shell {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 100vh;
  }

  .handoff-leaving {
    flex: 1;
    min-height: 30vh;
  }
</style>
