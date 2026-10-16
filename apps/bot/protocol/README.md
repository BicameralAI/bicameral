# Bicameral Protocol

The typed wire contract for everything that enters Bicameral governance — evidence, candidates, decisions, review commands, and advisories. It is embedded in `bicameral-bot` because the bot is the **local authority boundary** where integrations, local tools, and cloud advisories converge.

The former standalone `bicameral-protocol` repository is deprecated; schemas, conformance fixtures, and compatibility notes live here. Machine-readable JSON Schemas (generated from the Rust types) are under [`schemas/`](schemas/README.md).

> **The rule.** Protocol objects are reviewable claims, evidence, commands, or advisories. They are **not** canonical authority until governance policy accepts them and an event-store adapter materializes the accepted event.

## Surface categories

The protocol may share object schemas across categories, but it **must not share caller authority.**

| Category | Contract | Who uses it | May carry authority? |
|---|---|---|---|
| **Local tool API** | `ToolRequest` / `ToolResponse` | MCP, CLI, TUI, dashboard | Yes — an enforced `AuthorityContext` |
| **External ingest** | `ExternalIngestEnvelope` | Jira, Fathom, Slack, Notion, GitHub, Linear, support, meetings | No — evidence only |
| **Egress** *(future)* | `Egress` | Bicameral publishing/notifying/syncing outward | n/a (outbound) |

External ingest must never emit review commands, canonical decisions, direct event-store writes, signoff approval, compliance resolution, local actor/session authority, policy grants, or blocking governance results.

## Core object families

- **Evidence & decisions** — `SourceEvidence`, `EvidenceReference`, `SourceKind`, `DecisionCandidate`, `Decision`, `TrackingState`
- **Binding & dependencies** — `BindingHint`, `BindingEvidence`, `DependencySignal`
- **Review & governance** — `ReviewCommand`, `ReviewEvent`, `ReviewState`, `GovernanceResult`
- **Grounding & conflict** — `ConflictSignal`, `GroundingSuggestion`
- **Local tool API** — `ToolRequest` (canonical local request, [ADR-0002](../docs/adr/0002-agent-surfaces-and-bot-runtime-interface.md), issue #103), `ToolResponse`, `AuthorityContext` (actor identity, auth method, policy scope, audit metadata), `ToolCommand` (the local command registry)
- **Multi-Product routing** — Product-scoped `ToolRequest` values carry an explicit optional `product_id`; the operator daemon resolves event-store and workspace-binding context through its registries and never infers Product identity from cwd
- **External ingest** — `ExternalIngestEnvelope` (evidence submission, separate from `ToolRequest`)
- **Workspace binding** — `WorkspaceBindingProposal` (transient MCP-authored bind proposal), `WorkspaceBindRequest` / `WorkspaceBindResponse` / `WorkspaceBindErrorResponse` (the `workspace.bind` contract), `WorkspaceBindingReadinessView` (hosted-safe projection). MCP proposes; the daemon validates/materializes. See [`docs/specs/workspace-binding-proposal-protocol.md`](../docs/specs/workspace-binding-proposal-protocol.md).

## ToolRequest contract (v2)

Per [ADR-0002](../docs/adr/0002-agent-surfaces-and-bot-runtime-interface.md) and issue #103, `ToolRequest` is the transport-neutral local action/query object. All local surfaces (MCP, CLI, TUI, dashboard) map their actions into a `ToolRequest` before the daemon validates and evaluates governance policy.

**MCP v0.2 direct payload compatibility is not preserved.** MCP clients must migrate to tool calls that map into canonical `ToolRequest` commands.

### Interface rule

```text
surface action
  → ToolRequest (command + AuthorityContext)
  → daemon validation and governance policy
  → ToolResponse (status + result + governance_result)
  → event-store adapter materialization, if accepted
```

### Local command registry

| Command | Description |
|---------|-------------|
| `ingest.submit_local` | Submit source evidence and/or decision candidates |
| `preflight.run` | Surface relevant decisions before implementation |
| `binding.create` | Propose binding evidence for a decision/candidate |
| `binding.inspect` | Inspect existing bindings |
| `review.accept_candidate` | Accept a candidate through governance |
| `review.reject_candidate` | Reject a candidate |
| `review.approve_signoff` | Approve signoff on a promoted decision (ownership approval — does not imply compliance or enforcement) |
| `review.reject_signoff` | Reject signoff on a promoted decision (ownership rejection — does not alter compliance state) |
| `review.resolve_compliance` | Resolve compliance state for a decision |
| `history.list` | Read replayed/materialized decision state |
| `search.query` | Search decisions, candidates, source excerpts, and bindings (read-only) |

#### `search.query` scopes

`search.query` supports the scopes `decisions`, `candidates`, `bindings`, and
`all`. All scopes read **bot-owned replayed/materialized state only** — never
MCP-owned ledger/search state.

The `bindings` scope returns accepted, bot-owned `BindingEvidence`. Each binding
result carries `binding_id`, the linked `decision_id`, `target` with
`target_kind`/`target_identifier`/`target_metadata`, `binding_type`, `graph_snapshot_id`,
`validated_sha`, `evidence_state`, `readiness_warnings`, an
`unknown_or_partial_scope` flag, and advisory `caveats`. Results are **read-only
recorded evidence** and never imply complete project truth, compliance, signoff,
no-conflict, verified binding state beyond the recorded evidence, or merge
safety. A binding whose `evidence_state` is not `verified`, or which lacks a
durable snapshot/commit anchor, is flagged as unknown/partial scope. Binding
results are kept distinct from approximate text/source search and are never
returned as a typed unsupported-capability answer.

### HTTP surface

The daemon serves the v2 ToolRequest contract over HTTP:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/v2/capabilities` | GET | Returns `CapabilityReport` (protocol version, supported/deferred commands, preflight stages) |
| `/v2/tool-requests` | POST | Accepts a `ToolRequest` body, dispatches through the canonical dispatcher, returns `ToolResponse` |
| `/health` | GET | Daemon health check |

### Port contract

The daemon default bind address is `127.0.0.1:37373`. The MCP thin client (`bicameral-mcp`) uses the same default via `BICAMERAL_DAEMON_URL`. Both sides accept environment-variable overrides:

- **Daemon**: `gateway.port` in `.bicameral/config.yaml`, or launch with `--workspace` pointing to a config with a custom port.
- **MCP**: `BICAMERAL_DAEMON_URL` or `BICAMERAL_BOT_DAEMON_URL` environment variable.

The canonical default port `37373` is the single source of truth. Consumers that need a different port must configure their side to match.

### External ingest separation

`ExternalIngestEnvelope` is **not** a `ToolRequest`. It enters the daemon through the external ingest gate and cannot carry review commands, authority context, tool commands, policy grants, canonical decisions, event-store writes, signoff approval, compliance resolution, or blocking governance results. See [`schemas/v2/fixtures/invalid-external-ingest-*.json`](schemas/README.md) for forbidden-path examples.

Separately distributed producers use the Bot-owned
[`external-ingest/contract.json`](external-ingest/contract.json) distribution
manifest. They must read `GET /api/external-ingest/capabilities`, match protocol
version, schema digest, and contract fingerprint exactly, then deliver to the
advertised endpoint. Protocol v2 requires a value-free redaction receipt and
fails closed on skew. The v1 endpoint is a temporary migration seam and is not
advertised as a supported distributed protocol.

## Provider acquisition — provisional alpha (#462, #405)

> **Provisional alpha.** `ProviderResourceDescriptor` and `ProviderItemEnvelope`
> are purpose-neutral provider-fact handoff objects. They are **not**
> `SourceBinding`, `Source`, `SourceSnapshot`, `SourceEvidence`,
> `DecisionCandidate`, `Decision`, review, signoff, compliance, or event-store
> authority. They occupy a separate protocol surface for cross-repo provider-fact
> handoff and must **not** be routed through `ToolRequest` or
> `ExternalIngestEnvelope`.

| Object | Purpose | What it is **not** |
|--------|---------|-------------------|
| `ProviderResourceDescriptor` | Provider-neutral resource discovery fact (identity, capabilities, permission/`action_needed` state, freshness) | Not a `SourceBinding`, setup completion, event-store config, or canonical Bicameral state |
| `ProviderItemEnvelope` | Screened provider item fact (fetched content with metadata) | Not accepted `Source`, `SourceSnapshot`, `SourceEvidence`, `DecisionCandidate`, or event-store mutation |

Provider acquisition objects enter downstream governance only after explicit
promotion through the existing `SourceBinding` approval path. See
[`schemas/v2/fixtures/invalid-provider-*-*.json`](schemas/README.md) for
forbidden-path examples (secret fields, local authority, review commands,
event-store writes, bot lifecycle terms).

Field vocabulary, canonical enum values, namespaced extension handling, and the
integrations-vs-bot ownership split are documented in
[`docs/specs/provider-acquisition-schema.md`](../docs/specs/provider-acquisition-schema.md)
(#523, BicameralAI/bicameral-integrations#173).

Rust types: [`crates/bicameral-api/src/provider_acquisition.rs`](../crates/bicameral-api/src/provider_acquisition.rs)

## Substrate-neutral contract

The Decision Ledger contract is substrate-neutral. Both Git and Drive/basic-store substrates satisfy the same baseline ledger purpose:

- Durable append of accepted governance events.
- Deterministic replay into materialized decision state.
- Health/freshness reporting.
- Audit trail preservation.

**Git** extends the baseline with PR-resolution capabilities (branch-local proposals, PR-native review, native merge-conflict resolution, and optionally CI/merge-gate enforcement). These capabilities are advertised separately and do not change the baseline protocol contract.

**Drive/basic-store** substrates satisfy the baseline without PR-resolution. Governance enforcement on these substrates surfaces through dashboard warnings, queued actions, or paused review states — not through CI blocking.

See [ADR-0001](../docs/adr/0001-local-daemon-gateway-and-event-store-substrates.md) and [`docs/SUBSTRATE-MIGRATION.md`](../docs/SUBSTRATE-MIGRATION.md) for details.

## See also

- [`schemas/`](schemas/README.md) — versioned JSON Schemas (v1 ingest, v2 ToolRequest) and conformance fixtures, generated from the Rust types.
- [Root README](../README.md) · [ADR-0002](../docs/adr/0002-agent-surfaces-and-bot-runtime-interface.md) · [ADR-0007](../docs/adr/0007-substrate-neutral-governance-flow.md)
