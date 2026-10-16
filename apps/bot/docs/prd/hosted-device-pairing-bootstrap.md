# PRD: Hosted device pairing bootstrap (`/connect`)

**Status:** accepted product intent (path + surface split), updated for v0.1.26 restore  
**Owner:** product (Jin)  
**Surfaces:** Cloud `GET /connect` (pre-pair bootstrap) · Cloud `GET /restore` (transparent recovery) · Bot `/app` (post-pair product only)  
**Related implementation:** Cloud #256 / PR #258 · Bot #1092 / PR #1103 · Bot #1109  
**Related contracts:** `docs/specs/hosted-bridge-user-level-pairing.md` · `docs/specs/hosted-bridge-cross-visit-restore.md` · ADR-0027 · `dashboard/src/lib/hosted-handoff.ts`

---

## Problem statement

Operators using Bicameral through the hosted web must connect a **browser origin** to their **local daemon** once, so later Product work can run against local authority without re-teaching install, gateway, or MCP architecture every visit.

The product path is intentionally asymmetric:

> **Cloud `/connect` is a rare pairing bootstrap.**  
> Bot `/app` is product **after** a valid handoff only.  
> A still-valid previously approved pair may pass through transparent Cloud `/restore` and return to `/app` without local re-approval.  
> `/app` must not re-implement pre-pair UI.  
> **There is no browser path named `/dashboard`.**

---

## Product path (authoritative)

```text
Sign in
   │
   ▼
/connect          ← ONLY production fresh-pair surface (rare)
   │  Connect this device
   │  [ Show help ] install / start when needed
   ▼
handoff → /app    ← ONLY product shell (default)

Later visit:
/app
   │
   ├─ transient handoff still present ───────────────> /app
   │
   ├─ bounded restore proof + live daemon session ──> /restore ──> /app
   │
   └─ no valid restore / expired / revoked ─────────> /connect
```

`/restore` is an internal recovery surface, not a standing product destination and not a second pairing wizard. It does not show install, pairing-code, MCP, Product-create, or workspace setup pedagogy.

| Path | Role | Visit frequency |
| --- | --- | --- |
| **`/connect`** | Fresh device link bootstrap + Show help for install/start | Once, or after pairing is no longer restorable |
| **`/restore`** | Quiet re-attachment to an already approved, still-live daemon session | Only when transient browser handoff is absent |
| **`/app`** | Products/admin after handoff; optional MCP readiness support | Default |
| **`/dashboard`** | **Does not exist** as a product route | Never |

### Redundancy rule (non-negotiable)

| Surface | May render | Must not render |
| --- | --- | --- |
| Cloud `/connect` | Pair request, pairing code, local-approve wait, install/start **Show help** | Products, MCP gate, Drive create |
| Cloud `/restore` | Minimal restoring/retry state | Fresh pairing code, install/start rail, Product UI |
| Bot `/app` | Products/admin, post-pair MCP readiness (non-gating), re-pair redirect | Full connect wizard, install/gateway rail, second pair form |
| Install/start help | **Only on `/connect`** | `/restore` or `/app` |

---

## Persistence / rarity

**Product intent:** durable-enough session restore so `/connect` stays rare.

### v0.1.26 decision

Authority-bearing handoff material remains transient:

- bridge `session_token`: `sessionStorage` only;
- Cloud/Supabase access token: transient handoff only;
- neither token is written to durable browser storage.

Cross-visit restore durably stores only the bounded opaque rebind material already issued by the daemon pairing protocol:

- `request_handle`;
- `poll_proof`;
- bound `cloud_account_id`;
- local daemon URL;
- daemon-authored bridge-session `expires_at`.

On restore, Cloud first revalidates the current Supabase session and exact account binding. It then presents the existing handle/proof/account tuple to the local daemon's retry-safe pair-status exchange. The daemon must still find the original bridge session live and non-revoked before it may re-release the transient token.

The durable restore record expires no later than the daemon session and cannot extend daemon policy. The existing bridge policy defaults to an 8-hour session and caps requests at 24 hours. Explicit sign-out, explicit re-pair, terminal expiry/revocation, account mismatch, or invalid restore state clears the durable proof.

A daemon process restart currently removes its in-memory pairing/session state. This slice therefore makes browser **visits/tabs** durable enough to avoid needless re-pairing while the daemon session remains live; it does not claim restart-persistent local authority.

See `docs/specs/hosted-bridge-cross-visit-restore.md` for the exact security and failure contract.

---

## Authority boundary

Restore changes continuity, not authority.

- Cloud re-authenticates and coordinates.
- The daemon validates the restore proof and remains authoritative for pairing-session expiry/revocation.
- Browser persistence cannot create Product membership, select a Product, bind a workspace, create source evidence, approve signoff, resolve compliance, or write canonical events.
- A temporarily unreachable daemon is not treated as proof of revocation; the browser may retry while the restore credential remains unexpired, but it must not claim a live session.

---

## Implementation checklist

- [x] Product path name **`/connect`** only for fresh pre-pair
- [x] Cloud: serve bootstrap at `/connect`; **no `/dashboard` browser route**
- [x] Auth success and signup entry → `/connect`
- [x] Bot: unpaired `/app` leaves the product shell rather than rendering a second pairing wizard
- [x] Install/start help only on `/connect` (Show help)
- [x] `/app` = post-pairing product only
- [x] Cross-visit session restore uses bounded proof, Cloud re-authentication, and daemon revalidation (Bot #1109, v0.1.26)
- [x] Expired, revoked, malformed, and account-mismatched restore state fails closed to `/connect`
- [ ] Production deployment of the v0.1.26 Bot + Cloud implementation and exact-head release evidence

---

## Decision log

| Date | Decision |
| --- | --- |
| 2026-08-12 | Bootstrap is rare; install/start remains Show help on the fresh pair surface only. |
| 2026-08-12 | Canonical fresh pre-pair path is `/connect`; `/app` is post-pair product only; `/dashboard` is not a product route. |
| 2026-08-12 | v0.1.26 cross-visit continuity uses a bounded opaque restore proof, not durable session/Cloud tokens. Cloud re-authenticates; daemon expiry/revocation remains authoritative. |
| 2026-08-12 | Daemon-restart persistence is explicitly outside this slice; loss of daemon in-memory session state fails closed to fresh `/connect`. |
