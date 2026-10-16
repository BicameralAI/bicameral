# Token Spend Telemetry: TokenSpendEvent, Capture Mechanisms, And Logging Convention

**Date:** 2026-06-10
**Status:** proposed
**Issue:** #110 (RFQ)
**Related:** ADR-0002, ADR-0008, ADR-0014, ADR-0015; bicameral-mcp
`telemetry.py`, `telemetry_flags.py`, `preflight_telemetry.py`,
`audit_log.py`, `local_counters.py`, `consent.py`; bicameral-cloud ADR-0004
**Supersedes:** draft ADR-0024 on the deprecated branch
`docs/110-token-spend-telemetry` (reconciliation notes inline and in
§Reconciliation)

## Purpose

Token logging tied to the implementation, so the team can read logs and
diagnose failures (e.g. a code graph stalled at "verification pending"
should show whether MCP provided a hint and whether the bot acted on it),
and so token spend can later be projected to dollars. Dollar projection is
a nice-to-have; the underlying data fields are the deliverable.

The one rule everything below enforces: **token-spend telemetry is
observation, never governance.** It records what an LLM call cost. It must
never create a Decision, approve signoff, resolve compliance, materialize
an event, or block a workflow. This is enforced structurally
(§Non-Governance Enforcement), not by convention.

## Low-Level Call Inventory (Where Tokens Are Actually Spent)

Surveyed 2026-06-09. Bicameral-owned runtime LLM calls today: **zero** on
the bot; the MCP enforces a `no-LLM-in-the-server` invariant
(`handlers/gap_judge.py`, `server.py`). Spend maps onto three surfaces:

| # | Call site | `surface` | Status |
|---|---|---|---|
| 1 | Caller agent's LLM consuming/producing Bicameral payloads: tool call args in the caller's output; tool results, preflight context packs, `judge_gaps` rubric packs, `link_commit → resolve_compliance` two-step in the caller's input | `caller_llm` | live today, unmeasured |
| 2 | Bot daemon background reasoning provider (ADR-0015: `local_openai_compatible`, `local_cli`) | `daemon_llm` | not implemented; placement decided separately |
| 3 | Hosted advisory reasoning. bicameral-cloud's `ReasoningArtifact` **already records** selected model, provider, routing reason, token usage, estimated cost, cache hit/miss, and policy profile (cloud ADR-0004); the local event is a projection of that metadata | `cloud_advisory` | future; cloud never writes the local sink itself |
| 4 | MCP local embedder (`SentenceTransformerEmbed`, optional `cocoindex` backend) | `caller_llm`-adjacent; emit as `daemon_llm`-style owned call with `workflow: index` | live; in-process, zero API cost; input units only |
| 5 | Eval harness judges (`tests/eval/_bind_judge.py`, `_skill_invocation_judge.py` → Anthropic Messages API, already read `usage.input_tokens/output_tokens`) | out of runtime scope | adopt the schema for comparability, not required |

## Provider Usage Surfaces (Verified From Vendor Docs, 2026-06-09)

How each provider actually exposes usage — the ground truth any capture
mechanism reads from:

| Provider | In-band (per request) | Out-of-band (aggregate) |
|---|---|---|
| Anthropic Messages API | `usage.input_tokens`, `usage.output_tokens`, `usage.cache_read_input_tokens`, `usage.cache_creation_input_tokens` | Console Usage page (authoritative billing) |
| Anthropic Claude Code (caller) | OTel metric `claude_code.token.usage` (`type`: `input`\|`output`\|`cacheRead`\|`cacheCreation`, `model`, `session.id`) and event `claude_code.api_request` (`input_tokens`, `output_tokens`, `cache_read_tokens`, `cache_creation_tokens`, `cost_usd`, `request_id`, `prompt.id`) — **with `mcp_server.name` / `mcp_tool.name` attribution per request**; `/usage` in-CLI view | `claude_code.cost.usage` is explicitly an approximation: "for official billing data, refer to your API provider" |
| OpenAI | `usage` block (`input_tokens`, `output_tokens`, cached/reasoning details) | org Usage API (`/organization/usage/completions`: `input_tokens`, `output_tokens`, `input_cached_tokens`, group_by `model`/`project_id`/`user_id`/`api_key_id`/`service_tier`) + separate Costs API returning authoritative USD `amount` |
| Gemini | `usageMetadata`: `promptTokenCount`, `candidatesTokenCount`, `thoughtsTokenCount` (thinking), `toolUsePromptTokenCount`, `cachedContentTokenCount`, `totalTokenCount` | Cloud billing console |

**Normalization rule (cross-provider):** `input_tokens` includes cached
tokens; `output_tokens` includes thinking/reasoning; cache and reasoning
counts are subset detail fields. This follows OTel GenAI semconv and how
OpenAI/Gemini report totals. *This deliberately overrides the ADR-0024
draft's Anthropic-style "non-cached input only" definition:* Anthropic's
`usage.input_tokens` excludes cached tokens, so the Anthropic mapping is
`input_tokens = usage.input_tokens + cache_read + cache_creation`, with the
cache fields preserved as subsets. Per-leg pricing is the read-time
calculator's job (it subtracts subsets), not the event schema's.

## Capture Mechanisms (How Tools Like readout/token-map Grab Spend)

In fidelity order; every event records which rung produced it via
`token_source`:

| Rung | `token_source` | Mechanism | Fidelity | Used by |
|---|---|---|---|---|
| 1 | `provider_usage` | Read the provider response `usage` block on a call Bicameral made itself | Exact | `daemon_llm`/embedder/eval; Helicone/LiteLLM gateways |
| 2 | `caller_telemetry` | Receive the caller agent's own OTel export (Claude Code: `CLAUDE_CODE_ENABLE_TELEMETRY=1` → OTLP collector, e.g. `localhost:4317`) | Exact, caller-scoped | readout.org-style dashboards, token-map, cc-analytics (Claude Code → OTLP → SQLite → dashboard) |
| 3 | `client_reported` | MCP client reports usage in-protocol (sampling/usage surfaces) | Exact when available | Future; today's clients do not expose it to third-party MCP servers |
| 4 | `estimated_tiktoken` | Tokenize the payloads Bicameral sent/received | Approximate (cross-tokenizer; magnitudes only) | v1 default for `caller_llm` |

**Rejected mechanism — JSONL transcript parsing** (ccusage-style,
`~/.claude/projects/*.jsonl`): documented to undercount input tokens by 1–2
orders of magnitude (entries written during streaming with placeholder
`usage.input_tokens` never finalized; `output_tokens` excludes thinking).
Do not build on it.

**Rejected mechanism — skill-prompted self-report.** The ADR-0024 draft's
preferred caller hook was SKILL.md instructing the caller to populate token
counts in `skill_end` diagnostics. Rejected as a *count source*: the model
does not know its own token usage — only the caller harness does — so
skill-populated counts would be confabulated. The `skill_end` emission
plumbing (`server.py` `record_skill_event`/`send_event` call sites) is kept
as the MCP's emission point; the counts come from rung 4 (or rung 2/3 when
available), never from model self-report.

Rung-2 caveats (from Claude Code monitoring docs; they shape the v2 design):

- telemetry is opt-in env config on the caller; Claude Code does **not**
  pass `OTEL_*` variables to MCP server subprocesses, so Bicameral cannot
  inherit the caller's exporter — it must run/point its own receiver;
- user-configured MCP server names are redacted to `"custom"` in
  `mcp_server.name` (verbatim names only for official-registry servers), so
  attribution may be "some MCP tool in this turn", not provably Bicameral,
  until the server is registry-listed;
- metrics are counters; per-request joins use the `claude_code.api_request`
  event (`prompt.id`, `request_id`). Joining caller events to Bicameral
  request ids is approximate (timestamp + `mcp_tool.name` + workspace)
  until clients expose session linkage to MCP servers.

## The TokenSpendEvent (One Event, Three Surfaces)

One event per LLM invocation that contributed to a Bicameral-governed
workflow. Flat, append-only, JSON-line serializable. The schema lives once
in bot-owned `protocol/schemas/` (per ADR-0008) as
`token_spend_event.schema.json`, mirrored by a Rust type (bicameral-bot)
and a Pydantic model (bicameral-mcp).

The meeting defined "the two token spend events"; reconciling with the
ADR-0024 draft resolved this to **one event with a `surface`
discriminator**, because the third surface (`cloud_advisory`) does not fit
a two-name split. The meeting's "two events" survive as the **two required
v1 emission paths**: `caller_llm` (now) and `daemon_llm` (when the
enrichment worker lands). `cloud_advisory` is additive.

| Field | Type | Req | Notes / OTel equivalent |
|---|---|---|---|
| `event` | string | yes | constant `token_spend` |
| `event_id` | string | yes | UUID v4 |
| `ts` | string | yes | UTC ISO 8601, call completion |
| `schema_version` | int | yes | `1` |
| `surface` | enum | yes | `caller_llm` \| `daemon_llm` \| `cloud_advisory` |
| `actor` | string | yes | tool/route that triggered the spend: `bicameral.bind`, `binding.create`, `daemon.extract`, `cloud.cross_repo_drift`, `embedding.index` |
| `command` | string\|null | no | sub-command within the actor, e.g. `gap_judge`, `compliance_verdict` |
| `workspace_id` | string\|null | yes | salted hash of workspace root (reuse `~/.bicameral/salt`); raw path never leaves the machine |
| `session_id` | string\|null | yes | MCP session / `AuthorityContext.session_id` |
| `request_id` | string\|null | yes | `ToolRequest.request_id`; join key to lifecycle logs |
| `flow_id` | string\|null | no | reuse the mcp `resolve_compliance` flow id where present |
| `parent_event_id` | string\|null | no | chains ingest → bind → compliance spend |
| `provider` | string\|null | yes | `gen_ai.provider.name`; OTel well-known values (`anthropic`, `openai`, …) plus `local.sentence_transformers`, `local.openai_compatible`, `local.cli`, `bicameral` (cloud) |
| `model` | string\|null | yes | `gen_ai.request.model`, exact requested name; null only when genuinely unknown (estimation without `clientInfo`) — never guessed |
| `model_normalized` | string\|null | no | date-suffix-stripped pricing key |
| `response_model` | string\|null | no | `gen_ai.response.model` when reported |
| `input_tokens` | int | yes | `gen_ai.usage.input_tokens` — **includes cached** (see normalization rule) |
| `output_tokens` | int | yes | `gen_ai.usage.output_tokens` — includes thinking; 0 for embeddings; Gemini: `candidatesTokenCount + thoughtsTokenCount` |
| `cache_read_input_tokens` | int | no | `gen_ai.usage.cache_read.input_tokens` (subset) |
| `cache_creation_input_tokens` | int | no | `gen_ai.usage.cache_creation.input_tokens` (subset) |
| `reasoning_output_tokens` | int | no | subset (OpenAI `reasoning_tokens`, Gemini `thoughtsTokenCount`) |
| `token_source` | enum | yes | `provider_usage` \| `caller_telemetry` \| `client_reported` \| `estimated_tiktoken` |
| `trust` | enum | yes | `measured` \| `reported` \| `opaque` (see Trust Model) |
| `cost_hint_usd` | float\|null | no | **received** cost hint only (e.g. cloud `ReasoningArtifact` metadata, caller-telemetry `cost_usd`). Never derived locally at write time; see Dollar Projection |
| `duration_ms` | int | no | matches existing `record_skill_event` field |
| `errored` | bool | yes | call failed after spend (failed calls still bill input) |
| `error_class` | string\|null | no | `timeout` \| `rate_limit` \| `api_error` \| exception class; no message text |
| `workflow` | enum\|null | no | `ingest` \| `preflight` \| `bind` \| `compliance` \| `drift` \| `review` \| `search` \| `history` \| `index` — closed enum naming the governed workflow, **not** a foreign key |
| `governed_object_type` | enum\|null | no | `decision` \| `code_region` \| `compliance_check` \| `input_span` |
| `governed_object_id` | string\|null | no | ledger row id; **local sink only, never relayed** |
| `prompt_hash` | string\|null | no | 16-hex SHA-256 of (per-install salt ‖ prompt); dedup, not reconstructible |

Minimum valid event (RFQ Q1): `event`, `event_id`, `ts`, `schema_version`,
`surface`, `actor`, `input_tokens`, `output_tokens`, `token_source`,
`trust`. Everything else is optional with defaults, so a thin emitter can
comply with very little.

### Workflow attachment (RFQ Q4)

`workflow` + `governed_object_type/id` label which governed work the spend
served, as a non-authoritative observation:

| `workflow` | mcp tool / bot route | `governed_object_type` |
|---|---|---|
| `ingest` | `bicameral.ingest` / `POST /api/v1/ingest` | `input_span` then `decision` |
| `preflight` | `bicameral.preflight` / `POST /api/v1/preflight` | `decision` |
| `bind` | `bicameral.bind` / `POST /api/v1/bind` | `code_region` |
| `compliance` | `bicameral.resolve_compliance` / `GET /api/v1/compliance/evaluate` | `compliance_check` |
| `drift` | `link_commit` drift interpretation | `compliance_check` |
| `review` / `search` / `history` | review summaries, `POST /api/v1/search`, `GET /api/v1/history` | `decision` / none |
| `index` | `code_locator` embedding indexing | none |

## Trust Model (RFQ Q3)

`trust` is epistemics; `token_source` is mechanism. Derivation:

| `trust` | From | Accuracy | Dashboard treatment |
|---|---|---|---|
| `measured` | `provider_usage` (Bicameral made the call, read the usage block) | exact | solid, authoritative |
| `reported` | `caller_telemetry`, `client_reported` (exact counts from an external reporter Bicameral cannot verify) | exact, unverifiable | solid, "reported" badge |
| `opaque` | `estimated_tiktoken`, cloud usage hints | approximate | hatched, "estimate" badge, **never blended** with measured/reported totals |

Validation runs at **read time** and returns warnings, never rejections
(keeps the writer off the governance path):

- `daemon_llm` with non-`measured` trust → warn;
- `caller_llm` with `measured` trust → warn (the MCP server holds no API key);
- `cost_hint_usd` present on `measured` events → warn (measured events
  should not carry hints);
- zero or negative token counts → warn;
- `reported`/`opaque` events far outside the trailing distribution for the
  same `actor` → flag; when a `measured` and a `reported` event share a
  `flow_id`, prefer the measured value in aggregates and retain the
  reported one for audit.

The writer never blocks, rejects, or gates a call on these checks. Write
failures are swallowed.

## Storage, Retention, Relay, Privacy

- **Local sink:** `~/.bicameral/token_spend.jsonl`, append-only, one event
  per line, mode 0600, shared by all local writers (advisory lock for
  concurrent daemon+caller writers). Rotation identical to
  `preflight_telemetry.py`: 50 MB or 30 days per file, keep 5. In
  bicameral-bot the directory joins `BicameralPaths` as `telemetry_dir`.
- **Relay (optional, additive):** aggregate numeric token counts only, via
  the existing `telemetry.py` relay path; the `telemetry-relay` Cloudflare
  Worker contract (numeric-only `diagnostic`, drops strings/nested objects)
  is stable and needs no redeploy. `governed_object_id` and any string
  content beyond controlled enums are **never relayed**; the Worker drops
  non-numeric diagnostics as a second line of defense.
- **Flags:** add `token_spend` as a recognized source in
  `telemetry_flags.py`; `BICAMERAL_TELEMETRY=0` disables everything;
  dedicated `BICAMERAL_TOKEN_SPEND=0` disables only token-spend recording.
  Bot equivalent: a `telemetry` section in `BicameralConfig` plus the same
  env check.
- **Consent:** local-only recording follows the `local_counters.py`
  precedent (no new consent); only the optional relay leg defers to
  `consent.telemetry_allowed()`.
- **Excluded from the schema entirely:** prompt/completion text,
  source/transcript/decision text, file paths, repo names, search queries,
  code, API keys, user identity (matches `audit_log.py`'s forbid-list).
- **No team sync in v1:** token spend is operator-local telemetry, not
  governance state; it is never written to `.bicameral/events/`.

## Logging Convention And Framework Recommendation

No new frameworks. Both repos already have the right substrate; the gap is
a shared schema and naming discipline.

**MCP (Python):** stdlib `logging` + the existing custom JSONL-writer
pattern. New module `token_spend.py` modeled on `preflight_telemetry.py`
(same salt, 0600 perms, rotation), writing the shared sink, emitting from
the existing `skill_end` call sites in `server.py`, with relay mirroring
via `telemetry.send_event`. Do not adopt structlog.

**Bot (Rust):** keep `tracing` (already a dependency). Convention: a stable
`event` field rather than free-text-only messages, e.g.
`tracing::info!(target: "bicameral::telemetry", event = "token_spend", surface = "daemon_llm", provider, model, workspace_id, session_id, request_id = %req_id, actor, input_tokens, output_tokens, "token spend")`,
plus the JSONL sink via the `bicameral-telemetry` leaf crate (below).

**Correlation contract (the debugging requirement):** every lifecycle
operation log line — not just spend events — carries `request_id`,
`session_id`, `workspace_id`, `actor`, plus step outcomes
(`evidence_state`, `hint_provided: bool`, `remediation_attempted: bool`).
Worked example, code graph stalled at "verification pending":

```text
grep request_id across token_spend.jsonl + daemon logs:
  1. token_spend  surface=caller_llm  workflow=bind  output_tokens>0
       -> MCP delivered the caller's hint payload          (hint provided)
  2. daemon log  event=binding.prevalidated  evidence_state=unknown_stale
     hint_provided=true  remediation_attempted=false
       -> bot saw the hint but did not remediate           (bot did not act)
  3. no token_spend with surface=daemon_llm for request_id
       -> background reasoning never ran                   (stall explained)
```

## Non-Governance Enforcement (Structural)

| Constraint | How it holds |
|---|---|
| Telemetry must not become governance authority | Bot writer is a **leaf crate** (`bicameral-telemetry`) that cannot depend on `bicameral-api`/`runtime`/`gateway`; no access to `EventStoreAdapter::append()` or any ledger type. Enforced by a `cargo metadata` dependency-graph test, mirroring `bicameral-audit`. MCP writer is a sibling of `preflight_telemetry.py` with no ledger imports |
| No Decisions, signoff, compliance, or blocking results | Writers only append JSONL (and optionally relay numeric counts); they never construct an `EventStoreEntry`, call a governance route, or return a blocking result; write failures are swallowed |
| Caller-reported distinguishable from daemon-measured | `trust` is mandatory on every event and surfaced in every read view |
| Works without daemon LLM capability | The `caller_llm` path ships standalone and fully local; daemon and cloud paths are additive |
| Emission points (RFQ Q2) | Caller: MCP `skill_end` plumbing + estimation (and `AuthorityContext.audit_metadata` for bot-routed surfaces). Daemon: enrichment worker emits after each call. Cloud: local client projects `ReasoningArtifact` usage metadata. **A single gateway wrapper around all model calls is intentionally rejected** — no one process sees all calls (`no-LLM-in-the-server`), and a chokepoint would look like an authority surface. The schema and sink are the unifying contract, not a wrapper process |

## Dollar Projection (Nice-To-Have)

Tokens are facts we log; dollars are derived views. Project at **read
time**: the `bicameral spend` reader joins `(provider, model_normalized)`
against a **versioned local pricing table** shipped with the reader,
overridable at `~/.bicameral/pricing.json`, with remote refresh as a later
opt-in (`BICAMERAL_PRICING_REMOTE=1`; LiteLLM cost map / OpenRouter as
refresh sources). Every projected number is stamped with the
`pricing_version` used at read time.

Do **not** store locally-derived cost at write time (overriding the
ADR-0024 draft's `cost_estimate_usd`): prices drift, and actual cost
differs when a third-party inference provider serves the model — which is
exactly why `provider` is recorded verbatim. The only stored cost field is
`cost_hint_usd`, for hints **received** from elsewhere (cloud artifact
metadata, caller telemetry `cost_usd`), and vendors themselves treat those
as approximations (Claude Code: "for official billing data, refer to your
API provider"; OpenAI computes authoritative cost server-side in the Costs
API). Hinted and projected costs are never blended with each other or
presented as billing.

Seed pricing table (verified 2026-06-07 against provider pricing pages;
per 1M tokens, USD; `cache_write` is Anthropic-style only — OpenAI does not
bill cache writes, Google bills reduced cached-input plus storage/hour):

```text
claude-opus-4-8     input 5.00  output 25.00  cache_read 0.50  cache_write 6.25
claude-sonnet-4-6   input 3.00  output 15.00  cache_read 0.30  cache_write 3.75
claude-haiku-4-5    input 1.00  output  5.00  cache_read 0.10  cache_write 1.25
gpt-4o              input 2.50  output 10.00  cache_read 1.25
o3                  input 2.00  output  8.00  cache_read 0.50
gemini-2.5-pro      input 1.25  output 10.00  cache_read 0.13
gemini-2.5-flash    input 0.30  output  2.50  cache_read 0.03
pricing_version = "2026-06-07-v1"
```

Because event totals are cached-inclusive, the calculator prices per leg as
`(input_tokens − cache_read − cache_creation) · input_rate + cache_read ·
cache_read_rate + cache_creation · cache_write_rate + output_tokens ·
output_rate`.

Natural surfaces: `bicameral spend [--export csv|jsonl]` CLI, read-only
`GET /api/v1/telemetry/spend` rollup route, and a `token_spend` section in
the MCP `usage_summary` tool.

## Conformance Fixtures And Acceptance Criteria

Fixtures in `protocol/schemas/fixtures/token_spend_*.json`, consumed by
both repos' tests (counts shown cached-inclusive per the normalization
rule):

```json
{"event":"token_spend","event_id":"00000000-0000-4000-8000-000000000001","ts":"2026-06-01T10:00:00Z","schema_version":1,"surface":"caller_llm","actor":"bicameral.bind","workspace_id":"a1b2c3d4e5f60718","session_id":"sess-001","workflow":"bind","provider":"anthropic","model":"claude-sonnet-4-6-20260514","model_normalized":"claude-sonnet-4-6","input_tokens":2000,"output_tokens":800,"cache_read_input_tokens":500,"token_source":"estimated_tiktoken","trust":"opaque","errored":false}
{"event":"token_spend","event_id":"00000000-0000-4000-8000-000000000002","ts":"2026-06-01T10:02:00Z","schema_version":1,"surface":"caller_llm","actor":"bicameral.preflight","workflow":"preflight","provider":"anthropic","model":"claude-sonnet-4-6","input_tokens":15200,"output_tokens":1400,"cache_read_input_tokens":12100,"cost_hint_usd":0.021,"token_source":"caller_telemetry","trust":"reported","errored":false}
{"event":"token_spend","event_id":"00000000-0000-4000-8000-000000000003","ts":"2026-06-01T10:05:00Z","schema_version":1,"surface":"daemon_llm","actor":"daemon.extract","workflow":"ingest","provider":"local.openai_compatible","model":"claude-haiku-4-5","input_tokens":3200,"output_tokens":1200,"duration_ms":1850,"token_source":"provider_usage","trust":"measured","errored":false}
{"event":"token_spend","event_id":"00000000-0000-4000-8000-000000000004","ts":"2026-06-01T11:00:00Z","schema_version":1,"surface":"cloud_advisory","actor":"cloud.cross_repo_drift","workflow":"drift","provider":"bicameral","model":"internal","input_tokens":8000,"output_tokens":2500,"cost_hint_usd":0.042,"token_source":"provider_usage","trust":"opaque","errored":false}
```

Acceptance criteria:

- Rust and Python implementations round-trip all fixtures through writer
  and reader with no field loss; a line with an unknown extra field is
  still readable (forward compat).
- Rotation triggers at size/age limits and keeps exactly 5 files.
- `BICAMERAL_TELEMETRY=0` and `BICAMERAL_TOKEN_SPEND=0` each produce zero
  writes.
- Read-time validation flags (does not reject): daemon+non-measured,
  caller+measured, measured+cost_hint, zero/negative counts.
- bicameral-bot: dependency-graph test proves `bicameral-telemetry` has no
  path to `bicameral-api`/`runtime`/`gateway`.
- bicameral-mcp: a test proves the relay payload contains no
  `governed_object_id` and no string content beyond controlled enums.
- MCP emission degrades gracefully: missing counts → zero events, never a
  tool failure.

## Implementation Phases (Disjoint Write Scopes)

- **Phase 0 — shared contract (bicameral-bot):**
  `protocol/schemas/token_spend_event.schema.json` + fixtures + README
  listing under observation contracts.
- **Phase 1 — caller emission (bicameral-mcp), the required slice:**
  `token_spend.py` writer (clone `preflight_telemetry.py` patterns);
  estimation via the already-pinned `tiktoken`; emission from the existing
  `skill_end` call sites; `telemetry_flags.py` + kill switch;
  `usage_summary` extension; tests.
- **Phase 1.5 — optional rung-2 receiver:** local OTLP intake (or a
  documented token-map-style collector config) writing
  `token_source: caller_telemetry` events.
- **Phase 2 — bot read surfaces + writer crate (bicameral-bot):**
  `crates/bicameral-telemetry` leaf crate (record/read/rollup/rotation +
  pricing calculator); `BicameralPaths.telemetry_dir`; config section;
  `bicameral spend` subcommand; `GET /api/v1/telemetry/spend`;
  dependency-graph test.
- **Phase 3 — daemon emission:** enrichment worker records
  `trust: measured` after each call (when daemon-LLM lands).
- **Phase 4 — cloud projection:** local client projects `ReasoningArtifact`
  usage metadata into `cloud_advisory` events; cloud never writes the sink.

## Reconciliation With Draft ADR-0024 (RFQ #110)

The deprecated branch `docs/110-token-spend-telemetry` carried a 490-line
draft. Adopted here: the single-event model with `surface`, the `trust`
tiers, governance-attachment fields with the not-a-foreign-key rule,
`flow_id`/`parent_event_id`, the leaf-crate structural enforcement and
dependency-graph test, the versioned local pricing table + override, the
fixtures/acceptance/phases scaffolding, the relay Worker contract details,
and the cloud `ReasoningArtifact` projection. Overridden, with reasons:

1. **Input-token semantics**: draft defined `input_tokens` as non-cached
   only (Anthropic wire format); this spec uses cached-inclusive totals
   (OTel semconv; OpenAI/Gemini report totals that way) with cache subsets
   — cross-provider events must agree on what `input_tokens` means.
2. **Write-time cost**: draft stored `cost_estimate_usd` per event; this
   spec derives cost at read time from the versioned table and stores only
   **received** `cost_hint_usd` — per the meeting's third-party-provider
   caveat and the vendors' own "cost is approximate, billing is
   authoritative elsewhere" posture.
3. **Caller count source**: draft's preferred skill-prompted self-report is
   rejected (models confabulate their own usage); replaced by the capture
   ladder (estimation → caller harness OTel → in-protocol reporting).
4. **Two event names** (this spec's own earlier draft) collapsed into the
   single event + `surface`, which the meeting's "two events" maps onto as
   the two required v1 emission paths.

## Open Questions

- Sign-off on the one-event/`surface` resolution of the meeting's "two
  token spend events" (this spec's recommendation, backed by the
  three-surface model).
- v1.5 rung-2 capture: ship a tiny built-in OTLP receiver, or only document
  a collector config that writes into `token_spend.jsonl`?
- Should Bicameral pursue official MCP registry listing so
  `mcp_server.name` attribution survives Claude Code's third-party
  redaction to `"custom"`?
- Is the MCP local embedder in scope for v1 events (this spec: yes, cheap
  to emit, `workflow: index`)?
- Relay cardinality: per-call locally, aggregate at the relay — confirm.
- Remote pricing refresh stays off by default (`BICAMERAL_PRICING_REMOTE`)
  — confirm.
