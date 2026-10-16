# SMR Alpha Managed-Preflight and Candidate-Promotion Journeys

**Status:** Product-owner-confirmed journey semantics; implementation and
terminal evidence pending  
**Date:** 2026-07-30
**Scope:** SMR alpha sprint  
**Owner decisions:** [#936](https://github.com/BicameralAI/bicameral-bot/issues/936),
[#937](https://github.com/BicameralAI/bicameral-bot/issues/937),
[#938](https://github.com/BicameralAI/bicameral-bot/issues/938),
[#943](https://github.com/BicameralAI/bicameral-bot/issues/943),
[#967](https://github.com/BicameralAI/bicameral-bot/issues/967), and
[PR #942](https://github.com/BicameralAI/bicameral-bot/pull/942), with the
optional CodeGraph seam reconciled by
[PR #948](https://github.com/BicameralAI/bicameral-bot/pull/948), with
proof-preserving MCP transport selected in
[MCP PR #783](https://github.com/BicameralAI/bicameral-mcp/pull/783)

**Normative downstream contract:**
`docs/specs/state-machine-resolver-v1-runtime-architecture.md` and its merged
machine contract

Governance gate: `bic:spec-governance-gate`; Qor advisory/no-op pending
Factory policy revision. UserJourneys:
`developer_host_mcp_smr_managed_preflight_v1` and
`developer_host_mcp_smr_candidate_promotion_v1`. Journey Boundary Review:
passed for Product semantics; catalog registration, machine-contract
adaptation, and terminal authentic-host evidence remain pending. Derived
readiness: `caller_qa_required`.

This document defines two independent user journeys. Managed Preflight contains
Claim Proposal and read-only Preflight as two business/runtime stages of one
developer journey. Candidate Promotion is a separate journey because it
requires an explicit human gesture and may cross the canonical append boundary.
Keeping the commands separate preserves authority and retry semantics; it does
not make the first two stages separate user journeys. Read-only Preflight
contains two independent result lanes: deterministic grounded recall and deep
semantic analysis.

## Normative Ordering and Authority

```text
requested Product/workspace/head context
  -> Managed Preflight starts deterministic grounded recall
       -> CodeGraph target resolution
       -> accepted BindingEvidence backtrace
       -> exact accepted Claim revisions and ModelAtoms
  -> completed planning turn independently starts deep semantic analysis
       -> host Plan hook captures the exact final self-contained plan
       -> local ingest records the planning-context source snapshot
       -> smr.claims.propose when current semantic projection is absent
       -> preflight.run over that same semantic projection
  -> host renders each result without waiting for the other
  -> recall.promote_decision_candidate or existing rejection surface
  -> refreshed preflight.run
```

The commands retain separate authority:

- Plan capture and local ingest create evidence, not accepted Product Claims.
- `smr.claims.propose` may stage daemon-validated local working candidates.
- `preflight.run` is always read-only and must never create, update, promote,
  reject, or canonicalize a candidate.
- Deterministic grounded recall may report only accepted BindingEvidence-backed
  applicability at one pinned GraphSnapshot. It cannot report compliance,
  currentness, approval, safety, or an all-clear.
- Deep semantic analysis does not require CodeGraph and cannot reuse code scope
  as semantic scope.
- Candidate Promotion is the only journey in this sequence that may admit the
  exact reviewed Claim set into the accepted Product model.

## Joined Happy Path

```text
Pinned Product/workspace/head + completed planning turn
                 |
          +------+------+
          |             |
          v             v
+------------------+  +----------------------------------------+
| LANE A           |  | LANE B / STAGE A                       |
| GROUNDED RECALL  |  | PLAN SEMANTIC PROJECTION               |
|                  |  |                                        |
| CodeGraph target |  | exact Plan capture + local ingest      |
| -> accepted      |  | -> semantic projection                 |
| BindingEvidence  |  | -> impact atoms + optional candidate   |
| -> Claim/Atoms   |  +-------------------+--------------------+
+--------+---------+                      |
         | first grounded result          v
         |                     +--------------------------------+
         |                     | LANE B / STAGE B               |
         |                     | DEEP SEMANTIC ANALYSIS         |
         |                     |                                |
         |                     | accepted-model alignment       |
         |                     | -> semantic scope assessment   |
         |                     | -> bounded Alloy checks        |
         |                     +----------------+---------------+
         |                                      |
         +-------------------+------------------+
                             v
                   independent read-only results
                             |
                   candidate needs review?
                      |              |
                     no             yes
                      |              v
             developer continues   J2 Candidate Promotion
                                      |
                             canonical disposition
                                      |
                             refreshed preflight
```

Candidate and `no_candidate` are system outcomes, not user choices. A
`no_candidate` result means the Plan introduced no novel or changed Claim; it
does **not** mean that the Plan had no impacted ModelAtoms. A successful
analysis always emits its impact atoms and requires no governance gesture when
no candidate is needed.

## Plan Source Snapshot

J1 consumes one `PlanningContextSnapshotV1`, not a repository specification
file and not the full coding-agent transcript.

```yaml
PlanningContextSnapshotV1:
  source_kind: planning_context_v1
  product_id: string
  host_kind: claude_code | codex
  host_session_id: string
  host_turn_id: string
  capture_event: claude_exit_plan_mode | codex_plan_turn_complete
  final_plan_text: string
  content_digest: sha256:<lowercase-hex>
  repository_context: optional_advisory_object
```

The source identity is:

```text
(product_id, source_kind, host_kind, host_session_id, host_turn_id,
 content_digest)
```

`repository_context` may help retrieval, but it is not source identity,
Product authority, or proof that the Plan was implemented in code.

Capture is host-specific:

- Claude Code uses the successful `PostToolUse` event for `ExitPlanMode` and
  captures the exact approved `tool_response.plan`.
- Codex joins the Plan-mode `Stop` event with the matching
  `agent-turn-complete` notification by session and turn identity, then
  captures the exact `last-assistant-message`.
- The adapter must not infer the source by scraping an unstable transcript,
  concatenate intermediate assistant messages, or ingest user/tool chatter.
- If the required exact message or event join is unavailable, capture is
  incomplete and J1 must not claim proposal coverage.

## J1 - Managed Preflight

### Stage A - Plan Semantic Projection

```yaml
user_journey:
  id: developer_host_mcp_smr_managed_preflight_v1
  name: SMR Managed Preflight
  actor: product_participant_using_a_coding_agent
  entrypoint: pinned_work_context_with_optional_completed_plan_capture
  success_state: independent_grounded_and_semantic_lane_status_visible
  surfaces:
    - released_developer_host
    - managed_mcp_surface
    - local_bot_daemon
  boundary_contracts:
    - PlanningContextSnapshotV1
    - accepted BindingEvidence
    - ScopeSufficiencyProof
    - SmrCheckScopeAssessmentV1
    - ingest.submit_local
    - smr.claims.propose
    - preflight.run
    - RecallPacket
  required_evidence:
    - producer_consumer_contract_tests
    - real_process_plan_hook_test
    - real_process_fast_grounded_recall_test
```

The complete analysis key extends source identity with:

```text
(accepted SMR base position, accepted SMR base digest,
 extraction profile version)
```

The managed surface orchestrates capture, `ingest.submit_local`, and
`smr.claims.propose`, followed by `preflight.run`, as one user journey. The user
does not make a second proposal or preflight gesture. Local ingest may persist
eligible source evidence through its existing evidence-authority path. That
does not make extracted Claims accepted.

The stage output is one `PlanSemanticProjectionV1`:

```yaml
PlanSemanticProjectionV1:
  analysis_key: sha256:<source identity + accepted SMR base + extraction profile>
  source_binding_digest: sha256:<PlanningContextBinding>
  accepted_smr_base_position: integer
  accepted_smr_base_digest: sha256:<accepted Claim projection>
  impact_atoms:
    - model_atom_id: daemon-verified identity
      normalized_semantics: typed atom tuple
      source_anchors: [plan span or explicit target]
      confidence: exact | strong | exploratory
  proposal_outcome: candidate_projected | no_candidate | incomplete
  proposed_claim_set: optional SmrClaimSetProposalV2
  coverage_limitations: []
  provider_provenance: caller | local
```

For alpha, the default whole-Plan method is bounded caller-LLM enrichment at
this stage. It examines the exact final Plan plus caller-supplied file, symbol,
and repository hints and emits normalized impact atoms even when the Plan does
not name files. The daemon validates every atom identity and output bound. An
explicit file/symbol target is useful evidence but is not required for the
analysis to run.

This enrichment happens once per analysis key. Read-only Preflight consumes the
completed projection and must not invoke a second LLM to reinterpret the same
Plan. This avoids two probabilistic selectors disagreeing about which atoms the
Plan impacts.

If a completed `candidate_projected` or `no_candidate` projection already exists
for the same analysis key, the daemon reuses it. Deterministic hook firing
guarantees invocation, not deterministic LLM output. Repeated firing must not
create another generation for unchanged source bytes and analysis inputs.

The extractor may produce multiple Claims inside one candidate. The daemon
partitions candidates only when the Plan governs genuinely disconnected
behavioral scopes. It must not partition by sentence, paragraph, chunk, source
span, or provider-output item.

Every staged candidate remains daemon-local, disposable, noncanonical working
state. Extraction never creates a Decision, signoff, accepted Claim, or
Product authority.

### Stage B - Independent Read-Only Preflight Results

`preflight.run` first catches the materialized SMR projection up to canonical
Product history. If catch-up fails, it informs the user and does not return an
empty or stale accepted model as success.

The deterministic grounded-recall lane does not wait for Plan semantic
projection:

```text
pinned workspace/head + diff/files/symbols
  -> deterministic CodeGraph target resolution
  -> accepted BindingEvidence backtrace
  -> exact accepted Claim revisions and ModelAtoms
```

The daemon resolves the inverse relationship. CodeGraph supplies pinned target
and graph evidence; it does not select Claims. Each grounded result retains the
exact repository, head, GraphSnapshot, BindingEvidence, Claim revision,
ModelAtom, match reason, searched scope, unknown coverage, and
`ScopeSufficiencyProof`. The lane invokes no LLM.

The deep semantic lane retrieves accepted Claims by joining the projection's
impact atoms against accepted ModelAtoms and their Claim relations:

```text
Plan impact atoms
  -> exact normalized atom overlap
  -> bounded accepted atom-relation closure
  -> reverse join ModelAtom -> accepted Claims
  -> rank by typed selection reason
```

The mandatory deep semantic selector is deterministic after Stage A:

| Route | Recall effect | Accuracy/confidence treatment |
| --- | --- | --- |
| exact normalized atom identity | recalls every accepted Claim asserting that atom | `exact`; highest confidence |
| typed accepted atom relation | adds Claims within the bounded relation closure | `strong`; relation and path are retained |
| explicit accepted artifact/symbol binding and CodeLocator | adds impact-atom selectors | additive; binding/locator provenance is retained |
| optional semantic-lane CodeGraph candidate | adds impact-atom selectors or trusted-corpus search keys | additive and advisory; graph identity and limitations are retained |
| text/embedding fallback over accepted Claims | may add exploratory candidates when exact routes are sparse | `exploratory`; never silently upgraded to exact |

The first two routes are the default corpus method for a completed projection.
They are deterministic once the impact-atom set exists, but no natural-language
projection is foolproof: a missed impact atom can still reduce recall.
Projection coverage and fallback-route bounds must therefore remain visible.
CodeLocator and the future impact-ModelAtom -> code adapter may improve
deep-lane recall but do not replace the default.
Every returned Claim records the route, contributing impact atom, accepted atom
or relation path, and confidence tier. The resolver reports bounded or
unsearched routes instead of converting low recall into an all-clear.

The four semantic cases are:

1. same atom with a compatible assertion: recall the existing governing Claim;
2. same atom with an incompatible assertion: surface a contradiction and, when
   the Plan proposes changed semantics, retain the local candidate;
3. related atom: recall through the typed bounded relation closure; and
4. novel atom: keep it in `impact_atoms` and, when it asserts new semantics,
   construct a local candidate only—never an accepted Claim.

Preflight may render local staged candidates and contradictions alongside the
selected accepted Claims. It must label the two authority classes separately:

```text
accepted Claim       canonical Product history
unaccepted candidate local working proposal
```

The two scope contracts remain independent:

- CodeGraph `ScopeSufficiencyProof` states bounded code/graph coverage for the
  pinned grounded-recall request.
- `SmrCheckScopeAssessmentV1` states business-logic sufficiency for one named
  bounded Alloy check.

Missing, queued, stale, partial, ambiguous, unsupported, unavailable, or
mismatched CodeGraph evidence produces a typed grounded-lane limitation. It
does not block deep semantic analysis or weaken its result. Missing semantic
scope never borrows code scope. CodeGraph never proves implementation
compliance, closes SMR, accepts a Claim, or changes Product authority.

The daemon records graph preparation, warm grounded recall, time to first
grounded result, and deep-analysis completion as separate milestones. MCP must
render a completed grounded result while deep analysis remains pending.

Preflight may explain a candidate and offer entry into Candidate Promotion. It
must not silently fold candidate content into the accepted model, strengthen
its authority, or clear a contradiction.

## J2 - Candidate Promotion

```yaml
user_journey:
  id: developer_host_mcp_smr_candidate_promotion_v1
  name: Candidate Promotion
  actor: authenticated_product_member
  entrypoint: unaccepted_candidate_shown_by_preflight_or_recall
  success_state: canonical_receipt_and_refreshed_recall_show_selected_outcome
  surfaces:
    - managed_mcp_surface
    - local_bot_daemon
    - canonical_product_event_store
  boundary_contracts:
    - recall.promote_decision_candidate
    - ConfirmationChallengeV2
    - claim_set_candidate_promoted
    - claim_set_candidate_rejected
  required_evidence:
    - producer_consumer_contract_tests
    - restart_replay_test
    - real_process_candidate_promotion_test
```

The member reviews the exact candidate evidence, behavioral scope, Claim set,
and effect, then chooses one eligible outcome:

- establish a new constraint;
- supersede an existing Decision;
- keep both with explicit non-overlapping scope;
- route for signoff review;
- reject the candidate as noise.

One explicit human confirmation authorizes one terminal disposition for the
exact candidate generation. Promotion atomically appends the self-contained
Claim set and creates `Decision(approved)`. Its Claims are canonical,
recallable, and authoritative. Rejection records a durable non-claim-bearing
event and adds nothing to the effective Claim projection.

After canonical append, the daemon reconciles local candidate state, catches
up or rebuilds the SMR projection, and reruns read-only preflight. Alpha
skips a later owner `approve_signoff` on this path: confirm already writes
approved. Every current Product participant is Product Owner for this alpha
cut. Policy Owner remains creator-only.

## Current Machine-Contract Seam

The merged `SmrClaimSetProposalV1` contract requires `ArtifactBinding`:
repository, Git head, artifact path, blob, and content digest. That binding
cannot honestly encode a planning-session message. A fabricated virtual file
or borrowed repository path is forbidden.

The Product contract decision is:

```text
SmrClaimSetProposalV2
  source_binding =
    artifact_v1(ArtifactBinding)
    | planning_context_v1(PlanningContextBinding)

ConfirmationChallengeV3.proposal_identity
  binds source_binding_digest instead of artifact_binding_digest
```

`PlanningContextBinding` contains the source-identity fields from
`PlanningContextSnapshotV1` plus `content_digest`; the Plan text remains in
source evidence and is addressed by that digest. Digest profile v2
domain-separates `source-binding`, includes the binding discriminator, and
preserves the existing independent Claim-set, SMR-base, scope, evidence, and
confirmation digests. The daemon must reject a v1/v2 or v2/v3 cross-version
mix rather than translate it implicitly.

Therefore:

- the merged v1 contract remains authoritative for its existing
  artifact-bound proposal and all downstream confirmation, append,
  idempotency, rejection, and replay semantics;
- this document defines the new journey-level
  `PlanningContextSnapshotV1` source seam;
- full production conformance requires the machine-contract amendment that
  codifies `SmrClaimSetProposalV2`, digest profile v2, and
  `ConfirmationChallengeV3`;
- an isolated implementation may mock or adapt this seam and reuse the
  downstream v1 state, authority, append, idempotency, rejection, and replay
  rules, but it must report proposal-envelope and digest conformance as
  unverified until that amendment lands.

This is a conformance gap, not a reason to postpone isolated validation of
capture, extraction, candidate staging, read-only preflight, promotion,
idempotency, and replay.

## Partial Failure and Retry Boundary

| Boundary | User-visible result | Correct continuation | Forbidden recovery |
| --- | --- | --- | --- |
| Exact Plan-turn capture is incomplete | Proposal coverage is unavailable | repair the host event join and retry the same turn | scrape the full transcript or guess the final Plan |
| Claim extraction cannot complete | Proposal coverage is explicitly incomplete; accepted-model preflight may run with that limitation | repair extraction and retry the same analysis key | manufacture a fallback candidate or imply complete coverage |
| Repeated hook for unchanged analysis key | Existing candidate or `no_candidate` outcome is reused | continue to read-only preflight | create duplicate candidate generations |
| CodeGraph is absent or noncurrent | Grounded recall is typed unavailable, limited, or unknown; deep semantic analysis continues independently | refresh the pinned graph request or continue with the semantic lane | claim code compliance, borrow semantic scope, or erase the semantic result |
| BindingEvidence, head, or GraphSnapshot identity mismatches | Grounded match is rejected with an identity limitation | rerun against one exact pinned tuple | relabel the binding as current or reuse it across heads |
| Semantic scope is insufficient for an absence-sensitive check | Deep result is `unknown_scope` | repair semantic coverage and retry the exact analysis identity | borrow code scope or emit no-counterexample |
| Candidate Promotion commits but response is lost | Outcome is initially ambiguous | retry the same bound operation and return the canonical receipt | append another terminal disposition |
| Projection cannot catch canonical head | SMR result is unavailable and the user is informed | catch up from canonical history, then rerun preflight | return an empty or stale accepted model |

## State Refinement and Minimization

- **Smallest release state:** read-only Preflight may expose an independently
  completed grounded-recall result and a completed, pending, or explicitly
  limited deep semantic result; only confirmed Candidate Promotion changes
  accepted Product history.
- **Product states added:** none.
- **Local working state refined:** `DecisionCandidate` may carry one
  `smr_claim_set_v1` proposal generation derived from a Plan snapshot.
- **Operational witnesses:** capture joins, analysis receipts, CodeGraph proof
  carriers, and topology receipts are evidence, not Product states.
- **Canonical state reused:** existing
  `Decision(proposed|approved|rejected|superseded)` lifecycle.
- **Rejected states:** `plan_captured`, `preflight_accepted`,
  `candidate_in_model`, `hook_complete`, `codegraph_complete`,
  `topology_passed`, and `sqlite_synced`.
- **Invariant:** preflight never writes canonical or local candidate state.
- **Invariant:** BindingEvidence proves code applicability only at its pinned
  GraphSnapshot; it does not prove compliance or currentness.
- **Invariant:** code scope and semantic check scope never substitute for one
  another.
- **Invariant:** only Candidate Promotion crosses the accepted Claim-model
  append boundary.

## Journey Boundary Review

```yaml
journey_boundary_review:
  status: passed
  journey_ids:
    - developer_host_mcp_smr_managed_preflight_v1
    - developer_host_mcp_smr_candidate_promotion_v1
  authoritative_commit_points:
    - planning_source_evidence_ingest
    - local_candidate_stage
    - canonical_candidate_disposition_append
  independently_observable_facts:
    - deterministic_grounded_recall_result
    - grounded_recall_scope_and_latency
    - exact_plan_snapshot
    - deep_semantic_analysis_outcome
    - local_candidate_generation
    - read_only_preflight_result
    - canonical_candidate_disposition
    - caught_up_smr_projection
  partial_failure_reviewed: true
  retry_and_idempotency_reviewed: true
  validation_witness_separated_from_product_state: true
  user_confirmed: true
  source_ref: docs/specs/smr-alpha-managed-preflight-user-journeys.md
```

Catalog registration, the planning-context machine-contract amendment, and
terminal authentic-host evidence remain pending. Bot #820 owns fast-lane
supporting validation; Bot #943 owns the integrated two-lane authentic-host
journey. MCP PR #783 is the selected proof-preserving transport. The nearest catalog path,
`developer_host_mcp_v1`, remains partial and does not by itself prove Plan
capture, Claim Proposal, Candidate Promotion, canonical append, or
restart/replay.

Release Unit: not applicable to this documentation-only journey clarification.
This document does not itself authorize deployment, release, or Product
acceptance.
