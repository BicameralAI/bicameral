---
id: ARCH-0004
title: Signoff is the GovernedDomain
kind: decision
status: accepted
date: 2026-10-01
owners: [@jinhongkuan]
governs: [signoff, conflict-resolution, approve-permission]
depends_on: [ARCH-0003]
supersedes: []
references: [ARCH-0001, apps/bot/docs/adr/0034-signoff-is-the-governed-domain.md]
---

# ARCH-0004: Signoff is the GovernedDomain

## Context

This record rewrites Bot ADR-0034 (`apps/bot/docs/adr/0034-signoff-is-the-governed-domain.md`) into this repository's format, per ARCH-0001. Its status was checked, not assumed. The ADR was accepted on 2026-09-25 and amended on 2026-09-29, and its final text landed in the Bot repository through a reviewed and approved pull request merged on 2026-10-01. The decision text below is unchanged. It builds on the `GovernedDomain` and `CheckRecord` defined in ARCH-0003.

## Decision

Signoff is the approved state of the `GovernedDomain` that pairs with one `SourceSnapshot`. It is not a state of a separate decision object. `Decision`, `DecisionCandidate`, `Tracked Decision`, and `Decision Ledger` are purged. Do not introduce them again. Do not call this value standing or status. Status is the operational receipt.

A newer snapshot gets its own domain. It does not inherit check records or signoff.

### State

Replay of the event substrate gives each snapshot in the maintenance set one state.

- **Tracked.** A person tracked the `Source`. Capture wrote the snapshot and an empty domain. There is no `Signoff`.
- **Approved.** A signoff event was accepted. This is the only state an agent is given as governing.
- **Rejected.** A reject event was accepted for this snapshot. The domain is not governing.
- **Superseded.** A supersede event names the successor snapshot. The approval event stays in the log. Replay folds the state to superseded.

The person who tracked a source may untrack that source while its snapshot is still tracked. Untrack removes the source from the maintenance set, and the snapshot then has none of these states. A later track places the source in the maintenance set again. The owner rejects a tracked snapshot that is not in a conflict set. Policy does not untrack, reject, or supersede. A member who did not track the source does not untrack it.

Track, untrack, reject, and supersede are their own payloads. Each records `daemon_id`. Track and untrack record the `person_id` of the person who tracked the source. Reject and supersede record the `person_id` of the owner. Supersede also names `successor_snapshot_addr`. Untrack writes no authorization state.

### Conflict

A `CheckRecord` names two domain digests, `conflict`, and the `daemon_id` that ran the check. The daemon is not part of the pair. The same pair is written once. A missing or failed check is not stored as `conflict` false. Use conflict. Do not use contradiction or contradicts.

A conflict set is every source still in the maintenance set tied by a stored check with `conflict` true, including tracked against tracked. `in_conflict` is true when a source is in a conflict set. It qualifies `resolve_conflict`. An untracked source is not in the set. An empty check list is not a conflict. A branch whose bytes differ from `main` does not set the flag. An earlier snapshot of the same source is a previous version, not a conflict.

`approve` and `reject` apply to one source that is not in a conflict set. An approval is accepted only when every other approved domain of another source has a `CheckRecord` against this digest with `conflict` false. Two approved domains are therefore not in conflict. When no other domain is approved, the check list may be empty.

Approving a newer snapshot of a source that already has an approved snapshot supersedes that previous snapshot in the same commit. That pair is a version update.

### Resolution

The owner calls `resolve_conflict` for a conflict set. One substrate commit holds the whole resolution. `approve` and `reject` are refused for a member of that set.

The payload is `members`, plus the owner's `person_id` and the `daemon_id` on the command. Each member has `source_uri`, `snapshot_addr`, and `state` of `approved`, `rejected`, or `superseded`. The actor is not on a member row. The list must be the whole set. A `rejected` member is currently tracked. A `superseded` member is currently approved, and the one `approved` member in that call is its successor. At most one member is `approved`. All `rejected` is valid.

- A tracked snapshot against an approved one: the owner rejects the tracked snapshot, and the approved one stays. Or the owner supersedes the approved snapshot and approves the tracked one in that same commit.
- Two tracked snapshots: the owner approves one and rejects the other. Or the owner rejects both. A third source stays tracked until a later approval.

The person who tracked a snapshot may untrack it while it is still tracked, without the owner. The other snapshot's state stays as it was. Untrack is not a resolution of the conflict.

### Who may approve

Who may approve a snapshot that is not in a conflict set is a product policy, not a hardcoded owner-only act. `approve-permission` is that policy. Its `allow` bit means approve. When `in_conflict` is true, `allow` is false for every role. That cell is not configurable, and it is not permission to `resolve_conflict`.

```text
auto-approve: bool
approve-permission: (role: owner | member, in_conflict: bool) -> bool
```

The alpha default is:

- `auto-approve = true`
- `approve-permission(owner, in_conflict = false) = true`
- `approve-permission(owner, in_conflict = true) = false`
- `approve-permission(member, in_conflict = false) = true`
- `approve-permission(member, in_conflict = true) = false`

The owner calls `resolve_conflict`. A member does not. That act is the owner role. It is not an `approve-permission` cell. Policy approves only when `auto-approve` is true and the source is not in a conflict set. An empty list is stored only when no other domain is approved. The record is provenance for the signoff. It is not a claim that no conflict exists anywhere else.

A daemon skips a pair only when that exact pair of digests is already recorded.

For alpha, the approving daemon may author the record. That is trust gap T1 on factory #937.

Every signoff is cranked by a daemon, including auto-approve. The signoff records that daemon. The authorization contract also records whether the actor is `manual` or `policy`. `policy` means this daemon cranked an auto-approve, and it has no role. `manual` means a person took the act and this daemon cranked the write. A `manual` signoff also records that person and their role, `owner` or `member`.

Only the Product Owner may change `auto-approve` or `approve-permission`. A change applies to later signoffs. It does not clear or rewrite a signoff already stored, and it does not rewrite that signoff's check record. This update rule is part of the signoff entity, not a factory #901 handoff. A handoff row appears only if a specific surface must hand a policy command to a specific daemon.

## Consequences

Considered options:

1. Sign off the `GovernedDomain`. Chosen.
2. Keep signoff on a `Decision` that cites the snapshot.
3. Sign off the `SourceSnapshot` independently of its domain.
4. Let the owner approve while `in_conflict` is true and leave both domains approved. The 2026-09-25 default did this. The 2026-09-28 amendment rejects it. A conflict is resolved before the approval, so two approved domains are not in conflict. Encoding `resolve_conflict` as `approve-permission(owner, in_conflict = true) = true` was considered on 2026-09-29 and rejected. That bit is approve, and it stays false. The owner resolves the set by `resolve_conflict`.
5. Let the person who tracked a snapshot reject it. Rejected on 2026-09-28. The tracker places an existing source, such as a Jira ticket written by someone else, into the maintenance set. The reporter and the ingest adapter have no reject authority. Reject stays with the owner. The tracker may untrack the source while it is still tracked.
6. Call the snapshot value standing, or call it status. Rejected on 2026-09-29. The existing field is `signoff_state`. Status is the operational receipt. The value is state.
