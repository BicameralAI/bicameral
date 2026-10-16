/**
 * Journey review fixtures (bot#899).
 *
 * Contract fixtures for the recovery and lifecycle states the review surface
 * must render explicitly: candidate, no-candidate, deferred, failed, retry,
 * missing-trace, hidden/suppressed, and promoted-Decision journeys.
 *
 * Fixtures are test data only; production views always come from the daemon.
 */

import type {
  JourneyPhaseView,
  JourneyReviewView,
} from "./journey-review-types";

function basePhases(): JourneyPhaseView[] {
  return [
    {
      phase_id: "external_admission",
      component: "bicameral-gateway",
      function: "external_ingest",
      status: "completed",
      persistence: { plane: "none", operation: "none" },
      authority: {
        before: "evidence",
        after: "evidence",
        transition: "admitted",
      },
      sanitized_summary: { receipt: "validated" },
    },
    {
      phase_id: "evidence_acceptance",
      component: "bicameral-gateway",
      function: "accept_evidence",
      status: "completed",
      persistence: { plane: "canonical_event", operation: "append" },
      authority: {
        before: "evidence",
        after: "canonical",
        transition: "evidence_accepted",
      },
      sanitized_summary: { event: "EvidenceAccepted" },
    },
    {
      phase_id: "projection",
      component: "bicameral-runtime",
      function: "project",
      status: "completed",
      persistence: { plane: "local_work", operation: "write" },
      authority: {
        before: "canonical",
        after: "proposal",
        transition: "candidate_projected",
      },
      sanitized_summary: { outcome: "candidate_projected" },
    },
  ];
}

function baseView(journeyId: string): JourneyReviewView {
  return {
    schema_version: 1,
    journey_id: journeyId,
    source: {
      provider: "github",
      evidence_identity: "ev-fixture-1",
      source_uri: "https://github.com/BicameralAI/example/issues/7",
      content_digest: "sha256:" + "a".repeat(64),
      accepted_at: "2026-07-20T12:00:00Z",
    },
    phases: basePhases(),
    recall_packet: {
      packet_id: "00000000-0000-4000-8000-000000000001",
      canonical_decisions: [],
      decision_candidates: [],
    },
    allowed_actions: [],
    caveats: [],
    generated_at: "2026-07-20T12:00:05Z",
  };
}

export function candidateJourney(): JourneyReviewView {
  const view = baseView("journey-candidate");
  view.recall_packet.decision_candidates = [
    {
      candidate_id: "00000000-0000-4000-8000-00000000c001",
      title: "We will adopt a dedicated cache layer",
      candidate_state: "proposed",
      review_state: "advisory",
    },
  ];
  view.allowed_actions = [
    {
      command: "candidate_hide",
      target_id: "00000000-0000-4000-8000-00000000c001",
      requires_authority: false,
    },
    {
      command: "candidate_suppress",
      target_id: "00000000-0000-4000-8000-00000000c001",
      requires_authority: true,
    },
    {
      command: "candidate_promote",
      target_id: "00000000-0000-4000-8000-00000000c001",
      requires_authority: true,
    },
  ];
  return view;
}

export function noCandidateJourney(): JourneyReviewView {
  const view = baseView("journey-no-candidate");
  view.phases[2] = {
    ...view.phases[2],
    status: "completed",
    authority: {
      before: "canonical",
      after: "canonical",
      transition: "no_candidate",
    },
    persistence: { plane: "local_work", operation: "write" },
    reason_codes: ["no_decision_like_content"],
    sanitized_summary: { outcome: "no_candidate" },
  };
  return view;
}

export function deferredJourney(): JourneyReviewView {
  const view = baseView("journey-deferred");
  view.phases[2] = {
    ...view.phases[2],
    status: "deferred",
    retryable: true,
    attempt: 1,
    reason_codes: ["provider_unavailable"],
    authority: {
      before: "canonical",
      after: "canonical",
      transition: "projection_deferred",
    },
    sanitized_summary: { outcome: "projection_deferred" },
  };
  view.caveats = [
    {
      code: "retry_scheduled",
      detail: "Projection retry is scheduled; canonical evidence is retained.",
    },
  ];
  return view;
}

export function failedJourney(): JourneyReviewView {
  const view = baseView("journey-failed");
  view.phases[2] = {
    ...view.phases[2],
    status: "failed",
    retryable: false,
    attempt: 3,
    reason_codes: ["hard_rejected"],
    authority: {
      before: "canonical",
      after: "canonical",
      transition: "projection_failed",
    },
    sanitized_summary: { outcome: "hard_rejected" },
  };
  view.caveats = [
    {
      code: "terminal_failure",
      detail:
        "Projection failed terminally. Canonical evidence is unchanged and retained.",
    },
  ];
  return view;
}

export function missingTraceJourney(): JourneyReviewView {
  const view = baseView("journey-missing-trace");
  view.phases.unshift({
    phase_id: "integrations_trace",
    component: "bicameral-integrations",
    function: "trace_ingest",
    status: "unavailable",
    persistence: { plane: "none", operation: "none" },
    authority: {
      before: "evidence",
      after: "evidence",
      transition: "trace_missing",
    },
    reason_codes: ["finalized_trace_not_attached"],
    sanitized_summary: null,
  });
  view.caveats = [
    {
      code: "missing_trace",
      detail:
        "No finalized Integrations trace is attached. Upstream phases are unknown, not successful.",
    },
  ];
  return view;
}

export function hiddenAndSuppressedJourney(): JourneyReviewView {
  const view = baseView("journey-disposed");
  view.recall_packet.decision_candidates = [
    {
      candidate_id: "00000000-0000-4000-8000-00000000c002",
      title: "Hidden for this session",
      candidate_state: "proposed",
      hidden: true,
    },
    {
      candidate_id: "00000000-0000-4000-8000-00000000c003",
      title: "Durably suppressed locally",
      candidate_state: "proposed",
      suppressed: true,
    },
  ];
  return view;
}

export function promotedJourney(): JourneyReviewView {
  const view = baseView("journey-promoted");
  view.recall_packet.canonical_decisions = [
    {
      decision_id: "DEC-201",
      title: "We will adopt a dedicated cache layer",
      level: "architecture",
      promoted_at: "2026-07-20T13:00:00Z",
    },
  ];
  view.phases.push({
    phase_id: "promotion",
    component: "bicameral-runtime",
    function: "promote_decision_candidate",
    status: "completed",
    persistence: { plane: "canonical_event", operation: "append" },
    authority: {
      before: "proposal",
      after: "canonical",
      transition: "human_promotion",
    },
    sanitized_summary: { decision_id: "DEC-201" },
  });
  return view;
}

export const JOURNEY_FIXTURES = {
  candidate: candidateJourney,
  noCandidate: noCandidateJourney,
  deferred: deferredJourney,
  failed: failedJourney,
  missingTrace: missingTraceJourney,
  disposed: hiddenAndSuppressedJourney,
  promoted: promotedJourney,
};
