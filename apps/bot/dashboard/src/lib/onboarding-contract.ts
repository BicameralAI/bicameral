/**
 * TypeScript mirror of the domain-discovered Product onboarding contract
 * (bicameral-bot#765). These types forward-define the wire objects the SPA
 * consumes for per-Product discovery and same-domain auto-join; they are kept
 * in lockstep with the hand-authored contract-spec schemas and fixtures under
 * `protocol/onboarding/` and with the authoritative contract in
 * `docs/specs/domain-product-onboarding-contract.md`.
 *
 * Authority boundary (contract § Runtime Invariant): the local daemon is the
 * sole canonical writer of Product identity, roles, membership, and events.
 * Hosted cloud discovers, previews, and mints a narrow join ticket; it never
 * writes canonical state. The SPA renders projections and sends requests — it
 * cannot supply actor context or write canonical membership. A `ProductJoinTicket`
 * carries only bounded claims and NEVER the full hosted session token.
 *
 * This module is types + copy only. It introduces no runtime materialization
 * (daemon membership, ticket signing, Cloud persistence) — those are explicit
 * non-goals of #768 and are owned by the runtime/Cloud slices.
 */

// ── HostedProjectPreview (Cloud, non-canonical directory projection) ─────────

/** Mirrors `hosted-project-preview.schema.json`. */
export type HostedProjectPreview = {
  preview_id: string;
  /** Canonical Product id published by the daemon. Cloud never invents this. */
  product_id: string;
  product_display_name: string;
  /** Non-canonical company/team label. */
  team_display_name?: string;
  /** Verified domain used for same-domain discovery eligibility. */
  verified_domain?: string;
  event_store_hint?: {
    substrate?: "git" | "drive_folder" | "unknown";
    provider?: "github" | "google" | "unknown";
  };
  required_connection?: {
    provider?: "github" | "google" | "none";
  };
  /** Always false — a directory projection is explicitly non-canonical. */
  canonical: false;
  expires_at: string;
};

// ── ProductJoinTicket (Cloud, narrow signed short-lived) ─────────────────────

/**
 * Mirrors `product-join-ticket.schema.json`. The ticket carries only the
 * minimum bounded claims (schema is fail-closed). It must NEVER carry the full
 * hosted session token, refresh tokens, roles, or membership assertions — roles
 * and membership are daemon-authoritative outputs, not ticket inputs.
 */
export type ProductJoinTicket = {
  issuer: string;
  audience: string;
  product_id: string;
  cloud_account_id: string;
  verified_domain: string;
  issued_at: string;
  expiry: string;
  nonce: string;
  registration_ref: string;
  provider_hint?: string;
  substrate_hint?: string;
};

/** Cloud transport envelope authenticated by the daemon before use. */
export type SignedProductJoinTicket = {
  ticket: ProductJoinTicket;
  key_id: string;
  /** Base64-encoded detached Ed25519 signature over `ticket`. */
  signature: string;
};

// ── ProductJoinRequest (daemon-inbound) ──────────────────────────────────────

/** Mirrors `product-join-request.schema.json`. */
export type ProductJoinRequest = {
  request_id: string;
  requested_at: string;
  /** Locally confirmed actor context reference. Cloud identity alone is never a local actor. */
  local_actor_ref: string;
  ticket: ProductJoinTicket;
};

// ── ProductJoinResult (daemon-authoritative) ─────────────────────────────────

/** Typed same-domain join failure codes (contract § Contract: product.join). */
export type ProductJoinFailureCode =
  | "expired_ticket"
  | "wrong_domain"
  | "wrong_account"
  | "wrong_product"
  | "replayed_ticket";

export type ProductJoinFailure = {
  code: ProductJoinFailureCode;
  message: string;
};

/**
 * Mirrors `product-join-result.schema.json`. On success the daemon grants
 * `Product Member` ONLY (never Policy/Product Owner) and appends a canonical
 * membership event + receipt. On failure it returns a typed rejection and
 * creates no membership. There is no durable pending-approval state.
 */
export type ProductJoinResult = {
  request_id: string;
  product_id: string;
  cloud_account_id: string;
  outcome: "granted" | "rejected";
  /** `Product Member` on success; null on rejection. */
  granted_role: "Product Member" | null;
  membership_event_ref: string | null;
  receipt_id: string | null;
  failure: ProductJoinFailure | null;
  decided_at: string;
};

export const ALL_PRODUCT_JOIN_FAILURE_CODES: readonly ProductJoinFailureCode[] = [
  "expired_ticket",
  "wrong_domain",
  "wrong_account",
  "wrong_product",
  "replayed_ticket",
] as const;
