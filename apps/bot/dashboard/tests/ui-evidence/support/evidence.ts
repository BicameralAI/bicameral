/**
 * Evidence record + manifest plumbing for the headed UI evidence harness.
 *
 * Each successful capture writes one JSON record; `global-teardown` folds the
 * records into a single machine-readable manifest bound to the exact git SHA
 * the run was built from. The manifest is the thing a PR cites; the screenshots
 * and traces are what it points at.
 */

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

export const EVIDENCE_SUBDIR = "evidence/ui";
export const MANIFEST_SCHEMA = "bot.ui.headed_evidence.v1";

export type EvidenceRecord = {
  scenario_id: string;
  scenario_title: string;
  capture: string;
  project: string;
  viewport: { width: number; height: number };
  url_path: string;
  screenshot: string;
  assertions: string[];
  console_errors: string[];
  console_warnings: number;
  page_errors: string[];
  layout_issues: string[];
  unexpected_api_calls: string[];
  observed_api_calls: string[];
  window_open_calls: string[];
  browser: { name: string; version: string; mode: "headed" | "headless" };
  captured_at: string;
};

/**
 * The dashboard package root.
 *
 * Playwright's `config.rootDir` is the common base of the test files, not the
 * package, so evidence would otherwise land inside `tests/`. Resolve the
 * package explicitly and fail loudly if the harness is invoked from elsewhere.
 */
export function packageRoot(): string {
  let dir = process.cwd();
  for (let i = 0; i < 6; i += 1) {
    const pkg = path.join(dir, "package.json");
    if (fs.existsSync(pkg)) {
      const name = JSON.parse(fs.readFileSync(pkg, "utf8")).name;
      if (name === "bicameral-dashboard") return dir;
    }
    const parent = path.dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  throw new Error(
    "ui-evidence harness must run from the dashboard package (cwd: " +
      process.cwd() +
      ")",
  );
}

export function evidenceRoot(rootDir: string = packageRoot()): string {
  return path.join(rootDir, EVIDENCE_SUBDIR);
}

export function recordsDir(rootDir: string = packageRoot()): string {
  return path.join(evidenceRoot(rootDir), "records");
}

export function screenshotsDir(rootDir: string = packageRoot()): string {
  return path.join(evidenceRoot(rootDir), "screenshots");
}

export function writeRecord(record: EvidenceRecord, rootDir: string = packageRoot()): void {
  const dir = recordsDir(rootDir);
  fs.mkdirSync(dir, { recursive: true });
  const name = `${record.project}__${record.scenario_id}__${record.capture}.json`;
  fs.writeFileSync(path.join(dir, name), JSON.stringify(record, null, 2) + "\n");
}

function git(args: string[], cwd: string): string {
  try {
    return execFileSync("git", args, { cwd, encoding: "utf8" }).trim();
  } catch {
    return "";
  }
}

/**
 * Bind the evidence to an exact commit.
 *
 * On CI the workflow SHA is authoritative (a PR run checks out a merge commit,
 * so `GITHUB_SHA` alone would be misleading — the workflow passes the PR head
 * through `UI_EVIDENCE_SHA`). Locally we fall back to the working tree, and we
 * always record whether the tree was dirty.
 */
export function gitContext(rootDir: string = packageRoot()) {
  const head = git(["rev-parse", "HEAD"], rootDir);
  const sha = process.env.UI_EVIDENCE_SHA || head;
  const dirty = git(["status", "--porcelain"], rootDir);
  return {
    sha,
    short_sha: sha.slice(0, 12),
    checked_out_head: head,
    branch:
      process.env.UI_EVIDENCE_REF ||
      git(["rev-parse", "--abbrev-ref", "HEAD"], rootDir),
    working_tree_clean: dirty.length === 0,
  };
}

export function ciContext() {
  const server = process.env.GITHUB_SERVER_URL ?? "https://github.com";
  const repo = process.env.GITHUB_REPOSITORY ?? "";
  const runId = process.env.GITHUB_RUN_ID ?? "";
  return {
    on_ci: !!process.env.GITHUB_ACTIONS,
    workflow: process.env.GITHUB_WORKFLOW ?? "",
    run_id: runId,
    run_attempt: process.env.GITHUB_RUN_ATTEMPT ?? "",
    run_url: runId && repo ? `${server}/${repo}/actions/runs/${runId}` : "",
    artifact_name: process.env.UI_EVIDENCE_ARTIFACT ?? "",
  };
}

export function buildManifest(rootDir: string = packageRoot()) {
  const dir = recordsDir(rootDir);
  const files = fs.existsSync(dir)
    ? fs.readdirSync(dir).filter((f) => f.endsWith(".json")).sort()
    : [];
  const records: EvidenceRecord[] = files.map((f) =>
    JSON.parse(fs.readFileSync(path.join(dir, f), "utf8")),
  );

  const byScenario = new Map<string, EvidenceRecord[]>();
  for (const r of records) {
    const list = byScenario.get(r.scenario_id) ?? [];
    list.push(r);
    byScenario.set(r.scenario_id, list);
  }

  const scenarios = [...byScenario.entries()].map(([id, list]) => ({
    id,
    title: list[0].scenario_title,
    viewports: [...new Set(list.map((r) => r.project))].sort(),
    captures: list.map((r) => ({
      capture: r.capture,
      viewport: r.project,
      viewport_px: r.viewport,
      url_path: r.url_path,
      screenshot: r.screenshot,
      assertions: r.assertions,
      layout_issues: r.layout_issues,
      console_errors: r.console_errors,
      page_errors: r.page_errors,
      observed_api_calls: r.observed_api_calls,
      unexpected_api_calls: r.unexpected_api_calls,
      window_open_calls: r.window_open_calls,
      captured_at: r.captured_at,
    })),
  }));

  return {
    schema_version: 1,
    kind: MANIFEST_SCHEMA,
    generated_at: new Date().toISOString(),
    repository: process.env.GITHUB_REPOSITORY ?? "BicameralAI/bicameral-bot",
    git: gitContext(rootDir),
    ci: ciContext(),
    browser: records[0]?.browser ?? null,
    viewport_contract: [
      { name: "desktop-1440x900", width: 1440, height: 900 },
      { name: "narrow-390x844", width: 390, height: 844 },
    ],
    totals: {
      scenarios: scenarios.length,
      captures: records.length,
      console_errors: records.reduce((n, r) => n + r.console_errors.length, 0),
      page_errors: records.reduce((n, r) => n + r.page_errors.length, 0),
      layout_issues: records.reduce((n, r) => n + r.layout_issues.length, 0),
      unexpected_api_calls: records.reduce(
        (n, r) => n + r.unexpected_api_calls.length,
        0,
      ),
    },
    scenarios,
  };
}

export function writeManifest(rootDir: string = packageRoot()): string {
  const manifest = buildManifest(rootDir);
  const dest = path.join(evidenceRoot(rootDir), "manifest.json");
  fs.mkdirSync(path.dirname(dest), { recursive: true });
  fs.writeFileSync(dest, JSON.stringify(manifest, null, 2) + "\n");
  return dest;
}
