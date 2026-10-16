# Protocol Schemas

Versioned JSON Schemas for the Bicameral bot wire contracts — the canonical
reference for any client integrating with the bot's HTTP API.

> **Generated, not hand-written.** Schemas are produced from the Rust `serde`
> types via [`schemars`](https://graham.cool/schemars/), so they cannot drift
> from the implementation. A CI test in `bicameral-gateway` (`schema_gen`)
> verifies that the committed schemas still match the current types. Edit the
> Rust types, then [regenerate](#regenerating) — never edit `.schema.json` by hand.

## v1 — ingest

| File | Description |
|------|-------------|
| `v1/ingest-request.schema.json` | `POST /api/v1/ingest` request body |
| `v1/ingest-response.schema.json` | `POST /api/v1/ingest` response body |
| `v1/fixtures/clean-projection-ingest.json` | Minimal valid request with no `level` or `span_start`/`span_end` (clean adapter projection) |

### Compatibility notes

- **`level`** is optional. When omitted the bot infers it from `source_type`
  using the MCP #340 heuristic: product sources (`transcript`, `notion`,
  `slack`, `document`) → `product` (L1); implementation sources
  (`implementation_choice`, `agent_session`) → `implementation` (L3);
  everything else → `architecture` (L2). Explicit values are always honored.
- **`span_start` / `span_end`** on evidence items are optional. When omitted
  they default to `0` and the excerpt character length respectively, covering
  the full excerpt.
- **`evidence`** must contain at least one item (the `EmptyEvidence` rejection
  gate is unchanged).

## v2 — ToolRequest / ToolResponse / ExternalIngestEnvelope ([ADR-0002](../../docs/adr/0002-agent-surfaces-and-bot-runtime-interface.md), issue #103)

| File | Description |
|------|-------------|
| `v2/tool-request.schema.json` | `ToolRequest` — canonical local tool API request |
| `v2/tool-response.schema.json` | `ToolResponse` — canonical local tool API response |
| `v2/external-ingest-request.schema.json` | `ExternalIngestEnvelope` — strict distributed adapter request (`POST /api/v2/external-ingest`); requires a redaction receipt |
| `v2/external-ingest-response.schema.json` | `ExternalIngestResponse` — external adapter ingest response |

### Provider acquisition — provisional alpha (#462, #405)

> **Provisional alpha.** These schemas describe provider-fact handoff objects,
> not `SourceBinding`, `Source`, evidence, candidate, review, signoff,
> compliance, or event-store authority. They occupy a separate protocol surface
> and must not be routed through `ToolRequest` or `ExternalIngestEnvelope`.

| File | Description |
|------|-------------|
| `v2/provider-resource-descriptor.schema.json` | `ProviderResourceDescriptor` — provider-neutral resource discovery fact |
| `v2/provider-item-envelope.schema.json` | `ProviderItemEnvelope` — screened provider item fact |

Field vocabulary, canonical enum values (`ResourceCapability`, `PermissionState`,
`ActionNeededReason`), extension handling, and the integrations-vs-bot ownership
split are documented in
[`docs/specs/provider-acquisition-schema.md`](../../docs/specs/provider-acquisition-schema.md)
(#523).

See [`v2/fixtures/`](v2/fixtures/) for valid and invalid fixture examples.

### External ingest canonical field mapping

Before delivery, a separately distributed producer must match the Bot capability
report at `GET /api/external-ingest/capabilities` against
[`../external-ingest/contract.json`](../external-ingest/contract.json). A
version, schema-digest, or fingerprint mismatch is a fail-closed delivery state,
not permission to downgrade automatically.

External integrations **must** use the exact field names below. Common aliases
are **not** accepted and will produce an `invalid_payload` rejection with an
actionable hint naming the correct field.

| Wrong (alias) | Correct (canonical) | Notes |
|---------------|---------------------|-------|
| `source_ref` | `source_uri` | Provenance URI for the source artifact |
| `body` | `content` | Full captured content of the source |
| `text` | `content` | Use `content` for the captured text |
| `description` | `content` | Use `content`; `description` is not an envelope field |
| `source_url` | `source_uri` | Use `source_uri` for the provenance link |
| `evidence[].title` | `evidence[].excerpt` | Evidence items carry an `excerpt`, not a `title` |
| `evidence[].body` | `evidence[].excerpt` | Evidence items carry an `excerpt`, not a `body` |

### Conformance fixtures (`v2/fixtures/`)

Fixtures ship in three flavors so positive, negative, and alias-rejection paths are all pinned:

- **Valid mappings** — `mcp-to-tool-request-{ingest,review,preflight}.json`: MCP tool calls mapped to canonical `ToolRequest` commands.
- **Valid external ingest** — `valid-external-ingest-minimal.json`, `valid-external-ingest-candidate-hints.json`: accepted `ExternalIngestEnvelope` payloads demonstrating the correct field contract.
- **Invalid (must be rejected)** — `invalid-external-ingest-*.json`: an `ExternalIngestEnvelope` attempting a forbidden path (review command, authority injection, compliance resolution, role impersonation, injection canary, private key, sensitive data, **common alias fields**). These encode the authority boundary and field contract as test inputs the daemon must refuse.
- **Binding lineage** — `binding-lineage-*.json`: cross-snapshot revalidation and lineage cases.
- **Valid provider acquisition** — `valid-provider-resource-descriptor-{minimal,github,linear,google-drive,action-needed}.json`, `valid-provider-item-envelope-{minimal,github,linear,google-drive}.json`: accepted provider-fact shapes covering ≥3 providers plus the `action_needed` permission branch.
- **Invalid provider acquisition** — `invalid-provider-*-*.json`: provider-fact payloads attempting forbidden authority paths (secret fields, local actor authority, review commands, event-store writes, bot lifecycle terms).

See [`v2/fixtures/`](v2/fixtures/) for the full set.

### Compatibility notes — v2 vs v1

**MCP v0.2 direct payload compatibility is not preserved.** MCP clients must
migrate to tool calls that map into canonical `ToolRequest` commands. The v1
ingest schemas remain for backward-compatible ingest-only flows; all other MCP
actions (preflight, bind, review, history, search) use the v2 `ToolRequest`
contract.

## Regenerating

After changing the `IngestRequest` / `IngestResponse` / `ExternalIngestEnvelope`
/ `ExternalIngestResponse` / `ProviderResourceDescriptor` /
`ProviderItemEnvelope` structs:

```bash
WRITE_SCHEMAS=1 cargo test -p bicameral-gateway --test schema_gen
```

Then commit the updated `.schema.json` files.

## See also

- [`protocol/README.md`](../README.md) — the protocol contract and surface categories.
- [Root README](../../README.md) · [ADR-0002](../../docs/adr/0002-agent-surfaces-and-bot-runtime-interface.md)
