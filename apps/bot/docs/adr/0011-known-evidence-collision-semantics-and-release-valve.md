# ADR-0011: Known-Evidence Collision Semantics

**Date:** 2026-05-31
**Updated:** 2026-06-26 - collision is modeled as a durable finding, not a Decision signoff state.
**Status:** proposed
**Level:** L1
**Extends:** ADR-0006 Ledger View, ADR-0007 governance flow, ADR-0010 graph evidence contract
**Related:** `../releases/alpha/desync-ux-hitl-readiness.md`, `../specs/preflight-search-readiness.md`, `../specs/mcp-authenticated-governance-surface.md`

## Problem

The Decision Ledger needs a conflict invariant, but Bicameral cannot honestly
mean "there are no conflicts anywhere." Local installs do not see every branch,
every repository, every future implementation, or every ungrounded semantic
relationship between source-only Decisions.

If the system treats absence of code grounding as a reason to hold every
Decision in `proposed`, the product becomes unusable before graph coverage is
complete. If it treats "no conflict found" as global truth, it overclaims and
hides later-discovered contradictions.

The missing distinction is:

- source-only approval should be allowed now;
- "no conflict" means no conflict was found within known evidence and enabled
  checks;
- later-discovered conflicts must be durably visible without letting lower
  authority inputs nullify approved governance.

## Decision

Source-only Decision approval is allowed by default. It is not a workspace
feature flag and not a paid capability. A Decision may move through normal
signoff review without `BindingEvidence` or verified graph evidence.

The Ledger invariant is:

> An approved Decision had no unresolved conflict known to Bicameral within the
> evidence and checks available at the time of approval.

This is a known-evidence invariant, not a global consistency proof.

Later-discovered conflict evidence creates a durable contradiction finding. It
does not, by default, mutate Decision signoff state.

```text
Decision approval axis:
  proposed | approved | rejected | superseded

Contradiction finding axis:
  open | acknowledged | resolved | dismissed

Preflight / Ledger View / owner inbox:
  compose both axes at render time
```

`collision_pending` is not a canonical Decision signoff state in this model. It
may remain as legacy or display vocabulary for "there is active collision
context," but the durable state is the contradiction finding plus the affected
Decision references.

## Known Evidence Scope

Known evidence may include:

- reviewed `SourceEvidence`;
- durable `RaiseContradiction` events and their `ContradictionReport`s;
- explicit `conflicts_with` metadata derived by replay;
- normalized source/Decision feature or scope hints;
- verified local `BindingEvidence`;
- verified local `GraphSnapshot`/`SymbolOccurrence` evidence from ADR-0010;
- hosted/team advisory conflict results, when the workspace opts into that
  service and policy accepts the advisory as evidence;
- drift or compliance evidence discovered when implementation is later grounded
  to code.

Evidence outside that set is unknown, not implicitly clean.

## Deferred Ungrounded Conflict Detection

Ungrounded conflict detection is a separate future feature. It may compare
source-only Decisions using semantic similarity, feature/scope modeling, or
reviewed domain signals before code grounding exists.

Until that feature exists, absence of ungrounded conflict detection is treated
as no known conflict. It does not keep a Decision in `proposed`, and it does not
weaken later review. It means later contradiction findings must be surfaced
honestly when they are discovered.

## Collision Sets

A collision set is the related item set carried by a contradiction finding.

Current runtime terminology maps this to existing event-store structures:

```text
collision_set_id = ContradictionReport.id
collision set members = ContradictionReport.related_constraint_ids
collision set status = ContradictionReport.status
```

The durable event is `RaiseContradiction`. Replay stores the
`ContradictionReport` and writes `conflicts_with` edges between related catalog
constraints. Read models may use those reports and edges to show affected
Decisions, candidates, and constraints together.

Creating or joining a collision set does not by itself change signoff state:

```text
Decision A(signoff.state = approved)
Decision B(signoff.state = approved)
  -> RaiseContradiction(report: { related_constraint_ids: [A, B] })
  -> A.signoff.state remains approved
  -> B.signoff.state remains approved
  -> preflight and Ledger View show approved Decisions with active collision context
```

If the conflicting item is a `DecisionCandidate` or an unapproved proposed
Decision, it carries collision evidence as review context. Promoting a candidate
or proposed Decision must not nullify governance of a previously approved
Decision. Candidate-promotion authority is not collision-resolution authority.

## Collision Review

The automatic transition only records and surfaces contested context. It must
not choose the winner, supersede any Decision, reject any Decision, change
signoff, or claim compliance resolution.

A Product Owner or explicit delegate resolves the contradiction finding through
governed review commands such as:

- `supersede_decision`;
- `mark_different_scopes`;
- `reject_signoff`;
- request more grounding or context;
- transition the finding to `acknowledged`, `resolved`, or `dismissed` when the
  evidence has been handled.

Those commands still follow ADR-0007:

```text
ReviewCommand
  -> policy accepts/rejects command
  -> accepted domain event materialized by selected event store
  -> replay updates Decision Ledger / contradiction finding read model
```

## Preflight And Recall Behavior

Preflight and recall must surface active collision context anywhere an affected
Decision would otherwise be recalled.

A contested approved Decision remains approved on the signoff axis, but
preflight must label it as approved with active collision context and include:

- the contradiction report id;
- affected Decision/candidate/constraint references;
- evidence refs when available;
- finding status (`open` or `acknowledged` affects active warning behavior);
- allowed next actions for the actor.

Only `open` and `acknowledged` contradiction reports affect active preflight
warning behavior. `resolved` and `dismissed` reports remain audit/history and
must not produce active warnings.

Allowed wording:

- "Approved Decision with active collision context."
- "This Decision remains approved, but contradicting evidence exists in the
  searched scope."
- "Treat as contested context; Product Owner review can supersede, reject,
  mark different scopes, dismiss, resolve, or request more context."

Disallowed wording:

- "No conflict found" when an active contradiction report exists in searched
  scope.
- "Safe to proceed" based only on approved signoff.
- "Approval suspended" unless a future ADR introduces an explicit signoff
  suspension transition.

Preflight remains side-effect free:

```text
preflight_result =
  relevant Decisions
  + active ContradictionReports involving those Decisions
  + searched scope/readiness metadata
```

For a fixed event-store replay state, search index version, graph snapshot, and
preflight request, collision context in preflight must be deterministic and
idempotent.

## Relationship To Grounding

Graph readiness failures are not contradictions. Snapshot mismatch or unknown
graph evidence must not create contradiction findings by itself. It may block a
graph-grounded collision check, but it does not block source-only signoff under
this ADR.

Ledger View and preflight must distinguish:

- workspace or view-level graph readiness (`GraphReadiness`);
- Decision-specific grounding or binding evidence problems
  (`needs_grounding_review`);
- active contradiction findings (`ContradictionReport.status = open |
  acknowledged`).

Unresolved graph grounding must not produce collision context. Alpha UX and
HITL routing for readiness and grounding gaps are defined in
`../releases/alpha/desync-ux-hitl-readiness.md`.

## State Minimization Review

The following states are intentionally not introduced as canonical Decision
lifecycle states:

- `collision_pending`;
- `approval_suspended`;
- `owner_review_requested`;
- `suspension_grade_collision_set`.

They are unnecessary for the current behavior. The same product behavior is
achieved with fewer states by composing:

- Decision signoff state;
- contradiction finding status;
- derived preflight / Ledger View / owner inbox labels.

## Consequences

Positive:

- Bicameral remains useful before code grounding and ungrounded conflict
  detection are complete.
- The product promise stays honest: no known unresolved conflict at approval
  time, not global proof of consistency.
- Non-owners can promote candidates without laundering authority to nullify
  approved Decisions.
- Preflight behavior is idempotent because collision labels are pure
  projections over replayed event-store state.

Negative / risks:

- An approved Decision with active collision context remains approved on the
  signoff axis, so UI copy must make the contested context impossible to miss.
- Implementations must avoid treating `approved` alone as "safe" when active
  contradiction reports exist.
- Existing `collision_pending` enum variants in code need migration guidance:
  they should become legacy/display aliases or be demoted when wire compatibility
  allows.

## Non-Goals

This ADR does not define:

- the ungrounded semantic conflict detection implementation;
- hosted Conflict Oracle internals;
- graph indexing behavior beyond ADR-0010;
- automatic winner selection between conflicting Decisions;
- automatic supersession or rejection;
- an approval-suspension state machine.

## Follow-Up Implementation Work

1. Treat `RaiseContradictionPayload.report.id` as the collision set id in docs,
   fixtures, and read models.
2. Ensure replay remains idempotent by report id and dedupes preflight collision
   context by `ContradictionReport.id`.
3. Update preflight/recall result shapes to include active contradiction report
   context for affected Decisions.
4. Update Ledger View and MCP owner inbox to derive collision rows from active
   contradiction findings, not from Decision signoff mutation.
5. Demote or preserve `collision_pending` enum variants only as compatibility or
   display aliases until a protocol cleanup can remove them.
6. Implement bot-owned async `CollisionReactor` for post-ingest collision
   detection ([#270](https://github.com/BicameralAI/bicameral-bot/issues/270)).
   Detection emits `RaiseContradiction` events; resolution remains human-owned
   through governed review commands.

## References

- [Issue #19](https://github.com/BicameralAI/bicameral-bot/issues/19):
  design-resolution request for HITL behavior around `needs_grounding_review`.
- [Issue #13](https://github.com/BicameralAI/bicameral-bot/issues/13):
  dependency/collision check design that established graph blockers are not
  conflicts.
- [Issue #2](https://github.com/BicameralAI/bicameral-bot/issues/2) and
  [PR #17](https://github.com/BicameralAI/bicameral-bot/pull/17):
  snapshot-scoped graph contract that introduced ADR-0010 evidence states.
- ADR-0006 for Ledger View and Ingestion Gate dashboard boundaries.
- ADR-0007 for substrate-neutral governance state and review commands.
- ADR-0010 for `GraphSnapshot`, `SymbolOccurrence`, `BindingEvidence`, and
  `GraphEvidenceState` semantics.
