# Runtime Architecture: Mandatory Reasoning Candidate Projection

Date: 2026-06-12
Status: draft
Level: L2 cross-repo contract
Owns: The live-system contract for routing preflight observation, local ingest, and external ingest through mandatory reasoning-assisted candidate projection.
Related context: `docs/specs/mandatory-reasoning-candidate-projection.md`, ADR-0002, ADR-0004, ADR-0007, ADR-0014, ADR-0015, ADR-0020, bicameral-bot#261.
Governance gate: `bic:spec-governance-gate` advisory/no-op; Qor integration disabled pending factory policy revision.

## Reading Guide

Read the feature proposal first, then this runtime spec. Main runtime invariant: **source/evidence may be observed deterministically, but candidate projection must pass through a reasoning projection stage before a `DecisionCandidate` is materialized.**

## Runtime Problem

The current runtime has three partially overlapping paths:

- `ingest.submit_local` creates a candidate directly from caller fields.
- `/api/v1/external-ingest` creates candidates from hints and also creates generic default candidates from evidence-only payloads.
- `preflight.run` is deferred, leaving the automatic MCP skill-trigger path without useful alpha behavior.

The missing runtime contract is a shared projection stage that makes MCP preflight, local ingest, and integrations symmetric without granting authority to LLM output.

## Goals

- Add `preflight.observe` as an alpha ToolRequest command.
- Add a `ReasoningProjectionService` runtime boundary.
- Require reasoning output before materializing `DecisionCandidate`s.
- Use `local_cli` as the alpha default daemon reasoning mode.
- Preserve source/evidence capture as distinct from candidate projection.
- Return typed provider/projection failures without silent candidate creation.
- Keep canonical state changes behind event-store materialization and review commands.

## Non-Goals

- Do not implement `preflight.run` Constraint Lookup.
- Do not implement blocking enforcement or `workgate.evaluate`.
- Do not require hosted providers for local operation.
- Do not turn runtime traces, provider outputs, or `ReasoningArtifact`s into canonical Decisions.
- Do not rewrite dashboard review authority or event store substrates.

## Runtime Components

| Component | Responsibility | Inputs | Outputs | Persistent state | Must not |
| --- | --- | --- | --- | --- | --- |
| MCP server | Present `bicameral.preflight`; assemble local task/session context. | Agent task, files/diff hints, command output, caller LLM hints. | `ToolRequest(preflight.observe)`. | None canonical. | Write ledger, signoff, compliance, BindingEvidence, or materialized DecisionCandidates. |
| ToolRequest Dispatcher | Validate authority and route commands. | `ToolRequest`. | `ToolResponse` with trace. | None directly, except through adapter calls. | Treat provider output as authority. |
| ReasoningProjectionService | Convert observed evidence and hints into candidate projections. | Source/evidence, hints, provider config. | Candidate projections, rationale, provenance, typed failures. | Optional advisory artifacts. | Materialize Decisions or approve review commands. |
| Reasoning provider adapter | Execute daemon-managed local or hosted reasoning mode. | Scoped projection request. | JSON projection output. | Provider-specific logs/artifacts. | Receive unredacted secrets or write event store. |
| EventStoreAdapter | Persist accepted ingest/projection events. | Validated event payloads. | Durable events. | Canonical accepted events and evidence blobs where substrate supports them. | Infer candidates. |
| External ingest gateway | Validate external envelopes and invoke projection. | `ExternalIngestEnvelope`. | Evidence/projection response. | Through event store and state. | Accept local authority or review commands. |
| Dashboard | Configure reasoning/model providers; render candidates, evidence, and provider provenance. | Provider settings, projections/read models. | Review commands and provider configuration updates. | UI state plus safe non-secret config writes. | Store provider secrets in shared workspace config or treat projection as signoff/compliance. |
| Provider config store | Preserve provider mode and non-secret settings; keep secrets local. | Dashboard/CLI config updates. | Runtime provider configuration. | Workspace-safe config plus user-local secrets. | Leak API keys into tracked or synced workspace files. |

## Canonical vs Derived State

| Artifact | Class | Authoritative writer | Mutation path | Prohibited shortcut |
| --- | --- | --- | --- | --- |
| `Source` | evidence metadata | Bot runtime/gateway | accepted ingest/projection event | MCP direct file write |
| `SourceSnapshot` | evidence | Bot runtime/gateway | accepted ingest/projection event | provider-owned snapshot identity |
| `SourceEvidence` / `EvidenceReference` | evidence | Bot runtime/gateway | accepted ingest/projection event | LLM rationale as evidence |
| Candidate projection output | candidate/proposal | ReasoningProjectionService validates provider output | materialized only through bot event path | provider writes candidate directly |
| `DecisionCandidate` | candidate/proposal | Bot runtime after projection validation | accepted ingest/projection event | evidence-only default candidate |
| `ReasoningArtifact` | advisory artifact | hosted/local provider boundary | provider lifecycle, referenced by provenance | canonical ledger replay dependency |
| `RuntimeTrace` | operational receipt | dispatcher | response/log emission | proof of governance acceptance |
| Provider configuration | config | dashboard/CLI through config writer | config update path with secret separation | ad hoc environment scraping as durable config |
| `Decision` | canonical state | event-store replay after `accept_candidate` | review command accepted by policy | projection creates Decision |

## Authority Ownership Matrix

| Actor/component | Observe | Propose | Validate | Accept | Materialize | Project | Render | Mutate |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| MCP/caller LLM | yes | hints/drafts | no verified facts | no | no | no | tool response formatting | no |
| ReasoningProjectionService | selected evidence | candidate projections | schema/provider output | no | no | advisory candidates | no | no |
| ToolRequest Dispatcher | request metadata | no | authority/payload | no | via adapter only | no | response | no direct state |
| Governance policy | no | no | state/capability | review commands | no | no | allowed commands | no direct storage |
| EventStoreAdapter | no | no | append rules | no | accepted events | replay source | no | canonical event append |
| Dashboard | no | review commands | no | no | no | no | yes | no direct canonical |

## Runtime Scenarios

### Startup

Trigger: MCP starts.

Flow:

```text
MCP -> daemon capability handshake
daemon -> CapabilityReport {
  supported_commands: [..., "preflight.observe", ...],
  deferred_commands: ["preflight.run", "review.resolve_compliance", ...],
  reasoning_modes: configured/available modes
}
```

Failure: if no projection-capable reasoning mode exists, MCP may start but must
mark `bicameral.preflight` as projection-deferred or prompt the user to
configure a provider.

Resolved alpha cut line (#424): candidate-producing commands must return typed
`projection_deferred` / provider-unavailable outcomes and create no
DecisionCandidates when no projection-capable provider exists. Explicit
evidence-capture commands may persist Source/Snapshot/Evidence while projection
is deferred.

### Provider Configuration

Trigger: dashboard or CLI opens reasoning/model settings.

```text
Dashboard reads current provider capability/config state
User selects provider mode:
  local_openai_compatible | local_cli | hosted_bicameral
User configures non-secret settings and task toggles
Secrets are stored in user-local secret storage or environment-specific location
Daemon reloads or receives provider config update
CapabilityReport reflects projection-capable modes
```

Expected result: the dashboard can show whether mandatory candidate projection is ready before `preflight.observe`, local ingest, or external ingest attempts candidate creation.

Failure: missing secrets, unreachable endpoint, invalid CLI command, or hosted
auth failure returns typed provider configuration/readiness errors and does not
create candidates.

Projection provider readiness is setup state. It may be rendered in Settings /
Sources / Connections and in command responses, but it must not appear as an
Ingestion Gate queue item unless a DecisionCandidate exists.

Alpha default: `local_cli`. The dashboard and CLI should make this the first setup path. Local OpenAI-compatible and hosted Bicameral modes are follow-up provider options unless explicitly configured.

### MCP Alpha Preflight

Trigger: automatic skill calls `bicameral.preflight`.

```text
MCP observes task/session context
MCP optionally uses caller LLM to produce hints
MCP sends ToolRequest(preflight.observe)
dispatcher validates AuthorityContext
runtime validates evidence and redaction gates
ReasoningProjectionService invokes daemon-managed provider over evidence and hints
runtime materializes Source/Snapshot/Evidence and projected candidates
ToolResponse returns ids, provenance, and trace
```

Expected result: reviewable candidates exist only when projection succeeds.

### External Ingest

Trigger: Jira/Slack/GitHub adapter posts envelope.

```text
gateway validates forbidden fields
gateway records source/evidence
gateway passes content/hints to ReasoningProjectionService
service returns candidate projections or projection_deferred
gateway appends accepted evidence/projection event(s)
response returns evidence ids and candidate ids if any
```

Failure: provider unavailable yields typed `projection_deferred`; no generic default candidate.

### Manual Local Ingest

Trigger: CLI/dashboard/MCP submits `ingest.submit_local`.

Flow is identical after source/evidence validation. If caller submits
`candidate_drafts`, the projection service treats them as advisory hints,
invokes daemon-managed projection, and attaches provenance. If no daemon
provider is configured, no candidate is created.

Alpha state mapping:

| State | Transition authority | Persistence | Notes |
| --- | --- | --- | --- |
| `candidate_projected` | ReasoningProjectionService output validated by daemon, then event adapter materialization | accepted ingest/projection event and read model | creates DecisionCandidate proposals only |
| `no_candidate` | ReasoningProjectionService handled request with no decision-like claim | trace/response; no candidate queue row | debug/audit visibility only in alpha |
| `projection_deferred` | dispatcher/runtime when provider missing, unavailable, timed out, or projection cannot run | response/readiness metadata; evidence may persist for explicit capture commands | repair through provider setup, not candidate review |
| `hard_rejected` | dispatcher/gateway validation and policy gates | rejection response/audit | malformed, forbidden, sensitive, or invalid output paths |

Deferred states: automatic retry, same-claim convergence, near-duplicate
conflict, manual retry UI, no-candidate browser, and richer logged-only
diagnostics.

Minimization review: provider-specific errors remain metadata under
`projection_deferred` unless validation makes the request unsafe. `no_candidate`
is not promoted to a user-facing review state because it has no candidate
action.

### Review And Materialization

Projected candidates enter the existing review flow:

```text
Dashboard/MCP/CLI emits review.accept_candidate
dispatcher/gateway validates actor and state
EventStoreAdapter appends AcceptCandidate
replay projects Decision
signoff remains proposed until separate command
```

### Forbidden Flow

```text
provider output -> Decision
provider output -> approve_signoff
provider output -> resolve_compliance
provider output -> BindingEvidence::verified
provider output -> event store direct write
```

All are forbidden. Provider output must pass through candidate projection and review.

## Event, Command, And Protocol Contracts

### `preflight.observe`

Purpose: alpha pre-work observation and candidate projection.

Producer: MCP, CLI, dashboard test harness.

Consumer: ToolRequest Dispatcher.

Payload:

- `source_uri`
- `source_type`
- `title`
- `description`
- `snapshot_content`
- `evidence[]`
- `candidate_hints[]`
- `binding_hints[]`
- `rationale`
- `reasoning_mode`
- `metadata`

Validation:

- requires valid `AuthorityContext`;
- requires at least one evidence item, candidate hint, or snapshot content;
- rejects forbidden authority, signoff, compliance, policy, and verified-graph fields;
- requires daemon projection-capable reasoning mode before creating candidates.

Idempotency: request id should dedupe retries when the same source snapshot and projection request repeats. Initial implementation may return duplicate-detected as typed rejection or same ids if persisted request receipts exist.

Compatibility: additive v2 ToolRequest command; MCP v0.2 direct payload compatibility is not preserved.

### Reasoning Projection Request

Internal contract:

- source id/snapshot id;
- evidence excerpts;
- optional candidate hints/drafts;
- optional binding hints;
- source type/kind;
- task kind: `candidate_projection`;
- policy limits and redaction status.

Response:

- projections: title, body, suggested level, labels, evidence refs, confidence/rationale;
- provider provenance;
- advisory warnings;
- artifact id when available.

### `local_cli` Provider Process Contract

Purpose: alpha default daemon-managed reasoning provider.

Producer/caller: `ReasoningProjectionService`.

Consumer/handler: configured local command.

Invocation:

```text
<configured-command>
```

The daemon writes one `reasoning_projection.v1` JSON object to stdin and reads one JSON object from stdout. Each projection request gets a fresh process invocation in the alpha contract.

Input fields:

- `schema_version`: `reasoning_projection.v1`.
- `task_kind`: `candidate_projection`.
- `request_id`.
- `source`: `source_uri`, `source_type`, `title`, `description`.
- `snapshot_content`.
- `evidence[]`: excerpt, span, confidence.
- `candidate_hints[]`: advisory caller/integration hints.
- `binding_hints[]`: advisory code hints.
- `constraints`: `max_candidates`, `allowed_levels`.

Output fields:

- `schema_version`: `reasoning_projection.v1`.
- `status`: `ok`, `no_candidate`, or `error`.
- `projections[]`: title, body, suggested_level, labels, evidence_indexes, confidence, rationale.
- `warnings[]`.

Validation:

- exit code `0` with valid JSON means handled;
- `status = no_candidate` is a successful handled response that creates no candidates;
- non-zero exit maps to `provider_unavailable`;
- invalid JSON maps to `provider_invalid_output`;
- output containing signoff, compliance, policy grants, verified graph state, canonical decision ids, or direct event-store instructions is rejected.

Idempotency/retry:

- the daemon may retry only when provider execution fails before producing valid output;
- provider output is not canonical until the daemon validates and materializes accepted projection events.

### Projection Events

Implementation may reuse `EventType::Ingest` initially if payload clearly records projection provenance. A later cleanup may add `EventType::ProjectCandidates` if event semantics need separation.

## Substrate Capability Matrix

| Substrate | Persist canonical state | Persist evidence | Emit events | Host projections | Enforce policy | Identity | Offline | Team-wide |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Local MCP | no | transient only | no | no | no | supplies session authority | yes | no |
| Bot runtime | no direct canonical store | coordinates | via adapter | yes | yes | validates ToolRequest authority | yes | no |
| Git EventStore | yes | yes | yes | replay | via review/CI where configured | actor in events | yes | via git |
| Drive/basic-store | yes | yes | yes | replay | warnings/queued state | actor in events | partial | via sync |
| Local CLI provider | no | no | no | advisory output | no | local process | yes | no |
| Hosted Bicameral | no local canonical authority | advisory artifacts | no local events | hosted advisory | no local bypass | hosted auth | no | yes |

## Deployment And Runtime Topology

Local alpha topology:

```text
MCP process
  -> local bot daemon/gateway ToolRequest endpoint or client
  -> bicameral-runtime dispatcher
  -> ReasoningProjectionService
       -> daemon-managed local provider OR hosted provider
       -> caller/integration hints are inputs only
  -> EventStoreAdapter
  -> read projections/dashboard
```

Required implementation note: if the gateway still lacks a generic ToolRequest route, add one or provide an equivalent daemon client path before claiming MCP thin-client E2E coverage.

Provider configuration topology:

```text
Dashboard settings page / CLI config command
  -> config writer
       -> workspace-safe non-secret provider settings
       -> user-local secret store or environment reference
  -> daemon provider registry
  -> CapabilityReport.reasoning_modes
```

## Error, Retry, And Recovery Behavior

| Failure | Behavior |
| --- | --- |
| Missing provider for projection | typed `projection_deferred` / `provider_unavailable`; no candidate created. |
| Provider returns malformed JSON | typed `provider_invalid_output`; no candidate created. |
| Provider timeout | typed `provider_unavailable`; trace includes latency/failure reason. |
| Sensitive data detected | reject before provider call. |
| Missing provider configuration | dashboard shows setup-required; projection commands return typed provider_config_missing. |
| Invalid provider secret | dashboard shows auth/config failure; no candidate projection. |
| Local CLI provider exits non-zero | typed provider_unavailable or provider_invalid_output based on stderr/output. |
| Empty observation | reject as malformed/empty evidence. |
| Duplicate request id | return existing result if receipt store supports it, otherwise typed duplicate rejection. |
| Event append conflict | no partial candidate claim; return event append error. |
| Hosted provider unavailable | local modes may be tried only if policy permits fallback; otherwise typed failure. |

## Observability, Audit, And Receipts

Every projection response should include or record:

- request id;
- command;
- caller surface;
- actor id;
- workspace/session id;
- reasoning mode;
- provider;
- model or profile;
- task kind `candidate_projection`;
- token usage when known;
- latency;
- status;
- failure reason;
- source/evidence/candidate ids touched.

Receipts explain runtime behavior. They do not approve candidates or prove decision meaning.

## Security, Privacy, And Authority Boundaries

- Redaction and sensitive-data checks run before provider calls.
- Provider payloads include only scoped source content needed for candidate projection.
- Provider output is normalized and validated before materialization.
- Local provider configuration must not persist secrets in shared workspace config.
- Hosted provider use must be explicit workspace policy, not silent default.
- Dashboard configuration may write safe provider mode, endpoint URL, task toggles, and profile names to workspace config only when they are non-secret and intended to be shared.
- API keys, bearer tokens, hosted session secrets, and local CLI credentials must live in user-local secret storage or environment references.
- External integrations cannot inject `AuthorityContext`.
- MCP cannot claim verified graph evidence through projection output.

## Implementation Sequencing

1. ADR/protocol amendment
   - Files: ADR-0014, ADR-0015, protocol schemas, fixtures.
   - Acceptance: `preflight.observe` contract and mandatory projection states documented.
   - Task class: `AFK_REVIEW_ONLY`.

2. Projection service interface
   - Files: `crates/bicameral-runtime`, `crates/bicameral-api`.
   - Acceptance: provider trait, caller-hint normalizer, malformed-output tests.
   - Task class: `AFK_REVIEW_ONLY`.

3. Provider configuration UI and config substrate
   - Files: dashboard settings, config schema, daemon provider registry.
   - Acceptance: user can configure alpha-default local CLI plus local OpenAI-compatible or hosted Bicameral daemon projection modes; secrets are not written to shared workspace config.
   - Task class: `AFK_REVIEW_ONLY`.

4. Runtime command wiring
   - Files: dispatcher, command registry, capability report.
   - Acceptance: `preflight.observe` supported; `preflight.run` remains deferred.
   - Task class: `AFK_REVIEW_ONLY`.

5. Gateway/daemon ToolRequest ingress
   - Files: gateway routes or daemon client.
   - Acceptance: MCP can send ToolRequest to runtime without legacy `/api/v1/ingest`.
   - Task class: `AFK_REVIEW_ONLY`.

6. External ingest behavior change
   - Files: gateway external ingest route/tests.
   - Acceptance: evidence-only no longer creates default candidate.
   - Task class: `AFK_REVIEW_ONLY`.

7. MCP prompt and mapping
   - Files: bicameral-mcp tool map/prompts.
   - Acceptance: automatic skill trigger calls `bicameral.preflight -> preflight.observe`.
   - Task class: `AFK_REVIEW_ONLY`.

## Issue Generation Notes

Use tracer bullets that each leave the system shippable:

- First land protocol fixtures and typed unsupported/deferred behavior.
- Then land projection with a fake provider fixture before real provider adapters.
- Land provider configuration UI/config substrate before making mandatory `local_cli` projection the alpha default.
- Then wire MCP alpha path.
- Keep hosted provider work separate from local daemon provider mode.

Resolved human decision: alpha default daemon reasoning mode is `local_cli`. Local OpenAI-compatible and hosted Bicameral are follow-up or explicitly configured provider options.

## ADR Implications

- ADR-0002: amend command registry and MCP mapping.
- ADR-0004: reaffirm HITL boundaries.
- ADR-0007: amend Candidate Projection stage.
- ADR-0014: amend optional reasoning to mandatory for candidate projection.
- ADR-0015: amend provider mode expectations.
- ADR-0020: reaffirm gateway authority boundary.

## Acceptance Criteria

- Capability report advertises `preflight.observe`.
- Dashboard exposes reasoning/model configuration described by ADR-0015: hosted sign-in, local OpenAI-compatible endpoint, local CLI adapter, and task toggles.
- Provider secrets are not persisted into shared workspace config.
- Capability report keeps `preflight.run` deferred until Constraint Lookup exists.
- Runtime tests prove candidates require reasoning projection.
- Runtime tests prove evidence-only paths do not create generic candidates.
- Provider failure creates no candidates.
- Projection output cannot set signoff, compliance, policy grants, or verified graph state.
- MCP mapping test proves `bicameral.preflight` reaches `preflight.observe`.
- External ingest tests prove evidence-only behavior changed.

## Validation Plan

- `cargo fmt --all -- --check`
- `cargo test -p bicameral-api --locked`
- `cargo test -p bicameral-runtime --locked`
- `cargo test -p bicameral-gateway --locked`
- schema fixture validation
- dashboard provider-configuration UI/contract test
- config redaction/secret-storage test
- MCP package mapping tests
- E2E alpha preflight observe harness with fake provider

## Open Questions

- Which local CLI command should the dashboard prefill or document as the default user-facing adapter?
- Which user-local secret store is canonical for provider credentials on each supported platform?
- Which explicit capture commands should persist source/evidence when
  projection defers beyond the alpha default: evidence may persist for explicit
  ingest, but candidate-producing commands still create no candidates.
- Should projection use existing `EventType::Ingest` or introduce `EventType::ProjectCandidates`?

## Request Coverage

- Mandatory daemon LLM candidate processing: Runtime Components, Runtime Scenarios, Error Behavior.
- `preflight.observe`: Command contract and MCP scenario.
- `bic:to-runtime-spec`: this document follows the runtime architecture blueprint.
