# Cloud Admission Model

**Status:** accepted

**Related:** #343, ADR-0005, ADR-0006, ADR-0007

## Context

Supabase Auth answers who an authenticated hosted user is. Bicameral Cloud still needs a separate admission decision that answers whether that authenticated identity may enter the hosted Bicameral product at all.

The original alpha design used the Supabase-backed `public.design_partner_allowlist` table as a domain-only gate. That is sufficient for Bicameral staff and customers with a dedicated corporate domain, but it is not a safe general customer model. Admitting one customer who uses a shared provider such as `gmail.com` must not admit every authenticated user on that provider.

Admission is also not tenancy. A person may be admitted to Bicameral Cloud without yet belonging to a particular customer team, and team membership must not imply local daemon, Product, workspace, provider, Decision, signoff, or compliance authority.

## Decision

Bicameral Cloud separates hosted access into four layers.

1. **Authentication**
   - Supabase Auth validates Google identity and owns the hosted browser auth session.
   - The server consumes the authenticated subject and normalized verified email supplied by the identity layer.
   - Authentication alone does not grant Bicameral product access.

2. **Admission**
   - Bicameral Cloud applies a server-side admission policy after authentication and before tenant-scoped Cloud APIs are exposed.
   - Admission rules may match either an exact normalized email domain or an exact normalized email address.
   - Examples:
     - domain rule: `bicameral-ai.com`
     - exact-email rule: `customer@example.com`
   - A matching enabled domain rule OR matching enabled exact-email rule admits the identity.
   - Shared public domains such as `gmail.com`, `outlook.com`, and similar providers must not be admitted as whole domains merely to admit one customer.
   - The browser cannot create, modify, or bypass admission rules.
   - If the authoritative admission store is unavailable, Cloud fails closed with a typed service-unavailable result rather than silently admitting the user.

3. **Tenancy and roles**
   - Admission means only that the authenticated identity may enter Bicameral Cloud.
   - Admission does not create team membership, tenant ownership, project/product membership, or any role.
   - Team membership and Cloud roles remain explicit Cloud-owned records under ADR-0006 and are evaluated separately for tenant-scoped data and operations.

4. **Local and Product authority**
   - Daemon pairing approval remains local-daemon authority.
   - Product membership, workspace binding, provider authorization, source binding, Decision authority, signoff, and compliance outcomes remain governed by their existing authoritative components and contracts.
   - Neither authentication, admission, nor Cloud team membership may manufacture those states.

## Admission record shape

The target durable admission model is a server-managed Supabase Postgres table with a normalized subject type and subject value, conceptually:

```text
cloud_admission_rules
  id
  subject_type   # domain | email
  subject_value  # normalized lowercase domain or full email
  enabled
  notes
  created_at
  updated_at
```

The logical uniqueness constraint is `(subject_type, subject_value)`.

Admission policy must not use mutable `auth.users` metadata as its source of truth. Supabase Auth remains the identity/session substrate; Bicameral-owned admission records remain the authorization source of truth.

Email normalization for admission is intentionally conservative: lowercase and trim the verified email value. Bicameral does not infer provider aliases, plus-address equivalence, mailbox forwarding, or organization ownership from an address.

## Alpha compatibility and migration

The current alpha runtime may continue reading the existing domain-only `public.design_partner_allowlist` table while the generalized admission table is implemented.

For the alpha/demo path:

- `bicameral-ai.com` is a required enabled admission domain.
- The tracked migration set must idempotently reassert that internal domain so a prior manual edit cannot silently lock Bicameral staff out of the demo.
- Existing domain checks remain server-side and service-role-backed.
- No customer domain is added implicitly by this ADR.

When generalized exact-email admission is implemented:

1. create the generalized admission table with server-only mutation access;
2. migrate existing enabled and disabled domain rows transactionally into `subject_type = 'domain'` rules;
3. validate parity for the existing Bicameral internal domain and any approved design-partner domains;
4. change the application admission check to evaluate exact email OR domain;
5. keep the previous table available until production parity is verified, then deprecate it in a later migration.

## Revocation semantics

Disabling or removing an admission rule blocks new authorized Cloud access after the hosted identity is re-evaluated. Admission revocation must not pretend to revoke authority owned elsewhere.

Separate mechanisms remain responsible for team membership removal, hosted-session invalidation where required, daemon pairing revocation, Product membership changes, provider-token revocation, and other authority-bearing lifecycle actions.

## Consequences

- Bicameral staff can be admitted safely by the `bicameral-ai.com` domain.
- A customer organization with a dedicated trusted domain can be admitted by domain when Bicameral chooses to onboard the organization that way.
- A customer using a shared email provider can be admitted one exact account at a time without opening access to the provider's entire domain.
- Authentication, admission, tenancy, and local/Product authority remain inspectable and independently revocable layers.
- Admission-rule changes are security-sensitive Cloud coordination actions and should be included in the Cloud coordination audit trail when that audit surface covers administrative admission changes.

## Rejected alternatives

- **Supabase Auth user existence as admission:** rejected because successful authentication is identity proof, not Bicameral authorization.
- **Whole-domain admission for shared providers:** rejected because it over-grants access.
- **Supabase `auth.users` metadata as the allowlist:** rejected because admission is Bicameral policy and should not be hidden inside mutable identity-provider metadata.
- **Admission automatically creating a team membership:** rejected because product entry and tenant authorization are separate facts.
- **Admission implying local daemon or Product authority:** rejected because it violates ADR-0005/0006 authority boundaries.

## User Journey and Boundary Review

**UserJourney:** `cloud_identity_admission`

- Actor: a Google-authenticated Bicameral staff member or approved customer identity.
- Entrypoint: hosted Cloud sign-in from `/connect`.
- Observable success: the authenticated identity passes the server-side admission gate and may continue to the separate device-pairing stage. A non-admitted identity is denied before tenant-scoped Cloud access.
- Participating surfaces: browser, Supabase Auth, Bicameral Cloud auth dependency, Supabase Postgres admission policy, and the later local-daemon pairing boundary.
- Boundary contracts: Supabase authenticated identity, Bicameral admission rule, Cloud team membership, and hosted-bridge pairing remain independent facts.
- Required evidence: deterministic auth/admission tests, tracked migration evidence, production deployment migration receipt, and live browser qualification with an admitted internal identity.

**Journey Boundary Review:** passed. The owner-confirmed #343 direction explicitly separates authentication, admission, tenancy, and local/Product authority. The admission decision commits only the hosted-entry fact. It cannot commit team membership, daemon approval, Product membership, provider authorization, or any local governed state. Partial failure at admission remains a typed Cloud denial/service-unavailable outcome; retry re-evaluates identity and admission rather than manufacturing downstream state.

**State refinement:** the smallest honest hosted state distinction is `authenticated_not_admitted` versus `authenticated_admitted`. Admission is request-time authorization policy, not a durable per-user invitation lifecycle. Exact-email and domain are rule match types, not additional user states.

**State minimization:** no pending-invite, signup-requested, auto-member, or paired states are added by this ADR. Those states either belong to separate lifecycle owners or remain deferred until a real workflow requires them.

Governance gate: bic:spec-governance-gate; Qor advisory/no-op pending factory policy revision. UserJourney: `cloud_identity_admission`. Journey Boundary Review: passed.
