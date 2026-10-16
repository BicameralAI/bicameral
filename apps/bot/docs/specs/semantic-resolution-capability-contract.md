# Semantic Resolution Capability Contract

## Metadata

| Field | Value |
|---|---|
| Date | 2026-07-31 |
| Status | Proposed |
| Level | L2 cross-repository semantic contract |
| Contract ID | `bicameral.semantic-resolution` |
| Owns | The release-independent obligations that make semantic resolution source-bound, authority-safe, and suitable for deterministic bounded analysis |
| Revision identity | Exact artifact SHA-256 recorded by the governed review; mutable labels such as `latest` are invalid |
| Semantic source | [`smr-alpha-claim-semantic-trace-alloy-bridge.md`](smr-alpha-claim-semantic-trace-alloy-bridge.md) |
| Runtime source | [`smr-alpha-runtime-contract.md`](smr-alpha-runtime-contract.md) |
| Architecture decision | [`ADR-0029`](../adr/0029-complete-smr-hypergraph-projection-to-stable-alloy.md) |
| Governance gate | `bic:spec-governance-gate`; Qor advisory/no-op |
| UserJourney | Not applicable: this contract factors existing semantic obligations from release scheduling and adds no user-visible flow |
| Journey Boundary Review | Not applicable: consuming Product journeys retain their existing boundary reviews and terminal evidence |

## Reading guide

Product motivation: [Why SMR and Spec Governance](../strategy/why-smr-and-spec-governance.md).

Read **Decision**, **Normative obligations**, and **Conformance and release
binding** first.

Main invariant:

> A capability contract defines what must be true. Work may satisfy its
> obligations before a release activates the capability, but neither an issue,
> a merged implementation, nor supporting evidence can make a release claim.

## Problem

Semantic-resolution work has been described using one product version for two
different facts:

1. the semantic behavior that implementations must satisfy; and
2. the release expected to activate that behavior.

That makes earlier implementation tickets appear to depend on a later release,
even when the earlier work is what makes the later activation possible. It also
encourages evidence laundering: satisfying one implementation slice can be
mistaken for fulfilling or shipping a whole release.

The existing SMR specifications already define the important semantics:
source-bound `ClaimSemanticTraceV1`, deterministic validation and alignment,
complete exact hypergraph projection, stable Alloy checks, bounded result
language, reverse provenance, and a strict authority ceiling. Those semantics
must have a durable identity independent of product release numbering.

## Goals

- Give semantic resolution one release-independent contract identity.
- Give each independently verifiable obligation a stable ID.
- Let work land incrementally while saying exactly which obligations it
  satisfies.
- Keep implementation evidence, current conformance, and release activation
  separate.
- Let consuming specs and tickets depend on contract obligations without
  depending on a future release number.
- Preserve the existing ClaimTrace, SMR hypergraph, Alloy, authority, and
  failure semantics without creating a competing model.

## Non-goals

- Redefining `ClaimSemanticTraceV1`, the SMR hypergraph, or Alloy semantics.
- Adding a conformance service, database, daemon state, or Product lifecycle
  state.
- Declaring any release ready or activated.
- Making historical implementation evidence current after its code has been
  removed from the target branch.
- Replacing consumer-specific runtime contracts, MCP display contracts, or
  terminal UserJourney evidence.
- General natural-language formalization, unbounded proof, or global safety.

## Decision

`bicameral.semantic-resolution` is the stable capability contract. Its exact
revision is content-addressed. Release names are not part of its identity.

```text
Semantic Resolution Capability Contract
  exact digest + stable SRC-* obligations
                 |
                 +--> implementation slice
                 |      exact subject revision
                 |      obligation-scoped evidence
                 |
                 +--> another implementation slice
                 |      exact subject revision
                 |      obligation-scoped evidence
                 |
                 +--> current conformance assessment
                 |      contract digest
                 |      reachable subject revisions
                 |      complete obligation coverage
                 |
                 `--> release binding
                        exact contract + conformance assessment
                        + consumer terminal journey evidence
                        + release-owner activation
```

The layers have different authority:

| Layer | May claim | Must not claim |
|---|---|---|
| Contract | Required semantic behavior | Implementation, readiness, or release |
| Issue | Intended `SRC-*` obligation coverage | That an unimplemented obligation is satisfied |
| PR evidence | A named obligation passed at an exact subject revision | Current conformance after the revision is removed or superseded |
| Conformance assessment | Named obligations are satisfied by reachable exact revisions and evidence | Product activation or release readiness |
| Release binding | A release consumes one exact contract revision and its complete current evidence | Stronger semantics than the bound contract or evidence supports |

## Existing infrastructure inventory

| Artifact | Existing responsibility | Use in this contract |
|---|---|---|
| Claim Semantic Trace ↔ Alloy bridge spec | Owns the semantic representation, projection algorithm, bounded analysis, and reverse provenance | Normative semantic source |
| SMR alpha runtime contract | Owns pinned source, snapshot, readiness, and result identity | Normative runtime source |
| ADR-0029 | Separates probabilistic extraction, exact projection, and stable Alloy semantics | Durable architecture decision |
| `ClaimSemanticTraceV1` | Source-grounded candidate hypergraph | Probabilistic boundary |
| SMR hypergraph | Typed accepted and proposed semantic relations | Alignment and projection vocabulary |
| Stable Alloy model and checks | Reviewed bounded relational semantics | Deterministic analysis boundary |
| Factory governance result | Binds reviewed spec bytes and authority review | Contract revision evidence |
| Factory semantic-contract pair registry | Registers product-owned producer/consumer artifacts and fingerprints | Reused for Bot-to-MCP drift control after the canonical contract bytes merge |
| Release manifest | Binds exact deliverables and validation to one release | Activation binding; not contract identity |

Prior implementation and benchmark work may provide evidence for individual
obligations. Evidence remains bound to its exact commit. If a later release
closure removes that implementation, the historical result stays useful as
design and regression evidence but does not establish current conformance for
the new branch head.

## Normative obligations

The tables below extract obligations from the governing SMR specifications.
They do not replace the detailed schemas and algorithms in those documents.
When prose conflicts, the cited semantic or runtime source owns the underlying
behavior; this contract owns the stable obligation ID and conformance boundary.

### Source and probabilistic boundary

| ID | Obligation | Required evidence |
|---|---|---|
| `SRC-SOURCE-01` | Every consequential proposed assertion and semantic relation binds to exact retained source units whose bytes, spans, identity, and digest can be recovered deterministically. | Source mutation and reverse-span fixtures |
| `SRC-TRACE-01` | The extractor produces the typed occurrence/assertion/edge/semantic-relation structure required by `ClaimSemanticTraceV1`; an impact-atom bag is not a conformant substitute. | Schema validation plus branch, exception, and adjacent-state fixtures |
| `SRC-TRACE-02` | Alternatives, partial selectors, unknown dimensions, quantifiers, branches, exceptions, and `equivalent_to` versus `distinct_from` relations remain explicit. Missing semantics are not invented to complete a tuple. | Controlled semantic mutations with visible unknown or preserved distinction |
| `SRC-PROFILE-01` | Extraction binds the exact provider, model, prompt, schema, decoding profile, placement, and provider configuration. Missing, aliased, `default`, or `latest` identities are rejected. | Extraction-profile identity and substitution tests |

### Deterministic bridge

| ID | Obligation | Required evidence |
|---|---|---|
| `SRC-VALIDATE-01` | Deterministic validation checks schema, source references, enums, graph references, size bounds, and digest. Invalid model output is rejected rather than semantically repaired. | Invalid-reference, omission, enum, digest, and capacity fixtures |
| `SRC-ALIGN-01` | Every assertion and semantic relation is aligned independently with accepted Claims and ModelAtoms while retaining alternatives and unknowns. Alignment never fills an omitted dimension. | Accepted-versus-proposed alignment fixtures with assertion-level outcomes |
| `SRC-PROJECT-01` | Every supported accepted and proposed record in the pinned domain is serialized exactly once. Overflow returns typed `capacity_exceeded`; it does not truncate, silently slice, or raise bounds. | Record-count, exact-cardinality, and overflow tests |
| `SRC-ALLOY-01` | Reviewed, version-pinned Alloy modules own quantifiers, closure, relational, and temporal semantics. Runtime generation supplies exact data, a named stable check, and explicit bounds; an LLM never emits executable Alloy. | Stable-model digest checks and generated-snapshot inspection |
| `SRC-IDENTITY-01` | Analysis identity binds the accepted base, supported-domain digest, extraction profile, trace, alignment, serializer, stable models, analyzer distribution, solver, and exact scope. A change to any constituent cannot reuse the prior result as current. | Deterministic digest, cache reuse, and invalidation tests |

### Result, authority, and efficacy

| ID | Obligation | Required evidence |
|---|---|---|
| `SRC-FAILURE-01` | Invalid, ambiguous, unsupported, incomplete, unavailable, timed-out, and over-capacity paths remain typed. A failed precondition cannot become a no-counterexample result. | Negative-path matrix covering every public refusal or unknown class |
| `SRC-SCOPE-01` | Every formal result exposes its exact snapshot, check, bounds, solver, timeout, and unknown coverage. Absence of a bounded counterexample is never rendered as global safety, correctness, compliance, approval, readiness, or completeness. | Public-language and false-clear negative tests |
| `SRC-PROVENANCE-01` | Every finding reverse-maps through stable snapshot and hypergraph IDs to the proposed assertion or accepted Claim and its exact source span. | Round-trip provenance fixtures for findings and unknowns |
| `SRC-AUTHORITY-01` | ClaimTrace, alignment, Alloy snapshots, results, and preflight are derived artifacts with no canonical mutation authority. Only the separately governed promotion path may append accepted Product truth. | Accepted-store digest invariance and mutation-denial tests |
| `SRC-EFFICACY-01` | The maintained critical semantic-mutation suite has zero known false-safe outcomes, and model-backed extraction efficacy is reported separately from deterministic-kernel correctness. | Blinded or held-out extraction lane plus oracle-kernel lane, with denominators and failure classes |

## Conformance record

No new service or repository-wide registry is required. A PR, issue, or release
manifest may carry the following minimal record:

```text
SemanticResolutionConformance {
  contract_id: bicameral.semantic-resolution
  contract_digest
  obligation_ids[]
  subject_repository
  subject_revision
  evidence_refs[]
  result: satisfied | not_satisfied
  assessed_at
}
```

Rules:

1. `satisfied` applies only to the named obligations, subject revision, and
   evidence.
2. Partial obligation coverage is valid and must remain visibly partial.
3. Evidence from an unreachable, reverted, or superseded implementation is
   historical evidence, not current conformance.
4. One obligation cannot stand in for another. Kernel correctness cannot stand
   in for extraction efficacy, and extraction efficacy cannot stand in for
   deterministic checking.
5. `not_satisfied` includes missing, stale, ambiguous, or failed evidence. No
   third state such as `future_ready` is introduced.
6. Reassessment at a new head may reuse an evidence procedure but must bind the
   new exact revision and result.

This record is an evidence shape, not a new canonical Product entity. It may
initially be represented by a checked table in the relevant PR or release
manifest.

No new registry is introduced. After this Bot-owned contract merges and has
stable canonical bytes, a Factory follow-up registers the Bot producer and MCP
consumer artifacts in the existing semantic-contract pair registry. That
registry proves producer/consumer fingerprint agreement; it does not replace
the obligation-level evidence record above or transfer Product authority to
Factory. The initial contract PR cannot honestly register a pre-merge
fingerprint.

## Ticket and project rules

Implementation tickets use:

- the actual release train into which their code or documentation is expected
  to land;
- the exact `SRC-*` obligations they intend to satisfy;
- acceptance criteria and evidence for those obligations.

They do not use:

- a future activation release as a prerequisite;
- “preparing for release” as an acceptance status;
- complete-contract or release-readiness language for partial evidence.

Recommended issue wording:

> This slice implements `SRC-TRACE-01` and `SRC-TRACE-02` of
> `bicameral.semantic-resolution`. Acceptance is obligation-scoped and does not
> claim complete capability conformance or release activation.

Project metadata should keep these axes separate:

| Field | Meaning | Applies to |
|---|---|---|
| Release train | Where this work physically lands | Implementation issue or PR |
| Capability contract | `bicameral.semantic-resolution` | Issue, PR, and umbrella |
| Obligation IDs | The exact contract subset | Issue and PR |
| Activation release | Where complete capability activation is planned | Release umbrella only |

## Conformance and release binding

A release may activate semantic resolution only when its release-owned
manifest or equivalent reviewed record binds:

1. the exact `bicameral.semantic-resolution` artifact digest;
2. exact reachable subject revisions for every required obligation;
3. complete obligation coverage with no `not_satisfied` result;
4. separate extraction-efficacy and deterministic-kernel evidence;
5. the consuming Product journey's terminal evidence;
6. the release owner's explicit activation decision.

The release binding consumes conformance; it does not create the contract and
does not retroactively rename earlier implementation work.

Consumer specs import the contract by ID, exact digest, and required obligation
IDs. They continue to own their session behavior, display contract, promotion
UX, failure rendering, and UserJourney. If required obligations are not
currently satisfied, the consumer must preserve the governing typed
unavailable, refusal, or unknown behavior rather than claim a weaker path is
semantic resolution.

## State refinement check

Release state: no Product lifecycle behavior changes. This document separates
contract definition, evidence, conformance assessment, and release activation;
it does not make them runtime lifecycle states.

States added or refined: none.

Transitions added: none.

Persistence changes: none. Contract review evidence remains in governed spec
artifacts; implementation evidence remains with its PR or release manifest.

Invariants:

- contract conformance is not Product acceptance;
- obligation evidence is not complete-contract conformance;
- complete-contract conformance is not release activation;
- release activation cannot strengthen stale or partial evidence;
- historical implementation evidence is not current branch conformance;
- consumer journeys cannot be inferred from contract conformance.

Deferred vocabulary:

- `future_ready`;
- `release_prepared`;
- a durable conformance lifecycle;
- a new conformance service or obligation-evidence ledger.

## State minimization review

Rejected states:

- `contract_partially_fulfilled`;
- `contract_ready`;
- `release_prepared`;
- `activation_pending`.

Obligation coverage and the binary per-obligation result are sufficient
evidence attributes. Release planning already owns activation scheduling.
Creating another lifecycle would reproduce the ceremony and release coupling
this contract is intended to remove.

## Authority, privacy, and security

- The contract may classify evidence against named obligations.
- It cannot approve implementation, promote a DecisionCandidate, mutate the
  Decision Ledger, assign a release, or activate a capability.
- Source text and model inputs retain the privacy and egress rules of the
  governing bridge specification.
- Conformance evidence should cite sanitized receipts and exact revisions, not
  raw private prompts or source bodies.
- A release owner cannot waive an unmet obligation by relabeling the evidence.

## Runtime conformance handoff

Runtime Conformance Handoff: not applicable. This document introduces no new
runtime element, relationship, state, transition, persistence, routing, or
failure behavior. The existing bridge and runtime contracts remain the
canonical runtime sources; this document extracts stable obligation IDs and
the release-independent conformance boundary from them.

## ADR implications

- ADR-0029 is reaffirmed: probabilistic extraction, exact projection, and
  stable formal semantics remain separate.
- No new ADR is required for the initial extraction because no runtime
  architecture changes.
- If multiple capability contracts adopt the same release-binding mechanism,
  a later ADR may standardize that mechanism rather than embedding it in each
  capability.

## Acceptance criteria

- The contract identity contains no product release number.
- Every obligation has one stable `SRC-*` ID and a concrete evidence class.
- Tickets can cite an obligation without citing a future activation release.
- Partial conformance cannot be rendered as complete conformance or readiness.
- Historical or reverted implementation evidence cannot be rendered as current
  conformance.
- A release binding must pin the exact contract digest and exact reachable
  implementation revisions.
- Extraction efficacy and deterministic-kernel correctness remain independent
  evidence lanes.
- Consumer-specific session, display, promotion, and UserJourney semantics
  remain in their owning contracts.
- No Product lifecycle state, runtime service, database, or authority path is
  added.

## Validation plan

- Validate links to the governing semantic, runtime, and ADR sources.
- Confirm each `SRC-*` ID is unique.
- Review every obligation against the source bridge's acceptance criteria.
- Run the deterministic spec-governance validation at the exact PR head.
- Review active consumer specs and tickets for future-release prerequisite
  language only after this contract is merged.

No new automated product test is required for this documentation-only
extraction. Implementations retain their existing obligation-specific tests.

## Issue generation notes

This spec does not itself authorize new implementation issues. When existing
issues are migrated:

- preserve their product intent and actual landing release;
- replace future-release prerequisite language with exact `SRC-*` obligations;
- keep responsibility with the existing assignee;
- do not broaden acceptance criteria beyond the issue's current implementation
  slice;
- leave activation and complete-conformance criteria on the future release
  umbrella.

## Open questions

None. Registering the merged Bot artifact and its MCP consumer pin in the
existing Factory semantic-contract pair registry is a mechanical propagation
follow-up, not an unresolved Product decision.

## Governance record

```text
Governance gate: bic:spec-governance-gate; Qor advisory/no-op pending factory
policy revision.
UserJourney: not applicable; the contract extraction adds no user-visible flow.
Journey Boundary Review: not applicable; consuming journeys retain their
existing reviewed boundaries.
```

## Request coverage

| Request | Covered by |
|---|---|
| Extract semantic-resolution behavior from release numbering | Decision and Normative obligations |
| Let earlier work fulfill the contract ahead of activation | Conformance record |
| Remove future-release dependencies from earlier tickets | Ticket and project rules |
| Avoid another release-train conundrum | Conformance and release binding |
| Avoid excessive ceremony | State minimization review |
