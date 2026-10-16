# ADR-0020: Gateway Authority Boundary

**Date:** 2026-06-04  
**Status:** proposed  
**Level:** L1  
**Extends:** ADR-0004 HITL boundaries, ADR-0005 event-store/review separation, ADR-0007 governance flow
**Updated:** 2026-06-24 — generalized as the local gateway slice of the shared action authorization policy.
**Amended:** 2026-07-06 — #698 network-origin read-route auth (see the amendment note under "Required HTTP Behavior").

## Problem

The gateway currently accepts review and dashboard HTTP commands that can change
canonical governance state without an authenticated, capability-checked actor.
This violates ADR-0007's allowed-command gating invariant: review surfaces may
emit commands, but they are not themselves canonical authorities.

The highest-risk paths are:

- `POST /api/v1/review`, which accepts any `ReviewCommand` and mutates
  `ReviewState` while trusting client-supplied `issued_by`.
- `POST /api/v1/dashboard/command`, which accepts commands such as
  `accept_candidate`, `approve_signoff`, `reject_signoff`,
  `supersede_decision`, and `resolve_compliance` and directly materializes
  lifecycle changes.
- Event-store and audit entries that use hardcoded actors such as `"dashboard"`
  or `"gateway"`.
- `Decision.promoted_by` being derived from `reason` in some paths, conflating
  rationale with actor identity.

This is acceptable only as a temporary scaffold. Once external producers,
dashboard users, or MCP clients can reach the gateway, unauthenticated canonical
mutations become a governance-forgery bug.

## Decision

Every route that can accept a command affecting canonical governance state or an
external egress effect must derive an explicit `ActorContext` before evaluating
or materializing the command. `ActorContext` is one input to the shared action
authorization policy; target API credentials, source-adapter credentials, and
projection profiles are not substitutes for actor approval.

Conceptually:

```rust
struct ActorContext {
    actor_id: String,
    authn_method: AuthnMethod,
    capabilities: BTreeSet<GatewayCapability>,
    source_surface: SourceSurface,
}
```

`ActorContext` is the semantic contract. A v0.1 local gateway implementation may
derive it from a local bearer token plus request headers, but the headers are an
implementation slice, not the product architecture. Future implementations may
derive the same context from CLI session identity, dashboard login, MCP client
credentials, authenticated remote review callbacks, OAuth, SSO, or hosted runner
identity.

The gateway must preserve the actor from `ActorContext` in all accountability
surfaces:

- `EventStoreEntry.actor`
- audit receipt `actor`
- `Decision.promoted_by`
- review command history, when a submitted review command supplies or records
  actor identity

The `reason` field is only rationale. It must never be used as actor identity.

## Route Scope

### In Scope For Issue #108

Issue #108 implements the gateway authority boundary for:

- `POST /api/v1/review`
- `POST /api/v1/dashboard/command`

These are the routes that currently allow review and dashboard clients to
perform owner/member governance transitions without proving actor authority.

### Explicit Follow-Ups

`POST /api/v1/bind` also appends a canonical event, but binding authority depends
on graph/runtime verification and ADR-0010 evidence semantics. It should receive
its own authority pass rather than being mixed into #108.

`GET /api/v1/compliance/evaluate` and
`GET /api/v1/compliance/decision/:decision_id` currently update in-memory
derived compliance state from GET handlers. That is a projection/API cleanup
issue, not the primary canonical mutation hole in #108.

Issue #109 remains separate. Ingest guards for size, rate limits, canary checks,
and sensitive-data rejection are not part of this ADR's first implementation
slice.

## Security Model

The dashboard, MCP, CLI, and future Slack or web surfaces are HTTP command
producers. They are not canonical authorities.

A dashboard served by the gateway on the same origin is still a browser client.
Serving UI assets from the gateway does not by itself authenticate the user or
grant review capability. The dashboard must submit review commands with actor
context.

For v0.1 local-first use, a local bearer token is sufficient as a first
defense against unauthenticated network callers. This protects the gateway
against accidental exposure and straightforward external POSTs. It does not
solve same-user browser compromise, multi-user team auth, OAuth, SSO, hosted
RBAC, or enterprise session policy.

Non-loopback gateway serving must fail closed unless an explicit authority
configuration is present. A warning is not enough when mutation routes can
materialize canonical governance events.

Secrets should not be written into workspace `.bicameral/config.yaml` by
default, because that file may be reviewed, shared, committed, or synced across
users. The first implementation should prefer `BICAMERAL_GATEWAY_TOKEN` or
user-local secret storage. Workspace config may record auth mode and safe
non-secret policy settings.

## Required Enforcement Layers

The gateway authority boundary has separate checks. They must not collapse into
a single "token present" boolean.

1. Authentication: the request proves it is allowed to speak for some local
   actor source.
2. Actor attribution: the gateway records which actor is accountable for the
   command.
3. Capability check: the actor has a capability for the command class.
4. State gating: the command is legal for the current candidate or decision
   state.
5. Action authorization policy: workspace governance policy may impose stronger
   review rules, approval classes, target-state checks, or egress restrictions.
   Full policy-engine integration can remain a follow-up, but #108 must leave a
   clear seam for it.

These checks also apply to authenticated remote review actions after provider
callback verification and actor binding. They do not apply to
`ExternalIngestEnvelope` as actor approval, because source ingest is evidence
submission rather than a review-command surface.

## Minimal Capability Vocabulary

The first implementation should use a narrow capability vocabulary:

| Capability | Commands |
|---|---|
| `review:candidate` | `accept_candidate`, `reject_candidate`, candidate review approve/reject |
| `review:signoff` | `approve_signoff`, `reject_signoff` |
| `review:decision` | `supersede_decision` |
| `review:compliance` | `resolve_compliance` |
| `review:context` | `request_context`, `assign_reviewer`, `escalate`, `request_evidence` |

These capabilities are intentionally coarse for v0.1. They are not a final RBAC
model.

## Required HTTP Behavior

Mutation routes in scope must return:

- `401 Unauthorized` when actor context cannot be authenticated.
- `403 Forbidden` when authentication succeeds but the actor lacks the required
  capability.
- `404 Not Found` when the target candidate or decision does not exist.
- `409 Conflict` when the target exists but the requested state transition is
  not legal from the current state.
- `400 Bad Request` for unknown or malformed commands.

Unknown dashboard commands must not return `202 Accepted`. Accepting an unknown
command creates false authority and hides integration drift.

Read-only dashboard projection routes may remain unauthenticated in the first
implementation unless a later product decision requires stricter local privacy.
This ADR only requires authority checks for mutation routes.

**Amendment (2026-07-06 — #698, network-origin reads).** That "later product
decision" has now been made for the cloud-served hosted dashboard's direct data
plane (ADR-0025). Read-only projection routes exposed to a **network origin**
(the browser on `app.bicameral-ai.com` pulling from
`http://localhost:37373/api/v1/hosted-bridge/*`) are **not** unauthenticated.
They require both:

1. an allowlisted request `Origin` — `DEFAULT_TRUSTED_ORIGINS`
   (`app.bicameral-ai.com`, `staging.bicameral-ai.com`) plus any operator-configured
   `custom_origins` (`crates/bicameral-api/src/hosted_bridge.rs`), enforced by the
   hosted-bridge CORS middleware; and
2. a valid bridge session token — `validate_bridge_session`
   (`crates/bicameral-gateway/src/routes.rs`): missing/unknown token → `401`,
   revoked/expired → `403`.

Loopback `/api/v1/dashboard/*` projection routes keep their original posture (may
remain unauthenticated in the first implementation) and are **not** exposed
cross-origin. Signed off on #698 by the security owner; this amendment records the
decision so ADR-0020 no longer contradicts the direct-data-plane spec. Supports
ADR-0025 and the app-level SOC2 control attestation.

## State Gating Requirements

The first implementation should enforce at least the states already implicit in
the route code:

- `accept_candidate` requires `CandidateState::Proposed`.
- `reject_candidate` requires a candidate that has not already been promoted.
- `approve_signoff` requires `SignoffState::Proposed`.
- `reject_signoff` requires a decision whose signoff is not already terminal.
- `supersede_decision` requires an existing decision that is not already
  superseded.
- `resolve_compliance` requires an existing decision and must preserve honest
  grounding/compliance states.

If a command is not implemented as a canonical transition yet, the gateway
should reject it with `400 Bad Request` or a typed unsupported response rather
than accept it as if it were applied.

## Implementation Handoff For #108

This section is intended for the factory implementation worker after human
acceptance of this ADR.

### Files To Inspect

- `crates/bicameral-gateway/src/routes.rs`
- `crates/bicameral-gateway/src/state.rs`
- `crates/bicameral-gateway/src/lib.rs`
- `crates/bicameral-runtime/src/daemon.rs`
- `crates/bicameral-config/src/schema.rs`
- `crates/bicameral-api/src/dashboard.rs`
- `crates/bicameral-api/src/review.rs`
- `dashboard/src/lib/api.ts`
- `dashboard/src/lib/types.ts`
- `crates/bicameral-gateway/tests/mcp_v02_integration.rs`

### Required Code Changes

1. Add gateway authority types, either in a new gateway module or a focused
   section of `state.rs`/`routes.rs`:
   - `ActorContext`
   - `AuthnMethod`
   - `SourceSurface`
   - `GatewayCapability`
   - an extraction/validation helper for in-scope mutation routes
2. Add non-secret gateway auth configuration to `GatewayConfig`, and pass any
   runtime auth settings into `AppState` from the daemon.
3. Support a v0.1 local auth source such as `BICAMERAL_GATEWAY_TOKEN`. Do not
   persist generated secrets into workspace config by default.
4. Guard `POST /api/v1/review` before mutating review state.
5. Guard `POST /api/v1/dashboard/command` before parsing and applying
   canonical commands.
6. Replace hardcoded event/audit actors for in-scope commands with
   `ActorContext.actor_id`.
7. Ensure `Decision.promoted_by` comes from `ActorContext.actor_id`, never
   `DashboardReviewCommand.reason`.
8. Change unknown dashboard commands from accepted audit-only no-ops to
   `400 Bad Request`.
9. Update dashboard client code so it can send the local auth credential and
   actor context for mutation requests.

### Required Tests

Negative tests:

- Unauthenticated `accept_candidate` fails and creates no decision or event.
- Bad token fails.
- Valid token with missing capability fails.
- A `reason` value that looks like an actor does not affect event actor,
  audit actor, or `Decision.promoted_by`.
- Unknown dashboard command returns `400 Bad Request`.
- Unauthenticated `/api/v1/review` mutation fails.

Positive tests:

- Authorized `accept_candidate` succeeds.
- The resulting `AcceptCandidate` event actor is the authorized actor.
- The resulting `Decision.promoted_by` is the authorized actor.
- Authorized `approve_signoff` succeeds with `review:signoff`.
- Read-only dashboard projection routes still work without authority
  credentials, unless the implementation deliberately chooses stricter local
  privacy and documents that choice.

### Validation Commands

```sh
cargo fmt --all -- --check
cargo test -p bicameral-gateway --locked
cargo test --workspace --locked
cargo clippy --workspace --locked -- -D warnings
```

## Factory Issue Comment Draft

After human acceptance of this ADR, comment on issue #108 with:

````markdown
Human design review complete. Implement #108 according to ADR-0020:

- In scope: `POST /api/v1/review` and `POST /api/v1/dashboard/command`.
- Out of scope: `/api/v1/bind`, compliance GET mutation cleanup, #109 ingest guards, OAuth/JWT/team RBAC.
- Required invariant: mutation routes derive an `ActorContext` before applying canonical transitions.
- `reason` is rationale only; event actor, audit actor, and `Decision.promoted_by` must come from `ActorContext`.
- Missing auth = 401; missing capability = 403; illegal transition = 409; unknown command = 400.

Required validation:

```sh
cargo fmt --all -- --check
cargo test -p bicameral-gateway --locked
cargo test --workspace --locked
cargo clippy --workspace --locked -- -D warnings
```

Please open a draft PR and report validation results and residual risks back on this issue.
````

Do not move #108 to `agent-ready` until this ADR is accepted or merged.

## Non-Goals

- Do not implement OAuth, SSO, JWT validation, hosted identity, or enterprise
  RBAC in #108.
- Do not redesign event-store semantics.
- Do not make the dashboard a canonical authority.
- Do not solve #109 ingest hardening.
- Do not fold binding authority into this issue unless a human explicitly
  broadens the scope.
- Do not treat a local bearer-token implementation as the final auth
  architecture.

## Consequences

Positive:

- Gateway mutation routes stop accepting unauthenticated canonical transitions.
- Audit and event-store actors become accountable and replay-visible.
- The dashboard remains a review surface rather than an authority source.
- Devin/factory implementation can proceed from a narrow, human-reviewed
  contract.

Tradeoffs:

- Local dashboard UX needs a way to supply the local credential for mutation
  commands.
- Existing tests that assume unauthenticated dashboard mutation works must be
  updated.
- Full workspace policy integration remains incomplete until follow-up work
  connects `ActorContext` to the governance engine.
