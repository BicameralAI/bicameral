/**
 * Guards that fixture data never contains token, secret, or
 * credential-shaped fields. Runs as a plain TypeScript compile-time
 * and runtime check — no test framework required.
 *
 * Execute: npx tsx scripts/check-fixture-secrets.ts
 */

import {
  SAMPLE_CONNECTIONS,
  SAMPLE_SOURCE_BINDINGS,
  SAMPLE_CHANNEL_ROUTES,
  SAMPLE_TOOL_SUBMISSIONS,
} from "../dashboard/src/lib/connection-fixtures";

const FORBIDDEN_KEYS = [
  "token",
  "secret",
  "password",
  "credential",
  "api_key",
  "access_token",
  "refresh_token",
  "oauth_token",
  "pat",
  "signing_secret",
  "private_key",
  "client_secret",
];

function assertNoSecretFields(label: string, obj: unknown): void {
  if (obj === null || obj === undefined) return;
  if (typeof obj !== "object") return;

  for (const [key, value] of Object.entries(obj as Record<string, unknown>)) {
    const lower = key.toLowerCase();
    for (const forbidden of FORBIDDEN_KEYS) {
      if (lower === forbidden || lower.endsWith("_" + forbidden)) {
        throw new Error(
          `${label}: forbidden secret-shaped key "${key}" found in fixture data`,
        );
      }
    }
    if (typeof value === "string" && /^(ghp_|gho_|github_pat_|cog_|ya29\.|sk-)/.test(value)) {
      throw new Error(
        `${label}: value for "${key}" looks like a real credential`,
      );
    }
    if (typeof value === "object") {
      assertNoSecretFields(`${label}.${key}`, value);
    }
  }
}

let failures = 0;

function check(label: string, items: unknown[]) {
  for (let i = 0; i < items.length; i++) {
    try {
      assertNoSecretFields(`${label}[${i}]`, items[i]);
    } catch (e) {
      console.error((e as Error).message);
      failures++;
    }
  }
}

check("SAMPLE_CONNECTIONS", SAMPLE_CONNECTIONS);
check("SAMPLE_SOURCE_BINDINGS", SAMPLE_SOURCE_BINDINGS);
check("SAMPLE_CHANNEL_ROUTES", SAMPLE_CHANNEL_ROUTES);
check("SAMPLE_TOOL_SUBMISSIONS", SAMPLE_TOOL_SUBMISSIONS);

if (failures > 0) {
  console.error(`\n${failures} fixture secret-guard failure(s)`);
  process.exit(1);
} else {
  console.log("fixture secret-guard: all checks passed");
}
