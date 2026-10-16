# Governance

This document describes Bicameral's interim governance during its open-source consolidation. It will be updated through recorded decisions before general community contributions are accepted (see [Pending decisions](#pending-decisions)).

The authoritative program record is [Factory #969](https://github.com/BicameralAI/bicameral-factory/issues/969).

## Roles

| Role | Holder | Authority |
| --- | --- | --- |
| **Decision owner** | Jin Kuan ([@jinhongkuan](https://github.com/jinhongkuan)) | Final say on product scope, licensing, governance policy, architecture acceptance, publication and releases. |
| **Maintainer** | Kevin Knapp ([@Knapp-Kevin](https://github.com/Knapp-Kevin)) | Reviews and merges changes within accepted policy; maintains repository tooling, CI and settings. |

The current roster is in [MAINTAINERS.md](MAINTAINERS.md). Adding or removing a maintainer is a decision owner decision and is recorded by a reviewed change to that file.

### What maintainers do

- Review pull requests for correctness, security, provenance and fit with accepted architecture.
- Keep the governance checks, CI workflow and generated files healthy.
- Propose architecture records and policy changes.
- Escalate high-risk changes (below) to the decision owner.

### What maintainers do not do

- Approve or merge their own pull requests.
- Accept an architecture record, change the license, change repository visibility, import component source, or publish a release on their own authority.
- Bypass required reviews or status checks, except to repair a broken protection configuration, and then only with the decision owner's recorded agreement.

## Product governance versus repository administration

These are separate on purpose.

- **Product governance** decides *what is true* for Bicameral: architecture, contracts, licensing, scope, and what ships. It is recorded in this repository through reviewed changes: accepted architecture records, this document, and the license file.
- **Repository administration** is the GitHub configuration that *enforces* those decisions: rulesets, required checks, CODEOWNERS, access levels. Holding administrator access confers no product decision authority. Settings changes must implement a recorded decision, not create one.

## Architecture decisions

Architecture is recorded as `ARCH-NNNN` records in [`docs/architecture/records/`](docs/architecture/records/). The rules are in [`docs/architecture/README.md`](docs/architecture/README.md) and are enforced by `tools/governance/check_architecture.py`.

- A record is **proposed** until a reviewed pull request changes it to **accepted**. Accepting a record requires decision owner approval.
- Only accepted records carry authority, and each topic has at most one accepted record.
- Changing accepted architecture means writing a new record that supersedes the old one. Accepted records are not rewritten in place except for typo-level corrections.
- The generated [architecture index](docs/architecture/INDEX.md) is the source for "what currently governs this topic".

## Pull request review

- Every change to `main` goes through a pull request.
- Every pull request needs approval from at least one code owner (see `.github/CODEOWNERS`) **who is not the author**. Self-approval does not count.
- Approvals apply to the reviewed commit. New commits dismiss earlier approvals.
- The `governance` status check must pass.
- Reviewers confirm the pull request template's evidence sections, including provenance for any imported material.

These rules are enforced by repository rulesets where GitHub supports them, and by reviewer discipline where it does not.

## High-risk changes

The following require explicit decision owner approval on the pull request, in addition to normal review:

- License, notice or copyright changes.
- Accepting, superseding or deprecating an architecture record.
- Changes to this document, `MAINTAINERS.md`, `SECURITY.md`, `governance/structure.json`, `tools/governance/`, or `.github/`.
- Importing component source (activating a reserved directory) or migration provenance records.
- Anything affecting repository visibility, release, deployment, credentials, or contributor eligibility.

When the decision owner is also the author, the change still needs an independent maintainer approval, and the decision owner's authorship is noted in the pull request.

## Escalation

If reviewers disagree, or a change's risk classification is unclear, the pull request is held and the question is raised to the decision owner on the pull request or in Factory #969. Security-sensitive matters follow [SECURITY.md](SECURITY.md) and are not discussed in public threads.

## Recorded decisions

| ID | Decision | Status | Record |
| --- | --- | --- | --- |
| **J-03** | Outbound license for the open-source product is **Apache-2.0 only**, covering Bot, Cloud, MCP, Integrations and this repository. | **Approved** (2026-10-15, Jin Kuan) | [Factory #969 comment](https://github.com/BicameralAI/bicameral-factory/issues/969#issuecomment-6199511106), [correction](https://github.com/BicameralAI/bicameral-factory/issues/969#issuecomment-6199520464) |

The copyright-holder permissions needed to carry MCP and Integrations into the Apache-2.0 tree are recorded as secured. That is not reopened here; the migration record for each import documents the provenance.

## Pending decisions

The following are open. Until each is recorded, nothing in this repository should be read as deciding it.

| ID | Topic | Status |
| --- | --- | --- |
| J-08 | Governance model at launch (founder-reserved, maintainer-led, elected) | Pending |
| J-09 | Inbound contribution terms: DCO or CLA | Pending |
| J-10 | Review thresholds for routine versus high-risk paths | Pending |
| J-11 | Use of contributor reputation signals | Pending |
| J-12 | Git history publication approach | Pending |
| J-13 | Publication go/no-go authority | Pending |
| J-14 | Community governance progression, trademarks, commercial hosting | Pending |

The current review rule above (one independent code-owner approval) is an interim floor, not a decision on J-10.
