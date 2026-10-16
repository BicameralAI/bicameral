# ADR-0022: Product Owner Authority Boundary

**Date:** 2026-06-07  
**Status:** proposed  
**Level:** L1  
**Related:** ADR-0004, ADR-0005, ADR-0007, ADR-0014, ADR-0020

## Problem

Bicameral already separates evidence, candidates, review commands, governance
policy, and event-store materialization. It does not yet state the alpha product
authority boundary for who reviews product-scoped Decision meaning. Without that
boundary, MCP and review surfaces can drift toward per-candidate reviewer lists,
domain-role routing fields, or customer-org-chart modeling before the product
has proved its core value: reducing EM cognitive debt.

> **Reconciliation (issue #765).** For the domain-discovered Product auto-join
> flow, a Product **creator** is materialized atomically as both `Policy Owner`
> and `Product Owner` (never a partial grant), and same-domain auto-join grants
> `Product Member` only. `Workspace Member` below reads as `Product Member`
> ("Workspace" is retired as a membership term; see `CONTEXT.md`). The
> authoritative end-to-end contract for create/join/publish authority is
> [`docs/specs/domain-product-onboarding-contract.md`](../specs/domain-product-onboarding-contract.md).

## Decision

For alpha, a `Product` has one `Product Owner` and zero or more
`Workspace Member`s. The `Product Owner` is the default accountable reviewer for
product-scoped Decision meaning and signoff. `Workspace Member`s may contribute
evidence, request review, perform allowed member actions, and help resolve
grounding or compliance evidence according to governance policy.

Candidate review and signoff remain separate authority transitions. Accepting a
candidate promotes it into `Decision(proposed)` and may be delegated to a
Workspace Member only when governance policy grants the relevant candidate-review
capability. Approving signoff records Product Owner accountability for the
Decision's meaning and requires the Product Owner or an explicit Product Owner
delegate. Neither transition implies compliance, implementation correctness, or
external egress approval.

External integrations do not change this boundary. Source-adapter ingest may
capture evidence that a user asked to supersede or approve something, but it is
not approval. An authenticated remote review action may submit a candidate,
signoff, or decision command only after the bot verifies the external callback,
binds it to a Bicameral actor, checks the actor's Product role/capability, and
accepts the command through governance policy.

Review routing should be derived from the current Product boundary, not from
per-candidate fields such as `required_reviewers`, `assigned_reviewers`,
`domain_role`, or `suggested_review_context`. Existing protocol or implementation
fields may remain as compatibility plumbing, but alpha product UX should not
present them as the routing model.

When a candidate or review command needs human attention, surfaces should use
existing `reason` fields to explain the source-grounded evidence gap, decision
rationale, or command consequence. `reason` reduces context reconstruction; it
does not assign a specialist reviewer or assert unverified compliance/legal
conclusions.

## Consequences

Alpha keeps authority legible: Decision meaning rolls up to the Product Owner,
members contribute evidence, and durable authority still requires governance
policy plus event-store materialization. Bicameral does not own the customer's
org chart in OSS alpha.

Hosted or premium deployments may later support multiple Products, each with
its own owner/member set, and may map ownership through codeowners, Jira
components, Slack groups, compliance workflows, or other administrative
metadata. That is a product-line expansion, not alpha infrastructure.
