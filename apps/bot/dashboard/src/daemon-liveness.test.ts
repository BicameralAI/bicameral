/**
 * Daemon liveness polling (bicameral#52, bot#1346).
 *
 * The hosted app used to learn that the local daemon stopped only when some
 * unrelated request failed, so an idle `/app` stayed "Connected" indefinitely.
 * The monitor probes the daemon on a bounded interval and reports only changes
 * in state. It owns no transport classification: the probe decides what counts
 * as up or down.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  startDaemonLiveness,
  type LivenessResult,
} from "./lib/daemon-liveness";

const INTERVAL = 5_000;

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

describe("startDaemonLiveness", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("probes on every interval and reports only changes of state", async () => {
    const results: LivenessResult[] = ["up", "down", "down", "up"];
    const probe = vi.fn(async () => results.shift() ?? "up");
    const onChange = vi.fn();
    const stop = startDaemonLiveness({
      probe,
      onChange,
      intervalMs: INTERVAL,
      isVisible: () => true,
    });

    await vi.advanceTimersByTimeAsync(INTERVAL); // up: no change from the assumed start
    expect(onChange).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(INTERVAL); // down
    expect(onChange).toHaveBeenLastCalledWith("down");
    await vi.advanceTimersByTimeAsync(INTERVAL); // down again: no repeat
    expect(onChange).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(INTERVAL); // up: recovery
    expect(onChange).toHaveBeenLastCalledWith("up");
    expect(onChange).toHaveBeenCalledTimes(2);
    expect(probe).toHaveBeenCalledTimes(4);
    stop();
  });

  it("detects a stop without any other request or page reload", async () => {
    let daemonUp = true;
    const onChange = vi.fn();
    const stop = startDaemonLiveness({
      probe: async () => (daemonUp ? "up" : "down"),
      onChange,
      intervalMs: INTERVAL,
      isVisible: () => true,
    });
    await vi.advanceTimersByTimeAsync(INTERVAL * 2);
    expect(onChange).not.toHaveBeenCalled();

    daemonUp = false;
    await vi.advanceTimersByTimeAsync(INTERVAL);
    expect(onChange).toHaveBeenCalledExactlyOnceWith("down");
    stop();
  });

  it("never runs two probes at once", async () => {
    const first = deferred<LivenessResult>();
    const probe = vi.fn(() => first.promise);
    const stop = startDaemonLiveness({
      probe,
      onChange: vi.fn(),
      intervalMs: INTERVAL,
      isVisible: () => true,
    });

    await vi.advanceTimersByTimeAsync(INTERVAL * 3);
    expect(probe).toHaveBeenCalledTimes(1);

    first.resolve("up");
    await vi.advanceTimersByTimeAsync(INTERVAL);
    expect(probe).toHaveBeenCalledTimes(2);
    stop();
  });

  it("does not probe while the tab is hidden, and probes as soon as it is visible again", async () => {
    let visible = false;
    const probe = vi.fn(async () => "up" as LivenessResult);
    const stop = startDaemonLiveness({
      probe,
      onChange: vi.fn(),
      intervalMs: INTERVAL,
      isVisible: () => visible,
    });

    await vi.advanceTimersByTimeAsync(INTERVAL * 3);
    expect(probe).not.toHaveBeenCalled();

    visible = true;
    document.dispatchEvent(new Event("visibilitychange"));
    await vi.advanceTimersByTimeAsync(0);
    expect(probe).toHaveBeenCalledTimes(1);
    stop();
  });

  it("stops cleanly: no more probes, and a late result is ignored", async () => {
    const late = deferred<LivenessResult>();
    const probe = vi.fn(() => late.promise);
    const onChange = vi.fn();
    const stop = startDaemonLiveness({
      probe,
      onChange,
      intervalMs: INTERVAL,
      isVisible: () => true,
    });

    await vi.advanceTimersByTimeAsync(INTERVAL);
    expect(probe).toHaveBeenCalledTimes(1);
    stop();
    late.resolve("down");
    await vi.advanceTimersByTimeAsync(INTERVAL * 3);
    expect(onChange).not.toHaveBeenCalled();
    expect(probe).toHaveBeenCalledTimes(1);
  });

  it("clears a disconnect that something else raised once the daemon answers", async () => {
    // The shared transport can raise the disconnect dialog from any failing
    // request. The monitor compares each probe with what the app currently
    // shows, so a daemon that is in fact reachable clears the dialog.
    let shownAs: LivenessResult = "down";
    const onChange = vi.fn((state: LivenessResult) => {
      shownAs = state;
    });
    const stop = startDaemonLiveness({
      probe: async () => "up",
      onChange,
      current: () => shownAs,
      intervalMs: INTERVAL,
      isVisible: () => true,
    });

    await vi.advanceTimersByTimeAsync(INTERVAL);
    expect(onChange).toHaveBeenCalledExactlyOnceWith("up");
    await vi.advanceTimersByTimeAsync(INTERVAL);
    expect(onChange).toHaveBeenCalledTimes(1);
    stop();
  });

  it("keeps its state when a probe throws unexpectedly", async () => {
    const probe = vi
      .fn<() => Promise<LivenessResult>>()
      .mockRejectedValueOnce(new Error("boom"))
      .mockResolvedValue("up");
    const onChange = vi.fn();
    const stop = startDaemonLiveness({
      probe,
      onChange,
      intervalMs: INTERVAL,
      isVisible: () => true,
    });

    await vi.advanceTimersByTimeAsync(INTERVAL * 2);
    expect(onChange).not.toHaveBeenCalled();
    expect(probe).toHaveBeenCalledTimes(2);
    stop();
  });
});
