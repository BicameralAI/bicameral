# State Machine Resolver v1 runtime architecture

**Issue:** [#937](https://github.com/BicameralAI/bicameral-bot/issues/937) RFQ: Bind artifact-context SMR claims to the existing candidate and MCP acceptance lifecycle
**Contract PR:** [#942](https://github.com/BicameralAI/bicameral-bot/pull/942)
**Owner bundle:** [#937 comment 5095866461](https://github.com/BicameralAI/bicameral-bot/issues/937#issuecomment-5095866461)
**Disposition:** **AMEND** (architectural direction accepted; the machine choices below replace the illustrative ones)
**Companion machine contract:** [`state-machine-resolver-v1-runtime-architecture.contract.json`](./state-machine-resolver-v1-runtime-architecture.contract.json)

**Normative temporal amendment:** [`state-machine-resolver-v1-runtime-architecture.amendment-002.md`](./state-machine-resolver-v1-runtime-architecture.amendment-002.md)
**Normative relation-support amendment:** [`state-machine-resolver-v1-runtime-architecture.amendment-003.md`](./state-machine-resolver-v1-runtime-architecture.amendment-003.md)
**Normative replay-predecessor amendment:** [`state-machine-resolver-v1-runtime-architecture.amendment-004.md`](./state-machine-resolver-v1-runtime-architecture.amendment-004.md)
**Normative alpha-authority amendment:** [`state-machine-resolver-v1-runtime-architecture.amendment-005.md`](./state-machine-resolver-v1-runtime-architecture.amendment-005.md)
**Protocol schemas:** [`protocol/smr-claim-acceptance/schemas/`](../../protocol/smr-claim-acceptance/schemas/)
**Visual review:** [`docs/domain/smr-candidate-promotion/`](../domain/smr-candidate-promotion/README.md)

This document is the authoritative runtime, state-machine, digest, replay, and evidence contract for State Machine Resolver (SMR) claim acceptance. It grants no implementation, deployment, Release Unit, or Product acceptance authority. It defines what an implementation must satisfy before it can be considered conformant.

Every field name, enum member, digest input, event name, transition, and failure result in this document is normative. Nothing here is illustrative.

## 1. Status summary

| State | Meaning |
| --- | --- |
| **NOW** | The contract is settled and machine-readable. Implementation is not started and is not authorized by this document. |
| **NEXT** | Product-owner review and merge of this contract. Implementation and terminal topology remain separate work. |
| **BLOCKED** | No remaining Product decision blocks the specification. Implementation remains blocked on the repo-owned issue slice and #943 topology execution profile. |

## 2. Authority and persistence boundary

These boundaries are settled. An implementation that violates one is non-conformant regardless of test results.

| Component | May do | Must not do |
| --- | --- | --- |
| Daemon | Validate proposals, mint challenges, append canonical events, reconcile local projections | Accept extractor-supplied authority; append on failed validation |
| MCP | Display daemon-authored exact effect; relay one explicit human gesture | Mint, normalize, rank, approve, or persist canonical Claims |
| Integrations | Transport screened evidence and advisory hints | Define Claims, contradictions, acceptance, or canonical identity |
| Provider / extractor | Emit advisory, noncanonical candidate content | Supply authority fields, approval, or canonical identity |
| Daemon-local SQLite | Hold durable working state and rebuildable projections | Serve as canonical truth |
| Canonical event log | Be the sole source of truth and the sole acceptance commit point | Be rewritten, deleted, or reordered |

Derived rules:

1. Canonical event append is the sole acceptance commit point.
2. SQLite candidate status and SMR projections reconcile **after** canonical append and are repairable from canonical history.
3. Rejection appends no accepted claim-bearing event.
4. Replay must not invoke providers or reconstruct extraction work.
5. Candidate promotion and proposed-Decision signoff remain distinct transitions.
6. No parallel `smr.approve` or `smr.accept_claim_set` command family exists. This is a standing prohibition, not a deletion: no such command has ever been implemented in this repository.
7. Alpha introduces no hosted LLM, hosted reasoning service, or hosted LLM cache.
8. CodeGraph is optional recall expansion only. It is never required for SMR closure and never supplies implementation-compliance authority.
9. Every Candidate generation has at most one terminal disposition, enforced at the canonical append boundary rather than by `operation_id` alone.
10. Current Product role and proposal-base freshness are revalidated atomically with every canonical append.

## 3. Command boundary

| Command | Effect | Canonical write |
| --- | --- | --- |
| `smr.claims.propose` | Daemon-mediated extraction and reconciliation, deterministic validation, local candidate staging | None |
| `preflight.run` | Read-only recall | None |
| `recall.promote_decision_candidate` | Existing governed acceptance path, extended for `smr_claim_set_v1` | `claim_set_candidate_promoted` |
| Existing review rejection surface | Existing governed rejection path | `claim_set_candidate_rejected` |

`smr.claims.propose` accepts either caller-supplied structured output or a configured `ClaimExtractionProvider`. The daemon, not the provider, brokers bounded canonical context. Caller, local, and future hosted providers produce the same `SmrClaimSetProposalV1`. Provider identity is provenance, never Claim identity. Alpha defaults remain caller and local; a hosted or Tinker adapter stays disabled until a separate egress, privacy, and Journey decision. There is no silent hosted fallback.

`preflight.run` remains read-only and never creates or updates a candidate. The read-only lookup successor named `preflight.lookup` in the owner bundle is a forward-looking name; it is not a shipped command today. The shipped read-only lookup path is the staged preflight response carrying `lookup.checkpoint_hint`. Renaming it is out of scope for this contract and changes no semantics here.

If caller extraction requires two turns, a read-only context-preparation response may precede `smr.claims.propose`. Only the final validated submit stages local working state.

## 4. Digest profile v1

`digest_profile_version: 1` is a required field on every canonical SMR event and every confirmation challenge. A consumer that does not recognise the profile version fails closed with `unsupported_contract_version`.

### 4.1 Construction

```text
digest = "sha256:" || lowerhex(
  SHA-256( UTF8("bicameral/" || <artifact-name> || "/v1") || 0x00 || JCS(normalized_value) )
)
```

`JCS` is RFC 8785 JSON Canonicalization Scheme. The encoded result is the ASCII string `sha256:` followed by exactly 64 lowercase hexadecimal characters. Encoding is normative: an implementation that emits uppercase hex, raw bytes, base64, or an unprefixed value is non-conformant.

### 4.2 Normalization rules

1. Unicode strings are NFC-normalized.
2. Repository paths are normalized POSIX repo-relative paths, without a leading `./` or `/`.
3. Set-like arrays are deduplicated and sorted by their stable semantic ID before JCS.
4. Ordered lifecycle fragments carry explicit integer ordinals. Array position alone is never semantic.
5. Optional absent fields are omitted. `null` is forbidden unless a schema explicitly defines it.
6. Floating-point values are forbidden anywhere in the normalized Claim schema.
7. Volatile timestamps, provider receipts, display summaries, and local database IDs are excluded from semantic digests.

For every `ModelAtom`, the daemon verifies one identity derived from the normalized tuple `(atom_kind, semantic_key, semantic scope)` under digest profile v1. Extractor-supplied `atom_id` is an assertion to verify, not authority. Equivalent normalized semantics with different IDs, or different normalized semantics with the same ID, fail with `proposal_identity_mismatch`. This prevents semantic aliases from hiding overlapping state transitions from contradiction detection.

### 4.3 Digest identities

| Field | Artifact name | Covers |
| --- | --- | --- |
| `claim_set_digest` | `claim-set` | Normalized Claims, atoms, assertions, relations, scopes, closure assertions, source anchors |
| `artifact_binding_digest` | `artifact-binding` | The exact artifact binding object |
| `smr_base_snapshot_digest` | `smr-base-snapshot` | Accepted Claim projection at `base_event_position` |
| `claim_scope_digest` | `claim-scope` | Normalized SMR semantic scopes |
| `evidence_bundle_digest` | `evidence-bundle` | Exact ordered evidence references, including any opaque CodeGraph proof carrier |
| `display_summary_digest` | `display-summary` | The daemon-authored human-readable exact effect string |
| `rejection_fingerprint` | `rejection-fingerprint` | Normalized semantic Claim content plus Product and semantic scope |

There is no single `canonical_snapshot_digest` and no single `scope_digest`. Both names are prohibited: they conflated SMR and CodeGraph identity in the earlier draft.

### 4.4 Base snapshot position

`smr_base_snapshot_digest` alone is not re-derivable, because appending the promotion event changes the projection it describes. The contract therefore binds a position:

- `base_event_position` (unsigned 64-bit, required) is the canonical event log position of the last event included in the base projection.
- `smr_base_snapshot_digest` is the digest of the accepted Claim projection built from canonical events up to and including `base_event_position`, strictly **before** this append.
- A validator re-derives the digest by replaying to `base_event_position`. Without the position field the digest is unverifiable, so both fields are required together.

### 4.5 Mismatch behavior

Any digest or identity mismatch invalidates the challenge, appends nothing, and returns a typed `proposal_identity_mismatch` naming the mismatched field. The daemon must not silently rehash, rewrite the proposal, substitute a newer candidate generation, or retry against a new snapshot.

### 4.6 Size bounds

Canonical events are immutable and permanent, so an unbounded normalized claim set is a durable resource hazard. Profile v1 bounds:

| Bound | Limit |
| --- | --- |
| `claims[]` | 512 |
| `model_atoms[]` | 4096 |
| `claim_assertions[]` | 8192 |
| `atom_relations[]` | 8192 |
| `scopes[]` | 256 |
| `source_anchors[]` | 8192 |
| Serialized canonical event bytes | 4 MiB |

Exceeding any bound is a typed `proposal_too_large`. It fails closed and appends nothing.

## 5. Canonical events

Both events are additive members of the existing `EventType` enum, serialized `snake_case`. Neither overloads `AcceptCandidate` payload interpretation. Existing event history is never rewritten.

### 5.1 `claim_set_candidate_promoted`

Schema: [`claim-set-candidate-promoted.schema.json`](../../protocol/smr-claim-acceptance/schemas/claim-set-candidate-promoted.schema.json)

```yaml
event_type: claim_set_candidate_promoted
payload:
  event_schema_version: 1
  digest_profile_version: 1
  operation_id: <uuid>
  routing:
    product_id: <stable Product identity>
    event_stream_id: <daemon-derived Product event stream identity>
  candidate:
    candidate_id: <uuid>
    generation: <u64>
    kind: smr_claim_set_v1
  artifact_binding:
    repository_id: <stable repository identity>
    git_head: <40-character lowercase hex sha>
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
  codegraph_proof_bundle: <opaque, optional; see section 10>
  acceptance:
    actor_id: <authorized human>
    authority_context_digest: <digest>
    confirmation_protocol_version: 2
    confirmation_binding_digest: <digest>
    accepted_at: <RFC3339 UTC>
  resulting_decision:
    decision_id: <deterministic uuid, see 5.3>
    signoff_state: approved
```

Every field is required except `evidence_refs`, which may be an empty array, and `codegraph_proof_bundle`, which is optional per section 10.

Workspace paths, daemon working directory, MCP session identity, provider identity, and SQLite identity are **not** Product routing identity and never appear in `routing`.

### 5.2 Routing derivation

`routing.product_id` is caller-supplied and authorization-checked. `routing.event_stream_id` is **daemon-derived** from `product_id`. A caller-supplied `event_stream_id` is rejected. A derived stream that does not belong to `product_id` is a typed `routing_mismatch` that appends nothing.

### 5.3 Resulting Decision identity

One promoted Claim set atomically creates exactly one `Decision(approved)` governance envelope. Alpha skips an observable `Decision(proposed)` on this path. The Decision is not a second semantic summary that a human must independently discretize: its context is the governing Claim context, and its Claims and ModelAtoms are the exact machine-readable content. `resulting_decision.decision_id` is required, never optional.

`decision_id` is deterministic so that a retried operation cannot mint a second Decision:

```text
decision_id = UUIDv5(
  namespace = 8f4d2c61-9a37-5b0e-8d21-4f6c7a9b3e50,
  name      = "claim_set_candidate_promoted/v1|" || product_id || "|" || operation_id
)
```

The value is stored in the payload, so replay reads it back rather than recomputing it. Determinism guards the concurrent-submit and lost-response retry cases, where two attempts under one `operation_id` must resolve to one Decision.

### 5.4 `claim_set_candidate_rejected`

Schema: [`claim-set-candidate-rejected.schema.json`](../../protocol/smr-claim-acceptance/schemas/claim-set-candidate-rejected.schema.json)

A self-contained, non-claim-bearing event. It carries `operation_id`, routing, candidate ID, generation and kind, `claim_set_digest`, `rejection_fingerprint`, artifact and scope identity, a closed `reason_code`, actor, confirmation binding, and timestamp.

Two digests are named so the invariant is testable:

- `canonical_event_log_digest` **advances** when a rejection is appended.
- `claim_projection_digest` is **unchanged** by a rejection.

`rejection_fingerprint` hashes normalized semantic Claim content plus Product and semantic scope, excluding source location, provider identity, timestamp, and candidate ID. Replay rebuilds a local rejection-tombstone index. Automatic extraction suppresses an exact fingerprint match after SQLite deletion or restart. Changed semantics produce a new fingerprint. An explicit human reconsideration may create a new generation referencing the prior rejection event, but automatic ingest must never silently resurface it. The old event is never deleted or rewritten.

Closed `reason_code`: `not_supported_by_artifact`, `semantically_incorrect`, `out_of_scope`, `duplicate_of_accepted`, `insufficient_evidence`, `superseded_by_newer_generation`, `policy_prohibited`.

## 6. Normalized Claim set v1

Schema: [`smr-claim-set-proposal.schema.json`](../../protocol/smr-claim-acceptance/schemas/smr-claim-set-proposal.schema.json)

`NormalizedClaimSetV1` contains `claims[]`, `model_atoms[]`, `claim_assertions[]`, `atom_relations[]`, `scopes[]`, `closure_assertions[]`, and `source_anchors[]`.

| Collection | Required fields |
| --- | --- |
| `claims[]` | `claim_id`, `revision` (u64), `context_statement`, `source_anchor_ids[]`, `assertion_ids[]` |
| `model_atoms[]` | `atom_id`, `atom_kind`, `semantic_key`, `scope_id` |
| `claim_assertions[]` | `assertion_id`, `claim_id`, `atom_id`, `polarity`, `value` |
| `atom_relations[]` | `relation_id`, `relation_kind`, `from_atom_id`, `to_atom_id`, non-empty `supporting_claim_ids[]`, non-empty `supporting_source_anchor_ids[]`, `ordinal` when ordered |
| `scopes[]` | `scope_id`, `scope_kind`, `selector` |
| `closure_assertions[]` | `closure_id`, `scope_id`, `closure_kind` |
| `source_anchors[]` | `anchor_id`, `artifact_binding_ref`, `byte_start`, `byte_end`, `line_start`, `line_end` |

Closed enumerations. Adding a member is a `proposal_schema_version` bump, never a runtime-additive change.

- `atom_kind`: `actor`, `state`, `transition`, `event`, `guard`, `action`, `data_field`, `invariant`, `term`
- `polarity`: `asserts`, `negates`
- `value.kind`: `boolean`, `integer`, `string_enum`, `atom_ref`, `absent`
- `relation_kind`: `precedes`, `triggers`, `guards`, `produces`, `refines`, `part_of`, `same_as`, `contradicts`
- `scope_kind`: `artifact`, `symbol`, `module`, `product_domain`
- `closure_kind`: `complete`, `partial`, `unknown_scope`

`ordinal` is required when `relation_kind` is `precedes` or `triggers`, and forbidden otherwise.

Every `AtomRelation` must identify at least one supporting Claim and at least one SourceAnchor from the same `NormalizedClaimSetV1`. Both support arrays are deduplicated and sorted by stable semantic ID before digesting. Every `supporting_claim_id` must resolve to a Claim in that set. Every `supporting_source_anchor_id` must resolve to a SourceAnchor that is reachable through at least one supporting Claim's `source_anchor_ids`.

A missing, foreign, duplicate, unsorted, or unreachable support reference fails closed with `proposal_identity_mismatch`, names the mismatched relation field, and appends nothing.

These links record the inputs presented for review. They do **not** prove semantic entailment, source explicitness, correctness, or authority. No general deterministic classifier exists for `source_explicit`, `rule_derived`, or `model_inferred` over arbitrary prose, so those fields and equivalent extractor-authored derivation classifications are not part of v1. Human acceptance of the exact `claim_set_digest` remains the sole acceptance boundary. A future proposal-schema revision may add a derivation receipt only when it can bind a versioned deterministic rule and reproducible input digest.

This is a pre-implementation correction to the unshipped v1 schema. No SMR implementation, Release Unit, or activation consumed the earlier relation shape. After the first shipped consumer, adding or changing a closed member requires a `proposal_schema_version` bump.

Prohibited without exception: arbitrary predicates, opaque `parameters_json`, executable expressions, provider-supplied derivation classifications, any authority field supplied by the extractor, and any provider-generated approval.

## 7. Confirmation protocol v2

Schema: [`confirmation-binding.v2.schema.json`](../../protocol/smr-claim-acceptance/schemas/confirmation-binding.v2.schema.json)

The challenge is short-lived and daemon-memory-only. The operation result is durable. Canonical event history is the durable operation ledger; the challenge is authorization scratch, never Product state.

### 7.1 Binding is a strict superset of v1

The shipped v1 `ChallengeBinding` in [`crates/bicameral-api/src/confirmation_challenge.rs`](../../crates/bicameral-api/src/confirmation_challenge.rs) binds `protocol_version`, `workspace`, `actor_id`, `session_id`, `packet_id`, `candidate_id`, `requested_outcome`, and `request_id`, with typed fail-closed outcomes including `WorkspaceMismatch` and `PacketMismatch`.

v2 **retains every v1 bound field** and adds SMR fields. Dropping `workspace`, `session_id`, or `packet_id` would remove two shipped fail-closed controls and is prohibited. The owner-bundle rule that workspace and MCP session identity are not *Product routing identity* is correct and is a separate concern: those fields remain **authorization scope**, and authorization scope stays bound.

```yaml
protocol_version: 2
digest_profile_version: 1
challenge_id: <random uuid>
operation_id: <durable idempotency uuid>
request_id: <uuid, retained from v1>
action: promote_smr_claim_set | reject_smr_claim_set
actor_id: <bound actor>
workspace: <bound workspace, retained from v1>
session_id: <bound agent session, retained from v1>
packet_id: <bound RecallPacket, retained from v1>
product_id: <bound Product>
candidate_id: <uuid>
candidate_generation: <u64>
candidate_kind: smr_claim_set_v1
proposal_identity:
  claim_set_digest: <digest>
  artifact_binding_digest: <digest>
  smr_base_snapshot_digest: <digest>
  base_event_position: <u64>
  claim_scope_digest: <digest>
  evidence_bundle_digest: <digest>
issued_at: <RFC3339 UTC>
expires_at: <issued_at + 300 seconds>
display_summary: <daemon-authored human-readable exact effect>
display_summary_digest: <digest>
confirmation_binding_digest: <digest of every field above except display_summary>
```

### 7.2 Time to live

`expires_at` is `issued_at + 300` seconds, matching the shipped `CONFIRMATION_CHALLENGE_TTL_SECONDS` constant. The owner bundle proposed 600 seconds. That doubles the window in which a stale or confused-deputy confirmation can be replayed against a human gesture, with no stated justification, and it silently diverges from the constant already enforced and tested in the runtime. v2 keeps 300 seconds.

### 7.3 Display summary binding

`display_summary` is the human-readable text the daemon rendered. It is excluded from every **semantic** digest under section 4.2 rule 7. It is nonetheless bound, through `display_summary_digest`, which is included in `confirmation_binding_digest`.

This is the only construction that satisfies both constraints: the human's confirmation provably covers the exact text they inspected, while a cosmetic wording change never alters Claim identity.

### 7.4 Confirmation submission

A confirmation submits only `protocol_version`, `challenge_id`, `operation_id`, `confirmation_binding_digest`, and the explicit confirm or reject gesture. Challenges are actor-bound, single-use, expire after 300 seconds, and are invalid after daemon restart.

### 7.5 Restart and retry behavior

| Situation | Result |
| --- | --- |
| Restart before canonical append | Old challenge fails; the same actor may request a fresh challenge after exact proposal revalidation |
| Restart after canonical append, before the response arrives | Retrying the same `operation_id` returns the prior canonical receipt, with no new append and no new challenge |
| Same `operation_id`, different confirmation binding | Permanent `operation_conflict` |
| Used, expired, wrong-actor, wrong-Product, or wrong-binding challenge | Fail closed, no append |

### 7.6 Operation identity

`operation_id` is **caller-supplied** at `smr.claims.propose` submit time and bound at challenge issue. Uniqueness is scoped to the `(product_id, actor_id)` pair, so one actor cannot squat or collide with another actor's identifier namespace.

Caller supply is required rather than daemon minting: a response lost before the caller ever observed a daemon-minted identifier would otherwise be unretryable, which defeats restart-safe idempotency.

`request_id` remains the per-request audit correlation value from the existing `ToolRequest` contract and is distinct from `operation_id`. They are never aliased.

### 7.7 Version compatibility

| Presented | Target | Result |
| --- | --- | --- |
| v1 challenge | `smr_claim_set_v1` candidate | Fail closed, `unsupported_contract_version` |
| v2 challenge | `decision_v1` candidate | Fail closed, `unsupported_contract_version` |
| v1 challenge | `decision_v1` candidate | Existing behavior, unchanged |
| Unknown `event_schema_version`, `proposal_schema_version`, or `digest_profile_version` | Any | Fail closed, `unsupported_contract_version`, render nothing as accepted |

## 8. Claim authority relative to Decision signoff

Alpha cut (A27): skip observable `Decision(proposed)` on claim-set acceptance, and treat every current Product participant as Product Owner.

1. Human confirm of a claim-set makes the exact Claim revisions canonical, immediately eligible for read-only recall, overlap selection, and deterministic contradiction checking, and **authoritative**. There is no recommendation-only window on this path.
2. The resulting Decision envelope is `approved` at the same append. A separate owner `approve_signoff` is not part of alpha claim-set acceptance.
3. Rejecting or superseding the Decision removes its Claims from the effective current projection while preserving immutable history and replay. Those transitions use Product Owner authority, which in this alpha cut every current participant holds.
4. Canonicality records human acceptance, not consistency. Two accepted Claims may contradict. The resolver surfaces the contradiction and never auto-retracts either.
5. Working-state candidate generations still use `proposed → promoted | rejected`. That is not Decision signoff.
6. `Decision(proposed)` as a member-created recommendation envelope is **out of this alpha cut**. Policy Owner remains creator-only.

No second Claim approval lifecycle is introduced. Influence ceiling enum: `recommendation`, `authoritative`. Alpha claim-set confirm writes `authoritative`.

The governance envelope has no independent semantic title or payload. Its governed content is the exact promoted Claim set and ModelAtoms:

| Decision signoff state | Transition authority | Effective Claim treatment |
| --- | --- | --- |
| `approved` | current Product Owner at claim-set confirm (every current participant, this alpha cut) | canonical, recallable, authoritative |
| `rejected` | current Product Owner at append | immutable history only; excluded from effective projection |
| `superseded` | current Product Owner at append | immutable history only; excluded from effective projection |
| `proposed` | out of this alpha cut for claim-set acceptance | not an observable alpha state on this path |

Challenge issuance never freezes role authority. The daemon rechecks current Product Owner authority (every current participant, this alpha cut) in the same compare-and-append operation that would commit the event.

## 9. Working-candidate state machine

Durable candidate lifecycle states and the only persisted transitions:

```text
proposed -> promoted
proposed -> rejected
```

For key `(product_id, candidate_id, candidate_generation)`, the canonical history contains at most one event from `{claim_set_candidate_promoted, claim_set_candidate_rejected}`. The event-store append must enforce this invariant atomically. `operation_id` only deduplicates one caller operation; it does not prevent two distinct operations from racing. A losing promotion or rejection returns `candidate_already_disposed` and appends nothing.

Every other term is an orthogonal typed attribute, not a lifecycle state. None has distinct acceptance authority or a distinct commit point.

| Attribute | Closed values | Closed reason enum |
| --- | --- | --- |
| `currentness` | `current`, `stale` | `artifact_drift`, `base_snapshot_drift`, `scope_drift`, `schema_drift`, `evidence_drift` |
| `promotion_eligibility` | `eligible`, `invalid` | `proposal_identity_mismatch`, `schema_unsupported`, `scope_insufficient`, `evidence_missing`, `authority_absent`, `proposal_too_large`, `routing_mismatch` |
| `analysis_status` | `complete`, `needs_reanalysis`, `deferred`, `failed` | `extraction_error`, `provider_unavailable`, `context_budget_exceeded`, `artifact_unreadable` |
| `visibility` | `visible`, `suppressed` | `duplicate_fingerprint`, `prior_rejection`, `policy` |

The reason enums are closed. The owner bundle left them open with "such as" and "a closed reason"; illustrative reasons are not acceptable in a durable contract, so they are enumerated here.

Reanalysis creates a new immutable candidate generation. It never rewrites the generation a human inspected.

Writers and recovery:

- The daemon extraction coordinator creates `proposed` generations in local working storage.
- The daemon derives `currentness` and `promotion_eligibility`, and may derive `visibility`.
- Only the existing authorized MCP-to-daemon review path produces `promoted` or `rejected`.
- Deleting local SQLite may lose unreviewed proposals. It must not lose accepted Claims or durable rejection intent.

## 10. CodeGraph optional recall boundary

CodeGraph is strictly an optional recall-expansion strategy in alpha. Core SMR closure, Claim acceptance, contradiction checking over the controlled spec corpus, and issue #937 closure do not depend on CodeGraph availability.

- A `codegraph_proof_bundle`, when present, remains opaque evidence identity and may expand candidate retrieval.
- CodeGraph never asserts that a specification was implemented, never supplies code-compliance authority, and never changes Claim acceptance or signoff authority.
- Missing, stale, insufficient, ambiguous, unsupported, or mismatched CodeGraph evidence cannot block core SMR closure.
- No absence claim may be inferred from unavailable or incomplete CodeGraph evidence.
- The exact optional `codegraph.ensure_current` carrier, materialization identity,
  retry behavior, and same-snapshot handoff are defined by
  [`smr-alpha-runtime-contract.md`](smr-alpha-runtime-contract.md) and resolve
  the contract question in [#936](https://github.com/BicameralAI/bicameral-bot/issues/936).
- The managed Plan path performs semantic projection once, then selects
  accepted Claims by exact/related ModelAtom joins. Accepted bindings,
  CodeLocator, and CodeGraph are additive selector routes; none constructs a
  Claim or replaces Plan impact atoms.

## 11. Canonical transaction and replay

### 11.1 Transaction order

```text
validate exact proposal
  -> check durable operation identity
  -> append one self-contained canonical event, or append nothing
  -> reconcile SQLite candidate status
  -> rebuild SMR projection from canonical history
  -> return canonical receipt and refreshed preflight
```

The reverse ordering is forbidden:

```text
mark SQLite promoted
  -> attempt canonical append
```

A failed append leaves the proposal noncanonical and reviewable. A SQLite or SMR failure after append is a typed reconciliation failure repaired from canonical history. A lost response is recovered by `operation_id`.

### 11.2 Replay

Replay reconstructs accepted Claims, proof identity, the SMR projection digest, and the rejection-tombstone index from canonical events only.

`provider_calls_during_replay` must be exactly `0`. Any non-zero value is a conformance failure, not a performance note. Replay never reconstructs extraction work and never calls a provider or extractor.

Replay is total over valid canonical history.

`claim_set_candidate_promoted` and `claim_set_candidate_rejected` are self-contained. Candidate generation is daemon-local working state created by `smr.claims.propose` (canonical write: none) and is never a canonical event. Replay must not require a `claim_set_candidate_generated` event or any other Candidate-generation predecessor. A well-formed promotion or rejection payload is valid history even when no local SQLite candidate row exists. Replay validates the event schema, the confirmation fields carried on the event, and the at-most-one terminal disposition per `(product_id, candidate_id, candidate_generation)`. A second terminal event for the same generation is `invalid_canonical_history` on replay (append-time equivalent: `candidate_already_disposed`). A payload that cannot deserialize as its schema is also `invalid_canonical_history`.

Live J2 append still requires the daemon-local candidate generation, ConfirmationChallenge v2, current Product member authority, and a fresh base snapshot, as in section 11.3. Those append-time guards are not replay predecessors. A throwaway-stream fixture may inject a self-contained promotion or rejection event for replay after named owner review; that path is not live J2 and does not skip ConfirmationChallenge on a real Product.

Signoff and supersession events require an eligible Decision predecessor. A missing Decision predecessor returns `invalid_canonical_history` and aborts projection. Silently ignoring an orphan signoff or supersession event is non-conformant.

### 11.3 Atomic promotion guards

The daemon evaluates the following guards in the same canonical compare-and-append operation:

1. the Candidate generation has no terminal disposition;
2. the actor currently holds Product member authority;
3. the proposal's `base_event_position` and `smr_base_snapshot_digest` equal the current canonical Claim projection;
4. the exact confirmation binding remains valid.

Failure appends nothing. A disposed generation returns `candidate_already_disposed`; a changed base returns `base_snapshot_drift`; revoked authority returns `Unauthorized`. The user may request a new analysis generation after drift.

### 11.4 Projection catch-up before reads

Before any successful SMR/preflight read, the daemon compares the SQLite projection cursor with canonical event head and catches up by replay. Success requires:

```text
daemon_local_sqlite.projection_cursor == canonical_event_log.head_position
```

If replay or catch-up fails, the user receives typed `model_unavailable`. The daemon must not return stale results, seed an empty authoritative model, or use SQLite as authority.

## 12. Terminal outcome vocabulary

Test-run states are distinct and are never collapsed into a percentage or a single green or red result.

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

Diagnostic evidence is not terminal evidence. Terminal evidence is not accepted until a named human records what it proves.

## 13. Test groups

Full definitions, positive proofs, negative controls, environments, and expected typed results are in [`docs/domain/smr-candidate-promotion/README.md`](../domain/smr-candidate-promotion/README.md) section 5, and machine-readable in [`test-manifest.schema.json`](../../protocol/smr-claim-acceptance/schemas/test-manifest.schema.json).

| ID | Group | Expected typed result |
| --- | --- | --- |
| T1 | Exact proposal identity | `exact_match` or typed mismatch |
| T2 | Authority boundary | `daemon_only` or typed authority rejection |
| T3 | Canonical transaction and reconciliation | `committed_once`, `not_committed`, or `reconciliation_required` |
| T4 | Restart-safe idempotency | `replayed_result`, `challenge_expired`, or typed stale result |
| T5 | Deterministic replay | `same_projection` or typed replay failure |
| T6 | Working-candidate lifecycle | One explicit lifecycle result, never silent disappearance |
| T7 | Backward compatibility | `compatible` or typed version incompatibility |
| T8 | Released-host terminal topology | `human_accepted` only after a named reviewer inspects the complete receipt |

Every group defines positive proof, negative controls, exact environment, expected typed result, a machine-readable receipt, a human-readable result, and a named human acceptance requirement where applicable.

## 14. Migration and compatibility

1. Legacy candidates default to `kind: decision_v1`. Absence of the discriminator reads as `decision_v1`.
2. New SMR candidates use `kind: smr_claim_set_v1`.
3. Event history is additive and never rewritten. `AcceptCandidate` payload interpretation is unchanged.
4. Local candidate storage gains `generation`, `kind`, `payload`, `digests`, `currentness`, `promotion_eligibility`, `analysis_status`, and `visibility` fields, or documents the equivalent exact JSON contract.
5. RecallPacket renders both kinds without ambiguity.
6. An older client receiving an unsupported version gets a typed `unsupported_contract_version` response rather than a corrupted state.

## 15. Journey contract

Journey: `developer_host_mcp_smr_claim_acceptance_v1`

```text
released developer host
  -> packaged MCP
  -> daemon smr.claims.propose
  -> local candidate generation + deterministic findings
  -> MCP exact-effect review
  -> daemon ConfirmationChallenge v2
  -> human confirm/reject
  -> canonical Product event append (sole commit point)
  -> local candidate/SMR reconciliation
  -> MCP canonical receipt
  -> daemon restart + replay reproduces result
```

- **Actor:** authenticated Product member with candidate-review authority.
- **Entrypoint:** `smr.claims.propose` from the managed MCP surface.
- **Observable success:** the exact Claim set appears after canonical replay, the resulting Decision is `proposed`, and retrying the same operation returns the same receipt with no duplicate append.
- **Exact environments:** `local-product-environment-v1` and `developer-host-environment-v1`.
- **Commit point:** canonical event append only.

Hosted onboarding and directory binding are owner-side prerequisites and are not redefined inside this acceptance journey.

**Journey Boundary Review status: passed for the specification boundary.** Owner-confirmed authority, commit, retry, replay, partial-failure, and witness distinctions are recorded here and in [#943](https://github.com/BicameralAI/bicameral-bot/issues/943). Catalog registration and executable terminal evidence remain delegated to #943. The `developer-host-environment-v1` authentic-host limitation stays visible: this journey cannot claim terminal readiness from component tests.

| Failure | Observable result | Correct retry | Forbidden recovery |
| --- | --- | --- | --- |
| concurrent disposition | `candidate_already_disposed`; one terminal event only | refresh Candidate state | append a second terminal event under another `operation_id` |
| role revoked after challenge | `Unauthorized`; no append | regain authority and request a new challenge | treat the old challenge as authority |
| Claim projection changed after challenge | `base_snapshot_drift`; no append | create a new analyzed generation | silently rebase or reuse the old confirmation |
| append committed, response lost | ambiguous client response | retry the same `operation_id` and binding | mint a new Decision or append again |
| SQLite reconciliation/catch-up fails | `reconciliation_required` after append or `model_unavailable` before read | replay canonical history and retry | return stale/empty SMR or treat SQLite as authority |
| Decision predecessor missing during replay of signoff or supersession | `invalid_canonical_history` | repair canonical history through its owning recovery process | silently skip the orphan event |
| second terminal event for one Candidate generation during replay | `invalid_canonical_history` | repair canonical history | project both terminals |
| well-formed promotion or rejection with no generated-candidate predecessor | reconstruct from the event payload | n/a — this is valid history | fail replay for a missing `claim_set_candidate_generated` event |

The canonical Product event is Product state. Confirmation challenges, SQLite rows, findings, traces, topology receipts, Factory governance results, and exact-head validation receipts are validation or projection witnesses, never substitutes for Product state.

## 16. Conformance obligations

| ID | Obligation |
| --- | --- |
| `CONF-SMR-PROPOSAL-SCHEMA-01` | Proposals validate against `SmrClaimSetProposalV1`; closed enums reject unknown members |
| `CONF-SMR-DIGEST-PROFILE-01` | Digest construction, encoding, normalization, and separate identities per section 4 |
| `CONF-SMR-PROVIDER-BOUNDARY-01` | Provider output is advisory; provider identity is provenance, never Claim identity |
| `CONF-SMR-CONFIRMATION-V2-01` | Challenge v2 shape, 300-second TTL, memory-only, single-use, actor-bound |
| `CONF-SMR-CANONICAL-COMMIT-01` | Canonical append is the sole commit point; reverse ordering is rejected |
| `CONF-SMR-REJECTION-REPLAY-01` | Rejection appends a non-claim-bearing event; tombstone index rebuilds on replay |
| `CONF-SMR-LEGACY-CANDIDATE-COMPAT-01` | `decision_v1` promotion is unaffected; unknown kinds and versions fail closed |
| `CONF-SMR-CLAIM-AUTHORITY-01` | Accepted Claims are `recommendation` while the Decision is `proposed` |
| `CONF-SMR-ACCEPTANCE-JOURNEY-01` | Journey topology, commit point, and replay reproduce the documented result |
| `CONF-SMR-CHALLENGE-BINDING-SUPERSET-01` | v2 binding retains every v1 bound field and every v1 typed mismatch outcome |
| `CONF-SMR-BASE-POSITION-01` | `base_event_position` accompanies `smr_base_snapshot_digest` and makes it re-derivable |
| `CONF-SMR-OPERATION-IDENTITY-01` | `operation_id` is caller-supplied, scoped per Product and actor, and distinct from `request_id` |
| `CONF-SMR-EVENT-BOUNDS-01` | Size bounds enforced; `proposal_too_large` fails closed |
| `CONF-SMR-ROUTING-DERIVATION-01` | `event_stream_id` is daemon-derived; caller-supplied values are rejected |
| `CONF-SMR-TERMINAL-DISPOSITION-01` | At most one terminal event exists per Product/Candidate/generation |
| `CONF-SMR-APPEND-AUTHORITY-01` | Current Product Owner authority is rechecked atomically at append. In this alpha cut every current participant is Product Owner |
| `CONF-SMR-BASE-FRESHNESS-01` | Promotion base equals the current canonical Claim projection at append |
| `CONF-SMR-PROJECTION-CATCHUP-01` | Successful reads require SQLite cursor at canonical head; catch-up failure is visible |
| `CONF-SMR-REPLAY-TOTALITY-01` | Missing Decision predecessors and a second terminal for one Candidate generation fail replay closed. Promotion and rejection events are self-contained and require no generated-candidate predecessor |
| `CONF-SMR-MODEL-ATOM-IDENTITY-01` | Equivalent normalized semantics map to one daemon-verified ModelAtom identity |
| `CONF-SMR-DECISION-AUTHORITY-01` | Alpha claim-set confirm writes `Decision(approved)` with authoritative influence. `Decision(proposed)` is out of this alpha cut on this path |
| `CONF-SMR-CODEGRAPH-RECALL-ONLY-01` | CodeGraph is optional recall expansion, never compliance or SMR closure authority |
| `CONF-SMR-RELATION-SUPPORT-01` | Every relation links to same-set supporting Claims and reachable SourceAnchors; links record input lineage without claiming entailment or authority |

## 17. Amendments to the owner bundle

The architectural direction in [comment 5095866461](https://github.com/BicameralAI/bicameral-bot/issues/937#issuecomment-5095866461) is accepted. These machine choices are replaced.

| # | Owner bundle | Replacement | Reason |
| ---: | --- | --- | --- |
| A1 | Challenge TTL 10 minutes | **300 seconds** | Shipped `CONFIRMATION_CHALLENGE_TTL_SECONDS` is 300; doubling the human-confirmation window widens stale and confused-deputy exposure with no stated justification |
| A2 | v2 binding drops `workspace`, `session_id`, `packet_id` | **v2 binding is a strict superset of v1** | Those fields carry shipped typed fail-closed outcomes (`WorkspaceMismatch`, `PacketMismatch`); routing identity and authorization scope are different concerns |
| A3 | `display_summary` inside `confirmation_binding_digest` | **`display_summary_digest` bound; text excluded from semantic digests** | The bundle both binds the summary and forbids display summaries in digests; a cosmetic wording change must not alter Claim identity |
| A4 | `operation_id` origin unspecified | **Caller-supplied, unique per `(product_id, actor_id)`** | A daemon-minted identifier the caller never observed cannot be retried, defeating restart-safe idempotency |
| A5 | `decision_id: <uuid>` | **Deterministic UUIDv5 over product and operation** | A retry or concurrent submit under one `operation_id` must resolve to one Decision |
| A6 | `smr_base_snapshot_digest` self-referential at append | **`base_event_position` required alongside** | The append changes the projection the digest describes, so without a position the digest is unverifiable on replay |
| A7 | `event_stream_id` selection unspecified | **Daemon-derived; caller-supplied rejected** | Otherwise a caller selects its own canonical destination |
| A8 | Rejection "does not change the effective canonical Claim-history digest" | **`canonical_event_log_digest` advances; `claim_projection_digest` unchanged** | Two named digests make the invariant testable |
| A9 | Lifecycle reasons "such as" / "a closed reason" | **All four reason enums closed and enumerated** | Illustrative reasons are not a durable contract |
| A10 | No digest encoding specified | **`sha256:` prefix, 64 lowercase hex characters** | Two conformant implementations would otherwise disagree byte-for-byte |
| A11 | No size bound on `normalized_claim_set` | **Explicit bounds, typed `proposal_too_large`** | Canonical events are immutable and permanent |
| A12 | "Correct `state-machine-resolver-v1-runtime-architecture.md`, removing `smr.accept_claim_set`" | **Document created here; prohibition recorded as standing constraint** | Neither the document nor any `smr.accept_claim_set` command existed in this repository or any sibling repository |
| A13 | Schemas implied under a new contract root | **`protocol/smr-claim-acceptance/{schemas,fixtures}`** | Matches the shipped hand-authored `protocol/onboarding/` convention; `protocol/schemas/v1` and `v2` are generated from Rust types by `schema_gen` and would drift |
| A14 | `preflight.lookup` named as the read-only successor | **Recorded as forward-looking; not a shipped command** | The shipped read-only path is staged preflight with `lookup.checkpoint_hint` |
| A17 | `operation_id` idempotency treated as terminal-state serialization | **One atomic terminal disposition per Product/Candidate/generation** | Distinct operation IDs can otherwise double-promote or promote/reject concurrently |
| A18 | challenge-time authorization treated as durable authority | **Recheck current Product member/owner role at canonical append** | Revoked actors must not spend stale challenges |
| A19 | bound base snapshot without current-state guard | **Compare base position and digest with current Claim projection at append** | Concurrent claims can change the reviewed effect |
| A20 | projection reconciliation specified only after writes | **Catch up before successful reads or return `model_unavailable`** | Stale/empty fallback would misrepresent Product history |
| A21 | orphan canonical events may be ignored by projection | **Fail replay with `invalid_canonical_history`** | Deterministic replay must be total over valid history |
| A22 | stable semantic ID left to extractor convention | **Daemon verifies identity from normalized kind/key/scope** | Aliases can otherwise hide overlap and contradictions |
| A23 | proposed/approved distinction lacks complete owner transitions | **Encode proposed, approved, rejected, and superseded Claim effects** | Canonical history and governing influence are distinct |
| A24 | CodeGraph framed as implementation-evidence composition | **Optional recall expansion only** | Alpha does not perform code compliance and CodeGraph cannot block SMR closure |
| A25 | `AtomRelation` has no support lineage; proposed derivation categories rely on semantic classification | **Require same-set supporting Claim and reachable SourceAnchor identities; prohibit categorical derivation labels in v1** | Natural-language explicitness is not generally decidable, and extractor self-classification would recreate evidence laundering |
| A26 | A21 required a Candidate-generation predecessor on promotion/rejection replay | **Promotion and rejection events are self-contained; only Decision signoff/supersession requires a canonical Decision predecessor; live J2 append still checks the local candidate** | Candidate generation is daemon-local working state. A generated-candidate event would put unreviewed extract into Product history before human confirm |
| A27 | Member promotion creates `Decision(proposed)` / recommendation; auto-join grants Member only | **Alpha claim-set confirm writes `Decision(approved)` with authoritative influence; every current Product participant is Product Owner; `Decision(proposed)` is out of this alpha cut on this path; Policy Owner remains creator-only** | Alpha scope cut: skip the recommendation window and the member/owner split on confirm |

Items not listed are accepted as written, including the canonical event names, the additive event approach, the digest algorithm and normalization rules, the Claim and ModelAtom collection structure, Product routing exclusions, the minimized candidate lifecycle, rejection durability semantics, the command boundary, the CodeGraph deferral, and the journey topology. Amendment A25 is bound to the owner decision in [issue #937 comment 5246206090](https://github.com/BicameralAI/bicameral-bot/issues/937#issuecomment-5246206090) and detailed in [amendment 003](./state-machine-resolver-v1-runtime-architecture.amendment-003.md). Amendment A26 supersedes the Candidate-generation predecessor clause of A21 / amendment 002 invariant 5 and is detailed in [amendment 004](./state-machine-resolver-v1-runtime-architecture.amendment-004.md). Amendment A27 supersedes A23 and the alpha reading of ADR-0022's member-creates-proposed path, and is detailed in [amendment 005](./state-machine-resolver-v1-runtime-architecture.amendment-005.md).

## 18. What this contract does and does not authorize

**Authorized:** the field vocabulary, enums, digest rules, event shapes, transition semantics, replay obligations, and conformance IDs above are settled and may be cited as the contract an implementation must satisfy.

**Not authorized:** implementation, deployment, Release Unit creation, Product acceptance, released-host validation, issue creation while `create_issues: false` remains authoritative, and any change to the CodeGraph proof carrier owned by #936.

## 19. Source implementation anchors

- [`crates/bicameral-api/src/candidate.rs`](../../crates/bicameral-api/src/candidate.rs)
- [`crates/bicameral-api/src/candidate_promotion.rs`](../../crates/bicameral-api/src/candidate_promotion.rs)
- [`crates/bicameral-api/src/confirmation_challenge.rs`](../../crates/bicameral-api/src/confirmation_challenge.rs)
- [`crates/bicameral-api/src/event_store.rs`](../../crates/bicameral-api/src/event_store.rs)
- [`crates/bicameral-api/src/recall.rs`](../../crates/bicameral-api/src/recall.rs)
- [`crates/bicameral-runtime/src/confirmation_challenge_store.rs`](../../crates/bicameral-runtime/src/confirmation_challenge_store.rs)
- [`crates/bicameral-runtime/src/dispatch.rs`](../../crates/bicameral-runtime/src/dispatch.rs)
- [`crates/bicameral-gateway/src/live_projection_work.rs`](../../crates/bicameral-gateway/src/live_projection_work.rs)
