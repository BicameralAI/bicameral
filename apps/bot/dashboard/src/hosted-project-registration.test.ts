/**
 * Hosted Product creation tests (bot#799).
 *
 * Proves the shipped browser flow coordinates daemon-canonical creation,
 * daemon-derived publication, authenticated Cloud directory publication, and
 * daemon-owned local setup without crossing token boundaries or reviving the
 * legacy cloud#66 team/expose path.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";
import registrationSource from "./pages/HostedProjectRegistration.svelte?raw";
import hostedDashboardSource from "./pages/HostedDashboard.svelte?raw";
import appSource from "./App.svelte?raw";
import orchestratorSource from "./lib/hosted-project-registration.ts?raw";
import hostedHandoffSource from "./lib/hosted-handoff.ts?raw";
import type {
  ProductCreationPublication,
  ProductCreationResult,
} from "./lib/hosted-project-registration";

class MemoryStorage implements Storage {
  readonly #values = new Map<string, string>();

  get length(): number {
    return this.#values.size;
  }

  clear(): void {
    this.#values.clear();
  }

  getItem(key: string): string | null {
    return this.#values.get(key) ?? null;
  }

  key(index: number): string | null {
    return [...this.#values.keys()][index] ?? null;
  }

  removeItem(key: string): void {
    this.#values.delete(key);
  }

  setItem(key: string, value: string): void {
    this.#values.set(key, value);
  }
}

const testStorage = new MemoryStorage();

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

function tokenFor(email: string): string {
  const payload = globalThis
    .btoa(JSON.stringify({ email }))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/g, "");
  return `header.${payload}.signature`;
}

function creationResult(): ProductCreationResult {
  return {
    product_id: "prd_1",
    creator_cloud_account_id: "acct_1",
    creator_roles: ["Policy Owner", "Product Owner"],
    membership_event_ref: "evt_1",
    governance_policy_version: "gpv_1",
    receipt_id: "rcpt_1",
    created_at: "2026-07-13T17:00:00Z",
  };
}

function createToolResponse(created = true): unknown {
  return {
    request_id: "req_create",
    status: "ok",
    result: {
      result: creationResult(),
      preview: {
        preview_id: "hpv_1",
        product_id: "prd_1",
        product_display_name: "Checkout Service",
        verified_domain: "acme.example",
        event_store_hint: { substrate: "git", provider: "github" },
        canonical: false,
        expires_at: "2026-07-14T17:00:00Z",
      },
      created,
    },
    responded_at: "2026-07-13T17:00:00Z",
  };
}

function commitProof(): ProductCreationPublication["first_event_commit_proof"] {
  return {
    schema_version: 2,
    attempt_id: "create-attempt-1",
    product_id: "prd_1",
    event_id: "0198d2b4-8a5c-7f22-9e31-9bb014cc8200",
    substrate: "drive_folder",
    resource_fingerprint: `sha256:${"1".repeat(64)}`,
    provider_revision: "provider-revision-abc123",
    event_digest: `sha256:${"2".repeat(64)}`,
    durability: "provider_confirmed_remote",
    confirmed_at: "2026-07-26T06:00:00Z",
    proof_digest: `sha256:${"3".repeat(64)}`,
    signer_key_id: `sha256:${"4".repeat(64)}`,
    signature_algorithm: "ed25519",
    signature: "dGVzdA==",
  };
}

function publication(
  overrides: Partial<ProductCreationPublication> = {},
): ProductCreationPublication {
  return {
    publication_id: "pub_1",
    product_id: "prd_1",
    creator_cloud_account_id: "acct_1",
    verified_domain: "acme.example",
    event_store_hint: { substrate: "git", provider: "github" },
    canonical: false,
    idempotency_key: "publish_publish-key",
    canonical_creation_receipt_ref: "rcpt_1",
    published_at: "2026-07-13T17:00:01Z",
    first_event_commit_proof: commitProof(),
    ...overrides,
  };
}

function publishToolResponse(): unknown {
  return {
    request_id: "req_publish",
    status: "ok",
    result: { publication: publication() },
    responded_at: "2026-07-13T17:00:01Z",
  };
}

function rejectedPublishToolResponse(): unknown {
  return {
    request_id: "req_publish_missing",
    status: "rejected",
    result: {
      diagnostics: {
        error_code: "product_not_found",
        canonical_product_count: 0,
        create_event_count: 0,
        malformed_create_event_count: 0,
        event_store_substrate: "git",
        product_id_fingerprint: "0123456789abcdef",
        idempotency_key_fingerprint: "fedcba9876543210",
      },
    },
    message:
      "product_not_found: no canonical Product exists for the supplied product_id",
    responded_at: "2026-07-13T17:00:01Z",
  };
}

function cloudReceipt(idempotent = false): unknown {
  return {
    canonical_product_id: "prd_1",
    display_name: "Checkout Service",
    eligible_domain: "acme.example",
    idempotent,
  };
}

function setupProjection(
  readiness: "open" | "continue_setup" | "repair" = "open",
): unknown {
  return {
    product_id: "prd_1",
    readiness,
    has_registration: readiness === "open",
    membership_retained: true,
    ...(readiness === "continue_setup"
      ? { next_step: "connect_provider" }
      : {}),
    ...(readiness === "repair" ? { repair_reason: "replay_failed" } : {}),
  };
}

function createOptions(overrides: Record<string, unknown> = {}) {
  const ids = ["create-key", "publish-key"];
  return {
    accessToken: tokenFor("owner@acme.example"),
    apiBase: "https://cloud.example",
    daemonBase: "http://localhost:37373",
    sessionToken: "bridge-session",
    cloudAccountId: "acct_1",
    displayName: "Checkout Service",
    storage: testStorage,
    idFactory: () => ids.shift() ?? "unexpected-id",
    now: () => "2026-07-13T17:00:00Z",
    ...overrides,
  };
}

function requestHeaders(call: unknown[]): Record<string, string> {
  return (call[1] as RequestInit).headers as Record<string, string>;
}

function requestBody(call: unknown[]): Record<string, unknown> {
  return JSON.parse((call[1] as RequestInit).body as string) as Record<
    string,
    unknown
  >;
}

beforeEach(() => {
  vi.restoreAllMocks();
  testStorage.clear();
});

describe("canonical hosted Product creation surface", () => {
  it("exports the daemon, Cloud, setup, recovery, and orchestration helpers", async () => {
    const mod = await import("./lib/hosted-project-registration");
    expect(typeof mod.submitHostedProductCreate).toBe("function");
    expect(typeof mod.submitHostedProductPublish).toBe("function");
    expect(typeof mod.publishHostedProductToCloud).toBe("function");
    expect(typeof mod.continueHostedProductSetup).toBe("function");
    expect(typeof mod.createHostedProject).toBe("function");
    expect(typeof mod.getPendingHostedCreateInput).toBe("function");
  });

  it("derives only the routing domain from the Cloud JWT", async () => {
    const { verifiedDomainFromAccessToken } = await import(
      "./lib/hosted-project-registration"
    );
    expect(
      verifiedDomainFromAccessToken(tokenFor("Owner@Acme.Example")),
    ).toBe("acme.example");
    expect(verifiedDomainFromAccessToken("not-a-jwt")).toBeNull();
  });

  it("does not call the legacy team or expose clients from the shipped orchestrator", () => {
    expect(orchestratorSource).not.toContain("import { createCloudTeam");
    expect(orchestratorSource).not.toContain("await createCloudTeam(");
    expect(orchestratorSource).not.toContain("await exposeCloudProject(");
  });
});

describe("createHostedProject", () => {
  it("creates in the daemon, publishes to Cloud, then continues local setup", async () => {
    const spy = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(jsonResponse(createToolResponse()))
      .mockResolvedValueOnce(jsonResponse(publishToolResponse()))
      .mockResolvedValueOnce(jsonResponse(cloudReceipt(), 201))
      .mockResolvedValueOnce(jsonResponse(setupProjection()));

    const { createHostedProject } = await import(
      "./lib/hosted-project-registration"
    );
    const outcome = await createHostedProject(createOptions());

    expect(outcome.status).toBe("recorded");
    expect(outcome.phase).toBe("published");
    expect(outcome.productId).toBe("prd_1");
    expect(outcome.localReadiness).toBe("open");
    expect(outcome.nextStep?.title).toBe("Product ready");
    expect(spy).toHaveBeenCalledTimes(4);

    const createCall = spy.mock.calls[0] as unknown[];
    expect(String(createCall[0])).toBe(
      "http://localhost:37373/api/v1/hosted-bridge/command",
    );
    expect(requestHeaders(createCall)["x-bridge-session-token"]).toBe(
      "bridge-session",
    );
    expect(requestHeaders(createCall).Authorization).toBeUndefined();
    expect(requestBody(createCall)).toEqual({
      command: {
        name: "product.create",
        params: {
          idempotency_key: "create_create-key",
          product_display_name: "Checkout Service",
          asserted_cloud_account_id: "acct_1",
          verified_domain: "acme.example",
        },
      },
    });

    const publishCall = spy.mock.calls[1] as unknown[];
    expect(requestBody(publishCall)).toEqual({
      command: {
        name: "product.publish",
        params: {
          product_id: "prd_1",
          idempotency_key: "publish_publish-key",
        },
      },
    });
    expect(requestHeaders(publishCall)["x-bridge-session-token"]).toBe(
      "bridge-session",
    );

    const cloudCall = spy.mock.calls[2] as unknown[];
    expect(String(cloudCall[0])).toBe("https://cloud.example/api/v1/products");
    expect(requestHeaders(cloudCall).Authorization).toBe(
      `Bearer ${tokenFor("owner@acme.example")}`,
    );
    expect(requestHeaders(cloudCall)["x-bridge-session-token"]).toBeUndefined();
    expect(requestBody(cloudCall)).toEqual({
      canonical_product_id: "prd_1",
      display_name: "Checkout Service",
      eligible_domain: "acme.example",
      creation_receipt: "rcpt_1",
      first_event_commit_proof: commitProof(),
      registration_ref: "prd_1",
      provider_hint: "github",
      substrate_hint: "git",
    });

    const setupCall = spy.mock.calls[3] as unknown[];
    expect(String(setupCall[0])).toBe(
      "http://localhost:37373/api/v1/hosted-bridge/product-continue-setup",
    );
    expect(requestHeaders(setupCall)["x-bridge-session-token"]).toBe(
      "bridge-session",
    );
    expect(requestHeaders(setupCall).Authorization).toBeUndefined();
    expect(requestBody(setupCall)).toMatchObject({
      product_id: "prd_1",
      registration_ref: "prd_1",
      provider_hint: "github",
      substrate_hint: "git",
    });
    expect(testStorage.length).toBe(0);
  });

  it("does not call Cloud when product.publish omits first_event_commit_proof", async () => {
    const spy = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(jsonResponse(createToolResponse()))
      .mockResolvedValueOnce(
        jsonResponse({
          request_id: "req_publish",
          status: "ok",
          result: {
            publication: publication({ first_event_commit_proof: undefined }),
          },
          responded_at: "2026-07-13T17:00:01Z",
        }),
      );
    const { createHostedProject } = await import(
      "./lib/hosted-project-registration"
    );
    const outcome = await createHostedProject(createOptions());
    expect(outcome.status).toBe("publication_retry_needed");
    expect(outcome.message).toContain("first_event_commit_proof");
    expect(spy).toHaveBeenCalledTimes(2);
    const cloudCalls = spy.mock.calls.filter((call) =>
      String(call[0]).includes("/api/v1/products"),
    );
    expect(cloudCalls).toHaveLength(0);
  });

  it("surfaces a typed daemon denial without calling Cloud or retaining a false recovery marker", async () => {
    const spy = vi.spyOn(globalThis, "fetch").mockResolvedValueOnce(
      jsonResponse({
        request_id: "req_create",
        status: "rejected",
        message: "unauthorized_actor: not permitted",
        responded_at: "2026-07-13T17:00:00Z",
      }),
    );
    const { createHostedProject } = await import(
      "./lib/hosted-project-registration"
    );
    const outcome = await createHostedProject(createOptions());
    expect(outcome.status).toBe("error");
    expect(outcome.message).toContain("unauthorized_actor");
    expect(spy).toHaveBeenCalledTimes(1);
    expect(testStorage.length).toBe(0);
  });

  it("keeps a safe recovery marker when Cloud publication fails after canonical creation", async () => {
    vi.spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(jsonResponse(createToolResponse()))
      .mockResolvedValueOnce(jsonResponse(publishToolResponse()))
      .mockResolvedValueOnce(
        jsonResponse({ detail: "Product directory unavailable" }, 503),
      );

    const { createHostedProject, getPendingHostedCreateInput } = await import(
      "./lib/hosted-project-registration"
    );
    const outcome = await createHostedProject(createOptions());

    expect(outcome.status).toBe("publication_retry_needed");
    expect(outcome.productId).toBe("prd_1");
    expect(outcome.message).toContain("exists in the local daemon");
    expect(getPendingHostedCreateInput("acct_1", testStorage)).toEqual({
      displayName: "Checkout Service",
    });

    const stored = testStorage.getItem(testStorage.key(0) as string);
    expect(stored).toContain('"productId":"prd_1"');
    expect(stored).not.toContain("bridge-session");
    expect(stored).not.toContain(tokenFor("owner@acme.example"));
    expect(stored).not.toContain("access_token");
    expect(stored).not.toContain("refresh_token");
  });

  it("retries publication without re-running canonical creation", async () => {
    const first = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(jsonResponse(createToolResponse()))
      .mockResolvedValueOnce(jsonResponse(publishToolResponse()))
      .mockResolvedValueOnce(jsonResponse({ detail: "temporary" }, 503));

    const { createHostedProject } = await import(
      "./lib/hosted-project-registration"
    );
    const options = createOptions();
    const failed = await createHostedProject(options);
    expect(failed.phase).toBe("publication_retry_needed");
    expect(first).toHaveBeenCalledTimes(3);

    first.mockReset();
    first
      .mockResolvedValueOnce(jsonResponse(publishToolResponse()))
      .mockResolvedValueOnce(jsonResponse(cloudReceipt(true), 200))
      .mockResolvedValueOnce(jsonResponse(setupProjection()));

    const recovered = await createHostedProject(options);
    expect(recovered.phase).toBe("published");
    expect(first).toHaveBeenCalledTimes(3);
    expect(
      (requestBody(first.mock.calls[0] as unknown[]).command as {
        name: string;
      }).name,
    ).toBe("product.publish");
    expect(String(first.mock.calls[1][0])).toBe(
      "https://cloud.example/api/v1/products",
    );
    expect(String(first.mock.calls[2][0])).toContain(
      "/product-continue-setup",
    );
    expect(testStorage.length).toBe(0);
  });

  it("returns the daemon request id when publication diagnostics are available", async () => {
    vi.spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(jsonResponse(createToolResponse()))
      .mockResolvedValueOnce(jsonResponse(rejectedPublishToolResponse()));

    const { createHostedProject } = await import(
      "./lib/hosted-project-registration"
    );
    const outcome = await createHostedProject(createOptions());

    expect(outcome.phase).toBe("publication_retry_needed");
    expect(outcome.supportId).toBe("req_publish_missing");
    expect(outcome.message).not.toContain("0123456789abcdef");
    expect(outcome.message).not.toContain("fedcba9876543210");
  });

  it("reuses the same create idempotency key after losing the daemon response", async () => {
    const spy = vi
      .spyOn(globalThis, "fetch")
      .mockRejectedValueOnce(new Error("connection lost after request"));

    const { createHostedProject } = await import(
      "./lib/hosted-project-registration"
    );
    const options = createOptions();
    const uncertain = await createHostedProject(options);
    expect(uncertain.phase).toBe("create_retry_needed");
    const firstBody = requestBody(spy.mock.calls[0] as unknown[]);
    const firstKey = (
      firstBody.command as { params: { idempotency_key: string } }
    ).params.idempotency_key;

    spy.mockReset();
    spy
      .mockResolvedValueOnce(jsonResponse(createToolResponse(false)))
      .mockResolvedValueOnce(jsonResponse(publishToolResponse()))
      .mockResolvedValueOnce(jsonResponse(cloudReceipt(), 201))
      .mockResolvedValueOnce(jsonResponse(setupProjection()));

    const recovered = await createHostedProject(options);
    expect(recovered.phase).toBe("published");
    const secondBody = requestBody(spy.mock.calls[0] as unknown[]);
    const secondKey = (
      secondBody.command as { params: { idempotency_key: string } }
    ).params.idempotency_key;
    expect(secondKey).toBe(firstKey);
    expect(testStorage.length).toBe(0);
  });

  it("reports honest partial success when local setup is not yet available", async () => {
    vi.spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(jsonResponse(createToolResponse()))
      .mockResolvedValueOnce(jsonResponse(publishToolResponse()))
      .mockResolvedValueOnce(jsonResponse(cloudReceipt(), 201))
      .mockResolvedValueOnce(jsonResponse({ message: "provider required" }, 409));

    const { createHostedProject } = await import(
      "./lib/hosted-project-registration"
    );
    const outcome = await createHostedProject(createOptions());
    expect(outcome.status).toBe("recorded");
    expect(outcome.phase).toBe("published");
    expect(outcome.localReadiness).toBeUndefined();
    expect(outcome.nextStep?.title).toContain("Continue setup");
    expect(testStorage.length).toBe(0);
  });
});

describe("Hosted Product Create UI", () => {
  it("renders canonical creation, published, and retry states", () => {
    expect(registrationSource).toContain('data-testid="create-project-form"');
    expect(registrationSource).toContain(
      'data-testid="registration-published"',
    );
    expect(registrationSource).toContain(
      'data-testid="registration-retry-needed"',
    );
    expect(registrationSource).toContain('data-testid="registration-retry"');
    expect(registrationSource).toContain("paired local daemon first");
    expect(registrationSource).toContain("Cloud never owns");
  });

  it("does not collect a workspace reference or expose the legacy Join form", () => {
    expect(registrationSource).not.toContain('id="create-project-ref"');
    expect(registrationSource).not.toContain('data-testid="join-project-form"');
    expect(registrationSource.toLowerCase()).not.toContain("invite code");
  });

  it("automatically resumes a safe pending Create after pairing reconnects", () => {
    expect(hostedDashboardSource).toContain("getPendingHostedCreateInput");
    expect(hostedDashboardSource).toContain("recoveryAttemptedFor");
    expect(hostedDashboardSource).toContain("oncreateproject(pending)");
  });

  it("preserves explicit missing Cloud session handling in App", () => {
    expect(appSource).toContain("missingCloudSessionState");
    expect(appSource).toContain('errorKind: "missing_cloud_session"');
    expect(appSource).toContain("requireCloudSession");
    expect(appSource).toContain(
      "Cloud session is missing. Reopen the hosted dashboard and sign in again.",
    );
    // Cloud-only create/join must not wipe a live daemon pair (local smoke).
    expect(appSource).toContain("registrationResult = missingCloudSessionResult()");
    expect(appSource).not.toContain(
      "bridgeState = missingCloudSessionState();",
    );
    expect(appSource).toContain("restartHostedPairing();");
    expect(hostedHandoffSource).toContain("clearHostedHandoff();");
    expect(hostedHandoffSource).toContain('navigate("/connect");');
  });
});

