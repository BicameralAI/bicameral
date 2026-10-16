# V1 Reviewable Context Projection

**Date:** 2026-06-10
**Status:** proposed
**Level:** L1 repo architecture
**Related:** ADR-0022,
`codegraph-ledger-ui-protocol-boundary.md`,
`enforcement-memory-projection.md`

## Purpose

V1 may use SurrealDB as a rebuildable projection-backed read path for local E2E
testing and dashboard/API reads, but the product claim remains Decision signoff
plus reviewable context.

This spec defines the V1 projection subset so implementation can exercise the
real projection layer without reintroducing compliance, correctness, readiness,
or enforcement claims.

## Scope

The V1 projection may materialize:

- replayed Decisions and their signoff/review state;
- cited source evidence and source freshness;
- accepted code evidence such as `BindingEvidence`;
- advisory code hints such as `BindingProposal`;
- graph snapshot provenance and readiness state;
- graph evidence state needed to guide the next user action;
- allowed recovery actions from the selected Decision view;
- event history needed to explain how the current read model was produced.

The V1 projection must not materialize or expose:

- compliance verdicts;
- implementation correctness;
- feature readiness;
- merge safety;
- blocking enforcement;
- global "no conflict" or global consistency claims;
- policy-expanded authority that did not replay from an accepted event.

## Authority Boundary

The event store remains the source of accepted governance events. The
projection is disposable and rebuildable:

```text
event store replay
  -> accepted Decision/signoff/source/code-evidence events
  -> SurrealDB reviewable-context projection
  -> dashboard/API/query reads
```

Deleting and rebuilding the projection must not change canonical authority. If
a fact cannot be reconstructed from accepted events plus current source/graph
indexes, it is not a canonical V1 fact.

## Read Model

The minimum useful V1 read model should support the Ledger View contract:

```text
Ledger View
  Decision list
    selected Decision
      Summary
      Signoff / review state
      Source evidence
      Code evidence
      Freshness / refresh
      Event history
```

Suggested SurrealDB record families:

| Record | Purpose |
|---|---|
| `decision` | Replayed selected Decision and signoff/review state. |
| `source_evidence` | Cited evidence spans and source snapshot provenance. |
| `code_evidence` | Accepted bindings and advisory code hints. |
| `graph_snapshot` | Snapshot identity, commit/SHA, and freshness metadata. |
| `graph_readiness` | Indexing/staleness/unsupported/ambiguous state for a Decision scope. |
| `ledger_event` | Replay/event history for the visible read model. |

Relationships should preserve provenance, not imply enforcement:

```text
decision -> cites -> source_evidence
decision -> has_code_evidence -> code_evidence
code_evidence -> observed_at -> graph_snapshot
decision -> has_readiness -> graph_readiness
decision -> changed_by -> ledger_event
```

## UI Contract

The projection feeds the existing Ledger View hierarchy. Code evidence appears
under the selected Decision, not as a top-level ledger entity.

Graph and code evidence failures render as action-oriented recovery states.
Different failure cases should look different only when the recovery path,
responsible actor, or available action differs. If two failure cases have the
same recovery path, the UI should collapse them into the same visible state.

The UI must not say "noncompliant", "failed signoff", "unsafe to merge",
"ready to ship", or "implemented incorrectly" unless a future
compliance/enforcement event is explicitly in scope and the UI is rendering
that future state.

## E2E Acceptance Slice

A local E2E test should prove the full read path:

```text
ingest source
  -> accept candidate
  -> approve/reject signoff
  -> optionally attach code evidence
  -> replay event store
  -> rebuild SurrealDB reviewable-context projection
  -> read Ledger View from the projection
```

The test passes when the dashboard/API can read selected Decisions, source
evidence, code evidence, graph readiness, and allowed recovery actions from the
projection without presenting compliance or enforcement state.

## Future Extension

`enforcement-memory-projection.md` owns the later enforcement-memory contract.
Future work may extend this projection into compliance or enforcement only
through an explicit spec/ADR update and reviewed events that make those claims
visible as future scope, not as V1 behavior.
