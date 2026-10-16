# Hosted Bridge Cross-Visit Restore

**Status:** accepted implementation decision for Bot #1109  
**Target:** v0.1.26  
**Owner:** BicameralAI/bicameral-bot#1109  
**Extends:** `hosted-bridge-user-level-pairing.md` and the accepted hosted-device-pairing bootstrap PRD

## Decision

A successfully approved hosted bridge pair may be restored across browser visits without repeating `/connect` while the original daemon session is still live.

The restore mechanism reuses the existing retry-safe pairing status exchange. It does **not** make the bridge session token durable and does **not** create a second authority surface.

### Browser material

| Material | Browser storage | Lifetime | Authority |
| --- | --- | --- | --- |
| bridge `session_token` | `sessionStorage` only | current browser session / daemon expiry | daemon-issued bridge authority |
| Supabase `cloud_access_token` | `sessionStorage` only as part of the handoff | current Cloud auth session | Cloud auth only |
| `request_handle` + `poll_proof` | `localStorage` as `bicameral_hosted_bridge_restore` | never beyond daemon-authored bridge session expiry | restore proof only; not sufficient by itself |
| `cloud_account_id`, `daemon_url`, `expires_at` | same bounded restore record | same expiry | binding/routing metadata only |

The durable restore record MUST NOT contain the bridge session token, Cloud access token, provider credentials, local filesystem paths, Product state, or canonical governance state.

## Restore flow

```text
cold /app visit
  |
  +-- transient sessionStorage handoff exists --> /app
  |
  +-- bounded restore record exists ----------> Cloud /restore
  |                                                |
  |                                                +-- revalidate current Supabase session
  |                                                +-- require same cloud_account_id
  |                                                +-- GET daemon /pair/status with
  |                                                |   request_handle + poll_proof + account
  |                                                |
  |                                                +-- approved + live + not revoked
  |                                                |      -> daemon re-releases same token
  |                                                |      -> sessionStorage handoff
  |                                                |      -> /app
  |                                                |
  |                                                +-- expired/revoked/mismatch/invalid
  |                                                       -> clear restore proof
  |                                                       -> /connect
  |
  +-- no usable restore record -----------------> /connect
```

Cloud coordinates the restore but does not authorize it. The local daemon remains authoritative for origin/account binding, proof validation, session expiry, and revocation.

## TTL and retention

Pending pair requests retain their existing short confirmation TTL. After explicit local approval, the same request record remains eligible for the retry-safe status exchange only until `session_expires_at`.

The browser MUST NOT extend that expiry. The daemon's hosted bridge policy remains the source of truth, including the existing default session lifetime and maximum lifetime. At the time of this decision those policy limits are 8 hours by default and 24 hours maximum.

A daemon restart currently loses in-memory bridge session and pairing-request state. Therefore this v0.1.26 mechanism provides **cross-browser-visit continuity while the daemon session remains live**, not daemon-restart persistence. A restore attempt after daemon state loss fails closed to fresh `/connect` pairing.

## Revocation and failure semantics

- Expired, revoked, denied, unknown, malformed, or account-mismatched restore state is terminal for that restore record. Clear it and return to `/connect`.
- A Cloud auth failure never causes the daemon proof to grant access. The user must regain a valid Cloud session, and the restored Cloud account must exactly match the stored binding.
- A temporarily unreachable local daemon is not evidence of revocation. Keep the still-unexpired proof, show retry/reconnect recovery, and do not mint or claim a session.
- A successful restore writes the authority-bearing bridge token only to `sessionStorage`.
- Explicit sign-out or explicit re-pair clears the durable restore record.

## Security boundary

The durable proof is intentionally less powerful than the bridge token but remains same-origin browser material and is therefore exposed to the normal XSS threat model of `localStorage`. Its risk is bounded by all of the following being required at restore time:

1. valid current Cloud authentication;
2. exact Cloud account match;
3. exact daemon request handle and poll proof;
4. daemon-side origin/account binding;
5. a still-live, non-revoked daemon bridge session.

No durable browser credential may bypass those checks.

## Protocol impact

No new daemon wire endpoint is required. Restore reuses the existing retry-safe approved `GET /api/v1/hosted-bridge/pair/status` contract. The daemon change is retention semantics only: an **approved** pending-pair record is no longer pruned by the original five-minute confirmation TTL while its associated session remains live.

This does not change Product membership, workspace binding, source authority, signoff authority, compliance authority, or canonical event-store authority.

## Acceptance evidence

- Bot unit tests cover bounded restore parsing, cold `/app` recovery routing, expiry cleanup, and explicit re-pair cleanup.
- Gateway unit tests cover approved-record retention through session expiry and conservative fallback when approved session expiry is absent.
- Cloud tests prove the durable restore payload excludes `session_token` and `cloud_access_token`, and that `/restore` revalidates Cloud auth plus daemon status.
- Existing hosted-bridge conformance remains the protocol authority for expiry/revocation behavior.
