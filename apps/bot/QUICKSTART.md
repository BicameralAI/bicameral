# Quickstart

Get from zero to your first reviewed Decision in under 15 minutes.

> **Alpha note:** Bicameral Bot is in early alpha. The public binary and user-level
> service path currently support Linux and macOS. Windows packaging, self-update,
> and service installation are not part of the current alpha release.

## 1. Install

Download the prebuilt binary for Linux or macOS. No Rust, Node, or Cargo is
required:

```bash
curl -fsSL https://install.bicameral-ai.com/install.sh | sh
```

The script detects the supported OS and architecture, downloads the matching
tarball, verifies its SHA-256 checksum, and places the `bicameral` binary on your
PATH.

Confirm the install:

```bash
bicameral --version
```

> **Developer path:** If no release tarball is published yet, build the daemon
> with `cargo build --release --locked`. Use `target/release/bicameral` in place
> of `bicameral` below. The SPA is built and served by Bicameral Cloud.

## 2. Initialize a workspace

Open a terminal in the repository or directory you want to govern:

```bash
cd /path/to/your-repo
bicameral init
```

This creates a `.bicameral/` directory with the default configuration, an empty
candidate inbox, an event-store root, and an audit root. The workspace is ready
for local governance. No hosted service or account is required.

## 3. Run diagnostics

Verify that the workspace and event-store substrate are healthy:

```bash
bicameral doctor
```

Doctor prints the detected substrate kind, capabilities, and any warnings. Fix
anything flagged before continuing.

## 4. Ingest your first candidate

Create a small JSON file containing the proposed claim and the source evidence
that supports it:

```bash
cat > my-decision.json <<'EOF'
{
  "title": "Use PostgreSQL for the accounts service",
  "description": "The accounts service will use PostgreSQL as its primary data store for transactional consistency.",
  "level": "architecture",
  "source": "quickstart://postgres-accounts-decision",
  "source_type": "manual",
  "label": "Quickstart architecture decision",
  "snapshot_content": "Architecture review: The accounts service will use PostgreSQL as its primary data store for transactional consistency.",
  "evidence": [
    {
      "span_start": 0,
      "span_end": 118,
      "excerpt": "The accounts service will use PostgreSQL as its primary data store for transactional consistency.",
      "confidence": 1.0
    }
  ]
}
EOF
```

Ingest it:

```bash
bicameral ingest my-decision.json
```

The CLI records the source snapshot and evidence, creates a reviewable
`DecisionCandidate`, and prints its candidate ID. A candidate is not yet a
Decision.

## 5. Start the daemon and pair this device

Register the daemon as a user-level service and start it:

```bash
bicameral service install
bicameral service start
```

The daemon exposes only its loopback ToolRequest API on
`http://127.0.0.1:37373`. It owns authorization and every canonical state
transition, and it is the sole persistence authority through its event store.
Request/render surfaces — the MCP session and the Cloud-hosted dashboard — never
own canonical state.

Open `https://app.bicameral-ai.com/connect` in your browser and follow the hosted
pairing flow to connect this running local daemon. The hosted `/connect` surface
is the alpha operator path. You do not need to run a local Bicameral Cloud
process, and the daemon never serves SPA assets or fixture data.

## 6. Promote the candidate through MCP

The accepted first-Decision path for alpha is an **MCP RecallPacket** promotion,
authorized by the daemon. Connect an AI coding agent to the local daemon through
[`bicameral-mcp`](https://github.com/BicameralAI/bicameral-mcp) (Claude Code and
Codex host adapters), then drive the promotion from that session:

1. **Recall.** The agent runs a RecallPacket lookup (`preflight.run`) against the
   daemon. The daemon authors the read-only packet and surfaces your
   `DecisionCandidate` with its allowed outcomes and decision-attention signal.
2. **Select.** You pick the surfaced **Quickstart architecture decision**
   candidate and request promotion as a new constraint
   (`recall.promote_decision_candidate`).
3. **Confirm once.** Because the request would write canonical state, the daemon
   replies `confirmation_required` with a single-use, five-minute confirmation
   challenge and appends **no** event yet. The MCP session renders the prompt;
   you explicitly confirm.
4. **Materialize.** The agent resubmits the same request carrying the
   confirmation token. The daemon validates actor, capability, policy, evidence,
   and target state, single-use-consumes the challenge, and materializes the
   accepted Decision through the event store. Only this daemon acceptance turns
   the candidate into a canonical Decision.

The `DecisionCandidate` stays non-canonical until the daemon accepts it, and the
confirmation challenge is transient authorization evidence — never durable
product state.

`bicameral review submit <candidate-id>` is not an acceptance command. It runs
the CLI governance evaluation and records whether a candidate is eligible for
materialization; it does not promote the candidate into the Decision Ledger.

> **Deferred for alpha (not prohibited):** promoting from the dashboard Activity /
> Ingestion Gate is deferred for the alpha first-Decision journey. It is not
> architecturally prohibited — a future Tool Surface may submit the same
> daemon-authorized promotion request, but it would own no persistence authority.
> The daemon remains the single promotion authority.

## 7. Verify the Decision

Open **Decisions** in the dashboard, or read the Ledger View through the daemon
API. The PostgreSQL decision now appears as a tracked Decision with its cited
source evidence. The Ledger View is a **read-only projection** of daemon-owned
canonical state; rendering it never mutates or promotes anything.

Restart the daemon service and confirm the same Decision still appears:

```bash
bicameral service stop
bicameral service start
bicameral service status
```

This proves the accepted event survives restart and is reconstructed through
event-store replay. A restart also rotates the daemon's confirmation-challenge
epoch, so any un-confirmed challenge from before the restart is invalidated —
only the materialized Decision persists.

## Validation scope

This page is a local operator walkthrough. It does not exercise the managed MCP
surface, released Claude Code or Codex host adapter, hosted dashboard, or a
cross-runtime production topology, so it is not release journey evidence.
Topology-applicable Release Units validate their exact assigned JourneyRevision
through the corresponding real-process or browser topology receipt.

## What's next

- **Preflight:** run `bicameral preflight` before implementation to surface
  relevant accepted Decisions.
- **Code grounding:** the local tree-sitter resolver can produce verified
  `BindingEvidence` linking Decisions to code symbols.
- **Source integrations:** feed evidence through
  [`bicameral-integrations`](https://github.com/BicameralAI/bicameral-integrations).
- **Team setup:** see [docs/manual_setup.md](docs/manual_setup.md) for event-store
  substrate configuration and multi-member onboarding.
- **MCP sessions:** connect an AI agent through
  [`bicameral-mcp`](https://github.com/BicameralAI/bicameral-mcp) for
  agent-assisted capture and preflight.

---

See the full [README](README.md) for architecture, design principles, and project
governance.
