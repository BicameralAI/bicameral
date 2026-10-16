/**
 * CORS / browser data-plane drift guards.
 *
 * Fails when the SPA calls a daemon path outside the inventory, or when the
 * inventory drifts from the gateway Rust list (documented lockstep).
 */
import { describe, expect, it } from "vitest";
import apiSource from "./lib/api.ts?raw";
import mcpSource from "./lib/mcp-readiness.ts?raw";
import clientSource from "./lib/hosted-bridge-client.ts?raw";
import {
  AUTHORITY_PLANE_PREFIXES,
  BROWSER_DATA_PLANE_PREFIXES,
  extractDaemonApiPathsFromSource,
  isBrowserDataPlanePath,
} from "./lib/browser-data-plane";

/** Must match crates/bicameral-gateway/src/browser_data_plane.rs */
const GATEWAY_PREFIXES_LOCKSTEP = [
  "/api/v1/hosted-bridge/",
  "/api/v1/setup/",
  "/api/v1/connections",
] as const;

describe("browser data plane inventory", () => {
  it("stays in lockstep with gateway BROWSER_DATA_PLANE_PREFIXES", () => {
    expect([...BROWSER_DATA_PLANE_PREFIXES]).toEqual([...GATEWAY_PREFIXES_LOCKSTEP]);
  });

  it("classifies bridge, setup, and connections as data plane", () => {
    expect(isBrowserDataPlanePath("/api/v1/hosted-bridge/pair")).toBe(true);
    expect(isBrowserDataPlanePath("/api/v1/setup/capability-readiness")).toBe(
      true,
    );
    expect(isBrowserDataPlanePath("/api/v1/connections")).toBe(true);
    expect(isBrowserDataPlanePath("/api/v1/connections/google/start")).toBe(
      true,
    );
  });

  it("excludes authority plane", () => {
    for (const p of AUTHORITY_PLANE_PREFIXES) {
      expect(isBrowserDataPlanePath(p)).toBe(false);
    }
  });
});

describe("SPA→daemon paths stay on the browser data plane", () => {
  it("every extracted hosted-daemon path is under the CORS inventory", () => {
    // api + mcp-readiness only: hosted-bridge-client uses api helpers, not raw paths.
    const sources = [apiSource, mcpSource].join("\n");
    const paths = extractDaemonApiPathsFromSource(sources);
    expect(paths.length).toBeGreaterThan(0);
    // mcp-readiness uses `${base}/...` where base = hostedDaemonDataBase()
    expect(paths.some((p) => p.includes("/setup/"))).toBe(true);
    expect(paths.some((p) => p.includes("/hosted-bridge/"))).toBe(true);
    const offenders = paths.filter((p) => !isBrowserDataPlanePath(p));
    expect(
      offenders,
      `SPA calls daemon paths outside browser data plane (add CORS inventory + gateway mount):\n${offenders.join("\n")}`,
    ).toEqual([]);
  });

  it("does not call authority-plane prefixes via hosted gateway templates", () => {
    const sources = [apiSource, mcpSource].join("\n");
    const paths = extractDaemonApiPathsFromSource(sources);
    for (const auth of AUTHORITY_PLANE_PREFIXES) {
      const hit = paths.filter(
        (p) => p === auth || p.startsWith(auth.endsWith("/") ? auth : `${auth}/`),
      );
      expect(hit, `authority path leaked into SPA daemon client: ${auth}`).toEqual(
        [],
      );
    }
  });

  it("routes all HOSTED_GATEWAY_BASE traffic through fetchFromHostedDaemon", () => {
    expect(apiSource).not.toMatch(/await fetch\(\s*`\$\{HOSTED_GATEWAY_BASE\}/);
    expect(apiSource).not.toMatch(/fetchFromHostedDaemon\(\s*`\$\{BASE\}/);
    const n = apiSource.match(/fetchFromHostedDaemon\(/g)?.length ?? 0;
    expect(n).toBeGreaterThanOrEqual(10);
  });

  it("documents client module for completeness", () => {
    expect(clientSource.length).toBeGreaterThan(0);
  });
});
