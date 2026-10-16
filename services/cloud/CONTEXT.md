# Bicameral

Bicameral captures implementation-constraining decisions from product, code, and collaboration evidence, then routes them through review into a durable event authority.

## Language

**Decision**:
A binding constraint on implementation. Not a suggestion, opinion, note, or general product knowledge.
_Avoid_: note, feedback, request, product knowledge

**DecisionCandidate**:
An extracted claim that has not yet been accepted into the Decision Ledger. It is non-canonical until governance policy accepts a review command and the selected event store substrate materializes the event.
_Avoid_: approved decision, canonical record, source note

**SourceEvidence**:
The excerpt, pointer, payload, or provenance record that supports a candidate, binding, dependency signal, or governance result.
_Avoid_: vague context, model memory

**BindingEvidence**:
Reviewable evidence that a decision relates to a code path, symbol, diff, dependency, workflow, or deploy surface.
_Avoid_: compliance verdict, signoff, status

**Decision Ledger**:
The canonical materialized decision record derived by replaying the selected event store substrate. Durable write authority remains the event store substrate.
_Avoid_: UI page, hosted cache, dashboard database

**Ledger View**:
The human-facing surface for inspecting Decision Ledger state and emitting review commands. It is not durable authority.
_Avoid_: Decision Ledger, source of truth

**Governance policy**:
Configurable rules that decide how candidates, review commands, and evidence route to review, advisory state, materialization, or enforcement according to workspace capability.
_Avoid_: connector logic, model prompt, fixed org-chart role

**GovernanceResult**:
A substrate-neutral outcome of governance or conflict analysis. It can express blocking, warning, or informational intent; each substrate maps it to honest enforcement channels.
_Avoid_: CI result only, dashboard warning only

**Signoff**:
The ownership lifecycle on a Decision. Approval is separate from candidate acceptance and separate from code compliance.
_Avoid_: status, compliance, drift, ratification

**Status / compliance state**:
The code-compliance state for a decision. It is computed or reviewed from grounding and drift evidence, not hand-authored as signoff.
_Avoid_: signoff, approval

**Read/write path**:
Review surfaces, MCP tools, integrations, and mods emit substrate-neutral commands/evidence. Governance policy and event store adapters decide materialization.
_Avoid_: UI writes YAML, connector writes canonical decisions directly

**Cloud oracle**:
The private hosted optimization layer that returns graph evidence, conflict signals, grounding suggestions, and advisory governance results.
_Avoid_: hosted daemon, canonical decision store

**Hosted code graph**:
A cached organization-scale code graph across branches, worktrees, repos, and history.
_Avoid_: local grounding, event store substrate

**Hosted reasoning**:
The Bicameral Cloud service that returns advisory reasoning artifacts for extraction, span selection, level hints, binding suggestions, conflict summaries, and review summaries.
_Avoid_: hosted daemon, canonical decision authority, signoff approver

**Compatibility envelope**:
Cloud-owned compatibility result that explains whether a daemon version and capability set can safely participate in hosted/dashboard flows, including any degraded or blocked mutable-control behavior.
_Avoid_: UI state name, daemon authority, general health check

**AuditEvent**:
Cloud-owned coordination audit record for tenancy, session, pairing, compatibility, or transport activity. It preserves advisory coordination history and never becomes Decision Ledger authority, signoff state, source binding, or local daemon projection truth.
_Avoid_: ReviewEvent, Decision, approval, compliance verdict, local workspace event

**AuditAction**:
Enum value naming the auditable cloud coordination action represented by an AuditEvent. It classifies what Cloud observed or routed; it does not grant authority to perform local workspace actions.
_Avoid_: command authorization, user role, policy decision

**FORBIDDEN_AUDIT_FIELDS**:
Boundary list of payload keys that must be stripped before persisting audit details, protecting local/source/project-sensitive data from entering cloud coordination history.
_Avoid_: redaction policy for all product data, permission model, allowlist

**Dashboard pairing state**:
Browser-rendered hosted dashboard state derived from auth, bridge reachability, pairing, daemon response, and compatibility signals. Current dashboard render states are `dashboard_pairing_unpaired`, `dashboard_pairing_unavailable`, and `dashboard_pairing_paired`. These states guide user repair and display operator-home readiness metadata; they do not publish a workspace, grant daemon authority, or persist canonical local projection state.
_Avoid_: daemon pairing source truth, workspace membership, Decision Ledger state, project binding

**Team**:
Tenant boundary for Bicameral Cloud users, membership, ownership, and cloud-scoped authorization. It is cloud organization boundary for alpha tenancy; it is not local daemon workspace authority or billing account authority unless a later decision grants that scope.
_Avoid_: local workspace, daemon role, billing account, source-code organization

**ReasoningArtifact**:
Immutable advisory output from hosted reasoning for one reasoning task and canonical request under a specific policy, prompt, schema, and redaction version.
_Avoid_: Decision, ReviewEvent, model truth, cache row only

**Reasoning policy profile**:
User-facing hosted reasoning cost/quality choice: `lowest_cost`, `balanced`, or `highest_quality`.
_Avoid_: raw model selector, governance policy

**Model policy version**:
Versioned cloud-owned routing policy that maps reasoning tasks to concrete model choices, fallbacks, and cost/quality behavior.
_Avoid_: model name only, prompt version

**ReasoningArtifact replacement**:
Hosted transition that marks a refreshed ReasoningArtifact as current and supersedes the previous current artifact for the same tenant and reasoning task.
_Avoid_: Decision promotion, local ledger event
