# SMR Feedback v0.1 Advisory Evaluation Contract

## Metadata

| Field | Value |
|---|---|
| Date | 2026-09-03 |
| Status | Proposed governed specification |
| Contract ID | `bicameral.smr-feedback.v0.1` |
| Experiment revision | `smr-feedback-v0.1-r2` |
| Product direction | [#1176](https://github.com/BicameralAI/bicameral-bot/issues/1176) |
| Specification delivery | [#1178](https://github.com/BicameralAI/bicameral-bot/issues/1178) |
| Benchmark sibling | [#1177](https://github.com/BicameralAI/bicameral-bot/issues/1177) |
| Baseline correction | [#1213 owner lock](https://github.com/BicameralAI/bicameral-bot/issues/1213#issuecomment-5530403316) |
| Machine contract | [`smr-feedback-v0.1.schema.json`](smr-feedback-v0.1.schema.json) |
| Capability dependency | [`bicameral.semantic-resolution`](semantic-resolution-capability-contract.md) |
| Governance gate | `bic:spec-governance-gate`; Qor advisory/no-op |

## Decision

`smr-feedback-v0.1` is a read-only internal product-discovery experiment. It
compares independently checkable assertions from one small feedback note with
one reviewed, frozen accepted-SMR snapshot and returns source-bound advisory
results.

It is not an implementation-compliance, feasibility, delivery-estimation,
statement-of-work, sales, marketing, investor, or release-acceptance product.
It does not decide whether a prospect is correct. It does not create or modify
accepted Claims, Decisions, Product events, bindings, or release state.

## Scope

V0.1 has one proposed command surface:

```text
bicameral smr check --input <file> [--format text|json]
```

The command admits UTF-8 plain text or Markdown and evaluates one frozen
accepted-SMR snapshot. It does not crawl repositories, select among snapshots,
inspect code, persist feedback, promote candidates, or expose MCP, Cloud, or
dashboard flows.

The command and its implementation are not authorized by this specification
issue. After this specification merges, implementation tickets must cite its
immutable revision and the applicable `SMR-FB-*` obligations below.

## 1. Baseline Authority And Identity (`SMR-FB-BASELINE-01`)

The baseline is an `AcceptedSmrBaselineV1`: the accepted SMR/hypergraph
projection reconstructed by canonical replay of one Product event stream
through an exact event position. It is not markdown, a `governing_spec`, a
repository head, a search result, a digest by itself, or a model-generated
summary.

The baseline identity binds:

- Product and event-stream identities;
- the inclusive canonical replay position and accepted promotion-event ID;
- the accepted projection and claim-set digests;
- the reviewed named scope, its `complete` or `partial` declaration, and
  explicit exclusions; and
- the baseline owner plus durable review evidence.

The replayed projection contains the accepted Claims, ModelAtoms, assertions,
relations, scopes, and source anchors. Source anchors preserve provenance; they
are not Product law independently of the accepted graph.

The first experiment may use a prepopulated canonical event fixture whose
accepted graph covers only the reviewed Product-creation and workspace-routing
slice. The fixture must be replayed through the same canonical
`claim_set_candidate_promoted` path that Product J1/J2 reads. Loading graph
JSON directly, extracting a written spec at run time, or treating fixture bytes
as authority is forbidden.

An accepted baseline identity is immutable. A different Product, event stream,
replay position, accepted event, projection, claim set, scope declaration,
exclusion, owner, or review record creates a new identity. Silent edits are
invalid. The first fixture's exact canonical events and resulting projection
digest require owner review before benchmark execution.

## 2. Input And Assertion Grain (`SMR-FB-INPUT-01`)

The exact input bytes are identified by digest before extraction. Raw input is
not retained by default. The normalized run record contains media type, byte
length, note digest, and privacy-gate identity, but never the local source path.

The extraction boundary proposes independently checkable assertions grounded
to exact byte spans. A deterministic validator rejects invalid UTF-8, invalid
or overlapping-out-of-range spans, missing source references, duplicate
ordinals, unsupported structures, and digest mismatch. It does not repair or
invent omitted meaning.

Assertions are ordered by first source byte, then final source byte, then
assertion ID. Duplicate assertion IDs or ordinals are invalid.
An assertion ID is:

```text
sha256(
  "smr-feedback-assertion-v1\0" +
  note_digest + "\0" + ordinal + "\0" +
  start_byte + "\0" + end_byte + "\0" + exact_span_digest
)
```

One note may yield zero or more assertions and may contain mixed verdicts. A
note with no admitted assertion returns `indeterminate`, not a vacuous bounded
result. There is no document-level semantic verdict and no "worst wins"
reduction.

## 3. Assertion Verdicts (`SMR-FB-VERDICT-01`)

Let `T` be the exact baseline projection and `p` one validated assertion. The
deterministic semantic mapper evaluates both bounded possibilities:

| `T + p` | `T + not-p` | Verdict |
|---|---|---|
| possible | impossible | `entailed` |
| impossible | possible | `refuted` |
| possible | possible | `underdetermined` |
| impossible | impossible | `conflict` |

`conflict` must cite the smallest available conflicting baseline witness.
`entailed` means only that the assertion follows in the cited, checked slice.
It does not establish complete product coverage or implementation compliance.

Exact references, accepted-atom overlap, relation provenance, and retrieval
scores may select evidence used to construct `T` and `p`. None of them proves a
verdict. A verdict exists only when both required bounded checks complete with
the exact recorded identities and scope.

## 4. Coverage And Gap Annotation (`SMR-FB-SCOPE-01`)

`unknown_scope` is a coverage disclosure, never a verdict, score, reassurance,
or all-clear. It may coexist with valid verdicts for other assertions.

An evaluated assertion records either:

- `complete_named_baseline`, meaning the complete accepted projection through
  the bound replay position was admitted and checked for a scope explicitly
  reviewed as complete; or
- `unknown_scope`, with one or more typed coverage limitations.

`candidate_product_gap` is an optional, non-authoritative discovery annotation.
It is permitted only when all of the following are true:

1. the assertion verdict is `underdetermined`;
2. the accepted baseline declares the named scope complete;
3. the full accepted projection in that scope was admitted and checked;
4. no governing baseline relation exists in that complete scope.

Otherwise the result remains `underdetermined` with `unknown_scope`. A missing
match in partial scope is never a product-gap signal.

## 5. Result And Failure Contract (`SMR-FB-RESULT-01`)

The normalized JSON envelope is `SmrFeedbackRunRecordV1` from the companion
schema. Text output is a rendering of that same normalized record.

Execution status is separate from assertion verdict:

| Status | Meaning |
|---|---|
| `bounded_result` | Every admitted assertion has a bounded verdict. |
| `partial` | At least one assertion has a verdict and at least one remains unevaluated. |
| `unavailable` | A required provider, analyzer, or baseline dependency is unavailable. |
| `timeout` | A pinned stage or whole-command deadline expired. |
| `malformed` | Input, extraction, accepted snapshot, or result failed structural validation. |
| `indeterminate` | Required semantic mapping or bounded evidence cannot justify a verdict. |
| `refused` | A privacy, capacity, profile, or policy precondition failed. |

Unevaluated assertions have no `verdict`. Every non-complete result includes a
typed reason and recovery guidance. Missing citations, baseline mismatch,
ambiguous mapping, incomplete coverage, timeout, provider failure, unsupported
semantics, capacity overflow, or unavailable Alloy never becomes a semantic
verdict or no-conflict result.

CLI exit status reports operational completion only:

- `0`: `bounded_result`;
- `2`: a valid envelope with any other execution status;
- `64`: invocation syntax failed before a run envelope could be formed.

## 6. Provenance And Reproducibility (`SMR-FB-IDENTITY-01`)

Every run binds:

- exact specification commit and artifact digest;
- experiment revision and exact companion-schema commit and artifact digest;
- accepted Product/event-stream identity, replay position, projection and
  claim-set digests, scope declaration, and review references;
- note digest and assertion identities;
- provider, model, prompt, extraction profile, and schema revisions;
- trace, accepted snapshot, Alloy snapshot, and result digests;
- analyzer distribution, solver, Java runtime, bounds, timeouts, and capacity;
- per-assertion source spans, baseline Claim/ModelAtom references,
  construction reasons, evidence state, coverage limitations, and caveats.

The run record represents both the specification and companion schema as
`ArtifactIdentity` values. `technical_pins_digest` is SHA-256 over the RFC 8785
canonical JSON `technical_pins` object. These identities are required fields;
they are not inferred from a release version or mutable repository head.

The result digest is SHA-256 over the RFC 8785 canonical JSON result with its
`result_digest` member omitted.

A material pin change that can affect assertion decomposition, evidence
selection, semantic mapping, verdicts, or gap annotation requires a new
experiment revision and benchmark rerun. It does not require separate Product
approval unless it changes the approved semantics or positioning boundary.

## 7. Technical Experiment Pins (`SMR-FB-PINS-01`)

These pins are technical experiment choices, not Product semantics or vendor
requirements of `bicameral.semantic-resolution`.

| Setting | `smr-feedback-v0.1-r2` |
|---|---|
| Input | UTF-8 text/Markdown, maximum 32 KiB |
| Accepted graph | maximum 512 Claims, 4,096 ModelAtoms, and 8,192 relations |
| Connect timeout | 5 seconds |
| Extraction timeout | 60 seconds |
| Alloy timeout | 10 seconds |
| Whole-command deadline | 90 seconds |
| Extraction profile | `smr-claim-trace-alpha-v1` |
| Prompt revision | `smr-feedback-extraction-prompt-v1` |
| Provider | Vertex OpenAI-compatible adapter using ADC |
| Model | `google/gemini-2.5-flash` |
| Analyzer | Alloy 6.2.0, SAT4J, Java 17 |
| Alloy distribution | `sha256:6b8c1cb5bc93bedfc7c61435c4e1ab6e688a242dc702a394628d9a9801edb78d` |
| Analyzer memory | 512 MiB maximum |

Limits fail closed. No stage truncates input, omits accepted graph elements, silently
raises bounds, swaps providers/models, or aliases a pin as `default` or
`latest`.

## 8. Privacy And Retention (`SMR-FB-PRIVACY-01`)

Synthetic inputs are allowed. Before any internal or prospect material reaches
the configured provider, the operator must record locally that:

1. processing has consent or an approved internal basis;
2. the note is redacted and contains no credentials or prohibited data;
3. the operator understands that content leaves the machine;
4. raw input will be deleted after the run; and
5. a responsible reviewer is named for accidental sensitive-data findings.

Failure of any gate item returns `refused`. The command retains no raw feedback
by default. Public GitHub artifacts may contain only safe metadata, digests,
aggregates, and sanitized synthetic examples. Private paths, raw notes,
credentials, provider tokens, customer identifiers, and restricted excerpts
must not be committed.

## 9. Benchmark Handoff (`SMR-FB-BENCHMARK-01`)

The sibling benchmark result must bind at minimum:

- merged specification commit and artifact digest;
- companion-schema digest;
- accepted baseline Product/event-stream identity, replay position, projection
  and claim-set digests, and durable review references;
- experiment revision and technical-pins digest;
- privacy-safe corpus, partition, annotation-rubric, and scorer digests;
- whole-note partition identity and leakage controls;
- execution-code commit, run timestamp, result digest, and cost budget;
- annotator count, adjudication reference, denominators, and failure classes.

Restricted inputs and gold labels remain in approved private storage. Reported
results are advisory evidence. They do not become Product law, satisfy
`SRC-EFFICACY-01`, close #1006 or #1089, unlock release work, or authorize
external performance claims.

## Authority Matrix

| Actor/component | May | Must not |
|---|---|---|
| Baseline owner/reviewer | Review one accepted projection identity and named scope | Confer authority through a digest or fixture alone |
| Internal operator | Start a privacy-approved run and inspect results | Promote output or make external claims from it |
| Extraction provider | Propose source-grounded assertions and relations | Emit accepted truth or executable Alloy |
| Deterministic validator/analyzer | Validate, compare, and produce bounded evidence | Repair meaning, mutate Product state, or claim global correctness |
| CLI renderer | Render the normalized record | Invent a document verdict or hide unknown coverage |
| Benchmark reviewer | Label, adjudicate, and report advisory evidence | Define Product semantics or release acceptance |

## State Refinement And Minimization

This specification adds no canonical Product lifecycle state, transition,
authority, persistence, or routing behavior. Execution status, assertion
verdict, coverage disclosure, and gap annotation are fields in a read-only run
record, not durable Product states.

Rejected lifecycle states include `feedback_accepted`, `prospect_wrong`,
`product_gap_confirmed`, `implementation_compliant`, and `release_ready`.
Candidate gap remains an annotation; unknown scope remains coverage metadata.
This is the smallest honest model because no output changes the next canonical
Product action.

## User Journey And Validation Boundary

Journey ID: `smr_feedback_advisory_cli_v0_1`.

```text
operator -> CLI -> privacy/input validation -> extraction provider
         -> deterministic trace validation -> SMR/Alloy checks -> CLI result
```

The operator invokes one command and observes either a provenance-bound result
or a typed fail-closed outcome. The only authoritative input boundary is the
accepted SMR projection reconstructed by canonical replay. There is no commit
point. Retries create a new run
identity unless every bound input and pin is identical; cached evidence may be
reattached only under that identical identity. A renderer, provider response,
log, or benchmark score is validation evidence, never Product state.

Journey Boundary Review is pending review of the exact #1178 specification
bytes. Runtime topology and released-host evidence are deferred to post-merge
implementation tickets and cannot be claimed by this documentation PR.

## Acceptance Obligations

- `SMR-FB-BASELINE-01`: authority, canonicalization, digest, versioning, and
  no-silent-edit rules are enforced.
- `SMR-FB-INPUT-01`: note and assertion identity, source grounding, ordering,
  and mixed verdict grain are preserved.
- `SMR-FB-VERDICT-01`: the formal two-check table is the only verdict mapper.
- `SMR-FB-SCOPE-01`: unknown coverage and gap preconditions fail closed.
- `SMR-FB-RESULT-01`: JSON/text parity and typed non-verdict outcomes hold.
- `SMR-FB-IDENTITY-01`: results bind exact inputs, pins, evidence, and digests.
- `SMR-FB-PINS-01`: fixed limits and runtime identities are visible and enforced.
- `SMR-FB-PRIVACY-01`: live-data gating and no-raw-default retention hold.
- `SMR-FB-BENCHMARK-01`: benchmark delivery uses immutable privacy-safe refs.

## Deferred

Runtime implementation, MCP, Cloud App, daemon workflows, persistence, async
jobs, large documents, multiple baselines, CodeGraph, code compliance,
feasibility or timeline estimates, connectors, dashboards, candidate
promotion, release activation, and external claims are outside this contract.
