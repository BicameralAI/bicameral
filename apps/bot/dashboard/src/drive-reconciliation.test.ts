// Regression suite for bot#1263: after real Google consent the hosted Drive row
// must reconcile to daemon-authored `connected` automatically, survive reload,
// and never need a second Connect click. Browser state can never manufacture
// `connected`: only a ConnectionRegistry projection may.
import { cleanup, fireEvent, render, waitFor, within } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  checkDriveAuthorization,
  readDriveRegistryTruth,
  watchDriveAuthorization,
  type DriveConnectionDeps,
} from "./lib/drive-connection";
import type { ConnectionCompletionOutcome, ConnectionRegistry } from "./lib/types";

const CONNECTED_REGISTRY: ConnectionRegistry = {
  connections: {
    google: {
      provider: "google",
      status: "connected",
      account_label: "kevin@example.com",
      capabilities: [],
    } as ConnectionRegistry["connections"][string],
  },
};
const EMPTY_REGISTRY: ConnectionRegistry = { connections: {} };

function deps(
  outcomes: ConnectionCompletionOutcome[],
  registries: ConnectionRegistry[],
): DriveConnectionDeps {
  return {
    pollStatus: vi.fn(
      async (): Promise<ConnectionCompletionOutcome> =>
        outcomes.shift() ?? { outcome: "pending", provider: "google" },
    ),
    fetchRegistry: vi.fn(
      async (): Promise<ConnectionRegistry> => registries.shift() ?? EMPTY_REGISTRY,
    ),
  };
}

describe("drive-connection controller", () => {
  it("terminal connected still requires the registry to agree", async () => {
    const d = deps([{ outcome: "connected", provider: "google" }], [EMPTY_REGISTRY]);
    expect(await checkDriveAuthorization(d, "s1")).toEqual({ kind: "pending" });
  });

  it("terminal connected + registry connected => connected with daemon label", async () => {
    const d = deps(
      [{ outcome: "connected", provider: "google", account_label: "browser-said" }],
      [CONNECTED_REGISTRY],
    );
    expect(await checkDriveAuthorization(d, "s1")).toEqual({
      kind: "connected",
      accountLabel: "kevin@example.com",
    });
  });

  it("consumed session (expired) reconciles to connected only when registry says so", async () => {
    const yes = deps([{ outcome: "expired", provider: "google", message: "" }], [CONNECTED_REGISTRY]);
    expect((await checkDriveAuthorization(yes, "s1")).kind).toBe("connected");
    const no = deps([{ outcome: "expired", provider: "google", message: "" }], [EMPTY_REGISTRY]);
    expect(await checkDriveAuthorization(no, "s1")).toEqual({ kind: "failed", phase: "expired" });
  });

  it("preserves typed failures", async () => {
    const cases: Array<[ConnectionCompletionOutcome, string]> = [
      [{ outcome: "denied", provider: "google", message: "" }, "denied"],
      [{ outcome: "action_needed", provider: "google", message: "" }, "action_needed"],
      [{ outcome: "provider_unavailable", provider: "google", message: "" }, "provider_unavailable"],
      [{ outcome: "unsupported", provider: "google", message: "" }, "provider_unavailable"],
    ];
    for (const [outcome, phase] of cases) {
      expect(await checkDriveAuthorization(deps([outcome], []), "s")).toEqual({
        kind: "failed",
        phase,
      });
    }
  });

  it("an unreadable registry is unknown, not disconnected", async () => {
    expect(
      await readDriveRegistryTruth(async () => {
        throw new Error("transport");
      }),
    ).toBeNull();
  });

  describe("watcher", () => {
    beforeEach(() => vi.useFakeTimers());
    afterEach(() => vi.useRealTimers());

    it("polls until terminal success and settles once", async () => {
      const d = deps(
        [
          { outcome: "pending", provider: "google" },
          { outcome: "connected", provider: "google" },
        ],
        [CONNECTED_REGISTRY],
      );
      const onsettled = vi.fn();
      watchDriveAuthorization({ deps: d, sessionId: "s", onsettled, intervalMs: 100 });
      await vi.advanceTimersByTimeAsync(100);
      expect(onsettled).not.toHaveBeenCalled();
      await vi.advanceTimersByTimeAsync(100);
      expect(onsettled).toHaveBeenCalledTimes(1);
      expect(onsettled).toHaveBeenCalledWith({ kind: "connected", accountLabel: "kevin@example.com" });
      await vi.advanceTimersByTimeAsync(1000);
      expect(onsettled).toHaveBeenCalledTimes(1);
    });

    it("checkNow reconciles immediately (focus return)", async () => {
      const d = deps([{ outcome: "connected", provider: "google" }], [CONNECTED_REGISTRY]);
      const onsettled = vi.fn();
      const w = watchDriveAuthorization({ deps: d, sessionId: "s", onsettled, intervalMs: 60_000 });
      await w.checkNow();
      expect(onsettled).toHaveBeenCalledWith(expect.objectContaining({ kind: "connected" }));
    });

    it("transient transport errors keep waiting; persistent ones fail typed", async () => {
      const d: DriveConnectionDeps = {
        pollStatus: vi.fn(async () => {
          throw new Error("refused");
        }),
        fetchRegistry: vi.fn(async () => EMPTY_REGISTRY),
      };
      const onsettled = vi.fn();
      watchDriveAuthorization({
        deps: d,
        sessionId: "s",
        onsettled,
        intervalMs: 10,
        maxTransportFailures: 3,
      });
      await vi.advanceTimersByTimeAsync(20);
      expect(onsettled).not.toHaveBeenCalled();
      await vi.advanceTimersByTimeAsync(20);
      expect(onsettled).toHaveBeenCalledWith({ kind: "failed", phase: "provider_unavailable" });
    });

    it("gives up as expired after the timeout", async () => {
      const d = deps([], []);
      const onsettled = vi.fn();
      watchDriveAuthorization({ deps: d, sessionId: "s", onsettled, intervalMs: 50, timeoutMs: 120 });
      await vi.advanceTimersByTimeAsync(200);
      expect(onsettled).toHaveBeenCalledWith({ kind: "failed", phase: "expired" });
    });

    it("stop() cancels without settling", async () => {
      const d = deps([{ outcome: "connected", provider: "google" }], [CONNECTED_REGISTRY]);
      const onsettled = vi.fn();
      const w = watchDriveAuthorization({ deps: d, sessionId: "s", onsettled, intervalMs: 10 });
      w.stop();
      await vi.advanceTimersByTimeAsync(100);
      expect(onsettled).not.toHaveBeenCalled();
      expect(d.pollStatus).not.toHaveBeenCalled();
    });
  });
});

// ── App journeys ───────────────────────────────────────────────────────────

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { "content-type": "application/json" },
  });
}

function setPairedHandoff(): void {
  window.sessionStorage.setItem(
    "bicameral_hosted_bridge",
    JSON.stringify({
      session_token: "bridge-session",
      daemon_url: "http://localhost:37373",
      cloud_account_id: "acct-1",
      cloud_access_token: "cloud-session",
    }),
  );
}

type FakeDaemon = {
  connected: boolean;
  statusOutcome: () => ConnectionCompletionOutcome;
  startCalls: number;
  statusCalls: number;
};

function installFakeDaemon(initialConnected: boolean): FakeDaemon {
  const daemon: FakeDaemon = {
    connected: initialConnected,
    statusOutcome: () => ({ outcome: "pending", provider: "google" }),
    startCalls: 0,
    statusCalls: 0,
  };
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.includes("/connections/google/start")) {
      daemon.startCalls += 1;
      return jsonResponse(
        daemon.connected
          ? { outcome: "connected", provider: "google", account_label: "kevin@example.com" }
          : {
              outcome: "start_url",
              provider: "google",
              url: "https://accounts.google.com/o/oauth2/v2/auth?state=x",
              auth_session_id: "gsess-1",
              flow_type: "local_loopback",
            },
      );
    }
    if (url.includes("/connections/google/status/")) {
      daemon.statusCalls += 1;
      return jsonResponse(daemon.statusOutcome());
    }
    if (url.endsWith("/api/v1/connections")) {
      return jsonResponse(daemon.connected ? CONNECTED_REGISTRY : EMPTY_REGISTRY);
    }
    return jsonResponse({ projects: [] });
  });
  return daemon;
}

type View = { getByTestId: (id: string) => HTMLElement };

function driveRow(view: View) {
  return within(view.getByTestId("settings-drive-oauth"));
}

async function openDriveSettings(view: View) {
  await fireEvent.click(view.getByTestId("empty-create"));
  await waitFor(() => expect(view.getByTestId("settings-drive-oauth")).toBeTruthy());
}

describe("hosted Drive journey (bot#1263)", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    window.sessionStorage.clear();
    window.history.replaceState(null, "", "/");
  });

  it("authorize -> consent completes -> focus return -> Connected, with one Connect click", async () => {
    setPairedHandoff();
    const daemon = installFakeDaemon(false);
    vi.spyOn(window, "open").mockReturnValue(null);
    const App = (await import("./App.svelte")).default;
    const view = render(App);
    await openDriveSettings(view);

    await fireEvent.click(view.getByTestId("drive-oauth-start"));
    await waitFor(() => expect(view.getByTestId("drive-oauth-open")).toBeTruthy());
    await fireEvent.click(view.getByTestId("drive-oauth-open"));
    expect(driveRow(view).getByText("In progress")).toBeTruthy();

    // Google consent completes: the daemon callback consumed the session and
    // wrote the registry.
    daemon.connected = true;
    daemon.statusOutcome = () => ({ outcome: "expired", provider: "google", message: "consumed" });
    window.dispatchEvent(new Event("focus"));

    await waitFor(() => expect(driveRow(view).getByText("Connected")).toBeTruthy());
    expect(driveRow(view).getByText("kevin@example.com")).toBeTruthy();
    expect(daemon.startCalls).toBe(1);
  });

  it("reconciles without focus via polling", async () => {
    setPairedHandoff();
    const daemon = installFakeDaemon(false);
    vi.spyOn(window, "open").mockReturnValue(null);
    const App = (await import("./App.svelte")).default;
    const view = render(App);
    await openDriveSettings(view);
    await fireEvent.click(view.getByTestId("drive-oauth-start"));
    await waitFor(() => expect(view.getByTestId("drive-oauth-open")).toBeTruthy());
    await fireEvent.click(view.getByTestId("drive-oauth-open"));

    daemon.connected = true;
    daemon.statusOutcome = () => ({ outcome: "connected", provider: "google" });
    await waitFor(() => expect(driveRow(view).getByText("Connected")).toBeTruthy(), { timeout: 5000 });
    expect(daemon.statusCalls).toBeGreaterThan(0);
    expect(daemon.startCalls).toBe(1);
  });

  it("reload while already connected renders Connected from the registry", async () => {
    setPairedHandoff();
    const daemon = installFakeDaemon(true);
    const App = (await import("./App.svelte")).default;
    const view = render(App);
    // The host rail reflects daemon truth with no user action at all.
    await waitFor(() =>
      expect(view.getByTestId("host-status-drive").textContent).not.toContain(
        "Drive not connected",
      ),
    );
    await fireEvent.click(view.getByTestId("nav-settings"));
    await waitFor(() => expect(driveRow(view).getByText("Connected")).toBeTruthy());
    expect(daemon.startCalls).toBe(0);
  });

  it("a terminal 'connected' the registry does not confirm never renders Connected", async () => {
    setPairedHandoff();
    const daemon = installFakeDaemon(false);
    vi.spyOn(window, "open").mockReturnValue(null);
    const App = (await import("./App.svelte")).default;
    const view = render(App);
    await openDriveSettings(view);
    await fireEvent.click(view.getByTestId("drive-oauth-start"));
    await waitFor(() => expect(view.getByTestId("drive-oauth-open")).toBeTruthy());
    await fireEvent.click(view.getByTestId("drive-oauth-open"));

    daemon.statusOutcome = () => ({ outcome: "connected", provider: "google" });
    window.dispatchEvent(new Event("focus"));
    await new Promise((r) => setTimeout(r, 50));
    expect(driveRow(view).queryByText("Connected")).toBeNull();
    expect(driveRow(view).getByText("In progress")).toBeTruthy();
  });

  it("production renders no simulated success/failure controls", async () => {
    setPairedHandoff();
    installFakeDaemon(false);
    const App = (await import("./App.svelte")).default;
    const view = render(App);
    await openDriveSettings(view);
    await fireEvent.click(view.getByTestId("drive-oauth-start"));
    await waitFor(() => expect(view.getByTestId("drive-oauth-open")).toBeTruthy());
    expect(view.queryByText("Simulate success")).toBeNull();
    expect(view.queryByText("Deny")).toBeNull();
  });

  it("an expired authorization is a typed, retryable failure", async () => {
    setPairedHandoff();
    const daemon = installFakeDaemon(false);
    vi.spyOn(window, "open").mockReturnValue(null);
    const App = (await import("./App.svelte")).default;
    const view = render(App);
    await openDriveSettings(view);
    await fireEvent.click(view.getByTestId("drive-oauth-start"));
    await waitFor(() => expect(view.getByTestId("drive-oauth-open")).toBeTruthy());
    await fireEvent.click(view.getByTestId("drive-oauth-open"));
    daemon.statusOutcome = () => ({ outcome: "expired", provider: "google", message: "" });
    window.dispatchEvent(new Event("focus"));
    await waitFor(() => expect(driveRow(view).getByText("Session expired")).toBeTruthy());
    expect(view.getByTestId("drive-oauth-retry")).toBeTruthy();
  });

  it("?onboarding=drive lands on the Drive step after pairing", async () => {
    setPairedHandoff();
    installFakeDaemon(false);
    window.history.replaceState(null, "", "/?onboarding=drive");
    const App = (await import("./App.svelte")).default;
    const view = render(App);
    await waitFor(() => expect(view.getByTestId("settings-drive-oauth")).toBeTruthy());
    expect(window.location.search).not.toContain("onboarding");
  });
});

describe("onboarding redirect is bounded (review follow-up)", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    window.sessionStorage.clear();
    window.history.replaceState(null, "", "/");
  });

  it("leaving the Drive step ends onboarding; a later connect does not yank the user", async () => {
    setPairedHandoff();
    const daemon = installFakeDaemon(false);
    window.history.replaceState(null, "", "/?onboarding=drive");
    const App = (await import("./App.svelte")).default;
    const view = render(App);
    await waitFor(() => expect(view.getByTestId("settings-drive-oauth")).toBeTruthy());

    await fireEvent.click(view.getByTestId("nav-products"));
    await fireEvent.click(view.getByTestId("nav-settings"));
    await waitFor(() => expect(view.getByTestId("settings-drive-oauth")).toBeTruthy());

    daemon.connected = true;
    window.dispatchEvent(new Event("focus"));
    await waitFor(() => expect(driveRow(view).getByText("Connected")).toBeTruthy());
    expect(view.getByTestId("settings-drive-oauth")).toBeTruthy();
  });
});
