# Security Policy

## Reporting a vulnerability

Please report security vulnerabilities to **security@bicameral-ai.com**. Do not disclose
suspected vulnerabilities publicly until maintainers have had a reasonable opportunity to
investigate and coordinate remediation.

When possible, include: a description of the issue, the affected component
(daemon, gateway, adapter, mod, protocol, or event-store), reproduction steps, and the
expected versus observed behavior.

## Supported versions

This repository tracks the Bicameral Bot core before a tagged release line exists. Security
fixes are accepted against the default branch (`main`) unless a release branch is announced.

## Software supply chain

The public install path downloads prebuilt binary archives from
`https://pub-7eab94b264754bee8019fd89ca9b5b0b.r2.dev/bicameral`. GitHub Releases remain the
release evidence and mirror, but the installer must not depend on public access
to the private repository release endpoints.
Installer-compatible releases ship:

| Artifact | What it is |
|---|---|
| `bicameral-<target>.tar.gz` | Platform-specific `bicameral` binary archive |
| `SHA256SUMS` | SHA-256 checksums verified by `install.bicameral-ai.com/install.sh` |
| `latest.json` | Public manifest mapping platform targets to versioned release artifacts |

This v0 alpha-grade trust posture is checksum verification against the
versioned public artifact directory. Detached cosign/minisign signatures are the
next release hardening milestone. macOS notarization deferred until
browser-download or `.pkg` distribution path exists.

## Scope

Security reports are especially relevant when a change could:

- Write canonical decision artifacts directly, bypassing the daemon and governance routing
- Materialize state through a path other than reviewed adapters + replay (ADR-0007)
- Convert an advisory signal or mod warning into direct blocking authority
- Leak source payloads, tokens, secrets, customer data, or **sibling-tool artifacts** into a
  commit, log, or tracked reference
- Weaken local code grounding so claims are no longer evidence-backed
- Forge or bypass the factory attestation gate

The repository ships a conservative secret scanner (`scripts/secret_scan.py`) wired into
pre-commit and CI; a finding there is a hard gate, not a warning.

## Response

Maintainers will acknowledge reports, triage severity, and publish remediation details when
disclosure is appropriate. Credit is offered to reporters who follow coordinated disclosure
unless they prefer to remain anonymous.
