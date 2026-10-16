/**
 * Fixture types and sample data for the project selector.
 *
 * These contracts let the Project Selector component develop against
 * stable shapes without live project-registry persistence.
 *
 * Boundary: fixtures carry display/metadata only — no tokens, secrets,
 * or credential material. The selector does not expose OAuth details.
 *
 * Spec ref: docs/specs/project-setup-ui.md § Global Project Selector
 */

import type { HostedProjectPreview } from "./onboarding-contract";

// ── Project readiness ────────────────────────────────────────────

export type ProjectReadiness = "ready" | "needs_access" | "offline";

export type EventStoreKind = "github" | "google_drive";

/**
 * How Cloud made a project discoverable under domain-gated discovery
 * (cloud#95). Coordination projection only; never a join code or authority
 * field. Present for Cloud-discovered rows that carry the hint.
 */
export type DiscoveryEligibility = "domain" | "team" | "membership";

// ── Access failure reason ────────────────────────────────────────

export type AccessFailureReason =
  | "missing_connection"
  | "missing_resource_selection"
  | "insufficient_account_access"
  | "token_expired"
  | "resource_moved_or_deleted";

// ── Project entry ────────────────────────────────────────────────

/**
 * Whether the actor is a canonical Product Member (`member`) or merely sees the
 * Product as available through verified-domain discovery (`available`, eligible
 * but not yet joined). Absent is treated as `member` for the local/registered
 * project rows that predate domain-discovered auto-join (bot#768). Membership is
 * a canonical daemon fact; the UI only projects it.
 */
export type ProductMembership = "member" | "available";

export type ProjectEntry = {
  id: string;
  name: string;
  event_store_kind: EventStoreKind;
  event_store_label: string;
  readiness: ProjectReadiness;
  access_failure?: AccessFailureReason;
  provider_id?: string;
  /**
   * Membership axis (bot#768). Absent ⇒ `member` (a `Your products` row).
   * `available` rows are eligible-but-not-joined and render a `Join` action.
   */
  membership?: ProductMembership;
  /**
   * Verified domain that makes an `available` row discoverable, used for the
   * `Available for <verified-domain>` group heading. Coordination hint only.
   */
  verified_domain?: string;
  /**
   * Joined + ready but local registration/provider setup is still incomplete:
   * the row surfaces `Continue setup` instead of `Open`. Never rolls membership
   * back — a joined Product stays a member even when setup is incomplete.
   */
  setup_incomplete?: boolean;
  /**
   * The Cloud directory projection an `available` row was derived from. Carried
   * so the row's `Join` action can drive the `product.join` contract without
   * re-deriving eligibility client-side. Non-canonical; absent for member rows.
   */
  preview?: HostedProjectPreview;
  /**
   * Owning cloud team (coordination id only, ADR-0006). Present for
   * Cloud-discovered projects; lets Create target the existing team instead of
   * spinning up a new one. Not a local path or secret.
   */
  team_id?: string;
  /**
   * How Cloud made this project discoverable (cloud#95 domain-gated discovery).
   * Optional coordination hint; absent for local/dev fixtures.
   */
  eligibility?: DiscoveryEligibility;
};

// ── Failure copy mapping ─────────────────────────────────────────

export const ACCESS_FAILURE_COPY: Record<AccessFailureReason, string> = {
  missing_connection: "not connected",
  missing_resource_selection: "Repository not selected",
  insufficient_account_access: "Connected account lacks repo access",
  token_expired: "Token expired",
  resource_moved_or_deleted: "Repo moved or deleted",
};

export const ACCESS_FAILURE_ACTION: Record<AccessFailureReason, string> = {
  missing_connection: "Connect provider",
  missing_resource_selection: "Select resource",
  insufficient_account_access: "Use another account",
  token_expired: "Reconnect",
  resource_moved_or_deleted: "Repair project",
};

// ── Sample data ──────────────────────────────────────────────────

export const SAMPLE_PROJECTS: ProjectEntry[] = [
  {
    id: "proj_acme_checkout",
    name: "Acme Checkout",
    event_store_kind: "github",
    event_store_label: "GitHub event store",
    readiness: "ready",
    provider_id: "github",
  },
  {
    id: "proj_bicameral_bot",
    name: "Bicameral Bot",
    event_store_kind: "google_drive",
    event_store_label: "Google Drive",
    readiness: "needs_access",
    access_failure: "missing_connection",
    provider_id: "google",
  },
  {
    id: "proj_factory_roadmap",
    name: "Factory Roadmap",
    event_store_kind: "github",
    event_store_label: "GitHub event store",
    readiness: "offline",
    provider_id: "github",
  },
];
