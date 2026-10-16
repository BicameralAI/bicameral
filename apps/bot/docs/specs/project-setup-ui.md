# Bicameral Project Setup UI Spec

Principle: Setup must keep the meaning-bearing project choice explicit while hiding operator bootstrap mechanics unless the CLI needs to report progress or failure.

Governance gate: bic:spec-governance-gate advisory/no-op; Qor integration disabled pending factory policy revision.

## Scope

This spec defines the first dashboard UI a user sees after running `bicameral dashboard` when no project is selected.

Related spec:
- [OAuth And Integration Connections](oauth-and-integration-connections.md)
- [Rendezvous State Machine](rendezvous-state-machine.md)

It covers:
- user-level provider connection status
- Bicameral-wide provider settings
- project create flow
- project join flow
- transition into the ledger dashboard

It does not cover:
- marketing site copy
- installer UI
- daemon internals
- event-store adapter implementation

## CLI Before Dashboard

`bicameral dashboard` may print first-run progress in the terminal.

Expected CLI shape:

```text
$ bicameral dashboard
✓ Created ~/.bicameral/
✓ Saved operator settings
✓ Initialized projects
→ Open http://127.0.0.1:37373
```

This progress is not shown inside the dashboard UI.

If first-run setup fails, the CLI should print the concrete filesystem or port error and stop before opening the dashboard.

## Dashboard Entry

When no project is selected, the dashboard opens directly to project setup.

The dashboard page contains:
- top navigation with Bicameral identity
- `Settings` action
- primary action button
- main setup panel

The dashboard must not include:
- state inspection/debug panels
- internal state names such as `project_setup_mode`
- operator bootstrap forms
- terminal output blocks
- substrate/descriptor jargon

## Provider Settings

Provider authentication is Bicameral-wide and user-level.

Settings contains provider cards:
- GitHub
- Google Drive

Each provider card shows:
- provider name
- connection state: connected / not connected
- action: connect / reconnect

Provider authentication does not create, join, or select a project. It only grants the local operator access to providers that projects may use.

## Project Setup

Project setup has exactly two tabs:
- Create
- Join

### Create Tab

Fields:
- Project name
- Event store selection

Event store options:
- GitHub
- Google Drive

Each event store option shows:
- provider label
- one short line describing the provider:
  - GitHub: `Install the Bicameral GitHub App to use GitHub as the project event store.`
  - Google Drive: `Use a shared drive as the project event store.`
- connection status: connected / not connected / action needed
- provider resource readiness (visible after connection)

If the selected provider is not connected:
- primary action is `Install Bicameral GitHub App` (GitHub) or `Connect Google Drive` (Google Drive)
- action initiates provider connection inline on the setup screen
- the page preserves project name and selected event-store state across the connect round-trip
- the page shows one short inline notice: `Install the Bicameral GitHub App to continue.` or `Connect Google Drive to continue.`
- GitHub connection directs the user to install/connect the hosted Bicameral GitHub App (not PAT or imported-token)

If the selected provider is connected but the event-store resource is not ready:
- primary action (`Create project`) is disabled
- the page shows provider resource readiness status and any required action
- for Google Drive: shows shared-drive selection (auto-select when exactly one eligible drive; selector only when multiple)
- for Google Drive: shows a callout with the standard project path convention: `<shared drive root>/.bicameral/<project-name>/`
- for Google Drive: rejects consumer Gmail / non-allowed-domain accounts with an action-needed notice

If the selected provider is connected and event-store resource readiness is satisfied:
- primary action is `Create project`
- creating the project persists the project event-store choice and opens the project dashboard

### Join Tab

> **Reconciled to per-Product Join from Operator Home (issue #765).** The alpha
> default Join surface is a **per-Product** Join from Operator Home driven by
> `HostedProjectPreview` + same-domain auto-join, not a global free-form
> invite-code entry. Same-domain auto-join grants `Product Member` only, with no
> durable pending-approval state. The free-form `Invite code or link` entry below
> is **legacy/deferred** (event-store-location bootstrap only, see
> `rendezvous-state-machine.md`); it is not the alpha default. Authoritative
> contract: [`domain-product-onboarding-contract.md`](domain-product-onboarding-contract.md).

Alpha default (per-Product Join from Operator Home):
- Operator Home lists discoverable same-domain Products from `HostedProjectPreview`.
- Selecting a Product runs per-Product `Join`; Cloud exchanges its session for a
  narrow `ProductJoinTicket` and the daemon grants `Product Member` only.
- If the Product's provider is not connected, the primary action is
  `Install Bicameral GitHub App` or `Connect Google Drive` (same as Create tab).

Legacy/deferred fields (not the alpha default):
- Invite code or link

After an invite is resolved (legacy path), show a compact preview:
- Project
- Event store
- Provider

If the invite requires an unconnected provider:
- primary action is `Install Bicameral GitHub App` or `Connect Google Drive`
- action initiates provider connection inline (same as Create tab)

If the provider is connected:
- primary action is `Join project`
- joining opens the project dashboard

## Google Drive Wording

Google Drive is an event store choice, not a workspace folder.

The UI should say:
- `Google Drive`
- `Use Drive as the project event store.`
- `Connect Google Drive`

The UI should not say:
- `Google Drive folder`
- `local Drive folder`
- `workspace folder`
- `Drive mount path`

If an implementation later requires a local sync path for a filesystem-backed adapter, that is adapter configuration and should appear only in a repair/settings surface, not in the default create/join flow.

## Running Dashboard

After create or join succeeds, the user lands in the normal project dashboard.

The first visible project surface should be the ledger, including:
- project name
- source evidence
- code evidence
- event store label

The running dashboard should not expose setup state unless the selected project becomes invalid and needs repair.

## Global Project Selector

Once the local operator has more than one registered project, the running
dashboard should include a global project selector in the top navigation.

Expected shape:

```text
Bicameral   Project: Acme Checkout v        Settings
```

The selector lists registered projects and their readiness:

```text
Acme Checkout        GitHub event store       Ready
Bicameral Bot        Google Drive             Needs access
Factory Roadmap      GitHub event store       Offline

Create project
Join project
Manage projects
```

The selector should not expose OAuth details directly. If selecting a project
requires local access repair, route to a project repair screen:

```text
Acme Checkout needs GitHub access
Connected account: none

[Connect GitHub]
[Use another account]
```

Use these nouns consistently:

- **Connection**: user-level provider auth grant, shown in Settings.
- **Project resource**: event store, source, repo, folder, channel, or team
  selected by project settings.
- **Access binding**: this local operator's selected connection for a project
  resource.

Failure copy should identify the failing layer:

- `GitHub not connected` means the user-level connection is missing.
- `Repository not selected` means project resource configuration is missing.
- `Connected account lacks repo access` means the access binding failed.
- `Token expired` means the user-level connection needs reconnect.
- `Repo moved or deleted` means the project resource needs repair.

## Project Sources And Tool Ingestion

Project setup and settings should distinguish managed source ingestion from
member/tool ingestion.

Managed source ingestion is configured at the project level by the project owner
or EM. It chooses the external resource and the ingestion owner's connection.
Example:

```text
GitHub repo: acme/checkout
Ingested via: Priya's GitHub connection
Approved by: Priya
Status: Healthy
```

This avoids redundant ingestion from every team member's OAuth connection and
lets members join/read the project without connecting every source provider.

Tool ingestion is different. Multiple workspace members and tools may submit
observations through MCP, CLI, or local agent surfaces. Those submissions should
show operator/tool provenance and route through review policy, but they should
not create duplicate managed source bindings.

Suggested settings structure:

```text
Project Settings
  Project
  Sources
  Channels

User Settings
  Connections
```

Project Settings should stay this small by default:

- **Project**: project identity, event store, and local access repair.
- **Sources**: owner-approved recurring evidence intake.
- **Channels**: communication, review, and notification routes.

Avoid adding separate first-class pages for `Tool Ingestion`, `Access`, or
`Proposal` until the product needs them. Tool submissions can appear in
Activity and Decision evidence. Local access repair belongs under Project when
the selected project cannot be opened. Proposals should appear where the work
is being reviewed: usually Activity for intake/candidate review or Decisions
when promoting evidence into the ledger.

## Sources, Channels, And Connections

Use these nouns consistently in the UI:

- **Connections** are user-level OAuth or provider grants. They live in User
  Settings.
- **Sources** are project-level approved evidence inputs, such as a GitHub repo,
  Linear team, Slack channel history, or Drive folder/document set. A Source
  records which operator connection is used for recurring ingestion.
- **Channels** are project-level communication routes, such as where review
  requests, notifications, or collaboration updates are sent or read.

Do not introduce `ChannelRoute` as a first-stage UI or config object. The
initial release can show Channels and can accept advisory routing hints from
connectors, mods, or tool submissions, but it should not persist those hints as
project route configuration. A future ChannelRoute refinement must define the
product-owner action, saved settings shape, authority boundary, edit lifecycle,
and recovery behavior before it appears in UI copy or data plumbing.

The same external provider can appear in more than one layer. For example,
GitHub may be:

- a Connection: Priya connected her GitHub account;
- a Source: the project ingests `acme/checkout` pull requests via Priya's
  connection;
- a Channel: the project posts review updates to GitHub issue comments.

Do not collapse these layers in the UI. The user-level grant answers who can
authenticate; the project Source answers what is ingested; the project Channel
answers where communication happens.

## Copy Rules

Keep setup copy short and action-oriented.

Avoid:
- explaining internal architecture
- repeated notes about user-level auth
- state-machine labels
- debug/status tables
- long paragraphs

Preferred button labels:
- `Project Settings`
- `Connections`
- `Connect GitHub`
- `Connect Google Drive`
- `Create project`
- `Join project`
- `Done`

## Mockup Status

Current local mockup route:

```text
site/src/routes/setup-mockup/+page.svelte
```

Known discrepancy: the current mockup may still include a terminal panel inside the page. The intended UI in this spec removes that panel from the dashboard and keeps CLI progress in the terminal only.
