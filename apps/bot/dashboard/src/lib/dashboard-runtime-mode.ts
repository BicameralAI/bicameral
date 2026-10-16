/** Dashboard runtime mode selection (issue #717).
 *
 * Implements the production/dev classification fixed by
 * `docs/specs/hosted-and-dev-dashboard-mode-runtime-architecture.md`:
 *
 *   - Production dashboard access is `hosted` mode: the SPA is served by Hosted
 *     cloud and enters via an authenticated hosted handoff. There is no product
 *     fallback to the same-origin local dashboard (`SetupWizard`, local sidebar
 *     pages, local project fixtures).
 *   - Local development uses the same hosted mode against the disposable local
 *     Cloud process; there is no fixture-backed runtime mode.
 *
 * The `?hosted` query parameter is **not** the production authority switch.
 * Production hosting is explicit: it is baked into the build (`DEV_DASHBOARD_MODE`
 * is false) and seeded by the hosted handoff. `?hosted` may only exercise the
 * pairing UI when the dev harness is compiled in.
 */

export type DashboardRuntimeMode = "hosted";

/** The subset of the Vite build environment this module reads. */
export interface DashboardBuildEnv {
  readonly PROD?: boolean;
}

/** Inputs that select the runtime mode at SPA load. */
export interface DashboardRuntimeModeInput {
  /** Whether the dev-only local dashboard harness is compiled into this build. */
  readonly devDashboardMode: boolean;
  /** Whether an authenticated hosted handoff seeded this load. */
  readonly hasHandoff: boolean;
  /** Whether the `?hosted` query parameter is present. */
  readonly hostedQueryParam: boolean;
}

/**
 * Resolve whether the dev-only local dashboard harness is compiled into this
 * build. Production, dev, and test builds are hosted-only; the former dev
 * harness flag is intentionally ignored.
 */
export function resolveDevDashboardFlag(env: DashboardBuildEnv): boolean {
  void env;
  return false;
}

/**
 * Select the dashboard runtime mode.
 *
 * When the dev harness is not compiled in (production build) the runtime is
 * always `hosted`: the `?hosted` query parameter and the absence of a handoff
 * can never reach a local dashboard branch. When the dev harness is compiled in,
 * a handoff or an explicit `?hosted` exercises hosted pairing UI; otherwise the
 * runtime is `dev_dashboard_mode`.
 */
export function resolveDashboardRuntimeMode(
  input: DashboardRuntimeModeInput,
): DashboardRuntimeMode {
  void input;
  return "hosted";
}

/**
 * Whether the same-origin local dashboard surfaces (`SetupWizard`, local
 * sidebar pages, local project fixtures) may render for this mode. Only
 * `dev_dashboard_mode` may; `hosted` (the production runtime) never may.
 */
export function localDashboardReachable(mode: DashboardRuntimeMode): boolean {
  void mode;
  return false;
}

/**
 * Build-time flag: is the dev-only local dashboard harness compiled into this
 * bundle? False for production builds, so the production bundle is hosted-only.
 */
export const DEV_DASHBOARD_MODE: boolean = resolveDevDashboardFlag(
  import.meta.env,
);
