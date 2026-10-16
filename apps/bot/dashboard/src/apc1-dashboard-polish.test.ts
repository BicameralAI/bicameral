import { afterEach, describe, expect, it, vi } from "vitest";
import {
  publishHostedProductToCloud,
  type ProductCreationPublication,
} from "./lib/hosted-project-registration";

function publication(): ProductCreationPublication {
  return {
    publication_id: "pub_apc1",
    product_id: "prd_apc1",
    creator_cloud_account_id: "acct_apc1",
    verified_domain: "acme.example",
    canonical: false,
    idempotency_key: "publish_apc1",
    canonical_creation_receipt_ref: "receipt_apc1",
    published_at: "2026-09-23T19:00:00Z",
  };
}

afterEach(() => vi.restoreAllMocks());

describe("APC1 structured error rendering", () => {
  it("renders FastAPI/Pydantic validation detail instead of [object Object]", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          detail: [
            {
              loc: ["body", "first_event_commit_proof", "schema_version"],
              msg: "Input should be 1",
              type: "literal_error",
            },
          ],
        }),
        { status: 422, headers: { "content-type": "application/json" } },
      ),
    );

    await expect(
      publishHostedProductToCloud({
        accessToken: "cloud-token",
        apiBase: "https://cloud.example",
        displayName: "Checkout Service",
        eligibleDomain: "acme.example",
        publication: publication(),
      }),
    ).rejects.toThrow(
      "first_event_commit_proof.schema_version: Input should be 1",
    );
  });

  it("preserves a normal string detail unchanged", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValueOnce(
      new Response(JSON.stringify({ detail: "Directory unavailable" }), {
        status: 503,
        headers: { "content-type": "application/json" },
      }),
    );

    await expect(
      publishHostedProductToCloud({
        accessToken: "cloud-token",
        apiBase: "https://cloud.example",
        displayName: "Checkout Service",
        eligibleDomain: "acme.example",
        publication: publication(),
      }),
    ).rejects.toThrow("Directory unavailable");
  });
});
