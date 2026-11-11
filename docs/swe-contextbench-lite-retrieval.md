# SWE-ContextBench Lite: Past-Context Retrieval

| | |
| --- | --- |
| Benchmark | SWE-ContextBench Lite ([Zhu et al., arXiv:2602.08316](https://arxiv.org/abs/2602.08316)) |
| Runs | 2026-09-23 |
| Status | Internal evaluation by the Bicameral team. Single retrieval pass with 95% bootstrap intervals. Not peer reviewed. |

## Summary

This evaluation asks one question: given a new GitHub issue, does Bicameral's ledger retrieve the past task that the issue depends on? SWE-ContextBench Lite supplies the gold answer, a human-curated dependency between each new issue and a past task. The evaluation measures retrieval only. It does not measure whether an agent then resolves the issue.

- **Reproducibility.** Over five repeated runs, Jev 1.13.0 cited the same stored statements **91%** of the time (mean pairwise Jaccard), compared with **13%** for gpt-4.1-mini with the same 3-candidate shortlist. With the 32-candidate BM25 shortlist, Jev reached 98%.
- **Precision.** With the BM25 shortlist, Jev named the gold past task with **0.89** precision (95% CI 0.78–0.97), against 0.45 (0.36–0.55) for gpt-4.1-mini with a 3-candidate shortlist. The paired difference, +0.43 (+0.31 to +0.55), is the only gap between these two arms whose interval excludes zero. The arms differ in shortlist size; see [Limitations](#limitations).
- **Recall, F1 and top-1.** Between the same two arms, the differences in recall (−0.06), F1 (+0.05) and nDCG@1 (0.00) are within noise. Jev names fewer past tasks and is usually right when it does.
- **Latency.** Median successful query: 2.8 s for Jev, 5.9 s for gpt-4.1-mini.

## Task

Each Lite edge links a new issue to a past task it depends on. There are 100 gold edges over 99 new issues; one issue depends on two past tasks. For each edge, the system receives only the new issue's text and must name the gold past task. Past tasks from the same repository that are not the gold are stored in the index as distractors.

Bicameral stores each past task as a set of extracted statements. A query extracts statements from the new issue. For each statement, the model either reuses a stored statement (chosen from a shortlist of the closest stored statements) or creates a new one. A past task is **named** when the query reuses at least one of its statements.

## Setup

| | |
| --- | --- |
| Dataset | SWE-ContextBench Lite, 100 gold edges ([jiayuanz3/SWEContextBench](https://github.com/jiayuanz3/SWEContextBench)) |
| Stored past task | Issue text, session summary lines, and the final assistant message. Tool output and hidden tests are excluded. |
| Query | New issue text only. Query results are not written back to the index. |
| Index | Gold past tasks plus up to two same-repository distractors per edge: 12 past tasks for edges 1–10, 86 for edges 11–100. |
| Extraction models | Jev 1.13.0 (TypeSafe API); gpt-4.1-mini (OpenAI API) |
| Shortlist | Jaccard top 3 (gpt-4.1-mini; Jev k=3) or BM25 top 32 (Jev k=32) |
| Reference arm | Agent Memory, revision `d86a080`: one shared pool of 95 past tasks; each query returns every past task from the same repository, ranked |
| Retrieval batch | One pass over all 100 edges, one fresh workspace per arm |
| Consistency batch | Five fresh repeats of edges 1–10 per arm |
| Intervals | 95% percentile bootstrap over the 100 edges, 10,000 resamples, paired across arms (seed 20261111) |

## Metrics

- **Recall:** fraction of edges whose gold past task is named.
- **Precision:** fraction of named past tasks that are gold.
- **F1:** harmonic mean of precision and recall.
- **nDCG@k:** rank quality of the gold past task, with named tasks ordered by the number of shared statements.
- **Consistency:** mean pairwise Jaccard similarity of the cited statement IDs across the five repeats (1.0 means every repeat cited the same statements).
- **Latency:** median wall-clock time per successful query or past-task write.

## Results

### Retrieval (100 edges, single pass)

95% bootstrap intervals in parentheses.

| Metric | gpt-4.1-mini (k=3) | Jev (k=3) | Jev (BM25 k=32) | Agent Memory |
| --- | ---: | ---: | ---: | ---: |
| Recall | 0.37 (0.28–0.46) | 0.08 (0.03–0.14) | 0.31 (0.22–0.40) | 1.00\* |
| Precision | 0.45 (0.36–0.55) | 0.80 (0.56–1.00) | **0.89 (0.78–0.97)** | 0.06 (0.05–0.06) |
| F1 | 0.41 (0.32–0.49) | 0.15 (0.06–0.24) | **0.46 (0.35–0.56)** | 0.10 (0.10–0.12) |
| nDCG@1 | 0.31 (0.22–0.40) | 0.08 (0.03–0.14) | 0.31 (0.22–0.40) | **0.45 (0.35–0.55)** |
| nDCG@3 | 0.34 (0.25–0.43) | 0.08 (0.03–0.14) | 0.31 (0.22–0.40) | **0.62 (0.54–0.70)** |
| Fail rate (writes / queries) | 0.00 / 0.00 | 0.00 / 0.00 | 0.00 / 0.00 | 0.00 / – |
| Median ms per query | 5,934 | **2,468** | 2,832 | 20 |
| Median ms per past-task write | 11,266 | **6,880** | 10,995 | – |

\* Agent Memory returns every past task from the query's repository, so the gold is always in the list. Its recall is not comparable to the other arms, which name only past tasks that share a statement with the query.

### Paired differences against gpt-4.1-mini (k=3)

Each difference is resampled on the same edges for both arms. Bold marks intervals that exclude zero.

| Arm minus gpt-4.1-mini | Recall | Precision | F1 | nDCG@1 | nDCG@3 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Jev (BM25 k=32) | −0.06 (−0.16, +0.04) | **+0.43 (+0.31, +0.55)** | +0.05 (−0.05, +0.15) | 0.00 (−0.10, +0.10) | −0.03 (−0.13, +0.07) |
| Jev (k=3) | **−0.29 (−0.40, −0.19)** | **+0.35 (+0.09, +0.60)** | **−0.26 (−0.37, −0.14)** | **−0.23 (−0.33, −0.13)** | **−0.26 (−0.36, −0.16)** |

With the same 3-candidate shortlist, Jev is more precise than gpt-4.1-mini but clearly worse on recall, F1 and ranking. With the 32-candidate shortlist, Jev keeps the precision gain and closes the other gaps to within noise.

### Consistency (edges 1–10, five repeats)

| Mean pairwise Jaccard, cited statement IDs | gpt-4.1-mini (k=3) | Jev (k=3) | Jev (BM25 k=32) |
| --- | ---: | ---: | ---: |
| New-issue queries | 0.13 | 0.91 | **0.98** |
| Past-task writes | 0.06 | 0.84 | **0.91** |

Given the same input, gpt-4.1-mini cited a largely different set of stored statements on each run. Because a past task is named only through cited statements, its retrieval results also change from run to run. Jev's citations were nearly identical across repeats.

## Limitations

- **The shortlist differs between arms.** gpt-4.1-mini was not run with the BM25 top-32 shortlist, so the precision gap between the Jev BM25 arm and gpt-4.1-mini mixes a model effect with a shortlist effect. With the same 3-candidate shortlist, Jev's precision was 0.80 against 0.45, but its recall fell to 0.08 against 0.37.
- **Sample size.** Retrieval was a single pass, so the intervals reflect sampling over edges, not run-to-run variation. Consistency was measured on 10 edges only.
- **Retrieval only.** No downstream patch or test results (FAIL_TO_PASS) are reported.
- **Not independent.** The Bicameral team ran this evaluation. The harness and raw run records are not yet public.

## Citation

Zhu et al. *SWE-ContextBench.* arXiv:2602.08316. Dataset: [github.com/jiayuanz3/SWEContextBench](https://github.com/jiayuanz3/SWEContextBench).
