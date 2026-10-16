/**
 * Hosted Product create: Google Drive folder selection (bot#1264).
 *
 * The operator-scoped daemon rejects `product.create` without a
 * daemon-validated Drive folder descriptor. These tests prove the shipped
 * hosted path drives the daemon's existing resource-selection flow, carries
 * the validated folder id into `product.create` verbatim, never invents
 * readiness, and never handles provider token material.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import appSource from "./App.svelte?raw";
import registrationSource from "./pages/HostedProjectRegistration.svelte?raw";
import hostedDashboardSource from "./pages/HostedDashboard.svelte?raw";
import apiSource from "./lib/api.ts?raw";
import typesSource from "./lib/types.ts?raw";

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
    .replace(/=+$/, "");
  return `header.${payload}.signature`;
}

function requestBody(call: unknown[]): Record<string, unknown> {
  return JSON.parse((call[1] as RequestInit).body as string) as Record<
    string,
    unknown
  >;
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
    now: () => "2026-09-11T17:00:00Z",
    ...overrides,
  };
}

function rejectedCreate(message: string): unknown {
  return {
    request_id: "req_create",
    status: "rejected",
    message,
    responded_at: "2026-09-11T17:00:01Z",
  };
}

beforeEach(() => {
  vi.restoreAllMocks();
  testStorage.clear();
});

describe("product.create carries the daemon-validated folder descriptor", () => {
  it("sends event_store_descriptor with the exact folder id the daemon validated", async () => {
    const spy = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(
        jsonResponse(rejectedCreate("product_event_store_unvalidated: test stop")),
      );
    const { createHostedProject } = await import(
      "./lib/hosted-project-registration"
    );
    await createHostedProject(
      createOptions({ eventStoreFolderId: "1AbC_folder-Id" }),
    );

    expect(spy).toHaveBeenCalledTimes(1);
    const body = requestBody(spy.mock.calls[0]);
    const command = body.command as Record<string, unknown>;
    expect(command.name).toBe("product.create");
    expect(command.params).toMatchObject({
      product_display_name: "Checkout Service",
      event_store_descriptor: { provider: "google", folder_id: "1AbC_folder-Id" },
    });
  });

  it("omits the descriptor when no folder was validated so the daemon fails closed", async () => {
    const spy = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(
        jsonResponse(rejectedCreate("product_event_store_required: test stop")),
      );
    const { createHostedProject } = await import(
      "./lib/hosted-project-registration"
    );
    const outcome = await createHostedProject(createOptions());
    const params = (requestBody(spy.mock.calls[0]).command as { params: unknown })
      .params as Record<string, unknown>;
    expect("event_store_descriptor" in params).toBe(false);
    expect(outcome.phase).toBe("failed");
    expect(outcome.phase === "failed" && outcome.message).toContain(
      "product_event_store_required",
    );
  });

  it("keeps the validated folder on the pending marker so a replayed create reuses it", async () => {
    vi.spyOn(globalThis, "fetch").mockRejectedValueOnce(
      new TypeError("Failed to fetch"),
    );
    const { createHostedProject } = await import(
      "./lib/hosted-project-registration"
    );
    const first = await createHostedProject(
      createOptions({ eventStoreFolderId: "1AbC_folder-Id" }),
    );
    expect(first.phase).toBe("create_retry_needed");

    const spy = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(
        jsonResponse(rejectedCreate("product_event_store_unvalidated: test stop")),
      );
    // Retry without re-supplying the folder: the marker carries it.
    await createHostedProject(createOptions());
    const params = (requestBody(spy.mock.calls[0]).command as { params: unknown })
      .params as Record<string, unknown>;
    expect(params.event_store_descriptor).toEqual({
      provider: "google",
      folder_id: "1AbC_folder-Id",
    });
    expect(params.idempotency_key).toBe("create_create-key");
  });
});

describe("hosted SPA drives the daemon resource-selection flow", () => {
  it("starts selection through the daemon route and polls the redacted projection", () => {
    expect(apiSource).toContain(
      "/api/v1/setup/google/resource-selection/start",
    );
    expect(apiSource).toContain(
      "/api/v1/setup/google/resource-selection/${encodeURIComponent(",
    );
    expect(appSource).toContain("startHostedResourceSelection(bridgeSessionToken)");
    expect(appSource).toContain("pollHostedResourceSelection(");
    // Readiness comes only from a daemon `ready` projection with a resource.
    expect(appSource).toContain(
      'projection.state === "ready" && projection.resource',
    );
    // The create request transports the daemon-issued folder id, nothing else.
    expect(appSource).toContain("eventStoreFolderId: driveFolder?.folder_id");
    // A daemon refusal of a stale/unvalidated folder drops the selection so
    // the user re-picks rather than replaying a dead descriptor.
    expect(appSource).toContain(
      'registrationResult.message.startsWith("product_event_store_unvalidated")',
    );
  });

  it("gates Create on a daemon-validated folder in the shipped dialog", () => {
    expect(hostedDashboardSource).toContain("folderPhase={driveFolderPhase}");
    expect(registrationSource).toContain(
      'const folderReady = $derived(!folderStepActive || folderPhase === "ready");',
    );
    expect(registrationSource).toContain(
      "eventSubstrateConnected && folderReady",
    );
    expect(registrationSource).toContain('data-testid="create-drive-folder-choose"');
    expect(registrationSource).toContain('"Choose folder"');
  });

  it("never exposes internal descriptor vocabulary or provider secrets to the browser surface", () => {
    expect(registrationSource).not.toContain("event_store_descriptor");
    expect(registrationSource).not.toContain("descriptor");
    // The projection the SPA consumes carries folder metadata only.
    const projection = typesSource.slice(
      typesSource.indexOf("export type ProviderResourceDescriptor"),
      typesSource.indexOf("export type ResourceSelectionStartOutcome"),
    );
    for (const forbidden of [
      "access_token",
      "refresh_token",
      "code_verifier",
      "client_secret",
      "authorization_code",
    ]) {
      expect(projection).not.toContain(forbidden);
    }
    // The SPA never posts to the selection callback; only the daemon's
    // loopback receives Google's redirect.
    expect(apiSource).not.toContain("resource-selection/callback");
    expect(appSource).not.toContain("picked_file_ids");
  });
});
