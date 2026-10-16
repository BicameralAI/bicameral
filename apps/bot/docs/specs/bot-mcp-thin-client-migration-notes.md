# Bot-MCP Thin-Client Migration Notes

Date: 2026-06-12

Status: Conformance migration notes (issue #265)

Scope: Records the terminal bot-side verification that a thin MCP client can map
its expected tool calls to canonical `ToolRequest` commands and receive usable
`ToolResponse` results from the bot daemon, with **no MCP-owned authority
fallback**. Companion to `docs/specs/bot-mcp-data-flow-runtime-architecture.md`
(ADR-0002) and the conformance suite
`crates/bicameral-runtime/tests/bot_mcp_thin_client_conformance.rs`.

These notes identify the MCP authority surfaces that were removed (or were never
permitted to exist in the bot runtime) and the canonical bot replacement that now
owns each concern. They are the bot-side input the MCP repo needs to finish its
own cutover (BicameralAI/bicameral-mcp#554).

## Thin-Client Contract

The MCP server is a thin local surface: stdio transport, `list_tools`, daemon
handshake, tool-call mapping, `AuthorityContext` assembly, and response
formatting. It owns local surface compatibility and request shaping only. Every
canonical state concern lives behind the bot-owned `ToolRequest -> ToolResponse`
dispatcher.

Startup is fail-closed: the client negotiates the daemon protocol version during
the capability handshake and **refuses to start** against an incompatible daemon
rather than falling back to legacy MCP-owned handlers. After a compatible
handshake, individual unsupported commands return typed daemon capability errors
(`unsupported_capability`) — they are never silently absorbed by a local handler.

## Multi-Product request routing

For the registry-backed multi-Product alpha daemon, the thin client places its
explicitly selected canonical Product id in `ToolRequest.product_id`.
`AuthorityContext.workspace` remains local surface/audit context and must not
be used as Product identity. Switching Products changes request routing without
restarting the daemon.

## Removed MCP Authority Surfaces → Canonical Bot Replacement

| Removed MCP-owned surface | Why it cannot live in MCP | Canonical bot replacement |
| --- | --- | --- |
| MCP-owned Decision ledger / direct ledger writes | Canonical state is materialized only by accepted events replayed through the event store (ADR-0007) | `EventStoreAdapter` append + replay behind `ingest.submit_local` and `review.*` dispatch |
| MCP graph verification / `GraphEvidenceState::verified` emission | Only bot validation may produce verified evidence; advisory graph state must not masquerade as verified | Bot graph/snapshot/binding services via `binding.create` (graph snapshot resolution + symbol validation) |
| MCP binding materialization | Verified `BindingEvidence` requires bot-owned validation; token acceptance is deferred | `binding.create` (materializes only on accepted verification) / `binding.inspect` (read-only projection) |
| MCP governance acceptance (review/signoff/compliance) | Acceptance and signoff are governance-owned canonical transitions | `review.accept_candidate`, `review.reject_candidate`, `review.approve_signoff`, `review.reject_signoff` |
| MCP source adapter runtime | Source adapters enter through a bot-owned gateway, not local `ToolRequest` authority | Bot external gateway `ExternalIngestEnvelope` (evidence-only); local evidence enters via `ingest.submit_local` |
| MCP-local event store / operational receipts as authority | Caches, telemetry, and receipts explain work but cannot establish authority | `RuntimeTrace` operational receipt attached to every `ToolResponse` (correlation only) |
| MCP compliance resolution | No V1 compliance enforcement | `review.resolve_compliance` returns typed `unsupported_capability` (deferred) |

## Authority Boundary Preserved by Verification

- **Edges propose; the daemon decides.** `preflight.run`, `search.query`,
  `history.list`, and `binding.inspect` are read-only and write nothing. Ingest
  and review enter as `ToolRequest` with an enforced `AuthorityContext`.
- **External evidence is gated.** External-source payloads routed through local
  ingest are rejected (`external_adapter_payload`); they must use the bot-owned
  external gateway.
- **Legacy shapes are rejected, not absorbed.** v0.2-style payloads
  (`mcp_version` / `direct_ledger_write` metadata) are rejected
  (`legacy_mcp_payload`) and append no event — proving there is no legacy
  direct-ledger fallback.
- **Every command is typed.** The capability surface partitions every lifecycle
  command into supported or deferred; there is no untyped third state for a thin
  client to fall back on.

## Conformance Evidence

`crates/bicameral-runtime/tests/bot_mcp_thin_client_conformance.rs` proves, per
issue #265 acceptance criteria:

1. **Handshake** — compatible daemon protocol allows startup; incompatible
   (older or unknown future) protocol is refused
   (`handshake_succeeds_for_compatible_daemon`,
   `handshake_refused_for_incompatible_daemon_protocol`).
2. **Tool mapping** — each MCP tool name maps to canonical commands for ingest,
   preflight, binding, review, search, and history; routing is proved via the
   `RuntimeTrace.command` field (`mcp_tool_names_map_to_canonical_commands`,
   `read_and_ingest_paths_return_usable_results`,
   `binding_create_routes_to_bot_graph_handler`,
   `binding_inspect_is_read_only_and_routes_to_bot`,
   `ingest_then_accept_is_accepted_v1_path_without_fallback`).
3. **Typed unsupported** — deferred commands return `unsupported_capability`
   (`deferred_commands_return_typed_unsupported`).
4. **No legacy fallback** — legacy and external-adapter payloads are rejected
   with typed reasons and append no canonical event
   (`legacy_mcp_payload_is_rejected_not_accepted`,
   `external_adapter_payload_does_not_borrow_local_authority`).
5. **Registry completeness** — supported and deferred sets are disjoint and
   cover every lifecycle command
   (`capability_surface_partitions_every_lifecycle_command`).
6. **Trace correlation** — accepted and rejected paths both carry a correlated
   `RuntimeTrace` (`runtime_trace_correlates_accepted_and_rejected_paths`).

## Testability Gaps

- Full MCP-package conformance (the MCP server's own thin-client cutover) remains
  tracked in BicameralAI/bicameral-mcp#554. These notes and tests are the
  bot-side half of the contract; they do not exercise the MCP stdio transport.
- Verified binding materialization is exercised under a real git workspace in
  `crates/bicameral-runtime/tests/binding_dispatch_conformance.rs` (#262). The
  #265 suite asserts canonical routing of `binding.create` without re-deriving a
  workspace, reporting the typed `graph_unavailable` daemon response instead.
