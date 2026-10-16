/**
 * Daemon MCP / capability readiness for the hosted connect-device gate.
 *
 * Spec: docs/specs/mcp-distribution-and-developer-host-setup.md
 * SPA is a renderer only — never installs MCP or mutates readiness.
 */

import { fetchFromHostedDaemon } from "./hosted-daemon-connection";
import { hostedDaemonDataBase } from "./api";

/** Mirrors ui/McpReadinessCard McpReadinessState (keep in lockstep). */
export type McpReadinessState =
  | "not_installed"
  | "prerequisite_missing"
  | "installed_pending_host_action"
  | "active_verified"
  | "repair_required";

/** Mirrors `DaemonCapabilityReadinessV1` wire shape from the daemon. */
export type DaemonCapabilityReadiness = {
  schema_version?: number;
  capability_kind?: string;
  subject_id?: string;
  artifact_generation?: string;
  state: string;
  reason_codes?: string[];
  observed_at?: string;
  last_verified_at?: string | null;
  next_action_code?: string | null;
  safe_display_argv?: string[] | null;
};

/** Default operator copy until daemon `safe_display_argv` is present. */
export const DEFAULT_MCP_SETUP_COMMAND = "bicameral setup mcp";

export type McpReadinessView = {
  state: McpReadinessState;
  hostId: string;
  reasons: string[];
  observedAt?: string;
  lastVerifiedAt?: string | null;
  /** Clipboard/display command; prefers daemon safe_display_argv when present. */
  setupCommand: string;
};

const STATE_MAP: Record<string, McpReadinessState> = {
  not_installed: "not_installed",
  prerequisite_missing: "prerequisite_missing",
  installed_pending_host_action: "installed_pending_host_action",
  active_verified: "active_verified",
  repair_required: "repair_required",
};

export function mapDaemonMcpState(raw: string | undefined): McpReadinessState {
  if (!raw) return "not_installed";
  return STATE_MAP[raw] ?? "not_installed";
}

export function setupCommandFromArgv(
  argv: string[] | null | undefined,
): string {
  if (!argv || argv.length === 0) return DEFAULT_MCP_SETUP_COMMAND;
  // Prefer primary verb setup mcp; alias developer-host still works on CLI.
  const joined = argv.join(" ").trim();
  return joined.length > 0 ? joined : DEFAULT_MCP_SETUP_COMMAND;
}

export function mcpViewFromCapabilities(
  items: DaemonCapabilityReadiness[],
): McpReadinessView {
  const mcp =
    items.find((i) => (i.capability_kind ?? "mcp") === "mcp") ?? items[0];
  if (!mcp) {
    return {
      state: "not_installed",
      hostId: "claude",
      reasons: ["package_absent"],
      setupCommand: DEFAULT_MCP_SETUP_COMMAND,
    };
  }
  return {
    state: mapDaemonMcpState(mcp.state),
    hostId: mcp.subject_id ?? "claude",
    reasons: mcp.reason_codes ?? [],
    observedAt: mcp.observed_at,
    lastVerifiedAt: mcp.last_verified_at,
    setupCommand: setupCommandFromArgv(mcp.safe_display_argv),
  };
}

/** GET /api/v1/setup/capability-readiness on the paired daemon. */
export async function fetchMcpReadiness(
  sessionToken?: string,
): Promise<McpReadinessView> {
  const base = hostedDaemonDataBase();
  const headers: Record<string, string> = {};
  if (sessionToken) headers["x-bridge-session-token"] = sessionToken;
  const res = await fetchFromHostedDaemon(
    `${base}/api/v1/setup/capability-readiness`,
    { headers },
  );
  if (!res.ok) {
    return {
      state: "not_installed",
      hostId: "claude",
      reasons: ["daemon_unavailable"],
      setupCommand: DEFAULT_MCP_SETUP_COMMAND,
    };
  }
  const body = (await res.json()) as
    | DaemonCapabilityReadiness[]
    | { capability_readiness?: DaemonCapabilityReadiness[] };
  const items = Array.isArray(body)
    ? body
    : (body.capability_readiness ?? []);
  return mcpViewFromCapabilities(items);
}

/** True only when a fresh activation receipt matched the package (strict). */
export function isMcpVerified(state: McpReadinessState): boolean {
  return state === "active_verified";
}

/**
 * Connect-device / product-shell gate for "enough to proceed".
 *
 * `installed_pending_host_action` means package + registration look good and
 * only host trust/session activation is outstanding. Blocking Continue on
 * activation alone stalls UX after a successful `setup mcp` and is too strict
 * for alpha connect-device. Full verification still surfaces as status copy.
 */
export function isMcpProgressible(state: McpReadinessState): boolean {
  return state === "active_verified" || state === "installed_pending_host_action";
}
