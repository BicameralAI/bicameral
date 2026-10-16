# Runtime Architecture: Shared Observation-to-Candidate Pipeline

Date: 2026-06-13
Status: draft
Level: L2 cross-repo contract
Owns: The daemon-owned runtime pipeline that turns observed source/session evidence from ToolRequest and external ingest entry points into replayable evidence, reasoning projection state, converged DecisionCandidates, review routing, and post-materialization conflict handling.
Related context: `CONTEXT.md`, ADR-0002, ADR-0004, ADR-0007, ADR-0011, ADR-0014, ADR-0015, ADR-0016, ADR-0018, ADR-0019, ADR-0020, `docs/specs/mandatory-reasoning-candidate-projection.md`, `docs/specs/mandatory-reasoning-candidate-projection-runtime-architecture.md`, `docs/specs/external-adapter-ingest-gateway.md`, bicameral-bot#261, bicameral-bot#270, bicameral-bot#315, bicameral-bot#320.
Governance gate: `bic:spec-governance-gate` advisory/no-op; Qor integration disabled pending factory policy revision.

## Reading Guide

Read ADR-0002, ADR-0007, ADR-0011, ADR-0016, ADR-0018, and the mandatory reasoning projection specs first. Main runtime invariant:

> All source/session observations, regardless of entry contract, must pass through the daemon-owned observation, projection, convergence, and review-routing pipeline before any `DecisionCandidate` materializes.

This spec separates the runtime gap from ToolRequest naming cleanup. ToolRequest command names may be renamed later, but the shared daemon pipeline is the durable architecture decision.

## Runtime Problem

Recent work implemented pieces of ingest and preflight:

- `ingest.submit_local` creates source/evidence and directly materializes a primary `DecisionCandidate` from caller title/description plus additional `candidate_drafts`.
- `/api/v1/external-ingest` validates `ExternalIngestEnvelope`, creates candidates from `candidate_hints`, and creates generic default candidates for evidence-only payloads.
- `preflight.run` has been implemented as daemon-owned read-only advisory lookup on merged `dev`.
- ADRs specify authority separation, HITL review, external ingest guardrails, clean projection defaults, and later collision release valves.

The under-specified runtime gap is the shared path between "valid evidence was observed" and "reviewable candidate exists":

```text
observed source/session evidence
  -> captured evidence
  -> daemon reasoning projection
  -> projection retry/deferred/no_candidate lifecycle
  -> candidate convergence
  -> candidate materialization
  -> review routing
  -> post-materialization conflict detection
```

Without this shared pipeline, each entry point guesses candidate semantics independently. That creates generic candidates, caller/integration-provided candidates that bypass daemon projection, inconsistent provider-failure behavior, and no first-class convergence rule for multiple bots observing the same decision.

## Goals

- Define one daemon-owned observation-to-candidate pipeline used by local ToolRequest ingest, MCP pre-work capture, external ingest, connector webhooks, and future hosted advisory inputs.
- Keep `ToolRequest`, `ExternalIngestEnvelope`, and future `Egress` semantically separate while allowing them to enter shared internal stages.
- Capture valid source/evidence independently from projection availability.
- Require daemon-managed reasoning projection before materializing any new `DecisionCandidate`.
- Record projection state as replayable runtime state sufficient to rebuild candidate review state without re-calling the model.
- Make projection retry automatic daemon recovery only.
- Run candidate convergence before candidate materialization.
- Keep conflict detection daemon-internal and event-triggered, with human resolution through governed review commands.
- Keep alpha preflight out of conflict resolution: it may surface possible
  contradictions and route proposed candidates/corrections, but it must not
  resolve, supersede, reject, sign off, or mark different scopes.
- Preserve `preflight.run` work as read-only lookup behavior, but treat command naming as a compatibility/migration issue rather than the core runtime decision.

## Non-Goals

- Do not rename the whole ToolRequest API before alpha.
- Do not redesign `ExternalIngestEnvelope` wire shape for connector setup work.
- Do not expose a manual conflict-detection or projection-retry tool.
- Do not expose MCP conflict-resolution commands in alpha; future MCP
  resolution requires an explicit authority gate and bot-owned governed review
  path.
- Do not make MCP the owner of lookup, conflict detection, enforcement, candidate convergence, or policy.
- Do not implement blocking enforcement or `gate.evaluate` in alpha.
- Do not make full prompts/completions canonical ledger authority.
- Do not require hosted Bicameral for local alpha.
- Do not let provider output create Decisions, signoff, compliance results, verified BindingEvidence, or direct event-store writes.

## Runtime Components

| Component | Responsibility | Process boundary | Inputs | Outputs | Persistent state | Must not decide or mutate |
| --- | --- | --- | --- | --- | --- | --- |
| MCP thin client | Active-session evidence surface and prompt/session bridge. | MCP process. | Task intent, files, diff hints, command output, caller hints, actor/session context. | ToolRequest for pre-work capture or local ingest; rendered daemon response. | None canonical. | Lookup authority, enforcement, conflict detection, conflict resolution, review acceptance, event writes. |
| Dashboard/CLI/TUI | Local actor surfaces for setup, reads, and governed actions. | Local UI/CLI process. | User commands, config, review actions. | ToolRequests and config updates. | UI state and safe local config only. | Direct ledger mutation or hidden provider secrets in shared config. |
| External integrations | Evidence producers outside local actor authority. | Connector/gateway caller. | Source content, source metadata, evidence excerpts, candidate hints. | `ExternalIngestEnvelope`. | Connector-local operational state. | AuthorityContext, review commands, signoff, compliance, canonical mutations. |
| Gateway guard layer | Reject unsafe external payloads before typed mapping. | Bot gateway. | Raw external ingest body. | Typed rejection or validated envelope. | Rejection logs/metrics only. | Silently ignore forbidden authority fields. |
| ToolRequest Dispatcher | Validate local actor authority and route local commands. | Bot runtime. | ToolRequest. | ToolResponse, RuntimeTrace, adapter calls. | RuntimeTrace/logs only unless routed stage appends events. | Treat local caller hints as candidate authority. |
| ObservationIngestor | Shared internal stage for valid source/session capture. | Bot runtime/gateway. | Validated local/external observation payload. | Source, SourceSnapshot, SourceEvidence, EvidenceReference, ProjectionRequested. | Accepted evidence/projection-request events. | Materialize candidates directly. |
| ReasoningProjectionService | Daemon-managed candidate projection over captured evidence and hints. | Bot runtime worker/service. | Captured evidence, advisory hints, provider config, projection profile. | ProjectionCompleted or ProjectionDeferred. | Normalized projection outcome events; optional artifact refs. | Accept review commands or write Decisions. |
| ProjectionRetryScheduler | Automatic retry for deferred projection. | Daemon worker. | Replayed deferred projection state and provider health/backoff. | Retry attempts through ReasoningProjectionService. | Operational retry queue is derived; retry outcome events are canonical enough for replay. | Expose user retry command in alpha. |
| CandidateConvergence | Idempotency and duplicate/conflict scan before candidate materialization. | Bot runtime. | Normalized projection output, current replayed candidates/decisions, evidence refs. | New candidate, converged provenance, duplicate flag, conflict flag, or no materialization. | Candidate materialization/provenance events. | Hide meaningful disagreement or choose conflict winner. |
| EventStoreAdapter | Durable accepted-event materialization. | Selected substrate adapter. | Accepted evidence/projection/candidate/review events. | Replayable event stream. | Canonical accepted events and evidence blobs where supported. | Infer decisions or bypass policy. |
| Projection/read model | Rebuild visible state from event replay. | Runtime/dashboard projection. | Event stream. | Candidates, decisions, evidence, projection status, conflict/readiness state. | Derived/disposable projection rows. | Become canonical authority. |
| CollisionReactor | Daemon-internal post-materialization conflict discovery. | Daemon worker. | Candidate/decision/binding/source/advisory events. | Conflict warnings or accepted conflict evidence routing. | Conflict/collision events only when policy accepts. | Expose "detect conflicts now" as a user/MCP authority path or choose winner. |
| Governance policy | Review and authority rules. | Bot runtime. | Current replayed state, actor authority, policy config, substrate capability. | Allowed/rejected review transitions. | Policy version/config as governed state where applicable. | Persist events directly without adapter path. |

## Canonical vs Derived State

| Artifact | Class | Authoritative writer | Allowed readers | Mutation/lifecycle | Prohibited shortcut |
| --- | --- | --- | --- | --- | --- |
| `ExternalIngestEnvelope` | protocol input | External integration | Gateway only | Request-scoped; rejected or mapped. | Carry AuthorityContext or review commands. |
| `ToolRequest` | protocol input | MCP/CLI/dashboard/TUI | Dispatcher | Request-scoped with RuntimeTrace. | External integrations using it for source ingest. |
| `Source` | evidence metadata | ObservationIngestor | Runtime, dashboard, search, history | Accepted observation event. | Direct UI/MCP file write. |
| `SourceSnapshot` | evidence | ObservationIngestor | Runtime, projection, review surfaces | Immutable content-addressed snapshot event. | Live source as replay authority. |
| `SourceEvidence` | evidence | ObservationIngestor | Projection, lookup, review surfaces | Accepted evidence event. | LLM rationale as evidence. |
| `EvidenceReference` | evidence pointer | ObservationIngestor | Projection, review, replay | Accepted evidence event. | Candidate without evidence reference when evidence is available. |
| `ProjectionRequested` | accepted event | ObservationIngestor | ProjectionRetryScheduler, read model | Appended once per captured evidence/projection profile request. | Hidden in operational queue only. |
| `ProjectionDeferred` | accepted event | ReasoningProjectionService | Retry scheduler, dashboard/status | Appended for provider unavailable, timeout, retryable invalid output. | Erase captured evidence on provider failure. |
| `ProjectionCompleted` | accepted event | ReasoningProjectionService | CandidateConvergence, replay, dashboard | Appended for projected, no_candidate, or terminal failed state. | Re-call model during replay to rebuild candidates. |
| Normalized projection output | candidate/proposal | ReasoningProjectionService after validation | CandidateConvergence, review surfaces | Stored in ProjectionCompleted or equivalent event payload. | Full provider transcript required for replay. |
| `ReasoningArtifact` | advisory artifact | Local/hosted provider boundary | Runtime/dashboard by ref/hash | Optional artifact storage/retention. | Required canonical replay dependency. |
| Candidate fingerprint | derived/proposal key | CandidateConvergence | Runtime/conformance tests | Deterministically recomputed from normalized claim, scope, evidence, profile. | Random id as dedupe key. |
| `DecisionCandidate` | candidate/proposal | CandidateConvergence through event store | Review surfaces, search/history | Materialized after projection and convergence. | Direct from candidate_hint or title/description. |
| Conflict/duplicate flags | candidate/review metadata | CandidateConvergence or CollisionReactor | Review surfaces | Appended as part of candidate/provenance/conflict events. | Hide or auto-resolve conflict. |
| `Decision` | canonical replayed state | Event replay after governed review command | All read surfaces | Review command accepted by policy and materialized by event store. | Projection creates Decision. |
| RuntimeTrace/logs | audit/receipt/log | Dispatcher/runtime workers | Operators/debug views | Operational retention. | Governance acceptance proof. |
| Retry queue | derived operational state | ProjectionRetryScheduler from replay | Daemon only | Rebuilt from ProjectionDeferred/Completed state. | Canonical source of projection state. |

## Authority Ownership Matrix

| Actor/component | Observe | Propose | Validate | Project | Converge | Accept | Materialize | Lookup | Enforce | Resolve conflict |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| MCP/caller LLM | yes, session context | hints only | no verified facts | no | no | no | no | displays daemon result | no | no |
| External integration | yes, source content | candidate_hints only | source-local only | no | no | no | no | no | no | no |
| Gateway guard layer | payload shape | no | external guardrails | no | no | no | no | no | no | no |
| ObservationIngestor | yes | no | evidence schema/trust | no | no | no | evidence/projection request events | no | no | no |
| ReasoningProjectionService | captured evidence | normalized projections | provider output schema | yes | no | no | projection outcome events | no | no | no |
| CandidateConvergence | replayed candidates/decisions | candidate materialization proposal | idempotency/duplicate/conflict | no | yes | no | candidate/provenance events | no | no | no winner selection |
| CollisionReactor | replayed events | conflict evidence | known-evidence rules | no | no | no | conflict events when policy accepts | no | no | no winner selection |
| Governance policy | current state | no | actor/state/substrate | no | no | review command eligibility | no direct storage | no | future only | allowed review commands |
| EventStoreAdapter | no | no | append contract | no | no | no | accepted events | no | substrate-specific effect only | no |
| Dashboard/CLI | user review input | review commands | no | no | no | human/governed action request | no direct | yes | future display | through governed commands; dashboard is the alpha conflict-resolution surface |

## Runtime Scenarios

### 1. Startup and Capability Reporting

Trigger: local surface starts.

```text
MCP/CLI/dashboard
  -> daemon capability report
daemon returns:
  ToolRequest protocol version
  supported commands
  deferred commands
  projection provider readiness
  lookup capability
  enforcement capability: not_configured for alpha
```

Expected result: local surfaces can start even when projection provider is down, but they must show capture/projection status honestly. Unsupported stages return typed capability state rather than falling back to legacy authority paths.

Failure behavior: protocol mismatch fails startup for MCP; provider missing marks projection unavailable but does not grant local caller candidate authority.

### 2. MCP Pre-Work Capture

Trigger: automatic skill calls `bicameral.preflight` before work.

Alpha recommendation:

```text
bicameral.preflight
  -> stable daemon pre-work ToolRequest command
  -> ObservationIngestor
  -> ProjectionRequested
  -> ReasoningProjectionService
  -> CandidateConvergence if projected
  -> optional daemon-owned lookup summary
  -> ToolResponse with stage statuses and session directive continue
```

Command naming may use the current compatibility name or a future semantic name. The runtime contract is that MCP owns active-session capture packaging only. It does not own lookup logic, convergence, conflict detection, enforcement, or review authority.

If preflight lookup returns a possible contradiction, alpha surfaces that
contradiction as read-only context or as a proposed item routed for review. It
does not resolve the contradiction. Existing Decision authority can be
superseded, rejected, approved, or marked different-scope only through the
dashboard / governed review surface in alpha.

Expected alpha response shape:

```json
{
  "capture": {"status": "captured", "evidence_refs": []},
  "projection": {"status": "completed|deferred|no_candidate", "candidate_refs": []},
  "lookup": {"status": "completed|not_available", "decision_refs": [], "limitations": []},
  "enforcement": {"status": "not_configured"},
  "session_directive": {"mode": "continue"}
}
```

Failure behavior: invalid or unsafe capture rejects the command. Valid capture succeeds even if projection is deferred.

### 3. Local Ingest

Trigger: MCP/CLI/dashboard submits explicit local source/session evidence.

```text
ToolRequest(ingest.submit_local or future observe.source)
  -> dispatcher validates AuthorityContext
  -> ObservationIngestor persists Source/Snapshot/Evidence
  -> ProjectionRequested
  -> projection lifecycle
  -> convergence
  -> candidate materialization if projected
```

Caller `candidate_drafts`, `candidate_hints`, `binding_hints`, and rationale are advisory projection inputs. They are not materialized candidates without daemon projection.

Failure behavior: if capture validates and event append succeeds, evidence is retained even when projection is unavailable. The response returns projection status separately from capture status.

### 4. External Ingest

Trigger: Slack/Devin/Linear/GitHub/Fathom adapter posts `ExternalIngestEnvelope`.

```text
ExternalIngestEnvelope
  -> gateway raw guard checks
  -> typed envelope validation
  -> ObservationIngestor persists source/evidence
  -> ProjectionRequested
  -> projection lifecycle
  -> convergence
  -> reviewable candidates if projected
```

`candidate_hints[]` are advisory inputs to daemon projection. Evidence-only payloads no longer create generic default candidates.

Failure behavior: guard rejection persists no evidence. Valid evidence plus provider failure persists evidence and projection-deferred state if the route contract permits capture.

### 5. Projection Retry

Trigger: provider recovers or daemon backoff timer fires.

```text
replay events
  -> derive retry queue from ProjectionDeferred without matching terminal ProjectionCompleted
  -> ProjectionRetryScheduler invokes ReasoningProjectionService
  -> ProjectionCompleted(projected|no_candidate|failed_terminal)
  -> CandidateConvergence if projected
```

Retry uses captured immutable snapshot/evidence, not the live source. Retry is daemon-owned automatic recovery only. Review surfaces may show status but must not expose an alpha manual retry command.

Idempotency key:

```text
source_snapshot_id
+ evidence_set_id
+ projection_profile_id
+ original_projection_request_id
```

### 6. Candidate Convergence

Trigger: normalized projection output is available.

```text
ProjectionCompleted(status=projected)
  -> compute canonical_candidate_fingerprint
  -> compare against replayed candidates and active Decisions
  -> materialize one of:
       new DecisionCandidate
       converged provenance on existing candidate
       duplicate_review_needed candidate
       conflict_review_needed candidate
```

Candidate creation idempotency key:

```text
source_snapshot_id
+ projection_profile_id
+ canonical_candidate_fingerprint
```

Same claim merges provenance. Near duplicate remains reviewable. Conflict remains visible. The fastest bot does not win.

Conflict visibility is not conflict resolution. A conflict candidate or
conflict flag can block automatic acceptance or route the item to dashboard
review, but it must not choose a winner, supersede existing Decisions, or
approve signoff.

### 7. Review and Candidate Promotion

Trigger: human or allowed local actor submits governed review command.

```text
review.accept_candidate
  -> dispatcher validates AuthorityContext
  -> governance policy checks candidate state and known evidence
  -> EventStoreAdapter materializes accepted review event
  -> replay builds Decision with signoff proposed
```

Signoff, compliance, supersession, and conflict resolution remain separate governed transitions. Candidate promotion is not signoff approval.

### 8. Post-Materialization Collision Reactor

Trigger: candidate materialized, Decision created/promoted, BindingEvidence accepted, source/evidence refreshed, hosted advisory received, or projection retry completes.

```text
event append / replay update
  -> CollisionReactor evaluates known evidence
  -> no accepted conflict evidence: no known conflict status only
  -> accepted conflict evidence: route to collision_pending review state
```

No external tool triggers "detect conflicts now" in alpha. The reactor is daemon-internal and event-driven.

### 9. Forbidden Flow

These paths must be rejected or impossible:

```text
ExternalIngestEnvelope -> review.accept_candidate
candidate_hint -> DecisionCandidate without daemon projection
evidence-only ingest -> generic default DecisionCandidate
MCP preflight -> enforcement/blocking authority
MCP lookup result -> no-conflict or safe-to-proceed claim
provider output -> Decision/signoff/compliance/BindingEvidence
manual retry tool -> projection retry authority
manual conflict detection tool -> collision_pending authority
dashboard/read model -> canonical event mutation
```

## Event, Command, And Protocol Contracts

### `ToolRequest` Pre-Work Capture

Purpose: capture active local work-session evidence and start projection. This may be named with the current alpha compatibility command or a future semantic command, but MCP's `bicameral.preflight` maps to this capture-first behavior.

Producer: MCP, CLI, dashboard test harness.

Consumer: ToolRequest Dispatcher and ObservationIngestor.

Required payload classes:

- source/session identity;
- task title/description;
- snapshot content or evidence excerpts;
- files/diff/symbol hints;
- optional candidate hints/drafts;
- optional binding hints;
- inert metadata.

Validation:

- requires valid `AuthorityContext`;
- rejects signoff, compliance, policy grants, verified graph claims, canonical mutation instructions, and direct event-store writes;
- rejects unsafe or sensitive payloads before provider calls.

Response:

- capture status;
- projection status;
- candidate ids when materialized;
- optional lookup summary;
- enforcement status, `not_configured` in alpha;
- RuntimeTrace.

### `ExternalIngestEnvelope`

Purpose: source-system evidence submission without local actor authority.

Producer: external connectors.

Consumer: gateway guard layer, then ObservationIngestor.

Validation:

- raw recursive forbidden-field guard;
- empty evidence/hints rejection unless a later source-only capture contract explicitly permits empty snapshots;
- sensitive-data and injection-canary checks;
- source trust and schema checks.

Compatibility:

- Keep wire shape stable for connector setup and live GatewaySink work.
- Change internal mapping so candidate hints and evidence-only payloads enter projection, not direct candidate creation.

### Projection Events

`ProjectionRequested`

- Producer: ObservationIngestor.
- Purpose: records that captured evidence is eligible for daemon candidate projection.
- Required fields: request id, source snapshot id, evidence refs, projection profile id, provider policy, candidate/binding hints hashes, actor/source provenance.
- Idempotency: unique by source snapshot, evidence set, projection profile, original request id.

`ProjectionDeferred`

- Producer: ReasoningProjectionService.
- Purpose: records retryable projection failure without erasing evidence.
- Required fields: projection request id, reason, provider/profile, retry class, observed_at.
- Retry: daemon automatic only.

`ProjectionCompleted`

- Producer: ReasoningProjectionService.
- Purpose: records normalized projection outcome sufficient for replay.
- Required fields: status `projected|no_candidate|failed_terminal`, provider/profile/model or adapter id, input snapshot hash, normalized projections, evidence refs, warnings, optional artifact ref/hash.
- Replay rule: replay must rebuild candidate review state from normalized event output without re-calling the model.

### Candidate Materialization Event

Purpose: materialize projected/converged candidates into the review queue.

Producer: CandidateConvergence.

Payload:

- candidate id;
- canonical candidate fingerprint;
- title/body/level/labels;
- evidence refs;
- source snapshot id;
- projection request/completion ids;
- provenance sources;
- duplicate/conflict flags when applicable.

Validation:

- must reference a valid ProjectionCompleted(projected);
- must not reference provider output that failed validation;
- must not choose conflict winner.

### `preflight.lookup` / Read-Only Lookup

Purpose: read relevant existing Decisions/candidates/readiness. This is the semantic successor to `preflight.run`.

Producer: ToolRequest local surfaces or internal pre-work capture response.

Consumer: daemon lookup engine.

Rules:

- read-only;
- appends no events;
- mutates no candidate, source, signoff, compliance, binding, or conflict state;
- may be used internally by pre-work capture to return context;
- must mark results non-exhaustive when graph/hosted/semantic coverage is unavailable.

### Future `gate.evaluate`

Purpose: future enforcement/gating evaluation.

Rules:

- daemon-owned, not MCP-owned;
- may return allow/warn/pause/block only when policy and substrate capability support it;
- MCP may render/inject the directive but does not compute it;
- not alpha.

## Substrate Capability Matrix

| Substrate/component | Persist canonical state | Persist evidence | Emit accepted events | Host read projections | Enforce policy | Identity/permission | Offline/local-first | Team-wide sync | Recovery |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| MCP | no | no canonical | no | no | no | supplies local session context | yes | no | restart/retry request |
| External connector | no | connector-local only | no | no | no | source-system identity only | partial | source-specific | re-post envelope |
| Gateway | no | through adapter only | through runtime/adapter | no | guardrails only | endpoint/source trust | yes local | no by itself | typed rejection/logs |
| Bot runtime | coordinates only | through adapter | yes via adapter | yes | yes | validates AuthorityContext | yes | depends substrate | replay/retry |
| EventStoreAdapter | yes | yes where supported | yes | replay source | substrate-specific | actor in events where available | yes for local/git | git/drive/etc. | replay/migration |
| Projection read model | no | no | no | yes | no | inherits reads | yes | rebuildable | rebuild projection |
| Local CLI provider | no | no | no | advisory output only | no | local process | yes | no | retry |
| Hosted Bicameral provider | no local ledger authority | hosted artifacts | no local accepted events | hosted advisory | no local bypass | hosted auth | no | yes | artifact refresh |
| Dashboard | no | no canonical | review commands only | renders | no direct | user auth/local context | yes | substrate-dependent | reload/replay |

## Deployment And Runtime Topology

Local alpha:

```text
MCP / CLI / dashboard
  -> ToolRequest dispatcher
  -> ObservationIngestor
  -> EventStoreAdapter appends evidence + ProjectionRequested
  -> ReasoningProjectionService
       -> local_cli provider by default
       -> ProjectionCompleted or ProjectionDeferred
  -> CandidateConvergence
  -> EventStoreAdapter appends candidate materialization
  -> replay/read models/dashboard
```

External ingest:

```text
connector
  -> gateway /api/v1/external-ingest
  -> guardrails
  -> ObservationIngestor
  -> same projection/convergence pipeline
```

Future enforcement:

```text
MCP sends pre-work context
  -> daemon gate.evaluate
  -> policy/substrate capability check
  -> WorkgateResult allow|warn|pause|block
  -> MCP injects returned directive into session
```

## Error, Retry, And Recovery Behavior

| Failure | Runtime behavior |
| --- | --- |
| Invalid ToolRequest authority | Reject; append no evidence/projection/candidate event. |
| External forbidden authority fields | Reject before typed deserialization; append no event. |
| Unsafe/sensitive capture payload | Reject before provider call; append no event unless policy later defines redacted capture. |
| Valid capture, provider unavailable | Append evidence and ProjectionRequested, append ProjectionDeferred, create no candidate. |
| Provider timeout | ProjectionDeferred with retryable reason; daemon automatic retry only. |
| Provider invalid JSON | ProjectionDeferred if retryable; failed_terminal only if policy/profile marks it terminal. |
| Provider valid `no_candidate` | ProjectionCompleted(no_candidate); no retry and no candidate. |
| Event append failure after provider success | Return append failure; no claim that candidate exists unless replay confirms event. |
| Duplicate projection request | Return existing projected/deferred state when idempotency record exists; otherwise converge by fingerprint before candidate materialization. |
| Same candidate from multiple bots | Converge provenance into existing candidate when fingerprint matches. |
| Near duplicate candidate | Materialize or flag as `duplicate_review_needed`; do not silently merge if meaning differs. |
| Conflict with existing candidate/Decision | Preserve both and route review context; do not choose winner. |
| CollisionReactor unavailable | Candidate/review state remains valid; dashboard shows conflict analysis stale/unavailable, not no-conflict proof. |

## Observability, Audit, And Receipts

Required operational receipts:

- ToolRequest request id and command;
- external ingest source id/request id;
- actor/session/source provenance;
- ProjectionRequested/Deferred/Completed ids;
- provider mode/profile/model or adapter id;
- token/latency/error metadata when available;
- candidate fingerprint and convergence result;
- event ids appended;
- collision reactor status and stale/unavailable indicators.

Receipts and traces explain runtime behavior. They are not governance acceptance, signoff, compliance resolution, or proof of global no-conflict.

## Security, Privacy, And Authority Boundaries

- Separate contracts by authority posture, not transport:
  - `ToolRequest`: local actor command API.
  - `ExternalIngestEnvelope`: external evidence submission API.
  - `Egress`: future outbound effect API.
- MCP is semantically different from integrations because it sits inside an active work session, but alpha uses that only for richer capture context, not review/enforcement authority.
- External integrations submit evidence/hints only.
- Full reasoning transcripts are non-canonical for alpha; normalized projection outcomes plus provenance hashes are enough for replay.
- Provider secrets stay user-local or environment-referenced, not in shared workspace config.
- Hosted provider use must be explicit and policy-controlled.
- Lookup results are hints/context, not enforcement authority.
- "No known conflict" is scoped to available evidence and enabled checks.

## Implementation Sequencing

1. Runtime spec/ADR reconciliation
   - Target: `docs/specs`, ADR-0014/0015/0018/0019 follow-up notes.
   - Acceptance: this pipeline is referenced as the authority for ingest/projection issue slicing.
   - Validation: docs review.
   - Task class: `AFK_REVIEW_ONLY`.

2. Event/protocol contract slice
   - Target: `crates/bicameral-api`.
   - Add projection event payloads, projection statuses, candidate fingerprint/provenance fields or fixtures.
   - Acceptance: schema fixtures for requested/deferred/completed/no_candidate.
   - Validation: `cargo test -p bicameral-api --locked`.
   - Task class: `AFK_REVIEW_ONLY`.

3. ObservationIngestor extraction slice
   - Target: `crates/bicameral-runtime`, `crates/bicameral-gateway`.
   - Extract shared source/snapshot/evidence creation used by local and external ingest.
   - Acceptance: local and external ingest both persist evidence through shared helper without candidate creation.
   - Validation: focused runtime/gateway tests.
   - Task class: `AFK_REVIEW_ONLY`.

4. ReasoningProjectionService slice
   - Target: `crates/bicameral-runtime`.
   - Implement fake provider and `local_cli` contract behind feature/config.
   - Acceptance: candidate materialization impossible without validated ProjectionCompleted(projected).
   - Validation: provider success, no_candidate, invalid JSON, timeout tests.
   - Task class: `AFK_REVIEW_ONLY`.

5. Projection retry scheduler slice
   - Target: daemon/runtime worker.
   - Build derived retry queue from replay; automatic retry only.
   - Acceptance: deferred projection retries after fake provider recovery; no user command exposed.
   - Validation: lifecycle simulation.
   - Task class: `AFK_REVIEW_ONLY`.

6. Candidate convergence slice
   - Target: runtime candidate materialization.
   - Implement deterministic fingerprint and duplicate/conflict routing.
   - Acceptance: same claim merges provenance; near duplicate and conflict remain reviewable.
   - Validation: multi-bot fixture tests.
   - Task class: `AFK_REVIEW_ONLY`.

7. External ingest behavior change
   - Target: `crates/bicameral-gateway`.
   - Remove evidence-only generic default candidate creation; treat hints as projection inputs.
   - Acceptance: external evidence-only payload creates evidence/projection state but no generic candidate.
   - Validation: update `external_ingest_gateway` tests.
   - Task class: `AFK_REVIEW_ONLY`.

8. MCP alpha bridge update
   - Target: `bicameral-mcp` and bot conformance.
   - Keep MCP thin; make `bicameral.preflight` call the capture-first daemon command and render stage statuses.
   - Acceptance: MCP does not own lookup/convergence/conflict/enforcement logic.
   - Validation: thin-client conformance.
   - Task class: `AFK_REVIEW_ONLY`.

9. Read-model/dashboard status slice
   - Target: projection/read model and dashboard.
   - Render projection status, deferred reasons, candidate provenance, duplicate/conflict flags.
   - Acceptance: users can distinguish captured evidence, deferred projection, candidates, and Decisions.
   - Validation: dashboard contract/UI tests.
   - Task class: `AFK_REVIEW_ONLY`.

## Issue Generation Notes

Hard dependencies:

- Event/protocol statuses before runtime implementation.
- Shared ObservationIngestor before local/external ingest behavior changes.
- ReasoningProjectionService before mandatory candidate projection is enforced.
- Candidate convergence before broad multi-bot dogfood.

Soft dependencies:

- `preflight.lookup` rename can follow after alpha if current `preflight.run` is already merged.
- Full ToolRequest semantic namespace refactor should be post-alpha and separate.
- Hosted provider work can run after local fake/CLI provider contract.

Fixture opportunities:

- fake provider returning projected, no_candidate, invalid JSON, timeout;
- two MCP sessions proposing same decision;
- Slack evidence-only envelope;
- Slack/Devin candidate_hint envelope;
- projection deferred then provider recovery;
- conflict with existing approved Decision.

Human-input blockers:

- Whether the public pre-work daemon command should be named `preflight.evaluate`, `preflight.observe`, or kept as current compatibility name for alpha.
- Whether to introduce separate `EventType`s immediately or encode projection payloads under existing ingest event shape for first slice.

## ADR Implications

- ADR-0002: reaffirmed for ToolRequest versus ExternalIngestEnvelope separation; likely amended later for pre-work command naming.
- ADR-0004: reaffirmed. HITL remains required for authority-changing transitions.
- ADR-0007: amended. Candidate Projection is now a daemon runtime stage with mandatory reasoning and convergence before materialization.
- ADR-0011: reaffirmed and extended. Later collision release valve remains; CollisionReactor is daemon-internal and event-triggered.
- ADR-0014: amended. Reasoning remains non-authoritative, but daemon-managed projection is mandatory for candidate materialization.
- ADR-0015: amended. Provider configuration becomes alpha setup/readiness for candidate projection, while full artifacts remain non-canonical.
- ADR-0016: reaffirmed. Gateway guardrails still precede external ingest mapping.
- ADR-0018: amended. Clean projection remains, but direct candidate materialization from hints/defaults is superseded by daemon projection.
- ADR-0019: amended or renamed. Current `preflight.run` behavior is read-only lookup; semantic successor should be `preflight.lookup`.
- ADR-0020: reaffirmed. Gateway external ingest must not carry local actor authority.

Recommended ADR action: amend ADR-0014, ADR-0015, and ADR-0018 together, then separately decide ToolRequest naming/migration in ADR-0002/ADR-0019.

## Acceptance Criteria

- Valid source/evidence capture can persist without projection success.
- Provider unavailable creates projection-deferred state and no candidate.
- Projection retry is automatic daemon recovery only.
- `no_candidate` is terminal for that projection request and does not retry forever.
- Replay can rebuild projection/candidate state without re-calling the model.
- No path creates a `DecisionCandidate` directly from `candidate_hint`, `candidate_draft`, title/description, or generic evidence-only text.
- External evidence-only ingest no longer creates a generic default candidate.
- Candidate convergence dedupes same-claim multi-bot projections by deterministic fingerprint.
- Near duplicates and conflicts remain visible for review.
- No exposed alpha tool triggers conflict detection manually.
- CollisionReactor can mark `collision_pending` only from accepted conflict evidence and cannot choose a winner.
- MCP `bicameral.preflight` remains a thin bridge and does not own lookup/convergence/conflict/enforcement logic.
- ToolRequest naming changes are not required to land the shared runtime pipeline.

## Validation Plan

- `cargo fmt --all -- --check`
- `cargo test -p bicameral-api --locked`
- `cargo test -p bicameral-runtime --locked`
- `cargo test -p bicameral-gateway --locked`
- schema fixture validation for projection lifecycle payloads
- gateway external ingest regression tests for no default candidate
- runtime local ingest regression tests for no direct primary candidate without projection
- fake-provider lifecycle tests for projected, deferred, retry, no_candidate, failed_terminal
- candidate convergence tests for same claim, near duplicate, and conflict
- MCP thin-client mapping test proving no legacy authority fallback
- dashboard/read-model test for projection status visibility

Validation gaps:

- Hosted provider artifact retention and redaction policy require later provider-specific tests.
- Substrate-specific event migration from existing `EventType::Ingest` may need adapter migration tests once event shape is chosen.

## Open Questions

- Public pre-work command name: `preflight.evaluate`, `preflight.observe`, or current compatibility name for alpha?
- Projection event shape: new event types immediately or transitional projection payloads under existing ingest event?
- Candidate fingerprint canonicalization details: how much normalization is deterministic versus provider/profile-specific?
- Which provider secret store is canonical on each supported platform?

## Request Coverage

- "Under-specified bot runtime architecture": Runtime Problem, Runtime Components, Runtime Scenarios.
- "Ingestor side prior work did not cover this": Runtime Problem and Implementation Sequencing.
- "MCP ingest-only alpha": Runtime Scenarios, Security/Authority Boundaries, MCP alpha bridge slice.
- "ToolRequest is only a subset of bot API": Security/Authority Boundaries.
- "Projection retry/lazy load": Projection Retry scenario and Error/Retry table.
- "Race condition for decisions from different bots": Candidate Convergence scenario.
- "Conflict detection exposed tool or daemon internal": CollisionReactor scenario and Acceptance Criteria.
- "`bic:to-runtime-spec`": this document follows the runtime architecture blueprint and records the advisory governance gate.
