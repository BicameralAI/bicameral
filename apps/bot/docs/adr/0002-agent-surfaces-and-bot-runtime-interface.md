# ADR-0002: Agent Surfaces and Bot Runtime Interface

**Date:** 2026-05-27  
**Status:** proposed  
**Level:** L1  
**Carries forward:** `bicameral-daemon` ADR-0002 plus the original `bicameral-bot` protocol ownership ADR
**Updated:** 2026-06-03 — `ToolRequest` is the canonical local tool request contract.
**Updated:** 2026-06-24 — authenticated remote review/action callbacks are a separate contract from source ingest, local `ToolRequest`, and egress.

## Problem

The original daemon ADR described a split between `bicameral-mcp` as the local agent tool surface and `bicameral-daemon` as hosted/private infrastructure. The repo split changed that shape: `bicameral-bot` is now the public local daemon/gateway/runtime, and `bicameral-mcp` is one client surface that exposes bot capabilities to coding agents.

We still need the same boundary: agent-facing tools can propose, query, and submit review commands, but persistence authority sits behind bot governance and the selected event store adapter.

## Decision

`bicameral-bot` owns the local runtime interface. It exposes a canonical `ToolRequest` contract for local tools to request governed actions or queries from the daemon.

`ToolRequest` is the transport-neutral request object for local Bicameral tools to ask the daemon to perform governed actions or queries under local actor/session authority. MCP, CLI, TUI, and dashboard actions map to `ToolRequest` before daemon validation and policy evaluation. `ToolRequest` is not an external evidence-submission format and not an outbound write-back format.

The local runtime interface is exposed to:

- `bicameral-mcp` stdio tools;
- CLI/TUI commands;
- local dashboard actions;
- EM-safe mod runners;
- optional cloud advisory clients.

These local surfaces submit `ToolRequest` objects whose commands may carry shared protocol objects such as `SourceEvidence`, `DecisionCandidate`, `BindingHint`, `DependencySignal`, `ComplianceVerdict`, and `ReviewCommand`. The bot validates authority context, runs governance policy, and delegates accepted materialization to the selected event store substrate.

External source integrations do not use `ToolRequest`. They submit `ExternalIngestEnvelope` objects at the ingest boundary. The gateway or daemon maps those envelopes into internal ingest behavior without granting local actor authority.

Authenticated remote review/action callbacks are also separate from source
ingest. A Slack modal, Linear action, hosted review button, or similar external
review surface may submit a governed action only after the bot verifies the
provider callback, binds the external identity to a Bicameral actor, checks
workspace/product scope, and evaluates governance policy. Such callbacks are
not `ExternalIngestEnvelope` payloads, because source text, status, comments,
labels, or reactions must not smuggle review authority into evidence ingest.

Outbound write-back, notification, sync, or annotation is a separate future
`Egress` contract category. It must not be modeled as `ToolRequest`,
`ExternalIngestEnvelope`, or an authenticated remote review/action callback.

## MCP Server

`bicameral-mcp` is the coding agent's direct tool surface. Its MCP tool calls map to `ToolRequest` commands such as:

- `ingest.submit_local` — local actor submits source/session evidence or candidates;
- `preflight.run` — surface relevant decisions before implementation;
- `binding.create` / `binding.inspect` — propose or inspect binding evidence;
- `review.accept_candidate`, `review.reject_candidate`, `review.approve_signoff`, `review.reject_signoff`, and `review.resolve_compliance`;
- `history.list` / `search.query` — read replayed/materialized decision state;
- `bicameral.dashboard` — open the local review UI.

MCP may keep helper caches for speed, but it is not canonical authority. MCP v0.2 direct tool input/output contracts are not preserved as a compatibility requirement. The MCP refactor may break v0.2 clients if the new `ToolRequest` contract requires different command names, authority context, or payload shapes.
Configurable inline review behavior for MCP and other tool calls is specified
in `../specs/configurable-inline-governance-review.md`; it remains a Review
Surface / `ToolRequest` UX pattern rather than a separate authority model.

## CLI / TUI

The CLI is the boring operator surface for setup, smoke tests, local runs, fixture-based mod tests, and governance commands:

```bash
bicameral init
bicameral ingest fixtures/source.json
bicameral preflight --repo .
bicameral review accept-candidate DEC-CAND-123
bicameral mod validate mods/jira-dependency-risk/mod.yaml
bicameral mod run mods/jira-dependency-risk --input fixtures/jira-issue.json
```

## Dashboard / Review UX

The dashboard is a review surface, not a database editor. It renders allowed commands and emits `ToolRequest`s carrying review commands. It must not write substrate-specific files directly.

## Integrations and Mods

Integrations and mods are edge producers. Source-adapter integrations use
`ExternalIngestEnvelope`, not `ToolRequest`. They can create candidates, hints,
evidence, dependency signals, suggested reviewers, and advisory governance
results. They cannot approve signoff, resolve compliance, emit review commands,
claim local actor/session authority, or write canonical state directly.

An external product can also host an authenticated review surface, but that is
a different contract from source ingest. The bot must verify the remote action,
derive actor context, and route the action through the same governed command
policy used by local review surfaces before any canonical mutation occurs.

## Protocol Ownership

`bicameral-bot/protocol/` owns the public-local contract vocabulary for bot, MCP, integrations, and cloud clients: schemas, conformance fixtures, object vocabulary, compatibility notes, and command/result contracts.

The protocol must keep four surface categories separate:

- `ExternalIngestEnvelope` for external/source-system evidence submission.
- `ToolRequest` for local actor/agent/UI actions and queries.
- Authenticated remote review/action callbacks for verified user actions
  initiated from external review surfaces such as Slack, Linear, or hosted UI.
- `Egress` for future outbound publication, notification, sync, or annotation.

This replaces the old idea that `bicameral-daemon/protocol/` is the sole internal contract module and also replaces the temporary standalone `bicameral-protocol` repository. `bicameral-protocol` should remain a deprecated pointer until an independent protocol package is justified.

## Interface Rule

All surfaces must follow this path:

```text
surface action
  → ToolRequest
  → bot validation and governance policy
  → ReviewEvent / GovernanceResult
  → event store adapter materialization, if accepted
  → replayed/materialized state
```

No surface bypasses this path.

External source ingest follows a separate path:

```text
external/source-system evidence
  → ExternalIngestEnvelope
  → ingest validation and mapping
  → candidates/evidence/hints/signals
  → governance policy and review routing
```

External ingest must not carry `ToolRequest` authority fields such as local actor identity, session identity, policy grants, review commands, signoff approval, compliance resolution, canonical mutations, or blocking governance results.

Authenticated remote review/action callbacks follow a separate path:

```text
verified external review action
  → provider callback verification and anti-replay checks
  → Bicameral actor binding and capability lookup
  → bot action authorization policy
  → ReviewEvent / GovernanceResult
  → event store adapter materialization, if accepted
```

External target state, source text, comments, labels, statuses, or reactions do
not follow this path unless they are part of a verified action callback and
successfully bind to an authorized Bicameral actor.

## What MCP / Agents Do Not Own

- Event store adapter internals.
- Canonical audit log.
- Direct `.bicameral/decisions/*.yaml` writes.
- Hosted organization-scale code graph.
- Cross-branch conflict advisory service.
- Multi-tenant auth for cloud services.
- Final authority over signoff or compliance unless governance policy explicitly accepts the submitted review command.

## Consequences

The local bot is the authority boundary; MCP stays a focused agent tool surface. Protocol contracts live where the local runtime lives. Integrations and cloud can evolve as clients without owning local authority.

## Rejected Alternatives

- **MCP writes substrate-specific state directly:** rejected because it couples agent UX to git/Drive internals and bypasses governance.
- **A separate protocol repo now:** rejected as premature.
- **All-hosted daemon authority:** rejected for the public bot path because local-first trust boundary and offline usefulness are core product requirements.
