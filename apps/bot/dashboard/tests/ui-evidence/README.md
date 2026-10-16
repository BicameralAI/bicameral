# Headed UI evidence harness

Reusable browser evidence for the hosted dashboard SPA.

## Why this exists

Several frontend PRs in this repo have been fully deterministic-green — vitest,
`svelte-check`, `vite build`, Rust workspace, governance, secret scan — and
still could not leave draft, because the open question was never "does the code
compile". It was "does the operator actually see the right thing, at desktop and
at phone width, without a console error or a clipped control". Copying bespoke
screenshot code into each PR would have answered that four times, badly. This is
the shared answer.

## What a passing run means

Each scenario drives the **real production bundle** (`vite build` + `vite
preview`) in a **real Chromium browser** at both required viewports:

| viewport | size |
| --- | --- |
| desktop | 1440x900 |
| narrow | 390x844 |

A capture only exists if every one of these held at that moment:

- no uncaught page error;
- no browser console error (allowlist in `support/harness.ts` is deliberately
  near-empty);
- no `/api/**` request the scenario did not explicitly model — an unmodelled
  call is a failure, not a silent pass;
- no horizontal overflow, and every declared primary control is present, sized,
  and inside the viewport;
- no harness marker value rendered into the DOM or the URL.

Screenshots are therefore captured on **success**, not only on failure. Traces
are retained for every test.

## What it does not mean

The harness proves rendering and interaction against **replayed** daemon and
Cloud wire shapes. It is not a substitute for terminal QA against a live daemon,
real Google OAuth, or a real Cloud tenant. A scenario that says "daemon-confirmed
connected" means the SPA correctly required and consumed a daemon confirmation —
not that a Google account was actually connected.

## Authority boundary

Everything under `tests/ui-evidence/` lives inside Playwright request
interception. It is never imported by `src/`, never bundled, and never served.
It cannot make a Product, Connection, credential, pairing, or Decision
canonical. Nothing here may become a production success path; if a scenario
seems to need that, the surface under test is wrong, not the harness.

Fixture values are obviously synthetic (`...-not-a-credential`) and every
capture asserts they never reach the rendered page.

## Running it

From `dashboard/`:

```bash
npm ci
npx playwright install chromium
npm run evidence:ui                 # headless Chromium
```

Headed (a visible window):

```bash
UI_EVIDENCE_HEADED=1 npm run evidence:ui        # bash
$env:UI_EVIDENCE_HEADED=1; npm run evidence:ui  # PowerShell
```

Output lands in `dashboard/evidence/ui/` (git-ignored):

```
evidence/ui/manifest.json                  SHA-bound machine-readable summary
evidence/ui/screenshots/<viewport>/*.png   success captures
evidence/ui/records/*.json                 per-capture records
evidence/ui/_playwright/                   traces
```

CI runs the same command under Xvfb in `.github/workflows/ui-evidence.yml`,
checks out the **PR head SHA** rather than the merge commit, and uploads
`evidence/ui` as `ui-evidence-<short-sha>` with 14-day retention.

## Writing a scenario

```ts
import { test, expect } from "./support/harness";
import { cloudMemberProject, HARNESS } from "./support/daemon-stub";

test("google drive shows action required until the daemon says otherwise", async ({ ui, page }) => {
  await ui.open({
    id: "drive-action-required",
    title: "Drive renders non-connected while the daemon registry says not_connected",
    daemon: {
      mcp: { state: "active_verified" },
      connectionRegistry: [{ status: "not_connected" }],
      memberships: [{ product_id: HARNESS.productId, setup_incomplete: false }],
      cloudProjects: cloudMemberProject(),
    },
    path: "/app",
  });

  await expect(page.getByTestId("hosted-connections")).toBeVisible();

  await ui.capture({
    name: "01-action-required",
    assertions: ["Google Drive is not presented as connected."],
    primaryControls: ['[data-testid="hosted-drive-oauth"]'],
  });
});
```

Array-valued scenario fields (`connectionRegistry`, `connectionStatus`, ...) are
consumed one per call with the last entry repeating, which models polling
without a timing race.

Provider handoffs that call `window.open` are recorded rather than followed, so a
run never navigates to a third-party site; assert the attempted URL with
`await ui.windowOpenCalls()`.

Use `ui.expectNeverCalled(pattern, why)` to assert a contract hop was not
invoked — for example that a provider repair resumed the same Product instead of
replaying `product.join`.
