# Authority Enforcement Viability Evaluation

**Date:** 2026-06-03  
**Related:** ADR-0001, ADR-0007, ADR-0011,
[ADR-0015: Enforcement Memory Projection](../adr/0015-enforcement-memory-projection.md)

## Problem

Bicameral's Decision Ledger can preserve reviewed authority without that
authority effectively reaching the work it constrains. Retaining more source
snapshots, ranking memories by access frequency, or assigning generic
importance scores to ledger artifacts does not answer the operational question:
is an accepted Decision actually affecting implementation behavior before or
during relevant work?

## Method

Use **Authority Enforcement Viability Evaluation** as an architectural
evaluation method.

The research-paper insight is valuable for judging Bicameral's architecture:
can accepted authority plausibly reach the work it constrains, and can the
system explain where that chain is weak? It is not a decision to add
enforcement intent, route plans, enforcement cases, or receipt processing to
the production hotpath.

The selected event store substrate remains the only durable authority for
accepted governance events. The SurrealDB **Enforcement Memory Projection**
helps the evaluation by joining accepted Decisions with code/work context,
bindings, signoff, compliance checks, and operational observations. SurrealDB
may make authority queryable against work context; it must not decide what
authority exists.

This spec depends on the ADR-0010 and ADR-0015 boundary: codegraph components
validate code facts, the event store authorizes accepted binding facts, and
SurrealDB materializes those accepted facts into enforcement-memory
relationships.

This method is especially for semantic grounding uncertainty. Deterministic
projection facts should be exact after optimization and can be checked against
full recomputation. Semantic questions, such as fuzzy product scope, likely
code applicability, renamed intent, inherited bindings, or compliance drift,
cannot be made exact by the projection alone. They should be evaluated by
asking whether Bicameral can surface the uncertainty, reach the right review or
policy context, and explain what evidence was available.

Codegraph changes the evaluation by moving some questions out of semantic
uncertainty and into deterministic evidence. For example, line-number drift is
not a viability problem if codegraph can resolve the current symbol occurrence;
the projection should update its current range view while preserving the
accepted binding evidence. If codegraph cannot resolve the target, the
evaluation should ask whether the architecture makes that weakness visible
instead of pretending the shifted line range is still authoritative.

As an evaluation checklist, an Enforcement Path can be inspected through six
questions:

1. `scope_link` — is the constrained work scope known enough to test
   applicability.
2. `detection_link` — can Bicameral recognize current work that may fall in
   that scope.
3. `surface_link` — is there a surface where the relevant actor could be
   reached.
4. `policy_link` — could governance policy map the situation to an allowed
   action.
5. `capability_link` — could the selected surface and substrate honestly
   perform that action, such as warning, queuing review, pausing approval,
   requiring review, or blocking.
6. `observation_link` — could Bicameral record enough evidence to later explain
   what happened.

## Evaluation States

Enforcement state is an architectural assessment derived from the path links:

- `unknown` — no path evaluation has run.
- `detectable` — `scope_link` and `detection_link` are present.
- `reachable` — the path is detectable and has a `surface_link`.
- `actionable` — the path is reachable and has `policy_link` plus
  `capability_link`.
- `enforced` — the path is actionable and has an `observation_link` showing the
  action could be explained after it occurred.
- `degraded` — one or more links is stale, ambiguous, or unavailable, but a
  weaker honest action still exists.
- `broken` — no honest surface/action can make the Decision affect constrained
  work.

## Consequences

Architecture review should optimize for authority being traceable to
constrained work, not for preserving popular artifacts or maximizing retained
memory. Missing, stale, or ambiguous evidence matters when it prevents the
architecture from explaining how a Decision would affect implementation.

This keeps ADR-0007's governance flow intact. Enforcement-path viability does
not let preflight, dashboards, connectors, mods, caches, or projection rows
directly mutate canonical Decision Ledger state, and it does not require a
production routing hotpath.

## Rejected Alternatives

**Numeric authority-preservation scoring:** rejected because a score can rank
artifacts while still failing to prove that accepted authority reaches
constrained work.

**Access-frequency memory viability:** rejected because frequent access measures
visibility, not whether authority is effectively enforced.

**Retention-first ledger maintenance:** rejected because keeping every cited
artifact healthy is not the same as ensuring Decisions influence relevant work.
