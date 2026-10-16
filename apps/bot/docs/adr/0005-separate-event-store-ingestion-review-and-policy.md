# ADR-0005: Separate Event Store Substrate, Ingestion Connectors, Review UX, and Governance Policy

> Carries forward `bicameral-daemon` ADR-0005. In the split repo model, `bicameral-bot` owns local policy/event-store/review-command boundaries; `bicameral-integrations` owns source adapters; `bicameral-cloud` owns hosted advisory graph infrastructure.

**Date:** 2026-05-27
**Status:** proposed
**Level:** L1
**Related:** ADR-0001, ADR-0003, ADR-0004, ADR-0007

## Context

Bicameral needs to ingest product and technical decisions from many sources,
route uncertain claims through human review, store canonical decisions somewhere
predictable, and optionally enforce governance before code merges.

Those concerns are related, but they are not the same thing:

1. **Decision event store substrate** — where canonical decision events and review events
   live: git, Drive-backed log, or another replayable plain-file mechanism. It answers "what is durable authority?"
2. **Ingestion connectors** — where DecisionCandidates come from: Notion,
   Slack, Linear, email, documents, MCP sessions, agent traces, or manual input.
3. **Review UX** — where humans approve, reject, edit, request context, or
   resolve grounding/compliance: PR review, dashboard, Slack modal, CLI/TUI, or
   other surfaces.
4. **Governance policy** — the policy engine that decides which candidates are
   accepted, which signoff transitions are allowed, which claims remain advisory,
   which require owner/member capability review, and which can block implementation or merge.

Earlier daemon ADRs intentionally favored git because it gives the current team
predictable text, diff review, branch-local proposals, CI enforcement, and audit
history. That remains a strong default for a git-backed workspace. But tying the
entire bot model to git/CI would make ingestion automation, hosted review UX,
and future non-git deployments harder to reason about.

Public survey evidence reinforces this separation. Stack Overflow's 2024 survey
shows decision-adjacent work spread across Jira, Confluence, Markdown files,
Notion, GitHub Discussions, Azure DevOps, Obsidian, Slack, Teams, Zoom, Discord,
and other surfaces. Atlassian/DX developer-experience research reports large
time losses from workflow inefficiencies. See [Public Developer Survey Implications](../research/public-developer-survey-implications.md).

That evidence argues for connector breadth, but connector breadth increases
false-authority risk. The more sources Bicameral ingests, the more important it
is that connectors produce evidence/candidates/hints only while governance policy
and review commands decide authority.

## Decision

Separate the bot architecture into event store, ingestion, review, and policy
interfaces. Git/CI remains a first-class event store/enforcement adapter, not the
universal ontology of the product. ADR-0007 owns the substrate-neutral governance
flow and the shared command/result vocabulary; this ADR owns the separation of
interfaces so each layer can evolve independently.

### 1. Ingestion connectors produce evidence/candidates, not authority

Connectors may poll, subscribe, or accept manual payloads from external systems,
but their outputs remain evidence, candidates, hints, or signals as defined by
ADR-0007. A connector does not make a decision canonical by itself.

### 2. Governance policy is configurable by workspace and source

The policy interface should support different automation settings without changing
connector code. The configuration axes are owned by ADR-0007; this ADR only
records that policy is separate from connectors, review UX, and event authority.

Policy should be able to vary by at least:

- event store substrate;
- source connector;
- source trust level;
- decision level (`L1`, `L2`, `L3`);
- extraction/binding/compliance confidence;
- required owner/member capability or reviewer assignment;
- desired enforcement behavior.

Common modes:

| Mode | Behavior |
|---|---|
| `manual` | Human chooses sources and reviews all candidates before acceptance. |
| `auto_candidate` | Connectors ingest automatically, but candidates queue for review. |
| `auto_candidate_manual_approve` | Candidate creation is automatic; `accept_candidate` and `approve_signoff` require owner/member review plus any additional policy checks according to level and evidence. |
| `auto_approve_low_risk` | Narrow, configured classes of high-confidence/low-risk candidates can be accepted and signoff-approved automatically with audit events when workspace policy permits both transitions. |
| `advisory_only` | Candidates inform agents and dashboards but never block, enter the Decision Ledger, or approve signoff without explicit review. |

Default policy should favor progress without false authority: automate candidate
creation, require owner/member review for candidate acceptance and signoff approval
unless policy explicitly permits low-risk automation, and keep weak
grounding/compliance advisory until reviewed.

Low-risk automation must remain narrow and inspectable.

### 3. Review UX emits review commands

Review surfaces must not write directly to event-store-specific internals. PR review,
dashboard actions, Slack modals, and CLI/TUI commands emit substrate-neutral
review commands defined by ADR-0007. ADR-0006 constrains the dashboard version of
this interface for the owner/member-safe mods spike.

### 4. Event store substrate implements canonical persistence

Event store adapters persist accepted events and expose replay/materialization
semantics. ADR-0001 owns substrate details such as git YAML/commits versus Google
Drive event files/freshness metadata.

### 5. Enforcement is capability-specific

The abstract policy output is a `GovernanceResult` as defined in ADR-0007, not
necessarily a CI check. ADR-0001 owns the selected substrate's enforcement
capabilities.

## Core Tensions Left Unresolved

This ADR intentionally leaves several product and implementation choices open:

1. **Default automation policy** — which mode should a new workspace start with?
   The safe default is `auto_candidate_manual_approve`, but solo users may expect
   more automation.
2. **Automation threshold** — which decision classes can auto-accept candidates
   and/or auto-approve signoff, and which always require owner/member review plus
   any additional policy checks?
3. **Global vs branch-scoped candidates** — an externally ingested Notion decision
   may apply globally, while an agent-discovered implementation gap may belong
   to a feature branch. Policy must decide when a candidate attaches to a branch,
   a global queue, or both.
4. **Review command ownership** — protocol should define command shapes, but the
   bot must decide which commands it owns directly vs delegates to MCP clients
   or event store adapters.
5. **Non-git enforcement semantics** — git-backed workspaces can fail CI. Other
   substrates need equivalent visibility without pretending they can enforce a
   merge boundary they do not control.
6. **Review UX product shape** — dashboard, PR review, Slack modal, and CLI/TUI
   can share commands, but each imposes different latency, batching, and audit
   expectations.

## Progress Without Committing

We can make implementation progress without choosing every substrate or review
surface now by building the seams first:

1. Keep ingestion connectors producing evidence/candidates/hints only.
2. Keep review UX surfaces thin: they emit ADR-0007 commands and render
   `GovernanceResult`, not event-store-specific state.
3. Keep event store adapters responsible for ADR-0001 materialization/replay.
4. Keep governance policy configurable by source, level, evidence strength,
   owner/member capability or reviewer assignment, and substrate capability.

This preserves the team's current git-native path while preventing the bot
from baking git/CI assumptions into every future connector and review workflow.

## Consequences

Positive:

- Prevents ingestion, event authority, review UX, and enforcement from collapsing into a
  single hardcoded git/CI flow.
- Allows different workspaces to choose different ingestion automation settings.
- Keeps git-backed teams on predictable text + PR review without requiring every
  future workspace to use git as the event store substrate.
- Provides a stable path for dashboard, Slack, and CLI review surfaces to share
  command semantics.

Tradeoffs:

- Introduces policy and adapter interfaces before every adapter exists.
- Requires discipline: connectors must not quietly bypass review policy.
- Requires governance result semantics that can degrade gracefully when a
  substrate cannot enforce a hard block.
## Rejected Alternatives

- **Git/CI as the universal model:** rejected because it conflates event authority and
  enforcement with the broader Bicameral domain lifecycle.
- **Connector-specific review flows:** rejected because each source would invent
  different authority semantics and make cross-source governance incoherent.
- **Review UX writes directly to event store internals:** rejected because it couples dashboard,
  Slack, PR review, and CLI behavior to one substrate's internals.
- **Fully automatic ingestion-to-canonical flow by default:** rejected because it
  creates false authority when extraction, grounding, or compliance is uncertain.
