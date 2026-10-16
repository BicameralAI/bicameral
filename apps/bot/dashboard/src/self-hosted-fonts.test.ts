// The hosted SPA is served under Cloud's CSP (`style-src 'self'`,
// `font-src 'self'`). Fonts must ship as same-origin bundle assets, never
// from Google Fonts, or the browser blocks them.
import { describe, expect, it } from "vitest";
import indexHtml from "../index.html?raw";
import mainSource from "./main.ts?raw";

describe("self-hosted fonts", () => {
  it("index.html loads no external font origins", () => {
    expect(indexHtml).not.toMatch(/fonts\.googleapis\.com|fonts\.gstatic\.com/);
  });

  it("main.ts bundles the brand fonts locally", () => {
    expect(mainSource).toContain("@fontsource/ibm-plex-sans/");
    expect(mainSource).toContain("@fontsource/jetbrains-mono/");
  });
});
