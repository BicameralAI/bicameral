# Hosted Project Discovery Contract

Date: 2026-07-08
Status: draft
Level: L1 repo architecture
Owns: The Cloud/Supabase project discovery contract — Cloud project record, project membership, legacy create/join request, selected project, and daemon binding readiness semantics; the API response shape the SPA consumes to render a project list; and the authority split between Supabase, the Bicameral Cloud API, and the local daemon.

> **Alpha onboarding refinement (cloud#126):** the primary same-domain Product
> onboarding path is per-Product `ProductJoinTicket` issuance followed by
> daemon-authoritative membership. The owner-reviewed
> `/api/v1/teams/{team_id}/join-requests` route is legacy compatibility only and
> must not be surfaced by the new Operator Home. Ticket issuance creates no
> durable pending or approval state. See
> `product-join-ticket-contract.md`.
Related context: ADR-0005, ADR-0006, ADR-0007, ADR-0008; `docs/specs/design-partner-signup.md`, `docs/specs/design-partner-signup-runtime-architecture.md`. Source issue: BicameralAI/bicameral-cloud#64. Runtime contract anchor: BicameralAI/bicameral-bot#716 (hosted vs dev runtime architecture).

## Reading Guide

Main invariant: the hosted dashboard may **discover and render** projects, but the **local daemon remains authoritative for project registration and workspace binding** (ADR-0005). Cloud project records are coordination/projection records, never canonical local workspace state.

Governance gate: `bic:spec-governance-gate` advisory/no-op; Qor integration disabled pending factory policy revision.

Scope note: this is a documentation/contract slice only. **No runtime implementation** is included per the source issue's non-goals. Where production Supabase schema does not exist, this contract defines a **local alpha adapter** rather than blocking (see "Persistence and source of truth").

## Problem

After design-partner signup (`docs/specs/design-partner-signup.md`), an authenticated invited user reaches `/dashboard` but there is no defined contract for how the SPA lists the projects a user can work on, how a project is created or joined, how a selected project resolves to a paired daemon, or how "ready to use" is computed. The runtime already exposes minimal **team-visible workspace discovery** (`GET /api/v1/teams/{team_id}/workspaces`, ADR-0007), but issue #64 asks for a first-class **project** discovery contract the SPA can consume, with explicit authority and persistence semantics.

This contract also fixes a terminology gap: the running code says "workspace" (ADR-0007, `models/discovery.py`) while the product/UI language says "project". This document treats **project** as the SPA-facing name for the daemon-exposed workspace surface and records the naming reconciliation as a domain-entity candidate.

## Goals And Non-Goals

Goals:

- Define the **Cloud project record** the SPA renders (schema fields).
- Define **project membership** and name its source of truth (or mark the adapter assumption).
- Define the **create project** and **join project** request contracts.
- Define **selected project** semantics for the SPA/session.
- Define **daemon binding readiness** semantics (states and transitions).
- Define the **API routes** and the **project list response shape** the SPA consumes.
- Define the **authority matrix** across Browser/SPA, Supabase, Cloud API, and local daemon.
- Define **failure modes** for each route.
- State whether the relevant **Supabase tables already exist** or a **local alpha adapter** is used.

Non-goals:

- No runtime implementation, no new endpoints wired, no migrations applied.
- No per-user project filtering inside a team (ADR-0007: all team members see the same list in alpha).
- No moving local workspace authority into Cloud (ADR-0005).
- No persisted projection cache of daemon state beyond operational coordination records (ADR-0007).
- No provider-secret, source-binding, signoff, compliance, or Decision Ledger data in Cloud (ADR-0006).
- No per-email invite/approval workflow (design-partner specs defer this).

## Domain Model

### Cloud project record

A non-canonical coordination/projection record describing a daemon-exposed project so the SPA can render a project list and route intent to the correct daemon. It is **not** the source of truth for the project's existence, files, or bindings; the local daemon is (ADR-0005).

| Field | Type | Notes |
| --- | --- | --- |
| `project_id` | string | Stable cloud-visible id for the daemon-exposed project/workspace. Opaque; not a filesystem path. |
| `team_id` | string | Owning cloud team (tenancy boundary, ADR-0006). |
| `display_name` | string | Human label exposed by the daemon; no path/remote leakage. |
| `pairing_status` | enum `unpaired` \| `unavailable` \| `paired` | Mirrors dashboard pairing state (CONTEXT.md). |
| `binding_readiness` | enum (see below) | Coarse readiness; not a compliance/signoff state. |
| `daemon_compatible` | boolean | From the compatibility envelope (ADR-0007). |
| `created_at` | ISO-8601 | Coordination record creation time (adapter/DB), not project creation authority. |

Forbidden fields (must never appear, reusing `FORBIDDEN_DISCOVERY_FIELDS` from `models/discovery.py`): file/local/filesystem paths, branch names, repository remotes, provider names/secrets, `SourceEvidence` bodies, Decision Ledger payloads, actor/member lists, issue titles, signoff/compliance/authority fields.

### Project membership

The set of users who can **discover** a project through the hosted dashboard. In alpha this is **derived from cloud team membership**, not stored per-project: per ADR-0007 every member of a cloud team sees the same daemon-exposed project list for that team. There is no per-user, per-project membership row in alpha.

| Field | Type | Notes |
| --- | --- | --- |
| `user_id` | string | Supabase-authenticated user (canonical identity). |
| `team_id` | string | Cloud team the user belongs to. |
| `role` | enum `team_owner` \| `team_member` | Cloud role only; **not** a daemon workspace role (ADR-0006). |

Source of truth: **cloud team membership** (see "Persistence and source of truth"). Daemon workspace membership/roles are daemon-owned and out of scope for cloud discovery.

### Create/join request

- **Create project**: in the hosted flow, "create" does **not** register a project in Cloud. Project registration is daemon-owned (ADR-0005). The hosted "create" contract is one of two shapes, both routed through the hard-allowlist bridge (ADR-0007):
  - `create_team` — create a cloud tenancy boundary (existing team API), then expose daemon workspaces into it; or
  - `expose_project` — a daemon-owner intent asking the paired daemon to expose an existing local workspace to the team. Cloud only routes/records the request; the daemon registers and authorizes it.
- **Join request**: join the **cloud team** that a project is discoverable under. Joining a team grants cloud discovery visibility only; it never grants daemon workspace action authority (ADR-0006). Alpha uses the existing team-membership APIs; there is no separate persisted "join request" object.

| Field | Type | Notes |
| --- | --- | --- |
| `team_id` | string | Target team for create/join. |
| `display_name` | string | For `create_team`. |
| `project_ref` | string (opaque) | For `expose_project`; daemon-local reference, not a path. |
| `requested_role` | enum `team_member` (default) | Join requests default to member; owner-granting is an owner action. |

### Selected project

The project the SPA currently operates against. It is **SPA/session-scoped selection state**, not persisted canonical cloud state (projection caching is deferred, ADR-0007). Selecting a project triggers a binding-readiness resolution against the paired daemon.

| Field | Type | Notes |
| --- | --- | --- |
| `project_id` | string | Currently selected project. |
| `resolved_at` | ISO-8601 | When readiness was last resolved for this selection. |
| `binding_readiness` | enum (see below) | Result of the last resolution. |

If a selected `project_id` is no longer discoverable (unpaired, team membership lost, daemon stopped exposing it), the selection is invalidated and the SPA falls back to the list with a repair hint.

### Daemon binding readiness

Coarse readiness describing whether the SPA can safely enable project actions against the paired daemon. It composes pairing status, compatibility envelope, and daemon-reported workspace readiness. It is **not** a compliance or signoff state.

| State | Meaning | SPA behavior |
| --- | --- | --- |
| `unpaired` | No active daemon pairing for the team. | Show pairing/install CTA. |
| `upgrade_required` | Paired daemon version incompatible (ADR-0007 too-old). | Block mutable controls; show upgrade repair hint. |
| `unbound` | Compatible pairing, but the workspace is not bound/exposed yet. | Offer expose/bind flow; reads only. |
| `degraded` | Bound but daemon reports degraded readiness. | Enable reads; warn on mutable controls. |
| `ready` | Bound and daemon reports ready. | Enable allowlisted dashboard actions. |
| `unknown` | Readiness not yet resolved. | Resolve on select; show neutral state. |

Mapping to existing types: `ready`/`degraded`/`unavailable`/`unknown` reuse `WorkspaceReadiness` (`models/discovery.py`); `unpaired`/`unavailable`/`paired` mirror the dashboard pairing states (CONTEXT.md); `upgrade_required` reuses the compatibility verdict (`models/compatibility.py`).

## Persistence And Source Of Truth

Existing Supabase tables at time of writing (verified against `supabase/migrations/`):

- `public.design_partner_allowlist` — design-partner **domain** allowlist (exists).
- Supabase **Auth** users/sessions — canonical identity (exists, managed by Supabase Auth).
- **No** Supabase product tables for projects, project registry, project membership, teams, pairings, or workspaces exist. Cloud coordination state is **in-memory** in alpha (`src/bicameral_cloud/store.py`, ADR-0008).

Therefore this contract uses a **local alpha adapter** for the project registry/membership tier and marks the assumption explicitly:

- **Local alpha adapter (assumption):** project registry and project membership are served by the in-memory cloud coordination `Store`, projecting daemon-exposed workspaces and gating visibility by cloud team membership. This is the adapter that stands in for the future Supabase-backed cloud coordination DB (ADR-0006). It is authoritative only for *cloud coordination/tenancy visibility*, never for local project truth.

Authority/source-of-truth by artifact:

| Artifact | Source of truth (alpha) | Future (per ADR) | Classification |
| --- | --- | --- | --- |
| User identity | Supabase Auth | Supabase Auth | canonical identity |
| Allowed domain | Supabase `design_partner_allowlist` | same | configuration authority |
| Cloud team + membership | in-memory `Store` (alpha adapter) | Supabase cloud DB (ADR-0006) | cloud tenancy state |
| Project registry / membership visibility | in-memory `Store` (alpha adapter) | Supabase cloud DB (ADR-0006) | cloud coordination projection |
| Project existence / registration | **local daemon** | local daemon | canonical local state |
| Workspace binding / provider secrets / local readiness / projections | **local daemon** | local daemon | canonical local state |

Explicit answer to acceptance criteria: **no existing Supabase table owns project membership.** In alpha, cloud-tier project membership is **derived from cloud team membership held by the in-memory alpha adapter** (`Store`); the canonical owner of *project registration and binding* is the **local daemon**. This is the marked adapter assumption.

## What Supabase Owns vs What The Local Daemon Owns

Supabase owns (canonical, cloud tier):

- Identity: authenticated users and sessions (Supabase Auth).
- Access configuration: the design-partner domain allowlist.
- (Intended, deferred) the cloud coordination DB for team registry/membership and project-visibility records, when the in-memory alpha adapter is replaced per ADR-0006. Supabase is **not** intended to own project registration, files, bindings, or any local workspace truth.

The local daemon owns (canonical, local tier, ADR-0005/0007):

- Project/workspace registration and existence.
- Workspace binding (which local path/repo a project maps to).
- Provider secrets and credentials.
- Local readiness and projections.
- Action authorization, signoff, source binding, Decision Ledger acceptance, compliance outcomes.

Cloud never stores or becomes authoritative for the local daemon's set (ADR-0006 boundary reaffirmed).

## API Contract

All project routes require a valid Supabase access token and pass the design-partner domain gate (`docs/specs/design-partner-signup.md`). Team-scoped routes additionally require cloud team membership (ADR-0006). These are the **proposed** contract routes; a subset already exists in the runtime and is noted.

### `GET /api/v1/projects`

- Caller: SPA (browser) with bearer token.
- Purpose: list all projects discoverable to the authenticated user across their teams — the primary SPA project-list surface.
- Response `ProjectListResponse`:
  - `projects`: array of `ProjectListItem`.
  - `selected_project_id`: string or null (session hint if the SPA supplies one; otherwise null).
- `ProjectListItem` fields: `project_id`, `team_id`, `display_name`, `pairing_status`, `binding_readiness`, `daemon_compatible`. (Subset of the Cloud project record; forbidden fields excluded.)

### `GET /api/v1/teams/{team_id}/projects`

- Caller: SPA.
- Purpose: projects for one team. **Alias/rename of the existing** `GET /api/v1/teams/{team_id}/workspaces` (ADR-0007, `routes/discovery.py`), reframed as projects. Same visibility rules: all team members see the same list; no per-user filtering.
- Response: `ProjectListResponse` scoped to `team_id`.

### `GET /api/v1/projects/{project_id}`

- Caller: SPA on project selection.
- Purpose: resolve the selected project and its current binding readiness.
- Response `ProjectDetail`: the Cloud project record plus a freshly resolved `binding_readiness` and `resolved_at`.

### `POST /api/v1/teams/{team_id}/projects`

- Caller: SPA (team owner for `expose_project`, any member for `create_team` is a separate route).
- Purpose: create/expose request. Body carries `create/join request` fields. Cloud **routes** the request to the paired daemon via the hard-allowlist bridge and records a coordination `AuditEvent`; Cloud does not register the project itself.
- Response: `202 Accepted` with a coordination receipt id; the project appears in discovery only after the daemon exposes it.

### `POST /api/v1/teams/{team_id}/members`

- Caller: SPA (existing team-membership API).
- Purpose: **join** — add a user to the cloud team so team-visible projects become discoverable. Cloud tenancy only; no daemon authority granted.

Response-shape note: all list/detail responses exclude `FORBIDDEN_DISCOVERY_FIELDS`. Cloud emits only coordination/projection data; it never returns local paths, remotes, secrets, evidence, or authority fields.

## Authority Matrix

| Actor/component | observe | validate | render | mutate | must not |
| --- | --- | --- | --- | --- | --- |
| Browser/SPA | own session, public config, discovered project list | none authoritative | project list, selected project, readiness | local session + selected-project selection | mutate project registry, membership authority, or daemon workspace state |
| Supabase Auth | OAuth identity | Google identity, JWT | auth-hosted UI | auth session | decide project membership or daemon authority |
| Supabase config/DB | domain allowlist (+future coord DB) | domain gate | none | allowlist rows (+future coord rows) | own project registration, bindings, or local truth |
| Bicameral Cloud API | JWT claims, team membership, daemon-exposed projects, compatibility | token/domain/team, route allowlist, compatibility envelope | `ProjectListResponse`/`ProjectDetail` JSON | alpha coordination records (team, membership, audit); route intent to daemon | register projects, bind workspaces, store secrets, write Decision Ledger/signoff/compliance |
| Local daemon | routed allowed intents | workspace authority, action policy | local projections | project registration, workspace binding, provider secrets, local readiness | trust cloud team role as workspace role |

## Failure Modes

| Condition | Result |
| --- | --- |
| Missing/invalid bearer token | `401` |
| Valid token, disallowed email domain | `403 Design partner access required` |
| Not a member of the requested team | `403` |
| Team has no active daemon pairing | `200` with empty `projects` and `binding_readiness=unpaired` for that team |
| Paired daemon incompatible (too old) | `409` on team-scoped discovery (existing behavior), or item `binding_readiness=upgrade_required` in aggregate list |
| Selected `project_id` no longer discoverable | `404`; SPA invalidates selection and returns to list with repair hint |
| Bridge/daemon unreachable at resolve time | `binding_readiness=unknown` (reads degrade gracefully; mutable controls disabled) |
| Create/expose request routed but daemon rejects/never exposes | project simply does not appear in discovery; coordination `AuditEvent` records the routed request |
| Supabase auth/config missing | authenticated project routes unavailable; signup reports configuration-needed (design-partner spec) |

## State Refinement Check

- Release state: an authenticated, invited user can list discoverable projects, select one, and see whether it is ready to act on via the paired daemon.
- States added/refined: `binding_readiness` ladder (`unpaired`, `upgrade_required`, `unbound`, `degraded`, `ready`, `unknown`); SPA-scoped `selected project`.
- Transition triggers: team membership change (list contents), daemon pairing/compat change (readiness), project selection (readiness resolution), daemon expose/stop-expose (list membership).
- Persistence: identity in Supabase Auth; domain gate in Supabase; team/membership/project-visibility in the in-memory alpha adapter (future Supabase coord DB); selected project in SPA/session only; project registration/binding in the local daemon.
- Invariants:
  - Cloud project records are coordination/projection, never canonical local project truth (ADR-0005).
  - Cloud team membership grants discovery visibility, never daemon workspace authority (ADR-0006).
  - Discovery responses never leak `FORBIDDEN_DISCOVERY_FIELDS` (ADR-0007).
  - Only hard-allowlisted intents route to the daemon; authority-bearing commands never transit Cloud (ADR-0007).
- Deferred vocabulary: per-user project ACLs, per-project membership rows, persisted invite/join-request objects, persisted projection cache.

## State Minimization Review

Project membership is represented as **derived team membership**, not a new persisted per-project state, because the alpha requirement is team-visible discovery (ADR-0007). Selected project is session state, not persisted, because projection caching is deferred (ADR-0007). Rejected/collapsed states:

- `per_user_project_acl`: rejected for alpha; ADR-0007 mandates uniform team visibility.
- `persisted_join_request`: collapsed into existing team-membership add; no review queue in alpha (design-partner specs defer invite/approval).
- `cloud_registered_project`: rejected; project registration is daemon-owned (ADR-0005). Cloud holds only a projection record.

## ADR Implications

- ADR-0005 reaffirmed: hosted dashboard discovers/renders projects; local daemon stays authoritative for project registration and binding.
- ADR-0006 reaffirmed: Supabase owns identity + allowlist; cloud coordination (team/membership/project visibility) is cloud-tier and, in alpha, in-memory; product tables are not browser-accessible.
- ADR-0007 reaffirmed: uniform team-visible discovery, minimal metadata, hard-allowlist routing, deferred projection cache.
- ADR-0008 reaffirmed: in-memory alpha store is the documented alpha posture; this contract's registry/membership tier rides that adapter until the coordination DB lands.
- No new ADR required: this is a reversible alpha contract that reuses accepted boundaries.

## Validation Plan

- Documentation review against ADR-0005/0006/0007/0008 and the design-partner specs.
- `rg "project discovery|project membership|Supabase" docs src tests`
- `python3 scripts/validate_governance_boundary.py`
- Repo test/lint/type commands (`pytest -q`, `ruff check .`, `mypy src`) for regression baseline; this slice changes docs + a factory attestation only.

## Open Questions / Human Input

- Confirm the SPA-facing noun: adopt **project** (this contract) and rename the existing `workspace` discovery surface, or keep both with `project` as an alias. Recorded as a domain-entity candidate.
- Confirm when the in-memory alpha adapter is replaced by the Supabase coordination DB (ADR-0006) and whether project-visibility records live there or remain daemon-projected.
- Confirm whether `expose_project` is in scope for the first discovery-API slice (cloud#65) or deferred.
