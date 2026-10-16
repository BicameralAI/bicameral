# ADR-0008: Repository Split and Protocol Transition

**Date:** 2026-05-27  
**Status:** proposed  
**Level:** L1  
**Carries forward:** `bicameral-daemon` ADR-0008 plus the original `bicameral-bot` protocol-folder ADR

## Problem

The original daemon repo bundled hosted code graph, ingestion adapters, protocol contracts, local runtime, dashboard backend, and governance concepts. The new public/private repo boundaries separate those responsibilities.

A separate `bicameral-protocol` repository also added coordination overhead before the protocol had independent release needs. The bot is now the local authority boundary all public edge surfaces enter.

## Decision

Use `bicameral-daemon` as private architecture staging and gold-standard documentation reference while code and docs migrate into target repos:

- `bicameral-bot` — public local daemon/gateway/runtime, local code grounding, review UX, storage-adapter materialization, embedded protocol, and EM governance skills.
- `bicameral-integrations` — public source adapters and EM-safe mods that emit candidates/evidence/signals.
- `bicameral-mcp` — public agent-facing MCP tools that emit protocol-shaped evidence, queries, and review commands.
- `bicameral-cloud` — private hosted services for code graph, conflict advisory, blast-radius analysis, grounding optimization, managed provider/API paths, plus lightweight hosted amenities such as encrypted rendezvous relay for settings pairing.
- `bicameral-daemon` — legacy/private architecture staging repo and gold-standard documentation reference until the split repos catch up.

Protocol contracts move into `bicameral-bot/protocol/`.

## Protocol Ownership

`bicameral-bot/protocol/` owns schemas, conformance fixtures, object vocabulary, and compatibility notes used by bot, MCP, integrations, and cloud clients.

This does not make integrations or MCP subpackages of the bot. They remain separate repos that depend on bot-owned protocol contracts.

The standalone `bicameral-protocol` repo should become a deprecated pointer unless and until an independent protocol release/conformance process becomes necessary.

## Non-Goals

This ADR does not delete the daemon ADR set. Existing daemon ADRs remain the most complete architecture reference until target repos catch up.

This ADR does not move hosted graph, conflict advisory behavior, managed provider/API paths, or hosted amenities into the public bot. The bot owns local grounding and runtime authority; cloud owns organization-scale advisory infrastructure and any lightweight hosted amenities that reduce setup friction.

Hosted amenities are not the same as hosted services. Services are heavyweight
paid value such as shared codegraph, organization-scale indexing, managed LLM/API
paths, and conflict intelligence. Amenities are generally available convenience
surfaces for local-first users, such as short-lived encrypted rendezvous relay.
Each amenity should have a local-only sharing alternative where feasible.

## Consequences

Future target-repo ADRs should follow the structure and terminology used by the daemon ADRs, but only cover decisions that repository actually owns. `bicameral-bot` becomes the natural home for shared public-local protocol objects. Repo boundaries are clearer: integrations produce evidence, MCP exposes tools, cloud advises and hosts amenities, bot governs and materializes through selected substrates.
