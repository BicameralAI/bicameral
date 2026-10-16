/**
 * Browser → local daemon data plane inventory.
 *
 * Rule: any path the hosted SPA calls on the loopback daemon (via
 * `fetchFromHostedDaemon` / `HOSTED_GATEWAY_BASE` / `hostedDaemonDataBase`)
 * MUST be under one of these prefixes and MUST receive trusted-origin CORS
 * on the gateway (`browser_data_plane` in bicameral-gateway).
 *
 * Keep prefixes in lockstep with:
 *   crates/bicameral-gateway/src/browser_data_plane.rs
 *     BROWSER_DATA_PLANE_PREFIXES
 *
 * Prefer nesting new surfaces under `/api/v1/hosted-bridge/` so CORS is
 * automatic. If you must use another prefix, add it here **and** on the
 * gateway, mount it under `browser_data_plane_routes()`, and extend probes.
 */

/** Path prefixes allowed for browser→daemon calls (prod + local). */
export const BROWSER_DATA_PLANE_PREFIXES = [
  "/api/v1/hosted-bridge/",
  "/api/v1/setup/",
  "/api/v1/connections",
] as const;

/** Paths that must never be called cross-origin from the hosted SPA. */
export const AUTHORITY_PLANE_PREFIXES = [
  "/api/v1/ingest",
  "/api/v1/dashboard/",
  "/health",
] as const;

export function isBrowserDataPlanePath(path: string): boolean {
  const bare = path.split("?")[0] ?? path;
  return BROWSER_DATA_PLANE_PREFIXES.some((prefix) => {
    if (prefix.endsWith("/")) {
      return bare.startsWith(prefix) || bare === prefix.slice(0, -1);
    }
    return bare === prefix || bare.startsWith(`${prefix}/`);
  });
}

/**
 * Extract absolute daemon API paths from SPA source that target the **hosted**
 * gateway (cross-origin data plane), not same-origin `BASE` local dashboard
 * and not Cloud API `base` variables.
 */
export function extractDaemonApiPathsFromSource(source: string): string[] {
  const paths = new Set<string>();
  // Explicit daemon bases only — do not match generic `${base}` (Cloud).
  const re =
    /\$\{(?:HOSTED_GATEWAY_BASE|hostedDaemonDataBase\(\)|hostedGatewayBase\(\))\}(\/api\/v1\/[A-Za-z0-9_./-]+)/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(source)) !== null) {
    const raw = (m[1] ?? "").split("?")[0] ?? "";
    if (!raw || raw.includes("${")) continue;
    paths.add(raw);
  }
  // mcp-readiness.ts: `const base = hostedDaemonDataBase();` then `${base}/api/...`
  const mcpBaseRe = /hostedDaemonDataBase\(\)[\s\S]{0,200}?\$\{base\}(\/api\/v1\/[A-Za-z0-9_./-]+)/g;
  while ((m = mcpBaseRe.exec(source)) !== null) {
    const raw = (m[1] ?? "").split("?")[0] ?? "";
    if (raw) paths.add(raw);
  }
  return [...paths].sort();
}
