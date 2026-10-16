# ADR-0022: V1 Decision Signoff and Reviewable Context

**Date:** 2026-06-08
**Status:** proposed
**Level:** L1
**Related:** #183, #185, ADR-0001, ADR-0007, ADR-0010, ADR-0015

## Decision

V1 centers Bicameral on **decision signoff plus reviewable context**, not
enforcement memory. Bicameral should capture claims, source evidence, explicit
evidence references, and the human review state that determines whether a claim
becomes shared team memory.

V1 must not claim code correctness, feature readiness, verified dependency
coverage, implementation compliance, or automated enforcement unless a human
review explicitly records that narrower verdict. Verified binding, compliance,
desynchronization handling, and the SurrealDB Enforcement Memory Projection are
future extensions tracked by #185.

SMR alpha is a V1 reviewable-context capability: it recalls applicable
product-approved specifications and names missing or unknown corpus coverage.
It does not turn CodeGraph, binding, currentness, or implementation evidence
into a closure requirement or implementation truth; ADR-0028 owns that
boundary.

Candidate promotion also defines V1's maintenance boundary. A promoted
Decision is tracked by default, even while `signoff.state = proposed`:
Bicameral should keep its cited evidence reviewable by checking freshness and
enabling source refresh when the cited Source changes or becomes stale. Signoff
approval remains a separate ownership lifecycle and does not mean code
correctness or compliance. Raw sources, rejected candidates, and unpromoted
context remain advisory context and do not create an automatic refresh
obligation unless a workspace policy or explicit user action opts them into
tracking.

Future enforcement workflows may consult signoff as a prerequisite or policy
input, but V1 signoff itself remains ownership approval of Decision meaning. It
does not create blocking behavior, compliance state, or implementation truth.

The event-store boundary remains composable. `EventStoreAdapter` continues to
be the substrate-neutral append/replay/materialization contract for accepted
governance events. Git, Drive/basic-store, memory, and future adapters may
provide different review mechanics and optional capabilities, but V1 domain
objects must not hardcode Git, SurrealDB, dashboard, MCP, or any other surface
as the ontology.

ADR-0022 is the product-scope delta. Granular engineering decisions should
manifest as targeted updates to the ADRs that already own those boundaries:
ADR-0001 for event-store substrates, ADR-0007 for governance flow, ADR-0010
for graph/snapshot evidence, ADR-0015 for enforcement memory, and related
surface ADRs for MCP, dashboard, gateway, and external ingest.

## Rationale

The prior architecture over-centered the first product slice on binding,
compliance, desync coverage, and enforcement-memory projection. That made early
delivery depend on the hardest correctness problems before proving the smaller
customer value: reducing repeated context reconstruction and making AI-assisted
engineering claims reviewable.

The narrower V1 is more predictable. Connectors, MCP sessions, PRs, and other
sources can submit evidence and candidate claims; governance policy and human
review decide what becomes durable team memory; replay from the selected event
store remains the authority path. Later enforcement work can strengthen
selected references into verified bindings and compliance verdicts without
rewriting V1 artifacts.

Scoping automatic refresh to tracked Decisions keeps V1 from becoming a
general-purpose crawler or self-maintaining external knowledge base. Bicameral
maintains the evidence behind shared team memory; everything else can remain
stale until a human or integration asks for a new capture.

## Consequences

Product language should describe V1 as signoff and context, not automatic
source-of-truth maintenance or enforcement.

MCP, dashboard, and connector behavior should prefer tracked Decisions over raw
context when injecting or displaying team memory. They may surface unpromoted
context as advisory, but stale warnings, refresh queues, and automatic
recapture should be driven by tracked Decisions by default.

Docs and code should keep extension points for enforcement memory, graph
grounding, binding revalidation, and compliance, but those paths must not be
required for the first useful workflow.

ADR-0015 remains the future extension decision for Enforcement Memory
Projection. It no longer defines the V1 delivery center.

Future detailed work should update the owning ADR directly and reference this
ADR only for the V1 scope constraint.
