# ADR-0029: Complete SMR Hypergraph Projection to Stable Alloy

**Date:** 2026-07-29

**Status:** proposed

**Level:** L1

**Related:** ADR-0004, ADR-0014, ADR-0028,
`docs/specs/smr-alpha-managed-preflight-user-journeys.md`,
`docs/specs/state-machine-resolver-v1-runtime-architecture.md`

## Context

Product framing for *why* this formal path exists: [Why SMR and Spec Governance](../strategy/why-smr-and-spec-governance.md).

Managed Preflight must preserve distinctions among actors, actions, authority,
states, guards, effects, ordering, branches, exceptions, and unknowns.
Retrieving only impacted atoms can find a relevant semantic neighborhood while
discarding the relation that constitutes the conflict.

Generating Alloy from natural language would move quantifiers, closure, frame
conditions, and invariant selection into a probabilistic boundary. Generating
incident-specific Alloy checks or hand-selecting a favorable graph slice would
also make evaluation circular.

## Decision

For the SMR alpha domain, Bicameral represents the complete supported accepted
and proposed hypergraphs in one exact bounded Alloy snapshot.

- A versioned extraction profile proposes a source-grounded
  `ClaimSemanticTraceV1` candidate hypergraph. The provider cannot emit Alloy
  or accepted Product state.
- Deterministic validation preserves source units, alternatives, partial
  selectors, state-identity relations, and unknown dimensions.
- The bridge serializes every supported accepted and proposed alpha-domain
  record exactly once. If the complete snapshot exceeds reviewed bounds, it
  returns `capacity_exceeded`; it does not truncate or choose an implicit
  slice.
- Stable, hand-written, version-pinned Alloy modules own the relational and
  temporal semantics. Runtime generation supplies exact data declarations,
  one named check, and explicit bounds—not new semantic logic.
- Findings reverse-map to accepted Claims, ClaimAssertions, candidate
  assertions, and exact Plan spans.
- Alloy results are bounded read-only evidence. They cannot promote a
  candidate, approve a Decision, claim global safety, or mutate canonical
  Product history.

Alpha uses textual exact snapshot declarations for inspectability. A future
implementation may construct equivalent exact Alloy/Kodkod bounds directly
without changing this semantic contract.

## Consequences

- The full supported alpha projection is the reference implementation.
- A generic or template-specific slicer is deferred until capacity requires
  it. Any future slicer must be checked against the complete reference for
  closure, provenance, and finding preservation.
- Accepted-model incompleteness remains explicit. Structural closure of the
  serialized snapshot is not proof of complete Product knowledge.
- Missing, invalid, or ambiguous consequential traces fail closed before
  Alloy. A no-counterexample result is never a fallback for failed extraction.
- Model, prompt, schema, decoding, accepted-base, serializer, Alloy module,
  analyzer, solver, and exact-scope identities participate in analysis
  identity.

## State and Authority Review

This decision adds no Product lifecycle state and changes no promotion,
signoff, or canonical append authority. Claim traces, alignments, Alloy
snapshots, and Alloy results are local rebuildable artifacts or evidence.
Candidate Promotion remains the only path in this design that can create
accepted Product history.

## Rejected Alternatives

- **Generate Alloy from the Plan:** rejected because probabilistic output would
  own formal semantics.
- **Extract impact atoms only:** rejected because relation and state identity
  can be lost.
- **Generate incident-specific checks:** rejected because it launders the
  expected answer into the evaluator.
- **Ship an intelligent slicer in alpha:** rejected because omitted closure is
  difficult to detect and the bounded alpha domain can use exact projection.
- **Treat no counterexample as clearance:** rejected because bounded absence is
  not safety, completeness, compliance, or Product approval.
