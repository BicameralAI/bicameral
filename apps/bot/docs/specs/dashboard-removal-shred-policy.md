# Dashboard Removal And Shred Policy

**Date:** 2026-06-06
**Status:** proposed
**Issue:** https://github.com/BicameralAI/bicameral-bot/issues/118
**Related:** ADR-0006, ADR-0007, ADR-0011;
`mcp-integration-v02.md`

## Problem

The MCP refactor deletes MCP-owned dashboard, Pulse, `remove_decision`, and
`remove_source` behavior. Bot/dashboard must own the product policy before MCP
can safely remove or remap those handlers.

The open ambiguity is that removal-like actions are not one thing:

- reducing a Decision's standing is not deletion;
- superseding a Decision is not erasure;
- untracking a source is not deleting historical provenance;
- privacy/legal erasure is not an ordinary Ledger View button.

If those states are collapsed, the dashboard can either over-delete durable
authority or make legal/destructive workflows look impossible. This RFQ defines
the first-pass policy and the future Shred surface boundary.

## Decision

Ordinary dashboard and Ledger View UX must not expose casual hard delete.

Ordinary removal-like actions use canonical governance language:

- `demote`: reduce standing or visibility without deleting history;
- `supersede`: mark a Decision as replaced by a newer Decision;
- `untrack_source`: stop actively using a source while preserving historical
  provenance;
- `reject_signoff` / rejection commands: lower or reject authority through
  normal review lifecycle;
- `hard_delete`: not available through ordinary dashboard or MCP UX;
- `erasure`: supported only through a dedicated Shred workflow for explicit
  destructive, privacy, legal, or admin handling.

Shred may be future-only in the first dashboard pass. If it is not implemented,
dashboard/docs must show it as planned or unavailable rather than implying
erasure is impossible.

Old MCP `remove_decision` and `remove_source` behavior must not be preserved as
direct local authority. It is either:

- re-expressed as bot-owned ordinary commands where semantically valid;
- routed to future bot-owned Shred commands for destructive erasure; or
- retired in migration docs with the MCP pre-refactor pointer.

## Ordinary Ledger View Policy

Ledger View owns ordinary Decision lifecycle actions over replayed Decision
Ledger state.

Allowed ordinary affordances:

| User intent | Canonical action | Result boundary |
|---|---|---|
| This Decision should not stand as approved. | `reject_signoff` or future `demote_decision` successor command. | Authority is lowered; history remains inspectable. |
| This Decision is replaced by another Decision. | `supersede_decision`. | Older Decision remains historical and linked to successor. |
| These conflicting Decisions are both valid in different areas. | `mark_different_scopes`. | Conflict review is resolved without deleting either Decision. |
| Stop actively tracking this source. | `untrack_source`. | Active source tracking stops; cited historical evidence remains preserved. |
| Request more review before action. | `request_context` / `assign_reviewer`. | No authority change until later command acceptance. |

Ordinary Ledger View must not:

- hard-delete Decisions;
- erase accepted source evidence;
- remove event history;
- write substrate-specific files directly;
- call old MCP removal handlers;
- bypass policy with dashboard-only annotations.

## Shred Surface

Shred is a dedicated destructive/privacy/legal/admin workflow. It is separate
from Ledger View so ordinary review cannot accidentally perform erasure.

Shred owns future workflows such as:

- destructive removal of source payloads when privacy/legal policy requires it;
- removal or redaction of personal data from retained source snapshots when the
  event-store substrate supports the operation;
- destructive Decision/source erasure requests that cannot be expressed as
  demote, supersede, reject, or untrack;
- migration or remediation of old MCP removal requests into bot-owned review
  flows.

Shred requirements:

- authenticated actor identity;
- capability-checked actor context;
- explicit confirmation;
- reason code;
- target preview;
- blast-radius preview, including affected Decisions, SourceEvidence,
  snapshots, bindings, and projections;
- audit trail;
- refusal states when the selected substrate cannot perform the requested
  erasure honestly;
- no silent collapse of erasure into ordinary demotion/untracking.

Shred is not a normal dashboard button and not an MCP-local behavior.

## Actor And Authority Requirements

All removal-like actions require a real authenticated actor context.

The dashboard must not submit removal, demotion, supersession, untracking, or
Shred commands with a hardcoded actor such as `"dashboard"` as authority. The
surface may include `"surface": "dashboard"` metadata, but policy needs an
accountable actor/session identity and capability.

Required command context:

- actor id;
- actor capability or reviewer assignment;
- session id or authenticated request context;
- surface metadata;
- target id and target kind;
- requested command;
- reason, when the command lowers authority or destroys data;
- substrate capability result.

The dashboard may render controls based on policy-provided allowed commands,
but final acceptance remains with daemon governance policy and event-store
materialization.

## Disabled And Refusal States

The UI must distinguish why an action is unavailable.

Required reasons:

| Reason | Meaning | User-facing behavior |
|---|---|---|
| `policy_forbidden` | Workspace policy does not allow this command for the target. | Control disabled or hidden with policy reason. |
| `missing_actor` | No authenticated actor/session is available. | Refuse command; ask user to authenticate or choose a valid surface. |
| `missing_capability` | Actor lacks required owner/member/admin/reviewer capability. | Refuse command and show required capability. |
| `unsupported_command` | Bot protocol does not expose the command yet. | Show unavailable/planned state; do not fall back to MCP-local behavior. |
| `substrate_unavailable` | Selected event store substrate cannot materialize the requested action. | Refuse or route to supported substitute. |
| `requires_shred` | Request is destructive/erasure-like and must use Shred. | Route to Shred or show future Shred unavailable state. |
| `target_state_invalid` | Target lifecycle state does not permit the command. | Show state-specific reason and allowed alternatives. |

Disabled states must not be represented as "no-op success."

## Command Mapping

First-pass ordinary mapping:

| Old or user-facing action | Bot-owned outcome |
|---|---|
| MCP `remove_decision` for "this should no longer stand" | `reject_signoff`, `supersede_decision`, or future demotion command depending on target state. |
| MCP `remove_decision` for destructive erasure | Shred-only future command or retired behavior. |
| MCP `remove_source` for stop tracking | `untrack_source`. |
| MCP `remove_source` for privacy/legal deletion | Shred-only future command or retired behavior. |
| Dashboard delete button | Not allowed in ordinary Ledger View. |

Adding a new canonical command, such as `demote_decision` or a Shred command
family, requires ADR-0007 or successor ADR updates. Until that lands, product
docs may reserve the concept but implementation must not invent private command
names that bypass the shared review flow.

## Data Shape

Removal-like command affordances should preserve this response shape:

```json
{
  "target": {
    "kind": "decision",
    "id": "DEC-123",
    "state": "approved"
  },
  "allowed_actions": [
    {
      "action": "supersede_decision",
      "requires_reason": true,
      "requires_confirmation": true,
      "required_capability": "workspace_member",
      "surface": "ledger_view"
    }
  ],
  "disabled_actions": [
    {
      "action": "hard_delete",
      "reason": "requires_shred",
      "detail": "Destructive erasure is handled by the Shred workflow."
    }
  ]
}
```

Shred affordances should additionally include:

- target preview;
- affected evidence/projection preview;
- substrate support;
- required confirmation text or equivalent explicit confirmation;
- audit reason;
- refusal reason when unavailable.

## Migration Notes For MCP Retirement

MCP release and migration docs must say:

- old MCP dashboard/Pulse removal behavior is removed;
- old `remove_decision` and `remove_source` are not compatibility-preserved;
- ordinary lifecycle changes use bot-owned review commands;
- destructive erasure routes to Shred when supported;
- unsupported Shred behavior is unavailable, not handled by MCP fallback;
- the pre-refactor reference commit is
  `0827444c80d45fe3474f68002166e1fc35708eda`.

## Required Tests For Implementations

Implementation issues should cover:

- visible ordinary Ledger View affordances for reject/demote, supersede,
  mark-different-scopes, untrack-source, request-context, and assign-reviewer;
- no hard-delete action in ordinary Ledger View;
- disabled/refusal states for policy, actor, capability, unsupported command,
  substrate, Shred-required, and invalid target state;
- dashboard command emission shape with authenticated actor/session context;
- no dashboard command path that writes substrate files directly;
- old MCP `remove_decision` / `remove_source` names failing as retired or
  routing only through bot-owned commands;
- Shred future-state copy or disabled state when Shred is not implemented;
- audit payload for accepted ordinary removal-like commands.

## Non-Goals

- Do not implement dashboard behavior here.
- Do not implement Shred here.
- Do not add new canonical command names here.
- Do not preserve MCP dashboard/Pulse as a compatibility backend.
- Do not decide external source adapter placement.
