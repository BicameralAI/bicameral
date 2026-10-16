/**
 * Per-Product same-domain join tests (bot#768).
 *
 * Covers the pure Operator Home projection (grouping + row-action derivation)
 * and the `product.join` orchestration/state machine:
 *   - grouping into `Your products` vs `Available for <domain>`;
 *   - row action derivation across membership/readiness/setup;
 *   - the available heading (single domain vs mixed);
 *   - granted vs typed-rejection vs transport-failure outcomes;
 *   - never claiming membership before a granted daemon result;
 *   - concise recovery copy free of implementation/governance jargon.
 */

import { beforeEach, describe, it, expect, vi } from "vitest";
import type {
  HostedProjectPreview,
  ProductJoinResult,
  ProductJoinTicket,
  ProductJoinFailureCode,
  SignedProductJoinTicket,
} from "./lib/onboarding-contract";
import type { ProjectEntry } from "./lib/project-fixtures";
import {
  availableHeading,
  groupRows,
  isMember,
  rowAction,
} from "./lib/operator-home-model";
import {
  PRODUCT_JOIN_FAILURE_COPY,
  recoveryFromJoinError,
  runProductJoin,
  type ProductJoinTransport,
} from "./lib/product-join";

// ── Fixtures ───────────────────────────────────────────────────────

function member(overrides: Partial<ProjectEntry> = {}): ProjectEntry {
  return {
    id: "m1",
    name: "member-product",
    event_store_kind: "github",
    event_store_label: "GitHub event store",
    readiness: "ready",
    membership: "member",
    ...overrides,
  };
}

function available(overrides: Partial<ProjectEntry> = {}): ProjectEntry {
  return {
    id: "a1",
    name: "available-product",
    event_store_kind: "github",
    event_store_label: "GitHub event store",
    readiness: "ready",
    membership: "available",
    verified_domain: "acme.example",
    ...overrides,
  };
}

const PREVIEW: HostedProjectPreview = {
  preview_id: "prev_1",
  product_id: "prod_1",
  product_display_name: "Available Product",
  verified_domain: "acme.example",
  canonical: false,
  expires_at: "2999-01-01T00:00:00Z",
};

const TICKET: ProductJoinTicket = {
  issuer: "cloud",
  audience: "daemon",
  product_id: "prod_1",
  cloud_account_id: "acct_1",
  verified_domain: "acme.example",
  issued_at: "2026-01-01T00:00:00Z",
  expiry: "2999-01-01T00:00:00Z",
  nonce: "n1",
  registration_ref: "reg_1",
};

const SIGNED_TICKET: SignedProductJoinTicket = {
  ticket: TICKET,
  key_id: "join-key-1",
  signature: "c2lnbmF0dXJl",
};

function grantedResult(): ProductJoinResult {
  return {
    request_id: "req_1",
    product_id: "prod_1",
    cloud_account_id: "acct_1",
    outcome: "granted",
    granted_role: "Product Member",
    membership_event_ref: "evt_1",
    receipt_id: "rcpt_1",
    failure: null,
    decided_at: "2026-01-01T00:00:01Z",
  };
}

function rejectedResult(code: ProductJoinFailureCode): ProductJoinResult {
  return {
    request_id: "req_1",
    product_id: "prod_1",
    cloud_account_id: "acct_1",
    outcome: "rejected",
    granted_role: null,
    membership_event_ref: null,
    receipt_id: null,
    failure: { code, message: `daemon:${code}` },
    decided_at: "2026-01-01T00:00:01Z",
  };
}

function transport(
  result: ProductJoinResult | Error,
  ticket: SignedProductJoinTicket | Error = SIGNED_TICKET,
): ProductJoinTransport {
  return {
    requestJoinTicket: async () => {
      if (ticket instanceof Error) throw ticket;
      return ticket;
    },
    submitProductJoin: async () => {
      if (result instanceof Error) throw result;
      return result;
    },
  };
}

// ── Grouping + membership ──────────────────────────────────────────

describe("groupRows / isMember", () => {
  it("puts available rows in available and everything else in members", () => {
    const rows = [member({ id: "m" }), available({ id: "a" })];
    const groups = groupRows(rows);
    expect(groups.members.map((p) => p.id)).toEqual(["m"]);
    expect(groups.available.map((p) => p.id)).toEqual(["a"]);
  });

  it("treats a missing membership as member (legacy/local rows)", () => {
    const legacy = member({ membership: undefined });
    expect(isMember(legacy)).toBe(true);
    expect(groupRows([legacy]).members).toHaveLength(1);
  });

  it("preserves input order within each group", () => {
    const rows = [
      available({ id: "a1" }),
      member({ id: "m1" }),
      available({ id: "a2" }),
      member({ id: "m2" }),
    ];
    const groups = groupRows(rows);
    expect(groups.available.map((p) => p.id)).toEqual(["a1", "a2"]);
    expect(groups.members.map((p) => p.id)).toEqual(["m1", "m2"]);
  });
});

// ── Row action derivation ──────────────────────────────────────────

describe("rowAction", () => {
  it("available → join", () => {
    expect(rowAction(available())).toBe("join");
  });

  it("member + ready + complete → open", () => {
    expect(rowAction(member({ readiness: "ready" }))).toBe("open");
  });

  it("member + ready + setup incomplete → continue_setup", () => {
    expect(
      rowAction(member({ readiness: "ready", setup_incomplete: true })),
    ).toBe("continue_setup");
  });

  it("member + needs_access → repair", () => {
    expect(rowAction(member({ readiness: "needs_access" }))).toBe("repair");
  });

  it("member + offline → no action", () => {
    expect(rowAction(member({ readiness: "offline" }))).toBeNull();
  });
});

// ── Available heading ──────────────────────────────────────────────

describe("availableHeading", () => {
  it("names the shared verified domain", () => {
    expect(availableHeading([available({ verified_domain: "acme.example" })])).toBe(
      "Available for acme.example",
    );
  });

  it("falls back to a neutral phrase when domains differ or are absent", () => {
    expect(
      availableHeading([
        available({ verified_domain: "a.example" }),
        available({ verified_domain: "b.example" }),
      ]),
    ).toBe("Available to join");
    expect(availableHeading([available({ verified_domain: undefined })])).toBe(
      "Available to join",
    );
  });
});

// ── Orchestration outcomes ─────────────────────────────────────────

describe("runProductJoin", () => {
  const base = {
    preview: PREVIEW,
    accessToken: "cloud_tok",
    sessionToken: "bridge_tok",
  };

  it("returns granted only on a daemon Product Member grant", async () => {
    const outcome = await runProductJoin({
      ...base,
      transport: transport(grantedResult()),
    });
    expect(outcome.status).toBe("granted");
    if (outcome.status === "granted") {
      expect(outcome.result.granted_role).toBe("Product Member");
    }
  });

  it("maps each typed daemon rejection to its recovery copy", async () => {
    const codes: ProductJoinFailureCode[] = [
      "expired_ticket",
      "replayed_ticket",
      "wrong_product",
      "wrong_account",
      "wrong_domain",
    ];
    for (const code of codes) {
      const outcome = await runProductJoin({
        ...base,
        transport: transport(rejectedResult(code)),
      });
      expect(outcome.status).toBe("failed");
      if (outcome.status === "failed") {
        expect(outcome.failure.code).toBe(code);
        expect(outcome.failure).toEqual(PRODUCT_JOIN_FAILURE_COPY[code]);
      }
    }
  });

  it("wrong_domain is the only non-retryable failure", async () => {
    for (const [code, view] of Object.entries(PRODUCT_JOIN_FAILURE_COPY)) {
      expect(view.retryable).toBe(code !== "wrong_domain");
    }
  });

  it("converts a Cloud ticket hop error to cloud_unavailable (not offline local app)", async () => {
    const outcome = await runProductJoin({
      ...base,
      transport: transport(grantedResult(), new Error("network")),
    });
    expect(outcome.status).toBe("failed");
    if (outcome.status === "failed") {
      expect(outcome.failure.code).toBe("cloud_unavailable");
      expect(outcome.failure.retryable).toBe(true);
      expect(outcome.failure.message).not.toMatch(/local Bicameral app/i);
    }
  });

  it("converts a daemon submit transport TypeError to unavailable", async () => {
    const outcome = await runProductJoin({
      ...base,
      transport: transport(new TypeError("Failed to fetch")),
    });
    expect(outcome.status).toBe("failed");
    if (outcome.status === "failed") {
      expect(outcome.failure.code).toBe("unavailable");
    }
  });

  it("maps daemon unsupported_capability to join_not_configured", async () => {
    const err = Object.assign(
      new Error(
        "unsupported_capability: product.join requires configured issuer trust anchors",
      ),
      { status: 200, body: { status: "rejected", message: "unsupported_capability" } },
    );
    const outcome = await runProductJoin({
      ...base,
      transport: transport(err),
    });
    expect(outcome.status).toBe("failed");
    if (outcome.status === "failed") {
      expect(outcome.failure.code).toBe("join_not_configured");
      expect(outcome.failure.message).not.toMatch(/local Bicameral app/i);
    }
  });

  it("never reports granted for a rejected result missing the member role", async () => {
    const weird = { ...grantedResult(), granted_role: null } as ProductJoinResult;
    const outcome = await runProductJoin({ ...base, transport: transport(weird) });
    expect(outcome.status).toBe("failed");
  });
});

describe("recoveryFromJoinError", () => {
  it("does not use offline copy for unsupported_capability", () => {
    expect(
      recoveryFromJoinError(
        new Error(
          "unsupported_capability: product.join requires configured issuer trust anchors",
        ),
        "daemon",
      ),
    ).toBe("join_not_configured");
  });

  it("maps Cloud HTTP failures to cloud_unavailable", () => {
    expect(
      recoveryFromJoinError(
        Object.assign(new Error("ticket_signer_unavailable"), { status: 503 }),
        "cloud",
      ),
    ).toBe("cloud_unavailable");
  });
});

// ── Live Cloud → daemon transport contract ─────────────────────────────────

describe("live product join transport", () => {
  beforeEach(() => vi.restoreAllMocks());

  it("passes the Cloud-signed envelope unchanged in the daemon bridge body", async () => {
    const signedTicket = SIGNED_TICKET;
    const fetchSpy = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(
        new Response(JSON.stringify(signedTicket), {
          status: 200,
          headers: { "content-type": "application/json" },
        }),
      )
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            request_id: "transport-request",
            status: "ok",
            result: grantedResult(),
            responded_at: "2026-01-01T00:00:01Z",
          }),
          {
            status: 200,
            headers: { "content-type": "application/json" },
          },
        ),
      );

    const { requestProductJoinTicket, submitProductJoin } = await import(
      "./lib/api"
    );
    const cloudEnvelope = await requestProductJoinTicket({
      preview: PREVIEW,
      accessToken: "cloud-token",
      apiBase: "https://cloud.example",
    });
    expect(cloudEnvelope).toEqual(signedTicket);

    await submitProductJoin({
      sessionToken: "bridge-token",
      signedTicket: cloudEnvelope,
    });

    const [url, init] = fetchSpy.mock.calls[1];
    expect(String(url)).toContain("/api/v1/hosted-bridge/product-join");
    expect(JSON.parse((init as RequestInit).body as string)).toEqual({
      request_id: expect.any(String),
      signed_ticket: signedTicket,
    });
  });

  it("returns the ProductJoinResult inside the daemon ToolResponse", async () => {
    const granted = grantedResult();
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        JSON.stringify({
          request_id: "transport-request",
          status: "ok",
          result: granted,
          responded_at: "2026-01-01T00:00:01Z",
        }),
        { status: 200, headers: { "content-type": "application/json" } },
      ),
    );

    const { submitProductJoin } = await import("./lib/api");
    await expect(
      submitProductJoin({
        sessionToken: "bridge-token",
        signedTicket: SIGNED_TICKET,
      }),
    ).resolves.toEqual(granted);
  });
});

// ── Recovery copy hygiene (AC5) ────────────────────────────────────

describe("recovery copy hygiene", () => {
  it("contains no implementation or governance jargon", () => {
    const forbidden = [
      "ledger",
      "bridge",
      "ticket",
      "daemon",
      "cloud",
      "capability unavailable",
      "membership",
      "quorum",
      "policy",
    ];
    for (const view of Object.values(PRODUCT_JOIN_FAILURE_COPY)) {
      const lower = view.message.toLowerCase();
      for (const term of forbidden) {
        expect(lower, `"${view.message}"`).not.toContain(term);
      }
    }
  });
});
