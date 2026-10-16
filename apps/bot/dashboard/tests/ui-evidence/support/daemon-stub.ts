/**
 * Deterministic daemon + Cloud responses for the headed UI evidence harness.
 *
 * SCOPE BOUNDARY — read before adding anything here.
 *
 * Everything in this file exists only inside Playwright request interception.
 * It is never imported by `src/`, never bundled, and never served. It cannot
 * make a Product, Connection, credential, pairing, or Decision canonical: it
 * only replays the wire shapes the daemon and Cloud already own so the real SPA
 * can be exercised in a real browser without a live daemon.
 *
 * Two rules keep it honest:
 *   1. No response here may assert authority the real daemon would not assert.
 *      A `connected` registry entry is the daemon's word, replayed; the SPA
 *      still has to go and read it.
 *   2. Any `/api/**` request the SPA makes that this table does not explicitly
 *      answer is recorded as UNEXPECTED and fails the scenario. Silence is not
 *      a pass — an unstubbed call means the surface talks to something we did
 *      not model.
 */

import type { Page, Route } from "@playwright/test";

/** Obviously-synthetic values. Nothing here is or resembles a real credential. */
export const HARNESS = {
  accountId: "evidence-operator",
  /** Bridge session token: opaque routing value, never rendered by the SPA. */
  sessionToken: "ui-evidence-harness-session-value-not-a-credential",
  cloudAccessToken: "ui-evidence-harness-cloud-value-not-a-credential",
  teamId: "team_evidence",
  productId: "prod_evidence_alpha",
  productName: "Evidence Alpha",
} as const;

/** Strings that must never appear in a screenshot, DOM dump, or URL. */
export const SECRET_MARKERS = [
  HARNESS.sessionToken,
  HARNESS.cloudAccessToken,
];

export type McpState =
  | "not_installed"
  | "prerequisite_missing"
  | "installed_pending_host_action"
  | "active_verified"
  | "repair_required";

export type ConnectionStatus =
  | "connected"
  | "not_connected"
  | "action_needed"
  | "unsupported";

export type GoogleConnection = {
  status: ConnectionStatus;
  account_label?: string;
  capabilities?: string[];
  auth_method?: string;
  updated_at?: string;
};

export type JsonBody = Record<string, unknown> | unknown[];

/** One stubbed reply. `status` defaults to 200. */
export type Reply = { status?: number; body: JsonBody };

/**
 * A scenario is a declarative description of what the daemon and Cloud report.
 *
 * Array-valued fields are consumed one entry per call; the final entry repeats
 * for any further calls. That models polling without timing races.
 */
export type DaemonScenario = {
  /** Daemon `GET /api/v1/setup/capability-readiness`. */
  mcp?: {
    state: McpState;
    reasons?: string[];
    hostId?: string;
    safeDisplayArgv?: string[] | null;
    lastVerifiedAt?: string | null;
  };
  /** Daemon `GET /api/v1/connections` (ConnectionRegistry). */
  connectionRegistry?: (GoogleConnection | null)[];
  /**
   * Registry answer once a connection status call has returned a terminal
   * `connected` outcome — i.e. the daemon persisted the credential.
   *
   * Modelling the transition as a consequence of the OAuth result, rather than
   * as the Nth element of a sequence, keeps a scenario independent of how many
   * times a given layout happens to read the registry. Omit it to model a
   * daemon that reported terminal success but never made the connection real,
   * which is the case the UI must refuse to render as connected.
   */
  connectionRegistryAfterConnect?: GoogleConnection;
  /** Daemon `POST /api/v1/connections/google/start`. */
  connectionStart?: Reply[];
  /** Daemon `GET /api/v1/connections/google/status/:id`. */
  connectionStatus?: Reply[];
  /** Daemon `GET /api/v1/hosted-bridge/projects` (canonical memberships). */
  memberships?: { product_id: string; setup_incomplete: boolean }[];
  /** Cloud `GET /api/v1/projects` (discovery). */
  cloudProjects?: JsonBody;
  /** Daemon `POST /api/v1/hosted-bridge/product-continue-setup`. */
  continueSetup?: Reply[];
  /** Daemon `GET /api/v1/hosted-bridge/status`. */
  bridgeStatus?: Reply;
  /** Extra exact-path replies, for surfaces a scenario adds. */
  extra?: Record<string, Reply | Reply[]>;
};

export type RecordedCall = {
  method: string;
  path: string;
  /** True when no entry in the route table matched. */
  unexpected: boolean;
};

const OK = (body: JsonBody): Reply => ({ status: 200, body });

function mcpBody(scenario: DaemonScenario): JsonBody {
  const mcp = scenario.mcp ?? { state: "not_installed" as McpState };
  return {
    capability_readiness: [
      {
        schema_version: 1,
        capability_kind: "mcp",
        subject_id: mcp.hostId ?? "claude",
        state: mcp.state,
        reason_codes: mcp.reasons ?? [],
        observed_at: "2026-01-01T00:00:00Z",
        last_verified_at: mcp.lastVerifiedAt ?? null,
        safe_display_argv: mcp.safeDisplayArgv ?? ["bicameral", "setup", "mcp"],
      },
    ],
  };
}

function registryBody(google: GoogleConnection | null): JsonBody {
  const connections: Record<string, unknown> = {};
  if (google) {
    connections.google = {
      status: google.status,
      account_label: google.account_label,
      capabilities: google.capabilities ?? ["drive.file"],
      auth_method: google.auth_method ?? "local_loopback",
      updated_at: google.updated_at ?? "2026-01-01T00:00:00Z",
    };
  }
  return { connections };
}

/**
 * Serve one scenario for one page, recording every `/api/**` call.
 *
 * Returns a handle the spec can assert against — notably `unexpected` (any
 * unmodelled API call) and `countMatching` (used to prove a contract hop such
 * as `product.join` was never invoked).
 */
export class DaemonStub {
  readonly calls: RecordedCall[] = [];
  private cursors = new Map<string, number>();
  /** Set once a status call has returned a terminal `connected` outcome. */
  private providerConnected = false;

  constructor(private readonly scenario: DaemonScenario) {}

  get unexpected(): RecordedCall[] {
    return this.calls.filter((c) => c.unexpected);
  }

  paths(): string[] {
    return this.calls.map((c) => `${c.method} ${c.path}`);
  }

  /** How many recorded calls match a substring or regex. */
  countMatching(pattern: string | RegExp): number {
    return this.calls.filter((c) =>
      typeof pattern === "string"
        ? c.path.includes(pattern)
        : pattern.test(c.path),
    ).length;
  }

  private next<T>(key: string, values: T[] | undefined): T | undefined {
    if (!values || values.length === 0) return undefined;
    const i = this.cursors.get(key) ?? 0;
    this.cursors.set(key, i + 1);
    return values[Math.min(i, values.length - 1)];
  }

  private isConnectedOutcome(reply: Reply | undefined): boolean {
    const body = reply?.body as { outcome?: string } | undefined;
    return body?.outcome === "connected";
  }

  private resolve(method: string, path: string): Reply | undefined {
    const s = this.scenario;

    const extra = s.extra?.[path];
    if (extra) {
      return Array.isArray(extra) ? this.next(`extra:${path}`, extra) : extra;
    }

    if (path === "/api/v1/setup/capability-readiness") {
      return OK(mcpBody(s));
    }

    if (path === "/api/v1/connections" && method === "GET") {
      if (this.providerConnected && s.connectionRegistryAfterConnect) {
        return OK(registryBody(s.connectionRegistryAfterConnect));
      }
      const entry = this.next("registry", s.connectionRegistry);
      if (entry === undefined) return undefined;
      return OK(registryBody(entry));
    }

    if (/^\/api\/v1\/connections\/[^/]+\/start$/.test(path) && method === "POST") {
      const reply = this.next("start", s.connectionStart);
      if (this.isConnectedOutcome(reply)) this.providerConnected = true;
      return reply;
    }

    if (/^\/api\/v1\/connections\/[^/]+\/status\/[^/]+$/.test(path)) {
      const reply = this.next("status", s.connectionStatus);
      if (this.isConnectedOutcome(reply)) this.providerConnected = true;
      return reply;
    }

    if (path === "/api/v1/hosted-bridge/status") {
      return s.bridgeStatus;
    }

    if (path === "/api/v1/hosted-bridge/projects" && method === "GET") {
      if (!s.memberships) return undefined;
      return OK({ memberships: s.memberships, projection_only: true });
    }

    if (path === "/api/v1/hosted-bridge/product-continue-setup") {
      return this.next("continue", s.continueSetup);
    }

    if (path === "/api/v1/projects" && method === "GET") {
      if (!s.cloudProjects) return undefined;
      return OK(s.cloudProjects);
    }

    return undefined;
  }

  async handle(route: Route): Promise<void> {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    const method = request.method();
    const reply = this.resolve(method, path);
    this.calls.push({ method, path, unexpected: reply === undefined });
    if (!reply) {
      // Fail loudly and non-ambiguously: 599 is not a state the SPA models, so
      // an unmodelled call cannot masquerade as a normal error path.
      await route.fulfill({
        status: 599,
        contentType: "application/json",
        body: JSON.stringify({ error: "ui_evidence_harness_unstubbed_route", path }),
      });
      return;
    }
    await route.fulfill({
      status: reply.status ?? 200,
      contentType: "application/json",
      body: JSON.stringify(reply.body),
    });
  }
}

/**
 * Install the stub plus the offline shims the evidence run needs.
 *
 * Web fonts are fulfilled empty so a run is deterministic and network-free;
 * everything else is either modelled or recorded as unexpected.
 */
export async function installDaemonStub(
  page: Page,
  scenario: DaemonScenario,
): Promise<DaemonStub> {
  const stub = new DaemonStub(scenario);
  await page.route(/https:\/\/fonts\.(googleapis|gstatic)\.com\/.*/, (route) =>
    route.fulfill({ status: 200, contentType: "text/css", body: "" }),
  );
  await page.route("**/api/**", (route) => stub.handle(route));
  return stub;
}

/** A single member Product row as Cloud discovery reports it. */
export function cloudMemberProject(overrides: Record<string, unknown> = {}): JsonBody {
  return {
    projects: [
      {
        project_id: HARNESS.productId,
        team_id: HARNESS.teamId,
        display_name: HARNESS.productName,
        membership_role: "team_owner",
        pairing_status: "paired",
        binding_readiness: "ready",
        daemon_compatible: true,
        is_member: true,
        ...overrides,
      },
    ],
    selected_project_id: null,
  };
}
