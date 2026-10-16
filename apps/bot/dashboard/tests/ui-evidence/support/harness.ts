/**
 * Reusable headed UI evidence fixture.
 *
 * A spec describes a scenario (what the daemon and Cloud report), opens the
 * real built SPA in a real Chromium browser, asserts what the operator must be
 * able to see and do, and captures a screenshot on SUCCESS. Every capture is
 * gated on the same non-negotiables:
 *
 *   - no uncaught page error;
 *   - no browser console error;
 *   - no unmodelled `/api/**` call;
 *   - no horizontal overflow and no clipped primary control;
 *   - no harness secret marker anywhere in the rendered DOM or the URL.
 *
 * Anything that fails those is a failed capture, not a caveat in a PR comment.
 */

import { test as base, expect, type Page, type TestInfo } from "@playwright/test";
import {
  installDaemonStub,
  DaemonStub,
  HARNESS,
  SECRET_MARKERS,
  type DaemonScenario,
} from "./daemon-stub";
import { packageRoot, screenshotsDir, writeRecord, type EvidenceRecord } from "./evidence";
import fs from "node:fs";
import path from "node:path";

/** Console noise that is never a product defect. Keep this list near-empty. */
const CONSOLE_ALLOWLIST: RegExp[] = [
  /favicon\.(ico|svg)/i,
];

export type OpenOptions = {
  /** Stable id used in filenames and the manifest. */
  id: string;
  /** Human sentence describing what this scenario proves. */
  title: string;
  /** Deterministic daemon/Cloud responses for this scenario. */
  daemon: DaemonScenario;
  /** SPA path to enter at. Defaults to `/app`. */
  path?: string;
  /**
   * Seed a valid hosted handoff in `sessionStorage` (the post-pair state Cloud
   * hands to Bot). `false` models direct/unpaired entry.
   */
  handoff?: boolean;
  /** Override individual handoff fields (e.g. drop the Cloud access token). */
  handoffOverrides?: Record<string, unknown>;
};

export type CaptureOptions = {
  /** Short capture name, e.g. `01-action-required`. */
  name: string;
  /** What this capture proves, in operator terms. One sentence each. */
  assertions: string[];
  /** Selectors that must be present, sized, and inside the viewport. */
  primaryControls?: string[];
};

export class UiEvidence {
  stub!: DaemonStub;
  private scenarioId = "";
  private scenarioTitle = "";
  private consoleErrors: string[] = [];
  private consoleWarnings = 0;
  private pageErrors: string[] = [];

  constructor(
    readonly page: Page,
    private readonly info: TestInfo,
  ) {
    page.on("console", (msg) => {
      const text = msg.text();
      if (msg.type() === "error") {
        if (!CONSOLE_ALLOWLIST.some((re) => re.test(text))) {
          this.consoleErrors.push(text);
        }
      } else if (msg.type() === "warning") {
        this.consoleWarnings += 1;
      }
    });
    page.on("pageerror", (err) => this.pageErrors.push(`${err.name}: ${err.message}`));
  }

  private baseUrl(): string {
    const url = this.info.project.use.baseURL;
    if (!url) throw new Error("baseURL is not configured");
    return url.replace(/\/+$/, "");
  }

  /**
   * Enter the SPA under a scenario.
   *
   * The handoff's `daemon_url` points at the preview origin so that daemon
   * calls stay same-origin and are fully interceptable. That changes only where
   * the SPA sends the request, never what it is allowed to conclude from the
   * answer.
   */
  async open(opts: OpenOptions): Promise<void> {
    this.scenarioId = opts.id;
    this.scenarioTitle = opts.title;
    this.stub = await installDaemonStub(this.page, opts.daemon);

    // A provider handoff calls `window.open` with a daemon-authored URL. Record
    // the call and suppress the navigation: an evidence run must never leave
    // for a third-party site, and the URL the surface tried to open is itself
    // evidence worth asserting. Suppressing it changes nothing the SPA is
    // allowed to conclude — the component already documents that the return
    // value of `window.open` is not connection evidence.
    await this.page.addInitScript(() => {
      const calls: string[] = [];
      (window as unknown as { __uiEvidenceWindowOpen: string[] }).__uiEvidenceWindowOpen = calls;
      window.open = ((url?: string | URL, target?: string, features?: string) => {
        calls.push(`${String(url ?? "")} target=${target ?? ""} features=${features ?? ""}`);
        return null;
      }) as typeof window.open;
    });

    if (opts.handoff !== false) {
      const handoff = {
        session_token: HARNESS.sessionToken,
        daemon_url: this.baseUrl(),
        cloud_account_id: HARNESS.accountId,
        expires_at: "2099-01-01T00:00:00Z",
        cloud_access_token: HARNESS.cloudAccessToken,
        ...(opts.handoffOverrides ?? {}),
      };
      await this.page.addInitScript(
        ({ key, value }: { key: string; value: string }) => {
          window.sessionStorage.setItem(key, value);
        },
        { key: "bicameral_hosted_bridge", value: JSON.stringify(handoff) },
      );
    }

    await this.page.goto(opts.path ?? "/app");
  }

  /** Layout defects a screenshot alone would not prove absent. */
  private async layoutIssues(selectors: string[]): Promise<string[]> {
    return this.page.evaluate((sels: string[]) => {
      const issues: string[] = [];
      const vw = window.innerWidth;
      const docWidth = Math.max(
        document.documentElement.scrollWidth,
        document.body?.scrollWidth ?? 0,
      );
      if (docWidth > vw + 1) {
        issues.push(`horizontal overflow: content ${docWidth}px in ${vw}px viewport`);
      }
      for (const sel of sels) {
        const el = document.querySelector(sel);
        if (!el) {
          issues.push(`primary control missing: ${sel}`);
          continue;
        }
        const r = el.getBoundingClientRect();
        if (r.width < 1 || r.height < 1) {
          issues.push(`primary control has no rendered box: ${sel}`);
          continue;
        }
        if (r.right > vw + 1 || r.left < -1) {
          issues.push(
            `primary control clipped horizontally: ${sel} (${Math.round(r.left)}..${Math.round(r.right)} in ${vw}px)`,
          );
        }
      }
      return issues;
    }, selectors);
  }

  /**
   * Capture a SUCCESS screenshot plus its machine-readable record.
   *
   * Throws if any gate fails, so a screenshot can only exist for a state that
   * actually passed.
   */
  async capture(opts: CaptureOptions): Promise<void> {
    const primary = opts.primaryControls ?? [];
    const layout = await this.layoutIssues(primary);

    // No credential material may reach a rendered surface or an artifact path.
    const html = await this.page.content();
    const url = this.page.url();
    const leaked = SECRET_MARKERS.filter(
      (marker) => html.includes(marker) || url.includes(marker),
    );

    const project = this.info.project.name;
    const rootDir = packageRoot();
    const relative = path.join(
      "screenshots",
      project,
      `${this.scenarioId}--${opts.name}.png`,
    );
    const dest = path.join(screenshotsDir(rootDir), project, `${this.scenarioId}--${opts.name}.png`);
    fs.mkdirSync(path.dirname(dest), { recursive: true });

    const viewport = this.page.viewportSize() ?? { width: 0, height: 0 };
    const browser = this.page.context().browser();

    // Hard gates. Assert before writing the record so a written record always
    // corresponds to a state that passed every gate.
    expect(this.pageErrors, `uncaught page errors in ${this.scenarioId}/${opts.name}`).toEqual([]);
    expect(this.consoleErrors, `browser console errors in ${this.scenarioId}/${opts.name}`).toEqual([]);
    expect(
      this.stub.unexpected.map((c) => `${c.method} ${c.path}`),
      `unmodelled API calls in ${this.scenarioId}/${opts.name}`,
    ).toEqual([]);
    expect(layout, `layout defects in ${this.scenarioId}/${opts.name}`).toEqual([]);
    expect(leaked, `harness secret marker rendered in ${this.scenarioId}/${opts.name}`).toEqual([]);

    await this.screenshotWithRetry(dest);

    const record: EvidenceRecord = {
      scenario_id: this.scenarioId,
      scenario_title: this.scenarioTitle,
      capture: opts.name,
      project,
      viewport,
      url_path: new URL(url).pathname,
      screenshot: relative.split(path.sep).join("/"),
      assertions: opts.assertions,
      console_errors: [...this.consoleErrors],
      console_warnings: this.consoleWarnings,
      page_errors: [...this.pageErrors],
      layout_issues: layout,
      unexpected_api_calls: this.stub.unexpected.map((c) => `${c.method} ${c.path}`),
      observed_api_calls: [...new Set(this.stub.paths())].sort(),
      window_open_calls: await this.windowOpenCalls(),
      browser: {
        name: "chromium",
        version: browser?.version() ?? "unknown",
        mode: process.env.UI_EVIDENCE_HEADED === "1" ? "headed" : "headless",
      },
      captured_at: new Date().toISOString(),
    };
    writeRecord(record, rootDir);
    await this.info.attach(`${this.scenarioId}--${opts.name}`, {
      path: dest,
      contentType: "image/png",
    });
  }

  /**
   * Capture the full page, tolerating a compositor that has not produced a
   * frame yet.
   *
   * A real windowed Chromium under Xvfb intermittently answers
   * `Page.captureScreenshot` with "Unable to capture screenshot" on the first
   * full-page capture of a tall surface. That is a capture-timing artifact of
   * headed CI, not a product defect, and it must not be allowed to look like
   * one — but it also must not be papered over with a blanket test retry, which
   * would let genuinely flaky product behaviour pass on a second attempt.
   */
  private async screenshotWithRetry(dest: string): Promise<void> {
    let lastError: unknown;
    for (let attempt = 0; attempt < 3; attempt += 1) {
      try {
        await this.page.screenshot({
          path: dest,
          fullPage: true,
          animations: "disabled",
          caret: "hide",
        });
        return;
      } catch (error) {
        lastError = error;
        await this.page.waitForTimeout(750);
      }
    }
    throw lastError;
  }

  /** Every `window.open` the surface attempted, as `url target=... features=...`. */
  async windowOpenCalls(): Promise<string[]> {
    return this.page.evaluate(
      () =>
        (window as unknown as { __uiEvidenceWindowOpen?: string[] })
          .__uiEvidenceWindowOpen ?? [],
    );
  }

  /** Assert a contract hop was never invoked (e.g. `product.join` on repair). */
  expectNeverCalled(pattern: string | RegExp, why: string): void {
    expect(this.stub.countMatching(pattern), why).toBe(0);
  }
}

export const test = base.extend<{ ui: UiEvidence }>({
  ui: async ({ page }, use, testInfo) => {
    await use(new UiEvidence(page, testInfo));
  },
});

export { expect, HARNESS };
export type { DaemonScenario };
