/**
 * Workspace binding readiness — hosted dashboard consumer (bicameral-bot#734).
 *
 * Consumes the daemon-owned readiness projection exposed over the hosted bridge
 * by #733 (`GET /api/v1/hosted-bridge/projects/:project_id` →
 * `SelectedProjectSessionResponse.workspace_readiness`) and derives the
 * renderer-only view the hosted dashboard shows for a selected project.
 *
 * Authority boundary (ADR-0005 / ADR-0006 / ADR-0020 / ADR-0025): the hosted
 * surface is a RENDERER ONLY. It never binds a workspace, never computes
 * authority, and introduces no new durable UI-only state beyond the
 * loading / error / polling presentation states in this module. Binding is
 * performed exclusively on a LOCAL surface (MCP / CLI / local dashboard); the
 * hosted bridge is never a bind surface. The projection carries no local
 * absolute path, provider token, source body, graph index, or event-store
 * body, so neither does any derived view here.
 */

import type {
  HostedWorkspaceBindingReadiness,
  HostedDashboardCompatibility,
  HostedDashboardUpgradeRepair,
  SelectedProjectSessionResponse,
  WorkspaceBindingSourceSurface,
} from "./hosted-bridge-types";
import * as api from "./api";

// ── Renderer-only view model ───────────────────────────────────────

/**
 * The renderer-only view the workspace binding card shows. This is transient
 * presentation state, NOT durable/authoritative state: the durable workspace
 * lifecycle (`local_workspace_unbound` / `local_workspace_bound` /
 * `local_workspace_repair_required`) is owned by the daemon (#732/#733).
 */
export type WorkspaceBindingView =
  | { kind: "loading" }
  | { kind: "error"; message: string; retryable: boolean }
  | {
      /**
       * The binding flow is not supported by the paired daemon/bridge
       * capability. Fails closed: no bind controls are offered; only
       * upgrade/repair guidance is shown.
       */
      kind: "unsupported";
      message: string;
      repair?: HostedDashboardUpgradeRepair;
    }
  | {
      /** Bound and ready for code grounding. No next action, no path. */
      kind: "bound";
      projectName: string;
      message: string;
    }
  | {
      /**
       * Registered but not yet bound to a local workspace. This is a
       * next-step CTA, NOT a registration failure. MCP-assisted bind is the
       * primary alpha path; CLI is a fallback when the daemon surface offers
       * it.
       */
      kind: "unbound";
      projectName: string;
      caveats: string[];
      command: string;
      instruction: string;
      mcpAvailable: boolean;
      cliAvailable: boolean;
      /** Whether the UI is actively polling for the daemon to report bound. */
      polling: boolean;
    }
  | {
      /** A previously usable binding is broken; typed repair guidance. */
      kind: "repair_required";
      projectName: string;
      caveats: string[];
      command: string;
      instruction: string;
      mcpAvailable: boolean;
      cliAvailable: boolean;
    };

/** True while the view is a non-terminal state the UI should keep polling. */
export function shouldPoll(view: WorkspaceBindingView): boolean {
  return view.kind === "unbound";
}

// ── Pure derivation ────────────────────────────────────────────────

export type DeriveWorkspaceBindingViewInput = {
  /** Daemon-owned readiness projection, or null when not yet resolved. */
  readiness: HostedWorkspaceBindingReadiness | null;
  /**
   * Optional daemon protocol compatibility. When the daemon is too old to
   * support the binding flow (`upgrade_required`, or controls unavailable and
   * projections unavailable), the view fails closed to `unsupported`.
   */
  compatibility?: HostedDashboardCompatibility;
  /**
   * Set when the coarse daemon binding readiness reports `upgrade_required`
   * for the selected project even if the compatibility handshake was not run.
   */
  upgradeRequired?: boolean;
  /** Whether the UI is currently polling. */
  polling?: boolean;
};

function hasSurface(
  surfaces: WorkspaceBindingSourceSurface[] | undefined,
  surface: WorkspaceBindingSourceSurface,
): boolean {
  return (surfaces ?? []).includes(surface);
}

/**
 * Derive the renderer-only view from the daemon-owned readiness projection.
 *
 * Pure and deterministic: it never fetches, binds, or mutates. An unsupported
 * capability fails closed before any bind CTA is offered.
 */
export function deriveWorkspaceBindingView(
  input: DeriveWorkspaceBindingViewInput,
): WorkspaceBindingView {
  const { readiness, compatibility, upgradeRequired, polling = false } = input;

  // Fail closed first: an unsupported daemon/bridge capability must never
  // render a bind CTA — only upgrade/repair guidance.
  const capabilityUnsupported =
    upgradeRequired === true ||
    (compatibility !== undefined &&
      (compatibility.upgrade_required ||
        (!compatibility.controls_available &&
          !compatibility.read_only_projections_available)));

  if (capabilityUnsupported) {
    return {
      kind: "unsupported",
      message:
        compatibility?.note ??
        "The local daemon is too old to support workspace binding from the " +
          "hosted dashboard. Update the daemon, then reconnect.",
      repair: compatibility?.repair,
    };
  }

  if (readiness === null) {
    return { kind: "loading" };
  }

  const projectName = readiness.display.display_name;
  const caveats = readiness.caveats ?? [];
  const action = readiness.next_action;
  const mcpAvailable = hasSurface(action?.surfaces, "mcp");
  const cliAvailable = hasSurface(action?.surfaces, "cli");

  switch (readiness.binding_state) {
    case "local_workspace_bound":
      return {
        kind: "bound",
        projectName,
        message:
          "This project is bound to a local workspace and ready for " +
          "code-grounded review.",
      };
    case "local_workspace_unbound":
      return {
        kind: "unbound",
        projectName,
        caveats,
        command: action?.command ?? "bicameral workspace bind",
        instruction:
          action?.instruction ??
          "Bind this project to a local code folder from the MCP client on " +
            "the machine running the daemon.",
        mcpAvailable,
        cliAvailable,
        polling,
      };
    case "local_workspace_repair_required":
      return {
        kind: "repair_required",
        projectName,
        caveats,
        command: action?.command ?? "bicameral workspace repair",
        instruction:
          action?.instruction ??
          "Repair the local workspace binding from the MCP client or CLI on " +
            "the machine running the daemon.",
        mcpAvailable,
        cliAvailable,
      };
    default:
      return { kind: "loading" };
  }
}

// ── Live fetch + classification ────────────────────────────────────

export type LoadWorkspaceReadinessOptions = {
  sessionToken: string;
  projectId: string;
  teamId?: string;
  displayName?: string;
  compatibility?: HostedDashboardCompatibility;
  polling?: boolean;
  signal?: AbortSignal;
};

function isNetworkError(err: unknown): boolean {
  if (err instanceof TypeError) return true;
  if (err instanceof Error && err.message.includes("fetch")) return true;
  return false;
}

/**
 * Fetch the selected-project readiness from the paired daemon and derive the
 * renderer-only view. Never throws: network/HTTP failures resolve to an
 * `error` view so the caller renders a neutral, retryable state.
 */
export async function loadWorkspaceBindingView(
  opts: LoadWorkspaceReadinessOptions,
): Promise<WorkspaceBindingView> {
  let response: SelectedProjectSessionResponse;
  try {
    response = await api.fetchSelectedProjectReadiness(opts.sessionToken, opts.projectId, {
      teamId: opts.teamId,
      displayName: opts.displayName,
      signal: opts.signal,
    });
  } catch (err: unknown) {
    return {
      kind: "error",
      message: isNetworkError(err)
        ? "Could not reach the local Bicameral daemon to check workspace " +
          "readiness. Check that it is running, then retry."
        : "The daemon returned an error while resolving workspace readiness.",
      retryable: true,
    };
  }

  return deriveWorkspaceBindingView({
    readiness: response.workspace_readiness ?? null,
    compatibility: opts.compatibility,
    upgradeRequired: response.binding_readiness === "upgrade_required",
    polling: opts.polling ?? false,
  });
}
