# V1 Local Runtime & Ledger View Consolidation (#255)

**Status:** proposed — consolidation spec (no new architecture; ties together decisions already made)
**Issue:** [#255](https://github.com/BicameralAI/bicameral-bot/issues/255) "Define V1 local runtime, Ledger View, and dashboard ownership"
**Owner:** @Knapp-Kevin · **Secondary reviewer:** @jinhongkuan
**Field-role authority:** **ADR-0024** "Signal Variables, Authority Gates, and Durable State" — *(this is the model the #255 brief calls the "ADR-0018 field-role model"; it was renumbered from 0018 to 0024 in PR #518 to resolve a collision. ADR-0018 is the unrelated clean-projection contract. Use **0024**.)*

## Purpose & non-goal

Produce one map that ties the V1 local runtime together by **consolidating existing decisions**. This spec **introduces no new architecture** — every section points to an existing ADR, spec, or RFQ. Where those sources disagree, this spec records the reconciliation, not a new design.

## 1. Runtime ownership model

Canonical source: `docs/specs/bot-mcp-data-flow-runtime-architecture.md` (Runtime Invariant, Components, Canonical vs Derived, Forbidden Flows). ADRs: ADR-0002 (agent surfaces), ADR-0001 (local daemon/gateway/event-store substrates), ADR-0007 (substrate-neutral governance), ADR-0024 (field roles).

> **Invariant:** MCP may observe, propose, format, and relay. Bot validates, accepts, materializes, and projects. Integrations acquire. (Tracks 1–3.)

| Layer | Owns | Must not own |
|---|---|---|
| `bicameral-integrations` | provider acquisition, polling, webhook capture, screened provider facts | durable state; any actor/review/policy authority |
| `bicameral-bot` (daemon/gateway/runtime) | `ToolRequest` dispatch, validation/policy, event store, materialization, projection, graph/binding/preflight services | caller-local session authority beyond supplied `AuthorityContext` |
| `bicameral-mcp` | thin local surface: tool mapping, `AuthorityContext` assembly, reads bot state, relays governed commands | ledger, graph, binding, adapter, governance, event-store authority |
| CodeGenome (`CODEGENOME` repo + signal modules) | code-memory substrate, graph identity, overlay confidence, signal propagation, drift *classification*, continuity *matching* | verdicts, compliance, binding materialization (Track 3 / #221) |

## 2. Event-store contract

Canonical source: ADR-0001, ADR-0005 (separate event store / ingestion / review / policy), ADR-0007, RFQ 2 (`docs/plans/mcp-refactor-reassignment.md`). Substrates: Git, Drive (`crates/bicameral-runtime/src/event_store_adapters/drive_folder.rs`), local folder.

- The event store is the **only** source of accepted governance facts. Accepted events replay into materialized state.
- Receipts, caches, dashboard state, and projections are **derived/disposable** — never canonical (ADR-0010 §Graph Facts vs Governance Facts; `v1-reviewable-context-projection.md` §Authority Boundary).
- Substrate differences (Git PR-native review, Drive basic-store) are capability differences, not evidence-semantics differences (ADR-0010 §Substrate Neutrality).
- Drive appears in **two distinct roles** — do not conflate: (a) *event-store substrate* for ledger persistence (RFQ 5, this section), and (b) *source ingest* which is retired from MCP and moved behind the gateway (Track 2 / #218). This spec's runtime owns (a); (b) is `ExternalIngestEnvelope`.

## 3. Gateway & ingest boundary

Canonical source: `docs/specs/external-adapter-ingest-gateway.md` (#264, implemented), `docs/specs/source-adapter-gateway-migration-218.md` (Track 2), ADR-0016 (guardrails), ADR-0002 (envelope separation).

- **External provider ingest** → `POST /api/v1/external-ingest` (`ExternalIngestEnvelope`): authority-injection rejected (403), advisory candidates only, content-addressed `SourceSnapshot`/`SourceEvidence`, `EventType::ExternalIngest`. **19/19 gateway tests pass.**
- **Local actor ingest** → `POST /api/v1/ingest` (`IngestRequest`, MCP v0.2 `bicameral.ingest`): the local/MCP-actor path (RFQ 3 `ingest.submit_local`), distinct trust tier. *Integrations must target the external path, not this one (Track 2 contract decision).*
- Neither path creates canonical Decisions — only advisory candidates entering the review pipeline.

## 4. Ledger View read model

Canonical source: `docs/specs/v1-reviewable-context-projection.md`, ADR-0006 (ingestion gate + Ledger-View UI), ADR-0022 (V1 decision signoff & reviewable context), `enforcement-memory-projection.md`.

> **Ledger View rule (this issue): Ledger View explains. Ledger View does not become authority.**

The projection is rebuildable from accepted events + current source/graph indexes. If a fact cannot be reconstructed that way, it is not a canonical V1 fact. Deleting/rebuilding the projection must not change authority.

### Read-model fields classified by ADR-0024 role

| Ledger View field | ADR-0024 role | Why |
|---|---|---|
| Decision identity, `BindingEvidence`, signoff/review state | `identity_anchor` / accepted-gate result (`read_model_state` of an authority outcome) | replayed from accepted events |
| Tracked context | `read_model_state` | projection of governed decisions/evidence |
| Untracked context | `signal_output` | advisory — what is *not* yet governed |
| Freshness warnings | `signal_output` | derived from source currentness |
| Source readiness | `signal_output` | not a gate |
| Graph readiness | `signal_output` | ADR-0010 `GraphReadiness`; view precondition, not authority |
| Binding currentness | `signal_output` | ADR-0017 derived currentness; does not mutate accepted `BindingEvidence` |
| Preflight results | `read_model_state` + `signal_output` | advisory relevance/readiness |
| Allowed next actions | `signal_output` (`action_eligibility_reasons`) | explains options; does not perform them |

### Ledger View must not imply (negative invariants)
- canonical dashboard state (the dashboard is `read_model_state`, never the source of truth);
- V1 compliance enforcement (V1 is advisory — `bot-mcp-data-flow-runtime-architecture.md` Non-Goals);
- signoff completion (only an accepted signoff event);
- accepted binding without `BindingEvidence` (Track 1 verified-only gate);
- project truth from local diagnostics (a `signal_output` is not a governance fact).

## 5. Local workspace setup

Canonical source: ADR-0023 (installation & setup state machine), ADR-0013 (release-artifact install), `docs/specs/projection-local-setup.md`, `docs/specs/project-setup-ui.md`.

Golden path: install/start local runtime → pair/connect workspace → ingest a source → produce advisory grounding → read it in Ledger View → governance boundary preserved throughout. Setup surfaces runtime state without implying enforcement.

## 6. MCP migration boundaries

Canonical source: `docs/plans/mcp-refactor-reassignment.md` (RFQ 1–17), `docs/specs/bot-mcp-thin-client-migration-notes.md`. Each RFQ is `move`/`replace`/`retire`. MCP pre-refactor pointer: `0827444c80d45fe3474f68002166e1fc35708eda`.

| RFQ | Concern | Owner repo target | Status (this track set) |
|---|---|---|---|
| 1 | ToolRequest dispatch spine | bot | done (#114) |
| 2 | Ledger persistence/replay/history | bot | event store + replay present |
| 3 | `ingest.submit_local` | bot | local ingest path present (§3) |
| 4 | Source adapters & Drive runtime | acquire→integrations, materialize→bot, MCP retire | **Track 2 / #218 spec; MCP issue bicameral-mcp#623** |
| 5 | Team sync / event replication | bot substrate | open (Drive-substrate question) |
| 6–8 | Graph snapshot, validate, **binding** | bot | **Track 1 / #116 implemented; verified-only** |
| 9 | Preflight | bot | preflight surfaces present |
| 10 | **CodeGenome identity/drift** | keep substrate, retire MCP authority | **Track 3 / #221 spec; boundary vs RFQ 11 drawn** |
| 11 | Review: signoff & compliance | bot `review.*` | owns compliance command (boundary w/ #221) |
| 12 | Collision/context review | bot | open |
| 13 | Erasure/removal | bot/decide | open |
| 14 | Dashboard/Ledger View | bot | **this spec §4** |
| 15–16B | Setup/CLI/skills/telemetry | bot/retire | open |
| 17 | MCP thin-client strip + compat tests | MCP | terminal step |

## 7. Free/local vs hosted/team split

Canonical source: ADR-0010 §Free-Local / §Hosted-Team boundaries, ADR-0015 (reasoning-provider boundary & hosted-artifact exception).

- **Free/local:** exact grounding for locally resolvable snapshots, validate/bind/preflight against a known local snapshot, local drift checks, base-vs-HEAD when base+merge-base are local, explicit unknown states.
- **Hosted/team:** active-PR/cross-branch indexing, conflict detection across developers' branches, CI merge-gate, team-wide stale detection, Conflict Oracle, org-scale dependency intelligence.
- Hosted output remains evidence/advisory/`GovernanceResult` proposals; it does **not** become durable Decision authority unless bot governance materializes it.

## 8. Implementation sequence (consolidated)

Follows `mcp-refactor-reassignment.md` §Suggested RFQ Order, reconciled with Tracks 1–3:

1. RFQ 1 dispatch spine → 2. RFQ 2 ledger/replay → 3. RFQs 6–8 graph/validate/**binding** (Track 1 done) → 4. RFQ 9 preflight → 5. RFQ 3 local ingest → 6. RFQs 11–12 review/collision → 7. RFQs 4–5 source adapters/team-sync **with repo-target decisions** (Track 2) → 8. RFQ 10 CodeGenome retire (Track 3, after RFQ 11 boundary) → 9. RFQs 13–16B removal/dashboard/setup/CLI/telemetry (dashboard = §4) → 10. RFQ 17 MCP thin-client strip.

## 9. Negative authority-path tests (consolidated)

The V1 runtime must prove these are rejected, not silently accepted:

1. External ingest carrying any authority/review/signoff/compliance/policy field → 403 (`external_ingest_gateway` tests, present).
2. Non-verified graph evidence (`approximate`/`ambiguous`/`stale`/`not_indexed`/`unsupported`) → cannot materialize `BindingEvidence` (`allows_binding()`, Track 1).
3. A confidence/drift signal (e.g. `cosmetic`, `confidence≥0.80`) → does **not** by itself produce a compliance verdict (Track 3 / #221).
4. Approximate continuity match → cannot flip a binding without `binding.create` revalidation (Track 1/Track 3).
5. Rebuilding the SurrealDB projection → does not change canonical authority (`v1-reviewable-context-projection.md`).
6. Ledger View / dashboard read → never asserts signoff completion, compliance enforcement, or accepted binding without a replayed accepted event.
7. MCP → cannot write event-store/ledger/graph/binding authority (thin-client boundary).
8. Hosted/team advisory output → cannot become durable Decision authority without bot materialization (ADR-0015).

## Exit criteria (#255)

- [x] Single V1 runtime spec exists (this document).
- [x] All major RFQs mapped into it (§6).
- [x] Dashboard/read-model fields classified by ADR-0024 roles (§4).
- [x] Implementation sequence documented (§8).
- [x] Negative authority-path tests listed (§9).

## See also

- Specs: `bot-mcp-data-flow-runtime-architecture.md`, `v1-reviewable-context-projection.md`, `external-adapter-ingest-gateway.md`, `source-adapter-gateway-migration-218.md`, `codegenome-authority-cleanup-221.md`, `bot-mcp-thin-client-migration-notes.md`, `enforcement-memory-projection.md`, `projection-local-setup.md`, `project-setup-ui.md`
- ADRs: 0001, 0002, 0005, 0006, 0007, 0010, 0013, 0015, 0016, 0017, 0019, 0022, 0023, **0024**
- Plan: `mcp-refactor-reassignment.md` (RFQ 1–17)
