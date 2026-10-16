import { readHostedHandoff } from "./hosted-handoff";

export type HostedDaemonDisconnectReason =
  | { kind: "transport_unavailable" }
  | { kind: "daemon_unavailable"; status: number }
  | { kind: "pairing_required"; status: 401 | 403 | 410 };

type DisconnectListener = (reason: HostedDaemonDisconnectReason) => void;

const disconnectListeners = new Set<DisconnectListener>();

export function onHostedDaemonDisconnected(
  listener: DisconnectListener,
): () => void {
  disconnectListeners.add(listener);
  return () => disconnectListeners.delete(listener);
}

function notifyDisconnected(reason: HostedDaemonDisconnectReason) {
  for (const listener of disconnectListeners) listener(reason);
}

function isTransportUnavailable(error: unknown): boolean {
  if (error instanceof DOMException && error.name === "AbortError") return false;
  if (error instanceof TypeError) return true;
  return error instanceof Error && /failed to fetch|networkerror/i.test(error.message);
}

/**
 * Attach the signed-in operator's Cloud bearer only to the bounded Google
 * credential bootstrap request. The daemon consumes this request body before
 * the legacy connection handler runs, uses the bearer for one authenticated
 * Cloud credential fetch, then discards it. No other loopback request receives
 * the Cloud bearer and it is never durable provider state.
 */
function withGoogleCredentialBootstrap(
  input: RequestInfo | URL,
  init?: RequestInit,
): RequestInit | undefined {
  const method = (init?.method ?? "GET").toUpperCase();
  if (method !== "POST") return init;

  const url =
    typeof input === "string"
      ? input
      : input instanceof URL
        ? input.toString()
        : input.url;
  let pathname = "";
  try {
    pathname = new URL(url, window.location.origin).pathname;
  } catch {
    return init;
  }
  if (pathname !== "/api/v1/connections/google/start") return init;

  const cloudAccessToken = readHostedHandoff()?.cloud_access_token;
  if (!cloudAccessToken) return init;

  const headers = new Headers(init?.headers);
  headers.set("Content-Type", "application/json");
  return {
    ...init,
    headers,
    body: JSON.stringify({ cloud_access_token: cloudAccessToken }),
  };
}

export async function fetchFromHostedDaemon(
  input: RequestInfo | URL,
  init?: RequestInit,
): Promise<Response> {
  try {
    const response = await fetch(input, withGoogleCredentialBootstrap(input, init));
    // 401 / 410: session gone → re-pair. Do NOT treat 403 as disconnect:
    // origin_mismatch and policy denials are normal on the pairing path and
    // were incorrectly forcing "Local daemon disconnected" after a good pair.
    if (response.status === 401 || response.status === 410) {
      notifyDisconnected({
        kind: "pairing_required",
        status: response.status,
      });
    } else if (response.status >= 500) {
      notifyDisconnected({
        kind: "daemon_unavailable",
        status: response.status,
      });
    }
    return response;
  } catch (error) {
    if (isTransportUnavailable(error)) {
      notifyDisconnected({ kind: "transport_unavailable" });
    }
    throw error;
  }
}
