# Bot-MCP Data Flow Runtime Architecture

Date: 2026-06-10

Status: Draft runtime architecture spec

Scope: Bicameral bot local runtime, MCP thin-client surface, local daemon ToolRequest dispatcher, codegraph/cograph advisory grounding, source evidence, review commands, and read projections.

Related issues: `BicameralAI/bicameral-bot#208`, `#209`, `#216`, `#114`, `#115`, `#116`, `#117`, `#134`, `#207`, `#218`; `BicameralAI/bicameral-mcp#552`, `#554`, `#564`.

Related architecture: `protocol/README.md`, ADR-0002, ADR-0014, ADR-0015, ADR-0017, ADR-0019, ADR-0022, and `docs/specs/codegraph-ledger-ui-protocol-boundary.md`.

Governance gate: `bic:spec-governance-gate` advisory/no-op; Qor integration disabled pending factory policy revision.

## Reading Guide

Read `Runtime Invariant`, `Lifecycle Data-Flow Matrix`, `Canonical vs Derived State`, and `Forbidden Flows` first. Those sections define the boundary that every implementation slice must preserve.

This document uses `codegraph` as the canonical product term. Some linked issues say `cograph`; treat that as the same runtime concern unless a later ADR gives the term a narrower meaning.

## Runtime Invariant

The alpha daemon is operator-scoped and may host multiple registered Products.
Every Product-scoped `ToolRequest` therefore carries the canonical `product_id`
selected by the MCP session. The daemon resolves the Product's event-store
adapter and `LocalWorkspaceBinding` through operator-local registries. MCP must
not infer Product identity from its cwd, send a local path as Product identity,
or depend on a daemon process-global workspace. `workspace.bind` continues to
carry its target Product id in the proposal because it materializes the binding
itself.

MCP may observe, propose, format, and relay local work. Bot validates, accepts, materializes, and projects Bicameral state.

Caller LLM output, MCP code regions, binding hints, rationale, and command output are advisory until they pass through bot-owned validation or governance. Codegraph state is evidence, readiness, and advisory grounding; it is not compliance, enforcement, signoff, or ledger authority.

## Runtime Problem

The bot/MCP boundary is moving from MCP-owned handlers toward a bot-owned `ToolRequest -> ToolResponse` runtime. The current issue set identifies four risks:

- lifecycle data flow is not mapped phase by phase, so MCP and bot can duplicate or drop LLM work;
- MCP must remain useful as a local agent surface after losing ledger, graph, adapter, and governance ownership;
- code search is not ready to become the main contract for binding or source grounding;
- cold evidence, model compatibility, and graph readiness must be visible without letting advisory codegraph state masquerade as verified product state.

## Goals

- Define what data MCP sends to bot, what MCP computes locally, what bot computes, and what bot returns for ingestion, preflight, binding, verification, refresh, review, search, and history.
- Keep MCP functional as a thin local agent surface while removing MCP-owned canonical ledger, graph, binding, adapter, governance, and event-store behavior.
- Define the LLM responsibility boundary so MCP-side and bot-side reasoning can coexist without becoming authority.
- Preserve source and evidence links through ingest, review, search, history, inspection, and telemetry.
- Make unsupported or deferred states typed and observable rather than silently accepted.
- Align codegraph-ledger UI behavior with protocol behavior: graph evidence can guide review, but accepted governance events drive ledger state.

## Non-Goals

- No V1 compliance enforcement, code correctness guarantee, global readiness guarantee, or automated signoff.
- No hosted/team graph visibility as a local MCP contract.
- No Drive/source adapter runtime hidden inside MCP. Source adapters enter through bot-owned external ingest or another bot-owned gateway contract.
- No v0.2 direct payload compatibility.
- No validation-token acceptance for binding until signing, expiry, integrity, and revocation are specified.
- No binding search scope until bot-owned `BindingEvidence` materialization and search projection are implemented.

## Runtime Components

| Component | Runtime role | Owns | Must not own |
| --- | --- | --- | --- |
| MCP server | Stdio transport, `list_tools`, daemon handshake, tool-call mapping, `AuthorityContext` assembly, response formatting | Local surface compatibility and request shaping | Ledger state, event writes, graph verification, binding materialization, governance acceptance |
| MCP caller LLM | Optional local reasoning over prompt, files, command output, and tool results | Drafts, rationale, code-region hints, candidate text, query terms | Verified evidence, accepted decisions, signoff, compliance, `GraphEvidenceState::verified` |
| Bot daemon / gateway | Receives `ToolRequest`, validates schema and authority, dispatches commands, returns `ToolResponse` | Protocol compatibility, policy checks, runtime command routing | Caller-local session authority beyond supplied `AuthorityContext` |
| `bicameral-runtime` dispatcher | Canonical local `ToolRequest -> ToolResponse` path | Command registry, typed failures, service orchestration | Surface-specific UX formatting |
| EventStoreAdapter | Appends accepted events and replays materialized state | Canonical Decision Ledger state | Advisory hints or cache-only projections as canonical state |
| Source/evidence service | Creates `Source`, `SourceSnapshot`, `SourceEvidence`, and `EvidenceReference` from accepted ingest | Evidence identity and immutable source pointers | Review approval, signoff, compliance |
| Reasoning provider service | Optional daemon-side or hosted enrichment | `ReasoningArtifact` and advisory summaries | Governance authority, direct event-store writes |
| Graph/snapshot/binding services | Resolve graph snapshots, symbol identity, evidence state, and binding currentness | `GraphSnapshot`, graph readiness, verified binding facts when accepted | Compliance or signoff |
| Preflight/read/search services | Query replayed state, source evidence, graph readiness, and projections | Read-only relevance and readiness views | Mutation or policy resolution |
| Dashboard / Ledger View | Renders selected Decisions and reviewable context | UI projection | Treating graph evidence as ledger authority |
| External gateway/adapters | Source integration ingress outside MCP | `ExternalIngestEnvelope` evidence-only path | Local actor authority, review commands, signoff, compliance |

## Canonical vs Derived State

Canonical state is materialized only by accepted events replayed through the event store. This includes tracked Decisions, candidate review transitions, signoff transitions, and future accepted binding events.

Evidence state includes immutable source snapshots, source evidence, evidence references, and verified bot-owned binding evidence. Evidence can support review but does not itself approve work.

Proposal state includes `DecisionCandidate`, `BindingHint`, `BindingProposal`, caller LLM drafts, suggested levels, rationale, and suggested code regions. Proposal state can be shown, searched, or reviewed, but it is not canonical until accepted by the relevant bot-owned path.

Derived projections include `history.list`, `search.query`, preflight results, graph readiness, binding currentness, Ledger View context panes, and source-link inspection. Projections must be recomputable from canonical state plus evidence stores.

Caches and scratch state include MCP helper caches, local session context, graph indexes, search indexes, operational receipts, and telemetry spans. They may improve performance or explain work, but cannot establish authority.

## Forbidden Flows

- MCP writes ReviewEvents, GovernanceResults, Decisions, Signoffs, or BindingEvidence directly.
- MCP, caller LLM, or hosted reasoning emits `GraphEvidenceState::verified`.
- `search.query` materializes binding, compliance, signoff, or review state.
- `preflight.run` mutates Decisions, candidates, signoffs, compliance, or bindings.
- External ingest carries local actor authority, review commands, policy grants, signoff approval, or compliance resolution.
- Caller rationale, model output, or metadata is treated as verified source evidence.
- Legacy MCP payloads are accepted without explicit v2 mapping and validation.

## Authority Ownership Matrix

| Action | MCP | Bot daemon/runtime | Event store | Graph/binding service | LLM providers | Human/reviewer |
| --- | --- | --- | --- | --- | --- | --- |
| Observe local workspace/session | Yes | Read as request input | No | Read scoped inputs | Optional | Yes |
| Assemble `AuthorityContext` | Yes | Validate | No | No | No | Provides actor authority |
| Accept/reject candidate | Relay command | Validate and dispatch | Materialize accepted event | No | May summarize | Decides |
| Approve/reject signoff | Relay command | Validate and dispatch | Materialize accepted event | No | May summarize | Decides |
| Create source evidence | Submit local evidence payload | Validate and create | Materialize accepted event if required | No | May draft excerpts | Supplies review context |
| Verify graph binding | Submit hint | Validate command | Materialize only if accepted | Compute evidence state | May propose target | Reviews where required |
| Resolve compliance | Deferred/unsupported in V1 | Typed unsupported | No | No | No | No V1 support |
| Search/history | Relay query | Read projection | Replay source | Enrich where supported | May propose query | Reads result |

## Lifecycle Data-Flow Matrix

| Phase | MCP sends | MCP computes | Bot computes | Bot returns | Authority rule |
| --- | --- | --- | --- | --- | --- |
| Startup / capability | Protocol version, surface metadata, requested tools | Daemon endpoint discovery and stdio startup | Capability handshake, supported command list, protocol compatibility | Compatible capability set or startup refusal | MCP refuses start when daemon protocol is incompatible |
| Ingestion | `ingest.submit_local` ToolRequest with `AuthorityContext`, source pointer, excerpt, command/tool output, optional drafts, rationale, suggested level, metadata | Local context packaging; caller LLM may draft candidate text or source hints | Schema/authority validation, forbidden-field rejection, evidence floor checks, source/evidence creation, candidate creation where accepted | Source, snapshot, evidence, candidate ids, typed rejection, or deferred reason | Caller evidence is advisory until bot validates; level is governance-owned or inferred |
| Preflight / constraint lookup | `preflight.run` request with workspace, files/diff/symbols, requested snapshot scope | Optional query terms, code-region hints, rationale | Exact local snapshot resolution, replay/search lookup, graph readiness, relevance state, compliance-readiness warnings | Relevant Decisions/candidates, source links, snapshot scope, unknown/stale/unsupported warnings | Read-only. Local no-conflict is not global consistency |
| Binding proposal / create | `binding.create` ToolRequest with `BindingHint`, target identifiers, source context | Optional LLM target proposal/explanation | Graph snapshot lookup, symbol validation, evidence-state classification, policy checks | Verified binding evidence if accepted; otherwise unsupported/stale/ambiguous/not-indexed/approximate/snapshot-mismatch/unavailable | Only bot validation can produce verified binding evidence; token acceptance deferred |
| Binding inspect / verification | `binding.inspect` query | No authority computation | Read accepted binding evidence, current graph snapshot, validated SHA, readiness warnings | Binding status, provenance, currentness, source links | Inspection does not approve signoff or compliance |
| Search/history | `search.query` or `history.list` with filters and scope | Optional query shaping | Replay/read-model lookup, source-link projection, scope support checks | Decisions, candidates, evidence excerpts, source links, typed unsupported scope | Binding scope hidden/unsupported until binding evidence projection exists |
| Review / promotion / signoff | Review command ToolRequest | Request formatting only | Actor/policy/state/idempotency validation | Accepted/rejected transition and touched ids | Actor/audit/promoted_by derive only from `AuthorityContext.actor_id` |
| Source refresh / revalidation | Refresh/revalidation request or trigger | May identify changed files | New source snapshot, graph readiness/currentness, stale evidence warnings, revalidation proposal | Updated evidence/readiness projection or candidate review item | Old evidence remains immutable; refresh does not assert compliance |
| External adapters | None through MCP | None | External gateway validates `ExternalIngestEnvelope` | Evidence-only ingest outcome | External source integrations do not use local `ToolRequest` authority |
| Observability | Request id, caller surface, session/workspace metadata | Correlation id propagation | Status, failure reason, touched ids, provider spans, graph ids | Operational receipt and telemetry | Receipts explain runtime behavior; they are not governance state |

## Runtime Scenarios

### 1. Startup Handshake

MCP starts as a thin client and asks the bot daemon for ToolRequest protocol and command capabilities. If the daemon does not support the required protocol version, MCP refuses startup. After a compatible handshake, individual unsupported commands return typed daemon capability errors instead of falling back to old MCP-owned handlers.

### 2. Local Ingest Golden Path

MCP captures local session context, command output, code hints, or user-selected excerpts. A caller LLM may draft candidate text or rationale. MCP sends `ingest.submit_local` with `AuthorityContext` and source-backed evidence. Bot rejects missing authority, malformed payloads, unsafe or unverifiable evidence, canonical state fields, signoff/compliance fields, and legacy shapes. Accepted ingest creates source and evidence identities and may create a reviewable candidate. Search, history, and inspection expose source links.

### 3. Preflight / Constraint Lookup

MCP sends `preflight.run` with the local workspace scope, files, symbols, or diff. Bot resolves the exact local graph snapshot, searches replayed Decisions and candidates, classifies evidence and relevance, and returns readiness warnings. The response can include unknown, stale, unsupported, source-only, or graph-grounded states. It never mutates ledger, review, binding, signoff, or compliance state.

### 4. Binding Create / Inspect

MCP sends a `BindingHint`. Bot validates target identity against graph and symbol evidence. Verified evidence may materialize only through the bot-owned accepted path. Non-verified outcomes return typed states such as `unsupported`, `stale`, `ambiguous`, `not_indexed`, `approximate`, `snapshot_mismatch`, or `unavailable`. `binding.inspect` reads accepted evidence and derived currentness without changing state.

### 5. Review Commands

MCP relays review commands for candidate acceptance/rejection and signoff approval/rejection. Bot validates the actor, authority, state transition, and idempotency. Accepted commands append canonical events; replay updates the Decision Ledger. `review.resolve_compliance` remains typed unsupported/deferred in V1.

### 6. Read Projections

MCP uses `history.list` and `search.query` to read replayed state, evidence excerpts, and source links. Search remains a relevance/read-model contract. Binding search is not supported until bot-owned binding evidence and projection fields exist.

### 7. Source Refresh / Revalidation

Tracked Decisions can drive source refresh. Bot creates new source snapshots and graph readiness/currentness results. Old evidence remains immutable. Refresh may create a revalidation proposal or warning, but it cannot silently approve signoff, resolve compliance, or rewrite prior evidence.

### 8. Failure / Recovery

All failure modes must return typed outcomes: malformed request, missing or invalid authority, policy rejection, unknown command, unsupported capability, daemon unavailable, stale graph snapshot, partial graph coverage, provider unavailable, duplicate request id, and event append conflict. A failed or rejected request must not append canonical events.

## Protocol Contracts

`ToolRequest` is the canonical local surface contract. Common fields should include `request_id`, command name and params, `AuthorityContext`, issued timestamp, caller surface, workspace/session scope, and optional metadata.

`AuthorityContext` must carry actor identity and audit-relevant fields. Review ownership fields such as `promoted_by` derive from `AuthorityContext.actor_id`, not from rationale text, caller strings, or model output.

`ToolResponse` should distinguish accepted, malformed, missing/invalid authority, policy rejection, unknown command, unsupported capability, daemon unavailable, and runtime error. Responses should include touched ids where available.

`ingest.submit_local` accepts local actor evidence only. It may carry source pointers, excerpts, command output, candidate drafts, binding hints, rationale, `suggested_level`, and inert metadata. It must reject caller-supplied canonical fields, verified evidence assertions, signoff/compliance results, policy grants, and external integration envelopes.

`preflight.run` is a read-only constraint lookup. It returns relevant Decisions/candidates, source links, graph snapshot scope, readiness, evidence states, and warnings.

`binding.create` accepts hints and returns bot-classified binding outcomes. Verified materialization requires bot validation and the accepted event path. Validation-token acceptance is deferred.

`binding.inspect` reads accepted binding evidence and derived currentness.

`history.list` and `search.query` read projections. Binding search scope is unsupported until `BindingEvidence` materialization and projection are complete.

Source-adapter integrations use `ExternalIngestEnvelope`, not local
`ToolRequest`. Authenticated remote review/action callbacks are a separate path
defined by ADR-0002; they are not source ingest and must derive actor context
before submitting governed commands.

## Substrate Capability Matrix

| Substrate | Canonical events | Evidence | Projections | Policy enforcement | Identity/audit | Offline behavior | Recovery concern |
| --- | --- | --- | --- | --- | --- | --- | --- |
| MCP stdio server | No | No, except transient payloads | Formats bot responses | No | Supplies caller surface and authority context | Works only with compatible daemon | Refuse startup on incompatible protocol |
| Local daemon/runtime | Dispatches accepted commands | Coordinates evidence creation | Serves read commands | Yes | Validates actor/session/workspace | Local-first | Typed daemon unavailable/runtime errors |
| EventStoreAdapter | Yes | References evidence ids | Replay source | Append-time acceptance | Event audit trail | Depends on substrate | Append conflicts and replay repair |
| Source/evidence store | No | Yes | Source inspection | Evidence floor validation | Source provenance | Local snapshot retention | Immutable evidence plus refresh snapshots |
| GraphStore | No | Graph snapshots and derived readiness | Binding/preflight enrichment | No governance authority | Snapshot id and validated SHA | May be partial/stale | Unknown/stale/unsupported states |
| Reasoning provider | No | Advisory artifacts only | Summaries/hints | No | Provider/model/task spans | Provider optional | Provider unavailable must not block authority path |
| External source adapters | No direct local events | Evidence-only ingress | Adapter receipts | Ingest validation only | Integration provenance | Adapter-specific | Cannot carry local actor authority |
| Authenticated remote review actions | No direct local events | No source ingest | Action callback receipts | Actor/capability/policy checks | Bound Bicameral actor after callback verification | Online/provider-dependent | Cannot be inferred from source text, status, comment, label, or reaction |
| Dashboard/Ledger View | No | Renders evidence | UI projection | No | Displays audit | Read-only degraded mode | Projection staleness disclosure |

## Deployment Topology

The local topology is MCP process -> bot daemon/gateway -> `bicameral-runtime` dispatcher -> event store, source/evidence store, graph store, read models, and optional reasoning providers.

MCP owns stdio process lifetime and tool presentation. The bot daemon owns protocol compatibility and command execution. Graph and search indexes can be rebuilt or refreshed, but canonical ledger state comes from event replay. Hosted reasoning, when enabled, is an advisory provider behind bot-owned reasoning boundaries.

## Observability

Minimum telemetry fields: `request_id`, command, caller surface, actor id, auth method, workspace id, session id, protocol version, status, failure reason, touched decision/candidate/source/evidence/binding ids, graph snapshot id, validated SHA, provider, model, task kind, token usage, latency, and retry count.

Operational receipts may correlate MCP and bot work, but they are explanatory runtime artifacts, not governance records.

## Security and Privacy

MCP may observe local workspace and session context only within the user's local authority. It must relay evidence through ToolRequest instead of writing product state. Hosted or daemon LLM providers receive only the scoped inputs selected by the runtime path. Secrets and credentials must not be copied into rationale, metadata, telemetry, or invite flows.

External source adapters must not be reintroduced through MCP as hidden
authority. They require bot-owned ingress and evidence-only semantics.
Authenticated remote review/action callbacks are separate from source adapters
and require provider callback verification, actor binding, capability checks,
and governance policy acceptance before any canonical mutation.

## Implementation Sequencing

1. Land this runtime spec and protocol fixtures for the lifecycle matrix.
2. Implement the canonical dispatcher in `bicameral-runtime` for `ToolRequest -> ToolResponse` and migrate gateway/CLI/MCP call sites behind it.
3. Wire `ingest.submit_local` through ToolRequest with source/evidence creation and forbidden-field tests.
4. Implement the V1 advisory cograph golden path: MCP hint -> ToolRequest/ingest -> SourceSnapshot/EvidenceReference -> candidate/Decision source links -> search/history/telemetry correlation.
5. Complete grounded preflight as constraint lookup with snapshot scope, readiness, warnings, and no mutation.
6. Complete bot-owned binding create/inspect and `BindingEvidence` materialization for verified outcomes only.
7. Align `search.query` and `history.list` with source links, unsupported binding scope, and later binding projection fields.
8. Verify MCP thin-client conformance against the bot graph/search changes.
9. Move source adapters and Drive-like runtime ingress behind bot-owned external ingest or a separate bot-owned gateway contract.

RFQ/spec issues remain `HITL_BLOCKED` until accepted. Implementation issues can be `AFK_REVIEW_ONLY` when the spec and ADR authority boundaries are already accepted and the work is fixture-driven.

## ADR Implications

ADR-0002 is reaffirmed: MCP, CLI, TUI, and dashboard are surfaces; bot owns the local runtime interface.

ADR-0014 is reaffirmed and narrowed by the lifecycle matrix: caller LLM output may be submitted as hints, but bot-side validation/governance decides materialization.

ADR-0015 is reaffirmed: hosted or local reasoning artifacts are advisory and cannot bypass review.

ADR-0017 is reaffirmed: binding create/inspect are bot-owned ToolRequest commands, and validation-token acceptance remains deferred.

ADR-0019 is reaffirmed: preflight is read-only grounded lookup, not a governed work gate.

ADR-0022 is reaffirmed: V1 is decision signoff plus reviewable context, not compliance or enforcement memory.

No new ADR is required for this spec unless implementation introduces a new command family, accepts binding validation tokens, or gives external adapters local actor authority.

## Acceptance Criteria

- Each lifecycle phase documents MCP-sent data, MCP-local computation, bot computation, bot response, and authority rule.
- MCP remains a functional local surface after removing MCP-owned ledger, graph, adapter, governance, and event-store code.
- The spec has an explicit LLM responsibility boundary for MCP caller LLM, bot daemon LLM, and hosted reasoning.
- `ingest.submit_local`, `preflight.run`, `binding.create`, `binding.inspect`, review commands, `history.list`, and `search.query` have defined protocol responsibilities.
- Source and evidence links survive from ingest through candidate/Decision, search, history, inspection, and telemetry.
- Unsupported/deferred commands and scopes return typed responses.
- Negative authority cases are explicit: no MCP direct event writes, no LLM-verified graph state, no preflight mutation, no external local authority, and no search-backed binding materialization.

## Validation Plan

- Add protocol fixtures for accepted and rejected ToolRequest shapes.
- Add runtime dispatcher tests for malformed request, missing authority, policy rejection, unknown command, unsupported capability, daemon unavailable, and idempotent retry.
- Add ingest tests proving caller rationale is not verified evidence and forbidden canonical fields append no event.
- Add preflight tests proving read-only behavior, snapshot scope disclosure, stale/unknown warnings, and no compliance mutation.
- Add binding tests for verified and non-verified evidence states, including deferred validation-token acceptance.
- Add search/history tests proving source links are exposed and binding scope is typed unsupported until implemented.
- Add MCP conformance tests with a mocked daemon for startup handshake, tool mapping, typed unsupported commands, and no legacy handler fallback.

## Open Questions Resolved

`ingest.submit_local` should standardize on `source_uri`, `source_type`, `title`, `description`, `evidence[]`, `candidate_drafts[]`, `binding_hints[]`, `rationale`, `suggested_level`, and `metadata`.

MCP `history.list` and `search.query` should return both structured ids and a human inspection URI. Structured ids are the machine contract: `source_id`, `source_snapshot_id`, and `evidence_reference_ids[]`. `inspection_uri` is a stable display/deep-link target for dashboard, CLI, or MCP clients to open the relevant evidence without reconstructing a route from ids.

`search.query` should advertise `bindings` scope only after verified `BindingEvidence` is materialized and indexed with `binding_id`, `decision_id`, `target_identifier`, `target_kind`, `graph_snapshot_id`, `validated_sha`, `evidence_state`, and readiness warnings.

MCP and bot should share a `runtime_trace` object on receipts/logs with `request_id`, `command`, `caller_surface`, `actor_id`, `workspace_id`, `session_id`, `provider`, `model`, `task_kind`, `token_usage`, `latency_ms`, `status`, `failure_reason`, and touched ids.

`bicameral-bot` owns the first bot-owned external adapter gateway contract/runtime for Drive-like sources. MCP only consumes the resulting bot surface.

## Request Coverage

| Request / concern | Covered by |
| --- | --- |
| `#209` lifecycle data flow | `Lifecycle Data-Flow Matrix`, `Runtime Scenarios` |
| `#208` LLM responsibility boundary | `Runtime Invariant`, `Runtime Components`, `ADR Implications` |
| `#216` V1 advisory cograph golden path | `Local Ingest Golden Path`, implementation step 4 |
| Abdullahi concern: cold evidence/model compatibility | Preflight readiness, typed graph states, observability |
| Abdullahi concern: MCP remains local agent surface | MCP component role, startup handshake, thin-client conformance |
| Abdullahi concern: code search not main contract | Non-goals, search/history scope, forbidden flows |
| MCP `#552` thin client epic | Components, protocol contracts, startup handshake |
| MCP `#554` conformance | Validation plan and implementation step 8 |
| MCP `#564` terminology | Preflight as constraint lookup/advisory grounding |
