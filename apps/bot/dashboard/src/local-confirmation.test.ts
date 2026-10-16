/**
 * Local confirmation surface + pending-pairing contract tests (issue #790).
 *
 * The locally served confirmation surface must show the requesting origin and
 * account, the allowed and forbidden capabilities, and explicit Approve / Deny
 * controls. Only an explicit local Approve may transition a pending request to
 * a paired session; the surface itself never handles a token.
 *
 * These are supporting contract/UI checks only. The terminal browser journey
 * (dashboard -> local confirmation -> app acceptance) is owned and validated by
 * bicameral-cloud#121.
 */

import { describe, it, expect, afterEach, vi } from "vitest";
import { render, cleanup, fireEvent } from "@testing-library/svelte";
import LocalConfirmationScreen from "./pages/LocalConfirmationScreen.svelte";
import {
  BRIDGE_ALLOWED_ACTIONS,
  BRIDGE_NOT_ALLOWED_ACTIONS,
  type HostedBridgePairingPendingResponse,
  type HostedBridgePairingStatusResponse,
  type HostedBridgePairingUpdateRequiredResponse,
  type PendingPairingLocalView,
  PENDING_PAIRING_MIN_PROTOCOL_VERSION,
  HOSTED_BRIDGE_PROTOCOL_VERSION,
} from "./lib/hosted-bridge-types";

afterEach(() => cleanup());

describe("Local confirmation surface (#790)", () => {
  const props = {
    origin: "https://app.bicameral-ai.com",
    cloudAccountId: "acct-local-123",
    requestHandle: "req-handle-abc",
    expiresAt: "2026-07-13T13:00:00Z",
  };

  it("shows the requesting origin and account", () => {
    const { getByTestId } = render(LocalConfirmationScreen, { props });
    expect(getByTestId("requesting-origin").textContent).toContain(
      "https://app.bicameral-ai.com",
    );
    expect(getByTestId("requesting-account").textContent).toContain(
      "acct-local-123",
    );
  });

  it("lists allowed and forbidden capabilities", () => {
    const { container } = render(LocalConfirmationScreen, { props });
    const text = container.textContent ?? "";
    for (const allowed of BRIDGE_ALLOWED_ACTIONS) {
      expect(text).toContain(allowed);
    }
    for (const forbidden of BRIDGE_NOT_ALLOWED_ACTIONS) {
      expect(text).toContain(forbidden);
    }
    expect(text.toLowerCase()).toContain("forbidden");
  });

  it("exposes explicit Approve and Deny controls", async () => {
    const onconfirm = vi.fn();
    const oncancel = vi.fn();
    const { getByTestId } = render(LocalConfirmationScreen, {
      props: { ...props, onconfirm, oncancel },
    });
    const approve = getByTestId("approve-pairing");
    const deny = getByTestId("deny-pairing");
    expect(approve.textContent).toContain("Approve");
    expect(deny.textContent).toContain("Deny");

    await fireEvent.click(approve);
    expect(onconfirm).toHaveBeenCalledTimes(1);
    await fireEvent.click(deny);
    expect(oncancel).toHaveBeenCalledTimes(1);
  });

  it("never renders a session token", () => {
    const { container } = render(LocalConfirmationScreen, { props });
    expect(container.textContent ?? "").not.toContain("session_token");
  });
});

describe("Pending pairing wire contract (#790)", () => {
  it("pins the pending-pairing protocol floor", () => {
    expect(HOSTED_BRIDGE_PROTOCOL_VERSION).toBe(4);
    expect(PENDING_PAIRING_MIN_PROTOCOL_VERSION).toBe(4);
  });

  it("pending response is tokenless and carries handle/proof/expiry", () => {
    const res: HostedBridgePairingPendingResponse = {
      status: "pending",
      request_handle: "h",
      poll_proof: "p",
      pairing_code: "7K2M-PQ8R",
      expires_at: "2026-07-13T13:00:00Z",
      protocol_version: HOSTED_BRIDGE_PROTOCOL_VERSION,
      capabilities: {
        bridge_supported: true,
        workspace_binding_available: false,
        protocol_version: HOSTED_BRIDGE_PROTOCOL_VERSION,
        allowed: [...BRIDGE_ALLOWED_ACTIONS],
        forbidden: [...BRIDGE_NOT_ALLOWED_ACTIONS],
      },
      confirmation: {
        terminal_command: "bicameral pairing approve",
        message: "Paste this code into your local Bicameral gateway.",
        requesting_origin: "https://app.bicameral-ai.com",
        cloud_account_id: "acct-1",
        capability_summary: {
          bridge_supported: true,
          workspace_binding_available: false,
          protocol_version: HOSTED_BRIDGE_PROTOCOL_VERSION,
          allowed: [...BRIDGE_ALLOWED_ACTIONS],
          forbidden: [...BRIDGE_NOT_ALLOWED_ACTIONS],
        },
      },
    };
    expect(res.status).toBe("pending");
    expect(JSON.stringify(res)).not.toContain("session_token");
  });

  it("only the approved status outcome carries a token", () => {
    const approved: HostedBridgePairingStatusResponse = {
      status: "approved",
      request_handle: "h",
      session_token: "tok",
      expires_at: "2026-07-13T13:30:00Z",
    };
    const denied: HostedBridgePairingStatusResponse = {
      status: "denied",
      request_handle: "h",
    };
    const expired: HostedBridgePairingStatusResponse = {
      status: "expired",
      request_handle: "h",
    };
    expect(approved.session_token).toBeTruthy();
    expect(denied.session_token).toBeUndefined();
    expect(expired.session_token).toBeUndefined();
  });

  it("local view is tokenless and carries a confirmation nonce", () => {
    const view: PendingPairingLocalView = {
      status: "pending",
      request_handle: "h",
      requesting_origin: "https://app.bicameral-ai.com",
      cloud_account_id: "acct-1",
      capability_summary: {
        bridge_supported: true,
        workspace_binding_available: false,
        protocol_version: HOSTED_BRIDGE_PROTOCOL_VERSION,
        allowed: [...BRIDGE_ALLOWED_ACTIONS],
        forbidden: [...BRIDGE_NOT_ALLOWED_ACTIONS],
      },
      expires_at: "2026-07-13T13:00:00Z",
      local_confirmation_nonce: "nonce-xyz",
    };
    expect(view.local_confirmation_nonce).toBeTruthy();
    expect(JSON.stringify(view)).not.toContain("session_token");
  });

  it("update-required outcome is typed and incompatible", () => {
    const res: HostedBridgePairingUpdateRequiredResponse = {
      status: "incompatible",
      error: "update_required",
      required_protocol_version: PENDING_PAIRING_MIN_PROTOCOL_VERSION,
      client_protocol_version: 2,
      daemon_protocol_version: HOSTED_BRIDGE_PROTOCOL_VERSION,
    };
    expect(res.error).toBe("update_required");
    expect(res.client_protocol_version).toBeLessThan(
      res.required_protocol_version,
    );
  });
});
