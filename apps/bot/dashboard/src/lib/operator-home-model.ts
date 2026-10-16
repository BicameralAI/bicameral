/**
 * Operator Home view-model (bot#768): groups discovered Product rows into
 * `Your products` (canonical members) and `Available for <verified-domain>`
 * (eligible-but-not-joined), and derives the single primary row action.
 *
 * These are pure projections over `ProjectEntry` — grouping and actions are UI
 * facts, never Product authority. Membership, readiness, registration, and
 * workspace binding are orthogonal (contract § Lifecycle Minimization); this
 * module only reads them.
 */

import type { ProjectEntry } from "./project-fixtures";

/** The single primary action a row exposes. `null` = no action (e.g. offline). */
export type RowAction = "open" | "continue_setup" | "repair" | "join" | null;

export const ROW_ACTION_LABEL: Record<Exclude<RowAction, null>, string> = {
  open: "Open",
  continue_setup: "Continue setup",
  repair: "Repair",
  join: "Join",
};

/** Whether a row is a canonical member (`Your products`) vs merely available. */
export function isMember(project: ProjectEntry): boolean {
  return project.membership !== "available";
}

/**
 * Derive the row's primary action.
 *
 *  - available (eligible, not joined) → `join`
 *  - member + ready + setup complete  → `open`
 *  - member + ready + setup incomplete → `continue_setup`
 *  - member + needs_access            → `repair`
 *  - member + offline                 → none
 */
export function rowAction(project: ProjectEntry): RowAction {
  if (!isMember(project)) return "join";
  if (project.readiness === "ready") {
    return project.setup_incomplete ? "continue_setup" : "open";
  }
  if (project.readiness === "needs_access") return "repair";
  return null;
}

export type OperatorHomeGroups = {
  members: ProjectEntry[];
  available: ProjectEntry[];
};

/** Split rows into the two Operator Home groups, preserving input order. */
export function groupRows(projects: ProjectEntry[]): OperatorHomeGroups {
  const members: ProjectEntry[] = [];
  const available: ProjectEntry[] = [];
  for (const project of projects) {
    (isMember(project) ? members : available).push(project);
  }
  return { members, available };
}

export const YOUR_PRODUCTS_HEADING = "Your products";

/**
 * Heading for the available group. Uses the shared verified domain when every
 * available row agrees on one; otherwise falls back to a neutral phrase. Never
 * exposes governance/bridge language.
 */
export function availableHeading(available: ProjectEntry[]): string {
  const domains = new Set(
    available
      .map((p) => p.verified_domain)
      .filter((d): d is string => typeof d === "string" && d.length > 0),
  );
  if (domains.size === 1) {
    return `Available for ${[...domains][0]}`;
  }
  return "Available to join";
}
