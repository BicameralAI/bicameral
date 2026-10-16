# State Machine Resolver v1 runtime architecture amendment 003

**Base contract:** [`state-machine-resolver-v1-runtime-architecture.md`](./state-machine-resolver-v1-runtime-architecture.md)

**Machine amendment:** [`state-machine-resolver-v1-runtime-architecture.contract.amendment-003.json`](./state-machine-resolver-v1-runtime-architecture.contract.amendment-003.json)

**Issue:** [#937](https://github.com/BicameralAI/bicameral-bot/issues/937)

**Owner decision:** [issue comment 5246206090](https://github.com/BicameralAI/bicameral-bot/issues/937#issuecomment-5246206090)

**Normative status:** this amendment completes the pre-implementation v1 `AtomRelation` support-lineage contract. It supersedes any proposal to classify arbitrary natural-language relations as `source_explicit`, `rule_derived`, or `model_inferred` without a reproducible derivation witness.

## Decision

Every `AtomRelation` requires:

- one or more deduplicated, stable-ID-sorted `supporting_claim_ids` resolving inside the same `NormalizedClaimSetV1`;
- one or more deduplicated, stable-ID-sorted `supporting_source_anchor_ids` resolving inside the same set; and
- every cited SourceAnchor to be reachable through at least one cited Claim's `source_anchor_ids`.

Missing, foreign, duplicate, unsorted, or unreachable support fails closed as `proposal_identity_mismatch`, names the relation field, and appends nothing.

These references identify the inputs presented for review. They do not prove semantic entailment, source explicitness, correctness, or authority. A model, provider, or extractor may not promote `source_explicit`, `rule_derived`, `model_inferred`, or an equivalent derivation classification into canonical fact.

Human acceptance still binds the exact `claim_set_digest` and remains the sole acceptance transition. This amendment creates no second approval, epistemic, attestation, or relation lifecycle.

## Deferred derivation receipts

A later `proposal_schema_version` may add a derivation receipt when a real deterministic producer can supply a stable rule identifier, rule version, and reproducible input digest. Until then, v1 records support lineage and makes no stronger epistemic claim.

Because SMR has no shipped implementation, Release Unit, or activation, the golden v1 schema and fixtures are corrected in place before their first consumer. Once a shipped consumer exists, closed-member changes require the version bump already mandated by the base contract.

## QA consequences

Conformance requires positive and negative coverage proving that:

1. valid relations round-trip with their exact support identities;
2. missing or empty support lists fail closed;
3. dangling Claim or SourceAnchor identities fail closed;
4. a SourceAnchor not reachable through a supporting Claim fails closed; and
5. duplicate or unsorted support identities fail closed; and
6. provider-supplied `source_explicit`, `rule_derived`, or `model_inferred` fields are rejected as additional properties.

The canonical event-store adapter must preserve these fields byte-for-byte as part of the opaque SMR payload. It does not classify or reinterpret relation support.

Semantic efficacy is a separate QA obligation under [#1089](https://github.com/BicameralAI/bicameral-bot/issues/1089), a child of the Silong-owned [SRC-EFFICACY-01 lane #1006](https://github.com/BicameralAI/bicameral-bot/issues/1006). Schema validity and referential closure do not prove that cited support is semantically sufficient.

## Governance record

```text
Governance gate: bic:spec-governance-gate; Qor advisory/no-op pending factory policy revision.
UserJourney: not applicable — relation support metadata changes no user-visible step, commit point, recovery path, or topology.
Journey Boundary Review: not applicable — the accepted SMR journey and authority boundaries are unchanged.
State refinement: no lifecycle state added; relation support is metadata inside the existing proposed-to-promoted/rejected machine.
Release Unit: proposed as v021-smr-relation-support-contract; proposal records inclusion intent only and grants no assignment, merge, release, or implementation authority.
```

This amendment authorizes no implementation, deployment, Release Unit, Product acceptance, topology execution, merge, or release.
