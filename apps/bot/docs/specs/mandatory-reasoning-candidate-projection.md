# Mandatory Reasoning-Assisted Candidate Projection

Date: 2026-06-12
Status: draft
Level: L2 cross-repo contract
Owns: The feature contract for requiring a reasoning pass before source, integration, or preflight observations become `DecisionCandidate`s.
Related context: ADR-0002, ADR-0004, ADR-0007, ADR-0014, ADR-0015, ADR-0020, `docs/specs/bot-mcp-data-flow-runtime-architecture.md`, `docs/specs/external-adapter-ingest-gateway.md`, `docs/specs/mandatory-reasoning-candidate-projection-runtime-architecture.md`, bicameral-bot#261.

Governance gate: `bic:spec-governance-gate` advisory/no-op; Qor integration disabled pending factory policy revision.

## Reading Guide

Read ADR-0014 and ADR-0015 first. This spec intentionally changes their posture: reasoning remains non-authoritative, but candidate projection is no longer optional for flows that create `DecisionCandidate`s.

Main invariant: **a `DecisionCandidate` is a reasoning-projected claim with cited evidence, never a raw default wrapper around observed evidence.**

## Problem

The current code and specs allow evidence-only ingest to auto-create generic candidates such as "External evidence from slack" or local ingest candidates from caller-provided title/description without a mandatory daemon reasoning step. That made the alpha MCP story weak: the automatic skill-trigger path was `preflight`, but useful `DecisionCandidate`s depended on manual `ingest.submit_local` calls or caller-prepared drafts that could bypass bot-owned projection.

This creates two product problems:

- Automatic pre-work capture can record evidence without producing reviewable decision memory.
- Evidence-only integrations can pollute the review queue with generic candidates that are not actual extracted decisions.

## Goals

- Make reasoning-assisted candidate projection mandatory before any new `DecisionCandidate` is created.
- Add `preflight.observe` as the alpha command behind `bicameral.preflight`.
- Keep all reasoning output advisory until governance policy and review accept it.
- Preserve caller-LLM as an advisory hint source, and require a daemon-managed local or hosted provider mode for candidate projection.
- Use `local_cli` as the alpha default daemon reasoning mode.
- Stop creating generic default candidates from evidence-only ingest.
- Keep source/evidence capture separate from candidate projection.

## Non-Goals

- Do not make LLM output a `Decision`, signoff, compliance result, binding authority, or enforcement result.
- Do not require hosted Bicameral for local alpha; a daemon-managed local CLI or local OpenAI-compatible provider can satisfy the reasoning requirement.
- Do not implement `preflight.run` Constraint Lookup in this feature.
- Do not implement `workgate.evaluate` or blocking enforcement.
- Do not allow MCP, dashboard, integrations, or providers to write canonical ledger state directly.

## Decision Or Proposed Direction

Adopt a mandatory **Reasoning Projection** stage for candidate creation.

`SourceSnapshot`, `SourceEvidence`, and `EvidenceReference` may be created from validated source payloads without a model. `DecisionCandidate` creation requires daemon-side `ReasoningProjectionService` processing over one or more of:

- caller-provided `candidate_drafts` or integration `candidate_hints`, treated only as advisory inputs;
- source evidence using a configured local daemon provider;
- hosted Bicameral reasoning that returns advisory `ReasoningArtifact` output.

Caller or integration LLM output can seed the projection request, but it cannot substitute for daemon projection. If no daemon-managed reasoning mode is available, the system may still record evidence when the command contract permits evidence-only capture, but it must not create a `DecisionCandidate`. If the command promises candidate projection, it returns a typed provider-unavailable or projection-deferred result.

The alpha default daemon reasoning mode is `local_cli`. `local_openai_compatible` and `hosted_bicameral` remain supported design targets, but they should not block the first alpha projection implementation.

For alpha, `bicameral.preflight` maps to `preflight.observe`, not `preflight.run`.

## Owner Decision: Alpha Projection Cut Line

Related: #424

The alpha Reasoning Projection lifecycle is intentionally narrow:

- `candidate_projected`: daemon projection produced one or more validated
  DecisionCandidate proposals.
- `no_candidate`: daemon projection completed successfully and found no
  decision-like claim. This is not user-facing candidate state.
- `projection_deferred`: evidence capture may be accepted, but candidate
  creation is deferred because the daemon projection provider is missing,
  unavailable, timed out, or returned no validated projection.
- `hard_rejected`: the request is malformed, forbidden, sensitive, or otherwise
  invalid before projection can safely proceed.

Evidence may persist when projection defers for explicit capture/ingest
commands, because source/evidence capture and candidate projection are separate
steps. Commands that promise candidate projection should return a typed
projection-deferred/provider-unavailable outcome rather than manufacturing a
fallback candidate.

`candidate_hints` and local `candidate_drafts` are strictly advisory inputs.
They must pass through daemon-owned projection before any DecisionCandidate
materializes.

Same-claim convergence, automatic retry scheduling, near-duplicate conflict
handling, manual retry UI, and richer no-candidate archives are deferred until
the alpha tracer bullet proves the basic projection path.

State minimization: no additional alpha states are accepted. Provider-specific
failures collapse into `projection_deferred` with detail metadata unless they
are hard validation failures. `no_candidate` remains a successful handled
projection outcome, not a review queue state.

## Existing Infrastructure Inventory

| Item | Current behavior | Use in this proposal |
| --- | --- | --- |
| `ToolRequest` / `ToolResponse` | Canonical local tool contract. | Add `preflight.observe`; keep `preflight.run` future lookup. |
| `ingest.submit_local` | Creates source/evidence and a primary candidate from caller title/description. | Split evidence capture from reasoning-projected candidates; allow shared projection behavior. |
| `ExternalIngestEnvelope` | External evidence plus `candidate_hints`; evidence-only currently creates default candidate. | Treat hints as projection inputs; require daemon projection for candidates; remove default candidate creation. |
| `DecisionCandidate` | Proposed extracted claim awaiting review. | Tighten meaning: must be produced by daemon reasoning projection after bot validation. |
| `ReasoningArtifact` | Advisory output from reasoning provider. | Use as provenance for daemon/hosted projections where available. |
| `RuntimeTrace` | Operational receipt fields for provider/model/task/token usage. | Required on reasoning projection responses. |
| ADR-0014 | Reasoning optional, deterministic fallbacks required. | Amend: deterministic fallback remains for validation/evidence, not semantic candidate projection. |
| ADR-0015 | Provider modes optional and advisory; dashboard should expose hosted sign-in, local OpenAI-compatible endpoint configuration, local CLI adapter configuration, and task toggles. | Amend: candidate projection requires an available reasoning mode, so provider configuration becomes part of the alpha setup path. |

## Contract Shape

### Command Registry

Add:

```text
preflight.observe
```

Keep:

```text
preflight.run
```

`preflight.observe` is alpha pre-work observation and reasoning projection. `preflight.run` is future read-only Constraint Lookup.

### `preflight.observe` Request

Fields:

- `source_uri`: stable local session/task URI.
- `source_type`: usually `mcp_session`, `agent_session`, `command_run`, or `manual`.
- `title`: short task title.
- `description`: task/request context.
- `snapshot_content`: captured prompt, task, command output, diff summary, or selected context.
- `evidence[]`: source excerpts.
- `candidate_hints[]`: optional caller/integration hints for daemon projection.
- `binding_hints[]`: optional advisory code hints.
- `rationale`: optional caller/provider reasoning.
- `reasoning_mode`: `local_openai_compatible`, `local_cli`, `hosted_bicameral`.
- `metadata`: inert operational metadata.

Excluded fields:

- canonical decision ids except as explicit lookup targets;
- signoff state;
- compliance verdicts;
- policy grants;
- verified graph evidence assertions;
- direct event-store write instructions.

### Projection Result

Successful projection returns:

- source id;
- snapshot id;
- evidence ids;
- evidence reference ids;
- candidate ids, when projection created candidates;
- reasoning provenance: mode, provider, model or profile, task kind, artifact id when available;
- trace correlation.

Provider unavailable returns a typed non-success result and must not create candidates.

Alpha status mapping:

| Runtime result | Public projection state | Candidate materialized |
| --- | --- | --- |
| Valid provider output with projections | `candidate_projected` | yes |
| Valid provider output with no projections | `no_candidate` | no |
| Missing/unavailable provider or timeout | `projection_deferred` | no |
| Malformed request, forbidden authority field, sensitive data, invalid provider output | `hard_rejected` | no |

### Alpha `local_cli` Provider Contract

The alpha default provider mode is `local_cli`. The daemon invokes the configured command once per projection request, writes one JSON object to stdin, and reads one JSON object from stdout.

Input:

```json
{
  "schema_version": "reasoning_projection.v1",
  "task_kind": "candidate_projection",
  "request_id": "uuid",
  "source": {
    "source_uri": "mcp-session://...",
    "source_type": "mcp_session",
    "title": "Implement checkout retry behavior",
    "description": "User asked to add retry behavior around checkout payment calls."
  },
  "snapshot_content": "...",
  "evidence": [
    {
      "excerpt": "Retry failed checkout payment calls up to 3 times",
      "span_start": 120,
      "span_end": 168,
      "confidence": 0.9
    }
  ],
  "candidate_hints": [
    {
      "title": "Checkout payments retry failed calls",
      "body": "Payment calls should retry transient failures up to 3 times.",
      "level": "implementation",
      "labels": ["checkout"]
    }
  ],
  "binding_hints": [],
  "constraints": {
    "max_candidates": 5,
    "allowed_levels": ["product", "architecture", "implementation"]
  }
}
```

Success output:

```json
{
  "schema_version": "reasoning_projection.v1",
  "status": "ok",
  "projections": [
    {
      "title": "Checkout payments retry transient failures",
      "body": "Payment calls in checkout should retry transient failures up to 3 times before surfacing an error.",
      "suggested_level": "implementation",
      "labels": ["checkout"],
      "evidence_indexes": [0],
      "confidence": 0.86,
      "rationale": "The evidence states a concrete implementation constraint."
    }
  ],
  "warnings": []
}
```

No-candidate output:

```json
{
  "schema_version": "reasoning_projection.v1",
  "status": "no_candidate",
  "projections": [],
  "warnings": ["No decision-like claim found in the supplied evidence."]
}
```

Rules:

- Exit code `0` with valid JSON means the provider handled the request, even when `status` is `no_candidate`.
- Non-zero exit maps to `provider_unavailable`.
- Invalid JSON maps to `provider_invalid_output`.
- Provider output must not include signoff, compliance, policy grants, verified graph state, canonical decision ids, or direct event-store instructions.
- The daemon validates output before materializing any `DecisionCandidate`.

### External Ingest Change

Evidence-only external ingest no longer auto-creates a generic default candidate. It either:

- stores evidence only and returns `projection_deferred`, or
- calls configured daemon/hosted reasoning and returns projected candidate ids.

`candidate_hints[]` remain advisory and must pass through daemon `ReasoningProjectionService` processing before materialization as `DecisionCandidate`s.

## Workflow Mapping

| Flow | Input | Reasoning projection | Output |
| --- | --- | --- | --- |
| MCP automatic preflight | `bicameral.preflight` | `preflight.observe` sends caller hints/evidence to daemon projection using alpha-default `local_cli` | Source/evidence plus candidates |
| Manual local ingest | `ingest.submit_local` | caller drafts/evidence pass through daemon projection | Source/evidence plus candidates |
| External integration | `ExternalIngestEnvelope` | integration hints/evidence pass through daemon projection | Source/evidence plus candidates |
| Evidence-only capture | source/evidence only | none available | Evidence accepted, candidate projection deferred |

## Trust, Privacy, Security, And Authority Boundaries

- Reasoning providers may observe only the scoped source content selected for projection.
- Provider output may propose candidates, evidence excerpts, levels, labels, rationale, and binding hints.
- Provider output cannot accept candidates, approve signoff, resolve compliance, create `BindingEvidence`, or mutate canonical state.
- Hosted providers may receive only workspace-policy-allowed content.
- Secrets, credentials, and sensitive data must be rejected or redacted before provider calls.
- `ReasoningArtifact` and runtime traces are advisory/provenance, not ledger authority.

## Per-Surface Behavior

| Surface | Behavior |
| --- | --- |
| MCP | `bicameral.preflight` calls `preflight.observe`; prompt says observe/project, not enforce. |
| Bot runtime | Validates authority, evidence, provider availability, and forbidden fields; invokes projection; appends accepted ingest/projection events. |
| Gateway | Exposes a ToolRequest route or daemon client path for local surfaces; external ingest may invoke projection. |
| Dashboard | Adds reasoning/model configuration for provider mode, hosted sign-in, local OpenAI-compatible endpoint, local CLI adapter, and task toggles; shows projected candidates and provider provenance; groups pre-work observations separately if needed. |
| Integrations | Submit evidence and hints; do not own authority or signoff. |
| CLI | Can smoke-test provider config and projection using fixture payloads. |

## Implementation Plan

1. Protocol and ADR amendment slice
   - Target: `protocol/`, `docs/adr/`, `CONTEXT.md` if terms are accepted.
   - Add `preflight.observe`, projection result states, and provider-unavailable errors.
   - Acceptance: schemas and fixtures cover accepted and provider-unavailable paths.
   - Task class: `AFK_REVIEW_ONLY`.

2. Runtime projection service slice
   - Target: `crates/bicameral-runtime`, `crates/bicameral-api`.
   - Add reasoning projection interface, alpha-default `local_cli` provider adapter, and deterministic validation around provider output.
   - Acceptance: no candidate is created without daemon projection output.
   - Task class: `AFK_REVIEW_ONLY`.

3. MCP alpha preflight slice
   - Target: `bicameral-mcp` plus bot conformance fixtures.
   - Map `bicameral.preflight` to `preflight.observe`.
   - Acceptance: automatic skill trigger produces source/evidence/candidate ids through ToolRequest.
   - Task class: `AFK_REVIEW_ONLY`.

4. External ingest symmetry slice
   - Target: `crates/bicameral-gateway`.
   - Remove evidence-only default candidate creation.
   - Acceptance: evidence-only returns evidence ids plus projection-deferred unless provider configured.
   - Task class: `AFK_REVIEW_ONLY`.

5. UI/read model slice
   - Target: dashboard and projection code.
   - Add reasoning/model configuration UI for hosted Bicameral sign-in, local OpenAI-compatible endpoint, local CLI adapter, provider mode selection, and projection task toggles.
   - Show reasoning provenance and pre-work observation source.
   - Acceptance: user can configure or see why no projection-capable provider is available, and can distinguish projected candidates from accepted Decisions.
   - Task class: `AFK_REVIEW_ONLY`.

## ADR Implications

- ADR-0002: amended. Add `preflight.observe` to MCP command mapping and clarify `preflight.run` remains Constraint Lookup.
- ADR-0004: reaffirmed. Reasoning output remains candidate/proposal state and HITL review remains required.
- ADR-0007: amended. Candidate Projection now requires daemon reasoning projection output; evidence capture and caller/integration hints alone are not candidate projection.
- ADR-0014: amended. Caller/integration/cloud reasoning remains non-authoritative input, but candidate materialization requires daemon-side projection processing.
- ADR-0015: amended. Provider modes remain advisory, but at least one mode must be configured for flows that promise candidate projection.
- ADR-0020: reaffirmed. Gateway mutation authority still needs actor context; reasoning does not bypass command gating.

Recommended ADR action: amend ADR-0014 and ADR-0015 together, because the optional-to-mandatory change is durable, surprising without context, and authority-sensitive.

## Issue Generation Notes

Slice issues in this order:

1. Protocol/ADR update for `preflight.observe` and mandatory projection.
2. Runtime projection service and provider interface.
3. ToolRequest route/daemon client path if missing.
4. MCP `bicameral.preflight` remap and prompt update.
5. External ingest default-candidate removal.
6. Dashboard reasoning/model configuration and read-model provenance.

Hard dependency: protocol/ADR update before cross-repo MCP implementation.

Human-input blocker: choose whether evidence-only capture without provider should still append source/evidence events or reject the whole command. Recommendation: append evidence for explicit capture commands; reject candidate-projection commands with typed `projection_deferred`.

## Acceptance Criteria

- No code path creates a new `DecisionCandidate` from evidence-only default text.
- `preflight.observe` exists in schemas and conformance fixtures.
- `bicameral.preflight` maps to `preflight.observe` for alpha.
- A reasoning mode is required before candidate projection succeeds.
- Dashboard exposes provider configuration or a clear projection-unavailable state before alpha candidate projection is required.
- Provider output is stored as advisory provenance, not authority.
- Missing provider returns a typed failure/deferred result without creating candidates.
- `preflight.run` remains unavailable or separate until Constraint Lookup is implemented.
- Negative tests prove reasoning output cannot create Decisions, signoff, compliance, or BindingEvidence.

## Validation Plan

- `cargo test -p bicameral-api --locked`
- `cargo test -p bicameral-runtime --locked`
- `cargo test -p bicameral-gateway --locked`
- schema fixture validation for `preflight.observe`
- MCP mapping test for `bicameral.preflight -> preflight.observe`
- external ingest regression test: evidence-only no longer creates default candidate
- dashboard/provider configuration smoke test or UI contract test
- trace inspection for provider/model/task/token fields when a provider is used

## Open Questions

- Should evidence-only `preflight.observe` append source/evidence without candidates, or reject entirely when projection is unavailable?
- Which local CLI command should the dashboard prefill or document as the default user-facing adapter?
- Which provider settings live in workspace config versus user-local secret storage?
- Should `ReasoningArtifact` be required for caller-LLM drafts, or is trace metadata sufficient?

## Request Coverage

- "Change optional requirement into mandatory": covered by Decision, Contract Shape, ADR Implications.
- "There should always be LLM processing candidate hints and turning them into candidates": covered by mandatory daemon Reasoning Projection.
- "local CLI provider is alpha default": covered by Decision, Workflow Mapping, and Implementation Plan.
- "`local_cli` command contract": covered by Contract Shape.
- "Run bic:to-spec": this document follows `bic:to-spec` and the feature proposal blueprint.
- "Run bic:to-runtime-spec": see the paired runtime spec.
