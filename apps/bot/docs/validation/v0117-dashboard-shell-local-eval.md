# Local eval: v0.1.17+ dashboard shell (bot#1025 → #1035)

From repo root:

```bash
cd dashboard && npm install && npm run dev
```

- **Production hosted App:** http://127.0.0.1:5173/  
  (connect-device, products, connections — real `App` → `HostedDashboard`)
- Optional isolated create UX experiment: http://127.0.0.1:5173/?eval=create-product-ux

**Retired:** `?eval=spa-mock` (removed after production promote in #1035).

## Browser data plane / CORS

SPA→daemon paths are inventory-gated (`BROWSER_DATA_PLANE_PREFIXES` in
`dashboard/src/lib/browser-data-plane.ts` and
`crates/bicameral-gateway/src/browser_data_plane.rs`). Gateway OPTIONS tests
and SPA path-scan tests fail CI if a new `fetchFromHostedDaemon` path sits
outside that list or lacks trusted-origin CORS.

Local Vite smoke:

```bash
export BICAMERAL_DEV_TRUST_VITE=1   # trust http://127.0.0.1:5173 for pair
"$BIC" gateway start
```

Prefer verifying post-pair MCP/connections with **direct** daemon origin +
`Origin: https://app.bicameral-ai.com` (or Vite with the env flag) — the Vite
proxy can mask CORS gaps.
