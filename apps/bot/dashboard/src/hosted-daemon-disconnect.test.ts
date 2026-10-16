import { cleanup, fireEvent, render, waitFor } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  fetchFromHostedDaemon,
  onHostedDaemonDisconnected,
} from "./lib/hosted-daemon-connection";
import { fetchCloudProjects, fetchHostedLedgerProjection } from "./lib/api";
import LocalDaemonDisconnectedDialog from "./pages/LocalDaemonDisconnectedDialog.svelte";
import apiSource from "./lib/api.ts?raw";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  window.sessionStorage.clear();
});

describe("Local daemon disconnected dialog", () => {
  it("blocks the current screen with the approved one-action recovery", async () => {
    const onpairing = vi.fn();
    const { getByRole, getByText, queryByRole } = render(
      LocalDaemonDisconnectedDialog,
      { props: { onpairing } },
    );

    const dialog = getByRole("dialog", { name: "Local daemon disconnected" });
    expect(dialog.getAttribute("aria-modal")).toBe("true");
    expect(
      getByText(
        "Bicameral can’t reach your local daemon. Reconnect this device to continue.",
      ),
    ).toBeTruthy();

    const action = getByRole("button", { name: "Reconnect device" });
    await waitFor(() => expect(document.activeElement).toBe(action));
    expect(queryByRole("button", { name: /close|cancel|dismiss/i })).toBeNull();

    await fireEvent.keyDown(window, { key: "Escape" });
    expect(onpairing).not.toHaveBeenCalled();

    await fireEvent.click(action);
    expect(onpairing).toHaveBeenCalledTimes(1);
  });
});

describe("Hosted daemon connection boundary", () => {
  it("notifies every subscriber when the browser cannot reach the daemon", async () => {
    const first = vi.fn();
    const second = vi.fn();
    const unsubscribeFirst = onHostedDaemonDisconnected(first);
    const unsubscribeSecond = onHostedDaemonDisconnected(second);
    vi.spyOn(globalThis, "fetch").mockRejectedValue(
      new TypeError("Failed to fetch"),
    );

    await expect(fetchFromHostedDaemon("http://localhost:37373/health")).rejects.toThrow(
      "Failed to fetch",
    );
    expect(first).toHaveBeenCalledOnce();
    expect(second).toHaveBeenCalledOnce();

    unsubscribeFirst();
    unsubscribeSecond();
  });

  it("notifies when the local daemon is temporarily unavailable", async () => {
    const disconnected = vi.fn();
    const unsubscribe = onHostedDaemonDisconnected(disconnected);
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response("unavailable", { status: 503 }),
    );

    const response = await fetchFromHostedDaemon(
      "http://localhost:37373/api/v1/hosted-bridge/projection/ledger",
    );
    expect(response.status).toBe(503);
    expect(disconnected).toHaveBeenCalledWith({
      kind: "daemon_unavailable",
      status: 503,
    });

    unsubscribe();
  });

  it("does not turn a typed daemon domain rejection into a disconnect", async () => {
    const disconnected = vi.fn();
    const unsubscribe = onHostedDaemonDisconnected(disconnected);
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({ kind: "wrong_domain" }), {
        status: 400,
        headers: { "content-type": "application/json" },
      }),
    );

    const response = await fetchFromHostedDaemon(
      "http://localhost:37373/api/v1/hosted-bridge/product-join",
    );
    expect(response.status).toBe(400);
    expect(disconnected).not.toHaveBeenCalled();

    unsubscribe();
  });

  it("routes an invalid hosted session back through pairing recovery", async () => {
    const disconnected = vi.fn();
    const unsubscribe = onHostedDaemonDisconnected(disconnected);
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response("unauthorized", { status: 401 }),
    );

    const response = await fetchFromHostedDaemon(
      "http://localhost:37373/api/v1/hosted-bridge/projection/ledger",
    );
    expect(response.status).toBe(401);
    expect(disconnected).toHaveBeenCalledWith({
      kind: "pairing_required",
      status: 401,
    });

    unsubscribe();
  });

  it("routes production hosted projection calls through the boundary", async () => {
    const disconnected = vi.fn();
    const unsubscribe = onHostedDaemonDisconnected(disconnected);
    vi.spyOn(globalThis, "fetch").mockRejectedValue(
      new TypeError("Failed to fetch"),
    );

    await expect(fetchHostedLedgerProjection("session-token")).rejects.toThrow(
      "Failed to fetch",
    );
    expect(disconnected).toHaveBeenCalledOnce();

    unsubscribe();
  });

  it("keeps Cloud discovery failures outside the daemon disconnect boundary", async () => {
    const disconnected = vi.fn();
    const unsubscribe = onHostedDaemonDisconnected(disconnected);
    vi.spyOn(globalThis, "fetch").mockRejectedValue(
      new TypeError("Cloud request failed"),
    );

    await expect(
      fetchCloudProjects({ accessToken: "cloud-session" }),
    ).rejects.toThrow("Cloud request failed");
    expect(disconnected).not.toHaveBeenCalled();

    unsubscribe();
  });

  it("does not leave any hosted gateway request on an unobserved fetch path", () => {
    expect(apiSource).not.toMatch(
      /await fetch\(\s*`\$\{HOSTED_GATEWAY_BASE\}/,
    );
    expect(apiSource).not.toMatch(
      /fetchFromHostedDaemon\(\s*`\$\{BASE\}/,
    );
    // Count rises as new hosted-daemon helpers are added (pair status poll,
    // connections, etc.). The invariant is: every hosted gateway call goes
    // through fetchFromHostedDaemon — never raw fetch on HOSTED_GATEWAY_BASE.
    expect(apiSource.match(/fetchFromHostedDaemon\(/g)?.length).toBe(16);
  });
});

describe("Hosted dashboard disconnect recovery", () => {
  it("preserves the paired screen and handoff beneath one blocking dialog", async () => {
    window.sessionStorage.setItem(
      "bicameral_hosted_bridge",
      JSON.stringify({
        session_token: "bridge-session",
        daemon_url: "http://localhost:37373",
        cloud_account_id: "acct-1",
        cloud_access_token: "cloud-session",
      }),
    );
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({ projects: [] }), {
        status: 200,
        headers: { "content-type": "application/json" },
      }),
    );
    const App = (await import("./App.svelte")).default;
    const view = render(App);
    expect(view.getByTestId("hosted-dashboard")).toBeTruthy();

    vi.mocked(globalThis.fetch).mockRejectedValue(
      new TypeError("Failed to fetch"),
    );
    await expect(fetchHostedLedgerProjection("bridge-session")).rejects.toThrow(
      "Failed to fetch",
    );
    await expect(fetchHostedLedgerProjection("bridge-session")).rejects.toThrow(
      "Failed to fetch",
    );

    await waitFor(() =>
      expect(
        view.getByRole("dialog", { name: "Local daemon disconnected" }),
      ).toBeTruthy(),
    );
    expect(view.getByTestId("hosted-dashboard")).toBeTruthy();
    expect(view.container.querySelector(".app-shell")?.hasAttribute("inert")).toBe(
      true,
    );
    expect(window.sessionStorage.getItem("bicameral_hosted_bridge")).toBeTruthy();
    expect(view.getAllByRole("dialog")).toHaveLength(1);

  });

  it("clears the stale handoff and navigates only when pairing is requested", async () => {
    window.sessionStorage.setItem(
      "bicameral_hosted_bridge",
      JSON.stringify({ session_token: "stale" }),
    );
    const navigate = vi.fn();
    const { restartHostedPairing } = await import("./lib/hosted-handoff");

    restartHostedPairing(navigate);

    expect(window.sessionStorage.getItem("bicameral_hosted_bridge")).toBeNull();
    expect(navigate).toHaveBeenCalledWith("/connect");
  });
});
