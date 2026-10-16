# APC1 Connect Polish

This successor branch is rooted directly at promoted Alpha Cloud `main` commit `2472b681208b7d672f9f5e6668bf8f3cddf46d34`.

The original pre-promotion implementation was reviewed on PR #333 from frozen Alpha commit `b91e8fead80cbe3a44240049527c5e621d06af06`. PR #336 is the authoritative post-Alpha integration vehicle.

## Scope

Cloud issue #332 implements the APC1 frictionless `/connect` onboarding overlay without changing pairing authority or hosted-bridge protocol contracts.

The promoted Alpha `dashboard.py` implementation remains unchanged. `connect_polish.py` contains the APC1 onboarding surface. The bounded `connect_polish_apc1.py` layer applies the exact-head review repairs and is the router registered by `app.py`. The layer is intentionally fail-closed: if its expected base markers disappear, import fails rather than silently serving a partially patched surface. This keeps the Alpha remediation isolated while avoiding a larger critical-path rewrite of the reviewed onboarding implementation.

Cloud #319 is included as a bounded recovery refinement: when Chromium exposes the `local-network-access` permission, `/connect` distinguishes a browser permission denial from a missing local daemon. `prompt` remains undecided rather than being treated as blocked. A denied permission produces browser guidance; an unanswered prompt still offers the normal Setup and Retry path with explicit Allow guidance.

## Governing principle

**Frictionless user entry.**

The browser detects the local daemon first, skips setup when it is already available, guides the user only when setup is required, and continues automatically after install/start while preserving explicit local approval.

Daemon/setup and pairing polling are sequential and bounded. Retry, cancel, and restart invalidate older polling generations so late responses cannot create duplicate pairing requests or resurrect cancelled state.

## Preserved Alpha semantics

The APC1 overlay preserves the promoted Alpha behavior for:

- distinct Cloud session diagnostics, including design-partner denial and temporary Cloud verification failure;
- typed pairing recovery for account mismatch, update-required/incompatible, tenancy, denied, expired, revoked, and unavailable states;
- explicit local approval as the only pairing authority;
- transient session handoff in `sessionStorage`;
- optional durable restore proof containing only request handle, poll proof, daemon URL, Cloud account, team, and expiry;
- restore invalidation on malformed, expired, or cross-account state;
- restore removal on sign-out;
- no durable Cloud access token in the restore record;
- the accepted `bicameral_daemon_url` override across probe, pairing, polling, and restore state.

## Verification and Actions budget

Browser tests are behavioral evidence, not a substitute for real-device acceptance.

The Hosted Dashboard Browser Gate is intentionally scoped to pull requests targeting `dev` or `main` that change Cloud runtime code, journey evidence, the dashboard vendor script, `pyproject.toml`, or the workflow itself. It does not run again on `push` to `dev`; the organization ruleset requires changes to `dev` to arrive through pull requests, so that second run duplicated the PR merge-result evidence.

The browser gate covers:

- vendored dashboard boot and responsive connect presentation;
- Local Network Access diagnosis and guided setup lifecycle;
- bounded setup polling and duplicate-pair prevention;
- custom daemon URL routing;
- the six-field pairing request contract;
- durable restore state without `cloud_access_token`;
- account-mismatch recovery;
- Windows troubleshooting commands that do not advertise Unix or unsupported service commands.

For exact-head verification without GitHub Actions spend, run from a prepared developer checkout:

```bash
python -m pip install -e '.[dev,journey]'
python -m playwright install chromium
pytest -q -m journey tests/journey/test_apc1_pairing_behavior.py
```

The full targeted browser set can be run locally with:

```bash
pytest -q -m journey tests/journey/test_vendored_dashboard_smoke.py tests/journey/test_apc1_local_network_permission.py tests/journey/test_apc1_setup_polling.py tests/journey/test_apc1_pairing_behavior.py
```

## Platform boundary

Promoted Alpha remains the accepted baseline. APC1 separately adds Windows platform enablement through:

- `BicameralAI/bicameral-bot#870` / PR #1318 for the Windows x64 release artifact and governed publication parity;
- `BicameralAI/bicameral-install#13` / PR #15 for the Windows installer/service bootstrap;
- Cloud #332 / PR #336 for the OS-aware browser onboarding handoff.

The Windows onboarding path must not be promoted before the governed Windows release asset is published, the installer is validated against the live manifest and deployed, and the Cloud successor passes qualified exact-head review or Jin's authoritative merge.

Real Windows install/start/detect/pair acceptance remains mandatory and is not satisfied by Linux-hosted Playwright evidence.

## Semantic impact

Cloud authority/protocol semantic impact: **none**.

The Local Network Access and setup refinements change browser diagnosis and orchestration only. They do not change pairing requests, local approval authority, daemon authority, tenancy, persistence authority, or network exposure.

The unrelated formatting-only `models/product_directory.py` delta from the old pre-promotion branch was intentionally dropped when PR #336 was recomposed on promoted `main`.
