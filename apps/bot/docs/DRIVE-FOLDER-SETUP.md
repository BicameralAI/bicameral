# Drive-Folder Substrate Setup

## Scope

This document covers the existing **local synced-folder** Drive substrate, implemented by `DriveFolderAdapter`.

It does **not** describe the provider-backed Google Drive REST path. The provider adapter core introduced by Bot #1086 / PR #1088 is documented separately in [`DRIVE-API-EVENT-STORE.md`](DRIVE-API-EVENT-STORE.md). That path preserves the same event representation but requires provider-confirmed readback and additional credential/resource/composition work before production activation.

Do not treat these two modes as interchangeable evidence boundaries:

- `DriveFolderAdapter` proves local filesystem materialization and relies on an external sync agent.
- `DriveApiAdapter` is designed to prove provider materialization through exact remote readback.

## Overview

The Drive-folder substrate lets multiple team members share a single
Decision Ledger through Google Drive (or any folder-sync tool). Each
Bicameral daemon reads and writes events as individual JSON files in a
shared local folder. An external sync agent (Google Drive Desktop, rclone,
Syncthing, etc.) propagates changes between machines.

This substrate satisfies the baseline Decision Ledger contract: durable
append, deterministic replay, audit, and health/freshness reporting. It
does **not** provide PR review, branch isolation, CI gating, or merge
conflict resolution — those are Git-substrate capabilities.

## Quick Start

### 1. Create a shared folder

Create a folder in Google Drive (or another sync service) and share it
with your team:

```
Google Drive > Shared drives > YourTeam > bicameral-events/
```

### 2. Sync the folder locally

Use Google Drive Desktop (or rclone/Syncthing) to sync the folder to a
local path on each team member's machine. For example:

```
~/Google Drive/Shared drives/YourTeam/bicameral-events/
```

### 3. Configure Bicameral

In your workspace's `.bicameral/config.yaml`, set:

```yaml
event_store:
  substrate: drive_folder
  drive_folder_path: ~/Google Drive/Shared drives/YourTeam/bicameral-events
```

Each team member configures the same `drive_folder_path` (their local
mount point for the same shared folder).

### 4. Start the daemon

```bash
bicameral gateway start
# or
bicameral service start
```

The daemon will construct a `DriveFolderAdapter` from your config and
begin appending/replaying events from the shared folder.

### 5. Verify

```bash
bicameral doctor
```

The doctor command will show the configured substrate kind, capabilities,
and any freshness/health warnings.

You can also query the gateway:

```bash
curl http://127.0.0.1:37373/api/v1/status | jq .substrate
```

This returns the substrate kind, capabilities, reachability, freshness
state, and any advisory warnings.

## Freshness and Warnings

The Drive-folder adapter tracks freshness via a `.bicameral-sync-marker`
file in the shared folder. The sync agent (or a periodic cron job) should
call `mark_synced()` or write the current RFC 3339 timestamp to this file.

| State   | Meaning                                          |
|---------|--------------------------------------------------|
| Fresh   | Sync marker is recent (within 15 min threshold)  |
| Stale   | Sync marker exists but is older than threshold   |
| Unknown | Folder exists but no sync marker present         |
| Offline | Folder path does not exist                       |

Warnings surface through `/api/v1/status` and the dashboard. They are
**advisory** — the Drive substrate does not claim enforcement authority.

## Capability Boundary

Drive-folder substrates advertise only `DurableReplay`. They do **not**
advertise:

- `BranchLocalProposals` — no branch isolation.
- `PrNativeReview` — no PR review mechanism.
- `NativeMergeConflictResolution` — no branch/merge model.
- `CiMergeGateEnforcement` — no CI pipeline.

Governance enforcement on Drive substrates surfaces as dashboard warnings,
queued actions, or paused review states — never as CI blocking.

## Configuration Reference

```yaml
event_store:
  # Required: which substrate to use.
  substrate: drive_folder

  # Required for drive_folder: path to the synced local folder.
  drive_folder_path: /path/to/shared/folder

  # Not applicable for drive_folder:
  # git_path: ...
  # ci_merge_gate: ...
```

This configuration selects the **local synced-folder** adapter only. It does not configure the provider-backed Drive API adapter or a Google OAuth credential.

## Troubleshooting

- **"Offline" freshness**: The folder path does not exist on this machine.
  Verify your sync agent is running and the mount point is correct.
- **"Stale" freshness**: The sync marker is older than 15 minutes. Check
  that your sync agent is actively syncing. You can manually mark fresh:
  `date -u +%Y-%m-%dT%H:%M:%SZ > /path/to/folder/.bicameral-sync-marker`
- **"Unknown" freshness**: No sync marker exists. Write one manually or
  configure your sync agent to periodically update it.
- **Write conflict warnings**: Another team member wrote an event with the
  same UUID. The local synced-folder adapter retains its documented local
  conflict behavior. Do not infer the stronger provider idempotency/readback
  semantics of `DriveApiAdapter` from this local mode.
