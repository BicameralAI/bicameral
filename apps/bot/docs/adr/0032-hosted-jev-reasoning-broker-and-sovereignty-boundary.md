# ADR-0032: Hosted Jev Reasoning Broker — Sovereignty Boundary and Provider Supersession

**Date:** 2026-09-21
**Status:** proposed — docs-only reversal; not implementation authorization
**Level:** L2 cross-repo contract (bot ↔ cloud; Factory cross-repo processing contract still required before implementation)
**Related:** ADR-0025 (hosted-tier trust boundary and SOC 2 control scope); ADR-0030 (ClaimTrace OpenAI-compatible extract); Bot #1096 (OpenRouter free-model default, superseded by this ADR); Bot #1091 / ADR-0031-draft "hosted-vertex-reasoning-broker" (closed, superseded); Bot #1300 (Jev wayfinder experiment); Bot #1278 (Reasoning Projection Alpha Advisory umbrella); Factory #907 (external trust-boundary claim); Factory #490 / PR #491 (orphaned hosted-reasoning processing contract, closed unmerged 2026-08-11 — recreation required, not reused)
**Owner:** jinhongkuan; requested reviewer: @Knapp-Kevin (Product Design owner of record on #1096 and the #1091 review)

## Problem

Two prior product decisions currently govern hosted reasoning:

1. **ADR-0025 decisions (1) and (6):** Hosted Cloud never holds Confidential data (no source bodies), and there is no canonical Organization/Team entity or multi-tenant credential store. Both are explicitly marked as reversible only "gated behind a customer contract that contractually requires it," never speculatively.
2. **Bot #1096 (Kevin, 2026-08-11, accepted with amendments):** superseded an earlier Vertex-hosted-broker proposal (#1091) with **OpenRouter free-model** as the default hosted-reasoning provider path, conditioned on: (a) operator-configured BYOK providers remaining an explicit, never-silently-dropped alternative, and (b) explicit affirmative hosted-processing consent gated on first real invocation, never on authentication/pairing alone.

Since #1096, the Jev/TypeSafe System One model (Bot #1300) has been evaluated as a typed-decision producer with no string generation and materially better cost/latency characteristics than either OpenRouter or Vertex. Adopting it as the hosted default requires reopening both the ADR-0025 boundary and the #1096 provider decision, and — per this ADR's decision below — retiring the BYOK-remains-an-alternative condition Kevin attached to that acceptance.

This ADR does not claim Kevin has reviewed or agreed with the changes below. It is filed as a PR specifically so he sees the supersession of his own recorded decisions before this lands, consistent with how #1091/#1096 were themselves negotiated.

## Decision

1. **Provider supersession.** Bot #1096's OpenRouter-free-model default is superseded. The hosted-reasoning default becomes **Bicameral-managed Jev (TypeSafe System One)**, reached only through a Bicameral cloud relay (see decision 3) — never a direct daemon→TypeSafe call, since TypeSafe/Opper's documented auth model exposes no scoped/delegated sub-credential mechanism a daemon could safely hold.

2. **BYOK is retired as a required alternative.** Kevin's #1096 condition — "operator-configured OpenAI-compatible providers remain an explicit alternative and never a silent fallback" — is explicitly withdrawn by this ADR. There is no BYOK path in the target architecture. This is a deliberate reversal of that specific condition, not an oversight.

3. **ADR-0025 decision (1) — narrow reversal.** Hosted Cloud may transiently process bounded, explicitly-submitted source/model payloads for the purpose of a single Jev reasoning call, persisting and logging only content-free metadata (model/provider identity, token/cost usage, routing reason — the same shape already defined for the `cloud_advisory` surface in `docs/specs/token-telemetry-spend-events.md`). This mirrors the boundary Kevin accepted "in principle" for Vertex in the #1091 review. Canonical authority, secrets, and Product event history remain governed by ADR-0025 decisions (2)-(5), unchanged.

   This reversal is exercised under the sovereignty framing already published and reviewed with Kevin in Factory #907 (`docs/external/trust-boundary-claim.md`): Bicameral's buyer promise is custody/authority (C1/C2), explicitly **not** a confidentiality/no-egress guarantee. #907 pre-named "planned reasoning paths that send bounded source material to external processors" as compatible with that promise.

4. **ADR-0025 decision (6) — narrow reversal.** Bicameral may hold a minimal, domain-scoped credential/routing record per team/org, sufficient only to authenticate and meter cloud-relay calls to the single Bicameral-held TypeSafe/Opper API key. This is explicitly **not** a reversal to a canonical Organization/Team entity or a server-side shared Product store — Product membership, policy authority, and event history remain exactly as ADR-0025 decisions (1)-(5) and CONTEXT.md's `Product`/`Product Member` model already define. The new record's only fields are: domain, routing/quota state, and non-secret usage metering keys — no Decision Ledger data, no Product event data, no source bodies.

5. **Consent gating is unchanged from #1096.** Authentication or pairing alone must not trigger inference or source egress. First real hosted-Jev invocation requires explicit affirmative consent (disclosure-only is insufficient), persisted as a non-secret preference, re-asked only after a material processing-policy or provider change. This condition from #1096 is *not* being withdrawn — only the BYOK-alternative condition is.

6. **Metering.** Wire Jev calls into the existing `cloud_advisory` `ReasoningArtifact` surface (`docs/specs/token-telemetry-spend-events.md`); no new metering primitive is introduced by this ADR. The `daemon_llm` surface (BYOK-shaped) is retired as moot, not merely deprioritized, per decision (2).

## What this ADR does not do

- It does not implement anything. No runtime code, credential-issuance service, or consent UI ships from this document.
- It does not recreate the Factory-level cross-repo processing contract that Kevin required before #1091/#1096-shaped work could be implementation-ready (Factory #490/#491, closed unmerged 2026-08-11 as an orphaned stack). Per Kevin's own closing note on #491, that contract must be **recreated against the then-current accepted Factory contract model**, not resurrected from the dead branch. That recreation is required before this ADR's decisions are implementation-ready, and is tracked as follow-on work, not satisfied here.
- It does not reopen ADR-0025 decisions (2)-(5) (daemon-per-member deployment, in-scope hosted controls, out-of-scope operator responsibilities, activation trigger).
- It does not authorize Jev as the ClaimTrace 958a/b/c producer for the separate, still-unresolved Reasoning Projection Alpha Advisory / Managed MCP preflight duplication (Bot #1155/#1278 vs #979) surfaced during this reversal's review. That reconciliation is separate, open work.

## Consequences

- Bot #1096, #1091, and their closed/superseded state remain the historical record; this ADR is what supersedes them going forward, not a rewrite of that history.
- Factory #907's trust-boundary claim (`docs/external/trust-boundary-claim.md`) needs no wording change — this ADR exercises a case it already named as out-of-claim.
- A new Factory-owned cross-repo semantic contract for hosted-Jev processing is required before any implementation PR can be considered governance-ready, per `bic:spec-governance-gate`.
- `docs/specs/reasoning-projection-v02-architecture.md` and `docs/specs/reasoning-projection-alpha-advisory-runtime-architecture.md` (PR #1306, currently unmerged) both assume an operator-configured/BYOK producer identity (`ReasoningProviderProfileV1`) and will need reconciliation or retirement once implementation work resumes — tracked separately, not resolved by this ADR.

## Alternatives considered

- **Direct daemon→TypeSafe call with a Bicameral-issued credential.** Rejected for now: TypeSafe/Opper's documented API exposes a single flat account-level API key with no scoped/delegated sub-credential or token-exchange mechanism (unlike GitHub Apps' installation-token pattern, which is the precedent for Bicameral holding one credential safely). Without that, a "direct" call either hands every daemon the literal shared master key or degenerates into the same cloud-relay data path anyway. Revisit if TypeSafe/Opper ships scoped token issuance.
- **Keep BYOK as a parallel path.** Rejected: reintroduces the full Journey-A credential/keyring/readiness apparatus this reversal is meant to retire, for a use case (fully local, zero-egress reasoning) that has no current demand and that Jev cannot serve locally regardless, since TypeSafe is a hosted-only API.
