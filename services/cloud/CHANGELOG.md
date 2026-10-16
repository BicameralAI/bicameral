# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project aims to follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Governance community-health set: `CONTRIBUTING.md`, `GOVERNANCE.md`, `SECURITY.md`,
  `CODE_OF_CONDUCT.md`, and this `CHANGELOG.md`.
- **Sibling registry** — `docs/governance/SIBLINGS.md` (+ `README.md`): a tracked registry
  of leak-guarded local tools, propagating the generalized governance boundary from
  `bicameral-bot`. Contributors may use their own tooling as a registered sibling; the only
  mandatory layer is the shared bic-logic contract.
- `.gitignore` covering agent-scratch / sibling-tool roots.
- Factory governance adoption: `.bicameral/repo-governance.yaml` (classification `product-critical`,
  governed by `bicameral-factory`) with `required_checks: [sbom, factory-attestation]` — the checks
  that run on this private repo today. `sbom` (org reusable) and `factory-attestation` (cross-repo
  validator, reusing the org `BICAMERAL_CROSS_REPO_TOKEN` for the read-only factory checkout) workflows.
  CodeQL, dependency-review, and Scorecard SARIF upload require GitHub Advanced Security and are
  deferred as a follow-up gated on GHAS enablement (owner decision on #101); tracked in
  `docs/governance/soc2-application-controls.md`.

### Security

- Fixed a data-plane access-control gap: `public.design_partner_allowlist` had RLS enabled but no
  policy/GRANT and no service-role marker, so the factory data-API ACL gate flagged `missing-grant`.
  Marked the table explicitly service-role-only — its intended access per the table's own comment —
  clearing the gate.
