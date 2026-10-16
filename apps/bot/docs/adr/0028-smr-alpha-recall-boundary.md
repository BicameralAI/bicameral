# ADR-0028: SMR Alpha Recall Boundary

**Date:** 2026-07-27
**Status:** proposed
**Level:** L1
**Related:** BicameralAI/bicameral-bot#936; ADR-0003, ADR-0006, ADR-0007,
ADR-0010, ADR-0019, ADR-0022, ADR-0024;
`../specs/smr-alpha-runtime-contract.md`;
product why: [`../strategy/why-smr-and-spec-governance.md`](../strategy/why-smr-and-spec-governance.md)

## Decision

Product motivation and the formal-verification framing (underdetermined HITL release valve, contradiction outcomes) are documented in [Why SMR and Spec Governance](../strategy/why-smr-and-spec-governance.md). This ADR owns the alpha **boundary**, not the 5-minute product bet.

For the SMR alpha, Bicameral's product value is controlled specification
cataloguing and reliable recall for supplied work or artifact context. The
resolver determines which product-approved specifications are applicable and
tracked in the searched trusted corpus, and reports missing or uncertain
coverage honestly.

For a Plan-driven preflight, the daemon performs one bounded semantic
projection of the exact Plan/source snapshot. That projection always produces
normalized impact ModelAtoms when analysis succeeds and produces a local
proposed Claim set only for novel or changed governed semantics. Read-only
preflight selects accepted Claims by exact accepted-ModelAtom overlap and
bounded typed relation closure. Accepted bindings, CodeLocator, and optional
CodeGraph evidence may add impact-atom selectors; they are additive recall
routes, not alternate Claim constructors or applicability authorities.

SMR alpha does not determine whether a specification is implemented in code,
whether code complies with a specification, or whether an implementation is
current or correct. It does not emit an implementation-derived absence,
contradiction, all-clear, compliance conclusion, or canonical Product claim.

The core resolver is available without CodeGraph. CodeGraph may discover code
regions or modules that expand recall, and may be attached as advisory recall
provenance. The daemon may invoke the versioned optional
`codegraph.ensure_current` operation defined by the runtime companion
contract. Its absence, staleness, pending state, or unsupported state must
degrade only that optional expansion route; it must not make core SMR recall
unavailable or change the acceptance of a tracked-spec claim.

The separately governed Code Evidence/Compliance lifecycle retains ownership of
graph snapshots, `ScopeSufficiencyProof`, `GraphEvidenceState`, bindings,
currentness, and implementation evidence. Those artifacts must not be named as
SMR-alpha closure or acceptance authority.

## Required SMR Identity

Every SMR alpha result carries these identities:

- exact input artifact/context digest;
- exact trusted-spec-corpus revision;
- exact Plan semantic-projection and impact-atom-set digests;
- exact accepted SMR base position and projection digest;
- recalled Claim/specification identities with contributing accepted
  ModelAtoms and typed selection reasons;
- missing and unknown-coverage findings; and
- when semantic projection stages a candidate, the exact proposed-claim
  digest, retained unchanged if a downstream governed path promotes it.

An optional CodeGraph expansion may additionally carry its snapshot, scope, or
proof identity, materialization job identity, and typed limitation as
provenance. It cannot replace, strengthen, or gate the required SMR identities.

## Unknown-Coverage Contract

SMR returns `unknown_scope` when it cannot honestly establish coverage for a
declared trusted-corpus scope. Required mappings are:

| Condition | SMR result |
|---|---|
| trusted corpus revision cannot be resolved or verified | `unknown_scope` for the affected corpus scope |
| a configured trusted source is unavailable, unsupported, permission-denied, stale, or not indexed | `unknown_scope` for that source or section |
| input context is ambiguous or its digest cannot be derived | `unknown_scope` for the affected requested context |
| recall route is bounded, stopped, or cannot evaluate a declared corpus partition | `unknown_scope` with the skipped partition and stop reason |
| CodeGraph expansion is absent, stale, unsupported, or unavailable | retain core result; attach optional-expansion limitation only |
| optional CodeGraph expansion finds an uncertain region or module | retain core result; add that region as optional expansion uncertainty, not core closure failure |

`unknown_scope` is a coverage disclosure. It is not a graph readiness state, a
code-compliance state, a contradiction, or a new canonical Product lifecycle
state.

## Consequences

- `preflight.run` may render the SMR alpha result through MCP, but MCP remains a
  transport and rendering surface.
- `preflight.run` may automatically invoke optional bounded CodeGraph
  materialization through `codegraph.ensure_current`; no separate graph
  command is part of the alpha user journey.
- A no-match result is limited to the named trusted-corpus revision and searched
  scope. It never means an implementation is clear or compliant.
- CodeGraph-backed recall expansion may be tested independently, but #943's
  alpha topology must also validate a CodeGraph-absent core SMR run.
- A future implementation-compliance product slice needs its own lifecycle,
  authority contract, and acceptance evidence. It cannot be introduced by
  extending SMR wording.

## Supersession And Retained Boundaries

This ADR supersedes prior readings of ADR-0003, ADR-0006, ADR-0007, ADR-0010,
ADR-0019, ADR-0022, ADR-0024, and their dependent recall/preflight contracts
that make SMR or its alpha `preflight.run` path code-compliance, implementation
verification/currentness, CodeGraph-gated closure, or code-derived
absence/all-clear work.

It does not retire their Code Evidence/Compliance rules. Those rules remain
applicable only when that separate lifecycle explicitly owns the request and
the result.

## Audit Disposition

The #936 repository-wide audit found the following authoritative ADR passages:

| ADR | Disposition |
|---|---|
| ADR-0003 | Retained for Code Evidence/Compliance; now explicitly excluded from SMR alpha. |
| ADR-0006 | Retained Ledger View compliance behavior; now explicitly excluded from SMR alpha recall. |
| ADR-0007 | Retained governance and BindingEvidence rules; now explicitly excludes SMR-alpha CodeGraph closure. |
| ADR-0010 | Retained graph evidence contract; now explicitly excludes SMR-alpha closure and all-clear authority. |
| ADR-0019 | Re-scoped to the separate Code Evidence/Compliance preflight lifecycle. |
| ADR-0022 product-owner authority | Inspected: no SMR or CodeGraph closure claim; no change required. |
| ADR-0022 V1 signoff and reviewable context | Reaffirmed: SMR alpha is reviewable specification context, not implementation truth. |
| ADR-0024 librarian recall and correction capture | Removed code applicability/currentness as a correction prerequisite. |
| ADR-0024 signal variables | Re-scoped graph/currentness signals to Code Evidence/Compliance and declares SMR's own identity set. |

Related specification passages are corrected in
`smr-alpha-runtime-contract.md`, `constraint-lookup-recallpacket-contract.md`,
`preflight-impact-to-decision-recall-strategy.md`,
`preflight-search-readiness.md`, and `librarian-recall-readme.md`.

## Non-Goals

- Do not change the Code Evidence/Compliance lifecycle or its graph proof
  semantics.
- Do not make CodeGraph jobs SMR or Product lifecycle states, and do not make
  successful `ensure-current` mandatory for a valid core SMR result.
- Do not permit SMR to accept a claim, report compliance, or resolve a
  contradiction from code evidence.
