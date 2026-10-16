# Runtime Architecture: Hosted Device Pairing Handshake

- **Date:** 2026-09-02 (approved 2026-09-02; **initiation-fields amendment freeze-with-changes 2026-09-16**)
- **Status:** `approved` — base contract Spec Frozen by owner 2026-09-02. **Amendment (bot#1290) incorporates owner freeze-with-changes 2026-09-16** of pair-initiation `team_id` + `cloud_access_token` rules below; this revision is the freeze target. Journey Boundary Review **passed** for the 2026-09-02 freeze; amendment does not reopen journey topology. Implementation slices still required before factory#901 row 3 is fully conformance-locked (Enforced + Tested e2e).
- **Level:** L2 cross-repo contract
- **Owns:** Cloud `/connect` + `/restore` → local daemon device pairing handshake, durable `pairing_id` reattach across daemon restart, and `/app` success witness (`session_token`). Also owns **minting** the enrollment authorization artifact at local approve (one ceremony, two artifacts). Does **not** own Cloud enrollment verify/persistence (row 12 consumer), Google OAuth/Drive continuity (row 5), join-trust poster (row 2), or create-proof (row 1).
- **Canonical machine contract:** `hosted-device-pairing-handshake-runtime-architecture.contract.json` (`RUNTIME-SPEC-HOSTED-DEVICE-PAIRING-HANDSHAKE`). Tables below are review projections of that file.
- **Governance gate:** `bic:spec-governance-gate`; Qor advisory/no-op. UserJourney: `hosted_device_pairing`. Journey Boundary Review: `passed` (owner confirmed 2026-09-02 in this session).
- **Factory registry:** [factory#901](https://github.com/BicameralAI/bicameral-factory/issues/901) row 3; lock issue [factory#905](https://github.com/BicameralAI/bicameral-factory/issues/905); contractor [factory#783](https://github.com/BicameralAI/bicameral-factory/issues/783)
- **Surfaced gap:** [bot#1290](https://github.com/BicameralAI/bicameral-bot/issues/1290) — row 12 enroll requires `team_id`, but this handshake never obligated Cloud to put `team_id` / `cloud_access_token` on `POST /pair`.
- **Row 12 RFCs (decision-locked):** [bot#1198](https://github.com/BicameralAI/bicameral-bot/issues/1198), [bot#1199](https://github.com/BicameralAI/bicameral-bot/issues/1199), [bot#1200](https://github.com/BicameralAI/bicameral-bot/issues/1200)
- **Related:** ADR-0005, ADR-0025, `docs/prd/hosted-device-pairing-bootstrap.md`, `docs/specs/hosted-bridge-cross-visit-restore.md`, draft `hosted-bridge-user-level-pairing.md`, cloud#184, bot#867, cloud#266, cloud#302/#303/#305/#306; Bot PR #1203 / #1206

Owner freeze (2026-09-02): local-only approve; `/app` = live `session_token`; durable opaque `pairing_id` remints session after daemon restart without a new code; one ceremony also mints enrollment auth; Enforced requires revoke + mismatch fail-closed with the durable store.

**Amendment freeze target (bot#1290, freeze-with-changes 2026-09-16):**

1. **Verified domain** = Supabase-verified **and** present on the design-partner allowlist. Consumer/shared domains are never allowlisted. `team_id` is a Cloud-issued **opaque** ID.
2. Durable storage; at most one domain-scoped team per allowlisted domain; **idempotent create** with `TEAM_CREATED` audit; a **distinct 409** with an operator repair path when more than one team exists. Any allowlisted-domain user may create/enroll **their own** pairing; `team_owner` is not required.
3. Keep raw `cloud_access_token` for Alpha (a narrower signed capability is post-Alpha). Custody is the pending record only; clear on approve/deny/expiry; never persist, log, or return it.
4. Missing/empty `team_id` or `cloud_access_token` → **HTTP 400** with a typed kind (`MissingTeamId` / `MissingCloudAccessToken`). `/connect` and `/app` SPA map that to return-to-`/connect`.
5. Design-partner domain allowlist is not team creation.
6. Cloud verifies bearer identity + allowlist, Rule-2 domain eligibility, and grant-to-path binding of `team_id` and `cloud_account_id` at **both** pairing create and enrollment.
7. Product selection stays out of the handshake. Client-supplied `team_id` is not enrollment trust alone.

## Reading Guide

Read **Runtime invariant**, **Decision (locked)**, **Forbidden flows**, and **Journey Boundary Review** first.

```text
Code possession ≠ approval.
Local approve is the commit.
session_token opens /app; pairing_id survives the daemon;
enrollment auth is a second wax from the same stamp — not HostedSession.
```

## Runtime Problem

Factory#901 row 3 was not Spec Frozen. Producer and consumer paths exist, but:

1. Written sources disagreed (draft L2 pairing, draft data-plane runtime, accepted PRD/restore, open row 12 RFCs).
2. Today’s `HostedBridgePairingSession` / pending store is **process-local**. Daemon restart forces a full code+approve even when the operator already approved this browser/daemon relationship.
3. Cloud `DaemonPairing` / `HostedSession` are a different object than the daemon-approved bridge session; conflating them enabled the cloud#303 enrollment defect class.
4. No harness composes **real browser + real daemon** for this hop (in-process / fake-daemon only).
5. Mayur/#783 could not remediate pairing/restart semantics against a frozen contract.

This approved contract closes the Spec Frozen gate. Enforced + Tested e2e remain open.

## Goals And Non-Goals

**Goals**

1. Pending-first `POST /pair`: never return `session_token` before local approve (`CONF-PENDING-FIRST-01`).
2. `POST /pair/decision` is LOCAL-only; Cloud/browser must not approve (`CONF-LOCAL-APPROVE-01`).
3. `/app` success witness is a **daemon-minted live `session_token` only** (`CONF-APP-SESSION-TOKEN-01`).
4. At `LOCAL_APPROVED`, mint opaque durable `pairing_id` bound to account + origin + protocol + daemon instance identity (`CONF-PAIRING-ID-01`).
5. Same approve mints enrollment authorization (short TTL, one-shot) under Alpha TOFU daemon identity (`CONF-ENROLLMENT-MINT-01`).
6. Durable `pairing_id` survives daemon process death; daemon may mint a new `session_token` without a new pairing code when relationship is non-revoked; Enforced requires revoke + mismatch fail-closed (`CONF-DURABLE-REATTACH-01`).
7. Browser cold start uses Cloud `/restore` when restore proof is valid; otherwise `/connect` (`CONF-RESTORE-01`).
8. HostedSession / DaemonPairing alone must not enroll proof-signing keys (`CONF-NO-SESSION-ENROLL-01`).
9. Real browser + real daemon e2e including at least one daemon restart reattach (`CONF-E2E-BROWSER-DAEMON-01`).
10. No coupling to Google OAuth / Drive token continuity (`CONF-NO-OAUTH-COUPLING-01`).
11. Pending `POST /pair` carries non-empty Cloud `team_id` + transient `cloud_access_token` so approve-time enrollment mint can complete (`CONF-PAIR-INIT-TEAM-01`) — **amendment bot#1290**.

**Non-goals**

- Cloud enrollment verify, multi-instance trust-history store, or replacing cloud#303 implementation (row 12 consumer follow-on).
- Dogwood / TRACE / UOR dependency adoption (parked; local semantics only).
- Row 5 Drive `restart-refresh-envelope`.
- Row 2 join-trust poster; row 1 create-proof; row 6 MCP `active_verified`.
- Declaring factory#783 done.
- Product picker inside `/connect` / pair handshake (Product selection remains a later flow).
- Multi-team membership UI (Alpha: one verified domain → one Cloud team).
- Changing `PairingAuthorizationV1` wire schema (row 12 already requires `team_id`).

**Not closed by this amendment**

- Daemon-issued `pairing_id` is carried **unchanged**.
- Cloud #306 enrollment route must be present in the served composition (independent Cloud hold).
- Durable team / trust-root records remain Cloud persistence work.
- Pre-amendment pairings recover by **revoke + re-pair**, not in-place repair.
- **No enrollment retry** as a pairing-handshake recovery path.

## Decision (locked)

Owner, 2026-09-02:

| Decision | Lock |
|---|---|
| `/app` witness | Live daemon `session_token` only. Not Cloud `DaemonPairing` / `HostedSession` / code possession. |
| `pairing_id` | Daemon-issued **opaque** id at `LOCAL_APPROVED`. |
| Bound metadata | `cloud_account_id` + approved origin + protocol/profile version + daemon instance identity. **Amendment:** also bound Cloud `team_id` from the pending pair request. |
| Ceremony | **One approve → two artifacts**: `session_token` + enrollment authorization. |
| Durability (posture B) | `pairing_id` survives daemon restart; may **silently remint** `session_token` without new code unless revoked / mismatch. |
| Enforced ops controls | Durable store slice **must** ship explicit **revoke** (clears relationship + live sessions) and **fail-closed** on account/origin/daemon-identity mismatch. No “delete the store to unstick.” |
| Enrollment grant | Short TTL + one-shot consume; enrolled key survives restarts until key revoke. Independent of `/app` session lifetime. |
| Alpha trust root | Explicit **TOFU** at first approve for daemon instance identity. |
| Dependencies | No UOR; no Dogwood for Alpha; TRACE reference only. |
| Spec scope | First pair + `/restore` + durable daemon-restart reattach. |
| OAuth | Unrelated hop (row 5). |
| Pair initiation (amendment bot#1290) | Verified domain = Supabase-verified + design-partner allowlist; consumer/shared domains never allowlisted. `team_id` is a Cloud-issued opaque ID. Durable storage: at most one domain-scoped team; idempotent create + `TEAM_CREATED`; distinct 409 with operator repair path if >1. Any allowlisted-domain user may create/enroll their own pairing (`team_owner` not required). `POST /pair` requires non-empty `team_id` + `cloud_access_token`. Bot **400** with typed kind if either is missing/empty; `/connect` and `/app` SPA map to return-to-`/connect`. Raw `cloud_access_token` is Alpha-appropriate; pending-record custody only; clear on approve/deny/expiry; never persist, log, or return. Cloud re-verifies identity, allowlist, domain eligibility, and `team_id`/`cloud_account_id` binding at create **and** enroll. Design-partner allowlist ≠ team creation. Product selection is out of handshake. |

## Runtime invariant

```text
Browser may request and poll.
Only local daemon approval commits the relationship.
Ephemeral session opens /app; durable pairing_id reopens it after restart;
enrollment wax is separate and never HostedSession.
```

## Forbidden flows

1. Cloud or SPA calling `/pair/decision` or otherwise approving.
2. Returning `session_token` on pending `POST /pair` (immediate-token legacy).
3. Treating Cloud `HostedSession` / `DaemonPairing` as `/app` paired or as enrollment authority.
4. Manufacturing `connected` Drive / OAuth state from a bridge session (row 5).
5. Silent TOFU upgrade to “production root” without an explicit maturity decision.
6. Deleting the durable pairing store as a recovery path (hides revoke/mismatch).
7. Reminting `session_token` after revoke or binding mismatch.
8. Accepting pending `POST /pair` without non-empty `team_id` or `cloud_access_token` (silent optional → enroll-time fail-closed) — **amendment bot#1290**.
9. Silently picking the first of multiple eligible teams, or inventing a provisional team without the Alpha auto-create rule — **amendment bot#1290**.
10. Persisting, logging, or returning `cloud_access_token` outside pending-record custody — **amendment bot#1290**.
11. Gating pairing create/enroll of the caller’s own pairing on `team_owner` — **amendment bot#1290**.

## State refinement (honest)

| State | Classification | Persistence today | Persistence required by this freeze |
|---|---|---|---|
| Pending pair request | coordination | daemon memory | daemon memory OK (short TTL) |
| `pairing_id` relationship | coordination / trust relationship | **missing** (not durable) | **operator-local durable store** |
| `session_token` / `HostedBridgePairingSession` | coordination | daemon memory + sessionStorage | remains ephemeral |
| Enrollment authorization (unconsumed) | evidence / grant | **missing** | daemon-minted, short TTL |
| Cloud enrolled key | Cloud trust material | cloud#303 provisional | row 12 consumer; out of mint scope |
| Restore proof (handle/proof) | coordination witness | localStorage | keep; not authority |

**States added by this freeze (vs shipped Alpha):** durable `pairing_id` relationship store; enrollment authorization mint.  
**States deferred:** Cloud durable enrollment trust-history; Dogwood-backed temporal reducer; install-time/hardware roots; soft re-auth (Touch ID) remint variant.

## User Journey Contract

```yaml
user_journey:
  id: hosted_device_pairing
  name: Hosted device connect and local daemon pairing
  actor: signed_in_operator
  entrypoint: Cloud GET /connect (fresh) or GET /restore (cold browser)
  success_state: /app reachable with live daemon-minted session_token
  surfaces:
    - cloud_connect_restore_html
    - local_operator_daemon_hosted_bridge
    - bicameral_pairing_cli
    - bot_hosted_app_spa
    - durable_pairing_store
  boundary_contracts:
    - HostedBridgePairingPendingResponse
    - LocalPairingDecisionRequest
    - HostedBridgePairingStatusResponse
    - durable pairing_id relationship
    - PairingAuthorizationV1 enrollment mint
  required_evidence:
    - producer_consumer_contract_tests
    - real_browser_plus_real_daemon_pair
    - daemon_process_restart_reattach
    - revoke_and_mismatch_fail_closed
journey_kind: product
ux_impact: true
```

`ux_design.disposition: existing_surfaces` — freeze governs `/connect`, `/restore`, local approve, and `/app` gate semantics already present; durable reattach adds operator-local persistence behavior.

### Journey Boundary Review

```yaml
journey_boundary_review:
  status: passed
  journey_ids:
    - hosted_device_pairing
  authoritative_commit_points:
    - local_pair_decision_approve
    - durable_pairing_id_persist
    - session_token_mint_or_remint
    - enrollment_authorization_mint
  independently_observable_facts:
    - pending_pairing_code
    - durable_pairing_id_record
    - live_session_token
    - restore_proof
    - unconsumed_enrollment_auth
    - cloud_hosted_session  # witness only
  partial_failure_reviewed: true
  retry_and_idempotency_reviewed: true
  validation_witness_separated_from_product_state: true
  user_confirmed: true
  source_ref: docs/specs/hosted-device-pairing-handshake-runtime-architecture.md
```

Walkthrough (owner confirmed 2026-09-02):

```text
  operator          Cloud /connect|/restore        local daemon              durable store
     |                      |                           |                         |
     |  open /connect       |                           |                         |
     |--------------------->|  POST /pair (pending)     |                         |
     |                      |  + team_id + cloud_access_token (amendment #1290)
     |                      |-------------------------->| pending (no session_token)|
     |  show pairing code   |<--------------------------|                         |
     |<---------------------|                           |                         |
     |  Approve on http://127.0.0.1:37373/local/pairing  |                         |
     |  (or: bicameral pairing approve)  (LOCAL only)    |                         |
     |------------------------------------------------->| LOCAL_APPROVED          |
     |                      |                           |--mint pairing_id ------->|
     |                      |                           |--mint session_token     |
     |                      |                           |--mint enrollment auth   |
     |                      |  GET /pair/status         |                         |
     |                      |-------------------------->|                         |
     |                      |<----- session_token ------|                         |
     |  /app (sessionStorage token)                     |                         |
     |<---------------------|                           |                         |
```

Restart / restore (no new code when relationship valid):

```text
  daemon kill/restart  →  load pairing_id  →  remint session_token  →  /app
  tab cold start       →  GET /restore      →  reattach             →  /app
  revoke / mismatch     →  full /connect code ceremony again
```

1. **Topology:** operator, Cloud `/connect`+`/restore`, daemon hosted-bridge, CLI approve, durable pairing store, `/app` SPA.
2. **Happy-path crossings:** (a) pending `POST /pair`, (b) local approve, (c) persist `pairing_id`, (d) mint `session_token` + enrollment auth, (e) `/pair/status` handoff, (f) `/app`.
3. **Commit points:** only local approve creates `pairing_id`. Remint after restart is not a new relationship commit.
4. **Must not cross:** Cloud/SPA approve; private enrollment keys; Google OAuth tokens; create-proof keys; join poster keys.
5. **Facts that may disagree:** `pairing_id` without live token (restart/tab loss); live token without unconsumed enrollment (TTL); enrolled Cloud key without `/app` session; HostedSession without daemon approve.
6. **Partial failure:** deny/expire → unpaired; mismatch/revoke → fail closed to `/connect`; enrollment TTL expiry does not unpair `/app`.
7. **Retry:** pending status poll is idempotent; restore/reattach remint when non-revoked; forbidden: remote approve, delete durable store to “fix”, treat HostedSession as paired/enrolled, manufacture Drive `connected` from pair.
8. **Witness vs authority:** code, restore proof, HostedSession, enrollment auth, HTTP 200s are witnesses. Authority: local approve → `pairing_id`; live `session_token` → `/app`; consumed enrollment → Cloud signing trust (row 12).

## Implementation slices (after Spec Frozen)

| Slice | What |
|---|---|
| A | Durable pairing relationship store + remint/reattach API **including revoke + mismatch fail-closed** |
| B | Mint opaque `pairing_id` + bound metadata + TOFU daemon instance identity at approve |
| C | Enrollment authorization mint (TTL, one-shot) — Cloud verify = separate row 12 issue |
| D | Align `/connect` + `/restore` + `/app` gate with frozen witnesses |
| E | Real browser + real daemon e2e including daemon kill/restart reattach (HITL) |
| F | **Amendment bot#1290 (after Spec Freeze):** Cloud `/connect`+SPA inject sole domain `team_id` + `cloud_access_token`; idempotent auto-create + `TEAM_CREATED`; distinct 409 repair if >1; Bot `/pair` typed 400 without `team_id` or `cloud_access_token` with SPA return-to-`/connect`; domain-eligible create/enroll (not `team_owner`); Cloud re-verify at create and enroll; keep enroll fail-closed as defense in depth |

## ADR implications

- **ADR-0005** (hosted shell / local daemon authority): **reaffirmed**.
- New/amended ADR recommended for durable `pairing_id` + Alpha TOFU daemon identity (implementation-adjacent).
- Draft L2 `hosted-bridge-user-level-pairing.md`: **supersede or demote** to historical now that this contract is approved.

## Out of scope reminders

- Mayur Codex / `active_verified` → row 6 / mcp#851.
- Aritro Drive restart → row 5 / factory#906.
- Join verification keys → row 2.
- Full row 12 Cloud enrollment verify / cloud#303 replacement.
