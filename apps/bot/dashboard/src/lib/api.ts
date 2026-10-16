/** Thin API client for the Bicameral gateway dashboard endpoints. */

import type {
  ConnectionCompletionOutcome,
  ConnectionRegistry,
  ConnectionStartOutcome,
  EvidenceRefreshOutcome,
  FindCodeImpactResult,
  IngestionGateItem,
  LedgerReviewItem,
  ReviewCommandPayload,
  ResourceSelectionProjection,
  ResourceSelectionStartOutcome,
} from "./types";
import type {
  HostedDashboardAdapterResponse,
  HostedDashboardCompatibility,
  HostedLedgerProjection,
  SelectedProjectSessionResponse,
} from "./hosted-bridge-types";
import { HOSTED_DASHBOARD_PROTOCOL_VERSION } from "./hosted-bridge-types";
import type {
  CloudProjectListResponse,
  LoadCloudProjectsOptions,
} from "./cloud-project-discovery";
import type {
  CloudRegistrationReceipt,
  CloudTeamCreateResponse,
  CreateCloudTeamOptions,
  ExposeCloudProjectOptions,
} from "./hosted-project-registration";
import type {
  HostedProjectPreview,
  ProductJoinResult,
  SignedProductJoinTicket,
} from "./onboarding-contract";
import { fetchFromHostedDaemon } from "./hosted-daemon-connection";

const BASE = "";

// ── Hosted gateway base ──────────────────────────────────────────
//
// Browser data plane: SPA → local daemon with trusted-origin CORS (see
// `lib/browser-data-plane.ts` and gateway `browser_data_plane`). Paths must
// stay under that inventory. Cloud cannot relay Confidential data — the
// browser talks to the daemon directly after pair.
// Default "" = same-origin (Vite proxy / daemon-served local).
let HOSTED_GATEWAY_BASE = "";

/** Point hosted-bridge calls at the paired daemon origin (e.g. http://localhost:37373). */
export function configureHostedGateway(baseUrl: string): void {
  HOSTED_GATEWAY_BASE = (baseUrl || "").replace(/\/+$/, "");
}

/** Current hosted-bridge base; "" means same-origin (local/daemon-served). */
export function hostedGatewayBase(): string {
  return HOSTED_GATEWAY_BASE;
}

/**
 * Base URL for post-pair daemon reads (MCP readiness, connections).
 * On local Vite (`:5173`) prefer the same-origin proxy so we do not depend on
 * CORS for `/setup/*` and `/connections` (those historically had no CORS and
 * caused a false "Local daemon disconnected" right after pair).
 * Cloud-hosted SPA still uses the paired daemon origin.
 */
export function hostedDaemonDataBase(): string {
  if (typeof window !== "undefined") {
    const o = window.location.origin;
    if (
      o === "http://127.0.0.1:5173" ||
      o === "http://localhost:5173"
    ) {
      return "";
    }
  }
  return HOSTED_GATEWAY_BASE;
}

// ── Deferred command handling ─────────────────────────────────────

/** Commands recognized by the runtime but deferred beyond the alpha cut line. */
export const DEFERRED_COMMANDS: Record<string, string> = {
  "resolve_compliance":
    "Compliance resolution is planned for a future release.",
  "preflight.run":
    "Preflight checks are planned for a future release.",
  "tracking.untrack_source":
    "Source tracking management is planned for a future release.",
  "tracking.refresh_query":
    "Tracking refresh is planned for a future release.",
};

/** Thrown when a command is recognized but deferred/unsupported in V1. */
export class DeferredCommandError extends Error {
  command: string;
  constructor(command: string, detail?: string) {
    super(
      detail ?? DEFERRED_COMMANDS[command] ?? `${command} is not yet available.`,
    );
    this.name = "DeferredCommandError";
    this.command = command;
  }
}

export async function fetchIngestionGate(): Promise<IngestionGateItem[]> {
  const res = await fetch(`${BASE}/api/v1/dashboard/ingestion-gate`);
  if (!res.ok) throw new Error(`ingestion-gate: ${res.status}`);
  return res.json();
}

export async function fetchLedgerView(): Promise<LedgerReviewItem[]> {
  const res = await fetch(`${BASE}/api/v1/dashboard/ledger`);
  if (!res.ok) throw new Error(`ledger: ${res.status}`);
  return res.json();
}

export async function submitReviewCommand(
  payload: ReviewCommandPayload,
): Promise<void> {
  const res = await fetch(`${BASE}/api/v1/dashboard/command`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  if (!res.ok) {
    // HTTP 410 GONE = typed deferred/unsupported command
    if (res.status === 410) {
      throw new DeferredCommandError(payload.command);
    }
    throw new Error(`command: ${res.status}`);
  }
}

// ── Connection API ────────────────────────────────────────────────

export async function fetchConnectionRegistry(): Promise<ConnectionRegistry> {
  const res = await fetch(`${BASE}/api/v1/connections`);
  if (!res.ok) throw new Error(`connections: ${res.status}`);
  return res.json();
}

export async function startConnection(
  provider: string,
): Promise<ConnectionStartOutcome> {
  const res = await fetch(`${BASE}/api/v1/connections/${provider}/start`, {
    method: "POST",
  });
  if (!res.ok && res.status !== 422) throw new Error(`start: ${res.status}`);
  return res.json();
}

export async function pollConnectionStatus(
  provider: string,
  sessionId: string,
): Promise<ConnectionCompletionOutcome> {
  const res = await fetch(
    `${BASE}/api/v1/connections/${provider}/status/${sessionId}`,
  );
  if (!res.ok) throw new Error(`status: ${res.status}`);
  return res.json();
}

export async function postConnectionCallback(
  provider: string,
  payload: {
    auth_session_id: string;
    code?: string;
    state?: string;
    error?: string;
  },
): Promise<ConnectionCompletionOutcome> {
  const res = await fetch(`${BASE}/api/v1/connections/${provider}/callback`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  if (!res.ok) throw new Error(`callback: ${res.status}`);
  return res.json();
}

// ── Code Impact API ──────────────────────────────────────────────

export async function findCodeImpact(
  decisionId: string,
): Promise<FindCodeImpactResult> {
  const res = await fetch(`${BASE}/api/v1/dashboard/decisions/${decisionId}/code-impact`, {
    method: "POST",
  });
  if (!res.ok) throw new Error(`code-impact: ${res.status}`);
  return res.json();
}

export async function acceptBinding(
  decisionId: string,
  candidateTarget: string,
): Promise<void> {
  const res = await fetch(`${BASE}/api/v1/dashboard/decisions/${decisionId}/accept-binding`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ target: candidateTarget }),
  });
  if (!res.ok) throw new Error(`accept-binding: ${res.status}`);
}

// ── Evidence refresh API ──────────────────────────────────────

export async function refreshEvidence(
  decisionId: string,
): Promise<EvidenceRefreshOutcome> {
  const res = await fetch(`${BASE}/api/v1/evidence/refresh`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ decision_id: decisionId }),
  });
  if (!res.ok) throw new Error(`evidence-refresh: ${res.status}`);
  return res.json();
}

// ── Setup & config API ───────────────────────────────────────────

export type SetupStatus = {
  workspace_exists: boolean;
  workspace_root: string | null;
};

export type WorkspaceConfig = {
  version?: number;
  workspace_name?: string | null;
  event_store?: {
    substrate?: string;
    git_path?: string | null;
    drive_folder_path?: string | null;
    ci_merge_gate?: boolean;
  };
  gateway?: {
    host?: string;
    port?: number;
  };
};

export type ConfigResponse = {
  config: WorkspaceConfig | null;
  errors: string[];
};

export type CreateWorkspaceResponse = {
  success: boolean;
  config_path: string | null;
  errors: string[];
};

export type UpdateConfigRequest = {
  workspace_name?: string;
  gateway_host?: string;
  gateway_port?: number;
  substrate?: string;
  git_path?: string;
  drive_folder_path?: string;
  ci_merge_gate?: boolean;
};

export type UpdateConfigResponse = {
  success: boolean;
  errors: string[];
};

export async function fetchSetupStatus(): Promise<SetupStatus> {
  const res = await fetch(`${BASE}/api/v1/setup/status`);
  if (!res.ok) throw new Error(`setup/status: ${res.status}`);
  return res.json();
}

export async function createWorkspace(opts: {
  workspace_name?: string;
  path?: string;
}): Promise<CreateWorkspaceResponse> {
  const res = await fetch(`${BASE}/api/v1/setup/create-workspace`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(opts),
  });
  if (!res.ok) {
    return {
      success: false,
      config_path: null,
      errors: [`Server error (${res.status}). Is the Bicameral daemon running?`],
    };
  }
  return res.json();
}

export async function fetchConfig(): Promise<ConfigResponse> {
  const res = await fetch(`${BASE}/api/v1/dashboard/config`);
  return res.json();
}

export async function updateConfig(
  req: UpdateConfigRequest,
): Promise<UpdateConfigResponse> {
  const res = await fetch(`${BASE}/api/v1/dashboard/config`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(req),
  });
  return res.json();
}

// ── Cloud project discovery API (bot#720 / cloud#65) ─────────────
//
// The hosted SPA renders its project list from Bicameral Cloud's project
// discovery endpoint. Cloud serves the SPA, so this is a SAME-ORIGIN call
// (`BASE`, not the paired-daemon `HOSTED_GATEWAY_BASE`): the daemon is never the
// source of the discovery list. It requires the caller's Supabase access token
// as a bearer credential (cloud#64 contract). The response is a safe
// coordination projection — no local paths, provider secrets, or authority
// fields (`FORBIDDEN_DISCOVERY_FIELDS`).

export async function fetchCloudProjects(
  opts: LoadCloudProjectsOptions,
): Promise<CloudProjectListResponse> {
  const base = (opts.apiBase ?? BASE).replace(/\/+$/, "");
  const query = opts.selectedProjectId
    ? `?selected_project_id=${encodeURIComponent(opts.selectedProjectId)}`
    : "";
  const res = await fetch(`${base}/api/v1/projects${query}`, {
    headers: { Authorization: `Bearer ${opts.accessToken}` },
    signal: opts.signal,
  });
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw Object.assign(new Error(body.message ?? `HTTP ${res.status}`), {
      status: res.status,
      body,
    });
  }
  return res.json();
}

// ── Cloud project registration API (cloud#66) ────────────────────
//
// The hosted Create/Join actions record a coordination request in Cloud.
// Same-origin (`BASE`) bearer-authenticated calls — never the paired daemon,
// which stays authoritative for project registration and workspace binding.
// Inputs are opaque labels/references; Cloud rejects local paths/secrets at the
// boundary (ADR-0006), so nothing sensitive leaves the browser here.

function cloudApiBase(apiBase?: string): string {
  return (apiBase ?? BASE).replace(/\/+$/, "");
}

async function postCloud<T>(
  url: string,
  accessToken: string,
  body: unknown,
  signal?: AbortSignal,
): Promise<T> {
  const res = await fetch(url, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${accessToken}`,
    },
    body: JSON.stringify(body),
    signal,
  });
  if (!res.ok) {
    const errBody = await res.json().catch(() => ({}));
    throw Object.assign(
      new Error(errBody.detail ?? errBody.message ?? `HTTP ${res.status}`),
      { status: res.status, body: errBody },
    );
  }
  return res.json();
}

export async function createCloudTeam(
  opts: CreateCloudTeamOptions,
): Promise<CloudTeamCreateResponse> {
  return postCloud<CloudTeamCreateResponse>(
    `${cloudApiBase(opts.apiBase)}/api/v1/teams`,
    opts.accessToken,
    { display_name: opts.displayName },
    opts.signal,
  );
}

export async function exposeCloudProject(
  opts: ExposeCloudProjectOptions,
): Promise<CloudRegistrationReceipt> {
  const body: { display_name: string; project_ref?: string } = {
    display_name: opts.displayName,
  };
  if (opts.projectRef) body.project_ref = opts.projectRef;
  return postCloud<CloudRegistrationReceipt>(
    `${cloudApiBase(opts.apiBase)}/api/v1/teams/${encodeURIComponent(
      opts.teamId,
    )}/projects`,
    opts.accessToken,
    body,
    opts.signal,
  );
}

// ── Per-Product same-domain join (bot#768; contract `product.join`) ──
//
// Two contract hops the SPA drives from a per-Product Operator Home `Join`:
//
//   1. Cloud exchanges the authenticated session for a narrow, short-lived
//      `ProductJoinTicket` (same-origin `BASE`, bearer). Cloud never returns the
//      full Supabase token to the browser for daemon use.
//   2. The SPA submits the ticket to the PAIRED DAEMON (`HOSTED_GATEWAY_BASE`,
//      bridge-session-token) which locally confirms actor context, validates the
//      ticket, and returns a daemon-authoritative `ProductJoinResult`.
//
// The SPA sends only the ticket envelope; `local_actor_ref` is added by the
// daemon from the paired session (Cloud identity alone is never a local actor).
// These routes are the SPA's expected surface for the Cloud/daemon join slices
// (cloud#125/cloud#126/bot#767); this frontend slice performs no ticket signing,
// Cloud persistence, or daemon membership materialization (#768 non-goals).

export type RequestProductJoinTicketOptions = {
  preview: HostedProjectPreview;
  accessToken: string;
  apiBase?: string;
  signal?: AbortSignal;
};

export async function requestProductJoinTicket(
  opts: RequestProductJoinTicketOptions,
): Promise<SignedProductJoinTicket> {
  return postCloud<SignedProductJoinTicket>(
    `${cloudApiBase(opts.apiBase)}/api/v1/products/${encodeURIComponent(
      opts.preview.product_id,
    )}/join-ticket`,
    opts.accessToken,
    { preview_id: opts.preview.preview_id },
    opts.signal,
  );
}

export type SubmitProductJoinOptions = {
  sessionToken: string;
  signedTicket: SignedProductJoinTicket;
  signal?: AbortSignal;
};

type ProductJoinToolResponse = {
  status: "ok" | "rejected" | "error";
  result?: ProductJoinResult;
  message?: string;
};

export async function submitProductJoin(
  opts: SubmitProductJoinOptions,
): Promise<ProductJoinResult> {
  const res = await fetchFromHostedDaemon(
    `${HOSTED_GATEWAY_BASE}/api/v1/hosted-bridge/product-join`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "x-bridge-session-token": opts.sessionToken,
      },
      body: JSON.stringify({
        request_id:
          globalThis.crypto?.randomUUID?.() ?? `req_${Date.now()}`,
        signed_ticket: opts.signedTicket,
      }),
      signal: opts.signal,
    },
  );
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw Object.assign(new Error(body.message ?? `HTTP ${res.status}`), {
      status: res.status,
      body,
    });
  }
  const body = (await res.json()) as ProductJoinToolResponse;
  if (body.status !== "ok" || !body.result) {
    // Preserve message + body so product-join recovery can distinguish
    // unsupported_capability / rejected tool responses from transport loss
    // (bot#1013). Prefer nested result when present even on non-ok status.
    if (body.result) {
      return body.result;
    }
    throw Object.assign(
      new Error(body.message ?? "Daemon did not return a product.join result"),
      { status: res.status, body, hop: "daemon" as const },
    );
  }
  return body.result;
}

// ── Local setup continuation (bot#769; contract `product.continue_setup`) ──
//
// After a canonical join, the SPA drives the local continuation against the
// PAIRED DAEMON. The daemon resolves provider access + descriptor validation
// from local state, writes at most one local `ProjectRegistration`, opens +
// replays the Product, and returns the redacted readiness projection below.
//
// The SPA sends only which Product to continue, an opaque routing
// `registration_ref`, and optional non-secret routing hints — never a provider
// secret, raw descriptor, filesystem path, membership claim, or actor context
// (all daemon-derived). The response carries no `registration_ref`, descriptor,
// account label, provider secret, or local path.

/** Redacted setup readiness projection returned by `product.continue_setup`. */
export type HostedSetupProjection = {
  product_id: string;
  /** `open` | `continue_setup` | `repair`. */
  readiness: "open" | "continue_setup" | "repair";
  /** Safe next-step code when `continue_setup` (e.g. `connect_provider`). */
  next_step?: string;
  /** Safe typed reason code when `repair` (e.g. `replay_failed`). */
  repair_reason?: string;
  has_registration: boolean;
  membership_retained: boolean;
};

export type SubmitProductContinueSetupOptions = {
  sessionToken: string;
  productId: string;
  registrationRef: string;
  providerHint?: "github" | "google";
  substrateHint?: "git" | "drive_folder";
  accountLabel?: string;
  signal?: AbortSignal;
};

export async function submitProductContinueSetup(
  opts: SubmitProductContinueSetupOptions,
): Promise<HostedSetupProjection> {
  const res = await fetchFromHostedDaemon(
    `${HOSTED_GATEWAY_BASE}/api/v1/hosted-bridge/product-continue-setup`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "x-bridge-session-token": opts.sessionToken,
      },
      body: JSON.stringify({
        request_id: globalThis.crypto?.randomUUID?.() ?? `req_${Date.now()}`,
        product_id: opts.productId,
        registration_ref: opts.registrationRef,
        provider_hint: opts.providerHint,
        substrate_hint: opts.substrateHint,
        account_label: opts.accountLabel,
      }),
      signal: opts.signal,
    },
  );
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw Object.assign(new Error(body.message ?? `HTTP ${res.status}`), {
      status: res.status,
      body,
    });
  }
  return res.json();
}

// ── Hosted bridge API ────────────────────────────────────────────

export type HostedBridgeStatusResponse = {
  pairing_state: string;
  origin?: string;
  cloud_account_id?: string;
  paired_at?: string;
  expires_at?: string;
  error?: string;
  kind?: string;
  message?: string;
};

export type HostedBridgePairRequest = {
  origin: string;
  cloud_account_id: string;
  csrf_token: string;
  team_id: string;
  cloud_access_token: string;
  requested_ttl_seconds?: number;
  client_protocol_version?: number;
};

export type HostedBridgePairResponse = {
  status?: "pending";
  request_handle?: string;
  poll_proof?: string;
  pairing_code?: string;
  protocol_version?: number;
  confirmation?: {
    local_confirmation_url?: string;
    terminal_command: string;
    message: string;
    requesting_origin: string;
    cloud_account_id: string;
  };
  state?: string;
  session_token?: string;
  expires_at?: string;
  capabilities?: {
    bridge_supported: boolean;
    /** Daemon release identifier for diagnostics; never a compatibility key. */
    daemon_build_version?: string;
    /** Bot-owned hosted-bridge wire-contract compatibility key. */
    hosted_bridge_protocol_version?: number;
    bridge_protocol_version: string;
    /** Integer hosted-dashboard protocol version — the compatibility key (#711). */
    daemon_protocol_version?: number;
    project_list_available: boolean;
    workspace_binding_available: boolean;
    note?: string;
  };
  // Error response fields (e.g. origin_not_trusted, authority_smuggling_rejected)
  error?: string;
  kind?: string;
  origin?: string;
  message?: string;
};

export async function fetchHostedBridgeStatus(
  sessionToken: string,
): Promise<HostedBridgeStatusResponse> {
  const res = await fetchFromHostedDaemon(
    `${HOSTED_GATEWAY_BASE}/api/v1/hosted-bridge/status`,
    { headers: { "x-bridge-session-token": sessionToken } },
  );
  return res.json();
}

export async function requestHostedBridgePairing(
  req: HostedBridgePairRequest,
): Promise<HostedBridgePairResponse> {
  const res = await fetchFromHostedDaemon(
    `${HOSTED_GATEWAY_BASE}/api/v1/hosted-bridge/pair`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        ...req,
        client_protocol_version:
          req.client_protocol_version ?? HOSTED_DASHBOARD_PROTOCOL_VERSION,
      }),
    },
  );
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw Object.assign(new Error(body.message ?? `HTTP ${res.status}`), {
      status: res.status,
      body,
    });
  }
  return res.json();
}

/** Pending-pairing status poll (#790). Token only when status === approved. */
export type HostedBridgePairStatusPollResponse = {
  status: "pending" | "approved" | "denied" | "expired" | "incompatible";
  request_handle?: string;
  session_token?: string;
  expires_at?: string;
  message?: string;
  error?: string;
  kind?: string;
};

export async function pollHostedBridgePairStatus(opts: {
  requestHandle: string;
  pollProof: string;
  cloudAccountId: string;
}): Promise<HostedBridgePairStatusPollResponse> {
  const qs = new URLSearchParams({
    request_handle: opts.requestHandle,
    poll_proof: opts.pollProof,
    cloud_account_id: opts.cloudAccountId,
  });
  const res = await fetchFromHostedDaemon(
    `${HOSTED_GATEWAY_BASE}/api/v1/hosted-bridge/pair/status?${qs}`,
  );
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw Object.assign(new Error(body.message ?? `HTTP ${res.status}`), {
      status: res.status,
      body,
    });
  }
  return res.json();
}

/** Hosted-path connections against the paired daemon (not same-origin cloud). */
export async function fetchHostedConnectionRegistry(
  sessionToken: string,
): Promise<ConnectionRegistry> {
  const res = await fetchFromHostedDaemon(
    `${hostedDaemonDataBase()}/api/v1/connections`,
    { headers: { "x-bridge-session-token": sessionToken } },
  );
  if (!res.ok) throw new Error(`connections: ${res.status}`);
  return res.json();
}

export async function startHostedConnection(
  sessionToken: string,
  provider: string,
): Promise<ConnectionStartOutcome> {
  const res = await fetchFromHostedDaemon(
    `${hostedDaemonDataBase()}/api/v1/connections/${provider}/start`,
    {
      method: "POST",
      headers: { "x-bridge-session-token": sessionToken },
    },
  );
  if (!res.ok && res.status !== 422) throw new Error(`start: ${res.status}`);
  return res.json();
}

// ── Google Drive folder selection (hosted path, #1264) ──────────────
//
// Drives the daemon-owned resource-selection seam. The daemon returns a
// Picker start URL, validates the picked folder itself (Drive `files.get`),
// and exposes only the redacted projection. The SPA never receives tokens.

export async function startHostedResourceSelection(
  sessionToken: string,
): Promise<ResourceSelectionStartOutcome> {
  const res = await fetchFromHostedDaemon(
    `${hostedDaemonDataBase()}/api/v1/setup/google/resource-selection/start`,
    {
      method: "POST",
      headers: { "x-bridge-session-token": sessionToken },
    },
  );
  if (!res.ok) throw new Error(`resource-selection start: ${res.status}`);
  return res.json();
}

export async function pollHostedResourceSelection(
  sessionToken: string,
  selectionSessionId: string,
): Promise<ResourceSelectionProjection> {
  const res = await fetchFromHostedDaemon(
    `${hostedDaemonDataBase()}/api/v1/setup/google/resource-selection/${encodeURIComponent(
      selectionSessionId,
    )}`,
    { headers: { "x-bridge-session-token": sessionToken } },
  );
  if (!res.ok && res.status !== 404) {
    throw new Error(`resource-selection status: ${res.status}`);
  }
  return res.json();
}

export async function pollHostedConnectionStatus(
  sessionToken: string,
  provider: string,
  sessionId: string,
): Promise<ConnectionCompletionOutcome> {
  const res = await fetchFromHostedDaemon(
    `${hostedDaemonDataBase()}/api/v1/connections/${provider}/status/${sessionId}`,
    { headers: { "x-bridge-session-token": sessionToken } },
  );
  if (!res.ok) throw new Error(`status: ${res.status}`);
  return res.json();
}

// ── Hosted-dashboard daemon protocol compatibility (#711) ────────

/** Query the daemon compatibility handshake keyed on `daemon_protocol_version`.
 *
 * Passes the SPA's expected protocol version so the daemon can classify itself
 * as current / N-1 supported / upgrade-required / forward-skew and fold the
 * outcome into non-confidential aggregate telemetry. */
export async function fetchHostedBridgeCompatibility(
  cloudProtocolVersion: number = HOSTED_DASHBOARD_PROTOCOL_VERSION,
): Promise<HostedDashboardCompatibility> {
  const res = await fetchFromHostedDaemon(
    `${HOSTED_GATEWAY_BASE}/api/v1/hosted-bridge/compatibility` +
      `?cloud_protocol_version=${encodeURIComponent(cloudProtocolVersion)}`,
  );
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw Object.assign(new Error(body.message ?? `HTTP ${res.status}`), {
      status: res.status,
      body,
    });
  }
  return res.json();
}

// ── Hosted dashboard adapter API (#616) ──────────────────────────

export async function fetchHostedDashboardAdapter(
  sessionToken: string,
): Promise<HostedDashboardAdapterResponse> {
  const res = await fetchFromHostedDaemon(
    `${HOSTED_GATEWAY_BASE}/api/v1/hosted-bridge/dashboard-adapter`,
    { headers: { "x-bridge-session-token": sessionToken } },
  );
  return res.json();
}

/**
 * Fetch the network-origin renderable ledger projection (#705, T4).
 *
 * Returns only the render-safe allow-list approved in #698 item 5 — opaque
 * ids, timestamps, status/review-state enums, counts, and redactor-passed
 * titles. The daemon drops decision bodies, source excerpts, code paths,
 * actors, and provider secrets before this crosses the browser boundary
 * (ADR-0020 / ADR-0025). This is a read-only, non-authoritative render model.
 */
export async function fetchHostedLedgerProjection(
  sessionToken: string,
): Promise<HostedLedgerProjection> {
  const res = await fetchFromHostedDaemon(
    `${HOSTED_GATEWAY_BASE}/api/v1/hosted-bridge/projection/ledger`,
    { headers: { "x-bridge-session-token": sessionToken } },
  );
  if (!res.ok) throw new Error(`projection/ledger: ${res.status}`);
  return res.json();
}

export type WorkspaceDiscoveryResponse = {
  workspaces: Array<{
    project_name: string;
    event_store_healthy: boolean;
    has_workspace_binding: boolean;
    setup_state: string;
    code_grounding_ready: boolean;
    decision_count: number;
    candidate_count: number;
    event_store_kind?: string;
  }>;
  projection_only: boolean;
};

export async function fetchWorkspaceDiscovery(
  sessionToken: string,
): Promise<WorkspaceDiscoveryResponse> {
  const res = await fetchFromHostedDaemon(
    `${HOSTED_GATEWAY_BASE}/api/v1/hosted-bridge/workspace-discovery`,
    { headers: { "x-bridge-session-token": sessionToken } },
  );
  if (!res.ok) throw new Error(`workspace-discovery: ${res.status}`);
  return res.json();
}

/** Daemon-canonical Product membership for the paired Cloud account. */
export type HostedProductMembershipProjection = {
  product_id: string;
  /** True until daemon-owned ProjectRegistration setup has completed. */
  setup_incomplete: boolean;
};

type HostedProjectsProjectionResponse = {
  memberships: HostedProductMembershipProjection[];
  projection_only: boolean;
};

/**
 * Read the paired account's canonical Product memberships from daemon replay.
 *
 * Cloud remains the discovery/eligibility source; this narrow daemon read
 * reconciles only membership and setup readiness. It carries no role, actor,
 * local path, provider configuration, event body, or credential.
 */
export async function fetchHostedProductMemberships(
  sessionToken: string,
): Promise<HostedProductMembershipProjection[]> {
  const res = await fetchFromHostedDaemon(
    `${HOSTED_GATEWAY_BASE}/api/v1/hosted-bridge/projects`,
    { headers: { "x-bridge-session-token": sessionToken } },
  );
  if (!res.ok) throw new Error(`hosted-bridge/projects: ${res.status}`);
  const body = (await res.json()) as HostedProjectsProjectionResponse;
  return body.memberships ?? [];
}

// ── Selected-project workspace readiness (bot#733 / #734) ────────
//
// Scopes the daemon's readiness projection to a single selected project,
// returning the daemon-owned `workspace_readiness` (#733). This is the paired
// DAEMON surface (`HOSTED_GATEWAY_BASE`), guarded by the bridge session token.
// The daemon reports only local readiness for the already-selected project; a
// selection with no local binding resolves to `unbound` ("needs binding"), not
// an error. The response carries no local path, provider secret, or source body.

export type FetchSelectedProjectReadinessOptions = {
  /** Owning cloud team id, echoed back by the daemon (coordination only). */
  teamId?: string;
  /** Human label for the selected project (no path/secret leakage). */
  displayName?: string;
  signal?: AbortSignal;
};

export async function fetchSelectedProjectReadiness(
  sessionToken: string,
  projectId: string,
  opts: FetchSelectedProjectReadinessOptions = {},
): Promise<SelectedProjectSessionResponse> {
  const params = new URLSearchParams();
  if (opts.teamId) params.set("team_id", opts.teamId);
  if (opts.displayName) params.set("display_name", opts.displayName);
  const query = params.toString() ? `?${params.toString()}` : "";
  const res = await fetchFromHostedDaemon(
    `${HOSTED_GATEWAY_BASE}/api/v1/hosted-bridge/projects/${encodeURIComponent(
      projectId,
    )}${query}`,
    {
      headers: { "x-bridge-session-token": sessionToken },
      signal: opts.signal,
    },
  );
  if (!res.ok) throw new Error(`hosted-bridge/projects: ${res.status}`);
  return res.json();
}
