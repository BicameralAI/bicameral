# Egress Projection Attack-Surface Review (#528)

**Status:** adversarial review artifact — input to the egress design (#527). Defines required invariants/controls; does **not** implement ProjectionPolicy/Profile/EgressProjection/TargetAdapter (those remain #527 follow-ups).
**Issue:** [#528](https://github.com/BicameralAI/bicameral-bot/issues/528) "Attack surface: Governed egress projections may recreate external truth drift"
**Owner:** @Knapp-Kevin
**Governs by:** ADR-0024 (signal vs authority gate), #221 (signal≠authority cut), the bot ↔ integrations sibling boundary
**Design context:** #527 (governed egress projections — closed)

## Spine: egress is the mirror of the ingest authority boundary

Bicameral's whole governance model already answers most of #528, in reverse. On **ingest**, ADR-0024 and #221 establish that probabilistic/external values (CodeGenome confidence, external evidence) are `signal_input` / `identity_substrate` and may rank, warn, propose, and explain — but may **never cross an `authority_gate`**; only bot-governed surfaces materialize durable state.

**Egress is the same cut, mirrored:**

```text
Ingest:  external/probabilistic input  → may inform, never authorize → governed acceptance materializes canonical state
Egress:  canonical state               → may be projected outward,  never re-imported as authority → governed review re-materializes any change
```

This yields the single governing rule, restating the issue's short form and the owner's clarification ("Observation is core. Promotion is governed."):

> A projection is a **read-model of canonical state** rendered into an external surface. It is never authority. No projection, profile, adapter, or external mutation may create, waive, or promote canonical Bicameral state. External change re-enters only as **observation → reconciliation finding → correction candidate → governed review**.

Everything below is a corollary.

## Required invariants

| ID | Invariant |
|---|---|
| **EG-1** | **Projection ≠ authority.** A `ProjectionProfile`/`TargetAdapter` may *declare capability* but can never grant permission, waive approval, or promote advisory→execution. Capability declaration and authority are separate fields (ADR-0024). |
| **EG-2** | **Eligibility is bot-owned and allowlisted.** Only explicitly accepted/verified canonical artifacts are egress-eligible. Advisory/preliminary state is barred (see eligibility table). The allowlist lives in bot-owned `ProjectionPolicy`, never in an integrations-owned profile. |
| **EG-3** | **No external state mutates canonical state directly.** Every external edit/status/label/reaction/comment enters as an *observation*, is classified as a *reconciliation finding*, and becomes a *correction candidate* requiring governed review before any canonical change. Zero direct promotion paths. |
| **EG-4** | **Typed projection body.** `projection_body` is a closed, typed schema — not arbitrary JSON or freeform template. Adapters cannot add/strip fields. Canonical ref, authority label, and receipt reference are non-omittable. |
| **EG-5** | **Enforceable receipts.** Every external mutation produces an `EgressReceipt` bound to `{canonical_artifact_id, canonical_revision, projection_payload_hash, policy_result, target_object_id, operation_type, delivery_status}`, replayable, and consumed during reconciliation. No receipt → no mutation. |
| **EG-6** | **Staleness is detectable and marked.** Every projection carries its `canonical_revision`. Supersession or SourceEvidence/BindingEvidence change marks the projection `stale` and enqueues reconciliation. Silent staleness is forbidden. |
| **EG-7** | **Adapter is mechanism-only.** A `TargetAdapter` executes exactly the approved operation and returns a receipt/result. It cannot choose a profile, alter the body, suppress receipt fields, retry a blocked op, map a target error to success, mutate the target outside the requested op, or write canonical state. |
| **EG-8** | **Profile validity is tied to verified target capability.** `ProjectionProfile` validity is gated on connector docs + API version + OAuth scope (write vs read) + capability verification. A read-scoped or stale-versioned profile cannot authorize a write projection. |
| **EG-9** | **Human-legibility.** Every projection visibly declares "Bicameral is canonical; this is a projection," with the canonical ref and a freshness/stale marker. |
| **EG-10** | **Approval mandatory for create/mutate.** Any projection that creates or mutates an external work object requires bot-owned approval + receipt. Notification-only projections are a distinct, non-mutating class and may not be used to smuggle mutation. |

## Review-question answers (13 attack surfaces)

| Attack surface | Review question | Required answer | Controlling invariant |
|---|---|---|---|
| Profile laundering | Can a ProjectionProfile grant permission / waive approval / turn advisory into execution? | **No.** Profiles declare shape/capability only; eligibility + approval are bot-owned `ProjectionPolicy`. | EG-1, EG-2, EG-10 |
| Projection drift | Can projections go stale without detection/marking/reconciliation? | **No.** Revision-stamped; supersession marks stale + enqueues reconciliation. | EG-6 |
| Backdoor authority | Can any external edit/status/comment/label/reaction mutate canonical state directly? | **No.** Observation → reconciliation finding → correction candidate → review. | EG-3 |
| Target-specific truth models | Are target statuses ever mapped directly to canonical Decision/evidence/binding/compliance/signoff? | **No.** Target status is an observation input to reconciliation, never a canonical mapping. | EG-3 |
| Receipt theater | Can every external mutation be traced to artifact + payload + policy + target object? | **Yes, mandatory.** Receipts are enforceable and replayed in reconciliation. | EG-5 |
| Arbitrary projection body | Is `projection_body` typed enough to block smuggling? | **Yes.** Closed typed schema; adapters cannot add fields; canonical ref/authority label/receipt non-omittable. | EG-4, EG-7 |
| Over-broad eligibility | Which artifacts are eligible vs barred? | **Allowlist** (see table); advisory/preliminary barred. | EG-2 |
| Approval bypass | Can any egress create/mutate external work without bot approval + receipt? | **No.** Create/mutate requires approval + receipt; batch = one-issue-per-approved-action. | EG-10, EG-5 |
| Reconciliation ambiguity | Are external changes classified as observations/findings/candidates, not direct mutations? | **Yes.** Three explicit non-canonical states precede any review. | EG-3, EG-6 |
| Doc/capability drift | How is profile validity tied to docs/API version/scope/capability? | Profile invalid unless capability-verified against current docs + write scope. | EG-8 |
| Adapter overreach | Can an adapter do anything beyond executing an approved op + returning a receipt? | **No.** Mechanism-only; enumerated prohibitions. | EG-7 |
| User interpretation gap | Does every projection communicate canonical-vs-projection? | **Yes, required.** Canonical ref + projection + freshness marker on every projection. | EG-9 |
| (Profile grants permission — checklist Q14) | Can a profile grant permission instead of declaring capability? | **No.** Capability declaration and authority are separate; only `ProjectionPolicy` authorizes. | EG-1 |

## Egress eligibility (EG-2 allowlist)

| Eligible (accepted/verified canonical) | Barred (advisory / preliminary / probabilistic) |
|---|---|
| Accepted `Decision` (current revision) | raw `RecallPacket` |
| Verified `BindingEvidence` | unreviewed contradiction candidate |
| Resolved compliance result | unaccepted `DecisionCandidate` |
| Accepted signoff state | approximate `BindingHint` |
| Accepted `SourceEvidence` | CodeGenome recall signal / overlay confidence |
| | stale graph advisory / unsupported-target warning |

The barred column is exactly the ADR-0024 `signal_input` / handoff-artifact set — consistent with #221: signals inform, they do not become external execution work.

## Required test cases

```text
EG-1  a ProjectionProfile declaring "create without approval" is rejected by ProjectionPolicy
EG-2  each barred artifact type (RecallPacket, unaccepted candidate, approximate BindingHint,
      CodeGenome signal, stale advisory) is refused egress eligibility
EG-3  an external edit/status/label/reaction/comment produces a correction *candidate*, never a
      canonical mutation; negative test asserts no canonical write occurs on observation
EG-4  an adapter that injects a field absent from the typed EgressProjection is rejected
EG-5  a mutation without a complete receipt (missing payload hash or canonical revision) is rejected;
      receipt replays to the exact projected payload
EG-6  superseding a Decision marks its outstanding projections stale and enqueues reconciliation
EG-7  adapter attempts (alter body / change profile / suppress receipt / retry blocked / error→success
      / out-of-scope mutation / write canonical) each fail
EG-8  a read-scoped or stale-API-version profile cannot authorize a write projection
EG-9  a rendered projection contains canonical ref + projection label + freshness marker
EG-10 work-item create/mutate without approval is rejected; one approved action ⇒ one external object
```

## Required policy boundaries (who owns what)

```text
bicameral-bot (authority):   ProjectionPolicy (eligibility + approval), receipt generation,
                             reconciliation classification, canonical acceptance of correction candidates
bicameral-integrations:      ProjectionProfile (target shape, rendering constraints, declared capability) —
                             declaration only, never authorization
external tool:               execution/notification/publication surface — signal & evidence source, never authority
```

This is the sibling boundary: integrations declares *shape/capability*; the bot decides *eligibility/approval/receipt/acceptance*. A profile that behaves like policy is a boundary violation, not a feature.

## Required receipt fields (EG-5)

```text
canonical_artifact_id, canonical_revision, projection_payload_hash, policy_result,
target_object_id, operation_type {create|update|comment|link|notify}, delivery_status
{delivered|partial|failed}, timestamp, profile_id, profile_capability_version
```

Partial/failed delivery must record an unambiguous state (never a success receipt for a failed mutation).

## Required reconciliation states (EG-3, EG-6)

```text
observation        — raw external change captured (expected, core)
reconciliation_finding — classified delta vs last projection (edited/deleted/status-changed/link-removed/
                         copied/split-merged/target-unavailable)
correction_candidate   — finding promoted to a governed candidate requiring review
(then) governed review — accept/reject/supersede via existing HITL paths; only this mutates canonical state
```

External observation never skips to canonical mutation. `stale` is an orthogonal projection marker, not a canonical state.

## High-risk projection paths (prioritize for adversarial implementation review)

```text
1. work-item creation/mutation paths (Linear/Jira/GitHub issues, PRs) — EG-10/EG-5 must hold
2. status/label/reaction observation paths — highest backdoor-authority risk (EG-3)
3. profile capability vs OAuth write-scope — EG-8 (read-scope authorizing writes)
4. batch projection — one-approval-to-many-objects fan-out (EG-10)
5. cross-tool copy of a projection without its receipt/canonical ref — EG-9 user-interpretation
```

## Non-goals (per #528)

Does not solve egress architecture, implement ProjectionPolicy/Profile/EgressProjection/TargetAdapter, decide the Linear alpha, or expand into general connector design.

## See also

- `docs/adr/0024-signal-variables-authority-gates-durable-state.md` · `docs/specs/codegenome-authority-cleanup-221.md`
- #527 (egress design) · #528 (this attack surface)
