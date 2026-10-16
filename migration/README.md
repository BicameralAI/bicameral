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
| `BicameralAI/bicameral-factory` (allowlist only) | `c7e682bc8a72f094f96a79be16f536ec47dcedd2`; doctrine at `09dd3184bf03a9ef6deccbfb7154306b521a6ad6`; release toolchain at `98d3d9b54aea2de3167ab1ff2216bd0f2e18aed5` | [`tools/factory/`](../tools/factory/) | Factory `dashboard-tests.yml`, `runtime-architecture-contract.yml` and release CI (`python3 -m unittest …`, `runtime_architecture_contract.py validate/project`); Bot `rust.yml` job `hosted-bridge-contract` and `v0112-release-rehearsal.yml` check out Factory | Bot's `hosted-bridge-contract` job and `protocol/hosted-bridge/contract.json`; Bot `docs/specs/*.contract.json` (`$schema`); Bot `docs/governance/PIN.json`; Bot's release rehearsal; Bot's operator-service topology runner; Cloud's release journey |

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

Factory is not imported as a component. Per #969 (J-06/J-07), Bicameral-owned Factory files the components use are copied, deny by default. Each copied file's SHA-256 and source commit is in `allowlist` in [`sources.json`](sources.json); all are byte-identical to their source. Files are copied at the Factory commit their consumer pins: `c7e682b` by default, the doctrine at `09dd318` (Bot's `PIN.json`), the release toolchain at `98d3d9b` (Bot's release rehearsal).

**Inventory.** `factory_inventory` in `sources.json` classifies every Factory path the monorepo depends on or names. Two independent scans over all tracked files (including each component's `.github/`) produced it: scan 1 matched Factory paths in files that mention Factory; scan 2 matched every Factory path and distinctive basename anywhere, Python imports and dynamic loads, paths used after any workflow checkout of the Factory repository, and `FACTORY_*` variables and `--factory-*` arguments. Every hit was reviewed against how its consumer uses it. Scan 2 found one dependency scan 1 missed (the operator-service topology profile); the method and counts are in `factory_inventory.method` and `scan_results`.

| Classification | Factory files | Why |
| --- | --- | --- |
| **Copied** | Semantic contract checker, its test, registry, registry schema, reusable workflow | Bot CI ran it through a cross-repository reusable workflow |
| **Copied** | Runtime architecture contract schema, validator, test, fixture | The format of Bot's `docs/specs/*.contract.json` (`$schema`) |
| **Copied** | `CONTEXT.md`, `PAMA.md`, `INSTRUCTIONS.md` at `09dd318` | Doctrine Bot pins; all three hashes match `PIN.json` |
| **Copied** | `release_manifest.py`, `release_unit.py`, `release_rehearsal.py`, their tests, `bot-release-closure-policy.v1.json`, `release-rehearsal.schema.json` at `98d3d9b` | Bot's release rehearsal pins `98d3d9b`; Cloud's release journey pins `8ea192c`, where `release_manifest.py` is byte-identical. At these commits the files have no Atlas or other excluded references |
| **Copied** | `topology-environments/operator-service-environment-v1.json`, `schemas/topology-environment-profile.schema.json` | Bot's `run_operator_service_topology.py` reads the profile from `--factory-repo` |
| **Blocked** | `release_manifest.py` at `4b74e4d` (Cloud deploy's pin) | At that commit it loads `release_unit.py`, which requires Atlas authenticated by Google IAP as release approval controller and verifies Atlas signatures |
| **Excluded** | `atlas-release-unit-status.yml` | Atlas |
| **Excluded** | Factory attestation and spec-governance scripts, policy, skill manifest, run-signal blueprints, playbook | Factory development-process control plane |
| **Not a dependency** | `docs/META_LEDGER.md` and the other Factory-only documents | Named by the components' boundary guards as paths that must not be committed, not read |
| **Reference only** | Architecture, strategy and product-line documents linked from Bot docs | Hyperlinks only; nothing builds, tests or runs from them |

The root `bot-rust.yml` runs the semantic contract checker with the same registry and contract id against `apps/bot`, and the root `bot-v0112-release-rehearsal.yml` runs Bot's release rehearsal against the `98d3d9b` copies (`root_workflows.rewired`). `factory-tools.yml` runs the copied tools' tests and the runtime architecture contract steps of Factory's `runtime-architecture-contract.yml`; Factory tests of files that are not imported (authoring skills, release assignment, Factory workflows) are not run. Differences from the source: Bot's hosted-bridge job used Factory `main` at run time and the monorepo uses the pinned commit; the rehearsal's exact-commit check compares the copies' SHA-256 with the allowlist instead of `git rev-parse` on a Factory checkout; no cross-repository token is used. The operator-service runner takes `--factory-repo tools/factory --factory-commit <monorepo commit>`. Until this import is accepted, Factory stays authoritative and changes are brought over by re-copying at a new commit.

### Release functionality

| Release operation | Before migration | In the monorepo |
| --- | --- | --- |
| Bot release descriptor, MCP and Integrations descriptors, Cloud descriptor | Component-local | Mirrored at the root and verified |
| Bot v0.1.12 release rehearsal (non-publishing) | Factory `release_unit.py`/`release_rehearsal.py` at `98d3d9b` | Runs from `tools/factory` (no Atlas at this pin) |
| Cloud terminal release journey, receipt step | Factory `release_manifest.py` at `8ea192c` | Tool in `tools/factory`; journey not activated, pending the release composition decision (`cross_repository_references`) |
| Cloud deploy served-identity check | Factory `release_manifest.py` at `4b74e4d` | Blocked: needs Atlas-coupled `release_unit.py`; deploy also needs authorization |
| Release Unit assignment, merge gate, Atlas release-assignment status | Atlas in Factory | Excluded |
| Bot binary release, MCP publish, installer Pages, Cloud deploy | Component workflows with production credentials | Not activated; need release authorization |

**Recorded, not changed** (source-baseline defects, not migration differences):

- **`bicameral.semantic-resolution` producer drift.** The Factory registry and MCP's consumer lock `packages/mcp/contracts/semantic-resolution-capability-contract.lock.json` both expect `sha256:f3513488…`, Bot's `docs/specs/semantic-resolution-capability-contract.md` at Bot `703cbd4` (2026-07-31). Bot `ed64bb4` (2026-08-01) added one non-normative reading-guide link ("Product motivation: …"), changing the bytes to `sha256:10381c1b…`. MCP's lock is a `draft_renderer_and_transport_slice` with `exact_contract_bytes_vendored: false`; its spec makes vendoring the exact bytes and CI fingerprint verification follow-up work before MCP #785 completes. No source workflow checks this pair. Reconciling the producer bytes, registry and lock is a contract-owner change for Bot and MCP maintainers.
- **`state-machine-resolver-v1-runtime-architecture.contract.json`.** Fails the Factory runtime architecture validator (`scenarios[21..22]` trace ID fields absent) and has failed it since the file was created at Bot `f1a2de3` (2026-07-27); the validator has not changed since 2026-07-17. Its governing check is Bot's own `validate_smr_claim_acceptance_contract.py` (amendments A1–A27), which passes in the root CI. The other six contracts pass the Factory validator.
- **Onboarding contract.** Cloud's vendored copy (`services/cloud/src/bicameral_cloud/contracts/onboarding/`, vendored from Bot `ff0b015`) and Bot's `protocol/onboarding/` both declare v1.0.0 but have different semantic fingerprints (`236243a3…` and `2725ec55…`). Each side's own check passes against its own copy.
- **Provider acquisition schemas.** Bot's `validate_cross_repo_conformance.py --check-drift`, run with `--integrations-schemas ../../packages/integrations/protocol/provider_acquisition/schemas`, reports `provider-resource-descriptor.schema.json` drift: Bot added an optional `action_needed` field after Integrations pinned its copy. Fixture conformance passes. No source CI ran the drift check (it needed a sibling checkout). Bot's `protocol/CROSS_REPO_CONFORMANCE.md` assigns this to the Integrations maintainer.

Cross-component pairs that agree: hosted bridge (Factory checker, both sides), external-ingest schema and contract (Integrations' copies byte-identical to Bot's), provider item envelope schema.

## Cross-repository references

`cross_repository_references` in `sources.json` lists every mechanism by which monorepo files reach a legacy repository (reusable workflows, checkouts, dispatch, raw fetches, tokens) and its disposition. Product runtime code has none: the installer downloads from Bicameral's distribution host, and the remaining URLs are schema `$id` identifiers, PR/issue links and commit-URL metadata, kept as identifiers. Pinned producer commits in data files record the provenance of vendored bytes and fetch nothing. `apps/bot/Cargo.toml` still names `BicameralAI/bicameral-bot` as `repository` (package metadata). The remaining workflow references are in release, deploy, vendoring and repository-security workflows that are not activated at the root.

`BicameralAI/bicameral-daemon` is not a #969 source. Its README describes it as the legacy architecture staging repository (documentation and ADRs only), so it is not imported.

## License headers

Integrations source files carried `SPDX-License-Identifier: MIT` headers. Bicameral-authored files now carry `Apache-2.0` (J-03), with only the header lines changed. Two exceptions:

- **Reviewed components.** Ten files are reviewed components of the information-cycle bundle, which pins their exact bytes, so their MIT header stays until the next bundle review. Like the rest of the repository, they are distributed under the root Apache-2.0 [`LICENSE`](../LICENSE).
- **Third-party text** keeps its own license and attribution. For example, the Contributor Covenant 2.0 text in the Integrations ingest fixtures.
