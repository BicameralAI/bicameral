# MCP-Mediated DecisionCandidate Conflict Resolution

Date: 2026-07-17

Status: accepted feature proposal

Level: L2 cross-repository contract

Owns: the product and authority contract for resolving a bot-selected `DecisionCandidate` from an MCP-rendered `RecallPacket` without giving MCP ranking, policy, lifecycle, or persistence authority.

Related context: `BicameralAI/bicameral-mcp#733`; `BicameralAI/bicameral-bot#797`; `BicameralAI/bicameral-bot#823`; `BicameralAI/bicameral-bot#861`; `CONTEXT.md`; `docs/domain/decision-lifecycle.md`; `docs/domain/runtime-protocol.md`; `docs/specs/default-signal-policy-v0.md`; `docs/specs/constraint-lookup-recallpacket-contract.md`; `docs/specs/dashboard-governance-actions-rfq-runtime-architecture.md`; `docs/specs/mcp-authenticated-governance-surface.md`; `docs/adr/0011-known-evidence-collision-semantics-and-release-valve.md`; `docs/adr/0024-librarian-recall-and-correction-capture-boundary.md`.

Composed runtime spec: `docs/specs/mcp-mediated-candidate-conflict-resolution-runtime-architecture.md`.

Governance gate: `bic:spec-governance-gate`; Qor advisory/no-op pending factory policy revision. `UserJourney`: `mcp_active_work_recall_and_candidate_promotion` plus included `mcp_decision_signoff`. Journey Boundary Review: **accepted by Jin on 2026-07-17**. The cross-process topology, commit point, idempotent retry, restart-invalidated challenge policy, omission of candidate-to-signoff routing, direct signoff for accepted Decisions, omission of generic candidate accept/reject, inclusion of read-only governance inspection, outcome-specific authority matrix, alpha MCP action inventory, and supersession of the dashboard-only surface rule are confirmed. Kevin owns the delegated alpha inclusion decision for `dismissed_as_noise`; omission remains the accepted default unless an amended contract passes its integration gate.

## Reading Guide

Read `Decision`, `Lifecycle Classification`, and `Lifecycle Ownership` first. The controlling invariant is: **MCP mediates an explicit human choice; bot decides what choices are eligible and is the only component allowed to validate and materialize the result.**

## Problem

Current documents describe two incompatible alpha boundaries. Older Bot ADRs and specs keep conflict resolution on the dashboard. The newer MCP owner decision allows MCP to mediate a narrow candidate-promotion flow, including a new constraint, supersession, scoped coexistence, a proposed candidate-to-signoff route now excluded from alpha, and a proposed dismissal outcome whose alpha inclusion is not yet accepted.

The code names those outcomes, but naming is not a complete lifecycle contract. Current implementation does not prove RecallPacket currentness, treats the presence of an opaque approval proof as sufficient, drops supersession and scope data from canonical event materialization, and can report a signoff route without persisting one. Without one governing delta spec, MCP and Bot can independently guess ranking, authority, persistence, retry, and recovery semantics.

## Goals

- Permit MCP to render only bot-selected, bot-ranked candidates and bot-declared eligible actions.
- Establish MCP as a first-class governance interaction surface, including governed candidate promotion, without moving governance authority or persistence out of Bot.
- Let a human choose an eligible outcome inside Claude Code or Codex.
- Require a bot-issued, single-use, outcome-bound confirmation challenge before any canonical mutation.
- Preserve bot ownership of packet currentness, actor authorization, transition validation, canonical event construction, append, replay, and projection.
- Define durable semantics for promotion, supersession, and scoped coexistence, plus a typed no-write response when stronger owner authority is required.
- Exclude `dismissed_as_noise` from the default alpha capability set unless Kevin explicitly chooses inclusion and supplies the required cross-lifecycle Signal Policy and retrieval-feedback contract.
- Make response-loss retry idempotent and safe across daemon restart.

## Non-Goals

- MCP-local candidate generation, filtering, ranking, conflict detection, or winner selection.
- MCP-local authority inference, challenge minting/validation, or event construction.
- Automatic supersession, expiry, signoff, policy satisfaction, enforcement, or `superseded_candidate` lifecycle state.
- Treating a RecallPacket, challenge, runtime trace, topology receipt, or model rationale as canonical Decision state.
- Reimplementing Bot policy, lifecycle validation, challenge verification, or canonical persistence inside MCP.
- A packet-local hide button, MCP preference, or unscoped negative RAG weight presented as `dismissed_as_noise`.

## Decision

The former dashboard-only governance-surface rule is **superseded**. MCP now handles governance interaction and candidate promotion as a first-class product surface. “Handles governance” means MCP may present Bot-declared governance actions, collect explicit human intent/confirmation, submit governed commands, render typed results, and perform safe idempotent retry. It does not mean MCP owns governance policy, ranking, lifecycle authority, challenge validity, event construction, or canonical persistence.

This artifact fully specifies the candidate-promotion/conflict flow. Other MCP governance action families must inherit the same surface/authority boundary and receive action-specific lifecycle and conformance coverage; they are not forbidden merely because they are outside this document.

MCP may mediate `recall.promote_decision_candidate` for a bot-known candidate present in the current daemon-authored `RecallPacket`. MCP renders the candidate, conflict context, evidence, and only the actions the bot says are currently eligible. A human selects the action and confirms it. MCP relays the exact selection and confirmation. Bot revalidates the actor, candidate, packet generation, target Decision, selected outcome, scope payload, challenge currentness, and idempotency key before appending an accepted event.

The product outcome dispositions are:

| Outcome | Lifecycle meaning | Required durable effect |
| --- | --- | --- |
| `new_constraint` | Decision Stage 0 candidate acceptance | Append the accepted candidate-review event and materialize a new `Decision`. |
| `supersedes_existing_decision` | Decision Stage 2 revision/correction | Append a new Decision plus an explicit supersession relationship; retain the old Decision and history. |
| `keeps_both_with_scope` | Decision Stage 0 acceptance plus relationship refinement | Append the new Decision plus a structured scope relationship that explains why both remain governing. |
| `needs_signoff_review` | **Excluded from alpha.** An unaccepted candidate cannot enter Decision signoff. | Bot omits the action. Insufficient authority returns `owner_authorization_required` with an inspection/deep-link reference and creates no route or review state. Signoff begins only after an accepted `Decision` exists. |
| `dismissed_as_noise` | **Deferred from the default alpha cut; Kevin owns the inclusion decision.** | Bot omits the action from eligible outcomes and returns typed `unsupported_alpha` if an older client submits it. Inclusion requires the integrated Signal Policy/RAG feedback contract below; packet-local hiding is insufficient. |

MCP must not generate, reorder, add, remove, or reinterpret those actions. It must not infer that one action is correct. The human owns the choice; Bot owns whether the choice is permitted and what accepted event it means; the event store owns the durable result.

Alpha authority matrix, confirmed by the decision owner on 2026-07-17:

| Outcome | Required authority | Result |
| --- | --- | --- |
| `new_constraint` | Candidate-review authority held by the confirmed human actor | Materialize a new accepted but unsigned `Decision`; later signoff remains separate. |
| `supersedes_existing_decision` | Owner-level revision authority | Materialize the new Decision and supersession lineage without rewriting the old Decision. |
| `keeps_both_with_scope` | Owner-level revision authority | Materialize the new Decision and the structured scope relationship governing both Decisions. |

The `ConfirmationChallenge` proves explicit intent for the already-authorized actor. It never grants, upgrades, delegates, or substitutes for lifecycle authority. MCP server identity and agent identity are not human candidate-review or owner authority.

### Removed product-domain lifecycle

The product must not model a separate **Compliance / Compliance Review** lifecycle. That concept is removed rather than deferred. It must not appear as a candidate state, Decision axis, command, capability, dashboard action, MCP tool, integration permission/forbidden-action token, or implied governance conclusion.

The concerns previously bundled under that label return to their actual owners:

- Binding owns whether accepted Decision-to-code evidence is eligible, accepted, rejected, current, stale, or refreshed.
- Decision signoff owns human ownership approval or rejection for an accepted Decision.
- Constraint Discovery and Signal Policy own evidence quality, retrieval relevance, and advisory routing without creating a second Decision status axis.
- Legal, regulatory, security, and SOC 2 work remains organizational assurance material outside the product-domain lifecycle and is not removed by this decision.

This is a coordinated breaking contract change across Bot, MCP, Integrations, and generated Cloud/dashboard artifacts. Capability discovery must omit the removed action entirely; callers must not receive a permanent typed-deferred promise for it. The decision owner selected a **clean incompatible cut** on 2026-07-17: there is no data migration, compatibility decoder, semantic remapping, or legacy action adapter. Old clients fail the protocol/capability boundary explicitly, and stores containing the removed lifecycle fail the declared store-version/open boundary explicitly. They are not opened in a partially interpreted or writable mode. The lifecycle author owns the exact version gates, diagnostic, release sequencing, and rollback procedure—not a translation into replacement semantics.

## Lifecycle Classification

This work touches three layers but changes authority in only one:

1. **Constraint Discovery** supplies the read-only `RecallPacket`, candidate ranking, conflict context, and eligible action projection. It does not resolve the conflict.
2. **Decision Stage 0** owns candidate acceptance and scoped coexistence. Insufficient authority is a typed refusal, not a signoff transition.
3. **Decision Stage 2** owns revision/correction when a new Decision supersedes an existing Decision without rewriting history.
4. **Signal Policy plus Constraint Discovery retrieval** would own any future dismissal-derived relevance feedback. That path may influence later candidate routing/ranking but cannot reject a candidate, rewrite a Decision, or become MCP-local policy.
5. **Binding** owns acceptance/rejection of eligible Decision-to-code binding candidates. MCP is the governance interaction surface; Bot/daemon owns eligibility, currentness validation, `binding.create` or an approved successor, and accepted `BindingEvidence` materialization.

Decision signoff remains the existing post-acceptance lifecycle for a materialized `Decision`. A future asynchronous handoff for an unaccepted candidate would be a separate `ReviewRequest` lifecycle, not signoff.

`mcp_active_work_recall_and_candidate_promotion` is the cross-runtime `UserJourney`. It is not a new canonical lifecycle. `ConfirmationChallenge` is authorization state, not a Decision state. `RecallPacket` is a derived read model. Runtime traces and topology receipts are validation witnesses only.

## State Refinement Check

Release state: one current RecallPacket candidate can be acted on through one bot-authorized, human-confirmed, idempotent transition, and the resulting event can be replayed into the same canonical Decision relationship after restart.

States refined:

- `DecisionCandidate` remains a proposal until Bot accepts a governed transition.
- `Decision` gains no generic MCP state; supersession and coexistence are explicit Decision relationships.
- `NeedsSignoffReview` is not created by this alpha flow. Unauthorized actions return `owner_authorization_required` without a write.
- `ConfirmationChallenge` is short-lived authorization state and is invalidated on daemon restart in the alpha release. A fresh packet and challenge are required after restart.

Transitions are defined by stable ids in the runtime spec: `TRANS-ISSUE-CHALLENGE`, `TRANS-PROMOTE-NEW`, `TRANS-SUPERSEDE`, `TRANS-SCOPE-COEXIST`, and `TRANS-IDEMPOTENT-RETRY`. `TRANS-CREATE-REVIEW-REQUEST` is deferred. `TRANS-RECORD-RELEVANCE-FEEDBACK` is deferred unless Kevin chooses the integrated dismissal path.

Invariants:

- Old accepted Decisions are never rewritten by correction or supersession.
- Candidate review authority remains weaker than product-owner signoff authority.
- `new_constraint` requires confirmed human candidate-review authority; supersession and scoped coexistence require confirmed human owner-level revision authority.
- Surface identity, model output, MCP server identity, and proof presence do not grant authority.
- No canonical append occurs until Bot validates the current packet selection and consumes the confirmation challenge atomically with the accepted action.
- A retry after an accepted append returns the already-applied result and never creates a second Decision or relationship.

Deferred:

- automatic candidate expiry;
- automatic conflict resolution;
- `superseded_candidate` as a separate candidate state;
- `dismissed_as_noise` and durable relevance feedback unless Kevin approves the integrated Signal Policy/RAG path;
- asynchronous `ReviewRequest` creation and candidate delegation queues;
- challenge delegation across devices or actors;

## State Minimization Review

- No `mcp_mediated` canonical state: the originating surface belongs in event metadata.
- No generic `conflict_resolved` state: the accepted outcome and explicit Decision relationship carry the meaning.
- No persistent challenge ledger in the smallest safe release: challenges are daemon-owned, short-lived, single-use, and restart-invalidated. Durable accepted events and idempotency keys handle post-commit recovery.
- No packet-local or terminal `dismissed_candidate` state in the default alpha cut. A local hide creates inconsistent retrieval behavior, while a terminal state conflates relevance feedback with governed rejection.
- No negative RAG weight without a governed feedback contract: relevance feedback must be scoped, attributable, inspectable, reversible, retention-bounded, and evaluated for false-negative/feedback-loop risk before it may influence Bot-owned retrieval.
- No response-only signoff route and no alpha `ReviewRequest`: both create invisible work unless assignee, queue visibility, completion, expiry, notification, and recovery are specified.
- No `superseded_candidate` state: supersession relates the new and old Decisions. Candidate provenance remains inspectable without another lifecycle branch.

## Conformance Boundary

This feature spec owns product scope, lifecycle meaning, and human ownership. The stacked runtime-conformance seed in PR #860 owns the status vocabulary and evidence shape. The composed runtime spec maps this decision to stable `CONF-*` rows with `proven`, `partial`, `missing`, or `unresolved` status; executable evidence remains in `protocol/schemas/v2/fixtures/` and `crates/*/tests/*_conformance.rs`.

Protocol fields, error variants, recovery fixtures, and implementation sequencing belong in those conformance rows and their linked implementation issues—not as parallel acceptance lists here.

## Dismiss-As-Noise Alpha Gate

Delegated decision owner: **Kevin**. Jin’s stated product belief is that the alpha cut should not expose `dismissed_as_noise`. This spec therefore defaults to omission, not packet-local suppression.

Kevin may choose one of two dispositions:

1. **Omit from alpha (recommended default):** Bot does not advertise the action; MCP does not render it; compatibility calls receive typed `unsupported_alpha`; no dismissal state or feedback is persisted.
2. **Include through an integrated feedback path:** amend this spec and its runtime companion before implementation. The action must record a Bot-owned, structured relevance-feedback fact through Signal Policy and route it into the same retrieval/RAG mechanism that surfaces relevant candidates.

The integrated path must define:

- whether the feedback concerns this candidate, this claim/topic, this source, this Product/workspace, or a bounded combination;
- actor, reason code, provenance, retrieval/packet generation, timestamp, retention/decay, and reversal/inspection behavior;
- a Bot-owned persistence location and routing key—never MCP-local preferences or a hidden side database;
- how Signal Policy interprets the feedback and how Constraint Discovery/RAG consumes it as advisory ranking input;
- protection against global blacklisting, silent hard suppression, feedback poisoning, accidental false negatives, and one user’s dismissal becoming everyone’s truth;
- evaluation showing reduced repeated irrelevant surfacing without hiding newly relevant or materially changed candidates;
- a visible explanation/reset path so the feedback does not become invisible, permanent cognitive debt;
- confirmation that relevance feedback does not reject a `DecisionCandidate`, mutate a `Decision`, imply no conflict, or acquire governance authority.

If these persistence, routing, retrieval, recovery, and user-inspection semantics are absent, the action stays out of alpha.

## MCP Governance Surface Alpha Inventory

Confirmed in the alpha cut:

- RecallPacket candidate promotion/conflict outcomes specified by this document.
- Binding candidate acceptance/rejection, linked to [bot#823](https://github.com/BicameralAI/bicameral-bot/issues/823).
- Approve/reject signoff for an already accepted `Decision` through the existing Decision signoff lifecycle.
- Read-only governance inspection: Decision/signoff status, RecallPacket candidate/conflict context, evidence/provenance, history/search, and Bot-declared available actions with their authority requirements.
- Manual `evidence.refresh` for one tracked Decision through Dashboard, MCP, or CLI. The accepted #377/#378 mechanism is a Bot/GraphSnapshotService-owned derived evaluation: surfaces may request and render it but cannot compute freshness, rebuild grounding locally, append a governance event, or mutate accepted `BindingEvidence`, Decision signoff, or other canonical state. The #644/#646 commit watcher may also downgrade recomputable currentness automatically, and preflight reads that daemon-authored state.

Read-only inspection requires authenticated access and scope enforcement but no `ConfirmationChallenge`. It creates no canonical event. Empty, partial, stale, unknown, or unsupported results must remain typed and may not imply policy satisfaction, no conflict, safe-to-change, signoff, or authority.

Explicitly excluded from the alpha cut:

- Generic `review.accept_candidate` and `review.reject_candidate` outside the RecallPacket promotion flow. Bot capability discovery omits them for MCP, and direct compatibility calls return typed `unsupported_alpha` without a candidate or Decision event.
- Tracking mutations, including `tracking.untrack_source` and `tracking.refresh_query`. They are dashboard journeys in alpha, not MCP governance actions. Bot capability discovery omits them for MCP and direct calls write nothing. [bot#861](https://github.com/BicameralAI/bicameral-bot/issues/861), assigned to Jin, owns the RFQ for the dashboard entrypoint, authority/confirmation UX, state visibility, commit point, retry/restart, history, and reversal/retracking journey.
- Standalone contradiction-finding controls such as acknowledge, dismiss, or mark-resolved, including `review.transition_finding`. Contradictions continue to surface through preflight/recall until Bot accepts a substantive governance action that actually resolves the underlying conflict. The accepted action’s existing authority, challenge, event, and replay contract is the only control path.
- `recall.request_correction` as a separate product action or deferred capability. Concrete correction cases use the current RecallPacket promotion outcomes (`new_constraint`, supersession, or scoped coexistence). Cases without an eligible candidate/outcome remain read-only context; alpha creates no correction request, hidden queue, or asynchronous `ReviewRequest` lifecycle.

The exclusion keeps alpha candidate mutation intentional and context-bound. RecallPacket promotion still permits `new_constraint`, supersession, and scoped coexistence under the authority matrix above. Generic candidate rejection cannot be used as a substitute for `dismissed_as_noise`, relevance feedback, or Binding rejection.

The tracking decision is a narrow surface allocation and does not restore the superseded general dashboard-only governance rule. Dashboard presents the tracking journey; Bot continues to own authorization, lifecycle validation, canonical events, persistence, replay, and currentness.

Dashboard-only tracking mutations and multi-surface evidence refresh are intentionally different. Tracking changes whether future maintenance is scheduled for a governed entity. `evidence.refresh` evaluates currentness for an already tracked Decision and returns a derived result without a canonical write. It requires authenticated workspace scope but no `ConfirmationChallenge` because it does not perform a governance mutation.

Contradiction visibility is likewise not a separate user-controlled lifecycle. Viewing, acknowledging, requesting more context, or supplying a note cannot clear an active warning. When a Bot-declared governance action—such as an eligible supersession, scoped-coexistence decision, or another action whose domain effect addresses the affected items—is accepted, Bot validates the report/target linkage and replay removes the contradiction from active preflight. The finding and resolution history remain inspectable. If the accepted action does not resolve the underlying conflict, the contradiction keeps appearing.

MCP signoff does not reverse the earlier no-routing decision. `needs_signoff_review` remains excluded for unaccepted candidates. Once a `Decision` exists, MCP may present `review.approve_signoff` and `review.reject_signoff`, collect explicit owner confirmation, and relay the action. Bot validates owner-level authority and target eligibility, appends the canonical signoff event, and returns the replayable result. Rejecting signoff preserves Decision history and does not delete the Decision.

Binding inclusion is a product-scope decision, not a claim that its exact UX is settled. bot#823 is reopened for the RFQ answer and must decide:

- whether acceptance is a separate action or intentional candidate selection itself;
- the exact user gesture constituting consent;
- rejection behavior, including persistence, resurfacing, inspection, and reversal semantics;
- how rejection remains distinct from `dismissed_as_noise`, RAG relevance feedback, DecisionCandidate rejection, and Decision governance;
- loading, cancellation, stale/partial evidence, authorization denial, daemon failure, idempotent retry, and already-bound behavior.

The included acceptance path must use canonical `binding.create` or an explicitly approved successor. Only an eligible, verified, current candidate may materialize `BindingEvidence`; approximate hints, stale snapshots, partial scope, or unavailable graph evidence remain visibly non-eligible. Rejection must not silently create a global negative retrieval signal or mutate Decision authority.

bot#823 is active for RFQ answering because its prerequisites are near complete: Cloud #137 has cleared code dependencies and awaits its hosted-runner terminal receipt/human review; Bot #788 has implementation and terminal evidence merged and awaits named-owner product acceptance. RFQ acceptance is still required before implementation issue creation.

## Runtime Conformance Handoff

The composed runtime spec is the only detailed live-system handoff. It defines the stable `CONF-*` registry, current status, evidence location, prohibited fallback, and missing witness for each product decision above. Implementation issues must cite those ids rather than copying this feature contract into new acceptance lists.

## User Journey Contract

Journey id: `mcp_active_work_recall_and_candidate_promotion`.

Entrypoint: a user invokes or reaches a staged MCP recall checkpoint in Claude Code or Codex.

Observable success: the host reports the bot-accepted outcome and stable Decision/review reference; replay and a fresh read projection show the same result.

Participating surfaces/runtimes: coding host, MCP adapter, MCP stdio process, Bot daemon/runtime dispatcher, EventStoreAdapter, and replayed read model.

Required terminal evidence: separate Claude Code and Codex journey receipts, the accepted event id/idempotency key, replayed Decision relationship, and a negative receipt proving no MCP-local fallback.

Additional included alpha journey: `mcp_decision_signoff`. Entrypoint is an accepted unsigned Decision rendered in either MCP host; success is one Bot-accepted approve/reject signoff event whose result survives replay and retry. It requires equivalent Claude Code and Codex receipts plus negative evidence for candidate targets and insufficient authority.

Journey Boundary Review status: **accepted by the decision owner on 2026-07-17**. MCP #733 confirms the numbered candidate-promotion boundary, response-loss case, safe idempotent retry, and that runtime traces are witnesses rather than product state. The accepted boundary includes restart-invalidated challenges, no candidate-to-signoff routing, the outcome authority matrix, MCP supersession of the dashboard-only rule, direct MCP signoff for accepted Decisions, omission of generic candidate accept/reject, and removal of the separate correction-request action. Kevin owns the delegated dismissal inclusion decision with omission as the committed default. Terminal Claude Code/Codex receipts and replay/restart evidence remain release obligations, not publication blockers for this contract.

## Trust, Privacy, Security, And Authority Boundaries

- MCP may observe only the local work context the host is authorized to provide and the Bot response.
- MCP may render, collect a selection, display the exact confirmation, and relay messages.
- MCP may not mint confirmation proof, infer authority from the user’s words, or send hidden context as approval.
- Bot may decide candidate eligibility, allowed outcomes, policy satisfaction, target currentness, and accepted event shape.
- EventStoreAdapter is the sole durable Decision materialization path.
- Challenge payloads must bind only ids/hashes needed for the action; secrets, raw source text, model rationale, and full work context are excluded.
- Logs and traces must redact challenge secrets and approval values.

## Per-Surface Behavior

| Surface | Required behavior | Prohibited behavior |
| --- | --- | --- |
| Claude Code adapter | Render Bot order/actions, show bound confirmation, relay one response, retry by idempotency key | Local ranking, inferred approval, direct persistence |
| Codex adapter | Same contract and equivalent terminal evidence | A different outcome vocabulary or authority rule |
| Bot daemon | Validate packet/currentness/actor/outcome/challenge/idempotency; append accepted event | Trust MCP proof presence or caller-selected hidden fallback |
| MCP governance surface | Present Bot-declared governance actions and candidate promotion, collect human intent, submit commands, render results, and retry safely | Own policy/lifecycle semantics, validate itself, or persist canonical state |
| Dashboard | Remain a peer governance surface for dashboard workflows | Claim exclusive governance-surface ownership |

## Lifecycle Ownership

Factory lifecycle ownership separates semantic calls from lifecycle authorship and implementation:

| Role | Person for this work | Owns | Must not decide alone |
| --- | --- | --- | --- |
| Decision owner | Jin | Alpha authority ceiling, signoff-routing semantics, challenge risk/UX trade-off, acceptance of the journey boundary, supersession of the dashboard-only product boundary, and alpha MCP governance action inventory | Storage layout, wire encoding, adapter implementation details that preserve the accepted invariants |
| Delegated dismissal inclusion owner | Kevin | Decide whether `dismissed_as_noise` is omitted from alpha or earns inclusion through the integrated Signal Policy/RAG feedback path; document the evidence and consequences | Ship packet-local hiding, MCP-local persistence, unscoped suppression, or redefine Signal Policy/Constraint Discovery authority alone |
| Lifecycle author | Kevin | Convert Jin’s calls into exact states, transitions, invariants, typed failures, and recovery rules; recommend the smallest safe option with consequences | Product semantics, authority expansion, or release acceptance merely because MCP implements the surface |
| MCP implementation owner | Kevin | Host adapters, command mapping, confirmation UX, retry/idempotency transport, conformance fixtures, Claude/Codex terminal evidence | Bot ranking, policy, challenge validation, accepted event shape, canonical writes |
| Bot implementation owner | Assigned Bot owner | Dispatcher, authorization, challenge service, event/replay semantics, projections, restart tests | Changing the accepted product meaning without the decision owner/lifecycle author handoff |
| Reviewer | Separately assigned | Verify authority, replay, negative paths, and cross-repo conformance | Substitute review for the decision owner’s semantic call |
| Release owner | Separately assigned | Decide whether terminal evidence supports the release claim | Infer product acceptance from merged component PRs |
| PM maintenance owner | Separately assigned | Keep dependencies, labels, and links current | Reclassify unresolved semantics as mechanical work |

### Questions for Jin as decision owner

Resolved on 2026-07-17: keep the `ConfirmationChallenge` term; challenges are short-lived and invalidated on daemon restart, requiring a fresh packet/challenge, while committed results remain recoverable through durable idempotency.

Resolved on 2026-07-17: omit `needs_signoff_review` from alpha MCP outcomes. Insufficient authority returns `owner_authorization_required` without persistence; existing Decision signoff applies only after candidate acceptance.

Resolved on 2026-07-17: confirmed human candidate-review authority may create a new unsigned Decision; supersession and scoped coexistence require confirmed human owner-level revision authority; a challenge never upgrades authority.

Resolved on 2026-07-17: the dashboard-only governance-surface rule is superseded. MCP handles governance interaction and candidate promotion as a first-class surface; Bot retains governance semantics, authority validation, and canonical persistence.

Resolved on 2026-07-17: MCP alpha includes approve/reject signoff for already accepted Decisions. This does not create candidate-to-signoff routing; owner-level authority and Bot-owned canonical persistence remain mandatory.

Resolved on 2026-07-17: generic `review.accept_candidate` and `review.reject_candidate` are excluded from the MCP alpha cut. RecallPacket promotion remains the only alpha MCP candidate-mutation path.

Resolved on 2026-07-17: MCP alpha includes read-only governance inspection. It requires authentication/scope checks but no challenge, writes no event, and preserves typed unknown/stale/partial semantics.

Resolved on 2026-07-17: tracking mutations are dashboard-only in alpha. This is action-family-specific surface allocation, not a reversal of MCP’s first-class governance role. bot#861 is assigned to Jin for dashboard journey scoping.

Resolved on 2026-07-17: contradiction findings have no independent control layer. They remain visible in preflight/recall until resolved by a substantive accepted governance action. Standalone acknowledge, dismiss, and mark-resolved controls are removed from the active product contract.

Previously resolved by #377, Silong’s accepted #378 architecture, Silong’s #644 authority decision, and completed #646/#657 validation: manual evidence refresh is requestable from MCP as well as Dashboard/CLI, while freshness computation and automatic commit-driven downgrades remain daemon-owned, recomputable, and non-governance state.

Resolved on 2026-07-17: remove `recall.request_correction` as a separate action rather than keeping a deferred correction bridge. Current RecallPacket promotion owns eligible concrete corrections; alpha has no separate correction-request lifecycle.

Resolved on 2026-07-17: Jin accepts the recorded candidate-promotion and Decision-signoff Journey Boundary Review. Terminal evidence remains required before an alpha release claim.

### Questions for Kevin as lifecycle author / MCP implementation owner

1. Decide whether alpha omits `dismissed_as_noise` or includes it through the integrated Signal Policy/RAG feedback path. Record Jin’s default belief—omit—as the baseline and explain any override against the gate above.
2. Propose the exact challenge TTL, binding fields, and typed stale/expired/consumed/restarted responses; implement the confirmed restart-invalidation policy consistently in both hosts.
3. Define the structured scope relationship schema and validation rules; free-form text alone is not enough for replayable coexistence semantics.
4. Define the accepted event payloads and replay projections for supersession and scoped coexistence with the Bot owner; define `owner_authorization_required` as a typed no-write result.
5. Define the request id/idempotency key and how Bot returns the original accepted result after response loss and restart.
6. Produce equivalent Claude Code and Codex fixtures for selection, confirmation, refusal, timeout, response loss, and no-fallback behavior.
7. Identify migration behavior for the current unvalidated `ApprovalProof` shape and response-only signoff routing; do not silently preserve them as conforming behavior.
8. Inventory legacy product records and protocol consumers for the removed lifecycle axis; implement the selected clean incompatible cut with exact protocol/store version gates, explicit diagnostics, release sequencing, rollback, and generated-artifact regeneration. Do not build a decoder, migration, or semantic remapping path.

Binding lifecycle author / implementation owner must answer bot#823 before Binding implementation slicing. The action family itself is no longer optional for alpha.

The handoff rule is: Kevin owns the dismissal alpha inclusion call under the gate above and recommends mechanics for the remaining flow; Jin retains Signal Policy, Constraint Discovery, Decision, and authorization authority semantics; Kevin then freezes the lifecycle/runtime contract with the relevant lifecycle owners; implementation owners build to it; reviewer and release owner inspect evidence separately.

## Implementation Handoff

Implementation is sliced from the runtime spec's conformance registry: Bot packet/challenge, Bot event/replay, MCP host adapters, and terminal Claude Code/Codex evidence. bot#823 gates Binding semantics; bot#861 gates the dashboard tracking journey. No duplicated acceptance or test matrix is maintained in this feature proposal.

## ADR Implications

- ADR-0024 and the RecallPacket contract are superseded only where they make dashboard the exclusive governance/conflict surface or retain `recall.request_correction`; their read-only recall and Bot-owned authority rules remain.
- The dashboard governance RFQ spec is superseded only where it claims an exclusive governance surface; tracking mutations remain dashboard-only for alpha under bot#861.
- ADR-0011 and the authenticated governance-surface spec are superseded where they expose standalone contradiction acknowledge/dismiss/resolve transitions.
- The Bot-MCP data-flow runtime architecture is reaffirmed: MCP relays; Bot validates and materializes.
- Signal Policy v0 is unchanged while dismissal is omitted. Inclusion requires an amended Bot-owned relevance-feedback contract.

## Open Questions

No Jin publication blocker remains. Kevin owns the delegated dismissal inclusion decision, with omission as the accepted default, plus the exact challenge TTL, structured scope schema, event versions, typed failures, and incompatible-cut mechanics. bot#823, bot#861, and terminal journey evidence gate their respective implementation or release claims.
