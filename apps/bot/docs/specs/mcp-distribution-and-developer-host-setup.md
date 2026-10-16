# MCP Distribution and Developer-Host Setup

Date: 2026-07-28 (amended 2026-09-10 for the locked bot#1239 host-permission policy)
Status: draft — proposed product contract, not release authorization
Level: L2 cross-repository feature proposal
Owners: Bicameral Bot (per-user setup), Bicameral MCP (package and host adapters)

Runtime companion: [MCP distribution and developer-host setup runtime architecture](mcp-distribution-and-developer-host-setup-runtime-architecture.md).
Canonical machine contract: [mcp-distribution-and-developer-host-setup-runtime-architecture.contract.json](mcp-distribution-and-developer-host-setup-runtime-architecture.contract.json).

Governance gate: `bic:spec-governance-gate`; Qor advisory/no-op pending Factory policy revision.
User journey: canonical `developer_host_mcp_v1` in
`developer-host-environment-v1` + `local-product-environment-v1`.
Journey Boundary Review: **passed** — the per-user, non-elevated boundary was
previously confirmed, and the responsible human approved daemon-owned,
pairing-independent readiness with a read-only dashboard projection for exact
PR head `ae7fb94276cf1e9fb15ab83cbbcfbdaeb342a764` in
[PR #950 comment 5106871327](https://github.com/BicameralAI/bicameral-bot/pull/950#issuecomment-5106871327).
The journey is not executable until the Factory catalog contains it.

## Decision

Bicameral supports one normal, per-user setup flow on **macOS and Linux**:

```text
verified Bicameral daemon release
  -> operator runs local CLI setup
  -> exact compatible bicameral-mcp wheel from PyPI
  -> official host MCP registration
  -> one consented Bicameral-server-scoped host permission grant
  -> package-owned managed hook installation
  -> host-owned trust/permission review
  -> fresh-session activation verification
  -> daemon-owned readiness
  -> paired dashboard may render readiness
```

The local CLI coordinates consented setup with the daemon and the released
`bicameral-mcp` adapter. The daemon owns operator-local readiness independently
of dashboard pairing and Product context. It does not implement host
configuration formats. The released `bicameral-mcp` adapter performs consented
host mutations and reports their exact provenance.

The hosted dashboard is a read-only consumer of the daemon projection. Pairing
controls whether the browser may observe that projection; it does not create,
clear, validate, or repair readiness. The dashboard may render or copy a safe
terminal command, but it has no setup command route and supplies no pairing
code, setup token, Product identifier, executable path, or authority-bearing
argument to that command.

The normal path is strictly per-user. It must not require `sudo`, write
machine-wide policy, approve host trust, broaden an agent sandbox, or persist a
Claude/OpenAI credential.

Windows is out of scope for this contract.

## Current-State Finding

The previous MCP download flow still exists, but it is not the new product
flow:

- PyPI currently serves stable `bicameral-mcp==0.16.4`, including the retired
  monolithic `bicameral-mcp setup` behavior.
- The thin ToolRequest client and consented Claude/Codex adapters are versioned
  `0.17.0` at the reviewed MCP head, but are not yet the stable PyPI release.
- The live Bot `v0.1.12` manifest distributes macOS and Linux daemon archives
  from the public R2 origin. It does not bind an MCP package version or digest.
- The existing Bot setup-status endpoint reports only workspace presence; it
  cannot truthfully report MCP registration, managed-hook, host-trust, or
  activation readiness.

Therefore the old `uv tool install bicameral-mcp && bicameral-mcp setup`
quickstart remains historically available but must not be presented as the
normal setup for the thin-client architecture. The daemon must fail closed with
`mcp_distribution_unavailable` until its release manifest binds a compatible
published MCP artifact.

## Goals

- Give a user one setup surface for the daemon, MCP client, and each selected
  developer host.
- Keep readiness useful before dashboard pairing and before any Product is
  created, joined, selected, registered, or opened.
- Let a paired dashboard render one shared readiness gate without gaining
  setup or validation authority.
- Bind every installed MCP package to the exact Bot release compatibility
  declaration.
- Use host-native MCP registration, permission, workspace-trust, and hook-trust
  flows.
- Preserve unrelated Claude and Codex configuration through install, update,
  disable, and uninstall.
- Distinguish installed configuration from verified activation.
- Make every filesystem or network mutation visible before consent.
- Keep install and runtime credentials out of commands, logs, receipts, and
  recordings.

## Non-Goals

- Installing Claude Code or Codex for the user.
- Copying repository-local skills into a host.
- Recreating the retired MCP setup wizard.
- Preapproving tools of any server other than the Bicameral MCP server, or
  weakening the host's normal approval policy for shell, sandbox, network,
  unrelated MCP servers, or host-wide bypass.
- Bypassing workspace trust or hook trust.
- Treating hook activation as proof of recall, candidate rendering,
  confirmation, Decision materialization, or compliance.
- Supporting machine-wide, root, MDM, or container-image installation.
- Supporting Windows in this version.
- Browser-initiated package installation, host registration, provider
  selection, validation, repair, or uninstall.
- Selecting or opening a Product as a side effect of pairing or readiness.
- Implementing reasoning-provider or narrator configuration in v0.1.14;
  reasoning readiness is scheduled for v0.2.0.

## Release Sequencing

### v0.1.14

v0.1.14 implements the MCP member of a daemon-owned capability-readiness
contract:

- local terminal setup and repair;
- daemon-owned, operator-scoped MCP readiness;
- a read-only paired-dashboard projection;
- an explicit `Continue to Products` navigation action after readiness;
- no automatic Product selection, creation, join, registration, or open.

### v0.2.0

v0.2.0 may add `reasoning_provider` as a second capability in the same
daemon-owned readiness envelope. Its setup must remain terminal-driven and
dashboard-independent. A paired dashboard may render the resulting state and
suggest `bicameral setup reasoning`, but it may not select an LLM executable,
write provider configuration, start a validation run, or receive provider
credentials.

v0.1.14 must not manufacture a `reasoning_provider` readiness record. An absent
capability is represented by release capability metadata, not by a false
`ready`, `unavailable`, or `repair_required` lifecycle state.

## Exact Distribution Channels

### Bicameral daemon and dashboard

| Concern | Contract |
| --- | --- |
| Bootstrap URL | `https://install.bicameral-ai.com/install.sh` |
| Artifact origin | `https://pub-7eab94b264754bee8019fd89ca9b5b0b.r2.dev/bicameral` |
| Release pointer | `<origin>/latest.json`, or `BICAMERAL_VERSION=vX.Y.Z` for an exact version |
| Supported targets | `aarch64-apple-darwin`, `x86_64-apple-darwin`, `aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-gnu` |
| Install location | `${BICAMERAL_INSTALL_DIR:-$HOME/.local/bin}/bicameral` |
| Integrity | Archive digest must match the immutable versioned manifest/checksum record before extraction |
| Release mirror | GitHub Release for the same tag and asset digests; not the installer's floating source of truth |

The current bootstrap verifies `SHA256SUMS`, but fetches that file from the same
origin and ad-hoc signs the downloaded macOS binary. Before this journey can be
classified as fully verified, release CI must sign/notarize the macOS binary
before archiving and the bootstrap must verify a signed provenance statement or
equivalent release attestation. The installer must not modify the verified
binary after verification.

### Bicameral MCP

| Concern | Contract |
| --- | --- |
| Package index | Public PyPI project `bicameral-mcp` |
| Required package generation | Thin-client generation `>=0.17.0`; the release manifest names one exact version |
| Install input | Direct immutable wheel URL plus SHA-256 from the Bot release manifest |
| Environment | Dedicated per-user `uv` tool environment |
| Tool root | `$HOME/.local/share/bicameral/mcp-tools` |
| Executable root | `$HOME/.local/share/bicameral/bin` |
| Runtime requirement | Python 3.11+ resolved by `uv`; no global Python environment mutation |
| Provenance | Package version, wheel SHA-256, release descriptor digest, executable path, and Bot compatibility range |

The normal installer must not resolve the unqualified latest package. Its
effective install is equivalent to:

```sh
UV_TOOL_DIR="$HOME/.local/share/bicameral/mcp-tools" \
UV_TOOL_BIN_DIR="$HOME/.local/share/bicameral/bin" \
uv tool install --force \
  "https://files.pythonhosted.org/.../bicameral_mcp-X.Y.Z-py3-none-any.whl#sha256=<digest>"
```

The URL and digest come only from the verified Bot release manifest. The daemon
must then execute:

```sh
$HOME/.local/share/bicameral/bin/bicameral-mcp --version
```

and require the reported version and packaged release descriptor to match the
manifest. A package from another index, a source checkout, an editable install,
or a version selected from ambient `PATH` does not satisfy normal setup.

If `uv` is absent, setup may offer the official Astral per-user installer as a
separate, disclosed network action. Declining leaves readiness at
`prerequisite_missing`; setup must not fall back to `sudo pip`, global `pip`, or
the system Python environment.

### Compatibility binding

Bot release manifest v2 adds:

```json
{
  "mcp": {
    "package": "bicameral-mcp",
    "version": "0.17.0",
    "wheel_url": "https://files.pythonhosted.org/.../bicameral_mcp-0.17.0-py3-none-any.whl",
    "wheel_sha256": "<64 lowercase hex>",
    "release_descriptor_digest": "sha256:<64 lowercase hex>",
    "tool_request_protocol": "v2",
    "python_requires": ">=3.11"
  }
}
```

The publisher derives these fields from the published wheel and MCP release
descriptor. Hand-entered package coordinates are invalid. Any byte, version,
protocol, or descriptor change invalidates the binding.

## Exact Setup Journey

### 1. Preflight without mutation

`bicameral setup mcp` (alias: `bicameral setup developer-host`):

1. reads the verified Bot release manifest;
2. checks OS/architecture, user-home writability, `uv`, daemon health, and
   installed Claude/Codex versions;
3. runs no package or host mutation;
4. displays in the terminal the exact downloads, commands, files, bounded
   context, and rollback
   plan;
5. records no production attempt or success state.

Missing prerequisites produce typed remediation and no partial success claim.
Opening or pairing the dashboard performs none of these checks and causes none
of these transitions.

### 2. Install the exact MCP package

After an explicit operator confirmation, the setup controller invokes `uv`
directly with list-form arguments and the dedicated per-user roots. It verifies
the installed package and descriptor against the manifest.

The controller never passes secrets in argv, never uses a shell wrapper, and
never imports package code merely to discover version or provenance.

### 3. Register the MCP server through the host

For each selected host, the controller invokes the host's official CLI with the
absolute managed executable:

```sh
claude mcp add --scope user --transport stdio bicameral -- \
  $HOME/.local/share/bicameral/bin/bicameral-mcp

codex mcp add bicameral -- \
  $HOME/.local/share/bicameral/bin/bicameral-mcp
```

The adapter verifies registration through `claude mcp get bicameral` or
`codex mcp list`. The daemon does not hand-write `~/.claude.json`,
`.mcp.json`, or `~/.codex/config.toml`.

Registration is user-scoped. Project-scoped registration is a separate,
explicit choice because it changes repository or workspace trust behavior.

### 4. Configure the Bicameral-server-scoped host permission (locked, bot#1239)

Developer-host setup presents **one explicit consent step** authorizing
Bicameral to configure permissions for the Bicameral MCP server only. After
consent, the setup controller grants the broadest **host-native,
Bicameral-server-scoped** permission the host supports, so declared Bicameral
tool calls do not prompt individually in a fresh session:

- Claude Code: add the server-wide allow rule `mcp__bicameral` to the user
  `permissions.allow` list (the host form that matches every tool served by the
  registered `bicameral` server).
- Codex: set `default_tools_approval_mode = "approve"` under
  `[mcp_servers.bicameral]` only.

The grant must not broaden shell/Bash, sandbox bypass, generic network
authority, unrelated MCP servers, `bypassPermissions`/`defaultMode`, global
`approval_policy`/`sandbox_mode`, or managed organization-wide policy. Existing
stricter policy wins: a `deny` or `ask` rule for the Bicameral server, a
per-tool Codex `approval_mode` override, or an operator-set non-`approve`
server mode is preserved and nothing is added.

If a server-scoped grant is unavailable on the installed host generation, the
controller derives the read-only/context-tool allowlist deterministically from
the declared Bicameral capability/tool registry (tools whose declared daemon
action class is `read_only`), never from a handwritten list. Tools outside that
set stay host-prompted; when no classified registry is available the grant is
reported as `unsupported` and every tool stays host-prompted.

Host permission and Bicameral action authority are separate layers. The grant
does not bypass daemon-side ToolRequest authorization, Product authority,
confirmation, promotion/signoff, correction, or governance rules, and it never
produces `active_verified`.

The controller records exactly which rules it added (and any prior Codex
value) in `$HOME/.local/share/bicameral/mcp-host-permissions-v1.json`.
Repair/re-setup reconciles only that Bicameral-owned grant and is a no-op on
already-correct state. Uninstall removes only the grant Bicameral added; a grant
the operator changed since setup is preserved and reported. Setup never changes
shell approval, sandbox mode, writable roots, or network access.

Customer-data locality, egress, processing, retention, and exposure are
governed separately and are neither defined nor widened by this permission.

MCP prompts are discovered through MCP. Repo-local skills remain owned by the
repository/team and are not copied, linked, installed, or allowlisted by this
flow.

### 5. Install the managed pre-work hook

The controller runs the exact packaged command:

```sh
bicameral-mcp adapters install --host <claude|codex> --consent --json
```

It then runs packaged status and requires:

- `installed_enabled`;
- exactly one managed entry;
- `package_matches: true`;
- the expected package version, absolute executable, managed command, and
  runner provenance.

The package adapter, not the daemon, owns writes to:

- Claude: `~/.claude/settings.json`
- Codex: `$CODEX_HOME/hooks.json`, normally `~/.codex/hooks.json`

Writes are locked, backed up, atomic, schema-validated, and preserve unrelated
configuration. The adapter must support disable, update/re-enable, and
uninstall with the same preservation guarantee.

### 6. Complete host-owned trust

Setup opens or instructs the normal host flow:

- the operator accepts workspace trust where required;
- Codex displays the newly discovered hook and the operator reviews and trusts
  that exact definition;
- Claude/Codex authentication remains host-owned;
- no `--dangerously-bypass-hook-trust`, auto-response, seeded home, or managed
  enterprise policy is used.

The daemon may observe sanitized status, but it may not write the host's trust
store or convert an installer confirmation into host trust.

### 7. Start a fresh session and verify activation

The operator starts a new normal host session. The package-owned
`SessionStart` adapter writes a durable, sanitized product receipt containing:

- host id and exact version;
- MCP package version and descriptor digest;
- managed entry and hook-document digests;
- session-boundary source;
- correlation id;
- daemon protocol/capability result;
- preflight outcome and timestamp;
- no prompt text, transcript, environment, credential, or raw PTY bytes.

`bicameral-mcp adapters verify-host --host <host> --receipt <path> --json`
validates the receipt and managed configuration. The daemon reduces the result
to `active_verified` only after the receipt verifies and its correlation has not
expired or already been consumed.

Process sampling and transient prompt matching may assist diagnostics but cannot
substitute for the durable product receipt.

## Readiness Model

The daemon persists one readiness record per
`(operator, capability_kind, subject_id, artifact_generation)`. For v0.1.14:

- `capability_kind` is exactly `mcp`;
- `subject_id` is the developer-host id;
- `artifact_generation` is the MCP release-descriptor digest;
- `product_id`, bridge session identity, dashboard session identity, pairing
  request identity, and provider credentials are forbidden.

The read-only `DaemonCapabilityReadinessV1` projection contains:

```text
schema_version
capability_kind
subject_id
artifact_generation
state
reason_codes[]
observed_at
last_verified_at?
next_action_code?
safe_display_argv?
```

`next_action_code` is an allowlisted presentation hint. In v0.1.14 the
dashboard may render only the stable local command:

```text
bicameral setup mcp
```

`safe_display_argv` must contain no secret, one-time challenge, bridge/session
token, Product identifier, local path, executable choice, or caller-supplied
shell fragment. Rendering or copying it is not a setup transition.

| State | Meaning |
| --- | --- |
| `not_installed` | No compatible package/registration/hook is established. |
| `prerequisite_missing` | A declared host, `uv`, Python runtime, credential, or compatible release binding is unavailable. |
| `installed_pending_host_action` | Package and managed configuration verify, but workspace trust, hook trust, restart, or fresh-session activation remains. |
| `active_verified` | A fresh, unexpired durable activation receipt matches the installed package and host configuration. |
| `repair_required` | Drift, duplicate entries, changed package/config, failed daemon handshake, or stale receipt invalidates readiness. |

Reason codes and observations are attributes, not additional lifecycle states.
`installed_enabled` is configuration status, not activation proof.

The projection also carries the optional `host_permission` attribute
(`state`: `configured`, `missing`, `drifted`, or `unsupported`; `mechanism`:
`server_scoped` or `registry_fallback`) with reason codes
`host_permission_missing`, `host_permission_drifted`, and
`host_permission_unsupported`. It distinguishes *permission configured* from
*permission missing/drifted* from *authentic host activation*. It never changes
`state`: a configured grant cannot manufacture `active_verified`, and only a
fresh-session activation receipt (bicameral-mcp#851 terminal evidence) does.

### Orthogonal pairing and Product state

The readiness reducer never reads dashboard pairing or Product state. The
projection route applies the existing bridge guard only to authorize a remote
browser read:

```text
unpaired + active_verified -> daemon remains active_verified; browser cannot read
paired + not_installed     -> browser renders terminal guidance
paired + active_verified   -> browser renders ready
revoked + active_verified  -> daemon remains active_verified; browser cannot read
```

After the gate renders, `Continue to Products` is navigation to Operator Home.
It is not a readiness transition and does not create, join, select, register,
bind, or open a Product.

### Dashboard authority

The dashboard may:

- read the daemon projection while its bridge session is authorized;
- render state, reason codes, and safe terminal guidance;
- copy the stable command locally in the browser;
- navigate to Product discovery after an explicit user action.

The dashboard must not:

- invoke setup, validation, repair, or uninstall;
- write readiness or host configuration;
- submit a terminal/setup receipt;
- choose a developer host or reasoning provider;
- treat pairing, copied-command state, or Product navigation as readiness.

## System Permission Contract

### Allowed after explicit setup consent

- Download exact public artifacts over HTTPS.
- Create per-user directories under `$HOME/.local/share/bicameral` and
  `$HOME/.local/bin`.
- Invoke `uv`, `claude`, `codex`, and `bicameral-mcp` directly with fixed
  list-form arguments.
- Mutate only the selected host's user-scoped MCP and hook configuration
  through the official host CLI or packaged adapter.
- After the explicit host-permission consent, add only the
  Bicameral-server-scoped permission entry (Claude Code user
  `permissions.allow` rule `mcp__bicameral`; Codex
  `[mcp_servers.bicameral] default_tools_approval_mode`) with a bounded backup
  and atomic replace.
- Create a bounded backup beside a host config before an atomic replacement.
- Start the local daemon and open the loopback dashboard.

### Always requires the host's own user interaction

- Provider authentication.
- Workspace/project trust.
- New or changed hook trust.
- Per-tool approval when the host policy asks.

### Forbidden

- `sudo`, `su`, root-owned paths, `/etc`, `/usr/local`, or machine-wide policy.
- Global `pip`, `pip install --user`, editable/source-checkout installation, or
  dependency resolution from an undeclared index.
- Shell wrappers or interpolated shell strings.
- Reading, storing, logging, or forwarding Anthropic/OpenAI credentials.
- Accepting a setup request, executable choice, provider choice, or validation
  trigger from the hosted dashboard.
- Binding readiness to a bridge session, pairing request, or Product.
- Broadly allowing Bash, shell, write, network, unrelated MCP servers, or a
  host-wide approval bypass; only the Bicameral MCP server may be granted.
- Trust-bypass flags or direct execution of the managed hook.
- Replacing malformed host configuration with an empty document.
- Claiming `active_verified` from config presence, process sampling, prompt
  matching, or another host's receipt.

## Failure and Recovery

- Every step is idempotent and independently reportable.
- Failure after package installation but before host registration leaves the
  package installed and readiness pending; retry starts from verified package
  status.
- Failure after one host succeeds does not roll back or imply success for the
  other host.
- A changed package or managed hook invalidates the prior activation receipt
  and requires host review plus a fresh session.
- Uninstall first removes the Bicameral-owned host permission grant, then the
  managed hook, then the host MCP registration, then the managed `uv`
  environment. It preserves unrelated configuration, operator-modified
  permission entries, host authentication, and session history.
- If rollback cannot prove preservation, setup stops at `repair_required` and
  points to the backup; it does not guess.

## Acceptance Criteria

- A clean macOS ARM64, macOS x86_64, Linux ARM64, and Linux x86_64 user can
  install without administrator privileges.
- A manifest without an MCP binding cannot start MCP setup.
- The installer refuses PyPI `0.16.4` and any other unbound package.
- Wheel, version, descriptor, protocol, and executable provenance all match.
- Claude and Codex registration is performed and inspected via their official
  CLIs.
- No repo-local skill is installed; the only permission added is the
  consented Bicameral-server-scoped grant, and shell, sandbox, network,
  unrelated MCP, and host-wide approval settings are byte-for-byte unchanged.
- Consented Claude Code and Codex setup stop per-call prompts for declared
  Bicameral tools in a fresh session; repair is idempotent; uninstall removes
  only the Bicameral-owned grant; readiness reports the grant honestly and
  never manufactures `active_verified`.
- Managed-hook install/update/disable/uninstall preserves unrelated config.
- Codex hook trust and all workspace trust are completed in the host UI.
- A durable product receipt, not process sampling, is required for
  `active_verified`.
- Secrets, prompt text, transcript content, PTY content, and raw environment do
  not enter logs or receipts.
- Removing or changing any exact binding returns readiness to
  `repair_required`.
- MCP readiness exists and survives independently of browser pairing and
  Product selection.
- A paired dashboard can render the readiness projection and safe terminal
  command but exposes no setup mutation route.
- Pairing, unpairing, revocation, refresh, copied-command state, and
  `Continue to Products` do not change readiness.
- v0.1.14 emits no reasoning-provider readiness record and runs no LLM
  discovery or validation.
- The topology journey runs independently for Claude and Codex and proves
  package install, registration, managed hook, host trust, fresh-session hook
  firing, daemon handshake, receipt verification, lifecycle preservation, and
  teardown.

## Journey Boundary Review

### Walkthrough

```text
Operator
  -> public Bot bootstrap
  -> verified per-user daemon
  -> consented exact MCP wheel install
  -> official host MCP registration
  -> packaged managed-hook install
  -> host-owned trust and permission review
  -> fresh host session
  -> durable activation receipt
  -> daemon readiness: active_verified
  -> optional paired-dashboard read projection
  -> explicit navigation to Products
```

### Commit points

1. Bot binary is copied into the user install directory.
2. Exact MCP wheel is installed in the managed `uv` environment.
3. Host MCP registration is persisted by the host.
4. Managed hook configuration is atomically persisted by the package adapter.
5. Host trust is persisted by the host.
6. Activation receipt is persisted by the package adapter and consumed by the
   daemon readiness reducer.

These facts are independently observable and may disagree during partial
failure. No earlier fact implies a later one.

Dashboard pairing and Product navigation are independently observable but are
not commit points in this lifecycle. Pairing gates only the remote read.
Navigation changes only the browser route. Neither changes daemon readiness or
Product state.

### Correct retry and forbidden recovery

- Retrying the local CLI setup reconciles the exact package, host, hook,
  receipt, and daemon state idempotently.
- Refreshing or re-pairing the dashboard only re-reads the projection.
- The dashboard must never repair a missing or stale receipt by posting setup
  intent, replaying a command, or manufacturing `active_verified`.
- Product creation or selection must never be used as a recovery mechanism for
  readiness.

### Required catalog refinement

Factory must refine the existing partial `developer_host_mcp_v1` journey,
preserving its exact `developer-host-environment-v1` +
`local-product-environment-v1` set, with required checkpoints for:

- verified per-user daemon artifact;
- exact compatible MCP package;
- host-native MCP registration;
- managed-hook lifecycle preservation;
- host-owned trust;
- fresh-session durable activation receipt;
- daemon `active_verified`;
- read-only paired-dashboard projection with no setup action route;
- pairing and Product-independence negative controls;
- complete uninstall/teardown.

Until that partial journey gains these executable checkpoints and passes them,
this specification is design-ready but not topology-ready.

## Confirmed Permission Boundary

Normal setup remains strictly per-user and non-elevated. Any future enterprise
or machine-wide deployment requires a separate contract and journey. The
responsible human confirmed this boundary on 2026-07-28.

## Confirmed Readiness Authority Boundary

The responsible human selected this boundary in the 2026-07-28 design session
and approved it durably for exact PR head
`ae7fb94276cf1e9fb15ab83cbbcfbdaeb342a764` in
[PR #950 comment 5106871327](https://github.com/BicameralAI/bicameral-bot/pull/950#issuecomment-5106871327):

```text
MCP and future reasoning readiness are daemon-owned operator-local state,
completely independent of dashboard pairing and Product context.
The dashboard may suggest terminal commands and render readiness, but it has no
direct setup control.
```

This confirmation resolves the Journey Boundary Review only. It is not Product
acceptance, merge approval, release assignment, topology evidence, or
deployment authorization.

## ADR And Existing-Spec Implications

- `docs/specs/hosted-bridge-user-level-pairing.md` is reaffirmed: pairing is
  user-level access to projections and governed requests, not Product
  selection. This proposal further constrains the bridge by exposing readiness
  as read-only.
- ADR-0023 is reaffirmed and should name capability readiness as
  operator-scoped daemon state that exists with zero registered Products.
- `docs/specs/mandatory-reasoning-candidate-projection-runtime-architecture.md`
  is deferred for v0.2.0 amendment. Its current “dashboard or CLI” provider
  configuration trigger must not be used to grant the dashboard direct setup
  authority. The accepted future shape is terminal configuration plus a
  daemon-owned read projection.
- No new Product lifecycle state or canonical Product event is introduced.
- No new ADR is required if the two existing contracts are amended before
  implementation. A new ADR would duplicate the hosted/local and operator-home
  authority decisions already owned by those documents.

## Implementation And PR Sequencing

1. Publish and obtain responsible-human review of this feature spec, runtime
   companion, and canonical contract.
2. Rebase the operator-scoped daemon work in bot PR #945 onto the accepted
   contract and implement only the daemon-owned MCP readiness reducer,
   persistence, read projection, and terminal command boundary assigned to that
   slice.
3. Rebase the v0.1.14 Product-creation implementation in bot PR #934 onto a
   `dev` base containing the accepted spec and the required operator-daemon
   seam. Reconcile its pairing/setup screens so pairing stops at the read-only
   local-runtime gate and Product entry requires an explicit navigation action.
4. Re-run #934's runtime contract, dashboard, factory-attestation, Release
   Unit, and topology checks. Because rebasing changes the exact head/base
   tuple, any prior Atlas/release assignment must be replaced; it must not be
   reused.
5. Keep reasoning-provider discovery, configuration, validation, narration,
   and readiness implementation out of #934, #945, and v0.1.14. Slice those
   against the shared envelope for v0.2.0.

The spec branch is an architecture dependency, not an implementation branch.
PR #934 should ultimately target `dev`, not retain the spec branch as its
permanent GitHub base.

## Request Coverage

| Request | Contract location |
| --- | --- |
| MCP and LLM readiness are daemon state | `Decision`, `Readiness Model`, runtime companion |
| Readiness is independent of dashboard pairing | `Orthogonal pairing and Product state` |
| Dashboard suggests commands but has no direct control | `Dashboard authority` |
| Pairing does not enter a Product | `Readiness Model`, `Implementation And PR Sequencing` |
| MCP ships with v0.1.14 | `Release Sequencing: v0.1.14` |
| Current reasoning/LLM work remains v0.2.0 | `Release Sequencing: v0.2.0` |
| v0.1.14 PR rebases onto the accepted contract | `Implementation And PR Sequencing` |
