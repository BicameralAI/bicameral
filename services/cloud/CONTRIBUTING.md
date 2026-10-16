# Contributing to Bicameral Cloud

Thank you for considering a contribution. Bicameral Cloud is the private hosted
optimization layer for Bicameral: organization-scale code graph, conflict oracle, expensive
indexing, and grounding optimization. It returns evidence, signals, suggestions, and
advisories — **it is not the canonical decision store** (ADR-0001). Contributions must
preserve that boundary.

## How to contribute

1. Create a topic branch for your change.
2. Keep behavior aligned with the authority boundaries in `README.md` and `docs/adr/` —
   Cloud emits `ConflictSignal`, `GroundingSuggestion`, `CodeGraphEvidence`,
   `DependencyRisk`, and advisory `GovernanceResult` objects; it never owns canonical state.
3. Record architecture decisions as ADRs in `docs/adr/`.
4. Add or update tests when code behavior changes.
5. Run the relevant checks for the surface you touched before opening a pull request.
6. Open a pull request against the default branch describing the change and its evidence.

## Bring your own tools — the sibling registry

You are **not** required to adopt the maintainer's process tooling. The only thing your PR
must satisfy is the shared **bic-logic** contract plus a clean working tree. Everything you
run *locally* — a governance system, an AI assistant, an IDE plugin, or a homegrown
framework — is welcome as a **registered sibling**: leak-guarded, never tracked, never
referenced. This matters especially here: Cloud handles organization-scale code context, so
no local tool's scratch may ever leak into the repo.

To use your own tool: add a row to [`docs/governance/SIBLINGS.md`](docs/governance/SIBLINGS.md),
add its scratch root to `.gitignore`, and keep its artifacts out of tracked files.

## Issue reports

Describe the issue, the affected surface (code graph, conflict oracle, grounding), expected
behavior, and any relevant protocol contracts.

## License

By contributing, you agree that your contributions are licensed under the license that
governs this repository.
