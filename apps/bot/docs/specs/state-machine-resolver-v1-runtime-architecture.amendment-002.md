# State Machine Resolver v1 runtime architecture amendment 002

**Base contract:** [`state-machine-resolver-v1-runtime-architecture.md`](./state-machine-resolver-v1-runtime-architecture.md)

**Machine amendment:** [`state-machine-resolver-v1-runtime-architecture.contract.amendment-002.json`](./state-machine-resolver-v1-runtime-architecture.contract.amendment-002.json)

**Issue:** [#937](https://github.com/BicameralAI/bicameral-bot/issues/937)

**Contract PR:** [#942](https://github.com/BicameralAI/bicameral-bot/pull/942)
**Normative status:** this amendment supersedes conflicting temporal, authority, replay, projection-freshness, ModelAtom-identity, and CodeGraph-composition language in the base contract.

## Relational audit result

The architecture is accepted only with the following invariants. They close bounded counterexamples in which two operations dispose one Candidate generation differently, revoked authority is reused, a stale base is promoted, a stale projection is read, an orphan event is ignored, or equivalent semantics evade overlap through distinct ModelAtom IDs.

## Normative invariants

1. For `(product_id, candidate_id, candidate_generation)`, canonical history contains at most one event from `{claim_set_candidate_promoted, claim_set_candidate_rejected}`. The event store enforces this in one compare-and-append operation; `operation_id` alone is insufficient.
2. Candidate promotion/rejection requires current Product member authority at append. Decision approval/rejection/supersession requires current Product Owner authority at append. Challenge issuance is not durable authority.
3. Promotion atomically verifies the bound `base_event_position` and `smr_base_snapshot_digest` against the current canonical Claim projection. Drift returns `base_snapshot_drift` and appends nothing.
4. A successful SMR read requires the SQLite projection cursor to equal canonical event head. The daemon catches up before reading; failure returns `model_unavailable`, never stale or empty fallback.
5. Replay fails with `invalid_canonical_history` when a Candidate or Decision predecessor required by an event is missing. Silently ignoring an orphan event is prohibited.
6. The daemon verifies each ModelAtom identity from normalized `(atom_kind, semantic_key, semantic scope)`. Extractor-supplied identity is advisory. Semantic aliasing returns `proposal_identity_mismatch`.
7. Product member promotion makes the exact Claims canonical and recommendation-only under `Decision(proposed)`. Product Owner approval raises them to authoritative. Owner rejection or supersession preserves immutable history but excludes the Claims from the effective current projection.
8. CodeGraph is optional recall expansion only. It is never required for core SMR closure, never determines whether a specification was implemented, and never supplies code-compliance or Claim authority.

## Journey boundary

The specification-level Journey Boundary Review passes: responsible actor, commit point, independently observable facts, partial failures, retries, forbidden recovery, and Product-state-versus-witness distinctions are now explicit in the base contract and [#943](https://github.com/BicameralAI/bicameral-bot/issues/943).

Executable catalog registration and terminal real-host evidence remain delegated to #943. They are implementation and validation prerequisites, not unresolved Product semantics and not evidence that this contract itself authorizes implementation or release.

## Governance record

```text
Governance gate: bic:spec-governance-gate; Qor advisory/no-op pending factory policy revision.
UserJourney: developer_host_mcp_smr_claim_acceptance_v1.
Journey Boundary Review: passed for the specification boundary; catalog/profile execution delegated to #943.
State minimization: no parallel SMR approval lifecycle and no titled Decision entity introduced.
```

This amendment authorizes no implementation, deployment, Release Unit, Product acceptance, topology execution, merge, or release.
