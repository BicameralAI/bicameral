# Alpha State-Machine Release Readiness

This release is governed by **release loops**: closed, user-visible state-machine
slices that start from a real user action, move through canonical or advisory
state transitions, and end in either useful product state or an explicit
unsupported/recovery state.

A release loop is the state-machine analogue of a feature. A feature can be a
surface promise; a release loop is stricter. It says what state exists, which
transitions are supported, what must not become canonical, and what QA evidence
proves the loop worked.

Governance gate: `bic:spec-governance-gate` advisory/no-op; Qor integration
disabled pending factory policy revision.

## Supported Release Loops

| Loop | Supported path | Terminal state | Release note |
|---|---|---|---|
| Local capture | Local surface submits a governed `ToolRequest`; daemon validates authority and records source-backed evidence or candidates. | Evidence visible or candidate proposed. | Edges propose; daemon owns canonical writes. |
| Candidate review | Reviewer accepts or rejects a `DecisionCandidate`. | Accepted candidate becomes a `Decision`; rejected candidate stays non-canonical. | Acceptance is promotion, not signoff or compliance. |
| Decision signoff | Reviewer approves or rejects signoff for an accepted `Decision`. | Decision has signoff state, or signoff is rejected. | Signoff is ownership state, not enforcement. |
| Ledger retrieval | User views history, search, and ledger context for accepted Decisions and cited evidence. | Decision and evidence are visible in read models. | Read models explain authority; they are not authority. |
| Advisory preflight | Preflight returns staged advisory context and decision references. | Advisory result returned without event mutation. | Not a blocking enforcement path in this release. |
| Binding guardrail | `binding.create` accepts only verified graph evidence for binding materialization. | Accepted binding evidence or typed rejection. | Hints and unverified graph state do not become binding authority. |
| External ingest guardrail | External envelopes submit source/evidence/candidate material only. | Evidence/candidate material accepted, or forbidden authority rejected. | External systems cannot inject review, signoff, policy, or compliance authority. |
| Setup and channel boundary | User can initialize, diagnose, open dashboard, create a workspace, and run without durable `ChannelRoute` config. | Workspace operational with channel routing explicitly deferred. | Stage 1 ships "no ChannelRoute" as a named boundary. |

## Not Release Promises

- Durable `ChannelRoute` configuration.
- Blocking enforcement or compliance resolution.
- Hosted/team graph authority beyond the locally verified paths.
- Full source refresh, repair, and revalidation across all providers.
- Token telemetry, tracking commands, and recurring integration ingestion.
- Provider-discovery join flows beyond the current setup boundary.

## QA Artifact Contract

Each release loop should have one attached QA issue. The acceptance artifact is:

- Devin run link or run identifier.
- Footage showing the loop end to end.
- Release commit or build artifact under test.
- Exact setup path and commands used.
- Pass/fail result for supported, rejected, and deferred states.
- Firefighting notes: what failed, what was confusing, and what product should
  watch during pilot usage.

QA issues should be filed with `.github/ISSUE_TEMPLATE/release_loop_qa.yml`.

Attached execution issues:

- P0: [#362](https://github.com/BicameralAI/bicameral-bot/issues/362)
  proves the alpha decision lifecycle release matrix.
- P1: [#364](https://github.com/BicameralAI/bicameral-bot/issues/364)
  burns down fixture-to-live proof gaps.
- P2: [#363](https://github.com/BicameralAI/bicameral-bot/issues/363)
  captures deferred state-refinement backlog work.

## Priority Followups

**P0: Release acceptance matrix.** Run every supported release loop from a clean
install artifact and attach footage. The release is not "full lifecycle ready"
until this matrix proves setup, capture, review, signoff, ledger, advisory
preflight, binding, and external-ingest guardrails together.

**P1: Fixture-to-live gap.** Mark which loops are proven only by deterministic
tests or fixtures, then replace the highest-risk fixture coverage with real
release-build QA.

**P2: State refinement backlog.** Promote deferred states only when the current
loop has a precise place to hold the complexity. `ChannelRoute`, source refresh,
provider join, telemetry, and compliance should each refine an existing loop
instead of becoming parallel vocabulary.

## Firefighting Notes

- Watch for language drift where visible, reviewable, accepted, signed off,
  compliant, routed, and persistent are treated as the same state.
- Treat "preflight works" carefully: this release supports advisory responses,
  not mutation or blocking enforcement.
- Treat "ChannelRoute" carefully: this release supports operating without
  durable channel routing, not configuring it.
- Distinguish test proof from pilot proof. A passing in-memory or fixture test
  is not the same as a clean release artifact with footage.
- Keep rejected/deferred states visible. A typed unsupported response is part of
  the release loop, while a crash or silent no-op is not.

## P0 QA Release Matrix — #362

**Build:** `cargo build --release` from main at `227e746`, binary version 0.1.4.
**QA environment:** `BicameralAI/bicameral-qa-environment` at `9eb919a`.
**Fixture pack:** `fixtures/release-alpha/qa-fixture-pack.json`.
**Open-source repo fixture:** ripgrep `https://github.com/BurntSushi/ripgrep` at
`0e8390a66fbcf6eeac1aeb0541b367663a597c79` (refs/tags/14.1.1).

### Synthetic integrations bundle

| Source type | Fixture file | Records |
|---|---|---|
| GitHub issues | `integrations/github-issues.json` | 2 (search ordering, ChannelRoute deferred) |
| GitHub PRs | `integrations/github-prs.json` | 1 (regex fallback docs) |
| Chat threads | `integrations/chat-threads.json` | 1 (evidence ≠ accepted decision) |
| Meeting transcripts | `integrations/meeting-transcripts.json` | 1 (advisory preflight boundary) |
| Tool outputs | `integrations/tool-outputs.json` | 2 (doctor healthy, preflight unsupported) |

### Setup commands

```
bicameral init .
bicameral doctor
bicameral ingest <fixture>.json            # ×4 (search-ordering, channelroute-deferred, visible-not-accepted, advisory-preflight)
bicameral review list
bicameral review submit <candidate-id>     # ×2 (governance routes to NeedsReview)
bicameral preflight .
bicameral gateway start
curl POST /api/v1/dashboard/command  accept_candidate
curl POST /api/v1/dashboard/command  approve_signoff
curl GET  /api/v1/dashboard/ledger
curl GET  /api/v1/candidates
curl POST /api/v1/external-ingest           # valid evidence + authority injection attempt
curl GET  /health
```

### Release loop results — comprehensive matrix (38 scenarios)

**QA tree issue:** [#366](https://github.com/BicameralAI/bicameral-bot/issues/366)
**QA logs:** [bicameral-qa-environment/qa-artifacts/run-362-1781499729](https://github.com/BicameralAI/bicameral-qa-environment/pull/1)
**Result:** 36 pass, 0 fail, 2 inconclusive

```
Installation & Setup
├── init
│   ├── fresh workspace init ✓
│   └── re-init existing workspace ✓
├── doctor
│   └── healthy workspace ✓
├── build verification ✓
├── health endpoint ✓
│
├── Local Capture (ingest.submit_local)
│   ├── valid fixture pack (4 candidates) ✓
│   ├── empty evidence array ✓
│   ├── duplicate ingest ✓
│   └── malformed JSON payload ✓
│
├── Candidate Review
│   ├── accept_candidate
│   │   ├── single candidate (Proposed → Promoted) ✓
│   │   ├── already-promoted candidate (idempotent) ✓
│   │   └── non-existent candidate ID (typed error) ✓
│   ├── reject_candidate
│   │   ├── single candidate (Proposed → Rejected) ✓
│   │   ├── already-rejected candidate (idempotent) ✓
│   │   └── reject after promote (invalid transition) ✓
│   └── governance engine
│       └── review list (CLI) ✓
│
├── Decision Signoff
│   ├── approve_signoff
│   │   ├── valid decision (Proposed → Approved) ✓
│   │   ├── already-approved (idempotent) ✓
│   │   └── non-existent decision ID (typed error) ✓
│   ├── reject_signoff
│   │   ├── valid decision (Proposed → Rejected) ✓
│   │   └── reject after approve ✓
│   └── supersede_decision ✓
│
├── Ledger Retrieval
│   ├── dashboard/ledger ✓
│   └── candidates list ✓
│
├── Advisory Preflight
│   └── clean workspace ✓
│
├── External Ingest Guardrails
│   ├── valid ExternalIngestEnvelope ✓
│   ├── empty evidence → EmptyEvidence rejection ✓
│   ├── authority injection → AuthorityInjection rejection ✓
│   ├── signoff injection → SignoffInjection rejection ✓
│   ├── compliance injection → ComplianceInjection rejection ✓
│   ├── review command injection → ReviewCommandInjection rejection ✓
│   ├── policy grant injection → PolicyGrantInjection rejection ✓
│   ├── malformed envelope → InvalidPayload rejection ✓
│   ├── sensitive data in payload ? (scanner not in alpha)
│   └── injection canary in payload ? (content-level canary not in alpha)
│
├── Deferred Commands
│   ├── resolve_compliance ✓
│   └── refresh_source ✓
│
└── Health & Capability
    └── capability handshake ✓
```

### Inconclusive scenarios

| Scenario | Reason |
|---|---|
| External ingest > sensitive data in payload | No content-level sensitive-data scanner in alpha. Structural field-injection guardrails work correctly. |
| External ingest > injection canary in payload | Content-level injection canary detection not implemented in alpha. `InjectionCanary` enum variant exists but scanner is not active. |

### Deferred/unsupported state handling

| State | Result | Behavior |
|---|---|---|
| `resolve_compliance` | **deferred** | Dashboard command returns empty response, no crash, no event mutation. |
| `refresh_source` | **deferred** | Dashboard command returns empty response, no crash. |
| Durable ChannelRoute | **deferred** | Doctor reports `channel_route: deferred`; no silent routing. |
| Preflight enforcement | **deferred** | CLI preflight returns advisory only; no blocking behavior. |

### Deterministic test results

```
cargo test --workspace: 694 passed, 0 failed, 0 ignored
cargo clippy --workspace -- -D warnings: clean
```

### Firefighting notes

- **CLI review surface gap:** No CLI subcommand for `accept_candidate` or `approve_signoff`.
  Full accept/signoff lifecycle requires the gateway dashboard endpoint. Consistent
  with the ship plan (dashboard is the review surface), but CLI-only users cannot
  complete the golden path without the gateway running.
- **Signoff state is flat string:** `signoff` field in ledger response is a string
  (`"proposed"`, `"approved"`, etc.), not a nested object. QA scripts must parse accordingly.
- **ExternalIngestEnvelope format:** Requires `source_uri` + `content` + `evidence[].excerpt` —
  not `source_ref`/`kind`/`title`/`body`. Documentation should clarify the contract.
- **Test ordering sensitivity:** `supersede_decision` overwrites signoff state permanently.
  QA scripts must verify signoff BEFORE applying supersede if testing both flows on
  the same decision.
- **Empty evidence accepted on local ingest:** `POST /api/v1/ingest` with empty evidence
  array returns HTTP 200 (not rejected). Differs from external ingest which rejects empty
  evidence. May be intentional (local = trusted) but should be documented.
- **Content-level canary gap:** External ingest structural guardrails correctly block
  authority *field* injection. Content-level canary patterns (authority keywords in evidence
  text) are an improvement opportunity but not a release blocker.
