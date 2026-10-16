# Enforcement Memory Projection

**Date:** 2026-06-03  
**Status:** proposed  
**Level:** L1  
**Related:** ADR-0001, ADR-0007, ADR-0010,
[ADR-0022](./0022-v1-decision-signoff-and-reviewable-context.md),
[Enforcement Memory Projection Spec](../specs/enforcement-memory-projection.md),
[Authority Enforcement Viability Evaluation](../specs/authority-enforcement-viability-evaluation.md),
`../releases/alpha/desync-ux-hitl-readiness.md`

> V1 scope note: ADR-0022 narrows the first product delivery slice to decision
> signoff plus reviewable context. This ADR remains the future extension path
> for enforcement memory; it no longer defines the V1 delivery center.

## Decision

Use SurrealDB as Bicameral's future **Enforcement Memory Projection**: a
rebuildable operational read model that bridges accepted governance events,
source evidence, binding evidence, graph facts, signoff, compliance, and
daemon-side observations.

SurrealDB is not an authority substrate. The event store remains the durable
source of accepted governance events; SurrealDB is rebuilt from those events
plus graph/source indexes so Bicameral can answer fast operational questions
about applicability, grounding, compliance, signoff, and reflected
implementation state.

The projection can support authority-enforcement viability evaluation, but it
is not the viability framework and should not inherit evaluation-only concepts
as product schema.

## Rationale

Bicameral needs the event store to preserve authority and the codegraph to
validate code facts. Neither is shaped for fast bidirectional product queries
such as "which Decisions constrain this code?" or "which accepted Decisions
have stale bindings?" SurrealDB provides the operational graph/read model for
those questions without becoming canonical authority.

## Consequences

SurrealDB is not optional for the future enforcement-memory product value: it
is the rebuildable bridge that makes accepted authority queryable against code
and work context.
Without the projection, or with only narrow desynchronization coverage, the
system may still preserve ledger authority but is functionally unusable as
enforcement memory because it cannot keep Decisions, bindings, code state, and
compliance observations aligned enough to support the product claim.

All major desynchronization scenarios in the projection spec are minimum viable
coverage, but they do not all have the same correctness standard.
Deterministic projection facts must remain exact after optimization: accepted
event replay, graph snapshot identity, file/symbol existence, deleted targets,
stale indexes, and projected ledger rows must match the expensive
recomputation result. Semantic grounding questions, such as whether a Decision
applies to fuzzy product context or whether changed code still satisfies the
Decision, must preserve uncertainty honestly and be assessed through the
authority-enforcement viability method.

For early EM pilots, the projection should prioritize the scenarios that reduce
context regurgitation and surface stale decision memory before broad
organization-wide enforcement is attempted. The first useful slices are:
existing implementation match suggestions for newly accepted Decisions;
decision-code drift after grounded code changes; multiple Decisions touching the
same implementation surface; exact validation gates for approximate retrieval or
LLM-suggested grounding; and ungrounded Decision candidate discovery after the
index gains new symbols. These are not the only required desync scenarios, but
they are the highest-leverage pilot path because they connect directly to EM
pain: repeated "what did we decide?" routing, agents acting on plausible but
wrong context, and ADRs that no longer match implementation reality.

The ideal projection eventually covers every desync scenario with exact
deterministic facts where possible and governed semantic repair where
necessary. The alpha projection may instead expose incomplete coverage as
review queues, readiness blocks, source-only relationships, and explicit agent
constraints, as defined in
`../releases/alpha/desync-ux-hitl-readiness.md`.

Initial materialization may be expensive as long as deterministic facts are
accurate and semantic uncertainty is explicit. Performance optimizations,
incremental indexes, and acceleration layers must be introduced only after that
projection contract is clear.

Deleting and replaying the projection must not change canonical authority; if
it would, the data belongs in the event store instead.

The viability insight from the research paper is useful as an architectural
evaluation method: can the system show whether authority could be enforced?
It should not, by itself, create a production hotpath for route planning,
intent modeling, or enforcement-case execution.
