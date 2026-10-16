# TODO — Fix daemon ↔ cloud port contract divergence

## Problem

Bicameral cloud fails to reach the local daemon:

> Ensure the daemon is running at `http://localhost:19520` with hosted bridge enabled. Error: Failed to fetch

But the daemon actually starts on `127.0.0.1:37373`:

> Starting Bicameral daemon/gateway at 127.0.0.1:37373

### Root cause

Two divergent hardcoded "contract" ports:

| Side | Port | Source |
|------|------|--------|
| Local daemon (canonical) | **37373** | `crates/bicameral-config/src/schema.rs:194` (`default_port`), asserted in `crates/bicameral-gateway/src/lib.rs:502`; also `protocol/README.md:79` declares it "the single source of truth" |
| MCP thin client, dashboard, docs | 37373 | `dashboard/vite.config.ts:12-13`, `dashboard/src/pages/Settings.svelte:25,34`, `protocol/README.md:74` |
| **Bicameral cloud frontend** | **19520** | hardcoded in the cloud repo (NOT in `bicameral-bot`; deployed separately) |

The hosted bridge routes are always mounted (`crates/bicameral-gateway/src/lib.rs:102`), so "bridge enabled" is not the issue — the only defect is the port constant.

Secondary issue reported: the cloud page gives **no instruction** to bring the daemon up (no mention of `gateway start` / `onboard`).

## Decision

Keep **37373** as canonical (it is already the documented single source of truth with tests, docs, dashboard, and MCP client aligned — least churn). Fix the **cloud** to match, and add a user override as a fallback (the "openclaw-style" escape hatch) — not as the primary mechanism.

## Tasks

### A. Cloud frontend — `BicameralAI/bicameral-cloud` ✅ DONE (PR #53)
Source: `src/bicameral_cloud/routes/dashboard.py` (server-rendered f-string page).
- [x] Replaced hardcoded `19520` with canonical `37373`.
- [x] Port injected server-side via `BICAMERAL_DAEMON_PORT` env (deploy-time override), default `37373`.
- [x] Added per-user `localStorage` override (`bicameral_daemon_url`) as the collision fallback.
- [x] Interpolated the port into the repair-hint copy. (Onboarding copy already instructed `bicameral onboard --install-daemon`.)
- [x] Contract-guard tests in `tests/test_dashboard.py` (uses 37373, never 19520, exposes override). 19 passed.
- [x] gh merge: branch `fix/daemon-port-contract-37373` → PR https://github.com/BicameralAI/bicameral-cloud/pull/53.

### B. bicameral-bot (this repo) — prevent future drift ✅ DONE
- [x] Named shared constant `bicameral_config::DEFAULT_GATEWAY_PORT` (schema.rs), re-exported from the crate root; `default_port()` now returns it — single source of truth in code.
- [x] Cross-artifact contract test `crates/bicameral-config/tests/port_contract.rs`: asserts `protocol/README.md` documents the same port as the code constant and contains no stale `19520`. This is what would have caught the original divergence. 3 passed.
- [x] gh merge: branch off `main` → PR to `BicameralAI/bicameral-bot` → squash-merge.

**Deferred (intentionally):** exposing the bound port on `/health` or the v2 capability report. Low value — a client has already connected to the daemon on its real port before it can call either endpoint, so the field aids no discovery; and `AppState` (~25 fields, many construction sites) would need a bound-port thread-through to report the *actual* (vs default) port without being misleading. Revisit only if a concrete discovery consumer needs it.

### C. Interim unblock (do now, local only — do NOT commit)
- [ ] Either set `gateway.port: 19520` in the workspace `.bicameral/config.yaml` and restart the daemon, OR point the cloud override at `37373`. Remove once A ships.

## Validation
- [ ] Fresh machine: `bicameral onboard`, open cloud page, daemon reachable with zero manual port config.
- [ ] Port collision case: 37373 busy → user sets override on both sides → still connects.
- [ ] Contract test (Task B) red when ports diverge, green when aligned.
