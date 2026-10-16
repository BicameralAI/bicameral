# ADR-0004: Hosted Reasoning Memoization

**Date:** 2026-06-03  
**Status:** proposed  
**Level:** L1  
**Related:** ADR-0001, ADR-0003

## Problem

Hosted Bicameral reasoning can make setup easier and produce more consistent decision extraction across a team. But hosted LLM calls are probabilistic and expensive. If every teammate or background job asks the model again for the same extraction request, the team may see duplicate inferred candidate sets and pay repeatedly for the same work.

Cloud must also preserve the core Bicameral authority boundary. Hosted reasoning can improve extraction and summaries, but it must not become the canonical Decision Ledger or a signoff/compliance authority.

## Decision

Bicameral Cloud owns hosted `ReasoningArtifact` storage, memoization, refresh state, and replacement state.

A hosted `ReasoningArtifact` is immutable advisory output from a hosted reasoning task. It may contain candidate drafts, evidence span selections, level hints, binding suggestions, summaries, or rationale. It does not approve signoff, resolve compliance, create canonical Decisions, write local event stores, or bypass governance policy.

Hosted reasoning must memoize artifacts by:

```text
tenant_id
reasoning_task
normalized_request_hash
policy_profile
model_policy_version
prompt_version
schema_version
redaction_policy_version
```

`normalized_request_hash` is the hash of the exact canonical request payload sent to hosted reasoning after redaction and normalization. If source text, workspace vocabulary, source snapshot identity, policy hints, or any other context can affect model output, it must be inside that canonical request payload or represented by one of the explicit version fields.

`workspace_id` is not part of the v1 memoization key. If different workspaces need different output, their canonical hosted reasoning requests must differ. Cloud may still record workspace id as metadata for attribution, billing, debugging, and UI filtering.

On cache hit, cloud returns the existing `ReasoningArtifact` instead of calling a model again.

## Model Routing And Cost

Hosted reasoning supports these user-facing policy profiles:

- `lowest_cost`
- `balanced`
- `highest_quality`

Cloud maps the selected profile to a concrete `model_policy_version`. A model policy may route each reasoning task to the cheapest sufficient model. For example, bounded span selection or title cleanup may use a cheaper model, while candidate extraction or conflict explanation may use a stronger model.

Artifacts record execution metadata such as selected model, routing reason, provider, token usage, estimated cost, cache hit/miss, policy profile, and model policy version. The cache key uses `model_policy_version` as the behavioral boundary. Cloud must bump the version when output behavior can materially change; it does not need to bump for infrastructure-only changes that preserve behavior.

## Refresh Extraction

Users can request `Refresh extraction` when an existing memoized artifact is unsatisfactory.

Refresh is an explicit cache bypass. It creates a new immutable artifact related to the previous artifact by `refresh_of_artifact_id` and starts in replacement-pending state.

In team mode:

- actors with ingest permission may request refresh;
- replacement acceptance requires the reviewer/member authority configured for the tenant or workspace;
- accepting replacement marks the refreshed artifact current and supersedes the prior artifact;
- rejecting replacement leaves the prior current artifact active;
- cloud stores this replacement state.

Refresh does not promote Decisions. It only changes which hosted advisory artifact backs the candidate set shown to the team.

## Storage Boundary

Cloud is the source of truth for hosted `ReasoningArtifact` payloads and replacement state.

The local bot may receive artifact ids, provenance, summaries, and candidate metadata from cloud, but the v1 local event store does not replay hosted artifact replacement. This is an intentional complexity exception: hosted artifacts are advisory, while canonical Decision Ledger transitions still materialize through local bot governance and the selected event store substrate.

## Consequences

Positive:

- The same hosted request produces stable team-visible advisory output.
- Cache hits reduce hosted model spend.
- Teams can understand cost/quality trade-offs through policy profiles instead of raw model selection.
- Refresh gives users an escape hatch without creating unrelated duplicate candidate sets.
- Cloud can improve model routing while preserving artifact provenance and version boundaries.

Negative:

- Local replay cannot reconstruct hosted artifact replacement history without cloud.
- Cache correctness depends on canonical request construction and version discipline.
- Cross-workspace reuse within a tenant is possible when canonical requests match; workspace-specific context must therefore be included in the request when it matters.

## Rejected Alternatives

- **Cache by tenant and workspace:** rejected for v1 because it adds complexity and reduces cost savings. Workspace-specific behavior belongs in the canonical request when it matters.
- **Cache by raw source snapshot only:** rejected because task, request context, redaction, prompt, schema, and policy versions can change output.
- **Expose raw model choice as the main control:** rejected because policy profiles better communicate cost/quality intent and let cloud route the right model to each task.
- **Store hosted replacement state in every local event store:** rejected for v1 because ReasoningArtifacts are advisory and cloud-owned.
