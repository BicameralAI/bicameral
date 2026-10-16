/**
 * Journey review contracts (bot#899, D4 acceptance contract).
 *
 * TypeScript mirror of `bicameral-api::journey_review` and the RecallPacket
 * fields the review surface consumes. The browser renders these shapes only;
 * it must never reconstruct candidate authority, infer missing phases, or
 * invoke a model.
 */

export type JourneyPhaseStatus =
  | "accepted"
  | "completed"
  | "deferred"
  | "failed"
  | "unavailable";

export type JourneyPersistencePlane =
  | "none"
  | "evidence_body"
  | "local_work"
  | "canonical_event";

export type JourneyPersistenceOperation =
  | "none"
  | "read"
  | "write"
  | "append"
  | "update";

/** Authority classes rendered with text labels, never color alone. */
export type JourneyAuthorityClass = "evidence" | "proposal" | "canonical";

export type JourneyActionKind =
  | "candidate_hide"
  | "candidate_suppress"
  | "candidate_reject"
  | "candidate_promote";

export interface JourneyPersistenceEffect {
  plane: JourneyPersistencePlane;
  operation: JourneyPersistenceOperation;
}

export interface JourneyAuthorityTransition {
  before: JourneyAuthorityClass;
  after: JourneyAuthorityClass;
  transition: string;
}

export interface JourneySourceView {
  provider: string;
  evidence_identity: string;
  source_uri: string;
  source_version?: string;
  content_digest: string;
  accepted_at: string;
}

export interface JourneyPhaseView {
  phase_id: string;
  component: string;
  function: string;
  status: JourneyPhaseStatus;
  reason_codes?: string[];
  persistence: JourneyPersistenceEffect;
  authority: JourneyAuthorityTransition;
  request_id?: string;
  attempt?: number;
  retryable?: boolean;
  occurred_at?: string;
  sanitized_summary?: unknown;
}

export interface JourneyAllowedAction {
  command: JourneyActionKind;
  target_id: string;
  requires_authority: boolean;
}

export interface JourneyCaveat {
  code: string;
  detail: string;
}

/** Canonical decision entry inside the RecallPacket (subset rendered). */
export interface JourneyCanonicalDecision {
  decision_id: string;
  title: string;
  level?: string;
  promoted_at?: string;
}

/** Daemon-local candidate surfaced for review (subset rendered). */
export interface JourneyDecisionCandidate {
  candidate_id: string;
  title: string;
  candidate_state?: string;
  review_state?: string;
  hidden?: boolean;
  suppressed?: boolean;
}

export interface JourneyRecallPacket {
  packet_id: string;
  canonical_decisions?: JourneyCanonicalDecision[];
  decision_candidates?: JourneyDecisionCandidate[];
  decision_attention?: unknown;
  unknown_scope?: unknown[];
}

export interface JourneyReviewView {
  schema_version: number;
  journey_id: string;
  product_scope?: string;
  source: JourneySourceView;
  phases: JourneyPhaseView[];
  recall_packet: JourneyRecallPacket;
  allowed_actions?: JourneyAllowedAction[];
  caveats?: JourneyCaveat[];
  generated_at: string;
}

/** Human-facing labels: authority must never rely on color alone. */
export const AUTHORITY_LABELS: Record<JourneyAuthorityClass, string> = {
  evidence: "Derived evidence",
  proposal: "Local proposal",
  canonical: "Canonical",
};

export const ACTION_LABELS: Record<JourneyActionKind, string> = {
  candidate_hide: "Hide",
  candidate_suppress: "Suppress",
  candidate_reject: "Reject",
  candidate_promote: "Promote",
};

/** Actions whose command contract requires an operator reason. */
export const ACTIONS_REQUIRING_REASON: ReadonlySet<JourneyActionKind> = new Set(
  ["candidate_suppress", "candidate_reject"],
);
