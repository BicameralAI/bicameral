# Rendezvous State Machine Spec

> **Legacy / not the alpha onboarding default (issue #765).** The
> invite/rendezvous exchange described here is **not** the alpha default for
> discovering and joining a domain-discovered Product. The authoritative alpha
> onboarding flow is per-Product Join from Operator Home via
> `HostedProjectPreview` + `ProductJoinTicket` + same-domain auto-join, codified
> in [`domain-product-onboarding-contract.md`](domain-product-onboarding-contract.md).
> This invite/rendezvous flow is retained only as a deferred/legacy
> event-store-location bootstrap; it must not reintroduce invite queues,
> owner-approval, or global free-form join as the alpha default. Where this spec
> conflicts with the onboarding contract, the onboarding contract wins.

Principle: Rendezvous helps two operators find the same event-store location; it must never become the event-store authority or carry project settings.

Governance gate: bic:spec-governance-gate advisory/no-op; Qor integration disabled pending factory policy revision.

## Scope

This spec defines the invite/rendezvous flow used by project setup.

It covers:
- creating a join token
- resolving an invite
- pairing the local operator with the provider connection needed to fetch the event-store descriptor
- resuming create/join after OAuth
- expiration, revocation, replay, and provider mismatch edge cases

It does not cover:
- hosted project storage
- event-store adapter internals
- provider OAuth implementation
- billing/team administration

Related specs:
- [Project Setup UI](project-setup-ui.md)
- [OAuth And Integration Connections](oauth-and-integration-connections.md)

## Decision

Rendezvous is optional cloud amenity for invite exchange and project discovery.

Rendezvous is a bootstrap pointer, not a project settings channel. The only
project-specific fact it should share is the event-store location needed to
fetch the live project descriptor. After provider access is validated, the
joiner reads current project settings, required integrations, and connection
requirements from the event store itself.

Rendezvous may hold:
- invite ID
- project display name for preview only
- event-store type: GitHub or Google Drive
- bootstrap provider capability needed to read the event store
- opaque event-store location pointer
- inviter identity label
- expiration timestamp
- one-time-use or multi-use policy

Rendezvous must not hold:
- OAuth access tokens
- refresh tokens
- full local operator config
- local filesystem paths
- integration settings
- required integration providers beyond the bootstrap event-store provider
- accepted ledger events as authority
- project state that supersedes the event store

The local daemon only enters normal project mode after:
- invite is resolved
- required provider connection exists locally
- event-store descriptor is validated against provider access
- latest project settings are read from the event store
- local project registry is updated
- any pending join checkpoint is marked complete

## Actors

- **Inviter**: operator who creates a share/join invitation.
- **Joiner**: operator who enters invite code/link.
- **Rendezvous service**: cloud amenity that stores short-lived invite metadata.
- **Provider**: GitHub or Google Drive.
- **Local daemon**: authority for local operator state and project selection.

## Core State Machine

```text
no_project_selected
  -> join_tab_open
  -> invite_input_pending
  -> resolving_invite
  -> invite_resolved
  -> provider_connection_check
  -> auth_handoff_required
  -> provider_auth_in_progress
  -> provider_connected
  -> event_store_access_check
  -> project_joined
  -> normal_dashboard
```

Alternate paths:

```text
resolving_invite -> invite_invalid
resolving_invite -> invite_expired
resolving_invite -> rendezvous_unreachable
provider_connection_check -> auth_handoff_required
auth_handoff_required -> provider_auth_in_progress
provider_auth_in_progress -> provider_auth_failed
event_store_access_check -> access_denied
event_store_access_check -> descriptor_stale
project_joined -> replay_failed_repairable
```

## Join Flow

### 1. `join_tab_open`

User sees:
- invite code/link input
- `Join project` primary action

No provider auth is requested yet.

### 2. `resolving_invite`

Dashboard sends invite code/link to rendezvous resolver.

Expected response:

```yaml
invite_id: inv_123
project_name: Acme checkout
event_store:
  type: github
  location_ref: opaque-provider-specific-pointer
bootstrap_connection:
  provider: github
  capabilities:
    - event_store.github
expires_at: 2026-06-14T00:00:00Z
inviter_label: alice@example.com
```

The UI shows a compact preview:
- project name
- event store
- inviter label, if available

The resolver response must not include project integration settings. For
example, if an engineering manager connects Linear after earlier users have
joined, new and existing users learn that through the event-store project
settings, not through rendezvous invite metadata.

### 3. `provider_connection_check`

Local daemon checks Bicameral-wide provider connections.

If connected with sufficient capability:
- continue to `event_store_access_check`

If missing or insufficient:
- transition to `auth_handoff_required`
- preserve pending invite state locally

### 4. `auth_handoff_required`

The dashboard shows a contextual holdover screen, not a generic Settings redirect.

Example for GitHub:

```text
Join project
Acme checkout uses GitHub.

Connect GitHub to continue.
[Connect GitHub]
```

Example for Google Drive:

```text
Join project
Acme checkout uses Google Drive.

Connect Google Drive to continue.
[Connect Google Drive]
```

Rules:
- The user should not re-enter the invite after OAuth.
- The pending join remains visible.
- OAuth remains user-level, not project-specific.
- The primary action starts the unified connection flow.
- Settings can still expose all connections, but the main flow should return to the pending join automatically.

Persist a pending join checkpoint before starting OAuth:

```yaml
pending_join:
  state: auth_handoff_required
  invite_id: inv_123
  invite_secret_ref: local-secret-ref
  project_name: Acme checkout
  event_store_type: github
  bootstrap_provider: github
  bootstrap_capabilities:
    - event_store.github
  resolved_at: 2026-06-13T00:00:00Z
  expires_at: 2026-06-14T00:00:00Z
```

The invite secret itself should be stored as a local secret or otherwise kept out of logs. The checkpoint may store a redacted reference.

### 5. `provider_auth_in_progress`

OAuth follows the unified connection state machine.

On success:
- return to join flow
- re-resolve or revalidate invite before use

On failure:
- show provider-specific retry action
- preserve invite input

### 6. `event_store_access_check`

Local daemon validates the resolved descriptor using the local provider connection.

GitHub examples:
- repo exists
- token can read required repo
- token can write if event-store append requires write

Google Drive examples:
- event-store files/folder are visible through allowed scope
- token can read/write Bicameral-created event files

If validation succeeds:
- fetch latest project settings from the event store
- persist local project registry entry
- mark pending join complete
- transition to `project_joined`

If validation fails:
- show `Request access` / `Choose another invite` / `Reconnect provider`
- preserve pending join as repairable or blocked
- do not create a runnable project entry

### 7. `project_joined`

Local project registry stores:
- project ID
- display name
- event-store type
- provider connection ID
- event-store descriptor
- last observed project settings version
- joined timestamp

Local project registry must not store provider tokens.

Project settings remain event-store-authoritative. The local registry can cache
the last observed settings version for startup and repair decisions, but it must
refresh from the event store before deciding which integrations are required.

After the registry entry is durably written, remove or mark complete the pending join checkpoint. If replay then fails, reopen the project in repair mode rather than losing the joined project.

## Restart Recovery

The join flow must be fault tolerant across daemon/dashboard restarts.

Persist pending join checkpoints at every external boundary:
- after invite resolution
- before OAuth handoff
- after OAuth success before event-store validation
- after event-store validation before registry write

On dashboard startup:

```text
operator_config_ready
  -> load_pending_join
  -> no_pending_join -> project_setup
  -> pending_join_found -> resume_pending_join
```

Resume behavior:

| Stored state | Startup behavior |
|---|---|
| `invite_resolved` | Revalidate invite; then check provider connection. |
| `auth_handoff_required` | Show contextual connect screen again. |
| `provider_auth_in_progress` | Treat as unknown; check provider connection, then resume or show connect. |
| `provider_connected` | Revalidate invite and event-store access. |
| `event_store_access_check` | Re-run access check idempotently. |
| `project_joined` but replay not complete | Open repair/replay screen for that project. |

Restart rules:
- Never assume OAuth completed because a browser was opened.
- Never consume a single-use invite until provider validation succeeds.
- Re-resolve or revalidate invite after restart because it may have expired.
- Event-store validation must be idempotent.
- Registry writes must be idempotent by project ID.
- A pending join whose invite expired should become an expired-invite screen, not disappear.
- A pending join with newly connected provider should continue automatically after a short validation step.

The visible recovery screen should be user-facing:

```text
Finish joining Acme checkout
Connect GitHub to continue.
[Connect GitHub]
```

or:

```text
Finish joining Acme checkout
Checking project access...
```

It should not show checkpoint names or internal state-machine labels.

## Create Invite Flow

Inviter can create an invite only from a project whose event-store descriptor is valid.

```text
project_running
  -> create_invite_requested
  -> provider_access_revalidated
  -> rendezvous_record_created
  -> invite_ready
```

Invite creation payload:

```yaml
project_name: Acme checkout
event_store_type: github
bootstrap_capabilities:
  - event_store.github
event_store:
  location_ref: opaque-provider-specific-pointer
expires_in: 24h
use_policy: single_use | reusable
```

`bootstrap_capabilities` only describes what the joiner needs to fetch the event
store. It must not list project integrations such as Linear, Slack, or GitHub
issues. Those requirements are project settings and must be read from the event
store after join.

Invite UI shows:
- copyable link/code
- expiration
- event store label

Invite UI must not show raw provider tokens or local paths.

## Edge-Case Matrix

| Case | State | Behavior |
|---|---|---|
| Bad invite code | `invite_invalid` | Show “Invite not found.” Keep input editable. |
| Expired invite | `invite_expired` | Show “Invite expired.” Offer paste another invite. |
| Invite already used | `invite_invalid` or `invite_used` | Show “Invite already used.” |
| Rendezvous offline | `rendezvous_unreachable` | Show retry; do not ask for provider auth yet. |
| User starts OAuth then cancels | `provider_auth_failed` | Return to join preview if invite still valid. |
| OAuth succeeds but invite expires | `invite_expired` | Ask for a new invite; keep provider connected. |
| OAuth account lacks access | `access_denied` | Show account label and reconnect/request access action. |
| Provider token revoked | `provider_auth_required` | Mark connection action needed; preserve invite. |
| Inviter revoked provider access | `access_denied` | Do not join; ask inviter to issue/fix invite. |
| Event-store descriptor changed | `descriptor_stale` | Re-resolve invite once, then show stale invite. |
| Existing local project with same ID | `project_joined` | Reuse/update registry entry idempotently. |
| Existing local project with same name but different ID | `project_name_collision` | Ask user to rename local label. |
| User opens invite on second machine | `provider_connection_check` | Require local provider connection on that machine. |
| User is offline after resolving invite | `event_store_access_check` blocked | Cannot join until provider access can be validated. |
| Daemon restarts before OAuth | `auth_handoff_required` | Reload pending join and show connect screen again. |
| Daemon restarts during OAuth | `provider_auth_in_progress` | Check provider connection; resume or show connect. |
| Daemon restarts after OAuth before validation | `provider_connected` | Revalidate invite, then check event-store access. |
| Daemon restarts after validation before registry write | `event_store_access_check` | Re-run idempotent access check and write registry. |
| Replay fails after join | `replay_failed_repairable` | Keep project entry but open repair mode, not normal dashboard. |

## Idempotency

Joining the same project twice should be safe.

Rules:
- same project ID + same descriptor: update `last_seen_at`, no duplicate
- same project ID + changed descriptor: require validation before update
- same display name + different project ID: ask for local label disambiguation
- repeated invite resolution must not create project state until provider access validates
- repeated resume after restart must not consume another invite use unless the previous attempt reached provider validation success

## Security Rules

- Invite codes are bearer capabilities and must expire.
- Prefer short TTLs by default, such as 24 hours.
- Single-use invites should be consumed only after successful provider validation.
- Failed OAuth must not consume the invite.
- Invite resolution should rate-limit attempts.
- Rendezvous records should reveal the minimum useful preview before provider auth.
- Sensitive descriptor details should be opaque until provider validation when possible.
- Audit locally when a project is joined, but do not log invite secret values.
- Pending join checkpoints must not contain provider tokens.
- Pending join checkpoints must not be treated as joined projects.

## UI Copy

Preferred labels:
- `Join project`
- `Invite code or link`
- `Connect GitHub`
- `Connect Google Drive`
- `Invite expired`
- `Invite not found`
- `Request access`
- `Reconnect`

Avoid:
- `rendezvous`
- `descriptor`
- `capability`
- `substrate`
- raw provider IDs in primary UI

## Validation Hooks

Implementation should include tests for:
- bad invite
- expired invite
- OAuth cancellation and resume
- daemon restart before OAuth
- daemon restart during OAuth
- daemon restart after OAuth before validation
- daemon restart after validation before registry write
- OAuth success followed by expired invite
- connected provider with insufficient repo/Drive access
- duplicate join idempotency
- same display name/different project collision
- replay failure routes to repair mode
- invite secret never logged

## Open Questions

- Should default invites be single-use or reusable?
- Should invite preview show inviter identity before provider auth?
- Should rendezvous support local-only invite bundles for air-gapped users?
- Where should pending invite state live while OAuth is in progress?
