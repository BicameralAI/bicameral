# P2 State-Refinement Backlog

Parent: `docs/releases/alpha/state-machine-release-readiness.md` (planned)
Source issue: #363
P0 QA result: 36 pass / 0 fail / 2 inconclusive (#362, PR #365)
P1 QA result: all 8 alpha release loops proven (#364, PR #375)

## Purpose

This backlog catalogues deferred lifecycle states identified during alpha
release planning and QA. Each entry maps to an existing proven release loop,
records the state it would add, the invariant it must preserve, the smallest
QA loop that would prove it, and why it does not ship in the current alpha.

The backlog is a planning artifact. It does not promise implementation order,
commit to release dates, or change canonical domain vocabulary.

## Backlog Table

| # | Seed state | Refines loop | New state added | Invariant preserved | Smallest QA loop | Why not alpha | Minimization note |
|---|---|---|---|---|---|---|---|
| 1 | ChannelRoute | Setup and channel boundary | `route_configured` — durable per-project routing config linking a Channel to an ingestion policy | Channels exist as evidence paths; routing hints remain display/advisory inputs and do not grant governance authority | Configure a ChannelRoute, verify ingestion respects it, verify removal stops routed evidence | No product owner workflow defined; no storage, authority, recovery, or review semantics specified (see `product-surfaces-and-integrations.md` §Deferred Vocabulary) | Single state (`route_configured`) with a boolean active flag; reject sub-states for route versions, route history, or route conflict until the owner workflow exists |
| 2 | Source refresh / repair | Advisory preflight | `source_refresh_pending` — Source whose latest SourceSnapshot is stale relative to governance freshness scope | Source Refresh is a lifecycle verb (`decision-lifecycle.md`); refreshing a Source does not create, promote, or demote a Decision | Trigger refresh on a tracked Source, verify new SourceSnapshot is captured, verify stale evidence is flagged in preflight context | Alpha advisory preflight has thin release-build coverage (1 scenario); Source Refresh requires provider access and recurring scheduling not yet implemented | One state per Source (fresh / stale); reject per-Decision or per-Evidence granularity; staleness is a Source property, not a Decision property |
| 3 | Provider join | Setup and channel boundary | `provider_join_pending` — operator has initiated provider OAuth but project setup has not completed | Rendezvous resolves bootstrap pointers only; successful OAuth does not join a project (`rendezvous-state-machine.md`) | Start provider join, interrupt mid-OAuth, verify project is not joined, complete OAuth, verify join completes | Alpha ships create-workspace path only; join flow requires rendezvous service and invite exchange not yet built | One pending state per provider per operator; reject nested sub-states for partial OAuth, token refresh during join, or multi-provider simultaneous join |
| 4 | Recurring integration ingestion | External ingest guardrail | `ingestion_active` — SourceBinding with a configured recurring fetch schedule | External ingest guardrail rejects unauthorized material; recurring ingestion must also respect the guardrail on every fetch cycle | Configure a SourceBinding with schedule, verify periodic fetch produces new SourceSnapshots, verify guardrail rejects tampered fetch | Alpha external ingest is submit-only (single-shot); recurring fetch requires connection registry, schedule persistence, and provider token refresh | One state per SourceBinding (active / paused / failed); reject per-fetch or per-cycle sub-states; failure is a SourceBinding property with retry policy |
| 5 | Token telemetry | Local capture | `telemetry_captured` — TokenSpendEvent recorded for an LLM call that consumed or produced Bicameral payloads | Token telemetry is observation, never governance; it must not create Decisions, approve signoff, resolve compliance, or block workflows (`token-telemetry-spend-events.md` §Purpose) | Trigger an LLM call via MCP surface, verify TokenSpendEvent is emitted with correct fields, verify no governance side-effects | Spec is proposed with zero implementation code; token telemetry is post-alpha priority #1 (`ship-plan-2026-06-14.md`) | One event type (`TokenSpendEvent`) with surface discriminator; reject per-model or per-provider sub-types; dollar projection is derived, not stored |
| 6 | Compliance resolution | Decision signoff | `compliance_resolved` — Decision whose BindingEvidence has been evaluated against current GraphSnapshot and a ComplianceState verdict issued | Compliance is the technical reality axis; it is separate from signoff (ownership axis) and does not block signoff in V1 | Bind a Decision, advance the GraphSnapshot, run compliance check, verify verdict is stored, verify signoff state is independent | Alpha does not ship `review.resolve_compliance` or `tracking.*`; compliance requires graph revalidation pipeline not yet built | One verdict per Decision per GraphSnapshot (compliant / drifted / ungrounded); reject per-binding or per-file granularity until graph pipeline supports it |

## Incorporation of P0/P1 QA Findings

### From P0 (#362) — 36 pass / 0 fail / 2 inconclusive

- **2 inconclusive:** Content-level scanners (out of alpha scope). This does not
  affect any seed state above — content scanning is an enforcement concern
  orthogonal to the deferred lifecycle states.
- **External ingest guardrail (8/10 pass):** Structural authority injection
  guardrails work. Recurring integration ingestion (seed #4) should inherit
  this structural guardrail as its baseline invariant.

### From P1 (#364) — all 8 loops proven

- **Binding guardrail gap closed:** P1 proved binding create/reject/inspect
  against release binary. Compliance resolution (seed #6) builds on this
  proven binding foundation.
- **Advisory preflight thin coverage:** Only 1 release-build scenario vs 21
  deterministic tests. Source refresh (seed #2) would deepen preflight
  coverage by providing real staleness signals.
- **Firefighting notes incorporated:**
  - Seed #5 (token telemetry): Aligns with "do not promise content-level
    injection detection" — telemetry is observation only.
  - Seed #6 (compliance): Aligns with "do not promise verified code grounding
    out of the box" — requires tree-sitter indexing configuration.
  - Seed #3 (provider join): Aligns with "do not promise real-time integration
    sync" — provider connections vary.

## Rejected Seed Candidates

None of the six seed states is rejected. Each maps to an existing proven
release loop and represents a genuine lifecycle refinement rather than
parallel vocabulary.

## State Explosion Prevention

Design principles applied to every entry:

1. **One state per entity** — each seed adds exactly one new named state to
   one entity type (Source, SourceBinding, Decision, Connection, Channel, or
   TokenSpendEvent). No combinatorial cross-product.
2. **No nested sub-states** — each minimization note explicitly rejects
   granularity that would create state trees within the new state.
3. **Refine, don't fork** — each seed refines an existing proven loop rather
   than creating a parallel lifecycle vocabulary.
4. **Defer derived states** — dollar projection (telemetry), route history
   (ChannelRoute), per-cycle tracking (ingestion), and cross-team scope
   (compliance) are explicitly deferred as derived concerns.
5. **Single proof loop** — each seed can be validated by one focused QA
   scenario that exercises the new state without requiring the full alpha
   matrix.
