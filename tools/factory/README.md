# `tools/factory/`

Files imported from [`BicameralAI/bicameral-factory`](https://github.com/BicameralAI/bicameral-factory) that the Bicameral components use. Factory is imported allowlist-only ([Factory #969](https://github.com/BicameralAI/bicameral-factory/issues/969), J-06/J-07). Only the files below are here, byte-identical to Factory at the commits their consumers pin, recorded in [`migration/sources.json`](../../migration/sources.json), with their Factory-relative paths. Every other Factory path stays out, including control-plane state, internal evidence and external development-support products. The full classification of the Factory files the components reference is `factory_inventory` in the same file.

| File | Purpose | Used by |
| --- | --- | --- |
| `scripts/semantic_contract_conformance.py` | Fails closed when a producer or consumer artifact drifts from an active contract pair | Root `bot-rust.yml`, job `hosted-bridge-contract` |
| `scripts/test_semantic_contract_conformance.py` | The checker's tests | `factory-tools.yml` |
| `registries/semantic-contract-pairs.json` | Contract pairs and their current fingerprints | The checker |
| `registries/semantic-contract-pairs.schema.json` | Schema of the registry | |
| `.github/workflows/semantic-contract-conformance.yml` | Factory's reusable workflow, kept because the tests read it. GitHub does not run it from here | The checker's tests |
| `schemas/runtime-architecture-contract.schema.json` | Schema of runtime architecture contracts | `$schema` of `apps/bot/docs/specs/*.contract.json` |
| `scripts/runtime_architecture_contract.py` | Validates and projects runtime architecture contracts | `factory-tools.yml` |
| `scripts/test_runtime_architecture_contract.py` | The validator's tests | `factory-tools.yml` |
| `fixtures/runtime-architecture-contract/accepted-explicit-product-registration.json` | Representative accepted contract | `factory-tools.yml` |
| `CONTEXT.md`, `PAMA.md`, `INSTRUCTIONS.md` | Shared doctrine, at the commit and hashes `apps/bot/docs/governance/PIN.json` pins | Bot's doctrine pin |
| `scripts/release_manifest.py`, `scripts/release_unit.py`, `scripts/release_rehearsal.py` and their tests, `registries/bot-release-closure-policy.v1.json`, `schemas/release-rehearsal.schema.json` | Release manifest validation, Release Unit proposals and merge eligibility, non-publishing rehearsal, at Factory `98d3d9b` | Root `bot-v0112-release-rehearsal.yml`; Cloud's release journey (not activated) |
| `topology-environments/operator-service-environment-v1.json`, `schemas/topology-environment-profile.schema.json` | Operator-service topology environment profile | `apps/bot/scripts/run_operator_service_topology.py --factory-repo tools/factory` |

```bash
python3 tools/factory/scripts/semantic_contract_conformance.py \
  --registry tools/factory/registries/semantic-contract-pairs.json \
  --contract-id hosted-bridge.capability.v1 \
  --repository BicameralAI/bicameral-bot --repo-root apps/bot
python3 tools/factory/scripts/runtime_architecture_contract.py validate apps/bot/docs/specs/<name>.contract.json
(cd tools/factory && python3 -m unittest scripts.test_semantic_contract_conformance scripts.test_runtime_architecture_contract \
  scripts.test_release_manifest scripts.test_release_unit scripts.test_release_rehearsal)
```

Until this import is accepted, Factory remains authoritative for these files. A change in Factory is brought here by re-copying the files at the new Factory commit and updating `migration/sources.json`, not by editing them here.
