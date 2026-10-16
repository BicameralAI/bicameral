# CodeGenome / UOR Recall Evaluation (#526)

**Status:** research note + recommendation — **pending the AC-required hands-on engineering notes from @jinhongkuan and @silongtan.** This document is the automated inspection/assessment deliverable that feeds their decision; it does not itself close #526 and creates no BindingEvidence or authority.
**Issue:** [#526](https://github.com/BicameralAI/bicameral-bot/issues/526) "Research: Evaluate CodeGenome Auto Research loop for alpha recall discovery"
**Owner:** @Knapp-Kevin · **Reviewers:** @jinhongkuan, @silongtan
**Governs by:** ADR-0024 (field roles), ADR-0010 (snapshot-scoped graph), ADR-0017 (binding evidence), ADR-0019 (grounded preflight)
**Inputs treated as fixed (per @silongtan, #526):** bounded Option B (#525), preflight impact-to-decision recall (#537), hybrid touched-surface freshness (#625)

## Governing principle (sharpens the whole evaluation)

> Everything leans hard toward **deterministic**. Probabilistic methods are reserved for **redefining the boundaries within deterministic limits** — run offline, within a bounded/reproducible envelope, with output **frozen into deterministic config**. The probabilistic step never sits in the request path or any user/agent-facing surface.

This is intentionally **stronger than ADR-0024's signal model.** ADR-0024 permits a `signal_input` to rank/warn at runtime. This principle says: do not rank-at-runtime with probabilistic scores at all. Reserve probabilistic work for setting the deterministic envelope, then freeze it. Where the two differ, this principle governs alpha shipping; ADR-0024's gate rules still hold (no signal ever crosses an `authority_gate`).

A corollary: an alpha **narrows scope, it does not lower the completeness bar.** A small deterministic recall shipped fully beats a larger probabilistic recall shipped with caveats.

## What was inspected

- **CodeGenome** (`MythologIQ-Labs-LLC/CodeGenome`): Rust 1.88+; 9 overlays (Syntax / Semantic / Flow / Process / PDG / Runtime / SCIP / LSP); BLAKE3 content identity; noisy-OR confidence fusion `P = 1 − ∏(1 − cᵢ)`; signal propagation; 4-tier Auto Research loop (Mechanical → Adaptive → Agent-Guided → Autopoietic). RUN-001: 224,703 iterations / ~62h. Reported finding: one architectural change (fusion) improved the baseline by **+0.35**; 224K parameter-tuning iterations improved it by **+0.006**.
- **uor-addr** (`UOR-Foundation/uor-addr`): deterministic `sha256:<64hex>` labels with **semantic equivalence across serialized forms** (JCS for JSON, Rivest S-expr, XML-C14N 1.1, DER) before hashing; 361 tests; `no_std`/`no_alloc` core; Rust/C/WASM bindings; Apache-2.0.
- **UOR-Framework** (`UOR-Foundation/UOR-Framework`): algebraic content-addressed ontology (Z/(2ⁿ)Z); 34 namespaces / 474 classes / 948 properties; exports JSON-LD/Turtle/OWL/SHACL; Rust + Lean; v0.5.2.
- **Local anchors:** ADR-0024 role taxonomy; `mythologiq-librarian-kg-assessment.md` (2026-06-19); `codegenome-authority-cleanup-221.md` (#221 — CodeGenome retained as substrate/signal, MCP-local authority retired).

## Recommendation: **C + constrained-A; explicitly not B**

Build a **smaller, Bicameral-specific deterministic recall (Path C)**, and retain CodeGenome's **Auto Research loop as the sanctioned probabilistic boundary-redefiner (a constrained slice of Path A)** — *not* as runtime fusion. Reject the originally-tempting **Path B** ("adapt selected components behind contracts + render caveats"): its fuse-and-caveat surface ships probabilistic output into the answer, which the governing principle disqualifies.

| Component | Verdict | Rationale |
|---|---|---|
| Deterministic recall surface | **Ship (C)** | Scope-proven, content-addressed: "searched X / found Y / excluded Z," with `unknown_scope` for anything outside local visibility. No confidence scores, no caveats in the surface. Complete because it is *honest and total within a declared scope*. |
| Auto Research loop | **Retain as boundary-redefiner (constrained A)** | This is its precise legitimate home: run **offline**, on a **bounded corpus** with **fixed reproducible fitness**, and **freeze** the output into deterministic config (overlay set, scope rules, thresholds-become-rules). Its own RUN-001 finding argues for this — architecture choices dominate tuning, so use the loop to *choose the deterministic envelope*, not to score live answers. **Mechanical/Adaptive tiers only.** |
| Autopoietic tier of the loop | **Exclude** | Self-evolving + unbounded + live = probabilistic redefining boundaries *outside* deterministic limits. Forbidden by the principle (and by the no-self-evolving-authority rule in the librarian assessment / ADR-0024). |
| Runtime confidence fusion | **Exclude from alpha** | Probabilistic in the request path. This is #639. |
| Caveat rendering surface | **Exclude from alpha** | Hedged/incomplete-feeling output. This is #640. |
| `uor-addr` | **Spike as the deterministic identity primitive** | Serialization-invariant content addressing is itself deterministic and fits the identity layer cleanly (same evidence as JSON vs XML vs S-expr → same label). Additive to CodeGenome's BLAKE3, justified where cross-serialization equivalence matters (SourceEvidence excerpts, SourceSnapshot artifacts, GraphSlice member identity). Spike, don't adopt blind. |
| `UOR-Framework` | **Defer** | 474-class ontology far exceeds alpha recall needs. |

## ADR-0024 field-role classification (AC requirement)

- `identity_anchor` / identity substrate (deterministic, ships): BLAKE3 node identity; `uor-addr` label if spiked.
- `signal_input` used **only offline** by the boundary-redefiner (frozen, never live): overlay confidence, fused confidence, propagation depth, fitness scores.
- `signal_output` at runtime: **none in alpha** under the governing principle — the deterministic surface reports searched-scope and matches, not probabilistic rankings.
- Never a runtime field, never an `authority_gate`: fitness functions (ImpactAccuracy, PropagationDepth, CycleTime, GraphDensity) and any loop fitness result. (The `confidence ≥ 0.80 → verdict` coupling #221 retired is the canonical violation.)

## Safe alpha use (AC requirement)

Deterministic recall over a fixed reviewed corpus, fed by CodeGenome's **deterministic overlays only** (Syntax / Semantic / SCIP / LSP) and content-addressed identity, surfaced through `CodeLocator` / `RecallPacket` with an explicit searched-scope proof. Ranks/explains by deterministic criteria (exact match, scope membership, recency-of-reviewed-Decision), never by probabilistic confidence. Materializes nothing; verified `binding.create` remains the only path to BindingEvidence.

## Object identity value vs authority value (AC requirement)

Content addresses (BLAKE3 or `uor-addr`) are **identity substrate** — they assert "this is the same artifact," never "this is approved/bound." Identity feeds ranking inputs and binding-*target* inputs; only the verified gate materializes durable state.

## Noise / risk to exclude from alpha runtime

- Heavy/low-signal overlays for a docs+code corpus: Runtime, Process, PDG.
- The autopoietic research tier (self-evolving, unbounded).
- Confidence-as-gate (the #221 side-door pattern).
- Runtime confidence fusion and caveat surfaces (#639 / #640).
- Federation / belief modules — out of alpha scope.

## Impact on the existing implementation queue (#635–#641)

| Issue | Disposition under this recommendation |
|---|---|
| #635 provider seam, typed statuses, no-op providers | **Keep** — deterministic plumbing. |
| #636 deterministic fake semantic provider fixtures | **Keep** — deterministic. |
| #637 recall-admissibility gate + negative authority fixtures | **Keep** — enforces the boundary. |
| #638 local semantic chunk/index metadata MVP | **Keep, scope to deterministic metadata** (identity, scope, recency); no live scoring. |
| #639 fuse admitted semantic recall into `CodeLocator` | **Question for alpha** — runtime probabilistic fusion; defer behind the boundary-redefiner. |
| #640 render recall caveats across surfaces | **Question for alpha** — caveat surface is the disqualified pattern. |
| #641 real-provider selection | **Stays blocked** pending Jin/Silong notes. |

## Open items still owned by humans (not closed by this note)

- @jinhongkuan and @silongtan each provide direct engineering notes from inspecting/running CodeGenome + UOR (AC requirement).
- Confirm the recommended deterministic overlay set against a representative Bicameral fixture corpus.
- Decide whether the `uor-addr` spike is funded for alpha or deferred.
- If C is accepted, the deterministic-recall build and the offline boundary-redefiner harness become separate implementation-ready issues.

## See also

- `docs/specs/codegenome-authority-cleanup-221.md` · `docs/specs/mythologiq-librarian-kg-assessment.md` · `docs/specs/graph-provider-abstraction.md`
- `docs/adr/0024-signal-variables-authority-gates-durable-state.md`
- `https://github.com/MythologIQ-Labs-LLC/CodeGenome` · `https://github.com/UOR-Foundation/uor-addr` · `https://github.com/UOR-Foundation/UOR-Framework`
