# ADR-0019: Code Evidence Preflight And Compliance Readiness

**Date:** 2026-06-05  
**Status:** proposed  
**Level:** L1  
**Issue:** https://github.com/BicameralAI/bicameral-bot/issues/117  
**Related:** ADR-0002, ADR-0003, ADR-0007, ADR-0010, ADR-0011, ADR-0017;
`../releases/alpha/desync-ux-hitl-readiness.md`;
`../specs/preflight-search-readiness.md`

## Problem

The Code Evidence/Compliance `preflight.run` profile is the local command that
uses graph and binding evidence before code changes begin. During the MCP
refactor, MCP-owned preflight, graph, relevance, telemetry, and CodeGenome
compatibility behavior must be deleted or mapped into bot-owned evidence
contracts.

The current bot has a source-only preflight surface. It honestly reports
`ungrounded` compliance and avoids false conflict claims, but it does not yet
carry graph readiness, exact snapshot scope, grounded binding evidence states,
or compliance readiness warnings.

The missing contract is not "find every possible conflict." Local Bicameral
cannot see every branch, active PR, team worktree, cross-repo graph, or hosted
Conflict Oracle result. The contract must be faithful about the exact local
snapshot scope used for preflight and must not present local "no conflict" as
global consistency across unseen branches.

## Decision

The Code Evidence/Compliance `preflight.run` profile is a bot-owned
`ToolRequest` query that returns relevant Decisions and candidates with:

- deterministic relevance results;
- exact graph snapshot scope;
- graph readiness metadata;
- binding/evidence states;
- compliance readiness visibility;
- typed warnings for unknown, stale, unsupported, ambiguous, approximate,
  source-only, or not-indexed evidence.

Local preflight may reason only over locally resolvable snapshots:

- current `HEAD`;
- authoritative base when present locally;
- feature branch `HEAD` when present locally;
- merge-base when both refs are locally resolvable;
- additional local worktree branches when explicitly present and resolvable.

Everything broader must surface as unknown or hosted-required, not as a clean
local pass. Broader active-PR, cross-branch, team-wide stale-branch,
cross-repo, and Conflict Oracle behavior belongs to hosted/team graph services
unless the relevant snapshot is explicitly available locally.

The durable target of this profile is grounded, scoped code-evidence preflight. Alpha release presentation
and HITL routing for incomplete graph coverage are defined in
`../releases/alpha/desync-ux-hitl-readiness.md`: alpha preflight may provide
useful Decision context while still surfacing unknown, stale, unsupported,
approximate, source-only, and hosted-required states as readiness boundaries.
The deterministic search order, progressive readiness modes, and local
no-conflict wording for implementation are defined in
`../specs/preflight-search-readiness.md`.

This ADR does not define the SMR alpha preflight result. Under ADR-0028, SMR
alpha recalls applicable product-approved specifications from the named trusted
corpus, reports missing or unknown corpus coverage, and stays available without
CodeGraph. This ADR's graph readiness, binding evidence, compliance warnings,
and no-conflict language apply only when the separate Code Evidence/Compliance
lifecycle owns the request.

## Readiness Contract

The response must include a top-level readiness model for the requested
preflight scope:

- `state`: ADR-0010 `GraphReadiness`;
- `snapshot_scope`: the list of snapshots considered;
- `snapshot_resolution`: resolved and unresolved refs/worktrees;
- `unknown_scope`: requested or implied scopes not visible locally;
- `warnings`: bounded user-facing warnings;
- `required_actor`: daemon, workspace member, reviewer, admin, or hosted/team
  service when applicable.

The response may include Decision-specific grounding status only when the issue
is specific to a Decision's binding/evidence, such as:

- stale accepted binding evidence;
- ambiguous symbol occurrence;
- unsupported target;
- approximate-only candidate;
- snapshot mismatch between accepted evidence and requested preflight scope.

General graph indexing, missing base refs, missing worktree branches, and
hosted/team-only visibility are view-level readiness facts. They must not be
sprinkled into every Decision as durable lifecycle state.

## Evidence And Compliance Semantics

Preflight distinguishes source-only relevance from grounded evidence:

- source-only matches may return relevant Decisions and candidates;
- source-only results carry `compliance_state = ungrounded` unless durable
  binding/compliance evidence exists;
- caller LLM output is not verified evidence;
- approximate search results are suggestions only;
- only verified graph evidence may support grounded compliance conclusions or
  blocking governance results when policy accepts them.

Compliance readiness is advisory/query output unless a later governed command
materializes an event. Preflight must not mutate signoff, review, compliance,
or binding state.

In alpha, non-verified preflight output must also constrain agent behavior. A
source-only, approximate, stale, unsupported, or not-indexed result may guide
planning, inspection, or reviewer requests, but it must not authorize the agent
to claim no conflict, create `BindingEvidence`, or resolve compliance.

Known-evidence collision semantics from ADR-0011 apply:

- no known local conflict is not global consistency;
- unknown graph visibility is not a conflict by itself;
- verified or policy-accepted conflict evidence may later trigger the ADR-0011
  release valve;
- unresolved graph grounding must not produce `collision_pending`.

## Relevance And Dedup

First-pass relevance must be deterministic and fixture-testable.

Allowed first-pass relevance signals:

- source text match;
- file path match;
- symbol name match;
- diff-context terms;
- verified binding target overlap when binding evidence exists;
- verified symbol occurrence overlap when resolver evidence exists.

Deduplication must be stable for fixture inputs:

- a Decision appears once even if several relevance signals match;
- promoted candidates do not duplicate their materialized Decisions;
- source-only and grounded relevance reasons may both be reported on the same
  row;
- ordering is deterministic, for example by severity/readiness and stable title
  or id.

## CodeGenome And MCP Telemetry

This RFQ must not recreate MCP CodeGenome as a bot subsystem.

First-pass preflight replaces MCP-local continuity behavior with:

- `BindingEvidence` and binding lineage semantics from ADR-0010/ADR-0017;
- `GraphSnapshot` scope;
- `GraphEvidenceState`;
- compliance readiness warnings;
- explicit unknown/hosted-required states for unavailable branch/team scope.

Any future CodeGenome-like continuity subsystem requires a separate ADR and
ownership decision.

MCP-local preflight telemetry is not authority. It may be retired or mapped to
future bot observability, but it must not become hidden relevance, compliance,
or conflict authority.

## Response Shape

The exact Rust structs may evolve, but `preflight.run` responses must carry:

- `relevant_decisions`;
- `relevant_candidates`;
- `readiness`;
- `snapshot_scope`;
- `warnings`;
- per-result relevance reasons;
- per-result evidence state when grounded evidence is involved;
- compliance readiness state or warning;
- explicit unknown state for unseen branches or hosted/team-only scope.

Example readiness shape:

```json
{
  "readiness": {
    "state": "ready",
    "snapshot_scope": [
      {
        "name": "HEAD",
        "graph_snapshot_id": "gs-...",
        "validated_sha": "...",
        "ref_name": "feature/preflight"
      },
      {
        "name": "merge-base(origin/dev, HEAD)",
        "graph_snapshot_id": "gs-...",
        "validated_sha": "...",
        "merge_base_sha": "..."
      }
    ],
    "unknown_scope": [
      {
        "scope": "team_active_prs",
        "state": "hosted_required",
        "detail": "Local preflight cannot see team-wide active PR branches."
      }
    ],
    "warnings": [
      "Local no-conflict result is limited to resolved local snapshots."
    ]
  }
}
```

## Implementation Boundary

Implementation depends on #114's daemon dispatch boundary and can consume #116
binding evidence fixtures before full binding materialization lands.

After the dispatch boundary is accepted, implementation should:

1. extend API response structs for preflight readiness and snapshot scope;
2. resolve requested local snapshots with `SnapshotResolver`;
3. preserve existing source-only preflight behavior as a fallback mode;
4. incorporate materialized `BindingEvidence` when available;
5. evaluate compliance readiness through existing compliance semantics without
   mutating state;
6. return explicit unknown/hosted-required states for unseen branch/team scope;
7. keep MCP as a thin client mapping to canonical `ToolRequest`.

## Required Tests

Acceptance coverage must include:

- grounded match;
- source-only result;
- stale graph;
- unsupported graph;
- no relevant Decisions;
- duplicate suppression;
- compliance warning output;
- explicit unknown state for unseen branches;
- preflight over `HEAD`;
- preflight over locally available authoritative base;
- preflight over locally available feature branch `HEAD`;
- preflight over locally resolvable merge-base;
- preflight over multiple local worktree branches where locally resolvable.

Validation path:

```sh
cargo test -p bicameral-runtime
cargo test -p bicameral-api
```

Fixture tests should cover `preflight`, `grounding`, and replayed Decision
state.

## Consequences

Positive:

- preflight becomes honest about local versus hosted/team graph visibility;
- source-only usefulness remains while grounded evidence improves precision;
- compliance readiness becomes visible without treating caller LLM output as
  verified evidence;
- MCP authority deletion can proceed without preserving MCP-local graph,
  telemetry, or CodeGenome behavior.

Risks:

- callers must handle unknown readiness states instead of assuming a clean pass;
- first-pass local preflight may feel conservative when hosted/team visibility
  is unavailable;
- deterministic relevance is intentionally simpler than future graph-powered
  ranking.

## Non-Goals

- Do not implement binding materialization here except by consuming the binding
  evidence contract.
- Do not implement dashboard rendering here.
- Do not recreate MCP CodeGenome as a bot subsystem.
- Do not preserve MCP-local preflight telemetry as hidden authority.
- Do not claim local no-conflict results are global/team-wide consistency.
- Do not materialize compliance or conflict events from preflight query output.
