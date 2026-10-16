# ADR-0001: Local Daemon, Gateway, and Decision Event Store Substrates

**Date:** 2026-05-27  
**Status:** proposed  
**Level:** L1  
**Carries forward:** `bicameral-daemon` ADR-0001 plus the original `bicameral-bot` ADR-0001

## Problem

Bicameral must ingest from many messy operational edges without letting each source decide what is canonical. Teams differ by Jira/Linear/Slack/meeting/GitHub usage, privacy boundary, repo topology, and review culture.

The decision ledger previously treated an opaque database/cache as durable authority. That creates a split-brain problem: canonical decisions live somewhere teams do not naturally review while the work they govern lives in git, Drive, dashboards, and agent sessions. The load-bearing product requirement is predictable authority, not clever self-maintenance.

## Decision

`bicameral-bot` is a public local-first runtime with a daemon + gateway shape.

The gateway is expressive. It adapts edge inputs into typed Bicameral protocol objects: source evidence, candidates, dependency signals, binding hints, review commands, and advisory governance results.

The daemon/core is boring. It validates objects, evaluates governance policy, preserves audit state, performs local grounding, and materializes accepted events through storage adapters.

Define an **event store substrate** boundary for canonical decision state. The selected event store substrate owns the durable, replayable event stream for a workspace. The bot's local database, dashboard state, MCP materialization, hosted code graph state, and search indexes are caches/materializations rebuilt from that substrate plus code/source indexes. They are not canonical authority.

The event store substrate boundary expresses the purpose of the Decision Ledger, not the mechanics of a particular storage system. Git and Google Drive must both satisfy the same ledger contract: accepted governance events are durably stored, replayable into the same materialized decision state, auditable, idempotent, ordered, and health/freshness-reportable. A basic shared store such as Google Drive is sufficient when it preserves that contract.

Git is a richer substrate, not a different ledger ontology. It extends the baseline Decision Ledger contract with repository-native review and resolution capabilities: branch-local proposals, PR diff review, merge conflict resolution, CI checks, and merge-gate enforcement where the repository actually has that boundary. Those capabilities must be advertised separately from the baseline ledger contract so non-git substrates are not forced to pretend they can provide PR resolution.

First-class substrate flows:

1. **Google Drive-backed event store** — canonical decisions and review events live as replayable files in a user-provisioned Drive folder. This is the baseline shared-store shape for the Decision Ledger contract.
2. **Git-backed event store** — canonical decisions and review events live as reviewable text in the repository, with optional PR/merge/CI capabilities layered on top of the baseline ledger contract.
3. **Future event store adapters** — must implement the same replay/materialization contract before they can claim canonical authority.

ADR-0007 defines the substrate-neutral governance flow: how source evidence, candidates, review commands, policy, and governance results become eligible for materialization. ADR-0001 only defines how accepted domain events are stored, replayed, and exposed by the chosen substrate.

### Survey-informed default

Public developer survey evidence supports git-backed `.bicameral/` text as the default onboarding path for repo-centric engineering teams, but not as the whole Bicameral ontology. Stack Overflow's 2024 survey shows that developer decision evidence is spread across Jira, Confluence, Markdown files, Notion, GitHub Discussions, Azure DevOps, Obsidian, Slack, Teams, Zoom, Discord, and other collaboration surfaces. See [Public Developer Survey Implications](../research/public-developer-survey-implications.md).

Implication: git remains the predictable first-class adapter where teams already use PR/code review as an authority boundary. The architectural invariant is still replayable, auditable accepted events through an event store substrate contract. Drive and future adapters remain real because the source of decision evidence and the source of code are not always the same substrate.

## Substrate Contract

Every event store substrate must implement the same contract even if its files, review mechanics, and enforcement capabilities differ.

### 1. Durable event log

The substrate must durably preserve accepted domain events, including:

- source/candidate acceptance events;
- review events;
- decision materialization events;
- supersession events;
- binding/compliance review events when those are accepted into durable history;
- governance result records that affect shared team state.

### 2. Replay

The bot must be able to rebuild current state from substrate artifacts:

```text
event store substrate
  → replay ordered events
  → materialized decisions / review state / signoff state / compliance state
  → local caches + dashboard + MCP/agent views
```

Replay must not depend on hidden local database rows.

### 3. Idempotence and ordering

Each accepted event must have a stable identity and enough ordering metadata for deterministic replay. Substrates may use different ordering sources:

- Git: commit order, branch/merge topology, file revision, event timestamp.
- Drive: event filename timestamp, event id, Drive revision id, last-confirmed sync cursor.

### 4. Conflict handling

The adapter must detect and surface duplicate candidate acceptance, incompatible decisions, concurrent edits, stale local state, substrate write conflicts, and replay failures.

Conflict explanation must be visible through governance results and the Ledger View, but the explanation mechanics can differ by substrate.

### 5. Auditability

A reviewer must be able to inspect source references, candidate text/rationale as reviewed, reviewer action and capability, policy result, materialized event artifacts, and supersession/collision resolution.

Secrets must not be persisted as canonical event content; adapters should store redacted excerpts or pointers when necessary.

### 6. Capability declaration

Each adapter must declare which substrate capabilities it can provide beyond the baseline ledger contract. Capability declaration is separate from append/replay/health semantics.

Baseline capabilities every durable substrate must provide:

- durable event preservation;
- deterministic replay into the same Decision Ledger state;
- audit inspection of accepted events and retained evidence pointers;
- idempotence/conflict detection sufficient to avoid silent corruption;
- health/freshness reporting.

Additional capabilities may include:

- branch-local proposals;
- PR-native review of event/materialization changes;
- native merge conflict resolution;
- CI or merge-gate enforcement.

An adapter must not claim CI/merge blocking unless the selected substrate actually provides a merge boundary. Drive-backed substrates should expose advisory review, dashboard flags, queued actions, or paused review states instead of claiming PR resolution.

## Shared Domain Objects

The substrate stores accepted events for the shared governance vocabulary from ADR-0007:

- `SourceEvidence`
- `DecisionCandidate`
- `Decision`
- `ReviewCommand`
- `ReviewEvent`
- `ReviewState`
- `BindingHint`
- `BindingEvidence`
- `DependencySignal`
- `ComplianceVerdict`
- `GovernanceResult`

A substrate does not have to store all objects as separate files. It must store enough event data to replay the same materialized domain state.

## Git-Backed Event Store

Use git when the workspace wants boring, predictable, text-in-repo review.

Git uses the same Decision Ledger event contract as Drive and future substrates. Its distinguishing feature is that accepted and proposed ledger artifacts can participate in ordinary repository review. Git can therefore extend the ledger with PR resolution: branch-local proposals are reviewed in diffs, merge conflicts expose competing event/materialization changes, and CI may enforce governance policy before merge.

This extension must not leak into the baseline ledger contract. Append/replay/audit semantics must remain substrate-neutral so the same accepted domain events can be represented by a basic shared store.

The canonical filesystem format is event-first. Accepted governance events are
the durable authority. Human-readable materialized files may exist for review
ergonomics, but replay must be able to reconstruct the same Ledger state from
the accepted event files plus referenced canonical blobs.

Minimum layout:

```text
.bicameral/
  events/
    2026-05-27T180000Z-accept-candidate.yaml
  decisions/
    DEC-0001.yaml
  bindings/
    DEC-0001.yaml
  sources/
    <source-id>/
      snapshots/
        sha256-<source-snapshot-addr>.json
      evidence/
        <evidence-id>.yaml
  policy/
    governance.yaml
```

The exact layout can evolve, but git-backed materialization must preserve these rules:

- accepted event files under `.bicameral/events/` are the canonical replay log;
- canonical state is reviewable text;
- `decisions/` and `bindings/` are reviewable materializations of accepted
  events and must remain replay-compatible with the event log;
- `sources/` stores retained `SourceSnapshot` bodies and `SourceEvidence`
  pointers when the selected substrate owns retained source evidence;
- accepted events are replayable without the local cache;
- branch-local proposals stay reviewable in ordinary PR workflow;
- CI/merge enforcement is only claimed when the repository actually has that boundary;
- mods, connectors, and agents never write `.bicameral/decisions/*.yaml` directly.

### Source and graph artifact storage

`SourceSnapshot` content is canonical when Bicameral retains it as cited
evidence. In the git-backed substrate, retained source snapshot bodies should be
stored under `.bicameral/sources/` using content-addressed filenames, and events
should reference them by `snapshot_addr`/`snapshot_ref`. A source folder is
therefore appropriate because source evidence is part of the audit trail for
Decisions, bindings, and governance results.

`GraphSnapshot` is different. The full code graph index is a daemon-owned
rebuildable cache keyed by `graph_snapshot_id`, `repo_id`, `commit_sha`, and
`index_version`. Do not create a canonical `.bicameral/graphsnapshots/` folder
for parsed symbols, edges, or index internals. Accepted events may store graph
snapshot identity, validation metadata, and reviewed `BindingEvidence`, but the
graph index itself belongs in local cache storage or hosted graph
infrastructure.

## Google Drive-Backed Event Store

Use Drive when the workspace starts from shared product/team memory rather than a repo PR boundary.

Drive-backed events must still be replayable, ordered, idempotent, and auditable. Drive is the reference shape for a basic shared store: it satisfies the Decision Ledger contract without adding repository-native resolution. Because Drive cannot block a merge, enforcement should surface as dashboard flags, agent warnings, queued actions, or paused review states rather than pretending to be CI.

## Non-Goals

This ADR does not define extraction prompts, connector-specific ingest rules, review UI layout, owner/member capability policy, confidence thresholds, code graph indexing, or the default substrate choice.

Source-specific adapters belong in `bicameral-integrations`. Organization-scale hosted code graph behavior belongs in `bicameral-cloud`.

## Consequences

Bicameral can meet teams at their operational substrate while preserving one authority path. Git remains predictable for repo-centric teams; Drive and future substrates can exist without changing the domain lifecycle. Local cache corruption is recoverable by replay.
