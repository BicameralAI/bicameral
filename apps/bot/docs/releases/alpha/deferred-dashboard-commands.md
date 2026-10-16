# Deferred Dashboard Commands — Typed JSON Response

Status: alpha
Relates: #382, RFQ #374, RFQ #373

## Summary

Dashboard commands that are recognized but not yet implemented in the alpha
release return HTTP 202 with a typed JSON body instead of an empty response.
This makes deferred behavior explicit for consumers.

## Affected commands

| Command              | Status   | Reason                       |
|----------------------|----------|------------------------------|
| `resolve_compliance` | deferred | `not_supported_in_alpha`     |
| `refresh_source`     | deferred | `not_supported_in_alpha`     |
| `untrack_decision`   | deferred | `not_supported_in_alpha`     |

## Response shape

```json
{
  "status": "deferred",
  "reason": "not_supported_in_alpha",
  "command": "resolve_compliance"
}
```

### Fields

- `status` — always `"deferred"` for commands recognized but not active.
- `reason` — machine-readable reason code (`not_supported_in_alpha`).
- `command` — the command name that was deferred, echoed from the request.

## Governance boundary

Deferred command responses are informational. They do not:

- Mutate compliance state, signoff state, or canonical Decision state.
- Produce governance events or event-store entries.
- Activate compliance resolution or source refresh behavior.

## Rust type

`bicameral_api::dashboard::DeferredCommandResponse` — used by both the
gateway HTTP layer and the `ToolRequest` dispatcher.
