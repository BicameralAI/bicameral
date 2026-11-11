/**
 * The probe behind the liveness poll (bicameral#52).
 *
 * It calls the existing, browser-allowed `GET /api/v1/hosted-bridge/status`
 * (never `/health`, which the hosted SPA must not call cross-origin) and
 * classifies the answer for liveness only:
 *
 * - 200                              -> up
 * - transport failure, 401 or 410    -> down
 * - anything else (403, 5xx, aborted) -> inconclusive: it throws, so the
 *   monitor leaves the displayed state alone. A reachable daemon that returns a
 *   typed error is not a stopped daemon.
 *
 * Fetch directly so the monitor owns state changes after its lifetime check.
 * The shared transport's global disconnect notifications must not run here.
 */

import { hostedGatewayBase } from "./api";
import type { LivenessResult } from "./daemon-liveness";

function isTransportFailure(error: unknown): boolean {
  if (error instanceof DOMException && error.name === "AbortError") return false;
  if (error instanceof TypeError) return true;
  return error instanceof Error && /failed to fetch|networkerror/i.test(error.message);
}

export async function probeDaemonStatus(
  sessionToken: string,
): Promise<LivenessResult> {
  let response: Response;
  try {
    response = await fetch(
      `${hostedGatewayBase()}/api/v1/hosted-bridge/status`,
      { headers: { "x-bridge-session-token": sessionToken } },
    );
  } catch (error) {
    if (isTransportFailure(error)) return "down";
    throw error;
  }
  if (response.status === 401 || response.status === 410) return "down";
  if (response.ok) return "up";
  throw new Error(`liveness probe inconclusive: HTTP ${response.status}`);
}
