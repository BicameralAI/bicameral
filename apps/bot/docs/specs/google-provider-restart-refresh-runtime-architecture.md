# Runtime Architecture: Google Provider Restart-Refresh Continuity

- **Date:** 2026-09-02 (approved 2026-09-02); amended 2026-09-03 (Desktop client credential correction, bot#1211 / cloud#309)
- **Status:** `approved` — contract Spec Frozen by owner 2026-09-02, **amended** 2026-09-03 to correct the secretless-native assumption against observed live-provider behavior. Journey Boundary Review **passed** (original); amendment scope reviewed against the same boundary. Implementation slices still required before factory#901 row 5 is fully conformance-locked (Enforced + Tested e2e).
- **Level:** L2 cross-repo contract
- **Owns:** Cloud-published Google native **app-config** → daemon-local Google user auth / OS-keyring token envelope (incl. provider-required Desktop client credential) / `ConnectionRegistry` / post-restart refresh continuity, and what observation satisfies checklist id `restart-refresh-envelope`. Does **not** own hosted Supabase “Continue with Google”, device pairing (row 3), join-trust poster (row 2), create-proof (row 1), or pairing identity enrollment (row 12).
- **Canonical machine contract:** `google-provider-restart-refresh-runtime-architecture.contract.json` (`RUNTIME-SPEC-GOOGLE-PROVIDER-RESTART-REFRESH`). Tables below are review projections of that file.
- **Governance gate:** `bic:spec-governance-gate`; Qor advisory/no-op. UserJourney: `google_provider_restart_refresh`. Journey Boundary Review: `passed` (owner confirmed 2026-09-02 in this session).
- **Factory registry:** [factory#901](https://github.com/BicameralAI/bicameral-factory/issues/901) row 5; lock issue [factory#906](https://github.com/BicameralAI/bicameral-factory/issues/906); contractor [factory#784](https://github.com/BicameralAI/bicameral-factory/issues/784)
- **Related:** `docs/specs/google-native-oauth-app-config.md` (accepted #1113/#1114), `docs/specs/oauth-and-integration-connections.md`, cloud#262/#263, cloud#309, bot#1114, bot#1211, factory#542; release-units `google-native-oauth-app-config.json` / `v0128-google-provider-auth-secure-completion.json`

Owner freeze: real process-kill e2e for `restart-refresh-envelope`; envelope `client_id` refresh for existing grants; Cloud GET for new auth; producer Enforced = main + live 200; production OS keyring only; OAuth ⟂ pairing.

**Amendment freeze (2026-09-03):** Google's live token endpoint requires the Google-issued Desktop client credential on token exchange and refresh for the correctly-registered `bicameral-daemon-prod` Desktop client (verified in Google Cloud Console by Kevin 2026-09-03 — not a Web/confidential-client misregistration). The credential is provider-required request material, not proof of confidential-client status; PKCE remains the actual interception defense and the native/public classification is unchanged. See cloud#309 for the live-failure evidence and root-cause correction.

## Reading Guide

Read **Runtime invariant**, **Decision (locked)**, **Forbidden flows**, and **Journey Boundary Review** first.

```text
Cloud publishes the shop sign (app-config).
The daemon keeps the user’s keys in the keyring.
Registry may say connected only after the keyring write.
Restart must not invent connected — and must not demand a new shop sign to refresh an old key.
```

## Runtime Problem

Factory#901 row 5 was not Spec Frozen. Pieces exist, but:

1. `#901` names Cloud Google OAuth → `restart-refresh-envelope` while the checklist id is an **evidence observation**, not a wire payload — easy to “pass” with in-process reconstruction.
2. Cloud `GET /api/v1/provider-app-config/google` (cloud#262 lineage) has been shipped by Cloud #304; the production GET is live HTTP 200 (owner-stated 2026-09-03). Freeze-era probes (2026-09-02) that saw live 404 / not-on-main are obsolete and must not be frozen by this amendment.
3. Daemon refresh-from-envelope and credential-backed projection exist; OS keyring round-trip is `#[ignore]`; no real process-kill + Drive receipt satisfies the checklist.
4. Aritro/#784 could not remediate restart continuity against a frozen composed contract.

This approved contract closes the Spec Frozen gate. Enforced + Tested e2e remain open.

### Amendment 2026-09-03: the Google Desktop client-secret gotcha

The original approved contract (goal `CONF-CLOUD-APP-CONFIG-PUBLISH` and prior implementation) assumed the textbook OAuth2 native-app model: a Desktop/public client using PKCE needs no `client_secret` at all, per RFC 7636. Row-5 was written and frozen on that assumption.

Real-world failure (Aritro, live Drive-connect journey, cloud#309): every authorization-code exchange against Google's production token endpoint returned

```json
{"error": "invalid_request", "error_description": "client_secret is missing."}
```

Two rounds of diagnosis:

1. **First theory, disproved:** the OAuth client must be misregistered as Web/confidential. Kevin verified directly in Google Cloud Console (2026-09-03) that `bicameral-daemon-prod` **is** correctly registered as Desktop, the Client ID matches the production deployment variable exactly, and unused duplicate repo secrets from provisioning were not causal.
2. **Actual cause — the gotcha:** Google issues a client secret even for Desktop-type OAuth registrations, and its live token endpoint enforces the secret's presence on this registration regardless of PKCE being correctly used. This is provider behavior, not a Bicameral misconfiguration, and it does not match the RFC 7636 "PKCE alone suffices" assumption the frozen spec was built on.

The secret this forces us to send is **not** a confidential-client authentication boundary — it ships inside a distributed native binary and cannot be kept secret in the cryptographic sense. Sending it satisfies Google's API contract; it does not change what actually defends the authorization code (PKCE still does that). This distinction is why the amendment below adds credential plumbing without touching the daemon's public-client classification, PKCE requirement, or any part of the Cloud/daemon token-custody split.

**Goals**

1. Cloud publishes bounded non-secret `GoogleNativeAppConfigV1` without user tokens (`CONF-CLOUD-APP-CONFIG-PUBLISH`).
2. Daemon Cloud-first resolve for **new** auth; 410 fail-closed; legacy env only explicit opt-in (`CONF-LEGACY-ENV-OPT-IN-ONLY`).
3. Persist token envelope (incl. `client_id`) in OS keyring **before** registry `connected` (`CONF-PERSIST-BEFORE-CONNECTED`).
4. Projected `connected` is credential-backed; missing secret → `action_needed` without GET mutating disk (`CONF-CREDENTIAL-BACKED-PROJECTION`).
5. After real daemon process restart, refresh existing grant using envelope `client_id` without requiring live Cloud GET (`CONF-ENVELOPE-REFRESH-AFTER-RESTART`).
6. Production has **no** silent in-memory secrets fallback (`CONF-NO-SILENT-INMEMORY-FALLBACK`).
7. `restart-refresh-envelope` evidence means real OS kill → new process → Drive usable (`CONF-REAL-PROCESS-KILL-E2E`).
8. No coupling to pairing `pairing_id` / bridge session (`CONF-NO-PAIRING-COUPLING`).
9. Cloud never holds user Google tokens (`CONF-NO-CLOUD-USER-TOKENS`).
10. Provider-required Desktop client credential is available for initial exchange and refresh without weakening PKCE, changing the daemon's public-client classification, or requiring a live Cloud GET after restart (`CONF-DESKTOP-CREDENTIAL-PROVIDER-REQUIRED`).
11. The credential is distributed through an authenticated Cloud boundary distinct from the public app-config route and is never present in that route's payload (`CONF-CREDENTIAL-DISTRIBUTION-AUTHENTICATED-ONLY`).
12. A cached credential is rejected and re-fetched, not silently reused, when its bound `client_id` or distribution `config_version` disagrees with the daemon's current configuration (`CONF-CREDENTIAL-ROTATION-FAIL-CLOSED`).

**Non-goals**

- Hosted Supabase Google login for the dashboard session.
- Row 3 pairing / `/connect` / durable `pairing_id`.
- Moving user tokens through Cloud or making Cloud a ConnectionRegistry.
- Declaring factory#784 or cloud#263 done.
- Softening production to accept in-process “fresh store” as e2e.
- Making Cloud a Google OAuth token broker (the credential boundary carries only the Desktop client credential, never authorization codes or user tokens).
- Treating the Desktop client credential as a confidential-client authentication boundary.
- Requiring row-12 pairing / daemon-identity enrollment for Desktop client-credential distribution (caller authority is the signed-in operator’s transient Cloud bearer only).
- Reverting to manual `BICAMERAL_GOOGLE_CLIENT_*` env injection as the production distribution path (remains explicit lab opt-in only, per existing `CONF-LEGACY-ENV-OPT-IN-ONLY`).

## Decision (locked)

Owner, 2026-09-02:

| Decision | Lock |
|---|---|
| Runtime meaning of `restart-refresh-envelope` | **Real OS process kill** → new daemon → Drive still usable from keyring + registry. Checklist id records that observation. |
| Refresh after restart | Envelope-recorded `client_id` OK for **existing** grant. **New** OAuth still Cloud-first app-config GET. |
| Producer Enforced bar | Cloud route on **`main` + live non-secret HTTP 200** (provisioned client id). |
| Legacy env | Explicit opt-in lab only; must not silently outrank valid Cloud app-config; not the production path. |
| Secrets store | Production **OS keyring only**; no silent in-memory fallback across restart. |
| vs pairing | Citation only (durable vs transient vocabulary). No shared identity/store with row 3. |
| Authority split | Reaffirm #1113/#1114: Cloud = app identity; daemon = user tokens + Connection truth. |
| Desktop client credential (amendment, owner 2026-09-03) | Live Google token endpoint requires it for this Desktop registration; sent on initial exchange and refresh alongside PKCE. Not evidence of confidential-client status; does not change native/public classification. |
| Credential storage (amendment) | New OS-keyring entry `connection.google` / `client_secret`, a versioned envelope alongside the existing `oauth_token` envelope. Reuses `OsKeyringSecretsStore`; no new storage primitive. |
| Credential distribution (amendment) | New **authenticated** Cloud endpoint, distinct from the public `GET /api/v1/provider-app-config/google`. Caller authority is the signed-in operator’s existing transient Cloud bearer, handed to the local daemon for this credential fetch and then discarded — **not** row-12 pairing / daemon-identity enrollment. Daemon caches locally; restart/refresh never requires a live Cloud GET (reaffirms `CONF-ENVELOPE-REFRESH-AFTER-RESTART`). |
| Credential mismatch/rotation (amendment) | Cached credential envelope carries `client_id` + `source_config_version`. Use is rejected fail-closed (treated as missing → re-fetch), never silently reused, when either disagrees with current daemon configuration. |
| Public app-config schema (amendment) | `GoogleNativeAppConfigV1` is **unchanged**; the credential is never added to that payload. |

## Runtime invariant

```text
App-config is public shop sign.
User tokens stay in the daemon keyring.
connected requires keyring write then registry.
Restart refreshes from the envelope; it never manufactures connected.
The Desktop client credential travels its own authenticated boundary —
it is provider-required plumbing, never a confidential-client claim,
and never rides the public app-config shop sign.
```

## Forbidden flows

1. Cloud receiving/storing Google authorization codes, PKCE verifiers, or access/refresh tokens on this hop.
2. Registry `connected` without a successful durable secret write.
3. Projecting `connected` when the secret is unreadable (must be `action_needed`).
4. Counting in-process `InMemorySecretsStore` reconstruction as `restart-refresh-envelope` e2e.
5. Silent legacy env override beating live Cloud app-config in production.
6. Using bridge `session_token` / `pairing_id` to manufacture Drive `connected`.
7. Treating hosted Supabase Google login as this hop’s producer.
8. Sending the Desktop client credential through the unauthenticated `GET /api/v1/provider-app-config/google` response or any other public/non-secret surface.
9. Using a keyring-cached Desktop client credential whose bound `client_id` or `config_version` disagrees with current daemon configuration, instead of re-fetching or failing closed to `action_needed`.
10. Treating the Desktop client credential as proof the daemon is a confidential OAuth client, or using its presence to condition away PKCE.
11. Routing the Desktop client credential, or any Google authorization code/user token, through the credential-distribution boundary in the reverse direction (i.e. Cloud never receives them back).

## State refinement (honest)

| State | Today | Required by freeze |
|---|---|---|
| Cloud app-config publication | Cloud #304 shipped; production GET live HTTP 200 (owner-stated 2026-09-03) | main + live 200 for Enforced (deployment bar met; full row-5 Enforced lock still needs remaining conformance) |
| Token envelope in OS keyring | production constructors; ignored round-trip test | required; must survive process death |
| ConnectionRegistry file | metadata only | unchanged; never secrets |
| In-process restart stub tests | exist | supporting only — not e2e |
| Real process-kill + Drive | **missing** | required for Tested e2e / checklist |
| Desktop client credential on exchange/refresh | **missing** — live token endpoint rejects with `invalid_request: client_secret is missing` (cloud#309) | required; sent on initial exchange and refresh, sourced from authenticated distribution boundary |
| Credential keyring envelope | **missing** | required; versioned envelope with bound `client_id`/`config_version`, fail-closed on mismatch |
| Authenticated credential-distribution endpoint | **missing** | required on Cloud; distinct from public app-config route |

## User Journey Contract

```yaml
user_journey:
  id: google_provider_restart_refresh
  name: Google provider auth and Drive restart continuity
  actor: signed_in_operator_with_local_daemon
  entrypoint: daemon Google connect (Cloud app-config → PKCE → keyring → registry)
  success_state: after real daemon process restart, Drive remains usable via envelope refresh
  surfaces:
    - cloud_provider_app_config
    - local_operator_daemon
    - os_keyring
    - connection_registry
    - drive_api_transport
    - spa_connection_courier
  boundary_contracts:
    - GoogleNativeAppConfigV1
    - OAuthTokenEnvelopeV1
    - GoogleClientCredentialEnvelopeV1
    - ConnectionRegistry connected/action_needed
    - restart-refresh-envelope observation
  required_evidence:
    - live_app_config_200
    - real_process_kill_drive_continuity
    - negative_missing_secret_action_needed
    - initial_exchange_includes_credential_and_pkce
    - missing_credential_fails_safely_to_action_needed
    - credential_never_in_logs_or_projections
    - rotation_mismatch_forces_refetch_not_silent_reuse
journey_kind: product
ux_impact: true
```

`ux_design.disposition: existing_surfaces` — freeze governs existing Connect Google / Drive readiness semantics; no new primary UI surface.

### Journey Boundary Review

```yaml
journey_boundary_review:
  status: passed
  journey_ids:
    - google_provider_restart_refresh
  authoritative_commit_points:
    - cloud_app_config_publication
    - daemon_keyring_envelope_write
    - connection_registry_connected
    - envelope_refresh_after_restart
  independently_observable_facts:
    - app_config_get_status
    - keyring_envelope
    - registry_row
    - drive_api_success
  partial_failure_reviewed: true
  retry_and_idempotency_reviewed: true
  validation_witness_separated_from_product_state: true
  user_confirmed: true
  source_ref: docs/specs/google-provider-restart-refresh-runtime-architecture.md
```

Walkthrough (owner confirmed 2026-09-02):

```text
  operator/SPA          Cloud app-config           daemon                 keyring/registry         Google/Drive
     |                       |                        |                         |                      |
     |  Connect Google       |                        |                         |                      |
     |----------------------------------------------->| GET app-config          |                      |
     |                       |<-----------------------|                         |                      |
     |                       |  GoogleNativeAppConfig |                         |                      |
     |                       |----------------------->| PKCE + loopback OAuth   |--------------------->|
     |                       |                        | token envelope write    |                      |
     |                       |                        |------------------------>|                      |
     |                       |                        | registry connected      |                      |
     |                       |                        |------------------------>|                      |
     |  Drive usable         |                        |                         |                      |
     |<-----------------------------------------------|                         |                      |
     |                       |                        |                         |                      |
     |                 *** real OS process kill / new daemon process ***        |                      |
     |                       |                        | load registry+keyring   |                      |
     |                       |                        | refresh via envelope    |--------------------->|
     |                       |                        | client_id (no Cloud GET)|                      |
     |  Drive still usable   |                        |                         |                      |
     |<-----------------------------------------------|                         |                      |
```

1. **Topology:** Cloud app-config publisher, daemon gateway, OS keyring, ConnectionRegistry file, Drive transport, SPA courier, Google IdP.
2. **Happy path:** app-config GET → PKCE/user consent → keyring envelope (incl. `client_id`) → registry `connected` → Drive.
3. **Commit points:** Cloud publishes config; daemon keyring write then registry; refresh may rewrite envelope after restart. SPA never commits Connection truth.
4. **Must not cross:** user tokens to Cloud; pairing session → Drive connected; silent in-memory production store.
5. **Disagreeing facts:** registry `connected` without readable secret → `action_needed`; envelope refresh without Cloud; a failed/unavailable app-config GET must not fake connected.
6. **Partial failure:** missing app-config → new auth blocked (503/410 paths); missing secret after restart → `action_needed`; Drive API fail after refresh → surface error, do not invent connected.
7. **Retry:** refresh is idempotent relative to near-expiry; new auth re-resolves Cloud app-config; forbidden: in-process stub as e2e, legacy env silent override, Cloud token custody.
8. **Witness vs authority:** app-config GET 200, checklist receipt, SPA UI labels are witnesses. Authority: keyring+registry for Connection; Drive success for readiness claim.
9. **Credential boundary (amendment):** the Desktop client credential travels via the signed-in operator’s transient Cloud bearer (handed to the local daemon for this fetch, then discarded) → authenticated credential-distribution endpoint → daemon keyring. That path never intersects the SPA, the public app-config route, row-12 pairing / daemon-identity enrollment, or the authorization-code/token exchange with Google except as an additional request parameter alongside PKCE. It is witness material for satisfying Google's API contract, never authority for confidential-client status or connection truth.

## Implementation slices (after Spec Frozen)

| Slice | What |
|---|---|
| A | Promote Cloud app-config to main + provision + live 200 smoke (cloud#262 lineage) — **done** via Cloud #304 |
| B | Harden daemon envelope refresh / no silent in-memory / legacy opt-in labeling |
| C | Real-process kill → new daemon → Drive continuity evidence (`restart-refresh-envelope`) — cloud#263 / #784 |
| D | Amend release-unit / journey catalog prose so checklist id matches this runtime meaning |
| E | Cloud: authenticated Desktop client-credential distribution endpoint, distinct from public app-config route — cloud#309 |
| F | Daemon: `GoogleClientCredentialEnvelopeV1` in OS keyring; send credential on initial exchange and refresh alongside PKCE; fail-closed on missing/mismatched credential — bot#1211 |
| G | Deterministic tests: credential+PKCE on exchange, missing-credential safe failure, no credential in logs/projections, rotation/mismatch re-fetch, legacy env still explicit opt-in only — bot#1211 |

## ADR implications

- `google-native-oauth-app-config.md` (#1113/#1114): **reaffirmed**.
- `oauth-and-integration-connections.md`: **reaffirmed** on persist-before-connected; refine “visible keyring fallback” so production has **no** silent in-memory restart path.
- Pairing row-3 approved spec: **unaffected** (citation only).

## Out of scope reminders

- Mayur pairing / row 3 / #783.
- Row 6 MCP `active_verified`.
- Row 2 join-trust poster.
