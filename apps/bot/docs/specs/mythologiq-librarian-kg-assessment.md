# MythologIQ Assets For Librarian Recall And Knowledge Graph Gaps

Date: 2026-06-19
Status: draft assessment
Source reviewed: `BicameralAI/MythologIQ-Labs` cloned to `/private/tmp/MythologIQ-Labs`

## Decision

For the librarian/cataloguing idea, use this stack:

1. CODEGENOME for graph identity, affected-surface mapping, provenance, and
   confidence fusion.
2. FailSafe for the agent workflow surface: preflight, replay, development
   tracker, and review packet UX.
3. EvolveAI for freshness and decay concepts, not as a separate memory engine.
4. qortara-governance for deterministic tool-dispatch boundaries around MCP and
   integrations.
5. Qor-logic for governance vocabulary and evidence discipline only.

Do not import Qortara, FailSafe-Pro, COREFORGE, FORGEMIND, or Qortara-OS into
the librarian path now. They are useful references, but they expand the product
surface away from the narrow "reliable recall plus HITL decision update" wedge.

## Critical Assessment

| Asset | Verdict | Why |
|---|---|---|
| CODEGENOME | Highest relevance; spike now | Directly addresses knowledge graph gaps: content-addressed identity, overlays, provenance, confidence fusion. It is still a research prototype, so treat it as design substrate first. |
| FailSafe | Reuse patterns now | Strongest implemented workflow surface: Linear/Jira preview, governed MCP/Bicameral tool calls, tracker, replay, drift events. Use the interaction patterns, not the product shell. |
| EvolveAI | Adapt concepts | Good for memory freshness, tiering, decay, and failure memory. Do not import "autopoietic memory" wholesale; Bicameral needs tracked Decisions, not self-evolving authority. |
| qortara-governance | Adapt boundary | Useful model for synchronous tool-dispatch policy checks. Current scope is LangChain/LangGraph alpha, so reuse the contract idea, not the package. |
| Qor-logic | Vocabulary only | Good evidence/gate/ledger discipline. Importing the S.H.I.E.L.D. ceremony would add workflow noise, which is exactly Alex's complaint. |
| GG-CORE | Later / optional | Relevant for private local inference, PII, injection detection. Not needed for librarian recall unless a deployment demands air-gapped or local model execution. |
| CRITIQ | Optional evidence attachment | Useful only if visual evidence becomes part of sidecar/customer workflows. Not core to decision recall. |
| qortara-sdlc | Reference | Good handoff/workflow ideas, but planned. Pull only the explicit artifact-backed handoff idea. |
| FailSafe-Pro | Reference only | OS enforcement and L3 queues are overpowered for this wedge. Use approval/trust ideas later. |
| Qortara | Reference only | Hosted fleet governance/compliance is future packaging, not librarian V1. |
| COREFORGE | Reference only | Local-first Vault/agent boundaries are relevant, but building a desktop shell would distract. |
| FORGEMIND | Defer | Oversight/trust UI ideas later. Too product-expansive now. |
| Qortara-OS | Defer entirely | Long-range research, no immediate Bicameral implementation value. |

## Knowledge Graph Gaps To Patch

### Stable Node Identity

Borrow CODEGENOME's content-addressed identity idea for code, docs, source
snapshots, evidence spans, and constraint packets.

Required Bicameral shape:

- `CorpusNodeId` for docs/source corpus nodes;
- `CodeTargetIdentity` for code targets, aligned with existing ADR-0010 and
  ADR-0021 semantics;
- content hash plus source location, not location alone;
- old node identities remain inspectable after source refresh.

### Evidence Provenance

Every graph edge should record:

- producing observer or extractor;
- source snapshot or graph snapshot;
- method;
- confidence/evidence state;
- freshness;
- whether the edge is reviewed, inferred, or advisory.

This must not collapse into canonical authority. Provenance supports recall and
review; accepted governance events still materialize through Bicameral's event
store path.

### Completeness Boundary

Librarian recall must report what it actually searched:

- corpus identity and version;
- indexed docs and sources;
- skipped or unavailable sources;
- local graph scope;
- `unknown_scope` for anything outside local visibility.

Completeness means complete for the configured reviewed corpus and declared
scope, not global organizational omniscience.

### Preflight Graph Pipeline

CODEGENOME-style graph lookup should plug into the existing `preflight.run`
pipeline rather than replace it:

```text
request
  -> corpus/snapshot resolution
  -> source-only retrieval
  -> diff/path/symbol extraction
  -> graph slice lookup or expansion
  -> binding and graph overlap
  -> deduplication
  -> deterministic ordering
  -> readiness plus per-result evidence state
```

Preflight remains read-only. It must not mutate Decisions, candidates, signoff,
binding, compliance, or conflict state.

### Freshness And Decay

Adapt EvolveAI's temporal tiering and decay as metadata for recall confidence:

- recent reviewed Decisions remain high-salience;
- stale source evidence lowers readiness;
- repeated corrections raise review priority;
- immutable accepted Decisions do not decay out of authority.

Decay should change ranking, warnings, and review prompts. It must not silently
demote or supersede canonical Decisions.

### Contradiction And Failure Memory

Adapt Shadow Genome as recurring contradiction/failure pattern memory:

- repeated missed constraint;
- repeated stale decision surprise;
- recurring false positive;
- repeated agent violation of the same accepted constraint;
- repeated ambiguity in a source family such as Slack or Linear.

These are operational learning signals. Canonical updates still require HITL
review.

### Tool Boundary

Use qortara-governance's dispatch-path idea for MCP and integrations:

- policy decisions happen at the actual tool/request boundary;
- tool calls are evaluated before high-consequence action;
- evidence and review commands remain separate;
- denied or routed calls produce typed receipts.

This should strengthen Bicameral's `ToolRequest` dispatcher, not introduce a
second authority path.

### User-Facing Workflow

Use FailSafe's tracker/replay patterns for:

- session-start preflight results;
- why a constraint was shown;
- what corpus was searched;
- what contradictions were flagged;
- what review action is now allowed;
- replayable "why did the agent act on this?" evidence.

The UX target is a compact review packet, not a broad governance dashboard.

## Near-Term Implementation Shape

Build a `Librarian Recall Contract` runtime spec around:

```text
configured corpus
  -> source/doc/code graph nodes
  -> evidence-backed constraint packets
  -> preflight recall with coverage proof
  -> contradiction candidates
  -> HITL accept/reject/supersede
  -> tracked Decision freshness
```

CODEGENOME patches the graph substrate. FailSafe patches the operator and agent
workflow. EvolveAI patches freshness semantics. qortara-governance patches tool
boundaries. Qor-logic patches language discipline. Everything else stays out of
the initial wedge.

## Adoption Rules

- Extract primitives, not products.
- Preserve Bicameral's existing event-store and HITL authority path.
- Treat CODEGENOME as advisory graph/projection input until Bicameral validates
  exact snapshot-scoped evidence.
- Treat FailSafe as UX and workflow precedent, not as the shell Bicameral must
  run inside.
- Treat EvolveAI memory as ranking/freshness metadata, not mutation authority.
- Treat qortara-governance as a boundary pattern, not an external dependency
  without a separate integration decision.
- Treat Qor-logic as discipline, not ceremony.

## Recommended First Spikes

### Spike 1: CODEGENOME Graph Identity Feasibility

Goal: determine whether CODEGENOME concepts or crates can help Bicameral map
constraints to repos, files, symbols, source spans, and confidence-scored
affected surfaces.

Output:

- reuse/adapt/defer recommendation;
- required SDK/protocol contracts;
- bot read-model changes;
- conformance fixtures for identity, provenance, and unknown scope;
- risks around test status, API churn, and authority boundaries.

### Spike 2: FailSafe-Style Recall Packet

Goal: design the compact agent/operator packet for librarian recall.

Output:

- MCP response shape for session-start preflight;
- dashboard/sidecar packet shape;
- evidence/replay fields;
- contradiction triage action set;
- no-mutation tests for preflight and search.

### Spike 3: Freshness And Failure Memory Model

Goal: adapt EvolveAI/Shadow Genome concepts into Bicameral-native metadata.

Output:

- freshness fields;
- ranking impact;
- review escalation rules;
- failure-pattern candidate schema;
- explicit rule that freshness metadata cannot demote or supersede Decisions.

