# Manual Setup

Manual setup is the baseline team onboarding path when provider-based project
discovery is not yet configured. It uses only local CLI commands, out-of-band
instructions, and the user-configured event store substrate.

This path is intentionally explicit. It shows what provider-based project
discovery should simplify, not what the product should make every teammate do
forever.

## Scope

Manual setup can synchronize workspace settings by human instruction. It does
not synchronize accepted Decisions, evidence, or ledger history by itself.
Shared context still requires the configured event store substrate, such as Git
or a Drive-synced folder.

Manual setup does not use:

- provider-based project discovery (see `docs/specs/project-setup-ui.md`);
- hosted rendezvous relay (legacy, deprecated);
- invite import (legacy, deprecated — see issue #344);
- automatic Google Drive or Git provisioning;
- secret transfer;
- hosted workspace authority.

## Teammate Flow

1. Install Bicameral and register the local daemon if needed.

```bash
bicameral onboard --install-daemon
```

2. Open the relevant repo or workspace.

```bash
cd /path/to/repo
bicameral init
```

3. Receive workspace settings from the EM through Slack, email, docs, or another
out-of-band channel.

Example settings:

```yaml
workspace:
  id: backend-platform
  name: Backend Platform

event_store:
  substrate: drive_folder
  drive_folder_path: /Users/alice/Google Drive/Bicameral/backend-platform

gateway:
  host: 127.0.0.1
  port: 8787

policy:
  manifest: .bicameral/policy.yaml
```

4. Edit `.bicameral/config.yaml` locally to match the shared workspace settings.

5. Configure the selected event store substrate.

For a Drive-synced folder:

- install Google Drive Desktop, rclone, or the team's chosen sync tool;
- join or mount the shared folder;
- wait for the folder to sync locally;
- set `event_store.drive_folder_path` to the local synced path.

For Git:

- confirm repo remote access;
- pull the relevant branch;
- confirm `.bicameral/` event files are present or can be created through the
  configured review/materialization path.

6. Start the daemon API, then open the Cloud-hosted dashboard.

```bash
bicameral gateway start
```

For local/staging UI validation, use the disposable local Cloud process from
`BicameralAI/bicameral-cloud#135`. The daemon does not serve the SPA.

7. Run diagnostics.

```bash
bicameral doctor
```

## Common Failure Cases

- `drive_folder_path` points to a folder that has not synced yet.
- Workspace ID or policy manifest differs from the EM's instructions.
- Teammate has repo access but not Drive folder access, or vice versa.
- Gateway port conflicts with another local process.
- Git remote/branch permissions allow reads but not event materialization.
- The dashboard starts, but shared ledger state is empty because the event store
  substrate is not actually shared.

## Provider Discovery Comparison

Provider-based project discovery (see `docs/specs/project-setup-ui.md`) should
replace the fragile parts of this checklist:

- manually copying workspace IDs and paths;
- editing YAML by hand;
- guessing which fields are safe to share;
- diagnosing schema or path typos.

The legacy InviteBundle/pairing approach (deprecated, issue #344) previously
attempted to simplify this flow via checksummed bundles, but project discovery
through provider connections is now the product direction.
