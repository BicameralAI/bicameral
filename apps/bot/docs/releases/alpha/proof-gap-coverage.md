# P1 Proof-Gap Coverage — Fixture-to-Live Classification

**Issue:** [#364](https://github.com/BicameralAI/bicameral-bot/issues/364)
**Parent:** `docs/releases/alpha/state-machine-release-readiness.md`
**P0 dependency:** [#362](https://github.com/BicameralAI/bicameral-bot/issues/362)
completed — 36 pass, 0 fail, 2 inconclusive.

## Purpose

Classify each supported alpha release loop by its highest-confidence proof
evidence, then close the single highest-risk fixture-only gap with
release-build evidence.

## Proof classifications

Each release loop receives exactly one classification, from strongest to
weakest:

| Rank | Classification | Meaning |
|---|---|---|
| 1 | Proven by release build + footage | Release binary exercised end-to-end with screen capture. |
| 2 | Proven by release build + fixture set | Release binary exercised via CLI/gateway with fixture data and command logs. |
| 3 | Proven by synthetic integrations only | Only synthetic integration records (not real provider data) exercise the path. |
| 4 | Proven by pinned open-source repo only | Only a pinned OSS repo fixture exercises the graph/code path. |
| 5 | Proven by deterministic tests only | Only `cargo test` unit/integration tests cover the path — no release artifact involved. |
| 6 | Not proven | No evidence exists. |

## Coverage table

| Loop | P0 QA scenarios | Deterministic tests | Classification | Gap risk |
|---|---|---|---|---|
| Local capture | 4/4 pass (fixture pack, empty evidence, duplicate, malformed) | `v1_golden_path_e2e`, `v1_true_golden_path_e2e`, `event_store_conformance`, `external_ingest_gateway` | **Proven by release build + fixture set** | Low — P0 exercised all happy and error paths via release binary. |
| Candidate review | 6/6 pass (accept, reject, idempotent, invalid ID, reject-after-promote) | `v1_golden_path_e2e`, `v1_true_golden_path_e2e` | **Proven by release build + fixture set** | Low — full state-transition matrix via gateway. |
| Decision signoff | 6/6 pass (approve, reject, idempotent, invalid ID, reject-after-approve, supersede) | `v1_golden_path_e2e`, `v1_true_golden_path_e2e` | **Proven by release build + fixture set** | Low — signoff and supersede both exercised. |
| Ledger retrieval | 2/2 pass (dashboard/ledger, candidates list) | `v1_golden_path_e2e`, `projection_rebuild_conformance` | **Proven by release build + fixture set** | Low — read models exercised after full ingest→accept→signoff path. |
| Advisory preflight | 1/1 pass (clean workspace) | `progressive_preflight_conformance` (12 tests), `staged_preflight_conformance` (9 tests) | **Proven by release build + fixture set** | Medium — P0 tested only one scenario (clean workspace) via release build. Deterministic tests cover 21 staged/progressive scenarios but those are in-memory. |
| Binding guardrail | 0 in P0 matrix → **3/3 pass in P1 QA** (verified bind, non-verified rejection, inspect) | `binding_dispatch_conformance`, `binding_lineage_conformance`, `snapshot_scoped_binding_conformance`, `graph_governance_bind_conformance` | **Proven by release build + pinned open-source repo** | **Closed by P1** — was the highest-risk gap (deterministic-only); now has release-build evidence against ripgrep fixture. |
| External ingest guardrail | 8/10 pass, 2 inconclusive (content scanners not in alpha) | `external_ingest_gateway` (structural rejection tests) | **Proven by release build + fixture set** | Low — structural authority injection guardrails fully exercised. Content-level scanners (sensitive data, injection canary) are out of alpha scope; `InjectionCanary` and `SensitiveData` variants exist but scanners are inactive. |
| Setup and channel boundary | 5/5 pass (init, re-init, doctor, build, health) | `drive_folder_hardening`, CLI command tests | **Proven by release build + fixture set** | Low — setup exercised from clean state. Channel routing explicitly deferred as named boundary. |

## P1 binding guardrail QA evidence

The binding guardrail was the only loop with zero P0 release-build coverage.
This P1 run closed the gap.

**Build:** `cargo build --release` from `main` at `227e746`, binary version 0.1.4.
**Fixture repo:** ripgrep `https://github.com/BurntSushi/ripgrep` at tag 14.1.1
(`4649aa9700619f94cf9c66876e9549d83420e16c`).

### QA commands

```
bicameral init .                          # in ripgrep checkout
bicameral doctor                          # healthy workspace, git substrate
bicameral ingest qa-binding-test.json     # 1 candidate proposed
POST /api/v1/dashboard/command            # accept_candidate → Decision(proposed)
GET  /api/v1/dashboard/ledger             # decision visible

# Binding with verified evidence → 201 Created
POST /api/v1/bind
  decision_id: 4b38f3a9-08a3-4f48-9809-0d9fa1bae7bf
  proposal.binding_type: file_path
  proposal.target: crates/core/search.rs
  proposal.confidence: high
  proposal.graph_snapshot_id: 4649aa9700619f94cf9c66876e9549d83420e16c
  proposal.validated_sha: 5a268950a174865fd13b9eccbf69cf4686ab84fdfbeb019b0543dbd5a6a69386
  evidence_state: verified
  → HTTP 201, binding_id: 73d46be4-98d0-4e2e-81a7-1d1873c918c5

# Binding with non-verified evidence → rejected
POST /api/v1/bind (evidence_state: unknown_not_indexed)
  → {"status":"rejected","evidence_state":"unknown_not_indexed",
     "detail":"Only verified evidence may materialize bindings; got UnknownNotIndexed"}

# Ledger confirms code_evidence section with verified state
GET /api/v1/dashboard/ledger
  → code_evidence.evidence_state: "verified"
  → code_evidence.graph_snapshot_id: "4649aa9..."
```

### Result

| Scenario | Result |
|---|---|
| Bind with verified graph evidence | **pass** — 201 Created, binding materialized with snapshot provenance |
| Bind with non-verified evidence (unknown_not_indexed) | **pass** — rejected with typed explanation |
| Ledger reflects accepted binding evidence | **pass** — code_evidence section shows verified state |

## Remaining gaps and risk assessment

| Gap | Severity | Status |
|---|---|---|
| Content-level scanners (sensitive data, injection canary) | Low | Out of alpha scope. Structural guardrails pass. Scanner variants exist for future activation. |
| Advisory preflight depth | Medium | P0 tested only clean-workspace preflight against release build. 21 deterministic tests cover deeper scenarios but not via release binary. |
| CLI review surface gap | Medium | No CLI subcommand for `accept_candidate` / `approve_signoff`. Golden path requires gateway. Consistent with ship plan (dashboard is the review surface). |
| Source refresh / revalidation | Low | Deferred command returns empty response (not crash). Not a release loop. |

## Firefighting notes — what product should avoid promising during pilots

1. **Do not promise "full lifecycle from CLI alone."** The accept/reject/signoff
   commands require the gateway dashboard endpoint. CLI-only users cannot
   complete the golden path without `bicameral gateway start`. The ship plan is
   consistent with this (dashboard is the review surface), but pilot
   conversations should not promise CLI-only governance.

2. **Do not promise "content-level injection detection."** The external ingest
   guardrails block structural authority injection (review commands, signoff,
   compliance, policy grants) correctly. However, content-level patterns
   (authority keywords hidden inside evidence text) are not scanned in alpha.
   Pilots should explain: "we block the categories of injection we know about
   structurally; content-level scanning is a planned improvement."

3. **Do not promise "verified code grounding out of the box."** Binding requires
   verified graph evidence from tree-sitter indexing. Pilots who have not
   configured graph support will see `source_only` or `readiness_blocked` states.
   The product correctly reports these honestly, but pilot materials should set
   expectations: "grounding accuracy improves as you add code to the workspace."

4. **Do not promise "preflight will block bad changes."** Advisory preflight
   returns staged context and decision references. It does not block, enforce,
   or mutate. Pilots should hear: "preflight shows what the system knows —
   enforcement is post-alpha."

5. **Do not promise "real-time integration sync."** Synthetic integration records
   exercise the external ingest path, but live provider connections (GitHub,
   Jira, Slack) are in bicameral-integrations and not all are flip-ready. Pilot
   conversations should frame integrations as "we support ingesting from your
   tools" rather than "it syncs automatically."

6. **Do not promise "team-wide conflict detection."** Local alpha reports
   `hosted_required` or `unknown` scope for cross-team claims. Pilots should
   hear: "this release gives you local workspace governance; team-wide visibility
   is the next step."

7. **Treat signoff state as flat.** The `signoff` field in ledger responses is a
   string (`"proposed"`, `"approved"`, `"rejected"`), not a nested object. Pilot
   tooling or scripts that parse signoff state need to handle strings, not
   objects. Firefighting scripts should check `signoff == "approved"` not
   `signoff.state == "approved"`.

8. **Watch for test ordering sensitivity with `supersede_decision`.** Supersede
   permanently overwrites signoff state. QA or pilot demo scripts must verify
   signoff BEFORE applying supersede if both flows are demonstrated on the same
   decision.
