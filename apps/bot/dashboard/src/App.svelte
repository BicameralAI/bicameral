<script lang="ts">
  import { onMount } from "svelte";
  import IngestionGate from "./pages/IngestionGate.svelte";
  import LedgerView from "./pages/LedgerView.svelte";
  import Channels from "./pages/Channels.svelte";
  import SetupWizard from "./pages/SetupWizard.svelte";
  import ProjectSettings from "./pages/ProjectSettings.svelte";
  import UserSettings from "./pages/UserSettings.svelte";
  import ProjectRepair from "./pages/ProjectRepair.svelte";
  import OperatorHome from "./pages/OperatorHome.svelte";
  import Legend from "./lib/Legend.svelte";
  import ProjectSelector from "./lib/ProjectSelector.svelte";
  import HostedDashboard from "./pages/HostedDashboard.svelte";
  import JourneyReview from "./pages/JourneyReview.svelte";
  import LocalDaemonDisconnectedDialog from "./pages/LocalDaemonDisconnectedDialog.svelte";
  import {
    fetchSetupStatus,
    configureHostedGateway,
    fetchHostedProductMemberships,
    submitProductContinueSetup,
    startHostedConnection,
    pollHostedConnectionStatus,
    fetchHostedConnectionRegistry,
    startHostedResourceSelection,
    pollHostedResourceSelection,
    type HostedSetupProjection,
  } from "./lib/api";
  import type { ProviderResourceDescriptor } from "./lib/types";
  import type { ProjectEntry } from "./lib/project-fixtures";
  import type { HostedBridgeUIState } from "./lib/hosted-bridge-types";
  import { bridgeStateToConnectStatus } from "./lib/connect-status";
  import {
    pairWithDaemon,
    pollBridgeStatus,
    pollPendingPairStatus,
    isPairInitRecoveryError,
  } from "./lib/hosted-bridge-client";
  import {
    applyDaemonMembershipProjection,
    loadCloudProjects,
  } from "./lib/cloud-project-discovery";
  import { createHostedProject } from "./lib/hosted-project-registration";
  import type { RegistrationOutcome } from "./lib/hosted-project-registration";
  import {
    defaultProductJoinTransport,
    failureView,
    runProductJoin,
    type ProductJoinOutcome,
  } from "./lib/product-join";
  import {
    readHostedHandoff,
    recoverHostedPairing,
    resolveDaemonOrigin,
    restartHostedPairing,
  } from "./lib/hosted-handoff";
  import { onHostedDaemonDisconnected } from "./lib/hosted-daemon-connection";
  import {
    fetchMcpReadiness,
    type McpReadinessView,
    type McpReadinessState,
  } from "./lib/mcp-readiness";
  import type { DriveOAuthPhase } from "./ui";
  import {
    readDriveRegistryTruth,
    watchDriveAuthorization,
    type DriveAuthorizationWatcher,
  } from "./lib/drive-connection";

  // When Hosted cloud serves this SPA, the Cloud /connect shell authenticates
  // with Supabase, pairs with the local daemon, and hands the bridge session
  // token (for post-selection daemon projections) plus the Supabase access token
  // (for Cloud project discovery) here. The hosted project LIST is rendered from
  // Cloud discovery (`GET /api/v1/projects`, bot#720); the paired daemon
  // reconciles canonical membership/setup for those Product ids, while deeper
  // workspace projections remain selected-project reads. ?hosted may exercise
  // pairing UI, but it must not inject fabricated dashboard data.
  const handoff = readHostedHandoff();
  // Unpaired product path: Cloud /connect owns pair bootstrap (PRD). Only leave
  // from /app — never when this SPA is loaded under another path (Vite loop).
  if (
    typeof window !== "undefined" &&
    window.location.pathname.startsWith("/app") &&
    !handoff?.session_token
  ) {
    window.location.replace("/connect");
  }
  if (handoff) {
    configureHostedGateway(handoff.daemon_url);
  } else {
    configureHostedGateway(resolveDaemonOrigin());
  }

  // Cloud is the only supported SPA host. Local Vite and production builds use
  // the same hosted runtime and require real Cloud/daemon handoff state.
  const devDashboardMode = false;
  const hostedMode = true;
  const devMode = false;

const MISSING_CLOUD_SESSION_MESSAGE =
  "Cloud session is missing. Reopen the hosted dashboard and sign in again.";

function missingCloudSessionState(): HostedBridgeUIState {
  return {
    kind: "error",
    errorKind: "missing_cloud_session",
    message: MISSING_CLOUD_SESSION_MESSAGE,
    retryAfterRepair: false,
  };
}

function pairedHandoffState(): HostedBridgeUIState {
  // Daemon bridge session is enough to enter the paired shell. Cloud access
  // token is required for discovery/create; missing it is a discovery error,
  // not "unpaired" (which previously trapped operators on a connect dead-end).
  if (!handoff?.session_token) return missingCloudSessionState();
  return {
    kind: "paired",
    origin: window.location.origin,
    cloudAccountId: handoff.cloud_account_id,
    pairedAt: new Date().toISOString(),
    expiresAt: handoff.expires_at ?? "",
  };
}

let bridgeState = $state<HostedBridgeUIState>(
  handoff?.session_token ? pairedHandoffState() : { kind: "unpaired" }
);

  let bridgeSessionToken = $state<string | undefined>(handoff?.session_token);
  let hostedProjects = $state<ProjectEntry[]>([]);
  let hostedDiscoveryError = $state<string | null>(null);
  let selectedProjectId = $state<string | null>(null);
  let daemonDisconnected = $state(false);
  let appShell: HTMLDivElement;

  // Connect-device pending pairing (#790 / bot#1030)
  let pairingCode = $state<string | null>(null);
  let pendingRequestHandle = $state<string | null>(null);
  let pendingPollProof = $state<string | null>(null);
  let pairPollTimer: ReturnType<typeof setInterval> | null = null;

  // MCP readiness gate (read-only daemon projection)
  let mcpState = $state<McpReadinessState>("not_installed");
  let mcpHostId = $state("claude");
  let mcpReasons = $state<string[]>([]);
  let mcpSetupCommand = $state("bicameral setup mcp");

  // Alpha event store: Google Drive OAuth on the paired daemon
  let eventSubstrateConnected = $state(false);
  let drivePhase = $state<DriveOAuthPhase>("idle");
  let driveSessionId = $state<string | null>(null);
  let driveAccountLabel = $state("");
  let driveAuthUrl = $state<string | null>(null);
  // Observes a pending Google authorization until the daemon settles it
  // (bot#1263). Never a source of truth: it only reads daemon outcomes.
  let driveWatcher: DriveAuthorizationWatcher | null = null;
  // Bumped whenever Drive state is decided elsewhere, so a slow registry read
  // started earlier can never overwrite a newer outcome.
  let driveStateGeneration = 0;

  // Google Drive folder selection for Product create (#1264). The daemon
  // owns the Picker session and validates the folder; the SPA keeps only the
  // redacted projection. `driveFolder` is the daemon-validated descriptor the
  // create request transports as `event_store_descriptor`.
  let driveFolderPhase = $state<
    "idle" | "starting" | "picking" | "ready" | "failed"
  >("idle");
  let driveFolder = $state<ProviderResourceDescriptor | null>(null);
  let driveFolderMessage = $state("");
  let driveFolderPollTimer: ReturnType<typeof setInterval> | null = null;

  onMount(() =>
    onHostedDaemonDisconnected(() => {
      // Only after a live bridge session. Unpaired / mid-pair failures must not
      // pop the full-screen "Local daemon disconnected" dialog.
      if (bridgeState.kind === "paired" && bridgeSessionToken) {
        daemonDisconnected = true;
      }
    }),
  );

  $effect(() => {
    if (appShell) appShell.toggleAttribute("inert", daemonDisconnected);
  });

  // Create/Join registration state (cloud#66). The token lives here (handoff),
  // so App owns the async Cloud calls; HostedDashboard collects the input and
  // renders the resulting pending state.
  let registrationBusy = $state(false);
  let registrationResult = $state<RegistrationOutcome | null>(null);

function missingCloudSessionResult(): RegistrationOutcome {
  return {
    status: "error",
    message: MISSING_CLOUD_SESSION_MESSAGE,
  };
}

function requireCloudSession(): string | null {
  const accessToken = handoff?.cloud_access_token;
  if (accessToken) return accessToken;
  // Do not overwrite bridgeState — a live daemon pair must stay "connected".
  // Cloud create/join only needs the Supabase token for product LIST / register.
  registrationResult = missingCloudSessionResult();
  return null;
}

/** After daemon pair succeeds: MCP/Drive always; Cloud LIST only with handoff. */
async function onDaemonPaired() {
  await refreshMcpReadiness();
  await refreshDriveConnection();
  if (handoff?.cloud_access_token) {
    await loadHostedProjectDiscovery();
  }
}

  // When every discovered project shares one team, Create exposes into that
  // existing team; otherwise (e.g. the empty state) it creates a new team.
  const defaultTeamId = $derived.by<string | undefined>(() => {
    const ids = new Set(
      hostedProjects.map((p) => p.team_id).filter((id): id is string => !!id),
    );
    return ids.size === 1 ? [...ids][0] : undefined;
  });

  function resetRegistration() {
    registrationResult = null;
  }

async function handleHostedCreate(input: {
  displayName: string;
  projectRef?: string;
}) {
  if (registrationBusy) return;
  const accessToken = requireCloudSession();
  if (!accessToken) return;
  registrationBusy = true;
    registrationResult = null;
    registrationResult = await createHostedProject({
      accessToken,
      apiBase: handoff?.cloud_api_base,
      displayName: input.displayName,
      projectRef: input.projectRef,
      teamId: defaultTeamId,
      eventStoreFolderId: driveFolder?.folder_id,
    });
    registrationBusy = false;
    // The daemon validates a chosen folder for a bounded window. When it
    // refuses the create because that validation is gone (expired session,
    // daemon restart, or a folder it never validated), drop the stale
    // selection so the user re-picks instead of retrying a dead descriptor.
    if (
      registrationResult.phase === "failed" &&
      registrationResult.message.startsWith("product_event_store_unvalidated")
    ) {
      stopDriveFolderPolling();
      driveFolder = null;
      driveFolderPhase = "failed";
      driveFolderMessage =
        "The folder choice is no longer valid. Choose the folder again, then create.";
    }
    if (registrationResult.status === "recorded") {
      await loadHostedProjectDiscovery();
    }
  }

/**
 * Run a per-Product same-domain join (bot#768). Drives the two contract hops
 * (Cloud ticket → daemon `product.join`) and, only on a granted daemon result,
 * refreshes discovery so the row becomes a canonical member. Never claims
 * membership on failure; typed failures surface as retryable per-row copy.
 */
async function handleHostedProductJoin(
  project: ProjectEntry,
): Promise<ProductJoinOutcome> {
  const accessToken = requireCloudSession();
  const preview = project.preview;
  const sessionToken = bridgeSessionToken;
  if (!accessToken || !preview || !sessionToken) {
    return { status: "failed", failure: failureView("unavailable") };
  }
  const outcome = await runProductJoin({
    preview,
    accessToken,
    sessionToken,
    apiBase: handoff?.cloud_api_base,
    transport: defaultProductJoinTransport,
  });
  if (outcome.status === "granted") {
    await loadHostedProjectDiscovery();
  }
  return outcome;
}

/**
 * Continue local setup after a canonical join (bot#769). Drives the daemon
 * `product.continue_setup` hop against the paired daemon, which resolves
 * provider access + descriptor validation from local state, writes at most one
 * local `ProjectRegistration`, and opens+replays the Product. On success we
 * refresh discovery so the row reflects the new readiness. Never claims or
 * revokes membership: a daemon-unavailable/failed call returns `null` and the
 * member stays on the setup/repair surface.
 *
 * NOTE (cloud#126): the opaque `registration_ref` is not yet carried on the
 * member row post-join; we route the Product id as the stable reference until
 * the signed-ticket `registration_ref` shape lands. The daemon derives all
 * authority/descriptor facts locally regardless of this hint.
 */
async function handleHostedContinueSetup(
  project: ProjectEntry,
): Promise<HostedSetupProjection | null> {
  const sessionToken = bridgeSessionToken;
  if (!sessionToken) return null;
  const productId = project.preview?.product_id ?? project.id;
  try {
    const projection = await submitProductContinueSetup({
      sessionToken,
      productId,
      registrationRef: productId,
      providerHint: project.provider_id === "google" ? "google" : "github",
    });
    await loadHostedProjectDiscovery();
    return projection;
  } catch {
    return null;
  }
}

  /**
   * Render the hosted project list from Cloud project discovery (bot#720).
   *
   * Cloud `GET /api/v1/projects` remains the only discovery/eligibility source.
   * The paired daemon then reconciles the membership/setup axes for those
   * already-discovered Product ids from canonical replay. It cannot add rows or
   * alter Cloud tenancy/eligibility metadata. If replay is unavailable, fail
   * closed rather than offering a duplicate Join.
   */
async function loadHostedProjectDiscovery() {
  const accessToken = requireCloudSession();
  if (!accessToken) return;
  const sessionToken = bridgeSessionToken;
  if (!sessionToken) {
    hostedDiscoveryError =
      "Canonical Product membership is unavailable until the daemon is paired.";
    hostedProjects = [];
    return;
  }
  const result = await loadCloudProjects({
      accessToken,
      apiBase: handoff?.cloud_api_base,
      selectedProjectId: selectedProjectId ?? undefined,
    });
  if (result.error) {
    hostedDiscoveryError = result.error;
    hostedProjects = [];
    return;
  }
  try {
    const memberships = await fetchHostedProductMemberships(sessionToken);
    hostedDiscoveryError = null;
    hostedProjects = applyDaemonMembershipProjection(
      result.projects,
      memberships,
    );
  } catch {
    hostedDiscoveryError =
      "Could not verify canonical Product membership with the local daemon.";
    hostedProjects = [];
  }
  }

  if (handoff) {
    // Cloud completes pairing before `/app` opens, so there is no local pair
    // callback here to hang the first daemon read off. Read MCP readiness
    // directly: without this the post-handoff readiness block would report the
    // component default ("Not installed") no matter what the daemon says, which
    // is exactly the untruthfulness this slice is supposed to remove.
    refreshMcpReadiness();
    // Seed Drive from the daemon ConnectionRegistry on every load/reload so an
    // existing connection renders Connected without another Connect click.
    refreshDriveConnection();
    loadHostedProjectDiscovery();
  }

  function stopPairPoll() {
    if (pairPollTimer) {
      clearInterval(pairPollTimer);
      pairPollTimer = null;
    }
  }

  function applyMcpView(view: McpReadinessView) {
    mcpState = view.state;
    mcpHostId = view.hostId;
    mcpReasons = view.reasons;
    mcpSetupCommand = view.setupCommand;
  }

  async function refreshMcpReadiness() {
    if (!bridgeSessionToken) return;
    try {
      applyMcpView(await fetchMcpReadiness(bridgeSessionToken));
    } catch {
      mcpState = "not_installed";
      mcpReasons = ["daemon_unavailable"];
      mcpSetupCommand = "bicameral setup mcp";
    }
  }

  /**
   * Re-read daemon-authored Drive truth. An unreadable registry leaves the
   * current state alone; an in-flight authorization is left to its watcher.
   */
  /** The bridge session ended: drop all Drive presentation state with it. */
  function resetDriveState() {
    stopDriveWatcher();
    driveStateGeneration += 1;
    eventSubstrateConnected = false;
    drivePhase = "idle";
    driveAccountLabel = "";
    driveSessionId = null;
    driveAuthUrl = null;
  }

  async function refreshDriveConnection() {
    const sessionToken = bridgeSessionToken;
    if (!sessionToken) return;
    const generation = driveStateGeneration;
    const truth = await readDriveRegistryTruth(() =>
      fetchHostedConnectionRegistry(sessionToken),
    );
    if (!truth) return;
    if (
      generation !== driveStateGeneration ||
      sessionToken !== bridgeSessionToken ||
      driveWatcher
    ) {
      return;
    }
    if (truth.connected) {
      stopDriveWatcher();
      eventSubstrateConnected = true;
      drivePhase = "connected";
      driveAccountLabel = truth.accountLabel;
      driveSessionId = null;
      driveAuthUrl = null;
      return;
    }
    eventSubstrateConnected = false;
    if (drivePhase === "connected") {
      drivePhase = "idle";
      driveAccountLabel = "";
    }
  }

  function stopDriveWatcher() {
    driveWatcher?.stop();
    driveWatcher = null;
  }

  /** Begin observing the daemon for the outcome of this authorization. */
  function startDriveWatcher(sessionId: string) {
    stopDriveWatcher();
    const sessionToken = bridgeSessionToken;
    if (!sessionToken) return;
    driveWatcher = watchDriveAuthorization({
      sessionId,
      deps: {
        pollStatus: (id) =>
          pollHostedConnectionStatus(sessionToken, "google", id),
        fetchRegistry: () => fetchHostedConnectionRegistry(sessionToken),
      },
      onsettled: (step) => {
        // A watcher from an ended bridge session must not touch the new one.
        if (sessionToken !== bridgeSessionToken) return;
        driveWatcher = null;
        driveStateGeneration += 1;
        driveSessionId = null;
        driveAuthUrl = null;
        if (step.kind === "connected") {
          eventSubstrateConnected = true;
          driveAccountLabel = step.accountLabel;
          drivePhase = "connected";
          return;
        }
        eventSubstrateConnected = false;
        drivePhase = step.phase;
      },
    });
  }

  // Returning from the Google tab is the moment the user expects Bicameral to
  // notice; check immediately instead of waiting for the next poll tick.
  onMount(() => {
    const recheck = () => {
      if (document.visibilityState === "hidden") return;
      if (driveWatcher) {
        void driveWatcher.checkNow();
      } else if (drivePhase !== "connected") {
        // e.g. authorization finished in another tab after a reload.
        void refreshDriveConnection();
      }
    };
    window.addEventListener("focus", recheck);
    document.addEventListener("visibilitychange", recheck);
    return () => {
      window.removeEventListener("focus", recheck);
      document.removeEventListener("visibilitychange", recheck);
      stopDriveWatcher();
    };
  });

  function startPendingPairPoll(
    requestHandle: string,
    pollProof: string,
    cloudAccountId: string,
  ) {
    stopPairPoll();
    pairPollTimer = setInterval(async () => {
      const result = await pollPendingPairStatus({
        requestHandle,
        pollProof,
        cloudAccountId,
      });
      // Still waiting for local approve — keep code + pending UI.
      if (result.state.kind === "pairing_pending") return;
      // Transient transport / daemon errors must not wipe the pairing code.
      // The operator is mid-approve; next tick will retry.
      if (result.state.kind === "error") {
        return;
      }
      stopPairPoll();
      pairingCode = null;
      pendingRequestHandle = null;
      pendingPollProof = null;
      bridgeState = result.state;
      if (result.sessionToken) {
        bridgeSessionToken = result.sessionToken;
      }
      if (result.state.kind === "paired") {
        // Keep paired even without Cloud Supabase handoff (local Vite smoke).
        await onDaemonPaired();
      }
    }, 2000);
  }

  async function handlePair() {
    if (!hostedMode) return;
    stopPairPoll();
    pairingCode = null;
    pendingRequestHandle = null;
    pendingPollProof = null;
    bridgeState = {
      kind: "pairing_pending",
      reason: "connecting",
      message: "Connecting to local daemon...",
    };
    const origin = window.location.origin;
    const cloudAccountId =
      handoff?.cloud_account_id ?? "hosted_browser";
    const csrfToken = crypto.randomUUID();
    const teamId = handoff?.team_id ?? "";
    const cloudAccessToken = handoff?.cloud_access_token ?? "";
    const result = await pairWithDaemon(
      origin,
      cloudAccountId,
      csrfToken,
      teamId,
      cloudAccessToken,
    );
    if (isPairInitRecoveryError(result.state)) {
      restartHostedPairing((path) => window.location.replace(path));
      return;
    }
    bridgeState = result.state;
    if (result.sessionToken) {
      bridgeSessionToken = result.sessionToken;
    }
    if (result.pairingCode) pairingCode = result.pairingCode;
    if (result.requestHandle && result.pollProof) {
      pendingRequestHandle = result.requestHandle;
      pendingPollProof = result.pollProof;
      startPendingPairPoll(
        result.requestHandle,
        result.pollProof,
        cloudAccountId,
      );
    }
    if (result.state.kind === "paired") {
      await onDaemonPaired();
    }
  }

  async function handleDriveStart() {
    if (!bridgeSessionToken) return;
    stopDriveWatcher();
    driveStateGeneration += 1;
    driveAuthUrl = null;
    drivePhase = "starting";
    try {
      const outcome = await startHostedConnection(
        bridgeSessionToken,
        "google",
      );
      if (outcome.outcome === "connected") {
        eventSubstrateConnected = true;
        drivePhase = "connected";
        driveAccountLabel = outcome.account_label ?? "";
        return;
      }
      if (outcome.outcome === "start_url") {
        if (!outcome.auth_session_id || !outcome.url) {
          drivePhase = "provider_unavailable";
          return;
        }
        driveSessionId = outcome.auth_session_id;
        driveAuthUrl = outcome.url;
        drivePhase = "authorize";
        return;
      }
      if (outcome.outcome === "action_needed") {
        drivePhase = "action_needed";
        return;
      }
      drivePhase = "provider_unavailable";
    } catch {
      drivePhase = "provider_unavailable";
    }
  }

  function stopDriveFolderPolling() {
    if (driveFolderPollTimer !== null) {
      clearInterval(driveFolderPollTimer);
      driveFolderPollTimer = null;
    }
  }

  /**
   * Start the daemon-owned Google Drive folder selection (#1264). The daemon
   * mints the Picker URL and a selection session; the user picks exactly one
   * folder in Google; the daemon validates it on its loopback callback. We
   * poll the redacted projection until it is ready or fails. Nothing here
   * can mark a folder ready: only a daemon `ready` projection does.
   */
  async function handleDriveFolderSelect() {
    if (!bridgeSessionToken) {
      driveFolderPhase = "failed";
      driveFolderMessage = "Reconnect this device before choosing a folder.";
      return;
    }
    stopDriveFolderPolling();
    driveFolder = null;
    driveFolderMessage = "";
    driveFolderPhase = "starting";
    let outcome;
    try {
      outcome = await startHostedResourceSelection(bridgeSessionToken);
    } catch {
      driveFolderPhase = "failed";
      driveFolderMessage = "Bicameral could not start folder selection. Try again.";
      return;
    }
    if (outcome.outcome !== "start_url") {
      driveFolderPhase = "failed";
      driveFolderMessage = outcome.message;
      return;
    }
    const selectionSessionId = outcome.selection_session_id;
    window.open(outcome.url, "_blank", "noopener,noreferrer");
    driveFolderPhase = "picking";
    const sessionToken = bridgeSessionToken;
    driveFolderPollTimer = setInterval(async () => {
      let projection;
      try {
        projection = await pollHostedResourceSelection(
          sessionToken,
          selectionSessionId,
        );
      } catch {
        return;
      }
      if (projection.state === "ready" && projection.resource) {
        stopDriveFolderPolling();
        driveFolder = projection.resource;
        driveFolderPhase = "ready";
        driveFolderMessage = "";
        return;
      }
      if (projection.state !== "pending_selection") {
        stopDriveFolderPolling();
        driveFolder = null;
        driveFolderPhase = "failed";
        driveFolderMessage = projection.message;
      }
    }, 2000);
  }

  function handleDriveFolderReset() {
    stopDriveFolderPolling();
    driveFolder = null;
    driveFolderPhase = "idle";
    driveFolderMessage = "";
  }

  function handleDriveOpenAuth() {
    if (!driveAuthUrl || !driveSessionId) {
      drivePhase = "provider_unavailable";
      return;
    }
    window.open(driveAuthUrl, "_blank", "noopener,noreferrer");
    drivePhase = "waiting";
    // Reopening Google keeps the same daemon session; only start observing once.
    if (!driveWatcher) startDriveWatcher(driveSessionId);
  }

  function handleDriveCancel() {
    stopDriveWatcher();
    drivePhase = eventSubstrateConnected ? "connected" : "idle";
    driveSessionId = null;
    driveAuthUrl = null;
    // The user may have finished in Google before cancelling; daemon decides.
    void refreshDriveConnection();
  }

  async function handleConfirm() {
    if (bridgeSessionToken) {
      const result = await pollBridgeStatus(bridgeSessionToken);
      bridgeState = result.state;
      if (result.state.kind === "paired") {
        await onDaemonPaired();
      }
    } else {
      bridgeState = { kind: "unpaired" };
    }
  }

  function handleCancel() {
    stopPairPoll();
    pairingCode = null;
    pendingRequestHandle = null;
    pendingPollProof = null;
    bridgeState = { kind: "unpaired" };
    bridgeSessionToken = undefined;
    resetDriveState();
    hostedProjects = [];
    hostedDiscoveryError = null;
  }

  function handleRepair() {
    stopPairPoll();
    pairingCode = null;
    pendingRequestHandle = null;
    pendingPollProof = null;
    bridgeState = { kind: "unpaired" };
    bridgeSessionToken = undefined;
    resetDriveState();
    hostedProjects = [];
    hostedDiscoveryError = null;
  }

function handleUseLocal() {
  const url = new URL(window.location.href);
  url.searchParams.delete("hosted");
  window.location.href = url.toString();
}

function handleHostedAuthRestart() {
  restartHostedPairing();
}

function handleDaemonPairingRecovery() {
  bridgeSessionToken = undefined;
  resetDriveState();
  daemonDisconnected = false;
  recoverHostedPairing();
}

  type Page = "ingestion" | "ledger" | "channels" | "journeys" | "settings" | "repair" | "operator-home";

  let activePage = $state<Page>("ingestion");
  let setupMode = $state<boolean | null>(null);
  // In dev_dashboard_mode the harness registers local project fixtures; in
  // hosted/production this stays empty (local project registry is not a
  // product source — see the dashboard-mode runtime contract).
  const localProjects: ProjectEntry[] = [];
  let repairProject = $state<ProjectEntry | null>(null);
  let showUserSettings = $state(false);

  async function checkSetup() {
    // Same-origin setup status is a dev_dashboard_mode surface only; never
    // reached in the production hosted runtime.
    if (!devDashboardMode) {
      setupMode = false;
      return;
    }
    try {
      const status = await fetchSetupStatus();
      setupMode = !status.workspace_exists;
    } catch {
      setupMode = false;
    }
  }

  function onWorkspaceCreated() {
    setupMode = false;
    activePage = "settings";
  }

  function handleProjectSelect(project: ProjectEntry) {
    if (project.readiness === "needs_access") {
      repairProject = project;
      activePage = "repair";
    } else {
      repairProject = null;
      if (activePage === "repair") {
        activePage = "ingestion";
      }
    }
  }

  function handleRepairBack() {
    repairProject = null;
    activePage = "ingestion";
  }

  function handleRepairConnect(_providerId: string) {
    activePage = "settings";
    repairProject = null;
  }

  function handleOperatorHomeSelect(project: ProjectEntry) {
    selectedProjectId = project.id;
    activePage = "ingestion";
  }

  function handleOperatorHomeRepair(project: ProjectEntry) {
    repairProject = project;
    activePage = "repair";
  }

  function onGoSettings() {
    setupMode = false;
    activePage = "settings";
  }

  $effect(() => {
    if (devDashboardMode) {
      checkSetup();
    }
  });

  /** Hosted shell chrome: connection pill + profile avatar (single global header). */
  const hostedConnectStatus = $derived(bridgeStateToConnectStatus(bridgeState));
  const hostedConnTone = $derived.by(() => {
    switch (hostedConnectStatus) {
      case "connected":
        return "ok";
      case "pending":
      case "checking":
        return "pending";
      case "update_required":
        return "warn";
      default:
        return "error";
    }
  });
  const hostedConnLabel = $derived.by(() => {
    switch (hostedConnectStatus) {
      case "connected":
        return "Connected";
      case "pending":
        return "Pairing pending";
      case "checking":
        return "Checking";
      case "update_required":
        return "Update required";
      default:
        return "Not connected";
    }
  });
  const hostedAccountLabel = $derived(handoff?.cloud_account_id ?? "hosted");
  const hostedInitials = $derived(
    hostedAccountLabel
      .split(/[\s@._-]+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((p) => p[0]?.toUpperCase() ?? "")
      .join("") || "?",
  );
</script>

<div
  bind:this={appShell}
  class="app-shell"
  class:hosted-shell={hostedMode}
  aria-hidden={daemonDisconnected ? "true" : undefined}
>
  <header
    class="top-bar"
    class:top-bar-compact={hostedMode}
    data-testid="app-header"
  >
    <span class="brand">
      <svg
        class="logo-mark"
        viewBox="0 0 32 32"
        width={hostedMode ? 18 : 24}
        height={hostedMode ? 18 : 24}
        aria-hidden="true"
      >
        <rect width="32" height="32" rx="6" fill="var(--ink-900)" />
        <g transform="rotate(45 16 16)">
          <circle
            cx="16"
            cy="16"
            r="9"
            fill="none"
            stroke="#fff"
            stroke-width="3.2"
            stroke-dasharray="24.27 4"
            stroke-dashoffset="12.14"
          />
        </g>
      </svg>
      <span class="brand-name">bicameral</span>
    </span>
    {#if devMode}
      <span
        class="dev-mode-badge"
        data-testid="dev-dashboard-mode-marker"
        data-bicameral-dashboard-mode="dev_dashboard_mode"
        title="Dev dashboard harness — fixtures only, not a production runtime"
      >
        DEV MODE
      </span>
    {/if}
    {#if hostedMode}
      <div class="top-bar-spacer"></div>
      <span
        class="hosted-conn-pill"
        data-tone={hostedConnTone}
        data-testid="hosted-conn-status"
      >
        <span class="connection-dot" aria-hidden="true"></span>
        {hostedConnLabel}
      </span>
      <span
        class="hosted-avatar"
        title={hostedAccountLabel}
        data-testid="hosted-profile-avatar"
        aria-label={`Signed in as ${hostedAccountLabel}`}
      >
        {hostedInitials}
      </span>
    {/if}
    {#if !hostedMode && setupMode === false}
      <ProjectSelector
        projects={localProjects}
        bind:selectedId={selectedProjectId}
        onselect={handleProjectSelect}
      />
      <div class="top-bar-spacer"></div>
      <Legend />
    {/if}
    {#if !hostedMode}
      <button
        class="user-settings-btn"
        onclick={() => (showUserSettings = true)}
        aria-label="User Settings"
        title="User Settings"
      >
        ⚙
      </button>
    {/if}
  </header>

  {#if hostedMode}
    {#if hostedDiscoveryError && bridgeState.kind === "paired"}
      <div class="discovery-error" data-testid="cloud-discovery-error" role="alert">
        Could not load your projects from Bicameral Cloud. {hostedDiscoveryError}
      </div>
    {/if}
    <HostedDashboard
      {bridgeState}
      sessionToken={bridgeSessionToken}
      projects={hostedProjects}
      {pairingCode}
      {mcpState}
      {mcpHostId}
      {mcpReasons}
      mcpSetupCommand={mcpSetupCommand}
      accountLabel={handoff?.cloud_account_id ?? "hosted"}
      {eventSubstrateConnected}
      {drivePhase}
      {driveAccountLabel}
      {driveSessionId}
      onpair={handlePair}
      onconfirm={handleConfirm}
      oncancel={handleCancel}
      onrepair={handleRepair}
      onauthrestart={handleHostedAuthRestart}
      onuselocal={handleUseLocal}
      onrefreshmcp={refreshMcpReadiness}
      {registrationBusy}
      {registrationResult}
      oncreateproject={handleHostedCreate}
      onregistrationreset={resetRegistration}
      onjoinproduct={handleHostedProductJoin}
      oncontinuesetup={handleHostedContinueSetup}
      ondrivestart={handleDriveStart}
      ondriveopenauth={handleDriveOpenAuth}
      ondrivecancel={handleDriveCancel}
      {driveFolderPhase}
      driveFolderName={driveFolder?.folder_display_name ?? ""}
      {driveFolderMessage}
      ondrivefolderselect={handleDriveFolderSelect}
      ondrivefolderreset={handleDriveFolderReset}
    />
  {:else if devDashboardMode}
    <!-- dev_dashboard_mode only. Unreachable in the production hosted runtime:
         hostedMode is always true when the dev harness is not compiled in
         (DEV_DASHBOARD_MODE=false), so SetupWizard and the local sidebar cannot
         render in a production build. -->
    {#if setupMode === null}
      <div class="loading-state">Loading...</div>
    {:else if setupMode}
      <SetupWizard oncreated={onWorkspaceCreated} ongosettings={onGoSettings} />
    {:else}
    <div class="layout">
      <nav class="sidebar">
        <button
          class:active={activePage === "ingestion"}
          onclick={() => (activePage = "ingestion")}
        >
          <span class="nav-icon">●</span>
          <span>Activity</span>
        </button>
        <button
          class:active={activePage === "ledger"}
          onclick={() => (activePage = "ledger")}
        >
          <span class="nav-icon">●</span>
          <span>Decisions</span>
        </button>
        <button
          class:active={activePage === "channels"}
          onclick={() => (activePage = "channels")}
        >
          <span class="nav-icon">●</span>
          <span>Channels</span>
        </button>
        <button
          class:active={activePage === "journeys"}
          onclick={() => (activePage = "journeys")}
        >
          <span class="nav-icon">●</span>
          <span>Journeys</span>
        </button>
        <button
          class:active={activePage === "settings"}
          onclick={() => (activePage = "settings")}
        >
          <span class="nav-icon">●</span>
          <span>Settings</span>
        </button>
        <button
          class:active={activePage === "operator-home"}
          onclick={() => (activePage = "operator-home")}
        >
          <span class="nav-icon">●</span>
          <span>Projects</span>
        </button>
      </nav>

      <main>
        {#if activePage === "repair" && repairProject}
          <ProjectRepair
            project={repairProject}
            onback={handleRepairBack}
            onconnect={handleRepairConnect}
          />
        {:else if activePage === "operator-home"}
          <OperatorHome
            projects={localProjects}
            onopen={handleOperatorHomeSelect}
            oncontinuesetup={handleOperatorHomeSelect}
            onrepair={handleOperatorHomeRepair}
          />
        {:else if activePage === "ingestion"}
          <IngestionGate />
        {:else if activePage === "ledger"}
          <LedgerView />
        {:else if activePage === "channels"}
          <Channels />
        {:else if activePage === "journeys"}
          <JourneyReview />
        {:else}
          <ProjectSettings />
        {/if}
      </main>
    </div>
    {/if}
  {/if}

  {#if showUserSettings}
    <UserSettings onclose={() => (showUserSettings = false)} />
  {/if}
</div>

{#if daemonDisconnected}
  <LocalDaemonDisconnectedDialog onpairing={handleDaemonPairingRecovery} />
{/if}

<style>
  .app-shell {
    min-height: 100vh;
    display: flex;
    flex-direction: column;
  }

  .top-bar {
    display: flex;
    align-items: center;
    gap: var(--sp-6);
    min-height: 64px;
    padding: var(--sp-2) 1.75rem;
    background: var(--surface);
    border-bottom: 1px solid var(--line);
  }

  /* Langfuse-style thin app chrome (hosted SPA). */
  .top-bar-compact {
    min-height: var(--shell-topbar-h, 2.5rem);
    height: var(--shell-topbar-h, 2.5rem);
    gap: 0.75rem;
    padding: 0 0.875rem;
  }

  .top-bar-spacer {
    flex: 1;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    flex: 0 1 auto;
    min-width: 0;
  }

  .logo-mark {
    display: block;
    flex-shrink: 0;
  }

  .brand-name {
    font-family: var(--font-sans);
    font-size: 0.875rem;
    font-weight: 600;
    letter-spacing: -0.01em;
    color: var(--ink-900);
  }

  .top-bar-compact .brand-name {
    font-size: 0.8125rem;
    font-weight: 600;
  }

  .hosted-label {
    margin-left: 1.25rem;
    color: var(--accent-700);
    font-family: var(--font-mono);
    font-size: 0.5625rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .dev-mode-badge {
    display: inline-flex;
    align-items: center;
    padding: 2px var(--sp-2);
    border-radius: var(--radius-sm);
    font-size: var(--text-xs);
    font-weight: 700;
    letter-spacing: 0.06em;
    background: #fde68a;
    color: #7c2d12;
    border: 1px solid #f59e0b;
  }

  .hosted-conn-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 0.75rem;
    font-weight: 500;
    white-space: nowrap;
  }

  .hosted-conn-pill .connection-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--ink-400);
  }

  .hosted-conn-pill[data-tone="ok"] {
    color: var(--ok-600);
  }
  .hosted-conn-pill[data-tone="ok"] .connection-dot {
    background: var(--ok-600);
  }
  .hosted-conn-pill[data-tone="error"] {
    color: var(--danger-600, #dc2626);
  }
  .hosted-conn-pill[data-tone="error"] .connection-dot {
    background: var(--danger-600, #dc2626);
  }
  .hosted-conn-pill[data-tone="warn"] {
    color: var(--warn-600, #d97706);
  }
  .hosted-conn-pill[data-tone="warn"] .connection-dot {
    background: var(--warn-600, #d97706);
  }
  .hosted-conn-pill[data-tone="pending"] {
    color: var(--accent-700, #0e7490);
  }
  .hosted-conn-pill[data-tone="pending"] .connection-dot {
    background: var(--accent-600, #0891b2);
  }

  .hosted-avatar {
    width: 28px;
    height: 28px;
    border-radius: 50%;
    border: 1px solid var(--memory-200, #ddd6fe);
    color: var(--memory-600, #7c3aed);
    display: grid;
    place-items: center;
    font-size: 0.6875rem;
    font-weight: 600;
    flex-shrink: 0;
    background: var(--surface);
  }

  .top-bar-compact .hosted-avatar {
    width: 26px;
    height: 26px;
  }

  .hosted-shell {
    min-height: 100vh;
  }

  .hosted-shell > :global([data-testid="hosted-dashboard"]),
  .hosted-shell :global(.hosted-dashboard),
  .hosted-shell :global(.paired-shell) {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .hosted-shell :global(.operator-home) {
    flex: 1;
    min-height: calc(100vh - var(--shell-topbar-h, 2.5rem));
  }

  .user-settings-btn {
    background: none;
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    padding: 0.375rem 0.625rem;
    font-size: 1rem;
    cursor: pointer;
    color: var(--ink-500);
    line-height: 1;
    transition:
      background var(--dur-fast) var(--ease-out),
      color var(--dur-fast) var(--ease-out),
      border-color var(--dur-fast) var(--ease-out);
  }

  .user-settings-btn:hover {
    background: var(--canvas-muted);
    color: var(--ink-900);
    border-color: var(--line-strong);
  }

  .layout {
    display: flex;
    flex: 1;
  }

  .sidebar {
    width: 220px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    padding: var(--sp-4) 0;
    background: var(--surface);
    border-right: 1px solid var(--line);
  }

  .sidebar button {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-2) var(--sp-5);
    border: none;
    border-left: 3px solid transparent;
    background: none;
    color: var(--ink-500);
    font-size: var(--text-sm);
    font-family: inherit;
    font-weight: 500;
    cursor: pointer;
    transition:
      background var(--dur-fast) var(--ease-out),
      color var(--dur-fast) var(--ease-out);
    text-align: left;
  }

  .sidebar button:hover {
    background: var(--canvas-muted);
    color: var(--ink-900);
  }

  .sidebar button.active {
    background: var(--accent-50);
    border-left-color: var(--accent-500);
    color: var(--accent-700);
    font-weight: 600;
  }

  .nav-icon {
    font-size: 0.5rem;
    width: 1rem;
    text-align: center;
    flex-shrink: 0;
    color: var(--ink-400);
  }

  .sidebar button.active .nav-icon {
    color: var(--accent-500);
  }

  main {
    flex: 1;
    padding: var(--sp-5);
    overflow: auto;
    background: var(--canvas);
  }

  .loading-state {
    display: flex;
    justify-content: center;
    padding: var(--sp-8);
    color: var(--ink-500);
    font-size: var(--text-base);
  }

  .discovery-error {
    max-width: 720px;
    margin: 1rem auto 0;
    padding: 0.75rem 1rem;
    border: 1px solid #fde68a;
    border-left: 3px solid var(--warn-600);
    border-radius: var(--radius-md);
    background: var(--warn-50);
    color: var(--warn-600);
    font-size: 0.8125rem;
    line-height: 1.5;
  }

  @media (max-width: 720px) {
    .top-bar {
      gap: 0.75rem;
      padding: 0.75rem 1rem;
    }

    .brand {
      flex: 0 0 auto;
      min-width: 0;
    }

    .top-bar-spacer,
    .top-bar :global(.legend) {
      display: none;
    }
  }
</style>
