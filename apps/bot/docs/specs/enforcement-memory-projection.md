# Enforcement Memory Projection

> V1 scope note: ADR-0022 and
> `codegraph-ledger-ui-protocol-boundary.md` narrow the first product workflow
> to Decision signoff plus reviewable context. This spec remains the future
> Enforcement Memory Projection contract. A V1 E2E projection may use SurrealDB
> only through `v1-reviewable-context-projection.md`, as a rebuildable read
> model for Decisions, source evidence, code evidence, graph readiness, and
> recovery actions. It must not present compliance, implementation correctness,
> feature readiness, or enforcement as current product claims.

The Enforcement Memory Projection is Bicameral's rebuildable SurrealDB layer for
making accepted authority queryable against implementation context. It is not
the authority substrate. The event store remains the durable source of accepted
governance events; SurrealDB is rebuilt from those events plus graph/source
indexes so Bicameral can answer fast operational questions about applicability,
grounding, compliance, signoff, and reflected implementation state.

This is the dedicated layer contract for SurrealDB in Bicameral. ADR-0015 makes
the architectural decision to use SurrealDB for this projection. The Authority
Enforcement Viability Evaluation spec uses the projection to evaluate whether
authority could be enforced, but the projection itself is not the viability
framework and should not inherit evaluation-only concepts as product schema.
The alpha presentation and HITL routing for incomplete desync coverage is
defined in `../releases/alpha/desync-ux-hitl-readiness.md`.

## Scope

The projection owns the queryable read model that bridges accepted governance
events to implementation context:

- accepted Decisions and cited source evidence
- accepted `BindingEvidence` projected into decision-to-code/work edges
- signoff and compliance read state
- graph-backed currentness, drift, and grounding observations
- daemon-side operational receipts when useful for local audit or debugging

The projection does not own canonical authority. It does not approve Decisions,
accept bindings, resolve compliance, or create governance obligations. Those
actions belong to governance policy and the selected event store substrate.

The projection is required for usable enforcement memory. A daemon can preserve
ledger authority without SurrealDB, but if it cannot cover the major
desynchronization scenarios between Decisions, bindings, code state, graph
state, and compliance observations, it is functionally unusable for the
enforcement-memory product promise.

Accuracy is the first design constraint, but the projection distinguishes
deterministic correctness from semantic grounding.

Deterministic projection facts must be exact and replayable. Examples include
accepted event replay, graph snapshot identity, file and symbol existence,
deleted targets, stale indexes, binding rows, and projected ledger rows. The
projection should be designed from an exhaustive recomputation model for these
facts even if early operations are expensive.

Semantic grounding is not exact truth. Examples include whether an unbound
Decision probably applies to existing code, whether a renamed symbol preserves
implementation intent, whether superseded Decision text should inherit old
bindings, or whether drifted code still satisfies a Decision. For these cases,
the projection must preserve uncertainty as candidates, ambiguity, or
review-needed state. The authority-enforcement viability spec assesses whether
the architecture can make those uncertainties visible and actionable.

Optimizations such as incremental indexing, cached graph joins, or systems like
CocoIndex may speed up materialization later, but they must not define
correctness. For deterministic facts, optimized output must match full
recomputation. For semantic grounding, optimized output must preserve the same
uncertainty boundaries and must not silently promote candidates into accepted
`BindingEvidence` or compliance facts.

## Boundary

```text
event store
  -> accepted governance events
  -> replayable authority

codegraph
  -> snapshot-scoped code facts
  -> GraphSnapshot / SymbolOccurrence / GraphEvidenceState

SurrealDB Enforcement Memory Projection
  -> queryable decision-to-code/work relationships
  -> signoff + compliance read model
  -> daemon-side operational observations
```

The invariant is:

```text
Codegraph validates code facts.
Event store authorizes accepted binding facts.
SurrealDB materializes accepted facts into enforcement-memory relationships.
```

SurrealDB may make authority queryable against work context. It must not decide
what authority exists.

Accepted `BindingEvidence` is provider-agnostic. A local Rust resolver, future
CodeGenome-style provider, hosted graph service, or other accepted graph
provider may contribute code facts, identity evidence, confidence, and
provenance. The projection stores and queries the accepted binding facts and
their graph identity; it must not treat a provider's internal graph identity as
canonical authority outside the accepted evidence payload.

SurrealDB rows and edges are operational memory. If deleting and replaying the
projection would change canonical authority, the data was stored in the wrong
layer.

## Operational Observations

Operational receipts are allowed in the projection, but they are not the center
of the product model and they are not part of the event-substrate hotpath.

- `operational_receipt` is daemon-side projection state. It records that an
  enforcement-related check, search, preflight, or local surface happened. It
  is queryable and useful for debugging, architecture evaluation, and local
  audit, but it is not stored in the event substrate.
- Governance receipt facts are event-store-backed. They record outcomes that
  may later justify, challenge, or change shared authority, such as a required
  review request, a blocked approval, or a closed compliance decision.

The test is whether the receipt can later carry authority. If yes, it belongs
in the event store and is projected into SurrealDB. If no, it stays as daemon
operational memory.

## Core Shape

The old MCP SurrealDB model had the right product center:

```text
input_span -yields-> decision -binds_to-> code_region
```

The bot version should keep that shape, but with clearer authority boundaries:

- `input_span` / `SourceEvidence`: cited source material projected from accepted ingest events.
- `decision`: accepted Decision Ledger state projected from candidate/signoff events.
- `code_region` / `symbol_occurrence`: code/work targets derived from accepted `BindingEvidence` plus graph facts.
- `compliance_check`: projected or accepted compliance evaluations against current code state.
- `operational_receipt`: daemon-side record of checks, matches, preflights, or local surface attempts.

Expected relationship shape:

```text
input_span -yields-> decision
decision -binds_to-> code_region
decision -has_check-> compliance_check
operational_receipt -observed-> decision | code_region | work_context
```

Do not add `route_plan`, `enforcement_intent`, or enforcement-case tables to
the hotpath solely because the viability framework uses those concepts to
evaluate the architecture.

## What It Minimizes

The projection exists to minimize desynchronization between decisions, code, and
code indexes. It does not eliminate every failure; it makes the failures
visible, queryable, and repairable. Coverage of these scenarios is not optional
polish. If broad desync coverage is absent, the projection cannot serve as
enforcement memory.

All twelve scenarios below are minimum viable coverage for enforcement memory,
but this spec classifies the problem rather than choosing the mitigation.
Mitigation strategy belongs in ADRs or implementation plans once the projection
contract is accepted.

For deterministic scenarios, the optimization path works backward from exact
recomputation toward faster indexes and incremental refresh. For semantic
grounding scenarios, the optimization path works backward from honest
uncertainty preservation and viability evaluation. Mixed scenarios require both.

## Codegraph Relationship

Codegraph and the Enforcement Memory Projection address the desync scenarios at
different layers.

```text
codegraph
  -> snapshot-scoped code facts
  -> file/symbol existence
  -> symbol occurrence, move, rename, and readiness evidence

SurrealDB Enforcement Memory Projection
  -> accepted authority-to-code/work relationships
  -> currentness, compliance, and grounding read state
  -> candidates, ambiguity, and repair surfaces
```

Codegraph reduces uncertainty by proving code facts at a `GraphSnapshot`.
SurrealDB relates those facts to accepted Decisions and `BindingEvidence`.
Neither layer may accept authority by itself.

Indicator: `✓` direct help, `~` partial help, `-` mostly orthogonal.

| # | Desync scenario | Class | CG | Codegraph relationship |
|---:|---|---|:---:|---|
| 1 | New Decision is accepted while matching code already exists, but no binding is created. | semantic | ~ | Codegraph supplies the searchable code space and candidates, but whether the Decision applies is semantic grounding. |
| 2 | Code changes after a Decision was grounded. | mixed | ~ | Codegraph detects changed files, symbols, or neighborhoods; whether the change still satisfies the Decision is semantic/compliance grounding. |
| 3 | Bound code is deleted. | deterministic | ✓ | Codegraph can say the target no longer exists at the inspected snapshot. |
| 4 | Symbol is renamed. | mixed | ~ | Codegraph may supply rename or similarity evidence; whether the renamed symbol preserves implementation intent is semantic. |
| 5 | Symbol moves to another file. | mixed | ✓ | Codegraph can often find the same or similar symbol occurrence at a new location. |
| 6 | Code index gains new symbols that could ground previously ungrounded Decisions. | semantic | ~ | Codegraph supplies new candidates after index refresh; applicability remains semantic grounding. |
| 7 | Cold start has no code index. | deterministic | ✓ | Codegraph readiness state defines `not_configured`, `indexing`, `stale`, and `failed`. |
| 8 | Drifted Decision could still match code at a new location. | semantic | ~ | Codegraph may supply possible locations; fit remains semantic grounding. |
| 9 | Supersession changes Decision text while old binding may still be useful. | semantic | - | Codegraph can validate inherited target existence, but inheritance is Decision lineage/governance semantics. |
| 10 | Multiple Decisions bind to the same code target. | deterministic | ✓ | Codegraph gives stable target identity; SurrealDB fans out affected Decisions. |
| 11 | Approximate retrieval suggests the wrong grounding. | semantic | ~ | Codegraph can verify or reject exact targets suggested by retrieval; product relevance remains semantic and candidates must stay non-authoritative. |
| 12 | Line numbers shift after insertion or formatting. | deterministic | ✓ | Codegraph avoids raw line ranges as durable identity by using symbols and snapshot-backed occurrence evidence. |

Scenario 12 is the baseline example. A line shift is not a semantic question
when the symbol identity or snapshot-backed occurrence can still be resolved.
Codegraph should prove the current occurrence; SurrealDB should update the
current projected range while preserving the accepted `BindingEvidence` that
records the original reviewed target. If only raw lines exist and the symbol
cannot be resolved, the projection must mark the relationship as weak or
review-needed rather than silently treating the shifted range as authoritative.

## EM Pilot Prioritization

All twelve desync scenarios remain minimum viable coverage for enforcement
memory, but early EM pilots should not try to demonstrate all of them at once.
The first pilot path should prioritize scenarios that reduce context
regurgitation and make stale or ungrounded decision memory visible without
claiming full GraphStore coverage.

| Priority | Desync scenario | EM pain | Pilot feature |
|---:|---|---|---|
| 1 | New Decision is accepted while matching code already exists, but no binding is created. | EMs repeatedly answer where the team already solved the same problem. | Existing implementation match suggestions. |
| 2 | Code changes after a Decision was grounded. | ADRs and accepted Decisions become stale after engineers or agents ship code. | Decision-code drift dashboard. |
| 3 | Multiple Decisions bind to the same code target. | Architecture surfaces become overloaded, contested, or governed by unclear overlapping Decisions. | Decision overlap map. |
| 4 | Approximate retrieval suggests the wrong grounding. | AI agents can act on plausible but wrong decision/code context. | Exact validation gate for AI-suggested grounding. |
| 5 | Code index gains new symbols that could ground previously ungrounded Decisions. | Old ADR or Decision debt remains invisible until someone manually reconnects it. | Ungrounded Decision candidate discovery. |

These five pilot scenarios map to the first two product value arcs:

```text
context regurgitation -> agentic enforcement
```

They let Bicameral show value while preserving honest uncertainty:

- source-only and approximate matches stay non-authoritative until validated;
- missing graph coverage returns `unknown_not_indexed` or readiness state, not
  a clean result;
- agent-facing output may recommend review, but it must not materialize
  `BindingEvidence` or compliance facts without verified evidence and accepted
  governance flow.

Later pilots can expand into continuity and compliance/security readiness by
covering symbol rename/move lineage, supersession and inherited-binding review,
semantic repair/revalidation, cross-team collision review, governed preflight,
and hosted/team visibility.

Alpha releases must still present non-prioritized scenarios instead of
suppressing them. Route them through the release product states:

| Product state | Use for |
|---|---|
| `verified` | Deterministic projection facts or accepted evidence that are valid for the inspected scope. |
| `review_needed` | Ambiguous lineage, semantic repair, supersession inheritance, approximate grounding, or possible drift matches. |
| `readiness_blocked` | Missing index, stale graph, unsupported language, failed resolver, dirty worktree, unresolved ref, or hosted/team-required scope. |
| `source_only` | Useful Decision/source context without verified code grounding. |

## Query Classes

The projection should support bidirectional queries.

Forward from work to authority:

```text
Given a changed file, symbol, dependency, workflow, or deploy surface:
  Which accepted Decisions may constrain it?
  Which source and binding evidence justify that relationship?
  What current signoff and compliance state is projected?
```

Backward from authority to expected work:

```text
Given an accepted Decision:
  Which source evidence supports it?
  Which code/work contexts is it bound to?
  Which bindings or compliance checks look weak, stale, or missing?
```

This backward query is what lets the viability framework evaluate the
architecture. If the projection cannot explain why a Decision applies to work,
the repair likely belongs in binding, graph coverage, or compliance evidence.

## Event-To-Projection Mapping

Canonical events remain in the event store. SurrealDB projection rows/edges are
derived from replay and may be rebuilt.

| Event-store fact | Projection effect |
|---|---|
| `Ingest` with source evidence | `input_span` / source evidence nodes and `input_span -yields-> candidate` projection. |
| `AcceptCandidate` | `decision` node with proposed signoff state. |
| `ApproveSignoff` / `RejectSignoff` | Update decision signoff axis. |
| `BindToCode` | `decision -binds_to-> code_region` operational edge, with graph snapshot identity and evidence provenance. |
| `ResolveCompliance` | Update compliance axis and/or `compliance_check` rows. |
| `SupersedeDecision` | Supersession lineage and inherited/proposed binding repair candidates. |

Projection-only daemon observations may also create or update
`operational_receipt` rows. Those rows are intentionally absent from the event
store unless governance policy promotes the observation into an accepted
governance fact.

## Non-Goals

- Do not let SurrealDB create, approve, reject, supersede, or resolve canonical Decisions by itself.
- Do not treat graph search hits, approximate retrieval matches, or generated candidates as accepted bindings.
- Do not bake route plans, enforcement intents, or enforcement-case machinery into the hotpath from the viability framework alone.
- Do not make codegraph indexes canonical `.bicameral/` artifacts.
- Do not collapse signoff and compliance into one status.
