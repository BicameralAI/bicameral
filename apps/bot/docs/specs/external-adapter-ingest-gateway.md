# External Adapter Ingest Gateway Contract

**Status:** implemented
**Issue:** #264
**ADR:** ADR-0002 (ExternalIngestEnvelope separation)
**Supported distributed endpoint:** `POST /api/v2/external-ingest`
**Capability endpoint:** `GET /api/external-ingest/capabilities`
**Legacy migration seam:** `POST /api/v1/external-ingest`

## Purpose

Defines the bot-owned landing path for external source adapter output.
External adapters (Jira, Linear, Slack, GitHub, Drive, Notion, Fathom,
meetings, support threads, etc.) submit evidence through this gateway
using `ExternalIngestEnvelope`. This path is separate from the local
`ToolRequest`-based `ingest.submit_local` and enforces a strict
evidence-only boundary.

## Contract

### Input: `ExternalIngestEnvelope`

```json
{
  "source_system": "jira",
  "source_uri": "https://jira.example.com/PROJ-42",
  "content": "Full captured content of the source artifact",
  "content_hash": "sha256:...",
  "evidence": [
    {
      "excerpt": "The constraint text",
      "span_start": 0,
      "span_end": 20,
      "confidence": 0.85
    }
  ],
  "candidate_hints": [
    {
      "title": "Short decision title",
      "body": "Full description of the proposed constraint",
      "level": "architecture",
      "labels": ["tag1", "tag2"]
    }
  ],
  "redaction_receipt": {
    "schema_version": 1,
    "engine": "bicameral-stdlib-redaction",
    "engine_version": "1.0.0",
    "ruleset_id": "fx-sec-001-plus-pii-v1",
    "ruleset_digest": "sha256:<64-lowercase-hex>",
    "input_digest": "sha256:<64-lowercase-hex>",
    "output_digest": "sha256:<64-lowercase-hex>",
    "findings": [{"category": "pii", "action": "tokenized", "count": 1}],
    "structural_fields_preserved": true,
    "completed_at": "2026-07-19T00:00:00Z"
  }
}
```

### Required fields

| Field | Type | Description |
|-------|------|-------------|
| `source_system` | string | Adapter identifier (e.g. `jira`, `slack`, `github`) |
| `source_uri` | string | Provenance URI for the source artifact |
| `content` | string | Full captured content of the source |
| `redaction_receipt` | object | Value-free proof that required pre-Bot sanitization ran; mandatory on protocol v2 |

### Optional fields

| Field | Type | Description |
|-------|------|-------------|
| `content_hash` | string | SHA-256 of content (verified if provided, computed if omitted) |
| `evidence` | array | Evidence excerpts within the content |
| `candidate_hints` | array | Advisory candidate suggestions (not authority) |

## Protocol distribution lifecycle

Bot owns the canonical schema and
`protocol/external-ingest/contract.json`. Integrations must negotiate the
capability report before delivery and require an exact protocol version, schema
digest, and contract fingerprint match. Unknown or skewed contracts fail closed
without source cursor advancement. CI verifies generated-schema parity,
manifest/fingerprint integrity, producer pins, and negative compatibility cases.
The local release checklist lives at
`docs/releases/alpha/external-ingest-protocol-release-checklist.md`.

### Forbidden fields (rejected at gateway)

The following fields are **never** accepted on an `ExternalIngestEnvelope`.
Any payload containing them is rejected with HTTP 403:

| Category | Forbidden fields |
|----------|-----------------|
| Authority | `authority`, `auth_method`, `actor_id`, `session_id`, `policy_scope` |
| Review commands | `review_command`, `tool_command` |
| Signoff | `approve_signoff`, `reject_signoff`, `signoff_state` |
| Compliance | `resolve_compliance`, `compliance_verdict`, `compliance_state` |
| Policy/governance | `policy_grant`, `governance_override`, `canonical_decision`, `decision_id`, `tracking_state` |

## Output: `ExternalIngestResponse`

```json
{
  "source_id": "https://jira.example.com/PROJ-42",
  "snapshot_id": "sha256:abc123...",
  "evidence_ids": ["uuid-1", "uuid-2"],
  "candidate_ids": ["uuid-3"],
  "advisory_results": [
    {
      "candidate_id": "uuid-3",
      "action": "accepted_as_candidate",
      "reason": "External evidence recorded — advisory only",
      "review_state": "proposed",
      "candidate_state": "proposed"
    }
  ]
}
```

## What external ingest produces

1. **Source** record (portable URI-based identifier)
2. **SourceSnapshot** (content-addressed, SHA-256)
3. **SourceEvidence** items (span pointers into the snapshot)
4. **DecisionCandidate** entries (advisory, `proposed` state — requires human review to promote)
5. **ReviewState** for each candidate (governance entry point)
6. **EventStore entry** with `ExternalIngest` event type

## What external ingest cannot do

Per ADR-0002 and the safety model:

- ❌ Emit review commands (accept, reject, approve_signoff, etc.)
- ❌ Carry local actor/session authority
- ❌ Create canonical Decisions (only candidates)
- ❌ Write directly to the event store as authority
- ❌ Resolve compliance or signoff state
- ❌ Grant policy or governance overrides
- ❌ Produce blocking governance results

## Rejection reasons

| Reason | HTTP | Description |
|--------|------|-------------|
| `invalid_payload` | 400 | Missing/malformed fields |
| `empty_evidence` | 422 | No evidence items or candidate hints |
| `injection_canary` | 422 | Prompt-injection canary detected |
| `sensitive_data` | 422 | PII/secret/PHI detected |
| `authority_injection` | 403 | Payload carries authority fields |
| `review_command_injection` | 403 | Payload carries review commands |
| `signoff_injection` | 403 | Payload carries signoff operations |
| `compliance_injection` | 403 | Payload carries compliance resolution |
| `policy_grant_injection` | 403 | Payload carries policy/governance fields |
| `rate_limit_exceeded` | 429 | Token bucket exhausted |

## Common wrong field mappings

External integrations frequently send alias field names that are **not**
part of the `ExternalIngestEnvelope` contract. The gateway detects these
and returns an `invalid_payload` rejection with an actionable hint.

| Wrong field | Correct field | Explanation |
|-------------|---------------|-------------|
| `source_ref` | `source_uri` | The provenance link must be `source_uri`. |
| `body` | `content` | The captured artifact text must be `content`. |
| `text` | `content` | Same as above — use `content`. |
| `description` | `content` | Same as above — use `content`. |
| `source_url` | `source_uri` | The provenance link must be `source_uri`. |
| `source_id` | `source_system` | The adapter identifier must be `source_system`. |
| `evidence[].title` | `evidence[].excerpt` | Evidence items carry `excerpt`, not `title`. |
| `evidence[].body` | `evidence[].excerpt` | Evidence items carry `excerpt`, not `body`. |

### Example: alias rejection

**Request (incorrect):**

```json
{
  "source_system": "jira",
  "source_ref": "https://jira.example.com/PROJ-789",
  "body": "Sprint planning: auth must support SAML SSO",
  "evidence": [
    { "title": "SAML SSO requirement" }
  ]
}
```

**Response (400 Bad Request):**

```json
{
  "reason": "invalid_payload",
  "message": "Invalid ExternalIngestEnvelope: missing field `source_uri` ... Field mapping hint: 'source_ref' is not accepted — use 'source_uri' instead. 'body' is not accepted — use 'content' instead. evidence[].'title' is not accepted — use evidence[].'excerpt' instead. Expected fields: source_system (string), source_uri (string), content (string), evidence (array of {excerpt, span_start?, span_end?, confidence?}), candidate_hints (array of {title, body, level?, labels?})."
}
```

## V1 golden path compatibility

Accepted external ingest produces the same Source → SourceSnapshot →
SourceEvidence → DecisionCandidate records as `ingest.submit_local`.
Candidates enter the same review pipeline: they appear in the ingestion
gate, can be accepted/rejected through review commands issued by local
actors (MCP, CLI, dashboard), and follow the standard V1 advisory
lifecycle.

The difference: external ingest **never** carries the `AuthorityContext`
that local actors provide. Its candidates are always advisory and always
require human review before promotion.

## MCP authority removal

With this gateway, MCP no longer needs hidden source polling, webhook
authority, or Drive runtime authority for external adapter ingestion.
External adapters submit directly to the bot-owned gateway; MCP remains
a local actor surface that reads bot state and issues governed
`ToolRequest` commands.

## See also

- [ADR-0002](../adr/0002-agent-surfaces-and-bot-runtime-interface.md) — Agent surfaces and bot runtime interface
- [Protocol README](../../protocol/README.md) — Surface category separation
- [Bot-MCP data flow spec](bot-mcp-data-flow-runtime-architecture.md) — Runtime architecture
