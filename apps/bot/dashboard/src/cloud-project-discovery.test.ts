/**
 * Cloud project discovery consumer tests (bot#720).
 *
 * Proves the hosted SPA renders its project list from the Cloud project
 * discovery API (`GET /api/v1/projects`, cloud#65) and NOT from local fixtures
 * (SAMPLE_PROJECTS) or daemon-only workspace discovery:
 *
 *   1. module surface;
 *   2. `fetchCloudProjects` targets same-origin `/api/v1/projects` with the
 *      Supabase bearer token (never the paired-daemon origin / bridge token);
 *   3. `BindingReadiness` → UI readiness mapping;
 *   4. `loadCloudProjects` maps the response and never throws;
 *   5. the mapped list is grounded in the Cloud response, independent of
 *      SAMPLE_PROJECTS;
 *   6. the production hosted runtime (App.svelte) sources the list from Cloud
 *      discovery, not fixtures or daemon workspace discovery.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import appSource from "./App.svelte?raw";
import discoverySource from "./lib/cloud-project-discovery.ts?raw";
import type {
  CloudProjectListItem,
  CloudProjectListResponse,
} from "./lib/cloud-project-discovery";

function item(
  overrides: Partial<CloudProjectListItem> = {},
): CloudProjectListItem {
  return {
    project_id: "proj_cloud_1",
    team_id: "team_1",
    display_name: "Cloud Project One",
    pairing_status: "paired",
    binding_readiness: "ready",
    daemon_compatible: true,
    ...overrides,
  };
}

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

// ── 1. Module surface ──────────────────────────────────────────────

describe("Cloud project discovery module surface", () => {
  it("exports the consumer functions", async () => {
    const mod = await import("./lib/cloud-project-discovery");
    expect(typeof mod.loadCloudProjects).toBe("function");
    expect(typeof mod.mapCloudProjects).toBe("function");
    expect(typeof mod.mapCloudProjectItem).toBe("function");
    expect(typeof mod.resolveCloudReadiness).toBe("function");
  });

  it("exports fetchCloudProjects from the API client", async () => {
    const { fetchCloudProjects } = await import("./lib/api");
    expect(typeof fetchCloudProjects).toBe("function");
  });
});

// ── 2. fetchCloudProjects targets same-origin Cloud with a bearer token ──

describe("fetchCloudProjects", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("calls same-origin /api/v1/projects with the Supabase bearer token", async () => {
    const spy = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(jsonResponse({ projects: [] }));
    const { fetchCloudProjects } = await import("./lib/api");
    await fetchCloudProjects({ accessToken: "supabase_tok_123" });

    expect(spy).toHaveBeenCalledTimes(1);
    const [url, init] = spy.mock.calls[0];
    // Same-origin: no daemon origin (localhost:37373) in the URL.
    expect(String(url)).toBe("/api/v1/projects");
    expect(String(url)).not.toContain("37373");
    const headers = (init as RequestInit).headers as Record<string, string>;
    expect(headers.Authorization).toBe("Bearer supabase_tok_123");
    // Never sends the daemon bridge session-token header.
    expect(headers["x-bridge-session-token"]).toBeUndefined();
  });

  it("passes selected_project_id as a query param when provided", async () => {
    const spy = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(jsonResponse({ projects: [] }));
    const { fetchCloudProjects } = await import("./lib/api");
    await fetchCloudProjects({
      accessToken: "tok",
      selectedProjectId: "proj_cloud_9",
    });
    expect(String(spy.mock.calls[0][0])).toBe(
      "/api/v1/projects?selected_project_id=proj_cloud_9",
    );
  });

  it("honors an explicit cloud apiBase origin", async () => {
    const spy = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(jsonResponse({ projects: [] }));
    const { fetchCloudProjects } = await import("./lib/api");
    await fetchCloudProjects({
      accessToken: "tok",
      apiBase: "https://app.bicameral-ai.com/",
    });
    expect(String(spy.mock.calls[0][0])).toBe(
      "https://app.bicameral-ai.com/api/v1/projects",
    );
  });

  it("throws with the HTTP status on a non-ok response", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      jsonResponse({ message: "unauthorized" }, 401),
    );
    const { fetchCloudProjects } = await import("./lib/api");
    await expect(fetchCloudProjects({ accessToken: "bad" })).rejects.toThrow(
      "unauthorized",
    );
  });
});

// ── 3. BindingReadiness → UI readiness mapping ─────────────────────

describe("resolveCloudReadiness", () => {
  it("maps binding readiness to the UI readiness buckets", async () => {
    const { resolveCloudReadiness } = await import(
      "./lib/cloud-project-discovery"
    );
    expect(resolveCloudReadiness("paired", "ready")).toBe("ready");
    expect(resolveCloudReadiness("paired", "degraded")).toBe("ready");
    expect(resolveCloudReadiness("paired", "unbound")).toBe("needs_access");
    expect(resolveCloudReadiness("paired", "upgrade_required")).toBe(
      "needs_access",
    );
    expect(resolveCloudReadiness("paired", "unknown")).toBe("offline");
    expect(resolveCloudReadiness("paired", "unpaired")).toBe("offline");
  });

  it("treats an unpaired pairing status as offline regardless of binding", async () => {
    const { resolveCloudReadiness } = await import(
      "./lib/cloud-project-discovery"
    );
    expect(resolveCloudReadiness("unpaired", "ready")).toBe("offline");
  });
});

describe("mapCloudProjectItem", () => {
  it("maps a Cloud project item to the UI ProjectEntry shape", async () => {
    const { mapCloudProjectItem } = await import(
      "./lib/cloud-project-discovery"
    );
    const entry = mapCloudProjectItem(
      item({
        project_id: "proj_abc",
        display_name: "Payments",
        binding_readiness: "ready",
      }),
    );
    expect(entry.id).toBe("proj_abc");
    expect(entry.name).toBe("Payments");
    expect(entry.readiness).toBe("ready");
    expect(entry.event_store_label).toBe("Cloud project");
  });

  it("flags an incompatible daemon in the row label", async () => {
    const { mapCloudProjectItem } = await import(
      "./lib/cloud-project-discovery"
    );
    const entry = mapCloudProjectItem(item({ daemon_compatible: false }));
    expect(entry.event_store_label).toContain("update the local daemon");
  });
});

// ── 4. loadCloudProjects orchestration ─────────────────────────────

describe("loadCloudProjects", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("maps a successful discovery response to ProjectEntry[]", async () => {
    const response: CloudProjectListResponse = {
      projects: [
        item({ project_id: "p1", display_name: "Alpha", binding_readiness: "ready" }),
        item({
          project_id: "p2",
          display_name: "Beta",
          binding_readiness: "unbound",
        }),
      ],
      selected_project_id: "p1",
    };
    vi.spyOn(globalThis, "fetch").mockResolvedValue(jsonResponse(response));
    const { loadCloudProjects } = await import("./lib/cloud-project-discovery");
    const result = await loadCloudProjects({ accessToken: "tok" });

    expect(result.error).toBeUndefined();
    expect(result.selectedProjectId).toBe("p1");
    expect(result.projects.map((p) => p.id)).toEqual(["p1", "p2"]);
    expect(result.projects[0].readiness).toBe("ready");
    expect(result.projects[1].readiness).toBe("needs_access");
  });

  it("returns an empty list and an error on network failure (never throws)", async () => {
    vi.spyOn(globalThis, "fetch").mockRejectedValue(
      new TypeError("Failed to fetch"),
    );
    const { loadCloudProjects } = await import("./lib/cloud-project-discovery");
    const result = await loadCloudProjects({ accessToken: "tok" });
    expect(result.projects).toEqual([]);
    expect(result.error).toBeTruthy();
  });

  it("returns an empty list and an error on HTTP 500", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      jsonResponse({ message: "boom" }, 500),
    );
    const { loadCloudProjects } = await import("./lib/cloud-project-discovery");
    const result = await loadCloudProjects({ accessToken: "tok" });
    expect(result.projects).toEqual([]);
    expect(result.error).toBeTruthy();
  });
});

// ── 5. The mapped list is grounded in the Cloud response, not fixtures ──

describe("hosted project list is grounded in Cloud discovery, not fixtures", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("mapped ids come from the Cloud response and never match SAMPLE_PROJECTS", async () => {
    const { SAMPLE_PROJECTS } = await import("./lib/project-fixtures");
    const sampleIds = new Set(SAMPLE_PROJECTS.map((p) => p.id));
    const response: CloudProjectListResponse = {
      projects: [item({ project_id: "cloud_only_1", display_name: "Cloud Only" })],
    };
    vi.spyOn(globalThis, "fetch").mockResolvedValue(jsonResponse(response));
    const { loadCloudProjects } = await import("./lib/cloud-project-discovery");
    const result = await loadCloudProjects({ accessToken: "tok" });

    expect(result.projects).toHaveLength(1);
    for (const project of result.projects) {
      expect(sampleIds.has(project.id)).toBe(false);
    }
    expect(result.projects[0].id).toBe("cloud_only_1");
  });

  it("discovery module carries no fixtures / SAMPLE_PROJECTS import", () => {
    expect(discoverySource).not.toContain("SAMPLE_PROJECTS");
    expect(discoverySource).not.toContain("dev-fixtures");
    expect(discoverySource).not.toContain("operator-home-fixtures");
  });
});

// ── 6. Production hosted runtime sources the list from Cloud discovery ──

describe("App.svelte hosted project list source", () => {
  it("sources the hosted project list from Cloud discovery", () => {
    expect(appSource).toContain("loadCloudProjects");
    expect(appSource).toContain("projects={hostedProjects}");
  });

  it("does not populate the hosted list from daemon-only discovery or fixtures", () => {
    expect(appSource).not.toContain("fetchDaemonProjects");
    expect(appSource).not.toContain("fetchHostedDashboardAdapter");
    expect(appSource).not.toContain("SAMPLE_PROJECTS");
  });
});
