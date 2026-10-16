# D3 consolidated delivery

PR #886 is the canonical Item 5 / Factory D3 delivery against `main`.

## Included lifecycle

1. Screened external evidence is stored in the daemon-local content-addressed repository before projection.
2. Canonical `EvidenceAccepted` records the accepted source fact independently of candidate count or projection outcome.
3. Projection requests, leases, retries, outcomes, convergence, and unpromoted candidates live in daemon-local SQLite working state.
4. Candidate hints and drafts remain advisory; deterministic content-aware evaluation decides whether accepted evidence is decision-like.
5. Session hiding is transient. Suppression is durable only in local working state and invalidates when its evidence identity changes or expires.
6. Human promotion embeds the reviewed candidate snapshot in the canonical event and creates the Decision without relying on legacy candidate-bearing ingest events.
7. Canonical replay never invokes a projection provider, schedules work, or reconstructs local candidates.
8. Deleting local working state does not alter canonical evidence history, and separate local stores may expose different candidate sets from the same Product history.

## Authority boundary

- Integrations owns acquisition, normalization, sanitation proof, and delivery state.
- Bot owns evidence admission, durable evidence storage, projection working state, candidate exposure, and promotion semantics.
- Projection providers, hints, adapters, and Cloud receive no canonical Decision, review, policy, signoff, or lifecycle authority.
- Evidence relevance remains fail-open after hard privacy and admission checks pass.

## Supersession

After this PR merges, stacked implementation PRs #874, #875, #876, #879, #880, and #884 are historical slices and should be closed as superseded. PR #898 remains subject to a final unique-delta check before closure.
