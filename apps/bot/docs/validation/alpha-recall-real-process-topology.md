# Disposable real-process topology harness: alpha recall path

Source issue: [bicameral-bot#819](https://github.com/BicameralAI/bicameral-bot/issues/819)
Parent chain: [bicameral-bot#789](https://github.com/BicameralAI/bicameral-bot/issues/789) (alpha recall)
Blocks: [bicameral-bot#820](https://github.com/BicameralAI/bicameral-bot/issues/820) (lifecycle/UserJourney validation)

This document describes the **bot-side** disposable real-process topology
harness for the alpha recall path and the exact MCP request/render contract the
downstream cross-repo validation (#820, `bicameral-mcp`) attaches to.

The harness proves the bot portion of the narrowed alpha recall topology against
a **production daemon process**, not an in-process dispatcher or fake-daemon
substitute:

```text
coding agent
  -> MCP tool call                  (downstream #820: real bicameral-mcp stdio client)
  -> production MCP stdio process    (downstream #820)
  -> ToolRequest over loopback HTTP  (this harness: POST /v2/tool-requests)
  -> production bot daemon           (this harness: spawned `bicameral gateway start`)
  -> replay accepted Decisions + inspect local workspace/graph
  -> bot ToolResponse.result         (canonical nested result)
  -> MCP-rendered result             (downstream #820)
```

## What the harness provides

Code:
- `tests/support/real_process_topology.rs` — the reusable harness
  (`DisposableTopology`) and validation-witness types (`TopologyReceipt`,
  `TopologyHop`), typed transport (`dispatch`, `dispatch_raw`), crash/retry
  controls, and the client startup-negotiation model.
- `tests/alpha_recall_real_process_topology.rs` — the proof tests.

### Boundaries and authority

- The bot **event store is the only authoritative product state**. The
  `TopologyReceipt` and the per-response `RuntimeTrace` are **validation
  witnesses only** — never product state. The receipt records
  `authoritative: false` and `disposable: true` explicitly.
- Only **disposable, controlled data** is used: an ephemeral temp workspace, a
  default `git` event store rooted under that temp dir, and synthetic seed
  Decisions. No production credentials, customer data, or persistent developer
  stores are required or touched. The `TempDir` removes all stores on drop.
- The daemon binds **loopback only** (`127.0.0.1:<ephemeral-port>`); the port is
  reserved via an OS ephemeral-port probe so parallel runs do not collide.
- MCP is a **request/render surface** over loopback daemon communication. The
  real stdio client is supplied by `bicameral-mcp` for #820; this harness
  exercises the loopback HTTP boundary the stdio client renders over.

## Running the harness

The real-process tests are `#[ignore]` (they spawn a daemon process and bind a
socket). Run them explicitly:

```bash
cargo test -p bicameral --test alpha_recall_real_process_topology -- --ignored --nocapture
```

The default `cargo test --workspace` run still compiles the harness and runs the
fast, non-process `client_startup_negotiation_contract` test.

## MCP contract / fixture inputs and expected outputs (for #820)

The downstream MCP stdio client maps its tool calls to the canonical
`ToolRequest` JSON below and renders the canonical nested `ToolResponse.result`.
All requests are `POST /v2/tool-requests`; capability handshake is
`GET /v2/capabilities` (expect `protocol_version: "v2"`).

Every `ToolRequest` requires `request_id` (UUID) and `issued_at` (RFC3339); the
`command` is tagged `{ "name": <command>, "params": { ... } }`.

### 1. Seed input — `ingest.submit_local` then `review.accept_candidate`

```json
{
  "request_id": "<uuid>",
  "issued_at": "<rfc3339>",
  "command": {
    "name": "ingest.submit_local",
    "params": {
      "source_uri": "meeting://payments/architecture",
      "source_type": "meeting_transcript",
      "title": "Payment retries require idempotency keys",
      "description": "Every payment retry must call apply_idempotency before settlement.",
      "suggested_level": "architecture",
      "snapshot_content": "Every payment retry must call apply_idempotency before settlement.",
      "evidence": [
        { "excerpt": "…", "span_start": 0, "span_end": 60, "confidence": 0.95 }
      ]
    }
  },
  "authority": {
    "actor_id": "seed-agent",
    "auth_method": "mcp_session",
    "session_id": "issue-819-disposable-topology",
    "workspace": "/workspace",
    "policy_scope": ["default", "review"],
    "audit_metadata": { "surface": "mcp" }
  }
}
```

The response carries `result.candidate_ids[0]`. This harness seeds a canonical
accepted Decision only as recall input. The accepted alpha first-Decision path
is the daemon-authorized MCP promotion — `recall.promote_decision_candidate`
with an explicit single-use confirmation, materialized by the daemon through the
event store (see `QUICKSTART.md`). Dashboard Activity / Ingestion Gate promotion
is deferred for alpha (not prohibited); the daemon remains the single promotion
authority regardless of surface.

### 2. Recall input — `preflight.run`

```json
{
  "command": {
    "name": "preflight.run",
    "params": {
      "files": ["src/payments/idempotency.rs"],
      "symbols": ["apply_idempotency"],
      "checkpoint_hint": "pre_work"
    }
  },
  "authority": { "actor_id": "preflight-agent", "auth_method": "mcp_session", "…": "…" }
}
```

### 3. Expected canonical `ToolResponse.result` (recall)

- `status: "ok"`, `request_id` echoes the request.
- `result.live_recall` — daemon-authored staged recall:
  - `outcome`: `"results" | "no_match" | "readiness_limited"`.
  - `readiness_limitations`: array of daemon-authored caveats.
  - `ranked_results[]`: canonical ranked decisions with `source_refs`,
    `reason_codes`, `tier`.
  - `packet.searched_scope.level` (e.g. `"replayable"`),
    `packet.recall_trace.replay_available`, `packet.canonical_decisions[]`.
- `trace` — `RuntimeTrace` witness: `command: "preflight.run"`,
  `caller_surface: "mcp"`, `request_id` correlated, `status`, `latency_ms`.
- The recall path never asserts compliance/merge-safety language
  ("safe to merge", "signoff approved", "compliance confirmed", etc.).

## Downstream controls for #820

The harness exposes documented controls the lifecycle/UserJourney validation
drives — this issue does **not** implement #820's validation itself:

| Control | Purpose |
| --- | --- |
| `DisposableTopology::crash_daemon()` | Hard-kill (SIGKILL) the daemon; simulate abrupt failure. |
| `DisposableTopology::restart_daemon()` | Restart against the same disposable store; prove durable recall recovery. |
| `DisposableTopology::daemon_running()` | Liveness check for crash/retry sequencing. |
| `DisposableTopology::expect_daemon_unavailable(req)` | Assert the daemon-unavailable case is visible (typed transport error). |
| `DisposableTopology::dispatch_raw(body)` | Send a malformed body; assert shape-mismatch is a visible 4xx, not silent acceptance. |
| `negotiate_client_startup(client_protocols, daemon_protocol)` | Model protocol-drift refusal (no legacy fallback), mirroring `bot_mcp_thin_client_conformance`. |
| `DisposableTopology::topology_receipt()` | Non-authoritative witness of the crossed process boundary, serializable for attachment. |

## Related contracts and tests

- `docs/domain/runtime-protocol.md` — `ToolRequest`/`ToolResponse`/`RuntimeTrace`.
- `docs/specs/bot-mcp-data-flow-runtime-architecture.md` — thin-client boundary.
- `crates/bicameral-runtime/tests/bot_mcp_thin_client_conformance.rs`
- `crates/bicameral-gateway/tests/mcp_v02_integration.rs`
- `crates/bicameral-runtime/tests/live_preflight_recall_path.rs`
