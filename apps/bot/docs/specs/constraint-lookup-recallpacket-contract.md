# Constraint Lookup RecallPacket Contract

Date: 2026-06-19
Status: proposed contract
Related issues: https://github.com/BicameralAI/bicameral-bot/issues/478, https://github.com/BicameralAI/bicameral-mcp/issues/612
Related specs: `docs/specs/preflight-search-readiness.md`, `docs/specs/librarian-recall-readme.md`, `docs/specs/factory-alpha-librarian-lifecycle-delta.md`

## Purpose

`RecallPacket` is the read-only terminal artifact of the Constraint Lookup stage.
It makes recall honest by naming what was searched, what matched, what remains
unknown, and how the user can expand scope.

It is not a new lifecycle. It is not memory. It is not a guard, capture,
projection, candidate acceptance, binding, compliance, signoff, or enforcement
object.

This does not mean alpha has no capture. Alpha capture exists as a governed
corpus-update or correction path. The distinction is:

```text
RecallPacket return = read artifact, no mutation.
User-approved correction/corpus update = capture path, governed separately.
```

Alpha correction capture uses existing Bicameral paths. Product docs, ODRs,
ADRs, feature definitions, tickets, PR comments, chat, and meetings enter as
`Source`, `SourceSnapshot`, and `SourceEvidence` through integrations or local
ingest. Code evidence may be attached as optional recall provenance, but it is
not required to establish the alpha recall or correction boundary. Alpha does
not add a new direct "doc/ODR/ADR capture" class, and it does not mine arbitrary
implementation-session text for new technical constraints.

## Trusted Corpus Boundary

The trusted corpus is not a second authority system beside the Decision Ledger.

```text
Decision Ledger = authority spine
Trusted Corpus = ledger-governed evidence surface
```

The Decision Ledger owns accepted authority: Decisions, accepted constraints,
accepted review events, signoff, compliance/governance facts when supported,
and accepted BindingEvidence. The trusted corpus is the broader searchable
surface governed by that authority spine.

Searchable trusted corpus evidence may include:

- accepted Decisions;
- accepted constraint bundles;
- ODRs and ADRs;
- product docs and feature definitions;
- public docs and private notes;
- retained source evidence;
- accepted BindingEvidence;
- code-grounding references, GraphSnapshot identities, and code target evidence;
- contradiction or correction candidates, clearly labeled as non-authoritative.

Code grounding is therefore corpus evidence, not product truth by itself. It can
expand recall, but it cannot establish SMR applicability, currentness,
implementation truth, or closure. It cannot change product truth unless a
governed review path accepts a corresponding Decision, corpus correction,
BindingEvidence, or other governance fact.

## Lifecycle Placement

`RecallPacket` belongs to existing lifecycle surfaces:

```text
Constraint Lookup request
  -> Decision Retrieval read model
  -> Evidence Quality annotations
  -> RecallPacket returned
  -> optional user action:
       expand scope
       open contradiction candidate
       request correction
```

Primary lifecycle:

- Constraint Lookup: owns the user-facing "before work, show relevant constraints" request.

Backing lifecycle:

- Decision Retrieval: owns matching, deduplication, relevance reasons, and ordering.

Adjacent lifecycle:

- Evidence Quality Review: owns source-only, stale, unsupported, ambiguous, and review-needed annotations.

Follow-on lifecycle:

- Constraint Correction Capture: starts when the user acts on a missing, wrong,
  contradictory, or feature-definition-changing packet item and asks Bicameral
  to capture a correction against the controlled corpus.

Returning a `RecallPacket` must not create or mutate canonical state.

## Alpha Conflict Resolution Boundary

Alpha preflight may surface possible contradictions, source-only contradiction
candidates, relevant accepted Decisions, and unknown scope. It does not resolve
those contradictions.

When a user acts from preflight to add or promote information, the alpha outcome
is a proposed `DecisionCandidate`, proposed Decision, or governed correction
request. That outcome may carry conflict metadata for dashboard review, but it
must not supersede, reject, approve, mark-different-scopes, or otherwise resolve
an existing Decision.

For alpha, conflict resolution is handled through the dashboard / review
surface using governed review commands. A later MCP ToolRequest may expose
conflict-resolution actions only behind an explicit authority gate and the
bot-owned governed review path.

## Invariant

```text
Honest recall > asserted completeness.
```

Every packet must identify searched sources and unknown scope. If the user needs
more confidence, the product offers an explicit expand-scope action rather than
silently implying that unsearched sources were covered.

## Minimum Shape

```json
{
  "packet_id": "rp_...",
  "request": {
    "intent": "lookup",
    "checkpoint_hint": "optional: pre_work | mid_session | pre_write | manual_lookup",
    "query": "optional user/task text",
    "planned_action": "optional",
    "files": [],
    "symbols": []
  },
  "input_artifact_context_digest": "sha256:...",
  "corpus": {
    "corpus_id": "customer_or_workspace_corpus",
    "version": "commit, snapshot id, index id, or approved corpus version",
    "authority_spine": "decision_ledger",
    "trusted_sources": []
  },
  "applicable_spec_identities": ["spec_..."],
  "coverage_findings": [
    {
      "scope": "trusted-source-or-corpus-partition",
      "kind": "missing | unknown_scope",
      "reason": "missing_spec | unavailable | unsupported | permission_missing | stale | not_indexed | bounded | ambiguous_input"
    }
  ],
  "searched_sources": [
    {
      "source_id": "docs/product",
      "source_type": "git_docs | wiki | decision_ledger | issue_tracker | chat | meeting | other",
      "version": "source-specific version",
      "status": "searched | unavailable | unsupported | stale | not_configured"
    }
  ],
  "proposed_claim_digest": "sha256:... when a proposed claim is present",
  "optional_codegraph_expansion": {
    "status": "not_requested | unavailable | stale | unsupported | used",
    "provenance": "optional graph snapshot/scope identity"
  },
  "matches": [
    {
      "match_id": "decision_or_candidate_id",
      "kind": "accepted_decision | accepted_constraint | source_only_candidate | feature_doc | contradiction_candidate",
      "title": "short title",
      "summary": "compact context",
      "authority": "canonical | source_only | candidate | derived",
      "evidence_refs": [],
      "relevance_reasons": [],
      "freshness_state": "current | stale | unknown | unsupported",
      "review_state": "accepted | review_needed | source_only | unsupported"
    }
  ],
  "unknown_scope": [
    {
      "scope": "active PRs, unindexed wiki section, unavailable Slack channel, cross-repo docs, etc.",
      "reason": "not_configured | unavailable | unsupported | stale | permission_missing",
      "expand_action": "optional action id"
    }
  ],
  "allowed_next_actions": [
    "expand_scope",
    "open_contradiction_candidate",
    "request_correction"
  ],
  "receipt_ref": "runtime trace or preflight receipt id"
}
```

The exact wire format may differ, but implementations must preserve these
semantic fields.

The input artifact/context digest, exact corpus revision, applicable spec
identities, missing or unknown coverage findings, and any proposed-claim digest
are required SMR identities. `optional_codegraph_expansion` is not: its
unavailability may limit expansion only and must not make core recall
unavailable.

## Authority Rules

Allowed:

- cite accepted Decisions and constraints;
- cite source-only candidates separately from accepted facts;
- cite searched source versions and unknown scope;
- carry evidence quality labels;
- offer manual expand-scope actions;
- offer contradiction/correction actions that create or route proposed review
  items without resolving conflicts.

Not allowed:

- claim complete recall outside searched sources;
- claim no conflict, safe to change, compliance, signoff, or enforcement;
- create `SourceSnapshot`, `SourceEvidence`, `DecisionCandidate`, `Decision`,
  `BindingEvidence`, compliance, or signoff state;
- resolve a conflict, supersede an existing Decision, reject a conflicting
  Decision, approve signoff, or mark Decisions as different scopes from a
  preflight result;
- suppress relevant accepted Decisions because they are stale or low ranked;
- treat confidence, graph projection, or repeated use as authority;
- update the trusted corpus from chat, tickets, meetings, or PR comments unless
  that source has been explicitly approved and the governed review path accepts
  the update.
- treat source docs, code grounding, graph projections, or candidates as having
  the same authority label as accepted Decision Ledger facts.

## Correction Capture Boundary

`request_correction` is the alpha capture bridge.

It means the user is asking Bicameral to start a governed correction path using
existing Bicameral evidence/capture mechanics because the packet showed one of
these conditions:

- an implementation is about to change a product feature definition;
- a ticket, PR, chat, or meeting statement contradicts the trusted corpus;
- a relevant product constraint is missing from the trusted corpus;
- the packet found stale or ambiguous corpus text that needs product-owner
  clarification.

The captured object should preserve evidence using existing terms:

- source material becomes `SourceSnapshot` / `SourceEvidence` through
  integrations or local ingest;
- optional code evidence may be retained as provenance, while implementation
  applicability/currentness stays in the separate Code Evidence/Compliance
  lifecycle;
- any proposed doc, ODR, ADR, or feature-definition change is a downstream
  review output, not a new alpha capture object class.

The outcome may be a correction candidate or contradiction candidate that tells
the product owner what source/corpus item appears stale, missing, or
contradicted. It may also route a proposed Decision/candidate for dashboard
review. It is not conflict resolution.

It must not mean:

- automatically recording a new Decision;
- resolving a contradiction or superseding an existing Decision;
- mining arbitrary technical constraints from a session;
- introducing a new evidence class for ADRs, ODRs, product docs, or feature
  definitions instead of ingesting them as sources;
- treating agent-generated rationale as accepted product truth;
- writing source docs without the configured human/code-review path;
- bypassing the product team's corpus approval policy.

## Relation To Staged Preflight

For `preflight.run`, `RecallPacket` is the payload of the lookup stage.

Alpha should not create separate protocol modes for pre-work, mid-session, and
pre-write lookup. Those labels may be useful caller metadata, but they do not
yet define different authority, persistence, ranking, guard, or capture
semantics.

The alpha distinction that matters is:

```text
lookup = read-only recall over the configured corpus scope
request_correction = explicit user-approved request to start a governed
                     correction/corpus-update path
```

If a surface wants to preserve where lookup occurred in the session, it should
send an inert `checkpoint_hint` rather than a behavior-changing mode. The daemon
may record that hint in traces or relevance diagnostics, but it must not treat
the hint as permission to capture, block, enforce, or mutate state.

For SMR alpha, `preflight.run` still owns:

- lookup intent and optional checkpoint hint;
- capability gating;
- staged response status;
- the read-only SMR response and optional correction-request routing.

It does not own implementation guards, CodeGraph closure, code-compliance
output, or persistence from the read-only recall call.

`RecallPacket` owns only the lookup result contract:

- searched sources;
- corpus/version;
- matches;
- evidence refs;
- relevance reasons;
- freshness/readiness labels;
- unknown scope;
- allowed next actions.

Read-only staged preflight calls may return a `RecallPacket`, but they must not
capture evidence or create candidates. Correction capture is a separate
operation, even if alpha transports it through the same `preflight.run` command
family for compatibility.

For alpha, a capture-enabled flow should be interpreted as controlled corpus
correction/update capture, not general implementation-discovered technical
constraint capture.

## Relation To MCP

MCP consumes and renders the daemon-authored `RecallPacket`.

MCP may:

- choose when lookup is requested and optionally include a checkpoint hint;
- display searched sources, matches, unknown scope, and next actions;
- ask the user whether to expand scope;
- submit an expand-scope request when the user chooses it.

MCP must not:

- compute recall completeness locally;
- hide unknown scope;
- infer no-conflict from no matches;
- create local candidates or canonical state;
- resolve conflicts, supersede Decisions, reject conflicting Decisions, approve
  signoff, or mark Decisions as different scopes in alpha;
- retry capture or corpus updates without explicit user approval.

When the user chooses `request_correction`, MCP should make a second
bot-mediated request referencing the prior packet or selected packet items. The
first request remains read-only lookup; the second request starts the governed
correction path after explicit approval. If the referenced packet item appears
to conflict with existing corpus state, the second request may create or route a
proposed item for review, but alpha conflict resolution remains dashboard-owned.

## Expand-Scope Behavior

Expand scope is a user action, not an automatic hidden crawl.

The packet should make expansion concrete:

- what additional source or corpus section can be searched;
- why it was not searched already;
- what permission, connector, index, or configuration is required;
- whether the expansion is local, hosted, or unsupported in alpha.

Examples:

- "Search private product notes: not configured."
- "Search Linear project ABC: available as source-only evidence."
- "Search Slack channel #product-decisions: permission missing."
- "Search cross-repo docs: hosted/team scope required."

## Acceptance Criteria

- A packet with no matches still lists searched sources and unknown scope.
- A packet with partial coverage exposes expand-scope actions.
- A packet with source-only contradiction labels it as a candidate, not truth.
- A stale accepted Decision still appears when relevant.
- Returning a packet creates no event-store mutation.
- Acting from preflight may create or route only proposed review items; it must
  not resolve conflicts or mutate existing Decision authority.
- `request_correction` requires explicit user approval and routes to the
  governed corpus-update/correction path.
- Capture-enabled alpha flows target product corpus correction, not arbitrary
  technical-constraint mining.
- MCP conformance tests prove it renders daemon-authored unknown scope and does
  not infer completeness locally.
