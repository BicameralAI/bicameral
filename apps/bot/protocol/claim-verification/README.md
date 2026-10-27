# Claim verification corpus (Phase 0)

Synthetic gold cases and a metrics harness for issue
[#56](https://github.com/BicameralAI/bicameral/issues/56), under the Phase 0
of ARCH-0006 (proposed in
[#49](https://github.com/BicameralAI/bicameral/pull/49)). They change
no product behavior. They add no endpoint, no protocol type and no dependency.

Correct retrieval does not make a correct answer. The harness scores these
four stages separately, so a high score on one stage cannot hide a failure in
another:

| Score | A case counts as correct when |
|---|---|
| Retrieval | Every gold relevant source is in the retrieved set |
| Authority | The selected authoritative source equals the gold one (both absent also counts) |
| Groundedness | Per claim: the predicted verdict equals the gold verdict |
| Answer accuracy | The claims the answer presents as canonical are exactly the gold `supported` claims |

An answer that presents no canonical claim is an **abstain**.

## Verdicts

The labels follow the Expected column of #56. They live only in the test,
not in `bicameral-api`. ARCH-0006 says the final vocabulary needs a governed
spec; until then this table maps them:

| Corpus verdict | Nearest ARCH-0006 term |
|---|---|
| `supported` | fully supported |
| `contradicted` | refuted |
| `stale` | no direct term (a superseded source is cited) |
| `not_supported` | insufficient support |

## Cases

| Case | #56 row | Gold verdict |
|---|---|---|
| `CV-CONTROL-01` | none (correct control) | `supported` |
| `CV-NEGATION-01` | Negation | `contradicted` |
| `CV-SUPERSEDED-01` | Superseded | `stale` |
| `CV-PARAPHRASE-01` | Paraphrase | `not_supported` |
| `CV-MISSING-01` | Missing evidence | `not_supported`; the answer abstains |
| `CV-TRACEABLE-01` | Traceable | `supported`, with locator, hash and excerpt |

Each source carries `excerpt_sha256`, the SHA-256 of its exact `excerpt` bytes.

## Run

```sh
cd apps/bot
cargo test -p bicameral-api --test claim_verification_conformance -- --nocapture
```

The run prints each predictor's four scores. Two predictors exist today:

- `gold`: the gold answers. It must score full marks; this checks the scorer.
- `citation_trust_baseline`: retrieval is taken from gold, and every claim
  that cites a retrieved source is labeled `supported`. It shows the gap that
  ARCH-0006 names: full retrieval, low groundedness.

The verifier itself (#56 rows marked `BLOCKED`) is added as a new predictor
after ARCH-0006 is accepted.
