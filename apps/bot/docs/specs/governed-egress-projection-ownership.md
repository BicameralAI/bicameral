# Governed Egress Projection Ownership

**Date:** 2026-06-25
**Status:** proposed contract
**Level:** L2 cross-repo contract
**Owns:** Bot-owned egress authority, policy, projection generation, receipt,
and reconciliation interpretation.
**Related context:** BicameralAI/bicameral-bot#527,
BicameralAI/bicameral-bot#528, BicameralAI/bicameral-integrations#200,
BicameralAI/bicameral-integrations#204,
`docs/specs/action-authorization-policy.md`.

## Principle

Egress is governed projection from Bicameral authority into an external
execution surface. It is not export, sync, or a target-specific truth copy.

```text
Bicameral decides what is true.
Integrations define how projections fit target tools.
External tools receive governed projections suited to their workflow.
```

## Ownership Split

`bicameral-bot` owns canonical artifact eligibility, `EgressProjection`
generation, `EgressReceipt`, `EgressEligibilityCheck`, `ProjectionPolicy`,
approval requirements, reconciliation interpretation, and promotion of external
observations through governed review paths.

`bicameral-integrations` owns target API documentation references, target auth
scopes and surfaces, `TargetIntegrationDescriptor`, connector profiles,
`ProjectionProfile`, `TargetAdapter`, rendering constraints, and receipt
mapping details.

`bicameral-mcp` may render or invoke bot-owned egress actions through supported
tool requests. It must not own policy, approval, projection generation, target
mutation, receipt interpretation, or canonical state mutation.

## Boundary Rule

```text
Integrations may describe what a target can read, render, or write.
Bot decides whether Bicameral may project anything into that target.
```

`ProjectionProfile` is target shape and capability metadata only. It must not
grant permission, waive approval, decide artifact eligibility, mutate canonical
state, interpret external state as Bicameral authority, or bypass
`ProjectionPolicy`.

`TargetAdapter` executes an approved operation and returns target result data. It
must not rewrite projection content, decide policy, grant authority, or promote
target-side state.

## Universal Flow

```text
Canonical Bicameral artifact
  -> EgressEligibilityCheck
  -> approved ProjectionProfile reference
  -> EgressProjection
  -> ProjectionPolicy check
  -> ApprovalGate when required
  -> TargetAdapter execution
  -> EgressReceipt
  -> ReconciliationObservation
```

Target-specific renderers may differ, but they consume the same bot-owned
projection object and integrations-owned profile metadata.

## Bot-Owned Objects

`EgressEligibilityCheck`
: Evaluates whether a canonical artifact may be projected to a requested target
  and profile. It returns eligibility, approval requirements, warnings, and
  rejection reasons. It does not write externally.

`EgressProjection`
: Bot-authored projection payload bound to a canonical artifact revision,
  target system, profile id, authority label, evidence refs, and policy result.
  It is the only payload target adapters may execute.

`ProjectionPolicy`
: Bot-owned policy that decides whether a projection request may proceed, which
  approval class is required, and which target/profile combinations are allowed
  for the artifact and actor.

`EgressReceipt`
: Audit object recording the target operation, target object/url, payload hash,
  canonical revision, policy result, actor, timestamp, and reconciliation state.

`ReconciliationObservation`
: Observed target-side activity after projection. It may trigger recall,
  contradiction detection, correction candidates, review routing, or drift
  surfacing. It does not directly mutate canonical Bicameral authority.

## Integrations-Owned Objects Consumed By Bot

`TargetIntegrationDescriptor`
: Shared target-boundary descriptor covering API docs, auth scopes, target
  surfaces, ingress capabilities, egress capabilities, connector profiles,
  projection profiles, and target adapters.

`ProjectionProfile`
: Target-specific shape/capability declaration, such as
  `linear.issue.summary.v1`, `linear.issue.work_item.v1`, or
  `linear.comment.status.v1`.

`TargetAdapter`
: Target execution adapter that supports declared operations and returns
  receipt/reconciliation mapping data.

The integrations-owned contract is defined by integrations ADR-0019 and
`PROJECTION_CONTRACT.md` from PR #204.

## Linear Alpha

Linear is the first target profile family, not the egress architecture.

Alpha projection modes:

- Summary Projection: render accepted Bicameral state as a compact Linear issue
  or comment projection.
- Work Item Projection: create a Linear issue from an implementation-ready
  Bicameral artifact after policy and approval.
- Status Projection: publish constrained status updates such as review required
  or accepted decision linked.
- Reconciliation Observation: observe Linear-side changes as external activity,
  not canonical truth.

## External Edits

External edits are first-class observed input, not optional reconciliation
trivia.

```text
External edit
  -> observed external event
  -> screened provider fact or external activity observation
  -> bot-owned interpretation path
  -> optional recall, contradiction, correction, drift, or review candidate
  -> canonical mutation only through an accepted governed path
```

Observation is core. Promotion is governed.

## Forbidden Flows

- Raw `RecallPacket` to automatic external issue.
- Unreviewed contradiction candidate to canonical external truth.
- External issue body to canonical Bicameral state.
- External status to Decision/signoff/compliance state.
- External comment to accepted `SourceEvidence` without governed ingest.
- Sidecar-owned or SDK-owned egress policy for alpha.
- Target-specific truth models.
- Bot-owned target rendering sprawl.
- Full bidirectional sync.

## Implementation Slices

1. Define bot `EgressEligibilityCheck`, `EgressProjection`, `ProjectionPolicy`,
   and `EgressReceipt` schemas.
2. Add policy fixtures proving `ProjectionProfile` and `TargetAdapter` cannot
   grant permission or waive approval.
3. Consume integrations `TargetIntegrationDescriptor` and
   `ProjectionProfile` references by id.
4. Implement Linear summary/work-item/status projection eligibility and preview.
5. Add approved adapter execution and receipt persistence.
6. Add reconciliation observation ingest that never promotes external target
   state directly.

## Acceptance Criteria

- Sidecar/SDK-led egress is abandoned for alpha.
- Egress is governed projection, not export or sync.
- Bot-owned authority objects and integrations-owned target-shape objects are
  separate.
- Linear is modeled as a profile family, not a bespoke architecture.
- Projection profiles and target adapters cannot create authority.
- External edits are observed continuously but promoted only through governed
  bot-owned paths.
