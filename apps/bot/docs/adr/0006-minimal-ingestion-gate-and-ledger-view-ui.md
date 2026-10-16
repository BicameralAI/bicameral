# ADR-0006: Minimal Ingestion Gate and Ledger View UI for Owner/Member-Safe Mods

> Carries forward `bicameral-daemon` ADR-0006, adapted to the public local bot dashboard/review UX.

**Date:** 2026-05-27
**Status:** draft
**Level:** L1
**Related:** ADR-0001, ADR-0002, ADR-0003, ADR-0004, ADR-0005, ADR-0007;
`../specs/dashboard-removal-shred-policy.md`
**UI reference:** historical mockup at `site/src/routes/mockup` (a historical, unpublished mockup)

## Context

Bicameral needs to make it simple and safe for owners/members to configure or vibe-code small domain-specific extensions on top of core architecture. Prior ADRs define the authority boundaries:

- ADR-0001 defines the event store substrate boundary, with git-backed and Google Drive-backed flows replaying into the same domain lifecycle.
- ADR-0002 keeps MCP as a local agent tool surface and routes persistence through bot protocol commands.
- ADR-0003 separates bot-owned local grounding from cloud-owned hosted code graph and conflict infrastructure.
- ADR-0004 requires separate human-in-the-loop boundaries for extraction, grounding, and compliance.
- ADR-0005 separates event store substrate, ingestion connectors, review UX, and governance policy.
- ADR-0007 defines the substrate-neutral governance flow and the boundary between core-owned invariants and customizable integration behavior.

The current UI mockup under `site/src/routes/mockup` (a historical, unpublished mockup) adds an important product constraint: the UI should not expand into many top-level pages. The two surfaces that matter for the spike are:

1. **Ingestion Gate** — represented today by `channels/+page.svelte`.
2. **Ledger View** — represented today by `decisions/+page.svelte`, displays the Decision Ledger and emits review commands.

Other mockup pages (`activity`, `members`, `settings`) may remain supporting surfaces, but the first architecture should nail the two core pages rather than spreading review states across many routes.

## Decision

Adopt a minimal two-page product architecture for the owner/member-safe mods spike:

```text
Ingestion Gate
Ledger View
```

The two pages correspond to two different authority questions:

1. **Ingestion Gate:** "Should this source-derived candidate enter Bicameral governance?"
2. **Ledger View:** "What is the canonical Decision Ledger state, what evidence supports it, and what review action is allowed now?"

Do not introduce separate top-level pages for Approval, Drift, Sources, Dependency Map, or Mod Marketplace in the spike. Those are states or panels inside the two primary pages until usage proves they need their own navigation.

## UI Insights From Mockup

### 1. Ingestion Gate (`mockup/channels/+page.svelte`)

The Channels mockup already behaves like an ingestion gate:

- Sources are grouped by channel/source type: Fathom, Slack, Linear, MCP sessions.
- Each source entry has a detail pane with transcript/thread/ticket/session excerpts.
- Extracted decisions are shown next to source evidence.
- Clicking an extracted decision highlights the exact transcript excerpt.
- Untracked items expose `+ Add decision` and `Ingest →` actions.
- Tracked items become read-only/tracked with an explicit untrack action.
- MCP sessions include member invite/session context, tying agent-discovered decisions to source evidence.

This suggests the Ingestion Gate should own:

- source selection;
- source freshness/connectedness display;
- candidate extraction preview;
- source excerpt linking;
- candidate editing before ingest;
- ingest/untrack decisions;
- candidate-to-review handoff.

It should not own canonical decision mutation. Ingest emits substrate-neutral commands/candidates to bot governance.

### 2. Ledger View (`mockup/decisions/+page.svelte`)

The Decisions mockup is more intricate than a flat table and should drive the Ledger View ADR language:

- Left pane groups decisions by feature.
- Decisions form a hierarchy via parent/child relationships.
- Bound code regions appear inline under decisions.
- Each row carries two independent state axes:
  - signoff state: proposed, approved, rejected, collision_pending, superseded;
  - compliance state: reflected, partial, drifted, pending, ungrounded.
- Detail pane shows source evidence, implementation bindings, drift evidence, and action controls.
- Collision-pending decisions lock approval until the reviewer chooses a resolution:
  - supersede conflicting decision;
  - mark as different scopes;
  - reject this candidate.
- Drift resolution is code-owned: UI copy correctly says a workspace member updates the bound region or supersedes the decision.
- Agent-discovered gaps are visibly marked instead of silently accepted into the Decision Ledger or signoff-approved.

This implies the Ledger View page must model a decision as a composite of:

- ownership authority (`signoff`);
- compliance/grounding state (`compliance`);
- source evidence;
- code bindings;
- conflict relationships;
- allowed review commands.

The ledger page is not merely a list of YAML files. It is the human review surface for canonical state transitions.

## Owner/Member Mod Implication

owner/member-authored mods should target the Ingestion Gate first, not the canonical ledger.

A mod may produce:

- `DecisionCandidate`;
- `DependencySignal`;
- `BindingHint`;
- advisory `GovernanceResult`;
- suggested reviewers;
- source evidence links;
- confidence surfaces.

A mod must not:

- write directly to the selected event store substrate (`.bicameral/decisions/*.yaml`, Drive event files, or future event store internals);
- approve decisions;
- resolve compliance;
- create blocking CI results directly;
- collapse extraction, binding, and compliance into a single confidence score;
- bypass governance policy.

## Ingestion, Creation, and Demotion Rule

The dashboard must not provide a manual "create Decision" path. The Ingestion
Gate shows `DecisionCandidate`s that already exist for a selected `Source` and
`SourceSnapshot`, usually produced by a connector, extractor, MCP session, or
other candidate projection step. A `Source` is the live external object linkage;
the `SourceSnapshot` is the immutable captured view that preserves what
Bicameral saw when candidates were extracted.

The Ingestion Gate's `Ingest` action is a batch promotion action over candidates
for the selected source snapshot. The reviewer may reject or edit candidates
before ingesting. Rejecting a candidate is a durable `reject_candidate` action,
not a local browser discard. Clicking `Ingest` promotes the remaining candidates
into the Decision Ledger through the governed review flow from ADR-0007. A
Decision appears only after that promotion path materializes through the
selected event store substrate.

Source provenance must remain verifiable after the source system changes,
deletes content, or becomes unavailable. The Ingestion Gate therefore must treat
source snapshots as event-store-backed facts and cite evidence with a pointer
into those snapshots. SurrealDB, dashboard stores, and source-specific caches may
materialize those facts for query and display, but they are not authority.
The Ingestion Gate should not behave like a general external-source archive:
snapshots are shown when they support candidate, review, decision, binding, or
governance state.

Promoted Decisions enter the Ledger with `signoff.state = proposed` by default.
`proposed` is the dependency/collision-check staging state: the Decision is now
tracked and comparable against existing Decisions, but it is not approved until
review and known dependency checks resolve. Source-only approval is allowed when
no conflict is known; absence of `BindingEvidence`, graph grounding, or
ungrounded semantic conflict detection is not by itself a reason to keep the
Decision proposed. If checks find a conflict, it transitions to
`collision_pending`. If required non-source checks are unavailable, the UI must
distinguish that honest limitation from a detected conflict.

The Ingestion Gate should not add a separate manual candidate form in the v0.1
dashboard. Manual candidate creation can be introduced later as a source/input
workflow, but it must still produce `DecisionCandidate`s first and must not skip
the Ingestion Gate promotion boundary.

The Ledger View may provide direct demotion actions for existing Decisions, such
as reject, supersede, remove, or equivalent authority-lowering commands. Those
actions are direct in the UI sense: they do not require recreating a candidate.
They still emit substrate-neutral review commands and materialize through
governance and the selected event store substrate.
Removal-like actions are refined in
`../specs/dashboard-removal-shred-policy.md`: ordinary Ledger View actions use
demote/reject, supersede, mark-different-scopes, request-context, or
untrack-source semantics, while destructive/privacy/legal erasure belongs to a
separate Shred workflow rather than an ordinary dashboard delete button.

## Required Data Contracts For The Two Pages

### Ingestion Gate

Minimum contract:

```ts
type IngestionGateItem = {
  source: SourceRef;
  snapshot: SourceSnapshotRef;
  source_title: string;
  source_freshness: 'fresh' | 'stale' | 'offline' | 'unknown';
  evidence: SourceEvidence[];
  candidates: DecisionCandidatePreview[];
  tracked: boolean;
};

type SourceRef = {
  uri: string;
  source_type: 'fathom' | 'slack' | 'linear' | 'mcp' | string;
};

type SourceSnapshotRef = {
  snapshot_addr: string;
  captured_at: string;
};

type DecisionCandidatePreview = {
  id?: string;
  summary: string;
  feature_hint?: string;
  evidence_refs: string[];
  extraction_confidence?: number;
  conflict_hint?: boolean;
  review_state?: ReviewState;
};
```

### Ledger View

Minimum contract:

```ts
type LedgerReviewItem = LedgerDecision | LedgerCandidate;

type LedgerCandidate = {
  id: string;
  summary: string;
  feature_hint?: string;
  sources: SourceEvidence[];
  review_state: 'advisory' | 'needs_signoff_review' | 'collision_pending' | 'stale_source_pending';
  allowed_commands: CandidateCommandKind[];
};

type LedgerDecision = {
  id: string;
  summary: string;
  feature: string;
  parent_id?: string;
  signoff: 'proposed' | 'approved' | 'rejected' | 'collision_pending' | 'superseded';
  compliance: 'reflected' | 'partial' | 'drifted' | 'pending' | 'ungrounded';
  sources: SourceEvidence[];
  regions?: CodeRegion[];
  conflicts_with?: string[];
  discovered?: boolean;
  allowed_commands: DecisionCommandKind[];
};

type GraphReadiness = {
  state: 'ready' | 'indexing' | 'stale' | 'failed' | 'not_configured';
  snapshot_scope?: string;
  reason?: string;
};

type CandidateCommandKind =
  | 'accept_candidate'
  | 'reject_candidate'
  | 'edit_candidate'
  | 'request_context'
  | 'assign_reviewer';

type DecisionCommandKind =
  | 'approve_signoff'
  | 'reject_signoff'
  | 'request_context'
  | 'assign_reviewer'
  | 'bind_to_code'
  | 'resolve_compliance'
  | 'supersede_decision'
  | 'mark_different_scopes';
```

## Governance Flow Relationship

ADR-0007 owns the canonical governance flow. This ADR only defines how the two-page UI participates in that flow:

- Ingestion Gate is the review surface for source evidence and non-canonical candidate intake.
- `Ingest` applies candidate promotion to the remaining candidates for a selected source snapshot. Under the ADR-0007 command model, that means one or more accepted candidate promotion commands, each producing a Decision with `signoff.state = proposed` when materialized. Rejected candidates record durable review history without creating Decisions by default. Separate signoff actions use `approve_signoff` or `reject_signoff`.
- After promotion, dependency/collision checks may transition a proposed Decision to `approved` or `collision_pending`. Source-only approval is allowed when no conflict is known; stubbed ungrounded conflict detection is treated as no known conflict, while unavailable required graph checks are surfaced as grounding limitations rather than collisions.
- Ledger View must run graph readiness for Code Evidence/Compliance reads before showing code compliance, drift, grounded conflict results, or bound code regions. Graph indexing/freshness is displayed at the view level; individual Decisions get secondary grounding status only for evidence-specific problems such as ambiguous or unsupported bindings. This rule does not govern the SMR alpha recall result, which does not make implementation or compliance claims and is available without CodeGraph (ADR-0028).
- Ledger View is the review surface for Decision Ledger state, plus queued candidate review items when a decision context is needed for comparison. Candidate-only commands apply to `LedgerCandidate`; Decision lifecycle commands apply to `LedgerDecision`.
- Both pages emit substrate-neutral commands; neither page writes event-store-specific internals directly.
- Custom source/mod behavior may change extraction, routing, owner lens, and suggested reviewers, but cannot bypass the ADR-0007 authority path.

## Recovery-Oriented Error Display

Ledger View should expose only the state needed to guide the user to the next
valid action. Protocol and graph contracts may keep distinct failure states for
correctness, logging, audit, and routing, but the UI should not mirror every
internal state as a distinct user-facing message.

Different failure states should render differently only when they imply
different recovery paths, required actors, or blocked actions. If two failures
lead to the same user action, the same actor, and the same command availability,
Ledger View should collapse them into the same presentation and preserve the
specific protocol state in inspectable details, telemetry, or logs.

## Minimal Page Count Rule

For the spike, keep top-level navigation minimal:

1. **Ingestion Gate** — source/candidate intake.
2. **Ledger View** — canonical/review state over the Decision Ledger.

Optional supporting controls such as member invites, settings, source configuration, and recent activity should stay as panels, drawers, or secondary routes until required. Do not create separate first-class pages for every state axis.

## Acceptance Criteria

- The spike references `site/src/routes/mockup` (a historical, unpublished mockup) as the UI reference.
- The Ingestion Gate includes source excerpts, extracted candidates, source-to-candidate highlighting, and an ingest action.
- The `Ingest` action promotes the remaining candidate set for a source snapshot; it is not a manual candidate creation form.
- The Ledger View preserves the mockup's hierarchy: feature → decision → child decision → code region.
- The Ledger View exposes both `signoff` and `compliance` as independent state axes.
- The Ledger View collapses protocol failure states that share the same recovery path, required actor, and available actions.
- Collision-pending decisions lock approval until an explicit resolution command is chosen.
- Agent-discovered gaps are visually distinct from approved decisions.
- Manual Decision creation is not available in the dashboard; all Decision creation goes through candidate promotion.
- Ledger demotion actions may be initiated directly from Ledger View, but must emit review commands rather than writing event-store internals.
- Ordinary Ledger View does not expose hard delete; destructive/privacy/legal
  erasure is reserved for Shred as defined in
  `../specs/dashboard-removal-shred-policy.md`.
- Mods can only feed the Ingestion Gate / candidate pipeline; canonical writes happen through governance and event store adapters.
- The spike does not introduce additional top-level product pages beyond Ingestion Gate and Ledger View.

## Rejected Alternatives

- **Separate Approval, Drift, Sources, and Dependency Map pages in v0:** rejected because the mockup shows these can be represented as states and panels inside the two core pages.
- **Let mods write directly to the Decision Ledger:** rejected by ADR-0001, ADR-0005, and ADR-0007; mods produce candidates/signals, not authority.
- **Flat decision table:** rejected because the mockup shows feature grouping, hierarchy, source evidence, implementation bindings, collision resolution, and drift evidence are load-bearing.
- **Single confidence score in UI:** rejected by ADR-0004; extraction, binding, and compliance uncertainty must remain separate.
