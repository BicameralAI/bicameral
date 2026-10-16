# Application-Level SOC2 Control Attestation (#658)

**Status:** Evidence record for GH #658 (application/runtime security track).
**Scope:** Application-level controls — code, config, dependencies, runtime — for `bicameral-bot`. Repo/merge-policy controls are tracked separately in #659.
**Related:**
- ADR-0025 — [Hosted-tier trust boundary and SOC2 control scope](../adr/0025-hosted-tier-trust-boundary-and-soc2-control-scope.md)
- [Hosted-tier SOC2 control design](./hosted-tier-soc2-control-design.md)
- Source policies (Drive · *[Bicameral] Policies*): Secure Development, Operations Security, Data Management, Third-Party Management.

This record satisfies the #658 "Done when: each box is checked or has a documented, accepted exception recorded against the relevant policy" clause. Each control is grounded to a `file:line` on the branch that lands this attestation. Verification history: research briefs (ledger #25–#26), plan + audit PASS (ledger #28).

## Evidence matrix

| # | Checklist item | Policy | Disposition | Evidence (`file:line`) |
|---|---|---|---|---|
| 1 | Hardcoded-secret audit; `.env` handling | Data Management | **Met** | `.gitignore:16-19` (`.env`/`.env.*`/`*.env`, `!.env.example`); no source secrets (repo-wide grep sweep, research #26); CI secret scan `.github/workflows/secret-scan.yml` → `scripts/secret_scan.py`; onboarding names in `.env.example` |
| 3 | Untrusted-input validation | Secure Development | **Met** | `crates/bicameral-gateway/src/routes.rs:632` (`external_ingest`: rate-limit → forbidden-field → typed-deser → canary → sensitive gates); `:591` (`detect_forbidden_fields`); `:5389` (`hosted_bridge_command`: session + authority-smuggling gates + cloud-routable allowlist) |
| 4 | Secure defaults; attack-surface minimization | Secure Development | **Met** | `crates/bicameral-gateway/src/lib.rs:85,91` (1 MiB body limit → 413; 30 s timeout → 408), applied outermost `:129-130`; `hosted_bridge_cors.rs` (default-closed CORS — untrusted origin gets no headers); deprecated unmounted `invite_*` handlers + orphaned `config_from_state` removed in this change |
| 7 | Dependency pin/lock; vulnerability-scan path | Third-Party Management | **Met** | `Cargo.lock`, `dashboard/package-lock.json` (pinned); `.github/workflows/cargo-audit.yml` (RustSec, weekly cron + PR/push), `dependency-review.yml`, `sbom.yml`, `scorecard.yml` |
| 2 | Encryption at rest / in transit | Data Management (Cryptography) | **Accepted exception — deferred to hosted-tier design** | Local data plane is browser↔localhost daemon; hosted cloud serves static only and never sees Confidential data (ADR-0025). Transport/at-rest posture is owned by the hosted-tier control design, not app-level code. |
| 5 | Environment separation | Operations Security | **Disposition — documented** | Runtime config is env-driven (`.env.example`; `crates/bicameral-config/src/connections.rs`); no Confidential production customer data is used in dev/test. No prod/dev config is committed. |
| 6 | Third-party AI/automation provider inventory | Third-Party Management | **Open — documentation follow-up** | Inventory of automation providers (Devin/Cognition, Composio, Zapier, model providers) and their data-usage/training-terms + SOC 2 assessment is tracked as a documentation-track follow-up, not an app-level code control. |

## Read-route authentication note (informs #698)

Hosted-bridge **read** routes are session-gated identically to write routes: `hosted_bridge_status` (`crates/bicameral-gateway/src/routes.rs:5281`) and the operator-home projection both call `validate_bridge_session` (`:5156`), which enforces token presence → `401`, unknown token → `401`, revoked → `403`, and expiry → `403`. The operator-home projection strips filesystem paths, provider secrets, SourceEvidence bodies, and Decision-Ledger payloads. This is stricter than "advisory" and is the evidence basis for the #698 read-route auth-strictness decision.

## Items 2, 5, 6 — compliance record

These three checklist items are non-code confirmations (research brief, ledger #32). Recorded here so #658's "documented, accepted exception" clause is satisfied.

### Item 2 — Encryption at rest / in transit (Data Management / Cryptography)

- **In transit — Met.** All external provider calls use HTTPS/TLS (see the item-6 table). The browser↔daemon data plane is **loopback** (`http://localhost:37373`, no network). Hosted cloud serves static assets only and never sees Confidential data (ADR-0025). No Confidential data crosses a public network unencrypted.
- **At rest — documented accepted exception.** The daemon persists locally — `events.jsonl` (git substrate, `crates/bicameral-runtime/src/event_store_adapters/git.rs`) or a Drive-folder substrate — on the user's own machine / their own Drive. The application adds no bespoke at-rest crypto layer; at-rest protection is host-OS disk encryption (user-controlled) and, for the Drive substrate, Google's encryption. Single-tenant local storage — there is no shared datastore of customer Confidential data.

### Item 5 — Environment separation (Operations Security)

- **Met by architecture.** The product is **local-first, single-tenant**: each user runs their own daemon; there is no shared multi-tenant production environment holding customer Confidential data (hosted cloud = static only, ADR-0025). Runtime config is env-driven (`.env.example`; `crates/bicameral-config/src/connections.rs`). Dev/test uses fixtures and compile-time test-only transport seams (local stub token endpoints under `cfg(test)`, fixture corpora), never prod customer data; the former `BICAMERAL_GOOGLE_TOKEN_EXCHANGE_MOCK` switch was removed by #1132. No prod Confidential data pool exists to scrub.

### Item 6 — Third-party AI / automation provider inventory (Third-Party Management)

| Provider | Purpose | Transport | Assessment note |
|---|---|---|---|
| Google Drive | Source adapter / evidence ingest (OAuth) | HTTPS | Google SOC 2 available; assess data-usage terms |
| GitHub | Source adapter + device-flow auth | HTTPS | GitHub SOC 2 available |
| Linear | Egress projection target | HTTPS (GraphQL) | Linear SOC 2 available |
| Devin / Cognition | Dev-time factory bot + optional evidence connector | HTTPS | **Assess Cognition data-usage/training terms** (touches source) |
| Slack | Optional evidence connector | HTTPS | Slack SOC 2 available |
| Local OpenAI-compatible endpoint | Optional runtime reasoning/semantic provider (`ProjectionProviderMode::LocalOpenaiCompatible`) | **local** (user endpoint) | No cloud model provider by default (deterministic-first) |
| Cloudflare R2 | Release-artifact hosting | HTTPS | N/A — binaries only, no customer data |

- **Not integrated**: Composio, Zapier (named as examples in #658 — no code reference), and cloud model providers (Anthropic/OpenAI hosted) are not called by default.
- **Open action (owner: Kevin)**: confirm SOC 2 + data-usage/training-terms assessment for **Cognition/Devin** (touches source); confirm Google / GitHub / Linear / Slack SOC 2 on file. Tracking: PTM-22.

## Maintenance

Line references are point-in-time on the landing commit. If the gateway surface is refactored, re-verify each `file:line` (the citations were re-checkable via the research-brief greps in ledger #25–#26). Items 2, 5, and 6 remain owned by the hosted-tier control design and the documentation track respectively; this record does not close them as code.
