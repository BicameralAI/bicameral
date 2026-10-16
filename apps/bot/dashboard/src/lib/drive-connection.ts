/**
 * Google Drive connection reconciliation for the hosted SPA (bot#1263, #1072).
 *
 * The daemon's `ConnectionRegistry` is the only connection truth. The browser
 * starts the daemon-owned authorization, opens the daemon-provided Google URL,
 * then *observes* the outcome: it polls the typed completion endpoint and, on a
 * terminal `connected`, re-reads the registry. Only a registry projection that
 * says `connected` may render Connected. Browser phase is presentation only.
 */
import type {
  ConnectionCompletionOutcome,
  ConnectionRegistry,
} from "./types";

/** Presentation phase for the Drive row. Never persisted, never authority. */
export type DrivePhase =
  | "idle"
  | "starting"
  | "authorize"
  | "waiting"
  | "connected"
  | "denied"
  | "expired"
  | "action_needed"
  | "provider_unavailable";

/** Failure phases a settled authorization can land in. */
export type DriveFailurePhase =
  | "denied"
  | "expired"
  | "action_needed"
  | "provider_unavailable";

export type DriveRegistryTruth = {
  connected: boolean;
  accountLabel: string;
};

/** Result of one reconciliation check while authorization is pending. */
export type DriveAuthorizationStep =
  | { kind: "pending" }
  | { kind: "connected"; accountLabel: string }
  | { kind: "failed"; phase: DriveFailurePhase };

export type DriveConnectionDeps = {
  pollStatus: (sessionId: string) => Promise<ConnectionCompletionOutcome>;
  fetchRegistry: () => Promise<ConnectionRegistry>;
};

/**
 * Read daemon-authored Google connection truth. Returns `null` when the
 * registry cannot be read, so callers keep their current state rather than
 * guessing either way.
 */
export async function readDriveRegistryTruth(
  fetchRegistry: () => Promise<ConnectionRegistry>,
): Promise<DriveRegistryTruth | null> {
  try {
    const registry = await fetchRegistry();
    const google = registry.connections?.google;
    return {
      connected: google?.status === "connected",
      accountLabel: google?.account_label ?? "",
    };
  } catch {
    return null;
  }
}

/**
 * One reconciliation check. A terminal `connected` completion is not enough on
 * its own: the registry must agree before the caller may render Connected. A
 * transport error or a registry that does not (yet) agree stays `pending`.
 */
export async function checkDriveAuthorization(
  deps: DriveConnectionDeps,
  sessionId: string,
): Promise<DriveAuthorizationStep> {
  const outcome = await deps.pollStatus(sessionId);
  switch (outcome.outcome) {
    case "connected": {
      const truth = await readDriveRegistryTruth(deps.fetchRegistry);
      if (truth?.connected) {
        return { kind: "connected", accountLabel: truth.accountLabel };
      }
      return { kind: "pending" };
    }
    case "pending":
      return { kind: "pending" };
    case "denied":
      return { kind: "failed", phase: "denied" };
    case "expired": {
      // The daemon consumes the auth session on callback. If the registry says
      // connected, the authorization completed; otherwise it truly expired.
      const truth = await readDriveRegistryTruth(deps.fetchRegistry);
      if (truth?.connected) {
        return { kind: "connected", accountLabel: truth.accountLabel };
      }
      return { kind: "failed", phase: "expired" };
    }
    case "action_needed":
      return { kind: "failed", phase: "action_needed" };
    default:
      return { kind: "failed", phase: "provider_unavailable" };
  }
}

export type DriveAuthorizationWatcher = {
  /** Check now (e.g. the user returned from the Google tab). */
  checkNow: () => Promise<void>;
  stop: () => void;
};

export type DriveAuthorizationWatcherOptions = {
  deps: DriveConnectionDeps;
  sessionId: string;
  onsettled: (step: Exclude<DriveAuthorizationStep, { kind: "pending" }>) => void;
  intervalMs?: number;
  /** Give up and report `expired` after this long without a terminal result. */
  timeoutMs?: number;
  /** Consecutive transport failures before reporting `provider_unavailable`. */
  maxTransportFailures?: number;
  now?: () => number;
};

export const DRIVE_POLL_INTERVAL_MS = 2000;
export const DRIVE_AUTH_TIMEOUT_MS = 10 * 60 * 1000;
export const DRIVE_MAX_TRANSPORT_FAILURES = 5;

/**
 * Observe a pending Google authorization until the daemon reports a terminal
 * outcome. Settles exactly once; `stop()` cancels without settling.
 */
export function watchDriveAuthorization(
  options: DriveAuthorizationWatcherOptions,
): DriveAuthorizationWatcher {
  const intervalMs = options.intervalMs ?? DRIVE_POLL_INTERVAL_MS;
  const timeoutMs = options.timeoutMs ?? DRIVE_AUTH_TIMEOUT_MS;
  const maxFailures = options.maxTransportFailures ?? DRIVE_MAX_TRANSPORT_FAILURES;
  const now = options.now ?? (() => Date.now());
  const deadline = now() + timeoutMs;
  let done = false;
  let inFlight = false;
  let failures = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;

  function settle(step: Exclude<DriveAuthorizationStep, { kind: "pending" }>) {
    if (done) return;
    stop();
    options.onsettled(step);
  }

  function schedule() {
    if (done) return;
    if (timer !== null) clearTimeout(timer);
    timer = setTimeout(() => {
      timer = null;
      void checkNow();
    }, intervalMs);
  }

  async function checkNow(): Promise<void> {
    if (done || inFlight) return;
    inFlight = true;
    let step: DriveAuthorizationStep;
    try {
      step = await checkDriveAuthorization(options.deps, options.sessionId);
      failures = 0;
    } catch {
      failures += 1;
      step =
        failures >= maxFailures
          ? { kind: "failed", phase: "provider_unavailable" }
          : { kind: "pending" };
    } finally {
      inFlight = false;
    }
    if (done) return;
    if (step.kind !== "pending") {
      settle(step);
      return;
    }
    if (now() >= deadline) {
      settle({ kind: "failed", phase: "expired" });
      return;
    }
    schedule();
  }

  function stop() {
    done = true;
    if (timer !== null) clearTimeout(timer);
    timer = null;
  }

  schedule();
  return { checkNow, stop };
}
