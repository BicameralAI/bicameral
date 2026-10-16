# Component migration inventory

Consolidation of the Bicameral product repositories into this repository, tracked in [Factory #969](https://github.com/BicameralAI/bicameral-factory/issues/969). This is a relocation: each component keeps its internal layout, interfaces, dependencies and behavior. Only mechanical changes required by the move are made. The machine-readable record is [`sources.json`](sources.json).

Each component was copied from the tracked files of its default branch at the SHA below, as a source snapshot without Git history. All components are distributed under the repository's [Apache-2.0 license](../LICENSE) (decision J-03), so per-component license files were not carried over. Bot's third-party attribution moved to the root [`NOTICE`](../NOTICE).

## Inventory

| Source | SHA | Destination | Existing build and test procedure (from the source CI) | Depends on |
| --- | --- | --- | --- | --- |
| `BicameralAI/bicameral-bot` | `f5bde8e81fef6c7505cd9a3d75b186d9b1a5b3b9` | [`apps/bot/`](../apps/bot/) | `rust.yml`: `cargo fmt --all -- --check`; Python contract checks (`python3 scripts/…`, `python3 -m unittest scripts.tests.test_*`); `cargo build/test/clippy --workspace --locked`; dashboard (Node 22, `dashboard/`): `npm ci`, `npm run check`, `npm test`, `npm run build` | Publishes the protocol consumed by Integrations, MCP and Cloud; release binaries consumed by Installer |
| `BicameralAI/bicameral-integrations` | `3e00781d1fff7cf96346fd2f819658a336fa8e7b` | [`packages/integrations/`](../packages/integrations/) | `ci.yml` (Python 3.13): `ruff check`, `mypy`, `pytest adapter/core/tests connectors runtime mods scripts/tests tests/redteam tests/redaction_evaluation`, `scripts/validate_*.py`; `red-team.yml` | Vendored copy of the Bot external-ingest contract and v2 schema (pinned to a Bot SHA) |
| `BicameralAI/bicameral-mcp` | `9657ae288637cb3b4e4ef4dc1d43ae42a02b17c7` | [`packages/mcp/`](../packages/mcp/) | `lint-and-typecheck.yml`, `mcp-deterministic-tests.yml` (Python 3.11): `pip install --require-hashes -r requirements-ci.lock`, `pip install --no-deps .`, `ruff check .`, `ruff format --check .`, `mypy .`, `python scripts/sim_issue_108_flows.py --check`, `pytest tests/`, `python -m build` | Bot daemon at runtime (HTTP), no build-time dependency |
| `BicameralAI/bicameral-cloud` | `4e72bba576bedbd5b5bd23bc3a5bb1d1e3dffc8d` | [`services/cloud/`](../services/cloud/) | `contract-conformance.yml` (3.12), `hosted-security-hardening.yml` (3.11), `reconciliation-evidence.yml` (3.12): `pip install -e '.[dev]'`, `pytest` groups, `ruff`, `mypy`, `scripts/*.py` validators | Vendored Bot dashboard bundle and Bot release descriptor; Bot and MCP in hosted journey tests |
| `BicameralAI/bicameral-install` | `b492638ad4141b44a916dbf6420d7776c8cfc1a3` | [`installer/`](../installer/) | `unix-installer-ci.yml`: `sh -n site/install.sh`, `tests/test-unix-reconcile.sh`; `windows-installer-ci.yml`: PowerShell parse and `tests/test-windows-publication.ps1` | Bot release artifacts (`latest.json`, published binaries) |

Import order follows the dependencies: Bot, Integrations, MCP, Cloud, Installer.

## Not carried over

| Material | Reason |
| --- | --- |
| `.bicameral/` control-plane state (factory attestations, repo-governance records, spec-governance) | Confidential Factory artifacts, not product source (#969). Exceptions, because existing tests or CI read them: MCP's `.bicameral/customer-artifact-boundary.json` and `.bicameral/validator-provenance/` (packaging policy), and Cloud's `.bicameral/factory-attestations/0d972754…validation-terminal-hosted-v0.1.10.json` (read by `tests/test_real_process_hosted_topology.py`). The Cloud record contains Factory session context (loaded context, roadmap references); it must pass the publication-safety review before this repository is made public. |
| `bicameral-cloud/docs/governance/soc2-application-controls.md` | SOC 2 control documentation with operational detail; private evidence under #969. |
| Per-component `LICENSE`, `LICENSE-MIT`, `LICENSE-APACHE` | Superseded by the root Apache-2.0 license (J-03). Exception: `packages/mcp/LICENSE` now holds the Apache-2.0 text in place of BSL 1.1, because MCP's source distribution packages a license file. |
| Git history | Curated snapshot per #969; history remains in the source repositories. |

## Mechanical adjustments

Each component's `.github/` directory stays inside the component, because its own tests read it. GitHub only runs workflows from the repository root, so the root `.github/workflows/` carries copies of the components' core test workflows with `working-directory` and path filters pointing at the component. Release, deploy, publish, cross-repository dispatch, Factory attestation and Atlas workflows were not activated at the root: they deploy or publish, which needs separate authorization, or they depend on excluded Factory artifacts.

Individual source edits are listed in [`sources.json`](sources.json) under `adjustments`.

## Integrations provenance across the migration

The Integrations information-cycle bundle records an original review (`reviewed_commit` in `BicameralAI/bicameral-integrations`) and checks it against Git history, which a snapshot does not carry. The bundle, validator and generator are themselves reviewed components, so none of them is changed. Instead:

- [`manifests/bicameral-integrations.json`](manifests/bicameral-integrations.json) is a deterministic manifest of every file at the imported SHA with its content hash.
- [`verifications/bicameral-integrations.information-cycle-provenance.json`](verifications/bicameral-integrations.information-cycle-provenance.json) records, from the original history, that `reviewed_commit` is an ancestor of the imported SHA and that every reviewed component is byte-identical at both. It references the original approval and is marked `is_original_approval: false`.
- [`packages/integrations/scripts/migration_provenance.py`](../packages/integrations/scripts/migration_provenance.py) composes the validator, replacing only its provenance step. It checks the records' digests, their binding to the bundle and to `sources.json`, and the snapshot-to-monorepo content of every component. With the original repository available (`--source-repo` or `BICAMERAL_INTEGRATIONS_SOURCE_REPO`), it re-verifies ancestry, blobs and the manifest against that history.

**Trust boundary.** The records are hashes, not signatures. Offline, which is how CI runs (it has no credentials for the private source repository), a record that is edited and fully re-digested is only caught by code review: changes under `migration/` trigger the Integrations CI and require code-owner review. Running with the original repository re-verifies everything against its history and is authoritative.

Both records are regenerated only from the original history: `python scripts/migration_provenance.py write-records <bundle> --source-repo <path>`.

## License headers

Integrations source files carried `SPDX-License-Identifier: MIT` headers. Bicameral-authored files now carry `Apache-2.0` (J-03), with only the header lines changed. Two exceptions:

- **Reviewed components.** Ten files are reviewed components of the information-cycle bundle, which pins their exact bytes, so their MIT header stays until the next bundle review. Like the rest of the repository, they are distributed under the root Apache-2.0 [`LICENSE`](../LICENSE).
- **Third-party text** keeps its own license and attribution. For example, the Contributor Covenant 2.0 text in the Integrations ingest fixtures.

## Publication-sensitive material and rights evidence

The repository is private. The items below must be resolved or accepted before any publication decision (#969 J-04, J-12, J-13). The root `LICENSE` states the intended outbound license; it does not by itself establish that every imported file may be published under it.

### Rights evidence

| Source | License at the source SHA | In this repository | Evidence still needed |
| --- | --- | --- | --- |
| `bicameral-bot` | `MIT OR Apache-2.0` (`LICENSE-MIT`, `LICENSE-APACHE`, copyright BicameralAI), `NOTICE` | Apache-2.0; `NOTICE` at the root | Confirmation that contributions by people outside the company (commits by Aritro, Mayur Sonar and `rev-9w`) were made under the project license or a contractor agreement |
| `bicameral-mcp` | BSL 1.1, Licensor BicameralAI, change date 2030-07-10 | Apache-2.0 text in `packages/mcp/LICENSE`, `pyproject.toml` license Apache-2.0 | The copyright holders' written agreement to relicense, which J-04 assigns to Kevin; contributor list includes `ljschwarzer` (bicameral-ai.com) |
| `bicameral-integrations` | BSL 1.1, Licensor BicameralAI, while its 315 source files declared `SPDX-License-Identifier: MIT` | Root Apache-2.0; 305 headers changed to Apache-2.0, 10 bundle-pinned files still declare MIT | Which of the two declarations governed past contributions, the relicensing agreement, and the provenance of one commit authored as `MythologIQ` (Kevin's address), since MythologIQ is a separately owned entity |
| `bicameral-cloud` | No license file (all rights reserved) | Root Apache-2.0 | Owner authorization to license Cloud under Apache-2.0 and confirmation of contributor terms |
| `bicameral-install` | No license file | Root Apache-2.0 | Same as Cloud |
| `bicameral-factory` (allowlisted files) | No license file | Root Apache-2.0 | Owner authorization for the allowlisted files (the authorship is recorded in the PR that imports them) |

Third-party material:

- **Bot `NOTICE`** is preserved at the root.
- **Cloud's vendored dashboard** (`services/cloud/src/bicameral_cloud/static/dashboard/`) is a minified build of Bot's dashboard and its npm dependencies, with no third-party license notices in the bundle. Distributing it needs attribution generated from `apps/bot/dashboard/package-lock.json`.
- **Other third-party text** keeps its own license, for example the Contributor Covenant text in the Integrations fixtures.

### Publication-sensitive retained material

- **`services/cloud/.bicameral/factory-attestations/0d972754750c45d73aae4bf3606af248034e8dbf.validation-terminal-hosted-v0.1.10.json`** is a Factory session record:
  - **What it contains:** the agent and session, notes on the local setup, an internal roadmap status, links to private pull requests and issues, and Factory learning signals. It has no credentials, customer data or personal data.
  - **What reads it:** `tests/test_real_process_hosted_topology.py::test_factory_attestation_workflow_targets_this_validation_run`, which asserts only that the file exists, and the inactive `factory-attestation.yml` (excluded control plane).
  - **Verified option:** a stub that keeps `version`, `factory_repo`, `factory_commit`, `target_repo`, `target_branch` and `timestamp` and drops the session context keeps that test file green (24 passed).
  - **Approval:** replacing the record needs owner approval. The original stays in `bicameral-cloud`.
- **Private infrastructure identifiers** appear in workflows that are not activated: the GCP project `bicameral-cloud-500911` and service-account addresses in `services/cloud/.github/workflows/deploy-cloud-run.yml` and the three `atlas-release-assignment-status.yml` files. They are identifiers, not credentials.
- **Links to private repositories** (Factory and the component repositories) appear throughout component documentation and contract metadata. They do not resolve publicly.
- **MCP's `.bicameral/customer-artifact-boundary.json` and `.bicameral/validator-provenance/`** are packaging policy: which paths stay out of the customer wheel. They are not confidential.

