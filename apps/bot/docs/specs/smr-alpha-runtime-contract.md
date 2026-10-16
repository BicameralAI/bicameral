# SMR Alpha Runtime Contract

**Date:** 2026-07-30
**Status:** proposed contract
**Level:** L2 bot-owned runtime contract
**Related:** BicameralAI/bicameral-bot#936; ADR-0028;
[Why SMR and Spec Governance](../strategy/why-smr-and-spec-governance.md);
`constraint-lookup-recallpacket-contract.md`;
`preflight-impact-to-decision-recall-strategy.md`;
`deterministic-code-locator-graph-workflow.md`

## Purpose

For the product **why** (democratizing formal verification at decision boundaries; underdetermined escalation), see [Why SMR and Spec Governance](../strategy/why-smr-and-spec-governance.md).

This is the runtime companion contract for the SMR alpha. It defines the
daemon-authored, read-only tracked-spec recall result used by `preflight.run`
and rendered by MCP. It exposes deterministic grounded recall and deep semantic
analysis as independent lanes. Core SMR does not define implementation
verification or compliance.

For configured workspaces, CodeGraph supplies pinned target resolution and
graph-scope evidence to the deterministic grounded lane. The daemon backtraces
accepted BindingEvidence to exact accepted Claim revisions and ModelAtoms.
CodeGraph does not select Claims. Deep semantic analysis remains valid without
CodeGraph.

```text
MCP request
  -> daemon pins Product/workspace/repository/head context
  -> Lane A calls codegraph.ensure_current and resolves task targets
       -> accepted BindingEvidence backtrace
       -> exact accepted Claim revisions and ModelAtoms
       -> first grounded result without an LLM
  -> Lane B independently resolves the exact Plan/source snapshot
       -> one PlanSemanticProjection
       -> accepted Claims selected from impact atoms
       -> SmrCheckScopeAssessmentV1
       -> bounded semantic analysis
  -> daemon returns independent lane status and latency milestones
  -> MCP renders each lane without recomputation or serial waiting
```

`codegraph.ensure_current` with wire contract
`codegraph.ensure-current/v1` is a target contract. The current
`graph.refresh_snapshot` and `bounded_refresh_check` implementations do not yet
satisfy it because they do not start missing/stale materialization or return a
stable refresh job.

## Ownership And Authority

| Component | Owns | Must not |
|---|---|---|
| daemon | orchestration, target pinning, BindingEvidence backtrace, trusted-corpus recall, SMR result identity | convert graph readiness or applicability into compliance or Product authority |
| CodeGraph | derived graph cache, materialization jobs, snapshots, target resolution, scope/proof identity | select Claims, evaluate compliance, or mutate Product state |
| SMR resolver | matching product-approved specs from the named trusted corpus | treat a graph candidate as an applicable spec |
| MCP | request transport and faithful rendering | refresh, recompute, or strengthen the daemon result |
| Code Evidence/Compliance | bindings, implementation evidence, currentness, compliance observations | silently reuse an SMR graph result against a different identity |

Calling `codegraph.ensure_current` may mutate daemon-owned derived cache and
job state. It must not mutate accepted Decisions or Claims, bindings, signoff,
compliance, the canonical event-store digest, or any other Product state.

## Core Result Identity

Every result binds one requested context and keeps lane identities separate:

```text
requested Product/workspace/repository/head identity

grounded_recall:
  graph snapshot + scope fingerprint + ScopeSufficiencyProof
  accepted BindingEvidence identities
  exact accepted Claim revisions + contributing ModelAtom identities
  match reasons + searched scope + unknown coverage

deep_semantic_analysis:
  input_artifact_context_digest
  trusted_spec_corpus_revision
  plan_semantic_projection_digest
  impact_atom_set_digest
  accepted_smr_base_position + accepted_smr_base_digest
  recalled Claim revisions + contributing accepted ModelAtom identities
  SmrCheckScopeAssessmentV1
  missing_or_unknown_coverage_findings
  proposed_claim_digest

latency_milestones:
  graph preparation
  warm grounded recall
  time to first grounded result
  deep semantic completion
```

The daemon computes these values. MCP must not construct or substitute them.
An accepted claim-bearing event, when one is separately governed and accepted,
is canonical on the Product event substrate; a local SMR projection is
rebuildable derived state.

## Core Semantic Projection And Claim Selection

The authoritative managed-preflight journey and machine boundary are defined
by `smr-alpha-managed-preflight-user-journeys.md` and
`state-machine-resolver-v1-runtime-architecture.md`.

Grounded recall does not ask an LLM to select accepted Claims. The daemon
resolves pinned code targets, traverses accepted BindingEvidence, and resolves
the referenced accepted Claim revisions and ModelAtoms against accepted SMR
state.

Deep semantic recall also does not ask an LLM to select accepted Claims
directly. The daemon-mediated proposal stage performs one bounded semantic
enrichment of the exact Plan/source snapshot and produces
`PlanSemanticProjectionV1`. A successful projection always contains normalized
`impact_atoms`, even when `proposal_outcome` is `no_candidate`.

Read-only `preflight.run` then uses deterministic accepted-model operations:

```text
impact_atoms
  -> exact accepted ModelAtom identity/semantic overlap
  -> bounded typed accepted-atom relation closure
  -> reverse join to accepted Claims
```

The future impact-ModelAtom -> code-recall adapter may add selectors or
trusted-corpus search keys to the semantic lane. It remains additive. It is
distinct from the inverse code-target -> BindingEvidence -> Claim/ModelAtom
grounded lane. Neither route constructs a Claim, changes an accepted ModelAtom,
or replaces the Plan projection.

Every core match records:

- accepted Claim and revision identity;
- contributing Plan impact-atom identity;
- contributing accepted ModelAtom identity;
- `selection_reason` of `exact_atom`, `related_atom`,
  `accepted_binding`, `codelocator`, `codegraph_expansion`, or
  `exploratory_text`;
- relation/binding/locator provenance when applicable; and
- confidence tier `exact`, `strong`, or `exploratory`.

The resolver must report route bounds and uncertainty. A sparse or empty
selection is never silently strengthened into corpus completeness or
implementation all-clear.

## Result Shape

```json
{
  "contract_version": "smr-alpha-result/v2",
  "requested_context": {
    "product_id": "product_...",
    "workspace_id": "workspace_...",
    "repository": "owner/repo",
    "head_commit": "git-sha"
  },
  "grounded_recall": {
    "status": "not_requested | ready | pending | limited",
    "result_kind": "deterministic_grounded_recall",
    "identity": {
      "target_commit_sha": "git-sha",
      "job_id": "cgjob_...",
      "graph_snapshot_id": "graph_...",
      "scope_fingerprint": "sha256:...",
      "scope_proof_id": "proof_..."
    },
    "matches": [
      {
        "claim_id": "claim_...",
        "claim_revision": 1,
        "contributing_model_atom_ids": ["atom_..."],
        "binding_evidence_id": "binding_...",
        "selection_reason": "binding_backtrace",
        "authority": "product_approved",
        "applicability_only": true
      }
    ],
    "searched_scope": [],
    "unknown_coverage": [],
    "limitation": null
  },
  "deep_semantic_analysis": {
    "status": "not_requested | pending | bounded_result | incomplete | disputed | unsupported | unavailable",
    "input_artifact_context_digest": "sha256:...",
    "trusted_spec_corpus_revision": "corpus-revision",
    "plan_semantic_projection_digest": "sha256:...",
    "impact_atom_set_digest": "sha256:...",
    "accepted_smr_base": {
      "event_position": 123,
      "projection_digest": "sha256:..."
    },
    "proposal_outcome": "candidate_projected | no_candidate | incomplete",
    "core_matches": [],
    "scope_assessment": {
      "contract_version": "smr-check-scope-assessment/v1",
      "check_id": "check_...",
      "status": "sufficient_for_named_bounded_check | insufficient | unknown_scope",
      "snapshot_digest": "sha256:...",
      "exact_bounds": {}
    },
    "missing_or_unknown_coverage": [],
    "searched_scope": [],
    "proposed_claim_digest": "sha256:...",
    "result_digest": "sha256:..."
  },
  "latency_milestones": {
    "graph_preparation_ms": 0,
    "warm_grounded_recall_ms": 0,
    "time_to_first_grounded_result_ms": 0,
    "deep_semantic_analysis_ms": 0
  },
  "result_digest": "sha256:..."
}
```

Each lane object is always present so a consumer can distinguish not requested,
pending, ready, and limited without inferring from missing fields. A completed
grounded result remains renderable while deep analysis is pending.

`proposed_claim_digest` is present only when the deep lane's
`proposal_outcome` is `candidate_projected`. Candidate Promotion binds and
retains that exact digest rather than recomputing it.

`SmrCheckScopeAssessmentV1` binds exact Plan/source coverage, accepted SMR base,
included Claim revisions, ModelAtoms and relations, supported and unknown
semantic dimensions, required closure Claims, serialized snapshot
cardinalities and digest, named check, and exact bounds. It is independent from
the grounded lane's CodeGraph `ScopeSufficiencyProof`.

## CodeGraph Ensure-Current Operation

### Operation and versioning

The daemon calls the CodeGraph-owned operation:

```text
codegraph.ensure_current(
  EnsureCurrentRequestV1
) -> EnsureCurrentResultV1
```

The operation is internal to the daemon/CodeGraph boundary. It is not an
additional MCP action. `preflight.run` remains the single user-visible action.

Both request and result carry the literal contract version
`codegraph.ensure-current/v1`. An unsupported major version returns
`limited(incompatible_contract)` for the grounded lane. The daemon retains any
independent deep semantic result and must not reinterpret an older or newer
result shape. Additive fields within v1
may be ignored only when they are not marked required by the producer.

### Request type

```json
{
  "contract_version": "codegraph.ensure-current/v1",
  "expansion_request_id": "sha256:...",
  "purpose": "deterministic_grounded_recall | semantic_expansion",
  "repository_id": "repo_...",
  "workspace_identity": "sha256:local-workspace-binding",
  "target": {
    "commit_sha": "git-sha",
    "ref_provenance": "refs/heads/dev"
  },
  "graph_implementation": {
    "provider_id": "provider-name",
    "provider_version": "version-or-not_applicable",
    "index_version": "version-or-not_applicable",
    "parser_version": "version-or-not_applicable",
    "resolver_version": "version-or-not_applicable",
    "chunker_version": "version-or-not_applicable",
    "semantic_index_version": "version-or-not_applicable",
    "schema_version": "version-or-not_applicable"
  },
  "scope": {
    "kind": "diff | file_set | symbol_set | query | binding_revalidation",
    "fingerprint": "sha256:canonical-scope",
    "base_commit_sha": "git-sha-or-null",
    "files": [],
    "symbols": []
  },
  "policy": {
    "mode": "bounded",
    "max_targets": 64,
    "wait_budget_ms": 500,
    "queue_ttl_ms": 900000,
    "trigger": "managed_preflight",
    "compatibility_discriminator": "smr-codegraph-preflight-v1"
  }
}
```

The daemon resolves and pins `target.commit_sha` before calling CodeGraph.
`ref_provenance` is diagnostic provenance only and must never substitute for
the commit. `workspace_identity` is a stable digest of the explicit local
workspace binding, not a raw customer path.

All implementation-version fields are required. A component that does not
participate records `not_applicable`; omission is not equivalent to
compatibility.

The daemon derives `expansion_request_id` from the SMR input digest, corpus
revision, pinned target, scope fingerprint, and compatibility discriminator.
This joins the optional branch to the final SMR result but is not the
materialization job key.

The proposed alpha bounded policy waits for at most 500 milliseconds and admits
at most 64 file and symbol targets. The target-count limit comes from #646; the
500-millisecond wait and 15-minute queue TTL are new #936 policy choices that
require acceptance with this contract rather than being attributed to #625 or
#646.
The effective wait is the lesser of 500 milliseconds and the remaining parent
`preflight.run` deadline. A zero remaining budget performs attach/check and
returns immediately. Full or unbounded repository indexing is not authorized
on this request path.

### Result type and status vocabulary

```json
{
  "contract_version": "codegraph.ensure-current/v1",
  "expansion_request_id": "sha256:...",
  "status": "ready | pending | limited",
  "pinned_target": {
    "repository_id": "repo_...",
    "workspace_identity": "sha256:...",
    "commit_sha": "git-sha",
    "ref_provenance": "refs/heads/dev"
  },
  "snapshot": {
    "graph_snapshot_id": "graph_...",
    "commit_sha": "git-sha",
    "implementation_identity_digest": "sha256:..."
  },
  "scope": {
    "kind": "file_set",
    "fingerprint": "sha256:...",
    "graph_slice_id": "slice_...",
    "scope_proof_id": "proof_..."
  },
  "job": {
    "job_id": "cgjob_...",
    "job_key": "sha256:...",
    "attempt": 1,
    "state": "queued | indexing | ready | failed | cancelled | expired | superseded",
    "retry_token": "opaque-stable-token",
    "retry_after_ms": 250,
    "expires_at": "RFC3339 timestamp",
    "superseded_by_commit_sha": null
  },
  "candidate_regions": [
    {
      "candidate_id": "candidate_...",
      "kind": "file | symbol | module",
      "locator": "provider-owned-locator",
      "candidate_provenance_digest": "sha256:..."
    }
  ],
  "limitation": {
    "reason": "not_configured | unsupported | over_budget | stale_unmaterialized | materialization_failed | cancelled | expired | incompatible_contract | target_mismatch",
    "detail": "bounded non-authoritative diagnostic",
    "retryable": true
  }
}
```

Field requirements by status:

| Status | Required | Forbidden interpretation |
|---|---|---|
| `ready` | pinned target, exact snapshot, exact scope, candidate regions (possibly empty); job when materialization or reuse is job-backed | ready is not corpus completeness, applicability, compliance, or all-clear |
| `pending` | pinned target and stable job identity/token; no candidate may be consumed | queued/indexing is not a negative graph result |
| `limited` | typed limitation and every identity known before failure; terminal job when one exists | limitation does not invalidate the independent deep semantic result |

`stale` and `missing` are observed pre-materialization conditions, not terminal
success statuses. When bounded materialization is supported they lead to a
started or attached job. They become `limited(stale_unmaterialized)` only when
CodeGraph cannot start bounded work under the supplied policy.

## Job Identity, Coalescing, And Retry

The canonical job key is the digest of:

```text
repository_id
workspace_identity
target.commit_sha
provider_id + provider_version
index_version
parser_version
resolver_version
chunker_version
semantic_index_version
schema_version
scope.kind
scope.fingerprint
policy.mode
policy.max_targets
policy.compatibility_discriminator
contract major version
```

The ref name, expansion request ID, trigger, wait budget, queue TTL, wall-clock
deadline, and caller request ID are provenance or scheduling inputs. They do
not change the graph artifact and therefore do not form the job key.

CodeGraph maintains a durable, rebuildable local job registry keyed by this
digest. It is derived cache state, not Product state.

- An existing `queued` or `indexing` job with the same key is attached; no new
  materialization is created.
- An existing `ready` job with a compatible exact snapshot is reused.
- A retryable failed attempt retains the same `job_id` and increments
  `attempt`; CodeGraph may start that attempt only after its returned retry
  boundary. Concurrent callers still attach to one attempt.
- `cancelled`, `expired`, and non-retryable `failed` attempts return a typed
  limitation. They are never silently restarted within the same preflight.
- A queued job expires 15 minutes after creation if it has not begun. Expiry is
  persisted before a later request may create the next attempt under the same
  logical `job_id`.
- `retry_token` is opaque but stable for the active logical job. MCP does not
  interpret it; the daemon returns it so the next `preflight.run` can reattach.
- The first creator persists the job's queue TTL. Later callers with the same
  key attach to that job and use their own wait budget; they cannot extend or
  shorten the active job's expiry.

A request for a different commit or output-affecting implementation version has
a different job key. It must not attach to an older target.

## Missing Or Stale Materialization

For a missing or stale exact target, CodeGraph performs these steps:

1. Validate contract compatibility and the bounded policy.
2. Compute the canonical job key.
3. Reuse a compatible ready artifact, attach to active work, or atomically
   create one bounded materialization job.
4. Materialize only the supplied scope against the pinned commit.
5. Wait within the effective request budget.
6. Return `ready` if the exact artifact completes, `pending` if work remains,
   or `limited` for a typed terminal condition.

A readiness check alone is insufficient. Constructing `GraphSliceInfo` only
when a graph is already ready is also insufficient. Implementations must
demonstrate that one eligible stale/missing request starts work and that a
duplicate request attaches to it.

## Target Movement And Fast Commit Streams

HEAD/ref movement does not alter a pinned request:

- In-flight work may finish against the original commit and remains valid only
  as snapshot-scoped derived evidence for that commit.
- A request for the new commit receives a different job key.
- An old job may be marked `superseded` for scheduling priority, with
  `superseded_by_commit_sha`, but its output must not be relabeled as current
  for the new target.
- The daemon either completes the optional branch against the originally
  pinned commit or returns `limited(target_mismatch)` with the original target
  and retry guidance.
- No result may combine a candidate, snapshot, scope, or proof from different
  commits.

Coalescing by exact job key prevents duplicate work during repeated MCP calls.
Commit-specific keys and explicit supersession prevent fast commit streams
from permanently treating an older graph as current.

## Grounded Recall And Semantic Identity Chains

The grounded lane preserves:

```text
requested Product/workspace/repository/head
  -> grounded-recall request ID
  -> CodeGraph canonical job key + job_id
  -> pinned commit + graph implementation identity
  -> graph_snapshot_id
  -> scope fingerprint + graph_slice_id + scope_proof_id
  -> resolved code target identities
  -> accepted BindingEvidence identities
  -> exact accepted Claim revisions + ModelAtom identities
  -> grounded result digest
```

The deep semantic lane separately preserves:

```text
Plan/source digest + extraction profile
  -> PlanSemanticProjection + impact atoms
  -> accepted SMR base
  -> recalled Claim revisions + ModelAtoms
  -> SmrCheckScopeAssessmentV1
  -> exact snapshot + named bounded check
  -> deep semantic result digest
```

CodeGraph returns graph facts and target regions/modules. The daemon, not
CodeGraph, traverses accepted BindingEvidence and resolves every referenced
Claim and ModelAtom against accepted SMR state.

A CodeGraph target with no accepted BindingEvidence produces no grounded Claim
match. It does not create a specification, Claim, applicability decision,
missing-spec finding, contradiction, or Product claim.

## One Pinned Snapshot And Downstream Handoff

Within the grounded graph lane, ensure-current, CodeLocator, target discovery,
and BindingEvidence backtrace consume the same pinned commit,
graph implementation identity, snapshot ID, scope fingerprint, and proof ID.
The daemon rejects a mismatched intermediate result rather than recomputing or
combining it.

SMR does not become a compliance checker. A later Code Evidence/Compliance
request that claims to consume this graph observation must receive an explicit
handoff containing:

```text
target commit
graph implementation identity digest
graph snapshot ID
scope fingerprint
graph slice ID
scope proof ID
candidate provenance digest
```

That lifecycle either verifies the full identity tuple before reuse or obtains
and labels a new observation. A later observation cannot silently strengthen
the earlier SMR result.

## Unknown-Coverage Rules

The daemon must create a core `unknown_scope` finding when a trusted corpus
revision, configured source, declared corpus partition, or input context cannot
be resolved sufficiently to state searched coverage. The finding names the
affected scope and reason.

CodeGraph availability is a prerequisite only for a positive grounded-lane
result. A pending or limited grounded lane is rendered as a typed limitation;
it does not erase or delay the independent deep semantic result.

Code unknown coverage and semantic unknown coverage remain separate. A graph
limitation never satisfies or invalidates `SmrCheckScopeAssessmentV1`.

## Authority Boundary

SMR may:

- recall product-approved specifications from the named trusted corpus;
- backtrace accepted BindingEvidence to exact accepted Claim revisions and
  ModelAtoms for one pinned graph identity;
- state searched scope, missing specifications, and unknown coverage;
- invoke bounded CodeGraph target resolution through the exact contract above;
- attach graph identities and accepted applicability evidence; and
- return a proposed claim digest for a separately governed acceptance path.

SMR may not:

- require CodeGraph readiness or `ScopeSufficiencyProof` to complete deep
  semantic analysis;
- claim an implementation is present, absent, current, correct, compliant, or
  all-clear;
- derive contradiction or canonical Product truth from code evidence; or
- write canonical Product state.

The Binding and Code Evidence lifecycles own accepted BindingEvidence, graph
snapshots, currentness, `ScopeSufficiencyProof`, and `GraphEvidenceState`.
Grounded preflight may read those accepted artifacts but cannot create or
upgrade them. Compliance remains separately owned.

## Existing Runtime Coverage And Remaining Gap

Issues #625 and #646 already provide:

- `GraphRefreshParams`, `GraphRefreshResult`, and typed readiness vocabulary;
- commit, repository/workspace, scope-kind, and scope-fingerprint foundations;
- touched-scope freshness and a 64-target bounded gate;
- ready-snapshot reuse, commit-watcher context, and bounded slice metadata.

They do not yet provide this complete contract:

- `bounded_refresh_check` observes readiness but does not start materialization;
- `graph.refresh_snapshot` creates bounded slice metadata only for an already
  ready graph and currently returns no job;
- complete graph implementation identity and contract negotiation are absent;
- durable job coalescing, reattachment, expiry, and restart behavior are absent;
- one-snapshot ordering across refresh, CodeLocator, expansion, and downstream
  handoff is not proven; and
- the proposed 500-millisecond wait, 15-minute queue TTL, and retry semantics
  are new policy, not behavior already accepted through #625 or #646.

These are implementation gaps for the grounded lane, not reasons to introduce
CodeGraph closure into deep semantic analysis.

## Required Scenarios

1. **CodeGraph absent:** grounded recall is
   `limited(not_configured|unsupported)` while deep semantic analysis remains
   independently runnable.
2. **Exact current graph:** ensure-current returns `ready` with the exact
   snapshot and does not create duplicate work.
3. **Missing/stale completes:** bounded materialization starts and completes
   within 500 milliseconds; grounded recall uses the returned pinned identities.
4. **Queued beyond budget:** after 500 milliseconds, grounded recall returns
   `pending`, a stable job ID/token, and retry guidance; deep analysis is not
   delayed.
5. **Duplicate MCP retry:** the next `preflight.run` with the same canonical job
   key attaches to the same job and does not duplicate materialization.
6. **HEAD moves:** the original branch finishes against its pinned commit or
   returns `limited(target_mismatch)`; no mixed snapshot identity appears.
7. **Accepted binding backtrace:** an exact target resolves accepted
   BindingEvidence, exact Claim revisions, and ModelAtoms; CodeGraph does not
   declare applicability, implementation, or compliance.
8. **Restart/replay:** the durable derived job registry or compatible ready
   artifact reattaches by canonical key; the same pinned identities reproduce
   the same disclosure without duplicate work.
9. **Incompatible version:** grounded recall returns
   `limited(incompatible_contract)` while deep semantic analysis remains
   independently runnable.
10. **Downstream compliance handoff:** Code Evidence/Compliance either consumes
    the exact handoff tuple or records a distinct observation; it cannot
    silently reuse a different snapshot.

## Governance Record

Governance gate: bic:spec-governance-gate; Qor advisory/no-op pending factory
policy revision. UserJourney: developer-host SMR preflight, read-only
deterministic grounded recall plus deep semantic analysis. Journey Boundary
Review: passed for Product-result and Factory-witness separation in
BicameralAI/bicameral-bot#938 and amended by the owner-confirmed two-lane
disposition in BicameralAI/bicameral-bot#967. Bot #820 owns fast-lane
supporting validation; Bot #943 owns integrated authentic-host validation;
MCP PR #783 is the selected proof-preserving transport.

Disposition for BicameralAI/bicameral-bot#936 closeout clarification: **AMEND**.
The core boundary in ADR-0028 is retained. CodeGraph remains unnecessary for
SMR semantic correctness or bounded Alloy acceptance.
