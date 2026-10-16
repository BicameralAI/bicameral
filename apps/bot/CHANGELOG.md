# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project aims to follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **`workspace.resolve`** (mcp#831 / ADR-0023 SessionStart routing): local-only
  ToolRequest read that returns at most one registered `product_id` for an
  exact canonical host folder match against daemon-owned
  `LocalWorkspaceBinding` records. Typed `unbound` / `repair_required` /
  `ambiguous` / `unsafe_path` misses; never echoes `candidate_path`; never
  invents Product identity. Hosted-bridge routing is forbidden. Bind write
  semantics are unchanged.
- **Community-health gold-standard set** (parity with `bicameral-integrations`):
  `CONTRIBUTING.md`, `GOVERNANCE.md`, `SECURITY.md`, `CODE_OF_CONDUCT.md`, and this
  `CHANGELOG.md`.
- **Sibling registry** — `docs/governance/SIBLINGS.md` (with `docs/governance/README.md`):
  a tracked registry of leak-guarded local tools (Qor-logic, FailSafe, contributor tooling)
  that frees contributors to use their own tools without adopting the maintainer's process.
- Genesis governance: conservative secret scanner (`scripts/secret_scan.py`) wired into
  pre-commit and CI, and local-only governance documents (PR #184).

### Changed

- Generalized the three-layer governance boundary proposal
  (`docs/plans/governance-boundary-three-layer-proposal-2026-06-08.md`): the third layer is
  now a **sibling registry** rather than a single hard-coded FailSafe boundary, with a
  *sibling-autonomy* invariant and a registry-driven leak guard.

[Unreleased]: https://github.com/BicameralAI/bicameral-bot/commits/main
