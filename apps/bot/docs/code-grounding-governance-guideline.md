# Code-Grounding Governance Guideline

**Status:** proposed
**Issue:** https://github.com/BicameralAI/bicameral-bot/issues/163
**Related:** ADR-0002, ADR-0003, ADR-0010, ADR-0017, ADR-0019, ADR-0021;
`specs/graphslice-planning-sufficiency.md`;
`specs/graph-remediation-revalidation.md`

## Purpose

This guideline captures the operating model for tasks at the boundary between
code intelligence, code search, graph evidence, and governance authority.

Bicameral is not building generic code search. It is building the evidence
layer that tells the system whether an agent, tool, integration, or reviewer
claim about code should be trusted by governance workflows.

The core question is:

```text
How do we make code-grounding evidence trustworthy enough that governance
workflows can rely on it without pretending local graph evidence is stronger
than it is?
```

## Authority Model

The bot-owned protocol/runtime contract is the canonical authority boundary for
code-grounding evidence. The daemon executes validation, graph repair,
governance policy, and materialization under that contract.

Bot-owned authority includes:

- `ToolRequest` command/query semantics;
- snapshot-scoped graph identity such as `validated_sha` and
  `graph_snapshot_id`;
- `GraphStore`, `GraphSnapshot`, and `GraphSlice` lifecycle semantics;
- `ScopeSufficiencyProof` for deciding whether scoped graph evidence can
  support verified, absent, remediation, or readiness claims;
- `BindingEvidence` materialization;
- graph readiness and preflight readiness semantics;
- the event-store path for durable governance facts.

Daemon execution is not the same thing as durable authority. The daemon may
parse code, index snapshots, expand slices, refresh stale graph data, retry
validation, and propose derived currentness. Durable governance facts still
materialize only through the selected event store substrate after policy or
reviewer acceptance.

## Caller Roles

Local product surfaces map user or agent intent into bot-owned `ToolRequest`s:

- MCP remains the coding agent's local tool surface.
- CLI/TUI commands provide operator and fixture-driven surfaces.
- The dashboard renders review actions and emits `ToolRequest`s.
- Optional local/cloud advisory clients may request graph or conflict evidence.

These surfaces may improve ergonomics, cache local helper data, and present
warnings. They must not independently decide that a code binding is valid,
resolve compliance, approve signoff, or write canonical state.

External integrations do not use `ToolRequest`. They submit
`ExternalIngestEnvelope` evidence at the ingest boundary. Integrations can
produce source evidence, candidates, hints, dependency signals, and advisory
results. They cannot claim local actor/session authority or emit final
governance conclusions.

Outbound write-back, notification, sync, or annotation belongs to a separate
future `Egress` contract category.

## Governed Evidence Retrieval

`search.query` should be treated as governed evidence retrieval, not generic
search. A search result that influences governance should carry:

- source, path, symbol, binding, or graph relevance reason;
- snapshot identity when graph evidence is involved;
- evidence state, such as `verified`, `unknown_stale`,
  `unknown_not_indexed`, `unsupported`, `ambiguous`, `not_found`, or
  `approximate_candidate`;
- provenance distinguishing accepted bot-owned `BindingEvidence` from
  source-only matches, approximate candidates, LLM hints, or MCP-local helper
  state.

Approximate semantic or LLM-ranked search may improve recall. It must not prove
absence, compliance, conflict, or binding validity until exact validation
returns verified evidence over a sufficient scope.

## Graph Proof Boundary

Codegraph acts as a deterministic prover only for the scope it can identify,
inspect, and prove complete.

The proof flow is:

```text
TaskIntent
  -> SnapshotResolution
  -> GraphScopePlan
  -> GraphSlice build/read/expand
  -> ScopeSufficiencyProof
  -> GraphEvidenceState / readiness output
```

No local graph result may become `verified`, `not_found`, `no conflict`,
`safe`, or grounded compliance unless a sufficient proof covers the requested
task scope.

Graph states outside a sufficient proof remain explicit:

- `unknown_not_indexed`;
- `unknown_stale`;
- `unsupported`;
- `ambiguous`;
- `approximate_candidate`;
- `hosted_required`;
- `failed`.

Unknown or stale graph state is not a conflict by itself. It may block a
grounded graph read, produce warnings, or route a specific binding problem to
review, but it must not become `collision_pending`.

## Remediation Boundary

Graph remediation repairs recomputable graph facts. It may:

- index a snapshot;
- expand a slice;
- refresh or rebuild stale graph data;
- invalidate affected files, symbols, or neighborhoods;
- retry validation;
- return remediation job status;
- propose user, reviewer, or hosted next actions.

Graph remediation must not directly create or mutate:

- accepted `BindingEvidence`;
- durable `RevalidateBinding` events;
- review state such as `needs_grounding_review`;
- compliance verdicts;
- signoff state;
- `collision_pending`;
- release-valve governance results.

Automatic remediation is allowed only when deterministic local graph proof
covers the required scope. User, policy, reviewer, or hosted action is required
when repair needs workspace mutation, missing refs, dirty-worktree choices,
large scope expansion, hosted/team visibility, or semantic disambiguation.

## MCP Thin-Client Requirement

MCP should act as a client and transport layer over bot-owned `ToolRequest`.

MCP can own tool ergonomics, compatibility, and client-side fixtures. It should
not re-own graph identity, binding verification, grounded preflight, remediation
authority, or CodeGenome semantics that belong behind the bot-owned authority
boundary.

The MCP cutover is complete only when expected free/local workflows still work
through bot-owned contracts:

- local grounding against the current repo/worktree;
- preflight;
- binding create/inspect;
- history/search;
- dashboard opening or review handoff where applicable.

Hosted/cloud capabilities may add organization-scale graph intelligence,
cross-repo and cross-branch analysis, historical indexing, shared caches,
blast-radius analysis, conflict advisory, and optimized grounding recall or
latency. Hosted capability does not replace bot-owned local authority.

## CodeIdentity And CodeGenome Direction

Jin's review left the broader CodeIdentity direction intentionally open.

The current operating model is:

- deterministic GraphStore/codegraph proof can support verified graph evidence
  for a bounded scope;
- probabilistic CodeGenome-style evidence can improve recall, prioritization,
  and UX for a broader set of desync cases;
- probabilistic evidence must remain advisory or confidence-rated unless an
  accepted exact provider check proves the claim for the requested scope;
- CodeGenome-style evidence must not be laundered into
  `GraphEvidenceState::verified`.

Future CodeIdentity work should evaluate UX implications before deciding
whether Bicameral stays with narrow deterministic graph evidence or designs a
broader substrate combining deterministic and probabilistic providers.

The accepted direction from #154 is a graph provider abstraction: CodeGenome-
style capability is directionally useful as an optional provider behind
ADR-0010, while Rust `SymbolResolver` remains the first exact-validation slice
and conformance baseline. Provider-specific identities are evidence inside
`CodeTargetIdentity`, not canonical product identity by themselves.

## Jin Review Outcome

Jin reviewed the core boundary questions for this guideline.

1. Bot-owned authority boundary: yes. The bot should be canonical because even
   when grounding needs HITL, the human decision is mediated through the bot and
   therefore still lands through the bot-owned authority path.
2. Graph remediation: prefer proof-driven auto-remediation. If codegraph can
   deterministically prove a repair path, that path should be auto-remediable.
   The product balance is avoiding false positives without overwhelming users
   with review prompts.
3. CodeIdentity direction: undecided between narrow codegraph and broader
   CodeGenome. Evaluate this through UX implications. A promising shape is a
   `CodeIdentity` framework where deterministic codegraph gives guarantees for
   a smaller set of desync cases while CodeGenome-style methods give
   probabilistic support for a larger set.

Product surfaces should distinguish:

- auto-repaired by deterministic graph proof;
- needs human grounding review;
- probabilistic advisory evidence.

## Architecture Review Questions

Pull Jin or architecture review into boundary decisions, not routine
implementation. Best-fit review points:

- whether a graph state is auto-remediable, user-triggered, or review-only;
- whether a new provider can produce exact verified evidence or only advisory
  candidates;
- whether a caller is a local `ToolRequest` actor, external ingest producer, or
  future egress surface;
- whether a vocabulary item such as `validated_sha`, `GraphScopeCoverage`,
  `CodeIdentity`, or `ScopeSufficiencyProof` should become factory doctrine;
- whether MCP functionality still works as a thin client after graph/search
  changes.

## Source Anchors

- [ADR-0002: Agent Surfaces And Bot Runtime Interface](adr/0002-agent-surfaces-and-bot-runtime-interface.md)
  defines `ToolRequest` as the canonical local tool request contract and keeps
  `ExternalIngestEnvelope` and future `Egress` separate.
- [ADR-0003: Local Code Grounding And Cloud Conflict Boundary](adr/0003-local-code-grounding-and-cloud-conflict-boundary.md)
  says public/local Bicameral includes current-worktree grounding while cloud
  owns organization-scale graph and conflict advisory infrastructure.
- [Protocol README](../protocol/README.md) defines the protocol surface
  categories and shared vocabulary.
- [ADR-0010: Snapshot-Scoped Local Code Graph Contract](adr/0010-snapshot-scoped-local-code-graph-contract.md)
  defines `GraphSnapshot`, `GraphEvidenceState`, graph facts versus governance
  facts, and binding lineage semantics.
- [ADR-0017: Binding ToolRequest Evidence Contract](adr/0017-binding-toolrequest-evidence-contract.md)
  moves binding authority to bot-owned `ToolRequest` and restricts
  materialization to verified graph evidence.
- [ADR-0019: Grounded Preflight And Compliance Readiness](adr/0019-grounded-preflight-compliance-readiness.md)
  defines preflight readiness and unknown/hosted-required scope behavior.
- [ADR-0021: GraphStore Snapshot And Slice Lifecycle](adr/0021-graphstore-snapshot-slice-lifecycle.md)
  defines `GraphStore`, `GraphSnapshot`, `GraphSlice`, cache identity,
  invalidation, and `BindingAnchor` retention.
- [GraphSlice Planning And Sufficiency Semantics](specs/graphslice-planning-sufficiency.md)
  defines `TaskIntent`, `GraphScopePlan`, and `ScopeSufficiencyProof`.
- [Graph Remediation And Revalidation Repair Paths](specs/graph-remediation-revalidation.md)
  defines remediation classes, operations, revalidation outcomes, response
  shape, and graph/governance boundaries.
- [Graph Provider Abstraction And CodeTargetIdentity](specs/graph-provider-abstraction.md)
  defines the CodeGenome-style provider direction, provider contract, and
  `CodeTargetIdentity` sharpening from #154.

## Follow-Up Work

- #134: align `search.query` with the governed evidence model.
- #120, bicameral-mcp#552, and bicameral-mcp#554: complete MCP thin-client
  cutover and conformance.
- #162: clean up graph/preflight integration state after the dev-to-main cycle.
- #156, #160, and bicameral-factory#37: promote durable graph vocabulary such
  as `validated_sha` and `GraphScopeCoverage` into factory context.
