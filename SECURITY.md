# Security Policy

## Status

This repository is private staging. It contains governance scaffolding and documentation only; **no Bicameral product component has been imported yet**, and no release is published from it. Vulnerabilities in the product today belong to the component repositories where that code currently lives.

## Reporting a vulnerability

- **Do not open a public issue, discussion or pull request for a suspected vulnerability.**
- While the repository is private, report privately to a maintainer listed in [MAINTAINERS.md](MAINTAINERS.md).
- Include what is affected, how to reproduce it, and the impact you expect. Do not include live credentials or customer data; describe them instead.

This document does not yet name a dedicated security mailbox, response-time commitment or disclosure timeline. Those will be added here when they are established and staffed, not before.

## Before public launch

These are required before the repository is made public:

- [ ] Enable GitHub **private vulnerability reporting** for this repository and document it here as the primary channel.
- [ ] Publish a verified security contact and the disclosure process.
- [ ] Enable secret scanning and push protection.
- [ ] Complete the publication-safety review required by [Factory #969](https://github.com/BicameralAI/bicameral-factory/issues/969).

## Controls in this repository

What is actually in place here:

- CI runs on `pull_request` with a read-only token (`contents: read`), no secrets, no dependency installation, and an action pinned to a full commit SHA, with checkout credentials not persisted.
- The governance check validates repository layout, code ownership and architecture records on every pull request.
- [GOVERNANCE.md](GOVERNANCE.md) requires every change to `main` to arrive by pull request with an independent code-owner approval. A repository ruleset enforcing this (required `governance` check, code-owner review, approval of the latest push, stale-approval dismissal, no force pushes or deletion, no bypass) is pending setup by a repository administrator.

## Compliance posture

Bicameral is **preparing for a SOC 2 Type 1 examination** covering the Security and Confidentiality criteria. **No SOC 2 examination has been completed and no SOC 2 report has been issued.** Bicameral does not claim SOC 2 compliance or certification. This section will change only when a report is actually issued.
