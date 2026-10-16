# Issue #937 SMR candidate promotion

## Visual adversarial review and first-class test contract

**Review target:** [Issue #937 comment 5095093677](https://github.com/BicameralAI/bicameral-bot/issues/937#issuecomment-5095093677)  
**Owner contract bundle:** [Issue #937 comment 5095866461](https://github.com/BicameralAI/bicameral-bot/issues/937#issuecomment-5095866461)  
**Disposition:** **AMEND** (architectural direction accepted; fourteen machine choices replaced, see A1-A14)  
**Implementation unlock:** **No.** This lane settles the contract only. It grants no implementation, deployment, Release Unit, or Product acceptance authority.

### Durable contract location

The field vocabulary below is **settled**, not illustrative. The authoritative artifacts are:

| Artifact | Path |
| --- | --- |
| Runtime contract | [`docs/specs/state-machine-resolver-v1-runtime-architecture.md`](../../specs/state-machine-resolver-v1-runtime-architecture.md) |
| Companion machine contract | [`docs/specs/state-machine-resolver-v1-runtime-architecture.contract.json`](../../specs/state-machine-resolver-v1-runtime-architecture.contract.json) |
| Protocol schemas | [`protocol/smr-claim-acceptance/schemas/`](../../../protocol/smr-claim-acceptance/schemas/) |
| Golden fixtures and digest vectors | [`protocol/smr-claim-acceptance/fixtures/`](../../../protocol/smr-claim-acceptance/fixtures/) |
| Contract validator | [`scripts/validate_smr_claim_acceptance_contract.py`](../../../scripts/validate_smr_claim_acceptance_contract.py) |

> **Visualization requirement:** All consequential review, implementation, test, and release updates governed by this contract must follow the [ADHD-friendly visualization standard](./VISUALIZATION_STANDARD.md).

## Executive decision

The proposed direction is sound:

- reuse the existing MCP human interaction;
- retain daemon-only canonical authority;
- keep extraction, Integrations, and MCP non-authoritative;
- retain exact human confirmation;
- rebuild accepted SMR state from canonical events;
- avoid a parallel `smr.accept_claim_set` approval family.

The proposed sequence is not a minimal implementation change. It changes or adds at least these contracts:

1. `DecisionCandidate` payload identity and typing;
2. `RecallPacket` rendering and exact proposal identity;
3. confirmation challenge binding and protocol version;
4. canonical promotion event payload and replay behavior;
5. daemon-local SQLite reconciliation;
6. durable idempotency after daemon restart;
7. `preflight.run` read/write semantics;
8. Claim authority relative to separate Decision signoff;
9. working-candidate invalidation and rejection behavior;
10. terminal topology receipts and acceptance outcomes.

## 1. Current implementation versus proposed result

The existing path promotes one replayed `DecisionCandidate` into one proposed `Decision` through an `AcceptCandidate` event. The SMR path accepts a normalized multi-Claim graph whose exact artifact binding, base-snapshot position, claim scope, evidence bundle, generation, and claim-set identities must remain replayable.

The MCP experience and daemon authority boundary are reusable. The existing event and confirmation contracts are not sufficient unchanged.

## 2. Contract risk map

### Required amendments

| # | Amendment | Required result |
|---:|---|---|
| 1 | Version the canonical promotion event | One self-contained event can reconstruct accepted Claims and proof identity without candidate history or extraction replay. |
| 2 | Introduce a typed candidate discriminator | `decision_v1` and `smr_claim_set_v1` cannot be confused by storage, rendering, or promotion consumers. |
| 3 | Define canonical digest construction | Five separate identities (`claim_set_digest`, `artifact_binding_digest`, `smr_base_snapshot_digest` with `base_event_position`, `claim_scope_digest`, `evidence_bundle_digest`) are deterministic and independently reproducible. |
| 4 | Version the confirmation challenge | Confirmation binds the exact proposal tuple, not candidate ID alone. |
| 5 | Make canonical history the idempotency authority | A lost response followed by daemon restart returns the prior committed result without a duplicate append. |
| 6 | Define the sole commit point | Canonical event append decides acceptance. SQLite status and SMR projection reconcile afterward and are repairable. |
| 7 | Preserve read-only preflight or explicitly supersede it | Artifact analysis and candidate staging must not silently become an undocumented side effect of `preflight.run`. |
| 8 | Define Claim authority before and after Decision signoff | A canonical Claim revision has one explicit influence ceiling while the related Decision remains proposed. |
| 9 | Define typed working-state lifecycle | Proposed, rejected, suppressed, stale, invalidated, promoted, and reanalysis behavior are durable and testable. |
| 10 | Define the trace and acceptance packet | An independent reviewer can prove exact host-to-replay behavior without trusting an LLM summary. |

## 3. Amended target state

### Transaction rule

The canonical event append is the only acceptance commit.

```text
validate exact proposal
  -> check durable operation identity
  -> append one self-contained canonical event, or append nothing
  -> reconcile SQLite candidate status
  -> rebuild SMR read model from canonical history
  -> return canonical receipt and refreshed preflight
```

The following ordering is forbidden:

```text
mark SQLite promoted
  -> attempt canonical append
```

A canonical append failure must leave the proposal reviewable and noncanonical. A SQLite or SMR failure after canonical append must produce a typed reconciliation failure and recover from canonical history.

### Accepted canonical event shape

This is the settled payload, not a sketch. The normative schema is [`claim-set-candidate-promoted.schema.json`](../../../protocol/smr-claim-acceptance/schemas/claim-set-candidate-promoted.schema.json).

```yaml
claim_set_candidate_promoted:
  event_schema_version: 1
  digest_profile_version: 1
  operation_id: <uuid, caller-supplied, unique per (product_id, actor_id)>
  routing:
    product_id: <stable Product identity>
    event_stream_id: <daemon-derived>
  candidate:
    candidate_id: <uuid>
    generation: <u64>
    kind: smr_claim_set_v1
  artifact_binding:
    repository_id: <stable repository identity>
    git_head: <40-char lowercase hex>
    artifact_path: <normalized POSIX repo-relative path>
    blob_id: <git blob identity>
    artifact_content_digest: <digest>
  proposal_schema_version: 1
  base_event_position: <u64>
  normalized_claim_set: <NormalizedClaimSetV1>
  digests:
    claim_set_digest: <digest>
    artifact_binding_digest: <digest>
    smr_base_snapshot_digest: <digest>
    claim_scope_digest: <digest>
    evidence_bundle_digest: <digest>
  evidence_refs: []
  codegraph_proof_bundle: <opaque, optional, see #936>
  acceptance:
    actor_id: <authorized human>
    authority_context_digest: <digest>
    confirmation_protocol_version: 2
    confirmation_binding_digest: <digest>
    accepted_at: <RFC3339 UTC>
  resulting_decision:
    decision_id: <deterministic UUIDv5>
    signoff_state: approved
```

A digest is the string `sha256:` followed by exactly 64 lowercase hexadecimal characters, computed as `SHA-256(UTF8("bicameral/<artifact-name>/v1") || 0x00 || JCS(normalized_value))` over RFC 8785 canonical bytes.

Every field is required except `evidence_refs`, which may be empty, and `codegraph_proof_bundle`, which is always optional recall-expansion evidence. [#936](https://github.com/BicameralAI/bicameral-bot/issues/936) owns its exact carrier, not SMR closure or implementation-compliance authority.

The names `canonical_snapshot_digest` and `scope_digest` are **prohibited**: they conflated SMR and CodeGraph identity. Five separate digest identities replace them, and `base_event_position` accompanies `smr_base_snapshot_digest` so the base projection is re-derivable on replay.

## 4. Tests and outcomes are a first-class deliverable

Implementation is not complete when the code compiles or when component tests are green. The implementation pull request must ship the test contract, executable harness, structured receipts, and human-readable outcome report as product artifacts.

### Required deliverables in the implementation PR

1. **Versioned test manifest**
   - test group IDs;
   - exact implementation and contract revisions;
   - environment and process topology;
   - positive assertions;
   - negative controls;
   - expected typed outcomes;
   - sanitization and teardown requirements.

2. **Executable automated tests**
   - deterministic component and integration tests;
   - real-process local topology tests;
   - restart and lost-response tests;
   - backward-compatibility tests;
   - malformed and authority-violation controls.

3. **Authoritative terminal runner**
   - released developer host;
   - packaged MCP process;
   - production daemon process;
   - daemon-local SQLite working repository;
   - canonical event store;
   - SMR replay/materialization;
   - MCP-rendered human-visible result.

4. **Structured receipt schema**
   - machine-readable JSON;
   - exact digests and revisions;
   - process and handoff identities;
   - assertion results;
   - failure classification;
   - sanitization and teardown receipts.

5. **Human-readable report**
   - outcome;
   - what was proven;
   - what was not proven;
   - exact environment;
   - failed assertions;
   - product impact;
   - next action.

6. **Named human acceptance**
   - diagnostic evidence is not terminal evidence;
   - terminal evidence is not accepted until a named human records what it proves.

### Allowed test-run states

```text
not_run
blocked
diagnostic_failed
diagnostic_passed
authoritative_failed
authoritative_inconclusive
authoritative_passed
human_rejected
human_accepted
```

Do not collapse these into percentages or a single green/red status.

## 5. Required test groups

### T1. Exact proposal identity

**Positive proof**

- candidate generation matches;
- `claim_set_digest` matches;
- `artifact_binding_digest` matches, and repository, head, path, and blob identity match;
- `smr_base_snapshot_digest` matches, and `base_event_position` re-derives it;
- `claim_scope_digest` matches;
- `evidence_bundle_digest` matches;
- actor, workspace, session, packet, operation, and requested action match.

**Negative controls**

Mutate each bound value independently. Every mismatch must fail closed before canonical append.

**Terminal outcome:** `exact_match` or a typed mismatch result.

### T2. Authority boundary

**Positive proof**

- daemon mints and consumes the challenge;
- MCP transports and renders it;
- authorized human supplies the explicit confirmation;
- daemon alone appends canonical state.

**Negative controls**

- MCP attempts to mint or self-confirm;
- extractor submits accepted Claims;
- Integration submits canonical identity or approval;
- caller changes authority context between challenge and confirmation.

**Terminal outcome:** `daemon_only` or a typed authority rejection.

### T3. Canonical transaction and reconciliation

**Positive proof**

- one self-contained event is appended exactly once;
- SQLite candidate status reconciles after append;
- SMR projection rebuilds from canonical history;
- resulting receipt identifies the canonical event and projection digest.

**Failure controls**

- event append failure;
- SQLite update failure after append;
- SMR rebuild failure after append;
- process crash at every boundary.

**Terminal outcome:** `committed_once`, `not_committed`, or typed `reconciliation_required`.

### T4. Restart-safe idempotency

**Positive proof**

A retry using the same durable operation identity after daemon restart returns the prior canonical result without a duplicate append.

**Failure controls**

- restart before challenge;
- restart after challenge but before confirmation;
- restart after canonical append but before SQLite update;
- response lost after append;
- retry after in-memory challenge store is gone.

**Terminal outcome:** `replayed_result`, `challenge_expired`, or a typed stale/unauthorized result.

### T5. Deterministic replay

**Positive proof**

- delete disposable SQLite working state;
- restart the daemon;
- replay canonical events;
- reconstruct the same accepted Claim identities, proof identity, and SMR digest;
- perform zero provider/extraction calls during replay.

**Terminal outcome:** `same_projection` or typed replay failure.

### T6. Working-candidate lifecycle

**Positive proof**

Typed behavior exists for:

- proposed;
- rejected;
- suppressed;
- stale;
- invalidated;
- promoted;
- needs reanalysis.

**Negative controls**

- confirm a challenge after artifact digest changes;
- confirm after the base snapshot advances past `base_event_position`;
- attempt to resurface semantically identical rejected content;
- expire or remove suppression;
- create a new proposal generation without invalidating the old challenge.

**Terminal outcome:** one explicit lifecycle result, never silent disappearance.

### T7. Compatibility

**Positive proof**

- existing `decision_v1` candidate promotion still works;
- `smr_claim_set_v1` follows the new exact contract;
- RecallPacket renders both without ambiguity.

**Negative controls**

- unknown candidate kind;
- malformed proposal schema;
- protocol v1 challenge presented for an SMR proposal;
- older client receives unsupported-version response rather than corrupting state.

**Terminal outcome:** `compatible` or typed version incompatibility.

### T8. Terminal released-host topology

**Required path**

```text
released coding host
  -> packaged MCP
  -> production daemon
  -> daemon analysis/staging
  -> SQLite working repository
  -> read-only preflight RecallPacket
  -> explicit human confirmation
  -> canonical event append
  -> SMR rebuild
  -> refreshed MCP-visible result
  -> process restart and replay
```

**Negative controls**

- daemon unavailable;
- protocol drift;
- stale/partial scope;
- human rejection;
- lost response;
- cleanup failure;
- secret or raw-content leakage into retained trace.

**Terminal outcome:** `human_accepted` only after a named reviewer inspects the complete receipt.

## 6. Required terminal receipt fields

```yaml
receipt:
  test_manifest_version:
  journey_id:
  journey_environment_ids: []
  released_host_version:
  mcp_commit:
  bot_commit:
  contract_versions: {}
  artifact_binding:
    repository:
    git_head:
    artifact_path:
    blob_digest:
  candidate:
    candidate_id:
    candidate_kind:
    generation:
    claim_set_digest:
    claim_scope_digest:
    currentness:
    promotion_eligibility:
    analysis_status:
    visibility:
  canonical:
    appended:
    canonical_event_log_digest_before:
    canonical_event_log_digest_after:
    claim_projection_digest_before:
    claim_projection_digest_after:
    base_event_position:
    event_id:
    operation_id:
    resulting_decision_id:
  confirmation:
    protocol_version:
    binding_digest:
    actor_id:
    requested_outcome:
  working_state:
    sqlite_path_identity:
    status_before:
    status_after:
    reconciliation_result:
  smr:
    projection_digest_before:
    projection_digest_after:
    replay_projection_digest:
    provider_calls_during_replay:
  assertions: []
  negative_controls: []
  sanitization:
  teardown:
  outcome:
  limitations: []
  accepted_by:
```

Sensitive challenge values, credentials, raw private artifacts, and unrestricted prompts must never appear in retained evidence.

## 7. Implementation sequence after contract acceptance

1. Persist the SMR candidate and canonical-event schemas.
2. Define canonical serialization and digest algorithms.
3. Define confirmation challenge protocol v2.
4. Define durable operation lookup and post-restart idempotency.
5. Define the working-state lifecycle and reconciliation procedure.
6. Define Claim authority relative to Decision signoff.
7. Implement component tests and failure injection.
8. Implement the real-process local topology.
9. Implement released-host terminal validation.
10. Produce the authoritative receipt and named human acceptance.

## 8. Source implementation anchors

- [`crates/bicameral-api/src/candidate.rs`](../../../crates/bicameral-api/src/candidate.rs)
- [`crates/bicameral-api/src/candidate_promotion.rs`](../../../crates/bicameral-api/src/candidate_promotion.rs)
- [`crates/bicameral-api/src/confirmation_challenge.rs`](../../../crates/bicameral-api/src/confirmation_challenge.rs)
- [`crates/bicameral-api/src/recall.rs`](../../../crates/bicameral-api/src/recall.rs)
- [`crates/bicameral-runtime/src/dispatch.rs`](../../../crates/bicameral-runtime/src/dispatch.rs)
- [`crates/bicameral-gateway/src/live_projection_work.rs`](../../../crates/bicameral-gateway/src/live_projection_work.rs)

## 9. Amendments to the owner contract bundle

The architectural direction in [comment 5095866461](https://github.com/BicameralAI/bicameral-bot/issues/937#issuecomment-5095866461) is accepted with the machine-level amendments recorded in [runtime contract section 17](../../specs/state-machine-resolver-v1-runtime-architecture.md#17-amendments-to-the-owner-bundle).

Ranked by severity:

| # | Replacement | Why it matters |
| ---: | --- | --- |
| A1 | Challenge TTL stays **300 seconds**, not 600 | The shipped constant is 300; doubling the human-confirmation window widens stale and confused-deputy exposure |
| A2 | v2 binding is a **strict superset of v1** | The bundle dropped `workspace`, `session_id`, and `packet_id`, which carry shipped typed fail-closed outcomes |
| A6 | `base_event_position` required alongside `smr_base_snapshot_digest` | The append changes the projection the digest describes, so replay could not verify it |
| A4 | `operation_id` is **caller-supplied**, unique per `(product_id, actor_id)` | A daemon-minted identifier the caller never saw cannot be retried after a lost response |
| A3 | Bind `display_summary_digest`, exclude the text from semantic digests | The bundle both bound the summary and forbade summaries in digests |
| A11 | Explicit size bounds, typed `proposal_too_large` | Canonical events are immutable and permanent |
| A10 | Digest encoding fixed: `sha256:` plus 64 lowercase hex | Two conformant implementations would otherwise disagree byte-for-byte |
| A5 | `decision_id` is a deterministic UUIDv5 | A retry or concurrent submit under one `operation_id` must resolve to one Decision |
| A7 | `event_stream_id` is daemon-derived; caller values rejected | Otherwise a caller picks its own canonical destination |
| A8 | `canonical_event_log_digest` advances, `claim_projection_digest` unchanged | Makes the rejection invariant testable |
| A9 | All four lifecycle reason enums closed | The bundle left them as "such as" |
| A13 | Schemas at `protocol/smr-claim-acceptance/` | `protocol/schemas/v1｜v2` is generated from Rust types and would drift |
| A12 | Runtime spec **created**, not corrected | Neither the document nor `smr.accept_claim_set` existed in any repository |
| A14 | `preflight.lookup` recorded as forward-looking | Not a shipped command name |
| A25 | Relations require supporting Claim and reachable SourceAnchor identities; no categorical derivation label in v1 | Input lineage is checkable, but semantic explicitness over arbitrary prose is not; extractor self-labeling would launder evidence |

## Final recommendation

**AMEND.**

The architectural direction is accepted. The event, candidate identity, confirmation binding, idempotency, reconciliation, authority, trace, test, and terminal-outcome semantics are now explicit and machine-checked in the runtime contract, the companion contract JSON, the protocol schemas, and the golden fixtures.

The implementation may reuse the existing MCP interaction and the daemon-owned governance boundary. It must not reuse underspecified canonical semantics merely because the command names are already convenient.

**This document and its companions grant no implementation, deployment, Release Unit, or Product acceptance authority.** The specification-level Journey Boundary Review is passed; catalog registration and terminal real-host evidence remain delegated to [#943](https://github.com/BicameralAI/bicameral-bot/issues/943). CodeGraph remains optional recall expansion only, with its exact carrier owned through [#936](https://github.com/BicameralAI/bicameral-bot/issues/936).
