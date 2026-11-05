/**
 * Classification used by the liveness poll (bicameral#52, bot#1346).
 *
 * Only two things count as the daemon being gone: the request never reaching
 * it (a transport failure), and the paired session being refused with 401 or
 * 410. A reachable daemon that answers with anything else (a 403, a 5xx, a
 * typed body) is not evidence that it stopped, so the probe is inconclusive and
 * the displayed state stays as it is.
 */

import { afterEach, describe, expect, it, vi } from "vitest";
import { probeDaemonStatus } from "./lib/daemon-status-probe";

afterEach(() => {
  vi.restoreAllMocks();
});

function respondWith(status: number, body: unknown = {}) {
  vi.spyOn(globalThis, "fetch").mockResolvedValue(
    new Response(JSON.stringify(body), {
      status,
      headers: { "content-type": "application/json" },
    }),
  );
}

describe("probeDaemonStatus", () => {
  it("is up when the status route answers 200 for the paired session", async () => {
    respondWith(200, { pairing_state: "hosted_bridge_paired" });
    await expect(probeDaemonStatus("session-token")).resolves.toBe("up");
  });

  it("sends the bridge session token to the existing status route only", async () => {
    respondWith(200, { pairing_state: "hosted_bridge_paired" });
    await probeDaemonStatus("session-token");
    const [url, init] = vi.mocked(globalThis.fetch).mock.calls[0];
    expect(String(url)).toContain("/api/v1/hosted-bridge/status");
    expect(String(url)).not.toContain("/health");
    expect(new Headers((init as RequestInit).headers).get("x-bridge-session-token")).toBe(
      "session-token",
    );
  });

  it("is down when the request cannot reach the daemon", async () => {
    vi.spyOn(globalThis, "fetch").mockRejectedValue(new TypeError("Failed to fetch"));
    await expect(probeDaemonStatus("session-token")).resolves.toBe("down");
  });

  it.each([401, 410])("is down when the session is refused with %i", async (status) => {
    respondWith(status, { error: "session gone" });
    await expect(probeDaemonStatus("session-token")).resolves.toBe("down");
  });

  it.each([403, 404, 500, 502, 503])(
    "is inconclusive, not down, for a reachable daemon that answers %i",
    async (status) => {
      respondWith(status, { error: "typed_rejection" });
      await expect(probeDaemonStatus("session-token")).rejects.toThrow(/inconclusive/);
    },
  );

  it("is inconclusive when the request was aborted", async () => {
    vi.spyOn(globalThis, "fetch").mockRejectedValue(
      new DOMException("aborted", "AbortError"),
    );
    await expect(probeDaemonStatus("session-token")).rejects.toThrow();
  });
});
