# Alpha device connection

This page describes the current production-shaped browser-to-daemon connection path for Bicameral alpha development and troubleshooting.

## Canonical browser flow

Use these routes:

1. `/signup` authenticates the design-partner browser session with Google OAuth.
2. After Cloud accepts that session, `/connect` becomes the sole production device-connection surface.
3. `/connect` detects the browser platform and immediately probes the local Bicameral daemon.
4. If the daemon is unavailable, the same surface presents the supported installer/recovery guidance and lets the operator retry without restarting onboarding.
5. If the daemon is available, the browser starts the hosted-bridge pairing contract.
6. The daemon returns a pending request and pairing code. Approval remains explicit and local to the daemon.
7. After daemon approval, the browser stores only the bounded transient handoff in `sessionStorage` and opens `/app?onboarding=drive`.
8. The `onboarding=drive` value is navigation intent only. Cloud's `/app` wrapper waits for the paired SPA to render, opens the existing **Settings → Storage** surface, removes the query parameter, and makes no connection-status or authority claim itself.
9. Google Drive authorization is then driven by the existing daemon-backed provider flow. Drive OAuth consent, provider connection state, and token custody remain separate from Cloud sign-in and device pairing.
10. `/restore` may re-attach a still-valid approved pair on a later browser visit.
11. Bot `/app` owns the post-handoff Products/admin experience.

There is no production `/dashboard` browser onboarding route. MCP readiness is not part of device-connection completion and must not gate entry to Products/admin after a valid handoff.

## Progressive onboarding model

The user journey is one progressive connection task with three distinct states:

1. **Bicameral Cloud** — Google OAuth establishes the Cloud identity/session.
2. **This device** — the browser detects the daemon, recovers through install instructions when absent, and pairs only after explicit daemon-local approval.
3. **Google Drive** — after pairing, the existing provider UI asks the operator to authorize Drive separately.

These states are sequential, not competing alternatives. Completing one state must not imply completion of another.

In particular:

- Google sign-in does not prove Google Drive authorization.
- A paired device does not prove provider access or workspace binding.
- The `onboarding=drive` navigation hint does not prove any connection state; it only opens the existing Storage UI after a valid handoff.
- Google Drive authorization remains a separate provider OAuth transaction even when the same Google account is already signed in to the browser.

## Normal path versus recovery

The normal path is progressive: **sign in → detect/pair this device → offer Google Drive setup**. Do not present Google OAuth and device pairing as peer choices.

The browser attempts the existing pairing contract after Cloud authentication. If the local daemon is unavailable or incompatible, the connection surface renders the corresponding recovery state. Install, update, and `bicameral gateway start` commands are fallback/troubleshooting affordances, not proof that setup completed. The same `/connect` surface owns retry after local installation.

Required typed recovery families currently include unavailable daemon, update required, pairing denied, pairing expired, pairing revoked, and account mismatch. These states must remain distinct because they have different security and recovery meanings.

## Authority and secret boundaries

The local daemon is the pairing approval authority and remains authoritative for provider OAuth/token custody. Cloud coordinates browser authentication, pair request/poll, typed recovery, transient handoff, and bounded post-pair navigation only.

Do not:

- approve pairing from Cloud or the browser;
- infer Product membership, workspace binding, provider access, Decision authority, signoff, release, or deployment authority from pairing;
- infer Google Drive authorization from Google Cloud sign-in;
- let the `onboarding` query parameter set or synthesize provider/device connection state;
- place the session token, pairing code, poll proof, CSRF material, provider credential, or OAuth token in URLs, logs, analytics, or durable Cloud state;
- persist the transient session token in `localStorage`;
- treat MCP installation/readiness as proof that the browser-device connection completed;
- substitute a fixture-backed pairing success for terminal acceptance evidence.

## Deterministic developer checks

Run the Cloud pairing conformance suite:

```bash
pytest -v tests/conformance/test_cloud_dashboard_daemon_pairing.py
```

Run the progressive signup/onboarding checks:

```bash
pytest -v tests/test_signup_progressive_apc1.py
```

The pairing conformance suite verifies the browser-facing contract with controlled local test dependencies, including pending-first pairing, tokenless pending responses, explicit local approval before token release, request-handle/poll-proof binding, incompatible-daemon handling, denial, expiry, revocation, and secret-surface assertions.

Run the broader deterministic suite when changing Cloud behavior:

```bash
pytest -v tests/ \
  && python -m ruff check src/ tests/ \
  && python -m mypy src/
```

## Browser evidence

UI-changing work must use the Hosted Dashboard Browser Gate or the repository's current headed browser journey rather than relying only on HTTP or component tests. For the connection surface, review both desktop and narrow layouts and at minimum cover:

- signed-out / Google sign-in;
- authenticated ready-to-connect;
- daemon unavailable with platform-specific troubleshooting visible;
- local-confirmation/pending;
- successful handoff into `/app`;
- post-pair opening of Settings → Storage through the bounded Drive onboarding hint.

Rendered browser evidence is presentation evidence. It does not replace terminal acceptance against the real Cloud/Bot composition.

## Terminal acceptance boundary

The end-to-end acceptance journey is tracked separately from deterministic conformance. A terminal claim requires production implementations on the claimed boundaries: actual Cloud auth behavior, real browser engine, real Bot gateway/CLI, daemon-issued pairing code, explicit local approval, transient handoff, entry into `/app`, and real provider authorization when Google Drive completion is claimed.

Retained evidence must bind exact Cloud/Bot revisions and artifact identities while proving sensitive pairing/session/provider material is absent from retained logs, URLs, storage dumps, screenshots, and analytics. If credentials or a supported disposable environment are unavailable, stop at that boundary rather than replacing it with a fake daemon or synthetic success path.

## Source references

Current implementation and tests:

- `src/bicameral_cloud/routes/signup_polish_apc1.py`
- `src/bicameral_cloud/routes/connect_polish_apc1.py`
- `src/bicameral_cloud/routes/connect_polish.py`
- `src/bicameral_cloud/routes/dashboard.py`
- `tests/test_signup_progressive_apc1.py`
- `tests/conformance/test_cloud_dashboard_daemon_pairing.py`
- `tests/journey/test_vendored_dashboard_smoke.py`
- `scripts/run_real_process_hosted_topology.py`

Canonical work records:

- Cloud #256: progressive **Connect this device** UX and Cloud pre-pair ownership
- Cloud #184: terminal hosted browser-to-daemon pairing acceptance
- Bot #1092: Bot post-handoff ownership and MCP readiness separation

When these references disagree with older examples that name `/dashboard` as the browser onboarding route or describe Google sign-in and device connection as peer choices, the current `/signup` → `/connect` → `/app` progression is authoritative.
