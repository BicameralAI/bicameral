# State Machine Resolver v1 runtime architecture amendment 005

**Base contract:** [`state-machine-resolver-v1-runtime-architecture.md`](./state-machine-resolver-v1-runtime-architecture.md)

**Machine amendment:** [`state-machine-resolver-v1-runtime-architecture.contract.amendment-005.json`](./state-machine-resolver-v1-runtime-architecture.contract.amendment-005.json)

**Issue:** [#937](https://github.com/BicameralAI/bicameral-bot/issues/937)

**Owner decision:** 2026-09-04 session — skip observable `Decision(proposed)` on claim-set accept; every current Product participant is Product Owner for this alpha cut.

**Normative status:** this amendment supersedes A23 / section 8's member-creates-`proposed` / recommendation-only window for alpha claim-set acceptance. It also supersedes the alpha reading of ADR-0022 and onboarding auto-join-as-Member-only **for this alpha cut**. Policy Owner remains creator-only. Working-state candidate generations still use `proposed → promoted | rejected`.

## Decision

1. **Skip `proposed` on the claim-set path.** Human confirm of `smr_claim_set_v1` appends `claim_set_candidate_promoted` with `resulting_decision.signoff_state = approved`. Claims are immediately authoritative. There is no recommendation-only envelope and no later `approve_signoff` on this path.
2. **Everyone is Product Owner in alpha.** Every current Product participant holds Product Owner authority for confirm, reject, and supersede. Same-domain auto-join granting Member only is out of this alpha cut; alpha join grants Product Owner. It does not grant Policy Owner.
3. Reject and supersede remain owner transitions against an `approved` Decision.
4. `Decision(proposed)` stays in the enum for history and deferred post-alpha member-recommendation flows. It is not an observable alpha state for claim-set acceptance.

## What this does not change

- HITL remains. Confirm is still a human ConfirmationChallenge v2.
- Cloud is still not canonical.
- #1224 creation/routing Claims are a separate slice and stay as approved.

## Governance record

```text
Governance gate: bic:spec-governance-gate; Qor advisory/no-op pending factory policy revision.
UserJourney: developer_host_mcp_smr_claim_acceptance_v1.
State refinement: Decision(proposed) deferred out of alpha claim-set path; no new lifecycle state added.
Release Unit: not applicable — contract correction; grants no assignment, merge, or implementation authority by itself.
```

This amendment authorizes no implementation, deployment, Release Unit, Product acceptance, topology execution, merge, or release.
