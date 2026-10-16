// TEST-ONLY: verification suite for bot#1244. Self-contained, safe to
// remove or replace independently of the App.svelte fix it covers.
import { cleanup, fireEvent, render, waitFor } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

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

function mockFetchWithDriveStart(url: string): ReturnType<typeof vi.fn> {
  const impl = vi.fn(async (input: RequestInfo | URL) => {
    if (String(input).includes("/connections/google/start")) {
      return jsonResponse({
        outcome: "start_url",
        provider: "google",
        url,
        auth_session_id: "gsess-test",
        flow_type: "local_loopback",
      });
    }
    return jsonResponse({ projects: [] });
  });
  vi.spyOn(globalThis, "fetch").mockImplementation(impl);
  return impl;
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  window.sessionStorage.clear();
});

describe("Drive connect: Authorize opens the daemon-provided URL (bot#1244)", () => {
  it("does not open Google until Authorize is explicitly clicked", async () => {
    setPairedHandoff();
    mockFetchWithDriveStart("https://accounts.google.com/o/oauth2/v2/auth?state=first");
    const openSpy = vi.spyOn(window, "open").mockReturnValue(null);
    const App = (await import("./App.svelte")).default;
    const view = render(App);

    await fireEvent.click(view.getByTestId("empty-create"));
    await fireEvent.click(view.getByTestId("drive-oauth-start"));
    await waitFor(() => expect(view.getByTestId("drive-oauth-open")).toBeTruthy());

    expect(openSpy).not.toHaveBeenCalled();
  });

  it("opens the exact URL the daemon returned when Authorize is clicked", async () => {
    setPairedHandoff();
    const expectedUrl =
      "https://accounts.google.com/o/oauth2/v2/auth?state=second";
    mockFetchWithDriveStart(expectedUrl);
    const openSpy = vi.spyOn(window, "open").mockReturnValue(null);
    const App = (await import("./App.svelte")).default;
    const view = render(App);

    await fireEvent.click(view.getByTestId("empty-create"));
    await fireEvent.click(view.getByTestId("drive-oauth-start"));
    await waitFor(() => expect(view.getByTestId("drive-oauth-open")).toBeTruthy());
    await fireEvent.click(view.getByTestId("drive-oauth-open"));

    expect(openSpy).toHaveBeenCalledTimes(1);
    expect(openSpy).toHaveBeenCalledWith(
      expectedUrl,
      "_blank",
      "noopener,noreferrer",
    );
  });

  it("does not reopen a stale URL after cancelling and starting a new attempt", async () => {
    setPairedHandoff();
    const staleUrl = "https://accounts.google.com/o/oauth2/v2/auth?state=stale";
    const freshUrl = "https://accounts.google.com/o/oauth2/v2/auth?state=fresh";
    let nextUrl = staleUrl;
    vi.spyOn(globalThis, "fetch").mockImplementation(
      async (input: RequestInfo | URL) => {
        if (String(input).includes("/connections/google/start")) {
          return jsonResponse({
            outcome: "start_url",
            provider: "google",
            url: nextUrl,
            auth_session_id: "gsess-test",
            flow_type: "local_loopback",
          });
        }
        return jsonResponse({ projects: [] });
      },
    );
    const openSpy = vi.spyOn(window, "open").mockReturnValue(null);
    const App = (await import("./App.svelte")).default;
    const view = render(App);

    await fireEvent.click(view.getByTestId("empty-create"));
    await fireEvent.click(view.getByTestId("drive-oauth-start"));
    await waitFor(() => expect(view.getByTestId("drive-oauth-open")).toBeTruthy());
    await fireEvent.click(view.getByText("Cancel"));

    nextUrl = freshUrl;
    await fireEvent.click(view.getByTestId("drive-oauth-start"));
    await waitFor(() => expect(view.getByTestId("drive-oauth-open")).toBeTruthy());
    await fireEvent.click(view.getByTestId("drive-oauth-open"));

    expect(openSpy).toHaveBeenCalledTimes(1);
    expect(openSpy).toHaveBeenCalledWith(freshUrl, "_blank", "noopener,noreferrer");
  });

  it("clears the retained URL on denial, so retrying does not reopen the old link", async () => {
    setPairedHandoff();
    const deniedUrl = "https://accounts.google.com/o/oauth2/v2/auth?state=denied";
    const retryUrl = "https://accounts.google.com/o/oauth2/v2/auth?state=retry";
    let nextUrl = deniedUrl;
    vi.spyOn(globalThis, "fetch").mockImplementation(
      async (input: RequestInfo | URL) => {
        if (String(input).includes("/connections/google/status/")) {
          return jsonResponse({
            outcome: "denied",
            provider: "google",
            message: "consent denied",
          });
        }
        if (String(input).includes("/connections/google/start")) {
          return jsonResponse({
            outcome: "start_url",
            provider: "google",
            url: nextUrl,
            auth_session_id: "gsess-test",
            flow_type: "local_loopback",
          });
        }
        return jsonResponse({ projects: [] });
      },
    );
    const openSpy = vi.spyOn(window, "open").mockReturnValue(null);
    const App = (await import("./App.svelte")).default;
    const view = render(App);

    await fireEvent.click(view.getByTestId("empty-create"));
    await fireEvent.click(view.getByTestId("drive-oauth-start"));
    await waitFor(() => expect(view.getByTestId("drive-oauth-open")).toBeTruthy());
    await fireEvent.click(view.getByTestId("drive-oauth-open"));
    // The daemon reports the denial; the browser returning to focus checks now.
    window.dispatchEvent(new Event("focus"));

    nextUrl = retryUrl;
    await waitFor(() => expect(view.getByTestId("drive-oauth-retry")).toBeTruthy());
    await fireEvent.click(view.getByTestId("drive-oauth-retry"));
    await waitFor(() => expect(view.getByTestId("drive-oauth-open")).toBeTruthy());
    await fireEvent.click(view.getByTestId("drive-oauth-open"));

    expect(openSpy).toHaveBeenCalledTimes(2);
    expect(openSpy).toHaveBeenLastCalledWith(retryUrl, "_blank", "noopener,noreferrer");
  });

  it("does not offer a fake browser-only disconnect after a real completion", async () => {
    setPairedHandoff();
    const firstUrl = "https://accounts.google.com/o/oauth2/v2/auth?state=connected-first";
    let authorized = false;
    vi.spyOn(globalThis, "fetch").mockImplementation(
      async (input: RequestInfo | URL) => {
        const url = String(input);
        if (url.includes("/connections/google/start")) {
          return jsonResponse({
            outcome: "start_url",
            provider: "google",
            url: firstUrl,
            auth_session_id: "gsess-test",
            flow_type: "local_loopback",
          });
        }
        if (url.includes("/connections/google/status/")) {
          authorized = true;
          return jsonResponse({
            outcome: "connected",
            provider: "google",
            account_label: "test@example.com",
          });
        }
        if (url.endsWith("/api/v1/connections")) {
          return jsonResponse({
            connections: authorized
              ? {
                  google: {
                    provider: "google",
                    status: "connected",
                    account_label: "test@example.com",
                    capabilities: [],
                  },
                }
              : {},
          });
        }
        return jsonResponse({ projects: [] });
      },
    );
    vi.spyOn(window, "open").mockReturnValue(null);
    const App = (await import("./App.svelte")).default;
    const view = render(App);

    await fireEvent.click(view.getByTestId("empty-create"));
    await fireEvent.click(view.getByTestId("drive-oauth-start"));
    await waitFor(() => expect(view.getByTestId("drive-oauth-open")).toBeTruthy());
    await fireEvent.click(view.getByTestId("drive-oauth-open"));
    window.dispatchEvent(new Event("focus"));

    await waitFor(() =>
      expect(view.getByTestId("drive-disconnect-unavailable")).toBeTruthy(),
    );
    expect(view.queryByRole("button", { name: "Disconnect" })).toBeNull();
    expect(view.getByText("Disconnect unavailable in Alpha")).toBeTruthy();
  });
});
