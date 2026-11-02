/**
 * Hosted shell version truthfulness (bot#1347, bicameral#53).
 *
 * The hosted header used to render a hard-coded `v0.1.25` regardless of the
 * installed daemon or the served dashboard build, which misled operators on a
 * fresh v0.1.35 install. The SPA has no trusted daemon-version signal (hosted
 * compatibility is keyed on the integer `daemon_protocol_version`), so the
 * shell must not present a version it cannot verify.
 */

import { describe, it, expect } from "vitest";
import appSource from "./App.svelte?raw";

describe("hosted shell version", () => {
  it("does not render a version label in the hosted header", () => {
    expect(appSource).not.toContain('data-testid="app-shell-version"');
    expect(appSource).not.toContain("appShellVersionLabel");
  });
});
