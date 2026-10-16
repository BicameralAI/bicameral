# ADR-0009: Adopt ZeroClaw Runtime Scaffold

**Date:** 2026-05-27  
**Status:** proposed  
**Level:** L1

## Problem

`bicameral-bot` needs to become a local-first runtime rather than only a protocol/README repository. The runtime needs boring operational machinery before Bicameral-specific governance can be useful: CLI/service lifecycle, local config, gateway ingress, provider/tool abstraction, approval gates, audit receipts, and an extension surface for EM-authored mods.

Building all of that from scratch would delay the core product question: can software teams safely turn human context into typed, reviewable Bicameral objects without giving agents authority over canonical decisions?

## Decision

Import ZeroClaw as a tracked upstream scaffold under `third_party/zeroclaw` and use it as the reference implementation for the first `bicameral-bot` runtime extraction.

ZeroClaw is appropriate because its current architecture already matches the shape required by ADR-0001:

- local-first binary/service posture;
- gateway + runtime layering;
- provider-, channel-, tool-, and plugin-style extension points;
- supervised autonomy and security policy enforcement;
- SOP-style deterministic procedures with approval gates;
- audit/tool-receipt concepts;
- Rust workspace split around API, config, runtime, gateway, plugins, providers, memory, and tools.

The import is a scaffold, not a domain decision. Bicameral's domain authority remains in its protocol contracts, governance policy, review commands, and storage adapters.

### Survey-informed constraint

Public AI and buyer-behavior surveys support a local daemon/gateway scaffold only if it improves trust, security, code context, auditability, approval gates, and fast proof of value. Stack Overflow, JetBrains, and GitHub report broad AI tool adoption or organizational allowance, but Stack Overflow also reports codebase-context and output-trust concerns. G2's 2024 buyer report highlights short ROI expectations and security-breach scrutiny. See [Public Developer Survey Implications](../research/public-developer-survey-implications.md).

Implication: ZeroClaw's local-first runtime, gateway, security, approval, and audit primitives are useful. Generic assistant surface area should be removed or postponed unless it directly serves Bicameral decision capture, local grounding, review, audit, or safe extension.

## Imported Source

- Upstream: `https://github.com/zeroclaw-labs/zeroclaw`
- Imported as: git submodule at `third_party/zeroclaw`
- Pinned commit at import time: `cbf915d43a3c43116d63c122732942cf8782ff16`
- License observed at import time: MIT OR Apache-2.0

The submodule boundary is deliberate: it keeps upstream code auditable, makes future syncs explicit, and avoids silently mixing general assistant code into Bicameral's domain runtime.

## Mapping

| ZeroClaw concept | Bicameral use |
|---|---|
| Gateway | Ingress for MCP, local dashboard, webhooks, integration callbacks, and mod runs |
| Runtime security/autonomy | Governance gates, approval requirements, workspace boundaries, and blocked authority bypasses |
| SOP engine | Deterministic review/governance workflows for PM/EM/dev handoffs |
| Tool receipts | Audit chain for ingest, bind, review command, and storage-adapter materialization |
| Provider abstraction | Model routing for extraction, binding explanation, and review assistance |
| Plugins/skills | EM-safe mods and integration setup wizards |
| Config schema | `.bicameral/config.*`, governance policy, source configuration, and workspace trust boundary |
| Gateway dashboard | Local review queue, Ingestion Gate, and Ledger View |

## Required Adaptation

The first extraction must narrow ZeroClaw's generic personal-assistant surface into Bicameral's software-team governance surface:

1. Rename runtime crates and command surface to Bicameral-owned names before publishing them as first-party code.
2. Keep `protocol/` as the local source of truth for Bicameral objects: `DecisionCandidate`, `SourceEvidence`, `BindingEvidence`, `ReviewCommand`, `GovernanceResult`, and canonical storage commands.
3. Remove or disable unrelated channel, hardware, broad memory, and general-assistant affordances from the initial public bot binary.
4. Replace generic autonomy language with governance language: agents and mods may emit candidates, hints, evidence, review commands, and advisory warnings; they may not directly create canonical authority.
5. Preserve ZeroClaw license notices for any copied source and keep attribution visible in `NOTICE` or equivalent release metadata.

## Authority Boundary

This ADR does not weaken the core Bicameral invariant:

```text
Edges are expressive.
Core is boring.
Governance decides what becomes canonical.
```

The imported runtime may help execute, observe, validate, and audit. It may not decide canonical decision state by itself.

Mods, tools, integrations, and agents must not directly write `.bicameral/decisions/*.yaml`, approve decisions, mark compliance as resolved, or create blocking results without governance policy and required human review.

## Consequences

Positive:

- Faster path to a working local daemon/gateway.
- Reviewable upstream boundary through a pinned submodule.
- Reuse of security, approval, gateway, config, and service ideas that are already aligned with the local-first thesis.
- Cleaner path for EM-safe mods: start from deterministic manifests/SOPs, then graduate to sandboxed executable plugins later.

Negative / risks:

- ZeroClaw is a general autonomous-assistant runtime; Bicameral must actively remove surface area that distracts from spec alignment.
- Submodule-based bootstrap is not the final product shape; first-party copied code must be selectively extracted, renamed, licensed, and tested.
- Provider/channel/tool abstractions can become too generic if they are not constrained by Bicameral protocol objects.

## Follow-up Plan

1. Add a thin `runtime/` extraction plan that identifies the minimal crates and modules to copy or rewrite.
2. Prototype `bicameral init`, `bicameral ingest`, `bicameral preflight`, `bicameral review`, `bicameral mod validate`, and `bicameral mod run` against fixtures.
3. Implement the first EM-safe mod path as declarative YAML + fixture runner before allowing arbitrary executable plugins.
4. Add tests that prove mods cannot directly materialize canonical decisions or blocking governance results.
5. Revisit this ADR after the first runnable bot spike and either adopt copied first-party crates or remove the submodule.
