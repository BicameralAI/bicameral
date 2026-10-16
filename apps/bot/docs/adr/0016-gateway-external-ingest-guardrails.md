# ADR-0016: Gateway External Ingest Guardrails

**Date:** 2026-06-04  
**Status:** proposed  
**Level:** L1  
**Issue:** https://github.com/BicameralAI/bicameral-bot/issues/109  
**Related:** ADR-0002, ADR-0005, ADR-0006, ADR-0014

## Problem

`POST /api/v1/ingest` is the local gateway entry point for untrusted source
content from integrations and agent surfaces. The current gateway accepts the
clean ingest projection and rejects empty evidence, but it does not yet make
the ingest guardrail boundary explicit.

External ingest can provide source evidence and candidate hints. It must not
carry local actor authority, review commands, compliance resolution, signoff
state, canonical decisions, or event-store writes. Without explicit gateway
guards, unknown or nested authority-shaped fields can be silently ignored by
serde before the authority boundary has a chance to reject them, and high-risk
payloads can reach materialization code before deterministic refusal checks run.

## Decision

The gateway owns deterministic external ingest guardrails before projection into
bot evidence or candidate state.

For external ingest, the gateway must inspect the raw request body before
deserializing into the clean ingest request type. The guard layer rejects
payloads that are too large, rate-limited, prompt-injection canary matches,
sensitive-data matches, malformed, or carrying forbidden authority fields.

The clean-projection ingest contract remains valid: external sources may omit
fields that the bot can infer, such as candidate level and evidence spans. The
guard layer is additive and sits before projection. It must not turn external
ingest into a `ToolRequest` surface and must not add actor or review authority.

## Rejection Contract

The gateway should return deterministic typed rejection reasons:

| Condition | Reason | Status |
|---|---|---|
| malformed JSON | `malformed_payload` or existing invalid-payload equivalent | `400 Bad Request` |
| body too large | `size_limit_exceeded` | `413 Payload Too Large` |
| rate limited | `rate_limit_exceeded` | `429 Too Many Requests` |
| prompt-injection canary | `injection_canary_match` | `422 Unprocessable Entity` |
| sensitive data | `sensitive_data` | `422 Unprocessable Entity` |
| forbidden authority field | `forbidden_authority_field` | `422 Unprocessable Entity` |
| empty evidence | existing `empty_evidence` behavior | `422 Unprocessable Entity` |

Rejection messages must be bounded and must not echo secrets or sensitive
content.

## Forbidden Authority Fields

The raw JSON guard must reject forbidden authority-shaped field names anywhere
in the payload, including nested objects and arrays. The forbidden set includes:

- `authority`
- `tool_command`
- `review_command`
- `event_store_write`
- `compliance_resolution`
- `governance_result`
- `signoff_state`
- `candidate_state`
- `decision`
- `decision_id`
- `review_state`

This recursive check prevents attackers or buggy integrations from hiding
authority requests inside otherwise valid source evidence.

## Default Local Policy

The first gateway implementation should use conservative process-local
defaults:

- maximum body size: `1 MiB`;
- rate-limit burst: `10`;
- refill rate: `1 token / second`;
- one global process-local bucket for `POST /api/v1/ingest`.

The gateway must not use attacker-controlled request fields such as `source`,
`source_uri`, or arbitrary JSON metadata as rate-limit keys. Per-actor buckets
can follow after the gateway actor-authority boundary is implemented.

## MCP Precedent

`bicameral-mcp` provides useful deterministic detector precedent but not
authority or persistence precedent. The bot may port or reimplement equivalent
canary and sensitive-data detection for:

- override instructions;
- role impersonation;
- exfiltration directives;
- jailbreak markers;
- secret material such as AWS keys, GitHub tokens, Azure storage connection
  strings, private-key markers, and JWT-like tokens;
- PHI labels such as MRN, patient ID, DOB, and SSN;
- Luhn-valid PAN candidates with false-positive filtering.

The bot must not preserve MCP passive-mode DLQ behavior in this gateway
guardrail decision.

## Consequences

Positive:

- external ingest remains useful while failing closed on high-risk input;
- serde cannot silently discard forbidden authority fields before policy sees
  them;
- guardrail behavior becomes deterministic and testable;
- the ingest gateway stays separate from `ToolRequest` review authority.

Risks:

- simple process-local rate limiting is not sufficient for distributed or
  authenticated deployment topologies;
- deterministic sensitive-data detectors can create false positives and false
  negatives;
- future authenticated actor support will need a second pass over rate-limit
  keying and audit attribution.

## Implementation Notes

Implementation should add a gateway guard module, inspect raw request bytes
before deserialization, preserve existing clean-projection behavior, and add
tests proving rejected payloads create no candidate and append no ingest event.

Required regression coverage:

- oversized payload;
- rate exhaustion;
- prompt-injection canary;
- secret / PHI / PAN sensitive data;
- top-level and nested forbidden authority fields;
- clean-projection fixture still accepted;
- empty-evidence rejection unchanged.

## Non-Goals

- Do not implement actor authentication, OAuth, JWT, SSO, or RBAC here.
- Do not implement #108 gateway actor authority here.
- Do not add MCP passive-mode DLQ behavior.
- Do not build persistent or distributed rate limiting in the first slice.
- Do not redesign `ToolRequest` or `ExternalIngestEnvelope`.
- Do not change review or dashboard canonical mutation routes.
