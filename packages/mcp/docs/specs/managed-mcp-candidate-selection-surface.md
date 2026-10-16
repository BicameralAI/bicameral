# Managed MCP Candidate Selection Surface

## Status

- Source issue: `BicameralAI/bicameral-mcp#785` (surface) + amend `BicameralAI/bicameral-mcp#809` (UX wording)
- Product journey: `managed_mcp_drift_aware_continuation_v1`
- Consumer module: `candidate_selection_surface.py`
- Live wiring: registered in `server.py` / tool discovery (landed in v0.1.18 via PR #802)
- Evidence level for this slice: contract, component, and packaged-MCP integration
- Terminal validation owner: `BicameralAI/bicameral-mcp#786`
- Product activation authority: Delivery and Product Owner, not this document or PR

## Purpose

Define the MCP-owned transport and display contract for daemon-authored
`CandidateSetV1`, `CandidateSelectionV1`, `CandidateSelectionPreviewV1`,
`CandidateSelectionReceiptV1`, session heartbeat, and session end.

MCP owns:

- renderer-neutral tool schemas;
- stable-id request shaping;
- exact daemon-payload preservation;
- exact governing-spec text display when the daemon supplies it;
- accessible candidate and source-span annotations;
- explicit human-confirmation presentation;
- transport of the opaque daemon-issued confirmation token;
- typed failure and recovery rendering;
- heartbeat and end transport for the exact active host session.

MCP does not own:

- candidate generation or storage;
- dependency closure;
- currentness, Product policy, or actor authority;
- preview effect calculation;
- confirmation minting, validation, or consumption;
- canonical event append or replay;
- Product acceptance, release, or capability activation.

## Tool surface

| MCP tool | Daemon command | Canonical effect |
|---|---|---|
| `bicameral.candidate_selection.prepare` | `candidate_selection.prepare` | None. Exact no-write preview only. |
| `bicameral.candidate_selection.commit` | `candidate_selection.commit` | Daemon may atomically append the exact confirmed promotion batch. |
| `bicameral.candidate_session.heartbeat` | `candidate_session.heartbeat` | None. Extends transient session lease. |
| `bicameral.candidate_session.end` | `candidate_session.end` | None. Erases transient session state. |

Tools are defined in `candidate_selection_surface.py` and registered live in
the MCP server (shipped). This document describes the **landed** runtime.

## UX design disposition (bic:spec-governance-gate)

Governance gate: `bic:spec-governance-gate`; Qor advisory/no-op pending factory
policy revision. UserJourney:
`managed_mcp_drift_aware_continuation_v1` (issue also records
`developer_host_mcp_drift_aware_continuation_v1` bookkeeping). Journey Boundary
Review: passed — Bot #984 / #979. Journey tree + scenario pack: not applicable —
topology not required for #809 (contract/UX evidence; terminal host is #786).

**`ux_impact: true`** — coding-host MCP surface (not hosted browser SPA; Figma
N/A). Disposition: **product MCP sources updated** (not Figma). Owning surface
name: `mcp_candidate_selection`.

### Amendment #809 (confirmation / rejection wording)

Presentation-only. Authority model from #785/#802 is unchanged. Required UX:

1. **Consequence-first bulk actions** — no human label is only `Accept defaults`
   or `Don't track`. Labels are count-bearing, e.g. `Promote all 4 non-contradictory
   candidates`, `Reject all 6 candidates for this session`,
   `Promote 3 selected candidates`,
   `Promote and approve 3 selected candidates as Product Owner`.
2. **Stable effect fields** — promote/reject/remaining-pending/held-contradiction
   counts; resulting signoff; Product and actor; whether confirm appends
   canonical events.
3. **Session-only reject-all** — rejects only the active CandidateSet session;
   no canonical event, tombstone, or durable suppression; end/expiry erases
   disposition; ideas may be proposed again.
4. **Human-readable rows** — Decision text and source context lead; stable IDs
   remain visible and authoritative for transport.
5. **Confirmation summary** — promote/reject groups with Decision text, sources,
   IDs, daemon effects, counts, proposed vs owner-policy-approved, Product,
   actor, plain canonical-append statement. Opaque challenge is transport-only
   and never appears in human summary text.

Machine `selection_mode` keys (`accept_defaults`, `not_track`, `explicit`) stay
stable for protocol callers; only human-facing labels change.

| Presentation state | `page_or_route` | Visible result | Next user action |
|---|---|---|---|
| `candidate_set_review` | `app://mcp/tools/managed_preflight#candidate_set` | Exact spec + Decision-led rows; count-bearing promote-all / session reject-all options | Count-bearing action or explicit stable IDs → prepare |
| `confirmation_required_promote` | `app://mcp/tools/bicameral.candidate_selection.prepare` | Exact preview + summary; label **Promote N selected candidate(s)**; signoff **proposed** | Explicit human confirm or revise + reprepare |
| `confirmation_required_promote_and_approve` | same prepare tool | Exact preview + summary; label **Promote and approve N … as Product Owner** | Explicit human confirm (one action) or revise |
| `committed` | `app://mcp/tools/bicameral.candidate_selection.commit` | Daemon receipt (IDs, signoff, replay/session flags); not Product acceptance | Heartbeat, end, or continue coding |
| `session_heartbeat_ok` | `app://mcp/tools/bicameral.candidate_session.heartbeat` | Lease extended for exact session | Continue review/prepare |
| `session_ended` | `app://mcp/tools/bicameral.candidate_session.end` | Transient set erased; erasure ≠ rejection | New preflight/session if needed |
| `error_dependency_closure_required` | `app://mcp/tools/candidate_selection#failure` | Missing dependency IDs; no silent expand | Revise IDs and reprepare |
| `error_candidate_set_stale` | failure route | Generation/digest mismatch | Rerun preflight |
| `error_preview_stale` | failure route | Challenge no longer binds | Prepare again |
| `error_session_lease_expired` | failure route | Transient state erased | Start new session |
| `error_human_confirmation_required` | failure route | Commit refused without explicit human | Human confirms; agent must not |
| `error_unsupported_contract_version` | failure route | Version mismatch | Upgrade MCP + daemon |
| `error_generic` | failure route | Typed failure preserved | Follow recovery guidance |

Every render path attaches a structured `ux` object from
`PRESENTATION_STATE_MATRIX` in `candidate_selection_surface.py` (title,
visible_result, next_user_action, authority flags). Color and display position
are never authority.

### State refinement (selection UX only)

- **Release state:** Session-scoped CandidateSet review → exact prepare preview →
  explicit human commit; MCP remains transport/renderer. Live tools are landed.
- **States added:** none durable/canonical. Presentation states above are
  renderer-only projections of daemon payloads.
- **Remaining activation evidence (only):** terminal released-host +
  accessibility validation under **#786** after non-author wording review of #809.
- **Invariants:** no positional authority; no agent self-confirm; no MCP
  candidate store; receipts ≠ Product acceptance or release activation;
  reject-all is session-only.

## Candidate display contract

Every candidate display uses `MCPDisplayContractV1` and contains two parallel
representations:

1. `daemon_candidate_set`, an unmodified deep copy of the complete daemon
   object, including unknown future proof fields; and
2. `presentation`, additive accessible metadata for human and agent consumers.

A renderer must never reduce the daemon object through a fixed allowlist. This
preserves the repair doctrine from MCP #797: opaque or future proof fields must
survive unless a separately governed privacy rule explicitly removes them.

### Exact source-first rendering

When the daemon supplies exact governing-spec text, MCP returns it under:

```json
{
  "exact_spec": {
    "text": "<exact bytes decoded as UTF-8>",
    "utf8_byte_length": 123,
    "rewritten": false
  }
}
```

MCP does not summarize, normalize, reflow, or silently repair those bytes.
Source spans are displayed as daemon-authored byte offsets and repo-relative
paths. Any color is optional presentation metadata only.

### Accessible annotations

Every span has a stable annotation id and text label:

```text
Candidate <stable-id>, <artifact-path>, bytes <start> through <end>
```

The display contract always states:

- color is optional;
- color is not authority;
- stable candidate id is authority for selection transport;
- display position is not authority.

Initial candidates are always rendered with:

```json
{
  "selected": false,
  "accepted": false,
  "approved": false
}
```

MCP must not infer any of these states from candidate presence, ordering,
highlighting, relevance, or daemon analysis availability.

## Default selection policy (product UX)

**Auto-promote by default** unless:

- the human **explicitly rejects** a candidate id, or
- the candidate carries non-empty `contradiction_refs` (held out of default promote).

**Don't track** (`selection_mode=not_track`): reject **all** candidates in the set
(session reject; no promotions).

Prepare supports:

| `selection_mode` | Behavior |
|---|---|
| `accept_defaults` | Promote non-contradiction; honor `reject_candidate_ids`; optional `also_promote_candidate_ids` for held contradictions |
| `not_track` | `promote=[]`, reject every candidate id |
| `explicit` | Use `promote_candidate_ids` / `reject_candidate_ids` only |

`render_candidate_set` attaches `presentation.default_selection` and
`selection_options` so the host can offer **Accept defaults** and **Don't track**.

**Commit still requires explicit `human_confirmed: true`.** MCP never self-confirms.
Default auto-promote only builds the prepare subset; it does not append Decisions.

## Selection preparation

`bicameral.candidate_selection.prepare` submits exact stable ids only.

MCP may generate `selection_operation_id` because it is an idempotency identity,
not Product authority. A retry should reuse the operation identity returned by
the daemon preview.

MCP rejects locally before dispatch when:

- display positions, indexes, or candidate numbers are offered as authority;
- both promote and reject subsets are empty;
- the same candidate id appears in both subsets;
- duplicate ids appear in either subset;
- required explicit Product, session, lease, set, generation, digest, or actor
  identities are absent;
- the contract version is unsupported.

MCP does not validate dependency closure. The daemon returns
`dependency_closure_required` with exact missing candidate ids. MCP renders that
failure and lets the user revise the subset. It must not silently expand the
selection.

## Preview and explicit confirmation

A successful prepare response must contain the exact daemon preview and an
opaque daemon challenge. MCP renders:

- exact promote effects;
- exact reject effects;
- pending candidate count;
- Product, actor, set, generation, digest, spec, accepted-model base, and policy
  bindings;
- `Promote selected` or `Promote and approve selected` according to the exact
  daemon-authored signoff result;
- the ability to revise and prepare again;
- an explicit statement that an agent may not confirm.

The challenge is transported once under `confirmation_transport.challenge`.
It is marked opaque, non-persistent, and non-loggable. MCP does not extract,
copy into a human summary, validate, mint, refresh, or reuse the secret.

## Commit

`bicameral.candidate_selection.commit` requires:

- exact `selection_operation_id`;
- exact `selection_digest`;
- exact human actor id;
- `human_confirmed: true` supplied after an explicit human action;
- the opaque daemon-issued confirmation token.

MCP never defaults `human_confirmed`, treats an agent response as confirmation,
or modifies preview effects. The daemon owns all revalidation and canonical
append behavior.

The receipt is rendered without narrowing and exposes daemon-authored:

- promoted and rejected candidate ids;
- Decision and canonical event ids;
- resulting signoff;
- pending count;
- replay verification;
- session reconciliation.

A receipt is evidence of the daemon result. It is not release or Product
acceptance.

## Heartbeat and end

Heartbeat and end require exact Product, host session, lease, generation, and
contract version. MCP must not derive these identities from working directory,
transcript, prompt content, or global active Product state.

End erases transient CandidateSet and disposition state. Erasure is not
rejection and creates no canonical event, tombstone, or suppression memory.

## Failure contract

All daemon failures remain typed and complete. Unknown future fields are
preserved.

Required visible recovery examples:

| Failure | MCP recovery guidance |
|---|---|
| `dependency_closure_required` | Revise and resubmit exact stable ids. |
| `candidate_set_stale` | Rerun preflight for the current generation and digest. |
| `candidate_set_digest_mismatch` | Do not reuse another display or set. |
| `preview_stale` | Prepare the intended subset again. |
| `policy_changed` | Review a fresh daemon preview. |
| `governing_spec_changed` | Bind and display the current exact spec. |
| `accepted_model_base_changed` | Recompute against the current accepted base. |
| `human_confirmation_required` | Obtain an explicit human action. |
| `session_lease_expired` | Start a new session; transient state was erased. |
| `unsupported_contract_version` | Upgrade matching MCP and daemon components. |

No failure is rewritten as no candidate, no conflict, safety, compliance,
approval, readiness, or successful cleanup.

## Contract drift control

`contracts/semantic-resolution-capability-contract.lock.json` records:

- capability contract id and expected SHA-256;
- required `SRC-*` obligations;
- exact Bot source ref and producer file blob ids;
- CandidateSession and CandidateSelection contract versions.

The lock is not yet a substitute for required exact-byte vendoring. Before
merge, the PR must:

1. vendor the exact capability-contract bytes;
2. verify the exact SHA-256 in CI;
3. fail visibly on producer/consumer version or fingerprint drift;
4. bind the final reachable Bot producer revision after its PR lands.

## Required follow-up implementation

Before #785 can be review-complete:

- register the four MCP tools in `tool_schemas.py`;
- map them in `tool_request.py`;
- route and format them in `server.py` and `responses.py`;
- inject CandidateSet display into managed preflight when present;
- prove capability filtering against daemon-advertised commands;
- add producer/consumer fixtures from exact Bot contract bytes;
- add packaging, mypy, Ruff, and full pytest evidence;
- add collision-safe Factory attestation;
- obtain human review of wording and accessible presentation;
- leave released-host terminal evidence to #786.

## Non-goals

- arbitrary natural-language parsing inside MCP;
- MCP-owned selection closure or policy;
- local candidate persistence or success cache;
- canonical rejection;
- dashboard UX;
- terminal host evidence;
- product release or activation.
