# SMR Alpha Minimal Honest Test Harness

## Metadata

| Field | Value |
|---|---|
| Date | 2026-07-29 |
| Status | Prototype design; not a production runtime contract |
| Question | Can a real model preserve a version-bound Factory proposal as a source-grounded hypergraph, and can a real Alloy run expose bounded semantic gaps without authority laundering or false closure? |
| Governing bridge | [`smr-alpha-claim-semantic-trace-alloy-bridge.md`](smr-alpha-claim-semantic-trace-alloy-bridge.md) |
| Governing design | [`smr-alpha-managed-preflight-technical-design.md`](smr-alpha-managed-preflight-technical-design.md) |
| External fixture | [`smr-factory-rework-v1`](https://github.com/BicameralAI/bicameral-qa-environment/tree/282920b8f3371ce0ea8eef20790a37765b22e2ee/fixtures/smr-factory-rework-v1) |
| Fixture repository pin | `BicameralAI/bicameral-qa-environment@282920b8f3371ce0ea8eef20790a37765b22e2ee` |
| Governance | `bic:spec-governance-gate`; Qor advisory/no-op |
| UserJourney | Not applicable: disposable offline experiment; no Product state, authority, persistence, or released-host journey |

## 1. Decision

Build one disposable, in-memory command-line harness around three real
boundaries:

```text
version-bound Factory proposal
  -> real specialized model call
  -> ClaimSemanticTraceV1
  -> deterministic validation
  -> complete exact bounded hypergraph snapshot
  -> real pinned Alloy 6.2.0 execution
  -> provenance-linked result
  -> evaluator-only scoring
```

Do not simulate the model with fixture atoms. Do not simulate Alloy with a
Python contradiction checker.

Use fixture annotations only after the run, in the evaluator, to determine
which stage failed.

## 2. What the harness is allowed to prove

The harness may answer:

- whether the model preserves actor, action, target, modality, polarity,
  state, guard, order, branch, exception, alternatives, and unknowns;
- whether every proposed assertion selects a valid immutable source-range ID
  and deterministic validation maps it to exact source bytes and spans;
- whether deterministic validation rejects malformed or invented structure;
- whether the complete supported hypergraph round-trips into an exact Alloy
  snapshot without omission or duplication;
- whether stable Alloy checks return the expected bounded status and minimal
  witness;
- whether every finding maps back to trace assertions and exact source spans;
- whether the system returns `unknown_scope` instead of inventing closure;
- which errors came from extraction and which came from formalization.

It may not prove:

- authentic host Plan-turn capture;
- general natural-language understanding;
- Product-wide semantic completeness;
- code compliance or CodeGraph sufficiency;
- unbounded correctness;
- Candidate Promotion, replay, SQLite, MCP, or release behavior;
- production latency or efficacy from a smoke run.

## 3. Use a real version-bound Factory proposal

The primary Plan-shaped input is the exact proposal artifact from Factory PR
[#391](https://github.com/BicameralAI/bicameral-factory/pull/391):

```yaml
VersionBoundPlanEnvelopeV1:
  source_kind: version_bound_proposal_v1
  repository: BicameralAI/bicameral-factory
  base_commit: 41536ea7658aaa6d1efe0725a055eaaad90fb3df
  head_commit: cf9b989b11556eed8ca99c91a4feb3f29238a162
  artifact_path: docs/specs/version-staging-topology-orchestration-runtime-architecture.md
  base_blob: bae2adfd926e065db94828089e99f8c85d472209
  head_blob: 4493a85f8bc0d623f0ce0e62e275f19675fec334
  base_content_digest: sha256:95a8c53b4ab9581b955773f7412dfda4a10194211354410eccf6323d691a46b2
  head_content_digest: sha256:e9e4db2d1b4381e028fa3700e8197eef7db9c3ba1af061c36d79bc0867ae42aa
  head_bytes: exact fetched bytes
  changed_ranges: deterministic base-to-head new-side ranges
```

The provider receives the complete head artifact for reference, but only
assertions supported by `changed_ranges` may become proposed assertions.
Unchanged text may resolve vocabulary or references; it cannot silently become
new Plan intent.

This is deliberately not called `PlanningContextSnapshotV1`. The repository
artifact is a faithful source-bound proposal for testing the semantic bridge,
but it does not validate the released host capture seam.

The harness verifies repository, commit, path, blob, and byte digest before
calling the model. A mismatch is `inconclusive`, never a best-effort run.

## 4. Accepted baseline

Freeze one evaluator-reviewed `AcceptedModelSnapshotV1` from the Factory base
revision. It contains only Claims treated as accepted before the proposal:

```text
base revision sources
  -> evaluator-reviewed ModelAtoms
  -> ClaimAssertions
  -> Claim membership hyperedges
  -> AtomRelations
  -> explicit closure Claims, where justified
```

This is not cheating. Managed Preflight assumes an accepted SMR hypergraph
already exists; candidate Plan extraction is the operation under test.

The evaluated provider may see accepted vocabulary and stable atom identities.
It must not see:

- fixture `model`;
- fixture `expected`;
- `answer_key_refs`;
- cleaning notes;
- later repair commits;
- evaluator mappings or scoring rules tied to the case.

## 5. Two paired executions

Every case runs two paths against the same stable Alloy model and checks.

### 5.1 Oracle-trace path

```text
held-out curated semantic trace
  -> validator
  -> exact serializer
  -> Alloy
  -> result
```

Purpose: establish that the representation, serializer, and Alloy checks can
express the expected result.

### 5.2 Model-trace path

```text
exact version-bound proposal
  -> real model
  -> validator
  -> exact serializer
  -> the same Alloy files and check
  -> result
```

Purpose: measure the full NL-to-hypergraph-to-Alloy path.

Interpretation:

| Oracle trace | Model trace | Meaning |
|---|---|---|
| fails | any | Alloy model, checker, fixture mapping, or serializer is invalid |
| passes | fails validation | provider produced malformed or ungrounded structure |
| passes | validates, wrong finding | semantic extraction or alignment lost meaning |
| passes | passes | one bounded case succeeded |

The oracle path is not included in model efficacy scoring.

## 6. Minimal case set

### 6.1 Product-shaped primary case

Use PR #391's version-bound proposal and its base revision as the primary
source. Score changed-range extraction, state distinction, transition
relations, unknowns, exact serialization, and bounded Alloy findings.

Historical implementation observations may be used only in a separately
labelled retrospective run. They must not be smuggled into the accepted
Product model.

### 6.2 Retrospective corpus anchors

Run four existing fixture cases to prevent the harness from fitting one
proposal:

| Case | Required role | Claim boundary |
|---|---|---|
| `release-assignment-v3-reachability` | positive relational stress | retrospective representation test; implementation observations mean it is not pure alpha efficacy |
| `governance-result-after-merge` | positive precondition stress | retrospective state/observation test |
| `agent-judgment-authority-unknown` | unknown-scope control | must not turn observation into an accepted authority rule |
| `gcloud-beta-toolchain-negative` | negative control | must not claim every operational repair is an SMR finding |

Exclude `nonshipping-release-unit-admission` from the alpha decision set. Its
fixture explicitly requires future CodeGraph sufficiency review before
absence-based scoring.

Exclude the fixture's keystroke/event adversarial cases. They test host
protocol temporal behavior, which this harness intentionally abstracts away.

## 7. Minimal runtime

```text
Python coordinator
  ├── pinned-source fetcher / fixture materializer
  ├── isolated model adapter
  ├── ClaimSemanticTraceV1 schema validator
  ├── pure exact-snapshot serializer
  ├── pinned Alloy CLI adapter
  └── evaluator
```

No daemon, SQLite, MCP, browser, dashboard, queue, or canonical event store.
State lives in one temporary directory and is deleted by default.

The provider adapter should reuse the repository's existing isolated
`codex exec` experiment pattern:

- fresh session;
- temporary non-repository working directory;
- read-only sandbox;
- no approval prompts;
- strict JSON schema;
- explicit exact model identity; aliases such as `default` or `latest` are
  invalid for a scored run;
- raw event stream and prompt body deleted after normalized receipt creation.

The adapter boundary must remain provider-neutral enough to substitute a local
OpenAI-compatible or other specialized model command without changing
evaluation semantics.

## 8. Real Alloy execution

Use the fixture's pinned toolchain:

```yaml
alloy:
  version: 6.2.0
  tag_commit: 59ba2033993449d483d54acad0e11a7bbf20354f
  artifact: org.alloytools.alloy.dist.jar
  sha256: 6b8c1cb5bc93bedfc7c61435c4e1ab6e688a242dc702a394628d9a9801edb78d
  size_bytes: 21062377
```

The coordinator:

1. verifies the JAR size and SHA-256;
2. writes or constructs exact snapshot data;
3. loads stable `smr-meta-model.als` and `smr-checks.als`;
4. selects one named check from a fixed case manifest;
5. runs Alloy with pinned solver, timeout, memory, and exact scope;
6. parses the Alloy receipt and witness;
7. rejects undeclared symbols, missing provenance, scope drift, or malformed
   results.

The model never emits Alloy, selects the check, or chooses the scope.

If the complete supported snapshot exceeds the alpha capacity, the run returns
`capacity_exceeded`. It must not slice, truncate, or silently increase scope.

## 9. Run contract

One command should run the experiment:

```text
python3 run.py \
  --fixture-root <materialized-smr-factory-rework-v1> \
  --case release-assignment-v3-reachability \
  --profile smoke \
  --provider-command <isolated-provider-command> \
  --alloy-jar <verified-alloy-6.2.0-jar> \
  --output <temporary-result.json>
```

Profiles:

| Profile | Repetitions | Permitted claim |
|---|---:|---|
| `smoke` | 1 | plumbing only |
| `pilot` | 3 | variance and design diagnosis |
| `efficacy` | 10 | minimum profile for comparative rate claims |

The command emits one normalized JSON result and no database.

## 10. Result contract

```text
HarnessRunResultV1 {
  run_identity
  source_receipt
  provider_receipt
  trace_validation
  trace_metrics
  snapshot_receipt
  alloy_request
  alloy_result
  reverse_provenance
  end_to_end_metrics
  limitations[]
  status:
    completed
    inconclusive
    capacity_exceeded
    timed_out
    failed
}
```

Raw prompt, raw provider event stream, source credentials, and unredacted
private source bytes are not stored in the result.

## 11. Stage-specific metrics

### Trace

- state-distinction loss;
- assertion recall and precision;
- relation/branch recall;
- actor/action/target/modality/polarity/state/guard preservation;
- source-range selection and deterministic exact-span validity;
- invented-source count;
- unresolved/alternative preservation;
- authority-class escalation count.

### Snapshot

- supported input record count versus serialized record count;
- missing, duplicate, or invented record count;
- stable-ID and provenance round-trip;
- exact cardinality agreement.

### Alloy

- expected bounded status;
- contradiction recall and precision;
- minimal-witness accuracy;
- unknown-scope accuracy;
- negative-control accuracy;
- witness-to-source reverse-provenance completeness.

### End to end

Primary safety metric:

> A false-safe occurs when a consequential source mutation is erased,
> mistranslated, omitted, or laundered so the result shows neither the expected
> bounded gap nor an explicit limitation.

Also record model latency, Alloy latency, token usage, and run-to-run semantic
variance. Speed is diagnostic, not the alpha decision criterion.

## 12. Initial decision gate

Before the bridge design is promoted from plausible to implementation-ready:

- all oracle-trace cases produce the expected bounded status;
- all supported records serialize exactly once;
- every Alloy witness round-trips to source IDs and spans;
- the pilot has zero known authority escalations;
- the pilot has zero known false-safe outcomes on critical positive cases;
- unknown-scope and negative controls are correct in every pilot repetition;
- no model output contains executable Alloy;
- no run reads an answer key before result finalization;
- capacity, timeout, malformed output, and missing closure remain explicit
  limitations.

Failure does not automatically reject the product direction. It identifies
which boundary—trace vocabulary, validator, accepted baseline, Alloy model,
check, or scope—is not yet honest enough for alpha.

## 13. Throwaway boundary

The coordinator and terminal shell are prototype code. Delete them after the
experiment.

The durable outputs are:

- the validated trace schema;
- stable Alloy meta-model and checks if they survive review;
- exact serializer invariants;
- adversarial fixtures;
- measured result and decision notes;
- any amendment to the governing bridge spec.

Do not promote the harness coordinator into the daemon by accretion.

## 14. Governance and release status

State refinement:

- Product states added: none.
- Canonical mutation: none.
- Persistence: temporary experiment artifacts only.
- Authority: fixture annotations are evaluator evidence; model traces and
  Alloy results are nonauthoritative.
- Journey Boundary Review: not applicable.
- Release Unit: not applicable to the disposable harness design. Any retained
  runtime implementation requires its own governed implementation slice and
factory attestation.

## 15. Measured smoke result

The 2026-07-29 disposable run used exact Factory PR #391 source bytes, a real
isolated `codex exec` call, and the verified Alloy 6.2.0 JAR with SAT4J.

- Historical fixture-subject case: the model emitted zero unsupported
  assertions, preserved all four subjects as unresolved, and Alloy returned
  `no_counterexample_within_scope`.
- Controlled label-authority mutation: the model emitted one grounded
  `requires` assertion, deterministic validation resolved `change-1` to the
  exact bytes and span, and Alloy returned a counterexample whose skolems
  reverse-mapped to the accepted and proposed assertions.
- One intermediate repeat copied a requested quote inaccurately despite
  preserving its meaning. The provider contract was therefore narrowed to
  selecting immutable source-range IDs; deterministic code now attaches exact
  bytes, spans, and digests.

This supports the bridge's plumbing and boundary design only. It is not an
efficacy claim, a general semantic benchmark, or evidence about authentic host
Plan capture.
