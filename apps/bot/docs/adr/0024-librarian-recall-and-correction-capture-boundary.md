# ADR-0024: Librarian Recall and Correction Capture Boundary (Alpha)

**Date:** 2026-06-23  
**Status:** proposed  
**Level:** L1  
**Related:** ADR-0001, ADR-0004, ADR-0007, ADR-0010, ADR-0017, ADR-0019, ADR-0022 (v1 signoff);
`../specs/constraint-lookup-recallpacket-contract.md`;
`../specs/librarian-recall-readme.md`;
`../specs/factory-alpha-librarian-lifecycle-delta.md`  
**Resolves into ADR:** the scope decision recorded in BicameralAI/bicameral-bot#501 (closed) and the runtime-flow update commented there.

## Problem

The "librarian" alpha posture is defined across a spec set
(`constraint-lookup-recallpacket-contract.md`,
`factory-alpha-librarian-lifecycle-delta.md`,
`librarian-recall-readme.md`) and a closed RFQ (#501), but no ADR states it as a
canonical decision. As a result the ADR set still reads as if the full vision is
the build sheet:

- ADR-0019 still describes staged `preflight.run` behavior without recording that
  alpha collapses staged lookup modes into one read-only behavior.
- Nothing canonical states that alpha **capture** is constrained to controlled
  corpus correction over existing governed paths.
- Nothing canonical states that implicit agent-decision capture
  (BicameralAI/bicameral-mcp#148, #610) and arbitrary technical-constraint mining
  are **out of scope** for the librarian alpha, even though those issues are open
  and P1.

This drift makes the product look more complex than the alpha actually is, and it
lets adjacent R&D leak into the alpha authority contract. #501 already settled the
boundary; this ADR makes that settlement canonical.

## Decision

For the librarian alpha, Bicameral exposes exactly two behavior-changing
operations over the ledger-governed trusted corpus:

```text
lookup
  read-only recall over the configured corpus scope
  returns RecallPacket
  no canonical mutation

request_correction
  explicit, approval-gated request that starts a governed correction /
  corpus-update path using existing Bicameral evidence mechanics
```

The trusted corpus is not a second authority. The Decision Ledger remains the
authority spine; the trusted corpus is the ledger-governed evidence surface
(`constraint-lookup-recallpacket-contract.md`).

### 1. Lookup is read-only

`RecallPacket` is the terminal artifact of the Constraint Lookup stage. Returning
a packet must not create or mutate canonical state (no `SourceSnapshot`,
`SourceEvidence`, `DecisionCandidate`, `Decision`, `BindingEvidence`, signoff, or
compliance state). The packet must name searched sources, matches with explicit
authority labels (`canonical | source_only | candidate | derived`), freshness,
and `unknown_scope`. "No relevant decision found" never implies global
completeness. Field-level shape is owned by
`../specs/constraint-lookup-recallpacket-contract.md`.

### 2. Staged preflight modes are collapsed

Alpha does not define separate protocol semantics for `pre_work`, `mid_session`,
and `pre_write`. Those labels survive only as an inert `checkpoint_hint` carried
for trace/relevance diagnostics. The daemon must not treat a checkpoint hint as
permission to capture, guard, block, enforce, or mutate state. This supersedes,
for alpha, any reading of ADR-0019 or `preflight-search-readiness.md` that treats
staged lookup as distinct guard/capture modes.

### 3. Correction capture stays on existing governed paths

`request_correction` is the alpha capture bridge, and it is a request to repair
source truth, not the repair. It must:

- enter source material through existing `Source` / `SourceSnapshot` /
  `SourceEvidence` ingest (integrations or local ingest);
- preserve the supplied artifact/context and trusted-corpus revision needed to
  explain the recall or coverage finding; code evidence, if relevant, remains
  optional advisory provenance rather than a correction prerequisite;
- require explicit **approval proof** from an authorized actor or a
  policy-approved review path — MCP/caller intent is not approval proof
  (per #501 adversarial review);
- route durable mutation through the existing governed acceptance path only.

It must **not** introduce a new evidence class for ADRs, ODRs, product docs, or
feature definitions, mint a parallel correction lifecycle, automatically record a
new Decision, or treat agent-generated rationale as accepted product truth.

### 4. Authority separation is unchanged

This ADR adds no new canonical Decision lifecycle states. It reaffirms ADR-0004
(separate extraction/binding/compliance surfaces), ADR-0007 (all canonical state
passes through governed review + event-store materialization), and ADR-0022
(v1 = signoff plus reviewable context, not enforcement). Derived librarian
artifacts (`recall_packet`, `correction_request`, `correction_candidate`,
freshness/salience metadata, dispatch receipts) are read models and proposals,
never authority.

### 5. Alpha preflight does not resolve conflicts

`lookup` may return relevant accepted Decisions, possible contradictions,
source-only contradiction candidates, and unknown scope. `request_correction`
may create or route a proposed `DecisionCandidate`, proposed Decision, or
corpus-update/correction request. Neither operation resolves a conflict.

For alpha, conflict resolution is owned by the dashboard / review surface:
supersede, reject, approve signoff, or mark different scopes remain governed
review commands. MCP may later expose conflict-resolution ToolRequests, but only
behind an explicit authority gate and the bot-owned governed review path. The
alpha `preflight.run` path must not smuggle those actions into recall or
correction capture.

## Out of Scope for the Librarian Alpha

The following are explicitly **not** part of the librarian alpha. They may be
adjacent R&D or later-stage work, but they must not define the alpha authority
contract and must not be implemented as if accepted:

- **Implicit agent-decision capture** — snooping the agent call stack,
  thinking-trace / transcript prose scanning, diff-shape classification, or
  counterfactual replay to mine decisions the agent made silently
  (BicameralAI/bicameral-mcp#148, #610, #620). These are adjacent prior design
  work; #501 records that they "should not define this alpha authority contract."
- **Arbitrary technical-constraint mining** from implementation sessions.
- Autonomous / self-evolving / autopoietic memory; auto-decay, demotion, or
  deletion of accepted Decisions.
- Confidence-authorized correction or mutation; graph-derived product truth,
  "no conflict," compliance, or signoff.
- MCP/preflight conflict resolution actions such as supersede, reject, approve
  signoff, or mark-different-scopes.
- A second policy engine, blocking librarian gate, or dashboard command-center.

The full deferred/rejected state list is owned by
`factory-alpha-librarian-lifecycle-delta.md` and remains authoritative; this ADR
names the agent-decision-capture exclusion explicitly because its issues are
open, P1, and easy to mistake for alpha scope.

## Consequences

Positive:

- The librarian alpha boundary is now canonical, not buried in a closed issue.
- Open, high-priority R&D (mcp#148/#610) is unambiguously separated from the
  alpha build sheet, preventing scope leak.
- ADR-0019's staged-preflight description is reconciled with the collapsed alpha
  lookup behavior.
- Alpha can still surface contradiction candidates for the controlled corpus
  story without implying that preflight or MCP can resolve conflicts.

Tradeoffs:

- Alpha capture cannot record the (large and real) population of implicit agent
  decisions. That population is deferred deliberately, not forgotten — it needs
  its own ADR and ownership decision before any implementation.
- A future stage that re-introduces staged guard modes or implicit-decision
  capture requires a successor ADR; it cannot arrive through spec edits alone.
- A future stage that exposes MCP conflict resolution requires an explicit
  authority-gated ToolRequest design; it cannot be inferred from RecallPacket
  next actions.

## Rejected Alternatives

- **Leave the decision in #501 + specs only:** rejected because the ADR set is the
  canonical decision record and was already drifting from alpha reality.
- **Amend ADR-0019 instead of a new ADR:** rejected because the correction-capture
  and out-of-scope decisions are broader than preflight; a forward-link from
  ADR-0019 is sufficient.
- **Treat mcp#148/#610 as alpha because they are open and P1:** rejected; #501
  explicitly excludes them from the alpha authority contract.
