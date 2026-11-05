/**
 * Cloud project discovery consumer (bot#720, extended for cloud#95).
 *
 * The hosted SPA renders its project list from the Bicameral Cloud project
 * discovery API (`GET /api/v1/projects`, cloud#65) — the coordination/projection
 * surface defined by the cloud#64 contract
 * (`docs/specs/hosted-project-discovery-contract.md`).
 *
 * cloud#95 (Project Discovery PRD) makes discovery **domain/team-gated**: a
 * signed-in operator sees the Projects their verified Supabase identity / email
 * domain / team eligibility makes discoverable, with NO user-entered
 * `project_id + join_code` in the default path. Invite codes are a deprecated
 * manual fallback, not the PRD discovery/conformance path. The optional
 * `discovery_eligibility` field mirrors how Cloud made a row discoverable
 * (domain, team, or explicit membership) so the UI can reflect the domain-gated
 * model without re-deriving eligibility client-side. This module owns:
 *
 *   1. the TypeScript mirror of the Cloud `ProjectListItem` / `ProjectListResponse`
 *      shapes and their enums (kept in lockstep with
 *      `bicameral-cloud/src/bicameral_cloud/models/discovery.py`);
 *   2. mapping a Cloud `ProjectListItem` to the UI `ProjectEntry` the operator
 *      home renders; and
 *   3. `loadCloudProjects`, the orchestrator the hosted runtime calls.
 *
 * Boundary (ADR-0005 / ADR-0006): Cloud discovery is coordination-only. The
 * local daemon remains authoritative for canonical membership, project
 * registration, workspace binding, and projection truth. A narrow
 * paired-daemon replay reconciles membership for Cloud-discovered Product ids;
 * daemon workspace data is consumed ONLY after selection (see
 * `HostedProjectShell`) and never populates the discovery list. The Cloud
 * response deliberately omits provider/remote and
 * event-store detail (`FORBIDDEN_DISCOVERY_FIELDS`), so the rendered rows carry
 * a neutral coordination label rather than a fabricated provider surface.
 *
 * This module carries NO fixture data and must not import the local dev
 * fixtures; the production hosted project list comes exclusively from the live
 * Cloud API response mapped here.
 */

import type { ProjectEntry, ProjectReadiness } from "./project-fixtures";
import type { HostedProjectPreview } from "./onboarding-contract";
import {
  fetchCloudProjects,
  type HostedProductMembershipProjection,
} from "./api";

// ── Cloud discovery contract mirror (cloud#64) ─────────────────────

/** Mirrors cloud `ProjectPairingStatus` (models/discovery.py). */
export type CloudProjectPairingStatus = "unpaired" | "unavailable" | "paired";

/** Mirrors cloud `TeamRole`. */
export type CloudTeamRole = "team_owner" | "team_member";

/**
 * Mirrors cloud `DiscoveryEligibility` (cloud#95). How Cloud made a Project
 * discoverable for the signed-in operator via server-side domain/team gating:
 *
 *   - `domain` — discoverable through the operator's verified email domain;
 *   - `team`   — discoverable through team membership/eligibility;
 *   - `membership` — an explicit project membership record.
 *
 * This is a coordination projection only — it is NOT a join code and never
 * carries local paths, secrets, or authority-bearing fields.
 */
export type CloudDiscoveryEligibility = "domain" | "team" | "membership";

/**
 * Mirrors cloud `BindingReadiness`. Coarse readiness composing pairing status,
 * the compatibility envelope, and daemon-reported workspace readiness. It is
 * NOT a compliance or signoff state.
 */
export type CloudBindingReadiness =
  | "unpaired"
  | "upgrade_required"
  | "unbound"
  | "degraded"
  | "ready"
  | "unknown";

/** Mirrors cloud `ProjectListItem`. Safe coordination subset only. */
export type CloudProjectListItem = {
  project_id: string;
  team_id: string;
  display_name: string;
  membership_role?: CloudTeamRole;
  pairing_status: CloudProjectPairingStatus;
  binding_readiness: CloudBindingReadiness;
  daemon_compatible: boolean;
  /**
   * How Cloud made this row discoverable (cloud#95 domain-gated discovery).
   * Optional so the contract stays compatible while cloud#95 lands; when absent
   * the row is treated as discovered without a specific eligibility hint.
   */
  discovery_eligibility?: CloudDiscoveryEligibility;
  /**
   * Whether the daemon projects the operator as a canonical Product Member
   * (bot#768). Membership is daemon-authoritative; Cloud echoes the projected
   * value only so the UI can group rows. Absent/`true` → member (the pre-#768
   * default); explicit `false` → an eligible same-domain row the operator may
   * join. A directory row is never treated as membership.
   */
  is_member?: boolean;
  /** Verified email/identity domain gating an available same-domain row. */
  verified_domain?: string;
  /**
   * Non-canonical Cloud directory projection for an available (non-member) row.
   * Carries the narrow preview the SPA exchanges for a `ProductJoinTicket` on
   * Join. Present only for `is_member === false` rows.
   */
  preview?: HostedProjectPreview;
  /**
   * Whether local registration/provider setup is incomplete for a member row
   * (drives `Continue setup`). Local routing state, not canonical membership.
   */
  setup_incomplete?: boolean;
};

/** Mirrors cloud `ProjectListResponse`. */
export type CloudProjectListResponse = {
  projects: CloudProjectListItem[];
  selected_project_id?: string | null;
};

/** Result of a discovery load: the mapped list plus an optional error message. */
export type CloudProjectDiscoveryResult = {
  projects: ProjectEntry[];
  selectedProjectId: string | null;
  error?: string;
};

// ── Mapping to the UI ProjectEntry shape ───────────────────────────

/**
 * Compose the UI readiness from the Cloud pairing status and binding readiness.
 *
 * The UI `ProjectReadiness` has three buckets; the discovery contract's finer
 * `BindingReadiness` folds into them so the operator home can render a
 * selectable / repairable / offline row:
 *
 *   - `ready` / `degraded` → `ready` (bound; reads/actions enabled, degraded warns
 *     later at the projection layer);
 *   - `unbound` / `upgrade_required` → `needs_access` (offer bind / upgrade repair);
 *   - `unpaired` / `unknown` (or a non-paired pairing status) → `offline` (neutral;
 *     resolves on pairing/selection).
 */
export function resolveCloudReadiness(
  pairingStatus: CloudProjectPairingStatus,
  bindingReadiness: CloudBindingReadiness,
): ProjectReadiness {
  if (pairingStatus === "unpaired") return "offline";
  if (bindingReadiness === "ready" || bindingReadiness === "degraded") {
    return "ready";
  }
  if (bindingReadiness === "unbound" || bindingReadiness === "upgrade_required") {
    return "needs_access";
  }
  return "offline";
}

/**
 * Map a single Cloud `ProjectListItem` to the UI `ProjectEntry`.
 *
 * The Cloud record excludes provider/remote/event-store detail by contract, so
 * the rendered row uses a neutral coordination label and no `access_failure`
 * (repair intent is deferred to selection / cloud#66). `event_store_kind`
 * carries a neutral placeholder because the UI type requires one and the value
 * is not rendered (only `event_store_label` is shown).
 */
export function mapCloudProjectItem(item: CloudProjectListItem): ProjectEntry {
  const readiness = resolveCloudReadiness(
    item.pairing_status,
    item.binding_readiness,
  );
  const available = item.is_member === false;
  return {
    id: item.project_id,
    name: item.display_name,
    event_store_kind: "github",
    // Only `upgrade_required` is Cloud's evaluated "daemon too old" verdict.
    // `daemon_compatible: false` also covers rows Cloud cannot evaluate (a
    // directory row with no paired daemon), which is "unknown", not a reason to
    // tell the operator to update (bot#1347).
    event_store_label:
      item.binding_readiness === "upgrade_required"
        ? "Cloud project (update the local daemon)"
        : "Cloud project",
    readiness,
    team_id: item.team_id,
    eligibility: item.discovery_eligibility,
    ...(available ? { membership: "available" as const } : {}),
    ...(item.verified_domain ? { verified_domain: item.verified_domain } : {}),
    ...(available && item.preview ? { preview: item.preview } : {}),
    ...(item.setup_incomplete ? { setup_incomplete: true } : {}),
  };
}

/** Map a full Cloud discovery response to UI `ProjectEntry[]`. */
export function mapCloudProjects(
  response: CloudProjectListResponse,
): ProjectEntry[] {
  return (response.projects ?? []).map(mapCloudProjectItem);
}

/**
 * Reconcile Cloud eligibility rows with daemon-canonical membership replay.
 *
 * Cloud still decides which Products are discoverable. The daemon projection
 * can only promote an already-discovered row from available to member and
 * report whether local setup is incomplete; it cannot add a row or alter
 * Cloud tenancy/eligibility metadata.
 */
export function applyDaemonMembershipProjection(
  projects: ProjectEntry[],
  memberships: HostedProductMembershipProjection[],
): ProjectEntry[] {
  const byProductId = new Map(
    memberships.map((membership) => [membership.product_id, membership]),
  );
  return projects.map((project) => {
    const canonical = byProductId.get(project.id);
    if (!canonical) return project;
    const { membership: _membership, preview: _preview, ...member } = project;
    return {
      ...member,
      setup_incomplete: canonical.setup_incomplete,
    };
  });
}

// ── Orchestrator ───────────────────────────────────────────────────

export type LoadCloudProjectsOptions = {
  /** Supabase access token carried by the hosted handoff. */
  accessToken: string;
  /** Cloud API origin; "" (default) is same-origin, where Cloud serves the SPA. */
  apiBase?: string;
  /** Optional SPA-session selected project hint echoed back by the API. */
  selectedProjectId?: string;
  signal?: AbortSignal;
};

/**
 * Load and map the hosted project list from Cloud discovery. Never throws:
 * network/HTTP failures resolve to an empty list plus an `error` message so the
 * caller can render a neutral state without falling back to fixtures or to
 * daemon-only workspace discovery.
 */
export async function loadCloudProjects(
  opts: LoadCloudProjectsOptions,
): Promise<CloudProjectDiscoveryResult> {
  try {
    const response = await fetchCloudProjects(opts);
    return {
      projects: mapCloudProjects(response),
      selectedProjectId: response.selected_project_id ?? null,
    };
  } catch (e: unknown) {
    return {
      projects: [],
      selectedProjectId: null,
      error:
        e instanceof Error
          ? e.message
          : "Could not reach the Bicameral Cloud project discovery service.",
    };
  }
}
