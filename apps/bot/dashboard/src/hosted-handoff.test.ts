import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  readHostedHandoff,
  clearHostedHandoff,
  readHostedRestoreCredential,
  clearHostedRestoreCredential,
  hostedRecoveryPath,
  recoverHostedPairing,
  restartHostedPairing,
  resolveDaemonOrigin,
} from "./lib/hosted-handoff";
import {
  configureHostedGateway,
  hostedGatewayBase,
  fetchHostedDashboardAdapter,
} from "./lib/api";

function storageStub(): Storage {
  const store = new Map<string, string>();
  return {
    get length() {
      return store.size;
    },
    clear: () => store.clear(),
    getItem: (key: string) => store.get(key) ?? null,
    key: (index: number) => Array.from(store.keys())[index] ?? null,
    removeItem: (key: string) => store.delete(key),
    setItem: (key: string, value: string) => store.set(key, value),
  } as Storage;
}

describe("hosted handoff", () => {
  beforeEach(() => {
    Object.defineProperty(window, "sessionStorage", {
      configurable: true,
      value: storageStub(),
    });
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      value: storageStub(),
    });
  });

  it("returns null when no handoff is present", () => {
    expect(readHostedHandoff()).toBeNull();
  });

  it("parses a stored handoff and strips a trailing slash on daemon_url", () => {
    window.sessionStorage.setItem(
      "bicameral_hosted_bridge",
      JSON.stringify({
        session_token: "tok-123",
        daemon_url: "http://localhost:37373/",
        cloud_account_id: "acct-1",
        expires_at: "2026-07-06T23:59:00Z",
      }),
    );
    const h = readHostedHandoff();
    expect(h?.session_token).toBe("tok-123");
    expect(h?.daemon_url).toBe("http://localhost:37373");
    expect(h?.cloud_account_id).toBe("acct-1");
  });

  it("ignores a handoff missing a session token", () => {
    window.sessionStorage.setItem(
      "bicameral_hosted_bridge",
      JSON.stringify({ daemon_url: "http://localhost:37373" }),
    );
    expect(readHostedHandoff()).toBeNull();
  });

  it("clears the handoff", () => {
    window.sessionStorage.setItem(
      "bicameral_hosted_bridge",
      JSON.stringify({ session_token: "t", daemon_url: "x" }),
    );
    clearHostedHandoff();
    expect(readHostedHandoff()).toBeNull();
  });

  it("routes a cold app visit through restore when bounded proof is still live", () => {
    window.localStorage.setItem(
      "bicameral_hosted_bridge_restore",
      JSON.stringify({
        request_handle: "req-1",
        poll_proof: "proof-1",
        daemon_url: "http://localhost:37373/",
        cloud_account_id: "acct-1",
        expires_at: "2026-08-13T00:00:00Z",
      }),
    );

    const restore = readHostedRestoreCredential(Date.parse("2026-08-12T20:00:00Z"));
    expect(restore?.request_handle).toBe("req-1");
    expect(restore?.daemon_url).toBe("http://localhost:37373");
    expect(hostedRecoveryPath(Date.parse("2026-08-12T20:00:00Z"))).toBe("/restore");
  });

  it("fails closed to connect and clears an expired restore proof", () => {
    window.localStorage.setItem(
      "bicameral_hosted_bridge_restore",
      JSON.stringify({
        request_handle: "req-1",
        poll_proof: "proof-1",
        daemon_url: "http://localhost:37373",
        cloud_account_id: "acct-1",
        expires_at: "2026-08-12T19:59:59Z",
      }),
    );

    expect(hostedRecoveryPath(Date.parse("2026-08-12T20:00:00Z"))).toBe("/connect");
    expect(window.localStorage.getItem("bicameral_hosted_bridge_restore")).toBeNull();
  });

  it("fails closed and clears malformed durable restore state", () => {
    window.localStorage.setItem(
      "bicameral_hosted_bridge_restore",
      JSON.stringify({ request_handle: "req-1", session_token: "must-not-count" }),
    );

    expect(readHostedRestoreCredential()).toBeNull();
    expect(window.localStorage.getItem("bicameral_hosted_bridge_restore")).toBeNull();
  });

  it("restart clears both transient authority and durable restore proof", () => {
    window.sessionStorage.setItem(
      "bicameral_hosted_bridge",
      JSON.stringify({ session_token: "t", daemon_url: "x" }),
    );
    window.localStorage.setItem(
      "bicameral_hosted_bridge_restore",
      JSON.stringify({
        request_handle: "req-1",
        poll_proof: "proof-1",
        cloud_account_id: "acct-1",
        expires_at: "2099-01-01T00:00:00Z",
      }),
    );
    const navigate = vi.fn();

    restartHostedPairing(navigate);

    expect(readHostedHandoff()).toBeNull();
    expect(readHostedRestoreCredential()).toBeNull();
    expect(navigate).toHaveBeenCalledWith("/connect");
  });

  it("disconnect recovery preserves a valid restore proof and routes through restore", () => {
    window.sessionStorage.setItem(
      "bicameral_hosted_bridge",
      JSON.stringify({ session_token: "stale", daemon_url: "http://localhost:37373" }),
    );
    window.localStorage.setItem(
      "bicameral_hosted_bridge_restore",
      JSON.stringify({
        request_handle: "req-1",
        poll_proof: "proof-1",
        daemon_url: "http://localhost:37373",
        cloud_account_id: "acct-1",
        expires_at: "2099-01-01T00:00:00Z",
      }),
    );
    const navigate = vi.fn();

    recoverHostedPairing(navigate);

    expect(readHostedHandoff()).toBeNull();
    expect(readHostedRestoreCredential()).not.toBeNull();
    expect(navigate).toHaveBeenCalledWith("/restore");
  });

  it("clearHostedRestoreCredential removes only the durable restore proof", () => {
    window.localStorage.setItem(
      "bicameral_hosted_bridge_restore",
      JSON.stringify({
        request_handle: "req-1",
        poll_proof: "proof-1",
        cloud_account_id: "acct-1",
        expires_at: "2099-01-01T00:00:00Z",
      }),
    );
    window.sessionStorage.setItem(
      "bicameral_hosted_bridge",
      JSON.stringify({ session_token: "t", daemon_url: "x" }),
    );

    clearHostedRestoreCredential();

    expect(readHostedRestoreCredential()).toBeNull();
    expect(readHostedHandoff()?.session_token).toBe("t");
  });

  it("resolveDaemonOrigin honors the bicameral_daemon_url override", () => {
    window.localStorage.setItem("bicameral_daemon_url", "http://localhost:9999");
    expect(resolveDaemonOrigin()).toBe("http://localhost:9999");
  });

  it("resolveDaemonOrigin defaults to the canonical daemon port", () => {
    expect(resolveDaemonOrigin()).toBe("http://localhost:37373");
  });
});

describe("hosted gateway base (T1: browser->daemon direct)", () => {
  afterEach(() => {
    configureHostedGateway(""); // reset module state
    vi.unstubAllGlobals();
  });

  it("targets the configured daemon origin and sends the bridge token", async () => {
    configureHostedGateway("http://localhost:37373");
    expect(hostedGatewayBase()).toBe("http://localhost:37373");

    const fetchMock = vi.fn().mockResolvedValue({
      json: async () => ({ mode: "hosted", projects: [] }),
    });
    vi.stubGlobal("fetch", fetchMock);

    await fetchHostedDashboardAdapter("tok-abc");

    expect(fetchMock).toHaveBeenCalledWith(
      "http://localhost:37373/api/v1/hosted-bridge/dashboard-adapter",
      { headers: { "x-bridge-session-token": "tok-abc" } },
    );
  });
});
