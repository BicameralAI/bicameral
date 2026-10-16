# runtime — the local daemon runtime (ADR-0009)

> **Historical note.** This top-level `runtime/` directory began as the staging
> area for importing the [ZeroClaw](https://github.com/) scaffold ([ADR-0009](../docs/adr/0009-adopt-zeroclaw-runtime-scaffold.md)).
> That extraction is **complete**: the runtime now ships as the
> [`bicameral-runtime`](../crates/bicameral-runtime) crate in the workspace.
> This file documents the runtime as it exists today; the original import plan is
> preserved in [ADR-0009](../docs/adr/0009-adopt-zeroclaw-runtime-scaffold.md).

The runtime is the **boring middle** of Bicameral Bot — the local daemon core
that the [gateway](../crates/bicameral-gateway) drives. It validates typed
objects, applies governance policy, grounds claims in local code, materializes
accepted events through a storage adapter, and preserves an audit trail. It owns
authority; edge surfaces only propose.

Provenance and the narrowing rule are unchanged from the original import: extract
only what serves the local daemon/gateway lifecycle, governance gates, audit
receipts, local grounding, and EM-safe mods — never ZeroClaw's broad
personal-assistant surface. Agents and mods may emit candidates, evidence, hints,
review commands, and advisory governance results; they may **not** create
canonical decisions, approve signoff, resolve compliance, or write canonical
storage. See [`NOTICE`](../../../NOTICE) for attribution.

## What's here (`crates/bicameral-runtime`)

| Module | Role |
|---|---|
| `daemon` | Daemon lifecycle, config, and signal handling (`DaemonConfig`, `DaemonExit`). |
| `governance` | `GovernanceEngine` — policy evaluation over candidates, reviews, and bindings. |
| `event_store_adapters` | Existing pluggable Decision Ledger substrates: `git`, local synced `drive_folder`, and `memory`. Durable append + deterministic replay. |
| `drive_api_event_store` | Provider-backed Google Drive adapter core. Preserves the Drive event wire format while requiring exact provider readback through an injected transport. Production Google HTTP/credential/resource composition is separate work; see [`DRIVE-API-EVENT-STORE.md`](../docs/DRIVE-API-EVENT-STORE.md). |
| `replay` | `replay_from_adapter` → `ReplayedState`: rebuild materialized decision state from accepted events. |
| `history` | `query_history` — read replayed/materialized decision state. |
| `search` | `query_search` — search decisions, candidates, and bindings. |
| `preflight` | `query_preflight` / progressive preflight — surface relevant decisions over source, diff, and graph scope. |
| `grounding` | `SnapshotResolver` / `FileInspector` — snapshot-scoped local code grounding ([ADR-0010](../docs/adr/0010-snapshot-scoped-local-code-graph-contract.md)). |
| `symbol_resolver` | `RustSymbolResolver` — tree-sitter Rust symbol grounding ([ADR-0012](../docs/adr/0012-rust-symbolresolver-boundary-and-followups.md)). |
| `bind_to_code` | `bind_to_code` / `bind_symbol_to_code` — materialize verified `BindingEvidence`. |
| `compliance` | `ComplianceEvaluator` — compliance verdicts over bindings (`BindingVerdict`, `ComplianceVerdict`). |
| `graph_readiness_view` | View-level graph readiness gates; keeps non-verified graph state out of signoff/compliance. |
| `service` | OS service install/status (systemd user units, launchd). |
| `update` | Self-update / release-target resolution. |

## Event-store distinction

The two Drive-backed paths intentionally share canonical event semantics but **not** durability evidence:

- `DriveFolderAdapter` reads and writes a local folder synchronized by Drive Desktop, rclone, Syncthing, or another external sync agent. Its setup and freshness behavior are documented in [`DRIVE-FOLDER-SETUP.md`](../docs/DRIVE-FOLDER-SETUP.md).
- `DriveApiAdapter` is the provider-backed core introduced under Bot #1086. It keeps Google networking/credential ownership behind `DriveApiTransport` and does not confirm append success until exact provider readback matches the canonical serialized event. Its implementation and remaining production dependencies are documented in [`DRIVE-API-EVENT-STORE.md`](../docs/DRIVE-API-EVENT-STORE.md).

Do not describe the local synced-folder adapter as a Google REST client, and do not describe the provider adapter core as production-ready until durable credentials, validated folder authority, real Google transport, Product-create composition, and terminal provider evidence are complete.

## Authority boundary

```text
gateway / edge  ──▶  ToolRequest (+ enforced AuthorityContext)
                          │
                          ▼
                     runtime: validate → governance policy → grounding
                          │
                          ▼
                     event-store materialization
                     ├─ git
                     ├─ local synced drive_folder
                     ├─ provider Drive API core (transport injected)
                     └─ memory (tests)
                          │  deterministic replay
                          ▼
                     materialized decision state  ──▶  history / search / preflight
```

Only `verified` graph evidence may materialize bindings or produce blocking
governance results ([ADR-0010](../docs/adr/0010-snapshot-scoped-local-code-graph-contract.md)).
The local daemon is the authority for verified local grounding; MCP and other
agent surfaces are clients of that evidence.

A provider append/readback receipt is supporting storage evidence only. It does not create Product authority, membership, SourceBinding, Decision authority, deployment, release, or publication authority.

## Tests

Runtime-focused verification:

```bash
cargo test -p bicameral-runtime
```

Repository merge-quality gates:

```bash
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace -- -D warnings
```

The provider adapter core also has deterministic tests for exact readback, idempotent retry, conflicting content/duplicates, deterministic replay, safe non-event filtering, auth failure, and capability declaration. Real Google provider topology remains a separate evidence layer.

## See also

- [`crates/bicameral-runtime`](../crates/bicameral-runtime) — the runtime crate source.
- [`crates/bicameral-gateway`](../crates/bicameral-gateway) — the HTTP edge that drives the runtime.
- [`docs/DRIVE-FOLDER-SETUP.md`](../docs/DRIVE-FOLDER-SETUP.md) — local synced-folder Drive substrate.
- [`docs/DRIVE-API-EVENT-STORE.md`](../docs/DRIVE-API-EVENT-STORE.md) — provider-backed Drive adapter core and remaining production boundaries.
- [ADR-0001](../docs/adr/0001-local-daemon-gateway-and-event-store-substrates.md) · [ADR-0007](../docs/adr/0007-substrate-neutral-governance-flow.md) · [ADR-0009](../docs/adr/0009-adopt-zeroclaw-runtime-scaffold.md) · [ADR-0010](../docs/adr/0010-snapshot-scoped-local-code-graph-contract.md)
