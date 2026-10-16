# CodeGenome Authority Cleanup (RFQ 10 / #221)

**Status:** proposed — RFQ response (decision: **retire MCP-local authority; keep CodeGenome as substrate/signal**)
**Issue:** [#221](https://github.com/BicameralAI/bicameral-bot/issues/221) "Decide bot-owned replacement or retirement for CodeGenome identity and drift"
**Plan source:** `docs/plans/mcp-refactor-reassignment.md` §RFQ 10
**Owner:** @Knapp-Kevin · **Secondary reviewer:** @silongtan
**Governs by:** ADR-0024 (field roles), ADR-0010 (graph evidence states), ADR-0017 (binding evidence contract), ADR-0019 (grounded preflight)
**MCP pre-refactor pointer:** `bicameral-mcp` commit `0827444c80d45fe3474f68002166e1fc35708eda`
**Move/replace/retire:** **retire** MCP-local CodeGenome *authority*; **keep** CodeGenome *substrate/signals*; bot already owns the *interpretation/materialization* surfaces.

## This is not a "kill CodeGenome" track

CodeGenome stays valid as a code-memory substrate and signal source. What is retired is MCP **acting as a governance brain** — turning CodeGenome confidence signals into durable verdicts and binding changes without bot governance.

Per **ADR-0024**: CodeGenome-derived values (overlay confidence, signal propagation, graph readiness, drift classification, continuity match score) are `signal_input` / `signal_output` / `identity_substrate`. They may rank, warn, propose, and explain. They must **never** cross an `authority_gate`. Only bot-governed surfaces materialize durable state.

## Keep / Retire / Move

### KEEP (CodeGenome substrate & signals)
- Code-memory substrate, graph identity, content addressing — `CODEGENOME` Rust repo (`codegenome-core`, `codegenome-substrate`) and its signature/diff modules.
- Overlay confidence, signal propagation, graph readiness, affected-surface discovery, recall efficiency.
- Drift **classification** algorithm and continuity **matching/scoring** — as advisory signal generators (`bicameral-mcp/codegenome/drift_classifier.py`, `continuity.py`, `deterministic_adapter.py`, `diff_categorizer.py`).

### RETIRE (MCP-local authority — the side door)
- `bicameral-mcp/codegenome/drift_service.py` — `_write_auto_resolution()` writes a `compliance_check` row with `verdict="compliant"` directly; `_write_or_hint()` gates on `confidence >= 0.80` (`drift_service.py:127`). **A signal threshold materializing a governance verdict.**
- `bicameral-mcp/codegenome/continuity_service.py` — `_persist_resolved_match()` ends in `update_binds_to_region(...)` (`continuity_service.py:119`), flipping a decision's bound region from an approximate continuity `match.confidence`, with no verified-gate revalidation.
- `bicameral-mcp/handlers/link_commit.py` — `_run_drift_classification_pass()` (`242-299`) and `_run_continuity_pass()` (`373-443`) orchestrate the side-door auto-materialization. Retire the passes; ledger-sync mechanics (dedup, ephemeral detection, sync metrics) are RFQ 1/2/17, not #221.

### MOVE (already owned by the bot — confirmed, not net-new)
The "move behind bot surfaces" leg is **already done**; #221 does not re-implement it, it routes CodeGenome signals into it:
- `GraphSnapshot` / grounding — `bicameral-bot:crates/bicameral-runtime/src/grounding.rs`, `graph_snapshot_service.rs`.
- `GraphReadiness` / `GraphEvidenceState` — `crates/bicameral-api/src/evidence.rs:255`, `:354`.
- `preflight.run`, `decision.find_code_impact` (discovery gate) — runtime dispatch (`dispatch.rs:912`).
- `binding.create` / `binding.inspect` — verified-only materialization (`dispatch.rs:827`, `:1227`; `allows_binding()` at `evidence.rs:379`).

## Authority boundary vs RFQ 11 / RFQ 8 / RFQ 1-2 (the seam that must not collide)

`bicameral-mcp/handlers/resolve_compliance.py` enforces real governance invariants — caller-verdict persistence, the **reflected-before-drifted** state transition (`resolve_compliance.py:182-206`), `binds_to` pruning on `not_relevant`, holistic status re-projection. The CodeGenome side door (`drift_service`) writes `compliance_check` rows **bypassing those invariants** (it calls `upsert_compliance_check` directly, skipping `decision_exists` / `region_exists` / state-transition checks).

| Concern | Owner | #221 relationship |
|---|---|---|
| Compliance verdict command (semantics, reflected-before-drifted, pruning, projection) | **RFQ 11** → bot `review.resolve_compliance` | #221 stops CodeGenome from writing verdicts behind this command; it does **not** redefine the command |
| Binding materialization / rebind | **RFQ 8 / Track 1** → `binding.create` (verified-only) | #221 turns continuity auto-rebind into a *rebind proposal* that must pass `binding.create` revalidation |
| compliance_check / binds_to / identity-supersedes row writes | **RFQ 1/2** → bot event store | #221 removes MCP raw-row writes; durable state becomes governed events |

**Cut:** #221 owns **severing the signal→authority coupling** (closing the CodeGenome side door). The governed front doors that *apply the invariants and materialize* are RFQ 11 (compliance), RFQ 8 (binding), RFQ 1/2 (store). #221 must not implement compliance or binding semantics itself, or it re-creates the very overlap it is closing.

## Exhibits (the violations #221 removes)

1. **Signal→verdict** — `bicameral-mcp/codegenome/drift_service.py:127`: `if classification.verdict == "cosmetic" and classification.confidence >= 0.80:` → `_write_auto_resolution()` persists `verdict="compliant"`, `semantic_status="semantically_preserved"`. Confidence is acting as an authority gate (forbidden, ADR-0024).
2. **Approximate identity→binding change** — `bicameral-mcp/codegenome/continuity_service.py:119`: `update_binds_to_region(decision_id, region_id, new_region_id)` off `match.confidence`, no verified revalidation (violates Track 1 / ADR-0017 verified-only binding *in MCP's parallel store*).

Both write to MCP's own ledger store, so the bot's `BindingEvidence` is **not** corrupted today — but they are a parallel authority path that ADR-0024 forbids and #221 closes.

## Target shape

```
CodeGenome (CODEGENOME repo + MCP/integrations signal modules)
   │  emits: drift classification, continuity match candidates, confidence, readiness  (signals, no authority)
   ▼
bicameral-bot governed surfaces
   ├─ review.resolve_compliance (RFQ 11): applies verdict semantics + invariants → governed compliance event
   ├─ binding.create (RFQ 8/Track 1): revalidates a rebind proposal → verified-only BindingEvidence
   └─ preflight.run / find_code_impact: surfaces drift/continuity as advisory readiness, never as verdicts
   ▼
event store (RFQ 1/2): the only place durable verdict/binding state is materialized
```

## Exit criteria (mapped to #221)

1. **No MCP-local CodeGenome authority remains** — `drift_service` auto-resolution, `continuity_service` auto-rebind, and the `link_commit` drift/continuity passes are removed. ⬜
2. **CodeGenome-derived values are signal inputs or identity-substrate inputs** — drift/continuity output is advisory (classification + score + readiness), carrying no verdict and writing no governance row. ⬜
3. **Bot owns interpretation and materialization** — any verdict goes through RFQ 11; any rebind through `binding.create` revalidation (Track 1 gate); any durable write through the event store (RFQ 1/2). ✅ surfaces exist; ⬜ MCP rerouted.
4. **No approximate identity creates/flips a binding** — continuity proposes; only verified `binding.create` materializes. ✅ bot gate intact; ⬜ MCP side door removed.

## Required tests

- MCP: after retirement, no path writes `compliance_check` or flips `binds_to` from a confidence threshold; drift/continuity emit advisory signals only.
- Bot (exists): `binding.create` rejects non-verified evidence (`allows_binding()`); discovery gate rejects non-verified candidates.
- Cross-repo: a CodeGenome continuity match becomes a *rebind proposal* that is rejected by `binding.create` unless it re-verifies at the resolved snapshot.
- Negative: a `cosmetic`/`confidence=0.95` drift signal does **not** by itself produce a compliant verdict — it requires the governed compliance command.

## See also

- `docs/specs/source-adapter-gateway-migration-218.md` (Track 2, sibling cleanup)
- `docs/adr/0010-snapshot-scoped-local-code-graph-contract.md` · `docs/adr/0017-binding-toolrequest-evidence-contract.md` · `docs/adr/0024-signal-variables-authority-gates-durable-state.md` · `docs/adr/0019-grounded-preflight-compliance-readiness.md`
- `docs/plans/mcp-refactor-reassignment.md` §RFQ 10 (and §RFQ 11 compliance, §RFQ 8 binding, §RFQ 1-2 ledger for the boundary)
