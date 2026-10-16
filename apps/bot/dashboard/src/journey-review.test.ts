/**
 * Journey review surface tests (bot#899, D4 acceptance contract).
 *
 * Validates against the locked contract in
 * docs/specs/journey-review-dashboard.md:
 *   - authority labels are text plus structural treatment, never color alone;
 *   - ordered phase trace with persistence, authority transition, reasons;
 *   - canonical Decisions and local candidates render in separate regions;
 *   - recovery states (no-candidate, deferred, failed, missing-trace,
 *     hidden, suppressed) are explicit, never empty-as-success;
 *   - governed actions render only when advertised, revalidate before
 *     submission, and fail closed on stale capability;
 *   - suppression requires a reason;
 *   - results are announced through an aria-live region;
 *   - trace download link targets the sanitized no-store endpoint.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup } from "@testing-library/svelte";
import { tick } from "svelte";
import JourneyReview from "./pages/JourneyReview.svelte";
import { JOURNEY_FIXTURES } from "./lib/journey-review-fixtures";
import type { JourneyReviewView } from "./lib/journey-review-types";

function mockFetchView(view: JourneyReviewView) {
  return vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.includes("/api/v1/journeys/")) {
      return new Response(JSON.stringify(view), {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    }
    throw new Error(`unexpected fetch: ${url}`);
  });
}

async function renderJourney(view: JourneyReviewView) {
  vi.stubGlobal("fetch", mockFetchView(view));
  const utils = render(JourneyReview, {
    props: { initialJourneyId: view.journey_id },
  });
  // allow the initial load promise + reactive updates to settle
  await vi.waitFor(() => {
    expect(
      utils.container.querySelector(".summary"),
      "journey summary should render after load",
    ).toBeTruthy();
  });
  return utils;
}

beforeEach(() => {
  vi.restoreAllMocks();
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("authority presentation", () => {
  it("labels canonical, proposal, and derived evidence with text, not color alone", async () => {
    const { container } = await renderJourney(JOURNEY_FIXTURES.promoted());
    const tags = Array.from(container.querySelectorAll(".authority-tag"));
    const text = tags.map((t) => t.textContent?.trim() ?? "");
    expect(text.some((t) => t.includes("Canonical"))).toBe(true);
    expect(text.some((t) => t.includes("Local proposal"))).toBe(true);
    expect(text.some((t) => t.includes("Derived evidence"))).toBe(true);
    // structural treatment exists (data-authority drives border style)
    expect(
      container.querySelector('.authority-tag[data-authority="canonical"]'),
    ).toBeTruthy();
  });

  it("renders canonical Decisions and local candidates in separate labeled regions", async () => {
    const { container } = await renderJourney(JOURNEY_FIXTURES.candidate());
    expect(container.querySelector(".canonical-region")).toBeTruthy();
    expect(container.querySelector(".proposal-region")).toBeTruthy();
  });
});

describe("ordered phase trace", () => {
  it("renders phases in contract order with persistence and authority transitions", async () => {
    const view = JOURNEY_FIXTURES.candidate();
    const { container } = await renderJourney(view);
    const phases = Array.from(container.querySelectorAll(".phase h3")).map(
      (h) => h.textContent,
    );
    expect(phases).toEqual(view.phases.map((p) => p.phase_id));
    expect(container.textContent).toContain("canonical_event / append");
    expect(container.textContent).toContain("Derived evidence");
  });
});

describe("recovery states are explicit", () => {
  it("no-candidate renders an explicit state, not an empty region", async () => {
    const { container } = await renderJourney(JOURNEY_FIXTURES.noCandidate());
    expect(container.textContent).toContain(
      "No active local candidate. This is an explicit state",
    );
    expect(container.textContent).toContain(
      "No canonical Decision exists for this journey",
    );
  });

  it("deferred renders retry state", async () => {
    const { container } = await renderJourney(JOURNEY_FIXTURES.deferred());
    expect(container.textContent).toContain("Deferred (retry scheduled)");
    expect(container.textContent).toContain("retry_scheduled");
  });

  it("terminal failure renders as failed without losing canonical evidence claim", async () => {
    const { container } = await renderJourney(JOURNEY_FIXTURES.failed());
    expect(container.textContent).toContain("Failed (terminal)");
    expect(container.textContent).toContain("Canonical evidence is unchanged");
  });

  it("missing trace renders unknown, not success", async () => {
    const { container } = await renderJourney(JOURNEY_FIXTURES.missingTrace());
    expect(container.textContent).toContain("Unknown (not successful)");
    expect(container.textContent).toContain("missing_trace");
  });

  it("hidden candidates are counted, suppressed candidates labeled", async () => {
    const { container } = await renderJourney(JOURNEY_FIXTURES.disposed());
    expect(container.textContent).toContain("1 candidate(s) hidden");
    expect(container.textContent).toContain("Durably suppressed locally");
  });
});

describe("governed actions", () => {
  it("renders only daemon-advertised actions for each candidate", async () => {
    const view = JOURNEY_FIXTURES.candidate();
    const { container } = await renderJourney(view);
    const buttons = Array.from(
      container.querySelectorAll(".candidate-actions button"),
    ).map((b) => b.textContent?.trim());
    expect(buttons).toContain("Hide");
    expect(buttons).toContain("Suppress");
    expect(buttons).toContain("Promote");
    // reject was not advertised, so it must not render
    expect(buttons).not.toContain("Reject");
  });

  it("renders no action buttons when nothing is advertised", async () => {
    const view = JOURNEY_FIXTURES.candidate();
    view.allowed_actions = [];
    const { container } = await renderJourney(view);
    expect(container.querySelectorAll(".candidate-actions button").length).toBe(
      0,
    );
  });

  it("suppress requires a reason before submitting", async () => {
    const view = JOURNEY_FIXTURES.candidate();
    const utils = await renderJourney(view);
    const suppress = Array.from(
      utils.container.querySelectorAll(".candidate-actions button"),
    ).find((b) => b.textContent?.trim() === "Suppress") as HTMLButtonElement;
    suppress.click();
    await tick();
    expect(utils.container.querySelector(".reason-form")).toBeTruthy();
    const confirm = Array.from(
      utils.container.querySelectorAll(".reason-form button"),
    ).find((b) => b.textContent?.trim() === "Confirm") as HTMLButtonElement;
    confirm.click();
    await tick();
    const live = utils.container.querySelector(".live-region");
    expect(live?.textContent).toContain("A reason is required");
  });

  it("fails closed when the action is no longer advertised at revalidation", async () => {
    const advertised = JOURNEY_FIXTURES.candidate();
    const stale = JOURNEY_FIXTURES.candidate();
    stale.allowed_actions = [];
    let call = 0;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL) => {
        const url = String(input);
        if (url.includes("/api/v1/journeys/")) {
          call += 1;
          // first call renders the advertised view; every later view is stale
          const body = call === 1 ? advertised : stale;
          return new Response(JSON.stringify(body), { status: 200 });
        }
        throw new Error(`governed mutation must not fire on stale capability: ${url}`);
      }),
    );
    const utils = render(JourneyReview, {
      props: { initialJourneyId: advertised.journey_id },
    });
    await vi.waitFor(() => {
      expect(utils.container.querySelector(".summary")).toBeTruthy();
    });
    const hide = Array.from(
      utils.container.querySelectorAll(".candidate-actions button"),
    ).find((b) => b.textContent?.trim() === "Hide") as HTMLButtonElement;
    hide.click();
    await vi.waitFor(() => {
      const live = utils.container.querySelector(".live-region");
      expect(live?.textContent).toContain("no longer advertised");
    });
  });
});

describe("accessibility structure", () => {
  it("exposes an aria-live status region and labeled landmarks", async () => {
    const utils = await renderJourney(JOURNEY_FIXTURES.candidate());
    const live = utils.container.querySelector('[aria-live="polite"]');
    expect(live).toBeTruthy();
    expect(live?.getAttribute("role")).toBe("status");
    expect(
      utils.container.querySelector('[aria-labelledby="journey-review-heading"]'),
    ).toBeTruthy();
    expect(
      utils.container.querySelector('[aria-labelledby="journey-phases-heading"]'),
    ).toBeTruthy();
  });

  it("journey lookup is a labeled form control reachable by keyboard", async () => {
    await renderJourney(JOURNEY_FIXTURES.candidate());
    const input = screen.getByLabelText("Journey identifier");
    expect(input).toBeTruthy();
    expect((input as HTMLInputElement).type).toBe("text");
  });
});

describe("trace download", () => {
  it("links the sanitized trace endpoint with a download attribute", async () => {
    const view = JOURNEY_FIXTURES.candidate();
    const { container } = await renderJourney(view);
    const link = container.querySelector(
      ".trace-download",
    ) as HTMLAnchorElement;
    expect(link.getAttribute("href")).toBe(
      `/api/v1/journeys/${view.journey_id}/trace.json`,
    );
    expect(link.hasAttribute("download")).toBe(true);
  });
});
