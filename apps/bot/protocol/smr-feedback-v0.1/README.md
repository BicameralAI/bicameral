# SMR Feedback v0.1 Seed

This directory holds the fixture for bot #1221. It packages the twelve
owner-approved Product-creation and workspace-routing statements as one parked
`GovernedDomain` with `is_source=false` and a partial named scope.

The fixture is only an in-memory throwaway replay input. It is not Product
truth, a canonical event, a persisted domain, or an `AcceptedSmrBaselineV1`.

## Validate The Review Package

```sh
python3 scripts/validate_smr_feedback_seed_review.py
python3 -m unittest scripts.tests.test_validate_smr_feedback_seed_review
```

The validator recomputes all 27 content-addressed ModelAtom IDs and the exact
Claim-set digest, checks every Claim, assertion, relation, scope, and source
reference, then replays an isolated copy in memory. Negative controls prevent
the historical markdown, graph JSON, `is_source=true` domains, or legacy
candidate/promotion envelopes from being treated as the board.

## Admission Sequence

For the current experiment:

1. Load the exact `GovernedDomain` fixture.
2. Validate `is_source=false`, the approved V12/V13 identity rules, the partial
   scope, and the authority exclusions.
3. Replay an isolated copy of its normalized Claim projection in memory.
4. After owner review of these exact bytes, bind #1177 to this fixture identity.

The evaluator never accepts, appends, or persists Product state. Changing the
fixture, Claim-set digest, scope, exclusions, or review evidence requires a new
owner review before a labeled benchmark run.

The adjacent `*.fixture-pin.json` records the exact reviewed file, Claim-set,
analysis, scope, and review-commit identities used by both the Rust evaluator
and Python scorer. New fixture packages get a new pin; callers cannot substitute
new bytes while retaining an old benchmark identity.

## Current Holds

- The v2 runtime still needs a governed `GovernedDomain` schema and validator.
- `governed_domain_admitted` has no live event type or replay consumer.
- The exact domain-shaped fixture and Claim-set digest were owner-reviewed and
  merged in #1229. Independent annotation and adjudication for #1177 remain
  pending and are not a runtime-readiness gate.
