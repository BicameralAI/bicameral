# State Machine Resolver v1 runtime architecture amendment 004

**Base contract:** [`state-machine-resolver-v1-runtime-architecture.md`](./state-machine-resolver-v1-runtime-architecture.md)

**Machine amendment:** [`state-machine-resolver-v1-runtime-architecture.contract.amendment-004.json`](./state-machine-resolver-v1-runtime-architecture.contract.amendment-004.json)

**Issue:** [#937](https://github.com/BicameralAI/bicameral-bot/issues/937)

**Follow-up:** [#1221](https://github.com/BicameralAI/bicameral-bot/issues/1221) seed/replay identity (does not implement live J2)

**Normative status:** this amendment supersedes the Candidate-generation predecessor clause of amendment 002 invariant 5 / A21. Decision signoff and supersession still require an eligible Decision predecessor. Live J2 append-time candidate checks in section 11.3 are unchanged.

## Decision

Do not add a canonical `claim_set_candidate_generated` event.

Candidate generation remains daemon-local working state created by `smr.claims.propose` (canonical write: none). At live J2 append the daemon still verifies the exact local `(product_id, candidate_id, generation)`, current base, current Product member authority, and ConfirmationChallenge v2, then writes one self-contained `claim_set_candidate_promoted` or `claim_set_candidate_rejected` event.

Replay reconstructs accepted Claims from that event payload. It does not look for a generated-candidate predecessor. Unreviewed local candidates may be deleted with SQLite and must not be required to exist as Product history.

A21's fail-closed intent is preserved:

- a second terminal event for one `(product_id, candidate_id, candidate_generation)` is `invalid_canonical_history` on replay;
- signoff and supersession without an eligible Decision predecessor is `invalid_canonical_history`;
- silently ignoring an orphan signoff/supersession event remains non-conformant.

A throwaway-stream fixture may inject a self-contained promotion or rejection event for replay after named owner review. That path uses the same replay consumer. It is not live J2 and does not skip ConfirmationChallenge on a real Product.

## Why this is not a second acceptance family

J2 remains the only live path that admits Claims into an accepted Product model. The human confirm is still the resolution moment. Self-contained replay means the canonical event already carries the accepted set; it does not mean production may append without HITL.

## Governance record

```text
Governance gate: bic:spec-governance-gate; Qor advisory/no-op pending factory policy revision.
UserJourney: developer_host_mcp_smr_claim_acceptance_v1.
Journey Boundary Review: passed for the specification boundary; this amendment restores the already-locked J2 commit point (propose writes nothing canonical; confirm appends). Catalog/profile execution remains delegated to #943.
State refinement: no lifecycle state added; `claim_set_candidate_generated` is rejected, not introduced.
Release Unit: not applicable — non-shipping contract correction; grants no assignment, merge, release, or implementation authority.
```

This amendment authorizes no implementation, deployment, Release Unit, Product acceptance, topology execution, merge, or release.
