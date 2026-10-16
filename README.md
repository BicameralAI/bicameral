<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/branding/bicameral-lockup-dark.svg">
    <img src="docs/assets/branding/bicameral-lockup-light.svg" alt="Bicameral" width="344" height="68">
  </picture>
</p>

<h3 align="center">The decision ledger for engineering and product teams</h3>

<p align="center">
  Capture the decisions that constrain your code, ground them in the code itself,<br>
  and keep a human in the loop before any claim becomes authority.
</p>

<p align="center">
  <a href="https://github.com/BicameralAI/bicameral/actions/workflows/governance.yml"><img src="https://github.com/BicameralAI/bicameral/actions/workflows/governance.yml/badge.svg?branch=main" alt="governance check"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-0891b2" alt="License: Apache-2.0"></a>
  <a href="#current-status"><img src="https://img.shields.io/badge/status-OSS%20transition-71717a" alt="Status: open-source transition"></a>
  <a href="docs/architecture/INDEX.md"><img src="https://img.shields.io/badge/architecture-ARCH%20records-0891b2" alt="Architecture: ARCH records"></a>
  <br>
  <a href="SECURITY.md"><img src="https://img.shields.io/badge/security-policy-0891b2" alt="Security policy"></a>
  <a href="SECURITY.md#compliance-posture"><img src="https://img.shields.io/badge/SOC%202%20Type%201-in%20preparation-ea580c" alt="SOC 2 Type 1: in preparation"></a>
  <a href="tools/governance/"><img src="https://img.shields.io/badge/tooling-Python%203.10%2B%20stdlib-3f3f46" alt="Governance tooling: Python 3.10+ standard library"></a>
</p>

<p align="center">
  <a href="#how-bicameral-works"><b>How it works</b></a> ·
  <a href="docs/architecture/"><b>Architecture</b></a> ·
  <a href="GOVERNANCE.md"><b>Governance</b></a> ·
  <a href="SECURITY.md"><b>Security</b></a> ·
  <a href="CONTRIBUTING.md"><b>Contributing</b></a>
</p>

> [!IMPORTANT]
> **Open-source consolidation in progress.** This repository is being prepared as the public source home for Bicameral. The Bicameral components have been consolidated here from their individual repositories, each with its layout unchanged. A verified unified release is still in progress: releases and deployments still run from the original repositories, and an installable consolidated product is not yet available here. Progress is tracked in [Factory #969](https://github.com/BicameralAI/bicameral-factory/issues/969).

## Why Bicameral

Teams rarely fail because nobody wrote a decision down. They fail because product intent, architectural constraints and implementation drift apart while work is happening. The decision is in a ticket, the rationale is in a meeting, and the agent writing the code sees only a slice of it, then proceeds confidently.

Bicameral treats those decisions as first-class, reviewable records:

- **Grounded, not just documented.** Decisions link to source excerpts and to the code, symbols and diffs they constrain.
- **Authority is enforced, not asserted.** Integrations and agents can *propose* candidates and evidence. Only a reviewed transition in the local runtime makes something canonical.
- **Local first.** The most sensitive context (code, branches, conversations) stays at your team's trust boundary. Hosted services are optional and advisory.
- **Auditable.** Canonical decisions are replayable, attributable and reviewable.

## How Bicameral works

Bicameral is one product made of separately testable components with a strict authority boundary:

```mermaid
flowchart LR
    subgraph edge["Edge: proposes"]
        I["Integrations<br/>Jira · Linear · Slack · meetings"]
        M["MCP<br/>agent sessions"]
    end
    subgraph local["Your trust boundary: decides"]
        R["Local runtime<br/>validation · review · governance"]
        L[("Decision ledger")]
        D["Dashboard"]
    end
    C["Cloud (optional)<br/>advisory code-graph services"]

    I -- "evidence, candidates" --> R
    M -- "typed tool requests" --> R
    R -- "reviewed decisions" --> L
    D -. "review" .-> R
    R -. "advisory queries, with consent" .-> C
```

| Component | Role | Authority |
| --- | --- | --- |
| **Local runtime** | Daemon and gateway. Validates typed objects, applies governance policy, grounds claims in local code, keeps review state and the audit trail. | **Owns canonical state** |
| **Dashboard** | Human review surface for candidates and decisions. | Acts through the runtime |
| **MCP** | Agent-facing tools that map agent actions to typed requests. | Proposes only |
| **Integrations** | Provider-neutral adapters that turn source activity into evidence and candidates. | Proposes only |
| **Cloud** (optional) | Hosted advisory services, with explicit consent and tenant separation, plus a self-host path. | Advisory only |
| **Installer** | Installation and distribution. | None |

This is the intended architecture of the consolidated repository. Each boundary will be recorded as an architecture record and enforced by tests as the components arrive.

## Current status

| Area | State |
| --- | --- |
| Repository governance: structure manifest, CODEOWNERS generation, architecture records, CI check | **In place** in this repository |
| Foundational architecture records [ARCH-0001](docs/architecture/records/ARCH-0001-architecture-records-and-canonical-index.md) and [ARCH-0002](docs/architecture/records/ARCH-0002-repository-structure-and-structure-manifest.md) | **Accepted**; they govern architecture records, the architecture index, repository layout and code ownership |
| License | **Apache-2.0** authorized for the whole product (decision J-03) |
| Product components (runtime, dashboard, MCP, integrations, cloud, installer) | **Imported** as source snapshots with their existing build and test procedures; see the [migration inventory](migration/README.md) |
| Releases, deployments, public contributions | **Not from this repository yet.** Release and deploy workflows remain inactive here until moved with authorization |

## Getting started

Each component builds and tests as it did in its own repository, from its own directory:

```bash
git clone https://github.com/BicameralAI/bicameral.git
cd bicameral

(cd apps/bot && cargo test --workspace --locked)              # local runtime (Rust)
(cd apps/bot/dashboard && npm ci && npm test)                 # dashboard
(cd packages/mcp && pip install -r requirements-ci.lock && pip install --no-deps . && pytest tests/)
(cd services/cloud && pip install -e '.[dev]' && pytest)      # cloud
python3 tools/governance/check.py                             # repository governance checks
```

Each component's README has its full development guide, and its exact CI commands are listed in the [migration inventory](migration/README.md). End-user installation uses the published installer in [`installer/`](installer/).

## Repository structure

The layout is declared in [`governance/structure.json`](governance/structure.json), the single source of truth for where files may live and who owns them. [`.github/CODEOWNERS`](.github/CODEOWNERS) is generated from it.

| Path | Contents | Source repository |
| --- | --- | --- |
| [`apps/bot/`](apps/bot/) | Local runtime: Rust daemon, gateway, protocol, dashboard | `bicameral-bot` |
| [`packages/mcp/`](packages/mcp/) | MCP tool surface | `bicameral-mcp` |
| [`packages/integrations/`](packages/integrations/) | Source adapters and mods | `bicameral-integrations` |
| [`services/cloud/`](services/cloud/) | Optional hosted service and self-host configuration | `bicameral-cloud` |
| [`installer/`](installer/) | Installation scripts and install site | `bicameral-install` |
| [`docs/architecture/`](docs/architecture/) | Architecture records and the generated index | |
| [`governance/`](governance/) | Structure manifest | |
| [`tools/governance/`](tools/governance/) | Governance validators and tests | |
| [`migration/`](migration/) | Provenance for each imported component | |

Components keep their own internal layout, including their own `docs/` and pre-consolidation `docs/adr/` records. Repository-level architecture records live in `docs/architecture/records/`.

## Architecture and governance

- **Architecture records.** Decisions, designs and contracts are `ARCH-NNNN` records in [`docs/architecture/records/`](docs/architecture/records/), the only permitted location. [How records work →](docs/architecture/README.md)
- **Proposed versus accepted.** Only an *accepted* record carries authority, and each topic has at most one. *Proposed* records express intent and decide nothing. Writing something in this repository does not make it accepted.
- **The index is the answer.** [`docs/architecture/INDEX.md`](docs/architecture/INDEX.md) (machine-readable: [`index.json`](docs/architecture/index.json)) is generated from the records and shows which accepted record governs each topic.
- **Decision ownership.** Jin Kuan is the decision owner; maintainers review and merge within accepted policy, and nobody approves their own pull request. [Governance model →](GOVERNANCE.md)

## Security and trust

- **Reporting.** Do not file vulnerabilities as public issues. Follow [SECURITY.md](SECURITY.md).
- **CI.** The `governance` check runs on every pull request with a read-only token, no secrets, no dependency installation and a commit-pinned checkout action.
- **Review.** [GOVERNANCE.md](GOVERNANCE.md) requires every change to `main` to arrive by pull request with an independent code-owner approval. Enforcement by a repository ruleset is pending setup by a repository administrator.
- **SOC 2.** Bicameral is **preparing for a SOC 2 Type 1 examination** (Security and Confidentiality). No examination has been completed, no report has been issued, and Bicameral does not claim SOC 2 compliance or certification. [Details →](SECURITY.md#compliance-posture)

## Development and testing

```bash
python3 tools/governance/check.py           # what CI runs: structure, architecture, unit tests
python3 tools/governance/check.py --write   # regenerate CODEOWNERS and the architecture index, then validate
python3 -m unittest discover -s tools/governance/tests -t tools/governance   # unit tests only
```

Generated files are never edited by hand. Change `governance/structure.json` or a record, then run `--write` and commit the result. Each component builds and tests from its own directory (see [Getting started](#getting-started)), and CI mirrors each component's existing test workflows at the repository root.

## Contributing

Bicameral is not accepting public contributions yet. The rules that will apply are already in force for maintainers:

- [CONTRIBUTING.md](CONTRIBUTING.md): checks, file placement, architecture records, pull request evidence
- [GOVERNANCE.md](GOVERNANCE.md): roles, review rules, recorded and pending decisions
- [MAINTAINERS.md](MAINTAINERS.md) and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) (Contributor Covenant 2.1)

Whether outside contributions will use a DCO sign-off or a CLA (decision J-09) is still pending.


## Organizational contributors

**WhatBytes** contributes engineering, testing, and maintenance support to Bicameral through Aritro Roy and Mayur Sonar. See [Maintainers and continuing contributors](MAINTAINERS.md#organizational-contributors) for recognition and governance boundaries. Organizational participation does not confer repository or product authority.

## License and attribution

Bicameral is licensed under the [Apache License 2.0](LICENSE), the license authorized for the entire consolidated product.

Components are imported as curated source snapshots with their provenance and any third-party notices preserved, without importing unrelated source histories. Navigator, Atlas, FinOps, TARA, related Qortara/Cortera development-support products and MythologIQ's Event Radar are separate products. They are not part of Bicameral and are not licensed by this repository.

The Bicameral name and logo are brand assets of Bicameral AI; see [`docs/assets/branding/`](docs/assets/branding/) for their source and usage rules.
