/**
 * Hosted Product creation orchestration (bot#799).
 *
 * The shipped Create action is daemon-canonical:
 *
 *   paired daemon product.create
 *     -> daemon-derived product.publish descriptor
 *     -> authenticated Cloud directory publication
 *     -> daemon-owned local setup
 *
 * The browser coordinates those hops but owns none of their authority. The
 * Supabase access token is sent only to Cloud. The bridge session token is sent
 * only to the paired daemon. A small browser-local recovery marker preserves
 * idempotency across refresh or response loss, but it is never canonical Product
 * state and contains no token, secret, local path, source body, or raw event.
 */

import { readHostedHandoff } from "./hosted-handoff";

// ---------------------------------------------------------------------------
// Legacy cloud#66 type exports
// ---------------------------------------------------------------------------
//
// lib/api.ts still exports the old team/expose clients for compatibility with
// historical callers. The production Create orchestrator below does not import
// or call them. They are quarantined from the shipped hosted Create path.

/** @deprecated Legacy cloud#66 coordination receipt. Not used by hosted Create. */
export type RegistrationKind = "create_project";
/** @deprecated Legacy cloud#66 coordination status. Not used by hosted Create. */
export type RegistrationStatus = "pending";
/** @deprecated Legacy cloud#66 coordination next step. Not used by hosted Create. */
export type RegistrationNextStep = "local_daemon_binding_required";
/** @deprecated Legacy cloud#66 coordination receipt. Not used by hosted Create. */
export type CloudRegistrationReceipt = {
  request_id: string;
  kind: RegistrationKind;
  team_id: string;
  status: RegistrationStatus;
  next_step: RegistrationNextStep;
  message: string;
};
/** @deprecated Legacy cloud#66 team response. Not used by hosted Create. */
export type CloudTeamCreateResponse = {
  team_id: string;
  display_name: string;
  created_by: string;
  role: string;
};
/** @deprecated Options retained only for the quarantined lib/api.ts client. */
export type CreateCloudTeamOptions = {
  accessToken: string;
  apiBase?: string;
  displayName: string;
  signal?: AbortSignal;
};
/** @deprecated Options retained only for the quarantined lib/api.ts client. */
export type ExposeCloudProjectOptions = {
  accessToken: string;
  apiBase?: string;
  teamId: string;
  displayName: string;
  projectRef?: string;
  signal?: AbortSignal;
};

// ---------------------------------------------------------------------------
// Canonical Product creation wire shapes
// ---------------------------------------------------------------------------

export type ProductCreationResult = {
  product_id: string;
  creator_cloud_account_id: string;
  creator_roles: string[];
  membership_event_ref: string;
  governance_policy_version: string;
  receipt_id: string;
  created_at: string;
};

export type ProductCreationPreview = {
  preview_id: string;
  product_id: string;
  product_display_name: string;
  team_display_name?: string;
  verified_domain?: string;
  event_store_hint?: {
    substrate?: string;
    provider?: string;
  };
  required_connection?: {
    provider?: string;
  };
  canonical: false;
  expires_at: string;
};

export type ProductCreationCommitProof = {
  schema_version: number;
  attempt_id: string;
  product_id: string;
  event_id: string;
  substrate: string;
  resource_fingerprint: string;
  provider_revision: string;
  event_digest: string;
  durability: string;
  confirmed_at: string;
  proof_digest: string;
  signer_key_id: string;
  signature_algorithm: string;
  signature: string;
};

export type ProductCreationPublication = {
  publication_id: string;
  product_id: string;
  creator_cloud_account_id: string;
  team_display_name?: string;
  verified_domain?: string;
  event_store_hint?: {
    substrate?: string;
    provider?: string;
  };
  canonical: false;
  idempotency_key: string;
  canonical_creation_receipt_ref: string;
  published_at: string;
  first_event_commit_proof?: ProductCreationCommitProof;
};

type ProductCreateEnvelope = {
  result: ProductCreationResult;
  preview: ProductCreationPreview;
  created: boolean;
};

type ProductPublishEnvelope = {
  publication?: ProductCreationPublication;
  diagnostics?: {
    error_code?: string;
    canonical_product_count?: number;
    create_event_count?: number;
    malformed_create_event_count?: number;
    event_store_substrate?: string;
    product_id_fingerprint?: string;
    idempotency_key_fingerprint?: string;
  };
};

type ToolResponse<T> = {
  request_id: string;
  status: "ok" | "rejected" | "error";
  result?: T;
  message?: string;
  responded_at: string;
};

export type CloudProductPublicationReceipt = {
  canonical_product_id: string;
  display_name: string;
  eligible_domain: string;
  idempotent: boolean;
};

export type HostedProductSetupProjection = {
  product_id: string;
  readiness: "open" | "continue_setup" | "repair";
  next_step?: string;
  repair_reason?: string;
  has_registration: boolean;
  membership_retained: boolean;
};

export type RegistrationAxis = "local_daemon_setup";

export type RegistrationNextStepInfo = {
  axis: RegistrationAxis;
  title: string;
  detail: string;
};

export type RegistrationOutcomeStatus =
  | "recorded"
  | "recovery_needed"
  | "publication_retry_needed"
  | "error";

export type RegistrationOutcomePhase =
  | "published"
  | "create_retry_needed"
  | "publication_retry_needed"
  | "failed";

export type RegistrationOutcome = {
  /**
   * `recorded` is retained as a compatibility status for App.svelte. In the new
   * flow it means the canonical Product was created and its non-canonical Cloud
   * directory row was published successfully.
   */
  status: RegistrationOutcomeStatus;
  /** Optional for older callers that construct a generic error result. */
  phase?: RegistrationOutcomePhase;
  message: string;
  productId?: string;
  created?: boolean;
  canRetry?: boolean;
  errorKind?: string;
  /** Redacted daemon request correlation safe to share with support. */
  supportId?: string;
  localReadiness?: HostedProductSetupProjection["readiness"];
  nextStep?: RegistrationNextStepInfo;
};

export type CreateHostedProjectOptions = {
  /** Supabase access token. Sent only to Cloud publication. */
  accessToken: string;
  /** Cloud API origin; omitted for same-origin Cloud serving. */
  apiBase?: string;
  /** Human-facing Product display name. */
  displayName: string;
  /**
   * Daemon-validated Google Drive folder id chosen through the shipped
   * resource-selection flow (#1264). Required by operator-scoped daemons;
   * omitted only for legacy workspace-scoped callers.
   */
  eventStoreFolderId?: string;
  signal?: AbortSignal;

  /** Optional explicit bridge values for tests or embedded callers. */
  sessionToken?: string;
  cloudAccountId?: string;
  daemonBase?: string;
  storage?: Storage | null;
  idFactory?: () => string;
  now?: () => string;

  /**
   * @deprecated Legacy cloud#66 values. Accepted so existing callers compile,
   * but ignored by the daemon-canonical Product creation path.
   */
  projectRef?: string;
  /** @deprecated See projectRef. */
  teamId?: string;
};

export type SubmitHostedProductCreateOptions = {
  daemonBase: string;
  sessionToken: string;
  cloudAccountId: string;
  verifiedDomain: string;
  displayName: string;
  idempotencyKey: string;
  /**
   * Daemon-validated Google Drive folder id from the resource-selection
   * projection (#1264). Transported verbatim as the `event_store_descriptor`;
   * the daemon re-checks it against its own validated selection and rejects
   * anything it did not validate.
   */
  eventStoreFolderId?: string;
  signal?: AbortSignal;
};

export type SubmitHostedProductPublishOptions = {
  daemonBase: string;
  sessionToken: string;
  productId: string;
  idempotencyKey: string;
  signal?: AbortSignal;
};

export type PublishHostedProductToCloudOptions = {
  accessToken: string;
  apiBase?: string;
  displayName: string;
  eligibleDomain: string;
  publication: ProductCreationPublication;
  signal?: AbortSignal;
};

export type ContinueHostedProductSetupOptions = {
  daemonBase: string;
  sessionToken: string;
  productId: string;
  providerHint?: "github" | "google";
  substrateHint?: "git" | "drive_folder";
  signal?: AbortSignal;
};

// ---------------------------------------------------------------------------
// Browser-local recovery marker
// ---------------------------------------------------------------------------

const PENDING_CREATE_PREFIX = "bicameral_hosted_product_create_v1:";

type PendingCreatePhase = "create_requested" | "canonical_created";

type PendingHostedCreate = {
  version: 1;
  cloudAccountId: string;
  displayName: string;
  verifiedDomain: string;
  createIdempotencyKey: string;
  publishIdempotencyKey: string;
  phase: PendingCreatePhase;
  productId?: string;
  creationReceiptRef?: string;
  /** Daemon-validated Drive folder bound to this create intent (#1264). */
  eventStoreFolderId?: string;
  startedAt: string;
};

function pendingKey(cloudAccountId: string): string {
  return `${PENDING_CREATE_PREFIX}${encodeURIComponent(cloudAccountId)}`;
}

function resolveStorage(explicit?: Storage | null): Storage | null {
  if (explicit !== undefined) return explicit;
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch {
    return null;
  }
}

function readPending(
  cloudAccountId: string,
  storage: Storage | null,
): PendingHostedCreate | null {
  if (!storage || !cloudAccountId) return null;
  try {
    const raw = storage.getItem(pendingKey(cloudAccountId));
    if (!raw) return null;
    const value = JSON.parse(raw) as Partial<PendingHostedCreate>;
    if (
      value.version !== 1 ||
      value.cloudAccountId !== cloudAccountId ||
      !value.displayName ||
      !value.verifiedDomain ||
      !value.createIdempotencyKey ||
      !value.publishIdempotencyKey ||
      (value.phase !== "create_requested" &&
        value.phase !== "canonical_created")
    ) {
      return null;
    }
    return value as PendingHostedCreate;
  } catch {
    return null;
  }
}

function writePending(
  pending: PendingHostedCreate,
  storage: Storage | null,
): void {
  if (!storage) return;
  try {
    storage.setItem(pendingKey(pending.cloudAccountId), JSON.stringify(pending));
  } catch {
    // Recovery storage is best-effort. Runtime idempotency still protects a
    // same-key retry during the active page session.
  }
}

export function clearPendingHostedCreate(
  cloudAccountId: string,
  storage?: Storage | null,
): void {
  const target = resolveStorage(storage);
  if (!target || !cloudAccountId) return;
  try {
    target.removeItem(pendingKey(cloudAccountId));
  } catch {
    // Best-effort browser-local cleanup only.
  }
}

export function getPendingHostedCreateInput(
  cloudAccountId: string,
  storage?: Storage | null,
): { displayName: string } | null {
  const pending = readPending(cloudAccountId, resolveStorage(storage));
  return pending ? { displayName: pending.displayName } : null;
}

// ---------------------------------------------------------------------------
// Token-safe helpers
// ---------------------------------------------------------------------------

function decodeBase64Url(value: string): string {
  const normalized = value.replace(/-/g, "+").replace(/_/g, "/");
  const padded = normalized.padEnd(Math.ceil(normalized.length / 4) * 4, "=");
  return globalThis.atob(padded);
}

/**
 * Extract the verified email domain from the Supabase JWT payload.
 *
 * This is routing metadata, not authorization. Cloud validates the bearer token
 * and enforces that `eligible_domain` matches the authenticated account before
 * persisting a directory row.
 */
export function verifiedDomainFromAccessToken(token: string): string | null {
  try {
    const payloadPart = token.split(".")[1];
    if (!payloadPart) return null;
    const payload = JSON.parse(decodeBase64Url(payloadPart)) as {
      email?: unknown;
    };
    if (typeof payload.email !== "string") return null;
    const email = payload.email.trim().toLowerCase();
    const at = email.lastIndexOf("@");
    if (at <= 0 || at === email.length - 1) return null;
    return email.slice(at + 1);
  } catch {
    return null;
  }
}

function newId(factory?: () => string): string {
  if (factory) return factory();
  return (
    globalThis.crypto?.randomUUID?.() ??
    `${Date.now()}-${Math.random().toString(16).slice(2)}`
  );
}

function cloudBase(apiBase?: string): string {
  return (apiBase ?? "").replace(/\/+$/, "");
}

function normalizeDaemonBase(base: string): string {
  return base.replace(/\/+$/, "");
}

type ErrorBody = {
  message?: unknown;
  detail?: unknown;
};

type ApiError = Error & {
  status?: number;
  body?: unknown;
};

async function responseBody(res: Response): Promise<unknown> {
  return res.json().catch(() => ({}));
}

function detailMessage(value: unknown): string | null {
  if (typeof value === "string") {
    const trimmed = value.trim();
    return trimmed || null;
  }

  if (Array.isArray(value)) {
    const messages = value
      .map((entry) => detailMessage(entry))
      .filter((entry): entry is string => Boolean(entry));
    return messages.length > 0 ? messages.join("; ") : null;
  }

  if (!value || typeof value !== "object") return null;

  const record = value as Record<string, unknown>;
  const message =
    typeof record.msg === "string"
      ? record.msg.trim()
      : typeof record.message === "string"
        ? record.message.trim()
        : "";
  if (!message) return null;

  const location = Array.isArray(record.loc)
    ? record.loc
        .filter((segment) => typeof segment === "string" || typeof segment === "number")
        .map(String)
        .filter((segment) => segment !== "body")
        .join(".")
    : "";
  return location ? `${location}: ${message}` : message;
}

function errorBodyMessage(
  body: ErrorBody,
  fallback: string,
  preferDetail = false,
): string {
  const candidates = preferDetail
    ? [body.detail, body.message]
    : [body.message, body.detail];
  for (const candidate of candidates) {
    const rendered = detailMessage(candidate);
    if (rendered) return rendered;
  }
  return fallback;
}

function apiError(
  message: string,
  status?: number,
  body?: unknown,
): ApiError {
  return Object.assign(new Error(message), { status, body });
}

async function postBridgeCommand<T>(opts: {
  daemonBase: string;
  sessionToken: string;
  name: string;
  params: unknown;
  signal?: AbortSignal;
}): Promise<ToolResponse<T>> {
  const res = await fetch(
    `${normalizeDaemonBase(opts.daemonBase)}/api/v1/hosted-bridge/command`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "x-bridge-session-token": opts.sessionToken,
      },
      body: JSON.stringify({
        command: {
          name: opts.name,
          params: opts.params,
        },
      }),
      signal: opts.signal,
    },
  );
  const body = (await responseBody(res)) as ToolResponse<T> | ErrorBody;
  if (!res.ok) {
    const errorBody = body as ErrorBody;
    throw apiError(
      errorBodyMessage(errorBody, `Hosted bridge HTTP ${res.status}`),
      res.status,
      body,
    );
  }
  return body as ToolResponse<T>;
}

export function submitHostedProductCreate(
  opts: SubmitHostedProductCreateOptions,
): Promise<ToolResponse<ProductCreateEnvelope>> {
  return postBridgeCommand<ProductCreateEnvelope>({
    daemonBase: opts.daemonBase,
    sessionToken: opts.sessionToken,
    name: "product.create",
    params: {
      idempotency_key: opts.idempotencyKey,
      product_display_name: opts.displayName,
      asserted_cloud_account_id: opts.cloudAccountId,
      verified_domain: opts.verifiedDomain,
      ...(opts.eventStoreFolderId
        ? {
            event_store_descriptor: {
              provider: "google",
              folder_id: opts.eventStoreFolderId,
            },
          }
        : {}),
    },
    signal: opts.signal,
  });
}

export function submitHostedProductPublish(
  opts: SubmitHostedProductPublishOptions,
): Promise<ToolResponse<ProductPublishEnvelope>> {
  return postBridgeCommand<ProductPublishEnvelope>({
    daemonBase: opts.daemonBase,
    sessionToken: opts.sessionToken,
    name: "product.publish",
    params: {
      product_id: opts.productId,
      idempotency_key: opts.idempotencyKey,
    },
    signal: opts.signal,
  });
}

export async function publishHostedProductToCloud(
  opts: PublishHostedProductToCloudOptions,
): Promise<CloudProductPublicationReceipt> {
  const publication = opts.publication;
  const res = await fetch(`${cloudBase(opts.apiBase)}/api/v1/products`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${opts.accessToken}`,
    },
    body: JSON.stringify({
      canonical_product_id: publication.product_id,
      display_name: opts.displayName,
      eligible_domain: opts.eligibleDomain,
      creation_receipt: publication.canonical_creation_receipt_ref,
      first_event_commit_proof: publication.first_event_commit_proof,
      registration_ref: publication.product_id,
      provider_hint: publication.event_store_hint?.provider,
      substrate_hint: publication.event_store_hint?.substrate,
    }),
    signal: opts.signal,
  });
  const body = (await responseBody(res)) as
    | CloudProductPublicationReceipt
    | ErrorBody;
  if (!res.ok) {
    const errorBody = body as ErrorBody;
    throw apiError(
      errorBodyMessage(errorBody, `Cloud publication HTTP ${res.status}`, true),
      res.status,
      body,
    );
  }
  return body as CloudProductPublicationReceipt;
}

export async function continueHostedProductSetup(
  opts: ContinueHostedProductSetupOptions,
): Promise<HostedProductSetupProjection> {
  const res = await fetch(
    `${normalizeDaemonBase(opts.daemonBase)}/api/v1/hosted-bridge/product-continue-setup`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "x-bridge-session-token": opts.sessionToken,
      },
      body: JSON.stringify({
        request_id:
          globalThis.crypto?.randomUUID?.() ?? `req_${Date.now()}`,
        product_id: opts.productId,
        registration_ref: opts.productId,
        provider_hint: opts.providerHint,
        substrate_hint: opts.substrateHint,
      }),
      signal: opts.signal,
    },
  );
  const body = (await responseBody(res)) as
    | HostedProductSetupProjection
    | ErrorBody;
  if (!res.ok) {
    const errorBody = body as ErrorBody;
    throw apiError(
      errorBodyMessage(errorBody, `Local setup HTTP ${res.status}`),
      res.status,
      body,
    );
  }
  return body as HostedProductSetupProjection;
}

// ---------------------------------------------------------------------------
// Orchestration and validation
// ---------------------------------------------------------------------------

const EXACT_CREATOR_ROLES = ["Policy Owner", "Product Owner"];

function errorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error && error.message) return error.message;
  return fallback;
}

function failed(message: string, errorKind?: string): RegistrationOutcome {
  return {
    status: "error",
    phase: "failed",
    message,
    errorKind,
  };
}

function createRetryNeeded(
  pending: PendingHostedCreate,
  message: string,
): RegistrationOutcome {
  return {
    status: "recovery_needed",
    phase: "create_retry_needed",
    message:
      `${message} Retry uses the same creation key, so the daemon will return ` +
      "the existing Product if the canonical commit already succeeded.",
    canRetry: true,
    productId: pending.productId,
  };
}

function publicationRetryNeeded(
  pending: PendingHostedCreate,
  message: string,
  supportId?: string,
): RegistrationOutcome {
  return {
    status: "publication_retry_needed",
    phase: "publication_retry_needed",
    message:
      `The Product exists in the local daemon but is not available to others yet. ${message}`,
    productId: pending.productId,
    canRetry: true,
    supportId,
  };
}

function nextStepForSetup(
  setup: HostedProductSetupProjection | null,
): RegistrationNextStepInfo {
  if (setup?.readiness === "open") {
    return {
      axis: "local_daemon_setup",
      title: "Product ready",
      detail:
        "The local daemon completed registration, opened the event-store adapter, and replayed canonical Product state.",
    };
  }
  if (setup?.readiness === "repair") {
    return {
      axis: "local_daemon_setup",
      title: "Repair local setup",
      detail:
        `The Product remains canonical and published, but local setup needs repair${
          setup.repair_reason ? ` (${setup.repair_reason})` : ""
        }.`,
    };
  }
  if (setup?.readiness === "continue_setup") {
    return {
      axis: "local_daemon_setup",
      title: "Continue local setup",
      detail:
        `The Product is canonical and published. The daemon requires another local setup step${
          setup.next_step ? ` (${setup.next_step})` : ""
        } before it can open.`,
    };
  }
  return {
    axis: "local_daemon_setup",
    title: "Continue setup in the local Bicameral daemon",
    detail:
      "Product identity and creator roles are canonical and Cloud publication succeeded. Local registration, replay, and readiness still need confirmation.",
  };
}

function published(
  productId: string,
  created: boolean,
  setup: HostedProductSetupProjection | null,
): RegistrationOutcome {
  return {
    // Compatibility status used by App.svelte to refresh discovery.
    status: "recorded",
    phase: "published",
    message:
      setup?.readiness === "open"
        ? "Product created locally, published, registered, and replayed."
        : "Product created locally and published for eligible discovery.",
    productId,
    created,
    localReadiness: setup?.readiness,
    nextStep: nextStepForSetup(setup),
  };
}

function exactCreatorRoles(roles: string[]): boolean {
  return (
    roles.length === EXACT_CREATOR_ROLES.length &&
    EXACT_CREATOR_ROLES.every((role, index) => roles[index] === role)
  );
}

function validateCreateEnvelope(
  envelope: ProductCreateEnvelope | undefined,
  cloudAccountId: string,
): string | null {
  if (!envelope?.result?.product_id || !envelope.preview?.product_id) {
    return "Daemon returned an incomplete product.create result.";
  }
  if (envelope.result.product_id !== envelope.preview.product_id) {
    return "Daemon create receipt and preview disagree on Product identity.";
  }
  if (envelope.result.creator_cloud_account_id !== cloudAccountId) {
    return "Daemon create receipt does not match the paired Cloud account.";
  }
  if (!exactCreatorRoles(envelope.result.creator_roles ?? [])) {
    return "Daemon did not atomically materialize Policy Owner and Product Owner.";
  }
  if (envelope.result.governance_policy_version !== "gpv_1") {
    return "Daemon did not return the initial Governance Policy Version.";
  }
  if (envelope.preview.canonical !== false) {
    return "Hosted preview incorrectly claims canonical authority.";
  }
  return null;
}

function validatePublication(
  publication: ProductCreationPublication | undefined,
  pending: PendingHostedCreate,
): string | null {
  if (!publication || !pending.productId) {
    return "Daemon returned an incomplete product.publish result.";
  }
  if (publication.product_id !== pending.productId) {
    return "Publication descriptor does not match the canonical Product.";
  }
  if (publication.creator_cloud_account_id !== pending.cloudAccountId) {
    return "Publication descriptor does not match the paired Cloud account.";
  }
  if (publication.canonical !== false) {
    return "Publication descriptor incorrectly claims canonical authority.";
  }
  if (
    publication.canonical_creation_receipt_ref !== pending.creationReceiptRef
  ) {
    return "Publication descriptor does not reference the canonical creation receipt.";
  }
  if (
    publication.verified_domain &&
    publication.verified_domain !== pending.verifiedDomain
  ) {
    return "Publication descriptor does not match the authenticated email domain.";
  }
  if (!publication.first_event_commit_proof) {
    return "Publication descriptor is missing first_event_commit_proof.";
  }
  if (publication.first_event_commit_proof.product_id !== publication.product_id) {
    return "first_event_commit_proof does not match the canonical Product.";
  }
  return null;
}

function allowedProviderHint(
  provider: string | undefined,
): "github" | "google" | undefined {
  return provider === "github" || provider === "google" ? provider : undefined;
}

function allowedSubstrateHint(
  substrate: string | undefined,
): "git" | "drive_folder" | undefined {
  return substrate === "git" || substrate === "drive_folder"
    ? substrate
    : undefined;
}

/**
 * Create, publish, and continue local setup for a hosted Product. Never throws.
 *
 * A marker is written before `product.create`. If the browser loses the response
 * after the daemon commits, reconnecting reuses the same idempotency key and the
 * daemon returns the existing Product. Once canonical creation is observed, the
 * marker is updated with the Product id and only `product.publish` plus Cloud
 * publication are retried. The marker is cleared after Cloud publication; local
 * setup is a separate daemon-owned fact and cannot roll back creation or sharing.
 */
export async function createHostedProject(
  opts: CreateHostedProjectOptions,
): Promise<RegistrationOutcome> {
  const displayName = opts.displayName.trim();
  if (!displayName) return failed("Product name is required.", "invalid_name");

  const handoff = readHostedHandoff();
  const sessionToken = opts.sessionToken ?? handoff?.session_token;
  const cloudAccountId = opts.cloudAccountId ?? handoff?.cloud_account_id;
  const localDaemonBase = opts.daemonBase ?? handoff?.daemon_url;

  if (!sessionToken || !cloudAccountId || !localDaemonBase) {
    return failed(
      "A paired local daemon session is required before creating a Product.",
      "missing_bridge_session",
    );
  }

  const verifiedDomain = verifiedDomainFromAccessToken(opts.accessToken);
  if (!verifiedDomain) {
    return failed(
      "The Cloud session does not contain a verified email domain. Sign in again before creating a Product.",
      "missing_verified_domain",
    );
  }

  const storage = resolveStorage(opts.storage);
  let pending = readPending(cloudAccountId, storage);

  if (pending && pending.verifiedDomain !== verifiedDomain) {
    return failed(
      "The pending Product belongs to a different verified domain. Sign in with the original account or clear the pending recovery state.",
      "domain_changed",
    );
  }

  if (!pending) {
    pending = {
      version: 1,
      cloudAccountId,
      displayName,
      verifiedDomain,
      createIdempotencyKey: `create_${newId(opts.idFactory)}`,
      publishIdempotencyKey: `publish_${newId(opts.idFactory)}`,
      phase: "create_requested",
      eventStoreFolderId: opts.eventStoreFolderId,
      startedAt: opts.now?.() ?? new Date().toISOString(),
    };
    // Persist before the daemon call so response loss can safely replay the same
    // create intent instead of creating a second Product.
    writePending(pending, storage);
  }

  let created = false;

  if (!pending.productId) {
    let response: ToolResponse<ProductCreateEnvelope>;
    try {
      response = await submitHostedProductCreate({
        daemonBase: localDaemonBase,
        sessionToken,
        cloudAccountId,
        verifiedDomain: pending.verifiedDomain,
        displayName: pending.displayName,
        idempotencyKey: pending.createIdempotencyKey,
        eventStoreFolderId: opts.eventStoreFolderId ?? pending.eventStoreFolderId,
        signal: opts.signal,
      });
    } catch (error: unknown) {
      return createRetryNeeded(
        pending,
        errorMessage(error, "The daemon creation response was not confirmed."),
      );
    }

    if (response.status !== "ok") {
      // Product-create typed denials append no event by contract, so the intent
      // marker can be cleared rather than misrepresenting a canonical Product.
      clearPendingHostedCreate(cloudAccountId, storage);
      return failed(
        response.message ?? "The daemon rejected Product creation.",
        response.status,
      );
    }

    const createError = validateCreateEnvelope(response.result, cloudAccountId);
    if (createError) {
      // The daemon may have committed even though the response shape is unusable.
      // Keep the original key so a later retry can recover idempotently.
      return createRetryNeeded(pending, createError);
    }

    const envelope = response.result as ProductCreateEnvelope;
    pending = {
      ...pending,
      phase: "canonical_created",
      productId: envelope.result.product_id,
      creationReceiptRef: envelope.result.receipt_id,
    };
    created = envelope.created;
    writePending(pending, storage);
  }

  let publishResponse: ToolResponse<ProductPublishEnvelope>;
  try {
    publishResponse = await submitHostedProductPublish({
      daemonBase: localDaemonBase,
      sessionToken,
      productId: pending.productId as string,
      idempotencyKey: pending.publishIdempotencyKey,
      signal: opts.signal,
    });
  } catch (error: unknown) {
    return publicationRetryNeeded(
      pending,
      errorMessage(error, "The daemon publication descriptor was not confirmed."),
    );
  }

  if (publishResponse.status !== "ok") {
    return publicationRetryNeeded(
      pending,
      publishResponse.message ??
        "The daemon could not derive publication metadata.",
      publishResponse.request_id,
    );
  }

  const publication = publishResponse.result?.publication;
  const publicationError = validatePublication(publication, pending);
  if (publicationError || !publication) {
    return publicationRetryNeeded(
      pending,
      publicationError ?? "The publication descriptor was incomplete.",
    );
  }

  let cloudReceipt: CloudProductPublicationReceipt;
  try {
    cloudReceipt = await publishHostedProductToCloud({
      accessToken: opts.accessToken,
      apiBase: opts.apiBase,
      displayName: pending.displayName,
      eligibleDomain: pending.verifiedDomain,
      publication,
      signal: opts.signal,
    });
  } catch (error: unknown) {
    return publicationRetryNeeded(
      pending,
      errorMessage(error, "Bicameral Cloud could not publish the directory row."),
    );
  }

  if (cloudReceipt.canonical_product_id !== pending.productId) {
    return publicationRetryNeeded(
      pending,
      "Cloud returned a directory receipt for a different Product.",
    );
  }

  // Cloud publication is durable and idempotent. Clear the publication recovery
  // marker before local continuation because setup failure cannot make the
  // directory publication untrue or roll back canonical Product creation.
  clearPendingHostedCreate(cloudAccountId, storage);

  let setup: HostedProductSetupProjection | null = null;
  try {
    setup = await continueHostedProductSetup({
      daemonBase: localDaemonBase,
      sessionToken,
      productId: pending.productId as string,
      providerHint: allowedProviderHint(
        publication.event_store_hint?.provider,
      ),
      substrateHint: allowedSubstrateHint(
        publication.event_store_hint?.substrate,
      ),
      signal: opts.signal,
    });
  } catch {
    // Honest partial success: Product and Cloud directory publication are real;
    // local setup remains separately actionable from Operator Home.
    setup = null;
  }

  return published(pending.productId as string, created, setup);
}
