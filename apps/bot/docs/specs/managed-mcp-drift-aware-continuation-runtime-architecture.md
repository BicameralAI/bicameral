# Runtime Architecture: Managed MCP Drift-Aware Continuation

**Date:** 2026-07-30
**Status:** proposed
**Level:** L2 cross-runtime Product contract
**Target Product release:** Managed MCP v0.1
**Capability dependency:** `bicameral.semantic-resolution` at
`sha256:f35134886ed4a58a279aaef5de1c1404a4c3676f493f4470e3212d0e2f5a6459`
**Owns:** the post-Planning Managed MCP journey from exact governing-spec
binding through advisory semantic analysis, session-scoped candidate review,
explicit promotion or rejection, and refreshed recall.

**Product Design:** [#979](https://github.com/BicameralAI/bicameral-bot/issues/979)
**Product acceptance:** [#979 comment 5129196565](https://github.com/BicameralAI/bicameral-bot/issues/979#issuecomment-5129196565)
**Governed composition:** [#982](https://github.com/BicameralAI/bicameral-bot/issues/982)
**Companion contract:** [`managed-mcp-drift-aware-continuation-runtime-architecture.contract.json`](./managed-mcp-drift-aware-continuation-runtime-architecture.contract.json)

Governance gate: `bic:spec-governance-gate`; Qor advisory/no-op pending
Factory policy revision.

UserJourney gate: applicable and passed for
`developer_host_mcp_drift_aware_continuation_v1`. Journey Boundary Review:
passed for the specification boundary by composing the owner-confirmed
canonical append/replay review in #938/PR #942 with the accepted session,
presentation, timeout, and authority decisions in #979. Executable authentic
host evidence remains delegated to #943.

[Figma journey gate](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=17-3):
not applicable because this is an MCP text-and-structured
surface inside coding hosts, not because the work lacks a user journey.
`MCPDisplayContractV1` below is the normative display and interaction contract.

## Reading Guide

Read [Product Design #979](https://github.com/BicameralAI/bicameral-bot/issues/979)
for the Product outcome, then this document for the controlling runtime
contract. The most important invariant is:

> Post-Planning preflight is always advisory; candidates are session-scoped
> proposals; only an exact human-confirmed daemon command may append canonical
> Product state.

This contract composes rather than replaces:

- [`smr-alpha-managed-preflight-user-journeys.md`](./smr-alpha-managed-preflight-user-journeys.md);
- [`smr-alpha-claim-semantic-trace-alloy-bridge.md`](./smr-alpha-claim-semantic-trace-alloy-bridge.md);
- [`state-machine-resolver-v1-runtime-architecture.md`](./state-machine-resolver-v1-runtime-architecture.md)
  and amendments 001/002;
- [`mcp-mediated-candidate-conflict-resolution-runtime-architecture.md`](./mcp-mediated-candidate-conflict-resolution-runtime-architecture.md);
- [`semantic-resolution-capability-contract.md`](./semantic-resolution-capability-contract.md),
  contract ID `bicameral.semantic-resolution`, exact artifact SHA-256
  `f35134886ed4a58a279aaef5de1c1404a4c3676f493f4470e3212d0e2f5a6459`.

Managed MCP imports these semantic-resolution obligations:

- source and extraction: `SRC-SOURCE-01`, `SRC-TRACE-01`, `SRC-TRACE-02`,
  `SRC-PROFILE-01`, and `SRC-VALIDATE-01`;
- alignment and deterministic analysis: `SRC-ALIGN-01`, `SRC-PROJECT-01`,
  `SRC-ALLOY-01`, and `SRC-IDENTITY-01`;
- result, provenance, and authority: `SRC-FAILURE-01`, `SRC-SCOPE-01`,
  `SRC-PROVENANCE-01`, and `SRC-AUTHORITY-01`;
- activation evidence: `SRC-EFFICACY-01`, assessed separately from
  deterministic-kernel correctness.

[PR #963](https://github.com/BicameralAI/bicameral-bot/pull/963) remains
historical implementation and benchmark evidence. Because that implementation
was deliberately reverted from the current `dev` line, it is not current
conformance and is not a release prerequisite. A reachable exact subject
revision must be reassessed against the imported `SRC-*` obligations.

Where those artifacts conflict for this exact journey, this spec controls:

1. the Planning hook binds one exact governing specification revision rather
   than treating Plan prose as the promoted source;
2. Managed MCP candidates are retained only for the coding session;
3. explicit rejection is a transient session disposition, not a canonical
   rejection event or cross-session tombstone;
4. a Product Owner may enable policy-guarded approval as part of an explicit
   promotion gesture;
5. generic positional commands are replaced by structured, agent-mediated
   natural-language selection plus exact preview and confirmation.

Artifact-bound ingestion and candidate workflows outside this journey retain
their existing contracts.

## Runtime Problem

The governed specifications establish the exact ClaimTrace-to-SMR obligations
and a canonical candidate-promotion transaction, but the current branch does
not yet provide complete conformance for the imported semantic-resolution
obligations or define the complete developer-session interaction accepted in
#979. In particular:

- the existing managed-preflight journey treats the final Plan message as the
  proposal source, while the accepted experience reviews the exact governing
  spec;
- the existing acceptance contract lets daemon-local candidates outlive a
  coding session and persists rejection fingerprints;
- the existing MCP surface assumes one exact candidate action, while the
  accepted experience allows a user to select a coherent subset naturally;
- the existing lifecycle always creates `Decision(proposed)`, while alpha must
  offer Product Owners an auto-approve-on-promotion policy;
- colorized prose alone is not a reliable machine contract for automated
  coding hosts.

Without one composition contract, Bot, MCP, and the coding host would each
invent incompatible session, selection, confirmation, and cleanup semantics.

## Goals

- Run advisory preflight immediately after Planning through the installed
  developer-host tool hook.
- Bind analysis and presentation to one exact governing spec revision.
- Preserve source spans, ClaimTrace ordering/branches/authority/unknowns, and
  deterministic SMR/Alloy findings without presenting them as accepted truth.
- Keep one active CandidateSet available across turns and reconnects in the
  same coding session.
- Reprint the exact spec with human annotations while returning equivalent
  structured content to the coding agent.
- Let the user express promotion and rejection intent naturally.
- Validate a coherent subset and show the exact effect before confirmation.
- Promote selected candidates through the existing daemon/event-store
  authority boundary.
- Keep explicit rejection and pending candidates transient to the session.
- Erase unpromoted candidate state when the session ends or its reconnect lease
  expires.
- Support Product-owner auto-approval on promotion without granting promotion
  authority to an unattended agent.
- Preserve honest timeout, partial-analysis, response-loss, restart, and
  projection-catch-up outcomes.

## Non-Goals

- CodeGraph-backed completeness or implementation-compliance claims.
- Autonomous promotion inferred from agent output, silence, elapsed time, or a
  successful solver result.
- A dashboard review surface for this alpha journey.
- Cross-session candidate inboxes, rejection memory, semantic suppression, or
  candidate resurrection.
- Decision signoff policy beyond the narrow Product-owner
  auto-approve-on-promotion setting.
- Release readiness, merge safety, deployment, or solver-wide safety claims.
- A second canonical store, second approval lifecycle, or MCP-owned authority.
- Silent fallback from an absent or ambiguous spec binding to Plan prose,
  transcript scraping, filesystem mtime, branch name, or an LLM-generated
  summary.

## Controlling Runtime Decisions

### 1. Planning is the trigger; the exact spec is the source

The host's successful Planning completion event triggers Managed MCP. The hook
submits:

```yaml
ManagedPlanningBoundaryV1:
  product_id: string
  host_kind: claude_code | codex
  host_session_id: string
  host_turn_id: string
  plan_digest: sha256:<exact completed Plan bytes>
  governing_spec:
    repository_id: string
    git_head: <40-character lowercase git sha>
    artifact_path: <normalized repo-relative path>
    blob_id: string
    artifact_content_digest: sha256:<exact spec bytes>
```

`governing_spec` must resolve to the exact current spec revision selected for
the planned work. “Most recent” means the explicit Product/caller binding with
the exact content identity above; it never means filesystem modification time,
the latest branch name, or daemon guessing.

If the binding is absent, ambiguous, unreadable, or mismatched, Managed MCP
returns a typed limitation and does not generate candidates. Plan bytes may
scope retrieval and comparison, but they do not replace or mutate the source
spec.

### 2. One analysis, one session CandidateSet

The daemon validates a source-bound `ClaimSemanticTraceV1`, executes the
deterministic SMR/Alloy bridge, and projects zero or more coherent
`DecisionCandidate`s. Raw ModelAtoms and individual source sentences are not
review units.

```yaml
CandidateSetV1:
  candidate_set_id: uuid
  candidate_set_generation: integer
  candidate_set_digest: sha256:<ordered candidate identities and relations>
  product_id: string
  host_session_id: string
  session_lease_id: opaque
  governing_spec_binding_digest: sha256:<exact spec binding>
  plan_digest: sha256:<exact completed Plan bytes>
  accepted_model_base_position: integer
  accepted_model_base_digest: sha256:<accepted Product model>
  created_at: RFC3339
  lease_expires_at: RFC3339
  candidates:
    - candidate_id: uuid
      candidate_generation: integer
      proposed_decision: string
      source_spans: [SourceSpanV1]
      contradiction_refs: [string]
      dependency_candidate_ids: [uuid]
      claim_trace_digest: sha256:<validated trace fragment>
      proposal_digest: sha256:<exact proposed effect>
      disposition: pending | rejected | promoted
  coverage_limitations: [string]
```

CandidateSet is daemon-owned `candidate_or_proposal` state. A local SQLite
table or equivalent durable operational store may retain it across daemon
restart, but its routing key is `(product_id, host_session_id,
candidate_set_generation)`, not Product history. It is never replayed as
canonical state.

Only one CandidateSet generation is active for a Planning boundary. Retrying
the same analysis identity reuses it. A new completed Planning boundary
creates a new generation but does not silently copy pending or rejected
dispositions.

### 3. Session lease and cleanup

The host maintains the CandidateSet lease through MCP activity or an explicit
heartbeat. Alpha defaults:

- candidate lifetime: the active coding session;
- reconnect grace after the last valid heartbeat: 300 seconds;
- explicit host-session end: immediate cleanup;
- lease expiry after disconnect/crash: cleanup;
- promoted Decisions and canonical receipts: retained;
- pending and rejected candidate content, selection previews, and challenges:
  erased.

A reconnect may resume the CandidateSet only with the same host session and
lease identities. A different session cannot enumerate, promote, reject, or
refresh it.

Session end is not rejection. Pending candidates disappear without a canonical
event, rejection tombstone, Product-history entry, or future suppression
effect.

### 4. MCP session display contract

MCP returns two synchronized projections:

```yaml
human_content:
  exact_spec_text: <exact bound spec bytes>
  annotations:
    - source_span
    - label
    - display_role

structured_content:
  candidate_set_id: uuid
  candidate_set_generation: integer
  candidate_set_digest: sha256:<...>
  spec_binding_digest: sha256:<...>
  candidates:
    - stable identity
    - proposed Decision effect
    - source spans
    - contradiction references
    - dependency identities
    - coverage limitations
```

The exact spec, not an LLM rewrite, is the human review surface. A candidate
may map to multiple spans and a span may support multiple candidates.

Color is presentation only:

- green plus a textual `selected for promotion` label marks spans in the
  current prepared selection;
- red plus a textual `contradiction` label marks a daemon-authored
  contradiction;
- neutral spans carry no promotion selection;
- hosts must provide an accessible configurable palette and preserve textual
  labels when color is unavailable.

Initial candidate availability must not be rendered as already accepted or
already selected.

The absence of a canvas-bound screen does not make display behavior an
implementation detail. Every host adapter must implement this contract:

```yaml
mcp_display_contract:
  id: MCPDisplayContractV1
  user_journey_id: developer_host_mcp_drift_aware_continuation_v1
  surface_kind: mcp_text_and_structured_content
  entrypoint: first_managed_preflight_tool_result_after_successful_planning
  source_of_display_truth: daemon_authored_exact_spec_and_candidate_projection
  presentation_states:
    - analyzing
    - candidate_set_available
    - no_candidate_with_searched_scope
    - limitation_or_timeout
    - selection_preview
    - confirmation_required
    - committed_and_refreshed
    - stale_or_session_expired
  human_content:
    required:
      - exact_spec_text
      - textual_annotation_labels
      - candidate_effects_and_dependencies
      - searched_scope_coverage_and_unknowns
      - exact_next_action_or_typed_recovery
    prohibited:
      - rewritten_spec_as_source
      - color_only_meaning
      - positional_candidate_authority
      - approval_readiness_or_release_inference
  structured_content:
    required:
      - contract_version
      - spec_binding_digest
      - candidate_set_identity_and_generation
      - stable_candidate_identities
      - source_spans_and_relationships
      - contradiction_dependency_and_coverage_metadata
      - allowed_next_operations
      - typed_outcome_and_recovery
  synchronization:
    invariant: human_content_and_structured_content_describe_one_daemon_result
    mismatch_result: presentation_contract_error
  accessibility:
    textual_labels_required: true
    configurable_color_is_supplemental: true
  agent_compatibility:
    natural_language_user_intent: true
    stable_ids_submitted_by_agent: true
    explicit_human_confirmation_required: true
    promote_by_position_or_implicit_agent_confirmation: prohibited
```

The eight `presentation_states` are renderer states, not durable Product or
Candidate lifecycle states. A host may choose Markdown, ANSI, or native text
decoration, but it must preserve the exact content, labels, stable structured
identities, allowed next operations, and typed recovery semantics.

A conforming session projection reads conceptually as:

```text
Managed MCP preflight — advisory
<exact governing specification, with textual source annotations>

Candidates: <count>  Pending: <count>
Selected promotion: <exact effects, or none>
Contradictions / limitations: <daemon-authored facts>
Next action: describe the subset to the coding agent, then review the exact
preview before confirming.
```

This is a structural example, not permission to replace the exact specification
or daemon-authored values with prose.

### 5. Natural-language selection with structured resolution

The user speaks naturally in the active coding session, for example:

> Promote the release immutability and provider-evidence decisions. Reject the
> routing recommendation.

The coding agent resolves this intent only by calling a read-only MCP prepare
operation with stable identities from `structured_content`:

```yaml
CandidateSelectionV1:
  candidate_set_id: uuid
  candidate_set_generation: integer
  candidate_set_digest: sha256:<...>
  promote_candidate_ids: [uuid]
  reject_candidate_ids: [uuid]
```

The daemon, not the agent, verifies:

- every ID belongs to the current CandidateSet generation;
- promoted and rejected sets are disjoint;
- the promoted subset is dependency-closed;
- the accepted-model base, spec binding, proposal digests, actor authority, and
  Product policy revision are current;
- no selected candidate is already promoted or rejected.

If closure is incomplete, the daemon returns the exact missing dependencies
and no preview token. It must not silently add a candidate to the selection.

Unmentioned candidates remain `pending`.

### 6. Exact preview and confirmation

The prepare operation creates no Product or Candidate disposition. It returns:

```yaml
CandidateSelectionPreviewV1:
  selection_digest: sha256:<candidate set + selected ids + exact effects>
  exact_spec_projection: <same spec with selected/contradiction annotations>
  promote_effects: [exact daemon-authored Decision effect]
  reject_effects: [exact daemon-authored transient rejection effect]
  pending_candidate_count: integer
  resulting_signoff:
    mode: proposed | approved_by_owner_policy
    policy_revision: string
  confirmation_challenge: opaque
```

The preview must say whether the action is **Promote selected** or **Promote
and approve selected**. A changed selection, proposal, spec, accepted-model
base, actor, policy revision, display summary, or CandidateSet generation
invalidates the challenge.

The agent may explain the preview but may not confirm on the user's behalf.
Only an explicit human confirmation associated with the bound actor continues.

### 7. Promotion and explicit rejection

One confirmed selection is applied as follows:

1. revalidate the exact CandidateSet, selection, actor, policy, and accepted
   model base;
2. compare-and-append the selected candidate promotion events as one atomic
   event-store transaction, or append none;
3. replay the canonical event(s) into Decision and SMR projections;
4. mark selected candidates `promoted` and explicitly rejected candidates
   `rejected` in session working state;
5. leave unmentioned candidates `pending`;
6. return one selection receipt and refreshed read-only preflight.

Each promoted candidate remains a distinct Decision and distinct replayable
promotion event. The transaction may contain several events, but partial
canonical success is prohibited. Decision identity is deterministic from
`(product_id, selection_operation_id, candidate_id)`.

Explicit rejection:

- requires the same exact preview and human confirmation;
- writes only session CandidateSet disposition;
- appends no `claim_set_candidate_rejected` event;
- creates no cross-session tombstone or suppression;
- does not assert the candidate is semantically false;
- is erased with the CandidateSet at session end.

This is a journey-scoped refinement. Durable rejection in other ingestion or
candidate-review workflows is unchanged.

### 8. Product-owner auto-approve on promotion

The Product exposes canonical policy:

```yaml
auto_approve_on_promotion:
  enabled: boolean
  policy_revision: string
```

When disabled, authorized promotion produces `Decision(proposed)`.

When enabled, the daemon may produce `Decision(approved)` in the same atomic
promotion transaction only when:

- the confirming actor is a current Product Owner at append;
- the exact preview said `Promote and approve`;
- the confirmation binding includes the policy revision and approved effect;
- the policy remains current at compare-and-append.

The transition must have no externally observable intermediate
`Decision(proposed)` state. A policy change, owner-role revocation, or stale
preview returns a typed no-write result. MCP and the coding agent cannot set,
infer, or override this policy.

This setting combines two consequences into one explicit owner gesture. It
does not grant unattended agents promotion or approval authority.

### 9. Advisory timeout

The alpha Product governance policy sets:

```yaml
managed_preflight_timeout_seconds: 60
```

At the deadline, the tool returns `analysis_timed_out` with searched scope and
known limitations, cancels the timed-out analysis attempt, and lets the coding
session continue. It must not return `no_candidate`, `no_conflict`,
`safe_to_proceed`, or any equivalent reassurance.

Retrying creates or reuses work only under the exact analysis identity. The
timeout never becomes a work prohibition, governance event, candidate
disposition, Decision state, or release signal.

## Runtime Components

| ID | Element | Responsibility | Prohibited authority |
| --- | --- | --- | --- |
| `ELM-DEVELOPER` | Product participant | Plan, express review intent, and explicitly confirm | Cannot bypass daemon authorization or make solver output canonical |
| `ELM-HOST` | Released coding host and installed hook | Emit the exact Planning boundary and carry session identity | Cannot choose canonical Product state or fabricate a spec binding |
| `ELM-MCP` | Packaged MCP tool surface | Relay structured content, natural-language-resolved selection, preview, confirmation, and results | Cannot rank authoritatively, mint confirmation, or persist Product state |
| `ELM-DAEMON` | Product daemon | Validate identity, own CandidateSet, authorize transitions, and orchestrate canonical writes | Cannot infer human confirmation or treat local working state as canonical |
| `ELM-EXTRACTOR` | ClaimTrace extraction boundary | Produce a closed-schema, source-bound advisory ClaimTrace | Cannot approve, promote, or define canonical identity |
| `ELM-SMR` | Deterministic SMR/Alloy kernel | Validate/lower the ClaimTrace and return bounded findings | Cannot claim global completeness or mutate Decisions |
| `ELM-SESSION-STORE` | Daemon-local session store | Retain CandidateSet, leases, previews, and transient dispositions | Cannot become Product history or suppression authority |
| `ELM-EVENT-STORE` | Selected Product event store | Atomically append accepted promotion/signoff effects | Cannot accept MCP-local writes or partial batches |
| `ELM-PROJECTION` | Decision/SMR replay projection | Rebuild accepted current state before recall | Cannot call extraction during replay or return stale state as current |

The canonical companion carries the full component fields.

## Runtime Relationships

| ID | Source → destination | Contract |
| --- | --- | --- |
| `REL-HOST-MCP` | Host → MCP | Planning hook with exact session, Plan, and spec binding |
| `REL-MCP-DAEMON` | MCP → daemon | Versioned ToolRequest/ToolResult; no authority grant |
| `REL-DAEMON-EXTRACTOR` | Daemon → extractor | Bounded source context and accepted-model inputs |
| `REL-EXTRACTOR-DAEMON` | Extractor → daemon | Untrusted `ClaimSemanticTraceV1` |
| `REL-DAEMON-SMR` | Daemon → SMR kernel | Validated trace and exact bounded snapshot |
| `REL-DAEMON-SESSION` | Daemon → session store | CandidateSet, lease, preview, transient disposition |
| `REL-DAEMON-MCP` | Daemon → MCP | Exact spec plus synchronized structured candidate projection |
| `REL-DEVELOPER-MCP` | Human → MCP | Natural-language intent and explicit confirmation |
| `REL-DAEMON-EVENT` | Daemon → event store | Atomic exact promotion/approval append |
| `REL-EVENT-PROJECTION` | Event store → projection | Replay and catch-up |

## Runtime State Classification

| ID | Artifact | Class | Writer / persistence | Lifecycle |
| --- | --- | --- | --- | --- |
| `STATE-SPEC` | Exact governing spec revision | evidence | Daemon capture / content-addressed evidence | Immutable input for one analysis identity |
| `STATE-CLAIMTRACE` | Validated source-bound ClaimTrace | candidate/proposal | Daemon / session working state | Rebuildable advisory analysis artifact |
| `STATE-ANALYSIS` | Bounded SMR/Alloy result | derived projection | SMR kernel / session working state | Advisory and scope-limited |
| `STATE-CANDIDATE-SET` | Session CandidateSet | candidate/proposal | Daemon / session store | Active until explicit session end or lease expiry |
| `STATE-SELECTION` | Exact selection preview/challenge | local scratch | Daemon / session store | Single-use, short-lived, session-bound |
| `STATE-POLICY` | Auto-approve and timeout Product policy | canonical state | Product policy authority / selected canonical substrate | Versioned and replayable |
| `STATE-PROMOTION-EVENT` | Accepted promotion/approval event batch | accepted event | EventStoreAdapter / canonical Product event log | Immutable and replayable |
| `STATE-DECISION` | Promoted Decision and signoff state | canonical state | Event replay / Decision Ledger | Durable Product history |
| `STATE-RECALL` | Refreshed RecallPacket/preflight result | derived projection/read model | Daemon projection | Recomputable and never authority |

## State Refinement Check

Smallest honest alpha lifecycle:

```text
exact spec bound
  -> analysis completed | no_candidate | limitation | timed_out
  -> session CandidateSet pending
  -> exact selection previewed
  -> explicit confirmation
       -> selected promoted
       -> selected explicitly rejected in session
       -> unmentioned remain pending
  -> refreshed advisory recall
  -> session end erases unpromoted working state
```

States refined:

- `CandidateSet pending` is daemon-local session state, not canonical Product
  state.
- `rejected` is a session disposition for this journey, not a durable
  rejection event.
- auto-approved is not a new Decision state; it is the existing `approved`
  signoff state reached atomically under bound Product policy.
- `analysis_timed_out` is a typed operation result, not a lifecycle state.

Transition authority and persistence:

| Transition | Authority | Persistence |
| --- | --- | --- |
| analyze exact spec | host triggers; daemon validates | session working state |
| prepare selection | user intent relayed by agent; daemon validates | short-lived session state |
| reject selected | explicit human confirmation; daemon applies | session CandidateSet only |
| promote selected | authorized human confirmation; daemon/event store commits | canonical events |
| auto-approve with promotion | current Product Owner plus current policy | same atomic canonical transaction |
| expire session | explicit host end or daemon lease expiry | deletes transient session state only |

Invariants:

- no preflight or timeout outcome prohibits coding;
- no candidate, ClaimTrace, solver result, color, receipt, or agent statement is
  accepted Product truth;
- session rejection never creates cross-session suppression;
- promotion and approval effects are exactly previewed and bound;
- canonical append is the only Product-state commit point;
- replay never invokes extraction;
- unselected candidates remain pending until session cleanup;
- CodeGraph remains outside the alpha closure path.

Deferred vocabulary:

- cross-session candidate inbox;
- durable semantic rejection memory;
- agent-delegated promotion grants;
- collaborative/multi-user live CandidateSet review;
- CodeGraph-backed completeness;
- standalone contradiction acknowledgement/resolution.

## State Minimization Review

- `analysis_timed_out`, `no_candidate`, `spec_binding_missing`, and
  `dependency_closure_required` remain typed results, not durable states.
- `selected` remains preview metadata, not a Candidate lifecycle state.
- reconnect is a lease continuation, not a new Product or Candidate state.
- color roles remain rendering metadata.
- auto-approve reuses `Decision(approved)` instead of introducing
  `auto_approved`.
- rejection is intentionally not promoted into Product history for this
  journey.

The only new persisted operational aggregate is CandidateSet. It is necessary
because automated coding sessions span turns and MCP calls, while the agent's
prompt is not a reliable state store.

## Authority Ownership Matrix

| Verb | Developer | Coding agent | MCP | Daemon | Extractor | SMR | Event store |
| --- | --- | --- | --- | --- | --- | --- | --- |
| observe exact spec | yes | yes | relay | validate | bounded input | bounded input | no |
| propose candidate | no | no | no | materialize working proposal | advisory output | bounded findings | no |
| select intent | explicit language | resolve to structured IDs | relay | validate | no | no | no |
| reject candidate | explicitly confirm | never on own | relay | session disposition | no | no | no |
| promote | explicitly confirm | never on own | relay | authorize and construct | no | no | append only |
| approve on promotion | Product Owner gesture only | never | relay | verify current policy/owner | no | no | append only |
| render | inspect | explain | render | author exact effect | no | no | no |
| claim safety/readiness | no component may make this claim from preflight | | | | | | |

## Runtime Scenarios

### `SCN-PREFLIGHT`

Planning completes; the installed host hook sends one exact spec binding. The
daemon validates/extracts, invokes the deterministic kernel, stores one
CandidateSet generation, and returns exact annotated spec text plus synchronized
structured content. `no_candidate` and limitations remain bounded results.

### `SCN-TIMEOUT`

Analysis reaches the 60-second policy deadline. The attempt is canceled and the
host receives `analysis_timed_out`. Work continues; no Candidate or reassurance
is manufactured.

### `SCN-NATURAL-SELECTION`

The user describes desired Decisions. The agent resolves that intent through
the prepare tool. The daemon validates IDs and dependency closure, then returns
an exact preview. No mutation occurs.

### `SCN-PROMOTE`

The user confirms. The daemon revalidates the bound selection and atomically
appends each selected promotion event, or none. Replay creates durable
Decisions; unmentioned candidates remain pending.

### `SCN-AUTO-APPROVE`

The exact preview states `Promote and approve`; the current Product Owner
confirms while the bound Product policy remains enabled. The atomic transaction
replays directly to `Decision(approved)`.

### `SCN-REJECT`

The exact preview identifies selected transient rejections. After explicit
confirmation, the daemon marks those CandidateSet entries rejected. No
canonical event or cross-session tombstone is written.

### `SCN-RECONNECT-AND-CLEANUP`

The same host session reconnects during the 300-second grace period and resumes
the CandidateSet. Explicit end or lease expiry erases pending/rejected
candidates and scratch challenges. Promoted Decisions remain.

### `SCN-RESPONSE-LOSS`

Canonical append succeeds but the response is lost. The same bound operation
returns the existing canonical result. It must not append a duplicate batch or
reapply auto-approval.

### `SCN-FORBIDDEN-AGENT-PROMOTION`

An agent calls commit without a valid human-bound challenge, changes the
selection after preview, or attempts to enable auto-approval. The daemon
returns a typed no-write result.

## User Journey Contract

```yaml
user_journey:
  id: developer_host_mcp_drift_aware_continuation_v1
  name: Managed MCP Drift-Aware Continuation
  actor: authenticated_product_participant_using_a_released_coding_host
  entrypoint: successful_post_planning_tool_hook
  success_state: exact_spec_reviewed_and_selected_dispositions_reflected_in_refreshed_recall
  surfaces:
    - released_developer_host
    - packaged_mcp
    - local_product_daemon
    - daemon_session_store
    - canonical_product_event_store
  boundary_contracts:
    - ManagedPlanningBoundaryV1
    - ClaimSemanticTraceV1
    - MCPDisplayContractV1
    - CandidateSetV1
    - CandidateSelectionV1
    - CandidateSelectionPreviewV1
    - ConfirmationChallenge
    - accepted_candidate_promotion_events
    - RecallPacket
  required_evidence:
    - producer_consumer_contract_tests
    - deterministic_kernel_integration
    - restart_and_lease_test
    - canonical_append_and_response_loss_test
    - released_host_real_process_journey
```

Catalog execution selection:

```yaml
journey_environment:
  catalog_ref: docs/architecture/bicameral-product/journey-environments.json
  journey_id: developer_host_mcp_v1
  environment_ids:
    - developer-host-environment-v1
    - local-product-environment-v1
```

The catalog journey remains `partial`: authentic activation is not proven
until the normal setup path installs the real managed hook and the released
host fires it. Issue #943 owns that terminal evidence.

### Journey Boundary Review

```text
Developer
   │ natural-language intent / explicit confirmation
   ▼
Released host ──Planning hook──> Packaged MCP ──typed request──> Product daemon
                                                               │
                                     ┌─────────────────────────┴────────────┐
                                     ▼                                      ▼
                              Session CandidateSet                 Canonical event store
                              transient working state              sole Product commit
                                                                            │
                                                                            ▼
                                                                  replayed Decision/SMR
```

Numbered boundary walkthrough:

1. The host emits an exact Planning boundary and spec binding. This is evidence,
   not Product authority.
2. MCP relays it. The daemon validates the source, builds ClaimTrace, runs the
   deterministic kernel, and stores transient CandidateSet state.
3. MCP renders daemon-authored exact spec annotations and structured candidate
   metadata. Text/color and structured content can temporarily disagree only
   as a typed presentation error; neither is canonical.
4. The user describes a subset. The agent relays stable IDs to prepare an exact
   preview. No commit occurs.
5. The user confirms. The daemon revalidates current actor, policy, source,
   candidate generation, selection, and accepted-model base.
6. The event store atomically appends all selected promotions or none. This is
   the sole canonical commit point.
7. Session rejection and CandidateSet status reconcile after canonical append.
   Failure here cannot undo or duplicate canonical state.
8. Replay rebuilds Decisions/SMR and MCP shows refreshed recall.
9. Session end erases pending/rejected candidates. Canonical Decisions remain.

Independently observable facts:

- host Planning event;
- exact spec binding;
- ClaimTrace/SMR analysis result;
- session CandidateSet and lease;
- exact selection preview;
- canonical promotion event batch;
- session disposition reconciliation;
- replayed Decision/SMR;
- MCP response and terminal journey receipt.

Partial failures and correct recovery:

| Failure | User-visible result | Correct recovery | Forbidden recovery |
| --- | --- | --- | --- |
| missing/ambiguous spec binding | typed limitation; no candidates | supply one exact binding and retry | analyze Plan prose as the spec |
| analysis timeout | `analysis_timed_out`; work continues | retry the exact analysis identity | return no-conflict reassurance |
| dependency-incomplete subset | exact missing dependency list; no preview | amend selection | silently add candidates |
| role/policy/base changes after preview | typed no-write stale result | prepare a new preview | spend the old challenge |
| append response lost | ambiguous response | retry same operation and recover canonical receipt | append another batch |
| session reconciliation fails after append | canonical success plus reconciliation-required | replay/catch up and reconcile | roll back or duplicate canonical append |
| disconnect before promotion | resume within lease grace | reconnect with same session/lease | expose CandidateSet to a new session |
| session ends | pending/rejected state erased | run a new Planning analysis later | reconstruct rejection memory as Product truth |

Sensitive exclusions:

- provider secrets, unrelated session text, raw reasoning chain, and unrelated
  source bodies never enter canonical events;
- event payloads retain only accepted source/proposal identity and evidence
  required for Decision replay;
- session CandidateSet content remains local.

Validation witnesses—logs, challenges, receipts, process ancestry, governance
results, and topology reports—never become Product state or journey success by
themselves.

```yaml
journey_boundary_review:
  status: passed
  journey_ids:
    - developer_host_mcp_drift_aware_continuation_v1
  authoritative_commit_points:
    - canonical_candidate_promotion_batch_append
  independently_observable_facts:
    - exact_spec_binding
    - session_candidate_set
    - exact_selection_preview
    - canonical_promotion_batch
    - replayed_decision_and_smr
    - refreshed_mcp_recall
  partial_failure_reviewed: true
  retry_and_idempotency_reviewed: true
  validation_witness_separated_from_product_state: true
  user_confirmed: true
  source_ref: https://github.com/BicameralAI/bicameral-bot/issues/979#issuecomment-5129196565
```

UserJourney gate: applicable and passed. The terminal validation slice must
exercise the displayed MCP journey end to end against production processes.

Figma journey gate: not applicable. The
[live Product journey index](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=17-3)
does not bind this renderer-neutral MCP interaction inside coding hosts.
`MCPDisplayContractV1`, producer/consumer fixtures, and the released-host
journey receipt are the required design and evidence surfaces instead.

## Event, Command, and Protocol Contracts

| Contract | Producer → consumer | Required behavior |
| --- | --- | --- |
| `ManagedPlanningBoundaryV1` | Host/MCP → daemon | Exact session, Plan digest, and governing spec binding; no inferred fallback |
| `managed_preflight.run` | MCP → daemon | Read/analyze under 60-second advisory policy; no canonical write |
| `CandidateSetV1` | Daemon → session store/MCP | Stable IDs, spans, dependencies, contradictions, exact source and base digests |
| `candidate_selection.prepare` | MCP → daemon | Read-only closure/currentness/authority validation and exact preview |
| `candidate_selection.commit` | MCP → daemon | Human-bound challenge; atomic selected promotion plus transient rejection reconciliation |
| `candidate_session.heartbeat` | MCP → daemon | Extend only the matching session lease |
| `candidate_session.end` | Host/MCP → daemon | Erase unpromoted session state |
| promotion events | Daemon → event store | One distinct event per selected candidate in one atomic transaction |
| `RecallPacket` | Daemon → MCP | Accepted, proposed, contradictions, evidence, searched scope, and unknowns remain distinct |

Protocol versioning:

- every new payload has a version discriminator;
- older clients receive `unsupported_contract_version`;
- no client translates positional display numbers into authority;
- stale CandidateSet generations or selection digests fail closed;
- exact string/color rendering is not the semantic contract.

## Deployment and Runtime Topology

Startup order:

1. selected Product daemon and canonical event substrate;
2. packaged MCP bound to that daemon/Product;
3. released coding host with the normally installed Planning hook;
4. coding session establishes lease on first post-Planning preflight.

Working directory is not Product identity, spec identity, or session identity.
Multiple coding sessions may exist, but each CandidateSet is routed by Product
plus host session and lease identity. No session may enumerate another
session's CandidateSet.

## Error, Retry, and Recovery

Typed failures must include at least:

- `spec_binding_missing`;
- `spec_binding_ambiguous`;
- `spec_binding_mismatch`;
- `analysis_timed_out`;
- `analysis_incomplete`;
- `candidate_set_stale`;
- `candidate_not_in_set`;
- `candidate_dependency_required`;
- `selection_changed`;
- `policy_revision_changed`;
- `owner_authority_required`;
- `confirmation_required`;
- `confirmation_invalid`;
- `base_snapshot_drift`;
- `canonical_append_failed`;
- `reconciliation_required`;
- `model_unavailable`;
- `session_lease_expired`;
- `unsupported_contract_version`.

No failure may be translated to `no_candidate`, `no_conflict`, approval,
readiness, or release safety.

## Observability, Audit, and Receipts

Operational traces may record Product/session opaque IDs, request/operation
IDs, candidate counts, timing, typed outcomes, and exact artifact/contract
digests. They must exclude source bodies, raw prompts, raw reasoning chain,
provider secrets, and confirmation material.

`CandidateSelectionReceiptV1` records:

- exact candidate set and selection digests;
- selected promoted/rejected/pending identities;
- policy revision and resulting signoff mode;
- canonical event and Decision identities for promoted candidates;
- canonical append status;
- session reconciliation status;
- replay/projection position;
- typed limitations.

The receipt is an operational witness. Canonical events and replayed Decisions
are Product state.

## Security, Privacy, and Authority

- MCP sees only Product/session content authorized for the local actor.
- The agent receives candidate IDs for tool mediation but cannot derive a
  confirmation challenge or authority grant.
- CandidateSet and exact spec content remain local in alpha.
- Hosted services receive no spec body, candidate body, or provider output.
- Confirmation challenges are actor/session/Product/selection/policy/base
  bound, short-lived, and single-use.
- Auto-approve policy is canonical Product policy; only the Product policy
  authority may change it.
- The daemon rechecks current actor role and policy at canonical append.

## Runtime Architecture Conformance

| ID | Status | Required behavior | Prohibited fallback | Evidence obligation |
| --- | --- | --- | --- | --- |
| `CONF-MCP-PLAN-HOOK-01` | missing | Successful Planning completion triggers one advisory managed preflight with exact session identity | Manual-only invocation or transcript scraping claimed as the journey | Released-host topology |
| `CONF-MCP-SPEC-BINDING-01` | missing | Analysis and rendering bind one exact governing spec revision | Plan prose, mtime, branch name, or LLM summary substitutes for spec identity | Contract + real-process integration |
| `CONF-MCP-CLAIMTRACE-SMR-01` | missing | A source-bound ClaimTrace and deterministic analysis path have current exact-revision conformance for imported obligations `SRC-SOURCE-01`, `SRC-TRACE-01`, `SRC-TRACE-02`, `SRC-PROFILE-01`, `SRC-VALIDATE-01`, `SRC-ALIGN-01`, `SRC-PROJECT-01`, `SRC-ALLOY-01`, `SRC-IDENTITY-01`, `SRC-FAILURE-01`, `SRC-SCOPE-01`, `SRC-PROVENANCE-01`, and `SRC-AUTHORITY-01`; `SRC-EFFICACY-01` remains a separate activation evidence lane | Regex-only, model-only, fixture-only, historical, reverted, or partial obligation evidence claimed as current deterministic semantic resolution | Obligation-scoped component evidence plus real daemon integration against reachable exact revisions |
| `CONF-MCP-CANDIDATE-SESSION-01` | missing | CandidateSet persists only across the matching coding session and reconnect lease | Agent prompt, global candidate inbox, or canonical event log stores pending candidates | Restart/lease integration |
| `CONF-MCP-SPAN-PROJECTION-01` | missing | Exact spec text and structured many-to-many span/candidate mappings remain synchronized | Rewritten summaries or color alone become the agent contract | Producer/consumer contract |
| `CONF-MCP-DISPLAY-01` | missing | `MCPDisplayContractV1` exposes the correct renderer state, exact human content, synchronized structured content, accessible labels, allowed next actions, and typed recovery throughout the journey | Missing visual-design binding is treated as no UX contract; renderer invents state, hides recovery, uses positions or color as authority, or presents evidence as approval | MCP producer/consumer display fixtures plus released-host journey assertions |
| `CONF-MCP-SELECTION-CLOSURE-01` | missing | Daemon validates exact IDs, current generation, disjoint dispositions, and dependency-closed promotion subset | Agent silently adds dependencies or promotes an incoherent subset | Contract negative controls |
| `CONF-MCP-PREVIEW-AUTH-01` | missing | Exact preview binds selection, effects, actor, session, policy, spec, base, and display summary before human confirmation | Agent confirmation, stale preview, or positional number grants authority | Authority contract tests |
| `CONF-MCP-BATCH-COMMIT-01` | missing | All selected promotions append atomically as distinct events or none; response-loss retry is idempotent | Partial batch success or duplicate retry | Event-store integration + restart/replay |
| `CONF-MCP-SESSION-REJECTION-01` | missing | Explicit rejection changes session CandidateSet only and is erased at session end | Canonical rejection event or cross-session tombstone for this journey | Contract + cleanup test |
| `CONF-MCP-AUTO-APPROVE-01` | missing | Current Product Owner and bound current policy may atomically produce approved Decisions from explicit promotion | MCP/agent enables policy, background approval, or visible intermediate proposed state | Authority + replay tests |
| `CONF-MCP-ADVISORY-TIMEOUT-01` | missing | 60-second policy timeout returns an honest nonblocking limitation and cancels the attempt | Timeout becomes no-conflict or prohibition | Deterministic timeout test |
| `CONF-MCP-RECONNECT-CLEANUP-01` | missing | Same-session reconnect within 300 seconds resumes; end/expiry erases transient state | New session inherits candidates or pending becomes rejected Product state | Restart/lease topology |
| `CONF-MCP-NO-LAUNDERING-01` | missing | Accepted, proposed, solver result, evidence, limitation, receipt, approval, and release claims stay distinct | Any observation or witness acquires stronger authority | Adversarial contract suite |
| `CONF-MCP-JOURNEY-01` | missing | Released host, installed hook, packaged MCP, daemon, session store, canonical store, replay, and cleanup exercise the full journey | Component harness claimed as terminal journey | Real-process authentic-host validation |

The canonical JSON companion preserves these IDs and their source
relationships.

## Implementation Sequencing

### Slice A — Runtime semantic analysis and RecallPacket

Connect the post-Planning exact spec binding through ClaimTrace extraction,
currently conformant implementations for the imported
`bicameral.semantic-resolution` obligations, session CandidateSet creation,
and daemon-authored RecallPacket. Preserve honest no-candidate, limitation,
timeout, and unknown outcomes. If any required obligation lacks current
conformance, return its governed unavailable, refusal, or unknown outcome; do
not substitute a weaker analysis path.

Conformance: `CONF-MCP-SPEC-BINDING-01`,
`CONF-MCP-CLAIMTRACE-SMR-01`, `CONF-MCP-ADVISORY-TIMEOUT-01`,
`CONF-MCP-NO-LAUNDERING-01`.

### Slice B — CandidateSet session lifecycle

Implement daemon-owned CandidateSet routing, lease/heartbeat, reconnect,
generation reuse, explicit end, expiry cleanup, transient rejection, and
cross-session isolation.

Conformance: `CONF-MCP-CANDIDATE-SESSION-01`,
`CONF-MCP-SESSION-REJECTION-01`, `CONF-MCP-RECONNECT-CLEANUP-01`.

### Slice C — Agent-compatible MCP review and canonical disposition

Implement synchronized exact-spec/structured rendering, natural-language
selection mediation, closure validation, exact preview/challenge, atomic
promotion batch, and Product-owner auto-approve policy.

Conformance: `CONF-MCP-SPAN-PROJECTION-01`, `CONF-MCP-DISPLAY-01`,
`CONF-MCP-SELECTION-CLOSURE-01`, `CONF-MCP-PREVIEW-AUTH-01`,
`CONF-MCP-BATCH-COMMIT-01`, `CONF-MCP-AUTO-APPROVE-01`.

### Slice D — Terminal journey validation

Extend/consume #943's reusable developer-host topology so the normal installer
provisions the hook and released Codex/Claude fires it. Exercise both
no-candidate/limitation and promotion/rejection paths with replay, response
loss, reconnect, and teardown.

Conformance: `CONF-MCP-PLAN-HOOK-01`, `CONF-MCP-JOURNEY-01`, plus terminal
evidence for every earlier conformance row.

## Issue Generation Notes

Lifecycle anchors:

- Reasoning Projection Stage 1;
- Decision Stage 0;
- Tool Surface Contract;
- Action Authorization Policy.

The smallest issue DAG is:

```text
exact governed runtime contract
   ├── Slice A: semantic analysis + RecallPacket
   └── Slice B: CandidateSet session lifecycle
          └──────────────┐
                         ▼
                 Slice C: MCP review + disposition
                         │
                         ▼
                 Slice D: terminal User Journey
```

Slices A and B may begin in parallel. Slice C is hard-blocked on their exact
contracts. Slice D is hard-blocked on all implementation slices and the
executable topology profile from #943.

All implementation issues must:

- link #979, #982, this exact spec revision, its governance result, and the
  canonical JSON companion;
- preserve applicable `CONF-*` IDs verbatim;
- reference existing #955, #958, #960, #943 and relevant open MCP work instead
  of duplicating it;
- declare evidence level and test-double boundary;
- retain one terminal real-process User Journey issue.

## ADR Implications

- ADR-0002 is reaffirmed: MCP is a Tool Surface, not canonical authority.
- ADR-0004 is reaffirmed: probabilistic extraction cannot approve meaning.
- ADR-0007 is refined only for this journey: standard durable
  `reject_candidate` remains available elsewhere, while Managed MCP
  `CandidateSetV1` rejection is session-local and noncanonical.
- ADR-0015 is reaffirmed: reasoning output is advisory.
- ADR-0028 is reaffirmed: CodeGraph is outside alpha SMR closure.
- State Machine Resolver v1 is amended for this journey's session lifetime,
  subset selection, transient rejection, and Product-owner
  auto-approve-on-promotion path.

A separate ADR is unnecessary because this document scopes the refinement to
one reversible alpha Tool Surface journey and cites the durable Product-owner
decision.

## Acceptance Criteria

- [ ] Planning completion triggers advisory preflight without a second user
      gesture.
- [ ] One exact governing spec revision is the extraction and rendering source.
- [ ] CandidateSet remains available across turns/reconnects in the same
      session and never leaks across sessions.
- [ ] The exact spec and structured candidate projection remain synchronized.
- [ ] Every MCP presentation state conforms to `MCPDisplayContractV1`, including
      accessible text labels, structured next operations, and typed recovery;
      lack of a visual-design binding never bypasses UserJourney validation.
- [ ] Natural-language intent becomes a daemon-validated exact selection
      preview without exposing positional commands to the user.
- [ ] Incoherent partial promotion fails visibly with missing dependencies.
- [ ] Explicit confirmation promotes selected candidates atomically and leaves
      unmentioned candidates pending.
- [ ] Explicit rejection stays session-local and produces no canonical
      rejection tombstone.
- [ ] Session end/lease expiry erases every unpromoted candidate.
- [ ] Auto-approve works only for a current Product Owner under current bound
      Product policy and has no intermediate proposed state.
- [ ] Timeout at the configured 60-second policy is advisory and never
      reassuring.
- [ ] Response loss/restart returns the prior canonical result without
      duplicate append.
- [ ] Refreshed recall distinguishes accepted, proposed, contradiction,
      evidence, searched scope, and unknowns.
- [ ] Real released-host evidence proves the installed hook path; component
      evidence cannot claim journey completion.

## Validation Plan

- Validate the canonical runtime architecture companion with Factory's
  `runtime_architecture_contract.py`.
- Validate exact spec-binding and structured MCP payload schemas with positive
  and negative fixtures.
- Validate `MCPDisplayContractV1` across every presentation state in packaged
  MCP producer/consumer fixtures, including human/structured mismatch,
  no-color accessibility, stale-session recovery, and forbidden positional
  authority.
- Reassess the deterministic ClaimTrace/SMR procedures demonstrated by
  historical PR #963 against a reachable exact revision and the imported
  `SRC-*` obligations; do not reuse the historical result as current evidence.
- Run session-store restart, heartbeat, lease-expiry, cleanup, and isolation
  integration tests.
- Run authority tests for forged/stale selection, agent confirmation,
  cross-session access, Product role change, and policy revision change.
- Run event-store batch atomicity, response-loss, replay, and projection
  catch-up tests.
- Run packaged MCP producer/consumer tests in both supported hosts.
- Run #943's authentic-host topology with the normally installed hook and
  sanitized journey/teardown receipts.

## Open Questions

None block implementation slicing. Wire encoding, datastore table layout,
terminal color palette values, and internal module names are implementation
choices so long as they satisfy this contract.

## Request Coverage

| Accepted request | Contract section |
| --- | --- |
| Post-Planning tool-hook trigger | Controlling Decision 1 |
| Advisory-only and 60-second policy | Controlling Decision 9 |
| Automatic candidate proposal; explicit promotion | Decisions 2, 5–7 |
| Product-owner auto-approve setting | Controlling Decision 8 |
| Session-scoped CandidateSet and cleanup | Decisions 2–3 |
| Exact spec reprint and configurable annotations | Controlling Decision 4 |
| MCP text-surface display and UserJourney contract | Controlling Decision 4 and User Journey Contract |
| Agent-compatible natural-language subset selection | Decisions 5–6 |
| Promotion and explicit rejection in alpha | Controlling Decision 7 |
| No CodeGraph alpha dependency | Non-Goals and conformance |

Release Unit: not applicable for this specification-only PR. Implementation
slices must declare shipping Release Units against the Product release to which
they are assigned. The Product target for this issue set is Managed MCP v0.1.
Semantic-resolution implementation slices may land earlier and report
obligation-scoped conformance, but Managed MCP activation requires a release
binding to the exact capability-contract digest, current reachable evidence,
and terminal journey evidence.
