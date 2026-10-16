# ADR-0027: Cloud owns SPA serving; the daemon is API-only

Status: Accepted

## Decision

Bicameral Cloud is the only supported host for the dashboard SPA. This applies
to production and to local/staging validation: the disposable production Cloud
process from `BicameralAI/bicameral-cloud#135` builds and serves the bot SPA at
`/app`, while the operator-local daemon exposes loopback API, pairing, hosted
bridge, and authority-bearing runtime behavior.

The daemon does not locate, bundle, or serve `index.html` or static assets. The
hosted dashboard adapter requires a valid bridge session and fails closed with
the existing typed bridge error; it never turns an unpaired request into a
fixture-backed success. Vite development uses the same hosted runtime and no
longer installs a fixture fetch shim.

The `dashboard/` source remains in this repository because Cloud vendors that
product artifact. Bot release archives and self-update payloads contain only
the daemon binary.

## Consequences

- `bicameral gateway start`, the service command, and the deprecated
  `bicameral dashboard` alias start the same API-only daemon process.
- Local UI validation must start or consume a Cloud process; no deployed
  staging service is required.
- Missing Cloud, pairing, or daemon state stays visibly unavailable instead of
  rendering fixture data.
- Existing Product, ProjectRegistration, LocalWorkspaceBinding, MCP, Cloud,
  event-store, and authority semantics do not change.

## Supersedes

This supersedes the SPA-bundling and daemon-static-serving portions of ADR-0013
and ADR-0023. Their installer, setup, service, and daemon lifecycle decisions
remain in force where they do not require the daemon to host the SPA.
