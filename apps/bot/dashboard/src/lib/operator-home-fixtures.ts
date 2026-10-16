/**
 * Fixture types and sample data for the operator-home project readiness UI.
 *
 * These contracts let the Operator Home component develop against stable
 * shapes without live daemon pairing. The fixtures align with the Rust-side
 * ProjectReadinessEntry and OperatorHomeSessionResponse types in
 * crates/bicameral-api/src/hosted_bridge.rs.
 *
 * Boundary: fixtures carry display/readiness metadata only — no tokens,
 * filesystem paths, SourceEvidence bodies, Decision Ledger event payloads,
 * or OAuth internals.
 *
 * Spec ref: docs/specs/explicit-project-registration-hosted-local-discovery.md
 */

import type {
  ProjectEntry,
  ProjectReadiness,
  AccessFailureReason,
} from "./project-fixtures";

// ── Operator home session state ──────────────────────────────────

export type OperatorHomeState =
  | "loading"
  | "unpaired"
  | "paired"
  | "error";

// ── Fixture: mixed readiness (ready + needs_access + offline) ────

export const FIXTURE_PROJECTS_MIXED: ProjectEntry[] = [
  {
    id: "proj_my_service",
    name: "my-service",
    event_store_kind: "github",
    event_store_label: "GitHub event store",
    readiness: "ready",
    provider_id: "github",
  },
  {
    id: "proj_data_pipeline",
    name: "data-pipeline",
    event_store_kind: "github",
    event_store_label: "GitHub event store",
    readiness: "needs_access",
    access_failure: "missing_connection",
    provider_id: "github",
  },
  {
    id: "proj_docs_site",
    name: "docs-site",
    event_store_kind: "google_drive",
    event_store_label: "Google Drive",
    readiness: "offline",
    provider_id: "google",
  },
];

// ── Fixture: all projects ready ──────────────────────────────────

export const FIXTURE_PROJECTS_ALL_READY: ProjectEntry[] = [
  {
    id: "proj_ready_1",
    name: "checkout-service",
    event_store_kind: "github",
    event_store_label: "GitHub event store",
    readiness: "ready",
    provider_id: "github",
  },
  {
    id: "proj_ready_2",
    name: "payment-api",
    event_store_kind: "github",
    event_store_label: "GitHub event store",
    readiness: "ready",
    provider_id: "github",
  },
];

// ── Fixture: empty project list ──────────────────────────────────

export const FIXTURE_PROJECTS_EMPTY: ProjectEntry[] = [];

// ── Fixture: all projects inaccessible ───────────────────────────

export const FIXTURE_PROJECTS_ALL_INACCESSIBLE: ProjectEntry[] = [
  {
    id: "proj_broken_1",
    name: "broken-service",
    event_store_kind: "github",
    event_store_label: "GitHub event store",
    readiness: "needs_access",
    access_failure: "token_expired",
    provider_id: "github",
  },
  {
    id: "proj_broken_2",
    name: "offline-docs",
    event_store_kind: "google_drive",
    event_store_label: "Google Drive",
    readiness: "offline",
    provider_id: "google",
  },
];

// ── Fixture: each AccessFailureReason ────────────────────────────

export const FIXTURE_PROJECTS_EACH_FAILURE: ProjectEntry[] = [
  {
    id: "proj_fail_missing_conn",
    name: "needs-connection",
    event_store_kind: "github",
    event_store_label: "GitHub event store",
    readiness: "needs_access",
    access_failure: "missing_connection",
    provider_id: "github",
  },
  {
    id: "proj_fail_missing_resource",
    name: "needs-resource",
    event_store_kind: "github",
    event_store_label: "GitHub event store",
    readiness: "needs_access",
    access_failure: "missing_resource_selection",
    provider_id: "github",
  },
  {
    id: "proj_fail_insufficient",
    name: "wrong-account",
    event_store_kind: "github",
    event_store_label: "GitHub event store",
    readiness: "needs_access",
    access_failure: "insufficient_account_access",
    provider_id: "github",
  },
  {
    id: "proj_fail_token",
    name: "expired-token",
    event_store_kind: "github",
    event_store_label: "GitHub event store",
    readiness: "needs_access",
    access_failure: "token_expired",
    provider_id: "github",
  },
  {
    id: "proj_fail_moved",
    name: "moved-repo",
    event_store_kind: "github",
    event_store_label: "GitHub event store",
    readiness: "needs_access",
    access_failure: "resource_moved_or_deleted",
    provider_id: "github",
  },
];

// ── Fixture: domain-discovery mix (members + available non-members) ──
//
// Exercises the bot#768 grouping: a `Your products` group spanning every
// member readiness (ready / setup-incomplete / repair) plus an `Available for
// <verified-domain>` group of eligible-but-not-joined Products carrying the
// non-canonical Cloud preview that drives per-Product join.

export const FIXTURE_PROJECTS_DOMAIN_MIX: ProjectEntry[] = [
  {
    id: "proj_member_ready",
    name: "checkout-service",
    event_store_kind: "github",
    event_store_label: "GitHub event store",
    readiness: "ready",
    membership: "member",
    provider_id: "github",
  },
  {
    id: "proj_member_setup",
    name: "billing-api",
    event_store_kind: "github",
    event_store_label: "GitHub event store",
    readiness: "ready",
    membership: "member",
    setup_incomplete: true,
    provider_id: "github",
  },
  {
    id: "proj_member_repair",
    name: "data-pipeline",
    event_store_kind: "github",
    event_store_label: "GitHub event store",
    readiness: "needs_access",
    membership: "member",
    access_failure: "token_expired",
    provider_id: "github",
  },
  {
    id: "proj_available_platform",
    name: "platform-core",
    event_store_kind: "github",
    event_store_label: "GitHub event store",
    readiness: "ready",
    membership: "available",
    verified_domain: "acme.example",
    preview: {
      preview_id: "prev_platform_core",
      product_id: "proj_available_platform",
      product_display_name: "platform-core",
      team_display_name: "Acme",
      verified_domain: "acme.example",
      canonical: false,
      expires_at: "2999-01-01T00:00:00Z",
    },
  },
  {
    id: "proj_available_docs",
    name: "docs-portal",
    event_store_kind: "google_drive",
    event_store_label: "Google Drive",
    readiness: "ready",
    membership: "available",
    verified_domain: "acme.example",
    preview: {
      preview_id: "prev_docs_portal",
      product_id: "proj_available_docs",
      product_display_name: "docs-portal",
      team_display_name: "Acme",
      verified_domain: "acme.example",
      canonical: false,
      expires_at: "2999-01-01T00:00:00Z",
    },
  },
];

// ── Aggregate fixture map ────────────────────────────────────────

export const ALL_OPERATOR_HOME_FIXTURES = {
  mixed: FIXTURE_PROJECTS_MIXED,
  all_ready: FIXTURE_PROJECTS_ALL_READY,
  empty: FIXTURE_PROJECTS_EMPTY,
  all_inaccessible: FIXTURE_PROJECTS_ALL_INACCESSIBLE,
  each_failure: FIXTURE_PROJECTS_EACH_FAILURE,
  domain_mix: FIXTURE_PROJECTS_DOMAIN_MIX,
} as const;

// ── Domain-gated discovery copy (bot#768 / PRD) ──────────────────
//
// Product discovery is verified-domain-gated by Cloud: a signed-in operator
// sees the Products their verified identity/domain makes eligible, with no
// user-entered invite code, team id, or approval queue in the primary path. The
// empty state guides the operator to create the first Product or wait for
// same-domain eligibility — it must NOT teach any invite/team-id/approval model.

export const DISCOVERY_EMPTY_TITLE = "No products yet";

export const DISCOVERY_EMPTY_DESCRIPTION =
  "Products appear here automatically once your verified domain makes them " +
  "available to join. Create the first Product to get started, or wait for a " +
  "teammate on your domain to publish one you can discover.";
