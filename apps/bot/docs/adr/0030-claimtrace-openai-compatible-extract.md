# ADR-0030: ClaimTrace Extract Uses OpenAI-Compatible HTTP Only (v0.2)

**Date:** 2026-08-10

**Status:** accepted

**Level:** L1

**Related:** ADR-0014, ADR-0015, ADR-0028, ADR-0029,
`docs/specs/reasoning-projection-v02-architecture.md`,
`docs/specs/smr-alpha-claim-semantic-trace-alloy-bridge.md`,
`docs/specs/semantic-resolution-capability-contract.md`,
BicameralAI/bicameral-bot#1064, #957, #958, #959

## Context

v0.2 ships a live ClaimTrace producer into managed preflight. Two transports
were considered:

1. **`local_cli`** — daemon spawns a pinned executable with a custom JSON
   protocol.
2. **OpenAI-compatible HTTP** — daemon calls `base_url` + `model_id` (Ollama,
   LM Studio, vLLM, cloud, or an optional operator gateway).

Local model ecosystems (Ollama, LM Studio, Hermes, OpenClaw) treat weights as
**local OpenAI-compatible servers**, not as GGUF loaded into the agent process
and not as a second product CLI protocol for inference. Dual modes would split
readiness, secrets, and journey proof without buying additional local-model
coverage that HTTP already provides.

## Decision

For **ClaimTrace extract / Reasoning Projection Stage 1 (v0.2)**:

1. **Only transport:** OpenAI-compatible HTTP (`base_url`, `model_id`, optional
   secret, timeouts).
2. **`local_cli` is out of scope** for this path. Do not implement configure,
   readiness, or extract via subprocess for ClaimTrace.
3. **Local models:** operator-owned servers on loopback (or LAN with explicit
   policy). Bicameral does not embed GGUF or ship an inference server.
4. **Client:** prefer a thin Rust OpenAI-compatible client (`async-openai` or an
   equivalent already in-tree). LiteLLM, LangChain, and agent frameworks are not
   required product dependencies.
5. **Gate:** untrusted model JSON → deterministic admit gate
   (schema/provenance/coverage); reject, do not repair.
6. **Identity:** every analysis pins profile digests including base_url, model,
   and config; reject `default`/`latest`; **no silent multi-model failover**.
7. **Authority:** extract, gate, and SMR/Alloy results remain derived; they must
   not write the Decision Ledger.

Optional operator gateways (for example LiteLLM) are valid only as a configured
`base_url`, not as a Bicameral-required service.

This amends the alpha `local_cli` default in ADR-0015 **for ClaimTrace extract
only**. Other reasoning tasks may retain prior placement decisions until
separately amended.

## Consequences

### Positive

- One readiness surface (#957) and one extract adapter (#958).
- Local and remote models share the same mode.
- Aligns with ecosystem local-model practice and `SRC-PROFILE-01` pinning.
- Smaller attack and ops surface than a CLI + HTTP dual path.

### Negative / accepted costs

- Operators must run or point at an OpenAI-compatible server.
- Vendor coding CLIs are not the extract unit of configuration.
- Server quirks (context length, structured-output support) become operator
  documentation, not product mode forks.

### Follow-ons

- Implementation: #957 readiness, #958 producer, #959 journey.
- Architecture package: `docs/specs/reasoning-projection-v02-architecture.md`.
- Offline optimizers and observability export remain off the hot path.

## Non-goals

- Rebuilding the SMR/Alloy kernel.
- Activating full `bicameral.semantic-resolution`.
- Hosted Bicameral reasoning as the alpha default.
- Prompt optimizer or visibility SaaS as authority.
