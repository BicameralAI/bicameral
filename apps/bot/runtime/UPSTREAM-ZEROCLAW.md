# Upstream ZeroClaw Extraction Notes

## Source

- **Upstream repository:** https://github.com/zeroclaw-labs/zeroclaw
- **Pinned commit:** `cbf915d43a3c43116d63c122732942cf8782ff16`
- **License:** MIT OR Apache-2.0
- **Extraction date:** 2026-05-29

## What was copied and adapted

| Bicameral crate | Source ZeroClaw area | Adaptation |
|---|---|---|
| `bicameral-api` | `crates/zeroclaw-api/` | Narrowed to governance protocol types only. Removed: memory traits, channel types, media, VAD, peripherals, generic agent/provider APIs. Added: DecisionCandidate, SourceEvidence, BindingEvidence, ReviewCommand, GovernanceResult, EventStoreAdapter. |
| `bicameral-config` | `crates/zeroclaw-config/` | Replaced ZeroClaw config namespace with `.bicameral/` workspace shape. Removed: secrets management, autonomy levels, multi-agent config, platform detection, scattered types. Added: workspace discovery, substrate-neutral event store config, governance policy config. |
| `bicameral-runtime` | `crates/zeroclaw-runtime/src/daemon/`, `src/service/`, `src/approval/`, `src/sop/` | Kept daemon lifecycle and signal handling. Replaced approval manager with governance engine. Removed: SOP engine, tools, subagent, routines, process stats, observability backends. Added: event-store adapters (git, drive-folder, memory), governance evaluation. |
| `bicameral-gateway` | `crates/zeroclaw-gateway/` | Kept axum HTTP server structure. Replaced all routes with governance-specific endpoints: ingest, review, status, health. Removed: WebSocket, voice, canvas, TUI, WebAuthn, pairing, plugins API, static files, TLS, node tools. |
| `bicameral-mods` | `crates/zeroclaw-plugins/` | Replaced plugin system with EM-safe declarative mod manifests. Added: YAML manifest schema, trigger/filter validation, fixture runner. Removed: arbitrary code execution, dynamic loading, marketplace. |
| `bicameral-audit` | `crates/zeroclaw-runtime/src/security/audit.rs`, `src/sop/audit.rs` | Combined ZeroClaw's tool receipts and SOP audit into a hash-chained audit receipt store. Removed: security-specific audit (OTP, pairing, vulnerability), generic tool call logging. |

## What was intentionally excluded

- **Channels:** Telegram, Discord, Slack, WhatsApp, email adapters — out of scope for v0.1 governance runtime.
- **Hardware:** Robot kit, Aardvark, peripheral drivers — unrelated to software governance.
- **Memory:** RAG, vector store, conversation memory — not required for decision ledger.
- **TUI:** Terminal UI — may be re-added later for local dashboard.
- **Tools:** Generic tool execution, file operations, model switching — agents use MCP/external tools.
- **Providers:** LLM provider abstraction — extraction/binding uses external model routing.
- **Observability:** Prometheus, OTEL, Dora — may be re-added in future for runtime telemetry.
- **Security:** WebAuthn, OTP, pairing, sandboxing — Bicameral's trust boundary is governance policy, not execution sandbox.
- **Macros:** Procedural macros crate — not needed for v0.1.
- **Infra:** Infrastructure utilities — not needed.

## How future upstream syncs should be performed

1. Do **not** re-add the ZeroClaw submodule. The extraction is first-party.
2. To compare with upstream changes, clone ZeroClaw separately and diff the relevant source areas against the Bicameral crates.
3. Cherry-pick only changes that affect the narrow governance runtime surface (daemon lifecycle, config loading, HTTP serving).
4. Any imported change must be renamed and reviewed for authority boundary compliance before merging.
5. Document each sync in this file with date, upstream commit, and what was imported.

## Sync log

| Date | Upstream commit | What was synced |
|---|---|---|
| 2026-05-29 | `cbf915d43a3c43116d63c122732942cf8782ff16` | Initial selective extraction for v0.1 runtime skeleton. |
