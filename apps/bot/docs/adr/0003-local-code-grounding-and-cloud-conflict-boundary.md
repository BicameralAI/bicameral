# ADR-0003: Local Code Grounding and Cloud Conflict Boundary

**Date:** 2026-05-27  
**Status:** proposed  
**Level:** L1  
**Carries forward:** `bicameral-daemon` ADR-0003 plus the original `bicameral-bot` local grounding ADR

## Problem

The open-core split must not imply that free/public Bicameral is ungrounded. If the local bot only captures notes, it fails the product thesis.

At the same time, full cross-branch, cross-repo, organization-scale code graph and conflict analysis is expensive and belongs in `bicameral-cloud`, not in every local bot install.

The cloud boundary also needs a product distinction between heavyweight hosted
services and lightweight hosted amenities. Paid hosted services may require
large indexes, provider/API spend, or organization-scale compute. Hosted
amenities are small convenience services that reduce setup friction for the
local-first product and should be generally available to nonpaying users.

## Decision

`bicameral-bot` includes local code grounding for the current repo/worktree. In
the separate Code Evidence/Compliance lifecycle, it may inspect files, diffs,
paths, symbols, commits, and PR context to produce `BindingEvidence`,
`BindingHint`, and local `ComplianceVerdict` proposals.

This code-grounding contract is not the SMR alpha value proposition. SMR alpha
recalls product-approved specifications from a controlled corpus and remains
available without CodeGraph; it does not claim implementation compliance,
currentness, correctness, absence, or all-clear. ADR-0028 owns that boundary.

`bicameral-cloud` owns hosted services for code graph and conflict advisory infrastructure:

- cross-branch and cross-repo tree-sitter indexes;
- shared code graph caches across workspace members;
- conflict gate / conflict advisory service;
- blast-radius and dependency analysis across repositories;
- expensive or historical grounding optimization.

Cloud returns evidence, hints, advisories, and `GovernanceResult` proposals. It does not become canonical authority; bot governance and the selected event store substrate still decide what materializes.

`bicameral-cloud` may also host lightweight amenities, such as encrypted
rendezvous relay for team settings pairing. Amenities are not the paid product
boundary and must not become canonical context authority. For every hosted
amenity, Bicameral should preserve a local-only sharing alternative where
feasible, such as invite files, QR/copy-paste bundles, LAN pairing, or another
user-controlled transfer.

### Survey-informed rationale

Public developer surveys make local grounding a baseline requirement, not a paid-only enhancement. Stack Overflow's 2024 AI survey reports broad AI use/planned use while also identifying lack of trust in AI output and lack of codebase context as top team-level adoption blockers. DORA 2024 reports that AI can increase individual productivity, flow, and satisfaction while negatively affecting delivery stability and throughput. See [Public Developer Survey Implications](../research/public-developer-survey-implications.md).

Implication: local code evidence is useful for its own governed lifecycle, while
the public alpha can reduce cognitive debt through controlled specification
recall without requiring code inspection. Paid cloud value for code evidence
concentrates in scale and precision: cross-repo, cross-branch, historical,
shared-cache, blast-radius, and expensive conflict intelligence that local
inspection cannot provide.

## Local Bot Owns

- current-worktree grounding: files, line ranges, symbols, imports/callers/callees when a local index exists, current diff/branch context, commit metadata, tests, deploy/release files;
- Code Evidence/Compliance preflight before an agent edits code;
- local compliance proposals such as `reflected`, `partial`, `drifted`, `pending`, or `ungrounded`;
- routing weak evidence to review instead of silently approving it.

## Hosted Services Own

Hosted services may build and cache per-branch/per-repo graphs keyed by `(repo_ref, branch_name, pinned_commit)`, including imports, call-sites, type relationships, deploy surfaces, and cross-repo dependencies.

Hosted services may produce substrate-neutral `GovernanceResult` records:

- `severity: info | warning | blocking`
- `enforcement_capability: ci_block | dashboard_flag | agent_warning | slack_notification | queued_action | none`
- conflict type: decision conflict, implementation conflict, dependency conflict, compliance conflict.

For git-backed workspaces, review-resolved blocking results can materialize as CI failures if the repository has that enforcement boundary. For non-git substrates, the same result must surface through honest capabilities such as dashboard flags or agent warnings.

## Hosted Amenities Own

Hosted amenities reduce setup friction without owning product authority. The
first amenity is encrypted rendezvous relay for team settings pairing:

- short-lived pairing sessions;
- end-to-end encrypted, signed workspace-settings bundles;
- no secrets, OAuth refresh tokens, private code, or canonical Decision Ledger events;
- relay-side deletion after handoff or expiry;
- a local-only alternative for teams that do not want hosted mediation.

The relay may help a teammate receive policy defaults, workspace identity,
integration manifest references, event-substrate instructions, and gateway
profile settings. It does not synchronize accepted Decisions, evidence, or
ledger history. Shared context still requires a replayable event-store substrate
or an explicitly specified future peer-replication adapter.

## Conflict Advisory Service

Cloud may proactively ask: "If this proposed L2 decision were grounded, what code paths would it likely touch, and what approved decisions might conflict?"

Those predictions are advisory until reviewed.

## HITL Boundary

Local and cloud grounding both preserve the separate confidence surfaces from ADR-0004:

- extraction confidence;
- binding confidence;
- compliance confidence.

High binding confidence does not approve product meaning. High extraction confidence does not make a weak code binding safe to block on. Blocking requires governance policy plus review-resolved evidence.

## Non-Goals

Local grounding does not silently approve compliance or block work. Cloud graph intelligence does not replace local event-store authority.

## Consequences

Free/public Bicameral is useful as a developer tool because it can ground in the current workspace. Paid hosted-service value concentrates in scale, precision, history, managed provider/API paths, and cross-branch/cross-repo analysis. Hosted amenities can improve free/local setup without becoming the paid boundary. The same `BindingEvidence`, `ComplianceVerdict`, and `GovernanceResult` vocabulary works for both local and cloud sources.
