# ADR-0015: Reasoning Provider Boundary and Hosted Artifact Exception

**Date:** 2026-06-03  
**Status:** proposed  
**Level:** L1  
**Related:** ADR-0002, ADR-0005, ADR-0007, ADR-0010

## Problem

Bicameral needs LLM-backed reasoning for candidate extraction, evidence span selection, level hints, binding suggestions, conflict summaries, and review summaries. Teams also need setup ergonomics: one user may prefer a local OpenAI-compatible endpoint, another may use a local CLI, and a team may want a hosted default after signing in from the dashboard.

At the same time, Bicameral's core authority model must not shift. The daemon and event store decide what becomes canonical through governance policy and accepted review commands. A model provider, hosted or local, must not become a hidden Decision Ledger authority.

Hosted reasoning introduces another trade-off. A hosted service can give teams consistent, cached, versioned interpretation of the same evidence and lower cost by avoiding repeated model calls. But fully replaying hosted reasoning artifact replacement through every local event store would add complexity before these artifacts have canonical authority.

## Decision

Treat LLM reasoning as a provider-backed enrichment boundary, not an authority surface.

The bot may support these reasoning modes:

- `disabled` - deterministic fallback only.
- `caller` - MCP, CLI, or another local caller submits model-derived hints through `ToolRequest`.
- `local_openai_compatible` - the daemon calls a user-configured OpenAI-compatible endpoint.
- `local_cli` - the daemon calls a user-configured command with typed JSON input/output.
- `hosted_bicameral` - the daemon or dashboard uses Bicameral Cloud as a hosted reasoning provider after sign-in.

All provider outputs remain advisory artifacts: `DecisionCandidate`s, `SourceEvidence` selections, `BindingHint`s, level hints, summaries, rationale, or advisory `GovernanceResult` proposals. They cannot approve signoff, resolve compliance, create canonical Decisions, write event store internals, or bypass review command gating.

Hosted reasoning is the only v1 mode with mandatory memoization. Local provider memoization is out of scope.

For hosted reasoning, the cloud service owns `ReasoningArtifact` payloads, memoization, refresh state, and artifact replacement state. The local bot may reference hosted artifact identifiers and provenance in responses or candidate metadata, but v1 does not require the selected event store substrate to replay hosted artifact replacement events.

This is an intentional exception to avoid complexifying local event-store replay with advisory hosted artifact lifecycle before the artifact has canonical authority. Candidate promotion, signoff, binding, compliance, demotion, and other consequential Decision Ledger transitions still require ordinary governance and event-store materialization.

## Hosted Model Configuration

The dashboard should expose a reasoning/model configuration surface:

- hosted Bicameral sign-in and team selection;
- local OpenAI-compatible endpoint configuration;
- local CLI adapter configuration;
- task toggles for extraction, evidence span selection, level hints, binding suggestions, conflict summaries, and review summaries.

For hosted Bicameral, workspaces choose one of three policy profiles:

- `lowest_cost`
- `balanced`
- `highest_quality`

Bicameral Cloud maps the selected profile to a concrete `model_policy_version`. A model policy may route different reasoning tasks to different models so cheap models handle bounded low-consequence tasks while stronger models handle higher-risk extraction or conflict analysis.

## Refresh Extraction

`Refresh extraction` is the user-facing action for requesting a new hosted `ReasoningArtifact` when an existing memoized artifact already exists.

In team hosted mode:

- any actor with ingest permission may request refresh;
- a refreshed artifact is team-visible as a pending replacement;
- a governance-policy-approved reviewer/member accepts or rejects the replacement;
- accepting the replacement changes the cloud-current advisory artifact;
- refreshing or replacing an artifact is not Decision promotion.

The hosted provider must not produce duplicate unrelated candidate sets for the same extraction task. A refreshed artifact is related to the prior artifact through replacement state.

## Consequences

Positive:

- Users can bring local endpoints or CLIs without changing Bicameral authority.
- Hosted Bicameral can be a paid convenience and quality surface: zero setup, consistent team-wide reasoning, managed model routing, and lower cost through memoization.
- Hosted memoization reduces duplicate inferred decisions from the same request.
- The local bot remains simpler because hosted advisory artifact replacement is cloud-owned in v1.

Negative:

- Local replay will not fully reconstruct hosted artifact replacement history without cloud access.
- Hosted reasoning artifact lifecycle needs cloud availability and cloud audit controls.
- Some advisory review context may be less portable than canonical Decision Ledger events.

These trade-offs are acceptable for v1 because `ReasoningArtifact`s are advisory. Canonical decisions and consequential governance transitions still replay from the selected event store substrate.

## Rejected Alternatives

- **Make hosted reasoning a new authority surface:** rejected because it conflicts with the daemon/event-store governance boundary.
- **Require local event-store materialization of every hosted artifact replacement:** rejected for v1 because it adds complexity to replay for advisory artifacts.
- **Memoize local providers too:** rejected for v1 because local provider storage and user expectations differ from hosted team reasoning.
- **Expose raw model choice as the primary UX:** rejected because policy profiles better express cost/quality trade-offs while allowing Bicameral to route the right model to the right task.
