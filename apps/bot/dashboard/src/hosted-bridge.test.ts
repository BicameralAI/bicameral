/**
 * Hosted bridge pairing and session shell UI tests (issue #618).
 *
 * Validates fixture contracts, state rendering coverage, and authority
 * boundary constraints for the hosted bridge pairing UI. Uses mocked
 * API responses following the provider-setup.test.ts pattern.
 */

import { describe, it, expect } from "vitest";

describe("Hosted bridge fixture contract", () => {
  it("exports all required UI state fixtures", async () => {
    const { HOSTED_BRIDGE_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    expect(HOSTED_BRIDGE_FIXTURES.unpaired).toBeDefined();
    expect(HOSTED_BRIDGE_FIXTURES.pairing_pending_awaiting).toBeDefined();
    expect(HOSTED_BRIDGE_FIXTURES.pairing_pending_connecting).toBeDefined();
    expect(HOSTED_BRIDGE_FIXTURES.pairing_pending_handshake).toBeDefined();
    expect(HOSTED_BRIDGE_FIXTURES.confirmation).toBeDefined();
    expect(HOSTED_BRIDGE_FIXTURES.paired).toBeDefined();
    expect(HOSTED_BRIDGE_FIXTURES.expired).toBeDefined();
    expect(HOSTED_BRIDGE_FIXTURES.revoked).toBeDefined();
    expect(HOSTED_BRIDGE_FIXTURES.error_daemon_unavailable).toBeDefined();
    expect(HOSTED_BRIDGE_FIXTURES.error_wrong_daemon).toBeDefined();
    expect(HOSTED_BRIDGE_FIXTURES.error_wrong_account).toBeDefined();
    expect(HOSTED_BRIDGE_FIXTURES.error_unsupported_daemon).toBeDefined();
    expect(HOSTED_BRIDGE_FIXTURES.error_origin_mismatch).toBeDefined();
  });

  it("exports session info fixtures", async () => {
    const { SESSION_FIXTURES } = await import("./lib/hosted-bridge-fixtures");
    expect(SESSION_FIXTURES.active).toBeDefined();
    expect(SESSION_FIXTURES.expiring_soon).toBeDefined();
  });
});

describe("Hosted bridge UI state shapes", () => {
  it("unpaired state has correct kind", async () => {
    const { HOSTED_BRIDGE_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    expect(HOSTED_BRIDGE_FIXTURES.unpaired.kind).toBe("unpaired");
  });

  it("pairing_pending states have reason and optional message", async () => {
    const { HOSTED_BRIDGE_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const awaiting = HOSTED_BRIDGE_FIXTURES.pairing_pending_awaiting;
    expect(awaiting.kind).toBe("pairing_pending");
    if (awaiting.kind === "pairing_pending") {
      expect(awaiting.reason).toBe("awaiting_confirmation");
      expect(awaiting.message).toBeTruthy();
    }

    const connecting = HOSTED_BRIDGE_FIXTURES.pairing_pending_connecting;
    expect(connecting.kind).toBe("pairing_pending");
    if (connecting.kind === "pairing_pending") {
      expect(connecting.reason).toBe("connecting");
    }

    const handshake = HOSTED_BRIDGE_FIXTURES.pairing_pending_handshake;
    expect(handshake.kind).toBe("pairing_pending");
    if (handshake.kind === "pairing_pending") {
      expect(handshake.reason).toBe("handshake");
    }
  });

  it("confirmation state has origin and account", async () => {
    const { HOSTED_BRIDGE_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const confirmation = HOSTED_BRIDGE_FIXTURES.confirmation;
    expect(confirmation.kind).toBe("confirmation");
    if (confirmation.kind === "confirmation") {
      expect(confirmation.origin).toBeTruthy();
      expect(confirmation.cloudAccountId).toBeTruthy();
    }
  });

  it("paired state has origin, account, pairedAt, and expiresAt", async () => {
    const { HOSTED_BRIDGE_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const paired = HOSTED_BRIDGE_FIXTURES.paired;
    expect(paired.kind).toBe("paired");
    if (paired.kind === "paired") {
      expect(paired.origin).toBeTruthy();
      expect(paired.cloudAccountId).toBeTruthy();
      expect(paired.pairedAt).toBeTruthy();
      expect(paired.expiresAt).toBeTruthy();
    }
  });

  it("expired state has reason and revokedAt", async () => {
    const { HOSTED_BRIDGE_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const expired = HOSTED_BRIDGE_FIXTURES.expired;
    expect(expired.kind).toBe("expired");
    if (expired.kind === "expired") {
      expect(expired.reason).toContain("expired");
      expect(expired.revokedAt).toBeTruthy();
    }
  });

  it("revoked state has reason and revokedAt", async () => {
    const { HOSTED_BRIDGE_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const revoked = HOSTED_BRIDGE_FIXTURES.revoked;
    expect(revoked.kind).toBe("revoked");
    if (revoked.kind === "revoked") {
      expect(revoked.reason).toContain("revoked");
      expect(revoked.revokedAt).toBeTruthy();
    }
  });
});

describe("Hosted bridge error state variants", () => {
  it("daemon_unavailable is retryable", async () => {
    const { HOSTED_BRIDGE_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const state = HOSTED_BRIDGE_FIXTURES.error_daemon_unavailable;
    expect(state.kind).toBe("error");
    if (state.kind === "error") {
      expect(state.errorKind).toBe("daemon_unavailable");
      expect(state.retryAfterRepair).toBe(true);
      expect(state.message).toContain("not reachable");
    }
  });

  it("wrong_daemon is retryable", async () => {
    const { HOSTED_BRIDGE_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const state = HOSTED_BRIDGE_FIXTURES.error_wrong_daemon;
    expect(state.kind).toBe("error");
    if (state.kind === "error") {
      expect(state.errorKind).toBe("wrong_daemon");
      expect(state.retryAfterRepair).toBe(true);
      expect(state.message).toContain("different");
    }
  });

  it("wrong_account is retryable", async () => {
    const { HOSTED_BRIDGE_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const state = HOSTED_BRIDGE_FIXTURES.error_wrong_account;
    expect(state.kind).toBe("error");
    if (state.kind === "error") {
      expect(state.errorKind).toBe("account_mismatch");
      expect(state.retryAfterRepair).toBe(true);
      expect(state.message).toContain("account");
    }
  });

  it("unsupported_daemon is not retryable", async () => {
    const { HOSTED_BRIDGE_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const state = HOSTED_BRIDGE_FIXTURES.error_unsupported_daemon;
    expect(state.kind).toBe("error");
    if (state.kind === "error") {
      expect(state.errorKind).toBe("unsupported_daemon");
      expect(state.retryAfterRepair).toBe(false);
      expect(state.message).toContain("does not support");
    }
  });

  it("origin_mismatch is retryable", async () => {
    const { HOSTED_BRIDGE_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const state = HOSTED_BRIDGE_FIXTURES.error_origin_mismatch;
    expect(state.kind).toBe("error");
    if (state.kind === "error") {
      expect(state.errorKind).toBe("origin_mismatch");
      expect(state.retryAfterRepair).toBe(true);
    }
  });
});

describe("Session shell fixture contract", () => {
  it("active session has all required fields", async () => {
    const { SESSION_FIXTURES } = await import("./lib/hosted-bridge-fixtures");
    const session = SESSION_FIXTURES.active;
    expect(session.pairingState).toBe("hosted_bridge_paired");
    expect(session.origin).toBeTruthy();
    expect(session.cloudAccountId).toBeTruthy();
    expect(session.pairedAt).toBeTruthy();
    expect(session.expiresAt).toBeTruthy();
  });

  it("expiring_soon session has shorter TTL", async () => {
    const { SESSION_FIXTURES } = await import("./lib/hosted-bridge-fixtures");
    const active = SESSION_FIXTURES.active;
    const expiring = SESSION_FIXTURES.expiring_soon;
    const activeTTL =
      new Date(active.expiresAt).getTime() -
      new Date(active.pairedAt).getTime();
    const expiringTTL =
      new Date(expiring.expiresAt).getTime() -
      new Date(expiring.pairedAt).getTime();
    expect(expiringTTL).toBeLessThan(activeTTL);
  });
});

describe("Authority boundary constraints", () => {
  it("allowed actions list exists and is non-empty", async () => {
    const { BRIDGE_ALLOWED_ACTIONS } = await import(
      "./lib/hosted-bridge-types"
    );
    expect(BRIDGE_ALLOWED_ACTIONS.length).toBeGreaterThan(0);
  });

  it("not-allowed actions list exists and is non-empty", async () => {
    const { BRIDGE_NOT_ALLOWED_ACTIONS } = await import(
      "./lib/hosted-bridge-types"
    );
    expect(BRIDGE_NOT_ALLOWED_ACTIONS.length).toBeGreaterThan(0);
  });

  it("no fixture exposes provider tokens or local paths", async () => {
    const { HOSTED_BRIDGE_FIXTURES, SESSION_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const forbidden = [
      "provider_token",
      "access_token",
      "refresh_token",
      "/home/",
      "/Users/",
      "C:\\",
      "event_store_payload",
      "source_body",
      "workspace_binding",
    ];
    const allFixtures = [
      ...Object.values(HOSTED_BRIDGE_FIXTURES),
      ...Object.values(SESSION_FIXTURES),
    ];
    for (const fixture of allFixtures) {
      const json = JSON.stringify(fixture);
      for (const term of forbidden) {
        expect(json).not.toContain(term);
      }
    }
  });

  it("not-allowed actions include provider secrets and filesystem access", async () => {
    const { BRIDGE_NOT_ALLOWED_ACTIONS } = await import(
      "./lib/hosted-bridge-types"
    );
    const joined = BRIDGE_NOT_ALLOWED_ACTIONS.join(" ").toLowerCase();
    expect(joined).toContain("provider secret");
    expect(joined).toContain("filesystem");
  });

  it("allowed actions do not reference workspace binding", async () => {
    const { BRIDGE_ALLOWED_ACTIONS } = await import(
      "./lib/hosted-bridge-types"
    );
    const joined = BRIDGE_ALLOWED_ACTIONS.join(" ").toLowerCase();
    expect(joined).not.toContain("workspace binding");
  });
});

describe("Component imports resolve", () => {
  it("HostedPairingScreen resolves", async () => {
    const mod = await import("./pages/HostedPairingScreen.svelte");
    expect(mod).toBeDefined();
    expect(mod.default).toBeDefined();
  });

  it("LocalConfirmationScreen resolves", async () => {
    const mod = await import("./pages/LocalConfirmationScreen.svelte");
    expect(mod).toBeDefined();
    expect(mod.default).toBeDefined();
  });

  it("HostedSessionShell resolves", async () => {
    const mod = await import("./pages/HostedSessionShell.svelte");
    expect(mod).toBeDefined();
    expect(mod.default).toBeDefined();
  });

  it("HostedDashboard resolves", async () => {
    const mod = await import("./pages/HostedDashboard.svelte");
    expect(mod).toBeDefined();
    expect(mod.default).toBeDefined();
  });

  it("App.svelte resolves with hosted bridge imports", async () => {
    const mod = await import("./App.svelte");
    expect(mod).toBeDefined();
    expect(mod.default).toBeDefined();
  });
});

describe("API client hosted bridge functions", () => {
  it("fetchHostedBridgeStatus is exported", async () => {
    const { fetchHostedBridgeStatus } = await import("./lib/api");
    expect(typeof fetchHostedBridgeStatus).toBe("function");
  });

  it("requestHostedBridgePairing is exported", async () => {
    const { requestHostedBridgePairing } = await import("./lib/api");
    expect(typeof requestHostedBridgePairing).toBe("function");
  });
});
