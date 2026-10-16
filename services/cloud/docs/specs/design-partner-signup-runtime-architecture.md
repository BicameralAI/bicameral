# Design Partner Signup Runtime Architecture

Date: 2026-06-29
Status: draft
Level: L1 repo architecture
Owns: Runtime flow for Supabase browser signup, callback validation, and design-partner domain authorization.
Related context: ADR-0005, ADR-0006, ADR-0007; `docs/specs/design-partner-signup.md`.

## Reading Guide

Main runtime invariant: Cloud Run may be publicly reachable, but authenticated Bicameral Cloud APIs must validate Supabase identity and design-partner domain before rendering or returning tenant-scoped data.

Governance gate: bic:spec-governance-gate advisory/no-op; Qor integration disabled pending factory policy revision.

## Runtime Problem

Cloud Run public access is required so browsers can start Supabase signup. Public network reachability must not be confused with product access. Current runtime has API token validation but no browser entrypoint, no callback path, and no design-partner gate.

## Goals And Non-Goals

Goals:

- Browser can load a signup page without Cloud Run IAM credentials.
- Browser can initiate Supabase Google OAuth.
- Callback validates Supabase session with backend before showing dashboard.
- Backend enforces the Supabase `design_partner_allowlist` domain table on all authenticated API routes.
- Dashboard shell remains non-authoritative for local daemon state.

Non-goals:

- No persisted invitation or approval workflow beyond the domain allowlist table.
- No per-email allowlist; the table stores domains only.
- No public team self-service beyond existing team APIs.
- No direct browser reads/writes to Supabase product tables.
- No local daemon authority moved into Cloud.

## Runtime Components

| Component | Responsibility | Inputs | Outputs | Persistent state | Must not decide/mutate |
| --- | --- | --- | --- | --- | --- |
| Browser signup shell | Render signup/login/callback/dashboard HTML and run Supabase JS | Public config, OAuth callback, session token | OAuth redirect, API calls | Supabase browser session | Team authority, daemon workspace authority |
| Supabase Auth | Google OAuth and session issuance | Browser OAuth request | Access token/JWT | Supabase auth state | Bicameral team membership, daemon authority |
| Bicameral Cloud API | Validate JWT, enforce domain gate, serve team APIs | Bearer token, env config | Session/team/pairing responses | In-memory alpha store; later DB per ADR-0006 | Decision Ledger writes, signoff, source binding |
| Cloud Run | Public HTTPS runtime | HTTP requests | FastAPI responses | revision/env configuration | App authorization decisions beyond invoking service |
| Local daemon | Workspace authority | Routed allowed commands | Projections/receipts | local workspace state | Cloud signup state |

## Canonical vs Derived State

| Artifact | Classification | Authority | Mutation path | Prohibited shortcut |
| --- | --- | --- | --- | --- |
| Supabase auth user/session | canonical identity state | Supabase Auth | Supabase OAuth/session refresh | Backend must not mint browser identity |
| Design partner domain allowlist | configuration authority | Supabase table/admin | Supabase row insert/update | Browser must not self-assert allowlist |
| Authenticated user object | derived projection/cache | Bicameral Cloud API | access-token validation/upsert | Not proof of team membership |
| Team membership | cloud tenancy state | Bicameral Cloud API | team owner/member APIs | Does not grant daemon workspace role |
| Dashboard HTML | rendered projection | Bicameral Cloud API | request render | Not canonical product state |
| Logs/build receipts | audit/log | Cloud Run/Cloud Build | runtime/build system | Not approval, signoff, or Decision authority |

## State Refinement Check

- Release state: public browser entrypoint plus authenticated domain-gated API access.
- States added/refined:
  - `unauthenticated_browser_session`
  - `authenticated_invited_user`
  - `authenticated_uninvited_user`
- Transition triggers:
  - Browser loads signup: no state mutation.
  - Supabase OAuth succeeds: Supabase mutates auth session.
  - Backend session validation succeeds: Cloud projects user identity and allows request.
  - Domain validation fails: Cloud rejects request; no team state mutation.
- Persistence:
  - Browser session: Supabase client storage.
  - Domain gate: Supabase table `public.design_partner_allowlist`, read server-side with the service key.
  - User projection: alpha in-memory store, future DB boundary per ADR-0006.
- Invariants:
  - Public Cloud Run invocation is not product authorization.
  - Domain allowlist is not team membership.
  - Hosted dashboard cannot materialize local Decisions.
- Deferred vocabulary:
  - invite, waitlist, seat, onboarding approval.

## State Minimization Review

The release does not need a new persisted signup state. Domain allowlist is enough for the user-stated design partner phase. Per-email invites and approval queues are deferred because they require persistence, lifecycle, and recovery semantics not needed here.

## Authority Matrix

| Actor/component | observe | validate | render | mutate | must not |
| --- | --- | --- | --- | --- | --- |
| Browser | public config, own session | none authoritative | signup/dashboard UI | local Supabase session only | mutate team/domain/daemon state |
| Supabase Auth | OAuth identity inputs | Google identity | auth-hosted UI | auth session | decide Bicameral tenancy |
| Bicameral Cloud API | JWT claims, Supabase domain allowlist | token/domain/team | HTML/API JSON | alpha user/team/pairing state | write local Decision Ledger or signoff |
| Cloud Run | HTTP request metadata | service invocation | none app-specific | revision traffic/env | decide app authorization when invoker check disabled |
| Local daemon | routed allowed intents | workspace authority | local projections | local workspace state | trust cloud team role as workspace role |

## Runtime Scenarios

### Startup / configuration

Trigger: Cloud Run revision starts.

1. App loads environment into `CloudSettings`.
2. Public routes render regardless of Supabase config.
3. `/api/v1/auth/config` reports whether browser signup is configured.
4. Authenticated APIs enforce Supabase access-token validation when `SUPABASE_URL` and either `SUPABASE_JWT_SECRET` or `SUPABASE_ANON_KEY` are present. Hosted signup can validate tokens through Supabase Auth API using the anon key.

Failure behavior: missing Supabase browser config renders a setup-needed message on signup instead of exposing secrets or failing with a stack trace.

### Signup command flow

Trigger: user opens `/signup` and clicks Google.

1. Browser fetches `/api/v1/auth/config`.
2. Browser initializes Supabase client with public URL/anon key.
3. Browser calls `signInWithOAuth({ provider: "google", redirectTo: "/auth/callback" })`.
4. Supabase redirects user to Google, then back to callback.

Failure behavior: if config missing, button disabled and page shows setup-needed state.

### Callback validation flow

Trigger: Supabase redirects to `/auth/callback`.

1. Browser Supabase client reads session from callback URL/storage.
2. Browser calls `/api/v1/auth/session` with bearer access token.
3. API validates JWT and email domain.
4. Invited user sees dashboard; uninvited user sees design-partner access message.

Failure behavior: invalid token returns `401`; unallowed domain returns `403`; browser clears session on sign-out.

### Forbidden flow

Forbidden: browser or dashboard directly writes team/daemon workspace state just because Supabase login succeeded.

Reason: ADR-0005 and ADR-0007 reserve local workspace authority for the daemon, and ADR-0006 separates cloud team roles from daemon workspace roles.

## Event, Command, And Protocol Contracts

### `GET /api/v1/auth/config`

- Caller: browser.
- Handler: Bicameral Cloud API.
- Response fields:
  - `supabase_url`: string or null.
  - `supabase_anon_key`: string or null.
  - `supabase_auth_enabled`: boolean.
- Exclusions: never returns JWT secret, service key, or domain allowlist.

### `GET /api/v1/auth/session`

- Caller: browser with bearer token.
- Handler: shared auth dependency.
- Validation:
  - Authorization header starts with `Bearer `.
  - JWT is valid if Supabase enabled.
  - Email domain has an enabled row in the Supabase `design_partner_allowlist` table when `SUPABASE_SERVICE_KEY` is configured; otherwise it must match `DESIGN_PARTNER_DOMAINS` fallback.
- Failure:
  - `401` invalid/missing token semantics.
  - `403` design partner access required.

### `GET /signup`, `/login`, `/auth/callback`, `/dashboard`

- Caller: browser.
- Handler: dashboard route module.
- Behavior: render static shell; browser performs auth workflow.
- Compatibility: no build step required.

## Substrate Capability Matrix

| Capability | Browser | Supabase | Cloud Run/API | Local daemon |
| --- | --- | --- | --- | --- |
| Persist canonical identity | no | yes | no | no |
| Enforce design partner gate | no | no | yes | no |
| Host signup UI | yes/rendered | optional hosted auth | yes | no |
| Persist team tenancy | no | no product tables in alpha | alpha store/future DB | no |
| Enforce workspace action policy | no | no | no | yes |
| Support team-wide synchronization | no | auth only | future DB | workspace-local |

## Deployment Runtime Topology

```text
landing page
  -> https://app.bicameral-ai.com/signup
      -> Cloud Run public FastAPI route
      -> Supabase OAuth
      -> /auth/callback
      -> /api/v1/auth/session
      -> /dashboard or access denied
```

Cloud Run remains public at network level. Application access is enforced by Supabase access-token validation and domain gate.

## Failure And Recovery

- DNS/certificate pending: canonical Cloud Run URL remains usable.
- Supabase env missing: signup page reports configuration missing.
- JWT secret mismatch: API rejects with `401`.
- Partner domain missing from env: API rejects with `403`.
- OAuth provider misconfigured: Supabase returns provider error; browser stays on signup/callback with retry path.

## Observability, Audit, And Receipts

- Cloud Run request logs show route status.
- GitHub Actions and Cloud Build logs show deployment status.
- API responses distinguish invalid token from uninvited domain.
- Logs are operational evidence, not signoff or canonical Decision authority.

## Security, Privacy, Authority Boundaries

- Public browser config exposes only Supabase URL and anon key.
- Supabase service key and any configured JWT secret stay server-side.
- Domain allowlist is read server-side from Supabase and is never returned to browsers.
- Team APIs continue to require membership.
- Cloud does not store provider credentials, source bindings, signoff state, canonical projections, or local compliance outcomes.

## Implementation Sequencing

1. Add config and domain gate
   - Files: `config.py`, `auth/dependencies.py`, auth tests.
   - Acceptance: configured domains enforce `403` for outsiders.

2. Add browser auth routes
   - Files: `routes/dashboard.py`, dashboard tests.
   - Acceptance: `/`, `/signup`, `/login`, `/auth/callback`, `/dashboard` render.

3. Add public auth config endpoint
   - Files: `routes/auth.py`, auth tests.
   - Acceptance: returns only public config.

4. Deploy and configure env
   - Files/system: Cloud Run env vars.
   - Acceptance: public signup page can start OAuth.
   - Blocked on Supabase values.

## Issue Generation Notes

Checked for collisions: no open PRs. Open issues #21 and #28 are adjacent but not the same scope. This work should not close or mutate them.

Todo:

1. Implement Supabase-backed `design_partner_allowlist` domain table.
2. Implement public auth config route.
3. Implement signup/login/callback/root/dashboard shell.
4. Add tests.
5. Merge and auto-deploy.
6. Configure Cloud Run env when Supabase values are available.

## ADR Implications

- ADR-0005 reaffirmed.
- ADR-0006 reaffirmed with domain-gated design partner alpha access.
- ADR-0007 unaffected/reaffirmed.
- No new ADR required for reversible domain-only alpha gate.

## Acceptance Criteria

- `/` no longer returns `404`.
- `/signup` is a valid landing-page redirect target.
- `/api/v1/auth/config` exposes no secrets.
- Valid Supabase JWT with allowed domain can call session endpoint.
- Valid Supabase JWT with disallowed domain receives `403`.
- Existing team tenancy tests still pass.

## Validation Plan

- `pytest -q`
- `ruff check .`
- `mypy src`
- `python3 scripts/validate_governance_boundary.py`
- Public endpoint checks after deploy.
