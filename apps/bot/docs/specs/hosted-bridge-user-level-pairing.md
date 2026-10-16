# Hosted Bridge User-Level Pairing Spec

Date: 2026-06-25

Status: draft

Level: L2 cross-repo contract

Owns: the user-level hosted bridge pairing contract between a remote browser
origin and the local Bicameral daemon, including the state split between
user-level pairing, project-level registration, and workspace-level binding.

Source decisions:
- BicameralAI/bicameral-bot#539 owner decision and user-level pairing addendum
- BicameralAI/bicameral-bot#790 pending pairing / local-confirmation contract
- BicameralAI/bicameral-bot#865 pasted-code contract refinement
- BicameralAI/bicameral-bot#866 terminal confirmation adapter
- BicameralAI/bicameral-cloud#8 companion hosted-side boundary
- BicameralAI/bicameral-cloud#184 terminal browser journey (owns executable
  dashboard -> pasted-code terminal confirmation -> app acceptance evidence)

Related context:
- `docs/specs/explicit-project-registration-hosted-local-discovery.md`
- `docs/specs/bot-mcp-data-flow-runtime-architecture.md`
- `docs/domain/runtime-protocol.md`
- ADR-0002 (agent surfaces and bot runtime interface)
- ADR-0003 (local code grounding and cloud conflict boundary)
- ADR-0007 (substrate-neutral governance flow)
- ADR-0020 (gateway authority boundary)
- ADR-0023 (installation and setup state machine)

Governance gate: `bic:spec-governance-gate` advisory/no-op; Qor integration
disabled pending factory policy revision.

## Reading Guide

Read `Runtime Invariant`, `What Bridge Pairing Is Not`, and `User / Project /
Workspace State Split` first. The main invariant is:

```text
Bridge pairing is user/operator-level.
It is not project membership, project selection, workspace binding,
source authority, signoff, or event-store authority.
```

## Runtime Invariant

Hosted web may render, request, and relay. The local daemon validates,
authorizes, materializes, and projects Bicameral state.

No hosted browser surface may gain direct authority to write accepted events,
create Decisions, approve signoff, resolve compliance, claim no-conflict, or
bypass the local daemon's action authorization policy.

Every canonical mutation still passes through bot-owned action authorization
and selected event-store materialization, regardless of whether the request
originates from MCP, CLI, dashboard, or a paired hosted bridge.

## What Bridge Pairing Is Not

Bridge pairing establishes a user-level connection between a remote browser
origin and the local Bicameral daemon runtime. It grants the browser session
the ability to list locally registered projects, view setup/readiness state,
and request governed actions subject to local policy.

Bridge pairing is **not**:

- **project membership**: pairing does not join or create a Bicameral project;
- **project selection**: pairing does not select a `ProjectRegistration`;
  project selection happens after pairing through a narrower flow;
- **workspace binding**: pairing does not bind a local code folder or grant
  filesystem access; `LocalWorkspaceBinding` happens later, only when code
  grounding or preflight needs local files;
- **source authority**: pairing does not create `SourceEvidence`,
  `BindingEvidence`, or `SourceBinding`;
- **signoff or review authority**: pairing does not approve signoff, accept
  candidates, resolve compliance, or supersede decisions;
- **event-store authority**: pairing does not write accepted events or create
  canonical Decision Ledger entries;
- **permission to bypass action authorization**: every governed action request
  through the bridge still passes through the bot-owned action authorization
  policy (ADR-0020).

## Decision

The hosted bridge is a **paired route family on the bot gateway**, not a
separate authority surface and not the same contract as external source ingest
(`ExternalIngestEnvelope`).

This decision was established by the #539 owner decision and addendum. This
spec persists that decision as durable bot authority.

```text
Same bot gateway runtime:               yes
Same route/contract as evidence ingest:  no
Separate authority surface:              no
```

The bot gateway may host the bridge in the same process and HTTP server as
other gateway routes. The separation is contractual, not process-level:

```text
Bot gateway runtime
  -> ExternalIngestEnvelope routes
       evidence-only, no actor authority
  -> local ToolRequest routes
       local actor/session authority
  -> hosted bridge routes
       browser-origin pairing/session authority
```

## User / Project / Workspace State Split

Bridge pairing operates at the user/operator level. Project registration and
workspace binding are separate, later concerns. The three levels are:

```text
User-level (bridge pairing):
  hosted_bridge_unpaired
  pairing_pending        (transient daemon-memory request; not canonical)
  hosted_bridge_paired
  hosted_bridge_revoked

Project-level (after pairing):
  unregistered
  pending_registration
  registered_project
  registration_repair_required
  normal_project_running

Workspace-level (after project registration):
  local_workspace_unbound
  local_workspace_bound
  local_workspace_repair_required
```

### User-Level States

| State | Meaning | Allowed next |
| --- | --- | --- |
| `hosted_bridge_unpaired` | Hosted web can see only pairing/setup affordances; it cannot read local projections or request governed actions | `pairing_pending` |
| `pairing_pending` | A valid hosted `POST /pair` created a transient daemon-memory request. The daemon returned an opaque request handle, an opaque poll proof, a browser-displayed pairing code, an expiry, and capability/version info — never a session or token. The daemon retains only the code digest. Code matching locates the request but does not approve it. | `hosted_bridge_paired` (explicit local approve) · `hosted_bridge_unpaired` (deny/expiry) |
| `hosted_bridge_paired` | An explicit local-operator approval minted a bridge session; the initiating browser then retrieved the transient session handoff. Hosted web may list locally registered projects and request setup/repair flows allowed by local policy | `hosted_bridge_revoked` |
| `hosted_bridge_revoked` | Prior hosted pairing is invalid and must not be reused for reads or actions | `hosted_bridge_unpaired` (re-pair) |

`pairing_pending`, along with its `denied`, `expired`, and `update_required`
terminal request outcomes, are transient request/UI outcomes held only in
daemon memory. They are **not** canonical Decision, project, workspace,
provider, source, signoff, event-store, or Cloud state, and they are never
durably persisted.

### Progression

```text
1. Hosted browser POSTs /pair            -> pairing_pending (202, handle+proof)
2. Browser displays a short code         -> pairing_pending (no authority)
3. User pastes code in local terminal    -> matched origin/account (no authority)
4. User explicitly approves locally     -> hosted_bridge_paired
5. Browser polls status, gets handoff    -> transient session token (once)
6. User selects or creates a project     -> registered_project
7. User binds a local code folder (later)-> local_workspace_bound
```

A denied, expired, or protocol-incompatible request yields a typed terminal
outcome that never reveals a token, and returns the user-level state to
`hosted_bridge_unpaired` for recovery.

Project selection and `ProjectRegistration` happen after pairing.
`LocalWorkspaceBinding` happens later still, only when a feature needs local
code grounding or preflight access.

## Hosted Bridge Data Flow

```text
hosted web (remote browser origin)
  |
  v POST /api/v1/hosted-bridge/pair
hosted bridge route family on bot gateway
  smuggling reject, origin allowlist, account, CSRF, protocol compatibility
  |
  v 202 Accepted: {status: pending, request_handle, poll_proof, pairing_code,
  |                expires_at, protocol_version, capabilities, local confirmation affordance}
  |                (never a session, never a token)
  v
transient daemon-memory pending request
  |
  +--> local operator confirmation adapter (foreground terminal or CLI)
  |      GET  /pair/pending                    (local only; safe summaries)
  |      POST /pair/code/lookup {pairing_code} (local only; rate limited)
  |      POST /pair/decision {approve|deny, request_handle, nonce}
  |      code lookup shows requesting origin, requesting account,
  |      allowed + forbidden capabilities; a separate [y/N] decides
  |         approve -> mint bridge session (local origin only)
  |         deny    -> typed denied outcome, no token
  |
  +--> hosted browser polls
  |      GET /pair/status?request_handle&poll_proof (must match origin+account)
  |      pending | denied | expired | update_required -> no token
  |      approved + live session -> retry-safe transient token handoff (once path)
  v
local operator-home bridge session
  list registered projects, setup state, repair affordances
  |
  v
explicit project selection or registration
  ProjectRegistration required for project mode
  |
  v
optional local workspace binding
  only when code grounding/preflight needs local files
  |
  v
local daemon action authorization policy
  actor, capability, target state, and command checks
  |
  v
governed command handler or projection reader
  |
  v
selected EventStoreAdapter or derived read model
  |
  v
typed response to hosted web
```

## Hosted Bridge Route Requirements

Hosted bridge routes must add checks that ordinary local ToolRequest routes and
external ingest routes do not provide by themselves:

- **Origin allowlist and CORS validation**: only approved hosted domains and
  customer-owned domains may reach bridge routes.
- **CSRF and replay protection**: browser-origin traffic requires anti-forgery
  and replay mitigation.
- **Pending-first pairing**: a valid `POST /pair` never returns a session or
  token. It creates a transient daemon-memory request and returns `202` with an
  opaque request handle, an opaque poll proof, an expiry, capability/version
  info, a short browser-displayed pairing code, and a local confirmation
  affordance. The daemon stores only the normalized code digest.
- **Explicit local-operator confirmation**: a pairing-code match only locates
  the pending request. The local terminal must then show requesting origin,
  account, capability summary, and expiry before a separate default-deny
  Approve/Deny action may transition the request. Hosted, cross-origin,
  replayed, malformed, mismatched, expired, and rate-limited attempts fail
  closed. The terminal never prints or pre-fills the browser code.
- **Retry-safe transient token handoff**: after approval, the initiating
  browser retrieves the session token only through the approved status-poll
  response body. The token never appears in URLs, logs, analytics, local/session
  storage, Cloud persistence, or error reporting. Repeated valid polls return
  the same token while the session is live; a second approval never re-mints.
- **Local pairing lifecycle with expiry and revocation**: the daemon tracks
  pairing sessions and enforces expiry/revocation. Abandoned pending requests
  expire and are cleaned up.
- **Capability handshake scoped to hosted bridge support**: the daemon
  advertises bridge-compatible capabilities and the hosted-bridge protocol
  version. Incompatible old consumers/daemons receive a typed
  `update_required` outcome instead of silently receiving a session.
- **Cloud account or hosted session mapped to local actor only through local
  daemon policy**: cloud identity is not accepted directly as a local actor;
  it is an input to local confirmation.
- **Typed error outcomes**: wrong-daemon, wrong-project, wrong-account,
  unsupported-command, and unpaired-origin errors are typed and actionable.
- **Redacted local logs and diagnostics**: bridge logs must not leak provider
  tokens, local paths, source text, or event-store contents.

## UX Shape

The first prompt is "Connect this browser to your local Bicameral", not
"Connect this browser to this workspace".

```text
Hosted web:
  Connect this browser to your local Bicameral
  (POST /pair -> pending; then wait for local approval)

Hosted web:
  Pairing code: K7M2-PQ8R
  Paste this into the running Bicameral gateway, or run:
    bicameral pairing approve

Foreground local gateway / standalone pairing CLI:
  A hosted browser is waiting to pair.
  Paste the code shown in the browser: <hidden input>

  Pairing request matched:
    Requesting origin:  <requesting hosted origin>
    Requesting account: <requesting cloud account>

  Allowed:
    - list locally registered Bicameral projects
    - show setup/readiness status
    - request project registration or repair
    - request governed actions after local policy checks

  Forbidden:
    - provider secrets
    - filesystem access
    - workspace/code access
    - Decision approval without policy checks

  Approve? [y/N]

After local approval:
  The initiating browser polls status and receives a one-time transient
  session handoff. Hosted web then shows the local project list and setup
  repair prompts. Code workspace binding is offered only inside a project
  when a feature needs local code grounding.
```

The terminal/CLI is the canonical local confirmation adapter. A locally served
web component may remain as an optional adapter, but it must use the same
daemon lookup/decision transition and cannot weaken the pasted-code binding.
The live [Figma Hosted Access & Pairing journey](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=19-144)
renders dashboard code display, hidden terminal entry, matched identity and
capability review, separate default-deny approval, and the transient `/app`
handoff. Cloud issue #184 owns executable browser acceptance for that design;
this spec and its contract tests remain supporting evidence.

## Runtime Architecture Conformance

| Requirement ID | State / transition | Required behavior | Prohibited fallback | Evidence obligation |
| --- | --- | --- | --- | --- |
| `CONF-PAIR-001` | `pairing_pending` | Daemon generates an eight-symbol Crockford Base32 code, returns it only to the initiating browser, and retains only its SHA-256 digest | Plaintext persistence, logs, URLs, analytics, or Cloud storage | producer contract |
| `CONF-PAIR-002` | pending lookup | Only a local, rate-limited code lookup returns matched origin/account/capabilities/expiry | Hosted lookup, code-as-token, or implicit approval | negative contract |
| `CONF-PAIR-003` | local decision | Code match and explicit default-deny decision remain separate; only explicit approval mints a session | auto-approve, Cloud approval, or session mint on lookup | real-process integration |
| `CONF-PAIR-004` | operator adapter | Foreground gateway accepts hidden pasted input without blocking HTTP; background service uses the standalone CLI | terminal echo/pre-fill or TTY-required service startup | real-process integration |
| `CONF-PAIR-005` | browser handoff | Cloud displays daemon code and continues opaque proof/status polling until approved | Cloud-generated code, legacy confirmation-window dead link, or token persistence | consumer contract + browser acceptance |
| `CONF-PAIR-006` | expiry/retry | Codes expire with pending requests, are one-request scoped, and fail closed on wrong/replayed/rate-limited input | durable approval queue or relaxed compatibility fallback | negative contract + restart/retry |

Workspace/code access is requested later, inside a selected project, only when
code-grounding behavior needs local files.

## Cloud-Side Responsibilities

Cloud-side responsibilities during hosted bridge pairing are limited to:

- **Hosted discovery**: `HostedProjectPreview` team/project preview metadata and
  routing (non-canonical; never membership).
- **Session coordination**: hosted session lifecycle and pairing relay. For
  domain-discovered Product join, Cloud exchanges its authenticated session for
  a narrow, short-lived `ProductJoinTicket` (never the full session token) per
  [`domain-product-onboarding-contract.md`](domain-product-onboarding-contract.md).
- **Team coordination**: non-canonical directory/routing metadata only.
  Membership is a canonical daemon event, not Cloud state. Invite/rendezvous
  metadata is legacy and is not the alpha join default (issue #765); same-domain
  auto-join grants `Product Member` only with no durable pending-approval state.
- **Optional amenities**: hosted settings sync and similar setup conveniences.
  (Encrypted rendezvous relay is a legacy/deferred amenity, not the alpha join
  default.)

Cloud does **not**:

- own canonical Decision Ledger authority;
- own canonical project settings;
- create or accept SourceEvidence, BindingEvidence, or SourceBinding;
- approve signoff or resolve compliance;
- bypass local daemon action authorization;
- store provider tokens, local paths, or event-store contents.

This reaffirms ADR-0003: hosted web does not move local code grounding or
canonical authority into cloud.

## Bridge Must-Not Rules

The bridge must not:

- reuse `ExternalIngestEnvelope` for user-approved review actions;
- allow hosted origin traffic to fall back to legacy local mutation routes;
- let hosted preview metadata become canonical project settings;
- treat cloud identity alone as a local actor;
- create Decisions, signoff, compliance, BindingEvidence, SourceEvidence, or
  external egress without the same governed path used by local surfaces.

## ADR Implications For Follow-Up Implementation

The following ADRs may need amendments or reaffirmation when the hosted bridge
is implemented. This spec names them for follow-up; it does not perform the
amendments.

- **ADR-0002** (agent surfaces and bot runtime interface): may need amendment
  if hosted bridge uses a contract distinct from `ToolRequest` and distinct
  from authenticated remote review actions. The browser-origin bridge is
  closer to an authenticated remote action than to a local `ToolRequest`, but
  it has its own pairing lifecycle and origin-validation requirements.

- **ADR-0003** (local code grounding and cloud conflict boundary): reaffirmed.
  Hosted web does not move local code grounding or canonical authority into
  cloud. Cloud amenities such as encrypted rendezvous relay remain setup
  conveniences, not authority surfaces.

- **ADR-0007** (substrate-neutral governance flow): reaffirmed. Every canonical
  state change still passes through governed review and event-store
  materialization, regardless of whether the request originates from a paired
  hosted bridge.

- **ADR-0020** (gateway authority boundary): likely needs amendment for
  hosted-origin gateway authority behavior, origin/CORS requirements, and
  the hosted bridge route family's relationship to the action authorization
  policy. The bridge routes share the gateway process but add browser-origin
  checks that local ToolRequest routes do not require.

- **ADR-0023** (installation and setup state machine): may need amendment to
  reconcile setup language with user-level hosted bridge pairing. The current
  ADR-0023 describes `bicameral dashboard` as the canonical first-run
  command; hosted bridge pairing may become an alternative entry point that
  still requires the same operator-home bootstrap before project selection.

## Cross-Reference Summary

| Source | What it contributes |
| --- | --- |
| #539 owner decision | Bridge is a paired route family on the bot gateway, not a separate authority surface |
| #539 user-level pairing addendum | Bridge pairing is user/operator-level; project selection and workspace binding happen later |
| BicameralAI/bicameral-cloud#8 | Cloud-side boundary: hosted discovery, session coordination, amenities; no canonical authority |
| `explicit-project-registration-hosted-local-discovery.md` | Project registration contract, hosted bridge section, state split, data flow, and invariants |
| ADR-0002 | ToolRequest is the local tool contract; external review actions are separate |
| ADR-0003 | Local bot owns grounding; cloud owns scale/optimization; amenities are convenience |
| ADR-0007 | One governed path from evidence to canonical state |
| ADR-0020 | Gateway authority boundary and action authorization policy |
| ADR-0023 | Installation and setup state machine |

## Acceptance Criteria

- [x] Spec includes the user/project/workspace state split.
- [x] Spec includes hosted bridge ASCII data flow.
- [x] Spec says bridge pairing is not project membership, project selection,
  workspace binding, source authority, signoff, or event-store authority.
- [x] Spec cross-links #539 and BicameralAI/bicameral-cloud#8.
- [x] ADR implications are named for follow-up implementation.
