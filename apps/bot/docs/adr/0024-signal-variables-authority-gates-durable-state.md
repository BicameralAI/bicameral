# ADR-0024: Signal Variables, Authority Gates, and Durable State

## Status

Accepted for RFQ closure.

## Context

Several Bicameral RFQs define terms such as freshness, graph readiness, code evidence state, source authority, binding currentness, contradiction severity, correction eligibility, and egress readiness.

Those terms are not just labels. They are surfaced expressions of internal evaluation logic. If their mathematical role is not explicit, implementation can accidentally turn advisory signals into durable authority.

This ADR defines the shared field-role model that applies across BindingEvidence, preflight, search, CodeGenome-derived graph signals, Ledger View, contradiction candidates, correction candidates, and egress readiness.

## Decision

Every surfaced variable that influences product behavior must declare its role in the authority equation.

Supported roles:

```text
authority_gate
identity_anchor
signal_input
signal_output
provenance
handoff_artifact
read_model_state
```

Core rule:

```text
Signals guide attention.
Gates create durable authority.
Identity anchors define what durable authority attaches to.
Provenance explains origin.
Handoff artifacts assist transition.
Read-model states explain the current view.
```

Do not collapse these roles into one confidence score.

## Role definitions

### Authority gate

A required condition that must pass before durable state may be created, accepted, or changed.

Authority gates are pass/fail checks. A high signal score must not compensate for a failed authority gate.

Examples:

```text
accepted Decision eligibility
approved CodeResource
known graph_snapshot_id
known validated_sha
verified binding target
policy-approved materialization path
approved egress execution
```

### Identity anchor

A field that defines what durable state attaches to.

Identity anchors are structural, not advisory.

Examples:

```text
code_resource_id
graph_snapshot_id
validated_sha
source_snapshot_id
binding_id
decision_id
target_identifier
```

### Signal input

A measured or inferred value used to rank, warn, prioritize, recommend, or explain.

Signal inputs may influence attention and action eligibility. They must not directly create durable authority.

Examples:

```text
freshness
source authority
symbol resolution strength
graph readiness
overlay confidence
scope completeness
code overlap
unknown scope
local workspace cleanliness
```

### Signal output

A surfaced result derived from signal inputs.

Signal outputs explain why the system is showing a warning, ranking, recommendation, or allowed next action.

Examples:

```text
readiness_warnings
action_eligibility_reasons
freshness warnings
graph readiness state
binding currentness
contradiction priority
correction recommendation
```

### Provenance

A field that explains origin, route, actor, time, source, or context.

Provenance helps audit and replay. Provenance alone is not authority.

Examples:

```text
ref_name
created_from_request_id
created_by
accepted_by
created_at
provider id
source refs
locator refs
```

Important rule:

```text
ref_name is provenance, not durable code identity.
```

### Handoff artifact

A temporary or bounded object used to carry context from one step to another.

Handoff artifacts are not durable authority.

Examples:

```text
validation_token
candidate_id
selected_candidate_ref
RecallPacket action reference
EgressProposal id before approval
```

### Read-model state

A projected view of canonical state plus signals. It is useful for users and agents, but it is not itself canonical authority.

Examples:

```text
Ledger View state
preflight result
search result
CodeEvidence surface
CapabilityReport
DiscoveryOutcome
```

## BindingEvidence as the first concrete application

BindingEvidence is the first implementation case for this model.

BindingEvidence authority is gate-based. Binding readiness is signal-based.

Accepted BindingEvidence may only be materialized when the required authority gates pass:

```text
Decision is eligible
AND CodeResource is approved
AND graph_snapshot_id is known
AND validated_sha is known
AND target is verified
AND selected target matches the validated snapshot
AND binding.create revalidation passes
AND governed materialization path accepts creation
```

Binding readiness may use signal inputs such as graph readiness, source authority, freshness, symbol resolution strength, overlay confidence, scope completeness, code overlap, unknown scope, and workspace cleanliness.

Those signals may rank candidates, create warnings, recommend revalidation, or explain next actions. They must not create BindingEvidence.

## Canonical BindingEvidence state vocabulary

Use one evidence-state vocabulary for binding candidates and binding readiness:

```text
verified
approximate
ambiguous
not_found
unsupported
not_indexed
stale
graph_unavailable
snapshot_mismatch
workspace_mismatch
```

Only `verified` may proceed to accepted BindingEvidence materialization, and only after `binding.create` revalidation.

## CodeGenome application

CodeGenome-derived values such as overlay confidence, graph identity, source/code currentness, signal propagation, affected-surface discovery, and advisory drift detection are signal inputs or identity substrate inputs.

They are not Bicameral governance authority by themselves.

CodeGenome may provide code-memory and signal inputs. Bot decides how those inputs are interpreted, cited, corrected, bound, and governed.

## Preflight and search application

Preflight and search are read-model/advisory surfaces.

They may surface relevance, freshness, unknown scope, graph readiness, binding currentness, and allowed next actions.

They must not directly create accepted Decisions, accepted BindingEvidence, signoff, compliance resolution, conflict resolution, correction acceptance, or egress execution.

For SMR alpha, the identity anchors are the input artifact/context digest,
trusted-spec-corpus revision, applicable specification identities, missing or
unknown coverage findings, and (when present) proposed-claim digest. CodeGraph
readiness, graph snapshot identity, scope sufficiency, and binding currentness
remain Code Evidence/Compliance signals or identity anchors only. They may
enrich recall, but they cannot gate SMR core recall or become SMR closure,
absence, contradiction, all-clear, compliance, or canonical-claim authority
(ADR-0028).

## Ledger View and dashboard application

Ledger View may display tracked context, freshness, source readiness, graph readiness, binding currentness, advisory preflight results, and allowed next actions.

Ledger View state is a read model. It is not canonical authority.

Every dashboard field that influences user or agent behavior should declare whether it is canonical state, signal output, provenance, candidate state, or allowed next action.

## Retrospective RFQ consistency matrix

| RFQ / Decision | Role affected | Status | Clarification |
|---|---|---|---|
| #116 BindingEvidence | gates, identity anchors, provenance, handoff artifacts, signal outputs | Consistent | Final payload must classify fields by role. |
| #134 search.query | read-model state | Consistent | Binding search remains hidden or blocked until real BindingEvidence exists. |
| #221 CodeGenome cleanup | signal inputs, identity substrate, MCP authority removal | Consistent | Retire MCP-local authority, not CodeGenome as substrate. |
| #229 preflight.run | signal output, read-model state | Consistent | Preflight may surface gates but cannot satisfy them by itself. |
| #231 graph query replacement | signal input/output, bot-owned graph interpretation | Consistent | MCP renders bot-returned graph states only. |
| #240 signal entities | signal vocabulary | Consistent | Domain terms represent surfaced math, not authority. |
| #442 decision.find_code_impact | signal output, candidate state | Consistent | ImpactCandidate is not BindingEvidence. |
| #498 CodeGenome | signal input, code-memory substrate | Consistent | Overlay confidence and signal propagation do not create governance authority. |
| #500 contradiction candidates | signal output, candidate state | Consistent | Candidate priority is not accepted contradiction resolution. |
| #501 correction capture | signal output, governed action | Consistent | Correction signals may recommend action; only governed acceptance mutates corpus state. |
| #255 V1 runtime / Ledger View | consolidation surface | Consistent | Spec must include this role model. |

## Consequences

This ADR adds a requirement to future domain model and API work:

```text
Do not introduce a surfaced field without declaring its role.
```

Reviewers should reject implementations where:

```text
signal confidence overrides a failed authority gate
read-model state becomes canonical state
handoff artifacts create durable authority
provenance is treated as identity
advisory output creates acceptance, signoff, compliance, or binding
```

## Non-goals

This ADR does not define exact scoring formulas.

This ADR does not require a shared runtime scoring engine in V1.

This ADR does not implement BindingEvidence, preflight, search, Ledger View, or egress behavior.

This ADR defines the role model those implementations must respect.
