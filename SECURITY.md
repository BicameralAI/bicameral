# Security Policy

## Status

Bicameral is consolidating its components into this repository. No unified release has been published from this repository yet. Existing component security procedures remain applicable during migration.

## Reporting a vulnerability

- **Do not open a public issue, discussion or pull request for a suspected vulnerability.**
- Until GitHub private vulnerability reporting is enabled, contact a [maintainer](MAINTAINERS.md) privately. Do not include exploit details in public threads.
- Include what is affected, how to reproduce it, and the impact you expect. Do not include live credentials or customer data; describe them instead.

This document does not yet name a dedicated security mailbox, response-time commitment or disclosure timeline. Those will be added here when they are established and staffed, not before.

## Before accepting public security reports

The following security-reporting and publication checks must be completed before the public launch:

- [ ] Enable GitHub **private vulnerability reporting** for this repository and document it here as the primary channel.
- [ ] Publish a verified security contact and the disclosure process.
- [ ] Enable secret scanning and push protection.
- [ ] Complete the publication-safety review required by [Factory #969](https://github.com/BicameralAI/bicameral-factory/issues/969).

## Controls in this repository

What is actually in place here:

- The root `governance` pull-request workflow uses `contents: read`, a SHA-pinned checkout with `persist-credentials: false`, and runs the in-repository governance checker without installing dependencies.
- Other component verification workflows are mirrored at `.github/workflows/`. Their triggers, permission declarations, dependency installation, caches, and checkout credential behavior vary by workflow. Several install build/test dependencies; do not infer a uniform fork-safety or token policy from the governance workflow. These workflows must be assessed individually before accepting public fork contributions (see [Factory #971](https://github.com/BicameralAI/bicameral-factory/issues/971)).
- Release, deployment, publishing and public-contributor security hardening remain separate authorization/readiness steps. A successful component CI run is not evidence that a publication or release approval has been granted.
- The governance check validates repository layout, code ownership and architecture records on every pull request.
- [GOVERNANCE.md](GOVERNANCE.md) requires every change to `main` to arrive by pull request with an independent code-owner approval. A repository ruleset enforcing this (required `governance` check, code-owner review, approval of the latest push, stale-approval dismissal, no force pushes or deletion, no bypass) is pending setup by a repository administrator.

## Compliance posture

Bicameral is **preparing for a SOC 2 Type 1 examination** covering the Security and Confidentiality criteria. **No SOC 2 examination has been completed and no SOC 2 report has been issued.** Bicameral does not claim SOC 2 compliance or certification. This section will change only when a report is actually issued.
