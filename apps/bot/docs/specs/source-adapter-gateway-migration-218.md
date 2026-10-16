# Source Adapter & Drive Runtime Migration (RFQ 4 / #218)

**Status:** proposed — RFQ response
**Issue:** [#218](https://github.com/BicameralAI/bicameral-bot/issues/218) "Move source adapters and Drive runtime behind bot-owned gateway/runtime"
**Plan source:** `docs/plans/mcp-refactor-reassignment.md` §RFQ 4
**Owner:** @Knapp-Kevin · **Secondary reviewer:** @jinhongkuan
**Governs by:** ADR-0024 (field roles), ADR-0016 (gateway guardrails), ADR-0002 (agent surfaces & ExternalIngestEnvelope separation)
**MCP pre-refactor pointer:** `bicameral-mcp` commit `0827444c80d45fe3474f68002166e1fc35708eda`
**Move/replace/retire:** **retire** MCP source runtime; acquisition is **owned by** `bicameral-integrations`; materialization is **owned by** `bicameral-bot`.

## Architecture rule

> Integrations acquire. Bot materializes. MCP routes.

| Leg | Repo | Owns | Must not |
|---|---|---|---|
| Acquire | `bicameral-integrations` | provider connectors, polling, webhook capture, readiness, screened provider facts, discovery outcomes | materialize durable state; carry actor/review/signoff/policy authority |
| Materialize | `bicameral-bot` | gateway contract, source approval, `SourceSnapshot`, `SourceEvidence`, content addressing, dedup/replay, event-store writes | acquire from providers directly (no polling/webhook runtime in bot) |
| Route | `bicameral-mcp` | relay local `ToolRequest` commands; read bot state | own any source acquisition, polling, webhook, or Drive runtime authority |

Per **ADR-0024**: adapter output is `signal_input` / `provenance` / `handoff_artifact`. Only the bot gateway's approval step is an `authority_gate`; only it may create the durable `SourceSnapshot` / `SourceEvidence` (`identity_anchor` + `read_model_state`). No provider-supplied field — confidence, freshness, candidate hint — may become authority.

## Repo-target decision (the RFQ-4 question)

RFQ 4 flags: *"Some integration code may belong in `bicameral-integrations`, not bot. RFQ should decide repo target explicitly."* **Decision:**

- **Provider acquisition → `bicameral-integrations`.** It already hosts the connectors and polling runtime (see current state). The bot does **not** grow provider SDKs, OAuth flows, polling, or webhook receivers.
- **Materialization → `bicameral-bot`.** The external-ingest gateway already owns `Source` / `SourceSnapshot` / `SourceEvidence` / candidate creation, content addressing, and the `ExternalIngest` event (`docs/specs/external-adapter-ingest-gateway.md`, #264, implemented).
- **`bicameral-mcp` retires its source runtime entirely.** Not moved into bot — retired, because the acquisition equivalent already lives in integrations.

## Current state (verified)

### `bicameral-bot` — materialization leg: COMPLETE
- `POST /api/v1/external-ingest` accepts `ExternalIngestEnvelope` — `crates/bicameral-gateway/src/routes.rs:469`.
- Authority-injection rejection before deserialization (`detect_forbidden_fields`, `routes.rs:591-683`); 9+ injection tests.
- `SourceSnapshot` (`crates/bicameral-api/src/source.rs:33`) + `SourceEvidence` (`crates/bicameral-api/src/evidence.rs:59`) materialized only after the bot decision (`routes.rs:783-944`).
- SHA-256 content addressing (`snapshot_addr = "sha256:<hash>"`); dedup by content address; replay via `EventType::ExternalIngest` (`IngestPayload`, no `BindingEvidence` field).
- **19/19 `external_ingest_gateway` tests pass.**
- **No binding-payload coupling** — Track 2 is independent of #116 (confirmed).

### `bicameral-integrations` — acquisition leg: LARGELY PRESENT
- Connectors: `connectors/github/`, `connectors/google_drive/connector.py`.
- Polling runtime: `runtime/poll_client.py`, `poll_specs.py`, `poll_auth.py`, `graphql_poll.py`; OAuth `runtime/google_oauth.py`; delivery `runtime/delivery.py`, `sinks.py`.
- Gateway mapping: `runtime/gateway_mapping.py` maps `AdapterEmission` → wire request, redacting PII before send.
- Risk mods: `mods/webhook_risk/`, `mods/connector_freshness/`.

### `bicameral-mcp` — STILL OWNS SOURCE RUNTIME (the violation #218 closes)
| MCP code | Path | Disposition |
|---|---|---|
| GitHub acquisition | `sources/github/poller.py`, `sources/github/adapter.py`, `events/sources/github.py` | **retire** (integrations owns) |
| Drive acquisition + channels + OAuth + renewal | `sources/google_drive/*`, `cli/drive_renew_cli.py` | **retire** (integrations owns) |
| Other source adapters | `events/sources/{granola,local_directory}.py`, `sources/*` | **retire / reassess** |
| Webhook receivers | `webhooks/server.py`, `webhooks/github.py`, `webhooks/google_drive.py`, `webhooks/dedup.py` | **relocate** (see open question W) |
| Direct-ingest bypass | `cli/sync_and_brief_cli.py:206-217` calls bot `handle_ingest()` in-process | **rewire** to POST the gateway envelope (see seam below) |
| Source adapter registry | `events/sources/__init__.py:46-51` | **retire** |

No background daemon/polling loop exists in MCP — adapters fire only via CLI/SessionStart hook, so retirement does not remove an always-on capability.

## Contract decision: external adapters use `ExternalIngestEnvelope` (RESOLVED)

The bot exposes **two distinct ingest endpoints**, and external provider acquisition is currently pointed at the wrong one:

| Endpoint | Contract | Handler | Trust posture | Intended caller |
|---|---|---|---|---|
| `POST /api/v1/ingest` | `IngestRequest` (`title`/`description`/`source`/`source_type`/`evidence[]`, MCP v0.2 §2.2 `bicameral.ingest`; carries `source_kind: mcp_session/command_run`, typed evidence refs) | `ingest_candidate` (`routes.rs:212`) | **Local actor.** Typed deserialize; unknown authority fields are *ignored*, not rejected. | MCP session / CLI / command-run capture (RFQ 3 `ingest.submit_local`) |
| `POST /api/v1/external-ingest` | `ExternalIngestEnvelope` (`source_system`/`source_uri`/`content`/`content_hash`/`evidence[]`/`candidate_hints[]`, ADR-0002) | `external_ingest` (`routes.rs:469`) | **Untrusted external.** Raw-JSON `detect_forbidden_fields` → 403 on any authority/review/signoff/compliance/policy field; injection-canary + PII screen; advisory-only candidates. | External provider adapters |

`bicameral-integrations/runtime/gateway_mapping.py` emits the **`IngestRequest`** shape (it produces `title`/`description`/`source`/`source_type`/`evidence`, never `source_system`/`source_uri`/`content`) — i.e. external provider data is entering through the **local-actor** path, which only ignores rather than rejects injected authority. ("ADR-0012" cited in that file is *bicameral-integrations'* own ADR-0012 — connector-readiness-ladder — not a bot ADR.)

**Decision:** external provider acquisition is untrusted evidence and **must use `ExternalIngestEnvelope` → `POST /api/v1/external-ingest`**. `/api/v1/ingest` + `IngestRequest` is the local/MCP-actor contract (RFQ 3) and is **not** the external adapter path; reusing it for provider acquisition reintroduces the authority-boundary blur #218 removes.

**Required change (integrations):** update `runtime/gateway_mapping.py` to emit `ExternalIngestEnvelope` (`source_system`=provider id, `source_uri`=artifact URI, `content`=captured artifact body, `evidence[].excerpt`/`span_start`/`span_end`/`confidence`, optional `candidate_hints`) and POST to `/api/v1/external-ingest`; re-pin the vendored schema to the envelope. Keep the existing redaction-before-send.

**Sequencing:** this integrations retarget lands **before** the MCP rewiring/retirement (otherwise MCP would be re-pointed at a contract about to change). The MCP retirement is filed separately (see #218 follow-up issue).

## Target data flow

```
provider (GitHub/Drive/Linear/…)
   │  acquire (poll or webhook)
   ▼
bicameral-integrations  ──emit ExternalIngestEnvelope──▶  POST /api/v1/external-ingest  (bicameral-bot)
   │  (screened provider facts, redacted, no authority)        │  approve → materialize
   │                                                            ▼
   │                                              Source · SourceSnapshot · SourceEvidence
   │                                              · DecisionCandidate (advisory) · ExternalIngest event
   ▼
bicameral-mcp: routes local ToolRequest commands + reads bot state — NO source acquisition
```

## Exit criteria (mapped to #218)

1. **Adapter output enters bot through the gateway contract** — integrations emits `ExternalIngestEnvelope` to `/api/v1/external-ingest` (contract decided above); no in-process `handle_ingest()` call from MCP. ✅ bot side done; ⬜ integrations retarget `gateway_mapping.py`; ⬜ MCP bypass removed.
2. **Authority injection rejected** — ✅ enforced + tested in bot (`detect_forbidden_fields`).
3. **Drive/source ingest creates `SourceSnapshot`/`SourceEvidence` only after bot materialization** — ✅ enforced in bot; ⬜ confirm integrations carries no materialization and MCP carries none after retirement.
4. **MCP no longer owns polling/webhook/source runtime authority** — ⬜ retire the MCP files above; resolve webhook home (Q-W).

## Open questions

- **Q-W (webhook ingress home):** webhook *reception* (`webhooks/server.py`) is neither in bot nor clearly in integrations' runtime today. Decide: does integrations host the webhook receiver (emitting envelopes to the bot gateway), or does the bot grow a webhook ingress that normalizes to the same gateway path? Recommendation: integrations hosts reception (keeps bot free of provider-specific runtime), consistent with the acquire/materialize split.
- **Q-Drive-substrate:** RFQ 5 separately asks whether Google Drive remains a **bot event-store substrate** (`crates/bicameral-runtime/src/event_store_adapters/drive_folder.rs`) vs. moving to integrations. That is the *ledger persistence* Drive, distinct from the *source-ingest* Drive retired here — do not conflate. This spec retires only source-ingest Drive.
- **Q-canary/PII parity:** the bot gateway runs injection-canary + PII screens (`injection_canary`, `sensitive_data` rejections). Integrations also redacts in `gateway_mapping.py`. Confirm screening is defense-in-depth, not duplicated authority, and that the bot remains the enforcing gate.

## Required tests

- Integrations: connector emission → `ExternalIngestEnvelope` schema conformance; redaction before send.
- Bot (exists): envelope accept, authority-injection reject, materialization, content-address dedup.
- Cross-repo: an integrations emission is accepted by the live bot gateway and produces exactly one `SourceSnapshot` + N `SourceEvidence` + advisory candidate; replay reproduces them.
- Negative: a provider payload carrying any forbidden authority field is rejected at the gateway (not silently dropped).
- MCP: after retirement, no code path acquires source data or calls `handle_ingest()`; `sync-and-brief` either is retired or only reads bot state.

## See also

- `docs/specs/external-adapter-ingest-gateway.md` — implemented bot gateway contract (#264)
- `docs/specs/bot-mcp-data-flow-runtime-architecture.md` · `docs/specs/bot-mcp-thin-client-migration-notes.md`
- `docs/adr/0016-gateway-external-ingest-guardrails.md` · `docs/adr/0024-signal-variables-authority-gates-durable-state.md`
- `docs/plans/mcp-refactor-reassignment.md` §RFQ 4 (and §RFQ 5 for the Drive-substrate question)
