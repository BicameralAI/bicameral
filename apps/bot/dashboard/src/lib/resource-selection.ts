/**
 * Pure helpers for the daemon-owned Drive storage setup (ARCH-0007).
 * Kept free of fetch and component state so they can be tested directly.
 */

/** Shown when the daemon answers start with an outcome this page does not know. */
export const UNRECOGNISED_START_OUTCOME_MESSAGE =
  "The Bicameral app on this device answered in a way this page does not understand. Update Bicameral on this device, reload this page, and try again.";

/**
 * Message for a start outcome that is not `started`.
 *
 * The daemon and this page can be different versions (an older daemon still
 * answers `start_url`; a newer one may add outcomes). Such an outcome has no
 * `message`, so reading it blindly would show a blank failure.
 */
export function startFailureMessage(outcome: unknown): string {
  if (typeof outcome === "object" && outcome !== null) {
    const message = (outcome as { message?: unknown }).message;
    if (typeof message === "string" && message.trim() !== "") {
      return message;
    }
  }
  return UNRECOGNISED_START_OUTCOME_MESSAGE;
}
