/**
 * Journey review client (bot#899).
 *
 * Reads the daemon-owned `JourneyReviewView` and submits governed candidate
 * actions through `v2/tool-requests`. Every mutation re-fetches the journey
 * view first and fails closed when the candidate/action pair is no longer
 * advertised (stale-action revalidation per the D4 acceptance contract).
 */

import type {
  JourneyActionKind,
  JourneyAllowedAction,
  JourneyReviewView,
} from "./journey-review-types";

const BASE = "";

export class JourneyReviewError extends Error {
  readonly kind:
    | "not_found"
    | "unavailable"
    | "stale_action"
    | "rejected"
    | "network";

  constructor(kind: JourneyReviewError["kind"], message: string) {
    super(message);
    this.kind = kind;
  }
}

export async function fetchJourneyReview(
  journeyId: string,
): Promise<JourneyReviewView> {
  let res: Response;
  try {
    res = await fetch(
      `${BASE}/api/v1/journeys/${encodeURIComponent(journeyId)}`,
      { cache: "no-store" },
    );
  } catch {
    throw new JourneyReviewError(
      "network",
      "The daemon did not respond. The journey state is unknown, not failed.",
    );
  }
  if (res.status === 404) {
    throw new JourneyReviewError(
      "not_found",
      "No journey exists for this identifier.",
    );
  }
  if (!res.ok) {
    throw new JourneyReviewError(
      "unavailable",
      `Journey view unavailable (status ${res.status}).`,
    );
  }
  return (await res.json()) as JourneyReviewView;
}

/** URL for the sanitized finalized trace download (served no-store). */
export function traceDownloadUrl(journeyId: string): string {
  return `${BASE}/api/v1/journeys/${encodeURIComponent(journeyId)}/trace.json`;
}

const ACTION_COMMANDS: Record<JourneyActionKind, string> = {
  candidate_hide: "candidate.hide",
  candidate_suppress: "candidate.suppress",
  candidate_reject: "review.reject_candidate",
  candidate_promote: "recall.promote_decision_candidate",
};

function actionStillAdvertised(
  view: JourneyReviewView,
  action: JourneyActionKind,
  targetId: string,
): JourneyAllowedAction | undefined {
  return (view.allowed_actions ?? []).find(
    (entry) => entry.command === action && entry.target_id === targetId,
  );
}

export interface GovernedActionResult {
  /** The journey view refreshed after the daemon response. */
  view: JourneyReviewView;
}

/**
 * Submit a governed candidate action.
 *
 * 1. re-fetch the journey view;
 * 2. confirm the candidate and action remain advertised;
 * 3. submit through `v2/tool-requests`;
 * 4. refresh the journey view after the response.
 *
 * Fails closed on stale capability, unknown candidate, unsupported command,
 * or network ambiguity.
 */
export async function submitGovernedAction(options: {
  journeyId: string;
  action: JourneyActionKind;
  targetId: string;
  reason?: string;
}): Promise<GovernedActionResult> {
  const { journeyId, action, targetId, reason } = options;

  const current = await fetchJourneyReview(journeyId);
  if (!actionStillAdvertised(current, action, targetId)) {
    throw new JourneyReviewError(
      "stale_action",
      "This action is no longer advertised for the candidate. The view has been refreshed; re-check the candidate state before acting.",
    );
  }

  const params: Record<string, unknown> =
    action === "candidate_hide"
      ? { candidate_id: targetId }
      : action === "candidate_suppress"
        ? { candidate_id: targetId, reason: reason ?? "" }
        : { target_id: targetId, reason: reason ?? undefined };

  let res: Response;
  try {
    res = await fetch(`${BASE}/v2/tool-requests`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        command: { name: ACTION_COMMANDS[action], params },
      }),
    });
  } catch {
    throw new JourneyReviewError(
      "network",
      "The daemon did not acknowledge the action. Its effect is unknown; refresh before retrying.",
    );
  }

  if (!res.ok) {
    let detail = `status ${res.status}`;
    try {
      const body = await res.json();
      if (body && typeof body.message === "string") detail = body.message;
    } catch {
      // keep the status text; never fabricate a daemon message
    }
    throw new JourneyReviewError("rejected", `The daemon rejected the action: ${detail}`);
  }

  const view = await fetchJourneyReview(journeyId);
  return { view };
}
