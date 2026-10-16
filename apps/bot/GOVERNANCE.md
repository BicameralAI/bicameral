# Project Governance

## Project

Bicameral Bot — a local-first, grounded governance loop for engineering decisions.

## Maintainers

BicameralAI maintainers steward repository direction, review contributions, and preserve
the authority boundaries documented in [`README.md`](README.md) and [`docs/adr/`](docs/adr/).

## Decision making

Architecture decisions are recorded as ADRs in [`docs/adr/`](docs/adr/). The product
authority boundary is fixed: edges emit evidence and commands, the local daemon validates
policy, and canonical state materializes only through adapters and replay (ADR-0007).
Adapters and mods emit candidates, evidence, hints, signals, and advisories through
protocol-compatible paths; they must not write canonical state directly.

## Process governance (three layers)

Repository **process** governance is layered. The design and rationale live in the
[three-layer governance boundary proposal](docs/plans/governance-boundary-three-layer-proposal-2026-06-08.md);
the short version:

- **Shared process (bic-logic)** — factory-owned doctrine, consumed here by a pinned
  commit + hash. This is the **one mandatory layer**: the contract every PR must satisfy.
- **Sibling tools (registry)** — any local process, governance, or AI tooling a contributor
  uses is a *registered sibling*: leak-guarded, never tracked, never referenced. The
  registry is [`docs/governance/SIBLINGS.md`](docs/governance/SIBLINGS.md). The maintainer's
  own tool (Qor-logic) is itself a registered sibling, not a requirement on contributors.

Contributors are free to bring their own tooling — see
[`CONTRIBUTING.md`](CONTRIBUTING.md) → *Bring your own tools*.

This is repo/process governance only. It never produces product Decisions, gates, or
compliance outcomes; `protocol/`, governance policy, and event-store replay remain the only
product authority.

## Branch protection plan

The default branch should be protected with:

- Pull request review before merge
- Status checks for tests, linting, secret scanning, and (for product PRs) factory
  attestation
- Restrict force pushes on `main` and release branches
- Require branch lineage from `origin/main` before opening release PRs

## Contribution guidelines

See [`CONTRIBUTING.md`](CONTRIBUTING.md).

## Code of conduct

See [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).

## Security policy

See [`SECURITY.md`](SECURITY.md).
