# ADR-0017: Binding ToolRequest Evidence Contract

**Date:** 2026-06-04  
**Status:** proposed; HITL-blocked on token-shape confirmation  
**Level:** L1  
**Issue:** https://github.com/BicameralAI/bicameral-bot/issues/116  
**Related:** ADR-0002, ADR-0003, ADR-0007, ADR-0010, ADR-0012;
`../releases/alpha/desync-ux-hitl-readiness.md`

## Problem

`bicameral-mcp` is being reduced to a thin client that maps tool calls to
bot-owned `ToolRequest` commands. Binding authority must therefore move to the
bot daemon without preserving MCP-owned `bind`, `validate_symbols`,
`get_neighbors`, `link_commit`, or CodeGenome behavior.

The bot already has partial pieces:

- `ToolRequest` declares `binding.create` and `binding.inspect`;
- `BindingSpec` carries file/range/symbol hints;
- `SnapshotResolver` resolves `HEAD`, local refs, and merge-base snapshots;
- `FileInspector` and `bind_to_code` enforce verified-only file/range
  materialization;
- `RustSymbolResolver` returns ADR-0010 evidence states;
- replay/materialized state can carry `BindingEvidence`.

The missing contract is the canonical request/response shape and handoff rule
for binding evidence. In particular, issue #116 asks for human confirmation of
the exact binding evidence payload returned to MCP and whether validation
tokens are the first-pass handoff shape.

## Decision

`binding.create` and `binding.inspect` are canonical bot `ToolRequest`
commands. MCP and other local surfaces submit binding hints or inspection
queries; the bot resolves a graph snapshot, validates evidence, applies policy,
and returns typed evidence results.

Only `GraphEvidenceState::Verified` may materialize `BindingEvidence`.
Unsupported, stale, ambiguous, not-indexed, not-found, approximate, expired, or
snapshot-mismatched results remain visible to callers but do not become durable
binding evidence.

Durable git-backed binding identity anchors to:

- `graph_snapshot_id`;
- `validated_sha`.

`ref_name` is provenance and resolution input only. Branch names move and must
not be treated as durable identity.

## Human Decision Still Required

This ADR proposes the first-pass contract, but one point remains intentionally
HITL-blocked:

Should validation tokens be accepted in the first implementation slice?

Recommended answer:

- direct binding hints are accepted first and revalidated by the bot before
  materialization;
- validation-token input is represented in the protocol but returns
  `validation_token_unsupported` until token signing, expiry, integrity checks,
  and revocation behavior are implemented;
- no implementation may accept an unsigned or unprotected token as proof of
  verified evidence.

If reviewers choose immediate token support, tokens must bind at least:

- `graph_snapshot_id`;
- `validated_sha`;
- validated file/range or symbol occurrence identity;
- validator identity and version;
- evidence state;
- issue time;
- expiry time.

The token must be integrity-protected and rejected when expired, revoked,
mismatched, malformed, or non-verified.

## `binding.create` Request

`binding.create` accepts local binding hints plus optional snapshot identity.

```json
{
  "name": "binding.create",
  "params": {
    "decision_or_candidate_id": "DEC-1",
    "bindings": [
      {
        "file": "crates/bicameral-runtime/src/bind_to_code.rs",
        "range": "53-112",
        "symbol": "bind_to_code"
      }
    ],
    "commit_sha": "optional requested durable snapshot",
    "ref_name": "optional provenance ref",
    "graph_snapshot_id": "optional expected snapshot",
    "validation_token": "optional future handoff token"
  }
}
```

Contract notes:

- `decision_or_candidate_id` identifies the ledger target.
- `file` is a relative path inside the workspace.
- `range` is optional and identifies a code region.
- `symbol` is optional and asks for exact symbol validation.
- `commit_sha`, `ref_name`, and `graph_snapshot_id` scope validation.
- `validation_token` is optional and may be unsupported in the first
  implementation slice.

## `binding.create` Response

Successful materialization returns `status = "bound"` and includes:

- the materialized `BindingEvidence`;
- the `GraphSnapshot` used for validation;
- `validated_sha`;
- typed warnings, even when empty.

The binding metadata must preserve enough audit information to replay and
inspect the evidence without storing the full graph index:

- `validated_sha`;
- `validated_ref`, when known;
- `evidence_state`;
- content hash or structural hash when available;
- symbol name / qualified name when symbol-based;
- resolver or inspector identity and version.

Non-bound responses use explicit typed statuses:

- `snapshot_mismatch`;
- `not_found`;
- `unknown_not_indexed`;
- `unknown_stale`;
- `ambiguous`;
- `unsupported`;
- `approximate_candidate`;
- `validation_token_invalid`;
- `validation_token_expired`;
- `validation_token_unsupported`.

Non-bound responses must not include materialized `BindingEvidence`.

Alpha release UX for non-bound responses is defined in
`../releases/alpha/desync-ux-hitl-readiness.md`. Non-bound responses may become
review cards, readiness blocks, source-only context, or validation requests,
but they remain non-authoritative until exact validation and governed
materialization succeed.

## `binding.inspect` Request

`binding.inspect` reads durable binding evidence and may optionally ask for
derived currentness against a locally resolvable snapshot.

```json
{
  "name": "binding.inspect",
  "params": {
    "decision_or_candidate_id": "DEC-1",
    "commit_sha": "optional currentness snapshot",
    "ref_name": "optional currentness ref",
    "graph_snapshot_id": "optional expected currentness snapshot"
  }
}
```

## `binding.inspect` Response

Inspection returns:

- durable `BindingEvidence` records for the decision or candidate;
- the accepted snapshot identity for each record;
- graph readiness / snapshot-resolution state;
- derived currentness warnings when requested and locally resolvable.

Inspection is read-only. Derived currentness does not mutate old
`BindingEvidence` and does not become durable governance state unless a later
policy or reviewer action explicitly materializes a governed event.

## Snapshot Rules

The local daemon may validate against locally resolvable snapshots:

- current `HEAD`;
- authoritative base ref when present locally;
- feature branch `HEAD` when present locally;
- merge-base when both refs are locally resolvable.

Rules:

- unresolved refs return `SnapshotResolution` failures, not `not_found`;
- `not_found` is reserved for an inspected snapshot where the target was
  absent;
- snapshot mismatch rejects or revalidates explicitly;
- stale or dirty evidence returns `unknown_stale`;
- approximate search output remains `approximate_candidate` until exact
  validation succeeds;
- first-pass exact symbol support may be Rust-only through
  `RustSymbolResolver`, with non-Rust exact symbol targets returning
  `unsupported`.

## Implementation Boundary

Implementation depends on the daemon dispatch boundary in #114. Until that
boundary is accepted, #116 should not wire product behavior through an invented
dispatcher.

After #114 is accepted, implementation should add:

1. API response structs for bound and non-bound binding results.
2. Optional `graph_snapshot_id` on create/inspect params.
3. Optional `validation_token` only if token handling is approved.
4. A runtime binding service that resolves snapshots, revalidates hints, and
   materializes verified evidence only.
5. Read-only inspection over replay/materialized binding state.
6. Dispatch from `ToolCommand::BindingCreate` and
   `ToolCommand::BindingInspect` through bot validation and policy.

## Required Tests

Acceptance coverage must include:

- verified file binding;
- verified range binding;
- verified Rust symbol binding;
- snapshot mismatch;
- stale graph / dirty file;
- unsupported symbol target;
- ambiguous target;
- approximate target;
- existing binding inspection;
- validate-then-bind fixtures for `HEAD`, locally available authoritative base,
  feature branch `HEAD`, and merge-base;
- missing local refs produce snapshot-resolution failures rather than
  `not_found`.

## Consequences

Positive:

- binding authority moves to the bot without preserving MCP-owned graph
  behavior;
- callers get typed non-bound results instead of silent fallback;
- durable evidence remains snapshot-scoped and replayable;
- branch names stay provenance rather than identity.

Risks:

- deferring validation-token support means first-pass callers must submit hints
  for bot-side revalidation;
- immediate token support requires careful integrity and expiry design;
- symbol binding is limited by current resolver coverage and must expose
  `unsupported` honestly.

## Non-Goals

- Do not implement grounded preflight surfacing here.
- Do not preserve MCP `validate_symbols`, `get_neighbors`, `link_commit`, or
  CodeGenome authority.
- Do not add a full graph-per-branch storage model.
- Do not treat branch names as durable identity.
- Do not mutate old `BindingEvidence` during inspection.
- Do not materialize approximate, stale, ambiguous, unsupported, or not-indexed
  evidence.
