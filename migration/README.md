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
| `BicameralAI/bicameral-factory` (allowlist only) | `c7e682bc8a72f094f96a79be16f536ec47dcedd2`; doctrine at `09dd3184bf03a9ef6deccbfb7154306b521a6ad6` | [`tools/factory/`](../tools/factory/) | Factory `dashboard-tests.yml` and `runtime-architecture-contract.yml` (`python3 -m unittest …`, `runtime_architecture_contract.py validate/project`); Bot `rust.yml` job `hosted-bridge-contract` calls Factory's reusable `semantic-contract-conformance.yml` | Bot's `hosted-bridge-contract` job and `protocol/hosted-bridge/contract.json`; Bot `docs/specs/*.contract.json` (`$schema`); Bot `docs/governance/PIN.json` |

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

Factory is not imported as a component. Per #969 (J-06/J-07), Bicameral-owned Factory files the components use are copied, deny by default. Each copied file's SHA-256 and source commit is in `allowlist` in [`sources.json`](sources.json); all are byte-identical to their source.

**Inventory.** `factory_inventory` in `sources.json` classifies every Factory path that a tracked monorepo file names (paths, checkouts, reusable workflows, `$schema`, links), plus the doctrine files `apps/bot/docs/governance/PIN.json` pins. It was produced by scanning, then reviewing how each consumer uses each hit. The scan recognizes paths with a directory component in files that mention Factory.

| Classification | Factory files | Why |
| --- | --- | --- |
| **Copied** | Semantic contract checker, its test, registry, registry schema, reusable workflow | Bot CI ran it through a cross-repository reusable workflow |
| **Copied** | Runtime architecture contract schema, validator, test, fixture | The format of Bot's `docs/specs/*.contract.json` (`$schema`) |
| **Copied** | `CONTEXT.md`, `PAMA.md`, `INSTRUCTIONS.md` at `09dd318` | Doctrine Bot pins; all three hashes match `PIN.json` |
| **Blocked** | `release_manifest.py`, `release_unit.py`, `release_rehearsal.py`, `bot-release-closure-policy.v1.json` | `release_unit.py` requires Atlas as the release approval controller and verifies Atlas signatures, so importing it imports the excluded Atlas authority. Used only by release/deploy workflows that are not activated |
| **Excluded** | `atlas-release-unit-status.yml` | Atlas |
| **Excluded** | Factory attestation and spec-governance scripts, policy, skill manifest, run-signal blueprints, playbook | Factory development-process control plane |
| **Not a dependency** | `docs/META_LEDGER.md` and the other Factory-only documents | Named by the components' boundary guards as paths that must not be committed, not read |
| **Reference only** | Architecture, strategy and product-line documents linked from Bot docs | Hyperlinks only; nothing builds, tests or runs from them |

The root `bot-rust.yml` runs the semantic contract checker with the same registry and contract id against `apps/bot` (`root_workflows.rewired`). `factory-tools.yml` runs the copied tools' tests and the runtime architecture contract steps of Factory's `runtime-architecture-contract.yml`; Factory's `test_runtime_spec_conformance_contract` tests Factory's authoring skills, which are not imported. Two differences from the source: the source job used Factory `main` at run time and the monorepo uses the pinned commit, and no cross-repository token is used. Until this import is accepted, Factory stays authoritative and changes are brought over by re-copying at a new commit.

**Recorded, not changed:**

- **`bicameral.semantic-resolution` registry drift.** The registry fingerprint `sha256:f3513488…` is Bot's `docs/specs/semantic-resolution-capability-contract.md` at Bot `703cbd4` (2026-07-31). Bot `ed64bb4` (2026-08-01) added one non-normative reading-guide link ("Product motivation: …"), which changed the bytes to `sha256:10381c1b…`; the registry was not updated. MCP has never had `contracts/semantic-resolution-capability-contract.md`; the registry says MCP vendors it "before claiming consumer conformance". No source workflow invokes this pair. This is stale registry metadata plus a not-yet-adopted consumer, not a migration difference. Updating the fingerprint is a contract-owner change for Bot and MCP maintainers.
- **Runtime architecture contracts.** Six of Bot's seven `*.contract.json` files validate. `state-machine-resolver-v1-runtime-architecture.contract.json` fails (`scenarios[21..22]` ID fields are not arrays) with the source Factory validator too. No source CI validated these files, so no check is activated for them.

## Cross-repository references

`cross_repository_references` in `sources.json` lists every mechanism by which monorepo files reach a legacy repository (reusable workflows, checkouts, dispatch, raw fetches, tokens) and its disposition. Product runtime code has none: the installer downloads from Bicameral's distribution host, and the remaining URLs are schema `$id` identifiers, PR/issue links and commit-URL metadata, kept as identifiers. The remaining workflow references are in release, deploy, vendoring and repository-security workflows that are not activated at the root.

`BicameralAI/bicameral-daemon` is not a #969 source. Its README describes it as the legacy architecture staging repository (documentation and ADRs only), so it is not imported.

## License headers

Integrations source files carried `SPDX-License-Identifier: MIT` headers. Bicameral-authored files now carry `Apache-2.0` (J-03), with only the header lines changed. Two exceptions:

- **Reviewed components.** Ten files are reviewed components of the information-cycle bundle, which pins their exact bytes, so their MIT header stays until the next bundle review. Like the rest of the repository, they are distributed under the root Apache-2.0 [`LICENSE`](../LICENSE).
- **Third-party text** keeps its own license and attribution. For example, the Contributor Covenant 2.0 text in the Integrations ingest fixtures.
