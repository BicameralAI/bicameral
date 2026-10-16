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
      cloud_account_id: "acct-onboarding",
      cloud_access_token: "cloud-session",
    }),
  );
}

function installFakeDaemon(): void {
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/api/v1/connections")) {
      return jsonResponse({ connections: {} });
    }
    return jsonResponse({ projects: [] });
  });
}

describe("focused Drive onboarding", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    window.sessionStorage.clear();
    window.history.replaceState(null, "", "/");
  });

  it("renders the final-step context and hides unrelated host setup", async () => {
    setPairedHandoff();
    installFakeDaemon();
    window.history.replaceState(null, "", "/?onboarding=drive");

    const App = (await import("./App.svelte")).default;
    const view = render(App);

    await waitFor(() => expect(view.getByTestId("drive-onboarding-intro")).toBeTruthy());
    expect(view.getByText("Connect Google Drive", { exact: true })).toBeTruthy();
    expect(
      view.getByText("This is optional now. You can connect later from Settings.", {
        exact: true,
      }),
    ).toBeTruthy();
    expect(view.getByTestId("settings-drive-oauth")).toBeTruthy();
    expect(view.getByTestId("drive-onboarding-skip")).toBeTruthy();
    expect(view.queryByTestId("settings-agent-host")).toBeNull();
    expect(window.location.search).not.toContain("onboarding");
  });

  it("Skip for now ends onboarding and returns to Products", async () => {
    setPairedHandoff();
    installFakeDaemon();
    window.history.replaceState(null, "", "/?onboarding=drive");

    const App = (await import("./App.svelte")).default;
    const view = render(App);

    const skip = await waitFor(() => view.getByTestId("drive-onboarding-skip"));
    await fireEvent.click(skip);

    await waitFor(() => expect(view.queryByTestId("drive-onboarding-intro")).toBeNull());
    expect(view.getByTestId("empty-create")).toBeTruthy();
  });

  it("normal Settings retains the compact Storage and Agent host layout", async () => {
    setPairedHandoff();
    installFakeDaemon();

    const App = (await import("./App.svelte")).default;
    const view = render(App);

    const settings = await waitFor(() => view.getByTestId("nav-settings"));
    await fireEvent.click(settings);

    await waitFor(() => expect(view.getByTestId("settings-drive-oauth")).toBeTruthy());
    expect(view.getByTestId("settings-agent-host")).toBeTruthy();
    expect(view.queryByTestId("drive-onboarding-intro")).toBeNull();
    expect(view.queryByTestId("drive-onboarding-skip")).toBeNull();
  });
});
