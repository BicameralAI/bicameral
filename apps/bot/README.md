<p align="center">
  <h1 align="center">Bicameral Bot</h1>
</p>

<p align="center">
  <strong>The local daemon at your team's trust boundary — it captures implementation-constraining decisions from the tools you already use, grounds them in your actual code, and routes ambiguity to a human <em>before</em> any claim becomes authority.</strong>
</p>

<p align="center">
  <a href="https://github.com/BicameralAI/bicameral-bot">bot</a>&nbsp; • &nbsp;
  <a href="https://github.com/BicameralAI/bicameral-mcp">mcp</a>&nbsp; • &nbsp;
  <a href="https://github.com/BicameralAI/bicameral-integrations">integrations</a>&nbsp; • &nbsp;
  <a href="https://github.com/BicameralAI/bicameral-cloud">cloud</a>
</p>

<p align="center">
  <!-- CI / release posture -->
  <a href="https://github.com/BicameralAI/bicameral-bot/actions/workflows/rust.yml"><img src="https://github.com/BicameralAI/bicameral-bot/actions/workflows/rust.yml/badge.svg" alt="Rust CI"></a>
  <a href="https://github.com/BicameralAI/bicameral-bot/actions/workflows/factory-attestation.yml"><img src="https://github.com/BicameralAI/bicameral-bot/actions/workflows/factory-attestation.yml/badge.svg" alt="Factory Attestation"></a>
  <a href="https://github.com/BicameralAI/bicameral-bot/actions/workflows/release.yml"><img src="https://github.com/BicameralAI/bicameral-bot/actions/workflows/release.yml/badge.svg" alt="Release"></a>
</p>

<p align="center">
  <!-- project signals -->
  <img src="https://img.shields.io/badge/rust-1.75%2B-orange?logo=rust&logoColor=white" alt="Rust 1.75+">
  <img src="https://img.shields.io/badge/protocol-v2%20(ToolRequest)-blue" alt="Protocol v2">
  <img src="https://img.shields.io/badge/grounding-local%20(tree--sitter)-2ea44f" alt="Local grounding">
  <a href="#license"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg" alt="License: MIT OR Apache-2.0"></a>
</p>

---

**Bicameral Bot** is the open-source **local daemon, gateway, and protocol** for [Bicameral](https://github.com/BicameralAI). It is the *boring middle* of the system: it validates typed objects, keeps review state, applies governance policy, grounds claims in your local code, and materializes accepted events through a storage adapter. The expressive edge — Jira, Linear, Slack, meetings, MCP sessions, and explicitly approved source bindings — feeds it evidence. **Edge surfaces propose; the daemon decides what becomes canonical.**

| | |
|---|---|
| **Maturity** | Alpha — the daemon, gateway, protocol v2, and local Rust grounding are implemented and CI-gated. Canonical [`ToolRequest`](protocol/README.md) dispatch with enforced `AuthorityContext` has landed; the public install flow and source integrations are maturing. |
| **Footprint** | A single Rust workspace — six crates plus the `bicameral` binary. No runtime services required beyond the local daemon; storage is a pluggable event-store adapter (git / drive / in-memory). |
| **Safety model** | The daemon owns authority. Every local action enters as a `ToolRequest` carrying an **enforced** `AuthorityContext`; external/source evidence enters through a **separate** `ExternalIngestEnvelope` gate that can never emit review commands or canonical writes ([ADR-0002](docs/adr/0002-agent-surfaces-and-bot-runtime-interface.md), [ADR-0007](docs/adr/0007-substrate-neutral-governance-flow.md)). Human review is the mechanism that keeps probabilistic interpretation from becoming unreviewed authority. |
| **Assurance** | Rust CI (fmt, `clippy -D warnings`, full-workspace tests) on every push/PR; a factory-attestation gate on PRs; wire schemas generated from the Rust types (`schemars`) with a CI drift guard. |

## Why Bicameral Bot

Software teams do not usually lose because nobody wrote anything down. They lose because product intent, architectural constraints, dependency risk, and implementation evidence drift apart while work is happening.

The decision is in a Jira ticket. The real rationale is in a meeting. The dependency risk is in an EM's head. The implementation evidence is in a branch. The agent coding the feature sees only a slice of that context, then confidently proceeds.

That is **cognitive debt**: unresolved interpretation work accumulating until the team repays it through rework, surprise reviews, stale specs, phantom blockers, or yet another meeting to reconstruct what everyone meant.

Bicameral Bot works backwards from that failure mode. It is a local agent that sits at the team's trust boundary, captures implementation-constraining decisions from operational sources, grounds them in the current code workspace, and routes ambiguity to the right human before any claim becomes authority.

## The Shape

Bicameral Bot follows a local **daemon + gateway** shape:

- The **gateway is expressive.** It meets teams where their cognitive debt already lives: Jira, Linear, Slack, meetings, support threads, ADRs, docs, repos, EM-authored mods, and explicitly approved source bindings.
- The **daemon is boring.** It validates typed objects, keeps review state, applies governance policy, preserves audit trails, grounds claims in local code, and materializes accepted events through storage adapters.

That split is the product thesis:

```text
Expressiveness at the edge.
Determinism in the middle.
Governance decides what becomes canonical.
```

## Design Principles

- **Grounded, not just documented.** Decisions are linked to source excerpts, code paths, diffs, symbols, dependencies, and review events — not filed as prose. Local code grounding is exact and snapshot-scoped ([ADR-0010](docs/adr/0010-snapshot-scoped-local-code-graph-contract.md), [ADR-0012](docs/adr/0012-rust-symbolresolver-boundary-and-followups.md)).
- **Authority is enforced, not asserted.** Every local action is a `ToolRequest` with an authenticated, capability-checked `AuthorityContext`; canonical mutations are refused without one. External evidence enters through a separate gate that cannot cross into authority ([ADR-0002](docs/adr/0002-agent-surfaces-and-bot-runtime-interface.md)).
- **Local first by default.** The bot runs at the team's trust boundary and produces useful `BindingEvidence` against the current repo/worktree. Paid cloud adds organization-scale graph intelligence; it does not replace local authority ([ADR-0003](docs/adr/0003-local-code-grounding-and-cloud-conflict-boundary.md)).
- **HITL is debt control.** Human review is not a fallback for model failure; it is the mechanism that prevents probabilistic interpretation from becoming unreviewed authority ([ADR-0004](docs/adr/0004-hitl-boundaries-for-probabilistic-governance.md)).
- **Separate the confidence surfaces.** *Extraction* ("is this a decision?"), *binding* ("is this the right code evidence?"), and *compliance* ("does the implementation satisfy the decision?") are distinct scores. Collapsing them creates cognitive debt.
- **Predictable beats clever.** Canonical decisions are replayable, reviewable, and auditable. The Decision Ledger contract is substrate-neutral; git is the boring default for repo-centric teams ([ADR-0001](docs/adr/0001-local-daemon-gateway-and-event-store-substrates.md), [ADR-0007](docs/adr/0007-substrate-neutral-governance-flow.md)).
- **Mods emit evidence, not authority.** EM-authored mods can create candidates, dependency signals, binding hints, and advisories — never canonical decisions, signoff approvals, compliance resolutions, or blocking CI results.

## Key Features

- **Canonical local tool API** — every surface (MCP, CLI, TUI, dashboard) maps its actions to a typed [`ToolRequest`](protocol/README.md); the daemon dispatches it through authority enforcement, policy, read models, and event-store materialization.
- **Decision capture** — turn tickets, meeting excerpts, Slack threads, explicitly approved source bindings, and agent sessions into reviewable `DecisionCandidate` objects.
- **Local code grounding** — inspect the current workspace with a tree-sitter Rust `SymbolResolver` to produce snapshot-scoped, exact `BindingEvidence`.
- **Preflight** — surface relevant decisions to agents and developers before implementation goes too deep.
- **Review routing & state** — drive candidates and decisions through accept/reject, signoff, and compliance transitions with idempotency and audit.
- **Decision Ledger** — materialize reviewed authority through a pluggable event-store adapter (git / drive / in-memory), with deterministic replay.
- **Gateway integrations** — accept typed objects from [`bicameral-integrations`](https://github.com/BicameralAI/bicameral-integrations), MCP sessions, and manual fixtures.
- **Cloud advisory mode** *(planned)* — optionally query [Bicameral Cloud](https://github.com/BicameralAI/bicameral-cloud) for cross-branch, cross-repo, and historical code-graph intelligence.

## Quick Start

> **New here?** See [`QUICKSTART.md`](QUICKSTART.md) for the executable Linux/macOS alpha walkthrough from install through a restart-persistent reviewed Decision.

### Public install

The current alpha release path supports Linux and macOS. Windows packaging, self-update, and user-level service installation are not yet shipped.

```bash
curl -fsSL https://install.bicameral-ai.com/install.sh | sh
bicameral --version
```

The install script downloads the matching archive, verifies the SHA-256 checksum, and places the `bicameral` binary on your PATH. Then initialize and verify a workspace:

```bash
cd /path/to/your-repo
bicameral init
bicameral doctor
bicameral service start
```

Optionally open the Cloud-hosted dashboard and pair it with the local daemon to
watch state as a read-only projection. Developers can use the disposable local
Cloud topology from `bicameral-cloud#135`; the daemon does not host SPA assets
or fixture data.

Follow [`QUICKSTART.md`](QUICKSTART.md) for the exact source-backed ingest
payload and the alpha first-Decision path: an MCP RecallPacket surfaces the
`DecisionCandidate`, the operator promotes it with an explicit single-use
confirmation, and the daemon validates and materializes the accepted Decision
through the event store. `bicameral review submit` evaluates governance
eligibility but does not promote a candidate into the Decision Ledger. Promoting
from the dashboard Activity / Ingestion Gate is deferred for alpha (not
prohibited): a future Tool Surface may submit the same daemon-authorized
request, but the daemon stays the single promotion authority.

Alpha note: the public installer requires versioned platform tarballs,
`SHA256SUMS`, and `latest.json` published under
`https://pub-7eab94b264754bee8019fd89ca9b5b0b.r2.dev/bicameral`. If no release is published yet,
use the developer install path below.

### Developer install

Build from source (requires Rust 1.75+ and Node.js 22):

```bash
git clone https://github.com/BicameralAI/bicameral-bot.git
cd bicameral-bot
cargo build --release --locked
npm ci --prefix dashboard      # Cloud-vendored SPA source
npm run build --prefix dashboard
cargo test --workspace         # run the full test suite
```

The local Quickstart is an operator walkthrough, not release journey evidence.
Release Units that require end-to-end validation use their exact assigned
JourneyRevision topology and receipt.

The important promise is not the exact command spelling. It is the operating model: **install locally, connect only the sources you want, let agents propose candidates/evidence, and require governance before canonical state changes.**

## High-level Architecture

```text
External sources / agents / mods
Jira  Linear  Slack  meetings  support  MCP sessions  approved source bindings
        │
        ▼  ExternalIngestEnvelope (evidence)   /   ToolRequest (local actions)
┌──────────────────────────────────────────────────────────────┐
│ bicameral-bot                                                  │
│                                                                │
│  ┌─────────────┐     ┌────────────────────────────────────┐   │
│  │ Gateway     │────▶│ Daemon / core                      │   │
│  │ (HTTP edge) │     │ ToolRequest dispatch · validation  │   │
│  └─────────────┘     │ governance policy · audit          │   │
│                      └───────────────┬────────────────────┘   │
│  ┌─────────────┐     ┌───────────────▼────────────────────┐   │
│  │ protocol/   │◀───▶│ Local grounding · Review state     │   │
│  │ schemas (v2)│     │ SymbolResolver · BindingEvidence   │   │
│  └─────────────┘     └───────────────┬────────────────────┘   │
│                         ┌────────────▼─────────────┐          │
│                         │ Event-store adapter       │          │
│                         │ git · drive · in-memory   │          │
│                         └───────────────────────────┘          │
└──────────────────────────────────────────────────────────────┘
          │ optional advisory query
          ▼
 Bicameral Cloud: hosted code graph / conflict oracle  (planned)
```

The **local bot** owns gateway intake, protocol validation, review state, governance policy, local code grounding, Decision Ledger materialization, and the audit trail. The **cloud** (planned) owns the hosted code graph, cross-branch/cross-repo analysis, and conflict prediction — it returns evidence and advisories; local/team governance decides enforcement.

## Repository Layout

```text
├── crates/                  # Rust workspace
│   ├── bicameral-api/       # Protocol types: candidates, evidence, ToolRequest, governance
│   ├── bicameral-runtime/   # Daemon, governance engine, event-store adapters, grounding, preflight
│   ├── bicameral-gateway/   # Local HTTP gateway: ingest, ToolRequest dispatch, dashboard API
│   ├── bicameral-config/    # Workspace-scoped configuration
│   ├── bicameral-mods/      # EM-safe advisory mod manifests and fixture runs
│   └── bicameral-audit/     # Audit receipts and the audit store
├── src/                     # The `bicameral` binary (CLI entry point)
├── dashboard/               # Local review dashboard (Ingestion Gate + Ledger View)
├── protocol/                # Shared wire schemas and conformance fixtures (see protocol/README.md)
│   └── schemas/             # Versioned JSON Schemas (v1 ingest, v2 ToolRequest), generated from Rust
├── runtime/                 # Legacy ZeroClaw-scaffold notes (the runtime now lives in crates/, see runtime/README.md)
├── docs/                    # ADRs, specs, plans, research, reviews
├── tests/                   # Workspace-level integration tests
├── scripts/                 # CI and tooling helpers
├── CONTEXT.md               # Project glossary and resolved terms
└── README.md                # You are here
```

## Protocol Contracts

The protocol lives in this repo under [`protocol/`](protocol/README.md) rather than a separate repository: the bot is the local authority boundary that every public edge surface enters, so it owns protocol compatibility. Schemas are **generated from the Rust types** (`schemars`) and CI-guarded against drift — see [`protocol/schemas/`](protocol/schemas/README.md).

Two surface categories share object schemas but **never** share caller authority:

- **`ToolRequest` / `ToolResponse`** — the canonical local tool API (MCP/CLI/TUI/dashboard → `ToolRequest` → daemon validation → governance → materialization), carrying an enforced `AuthorityContext`.
- **`ExternalIngestEnvelope`** — external/source-system evidence submission; it cannot emit review commands, authority context, canonical decisions, signoff/compliance resolution, or blocking governance results.

Core object families: `SourceEvidence`, `DecisionCandidate`, `Decision`, `BindingHint`, `BindingEvidence`, `DependencySignal`, `ReviewCommand`, `ReviewEvent`, `ReviewState`, `GovernanceResult`, `ConflictSignal`, `GroundingSuggestion`. See [`protocol/README.md`](protocol/README.md) for the full contract.

## Testing

```bash
cargo test --workspace --locked        # full suite (api, runtime, gateway, mods, audit)
cargo clippy --workspace -- -D warnings # lint gate (warnings are errors)
cargo fmt --all -- --check             # formatting gate
```

After changing the `IngestRequest` / `IngestResponse` types, regenerate the committed wire schemas:

```bash
WRITE_SCHEMAS=1 cargo test -p bicameral-gateway --test schema_gen
```

## CI Gates

Every push and pull request to `dev` / `main` runs:

- **Rust CI** (`.github/workflows/rust.yml`) — `cargo fmt --check`, `cargo build --workspace --locked`, `cargo test --workspace --locked`, and `cargo clippy --workspace -- -D warnings`.
- **Bicameral factory attestation** (`.github/workflows/factory-attestation.yml`) — governance/provenance attestation on PRs.
- **Release** (`.github/workflows/release.yml`) — builds and publishes signed release artifacts on `v*` tags.

Generic journey simulations are not required merge gates. A topology-applicable
Release Unit binds its end-to-end claim to the exact assigned JourneyRevision
and immutable topology receipt.

## Related Repositories

- [`bicameral-mcp`](https://github.com/BicameralAI/bicameral-mcp) — agent-facing MCP tool surface; maps tool calls to canonical `ToolRequest` commands. Emits into the bot; holds no local authority.
- [`bicameral-integrations`](https://github.com/BicameralAI/bicameral-integrations) — read-only source adapters and EM-safe mods. Emits candidates/evidence/signals into the gateway.
- [`bicameral-cloud`](https://github.com/BicameralAI/bicameral-cloud) — private hosted services for code graph, conflict oracle, managed provider/API paths, plus lightweight hosted amenities such as encrypted rendezvous relay. Returns evidence/advisories and setup conveniences, not authority.

## Documentation

- **Getting started**: [`QUICKSTART.md`](QUICKSTART.md) — install, init, daemon, MCP-promoted first Decision
- **Architecture & decisions**: [ADRs](docs/adr/) — incl. [0001 substrates](docs/adr/0001-local-daemon-gateway-and-event-store-substrates.md), [0002 agent surfaces / ToolRequest](docs/adr/0002-agent-surfaces-and-bot-runtime-interface.md), [0007 substrate-neutral governance](docs/adr/0007-substrate-neutral-governance-flow.md), [0010 snapshot-scoped graph contract](docs/adr/0010-snapshot-scoped-local-code-graph-contract.md), [0012 Rust SymbolResolver boundary](docs/adr/0012-rust-symbolresolver-boundary-and-followups.md)
- **Specs**: [MCP integration v0.2](docs/specs/mcp-integration-v02.md) · [Enforcement Memory Projection](docs/specs/enforcement-memory-projection.md) · [Substrate migration](docs/SUBSTRATE-MIGRATION.md)
- **Protocol**: [`protocol/`](protocol/README.md) · [`protocol/schemas/`](protocol/schemas/README.md)
- **Context**: [`CONTEXT.md`](CONTEXT.md) (glossary and resolved terms)
- **Governance**: [`GOVERNANCE.md`](GOVERNANCE.md) · [`docs/governance/BOUNDARY.md`](docs/governance/BOUNDARY.md) · [`docs/governance/SIBLINGS.md`](docs/governance/SIBLINGS.md)

## Project Governance

- **Contributing**: [`CONTRIBUTING.md`](CONTRIBUTING.md) — including *Bring your own tools* (the sibling registry: use any local tooling, registered and leak-guarded).
- **Governance model**: [`GOVERNANCE.md`](GOVERNANCE.md) and the enforced [`docs/governance/BOUNDARY.md`](docs/governance/BOUNDARY.md) / [`docs/governance/SIBLINGS.md`](docs/governance/SIBLINGS.md).
- **Security**: [`SECURITY.md`](SECURITY.md) · **Conduct**: [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md) · **Changes**: [`CHANGELOG.md`](CHANGELOG.md).

> Local-process and sibling-tool artifacts (Qor-logic's S.H.I.E.L.D. set, research, plans, reviews, any sibling's scratch) stay local and gitignored by design — see `BOUNDARY.md`.

## Governed by the Bicameral Factory

This repository is part of the BicameralAI organization and is governed by the [Bicameral Factory](https://github.com/BicameralAI/bicameral-factory), the org's governance control plane. The factory aggregates each repo's declared governance facts (classification, visibility, required checks) into an org-wide governance dashboard. Governance is evidence-first: status is read from hard evidence such as CI runs, file presence, and branch protection, never inferred.

## Trust & Compliance

BicameralAI is pursuing SOC 2 Type II and ISO 27001. Framework-level status and evidence are available through our [Vanta Trust Center](https://app.vanta.com/bicameral-ai.com/trust/g4wnw551zp5l8jr88ig70); access to detailed reports is provided under a non-disclosure agreement. This summary is framework-level by design; detailed control and test posture is maintained internally and is not published here.

## FAQ

**Is Bicameral Bot just an ADR generator?**
No. ADRs are one possible output. The core product is a grounded governance loop: evidence → candidate → review → materialization → preflight/enforcement/advisory feedback.

**Why local first?**
The most sensitive context is local: code, branches, customer details, Slack excerpts, support threads, implementation plans. A local bot lets teams start with useful grounding without outsourcing authority to a hosted control plane.

**Why daemon + gateway?**
Every team's edge is different, but authority must be predictable. The gateway can be customized aggressively; the daemon/core stays boring, typed, audited, and deterministic.

**What is paid, then?**
Hosted services are paid scale and optimization: organization-wide codegraph,
cross-branch/cross-repo conflict detection, historical indexing, blast-radius
analysis, better grounding recall/latency, and managed provider/API paths.
Hosted amenities such as encrypted rendezvous relay are setup conveniences, not
the paid product boundary. Free/local Bicameral remains code-grounded, and
amenities should keep a local-only sharing alternative where feasible.

**Can a mod block a merge?**
Not directly. A mod can emit a warning or dependency signal. Blocking requires governance policy, reviewed evidence, and a substrate that can honestly enforce the result.

## License

Licensed under the [Apache License 2.0](../../LICENSE), the license of the Bicameral repository. See [`NOTICE`](../../NOTICE) for third-party attribution (incl. the ZeroClaw runtime scaffold, [ADR-0009](docs/adr/0009-adopt-zeroclaw-runtime-scaffold.md)).
