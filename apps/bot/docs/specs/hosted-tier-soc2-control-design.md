# Hosted-Tier SOC 2 Control Design

Date: 2026-07-03

Status: draft — gated on activation trigger (see below)

Level: L2 cross-repo control checklist

Owns: the application-level SOC 2 control scope for the hosted/team tier, and the attestation evidence each control needs before activation.

Related: ADR-0025 (hosted-tier trust boundary and SOC 2 control scope), `docs/releases/alpha/hosted-cloud-responsibility.md`, `docs/specs/cloud-hosted-dashboard-local-daemon-runtime-architecture.md`, `crates/bicameral-api/src/hosted_bridge.rs`, BicameralAI/bicameral-cloud#7 (token broker), BicameralAI/bicameral-daemon#7 (umbrella), BicameralAI/bicameral-daemon#9 (auth), BicameralAI/bicameral-bot#226 (availability), BicameralAI/bicameral-bot#547 (access-control scope), `SOC2-READINESS.md`. Policy set: Drive *[Bicameral] Policies*.

## Reading guide

Read ADR-0025 first — it fixes the trust boundary this checklist depends on. The one-line invariant:

```text
The hosted tier is a coordination layer. It holds no Confidential data.
Canonical authority, secrets, and Product/Ledger state stay in the paired local daemon.
Each Product Member runs their own daemon; a "team" is a shared Product in the event store.
```

Everything in scope below is a control on what the hosted cloud *does* touch:
session/pairing metadata, the GitHub App token broker, and transport. Everything
about the operator's Decision Ledger (backup, RTO/RPO, retention) is explicitly
**out of scope** and handled by operator responsibility + endpoint FDE.

## Activation trigger

These controls must be audit-attested **before the first paying hosted/team
customer** (ADR-0025 decision 5). They are *not* opened as build tickets now:
the surfaces already ship in alpha, so the work is verification + attestation,
not construction.

Accepted residual risk: an alpha/free external Cloud account may pair through the
hosted bridge in production before attestation is complete. Blast radius is
bounded because the cloud holds no Confidential data (ADR-0025 decision 1). If
this risk becomes unacceptable (e.g. a free external pilot), pull the trigger
forward to "first external production pairing."

## In-scope controls

Each row: the control, the policy it satisfies, the already-shipping surface it
attaches to, and the evidence/attestation needed at activation.

### C1 — TLS cipher grade in transit

- **Policy:** Cryptography Policy ("strong ciphers … over a public network", Web Cipher (TLS) row: B-or-greater SSL Labs grade); Data Management (Confidential data encrypted in transit over public networks).
- **Surface:** browser ↔ hosted cloud (`https://app.bicameral-ai.com`), and hosted cloud ↔ local daemon bridge (`hosted_bridge.rs` origin is HTTPS).
- **Attestation needed:** SSL Labs (or equivalent) report ≥ B for the hosted origin; TLS ≥ 1.2, no known-weak ciphers; HSTS on the hosted origin. Record report + date.
- **Status:** verify — hosted origin exists; grade not yet attested.

### C2 — GitHub App token-broker secret handling

- **Policy:** Cryptography (key management, secrets access-controlled); Data Management (secrets & private keys are Confidential); Secure Development (secure secret custody).
- **Surface:** `bicameral-cloud#7` broker — GitHub App private key/client secret held only in the cloud secret boundary; mints short-lived installation tokens; responses/logs carry no key material; emits non-secret audit/correlation IDs.
- **Attestation needed:** confirm private key lives only in the cloud secret manager (not in env files, images, or repo); confirm installation tokens are short-lived and not persisted in dashboard state/logs/fixtures; sample broker logs show no secret material; correlation IDs present for audit.
- **Status:** verify — design already forbids leakage (cloud#7 body); needs a point-in-time attestation against the running service.

### C3 — Hosted-bridge pairing / session authentication

- **Policy:** Access Control (authenticated, capability-checked access; MFA where feasible upstream via Cloud account identity); this is the hosted-path answer to daemon#9's "declare MCP transport trust boundary + auth".
- **Surface:** `hosted_bridge.rs` — pairing checks origin, CSRF token, replay/nonce, and expiry; sessions carry TTL + explicit `Revoked` state; account-linkage validated daemon-side; bridge cannot accept provider tokens, write events, or bypass ADR-0020 action authorization.
- **Attestation needed:** document the pairing state machine (`Unpaired → Paired → Revoked`) as the hosted trust-boundary statement in the SOC 2 system description; confirm session expiry + revocation are enforced (test evidence: `hosted_bridge_e2e_conformance`, `hosted_bridge_protocol_conformance`); confirm forbidden flows are enforced, not just documented.
- **Status:** built — needs the trust-boundary write-up to close daemon#9's Track 1.

### C4 — Non-canonical metadata retention

- **Policy:** Data Management (retention: data kept only as long as there is a business need; PII deleted/de-identified when no business use); the *Restricted* class covers session/preview/team metadata.
- **Surface:** hosted session/pairing/preview/team metadata + broker audit identifiers (the only things the cloud stores).
- **Attestation needed:** a short retention statement for hosted metadata (what is kept, why, for how long, how disposed); confirm no Confidential data is present to retain (follows from ADR-0025 decision 1); align with the Data Retention Matrix (Appendix B of the Data Management Policy).
- **Status:** declare — write the retention statement; no deletion machinery for Ledger data is needed here (that data isn't in the cloud).

### Operator-local operational diagnostics

Product creation and publication failures are diagnosed at the authority
boundary that owns them: the paired local daemon. The daemon writes bounded,
redacted operational receipts under
`~/.bicameral/operational-logs/`. These receipts are troubleshooting evidence,
not Decision Ledger events, governance facts, or hosted telemetry.

- **Retention:** at most 200 records. Records become eligible for deletion at
  14 days; the daemon enforces that age boundary at startup, after each write,
  and hourly while running. A stopped daemon performs the catch-up deletion on
  its next start.
- **Access:** the directory is mode `0700` and each receipt is mode `0600` on
  Unix platforms.
- **Allowed fields:** request ID, command, outcome status, bounded error code,
  caller surface, event-store substrate, aggregate canonical-creation counts,
  and truncated SHA-256 fingerprints for Product and idempotency correlation.
- **Forbidden fields:** raw Product IDs, idempotency keys, Cloud account or
  actor IDs, session tokens, credentials, source/event bodies, local paths, and
  event-store connection material.
- **Support flow:** the hosted SPA displays only the daemon request ID. It does
  not install an analytics SDK, persist diagnostics in browser storage, or send
  the local receipt to Cloud.

This setup supports the SOC 2 posture rather than weakening it: it improves
incident diagnosis and accountability while preserving data minimization,
locality, access restriction, and disposal. Any future upload or central
collection of these receipts would be a separate control-boundary change and
must define purpose, access, encryption, retention, and deletion before
activation.

## Out of scope (operator responsibility — documented, not built)

Named explicitly so an auditor sees they were considered and consciously scoped out:

- **Backup encryption of the Decision Ledger** → operator's event store (Git/Drive) + endpoint FDE. Covered by SOC2-READINESS team-wide "disk encryption" task.
- **RTO/RPO of decision data** → operator owns uptime and recovery of their own event store (availability stance, bot#226).
- **Scheduled retention enforcement of the Ledger** → operator owns local data lifecycle; right-to-erasure on the local product is tracked separately (mcp#545), not a hosted-tier control.
- **MCP transport auth-shim (daemon#9 Track 2)** → deferred; only needed for a shared/hosted daemon, which the daemon-per-member model (ADR-0025 decision 2) does not use.

## Deferred cluster (gated behind a contract + ADR reversal)

If a customer contractually requires a **server-side shared canonical store**
(ADR-0025 decision 6, the Model-B reversal), this checklist is superseded by the
full multi-tenant control set: tenant isolation, encryption at rest (AES-256 per
Cryptography Policy), encrypted backups + RTO/RPO, scheduled retention
enforcement, tenant-scoped audit logs, and full multi-tenant auth. Do not open
these until that contract exists; opening them speculatively is the over-scoping
ADR-0025 exists to prevent.

## Done when

Each in-scope control (C1–C4) is either attested with recorded evidence or has a
documented, accepted exception against the named policy — completed before the
activation trigger fires. Out-of-scope items each have a one-line scope statement
in the relevant policy's control mapping.
