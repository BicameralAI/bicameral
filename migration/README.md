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
| `BicameralAI/bicameral-factory` (allowlist only) | `c7e682bc8a72f094f96a79be16f536ec47dcedd2` | [`tools/factory/`](../tools/factory/) | Factory `dashboard-tests.yml`: `python3 -m unittest scripts.test_semantic_contract_conformance`; Bot `rust.yml` job `hosted-bridge-contract` calls Factory's reusable `semantic-contract-conformance.yml` | Bot's `hosted-bridge-contract` CI job; reads Bot's `protocol/hosted-bridge/contract.json` |

Import order follows the dependencies: Bot, Integrations, MCP, Cloud, Installer, then the Factory allowlist.

## Not carried over

| Material | Reason |
| --- | --- |
| `.bicameral/` control-plane state (factory attestations, repo-governance records, spec-governance) | Confidential Factory artifacts, not product source (#969). Exceptions, because existing tests or CI read them: MCP's `.bicameral/customer-artifact-boundary.json` and `.bicameral/validator-provenance/` (packaging policy), and Cloud's `.bicameral/factory-attestations/0d972754…validation-terminal-hosted-v0.1.10.json` (read by `tests/test_real_process_hosted_topology.py`). The Cloud record contains Factory session context (loaded context, roadmap references); it must pass the publication-safety review before this repository is made public. |
| `bicameral-cloud/docs/governance/soc2-application-controls.md` | SOC 2 control documentation with operational detail; private evidence under #969. |
| Per-component `LICENSE`, `LICENSE-MIT`, `LICENSE-APACHE` | Superseded by the root Apache-2.0 license (J-03). |
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

## Factory allowlist

Factory is not imported as a component. Per #969 (J-06/J-07), only Bicameral-owned files that the components' own CI depends on are copied, deny by default. The allowlist with each file's SHA-256 is in [`sources.json`](sources.json); all files are byte-identical to the Factory SHA.

| Slice | Files | Why | Review |
| --- | --- | --- | --- |
| Semantic contract conformance | `scripts/semantic_contract_conformance.py`, its test, `registries/semantic-contract-pairs.json` and `.schema.json`, `.github/workflows/semantic-contract-conformance.yml` | Bot's `hosted-bridge-contract` job ran this checker through a cross-repository reusable workflow, so the job could not run here | Standard library only; one Bicameral author; no secrets, private URLs or references to Navigator, Atlas, FinOps, TARA, Qortara or MythologIQ |

The root `bot-rust.yml` runs the same checker with the same registry and contract id against `apps/bot` (see `root_workflows.rewired` in `sources.json`). Two differences from the source follow from the move:

- The source job used Factory's `main` at run time; the monorepo uses the pinned SHA. Until this import is accepted, Factory stays authoritative and changes are brought over by re-copying at a new SHA.
- No cross-repository token is used.

**Recorded, not changed:** the registry's other active pair, `bicameral.semantic-resolution`, does not verify at the source either. Bot's `docs/specs/semantic-resolution-capability-contract.md` hashes to `sha256:10381c1b…` against the registry's `sha256:f3513488…`, and MCP has no `contracts/semantic-resolution-capability-contract.md`. No workflow invokes that pair in any source repository, so no check is activated for it here.

`BicameralAI/bicameral-daemon` is not a #969 source. Its README describes it as the legacy architecture staging repository (documentation and ADRs only), so it is not imported.

## License headers

Integrations source files carried `SPDX-License-Identifier: MIT` headers. Bicameral-authored files now carry `Apache-2.0` (J-03), with only the header lines changed. Two exceptions:

- **Reviewed components.** Ten files are reviewed components of the information-cycle bundle, which pins their exact bytes, so their MIT header stays until the next bundle review. Like the rest of the repository, they are distributed under the root Apache-2.0 [`LICENSE`](../LICENSE).
- **Third-party text** keeps its own license and attribution. For example, the Contributor Covenant 2.0 text in the Integrations ingest fixtures.
