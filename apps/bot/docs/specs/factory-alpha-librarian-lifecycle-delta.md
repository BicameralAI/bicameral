# Factory Alpha Librarian Lifecycle Delta

Date: 2026-06-19
Status: requirements delta, not implementation plan
Source material: design-partner setup notes, `docs/specs/librarian-recall-readme.md`, an internal asset assessment (not published), four asset-specific `$bic:to-runtime-spec` passes for CODEGENOME, FailSafe, EvolveAI, and qortara-governance, factory product-validation roadmap, cognitive-debt alpha posture docs, and bot alpha release readiness docs.

## Purpose

This document describes how factory alpha posture and lifecycle roadmaps should change so the "librarian" idea can land as a constrained runtime shape:

- recall the relevant accepted decisions, constraints, evidence, and known unknowns before work starts;
- expose contradiction and correction candidates through human review;
- support MCP, CLI, dashboard, and integration surfaces with the same authority model;
- allow a small set of MythologIQ-derived concepts only as non-authoritative projections, packets, receipts, and advisory metadata.

The goal is requirements, not implementation. The roadmap should make unsafe implementations structurally impossible, especially any accidental version of "autopoietic memory", canonical graph reality, confidence-authorized mutation, or a second policy engine.

## Central Invariant

The librarian is a corpus-scoped recall and review layer. It is not a self-improving memory system, enforcement gate, compliance engine, risk platform, graph authority, or dashboard product.

The trusted corpus is ledger-governed:

```text
Decision Ledger = authority spine
Trusted Corpus = ledger-governed evidence surface
```

The Decision Ledger owns accepted authority. Product docs, feature definitions,
ODRs/ADRs, retained source evidence, accepted BindingEvidence, and
code-grounding references may be searchable corpus evidence without being
Decisions themselves. Code grounding supports applicability, currentness, and
recall; it does not update product truth by itself.

At runtime:

1. A request enters through MCP, CLI, dashboard, agent, or an external source adapter.
2. The Bot owns the boundary check and returns an explicit dispatch or ingest outcome.
3. Read-only librarian preflight returns a recall packet with scope, matched decisions, evidence, relevance reasons, freshness/readiness metadata, and unknowns.
4. Contradictions, assumptions, missing constraints, and failure patterns become candidates or review packets only through existing governed review paths.
5. Accepted Decisions, signoff, BindingEvidence, compliance states, and event-store materializations remain the only canonical decision memory.

## Roadmap Shape Change

The existing lifecycle roadmap should gain a cross-cutting "Librarian Recall Contract" that constrains several roadmap branches without becoming a new implementation stack.

### New Cross-Cutting Contract

Add this contract to the factory roadmap as a requirements layer:

> Before an agent or human makes a product or technical change, Bicameral can return a corpus-scoped `RecallPacket` that states what corpus was searched, what accepted Decisions and source-backed candidates matched, why they matched, what evidence supports them, what freshness/readiness applies, what scope is unknown, and what human actions are allowed next.

This contract should sit across Decision Retrieval, Constraint Lookup, Evidence Quality Review, Advisory Code Grounding, Constraint Correction Capture, EM Review Surface, Tool-Agnostic, and Decision Memory Core.

It should not be represented as a new canonical Decision lifecycle.

### Branch-Level Deltas

**Decision Retrieval**

Change from "find relevant decisions" to "return a reproducible recall packet with corpus/version proof." Retrieval is incomplete without a searched-scope statement, source freshness/readiness, evidence references, and explicit unknown scope.

Required additions:

- `RecallPacket` result shape.
- `coverage_proof` or `searched_scope`.
- `unknown_scope` when the corpus, connector, index, or source freshness is incomplete.
- Replayable relevance reasons.
- Negative acceptance: "no relevant decision found" must never imply global completeness.

**Constraint Lookup**

Constraint lookup should become the primary librarian preflight call for design-partner-style use: "pull the relevant prior decisions and matching feature docs before work starts."

Required additions:

- Constraint lookup returns accepted Decisions, source-backed candidates, matching feature docs, known contradictory snippets, and review-needed items in one packet.
- Lookup remains read-only by default.
- Low-signal approximate matches can be suppressed or logged, but high-risk uncertainty must be shown as review-needed.
- Agent copy must distinguish accepted constraints, source-only evidence, stale constraints, and unsupported scope.

**Evidence Quality Review**

Evidence quality should own freshness, provenance, and ranking metadata as advisory review inputs. It must not become autonomous memory maintenance.

Required additions:

- `freshness_state`, `freshness_reason`, `last_verified_at`, `source_unavailable`, `recall_salience`, `ranking_reason`, and `review_priority` as derived metadata.
- Failure-pattern hints as candidates for review, not blockers.
- Negative fixtures proving stale or low-ranked accepted Decisions still appear when relevant.

**Constraint Correction Capture**

Correction capture should accept contradiction candidates from Linear, Slack, PR comments, MCP sessions, and recall misses.

Required additions:

- `ContradictionCandidate`.
- `FailurePatternCandidate`.
- `IntentPreview`.
- Product-owner review action requirements.
- Negative acceptance: contradiction candidates do not update the source of truth until accepted through governed review.

**Advisory Code Grounding**

CODEGENOME-like graph ideas may be used only as derived catalogue projections and affected-surface hints. They must stay under existing GraphSnapshot, BindingEvidence, and event-store authority.

Required additions:

- Catalogue node identity contract.
- Edge provenance contract.
- Projection readiness states.
- Overlay compatibility matrix for allowed graph-derived signals.
- Negative acceptance: graph confidence, neighborhood, or impact score cannot produce "safe", "no conflict", "compliant", "signoff", "binding accepted", or "complete recall" claims.

**Advisory Constraint Lookup**

FailSafe-style packets and replay receipts can improve UX, but the advisory lane remains informative and non-blocking.

Required additions:

- Packet/replay receipt support for "why did the agent see this?"
- Intent preview before capture.
- Existing inline review modes only: ask, recommend confirm, route to surface, force human review, advisory only.
- Negative acceptance: replay receipts and packets do not become approval, acceptance, or canonical mutation.

**EM Review Surface**

Narrow this from dashboard expansion to review packets. The alpha surface should help product owners triage recall, contradiction, and correction packets, not create a large command-center product.

Required additions:

- `ReviewPacket` contract.
- Product-owner actions mapped to existing review commands.
- No risk register, L3 approval queue, trust score, workspace taxonomy, or program tracker.

**Tool-Agnostic**

Tool-Agnostic should own schema parity for MCP, CLI, dashboard, and integrations.

Required additions:

- Shared `RecallPacket`, `IntentPreview`, `ReviewPacket`, `DispatchOutcome`, and `ExternalIngestEnvelope` protocol fields.
- Receipt parity across local tool calls and external ingest.
- Negative acceptance: private surface-only commands cannot bypass Bot governance.

**Decision Memory Core**

Decision Memory Core should remain the canonical ledger and retrieval contract. It should explicitly reject memory systems that self-update, self-rank into authority, or synthesize accepted constraints.

Required additions:

- "Confidence is not authority" rule.
- "Freshness is not deletion or demotion" rule.
- "Failure patterns are review inputs, not blockers" rule.
- "Catalogue projections are derived views, not source of truth" rule.

## Lifecycle State Changes

No new canonical Decision lifecycle states should be added for the librarian. The minimum honest release state is a set of derived read models, receipts, and metadata.

### Allowed Derived States And Metadata

- `recall_packet`
- `review_packet`
- `intent_preview`
- `preflight_replay_receipt`
- `catalogue_projection_ready`
- `catalogue_projection_partial`
- `catalogue_projection_stale`
- `freshness_state`
- `freshness_reason`
- `recall_salience`
- `ranking_reason`
- `failure_pattern_hint`
- `review_priority`
- `unknown_scope`
- `dispatch_denied`
- `dispatch_routed`
- `dispatch_deferred`

These states can explain what was searched, displayed, routed, or deferred. They cannot create, accept, supersede, demote, delete, hide, or sign off a Decision.

### Explicitly Deferred Or Rejected States

The roadmap should name these as out of scope so they are not accidentally implemented:

- `autopoietic_memory`
- `autonomous_memory`
- `self_evolved_constraint`
- `canonical_graph_reality`
- `belief_accepted`
- `confidence_authorized_mutation`
- `memory_tier`
- `crystallized_memory`
- `auto_decay_demoted`
- `shadow_blocked`
- `risk_record`
- `agent_trust_score`
- `l3_approval`
- `governance_mode`
- `program_tracker`
- `hosted_policy_decision_service`
- `second_policy_engine`
- `blocking_librarian_gate`
- `global_no_conflict`
- `global_safe_to_change`
- `compliance_from_recall`

## MythologIQ Asset Adaptation Boundaries

### CODEGENOME

Relevant shape:

- content-addressed catalogue identity;
- graph overlays;
- affected-surface hints;
- provenance on every node and edge;
- confidence as display metadata.

Allowed alpha use:

- derived catalogue projection over existing Decisions, source snapshots, evidence, bindings, and graph snapshots;
- relevance and affected-surface hints in recall packets;
- edge provenance and projection readiness.

Forbidden:

- canonical graph reality;
- replacing `GraphSnapshot` identity or BindingEvidence authority;
- confidence-authorized mutation;
- auto-promotion of beliefs into Decisions;
- graph-derived compliance, signoff, or "no conflict" claims.

### FailSafe

Relevant shape:

- compact packets;
- replayable explanations;
- intent preview;
- tracker-like freshness cues.

Allowed alpha use:

- `RecallPacket`;
- `ReviewPacket`;
- `IntentPreview`;
- `PreflightReplayReceipt`;
- contradiction packets.

Forbidden:

- Command Center clone;
- IDE save-time governance;
- L3 approval queue;
- risk register;
- agent trust score;
- governance modes;
- dashboard-first expansion;
- auto-ratification.

### EvolveAI

Relevant shape:

- freshness display;
- recall salience;
- failure-pattern advisory metadata;
- temporal context as a derived read model.

Allowed alpha use:

- freshness and ranking metadata;
- review-priority hints;
- failure-pattern candidates;
- negative tests proving stale accepted Decisions still appear.

Forbidden:

- autopoietic memory;
- autonomous decay, demotion, supersession, deletion, or hiding;
- memory tiers as authority;
- self-restructuring graph edges;
- REM-style synthesis into accepted memory;
- hidden authority from embeddings, repeated use, or ranking weights.

### qortara-governance

Relevant shape:

- boundary check at dispatch time;
- typed allow, deny, route, unsupported, and defer outcomes;
- receipts explaining why the Bot routed or denied a request.

Allowed alpha use:

- Bot-owned `ToolRequest` boundary for MCP, CLI, dashboard, and agent calls;
- Bot-owned `ExternalIngestEnvelope` boundary for Slack, Linear, PR comments, and other source systems;
- typed receipts with no canonical mutation;
- dispatch outcomes used in replay packets.

Forbidden:

- qortara-governance as runtime dependency or policy authority;
- AGT/LangChain/LangGraph policy adapters as Bicameral authority;
- second policy decision point;
- hosted cumulative risk layer;
- adaptive policy mutation;
- receipt-as-Decision, receipt-as-signoff, receipt-as-compliance, or receipt-as-BindingEvidence.

## Factory Document Patch Targets

The following factory docs should be updated after this delta is accepted:

- `knowledge/product-validation-roadmap.md`
- `product-lines/cognitive-debt-reduction/alpha-pilot-offer.md`
- `product-lines/cognitive-debt-reduction/value-ladder.md`
- `product-lines/cognitive-debt-reduction/decision-routing-policy.md`
- `product-lines/cognitive-debt-reduction/oss-vs-premium-boundary.md`

Recommended factory edits:

1. Add "Librarian Recall Contract" as a cross-cutting requirements layer, not a new product line.
2. Add packet, catalogue projection, freshness metadata, contradiction candidate, and boundary receipt requirements to the relevant roadmap branches.
3. Add the rejected-state list above to the roadmap or alpha posture appendix.
4. Reword alpha from "decision memory dashboard plus preflight" toward "corpus-scoped librarian recall packets plus human-reviewed correction."
5. Keep paid/hosted expansion focused on broader corpus coverage, managed connector reliability, policy customization, and rollout support, not stronger autonomous authority.

## Bot Alpha Document Patch Targets

The following bot docs should be updated after the factory delta is accepted:

- `docs/releases/alpha/state-machine-release-readiness.md`
- `docs/releases/alpha/state-refinement-backlog.md`
- `docs/releases/alpha/desync-ux-hitl-readiness.md`
- `docs/releases/alpha/proof-gap-coverage.md`
- `docs/specs/bot-mcp-data-flow-runtime-architecture.md`
- `docs/specs/external-adapter-ingest-gateway.md`
- future `docs/specs/librarian-recall-runtime-architecture.md`

Recommended bot edits:

1. Add "Librarian Recall Packet" as an alpha QA loop:
   request -> boundary check -> read-only preflight -> packet returned -> no canonical mutation.
2. Add derived state and metadata names from this document to state refinement backlog.
3. Add negative readiness checks for self-updating memory, confidence authority, graph authority, and second policy engines.
4. Add MCP parity requirements for `RecallPacket`, `DispatchOutcome`, and `ExternalIngestEnvelope`.
5. Add copy constraints for "unknown", "source-only", "stale", "review-needed", "routed", and "denied".

## Alpha Deliverables For RFQs

### RFQ 1: Librarian Recall Packet Contract

Objective: define the shared response contract for session-start recall over MCP, CLI, dashboard, and agent surfaces.

Inputs:

- accepted Decisions and their evidence;
- source-backed candidates;
- feature docs and private notes;
- catalogue projection status;
- source freshness metadata;
- dispatch receipt if a boundary decision was involved.

Required fields:

- `packet_id`
- `corpus_id`
- `corpus_version`
- `request`
- `searched_scope`
- `coverage_proof`
- `matches`
- `evidence_refs`
- `relevance_reasons`
- `freshness_state`
- `ranking_reason`
- `unknown_scope`
- `allowed_next_actions`
- `receipt_ref`

Acceptance criteria:

- returns accepted Decisions and source-only candidates distinctly;
- explains why each item matched;
- states unknown or unsupported scope explicitly;
- returns no canonical event by default;
- has fixtures for exact match, partial corpus, stale source, no relevant match, unsupported connector, and contradictory source snippet.

Open decisions:

- minimum `coverage_proof` shape for Git-first customers;
- whether relevance uses deterministic reasons only or permits embedding-assisted ranking with replay metadata;
- exact copy for "no relevant decision found" without overclaiming completeness.

### RFQ 2: Catalogue Projection And Provenance Contract

Objective: define the CODEGENOME-inspired catalogue projection without granting graph authority.

Required fields:

- catalogue node identity;
- source object type;
- source object ID;
- source snapshot or commit;
- edge type;
- edge evidence;
- confidence metadata;
- projection readiness.

Acceptance criteria:

- projection identity does not replace `GraphSnapshot` identity;
- every edge has provenance;
- confidence cannot authorize mutation, acceptance, signoff, binding, compliance, or no-conflict claims;
- fixtures prove derived graph edges can be absent, stale, or partial without changing canonical Decision state.

Open decisions:

- whether catalogue node IDs are content-addressed, path-addressed, or hybrid;
- allowed overlay edge types for alpha;
- whether affected-surface hints ship in alpha or remain later-stage.

### RFQ 3: Boundary Receipt And DispatchOutcome Contract

Objective: define the qortara-style Bot-owned boundary outcome for local tool calls and external ingest.

Required outcomes:

- `allowed`
- `denied`
- `routed`
- `unsupported`
- `deferred`

Acceptance criteria:

- all MCP, CLI, dashboard, and agent tool calls pass through `ToolRequest`;
- all Slack, Linear, PR, and source-system payloads pass through `ExternalIngestEnvelope`;
- receipts contain bounded reason, actor or source surface, command or envelope kind, policy version if available, and no secret echo;
- receipts do not create Decisions, candidates, signoff, BindingEvidence, compliance, or event-store mutation.

Open decisions:

- whether receipts are operational traces only or a future narrow audit-event class;
- reason enum shared across tool requests and external ingest;
- commands requiring `force_human_review` in alpha.

### RFQ 4: Freshness, Salience, And Failure Pattern Metadata

Objective: define EvolveAI-inspired metadata that helps recall without becoming autonomous memory.

Required metadata:

- `freshness_state`
- `freshness_reason`
- `last_verified_at`
- `last_seen_at`
- `source_unavailable`
- `recall_salience`
- `ranking_reason`
- `failure_pattern_hint`
- `review_priority`

Acceptance criteria:

- stale accepted Decisions still appear when relevant;
- low salience cannot hide accepted Decisions;
- failure pattern hints create only candidates or review prompts;
- metadata cannot emit governance events;
- no copy uses forgotten, decayed, self-updated, crystallized, or autonomous memory language.

Open decisions:

- freshness dimensions and thresholds;
- failure-pattern taxonomy;
- retention policy for recall misses and correction traces;
- ordering rule for stale highly relevant items versus fresh weakly relevant items.

### RFQ 5: Intent Preview And Contradiction Candidate Contract

Objective: define how Linear, Slack, PR comments, meeting notes, and MCP session statements become reviewable candidates without auto-updating the source of truth.

Required objects:

- `IntentPreview`
- `ContradictionCandidate`
- `CorrectionCandidate`
- `SourceEvidence`
- review action mapping.

Acceptance criteria:

- external phrasing is preserved as source evidence;
- nontechnical CEO phrasing can be flagged without becoming an accepted constraint;
- candidates route to product-owner review;
- no automatic source-of-truth update;
- fixtures include contradiction, ambiguity, unsupported scope, and low-signal noise suppression.

Open decisions:

- whether Linear/Slack previews create SourceEvidence only or can create DecisionCandidates immediately;
- who owns dismissal, promotion, and rewrite authority;
- minimum evidence required for contradiction candidate creation.

### RFQ 6: Review Packet And EM Review Surface Narrowing

Objective: constrain the alpha review surface to packet review, not dashboard sprawl.

Required packet types:

- recall packet review;
- contradiction packet review;
- correction packet review;
- stale or unsupported evidence packet;
- dispatch or ingest denial replay packet.

Acceptance criteria:

- actions map to existing review commands;
- product-owner role remains the default human authority;
- packets can be rendered in dashboard or MCP response without changing protocol semantics;
- no risk register, trust score, L3 queue, governance mode, or command-center surface.

Open decisions:

- minimum action set for alpha;
- whether review packets are persisted or recomputed;
- where packet history is visible.

### RFQ 7: Alpha QA Loop For Librarian Recall

Objective: add a release-readiness loop that proves the librarian posture without implementing a broad product surface.

Loop:

```text
request enters via MCP/local surface
-> Bot boundary check returns allowed/routed/denied/deferred
-> read-only preflight runs over configured corpus
-> RecallPacket returns matches, evidence, freshness, unknowns, and next actions
-> no canonical mutation unless a separate governed review command is accepted
```

Acceptance criteria:

- fixture for accepted decision recall;
- fixture for source-only match;
- fixture for partial corpus unknown;
- fixture for stale source;
- fixture for contradiction candidate;
- fixture for dispatch denied/routed;
- fixture proving packet return creates no event-store mutation;
- copy review for no global completeness, no enforcement, no compliance, no auto-update.

Open decisions:

- whether this loop belongs in the existing Advisory Preflight loop or becomes a named "Librarian Recall Packet" loop;
- minimum MCP response shape for alpha;
- required QA artifact format.

## Alpha Posture Change

Current alpha posture should shift from "decision memory dashboard plus advisory preflight" to:

> Bicameral alpha proves a controlled librarian loop: configured corpus in, read-only recall packet out, human-reviewed correction path available, and all authority boundaries explicit.

Alpha should prove:

- a configured source of truth can be searched with honest scope;
- relevant accepted Decisions and source-backed candidates can be recalled before work starts;
- stale, partial, source-only, unsupported, and contradictory results are clearly labeled;
- a human can promote, reject, or correct candidates through existing review paths;
- MCP/local calls and external ingest share the Bot-owned authority boundary;
- MythologIQ-derived graph, packet, freshness, and dispatch ideas remain derived and non-authoritative.

Alpha should not prove:

- global completeness;
- global no-conflict;
- blocking enforcement;
- compliance;
- self-updating decision memory;
- graph reality;
- autonomous decay or synthesis;
- second policy authority;
- dashboard command center;
- hosted risk layer.

## RFQ Sequencing

Recommended order:

1. RFQ 1: Librarian Recall Packet Contract.
2. RFQ 3: Boundary Receipt And DispatchOutcome Contract.
3. RFQ 2: Catalogue Projection And Provenance Contract.
4. RFQ 4: Freshness, Salience, And Failure Pattern Metadata.
5. RFQ 5: Intent Preview And Contradiction Candidate Contract.
6. RFQ 6: Review Packet And EM Review Surface Narrowing.
7. RFQ 7: Alpha QA Loop For Librarian Recall.

RFQs 1, 2, 3, and 4 need product/factory acceptance before implementation. RFQs 5, 6, and 7 can be refined after those contracts exist.

## Non-Negotiable Negative Tests

Every implementation slice derived from this delta should include negative acceptance for these cases:

- confidence cannot mutate or accept a Decision;
- graph projection cannot claim canonical truth;
- stale metadata cannot hide or demote an accepted Decision;
- failure-pattern hint cannot block work;
- recall packet return cannot create an event;
- denied or routed dispatch receipt cannot become evidence of product truth;
- external contradiction cannot update the source of truth without review;
- "no relevant decision found" cannot claim global completeness;
- MCP cannot bypass Bot governance;
- no object named or behaving like autopoietic memory can be introduced.
