# Project Governance

## Project

Bicameral Cloud — the private hosted optimization layer (code graph, conflict oracle,
indexing, grounding optimization).

## Maintainers

BicameralAI maintainers steward repository direction, review contributions, and preserve
the authority boundaries documented in `README.md` and `docs/adr/`.

## Decision making

Architecture decisions are recorded as ADRs in `docs/adr/`. The foundational boundary
(ADR-0001) is that Cloud is an **oracle, not an authority**: it returns evidence, signals,
suggestions, and advisories; the local bot owns workflow, review, and materialization. Cloud
must never become the canonical decision store.

## Process governance (three layers)

Repository **process** governance is layered:

- **Shared process (bic-logic)** — factory-owned doctrine, owned upstream in
  `bicameral-factory` and consumed here. This is the **one mandatory layer**: the contract
  every PR must satisfy.
- **Sibling tools (registry)** — any local process, governance, or AI tooling a contributor
  uses is a *registered sibling*: leak-guarded, never tracked, never referenced. The registry
  is [`docs/governance/SIBLINGS.md`](docs/governance/SIBLINGS.md). The maintainer's own tooling
  is itself a registered sibling, not a requirement on contributors.

Contributors are free to bring their own tooling — see `CONTRIBUTING.md` → *Bring your own
tools*. This is repo/process governance only; it never produces product Decisions, gates, or
compliance outcomes.

## Branch protection plan

The default branch should be protected with:

- Pull request review before merge
- Status checks for tests and secret scanning
- Restrict force pushes on `main` and release branches

## Contribution guidelines

See `CONTRIBUTING.md`.

## Code of conduct

See `CODE_OF_CONDUCT.md`.

## Security policy

See `SECURITY.md`.
