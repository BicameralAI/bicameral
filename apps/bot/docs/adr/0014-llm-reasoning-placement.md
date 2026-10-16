# ADR-0014: LLM Reasoning Placement

**Date:** 2026-06-03
**Status:** proposed
**Level:** L1
**Related:** ADR-0002, ADR-0004, ADR-0005, ADR-0007, ADR-0010

## Problem

Bicameral has several places where probabilistic reasoning can improve product quality: extracting candidate decisions from raw text, selecting evidence excerpts, classifying L1/L2/L3 level, proposing code bindings, summarizing conflicts, suggesting reviewers, and interpreting compliance or drift.

Those reasoning steps can happen in different places:

- in the caller, such as an MCP coding agent that already has LLM capacity;
- in the daemon, for integrations that only submit raw source evidence;
- in a hosted/cloud advisory service;
- in human review.

If Bicameral does not name this boundary, source integrations may accidentally gain reasoning and authority responsibilities they should not own, while MCP may be treated as generic external ingest even though it acts under local actor/session authority.

## Decision

LLM reasoning is an optional enrichment capability, not an authority boundary.

The daemon must be able to operate with deterministic fallbacks for schema validation, evidence acceptance, span defaults, L1/L2/L3 classification, local grounding validation, policy evaluation, and review routing. LLM output may improve candidate quality, hints, explanations, and prioritization, but it must remain evidence, hints, rationale, or advisory output until governance policy and review accept it.

Use these placement categories:

- **Caller-LLM**: the caller already has reasoning capacity, such as an MCP coding agent. The caller may submit candidate drafts, evidence excerpts, binding hints, level hints, compliance concerns, and rationale through `ToolRequest`.
- **Daemon-LLM**: the daemon enriches raw or lightly projected inputs from integrations that do not have built-in reasoning capacity. The daemon may extract candidates, choose excerpts, classify levels, suggest reviewers, summarize conflicts, or draft explanations.
- **Cloud/advisory LLM**: hosted services may return evidence, hints, advisories, and `GovernanceResult` proposals, but they do not become canonical authority.

## Data Flows

Caller with LLM capacity:

```text
MCP agent / local tool with LLM
  -> ToolRequest
       command: ingest.submit_local | binding.create | review.* | preflight.run
       payload: evidence, candidate drafts, hints, rationale
  -> daemon validates actor/session/policy context
  -> daemon verifies deterministic facts where possible
  -> governance policy and review
  -> event store adapter materialization, if accepted
```

Integration without LLM capacity:

```text
Jira / Fathom / Slack / GitHub adapter
  -> ExternalIngestEnvelope
       payload: source snapshot, evidence, source metadata
  -> daemon validation
  -> optional daemon enrichment
       extract candidates, default spans, infer level, suggest routing
  -> governance policy and review
  -> event store adapter materialization, if accepted
```

Integration with its own LLM capacity:

```text
integration LLM
  -> ExternalIngestEnvelope
       payload: evidence, candidate drafts, level_hint, rationale
  -> daemon validation and normalization
  -> governance policy and review
  -> event store adapter materialization, if accepted
```

Hosted advisory reasoning:

```text
daemon request for advisory analysis
  -> cloud/advisory service
  -> evidence, hints, advisories, GovernanceResult proposals
  -> daemon policy and review
  -> materialization only through accepted governance events
```

## Reasoning Surfaces

| Surface | Caller-LLM | Daemon-LLM | Authority rule |
|---|---:|---:|---|
| Candidate extraction | yes | yes | output is `DecisionCandidate`, not `Decision` |
| Evidence excerpt/span selection | yes | yes | source evidence must remain auditable |
| L1/L2/L3 classification | optional | optional | final level is daemon-normalized policy input |
| Candidate title/body cleanup | yes | yes | reviewer may edit before promotion |
| Duplicate/near-duplicate detection | optional | yes | result is advisory until policy accepts |
| Suggested reviewers/routing | optional | yes | policy decides required reviewers |
| Binding hints | yes | yes | daemon graph validation decides evidence state |
| Compliance/drift interpretation | yes | yes | review/policy decides compliance resolution |
| Conflict/collision explanation | optional | yes | no-conflict means no known conflict only |
| Review queue summaries | yes | yes | presentation only |
| Egress/write-back drafting | optional | yes | future `Egress` remains separate contract |

## L1/L2/L3 Classification

External integrations should not be required to assign canonical decision level.

They may provide `level_hint` plus rationale. The daemon owns normalization into candidate `level`, and governance policy treats that level as an input, not as authority.

The first implementation should use deterministic defaults:

```text
code regions present
  -> L2
product source: transcript | notion | slack | document
  -> L1
implementation source: implementation_choice | agent_session
  -> L3
fallback
  -> L2
```

Future daemon LLM classification may refine this, but it must produce an auditable hint/rationale and preserve deterministic fallback behavior.

## Consequences

- The ingest gateway does not require an LLM to accept clean projections.
- MCP remains a `ToolRequest` surface because it acts under local actor/session authority, even when it carries LLM-generated candidates or hints.
- External integrations remain non-authoritative even if they use an LLM internally.
- The daemon may host reasoning workers, but daemon LLM output cannot bypass governance policy, review commands, graph validation, or event store materialization.
- Protocol schemas should distinguish hints, rationale, confidence surfaces, and verified facts instead of collapsing them into generic confidence or authority fields.

## Rejected Alternatives

- **Require daemon LLM for ingest:** rejected because deterministic ingest, classification, and policy fallbacks are necessary for local-first reliability, testability, and offline operation.
- **Trust caller LLM output as authority:** rejected because MCP agents and integrations can propose evidence and commands, but canonical state still requires governance policy and materialization.
- **Push L1/L2/L3 classification to integrations:** rejected because source adapters should emit clean projections and hints, not own Bicameral governance classification.
- **Treat MCP as external ingest plus egress:** rejected because MCP is a local tool surface with actor/session authority; responses to MCP are not external write-back.
