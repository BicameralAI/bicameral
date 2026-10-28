# Claim verification corpus (Phase 0)

Synthetic gold cases and a metrics harness for issue
[#56](https://github.com/BicameralAI/bicameral/issues/56). This is test-only
measurement. It changes no product behavior. It adds no endpoint, no protocol
type and no dependency.

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
not in `bicameral-api`. The cases are author-written fixtures, not a
human-approved gold set.

| Corpus verdict | Meaning |
|---|---|
| `supported` | The cited approved source states the claim |
| `contradicted` | The cited approved source states the opposite |
| `stale` | The claim cites a superseded source |
| `not_supported` | No cited source states the claim |

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
  that cites a retrieved source is labeled `supported`. It shows the gap
  between full retrieval and low groundedness.

ARCH-0006 (claim-level verifier) was rejected in
[#49](https://github.com/BicameralAI/bicameral/pull/49). A future verifier
needs a new architecture record backed by real misstatement cases. It would
join this harness as a new predictor.
