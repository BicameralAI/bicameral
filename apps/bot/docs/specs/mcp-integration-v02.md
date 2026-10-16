# MCP Integration Scope Revision — v0.2 Spec

**Date:** 2026-05-31
**Status:** superseded-by-tool-request
**Source issue:** [#20](https://github.com/BicameralAI/bicameral-bot/issues/20)
**Related ADRs:** ADR-0001, ADR-0002, ADR-0003, ADR-0005, ADR-0007, ADR-0009, ADR-0010, ADR-0011
**Non-goal:** This document is a design spec, not a runtime implementation.

---

> **⚠ ToolRequest migration notice (issue #103)**
>
> The MCP v0.2 direct tool input/output contracts described in §2.2 below are
> **superseded** by the canonical `ToolRequest` / `ToolResponse` API defined in
> `protocol/` (see `protocol/README.md § ToolRequest contract (v2)`).
>
> MCP tools (`bicameral.ingest`, `bicameral.preflight`, `bicameral.bind`,
> `bicameral.review`, `bicameral.history`, `bicameral.search`) remain the
> agent-facing surface names, but their payloads must now map into a
> `ToolRequest` with an `AuthorityContext` before daemon validation.  The daemon
> returns a `ToolResponse` rather than the tool-specific output shapes shown
> below.
>
> **Direct payload compatibility with the v0.2 contracts below is not preserved.**
>
> **V1 scope note:** ADR-0022 and
> `docs/specs/codegraph-ledger-ui-protocol-boundary.md` narrow the first
> product workflow to Decision signoff plus reviewable context. The compliance
> commands, compliance states, and enforcement-memory delivery language below
> are historical/future-extension material unless a later spec explicitly
> reintroduces them. V1 E2E work may use a projection-backed read path, but it
> should follow `docs/specs/v1-reviewable-context-projection.md` and be
> described as a rebuildable reviewable-context projection, not as compliance
> or enforcement.
>
> The `ExternalIngestEnvelope` for source-system integrations remains a separate
> contract (not routed through `ToolRequest`) and cannot carry review commands,
> authority context, or governance overrides.
>
> **Migration path:**
>
> | v0.2 MCP tool | v2 ToolRequest command |
> |---------------|----------------------|
> | `bicameral.ingest` | `ingest.submit_local` |
> | `bicameral.preflight` | `preflight.run` |
> | `bicameral.bind` (propose) | `binding.create` |
> | `bicameral.bind` (inspect) | `binding.inspect` |
> | `bicameral.review` (accept_candidate) | `review.accept_candidate` |
> | `bicameral.review` (reject_candidate) | `review.reject_candidate` |
> | `bicameral.review` (approve_signoff) | `review.approve_signoff` |
> | `bicameral.review` (reject_signoff) | `review.reject_signoff` |
> | `bicameral.review` (resolve_compliance) | `review.resolve_compliance` |
> | `bicameral.history` | `history.list` |
> | `bicameral.search` | `search.query` |
>
> Type stubs: `crates/bicameral-api/src/tool_request.rs`
> JSON Schemas: `protocol/schemas/v2/tool-request.schema.json`, `protocol/schemas/v2/tool-response.schema.json`
> Conformance fixtures: `protocol/schemas/v2/fixtures/`

---

## 1. Problem Statement

The dashboard v0.1 implementation (PR #18) surfaced a concrete requirement gap: the MCP integration design (ADR-0002, ADR-0003) assumes a cloud-separation model where `bicameral-mcp` communicates with a hosted daemon, but `bicameral-bot` is now the default **local** daemon path.

Several documented capabilities have no implementation:

1. **Local code grounding** — `ComplianceState` enum exists but nothing populates it. Dashboard displays `ungrounded` for everything.
2. **SurrealDB or equivalent substrate** — Referenced in ADR-0001/ADR-0005 but not a crate dependency. No `DecisionCandidate → Decision` promotion path exists.
3. **MCP tool surface** — `bicameral.ingest`, `bicameral.preflight`, `bicameral.bind`, `bicameral.review`, `bicameral.history`, `bicameral.search`, `bicameral.dashboard` are specified but unimplemented.
4. **Source evidence pipeline** — `SourceEvidenceRef` types exist but candidates carry no evidence.

This spec resolves the requirement gap for v0.2 by defining concrete contracts, a grounding plan, a substrate decision, and a source evidence pipeline design.

---

## 2. MCP Tool Contracts for v0.2

MCP is a **local** tool surface for coding agents. It communicates with the local daemon via protocol-shaped objects — the same objects the CLI and dashboard use (ADR-0002). MCP does not maintain its own local artifacts and does not bypass governance.

### 2.1 Tool List

All tools follow the interface rule from ADR-0002:

```text
surface action
  → protocol-shaped object or ReviewCommand
  → bot validation and governance policy
  → ReviewEvent / GovernanceResult
  → event store adapter materialization, if accepted
  → replayed/materialized state
```

#### v0.2 tools (implement)

| Tool | Description | Priority |
|------|-------------|----------|
| `bicameral.ingest` | Submit source evidence and/or decision candidates | P0 |
| `bicameral.preflight` | Surface relevant decisions before implementation | P0 |
| `bicameral.bind` | Propose or inspect binding evidence for a decision/candidate | P0 |
| `bicameral.review` | Submit review commands (accept, reject, approve, etc.) | P1 |
| `bicameral.history` | Read replayed/materialized decision state | P1 |
| `bicameral.search` | Search decisions, candidates, and bindings | P2 |

#### Deferred (v0.3+)

| Tool | Reason |
|------|--------|
| `bicameral.dashboard` | Open the local review UI — dashboard already accessible via HTTP, MCP tool adds convenience but not functionality |

### 2.2 Input/Output Contracts

#### `bicameral.ingest`

Submits source evidence and/or decision candidates to the local daemon's Ingestion Gate.

**Input:**
```json
{
  "source": {
    "uri": "string",
    "source_type": "jira | linear | slack | github | meeting | manual | agent_session",
    "label": "string (optional)"
  },
  "snapshot": {
    "content": "string",
    "content_hash": "string (sha256, optional — daemon computes if omitted)",
    "captured_at": "ISO-8601"
  },
  "evidence": [
    {
      "span_start": "int",
      "span_end": "int",
      "excerpt": "string",
      "confidence": "float [0,1]"
    }
  ],
  "candidates": [
    {
      "title": "string",
      "body": "string",
      "level": "L1 | L2 | L3",
      "evidence_refs": ["int (index into evidence array)"],
      "labels": ["string"],
      "suggested_reviewers": ["string (optional)"]
    }
  ]
}
```

**Output:**
```json
{
  "source_id": "string",
  "snapshot_id": "string (content-addressed)",
  "evidence_ids": ["string"],
  "candidate_ids": ["string"],
  "governance_results": [
    {
      "candidate_id": "string",
      "action": "accepted_as_candidate | rejected_by_policy | needs_review",
      "reason": "string",
      "review_state": "advisory | proposed | needs_signoff_review | ready_to_materialize",
      "candidate_state": "proposed | rejected"
    }
  ]
}
```

**Behavior:** The daemon validates schema, computes content address for the snapshot if not provided, records `Source`, `SourceSnapshot`, and `SourceEvidence`, projects `DecisionCandidate`s through policy evaluation (ADR-0007 steps 1–3), and returns IDs and governance results.

#### `bicameral.preflight`

Surfaces relevant decisions, candidates, and bindings before an agent edits code. Returns the graph snapshot scope used for any local code grounding claims.

**Input:**
```json
{
  "files": ["string (relative path)"],
  "symbols": ["string (qualified name, optional)"],
  "diff_context": "string (unified diff, optional)",
  "branch": "string (optional, defaults to HEAD)"
}
```

**Output:**
```json
{
  "graph_snapshot": {
    "graph_snapshot_id": "string",
    "repo_id": "string",
    "commit_sha": "string",
    "ref_name": "string",
    "index_version": "string"
  },
  "relevant_decisions": [
    {
      "decision_id": "string",
      "title": "string",
      "level": "L1 | L2 | L3",
      "signoff_state": "proposed | approved | rejected | superseded",
      "review_state": "advisory | proposed | needs_signoff_review | needs_grounding_review | needs_compliance_review | collision_pending | stale_source_pending | ready_to_materialize | materialized",
      "compliance_state": "reflected | partial | drifted | pending | ungrounded",
      "bindings": [
        {
          "file": "string",
          "range": "string",
          "symbol": "string",
          "evidence_state": "verified | not_found | unknown_not_indexed | unknown_stale | ambiguous | unsupported | approximate_candidate",
          "graph_snapshot_id": "string"
        }
      ],
      "relevance": "direct_binding | file_overlap | symbol_overlap | dependency_chain"
    }
  ],
  "relevant_candidates": [
    {
      "candidate_id": "string",
      "title": "string",
      "candidate_state": "proposed | rejected | promoted"
    }
  ],
  "warnings": ["string"]
}
```

**Behavior:** Resolves a `GraphSnapshot` (ADR-0010), queries materialized decisions and candidates whose bindings intersect the requested files/symbols/diff, and returns them with evidence states. Warnings include stale index, unknown coverage, or potential conflicts.

#### `bicameral.bind`

Proposes or inspects binding evidence linking a decision or candidate to code.

**Input:**
```json
{
  "decision_or_candidate_id": "string",
  "bindings": [
    {
      "file": "string",
      "range": "string (optional, e.g. '10-25')",
      "symbol": "string (optional, qualified name)",
      "validation_token": "string (optional, from prior validate_symbols)"
    }
  ],
  "snapshot": {
    "commit_sha": "string (optional, defaults to HEAD)",
    "ref_name": "string (optional)"
  },
  "mode": "propose | inspect"
}
```

**Output:**
```json
{
  "binding_results": [
    {
      "file": "string",
      "range": "string",
      "symbol": "string",
      "evidence_state": "verified | not_found | unknown_not_indexed | unknown_stale | ambiguous | unsupported | approximate_candidate",
      "graph_snapshot_id": "string",
      "validated_sha": "string",
      "occurrence_id": "string (when verified)"
    }
  ],
  "materialized": "boolean (true only for 'propose' mode after verified bindings pass governance and event-store materialization)",
  "governance_result": {
    "action": "binding_accepted | binding_rejected | needs_verification",
    "reason": "string"
  }
}
```

**Behavior:** In `inspect` mode, validates requested symbols/files against the resolved `GraphSnapshot` (ADR-0010) and returns evidence states without materializing. In `propose` mode, validates first, and only verified bindings may enter governance for `BindingEvidence` materialization. Verification is necessary but not sufficient: policy and event-store materialization must still accept the command. Rejects with typed snapshot mismatch if validation and binding snapshots differ.

#### `bicameral.review`

Submits review commands through the governance path.

**Input:**
```json
{
  "command": "accept_candidate | reject_candidate | approve_signoff | reject_signoff | resolve_compliance | edit_candidate | request_context | assign_reviewer | supersede_decision | bind_to_code | mark_different_scopes | pause_approval | resume_approval",
  "target_id": "string (decision or candidate ID)",
  "payload": {
    "reason": "string (optional)",
    "edit": { "title": "string", "body": "string" },
    "reviewer": "string",
    "superseded_by": "string",
    "compliance_verdict": "reflected | partial | drifted",
    "binding_evidence": {}
  }
}
```

**Output:**
```json
{
  "review_event_id": "string",
  "command": "string",
  "result": "accepted | rejected | needs_capability",
  "reason": "string",
  "new_state": {
    "signoff_state": "proposed | approved | rejected | superseded",
    "review_state": "advisory | proposed | needs_signoff_review | needs_grounding_review | needs_compliance_review | collision_pending | stale_source_pending | ready_to_materialize | materialized",
    "compliance_state": "reflected | partial | drifted | pending | ungrounded"
  }
}
```

**Behavior:** Submits a `ReviewCommand` to the daemon. The daemon evaluates allowed-command gating (ADR-0007 step 4), checks owner/member capability, and produces a `ReviewEvent` and `GovernanceResult`. Only commands allowed by current governance state and substrate capability are accepted.

#### `bicameral.history`

Reads replayed/materialized decision state.

**Input:**
```json
{
  "decision_id": "string (optional — omit for full ledger)",
  "include_events": "boolean (default false)",
  "include_bindings": "boolean (default false)",
  "since": "ISO-8601 (optional)"
}
```

**Output:**
```json
{
  "decisions": [
    {
      "decision_id": "string",
      "title": "string",
      "body": "string",
      "level": "L1 | L2 | L3",
      "signoff_state": "proposed | approved | rejected | superseded",
      "review_state": "advisory | proposed | needs_signoff_review | needs_grounding_review | needs_compliance_review | collision_pending | stale_source_pending | ready_to_materialize | materialized",
      "compliance_state": "reflected | partial | drifted | pending | ungrounded",
      "source_evidence": [...],
      "bindings": [...],
      "events": [...]
    }
  ]
}
```

#### `bicameral.search`

Search decisions, candidates, bindings, and source evidence.

**Input:**
```json
{
  "query": "string",
  "scope": "decisions | candidates | bindings | all",
  "filters": {
    "level": "L1 | L2 | L3",
    "signoff_state": "proposed | approved | rejected | superseded",
    "compliance_state": "reflected | partial | drifted | pending | ungrounded",
    "file": "string (path filter)"
  },
  "limit": "int (default 20)"
}
```

**Output:**
```json
{
  "results": [
    {
      "type": "decision | candidate | binding",
      "id": "string",
      "title": "string",
      "snippet": "string",
      "relevance_score": "float"
    }
  ]
}
```

---

## 3. Grounding Implementation Plan

### 3.1 Architecture

Per ADR-0003 and ADR-0010, the **local daemon** owns code grounding for the current repo/worktree. MCP and dashboard are clients that request grounding operations and receive protocol-shaped evidence.

```text
Agent (MCP client)
  → bicameral.preflight / bicameral.bind
  → Local Daemon Grounding Service
    → GraphSnapshot resolution (HEAD, base, merge-base)
    → Tree-sitter parser (per-language)
    → File/symbol/range inspection
    → SymbolOccurrence + evidence state
  → Protocol-shaped response
```

### 3.2 What Code Inspects What

| Component | Inspects | Produces |
|-----------|----------|----------|
| **SnapshotResolver** | Git refs (`HEAD`, base branch, merge-base), worktree state | `GraphSnapshot` with `commit_sha`, `ref_name`, `index_version` |
| **FileInspector** | Filesystem paths against `GraphSnapshot` | File existence, blob hash, line count |
| **SymbolResolver** | Source files via tree-sitter (Rust, TypeScript, Python initially) | `SymbolOccurrence` records: qualified name, file, line/range, structural hash |
| **DiffAnalyzer** | `git diff` between snapshots | Changed files, changed symbols, new/removed symbols |
| **BindingValidator** | Proposed binding target + `SymbolOccurrence` or file/range evidence | Evidence state: `verified`, `not_found`, `unknown_not_indexed`, etc. |
| **ComplianceEvaluator** | Accepted `BindingEvidence` + current code state | `ComplianceVerdict`: `reflected`, `partial`, `drifted`, `pending`, `ungrounded` |

### 3.3 ComplianceVerdict Production

`ComplianceVerdict` answers: "Does the current code state satisfy or violate a bound decision?"

**Input:** A `Decision` with one or more accepted `BindingEvidence` records, each referencing a verified `SymbolOccurrence` or file/range target at a specific `GraphSnapshot`.

**Algorithm (minimum viable):**

1. Resolve current `GraphSnapshot` (typically `HEAD`).
2. For each accepted `BindingEvidence` record:
   a. Locate the bound file in the current snapshot.
   b. If file not found → binding is `not_found`.
   c. If file found, parse with tree-sitter and locate the bound symbol.
   d. If symbol found at same file/range with same structural hash → `verified`, binding holds.
   e. If symbol found at different range or with different structural hash → `verified` but `drifted` (code changed since binding).
   f. If symbol not found → `not_found` (symbol removed or renamed).
   g. If file/language not supported by parser → `unsupported`.
3. Aggregate binding evidence states into a `ComplianceVerdict`:
   - All bindings `verified` and structurally unchanged → `reflected`.
   - Some bindings `verified` but structurally changed → `partial`.
   - Any binding `not_found` or most structurally changed → `drifted`.
   - No bindings exist → `ungrounded`.
   - Bindings exist but cannot be validated (stale index, unsupported) → `pending`.

### 3.4 Minimum Viable Grounding (v0.2)

v0.2 focuses on what can be done locally without a hosted code graph:

1. **File-path matching:** Match `CodeRegion` file paths against the actual filesystem at `HEAD`.
2. **Tree-sitter symbol resolution:** Parse supported languages, extract symbols, validate bindings.
3. **Structural hash comparison:** Detect drift by comparing structural hashes between binding-time and current snapshot.
4. **Evidence state reporting:** Always return typed evidence states (ADR-0010), never approximate as verified.

**Not in v0.2:**
- Cross-branch or cross-repo grounding (hosted/cloud).
- Import/caller/callee graph traversal (requires SymbolDB, deferred).
- Historical grounding or time-series drift analysis.

---

## 4. Event-Store Substrate Decision

### 4.1 Decision: Git-backed event store for v0.2, SurrealDB deferred

**Rationale:**

1. **SurrealDB is premature for v0.2.** The bot has no SurrealDB dependency today. Adding it introduces operational complexity (embedded DB lifecycle, data migration, backup) before the governance flow and grounding are proven.

2. **Git-backed event store aligns with the product thesis.** ADR-0001 defines git as a first-class substrate. `.bicameral/` text files are reviewable, replayable, and auditable through ordinary PR workflow. This is the "boring is good" approach.

3. **The substrate contract is neutral.** ADR-0001 and ADR-0007 define a substrate-neutral contract. Building v0.2 on git forces the implementation to respect that contract, making future SurrealDB (or SQLite, or Drive) adoption a substrate swap rather than a rewrite.

4. **Local caches are acceptable.** SurrealDB (or SQLite) may serve as a **local materialization cache** for fast queries, search indexing, and dashboard state. The cache is rebuilt from the git event store via replay. This preserves SurrealDB's query strengths without making it canonical authority.

### 4.2 Git-Backed Event Store Layout

Extends ADR-0001's minimum layout:

```text
.bicameral/
  events/
    <timestamp>-<event-type>-<short-id>.yaml
  decisions/
    DEC-<id>.yaml
  candidates/
    CAND-<id>.yaml
  bindings/
    DEC-<id>.yaml
  sources/
    <source-id>/
      snapshots/
        sha256-<source-snapshot-addr>.json
      evidence/
        <evidence-id>.yaml
  policy/
    governance.yaml
```

**Event types:** `ingest`, `accept_candidate`, `reject_candidate`, `approve_signoff`, `reject_signoff`, `resolve_compliance`, `bind_to_code`, `supersede_decision`, `edit_candidate`.

**Materialization:** The daemon replays events from `.bicameral/events/` to produce the current materialized state in `.bicameral/decisions/`, `.bicameral/bindings/`, etc. Materialized files are derived state — the event log is the source of truth. Retained `SourceSnapshot` bodies and `SourceEvidence` pointers live under `.bicameral/sources/` when the git-backed substrate owns retained source evidence. `GraphSnapshot` indexes are daemon caches; accepted events store only graph snapshot identity and reviewed binding evidence, not parsed graph internals.

### 4.3 Promotion Path: DecisionCandidate → Decision

```text
DecisionCandidate (via ingest)
  → Policy evaluation (schema, provenance, source trust, level)
  → ReviewCommand: accept_candidate
    → Policy: allowed-command gating, capability check
    → ReviewEvent: candidate_accepted
    → Materialization: Decision(signoff_state = proposed)
  → ReviewCommand: approve_signoff
    → Policy: owner/member review, evidence strength
    → ReviewEvent: signoff_approved
    → Materialization: Decision(signoff_state = approved)
```

**Demotion:** `supersede_decision` or `reject_signoff` emits audit events and transitions the decision to `superseded` or `rejected`. Later-discovered accepted conflict evidence reopens an approved Decision to `signoff_state = proposed` with `review_state = collision_pending`. All transitions are replayable events.

### 4.4 SurrealDB Role (Future)

When the governance flow and grounding are proven on git:

- SurrealDB becomes the primary **Enforcement Memory Projection** for fast graph queries, full-text search, dashboard state, code/work applicability, signoff reads, compliance reads, and daemon-side operational observations.
- SurrealDB does not become an authority substrate. Accepted governance events still materialize through the selected event store substrate, and SurrealDB is rebuilt from replay plus graph/source indexes.
- The event-store contract (ADR-0001) ensures this is an additive operational-memory layer, not a rewrite of canonical authority.

For the V1 local E2E path, the same storage technology may be used only as the
rebuildable reviewable-context projection defined in
`docs/specs/v1-reviewable-context-projection.md`: Decisions, cited source
evidence, accepted/advisory code evidence, graph readiness, and allowed user
actions. That projection must not expose compliance verdicts, implementation
correctness, feature readiness, or blocking enforcement unless a future
reviewed compliance/enforcement event is explicitly in scope.

---

## 5. Source Evidence Pipeline Design

### 5.1 Overview

The evidence pipeline ensures that every `DecisionCandidate` entering the system carries provenance back to its source material. Without evidence, candidates are claims without audit trail.

```text
Source (Jira, Slack, meeting, manual)
  → Capture: connector or manual entry
  → SourceSnapshot: content-addressed immutable view
  → SourceEvidence: span pointer into snapshot
  → DecisionCandidate: cites evidence_refs
  → Decision: inherits evidence chain
```

### 5.2 Pipeline Stages

#### Stage 1: Capture

A source connector (or manual entry, or agent session) records a `Source`:

```yaml
source:
  id: SRC-001
  uri: "https://linear.app/team/issue/PROJ-123"
  source_type: linear
  label: "PROJ-123: Add health check endpoint"
  created_at: 2026-05-31T00:00:00Z
```

The `Source` is the mutable reference to an external object. It may change or disappear; Bicameral does not track every external update.

#### Stage 2: SourceSnapshot

When the source content is consumed for governance purposes, a `SourceSnapshot` is captured:

```yaml
snapshot:
  id: SNAP-abc123  # derived from content_hash
  source_id: SRC-001
  content: |
    Title: Add health check endpoint
    Description: The service needs a /healthz endpoint that returns 200 when...
    Priority: P1
    Labels: backend, reliability
  content_hash: "sha256:a1b2c3d4..."
  captured_at: 2026-05-31T00:05:00Z
  capture_method: api_poll | webhook | manual | agent_extract
```

**Content addressing:** `content_hash` is SHA-256 of the canonical snapshot representation (UTF-8 normalized content). This is the UOR-compatible content address referenced in ADR-0007.

**When to snapshot:** A new snapshot is recorded when captured source state affects Bicameral internal state: candidate projection, candidate invalidation, review evidence, decision provenance, binding/governance evidence, freshness-sensitive governance, or collision/dependency analysis (ADR-0007 §4).

#### Stage 3: SourceEvidence

`SourceEvidence` is a span pointer into a snapshot — the specific excerpt that supports a claim:

```yaml
evidence:
  id: EVD-001
  snapshot_id: SNAP-abc123
  span_start: 45
  span_end: 120
  excerpt: "The service needs a /healthz endpoint that returns 200 when the database connection is healthy"
  confidence: 0.92
  extraction_method: llm_extract | regex | manual
```

Multiple `SourceEvidence` records may point into the same snapshot. A candidate may cite evidence from multiple snapshots when the claim depends on multiple sources.

#### Stage 4: Candidate Attachment

`DecisionCandidate` carries `evidence_refs` linking to `SourceEvidence`:

```yaml
candidate:
  id: CAND-001
  title: "Health check endpoint must verify DB connection"
  body: "The /healthz endpoint should return 200 only when the database connection pool reports healthy status."
  level: L2
  evidence_refs:
    - EVD-001
  source_id: SRC-001
  created_at: 2026-05-31T00:10:00Z
```

When the candidate is promoted to a `Decision`, the evidence chain is preserved. Reviewers can trace: Decision → Candidate → Evidence → Snapshot → Source.

### 5.3 Integration Connectors (v0.2 Scope)

v0.2 implements the pipeline infrastructure. Specific connectors are integration concerns:

| Connector | v0.2 | Notes |
|-----------|------|-------|
| Manual/CLI | Yes | `bicameral ingest` with inline content or file input |
| Agent session (MCP) | Yes | `bicameral.ingest` tool with structured input |
| GitHub PR/issue ingest | Deferred | Requires explicit owner-approved SourceBinding; GitHub event-store access is not source-ingest approval |
| Linear | Planned v0.3 | Via `bicameral-integrations` |
| Slack | Planned v0.3 | Via `bicameral-integrations` |
| Meeting transcript | Planned v0.3 | Via `bicameral-integrations` |

### 5.4 Evidence Validation Rules

1. Every `DecisionCandidate` entering the Ingestion Gate must carry at least one `evidence_ref`. Candidates with empty evidence are rejected by policy with `reason: "no_source_evidence"`.
2. `SourceSnapshot.content_hash` is verified on ingest. If the caller provides a hash, the daemon verifies it matches the content. If omitted, the daemon computes it.
3. `SourceEvidence` span boundaries must fall within the snapshot content length.
4. Evidence `confidence` below a configurable threshold (default 0.5) triggers `needs_review` governance result instead of automatic candidate acceptance.

---

## 6. Dependency on #13 (Collision Check) — Clarification

### 6.1 Relationship

Issue #13 asks for the broader dependency/collision check ADR. ADR-0011 now defines the v0.2 known-evidence release valve: source-only signoff may proceed when no conflict is known within available evidence, and later-discovered accepted conflict evidence reopens the affected Decision for review. This spec (issue #20) asks for the MCP integration, grounding, substrate, and evidence pipeline design. The relationship is:

| Concern | Owner | Dependency direction |
|---------|-------|---------------------|
| Collision/dependency check design | ADR-0011 and #13 | ADR-0011 defines known-evidence signoff and reopen semantics; #13 remains the broader follow-up for exact check algorithms |
| MCP tools that surface collision results | #20 (this spec) | #20 defines *how* collision results appear in MCP preflight/review responses |
| Grounding implementation that collision checks depend on | #20 (this spec) | #13 depends on grounding producing `BindingEvidence` before code-grounded collision checks can run |
| Graph snapshot identity for collision checks | ADR-0010 | Both #13 and #20 depend on snapshot-scoped evidence |

### 6.2 What #20 Provides That #13 Needs

1. **`GraphSnapshot` resolution** — #13's collision checks cannot run code-grounded comparisons without snapshot identity. This spec defines how `SnapshotResolver` produces `GraphSnapshot` records.
2. **`BindingEvidence` production** — #13's same-symbol and same-region contradiction checks need verified bindings. This spec defines how `BindingValidator` produces evidence states.
3. **`ComplianceVerdict` semantics** — #13 needs to know what `drifted` vs `partial` vs `reflected` means in practice. This spec defines the algorithm.

### 6.3 What #13 Provides That #20 Needs (Not Blocking)

1. **Collision detection algorithm** — Which same-symbol or same-region contradictions are safe for local checks vs require hosted graph. This affects what `bicameral.preflight` can warn about.
2. **`ConflictSignal` contract** — The protocol object for collision results. `bicameral.preflight` will surface these when available.
3. **Additional post-ingest transition rules** — ADR-0011 allows source-only signoff with no known conflict; #13 may add more precise local/hosted collision checks that affect `bicameral.review` warnings or allowed-command gating.

### 6.4 Resolution

**#13 is not blocking for v0.2 implementation.** The MCP tools, grounding, substrate, and evidence pipeline can all be built with ADR-0011's known-evidence semantics: if collision data is unavailable or not implemented, MCP responses omit collision fields and signoff proceeds according to ordinary governance. When #13's remaining design work lands:

- `bicameral.preflight` gains a `conflicts` field in its response.
- `bicameral.review` gains collision-aware warnings or allowed-command gating for checks that produce policy-accepted conflict evidence.
- The `ComplianceEvaluator` gains collision-aware verdict production.

Until #13 lands, collision-related fields in MCP responses are omitted or empty, and `accept_candidate` proceeds without collision gating. Later-discovered conflict evidence follows ADR-0011 by reopening the affected approved Decision into `proposed` / `collision_pending` review state rather than silently superseding or rejecting it.

---

## 7. Implementation Sequence

Recommended build order for v0.2:

1. **Git-backed event store adapter** — `.bicameral/events/` replay, materialization to `decisions/`, `candidates/`, `bindings/`.
2. **Source evidence pipeline** — `Source`, `SourceSnapshot`, `SourceEvidence` capture and content addressing.
3. **Ingestion Gate with evidence** — `DecisionCandidate` creation with mandatory `evidence_refs`, policy evaluation.
4. **Promotion path** — `accept_candidate → Decision(proposed)`, `approve_signoff → Decision(approved)`.
5. **SnapshotResolver + FileInspector** — `GraphSnapshot` resolution from git refs.
6. **SymbolResolver** — Tree-sitter parsing for Rust, TypeScript, Python.
7. **BindingValidator + ComplianceEvaluator** — Evidence state production and `ComplianceVerdict` algorithm.
8. **MCP tools** — `bicameral.ingest`, `bicameral.preflight`, `bicameral.bind`, `bicameral.review`, `bicameral.history`, `bicameral.search` as thin clients over daemon services.
9. **Dashboard wiring** — Replace hardcoded/empty data with materialized state from event store.

---

## 8. Risks and Follow-ups

| Risk | Mitigation |
|------|-----------|
| Tree-sitter language coverage gaps | Start with Rust/TS/Python; return `unsupported` evidence state for others |
| Git event store performance at scale | Event log is append-only; materialized state is cached. If performance becomes an issue, add SQLite/SurrealDB as materialization cache |
| Evidence pipeline adds friction to manual ingest | CLI `bicameral ingest` accepts plain text with auto-generated evidence span covering full content |
| Collision checks (#13) change allowed-command gating | MCP tool contracts are designed with optional collision fields; adding them is additive |
| Snapshot identity mismatch between validate and bind | ADR-0010 contract with typed snapshot mismatch rejection prevents silent cross-snapshot errors |
| SurrealDB community expectations | Document the git-first, cache-second decision clearly; SurrealDB path remains open for v0.3+ |

---

## Appendix A: Domain Entity Candidates

| Term | Disposition | Notes |
|------|-------------|-------|
| `SnapshotResolver` | Product repo (bicameral-bot) | New component name for graph snapshot resolution |
| `FileInspector` | Product repo (bicameral-bot) | New component name for file-level code inspection |
| `SymbolResolver` | Product repo (bicameral-bot) | New component name for tree-sitter symbol extraction |
| `DiffAnalyzer` | Product repo (bicameral-bot) | New component name for diff-based change detection |
| `BindingValidator` | Product repo (bicameral-bot) | New component name for binding evidence validation |
| `ComplianceEvaluator` | Product repo (bicameral-bot) | New component name for compliance verdict production |
| `capture_method` | Product repo (protocol) | New field on `SourceSnapshot` for provenance |
| `extraction_method` | Product repo (protocol) | New field on `SourceEvidence` for extraction provenance |
