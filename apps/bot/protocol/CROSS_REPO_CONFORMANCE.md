# Cross-Repo Conformance: Provider Acquisition Schemas

Status: provisional alpha
Schemas: `ProviderResourceDescriptor`, `ProviderItemEnvelope`
Owner repo: `BicameralAI/bicameral-bot` (Rust types + generated JSON Schemas)
Consumer repo: `BicameralAI/bicameral-integrations` (golden fixtures + Python conformance tests)

---

## Schema ownership model

```
bicameral-bot (owner)                    bicameral-integrations (consumer)
─────────────────────                    ────────────────────────────────
crates/bicameral-api/src/                protocol/provider_acquisition/
  provider_acquisition.rs                  schemas/
        │                                    provider-resource-descriptor.schema.json
        │ schemars generate                  provider-item-envelope.schema.json
        ▼                                  fixtures/
protocol/schemas/v2/                       descriptors/*.json
  provider-resource-descriptor.schema.json items/*.json
  provider-item-envelope.schema.json       tests/
  fixtures/                                  test_fixture_schema_conformance.py
    valid-provider-resource-descriptor-*     test_fixture_secret_guard.py
    valid-provider-item-envelope-*
    invalid-provider-descriptor-*
    invalid-provider-item-*
```

**Rule**: Schemas are generated from the Rust types via `schemars`. Never edit
`.schema.json` files by hand. Edit the Rust types, regenerate, then propagate.

## Fixture/pin process

### 1. Bot-side (schema owner)

The bot repo maintains:

- **Canonical schemas** generated from Rust types (`protocol/schemas/v2/*.schema.json`).
- **Positive fixtures** demonstrating accepted shapes (`valid-provider-*.json`).
- **Negative fixtures** demonstrating rejected shapes (`invalid-provider-*.json`).
- **CI gate** (`schema_gen` test in `bicameral-gateway`) that fails if committed
  schemas diverge from the current Rust types.

### 2. Integrations-side (schema consumer)

The integrations repo maintains:

- **Pinned schema copies** at `protocol/provider_acquisition/schemas/`.
- **Golden fixtures** exercising real provider paths (Linear, Google Drive, GitHub).
- **Conformance tests** (`test_fixture_schema_conformance.py`) validating fixtures
  against the pinned schemas.
- **Secret/boundary guard** (`test_fixture_secret_guard.py`) ensuring fixtures do
  not leak credentials or bot-authority fields.

### 3. Cross-repo pin

When the bot schema changes, the integrations repo must update its pinned copies.
The pin is tracked by schema content hash:

```
# From bot repo root — compute the pin hashes:
sha256sum protocol/schemas/v2/provider-resource-descriptor.schema.json
sha256sum protocol/schemas/v2/provider-item-envelope.schema.json
```

The integrations repo records these hashes in its conformance test or a dedicated
pin file. A mismatch means drift has occurred.

## Validation commands

### Validate integrations fixtures against bot schema (run from bot repo)

```bash
python scripts/validate_cross_repo_conformance.py \
  --bot-schemas protocol/schemas/v2 \
  --integrations-fixtures ../bicameral-integrations/protocol/provider_acquisition/fixtures
```

This validates that integrations golden fixtures conform to the bot's canonical schemas.

### Validate bot fixtures against bot schema (self-check)

```bash
python scripts/validate_cross_repo_conformance.py \
  --bot-schemas protocol/schemas/v2 \
  --bot-fixtures protocol/schemas/v2/fixtures
```

### Validate schema drift between repos

```bash
python scripts/validate_cross_repo_conformance.py \
  --bot-schemas protocol/schemas/v2 \
  --integrations-schemas ../bicameral-integrations/protocol/provider_acquisition/schemas \
  --check-drift
```

Reports whether the integrations pinned schemas match the bot canonical schemas
byte-for-byte (modulo formatting). Exits non-zero on drift.

### Run integrations-side conformance (from integrations repo)

```bash
cd ../bicameral-integrations
pytest protocol/provider_acquisition/tests/ -v
```

## Promotion criteria: provisional alpha to stable

The `ProviderResourceDescriptor` and `ProviderItemEnvelope` schemas are currently
**provisional alpha**. Promotion to **stable** requires all of the following:

| # | Criterion | Evidence required |
|---|-----------|-------------------|
| 1 | **Three or more live providers** produce conforming descriptors/items in production or staging | CI logs or integration test output showing real provider data passing schema validation |
| 2 | **No breaking schema changes** for 30+ days | Git log showing no `required` field additions, no field removals, no type narrowing since last change |
| 3 | **Cross-repo conformance passes** | Both bot and integrations CI green on conformance checks |
| 4 | **Negative fixture coverage** for all authority-boundary violations | All `invalid-provider-*` fixture categories present and tested |
| 5 | **Human review of promotion decision** | ADR or issue documenting the promotion rationale, linking evidence for criteria 1-4 |
| 6 | **Version bump** | Schema `title` or a sibling `version` field updated; old fixtures preserved for backward-compat testing |

### What stable means

- Breaking changes require a new schema version (`v3/`) and a migration path.
- The `provisional alpha` warning is removed from protocol docs and schema descriptions.
- Consumer repos may rely on field stability for production code paths.

### What stable does NOT mean

- It does not imply product acceptance of the provider acquisition feature.
- It does not grant live provider readiness or deployment authority.
- It does not replace the existing `SourceBinding` approval path for governance promotion.

## Drift handling process

### Detection

Drift occurs when:

1. Bot Rust types change → schemas regenerate → integrations pinned copy is stale.
2. Integrations adds a fixture field not present in the bot schema.
3. A field is used with different semantics across repos (e.g. different enum values).

Automated detection:

```bash
# Run from bot repo — checks both directions:
python scripts/validate_cross_repo_conformance.py \
  --bot-schemas protocol/schemas/v2 \
  --integrations-schemas ../bicameral-integrations/protocol/provider_acquisition/schemas \
  --integrations-fixtures ../bicameral-integrations/protocol/provider_acquisition/fixtures \
  --check-drift --check-conformance
```

### Resolution

| Drift type | Resolution | Owner |
|------------|------------|-------|
| Bot schema updated, integrations stale | Integrations PR to copy new schemas + fix any broken fixtures | Integrations maintainer |
| Integrations fixture uses undeclared field | Remove field or propose bot schema extension via issue | Integrations maintainer |
| Semantic mismatch (same field, different meaning) | Escalate to bot maintainer; resolve via ADR if needed | Bot maintainer |
| Bot removes/renames required field | Major version bump required; integrations stays on old pin until migrated | Bot maintainer |

### Automation (cheap wins)

1. **CI cross-repo check** (recommended): A GitHub Actions workflow in the bot repo
   that, on schema changes, clones the integrations repo at `dev` HEAD and runs
   `validate_cross_repo_conformance.py --check-drift --check-conformance`. Failures
   are advisory (non-blocking) until schemas reach stable.

2. **Dependabot-style schema sync PR** (future): An automation that opens a PR in
   integrations when bot schemas change, copying the new schemas and reporting
   fixture breakage.

3. **Pin file in integrations** (recommended now): A `schema-pin.json` file recording
   the bot commit SHA and schema content hashes. The integrations conformance test
   reads this file and warns on staleness.

## Non-goals

- This document does not decide when to promote to stable (that is a human decision).
- This document does not cover live provider readiness or product acceptance.
- This document does not replace existing `SourceBinding` governance paths.
