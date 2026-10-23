# Security Policy

## Status

Bicameral's components are consolidated in this repository. No unified release has been published from it yet. This policy supersedes the `SECURITY.md` files inside component directories.

## Reporting a vulnerability

- **Do not open a public issue, discussion or pull request for a suspected vulnerability.**
- Use GitHub's private vulnerability reporting where available; otherwise contact a [maintainer](MAINTAINERS.md) privately. The October 23 OSS record reports private vulnerability reporting enabled, but the reporting entry point and named response contact still need independent verification. Do not include exploit details in public threads.
- Include what is affected, how to reproduce it, and the impact you expect. Do not include live credentials or customer data; describe them instead.

This document does not yet name a dedicated security mailbox, response-time commitment or disclosure timeline. Those will be added here when they are established and staffed, not before.

## Public repository reporting and verification

The repository is intentionally public. The following checks remain open for reliable public security intake and post-publication qualification; they are **not** prerequisites for a new visibility transition:

- [ ] Verify GitHub **private vulnerability reporting** is accessible and document its usable entry point (reported enabled on October 23).
- [ ] Publish a verified security contact and the disclosure process.
- [ ] Verify secret scanning and push protection in the actual GitHub settings (reported enabled on October 23).
- [ ] Complete the post-publication rights, history and contributor-security evidence tracked in [#22](https://github.com/BicameralAI/bicameral/issues/22).

## Controls in this repository

What is actually in place here:

- The root `governance` pull-request workflow uses `contents: read`, a SHA-pinned checkout with `persist-credentials: false`, and runs the in-repository governance checker without installing dependencies.
- Component verification workflows are mirrored at `.github/workflows/`. The root CI verification workflows use a read-only contents token and GitHub-hosted runners, do not reference repository secrets, and do not use `pull_request_target` or `workflow_run` events. CodeQL additionally requests `security-events: write` for scan uploads. The **separate installer Pages workflow** may request `pages: write` and `id-token: write` to publish the installer site **from protected push/manual triggers, not pull requests**; it is not a product or binary-release publisher. The publication preflight enforces these narrowly scoped exceptions and rejects the specifically enumerated privileged actions/workflow paths, but a static workflow scan is not proof of effective GitHub ruleset enforcement. Several CI workflows install build and test dependencies, some actions are pinned by tag rather than commit, and most do not set `persist-credentials: false`. Nested Cloud/Bot/MCP deploy, publish, Pages, Slack and privileged-trigger workflow copies have been removed from the public repository; GitHub runs only the root workflows. The private product repositories keep binary releases and product deployments under separate authority until their approved transition.
- Release, deployment, publishing and public-contributor security hardening remain separate authorization/readiness steps. A successful component CI run is not evidence that a publication or release approval has been granted.
- The governance check validates repository layout, code ownership and architecture records on every pull request.
- [GOVERNANCE.md](GOVERNANCE.md) requires pull requests and independent code-owner approval. The October 23 OSS move record reports a `main` ruleset with the `governance` check, code-owner review, approval of the latest push, stale-review dismissal and merge protections enabled. The ruleset's effective settings and bypass actors require an authenticated settings export plus independent negative tests under [Factory #971](https://github.com/BicameralAI/bicameral-factory/issues/971); a successful CI run alone cannot prove merge enforcement.
- **Known contributor CI qualification blockers (October 23):** CodeQL advanced analysis conflicts with GitHub's enabled default CodeQL setup and fails on SARIF processing. A repository security administrator must select a single supported setup, preferably advanced configuration for the current multi-language workflow, and verify a subsequent successful run. The MCP dependency audit identified vulnerable `certifi` and `virtualenv` pins; remediation is submitted separately and requires a fresh lockfile install/audit. Do not misrepresent advisory dependency review or pending adversarial checks as required green gates.

## Compliance posture

Bicameral is **preparing for a SOC 2 Type 1 examination** covering the Security and Confidentiality criteria. **No SOC 2 examination has been completed and no SOC 2 report has been issued.** Bicameral does not claim SOC 2 compliance or certification. This section will change only when a report is actually issued.
