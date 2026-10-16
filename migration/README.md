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
| Per-component `LICENSE`, `LICENSE-MIT`, `LICENSE-APACHE` | Superseded by the root Apache-2.0 license (J-03). Exception: `packages/mcp/LICENSE` now holds the Apache-2.0 text in place of BSL 1.1, because MCP's source distribution packages a license file. |
| Git history | Curated snapshot per #969; history remains in the source repositories. |

## Mechanical adjustments

Each component's `.github/` directory stays inside the component, because its own tests read it. GitHub only runs workflows from the repository root, so the root `.github/workflows/` carries mirrors of the components' workflows that verify the product with component-local commands: build, test, contract, journey, platform smoke, boundary guard, secret scan, quality and release-descriptor workflows. Each mirror runs its steps in the component directory with the component's path filters, so the commands are unchanged. `root_workflows` in [`sources.json`](sources.json) lists every component workflow as mirrored, inlined or not activated, with the reason:

- **Not activated, needs authorization or a decision:** release, publish and deploy workflows; cross-repository dispatch and vendoring; the cross-component release journey.
- **Not activated, excluded:** Factory attestation and Atlas workflows; MCP `repo-governance.yml`, which reads excluded `.bicameral/` state.
- **Not activated, deferred to publication readiness (#971):** third-party security and supply-chain scanners, which #969 recommends deduplicating into one suite; source-repository process automation.
- **Not activated, fails at the source:** Integrations `redaction-evaluation-stacked.yml`.

Mirroring the boundary guards exposed one path assumption: `scripts/validate_governance_boundary.py` in Bot, MCP, Integrations and Cloud compared `git diff` paths, which Git reports from the repository root, against component-relative roots, so inside the monorepo every change passed. `git diff --relative` restores the source behavior (a staged `.qor/` file passed before the change and fails after it, in all four).

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
- **Cloud's vendored dashboard** (`services/cloud/src/bicameral_cloud/static/dashboard/`) is a minified build of Bot `4deb004`'s dashboard. Its third-party notices are in [`DASHBOARD_THIRD_PARTY_NOTICES.md`](../services/cloud/src/bicameral_cloud/static/DASHBOARD_THIRD_PARTY_NOTICES.md), next to the bundle and inside the Cloud image. They were derived from a rebuild at `4deb004` that is byte-identical to the vendored files, not from the current lockfile; `tests/test_dashboard_third_party_notices.py` fails if the bundle is re-vendored without them.
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

## Release composition

Releases keep composing pinned component releases, as the components' own release specifications define: Cloud consumes "a manifest-selected Bot SPA bundle by immutable digest" (`services/cloud/docs/specs/release-manifest-cloud-adoption.md`), MCP is validated "against exact pinned Bot interface" (`packages/mcp/docs/specs/release-manifest-mcp-adoption.md`), and Integrations revalidates "with pinned Bot and MCP artifacts" (`packages/integrations/docs/specs/release-manifest-integrations-adoption.md`). The accepted repository records (ARCH-0001, ARCH-0002) do not address release composition, so building every component from the same commit would be an architecture change that no record authorizes.

Under this composition the pinned producer revisions are historical revisions of the imported components, available only in the source repositories' history (no history was imported):

| Pin | Producer revision | Snapshot | Behind the snapshot |
| --- | --- | --- | --- |
| Cloud's vendored dashboard and `release-inputs/bot-release-descriptor.json` | Bot `4deb004` | Bot `f5bde8e` | 60 commits |
| Cloud's terminal release journey | MCP `bb47e97` | MCP `9657ae2` | 203 commits |
| Cloud's vendored onboarding contract | Bot `ff0b015` | Bot `f5bde8e` | 1,419 commits |
| Integrations' external-ingest schema pin | Bot `8764ac3` | Bot `f5bde8e` | 960 commits (bytes identical to the snapshot) |

The vendored artifacts are in the tree and verify against their pins (`Pinned composition consistency` below). Rebuilding them from source needs those historical revisions; the snapshot builds a different dashboard bundle (`c5fdce1a…` instead of `05d2608e…`), with the same hosted-bridge interface and different integration-ingress and MCP-ToolRequest interface fingerprints.

## Verification ledger

Commands run from the repository root unless a directory is given. Results are from 2026-10-16 on `main` at `09aecd3` with PR #11 (`4bd6fcc`); CI results are from GitHub Actions on those heads.

### Passed: source equivalence

| Check | Command | Result |
| --- | --- | --- |
| Snapshot fidelity | Byte comparison of every imported file with its source SHA (PR #3); adjustments in `sources.json` | Every difference recorded |
| Factory allowlist fidelity | For each `allowlist` entry: `git -C <bicameral-factory> show <source_sha or sha>:<path> \| sha256sum` | 22/22 identical |
| Governance | `python3 tools/governance/check.py` | 131 tests pass |
| Bot Rust | `(cd apps/bot && cargo test --workspace --locked)` | 2,869 passed, 0 failed |
| Bot contract scripts | Python steps of `.github/workflows/bot-rust.yml` | 9/9 pass |
| Bot hosted-bridge contract | `python3 tools/factory/scripts/semantic_contract_conformance.py --registry tools/factory/registries/semantic-contract-pairs.json --contract-id hosted-bridge.capability.v1 --repository BicameralAI/bicameral-bot --repo-root apps/bot` | OK |
| Bot dashboard | `(cd apps/bot/dashboard && npm ci && npm run check && npm test && npm run build)` | 509 tests pass |
| Bot release descriptor | Steps of `.github/workflows/bot-release-provenance.yml` | Descriptor verified with `--verify-hosted-bundle` |
| Bot release rehearsal | Steps of `.github/workflows/bot-v0112-release-rehearsal.yml` | `REHEARSAL_READY`, `authorizes_publish: false` |
| Bot UI evidence | `.github/workflows/bot-ui-evidence.yml` | CI pass; locally 11 passed, 1 skipped with substitute Chromium 1194 |
| Bot Windows release smoke | `.github/workflows/bot-windows-release-smoke.yml` | CI pass on `windows-latest` |
| MCP | Steps of `mcp-deterministic-tests.yml` and `mcp-lint-and-typecheck.yml` | 894 passed, 4 skipped; ruff, format, mypy clean |
| Integrations | Steps of `integrations-ci.yml`, `integrations-red-team.yml` | 1,091 passed; all validators and `migration_provenance.py validate` OK |
| Cloud | `(cd services/cloud && pytest -m "not journey")` plus ruff, mypy, secret scan, distribution boundary, release descriptor | 661 passed, 1 skipped |
| Cloud hosted dashboard journeys | `(cd services/cloud && pytest -m journey tests/journey/test_vendored_dashboard_smoke.py tests/journey/test_apc1_*.py)` | CI pass; locally 17 passed with substitute Chromium 1194 |
| Installer (Unix) | `(cd installer && ./tests/test-unix-reconcile.sh)` | Pass |
| Boundary guards | `python3 scripts/validate_governance_boundary.py [--audit]` in each of the four components | Pass; a staged `.qor/` file is rejected |
| Factory tools | `(cd tools/factory && python3 -m unittest scripts.test_semantic_contract_conformance scripts.test_runtime_architecture_contract scripts.test_release_manifest scripts.test_release_unit scripts.test_release_rehearsal)` | 40 pass |
| Cross-component contracts | Hosted bridge (checker, both sides); external-ingest schema and contract (byte comparison); provider item envelope (byte comparison) | Agree |
| Pinned composition consistency | `(cd services/cloud && python - )` loading `scripts/verify_bot_dashboard_provenance.py`: `directory_dashboard_digest(src/bicameral_cloud/static/dashboard)` equals `version.json` `bundle_sha256` and `release-inputs/bot-release-descriptor.json` `artifacts.hosted_spa`; `validate_bot_descriptor` returns no errors | All equal; no errors |
| Pinned release reproducibility | Read-only full-history clones of the source repositories: Bot `4deb004`, MCP `bb47e97`, Bot `ff0b015` and `8764ac3` all resolve. A fresh `npm ci && npm run build` of Bot `4deb004`'s dashboard reproduces the vendored SPA digest (checked by the real-process runner). Bot's onboarding validator at `ff0b015` yields `236243a3…`, equal to Cloud's `contracts/onboarding/source.json`. The external-ingest schema at `8764ac3` hashes to `content_sha256` in Integrations' `runtime/schemas/ingest_schema_pin.json` | All reproduce; no re-vendoring needed |

### Fixed migration regressions

| Regression | Fix |
| --- | --- |
| Components' boundary guards passed every change inside the monorepo | `git diff --relative` (PR #11) |
| Bot hosted-bridge contract check and v0.1.12 release rehearsal could not run without a cross-repository token | Factory files imported; jobs run from `tools/factory` (PR #10) |

Source-baseline defects repaired during the migration: six Cloud tests that failed at the source SHA (PR #8), the locked `mcp` version with CVE-2026-59951 and 31 Cloud lint findings (PR #3).

### Known source-baseline defects, unchanged

Each one is present at the source SHA, is unchanged by the migration and has an owner outside this repository's migration. None needs a migration fix.

| Defect | Source baseline | Current | Consumers | Runtime consequence | Coverage | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| Terminal journey harness drift ([bicameral-cloud#266](https://github.com/BicameralAI/bicameral-cloud/issues/266)) | Cloud's `run_real_process_hosted_topology.py` (last changed 2026-07-21) requires the button text `Go to pairing` and sends `POST /api/v1/hosted-bridge/pair` without `team_id` or `cloud_access_token`. Bot `1641878` (2026-09-10) renamed the button to `Reconnect device`; Bot `b9aac42` (bot#1290, 2026-09-15) made both fields required. Cloud's own journey failed on the `4deb004` vendoring (run 36766714604) and on the r3 promotion; the drift was classified on bicameral-cloud#335 | Runner byte-identical to the source; same failure | `terminal-hosted-product-journey.yml` | None in the product: the Bot dashboard and Cloud SPA at `4deb004` agree. The real-process journey cannot pass against the pinned Bot until the harness is rebound | Cloud and Bot unit, conformance and hosted journey suites pass | Cloud maintainers (bicameral-cloud#266) |
| `bicameral.semantic-resolution` producer drift | See "Recorded, not changed" | Same | Factory registry; MCP's draft consumer lock | None: documentation contract, no runtime reader; no workflow checks the pair | None | Bot and MCP contract owners |
| `state-machine-resolver-v1` runtime-architecture contract | Fails the Factory validator since Bot `f1a2de3` | Same | Bot's own `validate_smr_claim_acceptance_contract.py` (passes) | None: no runtime reader | Bot's validator in root CI | Bot maintainers |
| Onboarding fingerprint pair | Cloud vendors Bot `ff0b015` (`236243a3…`, reproduced exactly from Bot's validator at `ff0b015`); Bot at the source SHA is `2725ec55…` | Same | Cloud `tests/test_onboarding_contract.py` (vendored copy only); Bot `protocol/onboarding` checks | None found. The difference is additive: a new `create-product-ledger-event` schema and an optional `first_event_commit_proof` on `product-creation-publication`. Cloud already accepts that proof through its own `models/product_directory.py`. No fingerprint is exchanged at runtime | Each side checks its own copy; the cross-side pair is covered only by the real-process journey | Cloud maintainers (re-vendoring is a release-composition decision) |
| `provider-resource-descriptor.schema.json` drift | Bot added optional `action_needed` after Integrations pinned its copy | Same | Integrations provider tests and `protocol/provider_acquisition/types.py` | None: the Integrations copy does not set `additionalProperties: false`, so Bot payloads with `action_needed` validate | Fixture conformance passes; the drift check runs only with `--check-drift` | Integrations maintainer (Bot `protocol/CROSS_REPO_CONFORMANCE.md`) |
| `redaction-evaluation-stacked.yml` | Names `scripts/evaluate_redaction_backends.py`, `scripts/measure_redaction_memory.py`, `tests/redaction_evaluation/test_evaluation_contract.py` and `test_candidate_hard_gates.py`, none in any fetched Integrations history | Same; `not_activated` | None | None: the workflow cannot pass at the source. The validators present (`validate_redaction_evaluation_contract.py`, `_governance.py`) pass; `_hosted.py` needs the package root on `PYTHONPATH`, as at the source | `tests/redaction_evaluation/test_validate_*.py` in the Integrations suite | Integrations maintainer |

### Not run: environment, operating system or credentials

| Check | What remains untested | Blocker |
| --- | --- | --- |
| Real-process Create/Join topology | The 55 receipt assertions after daemon pairing: pairing, Create and Join with response-loss recovery, anonymous and cross-tenant denial, replay and redaction. Run on 2026-10-16 with the CI workflow's exact inputs: Bot `4deb004` and MCP `bb47e97` (full-history clones), Factory `8ea192c`, Supabase CLI 2.39.2, Python 3.11, Playwright 1.59.0 with Chromium 147.0.7727.15, `--use-vendored-dashboard --terminal-user-journey`. Passed: release manifest built from `release-inputs/bot-release-descriptor.json`; Bot release build; the vendored SPA digest equals a fresh build of `4deb004`. It then stops at the acceptance-surface check (`Go to pairing`). A diagnostic run with only that label changed, in a scratch copy, passed: Supabase start; migrations (7 of 7, exact); RLS and grant checks; two real GoTrue sign-ins; Cloud health, SPA, provenance and Supabase directory; two daemons started. It stopped at `POST /pair` (HTTP 400, missing `team_id`) | Source-baseline harness drift (see "Known source-baseline defects"), not a migration or infrastructure failure. Supabase images came from `mirror.gcr.io` with manifest digests equal to `public.ecr.aws/supabase/*`; Chromium came from the Chrome for Testing bucket at the exact Playwright revision |
| Windows installer | The Windows PowerShell 5.1 steps of `installer-windows.yml` and the live published-installer lifecycle. The PowerShell 7 steps (parse, security and lifecycle invariants, `tests/test-windows-publication.ps1`, consent launcher) pass on PowerShell 7.4.6 on Linux | Windows PowerShell 5.1 needs a Windows host; the lifecycle job needs a published release |
| macOS | The macOS builds in Bot's `release.yml` (`macos-latest`) and the macOS lanes of the operator-service topology. `cargo check --target x86_64-apple-darwin` stops at `ring`'s C build script | No Apple SDK or macOS host here; Bot's release workflow is not activated (release authorization) |
| Operator-service topology | `apps/bot/scripts/run_operator_service_topology.py --factory-repo tools/factory --factory-commit <commit> ...`; its unit tests pass in `bot-rust.yml` | Needs released Bot and MCP artifacts and a native user service manager (systemd user bus or launchd); this container has neither |
| Live provider evidence | Google provider authentication evidence and live connector captures | Need provider credentials |

### Deferred: release and publication

Release, publish, tag and deploy workflows stay inactive (they need release authorization and production credentials). Cloud deploy's served-identity check also needs Factory `release_manifest.py` at `4b74e4d`, which loads Atlas-coupled `release_unit.py`; without a replacement release authority that check cannot run from this repository. Publication-readiness items are listed under "Publication-sensitive material and rights evidence".

