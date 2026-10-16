# Bicameral

Bicameral captures reviewable claims and context from product, code, and collaboration evidence, then routes them through review into a durable event authority.

## Domain Model

Bicameral domain language is split by centrality so core decision-lifecycle
terms do not get diluted by every protocol, UI, or integration helper type:

- [Core Decision Lifecycle](docs/domain/decision-lifecycle.md) defines the
  essential entities that turn observations and evidence into governed
  Decisions.
- [Runtime Protocol Contracts](docs/domain/runtime-protocol.md) defines stable
  machine contracts used by local surfaces and clients to request, report, or
  render lifecycle state.
- [Product Surfaces and Integrations](docs/domain/product-surfaces-and-integrations.md)
  defines product-specific UI, connector, channel, and setup contracts that
  adapt Bicameral to particular operator workflows.

`CONTEXT.md` remains the repo-level map and terminology guardrail. The focused
domain files carry the heavier classification and neighboring-term guidance.

### Entities

`Decision`
: Reviewed claim or decision that has been promoted into the Decision Ledger.

`Decision Ledger`
: Replayable authority spine for accepted Decisions and accepted governance
  facts in a Product. The selected event store substrate stores the durable
  events; local databases, search indexes, dashboards, MCP views, and graph
  projections are materializations or caches.

`Trusted Corpus`
: Ledger-governed evidence surface that the librarian may search for recall.
  The Decision Ledger is its authority spine; linked source docs, ODRs/ADRs,
  feature definitions, source evidence, accepted BindingEvidence, and
  code-grounding references may be searchable corpus evidence without becoming
  Decisions themselves.

`Tracked Decision`
: Promoted Decision whose cited evidence remains within Bicameral's governance freshness scope. In V1, candidate promotion makes a Decision tracked by default; signoff is a separate ownership lifecycle.

`Product`
: Governance scope for Decisions, source evidence, review, and member participation.

`Product Owner`
: Accountable reviewer for a Product's Decision meaning and signoff.

`Product Member`
: A person who may contribute evidence, request review, or perform allowed member actions for a Product. Identified on the hosted plane by a Cloud account. (Formerly "Workspace Member"; renamed because "workspace" is retired as a collaboration term — that scope is the Product.)

`Cloud account`
: A person's hosted identity, used for hosted-bridge pairing and coordination. It links to a Local actor per daemon through local account-linkage validation; it is not itself local authority. A person may participate in multiple Products. Company affiliation, when present, is a non-canonical billing label on the Cloud account — Bicameral has no canonical Organization or Team entity, and no server-side, cross-Product membership store. A canonical company/org layer would be a deliberate reversal (see ADR-0025) requiring hosted identity and multi-tenant auth that are currently out of scope.

`Hosted cloud`
: Optional coordination and advisory layer that helps Cloud accounts discover, preview, pair, relay, and render. It is never canonical authority and never holds Confidential data: no Decision Ledger events, provider secrets, source bodies, local paths, or code indexes. Authority, secrets, and canonical state remain in the paired Local daemon.

`DecisionCandidate`
: Extracted claim awaiting governed promotion into the Decision Ledger.

`CandidateDraft`
: Caller-submitted draft claim inside an accepted local ingest request. It may materialize as a reviewable DecisionCandidate in proposed state, but it is not a tracked Decision, signoff, compliance result, or verified evidence.

`Source`
: Mutable observed surface identified by URI or stable key, such as a tracker issue, transcript, MCP session, command run, pull request, file, diff, or code snapshot.

`Tracked Source`
: A `Source` a person has placed in Bicameral's maintenance set before `Signoff`. Its current `GovernedDomain` is not approved. It is not a parked planning board.

`SourceSnapshot`
: Immutable captured view of a Source, identified by content address.

`GovernedDomain`
: Board for one `SourceSnapshot`. It holds one claim set: claims, atoms, claim-to-atom assertions with polarity, and the `AtomRelationV1` edges. Capturing the snapshot creates it empty. `atom_ids` is not stored separately. It is not a spec. Prompt bytes for a hosted extract are a Cloud-hosted version in their own Supabase table, fetched through the Cloud API. They are not compiled into the daemon and they are not a `product_directory` row. `source_digest` is absent until extract and then names the claim set that was written. The snapshot's `content_hash` is the identity of the bytes. A conflict with another snapshot is a `CheckRecord`, not one of these edges.

`SourceEvidence`
: Current concrete evidence pointer into one SourceSnapshot. It is a source-backed form of EvidenceReference.

`EvidenceReference`
: Citeable reference into a SourceSnapshot. It may point at text spans, transcript segments, tool calls, command output, file ranges, diff hunks, or code snapshot regions.

`BindingEvidence`
: Future-extension verified relationship between a Decision and a code-shaped evidence target at a specific GraphSnapshot.

`BindingProposal`
: Runtime or codegraph prevalidation output that carries verified graph facts for a requested binding before governance acceptance.

`BindingHint`
: Caller-supplied advisory link between a claim or candidate and a code-shaped target. It is not binding authority; only bot-validated graph or symbol evidence may materialize accepted BindingEvidence.

`Binding target kind`
: Product vocabulary for the kind of code-shaped target that BindingEvidence points at, such as file, symbol, or code region. It classifies the evidence target; it does not by itself prove the target is verified, current, or compliant.

`Binding Revalidation`
: Governed evaluation of whether accepted BindingEvidence still holds at a later GraphSnapshot.

`RevalidationOutcome`
: Accepted result of Binding Revalidation, such as confirmed, replaced, failed, or inconclusive.

`ReasoningArtifact`
: Immutable advisory output from a configured reasoning provider for one reasoning task and canonical request. It may support candidates, hints, summaries, and rationale, but it is not Decision Ledger authority.

`BindingLineage`
: Derived or governed relationship that connects BindingEvidence and Binding Revalidation observations for the same Decision target across GraphSnapshots.

`GraphSnapshot`
: Named code snapshot against which local or hosted graph evidence is validated. In git-backed workspaces this is anchored by repository identity, commit SHA, and graph index version; branch names are provenance, not durable identity.

`SymbolOccurrence`
: Symbol observed at a specific GraphSnapshot with enough path, range, content hash, and parser or resolver evidence to support reviewable grounding.

`CodeTargetIdentity`
: Continuity evidence used to relate code targets across GraphSnapshots when line ranges, file locations, or implementation structure change.

`GovernanceResult`
: Substrate-neutral outcome of governance or conflict analysis.

`ExternalIngestEnvelope`
: Evidence-submission contract for external source integrations such as Jira, Fathom, Slack, Notion, explicitly approved source bindings, or support inboxes. A provider's event-store role does not make that provider a source-ingest surface by default.

`ToolRequest`
: Action contract for local tool surfaces such as MCP, CLI, dashboard, or TUI.

`ToolRequest Dispatcher`
: Bot-owned runtime path that validates local ToolRequest authority, routes supported commands, and returns ToolResponse outcomes. Local surfaces such as MCP, CLI, dashboard, and TUI may call it; they must not bypass it with surface-owned canonical event writes.

`CapabilityReport`
: Daemon capability handshake response that tells local surfaces which ToolRequest protocol version and command capabilities the running daemon supports. It is compatibility evidence for startup and command availability, not governance authority.

`PROTOCOL_VERSION`
: Local ToolRequest wire compatibility version used by MCP and other surfaces to decide whether they can speak to the running daemon. A mismatch is a compatibility failure and should not be recovered by falling back to legacy MCP authority.

`Egress`
: Outbound publication, notification, sync, or annotation from Bicameral to an external system.

`CollisionCheck`
: Evaluation that compares a Decision against known evidence and active Decisions for contradictions. A clean result means no conflict is known within available evidence, not global proof that no conflict exists.

`ModManifest`
: Declarative configuration artifact for EM-safe evidence-producing mods.

`InviteBundle` *(legacy/internal — deprecated in favor of provider-based project discovery, issue #344)*
: Checksummed local workspace bootstrap payload that helps a teammate configure a matching Bicameral workspace substrate, governance policy hash, and integration manifest references. It is configuration evidence only; it does not grant authority, synchronize Decisions, or write the Decision Ledger. No longer presented as a product join mechanism.

`Pairing Notice` *(legacy/internal — deprecated, issue #344)*
: User-facing warning previously attached to local workspace pairing flows that explains an InviteBundle configures local access to a shared substrate but is not itself an authority store, governance grant, or synchronization mechanism.

`Workspace Config Validation`
: Local configuration safety gate that checks whether a BicameralConfig is safe and internally consistent before dashboard or gateway setup writes it. Implementations such as `BicameralConfig::validate()` enforce this gate, including loopback-only gateway hosts for unauthenticated local setup.

`Enforcement Path`
: Evaluation lens for asking whether Bicameral's architecture can make an accepted Decision affect constrained work. It is not itself a hotpath routing model.

### Lifecycle Verbs

`Promote`
: Governed transition that turns a DecisionCandidate into a Decision Ledger record.

`Demote`
: Governed Ledger View transition that lowers a Decision's authority without recreating it as a candidate.

`Refresh extraction`
: User-facing action that requests a new hosted ReasoningArtifact for the same extraction task despite an existing memoized artifact.

`Source Refresh`
: Recapture or re-index a Source so Bicameral can compare a newer SourceSnapshot or source-specific readiness state against previously cited evidence.

`Untrack Source`
: Withdrawal, by the person who tracked it, of a still-tracked `Source` from the maintenance set. The snapshot then has none of tracked, approved, rejected, or superseded.

`ReasoningArtifact replacement`
: Team-visible hosted transition that marks a refreshed ReasoningArtifact as current and supersedes the prior hosted artifact for that extraction task.

`Build projection`
: Deterministic construction of a ProjectionState from event-store replay. Produces the V1 read model without mutating canonical state.

`Rebuild projection`
: Full reconstruction of a ProjectionState from event-store replay, replacing stale or corrupted projection state.

## Relationships

`DecisionCandidate` cites `SourceEvidence` [0..*].

`Product` has `Product Owner` [1].

`Product` has `Product Member` [0..*].

`Decision` belongs to `Product` [1].

`Tracked Decision` is a `Decision` [1].

`DecisionCandidate` may reference `ReasoningArtifact` [0..*].

`Decision` cites `SourceEvidence` [1..*].

`DecisionCandidate` cites `EvidenceReference` [0..*].

`Decision` cites `EvidenceReference` [0..*].

`BindingProposal` may become `BindingEvidence` [0..1].

`Binding Revalidation` evaluates `BindingEvidence` [1].

`Binding Revalidation` produces `RevalidationOutcome` [1].

`CodeTargetIdentity` relates code targets across `GraphSnapshot`s [0..*].

`SourceEvidence` points into `SourceSnapshot` [1].

`SourceSnapshot` captures `Source` [1].

`Source` has `SourceSnapshot` [0..*], found by `source_uri`, not stored as an array on the `Source`.

`Tracked Source` is a `Source` [1].

`Tracked Source` has `GovernedDomain` [1] and `Signoff` [0].

`SourceSnapshot` carries `source_uri` [1], the same string as the `Source`'s `uri`. It is a back-pointer. The captured bytes are `content_hash`.

`SourceSnapshot` has `GovernedDomain` [1].

`CheckRecord` cites `GovernedDomain` [2].

`Promote` turns `DecisionCandidate` into `Decision` [0..1].

`Demote` targets `Decision` [1].

`Refresh extraction` creates `ReasoningArtifact` [1].

`ReasoningArtifact replacement` supersedes `ReasoningArtifact` [1].

`Source Refresh` targets `Source` [1].

`Source Refresh` may create `SourceSnapshot` [0..1].

`Untrack Source` targets `Tracked Source` [1].

`Untrack Source` records `UntrackReason` [1].

`ActionAuthorizationPolicy` refines `Governance policy` for command-level authorization [1].

## Supporting Terms

`Decision Ledger`
: Canonical materialized decision state derived by replaying the selected event store substrate.

`Ledger View`
: Human-facing surface for inspecting Decision Ledger state and emitting review commands.

`Governance policy`
: Rules that decide how candidates, review commands, and evidence route to review, advisory state, materialization, or enforcement.

`ActionAuthorizationPolicy`
: Bot-owned policy vocabulary that classifies governed actions by surface, actor authority, target state, required capability, and approval class before a canonical mutation or external effect may occur.

`EventStoreAdapter`
: Substrate-specific materialization boundary for accepted governance events.

`Signoff`
: The approved state of the `GovernedDomain` paired with one `SourceSnapshot`, stored with the check records that were required for the approval and the daemon that cranked the write. The actor is `manual` or `policy`. A `manual` signoff also records the person and their role, `owner` or `member`. A `policy` signoff has no role. It is not compliance, drift, or grounding. A newer snapshot's domain does not inherit it. A domain is approved only when it is not in conflict with another approved domain. Who may set it is the approve policy in ADR-0034.

`Status / compliance state`
: Future-extension code-compliance state for a Decision, computed or reviewed from grounding and drift evidence.

`GraphEvidenceState`
: Evidence state of a graph or symbol claim, such as verified, not found, unknown_not_indexed, unknown_stale, ambiguous, unsupported, or approximate candidate. Only verified graph claims can become BindingEvidence or support blocking governance results.

`PublicGraphEvidenceState`
: Five-state wire projection of GraphEvidenceState for external consumers and UI surfaces: verified, not_found, unknown, not_supported, and ambiguous. It preserves the binding authority boundary while hiding internal diagnostic subdivisions.

`Graph fact`
: Recomputable claim about code evidence at a GraphSnapshot.

`Governance fact`
: Accepted authority transition that affects Decision Ledger state, review state, compliance state, signoff, or enforcement.

`Enforcement state`
: Architectural assessment of whether an Enforcement Path is unknown, detectable, reachable, actionable, enforced, degraded, or broken.

`Enforcement Memory Projection`
: Future-extension rebuildable SurrealDB projection that connects accepted Decisions, source evidence, bindings, code/work context, signoff, compliance checks, and operational observations so Bicameral can query whether authority is reflected in implementation.

`Operational receipt`
: Rebuildable daemon-side record that an enforcement-related check, search, preflight, or local surface happened, useful for debugging, architecture evaluation, and local audit, but not itself an accepted governance fact.

`Known evidence`
: Evidence available to Bicameral at a point in time, including reviewed source evidence, explicit conflict metadata, feature/scope hints, verified binding evidence, accepted hosted advisories, and drift/compliance evidence. Unknown evidence is not assumed clean.

`RefreshMode`
: Query posture for Source Refresh eligibility. Automatic refresh is daemon- or policy-driven and is scoped to tracked Decisions by default; manual refresh is explicitly requested and may inspect Decisions outside the automatic tracking filter.

`UntrackReason`
: Reason recorded on an `Untrack Source`. Replay does not derive it from neighboring events.

`Read/write path`
: Review surfaces, MCP tools, integrations, and mods emit substrate-neutral commands/evidence; governance policy and event store adapters decide materialization.

`Local daemon`
: Local core that validates protocol objects, evaluates governance policy, preserves audit state, performs local grounding, and materializes accepted events.

`Capability handshake`
: Startup exchange where a local surface asks the daemon for its CapabilityReport before dispatching ToolRequests. Protocol mismatches fail fast; unsupported commands remain explicit capability results after protocol compatibility is established.

`Gateway`
: Local edge boundary that adapts operational sources into typed protocol objects before they enter daemon governance.

`Protocol folder`
: In-repo `protocol/` module containing shared object schemas and conformance fixtures used by bot, MCP, integrations, and cloud clients.

`Shared object schema`
: Reusable protocol schema for a core domain object shared by multiple surface contracts.

`ProjectionState`
: Read model container for the six V1 projection record families (decisions, source evidence, code evidence, graph snapshots, graph readiness, ledger events). Built from event-store replay; not canonical authority.

`ProjectedDecision`
: Signoff and review state view of a Decision within the V1 projection read model.

`ProjectedSourceEvidence`
: Cited evidence span linked to a Decision within the V1 projection read model.

`ProjectedCodeEvidence`
: Accepted binding evidence view for a Decision within the V1 projection read model.

`ProjectedGraphSnapshot`
: Snapshot identity and provenance metadata within the V1 projection read model.

`ProjectedGraphReadiness`
: Per-Decision graph indexing and readiness state within the V1 projection read model.

`ProjectedLedgerEvent`
: Event history entry within the V1 projection read model.

`ProjectionStore`
: Substrate-neutral trait for projection storage. Implementations provide build and rebuild from event-store replay.

`MemoryProjectionStore`
: Deterministic in-memory ProjectionStore implementation for local tests and fixture replay.

`RuntimeTrace`
: Operational receipt struct for Bot-MCP command correlation. Carries request_id, command, caller_surface, actor_id, workspace_id, session_id, provider, model, task_kind, token_usage, latency_ms, status, and failure_reason. It is not governance authority.

`TraceStatus`
: Trace-scoped status enum mirroring ToolResponseStatus for runtime trace context.

`TokenUsage`
: Reasoning provider token metadata (prompt_tokens, completion_tokens, total_tokens) carried within a RuntimeTrace.

`DispatchOutcome`
: Typed dispatch result wrapper used by dispatch_with_trace functions to pair a ToolResponse with its RuntimeTrace.

`Code-grounding fixture`
: Committed synthetic workspace at `tests/fixtures/code-grounding/` used for V1 code-grounding E2E and fixture-replay tests. Convention for local grounding verification without requiring a live repository.

## Terminology Notes

- `Decision` is not a suggestion, opinion, note, or general product knowledge; in V1 it is also not by itself a code-compliance or feature-readiness verdict.
- `Product Owner` is the default alpha reviewer for product-scoped Decision meaning; it is not a per-candidate reviewer assignment field. A Product creator is materialized atomically as both `Policy Owner` and `Product Owner`; same-domain auto-join grants `Product Member` only. `Policy Owner`, `HostedProjectPreview`, and `ProductJoinTicket` are product-surface onboarding terms defined in `docs/domain/product-surfaces-and-integrations.md`; the authoritative end-to-end onboarding contract is `docs/specs/domain-product-onboarding-contract.md` (issue #765).
- `Product Member` is not a domain-specific role such as compliance owner, payments owner, or security owner.
- "Workspace" is retired as a collaboration/tenancy term — that scope is the `Product`, and its participants are `Product Member`s. The word survives only in the unrelated local sense of a bound code folder / config substrate (e.g. `workspace binding`, `Workspace Config Validation`, `workspace_id`); those are local, code-grounding concepts, not a membership or tenancy layer. There is no `Workspace` or `Organization` entity.
- Cloud may still store a coordination `team` record (hosted tenancy / discovery boundary). In Alpha, a **verified domain** is a Supabase-verified email domain that is also on the design-partner allowlist; consumer/shared domains are never allowlisted. One verified domain maps to one Cloud team (`team_id` is a Cloud-issued opaque ID); a team may contain many Products. That Cloud `team` is not a Bot glossary entity, not a `Product`, and not a canonical org layer for Confidential data (ADR-0025). Design-partner domain allowlisting is not team creation. An allowlisted-domain user may pair their own daemon without being Cloud `team_owner`.
- `Source` covers both code and non-code observed surfaces; source kinds share governance semantics but may have different capture, refresh, indexing, and grounding implementations.
- `Tracked Decision` is the V1 maintenance boundary: Bicameral keeps promoted shared memory reviewable, not every raw source, rejected candidate, or unpromoted context excerpt ever observed.
- `EvidenceReference` is not proof that the cited source satisfies the claim; it records exactly what source region, tool call, output, file range, or diff hunk the claim cited.
- `SourceEvidence` is retained for the existing source-backed evidence object; use `EvidenceReference` for the broader abstraction across text, agent sessions, commands, diffs, and code regions.
- `Source Refresh` has source-kind-specific implementations. Code source refresh uses GraphStore, GraphSnapshot, GraphSlice, BindingAnchor, and Binding Revalidation machinery; non-code source refresh may use polling, webhooks, API reads, or manual recapture. Automatic refresh is scoped to tracked Decisions unless workspace policy adds another explicit refresh class.
- `RefreshEntry` is an implementation read-model row for Source Refresh queries, not a domain entity.
- `UntrackSourcePayload` is an implementation event payload name, not a domain entity; use `Untrack Source` for the lifecycle verb.
- `DecisionCandidate` is non-canonical until governance policy accepts a review command and the selected event store substrate materializes the event.
- `CandidateDraft` is an input shape for proposed candidates, not durable authority. Use `DecisionCandidate` once the daemon has accepted and materialized reviewable candidate state.
- `BindingHint` is advisory input, not accepted BindingEvidence and not proof of code compliance.
- `target_kind` is the implementation field spelling for Binding target kind.
- `BindingEvidence` is not compliance, reflection, readiness, or enforcement; it only records a verified relationship between a Decision and a code-shaped target at one GraphSnapshot.
- `BindingProposal` is not accepted `BindingEvidence`; it becomes governance evidence only after daemon governance accepts the transition and the selected event store substrate materializes the accepted event.
- Use `Binding Revalidation` for the domain concept; `RevalidateBindingPayload` is an implementation payload name.
- `ReasoningArtifact` is not canonical Decision Ledger authority, model truth, ReviewEvent authority, or a substitute for governance policy.
- `Refresh extraction` is not Decision promotion; it creates a new hosted advisory artifact for team-visible replacement review.
- `Source` is not immutable evidence; immutable evidence is cited through `SourceSnapshot` and `SourceEvidence`.
- `Ledger View`, dashboards, SurrealDB, search indexes, and hosted graphs are not durable authority.
- SurrealDB is the Enforcement Memory Projection, not an event authority substrate.
- `Enforcement Path` is an architectural evaluation method, not a required hotpath subsystem.
- `Operational receipt` is not a governance event; a receipt that later justifies, challenges, or changes shared authority must be represented through the event store.
- A **spec** is not an entity. It is a `Source` that is a file or a tracker issue. Transcripts, MCP sessions, command runs, pull requests, diffs, and code snapshots are `Source`s and are not specs. Do not call a `GovernedDomain` a spec.
- A `SourceSnapshot` has one `GovernedDomain`, created empty when the snapshot is captured. The only stored relation between two domains is a `CheckRecord` that says whether the pair is in conflict. The pair is unordered, stored in lexicographic order of the two `source_digest` values, and a second write is the same check. A missing check is not `conflict: false`. The record stores the `daemon_id` that wrote it, and that daemon is not part of the pair. There is no `Join` type and no `RelationClass`. A newer snapshot does not inherit check records. Rationale: ADR-0033.
- A `GovernedDomain` with no `source_digest` has not been extracted. A finished failure is an operational receipt, `extract_error`, and still has no `source_digest`. A completed extract has `source_digest`, the SHA-256 of its four content arrays, which may be empty. That completion is immutable. A change of route, model, or prompt applies to the next capture. Do not add Lineage for that change. `BindingLineage` remains the binding term. C1 runs on the daemon that holds the snapshot lease. C2 runs on any daemon that has both completed domains. Which daemon captures a `Source` is not decided. Rationale: ADR-0033.
- A byte difference between a branch snapshot and `main` is an identification fact about one `Source`. `main` is the canonical comparison. It is not a `GovernedDomain` relation. Rationale: ADR-0033.
- A git `SourceSnapshot` records whether its bytes matched `main` at capture. It does not store a living "this is `main`" flag. A later move of `main` does not rewrite that fact. Rationale: ADR-0033.
- `Decision`, `DecisionCandidate`, `Tracked Decision`, and `Decision Ledger` are purged. Do not introduce them again. `Signoff` is the approved state of the `GovernedDomain` paired with one `SourceSnapshot`. The other states are tracked, rejected, and superseded. A newer snapshot does not inherit that signoff. Two approved domains are not in conflict. Use `state` for this value. Do not use standing. Do not use status for this value. Status is the operational receipt. Use `conflict`. Do not use contradiction or contradicts. Rationale: ADR-0034.
- Use `Tracked Source` for the pre-signoff state. Do not use park, parked domain, or `GovernedDomainProposed`. The person who tracked a source may `Untrack Source` while its snapshot is still tracked. The owner rejects a tracked source that is not in a conflict set, and calls `resolve_conflict` for a conflict set. That command records the owner and the daemon outside `members`. `approve` and `reject` are refused while `in_conflict` is true. The approve-permission conflict cells stay `allow: false`. Track, untrack, reject, and supersede each record the daemon. Track and untrack record the person who tracked. Reject and supersede record the owner. Supersede names the successor snapshot. Untrack writes no state. Rationale: ADR-0034.
- `Signoff` is not compliance, drift, or grounding status. A `CheckRecord` is not `Signoff`. Rationale: ADR-0033, ADR-0034.
- `No conflict` means no known unresolved conflict within available evidence, not global consistency across unseen branches, repos, future implementation, or unimplemented checks.
- Frontend, CLI, MCP, connector, and mod surfaces must not create Decisions directly; they use `Promote`.
- External ingest envelopes and tool requests share object vocabulary, but they are separate contracts with different authority and actor semantics.
- Ingest, tool, and egress are separate surface categories: ingest imports evidence, tools request governed actions or queries, and egress exports Bicameral state or advisories outward.
- `SUPPORTED_COMMANDS` and `DEFERRED_COMMANDS` are implementation constants that populate a CapabilityReport, not standalone domain entities.
- `dispatch()` is an implementation function name; use `ToolRequest Dispatcher` for the domain concept.
- `allows_binding()` and `to_public()` are implementation helpers for the GraphEvidenceState authority boundary and PublicGraphEvidenceState projection; they are not standalone domain entities.
- `ConfigPreview` and `InviteBundleError` are implementation/API detail names unless a future UI or protocol spec makes their semantics durable product vocabulary.
- `BicameralConfig::validate()` is an implementation hook for Workspace Config Validation, not the canonical domain term.
- `ProjectionState` and its `Projected*` members are derived read models; they are not canonical event-store authority and must not be treated as governance facts.
- `RuntimeTrace` is an operational receipt for observability and correlation. It does not affect governance, signoff, compliance, or Decision state.
- `Code-grounding fixture` is a test infrastructure convention; it does not describe production runtime behavior.
