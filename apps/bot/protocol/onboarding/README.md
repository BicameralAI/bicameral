# Onboarding Contract Fixtures

Cross-repo contract fixtures and hand-authored contract-spec JSON Schemas for the
domain-discovered Product auto-join onboarding flow, codified in
[`docs/specs/domain-product-onboarding-contract.md`](../../docs/specs/domain-product-onboarding-contract.md)
(issue [#765](https://github.com/BicameralAI/bicameral-bot/issues/765)).

These pin the reconciled object shapes that the runtime slices (bot#766–#769)
and Cloud slices (cloud#125–#127) implement, so every consumer builds against one
source of truth.

## Versioned identity

Every authoritative schema carries a stable, versioned `$id` under
`https://bicameral.dev/protocol/onboarding/v1/`. The contract version is
`1.0.0`. `manifest.json` records the contract version, the source commit, the
canonical **semantic fingerprint**, and per-file digests of every schema and
fixture input. Downstream consumers (cloud#136, cloud#137) pin the version +
semantic fingerprint rather than a UI bundle digest.

## Contract-spec and production types

> **Hand-authored, forward-defining.** Unlike `protocol/schemas/v2/*` (which are
> **generated** from Rust `serde` types via `schemars` and must never be edited
> by hand), the schemas here are hand-authored **contract-spec** shapes that
> forward-define the wire objects. The production `bicameral-api` wire types
> (`product_creation.rs`, `product_join.rs`) round-trip every fixture below
> under `crates/bicameral-api/tests/onboarding_contract_conformance.rs`, using
> the shipped types directly — no hand-maintained shadow types. Both the schema
> `additionalProperties: false` and the Rust `#[serde(deny_unknown_fields)]`
> keep the fail-closed shapes rejecting undeclared fields.

## Objects

| Schema | Object | Authority |
| --- | --- | --- |
| `hosted-project-preview.schema.json` | `HostedProjectPreview` | Cloud (non-canonical directory projection) |
| `product-join-ticket.schema.json` | `ProductJoinTicket` | Cloud (narrow, signed, short-lived; bounded claims only) |
| `product-creation-result.schema.json` | `ProductCreationResult` | Daemon (canonical identity + atomic creator roles) — SPA command result, **not** the ledger file |
| `create-product-ledger-event.schema.json` | `EventStoreEntry` + `ProductCreatedPayload` | Daemon Drive JSON ledger file (`{event_id}.json`); hashed into `ProductCreationCommitProof.event_digest`. Git YAML append is not this object. |
| `vectors/product-creation-commit-proof-v2.golden.json` | literal v2 canonical-bytes / digest / signature | Shared Bot↔Cloud golden vector (not a schema fixture) |
| `product-creation-publication.schema.json` | `ProductCreationPublication` | Cloud (non-canonical, retryable, idempotent by `product_id`) |
| `product-join-request.schema.json` | `ProductJoinRequest` | Daemon-inbound (carries a `ProductJoinTicket`) |
| `product-join-result.schema.json` | `ProductJoinResult` | Daemon (granted `Product Member` only, or typed rejection) |

## Fixtures

`fixtures/` holds deterministic examples. `ProductJoinResult` covers the success
path plus every typed failure required by #765:

- `product-join-result.granted.json` — success (Product Member only)
- `product-join-result.expired-ticket.json`
- `product-join-result.wrong-domain.json`
- `product-join-result.wrong-account.json`
- `product-join-result.wrong-product.json`
- `product-join-result.replayed-ticket.json`

`ProductCreationPublication` covers the base publication plus a `retry` fixture
proving idempotent republication preserves the canonical `product_id`,
`idempotency_key`, and creator-account binding while re-minting the row.

## Validation

Deterministic, dependency-free (stdlib only):

```bash
python3 scripts/validate_onboarding_contract_fixtures.py
```

The validator:

- structurally validates every fixture against its schema;
- verifies each fixture is canonical JSON and round-trips deterministically
  (run with `--write` to canonicalize);
- rejects any `ProductJoinTicket` claim outside the bounded set (fail-closed);
- enforces the `ProductJoinResult` granted/rejected invariants;
- verifies every schema `$id` is the versioned form;
- scans every fixture for forbidden boundary material (Supabase tokens,
  provider secrets, local paths);
- enforces atomic creator roles + creator-account publication binding + stable
  publication idempotency;
- recomputes the semantic fingerprint and fails on any schema/fixture/manifest
  drift with a diagnosable message (run with `--write` to regenerate
  `manifest.json`).

Rust serialization/round-trip conformance runs under
`cargo test -p bicameral-api --test onboarding_contract_conformance`, and both
run in CI (`.github/workflows/rust.yml`), so schema, runtime, fixture, and
manifest drift all fail the build.

## Cross-repo pin

Consumers (cloud, runtime) pin these shapes by schema content hash, mirroring
`protocol/CROSS_REPO_CONFORMANCE.md`:

```bash
sha256sum protocol/onboarding/schemas/*.schema.json
```

or, preferably, by pinning `manifest.json`'s `contract_version` +
`semantic_fingerprint`.

TypeScript serialization/round-trip conformance remains owned by the Cloud
slices (cloud#136, cloud#137), which must keep their generated types conformant
to the fixtures and semantic fingerprint pinned here.
