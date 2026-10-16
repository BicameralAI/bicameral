# `bicameral-smr`

Independently testable deterministic kernel for the v0.2.1 State Machine
Resolver work.

The crate begins at a source-bound `ClaimSemanticTraceV1`. It deliberately
does not call an LLM. A caller or local extraction provider may propose the
trace, but this kernel validates it as untrusted input, lowers it without
semantic repair, creates an exact bounded Alloy snapshot, executes a pinned
analyzer, and returns bounded evidence with reverse provenance.

## Boundary

Included:

- exact extraction-profile and source-unit validation;
- typed occurrences, assertions, transitions, branches, exceptions, semantic
  identity relations, and explicit unknowns;
- deterministic candidate-hypergraph lowering with a loss manifest;
- complete exact serialization of the supported records, with fixed capacity;
- stable reviewed Alloy meta-model and checks;
- pinned analyzer version, distribution digest, solver, timeout, and command;
- typed bounded status and source-span findings.

Excluded:

- LLM execution or extraction-quality claims;
- daemon, MCP, SQLite, queues, and canonical Product writes;
- Candidate Promotion or release authority;
- accepted-Claim alignment and accepted-versus-proposed authorization checks;
- host interaction semantics such as focus, UI settling, races, and
  confirmation timing;
- CodeGraph and code-compliance claims.

`accepted_model_base` currently pins the base identity. The accepted model's
semantic records are not yet projected in this slice. Consequently, the only
stable semantic check implemented here is
`NoSemanticIdentityConflict`. Broader authorization, lifecycle, precedence,
and reachability checks remain follow-up work.

## Drift-prevention controls

| Pitfall | Current control | Honest limitation |
|---|---|---|
| Lossy boundary compression | Adjacent states retain distinct IDs; `equivalent_to` and `distinct_from` remain addressable; conflicting identity claims produce a bounded Alloy counterexample | Extraction recall is outside this crate |
| Happy-path linearization | Assertions, alternatives, edges, guards, effects, and exception links are serialized as separate exact relations | Complex liveness and concurrency are unsupported |
| Evidence/authority laundering | Hypergraph, snapshot, and result carry content-bound authority classes: noncanonical candidate, noncanonical projection, bounded evidence | No artifact here can approve or promote Product state |
| Generic-harness overreach | Exact alpha extraction profile and closed typed vocabulary are required; unsupported dimensions stop before Alloy | Host-specific contracts require a future specialized model |
| Temporal reasoning weakness | Pre-state, post-state, guards, effects, and precedence remain distinct | Focus, settling, races, and confirmation order must be declared unknown and are refused |

No returned status means globally safe, complete, compliant, approved, ready,
or released. `no_counterexample_within_scope` means only that the selected
stable check found no counterexample in the exact bounded snapshot.

## Feedback v0.1 preview

The repository also contains a source-checkout-only feedback evaluator:

```sh
cargo run -p bicameral-smr --bin smr-feedback-experiment -- \
  --input crates/bicameral-smr/fixtures/feedback/c04-request.json \
  --java-executable /path/to/java17 \
  --alloy-jar /path/to/org.alloytools.alloy.dist.jar
```

This preview accepts one pre-mapped JSON request naming an exact accepted
Boolean assertion and its complete Claim, ModelAtom, relation, and source
witnesses. It does not accept a raw feedback note, extract novel assertions,
or implement the installed `bicameral smr check` command. Relation references
preserve witness closure and provenance; the v0.1 compatibility predicate does
not assign them temporal or causal semantics. The bundled board is partial and
non-authoritative, so its result is internal experiment evidence only.
Java and Alloy run in the temporary snapshot directory with a cleared parent
environment, so provider, OAuth, and host credentials are not inherited.

Another reviewed, non-authoritative fixture can be selected without rebuilding
the crate by supplying its bytes and separate immutable pin together:

```sh
cargo run -p bicameral-smr --bin smr-feedback-experiment -- \
  --input /path/to/mapped-assertion.json \
  --board /path/to/governed-domain.fixture.json \
  --fixture-pin /path/to/fixture-pin.json \
  --java-executable /path/to/java17 \
  --alloy-jar /path/to/org.alloytools.alloy.dist-6.2.0.jar
```

The loader rejects non-regular or oversized inputs and any mismatch in fixture
bytes, Claim-set digest, analysis key, scope, or read-only authority boundary.

The command emits a content-safe JSON receipt for both bounded and typed
no-verdict attempts. Exit status `0` means a `bounded_result` was produced;
exit status `2` means the receipt is `indeterminate`, `unavailable`, `timeout`,
or `malformed`, or that execution failed before a receipt could be formed.
Use the upstream AlloyTools 6.2.0 distribution JAR whose digest is listed
below. A repackaged distribution, including a package-manager JAR with a
different digest, is rejected rather than treated as equivalent.

## Alloy artifacts

[`alloy/smr_meta_model.als`](alloy/smr_meta_model.als) and
[`alloy/smr_checks.als`](alloy/smr_checks.als) are stable source artifacts.
They are not generated per trace. A run generates only singleton snapshot data,
facts that bind exact records, and named command bounds. The snapshot digest
binds:

- stable Alloy artifact digests;
- exact cardinalities;
- generated snapshot data;
- semantic-ID-to-Alloy-symbol reverse provenance;
- check-to-source evidence mappings.

The analyzer rejects drift in any of those fields before invoking Alloy.

## Tests

Deterministic tests:

```sh
cargo test -p bicameral-smr
cargo clippy -p bicameral-smr --all-targets -- -D warnings
```

The real analyzer tests require the pinned Alloy 6.2.0 distribution:

```sh
BICAMERAL_ALLOY_EXECUTABLE=/path/to/alloy \
BICAMERAL_ALLOY_DISTRIBUTION_JAR=/path/to/org.alloytools.alloy.dist-6.2.0.jar \
cargo test -p bicameral-smr --test alloy_snapshot -- --ignored
```

The feedback v0.1 analyzer acceptance also requires Java 17 and the same JAR:

```sh
BICAMERAL_JAVA17_EXECUTABLE=/path/to/java17 \
BICAMERAL_ALLOY_DISTRIBUTION_JAR=/path/to/org.alloytools.alloy.dist-6.2.0.jar \
cargo test -p bicameral-smr --locked \
  --test feedback_alloy_snapshot \
  --test feedback_analyzer \
  --test feedback_evaluation -- --ignored
```

Expected distribution SHA-256:
`6b8c1cb5bc93bedfc7c61435c4e1ab6e688a242dc702a394628d9a9801edb78d`.
