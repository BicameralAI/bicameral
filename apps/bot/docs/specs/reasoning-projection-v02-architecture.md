# Reasoning Projection v0.2 — Architecture

| Field | Value |
|---|---|
| Date | 2026-08-10 |
| Status | Accepted for v0.2 implementation sequencing |
| Level | L1 repository architecture |
| Owns | ClaimTrace extract transport, readiness, pipeline boundaries, trust classes for Reasoning Projection Stage 1 |
| Decision | [ADR-0030](../adr/0030-claimtrace-openai-compatible-extract.md) |
| Capability | `bicameral.semantic-resolution` (obligations; this doc does not replace the capability contract) |
| Related | ADR-0014, ADR-0015, ADR-0029; #1064, #957, #958, #959; ClaimTrace bridge and managed preflight specs |
| Not | Release claim · capability activation · kernel rebuild |

## Reading guide

Read **Decision summary**, **Runtime path**, and **Authority** first.

Main invariant:

> The daemon calls an OpenAI-compatible endpoint. The model returns untrusted
> ClaimTrace JSON. A deterministic admit gate accepts or rejects. Validated
> traces feed the landed SMR path. Nothing on this path writes Product history.

---

## 1. Goals

1. Source-bound, schema-constrained ClaimTrace extract without inventing Product law.
2. Exact **profile identity** (model, base_url, prompt/schema/decoding digests).
3. Strict **authority ceiling**: derived artifacts never write the Decision Ledger.
4. Single transport so local and remote models share one readiness + extract path.

### Quality goals (ordered)

1. Authority integrity (no laundering of model output into acceptance)
2. Traceability (source digests, reverse provenance)
3. Operability (content-free readiness; typed failures)
4. Transport simplicity (one HTTP mode)
5. Latency of coordination recall — secondary to honesty

---

## 2. Constraints

| Constraint | Implication |
|---|---|
| Free MCP / local-first | Daemon owns secrets and readiness; no required cloud gateway |
| Capability contract | Obligation IDs; partial coverage allowed; no fake complete activation |
| ADR-0014 / 0015 | LLM is enrichment, not authority |
| ADR-0029 / landed kernel | Do not rebuild Alloy/hypergraph in v0.2 producer tickets |
| Factory obligation playbook | Train tags ≠ coding prerequisite ≠ activation |
| Ecosystem local models | Ollama / LM Studio / vLLM speak OpenAI-compatible HTTP |

---

## 3. Decision summary

| Concern | v0.2 freeze |
|---|---|
| Transport | **OpenAI-compatible HTTP only** |
| `local_cli` | **Out** for ClaimTrace extract |
| Local models | Operator-owned servers behind `base_url` |
| Client library | `async-openai` (or thin equivalent in-tree) preferred |
| LiteLLM / LangChain / Rig | Not required product deps; LiteLLM OK as optional `base_url` |
| Optimizer | Offline only; promotion = new `prompt_template_id` |
| Observability SaaS | Optional export later; never authority |
| Silent multi-model failover | **Forbidden** |

```text
daemon ──HTTP──► base_url /v1
                   ├─ Ollama / LM Studio / vLLM (loopback)
                   └─ OpenAI / Azure-compatible / optional gateway
```

---

## 4. Context

Agents compress implementation time and invent product behavior. Reasoning
Projection surfaces coordination needs (underdetermined, conflict, accepted
constraint, unknown) before those inventions harden as code. ClaimTrace
preserves actor/action/authority distinctions that impact-atom bags erase.

### In scope

- OpenAI-compatible readiness (#957)
- Slice → extract → gate (#958)
- Real-process journey (#959)
- Interface honesty vocabulary (dual lanes, outcome classes)

### Out of scope

- `local_cli` / subprocess extract
- In-process GGUF loading
- Required LiteLLM / LangChain / optimizer
- Silent multi-model failover
- Full Phase I/II ladder as a v0.2 gate
- Capability activation / release ship claim

---

## 5. Building blocks

| Block | Responsibility | Must not |
|---|---|---|
| Bot CLI | Mutating configure / inspect / repair / disable | Own truth after submit |
| Readiness store | Non-secret ready state + pins | Store secrets; imply extract quality |
| Secrets store | API keys | Appear in readiness projection |
| ReasoningProjectionService | Orchestrate 958a–c + handoff | Mutate ledger |
| OpenAI-compatible client | HTTP call | Silently change model |
| Operator model server | Inference | Live inside the daemon process |
| Admit gate (958c) | Validate / reject | Semantic repair |
| SMR kernel (landed) | Align, project, bounded Alloy | Claim global safety; mutate Product |
| MCP / Dashboard | Render | Recompute; grant authority |

---

## 6. Runtime path

```text
[Operator CLI] ──config──► [Daemon readiness]
                                │
                                │  HTTP OpenAI-compatible
                                ▼
                         [Model server · operator]
                                │
                                ▼ untrusted JSON
                         [Admit gate]
                                │
                                ▼ validated ClaimTrace
                         [Landed SMR / preflight]
                                │
              ┌─────────────────┼─────────────────┐
              ▼                 ▼                 ▼
           [MCP]          [Dashboard]      [Ledger · not written]
```

### Pipeline stages

| ID | Stage | Input | Output |
|---|---|---|---|
| 958a | Deterministic slice + profile bind | Plan bytes, readiness pins | Extraction request + profile |
| 958b | HTTP extract | Profile + bounded source | **Untrusted** ClaimSemanticTraceV1 |
| 958c | Admit gate | Untrusted + source units | **Validated** ClaimTrace or typed refusal |
| SMR | Landed analysis | Validated trace + accepted base | Derived findings / preflight sections |

### Trust classes

| Artifact | Class | Must not |
|---|---|---|
| HTTP model body | Untrusted | Skip admit gate |
| Post-gate ClaimTrace | Candidate / disposable | Become Product history |
| Alloy / findings | Derived advisory | Claim global safety / approval |
| Decision / signoff | Canonical | Be written by extract path |

### Failure classes (distinct)

| Failure | Stage |
|---|---|
| not_configured / unavailable | Readiness |
| unsupported_extraction_profile | 958a |
| provider timeout / invalid HTTP body | 958b |
| trace_invalid / provenance_unresolved / coverage incomplete | 958c |
| model_unavailable / capacity_exceeded | Kernel (existing) |

### Analysis identity

```text
analysis_key ⊇ {
  source_bytes_digest,
  base_url_digest, model_id, provider_config_digest,
  prompt_template_id, output_schema_id, decoding_profile_id,
  accepted_smr_base   // when deep path runs
}
```

Any change invalidates prior identity; no silent cache reuse.

---

## 7. Deployment

| Component | Where it runs |
|---|---|
| Daemon + CLI | Operator machine / local product runtime |
| Model server | Operator (loopback or remote HTTPS) |
| MCP host | Coding agent environment |
| Dashboard | Hosted SPA or local harness; **read-only** readiness |
| Optional gateway | Operator infra in front of models |

Bicameral ships **no** inference server package for v0.2.

---

## 8. Cross-cutting rules

| Concept | Rule |
|---|---|
| Profile identity | Pin model, base_url digest, prompt/schema/decoding; reject default/latest |
| Non-laundering | Impact-search ranks ≠ deep-semantic findings ≠ acceptance |
| Coordination outcomes | underdetermined · conflict · accepted_constraint · unknown |
| Extraction honesty | inject · exact_reuse · fragment+delta · full · unavailable (stub unused as typed unavailable) |
| Secrets | Daemon custody; never in readiness projection |
| Efficacy | Separate evidence lane (`SRC-EFFICACY-01`); not ship gate for path correctness |

---

## 9. External dependencies

| Layer | Dependency | Required? |
|---|---|---|
| HTTP client (Rust) | `async-openai` (custom base_url) or thin equivalent | Yes (pick one) |
| JSON | serde / serde_json | Yes |
| Secrets | Existing daemon store | Yes |
| Inference | Ollama / LM Studio / vLLM / OpenAI / … | Operator-owned |
| Gateway | LiteLLM etc. | Optional operator infra |
| Observability / optimizer | Langfuse · DSPy | No on hot path |

### Out of path

`local_cli` · subprocess extract · in-process GGUF · required LiteLLM ·
optimizer hot path · visibility SaaS as authority · silent multi-model failover

---

## 10. Quality requirements

| ID | Requirement | Evidence |
|---|---|---|
| Q1 | Content-free validate never sends Plan/repo content | #957 contract tests |
| Q2 | Untrusted body cannot skip gate | #958c reject matrix |
| Q3 | Gate does not write ledger | Accepted-store digest invariant |
| Q4 | Profile change invalidates analysis identity | Digest tests |
| Q5 | Production path not fixture-selected success | #958 + #959 |
| Q6 | Dashboard cannot mutate readiness | Boundary tests / review |

---

## 11. Risks

| Risk | Mitigation |
|---|---|
| Local servers with tiny default context | Operator docs; typed failures; timeouts |
| Structured-output gaps | Best-effort provider schema + hard admit gate |
| Gateway silent failover | Forbid in profile policy; pin exact model_id |
| Scope creep into kernel | Explicit non-goal on #958 |

---

## 12. Operator setup (minimal how-to)

1. Run a model server (example: Ollama on `http://127.0.0.1:11434`).
2. Ensure the model is pulled and context is large enough for extract prompts.
3. Via Bot CLI: set `base_url` (e.g. `…/v1`), exact `model_id`, secret if required.
4. Run content-free validate; fix typed failures until ready.
5. Invoke managed preflight / projection on a version-bound Plan.
6. Expect dual-lane read-only results; never treat no-counterexample as global safety.

Full first-run tutorial deferred until #959 journey receipts exist.

---

## 13. Implementation index

| Ticket | Focus | Obligations (partial) |
|---|---|---|
| #1064 | Product Design freeze | — |
| #957 | Readiness | `SRC-PROFILE-01` |
| #958 | Pipeline 958a–c | SOURCE, TRACE, PROFILE, VALIDATE (gate) |
| #959 | Real-process journey | Journey evidence |

---

## 14. Glossary

| Term | Meaning |
|---|---|
| ClaimSemanticTraceV1 / ClaimTrace | Source-grounded candidate hypergraph; untrusted until gate |
| ExtractionProfileV1 | Pinned extract identity (model, base_url, prompt, schema, decoding, config digest) |
| OpenAI-compatible | HTTP API shaped like OpenAI chat completions (structured output where supported) |
| Admit gate | Deterministic validation of untrusted trace before kernel |
| Coordination outcome vocabulary | underdetermined, conflict, accepted_constraint, unknown |
| local_cli | **Out for v0.2 ClaimTrace** — subprocess extract path (ADR-0030) |

---

## Document control

| Version | Date | Change |
|---|---|---|
| 1.0 | 2026-08-10 | Accepted v0.2 architecture; OpenAI-compatible only (ADR-0030) |
