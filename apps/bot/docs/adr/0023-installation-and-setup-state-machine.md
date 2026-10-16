# ADR-0023: Installation and Setup State Machine

**Date:** 2026-06-13
**Status:** proposed
**Level:** L1
**Related:** ADR-0001, ADR-0002, ADR-0013, ADR-0020, `docs/specs/shared-observation-to-candidate-runtime-architecture.md`, `docs/specs/explicit-project-registration-hosted-local-discovery.md`, bicameral-bot#908

## Decision

### Alpha amendment: operator-scoped multi-Product daemon (2026-07-28)

This amendment supersedes the transitional cwd/nearest-`.bicameral/` discovery,
single-selected-workspace daemon startup, and process-global adapter language
that remains later in this ADR.

The alpha daemon is one user/operator-scoped process. Its identity is the
operator home, not a repo or workspace. It may start healthy with zero, one, or
many registered Products and hydrates:

- explicit `ProjectRegistration` records under the operator home;
- per-Product event-store routes;
- per-Product operator-local `LocalWorkspaceBinding` records.

Product-scoped `ToolRequest` operations carry an explicit canonical
`product_id`. The daemon resolves that Product's event-store adapter and
optional workspace binding per request/session. A workspace binding provides
local code-grounding context only; it is not Product identity, event-store
authority, source authority, or daemon identity.

The startup ordering invariant is:

```text
ensure operator home
-> hydrate Product registry and LocalWorkspaceBinding records
-> start the operator daemon with zero or many registered Products
-> receive explicit Product id on a surface session or ToolRequest
-> resolve that Product's event-store adapter and optional workspace binding
-> replay and execute only within that Product route
```

The installed launchd/systemd service command is `bicameral gateway start`.
It must not contain `--workspace`, a workspace-derived `WorkingDirectory`, or
any other process-cwd identity. `onboard --install-daemon --workspace <path>`
may use the path to propose an initial registration/binding, but installation
must not bake it into the service.

`--workspace` remains an explicit foreground/development compatibility
override. Ambient cwd discovery is not an alpha Product-selection path.

Lifecycle is split deliberately:

- operator daemon: `operator_bootstrap_mode` -> `operator_daemon_running`;
- per Product: `unregistered` -> `pending_registration` ->
  `registered_product` / `registration_repair_required`;
- per Product workspace: `local_workspace_unbound` ->
  `local_workspace_bound` / `local_workspace_repair_required`.

A Product route or binding failure must not degrade the daemon or another
Product. Operator-mode requests without explicit Product context fail closed
with typed `product_context_required` readiness.

#### Journey boundary review

This amendment advances `developer_host_mcp_v1` across
`developer-host-environment-v1` and `local-product-environment-v1`. The journey
remains `partial`: a healthy installed daemon and registry-backed routing do not
prove that a released developer host fired the normally installed managed hook.

```text
operator -> installer/service -> operator daemon
                                  | ProjectRegistration + product_id
                                  v
                         per-Product event store
                                  | LocalWorkspaceBinding
                                  v
                           bound local workspace
```

The operator is the responsible human. Registration records and workspace
bindings are written under the operator home; canonical Product events are
written only by the resolved Product event-store adapter. A binding, local
path, MCP audit context, or passing validation receipt must never cross the
boundary as Product identity or canonical Product authority.

The independently observable facts are service health, Product registration,
workspace binding, canonical Product history, and host-hook activation. They
may temporarily disagree. A failed Product route is retried by repairing that
Product's registration or binding and resending the same Product-scoped
request; restarting a daemon with a different cwd or registering a second
Product to replace the failed route is forbidden recovery. Journey success
remains a host-fired first Product recall. Component tests, service health, and
the v0.1.14 Release Unit topology are validation evidence, not Product
lifecycle states or release approval.

Journey Boundary Review: passed for the architecture and failure model above;
terminal real-process evidence remains required by the v0.1.14 Release Unit
before merge eligibility.

Bicameral's public install flow must be modeled as a state machine that
separates installer delivery, operator bootstrap, project/workspace startup,
daemon startup, and dashboard configuration. The shell installer's only
authority is to install a versioned release artifact and dashboard assets.
After that, `bicameral dashboard` is the canonical first-run command.

> Superseded in part by ADR-0027: this command is now an API-only compatibility
> alias, while Cloud serves the SPA. Setup state and service lifecycle remain
> daemon-owned.

The dashboard command must first ensure the user-level operator home exists.
Only after operator bootstrap should the dashboard let the operator create,
join, select, or repair a project/workspace and its event store. Missing or
incomplete project/workspace configuration must route to project setup inside
the dashboard before constructing substrate-specific runtime adapters. It must
not panic before the setup UI and setup endpoints are available.

The install state machine is part of the product architecture because it gates
whether a new user can reach the daemon-owned runtime pipeline described in the
shared observation-to-candidate spec. If local setup is implicit or spread
across shell copy, CLI onboarding, service installation, and dashboard startup,
agents and humans will keep patching one surface while another surface remains
stale.

## Terminology

**Operator home** means the user-level Bicameral control plane rooted under
`~/.bicameral/`. It belongs to the local human/operator account on this
machine. It can manage multiple Bicameral projects/workspaces and their local
operator bindings.

**Project/workspace** means one governed Bicameral project: a repo/folder plus
its selected event-store substrate and local operator binding. The current bot
code discovers it by walking upward from the current start path until it finds a
`.bicameral/` directory. That is a Git-substrate discovery mechanism and
transitional default, not the general project registration model for non-Git
substrates.

A project/workspace is not:

- the installed CLI binary;
- the hosted installer;
- the dashboard asset directory;
- the user-level home config/cache directory;
- a daemon/service registration;
- a hosted Bicameral account.

The long-term local-daemon layout follows the earlier `bicameral-mcp` locator
plan: repo-local `.bicameral/` belongs to the Git event-store flow, while the
operator home stores project registrations and operator-private runtime state.

Operator-home state includes:

- `~/.bicameral/operator.yaml` or equivalent local operator profile;
- `~/.bicameral/projects/` project registry;
- `~/.bicameral/archive/` migration backups;
- local logs, diagnostics, and non-canonical caches where applicable.

Git-substrate repo-local state includes:

- `.bicameral/config.yaml`;
- `.bicameral/events/`;
- Git-reviewable event-store and policy descriptors.

Per-project operator state defaults under:

- `~/.bicameral/projects/<project-id>/operator.yaml`;
- `~/.bicameral/projects/<project-id>/ledger.db` or event-store state;
- `~/.bicameral/projects/<project-id>/code-graph.db`;
- `~/.bicameral/projects/<project-id>/bm25_index.pkl`;
- `~/.bicameral/projects/<project-id>/watermark`;
- `~/.bicameral/projects/<project-id>/pending-transcripts/`;
- `~/.bicameral/projects/<project-id>/processed-transcripts/`;
- `~/.bicameral/archive/<project-id>/` for migration backups.

The project id must be stable for the same logical project across linked
worktrees. The user-level project directory stores private runtime state and
per-operator settings.

The current code discovers a workspace by the presence of `.bicameral/`, then
loads `.bicameral/config.yaml` when present or defaults missing config. That
implementation is a transitional layout. The setup state machine should support
registered user-level projects where `~/.bicameral/projects/<project-id>/origin.txt`
points at the repo root and no repo-local `.bicameral/` directory is present.
A marker-only `.bicameral/` directory must be treated as setup-needed rather
than proof that normal daemon startup is safe.

Substrate-specific implication:

- `git` substrate may use repo-local `.bicameral/events/` and a committed
  `.bicameral/config.yaml` because the repository is the review and sync
  mechanism.
- `drive_folder` substrate must not use a repo-local `.bicameral/` fallback.
  Its event store lives in the configured Drive/sync folder. Project
  registration comes from bicameral-cloud amenity/invite metadata or the
  user-level project registry. The local mount path is per-operator state under
  `~/.bicameral/projects/<project-id>/operator.yaml`.
- `memory` substrate must not create repo-local state.

## State Model

### Release Delivery State

| State | Meaning | Owner | Allowed next transition |
| --- | --- | --- | --- |
| `not_installed` | No supported `bicameral` binary is on PATH. | User environment. | Run hosted install script or developer build. |
| `release_resolved` | Installer has selected a target archive from the public release manifest. | Hosted install script. | Download archive and `SHA256SUMS`. |
| `release_verified` | Downloaded archive checksum matches release checksum. | Hosted install script. | Extract release archive. |
| `binary_installed` | `bicameral` binary exists in install dir. | Hosted install script. | Install dashboard assets. |
| `dashboard_assets_installed` | Compiled dashboard assets exist next to the binary. | Hosted install script. | Print next step. |
| `path_ready` | Current or next shell can resolve `bicameral`. | Shell/user environment. | Run `bicameral dashboard`. |

Release delivery must not create workspace config, event-store state, provider
secrets, daemon launch agents, review decisions, or canonical events. It may
only install release artifacts and tell the user how to continue.

### Workspace Setup State

| State | Detection | Dashboard command behavior |
| --- | --- | --- |
| `operator_home_missing` | `~/.bicameral/` operator profile/registry has not been initialized. | Start operator bootstrap; do not ask the user to create a project first. |
| `operator_home_ready` | User-level operator profile/registry exists and is readable. | Show project/workspace picker or continue with selected project. |
| `no_workspace_selected` | Operator home exists but no project/workspace is selected for this dashboard session. | Show create/join/select project flow inside dashboard. |
| `registered_user_project` | User-level project registry maps the selected root to `~/.bicameral/projects/<project-id>/`. | Use that project config/state; non-Git substrates must not require repo-local `.bicameral/`. |
| `workspace_marker_only` | A `.bicameral/` marker exists but lacks usable config. | Start setup mode or return typed setup-needed diagnostics. |
| `workspace_incomplete` | Config exists but required fields for selected substrate or provider are missing. | Start setup mode when the dashboard can collect/repair them. |
| `operator_state_missing` | Repo-local workspace identity exists but `~/.bicameral/projects/<project-id>/` has not been created. | Start setup mode or create operator state through explicit local setup action. |
| `workspace_configured` | Config validates for selected substrate and gateway host/port. | Start normal daemon/dashboard mode. |
| `workspace_unrecoverable` | Config is malformed, unsafe, or points at inaccessible state that setup cannot repair safely. | Return a typed diagnostic with repair instructions; do not panic. |

`workspace_incomplete` includes the concrete failure observed during the
dashboard-first install trial: `event_store.substrate = drive_folder` without
`event_store.drive_folder_path`. That state must route to setup mode before
constructing a `DriveFolderAdapter`.

### Daemon Startup State

| State | Entry condition | Runtime action |
| --- | --- | --- |
| `operator_bootstrap_mode` | Operator home is missing or unreadable. | Serve dashboard assets plus operator bootstrap endpoints; do not select or create an event store. |
| `project_setup_mode` | Operator home exists, but no project is selected or the selected project is repairably incomplete. | Serve dashboard assets plus project create/join/select/config endpoints; do not construct canonical event-store adapter for an incomplete project. |
| `normal_mode_preflight` | Selected project appears configured. | Validate project config, substrate capability, gateway safety, and dashboard assets. |
| `normal_mode_running` | Runtime adapters and audit store opened successfully. | Serve gateway/dashboard and replay event store. |
| `normal_mode_diagnostic_error` | Config or adapter cannot start safely. | Return typed diagnostics; never convert setup failure into canonical state. |

The key ordering invariant is:

```text
discover workspace/config
  -> ensure operator home
  -> if operator missing, serve operator bootstrap mode
  -> select/create/join project
  -> classify project setup state
  -> if project setup-needed, serve project setup mode
  -> only if selected project is configured, construct substrate adapter
  -> replay event store
  -> serve normal dashboard
```

## Canonical First-Run Flow

The public product flow is:

```bash
curl -fsSL https://install.bicameral-ai.com/install.sh | sh
bicameral dashboard
```

From there the local dashboard owns setup continuation. It may ask the user to
initialize the local operator home, then create/join/select a project/workspace
and repair its event-store substrate, Drive folder path, provider readiness,
source connectors, and daemon/service preference.
When setup writes configuration, it must preserve the split between repo/team
identity and user-level operator state. It must not write private provider
secrets, telemetry choices, local indexes, ledgers, watermarks, or pending
transcript queues into repo-local `.bicameral/`.
For Drive-folder setup, the default should be entirely user-level after
operator bootstrap: register the project root under
`~/.bicameral/projects/<project-id>/origin.txt` and write the local Drive mount
path to the project `operator.yaml`. A repo-local `.bicameral/` directory is
only used when the user/team chooses the Git substrate.

`bicameral onboard --install-daemon`, `bicameral service start`, `bicameral
doctor`, and `bicameral init` remain useful explicit commands, but they must be
optional advanced or diagnostic paths. They are not the canonical first-run
sequence for the public installer.

## Dashboard UI Flow

### Operator Bootstrap Mode

Purpose: establish the user-level Bicameral operator home for this local
machine. This mode does not create, join, select, or open a project event store.

Entry condition:

- `~/.bicameral/` operator profile/registry is missing, unreadable, or has an
  unsupported schema version.

Required UI steps:

1. **Welcome / local scope**
   - State that Bicameral runs locally and stores operator settings under
     `~/.bicameral/`.
   - Show the exact operator-home path that will be used.
   - Do not mention `.bicameral/config.yaml` as the default path.

2. **Create operator profile**
   - Ask for optional local display name or actor label.
   - Pick a local data root, defaulting to `~/.bicameral/`.
   - Generate or load a local operator/device id.
   - Create `~/.bicameral/operator.yaml` or equivalent profile.
   - Create `~/.bicameral/projects/`.

3. **Post-bootstrap transition**
   - Move to project/workspace setup.
   - If projects already exist in the registry after repair, show the project
     picker instead of forcing project creation.

Allowed writes:

- `~/.bicameral/operator.yaml` or equivalent operator profile.
- `~/.bicameral/projects/` directory.
- local logs/diagnostics needed to troubleshoot bootstrap.

Forbidden writes:

- repo-local `.bicameral/`;
- event-store files;
- Decisions, candidates, signoff, compliance, binding, source evidence, or
  canonical events.

### Project Setup Mode

Purpose: create, join, select, or repair one Bicameral project/workspace after
the operator home exists. This mode may register project metadata and operator
bindings, but it must not open a canonical event-store adapter until the
selected project is configured.

Entry conditions:

- operator home exists but no project is selected;
- selected project registry entry is missing or stale;
- selected project lacks required operator binding for its substrate;
- selected project has repairable substrate/provider configuration.

Required UI structure:

1. **Project picker**
   - List registered projects from `~/.bicameral/projects/`.
   - Show project name/id, origin path, substrate, last opened, and readiness
     state.
   - Actions: Open, Repair, Remove local registration, Relink moved project.

2. **Create project**
   - Ask for project name.
   - Ask for local repo/folder path or use current directory when explicit.
   - Ask for substrate:
     - Git event store;
     - Drive/basic-store;
     - memory/dev-only when enabled.
   - For Git: create repo-local `.bicameral/config.yaml` and event-store path
     only after confirmation.
   - For Drive: do not create repo-local `.bicameral/`; write project
     registration and operator binding user-level.

3. **Join project**
   - Accept bicameral-cloud amenity pairing, invite bundle, or pasteable
     project descriptor.
   - Preview project id, project name, substrate, and required local bindings.
   - Offer cloud sign-in/rendezvous here as a project discovery or pairing
     path, not as a prerequisite for operator bootstrap.
   - Allow skip/local-only.
   - Make clear the cloud amenity is not the local event-store authority.
   - For Drive: ask for local synced folder path on this machine.
   - For Git: ask for local repo path and verify repo-local `.bicameral/`
     state or initialize it only when the Git substrate is selected.

4. **Repair project**
   - Missing Drive mount path: prompt for local folder and validate existence or
     mark offline.
   - Missing Git repo-local config: offer initialize/migrate only for Git
     projects.
   - Moved repo: update `origin.txt` or equivalent registry metadata after user
     confirmation.
   - Legacy repo-local private state: offer migration into
     `~/.bicameral/projects/<project-id>/`.

5. **Readiness summary**
   - Show selected substrate and capability boundary.
   - Show whether the event store is reachable.
   - Show local-only warnings: Drive stale/offline, provider not configured,
     dashboard API-only, old daemon version.
   - Only enable "Open project" when adapter construction can proceed or when
     the selected degraded state is explicitly allowed.

Allowed writes:

- `~/.bicameral/projects/<project-id>/origin.txt` or equivalent registration.
- `~/.bicameral/projects/<project-id>/operator.yaml`.
- repo-local `.bicameral/config.yaml` and `.bicameral/events/` only for Git
  substrate projects.
- migration archives under `~/.bicameral/archive/<project-id>/`.

Forbidden writes:

- repo-local `.bicameral/` for Drive or memory projects;
- private Drive mount paths into repo-local config;
- canonical event-store events before the project is opened in normal mode;
- review/signoff/compliance mutations.

### Normal Mode Transition

The dashboard may leave project setup and enter normal mode only after:

- operator home is readable;
- exactly one project is selected for the session;
- project id resolves to a project registration;
- substrate is known;
- required local operator binding exists for that substrate;
- gateway host/port pass safety validation;
- adapter construction can return typed readiness or a usable adapter.

Normal mode then:

```text
selected project
  -> construct selected EventStoreAdapter
  -> replay event store
  -> hydrate read/projection state
  -> serve Activity, Decisions, Channels, Settings
```

## Rationale

The shared observation-to-candidate runtime spec says local surfaces must be
able to start and report capability/readiness honestly. That requirement also
applies before the observation pipeline exists. A first-run user cannot reach
projection readiness, source capture, candidate convergence, or review routing
if the dashboard process panics while constructing an adapter for incomplete
setup state.

The install flow also crosses repositories:

- `bicameral-install` hosts the shell script and public install page.
- `bicameral-bot` publishes release assets and owns CLI/dashboard behavior.
- `bicameral` website copy can point users at the installer but does not change
  the release artifact they receive.

Without an explicit state machine, it is easy to update website copy while the
hosted installer still prints an old command, or to publish a release while the
first-run dashboard still assumes preconfigured workspace state.

## Consequences

- The hosted installer must keep its "Next" instruction aligned with this ADR:
  `bicameral dashboard`.
- Release validation must check both archive contents and first-run behavior:
  the binary and `dashboard/` assets are installed together, and `bicameral
  dashboard` can enter setup mode from a clean environment.
- Dashboard startup must classify workspace setup state before constructing
  event-store adapters.
- Dashboard startup must distinguish operator bootstrap from project/workspace
  startup. The operator home can exist without any selected project, and one
  operator home can manage many projects.
- Setup-mode endpoints must be safe to serve without a canonical event-store
  adapter. They may write local configuration only through explicit setup
  actions.
- Setup must preserve the repo-local versus user-level split: Git-substrate
  event-store descriptors may live in `.bicameral/config.yaml`; private
  operator settings and runtime stores default under
  `~/.bicameral/projects/<project-id>/`.
- Non-Git substrates must not use a repo-local `.bicameral/` fallback. They
  resolve project state from bicameral-cloud amenity/invite metadata or the
  user-level project registry.
- Adapter construction failures should become typed diagnostics where possible,
  not panics.
- Existing CLI commands may remain, but docs should present them as explicit
  advanced paths rather than required first-run ceremony.

## Failure Behavior

| Failure | Required behavior |
| --- | --- |
| Hosted installer points at old release | New release tag must be published; site copy alone is insufficient. |
| Hosted installer prints stale next step | Update `bicameral-install` and deploy the install site. |
| Binary installed without dashboard assets | Installer/release validation fails. |
| PATH not updated for current shell | Installer prints a clear PATH instruction; state remains `dashboard_assets_installed`. |
| Operator home missing | Start operator bootstrap; do not create a project or event store implicitly. |
| Operator home exists but has no projects | Show create/join project flow in dashboard. |
| Multiple projects registered | Show/select active project; do not infer from cwd for non-Git substrates unless registry maps the cwd. |
| `.bicameral/` marker exists without usable config | Treat as setup-needed or typed diagnostic, not normal runtime. |
| Drive substrate missing `drive_folder_path` | Start setup mode before constructing `DriveFolderAdapter`. |
| Provider unavailable during setup | Dashboard shows readiness/deferred state; setup can continue for local config. |
| Event-store replay fails after normal startup | Normal mode may continue with warning only when adapter opened and replay failure is non-destructive. |

## Edge-Case Matrix

| Edge case | Classification | Required behavior |
| --- | --- | --- |
| Command is run inside nested repos and more than one parent contains `.bicameral/` | Workspace selection | Use the nearest parent `.bicameral/` by default; explicit `--workspace` may override when supported. |
| Command is run in a registered user-level project with no repo-local `.bicameral/` | Workspace selection | Resolve the project through `~/.bicameral/projects/<project-id>/origin.txt`, bicameral-cloud amenity metadata, or equivalent registry metadata. |
| Command is run outside any registered project but operator home exists | Project selection | Open dashboard project picker/create/join flow instead of failing as not installed. |
| Command is run from `$HOME` or another broad parent that accidentally contains `.bicameral/` | Workspace selection | Treat it as a workspace only if config is usable; setup UI should show the selected root before writing state. |
| `.bicameral/` exists because another tool or agent created a marker-only directory | Workspace setup | Classify as `workspace_marker_only`; do not infer normal runtime readiness. |
| `.bicameral/config.yaml` is missing | Workspace setup | Classify as setup-needed or create default config only through explicit setup/init action. |
| `.bicameral/config.yaml` contains private/operator keys from a legacy setup | Workspace setup | Offer migration to `~/.bicameral/projects/<project-id>/operator.yaml`; do not continue writing private keys repo-local. |
| `.bicameral/config.yaml` is malformed YAML | Workspace setup | Classify as `workspace_unrecoverable`; show typed parse error and do not overwrite automatically. |
| `.bicameral/config.yaml` has unknown future schema version | Workspace setup | Refuse normal startup unless a migration path is available; show migration/upgrade diagnostic. |
| Gateway host is non-loopback | Safety validation | Refuse normal startup per ADR-0020; setup may offer safe local rewrite only with explicit user action. |
| Gateway port is zero or already in use | Runtime startup | Return typed diagnostic and suggest/select a safe local alternative; do not silently bind a public interface. |
| Dashboard assets are missing | Runtime startup | Serve API-only mode only when intentional/developer path; public release validation must fail missing assets. |
| Binary path resolves to an older install while a newer release exists elsewhere on PATH | Release delivery | Installer should print install path; diagnostics should report resolved binary path/version. |
| Release archive checksum fails | Release delivery | Abort install; do not leave partially installed binary or dashboard assets as current. |
| Release archive lacks dashboard assets | Release delivery | Abort install; public release validation must catch this before publication. |
| Installed binary is symlinked from another install dir | Asset discovery | Resolve dashboard assets next to the real binary as well as the symlink path. |
| Existing daemon/service is running from an older binary | Daemon/service state | Dashboard command should report version/path mismatch; installer must not kill services without explicit user action. |
| User has no write permission to install dir | Release delivery | Abort with install-dir diagnostic and preserve existing install. |
| User has no write permission to workspace `.bicameral/` | Workspace setup | Setup mode may render diagnostics but must not claim configuration was saved. |
| User has no write permission to `~/.bicameral/projects/<project-id>/` | Workspace setup | Setup mode may render diagnostics but must not claim operator/runtime state was saved. |
| Same git repo is opened from two linked worktrees | Workspace selection | Resolve the same project id and user-level project directory; do not fork ledgers or indexes per worktree. |
| Repo is moved or renamed | Workspace selection | Preserve project identity when repo config or locator metadata can prove continuity; otherwise require explicit relink/migration. |
| Project directory under `~/.bicameral/projects/<id>/` exists but `origin.txt` points to a deleted repo | Workspace setup | Treat as orphaned operator state; offer relink or garbage-collection path, not automatic deletion. |
| Drive folder path is set but unavailable/offline | Workspace setup/runtime | Start with typed substrate readiness state when safe; do not report data as fresh or synced. |
| Drive substrate selected and no repo-local `.bicameral/` exists | Workspace setup/runtime | Required path for Drive; use user-level/cloud project registration and do not create repo-local `.bicameral/`. |
| Drive substrate selected and repo-local `.bicameral/` exists | Workspace setup/runtime | Ignore it for Drive unless migrating legacy state; do not treat it as the Drive project registry. |
| Git event-store path is configured outside workspace | Workspace setup/runtime | Allow only if policy permits; otherwise return typed diagnostic before adapter mutation. |
| Memory substrate is selected | Runtime startup | Permit normal startup but label state non-durable in dashboard/readiness. |
| Setup mode writes local config and then user refreshes dashboard | Setup transition | Reclassify state from disk; normal mode starts only after config validates. |
| Setup mode is open in two browser tabs | Setup transition | Writes must be idempotent or conflict-detected; last write must not silently discard incompatible substrate choices. |
| User cancels setup | Setup transition | Leave no partial canonical event-store state; partial config writes must be explicit and recoverable. |
| Provider setup is incomplete but event-store config is valid | Workspace setup | Normal dashboard may start with provider readiness `not_configured`; do not block local ledger reads. |
| Source connector setup fails | Workspace setup | Keep connector state separate from workspace readiness; local dashboard remains usable. |
| Existing event store contains data but config now points at a different substrate | Workspace setup/runtime | Require explicit migration or confirmation; do not silently fork ledger authority. |
| Replay succeeds but projection/read model rebuild fails | Runtime startup | Serve dashboard with projection/readiness diagnostic; canonical event store remains authoritative. |
| Adapter opens but append fails during later setup action | Runtime mutation | Return failed action receipt; do not claim the setup or governance event materialized. |
| `bicameral dashboard` is run outside a repo intentionally | Workspace setup | Setup mode must let the user choose/create a workspace root instead of guessing a hidden global workspace. |

## Non-Goals

- This ADR does not change release-artifact trust posture. ADR-0013 owns
  checksums, GitHub release assets, build provenance, and future signing.
- This ADR does not make the shell installer responsible for workspace setup,
  secrets, connectors, daemon registration, or source ingestion.
- This ADR does not define the full dashboard setup UX; it defines the state
  boundary that the UX and CLI must respect.
- This ADR does not grant setup mode authority to create Decisions,
  candidates, signoff, compliance, or canonical event-store mutations.

## Validation Hooks

Future release checks should cover:

- `curl ... | sh` equivalent against a release archive installs binary and
  dashboard assets.
- From a clean temp home and empty temp repo, `bicameral dashboard` starts setup
  mode.
- From a workspace with Drive substrate and missing `drive_folder_path`,
  `bicameral dashboard` starts setup mode or returns a typed setup-needed
  diagnostic.
- From a marker-only `.bicameral/`, `bicameral dashboard` starts setup mode or
  returns a typed setup-needed diagnostic.
- From a repo-local config with no user-level project directory, `bicameral
  dashboard` starts setup mode or creates operator state only through explicit
  local setup action.
- From two linked worktrees of the same repo, state resolution points at the
  same `~/.bicameral/projects/<project-id>/` directory.
- From a Drive-folder setup with no repo-local `.bicameral/`, `bicameral
  dashboard` resolves the registered user-level project and starts without
  creating repo-local state.
- From a Drive-folder setup with a stray repo-local `.bicameral/`, `bicameral
  dashboard` does not use it as a fallback project registry.
- From legacy repo-local private keys or local state files, setup offers
  migration into `~/.bicameral/projects/<project-id>/`.
- From malformed config, `bicameral dashboard` returns a typed parse diagnostic
  and does not overwrite config automatically.
- From unsafe gateway config, `bicameral dashboard` refuses normal startup
  before binding a socket.
- From a configured but unavailable substrate path, startup returns a substrate
  readiness diagnostic instead of panicking or claiming freshness.
- From a configured workspace, `bicameral dashboard` constructs the selected
  adapter and serves normal dashboard routes.
- Hosted `install.bicameral-ai.com/install.sh` prints `bicameral dashboard` as
  the next step.
