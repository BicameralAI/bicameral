# Design Partner Signup

Date: 2026-06-29
Status: draft
Level: L1 repo architecture
Owns: Browser signup entrypoints and domain-gated Supabase identity access for Bicameral Cloud alpha.
Related context: ADR-0005, ADR-0006, ADR-0007; PR #29 Supabase auth and team tenancy; user request to redirect landing page to signup.

## Reading Guide

Main invariant: Supabase Auth may prove browser identity, but Bicameral Cloud must still apply a server-side design-partner domain gate before exposing authenticated cloud APIs.

Governance gate: bic:spec-governance-gate advisory/no-op; Qor integration disabled pending factory policy revision.

## Problem

Bicameral Cloud is now public on Cloud Run, but it has only API-level Supabase access-token validation and a placeholder dashboard. A landing page needs a concrete signup URL. During design partner phase, signup must not silently grant product access to every Google-authenticated user.

## Goals

- Provide browser routes `/`, `/signup`, `/login`, `/auth/callback`, and `/dashboard`.
- Start Supabase Google OAuth from the hosted app.
- Validate returned Supabase sessions against `/api/v1/auth/session`.
- Gate authenticated API access by email domain through the Supabase `design_partner_allowlist` table, with `DESIGN_PARTNER_DOMAINS` retained only as local/emergency fallback.
- Keep cloud team roles separate from daemon workspace roles.
- Keep local daemon authority boundaries from ADR-0005 and ADR-0007 intact.

## Non-Goals

- No public self-serve billing, procurement, or team discovery.
- No email-specific invite table in this slice.
- No Supabase product-table browser reads or writes.
- No Cloud authority over local workspace actions, signoff, Decision Ledger writes, source bindings, or compliance outcomes.

## Proposed Direction

Use Supabase Auth for browser identity and add a small hosted HTML/JS shell that calls Bicameral Cloud APIs with the Supabase access token. The backend remains the authorization boundary: it validates JWTs and checks the user's email domain against the Supabase `design_partner_allowlist` table.

The landing page should redirect users to:

```text
https://app.bicameral-ai.com/signup
```

Until `app.bicameral-ai.com` certificate provisioning completes, the canonical Cloud Run URL can use the same path.

## State Refinement Check

- Release state: unauthenticated visitor can open signup, authenticate with Supabase Google OAuth, return to callback, and either enter dashboard or see a design-partner access denial.
- States added/refined:
  - `unauthenticated_browser_session`: no Supabase session in browser.
  - `authenticated_invited_user`: Supabase identity validated and email domain allowed.
  - `authenticated_uninvited_user`: Supabase identity valid but email domain not allowed.
- Transitions:
  - Browser clicks Google signup: unauthenticated -> Supabase OAuth redirect.
  - Supabase callback returns token: callback validates token through API.
  - Backend validates domain: valid token -> invited or uninvited result.
- Persistence:
  - Supabase owns auth session.
  - Browser stores Supabase client session.
  - Bicameral Cloud stores no signup approval record in this slice.
  - Domain allowlist lives in Supabase table `public.design_partner_allowlist`; Cloud Run reads it with the server-side service key.
- Invariants:
  - Supabase identity does not grant daemon workspace authority.
  - Dashboard shell is not Decision Ledger or local projection authority.
  - Domain allowlist is an access gate, not team membership.
- Deferred vocabulary:
  - `invite`, `waitlist`, `approval`, and `seat` are deferred until there is a persisted invitation workflow.

## State Minimization Review

Domain allowlisting is represented as configuration plus request-time validation, not a persisted invitation state. Email-specific invite states are intentionally deferred because the current requirement is domain-only.

Rejected/collapsed states:

- `pending_invite`: deferred, no invite object exists.
- `approved_partner`: collapsed into enabled allowlist domain.
- `signup_requested`: deferred, no review queue in this slice.

## Existing Infrastructure Inventory

- `src/bicameral_cloud/config.py`: environment-backed settings.
- `src/bicameral_cloud/auth/dependencies.py`: shared FastAPI auth dependency.
- `src/bicameral_cloud/auth/supabase.py`: Supabase access-token validation.
- `src/bicameral_cloud/routes/auth.py`: authenticated session and current-user team endpoints.
- `src/bicameral_cloud/routes/dashboard.py`: hosted dashboard shell.
- `cloudbuild.yaml` and `.github/workflows/deploy-cloud-run.yml`: auto-deploy from `main`.

## Contract Shape

Environment variables:

| Name | Required | Public to browser | Purpose |
| --- | --- | --- | --- |
| `SUPABASE_URL` | yes for real signup | yes | Supabase project URL. |
| `SUPABASE_ANON_KEY` | yes for browser signup | yes | Supabase browser client key. |
| `SUPABASE_JWT_SECRET` | no for hosted signup | no | Optional local JWT validation secret; when absent, backend validates access tokens through Supabase Auth API using the anon key. |
| `SUPABASE_SERVICE_KEY` | yes for hosted design partner gate | no | Service-role key used server-side to read the domain allowlist table. |
| `DESIGN_PARTNER_ALLOWLIST_TABLE` | no | no | Defaults to `design_partner_allowlist`. |
| `DESIGN_PARTNER_DOMAINS` | fallback only | no | Comma-separated local/emergency fallback domains when service key is absent. |

Routes:

| Route | Auth | Behavior |
| --- | --- | --- |
| `GET /` | none | Redirects to `/dashboard` or serves signup entry. |
| `GET /signup` | none | Renders signup page with Google OAuth button. |
| `GET /login` | none | Alias for signup. |
| `GET /auth/callback` | Supabase browser session | Validates current Supabase session through API then redirects/renders result. |
| `GET /dashboard` | browser session checked by JS | Renders dashboard shell and calls session API. |
| `GET /api/v1/auth/config` | none | Returns public Supabase browser config. |
| `GET /api/v1/auth/session` | bearer token | Returns current user only if Supabase access token is valid and domain allowed. |

Error behavior:

- Missing auth header: existing FastAPI validation behavior.
- Invalid JWT: `401`.
- Valid Supabase access token but domain not allowed: `403`, `detail="Design partner access required"`.
- Supabase browser config missing: signup page renders configuration-needed state.

## Trust, Privacy, Security, And Authority Boundaries

- Browser may observe public Supabase URL and anon key.
- Browser must not receive Supabase service key or JWT secret.
- Backend validates identity and domain membership.
- Domain allowlist does not create team membership.
- Cloud team ownership does not imply daemon workspace ownership.
- Hosted dashboard does not write accepted Decisions, signoff, source bindings, canonical projections, provider credentials, or local compliance outcomes.

## Per-Surface Behavior

- Landing page: redirect to `/signup`.
- Signup page: starts Google OAuth through Supabase.
- Callback page: validates session with backend; unauthorized users see access-denied message.
- Dashboard page: renders only after session validation succeeds; otherwise prompts login.
- API: shared dependency enforces domain gate for all authenticated routes.

## Implementation Plan

1. Signup surface and root route
   - Target: `src/bicameral_cloud/routes/dashboard.py`, tests.
   - Acceptance: `/`, `/signup`, `/login`, `/auth/callback`, `/dashboard` return HTML.
   - Task class: AFK_REVIEW_ONLY.

2. Public auth config route
   - Target: `src/bicameral_cloud/routes/auth.py`, `config.py`, tests.
   - Acceptance: browser can fetch Supabase URL/anon key without secrets.
   - Task class: AFK_REVIEW_ONLY.

3. Domain allowlist enforcement
   - Target: `config.py`, `auth/dependencies.py`, tests.
   - Acceptance: configured allowed domain passes; outside domain receives `403`.
   - Task class: AFK_REVIEW_ONLY.

4. Deploy configuration
   - Target: Cloud Run environment.
   - Acceptance: `SUPABASE_URL`, `SUPABASE_ANON_KEY`, `SUPABASE_JWT_SECRET`, and `DESIGN_PARTNER_DOMAINS` configured.
   - Task class: HITL_BLOCKED until Supabase values are provided.

## ADR Implications

- ADR-0005 reaffirmed: hosted dashboard shell remains non-authoritative for local daemon state.
- ADR-0006 reaffirmed/amended: confirms Supabase Auth plus application-level authorization as alpha enforcement point; adds domain allowlist as design-partner gate.
- ADR-0007 reaffirmed: no change to bridge routing allowlist or local daemon action authority.
- No new ADR required for the initial domain-only allowlist because it is reversible alpha configuration.

## Issue Generation Notes

No open PR collisions were found. Open issues at time of planning: #28 RFQ domain entities and #21 audit trail. This work should not modify audit trail or domain-promotion scope.

Todo slices:

1. Add hosted signup/login/callback/root routes.
2. Add public Supabase browser config endpoint.
3. Add `DESIGN_PARTNER_DOMAINS` config and shared auth gate.
4. Add tests for allowed and denied domains.
5. Configure Cloud Run Supabase/design-partner env vars after values are known.

## Validation Plan

- `pytest -q`
- `ruff check .`
- `mypy src`
- `python3 scripts/validate_governance_boundary.py`
- Manual public checks:
  - `GET /health` -> 200
  - `GET /signup` -> HTML
  - `GET /api/v1/auth/config` -> no secrets
  - valid JWT with allowed domain -> 200
  - valid JWT with unallowed domain -> 403

## Open Questions

- Supabase project URL, anon key, service key, and the allowlist table are needed before real Google signup works in production. JWT secret is optional because the backend can validate access tokens through Supabase Auth API.
- The first allowed domain is `bicameral-ai.com`; additional design partner domains are added by inserting/updating rows in `public.design_partner_allowlist`.

## Request Coverage

- `$bic:to-spec`: this document.
- `$bic:to-runtime-spec`: companion runtime spec.
- "break it down into todo": Issue Generation Notes.
- "/goal to implement": implementation goal active in this session.
- "dont collide": checked open PRs and issues before branch creation.
