import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
  server: {
    proxy: {
      "/api": "http://127.0.0.1:37373",
      "/health": "http://127.0.0.1:37373",
    },
  },
  // Under Vitest, resolve Svelte's browser (client) entry so component tests can
  // `mount(...)` in jsdom; otherwise Svelte resolves its server build and throws
  // `mount(...) is not available on the server`.
  resolve: process.env.VITEST ? { conditions: ["browser"] } : undefined,
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts"],
    // Several conformance tests intentionally import the complete Svelte
    // surface to prove the shipped component graph compiles. Cold transforms
    // can exceed Vitest's 5s default when files run in parallel, even though
    // the imports complete successfully. Keep the normal parallel command and
    // give those real compilation checks a bounded, CI-safe budget.
    testTimeout: 15_000,
  },
});
