# OAuth And Integration Connections Spec

Principle: Provider authentication is an operator-level capability; project setup and integration setup should consume verified provider connections instead of each inventing its own auth path.

Governance gate: bic:spec-governance-gate advisory/no-op; Qor integration disabled pending factory policy revision.

## Current State

Current integration auth is useful but fragmented.

### Shipped Patterns To Keep

- `secrets_store` persists credentials in the OS keyring with a deterministic test fallback.
- Secret writes emit `SOURCE_AUTH_GRANTED`; revokes emit `SOURCE_AUTH_REVOKED`.
- Google Drive has a real OAuth handshake via `bicameral-mcp source-auth google_drive`.
- Google Drive refreshes expired tokens and re-persists rotated access-token JSON.
- `source-list <source>` exists as a discovery primitive after auth.
- Integration inventory already treats settings as more than on/off auth: discovery, selection, filtering, hooks, active/passive mode, and quotas are first-class concerns.

### Current Gaps

- `source-auth` is source-specific and currently only dispatches `google_drive`.
- GitHub integration auth is still PAT or GitHub App token based, not a first-class OAuth connection.
- The CLI command names are source-ingest oriented, not provider-connection oriented.
- Event-store auth and integration auth are treated as separate surfaces even when they use the same provider.
- There is no canonical connection registry for “GitHub is connected as user X with scopes Y”.
- UI would need to infer provider state from source-specific secrets unless we add a connection model.

## Decision

Bicameral should use one connection model for all OAuth and provider authentication.

Connection purposes:
- event stores: GitHub, Google Drive
- integrations: GitHub PR/issues/commits, Google Docs/Drive ingest, future Slack/Linear/Notion
- discovery: list repos, folders, workspaces, channels, teams

Project setup should never run a bespoke OAuth flow. It asks the connection service whether the required provider is connected. If not, it sends the user to Bicameral-wide Settings.

Integration setup should use the same Settings connection flow.

Project invite resolution is specified separately in
[Rendezvous State Machine](rendezvous-state-machine.md). OAuth may interrupt
that flow, but successful OAuth does not by itself join a project.

## Terminology

- **Provider**: external system identity, such as GitHub or Google.
- **Connection**: user-level authorization grant for a provider.
- **Capability**: what a connection can do, such as `github.repo.read`, `drive.file`, or `drive.docs.read`.
- **Consumer**: project event store, source integration, webhook receiver, or discovery picker that uses a connection.
- **Resource selection**: choosing a repo, Drive folder/file, channel, workspace, or document after auth.
- **Source binding**: project-level approved ingestion configuration for one external resource, such as a GitHub repo, Linear team, Slack channel, or Drive folder/document set.
- **Ingestion owner**: the operator connection used to fetch a managed source binding.
- **Tool ingestion**: member/tool-submitted observations, such as MCP sessions, command runs, or local agent outputs. Tool ingestion is multi-operator by design and is not the same thing as a managed source binding.

## Connection Registry

Store non-secret connection metadata in user-level Bicameral state.

### Default storage location

The connection registry lives at `~/.bicameral/connections.yaml` inside the
operator home directory. This path is resolved by
`BicameralPaths::connections_file()` in the `bicameral-config` crate.

The operator home (`~/.bicameral/`) is the user-level Bicameral control
plane defined in ADR-0023. Connection metadata is operator-scoped — it is
not duplicated per project/workspace.

### Registry shape

```yaml
connections:
  github:
    status: connected
    account_label: jinhongkuan
    capabilities:
      - github.repo.read
      - github.repo.write
    updated_at: 2026-06-13T00:00:00Z
    auth_method: oauth
  google:
    status: connected
    account_label: user@example.com
    capabilities:
      - drive.file
      - drive.docs.read
    updated_at: 2026-06-13T00:00:00Z
    auth_method: oauth
```

Valid `status` values: `connected`, `action_needed`, `not_connected`.

Valid `auth_method` values: `oauth`, `device_flow`, `imported_token`, `pat`.

The Rust types are `ConnectionRegistry`, `ConnectionMetadata`,
`ConnectionStatus`, and `AuthMethod` in the `bicameral-config` crate. The
TypeScript mirror types live in `dashboard/src/lib/types.ts`.

Store secret material only in `secrets_store`.

Suggested secret keys:

```text
source_id="connection.github", key="oauth_token"
source_id="connection.google", key="oauth_token"
```

Existing source-specific secrets can remain as legacy read aliases during migration:

```text
source_id="github", key="api_key"
source_id="google_drive", key="oauth_token"
```

## Unified Settings UI

Settings has a `Connections` section.

Provider cards:
- GitHub
- Google Drive
- future: Slack, Linear, Notion

Each card shows:
- provider name
- connected account label, if known
- status: connected / action needed / not connected
- last checked or last connected time
- actions: connect, reconnect, disconnect

The card should not mention a specific project or integration unless it is showing a short “Used by” list.

Optional “Used by” examples:
- `Acme checkout event store`
- `GitHub PR ingest`
- `Drive Docs ingest`

## Unified OAuth Flow

All OAuth flows should use the same UI state machine.

1. User clicks `Connect <provider>`.
2. Dashboard calls `POST /connections/<provider>/start`.
3. Backend returns an auth session:
   - `browser_url` for local-loopback OAuth, or
   - `verification_uri` + `user_code` for device flow.
4. Dashboard opens the provider auth page or shows a code-copy step.
5. Backend completes the callback or polls the device flow.
6. Backend stores token in `secrets_store`.
7. Backend writes connection metadata.
8. Backend emits `SOURCE_AUTH_GRANTED` or a renamed future `CONNECTION_AUTH_GRANTED`.
9. Dashboard refreshes provider card and returns the user to the calling flow.

Failure states:
- user closes browser
- callback timeout
- provider denies consent
- missing OAuth client configuration
- scope denied or insufficient
- keyring unavailable
- refresh token revoked

Each failure state should show:
- provider name
- short cause
- retry action
- no stack trace
- no token material

## Auth-Session Completion Contract

The auth-session completion contract defines how a started provider auth flow
reaches a terminal result. It covers both device-flow polling (GitHub) and
local-loopback OAuth callback (Google Drive) without requiring every provider
to implement both paths.

### Auth session identity

When `POST /connections/:provider/start` returns `start_url`, the response
includes an opaque `auth_session_id`. This identifier correlates subsequent
polling or callback requests to the original auth session.

```json
{
  "outcome": "start_url",
  "provider": "github",
  "url": "https://github.com/login/device",
  "auth_session_id": "sess-abc-123",
  "user_code": "ABCD-1234",
  "flow_type": "device_flow"
}
```

- `auth_session_id` is server-generated, opaque, and unique per session.
- `flow_type` hints which completion path applies: `"device_flow"` or
  `"local_loopback"`. The dashboard uses this to choose between polling and
  waiting for a redirect.
- `user_code` is present only for device-flow providers.

### Completion endpoints

**Device-flow polling** — `GET /connections/:provider/status/:session_id`

The dashboard polls this endpoint after showing the user code. Responses use
`ConnectionCompletionOutcome`:

- `pending` — user has not yet entered the code; keep polling.
- `connected` — device flow completed; registry and secrets updated.
- `expired` — the device code timed out.
- `denied` — the user rejected the auth prompt.
- `provider_unavailable` — the provider endpoint returned an error.

**Local-loopback OAuth callback** — provider `GET /connections/:provider/callback`

For native Google OAuth, the provider returns the browser to the daemon loopback
URI with an HTTP GET query containing `code` + `state` on success or
`error` + `state` on denial. The provider never receives or echoes Bicameral's
internal `auth_session_id`.

The daemon requires a non-empty opaque `state`, resolves it to the matching
transient provider auth session, and feeds the result into the same
`ConnectionCompletionOutcome` completion path used by the internal callback
service. Missing, unknown, or mismatched state fails closed. A successful code
exchange consumes the transient auth session so the state cannot be replayed.

The default Google loopback URI derives from
`bicameral_config::DEFAULT_GATEWAY_PORT` and is currently
`http://127.0.0.1:37373/api/v1/connections/google/callback`. An explicit
`BICAMERAL_GOOGLE_REDIRECT_URI` override is reserved for controlled
development/recovery topology and must still terminate at the active daemon.

The browser receives only a static success/failure page. Authorization codes,
state values, PKCE material, and provider tokens are never reflected into that
page or durable evidence. Loopback responses are `no-store`, use a restrictive
Content Security Policy, disable referrer propagation, and set `nosniff`.

`POST /connections/:provider/callback` remains an internal/dashboard completion
surface that carries the explicit `auth_session_id`; it is not the redirect
method registered with Google.

The server exchanges the code for tokens (consumed server-side, never logged),
updates `ConnectionRegistry` and `secrets_store`, and returns a
`ConnectionCompletionOutcome` synchronously on the internal path.

### Terminal result vocabulary (`ConnectionCompletionOutcome`)

| Variant              | Meaning                                             |
|----------------------|-----------------------------------------------------|
| `connected`          | Auth succeeded; registry updated, tokens stored.    |
| `action_needed`      | Provider needs further user action.                 |
| `denied`             | User or provider denied consent.                    |
| `expired`            | Auth session timed out.                             |
| `unsupported`        | Provider is unknown or cannot be connected.         |
| `provider_unavailable` | Provider endpoint unreachable or errored.         |
| `pending`            | (polling only) Session still in progress.           |

### Registry and secrets_store update semantics

On `connected`:

1. The server persists token material (access token, refresh token) in
   `secrets_store` using the connection-scoped key
   `source_id="connection.<provider>", key="oauth_token"`.

   The stored value is a versioned JSON envelope (`OAuthTokenEnvelopeV1`,
   `envelope_version: 1`) carrying `access_token`, optional
   `refresh_token`, absolute `expires_at` resolved from the provider
   `expires_in`, optional `token_type`/`scope`, and `obtained_at`.
   Pre-envelope deployments stored the bare access-token string; readers
   must accept both (`parse_stored_oauth_token`), and a successful
   reconnect/refresh rewrites the value as a v1 envelope. A keyring write
   failure or unusable token bundle must fail the completion outcome
   before step 2 — registry metadata may never say `connected` without a
   durably written credential.

   In production the `SecretsStore` is OS-backed (Windows Credential
   Manager, macOS Keychain, Linux Secret Service) with the deterministic
   mapping `{source_id, key}` → service `bicameral.<source_id>`, account
   `<key>`. Keyring unavailability is an explicit failure; there is no
   silent fallback to volatile in-memory storage. `InMemorySecretsStore`
   remains injectable for deterministic tests only.
2. The server writes or updates `ConnectionRegistry` with non-secret metadata:
   `status: connected`, `account_label`, `capabilities`, `updated_at`,
   `auth_method`.
3. The server emits a `SOURCE_AUTH_GRANTED` audit event (or the future
   `CONNECTION_AUTH_GRANTED` equivalent).

**No-secret response invariant:** No `ConnectionCompletionOutcome` variant
contains token material, authorization codes, refresh tokens, API keys,
client secrets, or PAT values. All secret handling happens server-side.

**Credential-backed effective status:** registry metadata and a readable
credential must agree before `connected` is projected. When registry
metadata says `connected` but neither the canonical
`connection.<provider>` secret nor a supported legacy alias is readable
and usable, `GET /connections` and the existing-connected branch of
`POST /connections/:provider/start` project `action_needed` and route
into re-auth. The projection never mutates the durable registry on read.

### Scope boundary

Successful provider auth:

- **Does** update `ConnectionRegistry` and `secrets_store`.
- **Does** emit an audit event.
- **Does not** create a `SourceBinding`.
- **Does not** approve ingestion for any project.
- **Does not** materialize Decision Ledger state.
- **Does not** join the user to a project.

Provider auth is a user-level capability grant. Source approval, project
membership, and ingestion configuration remain separate governed flows.

### Credential-import exclusion

The `ConnectionCompletionOutcome` vocabulary intentionally excludes a
credential-import path. Credential import (`ImportInstructions`) is part of
`ConnectionStartOutcome` only, matching the narrowed start vocabulary
established in #390.

### Provider compatibility

A provider implements either device flow or local-loopback callback — not
necessarily both:

- **GitHub**: device flow. Dashboard polls `/status/:session_id`.
- **Google Drive**: local-loopback OAuth. Provider redirects to `/callback`.
- **Future providers**: implement whichever mechanism fits their OAuth model.
  The `flow_type` hint in `start_url` tells the dashboard which path to take.

## OAuth Method By Provider

### GitHub

Preferred default: OAuth device flow for CLI/local dashboard.

Why:
- no public callback URL required
- works from local dashboard and terminal
- already prototyped in `pilot/demo/electron/github-auth.ts`
- familiar GitHub UX: code + browser confirmation

Required capabilities:
- For GitHub event store: repo read/write capability appropriate to the chosen repo model.
- For GitHub ingest: repo read, PR/issue/commit read.

Legacy support:
- Existing PAT and GitHub App tokens remain importable.
- Imported tokens should create a `connection.github` metadata record with `auth_method: imported_token`.

### Google Drive

Preferred default: local-loopback OAuth for local dashboard.

Why:
- already implemented for Google Drive ingest
- supports refresh tokens
- fits a browser-based dashboard running on localhost

Required capabilities:
- For Drive event store: use the narrowest scope that can create/read/write Bicameral-owned event-store files.
- For Docs ingest: docs read capability and Drive metadata/listing capability as needed.

Scope direction:
- Prefer `drive.file` for event-store files created/opened by Bicameral.
- Avoid broad `drive` or `drive.readonly`.
- Keep Docs ingest scopes separate in capability metadata even if one Google token grants both.

Legacy support:
- Existing `source_id="google_drive", key="oauth_token"` remains readable.
- On next successful refresh or reconnect, migrate to `connection.google`.

## Project Setup Consumption

Create project:
- User selects event store: GitHub or Google Drive.
- Dashboard checks required provider connection.
- If missing, primary action becomes `Connect GitHub` or `Connect Google Drive`.
- After connection, return to the same project setup state.
- Project creation persists event-store configuration, not OAuth credentials.

Join project:
- Invite resolves bootstrap metadata, including event-store location and the provider capability needed to fetch it.
- Dashboard checks provider connection.
- If missing, connect provider first.
- After connection, resume join, validate event-store access, then read current project settings from the event store.

Project setup should not expose OAuth scope details by default. Scope detail belongs in the provider consent screen and optional settings detail.

## Integration Setup Consumption

Integration setup uses the same connection cards.

Example GitHub integration flow:

1. User opens Settings → Integrations → GitHub.
2. If GitHub is not connected, show `Connect GitHub`.
3. After connection, run discovery: list repos.
4. User selects repos.
5. User configures filters, ingest mode, quotas, hooks.
6. Save integration source config.

Example Google Drive integration flow:

1. User opens Settings → Integrations → Google Drive.
2. If Google is not connected, show `Connect Google Drive`.
3. After connection, run discovery: list folders or eligible docs.
4. User selects folders/docs.
5. User configures filters, ingest mode, quotas, hooks.
6. Save integration source config.

## Project Source Bindings

Project source ingestion should be project-owner controlled by default. The
project config should not merely say which provider is connected; it should say
which external resource is approved for ingestion and whose provider connection
is responsible for fetching it.

This keeps source authority distinct from member access:

- The project owner or EM decides which sources enter the project memory.
- Team members do not need to connect every provider just to join or read the
  project.
- A source is ingested once per approved source binding, preventing duplicate
  evidence streams from every member's personal token.
- Evidence provenance can name the source binding and ingestion owner without
  treating every member's local connection as project source authority.

Conceptual project settings shape:

```yaml
sources:
  - id: github_repo_primary
    provider: github
    resource:
      repo: acme/checkout
    ingestion_owner:
      operator_id: priya
      connection_id: connection.github.priya
    approved_by: priya
    mode: active
    filters:
      pull_requests: true
      issues: true
      commits: false
    dedupe_key: github:repo:acme/checkout
```

The `dedupe_key` identifies the provider resource and filter scope, not the
current operator. Transferring a source from one ingestion owner to another
should preserve the source binding identity and historical evidence lineage
after validating the replacement connection has access.

Example UI copy:

```text
GitHub repo: acme/checkout
Ingested via: Priya's GitHub connection
Approved by: Priya
Mode: Active
Status: Healthy
```

If the ingestion owner's token expires, the project source should become
`action needed`; other members should not be prompted to connect that provider
unless they are taking ownership of the source binding or submitting their own
tool observations.

## Tool Ingestion From Members

Tool ingestion is separate from managed source bindings. A project may accept
observations from many operators and tools, such as MCP sessions, CLI commands,
or local agent runs, under the project's review policy.

Tool submissions should carry operator and surface provenance:

```yaml
submitted_by:
  operator_id: alex
  local_actor_label: alex-macbook
surface:
  kind: mcp
  tool: claude
  session_id: sess_123
source:
  kind: mcp_session
  uri: mcp://local/sess_123
evidence:
  tool_call_id: call_456
  excerpt: "..."
```

Multiple users may submit observations, but their submissions do not each define
project source authority. Managed source bindings define recurring ingestion
from external systems; tool ingestion creates reviewable evidence or candidates
from member activity according to policy.

Project settings should therefore distinguish:

- **Sources**: owner-approved recurring external resource ingestion.
- **Tool ingestion**: member/tool-submitted observations shown in activity and
  routed through review policy; not a standalone project resource by default.
- **Connections**: user-level provider grants used by either source bindings or
  local member actions.
- **Channels**: project-level communication or review routes, such as Slack,
  Linear, GitHub issues, or email destinations. Channels are where Bicameral
  posts or listens for workflow communication; Sources are what Bicameral
  ingests as evidence.

There is no durable `ChannelRoute` object in this first stage. A connector,
mod, or tool surface may provide a routing hint for display or review policy,
but that hint is not saved project routing configuration and does not authorize
recurring delivery. `ChannelRoute` should become product vocabulary only after
the product-owner workflow, storage location, authority boundary, update path,
and failure behavior are specified.

## Current Product Decision

The default product model is intentionally small:

- Connections are user-level provider grants. They answer: "Can this operator
  authenticate to GitHub, Google, Slack, Linear, or another provider?"
- Sources are project-level approved evidence intake. They answer: "Which
  external resources are allowed to enter this project's decision memory?"
- Source bindings name an ingestion owner. They answer: "Whose provider
  connection should Bicameral use for this recurring source?"
- Channels are project-level communication routes. They answer: "Where should
  project review, notification, or collaboration traffic happen?"
- Tool submissions are reviewable activity from many members and tools. They
  carry provenance, but they do not become recurring source bindings unless a
  project owner explicitly promotes or configures that path.

For the default team flow, the EM or project owner connects the provider
accounts needed for managed ingestion and chooses the approved Sources. Other
members can join the project without connecting every provider. They only need
their own Connections when they need personal provider access, submit local
tool evidence, repair their own project access, or take ownership of a source
binding.

This avoids two failure modes:

- redundant ingestion, where every member's OAuth token creates the same source
  evidence stream again;
- authority confusion, where a user's ability to connect GitHub is mistaken for
  project approval to ingest a repo.

## Improvement Over Current Integration Auth

Current integration auth can be improved by separating:

- **Connect provider**: authenticate once, store token, expose capabilities.
- **Discover resources**: list repos/folders/channels using that connection.
- **Configure consumer**: project event store or integration source config.

This reduces duplicate OAuth prompts, avoids project-specific auth confusion, and gives every provider the same recovery UX.

## API Shape

Suggested local dashboard API:

```text
GET    /connections
GET    /connections/:provider
POST   /connections/:provider/start
POST   /connections/:provider/callback
GET    /connections/:provider/status/:session_id
POST   /connections/:provider/disconnect
POST   /connections/:provider/refresh

GET    /connections/:provider/resources
```

The auth-session completion endpoints are `callback` (local-loopback) and
`status/:session_id` (device-flow polling). Both return
`ConnectionCompletionOutcome`. See the Auth-Session Completion Contract section
above for the full contract.

Provider resource examples:

```text
GET /connections/github/resources?type=repo
GET /connections/google/resources?type=drive_file
GET /connections/google/resources?type=folder
```

Project setup and integration setup should call these APIs instead of source-specific auth commands.

CLI compatibility:

```text
bicameral connection auth github
bicameral connection auth google
bicameral connection list
bicameral connection disconnect google
```

Legacy aliases:

```text
bicameral-mcp source-auth google_drive
```

may remain as wrappers while migration completes.

## Security And Privacy

- Tokens never appear in config files, logs, audit fields, screenshots, or terminal success text.
- Secrets stay in `secrets_store`.
- Metadata may store account labels, provider IDs, scopes/capabilities, and timestamps.
- Keyring fallback must be visible to the operator because fallback may not persist across restart.
- Disconnect revokes local token storage and emits a revoke audit event.
- Provider-side revocation should be detected on refresh/use and shown as `Action needed`.

## UI Copy

Preferred labels:
- `Connections`
- `Connect GitHub`
- `Connect Google Drive`
- `Reconnect`
- `Disconnect`
- `Connected`
- `Action needed`
- `Not connected`

Avoid:
- `source-auth`
- `OAuth token`
- `refresh token`
- `scope` in primary UI
- source-specific implementation names like `google_drive`

## Migration Plan

1. Introduce connection registry metadata.
2. Add connection API over existing `secrets_store`.
3. Wrap current Google Drive OAuth as `connection.google`.
4. Add GitHub device-flow connection using the Electron demo as reference.
5. Update project setup to consume provider connections.
6. Update integration setup to consume provider connections before resource selection.
7. Keep `source-auth google_drive` as a compatibility alias.
8. Migrate existing source-specific secrets opportunistically on reconnect or refresh.

## Open Questions

- Do we want provider-level “minimum capability” bundles such as `event_store.github` and `integration.github_prs`?
- Should connection metadata live in `~/.bicameral/operator.yaml` or a separate `~/.bicameral/connections.yaml`?
- Should audit events remain `SOURCE_AUTH_*` or be generalized to `CONNECTION_AUTH_*`?
