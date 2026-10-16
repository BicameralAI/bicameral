# Configurable Inline Governance Review For Tool Calls

**Date:** 2026-06-06
**Status:** proposed
**Issue:** https://github.com/BicameralAI/bicameral-bot/issues/153
**Related:** ADR-0002, ADR-0004, ADR-0005, ADR-0007, ADR-0017

## Problem

Local tool surfaces such as MCP, CLI, dashboard, and TUI can ask the daemon
questions that naturally produce governance prompts:

- "Does this work satisfy DEC-123?"
- "Should this verified binding proposal become accepted BindingEvidence?"
- "Should this compliance result be resolved?"
- "Should this candidate be accepted or routed for review?"

Those prompts are useful inside the caller workflow, especially for coding
agents, but the review surface must not become durable authority. ADR-0002 says
tool calls map to `ToolRequest`. ADR-0007 says review surfaces emit
substrate-neutral `ReviewCommand`s and governance policy decides whether those
commands materialize. This RFQ defines the configurable UX modes for tool calls
that surface review questions without changing that authority path.

## Decision

Do not introduce "Tool-mediated review" as new canonical product vocabulary.
Treat it as a UX pattern under existing terms:

- `ToolRequest`;
- Review Surface;
- `ReviewCommand`;
- Governance policy;
- `GovernanceResult`;
- event-store materialization.

MCP and other local tool surfaces may present inline governance questions,
recommended answers, buffered review items, and route-to-dashboard links. They
must still submit ordinary `ReviewCommand`s to the daemon. The daemon evaluates
policy, actor/session authority, evidence strength, confidence, risk, and
substrate capability before materialization.

Tool-call review UX is configurable by workspace policy. The policy may choose
different modes by command kind, Decision level, source trust, evidence state,
confidence, actor capability, enforcement capability, and substrate.

## Supported UX Modes

| Mode | Behavior | Authority boundary |
|---|---|---|
| `auto_resolve` | Policy accepts and materializes a narrow low-risk command without asking inline. | Allowed only for explicitly configured low-risk classes; audit must record policy version and actor/session context. |
| `ask_inline` | Tool response includes one or more governance questions and allowed answers. | A later selected answer emits a normal `ReviewCommand`; no state changes until daemon policy accepts it. |
| `buffer_for_batch_review` | Tool response records review items for later dashboard, CLI, or batch handling. | Buffered items are prompts, not accepted commands. |
| `recommend_confirm` | Tool response pre-selects a recommended answer but requires explicit confirmation. | Recommendation is advisory until confirmed and accepted by policy. |
| `route_to_surface` | Tool response points the actor to dashboard, CLI, Slack, or another review surface. | The routed surface emits the same canonical command vocabulary. |
| `force_human_review` | Tool response refuses auto-resolution and requires accountable reviewer action. | Consequential governance state cannot change without explicit review. |
| `advisory_only` | Tool response provides context, warnings, or proposed next actions with no review command. | No canonical mutation is attempted. |

The response may include multiple questions when policy permits it, but each
question must map to one canonical command family and one target object. Batch
submission is allowed only as a list of independent commands so replay can
preserve each accepted transition.

## Tool Response Shape

Exact protocol structs may evolve, but tool responses that surface governance
questions should preserve this shape:

```json
{
  "governance_questions": [
    {
      "question_id": "q_123",
      "target_kind": "binding_proposal",
      "target_id": "bp_456",
      "command_kind": "bind_to_code",
      "prompt": "Accept this verified binding proposal for DEC-123?",
      "mode": "recommend_confirm",
      "recommended_answer": "accept",
      "allowed_answers": ["accept", "reject", "request_context"],
      "requires_explicit_actor": true,
      "required_capability": "workspace_member",
      "risk_class": "binding_materialization",
      "evidence_summary": {
        "evidence_state": "verified",
        "graph_snapshot_id": "gs_head",
        "validated_sha": "abc123"
      },
      "policy_reason": "Verified binding evidence can be accepted after explicit confirmation."
    }
  ],
  "buffered_review_items": [],
  "governance_result": {
    "review_state": "needs_grounding_review",
    "allowed_commands": ["bind_to_code", "request_context"],
    "reason": "Binding proposal is verified but requires configured confirmation."
  }
}
```

Questions are not accepted events. They are UI affordances for submitting
commands.

## Policy Inputs

The daemon policy decision for tool-call review UX should consider:

- command kind;
- target kind and lifecycle state;
- Decision level;
- source trust and source freshness;
- actor/session identity and capability;
- reviewer assignment;
- evidence state;
- confidence surfaces from ADR-0004, kept separate for extraction, binding,
  and compliance;
- graph readiness and snapshot scope;
- enforcement capability of the selected substrate;
- workspace automation mode from ADR-0005;
- whether the command changes shared authority or only records advisory state.

Policy must return:

- selected UX mode;
- allowed answers or commands;
- required actor/capability;
- reason;
- audit fields needed if a command is accepted automatically or later
  materialized.

## Auto-Accept Boundaries

Narrow `auto_resolve` is allowed only when all of the following hold:

- workspace policy explicitly enables the command class;
- actor/session authority is valid for the command;
- evidence is deterministic, verified, reviewed, or otherwise policy-accepted;
- the transition is low risk for the configured workspace;
- the event store substrate can materialize and replay the accepted command;
- audit records policy version, actor/session, evidence, and rationale.

Examples that may be policy-auto-accepted in narrow configurations:

- reject a duplicate low-confidence candidate before it becomes a Decision;
- accept a low-risk candidate from a trusted source into `proposed`, not
  approved;
- attach supplemental reviewed source evidence to an already proposed
  candidate or Decision;
- accept a verified binding proposal for a non-blocking Decision when there is
  no existing binding replacement, no ambiguity, and policy explicitly permits
  low-risk binding materialization;
- mark a local operational receipt or advisory observation as recorded when it
  does not change shared governance authority.

Even when allowed, `auto_resolve` must be visible in audit output as automated
policy acceptance, not as human review.

## Explicit Review Required

The following transitions require explicit accountable review unless a future
successor ADR narrows one of them with a reviewed automation class:

- `approve_signoff`;
- `reject_signoff`;
- `supersede_decision`;
- `mark_different_scopes` for a conflict resolution;
- materializing or replacing existing `BindingEvidence`;
- resolving compliance to an enforcement-relevant state;
- accepting a binding proposal with ambiguous, stale, unsupported,
  approximate, or not-indexed evidence;
- demoting, removing, or reopening approved Decisions;
- changing governance policy;
- destructive source removal or untracking when it affects accepted evidence;
- creating blocking `GovernanceResult`s;
- selecting a winner between conflicting Decisions.

Strong evidence may justify a recommendation, but it does not remove the need
for explicit review for these transitions.

## BindingProposal To BindingEvidence

`BindingProposal` maturation is the canonical example for this RFQ.

The tool-call flow is:

```text
binding.create or preflight result
  -> BindingProposal with graph evidence state
  -> policy selects UX mode
  -> inline question, buffered item, route, auto-resolve, or forced review
  -> ReviewCommand: bind_to_code
  -> policy accepts/rejects command
  -> event store materializes accepted BindingEvidence
  -> replay updates Decision Ledger / projection
```

Rules:

- only verified graph evidence may be proposed for materialized
  `BindingEvidence`;
- non-verified states may produce inline review questions only for correction,
  context, or follow-up validation;
- replacement of existing accepted `BindingEvidence` requires explicit review;
- source-only or approximate evidence must not materialize as
  `BindingEvidence`;
- the tool surface may recommend accept/reject/request-context, but it cannot
  write the binding directly.

## Implementation Handoff

Implementation issues should add:

- policy fixtures for all supported UX modes;
- response fixtures for inline, buffered, recommended, routed, forced-human,
  and advisory-only tool-call outcomes;
- tests showing questions do not mutate canonical state;
- tests showing confirmed answers emit ordinary `ReviewCommand`s;
- audit fixtures for automatic low-risk acceptance;
- binding proposal fixtures covering verified, ambiguous, stale, unsupported,
  approximate, and replacement cases;
- dashboard/MCP/CLI conformance fixtures proving the same command vocabulary is
  used across review surfaces.

## Non-Goals

- Do not implement MCP behavior here.
- Do not add new review command names.
- Do not change event-store materialization rules.
- Do not make dashboard the only review surface.
- Do not let MCP, dashboard, CLI, Slack, or any review surface write canonical
  state directly.
- Do not add `Tool-mediated review` to `CONTEXT.md` as canonical vocabulary.
