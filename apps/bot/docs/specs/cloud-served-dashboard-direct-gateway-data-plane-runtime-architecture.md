# Runtime Architecture: Cloud-Served Dashboard Direct-Gateway Data Plane

- **Date:** 2026-07-06
- **Status:** draft
- **Level:** L2 (cross-repo contract: `bicameral-cloud` ↔ browser ↔ `bicameral-bot` gateway)
- **Owns:** The runtime contract by which a browser, running the dashboard SPA that was **served by Hosted cloud**, pulls **redacted derived read projections directly from the paired Local daemon** — with Hosted cloud never in the data path.
- **Related context:**
  - Prior art (contract already decided): `docs/specs/cloud-hosted-dashboard-local-daemon-runtime-architecture.md`, `docs/specs/cloud-hosted-dashboard-parity-feature-spec.md`, `docs/specs/hosted-bridge-user-level-pairing.md`, `docs/specs/alpha-hosted-dashboard-ui.md`.
  - ADRs: 0007 (substrate-neutral governance), 0020 (gateway authority boundary), 0025 (hosted-tier trust boundary, **accepted**), 0022, 0018.
  - Shipped surfaces: `crates/bicameral-gateway/src/routes.rs` (`hosted_bridge_routes`), `crates/bicameral-gateway/src/hosted_bridge_cors.rs`, `crates/bicameral-api/src/hosted_bridge.rs`, `bicameral-cloud/src/bicameral_cloud/app.py` (SPA static serving, `/app`), `bicameral-cloud/src/bicameral_cloud/routes/dashboard.py` (inline auth+pairing `/dashboard`), `dashboard/src/lib/api.ts`, `dashboard/src/App.svelte`.
- **Governance gate:** `bic:spec-governance-gate` advisory/no-op; Qor integration disabled pending factory policy revision. Authority-sensitive surfaces touched: hosted/local boundary, protocol, projection read model, privacy/redaction, cross-repo contract. Open authority questions are recorded in **Open Questions**; State Refinement + Minimization are recorded below.

---

## Reading Guide

Read `cloud-hosted-dashboard-local-daemon-runtime-architecture.md` first — it defines `DashboardProjectionEnvelope`, the `hosted_dashboard_*` session states, and the typed-error catalog this spec reuses verbatim. Then read `hosted-bridge-user-level-pairing.md` for the three-level state split and the rule that the bridge is a **route family on the bot gateway, not a separate authority surface**.

**Main runtime invariant:** *The hosted dashboard's data plane is browser ↔ Local daemon, direct. Hosted cloud serves only the static client bundle and coordinates hosted identity; it never observes Confidential data and is never a data relay. The browser renders only **redacted, derived read projections** pulled from the paired daemon over an origin-allowlisted, session-token-gated, CORS+PNA bridge; it never treats a projection as authority and never routes a canonical mutation through cloud.*

---

## Runtime Problem

The dashboard SPA (`bicameral-bot/dashboard`) is now served by Hosted cloud at `/app` (static bundle, shipped). But its data client (`dashboard/src/lib/api.ts`) issues every request against `BASE = ""` — i.e. **same-origin to Hosted cloud** — and in hosted context it renders the **local-mode** pages (`LedgerView`, `IngestionGate`) that call `/api/v1/dashboard/ledger` and `/api/v1/dashboard/ingestion-gate`. Two failure modes result:

1. Those `/api/v1/dashboard/*` endpoints do not exist on Hosted cloud (Cloud Run) → `ledger: 404`.
2. Even if they existed, Hosted cloud **cannot reach the daemon**: the daemon binds `127.0.0.1:37373` on the Product Member's machine, unreachable from Cloud Run. And routing local ledger data through cloud would violate ADR-0025 (Hosted cloud holds no Confidential data).

The live-system ambiguity this spec resolves: **who the browser talks to for data, and under what contract.** The daemon already exposes the correct surface (`/api/v1/hosted-bridge/*`, CORS+PNA, session-token-gated, redacted) and the projection-envelope contract is already decided — but nothing owns (a) the **browser→localhost direct-pull topology** as a runtime decision, or (b) the **client-side wiring** that makes the SPA use it. The current client guesses wrong (same-origin), so the hosted dashboard shows either a bare pairing panel or a 404 shell.

---

## Goals And Non-Goals

### Goals

- G1. Define the browser↔daemon **direct** data plane: the served SPA pulls read projections from `http://localhost:37373/api/v1/hosted-bridge/*`, carrying the bridge session token, over CORS + Private Network Access.
- G2. Specify the **client-side runtime** the SPA needs: a hosted API client whose base is the paired daemon origin (not same-origin), token propagation, hosted-vs-local mode selection, and rendering only from redacted projections.
- G3. Specify the **pairing→data handoff**: how the session token and daemon origin reach the SPA's hosted data client from the auth+pairing step.
- G4. Keep Hosted cloud strictly a static-client host + identity coordinator, reaffirming ADR-0025.
- G5. Resolve ADR-0020's open read-route auth question for the network-origin case.

### Non-Goals

- N1. **No** hosted governed mutations in this release. `POST /hosted-bridge/command` remains present but the hosted UI keeps mutating controls disabled (per `alpha-hosted-dashboard-ui.md`) until local-policy confirmation UX + audit receipts + parity tests exist.
- N2. **No** server-side shared canonical store, no Hosted-cloud projection cache of payloads (ADR-0025 decision 6 deferred). Any browser-held projection is transient in-memory only.
- N3. **No** cloud↔daemon relay path for workspace data. Cloud is not a proxy.
- N4. **No** new tenancy vocabulary. No `Workspace`/`Team`/`Organization` entities (retired by ADR-0025).
- N5. **No** workspace binding flow (`workspace_binding_available = false` in alpha).
- N6. **No** widening of what the daemon exposes: this spec consumes the existing redacted projections, it does not add fields to them.

---

## Runtime Components

| Component | Responsibility | Process boundary | Authority role | Inputs | Outputs | Persistent state | Must NOT |
| --- | --- | --- | --- | --- | --- | --- | --- |
| **Hosted cloud shell** (`bicameral-cloud`) | Serve static SPA bundle; Supabase auth; design-partner allowlist; render inline pairing entry | Cloud Run (public HTTPS, `app.bicameral-ai.com`) | Identity coordinator only | HTTPS requests, Supabase session | SPA bundle, `HostedDashboardHandshake` seed (origin, cloud_account_id), auth verdict | Hosted account/session metadata only | Observe Confidential data; relay/cache ledger data; act as data authority |
| **Dashboard SPA** (`bicameral-bot/dashboard`, served by cloud) | Render hosted UI; pull redacted projections from daemon; hold session token in memory | Browser tab (origin = cloud) | Renderer / read client | Handshake seed, user actions | `GET /hosted-bridge/*` requests to daemon; rendered read models | In-memory session token + daemon origin (transient) | Treat projection as authority; call `/api/v1/dashboard/*` cross-origin; route mutations through cloud; persist projections |
| **Hosted bridge route family** (`bicameral-gateway`) | Origin allowlist, CORS+PNA, CSRF, pairing lifecycle, session-token gating, redacted projection reads | Local daemon process, bound `127.0.0.1:37373` | Read-projection producer; pairing authority | Pair request, token-bearing reads | `HostedBridgePairingResponse`, `DashboardProjectionEnvelope`-shaped payloads, typed errors | Pairing sessions (in-memory, expiring) | Accept cloud identity as local actor; append ledger events on a read; leak Confidential fields |
| **Local daemon core** (`bicameral-bot`) | Canonical governance; projection construction from event-store replay | Local daemon process | Canonical authority | Accepted events | `ProjectionState` read models | Decision Ledger (event store) | Serve unredacted local data to a network origin |

---

## Canonical vs Derived State

| Artifact | Classification | Authoritative writer | Allowed readers | Mutation path | Retention | Prohibited shortcut |
| --- | --- | --- | --- | --- | --- | --- |
| Decision Ledger events | Canonical state / accepted event | Local daemon (post policy-accept + EventStoreAdapter materialize) | Local daemon (replay) | Governed review command → policy → materialize → replay | Durable, local | Browser/cloud never writes; a read never appends |
| `ProjectionState` / `Projected*` | Derived projection / read model | Local daemon (replay build) | Local daemon; **redacted subset** to browser via bridge | Rebuild from replay | Rebuildable | Not authority; not persisted cloud-side |
| `DashboardProjectionEnvelope` payload (redacted ledger read, operator-home, projects) | Derived projection (wire form) | Bridge projection reader (daemon) | Paired browser only, token-gated | Read-only | Transient in browser memory | Cloud caching of payload (deferred RFQ; forbidden here) |
| Hosted bridge pairing session (`session_token`, origin, cloud_account_id, expires_at, revoked) | Local scratch / session state | Daemon (`hosted_bridge_sessions`) | Daemon guard (`validate_bridge_session`) | pair / revoke / expiry | In-memory, expiring (e.g. 30 min) | Not durable authority; not a Product membership record |
| Supabase session + design-partner verdict | Hosted account metadata | Hosted cloud / Supabase | Hosted cloud, browser | OAuth | Hosted-cloud retention policy | Never a local actor grant (must map through local policy) |
| Bridge session token held in browser | Cache (bearer credential) | Daemon (issued at pair) | SPA hosted client | Reissued at re-pair | In-memory only, cleared on revoke/expiry/tab close | Never written to localStorage/cloud/logs |

---

## State Refinement Check

- **Release state (smallest honest lifecycle):** a browser session moves `hosted_dashboard_unpaired → hosted_dashboard_paired → (reads redacted projections) → hosted_dashboard_revoked | hosted_dashboard_upgrade_required | expired`. This is the existing dashboard-session lifecycle from the prior-art runtime spec — **no new states are introduced.**
- **States added/split/merged/refined:** none added. This spec *refines the data-plane behavior* attached to the already-defined `hosted_dashboard_paired` state (in that state, data reads now target the daemon origin, not same-origin cloud). The three-level split (user/project/workspace) from `hosted-bridge-user-level-pairing.md` is reused unchanged.
- **Transition triggers / authority:**
  - `unpaired → pending`: daemon accepts `POST /hosted-bridge/pair` (origin + account + CSRF + protocol-version valid) → returns `202` with an opaque request handle, poll proof, browser-displayed pairing code, expiry, and capability/version info — **never** a `session_token`. Authority: daemon. Transient daemon-memory request only (see `hosted-bridge-user-level-pairing.md` #790/#865).
  - `pending → paired`: a local terminal matches the pasted code, displays the bound request, then requires a separate explicit default-deny Approve. Only that decision mints the session; the browser then retrieves the token through the approved status-poll handoff. Deny/expiry/incompatible yield typed terminal outcomes with no token. Authority: local operator.
  - `paired → upgrade_required`: `HostedDashboardHandshake` version intersection empty. Authority: daemon capability handshake.
  - `paired → revoked`: `POST /hosted-bridge/revoke` or session expiry. Authority: daemon.
- **Persistence:** pairing session in daemon memory (expiring); token in browser memory (transient); no cloud-side or on-disk persistence of session or projection payloads.
- **Invariants later refinements must preserve:** (i) projection reads never append events; (ii) cloud never holds Confidential data; (iii) token never leaves memory; (iv) mutable controls hidden without a compatible action capability.
- **Deferred vocabulary:** `cloud_projection_cache`, `hosted_workspace_binding`, `cloud_dashboard_canonical_project`, hosted-only read-model shapes, hosted governed-mutation confirmation states — deferred; their authority/persistence/recovery are undefined for this release.

## State Minimization Review

- **New states rejected:** A distinct `hosted_dashboard_data_connected` state was considered and **rejected** — "data reachable" is not a distinct authority/persistence/recovery state; it is `hosted_dashboard_paired` plus a per-request reachability result (rendered as a degraded/repair banner). Represent transient unreachability as an error result on the read, not a session state.
- **Collapsed into attributes:** "daemon reachable but a specific surface unsupported" → reuse existing `unsupported_surface` / `degraded_surfaces` metadata on the handshake, not a new state.
- **Deferred without lying:** hosted mutation lifecycle states — the release genuinely ships read-only, so no mutation states are needed now.
- **Rationale:** the smallest honest state machine is the already-shipped `hosted_dashboard_*` lifecycle; this spec changes *transport wiring within `paired`*, not the state set. Restraint here is deliberate.

---

## Authority Ownership Matrix

| Actor / component | observe | validate | project | render | mutate | revoke | notes |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Product Member (human) | ✓ (own data) | — | — | ✓ (views) | ✕ this release | ✓ (can revoke pairing) | authority is local, tied to OS user |
| Dashboard SPA (browser) | ✓ redacted projections | ✕ | ✕ | ✓ | ✕ (controls hidden) | requests revoke | holds token in memory only |
| Hosted cloud shell | ✕ (forbidden for workspace data) | ✕ | ✕ | serves client only | ✕ | ✕ | identity coordinator; no Confidential data |
| Hosted bridge routes (daemon) | ✓ | ✓ origin/CSRF/token | ✓ redacts | — | ✕ (read family) | ✓ (session) | producer of redacted projections |
| Local daemon core | ✓ | ✓ governance policy | ✓ builds `ProjectionState` | — | ✓ canonical (via review command path, not this data plane) | — | sole canonical authority |

---

## Runtime Scenarios

### S1. Startup / SPA load (hosted)

- **Trigger:** Member opens `app.bicameral-ai.com/dashboard`.
- **Components:** browser, Hosted cloud shell.
- **Flow:** cloud serves inline auth shell → Supabase auth + design-partner allowlist → seeds `HostedDashboardHandshake` (origin=`https://app.bicameral-ai.com`, cloud_account_id) → loads SPA (`/app`).
- **State:** `hosted_dashboard_unpaired`. **Failure:** unauthenticated/not-allowlisted → SPA not loaded; auth message shown.

### S2. Pairing / pasted-code local confirmation / token handoff

- **Trigger:** SPA in `unpaired` requests pairing.
- **Components:** browser, daemon bridge, foreground gateway terminal or standalone pairing CLI.
- **Messages:**
  - `POST http://localhost:37373/api/v1/hosted-bridge/pair` with headers `x-csrf-token`, body `{origin, cloud_account_id, csrf_token, client_protocol_version}`. Preceded by a CORS+PNA `OPTIONS` preflight answered by `hosted_bridge_cors_middleware` (`Access-Control-Allow-Private-Network: true`).
  - Daemon returns `202 {status: pending, request_handle, poll_proof, pairing_code, expires_at, protocol_version, capabilities, confirmation}` — **no session, no token**. It retains only the normalized code digest.
  - Browser displays `pairing_code`. The local operator pastes it into the foreground gateway terminal or `bicameral pairing approve`; the terminal performs a local-only, rate-limited lookup and shows requesting origin/account plus allowed/forbidden capabilities.
  - A separate default-deny terminal prompt POSTs `/hosted-bridge/pair/decision` (local only) to Approve or Deny. Code matching alone never changes state or mints a session.
  - Browser polls `GET /hosted-bridge/pair/status?request_handle&poll_proof` (must match the original origin + account). While pending it gets `pending`; on local approval it gets `approved` with the transient `session_token`.
- **State reads/writes:** daemon writes a transient `PendingPairingRecord`; on local approval it writes a `HostedBridgePairingSession` and links it to the record. The token is released only in the approved status-poll response body.
- **Handoff:** SPA holds `session_token` + daemon origin **in memory** (never localStorage/URL/logs/analytics/error reporting). Repeated valid polls return the same token while the session is live; a replayed approval never re-mints. Authority boundary crossed: cloud identity → local pairing (mapped by daemon policy after explicit local approval, never accepted as a local actor).
- **Result:** `hosted_dashboard_paired` (only after local Approve + handoff). **Failure:** origin not allowlisted → no CORS headers → browser blocks; typed body error (`origin_rejected`). Protocol-incompatible client/daemon → typed `update_required` (`426`), never a silent session. Denied/expired/mismatched/replayed decision or poll → typed terminal outcome with no token. Daemon unreachable → `daemon_unavailable` repair state naming the failing layer.

The governed [Figma Hosted Access & Pairing journey](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=19-144) shows the complete dashboard code → hidden local terminal entry → explicit default-deny approval → transient `/app` handoff. Cloud issue #184 owns executable browser acceptance for that journey; this runtime spec's contract coverage is supporting evidence.

### S3. Projection read flow (the core of this spec)

- **Trigger:** SPA renders a hosted surface (operator home, projects, ledger).
- **Components:** browser (hosted client), daemon bridge + projection reader.
- **Messages:** `GET http://localhost:37373/api/v1/hosted-bridge/{operator-home|projects|projection/ledger}` with header `x-bridge-session-token`.
- **Guard:** `validate_bridge_session` (token present, non-expired, non-revoked) runs before the reader.
- **State reads/writes:** daemon replays/reads `ProjectionState`, **redacts**, returns `DashboardProjectionEnvelope`-shaped payload (`surface_id`, `read_model_revision`, `freshness`, `payload`, `redactions`, `caveats`). No event append.
- **Authority boundary crossed:** derived read only.
- **Result:** browser renders the redacted read model. **Failure:** `401 missing/invalid_bridge_session` → back to pairing; `403 revoked/expired` → re-pair; `projection_schema_unsupported` → degraded banner.

### S4. Failure / recovery (daemon down, token expired)

- Token expiry (e.g. 30 min) → next read returns `403 PairingExpired` → SPA drops to `unpaired`, prompts re-pair, clears in-memory token.
- Daemon stopped → fetch/network error or blocked preflight → SPA shows repair state naming the failing layer (per alpha UI spec), offers retry; no cloud fallback.

### S5. FORBIDDEN flow (must be impossible / rejected)

1. **Mutation via cloud:** SPA must never send a canonical review command to Hosted cloud, and cloud must never forward one to a daemon. Mutations, when later enabled, go browser→daemon `POST /hosted-bridge/command` under local policy.
2. **Cross-origin local read:** SPA must never call `/api/v1/dashboard/ledger` (the non-CORS local route) from the cloud origin to fetch **unredacted** local data. That route is loopback/local-surface only; the network-origin browser uses the redacted `hosted-bridge` projection.
3. **Cloud data relay/cache:** Hosted cloud must never proxy, store, or log a `DashboardProjectionEnvelope` payload.
4. **Token persistence:** the bridge session token must never be written to `localStorage`, cookies, cloud, or logs.

Each forbidden flow has a negative acceptance criterion (below).

---

## Event, Command, and Protocol Contracts

This spec **adds no new wire contracts**; it consumes and wires existing ones. Documented for the implementing agent:

| Route (on daemon `127.0.0.1:37373`) | Method | Caller | Handler | Payload / headers | Validation | Idempotency | Notes |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `/api/v1/hosted-bridge/pair` | POST | SPA | `hosted_bridge_pair_request` | `x-csrf-token`; `{origin, cloud_account_id, csrf_token, client_protocol_version}` | origin allowlist, account, CSRF double-submit, smuggled-field detection, protocol compatibility | new pending request per call | returns `202 {status: pending, request_handle, poll_proof, pairing_code, expires_at, protocol_version, capabilities, confirmation}` — **never** a `session_token`; incompatible → typed `update_required` (`426`) |
| `/api/v1/hosted-bridge/pair/pending` | GET | terminal/CLI | none (local only) | local-origin check | safe | returns pending origin/account/expiry summaries; omits code, proof, and nonce |
| `/api/v1/hosted-bridge/pair/code/lookup` | POST | terminal/CLI | `{pairing_code}` (local only) | local-origin check, normalization, rate limit, constant-time digest match, not-expired | safe; no state transition | returns requesting origin/account, allowed/forbidden capabilities, expiry, and decision nonce; **no token** |
| `/api/v1/hosted-bridge/pair/decision` | POST | terminal/CLI | `{request_handle, action, local_confirmation_nonce}` (local only) | local-origin check, handle + constant-time nonce match, not-expired | approve mints once; deny/replay never re-mint | Approve mints a `HostedBridgePairingSession`; returns typed approved/denied/expired **without** a token |
| `/api/v1/hosted-bridge/pair/status` | GET | SPA | `hosted_bridge_pair_status` | `?request_handle&poll_proof`; original `Origin` + `cloud_account_id` | poll-proof (constant-time), origin match, account match | retry-safe | pending/denied/expired/update_required → **no token**; approved + live session → transient `session_token` handoff (same token on repeat while session live) |
| `/api/v1/hosted-bridge/status` | GET | SPA | `hosted_bridge_status` | `x-bridge-session-token` | `validate_bridge_session` | safe | post-pairing session/handshake status |
| `/api/v1/hosted-bridge/operator-home` | GET | SPA | `hosted_bridge_operator_home` | token | guard | safe | redacted operator-home projection |
| `/api/v1/hosted-bridge/projects` | GET | SPA | `hosted_bridge_projects` | token | guard | safe | redacted project list |
| `/api/v1/hosted-bridge/projection/ledger` | GET | SPA | `hosted_bridge_projection_ledger` | token | guard | safe | **redacted ledger read projection** (real decision/candidate read model, sensitive fields stripped) |
| `/api/v1/hosted-bridge/dashboard-adapter` | GET | SPA | `hosted_bridge_dashboard_adapter` | token | guard | safe | summary aggregate (counts + project metadata + fixture fallback) |
| `/api/v1/hosted-bridge/workspace-discovery` | GET | SPA | `hosted_bridge_workspace_discovery` | token | guard | safe | safe workspace metadata |
| `/api/v1/hosted-bridge/command` | POST | SPA (deferred UI) | `hosted_bridge_command` | token + CSRF | guard + local policy | per command | **not surfaced this release** |
| `/api/v1/hosted-bridge/revoke` | POST | SPA | `hosted_bridge_revoke` | token | guard | idempotent | ends session |

**CORS/PNA preflight contract** (`hosted_bridge_cors.rs`): `OPTIONS` on any `/hosted-bridge/*` from a trusted origin → `204` with `Access-Control-Allow-Origin: <origin>`, `Allow-Methods: GET, POST, OPTIONS`, `Allow-Headers: content-type, x-bridge-session-token, x-csrf-token`, `Allow-Credentials: true`, and `Access-Control-Allow-Private-Network: true` when the request carried `Access-Control-Request-Private-Network: true`. Untrusted origins receive no CORS headers (browser blocks). Scope: hosted-bridge routes only; `/api/v1/dashboard/*` deliberately gets no CORS.

**Terminology guard:** in this document "projection" always means a **derived read model / `DashboardProjectionEnvelope`**. It is **not** `EgressProjection` (governed outbound to external tools, `governed-egress-projection-ownership.md`). Do not conflate.

---

## Substrate Capability Matrix

| Capability | Hosted cloud | Browser (SPA) | Local daemon bridge |
| --- | --- | --- | --- |
| Persist canonical state | ✕ | ✕ | ✓ (event store) |
| Persist evidence | ✕ | ✕ | ✓ |
| Emit accepted events | ✕ | ✕ | ✓ (via policy) |
| Host projections / read models | ✕ | render only, transient | ✓ (build + redact) |
| Enforce policy | ✕ | ✕ | ✓ |
| Provide identity/permission | hosted identity only | — | local actor mapping |
| Provide audit receipts | non-canonical metadata only | ✕ | ✓ (operational receipts) |
| Offline / local-first | ✕ | needs daemon | ✓ |
| Team-wide sync | ✕ (no shared store this release) | ✕ | ✕ (daemon-per-member) |

Free/local vs hosted: all **workspace data capability is local**; Hosted cloud's only capabilities are static-client delivery + hosted-identity coordination (paid/hosted tier), consistent with ADR-0025.

---

## Deployment and Runtime Topology

```
 Browser tab (origin https://app.bicameral-ai.com)
   │  1. GET /dashboard, /app  ── static bundle + Supabase auth
   ▼
 Hosted cloud (Cloud Run, public HTTPS)      ← never sees workspace data
   │  (no data path back down)
   │
   │  2. OPTIONS+POST /hosted-bridge/pair      (CORS + PNA, CSRF)
   │  3. GET /hosted-bridge/{projection/ledger,operator-home,projects}
   │      header: x-bridge-session-token       (CORS + PNA, token-gated, redacted)
   ▼
 http://127.0.0.1:37373  Local daemon (per Product Member, OS-user trust boundary)
```

- **Processes / startup order:** daemon runs locally (`bicameral gateway start`); cloud is always-on; browser loads client from cloud then connects to daemon.
- **Ports / origins (shared contract):** daemon `127.0.0.1:37373` (named constant; keep in sync with the daemon default — see `daemon-cloud-port-contract-fix.md`); cloud origin `https://app.bicameral-ai.com` (in daemon origin allowlist). Per-machine override via existing `bicameral_daemon_url` localStorage key.
- **Credentials:** Supabase session (cloud); bridge `session_token` (browser memory).
- **Local-only state:** ledger, projections, secrets, pairing sessions. **Hosted state:** account/session/allowlist metadata only.
- **Migration path:** a future server-side shared store (ADR-0025 decision 6, deferred) would change this topology and requires its own ADR.

---

## Error, Retry, and Recovery Behavior

| Condition | Behavior |
| --- | --- |
| Missing/invalid token | `401` typed body → SPA returns to `unpaired`, re-pair |
| Revoked / expired session | `403 PairingRevoked/PairingExpired` → clear in-memory token, re-pair |
| Daemon unreachable / preflight blocked | network error → repair state naming the failing layer; retry button; **no cloud fallback** |
| Origin not allowlisted | no CORS headers → browser blocks; `origin_rejected` typed body for diagnostics |
| Projection schema unsupported | `projection_schema_unsupported` → degraded banner; read-only, no mutable controls |
| Handshake version intersection empty | `hosted_dashboard_upgrade_required` → prompt daemon upgrade; hide controls |
| Stale projection | render with `freshness`/`caveats` from envelope; never silently present stale as fresh |
| Duplicate/replayed pair | CSRF + replay protection rejects; existing session unaffected |

---

## Observability, Audit, and Receipts

- Daemon emits **operational receipts** (non-canonical) for pair/read/revoke, with **redacted** logs (no secrets, paths, source bodies, raw event bodies).
- Browser: console diagnostics only; no PII/token logging.
- Hosted cloud: may retain non-secret pairing/session **metadata** receipts; never projection payloads.
- Distinguish observability from authority: a read receipt records that a projection was served; it does not canonize anything.

---

## Security, Privacy, and Authority Boundaries

- **Observe:** browser observes only redacted projections; cloud observes only hosted identity metadata; daemon observes local canonical state.
- **Decide/mutate:** only the daemon, only via the governed review-command path — **not** this read data plane.
- **Must remain local:** ledger events, provider secrets, absolute paths, source bodies/private code, raw event-store bodies, code indexes.
- **May be relayed externally:** nothing on the data plane. Only the static bundle and hosted identity flow through cloud.
- **Authoritative-for-claim:** daemon for all workspace/Decision claims; cloud for hosted account/session/allowlist only.
- **Mandatory redaction (payloads AND logs):** enforced daemon-side; the browser must not attempt to reconstruct excluded fields from multiple projections.
- **Read-route auth decision (resolves ADR-0020 open item):** network-origin projection reads are **not** unauthenticated — they require origin allowlist + valid bridge `session_token`. Loopback-only local-surface routes (`/api/v1/dashboard/*`) keep their existing posture and are not exposed cross-origin.

---

## Implementation Sequencing

| Slice | Target | Files likely to change | Authority section | Acceptance | Validation | Deps | Task class |
| --- | --- | --- | --- | --- | --- | --- | --- |
| **T1. Hosted API client base + token** | `bicameral-bot/dashboard` | `dashboard/src/lib/api.ts` | S3, Contracts | Hosted-bridge calls target daemon origin (default `http://localhost:37373`, `bicameral_daemon_url` override) with `x-bridge-session-token`; local calls keep `BASE=""` | unit test: hosted client builds daemon-origin URLs + header; local client unchanged | none | AFK_REVIEW_ONLY |
| **T2. Pairing→data handoff** | `bicameral-bot/dashboard` (+ `bicameral-cloud` inline `/dashboard` if handoff via redirect) | `dashboard/src/App.svelte`, `dashboard/src/pages/HostedDashboard.svelte`, `routes/dashboard.py` | S2 | On pair success, SPA holds `session_token`+daemon origin in memory; hosted mode entered with a real bridge state (not fixture) | component test: paired state carries token; token never in localStorage | T1 | AFK_REVIEW_ONLY |
| **T3. Hosted read rendering (operator home + projects)** | `bicameral-bot/dashboard` | `HostedDashboard.svelte`, `OperatorHome.svelte`, `api.ts` (`fetchHostedBridgeStatus`, `dashboard-adapter`/`operator-home`/`projects`) | S3 | Operator home + project list render from real redacted projections; fixture fallback handled | integration test against daemon fixture; manual dogfood | T1,T2 | AFK_REVIEW_ONLY |
| **T4. Hosted redacted ledger view** | `bicameral-bot/dashboard` | new hosted ledger view or hosted-mode branch of `LedgerView.svelte` reading `projection/ledger` | S3, Privacy | Decisions view renders redacted ledger read projection (no 404, no unredacted fields); mutable controls hidden | integration test: envelope→render; negative test: no `/api/v1/dashboard/ledger` call from cloud origin | T1,T3 | HITL_BLOCKED (privacy sign-off on rendered fields) |
| **T5. Mode selection + repair states** | `bicameral-bot/dashboard` | `App.svelte`, repair components | S1,S4 | Hosted (served-by-cloud) always uses hosted client; repair states name failing layer; expiry/revoke → re-pair | component tests for each error→state mapping | T1–T4 | AFK_REVIEW_ONLY |
| **T6. Rebuild + vendor + deploy** | `bicameral-cloud` | `scripts/vendor_dashboard.sh`, vendored bundle | Topology | Cloud serves the rewired bundle | `/app` serves new bundle; smoke test | T1–T5 | AFK_AUTO_MERGE |

Tracer bullet: **T1 + T3 (operator-home)** is the smallest end-to-end proof that real daemon data reaches the cloud-served browser.

---

## Issue Generation Notes (for `bic:to-issues`)

- **Hard deps:** T1 → T2 → T3 → T4; T6 last.
- **Soft deps / parallelizable:** repair-state copy (T5) can start alongside T3.
- **Fixture/stub opportunities:** daemon already ships `dashboard_adapter_falls_back_to_fixtures_when_unpaired` and E2E conformance tests (`crates/bicameral-gateway/tests/hosted_bridge_e2e_conformance.rs`) — reuse as client-side integration fixtures.
- **HITL blockers:** T4 (which redacted ledger fields render on a network origin) needs privacy sign-off; hosted mutations remain out of scope (N1).
- **Validation commands:** `npm test` (dashboard vitest), daemon `cargo test -p bicameral-gateway hosted_bridge`, manual dogfood via `/dashboard`→pair→render.
- **Authority source per issue:** cite S2 (pairing), S3 (reads), S5 (forbidden flows), Security section (redaction).

---

## ADR Implications

| ADR | Verdict | Rationale |
| --- | --- | --- |
| **0007** substrate-neutral governance | **Reaffirmed** | Browser reads a *derived projection* = the "caches/materializations, not authority" pattern; no new canonical write path. |
| **0020** gateway authority boundary | **Amended (primary)** | 0020 left read-only projection routes optionally unauthenticated "unless a later product decision requires stricter local privacy." This spec is that decision: network-origin reads require origin allowlist + session-token gating; loopback local routes keep their posture. Recommend amending 0020. |
| **0025** hosted-tier trust boundary (accepted) | **Reaffirmed on invariant; amend transit topology** | Data-locality/coordination-only invariant is reaffirmed and *strengthened* (cloud fully out of data path). But 0025 describes transit as "cloud ↔ daemon bridge"; the direct **browser ↔ daemon over CORS+PNA** path changes the in-scope TLS/transit control description — recommend a targeted amendment. |
| **0018** clean-projection external ingest | **Unaffected** | Inbound ingest contract; this read path is neither ingest nor ToolRequest. |
| **0022** V1 signoff + reviewable context | **Unaffected** | Defines what the ledger read *contains*, not how it is transported. |
| **New ADR — recommended** | **Missing** | No ADR owns the **browser→localhost direct-pull topology** (CORS + Private Network Access + origin-allowlisted, session-token-gated, redacted reads). This is hard to reverse and surprising without context → meets the ADR threshold. |

---

## Acceptance Criteria

**Positive:**

1. Loading `app.bicameral-ai.com/dashboard`, authenticating, and pairing yields a `hosted_dashboard_paired` SPA that renders operator home + project list from **real** redacted daemon projections.
2. The Decisions view renders the **redacted ledger read projection** from `/hosted-bridge/projection/ledger` — no `ledger: 404`.
3. All hosted data requests go to the **daemon origin** with `x-bridge-session-token`; CORS+PNA preflight succeeds from the cloud origin.
4. Daemon down / token expired produces a repair state that names the failing layer and offers re-pair.

**Negative (forbidden paths):**

5. No request to `/api/v1/dashboard/ledger` (or other `/api/v1/dashboard/*`) is issued from the cloud origin.
6. No canonical mutation is routed through Hosted cloud; mutable controls are absent in hosted mode this release.
7. The bridge `session_token` never appears in `localStorage`, cookies, network requests to cloud, or logs.
8. Hosted cloud never stores or logs a projection payload.
9. Rendered ledger data contains no excluded fields (secrets, absolute paths, source bodies, raw event bodies).

---

## Validation Plan

- **Client unit tests:** hosted vs local API base selection; token header presence; token never persisted (assert no `localStorage` writes).
- **Client integration:** render from captured `DashboardProjectionEnvelope` fixtures (reuse daemon conformance fixtures); error→state mapping.
- **Daemon contract tests (existing):** `hosted_bridge_e2e_conformance.rs` — pair→read, unpaired/invalid/revoked/expired cannot read, `*_does_not_leak_sensitive_fields`, CORS/PNA preflight tests in `hosted_bridge_cors.rs`.
- **Negative tests:** static assert the SPA hosted path issues no `/api/v1/dashboard/*` calls; grep bundle/logs for token leakage.
- **Manual dogfood:** `/dashboard` → pair → operator home + Decisions render; stop daemon → repair state.
- **Validation gaps:** cross-browser PNA behavior (Chrome ships PNA; Safari/Firefox differ) — needs a manual matrix; whether any browser requires an HTTPS loopback rather than `http://localhost` (currently `http://localhost` is a secure context exempt from mixed-content, but confirm on the PNA matrix).

---

## Open Questions

1. **Projection caching (RFQ, prior art open):** confirm browser holds transient in-memory only and Hosted cloud caches nothing. *Consequence if unresolved:* risk of accidental cloud-side payload retention violating ADR-0025.
2. **Read-route auth strictness (resolved here, needs ADR record):** origin-allowlist + token gating for network-origin reads. *Consequence:* without the ADR amendment, 0020 still reads as "reads may be unauthenticated," contradicting this spec.
3. **PNA/CORS browser matrix (new surface):** which browsers honor `Access-Control-Allow-Private-Network` and under what conditions. *Consequence:* some Members' browsers may block the direct pull; need a documented support matrix / fallback message.
4. **Compatibility support window (resolved — ADR-0026, #697/#711):** compatibility is keyed on the integer `daemon_protocol_version` (not release/semver); the served SPA supports the current protocol plus N-1. A daemon 2+ breaking versions behind is gated with an explicit `hosted_dashboard_upgrade_required` state carrying a one-action repair path; a daemon newer than the SPA expects degrades to a benign forward-skew ("dashboard catching up") state rather than hard-failing. Upgrade-required fire rate and observed protocol-version distribution are instrumented as non-confidential aggregates. *Consequence if regressed:* version skew between cloud-shipped SPA and local daemons resurfaces as silent breakage or over-aggressive gating.
5. **Origin allowlist governance (RFQ):** who approves customer-owned hosted domains. *Consequence:* out of alpha scope; blocks non-default origins.

---

## Request Coverage

| Request | Answered in |
| --- | --- |
| "Do we need the safe summary projection, since data is client-side?" | Runtime Problem; Security/Privacy (redaction is a daemon policy dial, not a transport necessity; richer `projection/ledger` exists) |
| "Are things wired correctly for client-side pull?" | Runtime Problem (client uses same-origin — wrong); Implementation Sequencing T1–T5 |
| "What browser architecture do we need to pull directly from the gateway?" | Runtime Components; S2/S3; Contracts (CORS+PNA, token); Deployment Topology |
| "Spec out completely what needs to be done" | Implementation Sequencing + Issue Generation Notes + Acceptance Criteria |
