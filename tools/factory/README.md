# `tools/factory/`

Validators imported from [`BicameralAI/bicameral-factory`](https://github.com/BicameralAI/bicameral-factory) that the Bicameral components' own CI uses. Factory is imported allowlist-only ([Factory #969](https://github.com/BicameralAI/bicameral-factory/issues/969), J-06/J-07). Only the files below are here, byte-identical to Factory at the SHA recorded in [`migration/sources.json`](../../migration/sources.json), with their Factory-relative paths. Every other Factory path stays out, including control-plane state, internal evidence and external development-support products.

| File | Purpose |
| --- | --- |
| `scripts/semantic_contract_conformance.py` | Fails closed when a producer or consumer artifact drifts from an active contract pair |
| `scripts/test_semantic_contract_conformance.py` | The checker's tests |
| `registries/semantic-contract-pairs.json` | Registry of contract pairs and their current fingerprints |
| `registries/semantic-contract-pairs.schema.json` | Schema of the registry |
| `.github/workflows/semantic-contract-conformance.yml` | Factory's reusable workflow, kept because the tests read it. GitHub does not run it from here |

The root workflow `bot-rust.yml` runs the checker for Bot's `hosted-bridge.capability.v1` contract. `factory-semantic-contract.yml` runs the checker's tests:

```bash
python3 tools/factory/scripts/semantic_contract_conformance.py \
  --registry tools/factory/registries/semantic-contract-pairs.json \
  --contract-id hosted-bridge.capability.v1 \
  --repository BicameralAI/bicameral-bot --repo-root apps/bot
(cd tools/factory && python3 -m unittest scripts.test_semantic_contract_conformance)
```

Until this import is accepted, Factory remains authoritative for these files. A registry change in Factory is brought here by re-copying the files at the new Factory SHA and updating `migration/sources.json`, not by editing them here.
