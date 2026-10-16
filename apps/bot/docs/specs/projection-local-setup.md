# Projection Local Setup

**Date:** 2026-06-10
**Status:** proposed
**Related:** `v1-reviewable-context-projection.md`, ADR-0022

## Overview

The V1 reviewable-context projection provides a rebuildable read model for
Ledger View reads. It materializes Decisions, source evidence, code evidence,
graph snapshots, graph readiness, and event history from event-store replay.

The projection is disposable — deleting and rebuilding all rows from the event
store must produce identical read output.

## Running Projection Tests

### Unit tests (projection module)

```bash
cargo test -p bicameral-runtime -- projection
```

### Conformance tests (rebuild idempotency)

```bash
cargo test -p bicameral-runtime --test projection_rebuild_conformance
```

### All runtime tests

```bash
cargo test -p bicameral-runtime
```

## Gateway Projection Endpoints

When the gateway is running, the projection is available at:

- `GET /api/v1/projection/ledger` — Returns the current projection state
  built from event-store replay. Contains Decisions, source evidence, code
  evidence, graph snapshots, graph readiness, and event history.

- `POST /api/v1/projection/rebuild` — Clears and rebuilds the projection
  from event-store replay. Returns the fresh projection state.

These endpoints do not expose compliance verdicts, enforcement state,
readiness-to-ship, or merge safety claims.

## Local E2E Test Slice

The full E2E acceptance path:

```text
ingest source
  → accept candidate
  → approve/reject signoff
  → optionally attach code evidence
  → replay event store
  → build projection
  → read Ledger View from projection
```

This is exercised by the `projection_rebuild_conformance` test suite and
the gateway integration tests.

## Projection Store

The current implementation uses `MemoryProjectionStore` for deterministic
local tests. Future implementations may use embedded SurrealDB or other
local stores — the `ProjectionStore` trait is the extension point.

## What the Projection Contains

| Record family | Contents |
|---|---|
| `decisions` | Replayed Decision with signoff/review/tracking state |
| `source_evidence` | Cited evidence spans linked to Decisions |
| `code_evidence` | Accepted bindings and advisory code hints |
| `graph_snapshots` | Snapshot identity and provenance metadata |
| `graph_readiness` | Per-Decision graph indexing/readiness state |
| `ledger_events` | Event history with actor chain |

## What the Projection Excludes

- Compliance verdicts
- Implementation correctness
- Feature readiness
- Merge safety
- Blocking enforcement
- Policy-expanded authority
