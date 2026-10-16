/**
 * Shell version label (Langfuse-style "vX.Y.Z" next to brand).
 * Prefer workspace Cargo train when shipping; keep in sync on release anchors.
 */
export const APP_SHELL_VERSION = "0.1.25";

export function appShellVersionLabel(): string {
  return `v${APP_SHELL_VERSION.replace(/^v/, "")}`;
}
