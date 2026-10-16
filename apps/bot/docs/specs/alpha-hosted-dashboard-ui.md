# Alpha Hosted Dashboard UI Spec

Date: 2026-06-26

Status: draft

Level: L2 product UI contract

Owns: the alpha hosted dashboard screens that wrap existing `bicameral-bot`
dashboard surfaces after a hosted browser pairs with the local daemon.

Related context:

- `docs/specs/hosted-bridge-user-level-pairing.md`
- `docs/specs/explicit-project-registration-hosted-local-discovery.md`
- `docs/specs/project-setup-ui.md`
- `docs/domain/runtime-protocol.md`
- ADR-0002, ADR-0003, ADR-0007, ADR-0020, ADR-0023

Governance gate: `bic:spec-governance-gate` advisory/no-op; Qor integration
disabled pending factory policy revision.

## Runtime Invariant

The alpha hosted dashboard is a hosted shell over existing `bicameral-bot`
dashboard surfaces. It must not introduce hosted-only Decision, Activity,
Source, compliance, signoff, or project-setting shapes.

Hosted web may render, request, and relay. The local daemon validates,
authorizes, materializes, and projects Bicameral state.

## Alpha Cut Line

Alpha hosted dashboard is operator-home first:

```text
HOSTED WEB
  |
  | 1. Pair browser to local daemon
  v
LOCAL DAEMON CONFIRMATION
  |
  | 2. Paired session can list local projects/readiness
  v
OPERATOR HOME
  |
  | 3. Select ready project or repair access
  v
PROJECT DASHBOARD
  |
  | 4. Read-only Activity / Decisions / Channels inherited from bot
```

Governed mutations remain disabled in the hosted dashboard until the hosted
bridge command path has explicit local-policy UX, confirmation, audit receipts,
and tests proving parity with the local daemon's existing authority checks.

## Screen States

### Hosted Unpaired

The hosted page should keep the first connection screen short. Detailed
permission language belongs in the local daemon confirmation screen, where the
user grants access.

```text
+--------------------------------------------------------------+
| Bicameral                                      Account  Team |
+--------------------------------------------------------------+

  Connect this browser to your local Bicameral

  This hosted dashboard needs your local daemon to read project
  state. Canonical decisions, provider tokens, and local code stay local.

  Local daemon
  [ Not connected ]

  [ Connect local Bicameral ]      [ Use local dashboard instead ]

+--------------------------------------------------------------+
```

Required variants:

- pairing pending;
- daemon unavailable;
- wrong daemon;
- wrong cloud account;
- unsupported daemon version;
- pairing expired or revoked, with re-pair affordance.

### Local Confirmation

```text
+--------------------------------------------------------------+
| Local Bicameral                                              |
+--------------------------------------------------------------+

  app.bicameral-ai.com wants to connect to this local daemon

  Signed in locally as: <operator>
  Daemon: 127.0.0.1:37373
  Cloud account: <account>

  Allow hosted web to:
    [x] List local Bicameral projects
    [x] Show setup/readiness status
    [x] Read project projections after project selection
    [x] Request repair flows

  Not allowed:
    - approve decisions
    - write Decision Ledger events
    - bind workspace folders
    - read provider tokens or local paths

  Session expires: 30 minutes

  [ Allow ]                                            [ Deny ]

+--------------------------------------------------------------+
```

### Paired Operator Home

Operator home renders locally registered project readiness returned by the
daemon. It must not expose provider tokens, local paths, or event-store
contents.

```text
+--------------------------------------------------------------+
| Bicameral                         Paired: local daemon   ... |
+--------------------------------------------------------------+

  Local projects

  +----------------------+-------------------+-----------------+
  | Project              | Event store        | Readiness       |
  +----------------------+-------------------+-----------------+
  | Acme Checkout        | GitHub             | Ready           |
  | Bicameral Bot        | Google Drive       | Needs access    |
  | Factory Roadmap      | GitHub             | Offline         |
  +----------------------+-------------------+-----------------+

  [ Open selected ]   [ Create project ]   [ Join project ]

  Selected: Bicameral Bot

  Needs access
  Google Drive is not connected for this local operator.

  [ Connect Google Drive ]     [ Use another account ]

+--------------------------------------------------------------+
```

Required variants:

- no registered projects;
- all projects inaccessible;
- selected project offline;
- selected project needs resource selection;
- paired session expired while viewing operator home.

### Ready Project Dashboard

The ready project dashboard must inherit from existing bot surfaces. Hosted UI
adds only the pairing/session/project shell.

```text
+--------------------------------------------------------------+
| Bicameral  Project: Acme Checkout v   Paired hosted view ... |
+--------------------------------------------------------------+
| Activity | Decisions | Channels | Settings                   |
+----------+-----------+----------+----------------------------+

  Activity
  Existing bot IngestionGate surface
  Source: /api/v1/dashboard/ingestion-gate through hosted bridge projection

  +----------------------------------------------------------+
  | Source             Candidate / context        State      |
  +----------------------------------------------------------+
  | GitHub PR #611     Hosted bridge readiness    proposed   |
  | Linear issue       Setup repair follow-up     advisory   |
  +----------------------------------------------------------+

+--------------------------------------------------------------+
```

```text
+--------------------------------------------------------------+
| Decisions                                                    |
+--------------------------------------------------------------+

  Existing bot LedgerView surface
  Source: /api/v1/dashboard/ledger through hosted bridge projection

  +----------------------------------------------------------+
  | Decision                         Source       Code       |
  +----------------------------------------------------------+
  | Use GitHub as event store        fresh        verified   |
  | Hosted bridge is user-level      fresh        n/a        |
  +----------------------------------------------------------+

  Actions inherit bot enablement:
    - show existing allowed read actions
    - disable or hide governed mutations unless bridge command path
      supports the same local policy checks and receipts

+--------------------------------------------------------------+
```

Implementation inheritance rule:

- Reuse `dashboard/src/pages/IngestionGate.svelte`.
- Reuse `dashboard/src/pages/LedgerView.svelte`.
- Reuse `dashboard/src/pages/Channels.svelte`.
- Reuse `dashboard/src/pages/ProjectSettings.svelte` only for safe read-only
  or repair-scoped settings.
- Reuse `dashboard/src/lib/types.ts` dashboard API contracts.
- Do not invent hosted-only decision, activity, source, compliance, or signoff
  shapes.
- Hosted-specific UI is only the shell: pairing, project readiness, repair, and
  session controls.

### Project Repair

```text
+--------------------------------------------------------------+
| Bicameral  Project: Bicameral Bot        Paired hosted view  |
+--------------------------------------------------------------+

  Project needs repair

  Failing layer: Connection
  Provider: Google Drive
  Status: not connected

  This does not mean pairing failed.
  The browser is paired, but this local operator cannot access the
  project event store yet.

  [ Connect Google Drive ]   [ Use another account ]   [ Back ]

+--------------------------------------------------------------+
```

Repair copy must identify the failing layer:

- `GitHub not connected` means the user-level connection is missing.
- `Repository not selected` means project resource configuration is missing.
- `Connected account lacks repo access` means the access binding failed.
- `Token expired` means the user-level connection needs reconnect.
- `Repo moved or deleted` means the project resource needs repair.

### Workspace Binding Deferred

```text
+--------------------------------------------------------------+
| Code grounding                                               |
+--------------------------------------------------------------+

  Workspace binding is not available in this alpha.

  You can read project decisions and setup state through the hosted
  dashboard. Code-grounding features require a later local workspace
  binding flow.

  [ Open local dashboard ]                         [ Dismiss ]

+--------------------------------------------------------------+
```

## API Mapping

| UI area | Contract | Alpha behavior |
| --- | --- | --- |
| Pair request | `POST /api/v1/hosted-bridge/pair` | Starts or resumes pairing; handles typed errors |
| Pair status | `GET /api/v1/hosted-bridge/status` | Shows paired, expired, revoked, unsupported states |
| Operator home | `GET /api/v1/hosted-bridge/operator-home` | Lists local project readiness only |
| Project list | `GET /api/v1/hosted-bridge/projects` | Dedicated project readiness list |
| Activity | Existing dashboard ingestion-gate contract | Render through hosted shell without hosted-only shapes |
| Decisions | Existing dashboard ledger contract | Render through hosted shell without hosted-only shapes |
| Channels | Existing dashboard channels/settings contracts | Render only safe read/repair paths in alpha |
| Governed commands | `POST /api/v1/hosted-bridge/command` | Disabled in UI until local-policy confirmation and receipts exist |

## State Refinement Check

Release state: the hosted dashboard introduces no new canonical Decision Ledger
state. It defines UI states over existing hosted bridge pairing,
ProjectRegistration, project repair, and read-model projection contracts.

States refined:

- `hosted_bridge_unpaired`: render short hosted connection prompt.
- `hosted_bridge_paired`: render operator home and inherited bot project
  surfaces.
- `hosted_bridge_revoked`: render re-pair state.
- `registration_repair_required`: render project repair state by failing layer.
- `local_workspace_unbound`: render code-grounding deferred copy in alpha.

Transitions:

| Transition | Trigger | Authority | Persistence |
| --- | --- | --- | --- |
| Pair hosted browser | Browser displays daemon code; user pastes it locally, verifies the matched request, and explicitly approves | Local daemon | Pairing/session record |
| Select ready project | User opens a listed project | Local daemon validates ProjectRegistration | Session-local selected project plus daemon projection |
| Enter repair | Project readiness reports missing access/resource | Local daemon | Existing registration/connection state |
| Re-pair | Session expires or is revoked | Local daemon | New pairing/session record |

Invariants:

- Hosted shell must not create canonical Decision Ledger state.
- Pairing is not project membership, project selection, workspace binding,
  source authority, signoff, or event-store authority.
- Ready project dashboard must inherit from existing bot dashboard surfaces and
  dashboard API contracts.
- Provider tokens, local paths, SourceEvidence bodies, and event-store payloads
  must not appear in hosted operator-home readiness.
- Governed mutations remain disabled until local-policy confirmation and audit
  receipts are specified and implemented.

Minimization review: no new product lifecycle states are added. Visual states
are projections of existing pairing, registration, repair, and workspace
binding states. `workspace_binding_available` remains false in alpha rather
than creating a partial workspace binding flow.

Deferred vocabulary:

- Hosted governed review action confirmation.
- Hosted workspace binding flow.
- Hosted canonical project settings.
- Hosted-only activity or decision read models.
