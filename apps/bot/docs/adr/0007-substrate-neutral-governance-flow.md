# ADR-0007: Substrate-Neutral Governance Flow

> Carries forward `bicameral-daemon` ADR-0007 as the central bot-owned governance path.

**Date:** 2026-05-27
**Status:** proposed
**Level:** L1
**Related:** ADR-0001, ADR-0002, ADR-0003, ADR-0004, ADR-0005, ADR-0006;
`../specs/configurable-inline-governance-review.md`;
`../specs/dashboard-removal-shred-policy.md`

## Problem

The same governance path was being re-described across the event store, MCP/bot boundary, hosted code graph, HITL, event-store/policy, and UI ADRs. That makes the architecture look more coupled to a specific substrate or UI than it is, and it creates drift risk: each ADR can accidentally define a slightly different route from source evidence to canonical decision state.

Bicameral needs one generic governance flow that works across different integration choices:

- Git-backed workspaces with PR review and CI enforcement.
- Google Drive-backed workspaces with file sync, stale/offline states, and dashboard/agent warnings.
- Dashboard review, Slack modal review, CLI/TUI review, PR review, or batch review.
- owner/member-authored mods that customize extraction/routing without gaining authority over canonical state.
- Connectors for Notion, Slack, Linear, email, documents, MCP sessions, support inboxes, or future sources.

The flow must state which parts are core invariants and which parts are safe extension points.

Public survey evidence supports keeping enforcement capability explicit rather
than assuming every governance result can become a CI check. Stack Overflow's
2024 data shows important decision and coordination work happening in tickets,
docs, chat, meetings, and AI-assisted workflows, while DORA 2024 warns that AI
and platform changes can improve productivity while affecting delivery stability
and throughput. See [Public Developer Survey Implications](../research/public-developer-survey-implications.md).

Implication: `ci_block` is one mapping for git-backed workspaces with a real
merge boundary. Non-git or pre-code decisions need honest mappings such as
`pr_warning`, `dashboard_flag`, `agent_warning`, `slack_notification`,
`queued_action`, `paused_approval`, or no-op.

## Decision

Define a **substrate-neutral governance flow** as the only path from source evidence to canonical decision state. Every connector, mod, review surface, and event store substrate maps into this flow. ADR-0001 defines where accepted events are stored; ADR-0007 defines how a candidate becomes eligible to be stored.

```text
1. Evidence Capture
   source connector, agent session, manual entry, or mod records Source,
   SourceSnapshot, SourceEvidence, and broader EvidenceReferences
        ↓
2. Candidate Projection
   extractor/mod creates DecisionCandidate with cited evidence references;
   future extensions may add BindingHint, DependencySignal, or advisory GovernanceResult
        ↓
3. Policy Evaluation
   governance policy validates schema, provenance, source trust, freshness,
   permissions, decision level, evidence strength, collisions, and substrate capability
        ↓
4. Action Authorization
   bot-owned policy checks the surface category, authenticated actor or adapter,
   target lifecycle state, required capability, approval class, and whether the
   request may become a review command, materialized event, or external effect
        ↓
5. Review Command Gating
   a review surface shows only commands allowed by policy/state/capability
   and emits a substrate-neutral ReviewCommand
        ↓
6. Review Event Resolution
   policy converts accepted ReviewCommand into ReviewEvent / GovernanceResult
   while preserving separate extraction, signoff, grounding, and compliance axes
        ↓
7. Materialization
   event store adapter persists accepted domain events in the selected event store substrate
        ↓
8. Enforcement / Notification / Egress
   adapter translates GovernanceResult into an honest substrate-specific effect:
   V1 uses advisory surfacing; future extensions may add CI block, PR warning,
   dashboard flag, agent warning, Slack notification, queued action,
   paused approval, governed egress projection, or no-op
```

Canonical decision state changes only after:

1. governance policy accepts the command;
2. the selected event store adapter successfully materializes the accepted domain event; and
3. replay from the selected event store substrate produces the new state.

UI surfaces, Slack actions, CLI commands, connectors, and mods are command/evidence producers when their source or action path is explicitly configured. They are not canonical authorities. Git or GitHub may be the selected event-store substrate without making GitHub pull requests, issues, comments, checks, or commits source-ingest evidence by default.

Source-adapter ingest and authenticated remote review actions are different
producers. Source ingest may provide evidence, hints, and candidates, but it
cannot carry actor authority or review commands. Authenticated remote review
actions may submit governed commands only after callback verification, actor
binding, capability checks, target-state checks, and policy acceptance.

## Core-Owned Invariants

These parts are not customizable by individual integrations or mods.

### 1. Domain vocabulary

The core domain vocabulary is shared across substrates:

- `SourceEvidence`
- `Source`
- `SourceSnapshot`
- `EvidenceReference`
- `DecisionCandidate`
- `Decision`
- `Tracked Decision`
- `ReviewCommand`
- `ReviewEvent`
- `ReviewState`
- `BindingHint`
- `BindingEvidence`
- `DependencySignal`
- `ComplianceVerdict`
- `GovernanceResult`

Connectors and mods may add metadata, but they must map back to these objects before entering policy evaluation.

### 2. Authority separation

- Extraction can propose a `DecisionCandidate`; it cannot approve decision meaning.
- Decision creation is never a manual operation. Frontend, CLI, MCP, connector,
  and mod surfaces can create or edit candidates; a `Decision` can only appear
  in the Decision Ledger through the governed candidate promotion path.
- Owner/member review owns signoff according to workspace governance policy.
- V1 signoff answers whether the reviewed claim should become shared team
  memory; it does not answer whether implementation is correct, complete, or
  compliant.
- Future enforcement policies may require signoff before warnings, gates, or
  blocking effects become eligible, but signoff remains an ownership input to
  enforcement rather than an enforcement result.
- Code graph/grounding owns verified graph facts and binding validation; accepted `BindingEvidence` is a future-extension governance fact materialized through the selected event store substrate.
- Compliance/drift analysis owns `ComplianceVerdict` as a future extension; weak or unreviewed verdicts remain advisory.
- These Code Evidence/Compliance contracts do not close or gate SMR alpha recall. Under ADR-0028, SMR recalls product-approved specifications from a controlled corpus; CodeGraph is optional recall-expansion provenance only.
- Event store adapters own durable materialization.
- No connector, mod, review surface, or MCP tool directly mutates event store internals.
- Projection profiles and target adapters describe or execute external target
  mechanics; they cannot grant permission, waive approval, or make external
  target state canonical Bicameral truth.

### 3. Independent state axes

The system must not collapse these into one confidence score:

- extraction confidence / candidate quality;
- signoff state;
- grounding/binding confidence;
- compliance/drift state;
- source freshness/provenance;
- substrate enforcement capability.

A strong value on one axis cannot compensate for weakness on another. For example, high extraction confidence does not make a weak code binding safe to block on.

### 4. Replayability

Every canonical state must be rebuildable from the selected event store substrate. SurrealDB, dashboard stores, search indexes, and hosted code graph state are caches/materializations, not authority.

Source provenance must remain verifiable after the source system changes or
disappears. A `Source` records the mutable external object linkage. A
`SourceSnapshot` records the immutable captured view of that object, identified
by a UOR-compatible content address over the canonical snapshot representation.
`SourceEvidence` cites one pointer into a source snapshot. `EvidenceReference`
is the broader V1 citation abstraction: it can point at source excerpts, agent
session turns, tool calls, command output, file ranges, diff hunks, or code
snapshot regions. Candidates, decisions, bindings, and governance results can
therefore be audited without relying on the live source system still returning
the same content. Candidates, decisions, and governance results may cite
multiple evidence references when a claim depends on multiple source regions or
multiple sources.

`SourceSnapshot` is not an archival mirror of every external source update.
Bicameral records a new snapshot when the captured source state affects
Bicameral internal state: candidate projection, candidate invalidation, review
evidence, decision provenance, binding/governance evidence, freshness-sensitive
governance, or collision/dependency analysis. If an external source changes but
Bicameral does not use that change for governance, review, or materialized
state, the change does not need to become a snapshot event.

Source refresh is source-kind-specific. Non-code sources may refresh through
polling, webhooks, API reads, or manual recapture. Code sources use the
GraphStore refresh and revalidation machinery from ADR-0010 and ADR-0021:
resolve a code snapshot, build or refresh a `GraphSnapshot`, use `GraphSlice`
for scoped evidence, retain `BindingAnchor` metadata for accepted bindings, and
route durable currentness decisions through `Binding Revalidation` only when
governance needs shared authority.

Automatic source refresh is scoped to tracked Decisions by default. Candidate
promotion marks a Decision as tracked unless workspace governance policy
explicitly creates a different lifecycle state. This tracking axis is separate
from signoff: a `proposed`, `approved`, or `collision_pending` Decision may all
remain tracked, while `reject_signoff`, `supersede_decision`, `untrack_source`,
or future demotion commands may stop automatic refresh according to policy.
Rejected candidates, raw SourceSnapshots, MCP session context, command output,
and other unpromoted evidence may remain stale without creating a governance
failure. A user, connector, or policy may still request manual refresh or
re-extraction for untracked context, but the automatic freshness queue should
not try to maintain every observed Source.

For the git-backed event store, retained source snapshot blobs should live under
`.bicameral/sources/` and be referenced by accepted events. This source snapshot
storage is canonical because it preserves cited evidence. Code graph indexes do
not follow the same pattern: graph indexes are daemon caches, while accepted
`BindingEvidence` records store only reviewed graph snapshot identity and
evidence metadata. A verified binding does not imply code compliance,
implementation reflection, feature readiness, or enforcement.

### 5. Allowed-command gating

Review surfaces can only show or emit commands allowed by current governance state, owner/member capability or reviewer assignment, source freshness, evidence strength, and substrate capability.

### 6. Honest enforcement

A `GovernanceResult` must not claim stronger enforcement than the selected substrate can provide. Git can block merges through CI. Google Drive cannot; it can warn, pause, queue, or require review.

### 7. No opaque consequential automation

The system must not perform opaque consequential automation over shared team
state. Consequential transitions such as `approve_signoff`, `resolve_compliance`
to blocking, `supersede_decision`, destructive source removal, or governance
policy changes require explicit accountable review unless workspace policy has
defined a narrow low-risk automation class. That policy decision must itself be
reviewable and replayable.

## Customizable Parts

These parts are intentionally configurable by workspace, source integration, event store substrate, or owner/member-authored mod.

| Flow stage | Customizable | Not customizable |
|---|---|---|
| Evidence Capture | source connector, polling vs webhook, source filters, redaction/pointers, snapshot representation, manual refresh for untracked context | source URI, snapshot identity, and evidence pointers must be recorded; secrets must not be persisted as canonical content |
| Candidate Projection | extraction prompts/rules, labels, feature hints, owner lens, suggested reviewers, domain metadata | outputs remain candidates/hints/signals until policy accepts them |
| Policy Evaluation | source trust, automation mode, required reviewer by level/source, low-risk auto-candidate thresholds | policy cannot skip replayability, authority separation, or substrate capability checks |
| Review Surface | dashboard, Slack, CLI/TUI, PR comment, Drive batch UI, copy/presentation | surfaces emit shared `ReviewCommand`s; they do not invent authority semantics |
| Materialization | git YAML/commits, Drive YAML/event files, future adapter layouts | replayed domain state must match the shared lifecycle |
| Enforcement / Notification | CI block, dashboard flag, agent warning, Slack notification, queued local action, paused approval | enforcement must accurately reflect substrate capabilities |

## Standard Review Commands

Initial command vocabulary:

- `accept_candidate`
- `approve_signoff`
- `reject_signoff`
- `reject_candidate`
- `edit_candidate`
- `request_context`
- `assign_reviewer`
- `bind_to_code`
- `resolve_compliance`
- `supersede_decision`
- `mark_different_scopes`
- `untrack_source`
- `pause_approval`
- `resume_approval`

`accept_candidate` is the v0.1 Promote command: it promotes a valid
`DecisionCandidate` into a Decision Ledger record with `signoff.state =
proposed`. Future protocol revisions may rename this command to
`promote_candidate`, but they must preserve the invariant that all Decision
creation goes through candidate promotion.

Ingestion Gate `Ingest` is a UI-level batch action over an already-projected
candidate set for a source snapshot. It is not candidate creation. It emits one or
more candidate promotion commands for the remaining candidates after the reviewer
has rejected or edited unwanted candidates. Candidate rejection before ingest is
a durable `reject_candidate` transition, not a local UI discard. Replay must
preserve each candidate rejection and promotion as its own lifecycle transition.

A promoted Decision enters `signoff.state = proposed` by default. `proposed` is
the post-ingest dependency/collision-check staging state: the Decision is now in
the Ledger and can be compared against existing Decisions, but it is not approved
until review and known dependency checks resolve. Source-only approval is
allowed when no conflict is known; absence of `BindingEvidence`, graph
grounding, or ungrounded semantic conflict detection is not by itself a reason
to keep the Decision proposed. If checks find a conflict, it transitions to
`collision_pending`. If required non-source checks are unavailable, governance
must surface the limitation honestly without treating it as a detected conflict.

Demotion is a Decision lifecycle operation, not candidate intake. A frontend
Ledger View may directly initiate demotion commands for existing Decisions, such
as `reject_signoff`, `supersede_decision`, or a future `demote_decision` /
`remove_decision` command. Direct frontend initiation does not mean direct
storage mutation: the command must still pass through governance policy,
materialize through the selected event store substrate, and replay into Ledger
state.

Commands are substrate-neutral. `accept_candidate` means the extracted candidate is valid enough to become a Decision Ledger record with `signoff.state = proposed`; it does not approve signoff. `reject_candidate` means the extracted claim is not a valid Decision and records review/audit history without creating a Decision by default. `approve_signoff` means the workspace policy permits the actor/action to move ownership signoff to `approved`. `reject_signoff` means an existing proposed Decision is explicitly rejected and remains inspectable in the Decision Ledger with `signoff.state = rejected`. `resolve_compliance` means grounding, binding, or drift evidence has been reviewed enough to resolve the compliance state. The event store adapter decides whether an accepted command becomes a git commit, Drive file write, queued local event awaiting durable sync, or another substrate-specific artifact. Canonical lifecycle changes must still replay from the selected event store substrate; dashboard-only annotations remain non-canonical.

Adding new commands requires updating this ADR or a successor ADR. Mods may not mint private commands that bypass governance policy. A review surface may submit a batch of domain commands for one UX action, but replay must preserve each command as a separate lifecycle transition in `applied_commands`.

Tool calls may surface inline or buffered governance questions as a Review
Surface UX pattern, but the accepted command path remains unchanged. See
`../specs/configurable-inline-governance-review.md` for policy modes such as
inline ask, recommendation with confirmation, route-to-surface, forced human
review, and narrow low-risk auto-resolution.

Removal-like dashboard actions must also use this command path. Ordinary
demotion, supersession, and source-untracking are review commands; destructive
privacy/legal erasure is reserved for the Shred boundary in
`../specs/dashboard-removal-shred-policy.md` and must not be implemented as a
private review-surface command.

## Standard Governance Result

`GovernanceResult` must include at least:

```ts
type GovernanceResult = {
  severity: 'blocking' | 'warning' | 'info';
  review_state:
    | 'advisory'
    | 'proposed'
    | 'needs_signoff_review'
    | 'needs_grounding_review'
    | 'needs_compliance_review'
    | 'collision_pending'
    | 'stale_source_pending'
    | 'ready_to_materialize'
    | 'materialized';
  evidence_strength: 'weak' | 'reviewed' | 'resolved';
  enforcement_capability:
    | 'ci_block'
    | 'pr_warning'
    | 'dashboard_flag'
    | 'agent_warning'
    | 'slack_notification'
    | 'queued_action'
    | 'paused_approval'
    | 'none';
  source_refs: string[];
  allowed_commands: ReviewCommandKind[];
  applied_commands?: ReviewCommandKind[];
  reason: string;
};
```

## Review State Transitions

Command processing and domain lifecycle are separate. Every accepted command follows the same processing path:

```text
ReviewCommand
  → policy accepts/rejects command
  → accepted domain event is materialized by the selected event store substrate
  → replay updates Decision Ledger / review state / signoff state / compliance state
```

Candidate intake has two outcomes:

```text
Source + SourceSnapshot + SourceEvidence
  → DecisionCandidate
  → reject_candidate
  → rejected candidate review event only; no Decision Ledger record by default

Source + SourceSnapshot + SourceEvidence
  → DecisionCandidate
  → Ingestion Gate Ingest
  → accept_candidate / Promote per remaining candidate
  → Decision Ledger record with signoff.state = proposed
  → dependency/collision check
  → signoff.state = approved | collision_pending | proposed
```

There is no parallel transition from manual UI/CLI input directly to
`Decision`. Manual input enters as `SourceEvidence` and/or a
`DecisionCandidate`, then follows the same promotion path as connector,
integration, MCP, or mod output.

Once a Decision exists, signoff and compliance advance independently:

```text
Decision(signoff.state = proposed)
  → approve_signoff | reject_signoff | supersede_decision | collision_pending | stale_source_pending
  → signoff.state = approved | rejected | superseded | proposed/pending-review

Decision(signoff.state = approved)
  → later-discovered conflict evidence
  → signoff.state = proposed + review_state = collision_pending
  → reviewer resolves by superseding, rejecting, marking different scopes, or requesting more context

Decision + BindingEvidence/ComplianceEvidence
  → bind_to_code | resolve_compliance | request_context
  → ComplianceVerdict = reflected | partial | drifted | pending | ungrounded
```

A substrate may add intermediate operational states, such as `branch_pending`, `drive_sync_pending`, or `queued_local_write`, but those states must replay back into the shared lifecycle above.

## Integration Examples

### Git-backed workspace

1. MCP session, manual entry, or an explicitly approved SourceBinding emits `SourceEvidence`.
2. Extractor creates a `DecisionCandidate` with source refs and level hint.
3. Policy checks source trust, branch context, collisions, and reviewer requirements.
4. Review happens through dashboard, Slack, CLI, or another approved review surface; the surface emits `accept_candidate`, `reject_candidate`, `approve_signoff`, `reject_signoff`, `edit_candidate`, `mark_different_scopes`, etc.
5. Git adapter materializes accepted candidates as proposed Decisions in `.bicameral/` YAML changes and/or review commits.
6. Review-resolved blocking results can become CI failures because git has a merge boundary.
7. Replay from git reconstructs canonical decisions and review state.

GitHub pull requests, issues, comments, checks, and commits are not source evidence merely because GitHub or git is the event-store substrate. Treat them as source ingest only after a separate owner-approved SourceBinding says exactly which GitHub resources may enter the trusted corpus.

### Google Drive-backed workspace

1. Meeting transcript, Drive doc, Slack thread, or manual entry emits `SourceEvidence`.
2. Extractor creates a `DecisionCandidate` with source refs and freshness metadata.
3. Policy checks source trust, Drive auth/sync freshness, duplicate candidates, and reviewer requirements.
4. Review happens through dashboard, Slack, CLI/TUI, or Drive-aware batch review; review surfaces emit the same `accept_candidate`, `reject_candidate`, `approve_signoff`, and `reject_signoff` commands.
5. If Drive is stale/offline, policy may allow local draft review but emits `stale_source_pending` / `paused_approval` until shared state is fresh.
6. Drive adapter materializes accepted candidates as proposed Decisions in YAML/event files in the configured Drive folder.
7. Blocking intent becomes dashboard flags, agent warnings, Slack notifications, queued actions, or paused approval; it does not become CI failure.
8. Replay from Drive files reconstructs canonical decisions and review state.

### owner/member-authored mod

1. Mod reads an allowed source and emits candidate metadata, reviewer suggestions, and dependency/risk signals.
2. Policy treats those outputs as hints, not authority.
3. Review surface may display the mod's suggestions and route to the right reviewer.
4. Canonical materialization still requires accepted review commands and event store adapter replay.

## Consequences

Positive:

- Removes repeated governance-flow prose from the other ADRs.
- Lets different integration options share one authority model.
- Makes customization explicit without compromising canonical state.
- Allows git and Google Drive flows to differ in materialization/enforcement while replaying the same domain lifecycle.
- Gives owners/members a safe extension surface: extraction, routing, metadata, presentation, and advisory signals.

Tradeoffs:

- Every connector/mod/review surface must map to the shared command/result vocabulary.
- Event store adapters need clear capability declarations so UI and agents do not over-promise enforcement.
- New review commands require ADR updates instead of ad hoc per-integration behavior.
- Generic lifecycle states may feel more abstract than the concrete git/PR path, but that abstraction prevents Git/CI from becoming the product ontology.

## Rejected Alternatives

- **One governance flow per substrate:** rejected because it would duplicate authority semantics and make cross-substrate behavior unpredictable.
- **Let each review surface define its own commands:** rejected because Slack, dashboard, CLI, and PR review would drift.
- **Let mods customize canonical writes:** rejected because owner/member-safe mods must stay expressive at the edge and deterministic at the core.
- **Treat CI as the universal enforcement model:** rejected because Google Drive and other non-git substrates do not have a merge boundary.
- **Make the Ingestion Gate / Ledger View the flow itself:** rejected because they are the v0 dashboard surfaces from ADR-0006, not the generic governance model. Other review surfaces must enter the same flow.
