import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { startDaemonLiveness, type LivenessResult } from "./lib/daemon-liveness";
import { probeDaemonStatus } from "./lib/daemon-status-probe";
import { onHostedDaemonDisconnected } from "./lib/hosted-daemon-connection";

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});

it.each([200, 401, 410, 503])("only the active monitor projects a %i poll", async (status) => {
  vi.spyOn(globalThis, "fetch").mockResolvedValue(
    new Response(JSON.stringify({ error: "typed_rejection" }), { status }),
  );
  let displayed: LivenessResult = "up";
  const disconnected = vi.fn(() => { displayed = "down"; });
  const unsubscribe = onHostedDaemonDisconnected(disconnected);
  const stop = startDaemonLiveness({
    probe: () => probeDaemonStatus("synthetic-session"),
    current: () => displayed,
    onChange: (state) => { displayed = state; },
    isVisible: () => true,
  });
  try {
    await vi.advanceTimersByTimeAsync(5_000);
    expect(displayed).toBe(status === 401 || status === 410 ? "down" : "up");
    expect(disconnected).not.toHaveBeenCalled();
  } finally {
    stop();
    unsubscribe();
  }
});

it.each([401, 410, 503])("a stopped monitor's late %i does not notify a new session", async (status) => {
  let resolve!: (response: Response) => void;
  vi.spyOn(globalThis, "fetch").mockReturnValue(
    new Promise<Response>((r) => { resolve = r; }),
  );
  const disconnected = vi.fn();
  const onChange = vi.fn();
  const unsubscribe = onHostedDaemonDisconnected(disconnected);
  const stop = startDaemonLiveness({
    probe: () => probeDaemonStatus("old-synthetic-session"),
    onChange,
    isVisible: () => true,
  });
  try {
    await vi.advanceTimersByTimeAsync(5_000);
    stop();
    resolve(new Response("old session result", { status }));
    await vi.advanceTimersByTimeAsync(0);
    expect(disconnected).not.toHaveBeenCalled();
    expect(onChange).not.toHaveBeenCalled();
  } finally {
    stop();
    unsubscribe();
  }
});
