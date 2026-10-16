# Product Join Ticket Contract

Date: 2026-07-12

Status: implemented

Source issue: [BicameralAI/bicameral-cloud#126](https://github.com/BicameralAI/bicameral-cloud/issues/126)

Cross-repo contract: `BicameralAI/bicameral-bot/protocol/onboarding`

## Authority and lifecycle

`POST /api/v1/projects/{product_id}/join-ticket` exchanges an authenticated,
verified-domain Cloud account's directory eligibility for one narrow,
short-lived signed assertion. Issuance is ephemeral. It creates no canonical
membership, pending approval, join-request record, invitation, role, policy
state, or local registration.

The daemon verifies the ticket, binds local actor context, re-evaluates its
canonical Product policy, performs replay defense, and is the only writer of a
Product Member event. A directory row and a successfully issued ticket are
neither membership nor proof that the daemon will grant membership.

## Signed wire contract

The response matches the bot verifier's `SignedProductJoinTicket` envelope:

```json
{
  "ticket": {
    "issuer": "https://cloud.example",
    "audience": "daemon:prd_123",
    "product_id": "prd_123",
    "cloud_account_id": "acct_123",
    "verified_domain": "example.com",
    "issued_at": "2026-07-12T09:00:00Z",
    "expiry": "2026-07-12T09:05:00Z",
    "nonce": "jti_opaque",
    "registration_ref": "reg_opaque",
    "provider_hint": "github",
    "substrate_hint": "git"
  },
  "key_id": "join-2026-07",
  "signature": "<base64 Ed25519 signature>"
}
```

Bot contract names are preserved rather than adding JWT aliases:
`issued_at` is the issued/not-before boundary (`iat`/`nbf`), `expiry` is `exp`,
and `nonce` is the single-use `jti`. The canonical signing input is compact
UTF-8 JSON with recursively sorted object keys and omitted null optional
claims. Extra ticket claims fail closed.

The ticket never carries a Supabase access/refresh token, provider credential,
raw event-store descriptor, local path, role, membership assertion, actor
context, or unrelated account data.

## Eligibility and typed failures

Cloud loads the durable Product directory row by canonical Product ID and
requires:

- an authenticated account;
- an explicitly verified, normalized email domain;
- an enabled Product directory row;
- exact equality between the account domain and `eligible_domain`;
- an opaque `registration_ref`.

Failures use typed, redacted response details. Product existence and
eligibility are scoped to the requested canonical Product; a ticket for one
Product/account cannot be reused for another Product/account/tenant boundary.

## Signing and rotation

Tickets use a dedicated Ed25519 key, not a Supabase or provider credential.
TTL is constrained to at most five minutes. The active key signs new tickets.
`GET /api/v1/product-join/verification-keys` publishes versioned raw Ed25519
public-key metadata for the active key plus explicitly configured overlap keys.

Rotation proceeds:

1. Publish the new active key while the previous key remains `overlap`.
2. Sign new tickets only with the new active key.
3. Keep the previous public key published for at least the maximum ticket TTL
   plus verifier cache allowance.
4. Mark the previous key `retired`; it disappears from published metadata.

Unknown and retired `key_id` values have no trust anchor and fail closed in the
daemon. Cloud also fails closed when the active key, issuer, TTL, or key-ring
configuration is absent or malformed.

## Legacy path

`POST /api/v1/teams/{team_id}/join-requests` remains temporarily for
compatibility, is marked deprecated in OpenAPI, and is not the primary alpha
onboarding path. The vendored Operator Home uses per-Product Join and the
join-ticket exchange instead of creating a durable owner-reviewed request.
