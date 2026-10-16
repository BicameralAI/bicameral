<!-- markdownlint-disable MD033 MD041 -->
<div align="center">

# Bicameral Cloud

### The optional hosted optimization layer for Bicameral

<p align="center">
  <strong>
    🏗️ <a href="#high-level-architecture">Architecture</a> ·
    ⚡ <a href="#local-setup">Setup</a> ·
    🧭 <a href="#boundary-rule">Boundary Rule</a> ·
    🏛️ <a href="#governance">Governance</a> ·
    🛡️ <a href="#trust--compliance">Trust</a>
  </strong>
</p>

[![Classification: product-critical](https://img.shields.io/badge/Classification-product--critical-d1242f)](#governance)
[![SOC 2 Type 1: in preparation](https://img.shields.io/badge/SOC_2_Type_1-in_preparation-f5a623)](#trust--compliance)
[![Python 3.11+](https://img.shields.io/badge/python-3.11%2B-blue?logo=python&logoColor=white)](#local-setup)

</div>

**Bicameral Cloud** is the optional hosted optimization layer for Bicameral: organization-scale code graph, conflict oracle, expensive indexing, and grounding optimization.

It is not the default control plane and not the canonical decision store. The public local bot owns local workflow, review, and materialization. Cloud returns evidence, signals, suggestions, and advisories.

## Table of Contents

- [Key Features](#key-features)
- [High-level Architecture](#high-level-architecture)
- [Local Setup](#local-setup)
- [Repository Layout](#repository-layout)
- [Related Repositories](#related-repositories)
- [Boundary Rule](#boundary-rule)
- [Validation](#validation)
- [Governance](#governance)
- [Trust & Compliance](#trust--compliance)
- [Security](#security)

---

## Key Features

- **Hosted code graph** -- indexes branches, worktrees, repos, symbols, dependencies, and history at organization scale.
- **Conflict oracle** -- predicts cross-branch and cross-repo conflicts before teams discover them late.
- **Blast-radius analysis** -- estimates affected systems, deploy paths, owners, and dependencies.
- **Grounding optimization** -- improves binding quality, latency, and recall beyond local-only scans.
- **Substrate-neutral output** -- returns `ConflictSignal`, `GroundingSuggestion`, `CodeGraphEvidence`, `DependencyRisk`, and advisory `GovernanceResult` objects.

## High-level Architecture

```text
bicameral-bot local daemon
        | advisory query
        v
bicameral-cloud
  hosted code graph
  conflict oracle
  grounding optimizer
        | evidence / signals / advisories
        v
bicameral-bot governance policy + review
```

## Local Setup

Requires Python 3.11+.

```bash
# Create a virtual environment
python3 -m venv .venv
source .venv/bin/activate

# Install runtime and dev dependencies
pip install -e '.[dev]'

# Run the dev server (default: http://127.0.0.1:8000)
uvicorn bicameral_cloud.app:app --reload
```

### Environment Variables

| Variable | Purpose | Required |
| --- | --- | --- |
| `SUPABASE_URL` | Supabase project URL | Required for hosted signup |
| `SUPABASE_ANON_KEY` | Supabase browser anon key returned by `/api/v1/auth/config` | Required for hosted signup |
| `SUPABASE_JWT_SECRET` | Optional local JWT validation secret; when absent, backend validates access tokens via Supabase Auth API using the anon key | No for hosted signup |
| `SUPABASE_SERVICE_KEY` | Supabase service-role key used server-side to read `design_partner_allowlist` | Required for hosted design-partner access control; never returned to browsers |
| `PRODUCT_JOIN_TICKET_ISSUER` | Exact issuer pinned by local daemons for Product join tickets | Required for join-ticket issuance |
| `PRODUCT_JOIN_SIGNING_KEY_ID` | Active dedicated Ed25519 signing-key ID | Required for join-ticket issuance |
| `PRODUCT_JOIN_SIGNING_PRIVATE_KEY_B64` | Base64 raw 32-byte Ed25519 private seed | Required for join-ticket issuance; server-side only |
| `PRODUCT_JOIN_VERIFICATION_KEYS_JSON` | Optional overlap/retired public keys for rotation | No |
| `PRODUCT_JOIN_TICKET_TTL_SECONDS` | Ticket lifetime, constrained to 1–300 seconds | No; defaults to 300 |

Alpha uses an in-memory coordination store. Supabase Auth is the browser identity provider when configured; Bicameral Cloud still enforces design-partner domain access server-side from the Supabase table `public.design_partner_allowlist`, with one lowercase domain per row. Example row: `domain = bicameral-ai.com`, `enabled = true`.

## Repository Layout

```text
src/bicameral_cloud/
  app.py               # FastAPI application entrypoint
  audit.py             # Audit event emitter helper
  store.py             # In-memory coordination store (alpha)
  auth/                # Auth dependencies and stubs
  bridge/              # Bridge routing and allowlist logic
  models/              # Pydantic models (tenancy, pairing, discovery, compatibility, audit)
  routes/              # API and dashboard routes
    health.py          # GET /health
    dashboard.py       # GET /dashboard (placeholder shell)
    auth.py            # GET /api/v1/auth/session
    team.py            # GET /api/v1/teams/{team_id}
    pairing.py         # GET /api/v1/teams/{team_id}/pairing
    compatibility.py   # GET /api/v1/teams/{team_id}/compatibility
    discovery.py       # GET /api/v1/teams/{team_id}/workspaces
    bridge.py          # POST /api/v1/bridge/route
    audit.py           # GET /api/v1/audit/status, GET /api/v1/teams/{team_id}/audit
tests/                 # Pytest test suite
docs/adr/              # Cloud-specific architecture decisions
CONTEXT.md             # Project glossary and resolved terms
README.md              # You are here
```

## Related Repositories

- [`bicameral-bot`](https://github.com/BicameralAI/bicameral-bot) -- local authority boundary and protocol owner.
- [`bicameral-integrations`](https://github.com/BicameralAI/bicameral-integrations) -- public source adapters and mods.
- [`bicameral-mcp`](https://github.com/BicameralAI/bicameral-mcp) -- public agent-facing tool surface.

## Boundary Rule

Cloud can say "this probably conflicts," "this binding is stronger," or "this dependency risk has high blast radius." Cloud does not silently create signoff, compliance resolution, canonical decisions, or blocking enforcement.

## Validation

Run the full deterministic validation suite:

```bash
pytest -v tests/ && python -m ruff check src/ tests/ && python -m mypy src/
```

Individual checks:

```bash
# Tests
pytest -v tests/

# Lint
python -m ruff check src/ tests/

# Type check
python -m mypy src/
```

### Disposable real-process hosted topology

Run the supporting integration topology used by downstream hosted browser
journeys:

```bash
python scripts/run_real_process_hosted_topology.py \
  --bot-repo <bicameral-bot checkout> --mcp-repo <bicameral-mcp checkout> \
  --factory-commit <Factory commit>
```

> [!WARNING]
> This journey does not currently pass against the pinned Bot release: the harness predates a Bot pairing-contract change. See [#21](https://github.com/BicameralAI/bicameral/issues/21) and the [verification ledger](../../migration/README.md#verification-ledger). The Bot and MCP checkouts must be full Git clones at the pinned commits, which are published only in the original repositories.

The command requires Docker, Node/npm, recent stable Rust/Cargo (Cargo 1.85+),
`uv`, Git, and loopback port access. It allocates isolated ports; starts
disposable Supabase Auth/Postgres,
the production Cloud app, two production bot daemons, and the real MCP
server/client; applies and behaviorally checks every tracked migration and RLS
boundary; builds the bot dashboard SPA into an isolated Cloud runtime; verifies
bridge-session isolation and restart/replay from committed Git events; then
tears down all processes, containers, homes, repositories, credentials, and
ports. A sanitized receipt is written under
`test-artifacts/real-process-hosted-topology/`.

This is real-process integration evidence only. It supports, but does not
replace, terminal browser/User Journey acceptance.

### Cross-repo dashboard ↔ local daemon pairing conformance harness

`tests/conformance/` holds a deterministic cross-repo harness for the
Cloud-authenticated browser → local-daemon hosted-bridge pairing flow
(issue #39). It runs without real Google OAuth or production Supabase
credentials: the authenticated dashboard path is exercised with a mocked
Supabase session (a locally minted HS256 token validated via
`SUPABASE_JWT_SECRET`) plus a design-partner domain allowlist, and the local
daemon's `POST /api/v1/hosted-bridge/pair` endpoint is modeled by an
in-process fake daemon on loopback.

Run it locally:

```bash
pytest -v tests/conformance        # or: pytest -v -m conformance
```

It covers:

- authenticated allowlisted Cloud dashboard load (and 403 for a
  non-allowlisted domain);
- happy-path pairing returning the operator-home / project-readiness
  projection;
- daemon unavailable before pairing (connection refused → repair state);
- daemon `500` after pairing rendering the typed pairing/repair hint rather
  than raw daemon error text;
- a leak assertion that no local paths, provider secrets, source bodies,
  graph indexes, or event-store payloads appear in any browser-facing
  surface (dashboard HTML, auth config/session, paired projection).

**Manual browser gap.** This is an HTTP-level conformance harness because the
repo has no browser/JS test substrate. The dashboard's actual DOM rendering,
real Google OAuth, real production Supabase, and a real loopback daemon are
not exercised here; that production browser acceptance is
release-sensitive and tracked by the production deploy/acceptance issue
(#40). The harness instead asserts the daemon-response → UI-state contract
and that the dashboard JS mirrors the same typed repair-hint keys.

### Hosted Project Creation browser UserJourney

`tests/journey/` holds the deterministic **`hosted_project_creation`**
UserJourney (issue #114): a real headless browser driven through the hosted
dashboard's Create Project flow against the live Cloud app and its real
registration/discovery HTTP contracts. It runs entirely on loopback with **no
production credentials** — Supabase Auth is validated locally against a test
HS256 secret, the design-partner allowlist is an in-process loopback fake, and
the authenticated browser identity is a locally minted session.

Install the journey harness and browser once, then run it (the same command is
used locally and in CI):

```bash
pip install -e '.[dev,journey]'
python -m playwright install chromium

scripts/run_hosted_project_creation_journey.sh   # or: pytest -q -m journey tests/journey
```

The single command starts all required local services in-process. The journey:

- creates a uniquely named Project through the **real** Create UI (never seeds
  the final list state directly);
- asserts a successful `ProjectRegistrationReceipt` (`201`) before discovery;
- proves `GET /api/v1/projects` is refetched after registration (a create panel
  that merely closes while the list stays stale fails);
- asserts the new Project renders with the existing `Needs binding`
  presentation (`discoverable_unbound`) and no Bind CTA;
- reloads the browser and asserts the Project row persists;
- records the dashboard bundle bot commit/digest from `/app/version.json` and
  fails on absent/malformed version metadata;
- fails loudly if the registration-receipt or discovery-item wire shape drifts
  from the Cloud contract models.

On failure a Playwright trace, a full-page screenshot, and an HTTP/version
summary are retained under `test-artifacts/hosted_project_creation/` (uploaded
as a CI artifact by the `Hosted Project Creation Journey` workflow). This slice
is deterministic and local; the terminal deployed-environment validation of the
journey is tracked separately (cloud#115).

## Governance

This component is governed by the repository's [GOVERNANCE.md](../../GOVERNANCE.md). It is classified **product-critical** because it is the largest attack surface (an internet-facing, hosted data plane).

## Trust & Compliance

BicameralAI's intended first examination is **SOC 2 Type 1** (Security + Confidentiality). No SOC 2 report has been issued, and nothing here claims SOC 2 compliance or certification. ISO 27001 is not part of the approved first-examination posture and is not claimed here. No public trust page is advertised until it is deployed and approved. This summary is framework-level by design; detailed control and test posture is maintained internally and is not published here.

## Security

Please report security issues to **security@bicameral-ai.com**. Do not open a public issue for a suspected vulnerability. See [`SECURITY.md`](SECURITY.md) for the full policy, coordinated-disclosure timeline, and safe-harbor terms.
