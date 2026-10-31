# Contributing to Bicameral

## Current status

Bicameral is preparing its public open-source repository and contribution process. During the transition, changes are coordinated by the [maintainers](MAINTAINERS.md) and invited contributors. **Unsolicited external pull requests are not yet being accepted.** The rules below also apply to current contributions.

The Bot, MCP, Integrations, Cloud and Installer components are imported as source snapshots, each in its own directory with its own build and tests; [`migration/README.md`](migration/README.md) records their sources and every migration change. Do not copy source from other Bicameral repositories into this tree except through an authorized import recorded in `migration/sources.json`.

## Local checks

You need Python 3.10 or newer and Git. Nothing is installed.

```bash
python3 tools/governance/check.py           # what CI runs; never modifies files
python3 tools/governance/check.py --write   # regenerate generated files, then validate
```

Component checks are the commands under [Getting started](README.md#getting-started), and exactly the `run:` steps of the root workflow for that component in [`.github/workflows/`](.github/workflows/) (`bot-*.yml`, `mcp-*.yml`, `integrations-*.yml`, `cloud-*.yml`, `installer-*.yml`). None needs Bicameral credentials.

The check runs, in order:

1. **Structure**: `tools/governance/check_structure.py` validates `governance/structure.json` and every file Git would commit, and checks that `.github/CODEOWNERS` is current.
2. **Architecture**: `tools/governance/check_architecture.py` validates records in `docs/architecture/records/` and checks that `docs/architecture/INDEX.md` and `index.json` are current.
3. **Unit tests**: everything in `tools/governance/tests/`.

To run only the tests:

```bash
python3 -m unittest discover -s tools/governance/tests -t tools/governance
```

Generated files (`.github/CODEOWNERS`, `docs/architecture/INDEX.md`, `docs/architecture/index.json`) are never edited by hand. Change the source and run `--write`.

## Where files go

[`governance/structure.json`](governance/structure.json) decides where files may live.

- **Root files** must be declared in `root_files`.
- **Directories** are declared in `directories`. A `declared` directory only allows files directly inside it plus subdirectories that are themselves declared. An `open` directory allows any layout beneath it.
- **Reserved** directories hold only their README until an authorized import marks them `active` in the same pull request that brings in the code and its provenance record.
- Declared directories need a `README.md` describing their purpose unless the manifest sets `"readme": false`.
- Architecture records live only in `docs/architecture/records/`. Directories named `adr`, `adrs`, `decisions`, `architecture-decisions` or `design-docs` are rejected anywhere, except the pre-consolidation decision directories of imported components that the manifest lists as `legacy_decision_dirs`. Those directories are frozen: `governance/legacy-decisions.json` locks their contents. To build on a legacy record, first rewrite it as an `ARCH-NNNN` record (ARCH-0001).
- Symbolic links are rejected.

To add a location, add it to the manifest (parents before children), add its README, run `--write` so CODEOWNERS is regenerated, and explain the change in the pull request. Manifest changes are high-risk changes under [GOVERNANCE.md](GOVERNANCE.md).

## Architecture records

1. Copy [`docs/architecture/TEMPLATE.md`](docs/architecture/TEMPLATE.md) to `docs/architecture/records/ARCH-NNNN-short-slug.md`, using the next unused number.
2. Fill in the front matter. Start with `status: proposed`.
3. List every other record the text mentions in `depends_on`, `supersedes` or `references`; the check rejects undeclared `ARCH-NNNN` mentions.
4. Run `python3 tools/governance/check.py --write` and commit the regenerated index.

**Acceptance** is a separate, reviewed change that sets `status: accepted` and needs decision owner approval. An accepted record may only depend on accepted records.

**Supersession**: write a new record listing the old one in `supersedes`. While the new record is `proposed`, the old one stays `accepted`; that is a valid intermediate state. When the new record is accepted, the same pull request sets the old one to `superseded`. Reverse links (`required_by`, `superseded_by`) are generated into the index and are never written into records.

Full rules: [`docs/architecture/README.md`](docs/architecture/README.md).

## Pull request evidence

Use the [pull request template](.github/pull_request_template.md). Every pull request states:

- Scope and the related issue.
- Architecture impact, naming any record it relies on, proposes or changes.
- Security implications.
- The checks you ran and their result.
- Whether generated files changed and were regenerated with `--write`.
- For any imported or adapted material: source repository, commit SHA, license and the approval that authorizes it.

## Inbound contribution terms

Contributions use the [Developer Certificate of Origin](https://developercertificate.org/) (DCO), decision **J-09**. There is no Contributor License Agreement.

Every commit in a pull request must carry a `Signed-off-by` line with your real name and an email you control. `git commit -s` adds it. By signing off, you certify the DCO 1.1 for that commit: you wrote it or have the right to submit it under Apache-2.0. Reviewers check for sign-off before approving.

## Issues

Every issue follows the standard in [#47](https://github.com/BicameralAI/bicameral/issues/47): a summary, a scope, and a checks table whose rows each name a setup, an action, an expected result and a result.

**Backlog.** An issue with no assignee and the `help wanted` label is open to take. Comment on it to claim it; a maintainer assigns you. Anyone can claim one; commits must be signed off under the DCO (see [Inbound contribution terms](#inbound-contribution-terms)).

**What needs no further authorization.** An organization member may propose and merge any change that leaves product behavior unchanged: tests, fixtures, harnesses, documentation, audits and contract inventories. Membership confers this; an issue does not need to grant it. A change to product behavior also needs the record that governs it to be accepted first. In an issue, rows that depend on that record are marked `BLOCKED` until it is accepted. Either way, the review rules below apply.

## Review and merge

- All changes reach `main` through a pull request.
- At least one code owner other than the author must approve. Authors never approve their own pull requests.
- New commits dismiss earlier approvals; reviewers approve the final commit.
- The `governance` check must pass.
- High-risk changes (see [GOVERNANCE.md](GOVERNANCE.md)) also need decision owner approval.
- Prefer small pull requests with one purpose.


## Organizational contributions

Companies and independent developers contribute under the same repository review, security, provenance, and licensing requirements. Organizational acknowledgment, including [WhatBytes](MAINTAINERS.md#organizational-contributors), does not grant GitHub permissions, code-owner status, independent merge authority, or exemptions from the contribution process. Formal maintainer appointments and access grants are separate decisions governed by [GOVERNANCE.md](GOVERNANCE.md).

## Conduct

Participation is covered by the [Code of Conduct](CODE_OF_CONDUCT.md).
