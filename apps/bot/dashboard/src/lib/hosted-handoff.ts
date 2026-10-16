/** Hosted-bridge handoff: pairing token passed from the cloud auth shell.
 *
 * When Hosted cloud serves the SPA, the Cloud `/connect` shell does
 * Supabase auth + daemon pairing, then stores the resulting bridge session
 * here and redirects to `/app`. The SPA reads it to enter hosted mode and
 * pull redacted projections directly from the paired daemon.
 *
 * Authority-bearing tokens remain in `sessionStorage` only. Cross-visit
 * recovery uses a separate, bounded opaque restore credential in
 * `localStorage`: request handle + poll proof + account/daemon binding +
 * daemon-authored session expiry. The durable record never contains the
 * bridge session token or Cloud access token (#1109).
 */

const HANDOFF_KEY = "bicameral_hosted_bridge";
const RESTORE_KEY = "bicameral_hosted_bridge_restore";
/** Canonical local daemon port — matches daemon default_port = 37373. */
const DEFAULT_DAEMON_ORIGIN = "http://localhost:37373";

export type HostedHandoff = {
  session_token: string;
  daemon_url: string;
  cloud_account_id: string;
  expires_at?: string;
  /**
   * Supabase access token for Cloud project discovery (bot#720). The cloud
   * `/connect` shell already authenticates with Supabase before pairing; it
   * passes the bearer here so the SPA can call same-origin Cloud APIs.
   *
   * Row 5 has one explicit daemon handoff exception: the SPA may include this
   * transient bearer only in `POST /api/v1/connections/google/start` so the
   * daemon can perform one authenticated Google Desktop credential fetch from
   * Cloud. The gateway consumes and discards it before the connection handler
   * runs. It is never stored as provider state and never written to localStorage.
   */
  cloud_access_token?: string;
  /** Cloud team bound at /connect pair initiation (bot#1290). */
  team_id?: string;
  /** Cloud API origin for discovery; omit for same-origin (cloud serves the SPA). */
  cloud_api_base?: string;
};

/**
 * Durable recovery material for a previously approved hosted pair.
 *
 * This is deliberately not an authority-bearing session. Cloud must
 * re-authenticate the current Supabase user, and the daemon must revalidate
 * origin + account + poll proof and confirm that the original session is still
 * live and not revoked before releasing the transient bridge token again.
 */
export type HostedRestoreCredential = {
  request_handle: string;
  poll_proof: string;
  daemon_url: string;
  cloud_account_id: string;
  expires_at: string;
};

/** Read the pairing handoff, if the cloud shell left one. */
export function readHostedHandoff(): HostedHandoff | null {
  let raw = "";
  try {
    raw = window.sessionStorage.getItem(HANDOFF_KEY) || "";
  } catch {
    return null;
  }
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw) as Partial<HostedHandoff>;
    if (!parsed.session_token) return null;
    return {
      session_token: parsed.session_token,
      daemon_url: (parsed.daemon_url || DEFAULT_DAEMON_ORIGIN).replace(/\/+$/, ""),
      cloud_account_id: parsed.cloud_account_id || "",
      expires_at: parsed.expires_at,
      cloud_access_token: parsed.cloud_access_token,
      team_id: parsed.team_id,
      cloud_api_base: parsed.cloud_api_base
        ? parsed.cloud_api_base.replace(/\/+$/, "")
        : undefined,
    };
  } catch {
    return null;
  }
}

/** Clear the transient authority-bearing handoff. */
export function clearHostedHandoff(): void {
  try {
    window.sessionStorage.removeItem(HANDOFF_KEY);
  } catch {
    /* ignore */
  }
}

/** Clear only the bounded cross-visit recovery credential. */
export function clearHostedRestoreCredential(): void {
  try {
    window.localStorage.removeItem(RESTORE_KEY);
  } catch {
    /* ignore */
  }
}

/**
 * Read a structurally valid, unexpired restore credential.
 *
 * Expired or malformed durable state is deleted immediately so `/app` cannot
 * loop through `/restore`. Daemon revocation remains authoritative and is
 * checked only by Cloud's restore exchange.
 */
export function readHostedRestoreCredential(
  nowMs: number = Date.now(),
): HostedRestoreCredential | null {
  let raw = "";
  try {
    raw = window.localStorage.getItem(RESTORE_KEY) || "";
  } catch {
    return null;
  }
  if (!raw) return null;

  try {
    const parsed = JSON.parse(raw) as Partial<HostedRestoreCredential>;
    const expiresAtMs = parsed.expires_at ? Date.parse(parsed.expires_at) : Number.NaN;
    if (
      !parsed.request_handle ||
      !parsed.poll_proof ||
      !parsed.cloud_account_id ||
      !parsed.expires_at ||
      !Number.isFinite(expiresAtMs) ||
      expiresAtMs <= nowMs
    ) {
      clearHostedRestoreCredential();
      return null;
    }
    return {
      request_handle: parsed.request_handle,
      poll_proof: parsed.poll_proof,
      daemon_url: (parsed.daemon_url || DEFAULT_DAEMON_ORIGIN).replace(/\/+$/, ""),
      cloud_account_id: parsed.cloud_account_id,
      expires_at: parsed.expires_at,
    };
  } catch {
    clearHostedRestoreCredential();
    return null;
  }
}

/** Where an unpaired production `/app` should recover. */
export function hostedRecoveryPath(nowMs: number = Date.now()): "/restore" | "/connect" {
  return readHostedRestoreCredential(nowMs) ? "/restore" : "/connect";
}

/** Drop stale transient authority, then attempt bounded restore before fresh pairing. */
export function recoverHostedPairing(
  navigate: (path: string) => void = (path) => {
    if (typeof window !== "undefined") {
      window.location.assign(path);
    }
  },
): void {
  clearHostedHandoff();
  navigate(hostedRecoveryPath());
}

/** Clear a stale hosted session so the SPA can start a fresh explicit pair. */
export function restartHostedPairing(
  navigate: (path: string) => void = () => {
    // Prefer staying on the current document: HostedDashboard renders
    // HostedPairingScreen when unpaired. Navigating to /connect reloads the
    // SPA on Vite and caused infinite refresh loops.
    if (typeof window !== "undefined") {
      window.location.reload();
    }
  },
): void {
  clearHostedHandoff();
  clearHostedRestoreCredential();
  navigate("/connect");
}

/** Resolve the daemon origin for pairing when there is no handoff yet.
 * Honors the existing `bicameral_daemon_url` per-machine override. */
export function resolveDaemonOrigin(): string {
  try {
    const override = (window.localStorage.getItem("bicameral_daemon_url") || "").trim();
    if (override) return override.replace(/\/+$/, "");
  } catch {
    /* ignore */
  }
  return DEFAULT_DAEMON_ORIGIN;
}
