# Codegraph Ledger UI And Protocol Boundary

**Date:** 2026-06-10
**Status:** proposed
**Level:** L1 repo architecture
**Owns:** The V1 mapping from codegraph evidence lifecycle to Decision Ledger visibility, Ledger View UI placement, and RFQ gaps for protocol/LLM follow-up.
**Related context:** #210, #189, #207, #206, #209, #208, #185, #183, #194, #84; ADR-0007, ADR-0010, ADR-0021, ADR-0022; `graphslice-planning-sufficiency.md`

## Reading Guide

Read `Decision Ledger Boundary`, `Ledger View Contract`, and `RFQ Gaps` first.
The invariant is:

```text
codegraph provides evidence/readiness/advisory state;
the Decision Ledger remains the selected Decision list;
only accepted governance events replay into ledger state.
```

Some linked issue text says "cograph." This spec uses the canonical product
term `codegraph`.

## Problem

The codegraph redesign introduced lifecycle entities such as `GraphSnapshot`,
`GraphSlice`, `ScopeSufficiencyProof`, and graph evidence states. At the same
time, V1 narrowed Bicameral away from compliance/enforcement and toward
Decision signoff plus reviewable context.

Without one contract, implementation can drift in three ways:

- treating codegraph cache/readiness state as Decision Ledger authority;
- rendering graph failures as signoff, compliance, or governance failures;
- answering MCP/bot protocol and LLM ownership questions before the owning RFQs
  are resolved.

## Goals

- Map each codegraph lifecycle concept to its ledger-facing disposition.
- Define where code evidence appears in Ledger View.
- Define how graph evidence failure states render in the selected-Decision UI.
- Keep V1 compliance and enforcement out of the UI contract.
- Mark unresolved protocol and LLM ownership questions as RFQ gaps with owning
  issues.
- Give #189 enough UI/API direction to resume without waiting for #208/#209.

## Non-Goals

- Do not implement dashboard changes here.
- Do not decide MCP/bot information flow for every lifecycle phase; #209 owns
  that.
- Do not decide which component runs which LLM call; #208 owns that.
- Do not add hosted/team graph scope.
- Do not make compliance, delivery readiness, or merge blocking a V1 claim.
- Do not rename existing code symbols solely for terminology cleanup.

## Decision Ledger Boundary

For V1, the Decision Ledger is the replayed list of selected Decisions and
accepted governance state. Codegraph state can be ledger-visible as evidence or
advisory view state, but it is not a ledger change unless policy/review accepts
a governance command and an event replays.

```text
codegraph observation -> advisory evidence or view state
accepted command      -> event-store event
event replay          -> Decision Ledger state
```

`GraphStore`, graph caches, graph slices, readiness states, and sufficiency
proofs do not write canonical state by existing in memory, cache, or UI.

## 1:1 Codegraph To Ledger Mapping

| Codegraph concept | Ledger-facing disposition | Ledger change? | V1 UI handling |
|---|---|---:|---|
| `TaskIntent` | Request purpose for graph evidence | No | Hidden or shown only in debug/audit detail. |
| `SnapshotResolution` success | `GraphSnapshot` identity available for evidence | No | Show snapshot identity when relevant to code evidence. |
| `SnapshotResolution` failed/stale | Readiness state | No | Render in code evidence status div; do not mutate Decision. |
| `GraphSnapshot` | Evidence snapshot identity | No | Show as evidence provenance when cited. |
| `GraphStore` cache entry | Graph-internal cache | No | Never show as ledger entity. |
| `GraphSlice` | Scoped evidence container | No | Show only as "scope inspected" summary if useful. |
| `GraphScopePlan` | Advisory scope declaration | No | Debug/audit detail, not a Decision row. |
| `ScopeSufficiencyProof.sufficient` | Eligible proof for scoped graph claims | No by itself | Allows verified/absent labels for that scoped task only. |
| `ScopeSufficiencyProof.insufficient` | Readiness/review-needed state | No | Render as insufficient scope; no clean pass. |
| `GraphEvidenceState::verified` | Verified graph evidence candidate | No by itself | Show as grounded/verified evidence; accepted binding still requires governance. |
| `GraphEvidenceState::not_found` | Verified negative evidence in inspected scope | No by itself | Show missing at inspected snapshot; reviewer decides follow-up. |
| `GraphEvidenceState::unknown_not_indexed` | Indexing/readiness gap | No | Show not indexed; allow refresh/index action where available. |
| `GraphEvidenceState::unknown_stale` | Stale graph/readiness gap | No | Show stale graph; offer rebuild/revalidate where available. |
| `GraphEvidenceState::ambiguous` | Review-needed grounding state | No | Show ambiguous candidates; reviewer must choose or refine. |
| `GraphEvidenceState::unsupported` | Capability limit | No | Show unsupported; allow source-only or hosted/manual follow-up. |
| `GraphEvidenceState::approximate_candidate` | Search/LLM/retrieval hint | No | Show suggested only; exact validation required before verified evidence. |
| `BindingProposal` | Proposed decision-to-code evidence | No | Show as proposal/hint until accepted. |
| Accepted `bind_to_code` | `BindToCode` event and `BindingEvidence` | Yes | Show accepted binding evidence under selected Decision. |
| `BindingAnchor` | Retained metadata for accepted binding inspection | No new change | Use to explain evidence provenance; do not expose as authority. |
| Derived `BindingLineage` | Advisory lineage/currentness | No by default | Show as advisory unless backed by accepted revalidation. |
| Accepted revalidation | `RevalidateBinding` event and `RevalidationOutcome` | Yes | Show reviewed revalidation history; do not rewrite original binding. |
| Diff preflight graph result | Advisory preflight output | No | Warnings only; no Decision mutation. |
| Local "no conflict" graph result | Known-evidence advisory result | No | Show scoped clean result only; never global consistency proof. |
| Hosted/team graph result | External advisory evidence | No by default | Must enter same governance path before becoming durable. |
| Remediation candidate | Suggested repair | No | Show action suggestion; accepted repair needs governed event. |
| Compliance read from graph evidence | Advisory/future compliance view | No by default | Hidden or future/advisory in V1. |

## Ledger View Contract

Ledger View remains Decision-first.

```text
Ledger View
  Decision list
    selected Decision
      Summary
      Signoff / review state
      Source evidence
      Code evidence
      Freshness / refresh
      Event history
```

Code evidence appears as a section under the selected Decision, not as a
top-level ledger entity and not as a replacement for source evidence.

### Selected Decision Code Evidence Section

The code evidence section should render:

- accepted `BindingEvidence`, when replayed from the event store;
- advisory `BindingProposal` or code hints, clearly labeled as not accepted;
- graph snapshot provenance, including `graph_snapshot_id`, commit/SHA, and
  stale/currentness status when available;
- graph evidence state and scope sufficiency status;
- reviewer actions that are allowed by policy, such as refine binding,
  revalidate, source-only review, request hosted check, or leave advisory.

### Failure State Div

The selected Decision view should reserve a stable status div for graph
readiness/evidence failures. The div renders codegraph states as evidence
status, not Decision status.

| State | User-facing label | Copy posture | Allowed V1 action |
|---|---|---|---|
| `verified` | Grounded at snapshot | This evidence was verified for the named snapshot/scope. | View provenance; accept binding only through governance. |
| `not_found` | Missing at inspected snapshot | The inspected snapshot did not contain the requested target. | Refine target, revalidate, mark review-needed, or keep source-only. |
| `unknown_not_indexed` | Not indexed | Bicameral has not indexed the required graph scope. | Build/expand index or continue source-only. |
| `unknown_stale` | Stale graph | Cached graph data does not match the requested snapshot/version. | Rebuild/revalidate or continue source-only. |
| `ambiguous` | Needs grounding review | Multiple possible targets match. | Reviewer chooses/refines target. |
| `unsupported` | Unsupported evidence | Exact local validation is unavailable for this target/scope. | Use source-only, hosted/team check, or manual review. |
| `approximate_candidate` | Suggested only | Retrieval/LLM/search found a possible target, not verified evidence. | Run exact validation or reject suggestion. |

The div must not say "noncompliant", "failed signoff", "unsafe to merge", or
"implemented incorrectly" unless a future reviewed compliance/enforcement event
exists and the UI is explicitly rendering that future state.

## Ledger View Wireframe

This wireframe is not final visual design. It records the intended information
hierarchy for #189: the Ledger remains a Decision list, and code evidence
appears only inside the selected Decision detail.

```text
+--------------------------------------------------------------------------------+
| Ledger View                                                     Search [_____]  |
+------------------------------+-------------------------------------------------+
| Decisions                    | Decision: Use local daemon for governance        |
|                              | Signoff: Proposed          Freshness: Tracked   |
| > Use local daemon ...       +-------------------------------------------------+
|   tracked                    | Summary                                         |
|   stale source               |   Reviewed claim text and source summary.       |
|                              |                                                 |
|   Add healthz endpoint       | Source Evidence                                 |
|   tracked                    |   - Jira PROJ-12 snapshot sha256:...            |
|                              |   - MCP session turn #18                        |
|   Retire old auth flow       |                                                 |
|   untracked                  | Code Evidence                                   |
|                              |   Status: Grounded at snapshot                  |
|                              |   Target: crates/runtime/src/replay.rs:201-214  |
|                              |   Snapshot: gs1:abc... @ e935d6b                |
|                              |   Scope: binding_revalidation sufficient        |
|                              |                                                 |
|                              | Codegraph Readiness                             |
|                              |   verified / stale / ambiguous / unsupported    |
|                              |                                                 |
|                              | Actions                                         |
|                              |   [Revalidate] [Refine binding] [Source-only]   |
+------------------------------+-------------------------------------------------+
```

### Granular Status Modules

Use these modules inside the selected Decision's Code Evidence section. They
are communication patterns for failure state, readiness level, and user action,
not alternate full-page layouts.

```text
+------------------------------------------------------------------------------+
| Stale graph                                                                   |
| Cached graph data does not match the requested snapshot/version. This is      |
| evidence readiness, not signoff or compliance failure.                        |
|                                                                              |
| Readiness: unknown_stale                                                      |
| Snapshot:  gs1:abc... @ e935d6b                                               |
| Scope:     binding_revalidation                                               |
|                                                                              |
| [Rebuild graph] [Revalidate accepted binding] [Continue source-only]          |
+------------------------------------------------------------------------------+
```

```text
+------------------------------------------------------------------------------+
| Needs grounding review                                                        |
| Multiple possible targets matched this Decision's binding hint. Bicameral has |
| not selected one as verified evidence.                                        |
|                                                                              |
| Candidates                                                                    |
|   1. crates/runtime/src/replay.rs:201-214       exact symbol match            |
|   2. crates/api/src/event_store.rs:330-353      payload/event match           |
|                                                                              |
| [Choose target] [Refine hint] [Keep source-only] [Request hosted check]       |
+------------------------------------------------------------------------------+
```

```text
+------------------------------------------------------------------------------+
| Suggested only                                                                |
| Retrieval or LLM output found a possible code target, but exact validation    |
| has not accepted it as graph evidence.                                        |
|                                                                              |
| Suggestion: crates/runtime/src/refresh.rs:42-70                               |
| State:      approximate_candidate                                             |
|                                                                              |
| [Validate exactly] [Dismiss suggestion] [Keep as advisory context]            |
+------------------------------------------------------------------------------+
```

## Ingestion Gate Contract

Ingestion Gate may show source evidence, candidate claims, and code context
captured from sessions, PRs, diffs, commands, or tool calls. It must preserve
the same distinction:

- raw source/candidate/code context is advisory;
- candidate promotion creates a Decision;
- signoff reviews Decision meaning;
- code evidence can support review but does not create compliance.

Approximate code hints in Ingestion Gate should use "suggested" language until
exact validation or reviewer acceptance changes their status through a governed
path.

## Protocol Boundary

This spec does not decide the full MCP/bot protocol contract. It sets the
constraints #209 must obey:

- MCP is a local agent surface and hint producer, not ledger authority.
- Bot/daemon owns governance policy, event-store materialization, replayed
  Decision Ledger state, and exact local validation boundaries.
- ToolRequest remains the canonical local request object.
- Caller-provided code regions, rationale, and LLM outputs are hints unless the
  bot validates or governance accepts them.

## LLM Boundary

This spec does not decide which LLM call lives in MCP versus bot. It sets the
constraints #208 must obey:

- Any MCP-side LLM output is advisory unless passed through bot-owned
  validation/governance.
- An LLM must not emit `GraphEvidenceState::verified`.
- LLM-assisted binding fallback may propose targets or explanations, but
  durable binding/compliance facts require reviewer or policy acceptance with
  provenance.
- UI must distinguish graph-verified evidence from reviewer-accepted manual or
  LLM-assisted evidence.

## Existing Infrastructure Inventory

| Artifact | Role in this spec |
|---|---|
| `CONTEXT.md` | Owns product domain vocabulary for Decision, Decision Ledger, GraphSnapshot, GraphEvidenceState, BindingEvidence, Source Refresh, and signoff boundaries. |
| ADR-0007 | Owns substrate-neutral governance flow and event-store materialization path. |
| ADR-0010 | Owns snapshot-scoped graph evidence and graph failure state semantics. |
| ADR-0021 | Owns GraphStore, GraphSnapshot, GraphSlice, stale invalidation, and BindingAnchor lifecycle. |
| ADR-0022 | Owns V1 signoff plus reviewable context, and keeps compliance/enforcement future-scoped. |
| `graphslice-planning-sufficiency.md` | Owns task intent, scope planning, and sufficiency proof semantics. |
| `crates/bicameral-api/src/dashboard.rs` | Existing dashboard-facing state types. |
| `crates/bicameral-api/src/evidence.rs` | Existing graph/binding evidence types. |
| `crates/bicameral-runtime/src/graph_readiness_view.rs` | Existing helper boundary for graph readiness not mutating ledger/signoff state. |

## ADR Implications

| ADR | Disposition | Reason |
|---|---|---|
| ADR-0007 | Reaffirmed | Governance/event-store replay remains the only canonical transition path. |
| ADR-0010 | Reaffirmed | Graph facts remain snapshot-scoped evidence; failure states are not ledger events by themselves. |
| ADR-0021 | Reaffirmed | GraphStore and GraphSlice remain cache/proof machinery, not ledger entities. |
| ADR-0022 | Reaffirmed | V1 remains signoff plus reviewable context; compliance and enforcement stay future-scoped. |
| New ADR for #207 | RFQ gap | #207 owns whether to add a standalone ADR for advisory-only posture or treat ADR-0022 as sufficient. |

## RFQ Gaps

These are intentionally not answered here.

| Gap | Owning issue | Required answer |
|---|---|---|
| Whether ADR-0022 fully satisfies the V1 advisory-only ADR need or a new ADR is required. | #207 | ADR owner decision. |
| Whether to keep ADR-0010's richer graph evidence enum or collapse UI/API copy to the five-state team-sync shape. | #206 | Type/API owner decision. |
| Exact MCP->bot information flow for ingestion, preflight, binding, verification, refresh, and search. | #209 | Protocol contract mapping. |
| Which LLM calls live in MCP versus bot, including fallback binding proposals. | #208 | LLM responsibility table. |
| When failed revalidation becomes durable review state versus view-only currentness. | #84 | Binding revalidation policy. |
| Whether #185 needs implementation refactor slices beyond docs/UI/API copy after this spec. | #185 | Refactor owner triage. |

## Implementation Plan

### Slice 1: Spec and issue alignment

Target: `docs/specs/codegraph-ledger-ui-protocol-boundary.md`

Acceptance:

- #210 mapping is explicit.
- #189 UI contract is actionable.
- RFQ gaps link to owning issues without answering collaborator-owned decisions.
- Linked issues receive comments pointing to this spec.

Validation:

- `git diff --check`
- `python3 scripts/validate_factory_signal_context.py`

Task class: `AFK_REVIEW_ONLY`

### Slice 2: Dashboard API read model

Target: `bicameral-api`, `bicameral-gateway`

Acceptance:

- Selected Decision payload exposes source evidence, accepted code evidence,
  advisory code hints, graph evidence state, scope sufficiency/readiness, and
  freshness state without compliance overclaiming.
- Existing signoff fields remain ownership-only.

Validation:

- `cargo test -p bicameral-api`
- `cargo test -p bicameral-gateway`

Task class: `AFK_REVIEW_ONLY`

### Slice 3: Ledger View UI

Target: dashboard UI

Acceptance:

- Code evidence renders as a selected-Decision section.
- Failure states render in a stable status div.
- Copy follows this spec's V1 posture.
- Human UX/copy review completes before merge.

Validation:

- dashboard test/build command from repo metadata;
- manual screenshot review.

Task class: `AFK_REVIEW_ONLY`

### Slice 4: Protocol and LLM RFQ follow-up

Target: #209 and #208

Acceptance:

- Protocol/LLM tables obey the constraints in this spec.
- Any unresolved collaborator decisions remain labeled as RFQ gaps.

Task class: `HITL_BLOCKED` until owners answer.

## Acceptance Criteria

- #210 has a 1:1 codegraph-to-ledger mapping.
- #189 can proceed with Ledger View code evidence under selected Decision.
- Graph failure states render as evidence/readiness status, not signoff or
  compliance state.
- The spec does not answer #208/#209 collaborator decisions.
- RFQ gaps are explicit and linked.
- Docs-only factory attestation exemption applies.

## Validation Plan

- `git diff --check`
- `python3 scripts/validate_factory_signal_context.py`
- Manual issue comment audit for #210, #189, #207, #206, #209, #208, #185,
  and #84.

## Request Coverage

| Request / issue | Coverage |
|---|---|
| #210 mapping | `1:1 Codegraph To Ledger Mapping` |
| #189 UI behavior | `Ledger View Contract`, `Failure State Div`, `Ledger View Wireframe`, `Ingestion Gate Contract` |
| #207 advisory-only posture | `Decision Ledger Boundary`, `ADR Implications`, RFQ gap |
| #206 graph evidence enum | `Failure State Div`, RFQ gap |
| #209 protocol mapping | `Protocol Boundary`, RFQ gap |
| #208 LLM responsibility | `LLM Boundary`, RFQ gap |
| #185 architecture refactor | `Decision Ledger Boundary`, implementation slices, RFQ gap |
| #84 revalidation/fallback | mapping table and RFQ gap |
