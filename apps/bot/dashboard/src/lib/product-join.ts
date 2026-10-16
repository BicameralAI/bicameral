/**
 * Per-Product same-domain auto-join orchestration for Operator Home (bot#768).
 *
 * Implements the SPA side of the `product.join` contract
 * (`docs/specs/domain-product-onboarding-contract.md`,
 * `protocol/onboarding/`): on a per-Product `Join`, the SPA asks Cloud to
 * exchange its authenticated session for a narrow `ProductJoinTicket`, then
 * submits a `product.join` to the paired local daemon and renders the daemon's
 * typed `ProductJoinResult`.
 *
 * Boundary: the SPA never writes canonical membership and never sees the full
 * hosted session token — it only carries a narrow ticket to the daemon. The
 * daemon's typed result is the sole authority for whether the UI may move a row
 * from `Available` to `Your products`. This module maps typed Cloud/daemon
 * failures to concise, user-facing recovery copy with no implementation or
 * governance jargon (AC5). It performs no ticket signing, Cloud persistence, or
 * daemon membership materialization — those are #768 non-goals owned by the
 * runtime/Cloud slices; this module calls the contract seam and renders states.
 *
 * bot#1013: transport / Cloud hop / join-not-configured are distinct recovery
 * buckets so "couldn't reach local app" is not a catch-all for capability gaps.
 */

import type {
  HostedProjectPreview,
  ProductJoinFailureCode,
  ProductJoinResult,
  SignedProductJoinTicket,
} from "./onboarding-contract";
import { requestProductJoinTicket, submitProductJoin } from "./api";

// ── Join phase (transient UI state only; never persisted) ────────────────────

/**
 * Transient per-row join phase. `Joining…` is UI-only state (contract §
 * Lifecycle Minimization) — there is no durable pending/review state.
 */
export type ProductJoinPhase = "idle" | "joining" | "granted" | "failed";

/**
 * A recoverable failure bucket. Extends the daemon's typed rejection codes with
 * hop-specific transport/capability buckets so the UI can offer safe recovery
 * without blaming the wrong system.
 */
export type ProductJoinRecovery =
  | ProductJoinFailureCode
  | "unavailable"
  | "cloud_unavailable"
  | "join_not_configured";

export type ProductJoinFailureView = {
  code: ProductJoinRecovery;
  /** Concise user-facing recovery copy; no implementation/governance jargon. */
  message: string;
  /** Whether a plain retry can plausibly succeed (safe to retry regardless). */
  retryable: boolean;
};

/**
 * User-facing recovery copy for every typed failure. Deliberately free of
 * bridge/ledger/ticket/governance language (AC5). Every message is safe to show
 * before any membership exists.
 */
export const PRODUCT_JOIN_FAILURE_COPY: Record<
  ProductJoinRecovery,
  ProductJoinFailureView
> = {
  expired_ticket: {
    code: "expired_ticket",
    message: "That took too long, so the request expired. Try joining again.",
    retryable: true,
  },
  replayed_ticket: {
    code: "replayed_ticket",
    message: "That join request was already used. Try joining again.",
    retryable: true,
  },
  wrong_product: {
    code: "wrong_product",
    message: "This product’s details changed. Refresh and try joining again.",
    retryable: true,
  },
  wrong_account: {
    code: "wrong_account",
    message:
      "We couldn’t confirm this is your account on this device. " +
      "Sign in again, then try joining.",
    retryable: true,
  },
  wrong_domain: {
    code: "wrong_domain",
    message: "This product isn’t open to your email domain.",
    retryable: false,
  },
  unavailable: {
    code: "unavailable",
    message:
      "We couldn’t reach your local Bicameral app. " +
      "Make sure it’s running, then try again.",
    retryable: true,
  },
  cloud_unavailable: {
    code: "cloud_unavailable",
    message:
      "We couldn’t finish joining online right now. " +
      "Check your connection and try again.",
    retryable: true,
  },
  join_not_configured: {
    code: "join_not_configured",
    message:
      "This device isn’t set up to join products yet. " +
      "Update Bicameral, restart the local app, then try again.",
    retryable: true,
  },
};

export function failureView(code: ProductJoinRecovery): ProductJoinFailureView {
  return PRODUCT_JOIN_FAILURE_COPY[code];
}

/**
 * Map a thrown hop error to a recovery bucket (bot#1013).
 *
 * Prefer specific capability / Cloud failures over the offline catch-all.
 */
export function recoveryFromJoinError(
  error: unknown,
  hop: "cloud" | "daemon",
): ProductJoinRecovery {
  const message =
    error instanceof Error
      ? error.message
      : typeof error === "string"
        ? error
        : "";
  const status =
    typeof error === "object" &&
    error !== null &&
    "status" in error &&
    typeof (error as { status: unknown }).status === "number"
      ? (error as { status: number }).status
      : undefined;
  const body =
    typeof error === "object" && error !== null && "body" in error
      ? (error as { body: unknown }).body
      : undefined;
  const bodyText =
    typeof body === "object" && body !== null
      ? JSON.stringify(body)
      : typeof body === "string"
        ? body
        : "";
  const combined = `${message} ${bodyText}`.toLowerCase();

  if (
    combined.includes("unsupported_capability") ||
    combined.includes("requires configured issuer trust") ||
    combined.includes("product.join requires") ||
    combined.includes("join not configured")
  ) {
    return "join_not_configured";
  }

  if (hop === "cloud") {
    // Cloud responded with a typed HTTP failure (signer down, auth, eligibility).
    if (status !== undefined && status > 0) {
      return "cloud_unavailable";
    }
    // Network failure talking to Cloud (no local app involved).
    if (
      error instanceof TypeError ||
      /failed to fetch|networkerror|load failed/i.test(message)
    ) {
      return "cloud_unavailable";
    }
    return "cloud_unavailable";
  }

  // Daemon hop: real transport vs reachable-but-rejected.
  if (
    error instanceof TypeError ||
    /failed to fetch|networkerror|load failed/i.test(message)
  ) {
    return "unavailable";
  }
  if (status !== undefined && status >= 500) {
    return "unavailable";
  }
  // 4xx / rejected ToolResponse without ProductJoinResult → not "app offline".
  if (status !== undefined && status >= 400 && status < 500) {
    return "join_not_configured";
  }
  if (combined.includes("did not return a product.join result")) {
    return "join_not_configured";
  }
  return "unavailable";
}

// ── Transport seam (injectable; default wired to api.ts) ─────────────────────

export type RequestJoinTicketInput = {
  preview: HostedProjectPreview;
  /** Supabase access token carried by the hosted handoff. */
  accessToken: string;
  apiBase?: string;
  signal?: AbortSignal;
};

export type SubmitProductJoinInput = {
  signedTicket: SignedProductJoinTicket;
  /** Bridge session token for the paired daemon. */
  sessionToken: string;
  signal?: AbortSignal;
};

/**
 * The two contract hops the SPA drives. Implementations live in `api.ts`; tests
 * inject deterministic stubs so the UI states can be exercised without a live
 * Cloud/daemon (whose endpoints are owned by the runtime/Cloud slices).
 */
export type ProductJoinTransport = {
  requestJoinTicket(
    input: RequestJoinTicketInput,
  ): Promise<SignedProductJoinTicket>;
  submitProductJoin(input: SubmitProductJoinInput): Promise<ProductJoinResult>;
};

/** Default transport wired to the live Cloud/daemon endpoints in `api.ts`. */
export const defaultProductJoinTransport: ProductJoinTransport = {
  requestJoinTicket: (input) =>
    requestProductJoinTicket({
      preview: input.preview,
      accessToken: input.accessToken,
      apiBase: input.apiBase,
      signal: input.signal,
    }),
  submitProductJoin: (input) =>
    submitProductJoin({
      sessionToken: input.sessionToken,
      signedTicket: input.signedTicket,
      signal: input.signal,
    }),
};

// ── Orchestrator ─────────────────────────────────────────────────────────────

export type ProductJoinOutcome =
  | { status: "granted"; result: ProductJoinResult }
  | { status: "failed"; failure: ProductJoinFailureView; result?: ProductJoinResult };

export type RunProductJoinOptions = {
  preview: HostedProjectPreview;
  accessToken: string;
  sessionToken: string;
  apiBase?: string;
  transport: ProductJoinTransport;
  signal?: AbortSignal;
};

/**
 * Run the full per-Product join: request a narrow ticket from Cloud, submit
 * `product.join` to the daemon, and reduce the daemon's typed result to a UI
 * outcome. Never throws — hop failures resolve to typed recovery buckets so the
 * caller can always render a safe state and never claims membership before a
 * `granted` daemon result.
 */
export async function runProductJoin(
  opts: RunProductJoinOptions,
): Promise<ProductJoinOutcome> {
  let signedTicket: SignedProductJoinTicket;
  try {
    signedTicket = await opts.transport.requestJoinTicket({
      preview: opts.preview,
      accessToken: opts.accessToken,
      apiBase: opts.apiBase,
      signal: opts.signal,
    });
  } catch (error) {
    return {
      status: "failed",
      failure: failureView(recoveryFromJoinError(error, "cloud")),
    };
  }

  let result: ProductJoinResult;
  try {
    result = await opts.transport.submitProductJoin({
      signedTicket,
      sessionToken: opts.sessionToken,
      signal: opts.signal,
    });
  } catch (error) {
    return {
      status: "failed",
      failure: failureView(recoveryFromJoinError(error, "daemon")),
    };
  }

  if (result.outcome === "granted" && result.granted_role === "Product Member") {
    return { status: "granted", result };
  }

  const code: ProductJoinRecovery = result.failure?.code ?? "unavailable";
  return { status: "failed", failure: failureView(code), result };
}
