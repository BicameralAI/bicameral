# Hosted HTTP Response Security Policy

**Issues:** #149 and #150

**External assessment:** `BicameralAI/bicameral-factory#226`

## Baseline

Bicameral Cloud applies one shared response policy to hosted HTML, API, static,
redirect, error, and disclosure responses:

- `Strict-Transport-Security: max-age=31536000` on HTTPS requests only;
- `Referrer-Policy: strict-origin-when-cross-origin`;
- `X-Content-Type-Options: nosniff`;
- an enforcing `Content-Security-Policy` preserving the current hosted shell;
- `/.well-known/security.txt` using the approved monitored development contact.

The middleware does not overwrite a route-specific stricter header.

## HSTS boundary

The policy does not currently include `includeSubDomains` or `preload`. Those flags
must not be added until every affected Bicameral subdomain is verified as HTTPS-only
and the organization accepts the operational persistence and recovery implications.

## Current CSP compatibility allowances

The hosted signup, callback, and dashboard shell currently contains inline style and
script blocks and loads the Supabase browser client from jsDelivr. The CSP therefore
allows:

- inline scripts and styles;
- scripts from `https://cdn.jsdelivr.net`;
- connections to Bicameral's own origin;
- HTTPS and WebSocket connections to Supabase project subdomains; and
- loopback HTTP connections to the local Bicameral daemon.

These allowances are narrower than an unrestricted default but are not the desired
terminal policy. A future hardening phase should move inline code into versioned
assets or apply nonces or hashes, pin the browser dependency through the owned build,
and derive the exact Supabase connection origin from reviewed deployment
configuration.

The policy intentionally omits `upgrade-insecure-requests` because the supported
hosted dashboard pairs with a customer-controlled loopback daemon over HTTP. HSTS
still protects the external hosted origin.

## Disclosure record

The security disclosure route is intentionally excluded from OpenAPI. Its expiry
must be reviewed before June 30, 2027. Add a public policy link only after the
Security Information Page is deployed and approved.

## API schema and docs exposure

The OpenAPI schema (`/openapi.json`) and the interactive docs (`/docs`, `/redoc`)
are disabled by default so the hosted API surface is not publicly enumerable, in
remediation of the assessment's public OpenAPI-exposure finding. They are
re-enabled only outside production by setting `BICAMERAL_CLOUD_ENABLE_DOCS=1`. The
gate is applied at application construction in `app.py` (`_docs_kwargs`).

## Request rate limiting

An application-layer, per-client fixed-window rate limiter
(`rate_limit.RateLimitMiddleware`) bounds abusive request volume against a single
instance, in remediation of the assessment's absent-rate-limit finding. It is
explicitly a per-instance, in-process, best-effort control: Cloud Run may run
several instances, each holding its own counter, so the authoritative
cross-instance limit belongs at the edge (Cloudflare or Cloud Armor). This
middleware is defense-in-depth beneath that edge limit. Limits are set by
`BICAMERAL_CLOUD_RATE_LIMIT_REQUESTS` (default 300) and
`BICAMERAL_CLOUD_RATE_LIMIT_WINDOW_SECONDS` (default 60); a non-positive request
count disables it. Health and `/.well-known/` probe paths are never throttled, so
platform liveness checks and RFC 9116 discovery are unaffected. A 429 carries a
`Retry-After` header and still receives the response-security headers.

## Validation

Repository tests verify:

- headers on successful HTML and API responses;
- headers on error responses;
- HSTS absence over plain HTTP;
- disclosure-record content and schema exclusion;
- the minimal `OPTIONS /signup` rejection observed by the assessment;
- the OpenAPI/docs gate is disabled by the production default and enabled only by
  the explicit flag (`test_openapi_docs_gate.py`); and
- the rate limiter rejects over-limit requests with a `Retry-After`, exempts
  health/probe paths, and disables cleanly on a non-positive limit
  (`test_rate_limit.py`).

Deployment verification and an approved repeat external scan remain required before
the assessment findings are closed. The edge (Cloudflare or Cloud Armor)
rate-limit configuration remains the authoritative control and is confirmed
separately from application code.
