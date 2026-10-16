# ADR-0018: Clean-Projection External Ingest Contract

**Date:** 2026-06-04  
**Status:** proposed; mostly implemented on current code  
**Level:** L1  
**Issue:** https://github.com/BicameralAI/bicameral-bot/issues/92  
**Related:** ADR-0002, ADR-0005, ADR-0006, ADR-0016

## Problem

External integrations need a stable ingest shape that carries source evidence
and candidate hints without requiring integrations to know every bot-owned
normalization detail. At the same time, external ingest must not become a
review or event-store authority surface.

Issue #92 asked the gateway to accept a clean projection:

- `level` may be omitted and inferred by the bot;
- evidence spans may be omitted and defaulted by the bot;
- empty-evidence rejection remains unchanged;
- versioned request/response schemas and fixtures are published for downstream
  conformance.

## Decision

The gateway accepts clean external ingest projections and performs bot-owned
normalization before candidate/evidence materialization.

External ingest callers may omit fields the bot can infer safely:

- missing `level` is inferred from source context;
- missing evidence `span_start` defaults to `0`;
- missing evidence `span_end` defaults to the excerpt length.

This normalization does not grant external authority. External ingest remains a
source-evidence submission surface, not a `ToolRequest`, not a review command,
and not a direct event-store write.

## Contract

The request contract permits:

- `source_uri`;
- `source_type`;
- `title`;
- `description`;
- optional `level`;
- one or more evidence items;
- evidence excerpts with optional `span_start` and `span_end`.

The gateway must reject empty evidence with the existing empty-evidence
behavior.

The response and materialized candidate should expose the normalized result:

- inferred level, when omitted;
- source URI/type provenance;
- evidence span defaults;
- candidate title/body/source;
- candidate and review state.

Expected fixtures should avoid unstable IDs and timestamps.

## Current Implementation Assessment

Current `dev` already appears to implement the core behavior:

- `level` is optional and inferred by the gateway;
- explicit `level` is still honored;
- evidence spans are optional;
- omitted spans default to full excerpt coverage;
- empty-evidence rejection remains `422`;
- v1 ingest request/response schemas are committed;
- schema drift is tested;
- a clean-projection request fixture exists.

The remaining work is conformance polish, not a contract redesign:

- add an integration test that posts
  `protocol/schemas/v1/fixtures/clean-projection-ingest.json`;
- assert the normalized behavior from this ADR;
- add an expected normalized fixture with stable fields only;
- update `protocol/schemas/README.md` to list that expected fixture.

## Consequences

Positive:

- integrations can send smaller, source-shaped payloads;
- bot-owned normalization remains testable and schema-backed;
- downstream MCP/client conformance can rely on stable fixtures;
- external ingest remains separate from local actor/review authority.

Risks:

- inferred levels must remain conservative and deterministic;
- expected fixtures must avoid unstable IDs/timestamps;
- future guardrails from ADR-0016 must preserve this clean-projection behavior.

## Non-Goals

- Do not add gateway security guardrails here; ADR-0016 owns that boundary.
- Do not add actor authentication or local authority here.
- Do not redesign `ToolRequest` or `ExternalIngestEnvelope`.
- Do not move integration connector code into this repo.
- Do not make external ingest a review-command or event-store write surface.

## Required Conformance Follow-Up

The follow-up should add:

1. an integration test that posts the clean-projection fixture;
2. assertions for inferred level and default evidence spans;
3. an expected normalized fixture with stable fields only;
4. README documentation for the fixture.

Validation:

```sh
cargo fmt --all -- --check
cargo test -p bicameral-gateway --locked
cargo test --workspace --locked
```

Schema regeneration should only be needed if request or response types change:

```sh
WRITE_SCHEMAS=1 cargo test -p bicameral-gateway --test schema_gen
```
