# Alpha Desync UX And HITL Readiness

**Release:** alpha
**Date:** 2026-06-06
**Related ADRs:** ADR-0004, ADR-0006, ADR-0010, ADR-0011, ADR-0015,
ADR-0017, ADR-0019, ADR-0021
**Related specs:** `docs/specs/enforcement-memory-projection.md`,
`docs/specs/graphslice-planning-sufficiency.md`
**Product-line context:** `bicameral-factory/product-lines/cognitive-debt-reduction/`
introduced in BicameralAI/bicameral-factory#38

## Purpose

The alpha release will not fully automate every desynchronization scenario
between Decisions, source evidence, code, graph indexes, bindings, preflight,
and compliance. Some gaps are deterministic but not implemented yet. Others
are semantic and require human judgment even after graph evidence improves.

This release note defines how alpha product surfaces should present those gaps
without pretending incomplete evidence is verified.

The alpha guarantee is:

```text
verified scope + honest unknowns + reviewable repair paths
```

## Product-Line Boundary

This product repo owns the alpha runtime and UX constraints: which states the
bot may show, what agents may do with non-verified evidence, and which claims
must remain blocked until graph or governance evidence exists.

The commercial sequencing lives in the factory product-line context:

```text
bicameral-factory/product-lines/cognitive-debt-reduction/
```

That context defines the cognitive-debt reduction promise, OSS versus premium
boundary, alpha pilot offer, Decision Routing Policy, and value ladder. Product
implementation work should consult that factory product-line context when a
task involves release sequencing, buyer-facing prioritization, OSS/premium
boundaries, or hosted-service rationale.

## Durable Ideal Versus Alpha Posture

ADRs define the durable architecture:

- graph claims are snapshot-scoped and evidence-state typed;
- `GraphSlice` claims require a sufficient scope proof;
- `BindingEvidence` materializes only from verified evidence;
- local no-conflict is not global/team no-conflict;
- governance facts materialize only through the selected event store substrate.

Alpha does not relax those contracts. Instead, alpha presents incomplete
coverage as visible product state and HITL actions.

| Area | Ideal target | Alpha release posture |
|---|---|---|
| Graph completeness | GraphStore proves sufficient snapshots and slices for every requested local graph claim. | Show coverage/readiness state; use source-only and HITL where GraphStore is incomplete. |
| Desync coverage | All twelve desync scenarios have deterministic or governed repair paths. | Prioritize EM-pilot scenarios, while routing the remaining scenarios through explicit review states. |
| Continuity | Rename, move, supersession, and semantic repair preserve decision/code continuity when evidence supports it. | Present candidates and require reviewer confirmation before lineage or inherited binding is accepted. |
| Preflight | Agents receive grounded, scoped, complete readiness for the requested work. | Agents receive relevant Decisions plus typed unknown/stale/unsupported/hosted-required boundaries and action constraints. |
| Collision/compliance | Verified or policy-accepted evidence can trigger release valves and enforcement. | Source-only signoff remains allowed; unresolved graph gaps do not become `collision_pending` or compliance resolution. |
| Hosted/team visibility | Hosted graph checks cover cross-team and cross-repo scope when configured. | Local alpha reports `hosted_required` or unknown scope; it must not imply global consistency. |

## Product Information Architecture

Every decision/code relationship shown in alpha should carry three axes:

| Axis | Meaning |
|---|---|
| Grounding | Is the Decision connected to source-only evidence, approximate candidates, or verified graph evidence? |
| Currentness | Does the relationship still hold for the inspected snapshot or requested local scope? |
| Confidence boundary | Is the result `verified`, `review_needed`, `readiness_blocked`, or `source_only`? |

Do not collapse these into one confidence score. The surface should show what
is known, what changed, what cannot be proven, and what action is available.

## Alpha Product States

| Product state | Evidence inputs | Product meaning | Allowed alpha behavior |
|---|---|---|---|
| `verified` | `GraphEvidenceState::verified` over a sufficient scope, or accepted durable evidence replayed from the event store. | The claim can support binding, grounded preflight, compliance reads, or governance actions when policy permits. | Show as grounded; allow governed materialization where the relevant ADR permits it. |
| `review_needed` | `ambiguous`, semantic continuity candidate, supersession/inheritance candidate, approximate retrieval candidate, or possible repair target. | The system found plausible evidence, but product meaning or target identity needs human judgment. | Queue reviewer actions; do not materialize `BindingEvidence`, compliance facts, or conflict state automatically. |
| `readiness_blocked` | `unknown_not_indexed`, `unknown_stale`, `unsupported`, `failed`, dirty worktree, unresolved ref, or `hosted_required`. | The system cannot honestly make the requested graph-grounded claim yet. | Offer allowed actions: index, expand slice, rebuild, fetch ref, request hosted/team check, proceed source-only, or defer. |
| `source_only` | Reviewed source evidence or deterministic text/path relevance without verified graph evidence. | Useful context exists, but no code-grounded claim has been proven. | Show as context for humans/agents; allow signoff under ADR-0011's known-evidence invariant; do not claim grounded compliance. |

## HITL Routing For Desync Gaps

| Gap | Alpha presentation | HITL action |
|---|---|---|
| Cold index or graph not configured | "Graph grounding unavailable for this workspace or scope." | Configure graph support, proceed source-only, or defer grounded claims. |
| Bound code deleted | "Accepted Decision points to a missing target at the inspected snapshot." | Mark stale, rebind, supersede, or keep source-only with caveat. |
| Line/range shift | "Range changed; symbol-backed occurrence may still resolve." | Accept updated range, request exact symbol validation, or mark weak. |
| Rename or move | "Possible continuation found, but same-target identity is not accepted." | Confirm same target, split lineage, or mark ambiguous. |
| Supersession | "Old binding may apply to revised Decision text." | Inherit binding, reject inheritance, or request new binding. |
| Approximate or LLM-suggested grounding | "AI/retrieval suggested this link; exact validation is pending or failed." | Approve for validation, reject candidate, or refine target. |
| Drifted Decision may match elsewhere | "Possible repair target found." | Confirm semantic fit, mark unrelated, or request remediation. |
| Cross-team or hosted-only scope | "Local scope cannot see team-wide conflicts." | Request hosted/team check or proceed with known-evidence caveat. |
| Compliance/security evidence insufficient | "Evidence is insufficient for enforcement." | Require reviewer signoff, policy exception, or block automation. |

## Agent-Facing Constraints

Preflight and MCP-facing responses must include action constraints when a result
is not `verified`.

Example:

```text
Relevant Decision found.
Grounding: source_only
Currentness: unknown_not_indexed
Allowed agent behavior:
- mention the Decision in the implementation plan
- inspect suggested files
- ask for reviewer confirmation before modifying constrained code
Not allowed:
- claim no conflict
- create BindingEvidence
- resolve compliance
```

Agents may use `review_needed`, `readiness_blocked`, and `source_only` states
to plan, ask questions, expand context, or request review. They must not turn
those states into durable evidence, clean compliance, or "safe to proceed"
claims.

## UI Copy Pattern

Uncertain cards should use this structure:

```text
What we know:
- Decision D-17 was accepted from ADR-004.
- It was previously bound to BillingService.charge at abc123.

What changed:
- The symbol no longer appears at that location.
- A similar symbol appears in billing/payments.rs.

What we cannot prove:
- Whether this is the same implementation target.

Recommended review:
- Confirm same target
- Create new binding
- Mark old binding stale
```

## Non-Goals

- Do not redefine ADR-0010 `GraphEvidenceState`.
- Do not add a canonical Decision lifecycle state for every graph readiness
  condition.
- Do not let HITL review make unsupported code facts true. Reviewers may accept
  source-only or policy decisions, but they do not convert unsupported graph
  evidence into verified graph evidence.
- Do not claim full hosted/team conflict visibility in the local alpha.
