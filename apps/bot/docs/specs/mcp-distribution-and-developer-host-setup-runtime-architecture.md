# Runtime Architecture: MCP Distribution and Developer-Host Setup

Date: 2026-07-28 (amended 2026-09-10 for the locked bot#1239 host-permission policy)
Status: draft — proposed runtime contract, not implementation authorization
Level: L2 cross-repository contract

Feature proposal: [MCP distribution and developer-host setup](mcp-distribution-and-developer-host-setup.md).
Canonical machine contract: [mcp-distribution-and-developer-host-setup-runtime-architecture.contract.json](mcp-distribution-and-developer-host-setup-runtime-architecture.contract.json).

Governance gate: `bic:spec-governance-gate`; Qor advisory/no-op pending
Factory policy revision. `UserJourney: developer_host_mcp_v1` (canonical,
currently partial). Journey Boundary Review: **passed** for the daemon-owned,
pairing-independent readiness boundary at exact PR head
`ae7fb94276cf1e9fb15ab83cbbcfbdaeb342a764`, recorded in
[PR #950 comment 5106871327](https://github.com/BicameralAI/bicameral-bot/pull/950#issuecomment-5106871327).
Factory catalog registration remains required before topology execution.

## Runtime Invariant

```text
Only an exact, manifest-bound MCP artifact may be registered with a host.
Only the host may grant workspace, hook, or tool trust.
Only one consented, Bicameral-server-scoped host permission may be added by setup.
Only a verified fresh-session product receipt may produce active_verified.
Only the local daemon may reduce and persist capability readiness.
Dashboard pairing may authorize a read; it may never cause setup or readiness.
Capability readiness contains no Product identity and causes no Product entry.
```

## Scope

This contract covers macOS and Linux per-user installation of the Bicameral
daemon, thin MCP package, Claude Code or Codex MCP registration, managed
pre-work hook, host trust handoff, fresh-session activation evidence, repair,
and uninstall.

Windows, machine-wide deployment, host installation, provider authentication,
repo-local skills, and Product recall/Decision workflows are outside this
contract. v0.1.14 implements only MCP readiness. Reasoning-provider discovery,
configuration, validation, narration, and readiness are reserved for v0.2.0,
where they must reuse the daemon-owned readiness envelope and terminal-only
mutation boundary.

## Component and Authority Projection

| ID | Runtime component | Authority | Prohibited authority |
| --- | --- | --- | --- |
| `ELM-OPERATOR` | Person performing setup. | Consent and host-owned approvals. | Fabricate receipt or bypass host trust. |
| `ELM-RELEASE-ORIGIN` | Public immutable Bot artifacts and manifest. | Published artifact bytes/provenance. | Choose local install state. |
| `ELM-BOOTSTRAP` | Public per-user shell installer. | Copy verified Bot artifact to user path. | Install MCP/hosts, elevate, or configure trust. |
| `ELM-SETUP-CONTROLLER` | Local Bot CLI setup controller. | Orchestrate declared terminal steps, reconcile the consented Bicameral-server-scoped host permission grant, and submit verified local evidence. | Accept dashboard setup intent, reduce readiness, write host config beyond that grant, broaden shell/sandbox/network/unrelated-MCP/host-wide authority, or approve trust. |
| `ELM-UV` | Per-user Python tool installer. | Materialize exact wheel environment. | Resolve an unbound latest package or global environment. |
| `ELM-MCP-ADAPTER` | Released `bicameral-mcp` CLI and adapter. | MCP runtime, managed hook writes, product receipt. | Install daemon, copy repo skills, grant host trust, or own canonical Product state. |
| `ELM-DEVELOPER-HOST` | Claude Code or Codex. | MCP registration, workspace/hook/tool trust, SessionStart. | Assert Product activation without receipt verification. |
| `ELM-LOCAL-DAEMON` | Local Bicameral daemon. | Reduce, persist, invalidate, and project operator-local capability readiness. | Mutate host config/trust, bind readiness to pairing/Product state, or accept dashboard setup control. |
| `ELM-DASHBOARD` | Hosted or loopback dashboard readiness surface. | Render an authorized daemon projection, copy safe terminal guidance, and navigate explicitly to Products. | Invoke setup/validation/repair, write readiness, submit receipts, or create/select a Product as a side effect. |
| `ELM-OPERATOR-HOME` | Per-user filesystem stores. | Persistence boundary for install/config/receipts. | Become Product ledger authority. |

## State Projection

| ID | Artifact | Class | Authoritative writer | Persistence |
| --- | --- | --- | --- | --- |
| `STATE-RELEASE-BINDING` | Exact Bot target and compatible MCP wheel/descriptor/protocol coordinates. | Evidence | Release origin | Immutable public release manifest |
| `STATE-PACKAGE-INSTALL` | Installed MCP version, digest, environment, executable, descriptor. | Derived projection | `uv` plus setup verifier | Per-user MCP tool root |
| `STATE-HOST-REGISTRATION` | Host-native user-scoped MCP registration. | Derived projection | Developer host | Host user config |
| `STATE-MANAGED-HOOK` | Package-owned managed hook identity and lifecycle status. | Derived projection | MCP adapter | Host user hook config |
| `STATE-HOST-TRUST` | Workspace, hook, and tool approval state. | Derived external state | Developer host/operator | Host-owned state |
| `STATE-HOST-PERMISSION` | The single Bicameral-server-scoped grant Bicameral added after consent, plus its owned-rule record. | Derived projection | Setup controller | Host user permission file (only the Bicameral entry) and `$HOME/.local/share/bicameral/mcp-host-permissions-v1.json` |
| `STATE-ACTIVATION-RECEIPT` | Sanitized durable fresh-session receipt. | Audit receipt | MCP adapter | Per-host MCP state directory |
| `STATE-SETUP-READINESS` | Daemon-owned capability readiness and reason metadata; v0.1.14 has only `capability_kind=mcp`. | Derived projection | Local daemon | Operator-local daemon setup store |

No state above is a canonical Decision, accepted Product event, recall packet,
or compliance determination.

## Relationships

```text
release origin -> bootstrap -> operator home
release origin -> setup controller -> uv -> MCP adapter
setup controller -> developer host -> MCP adapter
developer host -> MCP adapter -> local daemon
MCP adapter -> activation receipt -> local daemon -> readiness
local daemon -> authorized read projection -> dashboard
operator -> consent/trust points
```

The local CLI setup controller may invoke only stable public CLIs. It consumes
structured JSON from the MCP adapter and host status commands. It must not
infer success from prompt text or directly edit a host document.

The dashboard is never part of the mutation chain. It may read readiness only
through the existing paired bridge or loopback read boundary. Pairing,
unpairing, revocation, polling, command copying, and navigation do not enter the
readiness reducer.

## State Refinement Check

The smallest honest readiness lifecycle is:

```text
                          ┌─> prerequisite_missing ─┐
not_installed ────────────┤                        ├─> not_installed
                          └─> installed_pending_host_action
                                    │
                                    v
                              active_verified
                                    │
                                    v
                              repair_required
```

`prerequisite_missing` may return to `not_installed` after remediation.
`installed_pending_host_action` becomes `active_verified` only through a valid
fresh-session receipt. Any exact-binding or configuration drift moves
`active_verified` to `repair_required`.

This lifecycle is keyed by
`(operator, capability_kind, subject_id, artifact_generation)`. It contains no
bridge, dashboard-session, pairing-request, or Product key. In v0.1.14
`capability_kind` is exactly `mcp`; `reasoning_provider` is a v0.2.0 additive
capability, not a dormant v0.1.14 state.

State minimization:

- Package, registration, hook, and trust are observable artifacts, not separate
  top-level lifecycle enums.
- Detailed missing/drift causes are reason codes.
- `installed_enabled` remains adapter configuration status.
- No `trusted` state is copied into Bicameral; trust stays external and is
  observed only as sanitized readiness evidence.
- Host permission is an attribute of readiness (`host_permission`:
  `configured`, `missing`, `drifted`, `unsupported`), not a lifecycle state; it
  never moves the state machine and never produces `active_verified`.
- No process-running state exists; process presence is diagnostic.
- Dashboard `copied`, `viewed`, `continued`, or `setup_requested` states are
  rejected. Copy/view/navigation are UI events, and dashboard setup requests
  are forbidden.
- `reasoning_not_ready` is deferred rather than serialized in v0.1.14 because
  no reasoning setup lifecycle ships in that release.

## Transitions

### Resolve release

The bootstrap resolves a macOS/Linux target from an immutable manifest, verifies
the archive, and copies the daemon to `$HOME/.local/bin` or an explicitly chosen
user-writable directory. Missing target, checksum, or provenance stops without
mutation.

### Install package

The local CLI setup controller requires an MCP binding in the verified Bot manifest. It
shows the exact wheel URL/digest and invokes `uv` in dedicated per-user tool
roots after consent. It verifies package metadata without selecting an ambient
executable.

### Register host

The controller invokes `claude mcp add` or `codex mcp add` with the absolute
managed executable, then verifies using the host CLI. This transition writes
host-owned registration state but no trust.

### Grant host permission

After one explicit consent step, the controller adds the broadest host-native
Bicameral-server-scoped grant: Claude Code user `permissions.allow` rule
`mcp__bicameral`, or Codex `[mcp_servers.bicameral]
default_tools_approval_mode = "approve"`. Where a server-scoped grant is
unavailable it derives the read-only allowlist deterministically from the
declared Bicameral tool registry; without a classified registry the grant is
`unsupported` and every tool stays host-prompted. Stricter existing policy,
unrelated settings, and malformed files are preserved untouched. The controller
records exactly what it added. Without consent nothing is written.

### Install hook

The controller invokes `bicameral-mcp adapters install --consent --json`. The
adapter locks, backs up, validates, atomically updates, and re-reads the expected
host file. Status derives the managed command and identity from product output.

### Grant trust

The operator completes workspace and hook trust in the normal host surface.
There is no programmatic transition for trust approval in this contract.

### Verify activation

At a genuine fresh-session `SessionStart`, the managed adapter performs a
daemon handshake/preflight and writes a sanitized receipt. The local CLI calls
the packaged receipt verifier and submits the verified local result to the
daemon. The daemon checks freshness and exact bindings and alone reduces
readiness to `active_verified`.

### Repair and uninstall

Changed bindings or config invalidate activation. Repair repeats only the
necessary declared steps and always requires host review for changed hooks.
Repair on already-correct permission state is a no-op and never clobbers an
operator-tightened grant. Uninstall removes only the Bicameral-owned permission
grant, then reverses hook, host registration, and package state in that order
while preserving unrelated config and operator-modified permission entries.

### Project readiness

This contract defines no Product-readiness transition. After the dashboard
renders local runtime readiness, `Continue to Products` performs browser
navigation only. Product discovery, creation, join, registration, workspace
binding, and open remain separate downstream lifecycles with their existing
authorities.

## Permission and Secret Boundary

The normal flow is non-elevated and may write only:

- `$HOME/.local/bin/bicameral`;
- `$HOME/.local/share/bicameral/**`;
- host user-scoped MCP registration through host CLI;
- `~/.claude/settings.json` through the package adapter (managed hook) and,
  after explicit consent, the single `permissions.allow` rule `mcp__bicameral`
  through the setup controller;
- `$CODEX_HOME/hooks.json` through the package adapter and, after explicit
  consent, `default_tools_approval_mode` under `[mcp_servers.bicameral]` in
  `$CODEX_HOME/config.toml` through the setup controller;
- sanitized operator-local readiness and receipt records.

It may not write `/etc`, `/usr`, `/usr/local`, root-owned files, managed host
policy, host authentication, workspace trust, hook trust, sandbox settings,
global approval policy, network settings, shell/Bash rules, unrelated MCP
server settings, or repository skills.

The dashboard may perform only the authorized readiness `GET`, render/copy a
safe allowlisted command, and navigate. There is no hosted setup `POST`, action
request, receipt submission, executable selection, provider selection, or
validation trigger in this contract.

Provider credentials are never inputs to this setup controller. Host
authentication is a separate host-owned journey. Environment aliases may be
checked for presence during external topology, but values may never be copied
into package commands, configuration, logs, receipts, recordings, or errors.

## Failure Semantics

| Failure | Result |
| --- | --- |
| Missing compatible MCP binding, target, `uv`, Python runtime, or host | `prerequisite_missing` |
| Artifact digest, descriptor, protocol, or executable mismatch | `repair_required` |
| Malformed/ambiguous host configuration | `repair_required`; no rewrite |
| Host permission consent declined, stricter host policy present, or grant later removed/changed by the operator | Lifecycle state unchanged; `host_permission` reports `missing` or `drifted`; nothing is written |
| Operator declines a declared mutation or host trust | `installed_pending_host_action` or unchanged prior state |
| Host registration succeeds but hook install fails | Partial evidence retained; no activation claim |
| Hook installed but no valid fresh-session receipt | `installed_pending_host_action` |
| Valid receipt differs from current package/config/host | `repair_required` |
| Daemon capability/handshake failure in the receipt | `repair_required` with typed reason |
| Dashboard unpaired or revoked | Remote projection unavailable; daemon readiness unchanged |
| Dashboard refresh or copied command | Projection re-read or browser-local event; daemon readiness unchanged |
| Product absent or no Product selected | Readiness remains resolvable; Product lifecycle unchanged |
| Reasoning provider absent in v0.1.14 | No readiness record; release metadata reports the capability is deferred to v0.2.0 |

External topology classifies missing credentials or unavailable providers as
environment inconclusive, runner/contract defects as harness failure, and a
ready session with a valid product receipt showing incorrect Product behavior
as product failure. Those test outcomes do not replace runtime readiness.

## Runtime Scenarios

### Clean installation

1. Operator installs a verified daemon artifact per-user.
2. Setup preflight finds a compatible release binding and selected installed
   host.
3. Operator confirms exact network/filesystem mutations.
4. `uv` installs the exact wheel into dedicated roots.
5. Host CLI persists and verifies user-scoped MCP registration.
6. Operator answers the single host-permission consent step; the controller
   adds only the Bicameral-server-scoped grant and records it.
7. MCP adapter persists and verifies exactly one managed hook.
8. Setup stops at `installed_pending_host_action` with
   `host_permission: configured` (or `missing` when consent was declined).

### Host activation

1. Operator completes workspace and hook trust in the host.
2. Operator starts a fresh normal host session.
3. Host fires the managed `SessionStart` hook.
4. Adapter handshakes with the local daemon and writes a durable receipt.
5. Packaged verifier validates the receipt and exact current configuration.
6. The daemon reduces the verified receipt and persists `active_verified`.

### Drift and repair

1. Package, executable, registration, hook document, or compatible Bot release
   changes.
2. Status refuses the stale receipt and produces a typed drift reason.
3. The daemon invalidates the stale binding and persists `repair_required`.
4. Operator reviews the repair diff.
5. Adapter update and host trust run normally.
6. A new session/receipt re-establishes `active_verified`.

### Uninstall

1. Operator reviews exact removals.
2. The controller removes only the host permission grant it recorded as its
   own; an operator-modified entry is preserved and reported.
3. Adapter removes only its managed hook entry and proves unrelated config is
   unchanged.
4. Host CLI removes only the `bicameral` MCP registration.
5. `uv` removes only the manifest-bound MCP tool environment.
6. Setup deletes only Bicameral activation/readiness records and reports
   `not_installed`.

### Dashboard readiness projection

1. Readiness already exists in the daemon, whether or not a dashboard is
   paired.
2. A paired browser performs the existing authorized read request.
3. The daemon returns `DaemonCapabilityReadinessV1` for `mcp`, reason codes,
   timestamps, and an allowlisted terminal next action.
4. The dashboard renders the state and may copy
   `bicameral setup mcp`.
5. No setup transition occurs. An explicit `Continue to Products` navigates to
   Operator Home without creating, joining, selecting, registering, binding, or
   opening a Product.

Unpairing or revocation stops the remote read and changes no readiness fact.

### Forbidden dashboard setup

Any dashboard request that attempts to install, configure, validate, repair,
uninstall, submit a receipt, select an executable/provider, or attach a Product
identity is rejected as an unsupported authority path. The daemon must not
translate such a request into a local CLI invocation.

## Conformance Summary

Implemented today:

- public Bot R2/GitHub release artifacts for four macOS/Linux targets;
- per-user shell bootstrap and archive SHA-256 check;
- public PyPI project and universal MCP wheel lineage;
- exact MCP package/host-adapter lifecycle at the reviewed `0.17.0` head;
- host-native Claude/Codex MCP and trust mechanisms.

Partial:

- release artifact authenticity is checksum-only in the current Bot bootstrap;
- developer-host topology exists but does not yet cover this complete setup
  journey.

Missing:

- stable publication of the thin `0.17.0` generation;
- Bot release-manifest MCP binding;
- daemon readiness reducer and operator-local persistence;
- read-only paired-dashboard readiness projection and negative setup route;
- official host registration orchestration;
- durable activation receipt consumption;
- Factory catalog journey and full macOS/Linux topology matrix.

Deferred to v0.2.0:

- reasoning-provider discovery and terminal configuration;
- content-free reasoning protocol validation;
- reasoning-provider readiness in `DaemonCapabilityReadinessV1`;
- reasoning-dependent narration readiness.

## Validation Obligations

- Contract tests for canonical manifest parsing, direct wheel URL/digest
  enforcement, and refusal of unbound/stale packages.
- Integration tests with disposable HOME/CODEX_HOME proving host CLI
  registration and adapter lifecycle preservation.
- Permission tests proving no elevation, machine path, global Python, shell,
  trust-bypass, unrelated-server or host-wide allowlist, or secret persistence.
- Host-permission tests with disposable Claude Code and Codex configuration
  homes proving the scoped grant, registry-derived deterministic fallback,
  idempotent repair, preservation of unrelated MCP, shell, network, sandbox,
  global approval, and stricter policy, owned-only uninstall, and that a
  configured grant never manufactures activation.
- Receipt tests proving activation cannot derive from config presence, prompt
  text, PTY bytes, or process sampling.
- Pairing/Product-independence tests proving ready-before-pairing,
  paired-not-ready, paired-ready, revoked-still-ready, zero-Product readiness,
  and navigation-without-Product-mutation.
- Route tests proving the dashboard has no setup, validation, repair, uninstall,
  receipt-submission, executable-selection, or provider-selection command.
- Release-cut tests proving v0.1.14 emits only MCP capability readiness and does
  not discover, invoke, validate, or configure an LLM.
- Topology for clean install, trust handoff, fresh-session activation, repair,
  uninstall, and cross-host independence on each supported OS/architecture
  class declared by the release.

## Journey Boundary Review

The human-visible walkthrough and commit points are canonical in the feature
proposal. The per-user, non-elevated portion was confirmed on 2026-07-28. The
added daemon-owned, pairing-independent, Product-independent readiness boundary
was approved for exact PR head
`ae7fb94276cf1e9fb15ab83cbbcfbdaeb342a764` in
[PR #950 comment 5106871327](https://github.com/BicameralAI/bicameral-bot/pull/950#issuecomment-5106871327).
Topology readiness still requires:

1. Factory refines the existing partial `developer_host_mcp_v1` journey with
   the setup/readiness checkpoints below;
2. topology checkpoints bind daemon artifact, MCP wheel, host registration,
   hook identity, host trust, activation receipt, daemon readiness, the
   read-only dashboard projection, pairing/Product-independence negative
   controls, and teardown.
