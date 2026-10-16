# Substrate Migration Notes

## Event-store layout changes

### Git substrate: YAML event-first layout (from PR #96)

The Git-backed event store now writes each accepted event as an individual
YAML file under the events directory:

```text
<root>/events/<timestamp>-<event_type>-<uuid-prefix>.yaml
```

**Backward compatibility:** If no YAML event files exist but a legacy
`events.jsonl` file is present, replay reads from the JSONL file. New
appends always use the YAML layout. No manual migration is required —
existing `events.jsonl` data continues to replay correctly.

**Recommendation:** To fully migrate, replay your existing JSONL file into
YAML files by running `bicameral doctor` (which will surface a health
warning about legacy layout) and then re-ingesting or manually converting
existing events. The legacy JSONL file can be retained as a backup.

### Drive/basic-store substrate: no layout change

Drive-folder substrates continue to store events as individual JSON files
named by UUID. No migration is needed for existing Drive-backed workspaces.

## Capability distinction (from issue #80)

### What changed

The status, doctor, and gateway surfaces now explicitly report which
substrate capabilities are available. This is a **documentation and UX
change only** — no behavioral change to event storage, replay, or
governance policy.

### Key points

1. **Drive/basic-store substrates** satisfy the baseline Decision Ledger
   contract: durable append, deterministic replay, audit, and
   health/freshness. They do **not** advertise PR review, branch proposals,
   merge conflict resolution, or CI/merge-gate enforcement.

2. **Git substrates** extend the baseline with PR-resolution capabilities
   (branch-local proposals, PR-native review, native merge conflict
   resolution). CI/merge-gate enforcement is only advertised when
   `event_store.ci_merge_gate: true` is set in the workspace config.

3. **No substrate claims capabilities it does not have.** Dashboard,
   gateway, and doctor surfaces will not show CI/merge enforcement messages
   for Drive-backed workspaces.

### Configuration change

A new optional field in `.bicameral/config.yaml`:

```yaml
event_store:
  substrate: git
  ci_merge_gate: true  # set when branch protection with required checks is enabled
```

This field defaults to `false`. It controls whether the Git adapter reports
`CiMergeGateEnforcement` as available. It does not affect the baseline
ledger contract.

### Impact on existing workspaces

- **No breaking changes.** Existing config files continue to work without
  modification.
- The `ci_merge_gate` field is optional and defaults to `false`, preserving
  existing behavior.
- Governance enforcement on Drive substrates continues to surface through
  dashboard warnings, queued actions, or paused review states.
