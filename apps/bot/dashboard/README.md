# Bicameral Dashboard

Svelte 5 + Vite single-page app for the Bicameral dashboard.

The dashboard has one runtime shape: Cloud serves the SPA and pairs it with the
operator-local daemon, which returns real redacted projections. For local or
staging validation, use the disposable local production Cloud process from
[`bicameral-cloud#135`](https://github.com/BicameralAI/bicameral-cloud/issues/135).
There is no daemon-served SPA or fixture-backed developer runtime.

## Prerequisites

- Node 22+ (`require(ESM)` support is needed by the jsdom-based test toolchain;
  the tests fail to start on Node 20). Use `nvm use 22` or equivalent.
- `npm install`

## `npm run dev`

```bash
npm install
npm run dev
```

`npm run dev` runs Vite for frontend iteration, but the application remains in
hosted mode and requires real Cloud/daemon handoff state. It does not install a
fetch shim or synthesize projects, setup, ledger, or connection responses.

## Commands

```bash
npm run dev    # hosted runtime; pair through a real local/remote Cloud process
npm run build  # production bundle vendored and served by Cloud
npm run check  # svelte-check type/diagnostics
npm test       # vitest (Node 22+)
```
