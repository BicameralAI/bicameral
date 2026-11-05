/**
 * Bounded liveness polling for the paired local daemon (bicameral#52).
 *
 * The hosted app only learned that the daemon stopped when an unrelated request
 * failed, so an idle `/app` kept showing "Connected". This monitor calls a
 * caller-supplied probe on an interval and reports only changes of state.
 *
 * It does not classify transport errors. The probe decides what is up or down;
 * the shared transport in `hosted-daemon-connection.ts` keeps its own rules.
 *
 * - at most one probe in flight; a slow probe is never overlapped
 * - no probing while the tab is hidden; one probe as soon as it is visible again
 * - a probe that throws unexpectedly leaves the last state unchanged
 */

export type LivenessResult = "up" | "down";

export type DaemonLivenessOptions = {
  probe: () => Promise<LivenessResult>;
  /** Called only when the state differs from the one currently shown. */
  onChange: (state: LivenessResult) => void;
  /**
   * What the app currently shows. Other code (the shared transport) can raise
   * a disconnect on its own; comparing each probe with this lets a reachable
   * daemon clear it. Defaults to the monitor's own last reported state.
   */
  current?: () => LivenessResult;
  intervalMs?: number;
  isVisible?: () => boolean;
};

export const DAEMON_LIVENESS_INTERVAL_MS = 5_000;

function documentIsVisible(): boolean {
  return typeof document === "undefined" || document.visibilityState === "visible";
}

/** Starts polling and returns a function that stops it. The start state is "up". */
export function startDaemonLiveness(options: DaemonLivenessOptions): () => void {
  const {
    probe,
    onChange,
    current,
    intervalMs = DAEMON_LIVENESS_INTERVAL_MS,
    isVisible = documentIsVisible,
  } = options;
  let stopped = false;
  let inFlight = false;
  let last: LivenessResult = "up";

  async function tick(): Promise<void> {
    if (stopped || inFlight || !isVisible()) return;
    inFlight = true;
    try {
      const result = await probe();
      if (stopped) return;
      if (result !== (current ? current() : last)) {
        last = result;
        onChange(result);
      }
    } catch {
      // An unexpected probe failure is not evidence either way.
    } finally {
      inFlight = false;
    }
  }

  const timer = setInterval(() => void tick(), intervalMs);
  const onVisibilityChange = () => void tick();
  if (typeof document !== "undefined") {
    document.addEventListener("visibilitychange", onVisibilityChange);
  }

  return () => {
    stopped = true;
    clearInterval(timer);
    if (typeof document !== "undefined") {
      document.removeEventListener("visibilitychange", onVisibilityChange);
    }
  };
}
