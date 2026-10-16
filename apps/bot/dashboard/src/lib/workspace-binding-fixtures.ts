/**
 * Fixture data for the hosted workspace binding readiness UI (bicameral-bot#734).
 *
 * These mirror the daemon-owned `HostedWorkspaceBindingReadiness` projection
 * (#733) for each durable workspace state. They carry NO local absolute path,
 * provider token, source body, graph index, or event-store body — the hosted
 * boundary never sees those. Follows the fixture pattern in
 * `hosted-bridge-fixtures.ts`.
 */

import type { HostedWorkspaceBindingReadiness } from "./hosted-bridge-types";

const BIND_SURFACES = ["mcp", "cli"] as const;

export const WORKSPACE_READINESS_FIXTURES: Record<
  string,
  HostedWorkspaceBindingReadiness
> = {
  unbound: {
    project_id: "ada-service",
    display: { display_name: "Ada Service", project_slug: "ada-service" },
    binding_state: "local_workspace_unbound",
    code_grounding_ready: false,
    caveats: [
      "No local workspace binding on this daemon; code grounding is " +
        "unavailable until the project is bound.",
    ],
    next_action: {
      kind: "bind_local_workspace",
      surfaces: [...BIND_SURFACES],
      command: "bicameral workspace bind",
      instruction:
        "Bind this project to a local code folder from the MCP client or CLI " +
        "on the machine running the daemon. The hosted dashboard cannot bind " +
        "a workspace.",
    },
  },

  bound: {
    project_id: "ada-service",
    display: { display_name: "Ada Service", project_slug: "ada-service" },
    binding_state: "local_workspace_bound",
    code_grounding_ready: true,
    caveats: [],
  },

  repair_required: {
    project_id: "ada-service",
    display: { display_name: "Ada Service", project_slug: "ada-service" },
    binding_state: "local_workspace_repair_required",
    code_grounding_ready: false,
    caveats: [
      "The local workspace binding is broken and must be repaired before " +
        "code grounding can resume.",
    ],
    next_action: {
      kind: "repair_local_workspace",
      surfaces: [...BIND_SURFACES],
      command: "bicameral workspace repair",
      instruction:
        "Repair the local workspace binding from the MCP client or CLI on the " +
        "machine running the daemon.",
    },
  },
};
