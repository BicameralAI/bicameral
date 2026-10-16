<script lang="ts">
  /**
   * Journey Review — transformation-debug and review surface (bot#899, D4).
   *
   * Renders one accepted-evidence journey from provider acquisition through
   * canonical Decision promotion. A view and governed command surface only:
   * it never reconstructs candidate authority, infers missing phases, or
   * invokes a model. Authority is always labeled in text, never color alone.
   */
  import { onMount } from "svelte";
  import {
    fetchJourneyReview,
    submitGovernedAction,
    traceDownloadUrl,
    JourneyReviewError,
  } from "../lib/journey-review-client";
  import {
    ACTION_LABELS,
    ACTIONS_REQUIRING_REASON,
    AUTHORITY_LABELS,
    type JourneyActionKind,
    type JourneyAllowedAction,
    type JourneyPhaseView,
    type JourneyReviewView,
  } from "../lib/journey-review-types";

  let { initialJourneyId = "" }: { initialJourneyId?: string } = $props();

  let journeyIdInput = $state(initialJourneyId);
  let view = $state<JourneyReviewView | null>(null);
  let loading = $state(false);
  let loadError = $state<string | null>(null);
  /** Announced through the aria-live region. */
  let liveMessage = $state("");
  let actionBusy = $state(false);
  let reasonFor = $state<{ action: JourneyActionKind; targetId: string } | null>(
    null,
  );
  let reasonText = $state("");

  async function load(journeyId: string) {
    if (!journeyId.trim()) return;
    loading = true;
    loadError = null;
    try {
      view = await fetchJourneyReview(journeyId.trim());
      liveMessage = `Journey ${view.journey_id} loaded.`;
    } catch (e) {
      view = null;
      loadError =
        e instanceof JourneyReviewError
          ? e.message
          : "Journey view unavailable.";
      liveMessage = loadError;
    } finally {
      loading = false;
    }
  }

  function actionsFor(candidateId: string): JourneyAllowedAction[] {
    return (view?.allowed_actions ?? []).filter(
      (a) => a.target_id === candidateId,
    );
  }

  function beginAction(action: JourneyActionKind, targetId: string) {
    if (ACTIONS_REQUIRING_REASON.has(action)) {
      reasonFor = { action, targetId };
      reasonText = "";
      return;
    }
    void runAction(action, targetId, undefined);
  }

  async function runAction(
    action: JourneyActionKind,
    targetId: string,
    reason: string | undefined,
  ) {
    if (!view) return;
    actionBusy = true;
    try {
      const result = await submitGovernedAction({
        journeyId: view.journey_id,
        action,
        targetId,
        reason,
      });
      view = result.view;
      liveMessage = `${ACTION_LABELS[action]} completed. Journey view refreshed.`;
    } catch (e) {
      if (e instanceof JourneyReviewError && e.kind === "stale_action") {
        // Fail closed and refresh so the operator sees the current state.
        await load(view.journey_id);
        liveMessage = e.message;
      } else {
        liveMessage =
          e instanceof JourneyReviewError
            ? e.message
            : "The action could not be completed.";
      }
    } finally {
      actionBusy = false;
      reasonFor = null;
      reasonText = "";
    }
  }

  function submitReason() {
    if (!reasonFor) return;
    if (!reasonText.trim()) {
      liveMessage = "A reason is required for this action.";
      return;
    }
    void runAction(reasonFor.action, reasonFor.targetId, reasonText.trim());
  }

  function phaseStateLabel(phase: JourneyPhaseView): string {
    switch (phase.status) {
      case "accepted":
        return "Accepted";
      case "completed":
        return "Completed";
      case "deferred":
        return phase.retryable
          ? "Deferred (retry scheduled)"
          : "Deferred";
      case "failed":
        return phase.retryable === false
          ? "Failed (terminal)"
          : "Failed";
      case "unavailable":
        return "Unknown (not successful)";
    }
  }

  const visibleCandidates = $derived(
    (view?.recall_packet.decision_candidates ?? []).filter(
      (c) => !c.hidden,
    ),
  );
  const hiddenCandidates = $derived(
    (view?.recall_packet.decision_candidates ?? []).filter((c) => c.hidden),
  );

  onMount(() => {
    if (initialJourneyId) {
      void load(initialJourneyId);
    }
  });
</script>

<section class="journey-review" aria-labelledby="journey-review-heading">
  <h1 id="journey-review-heading">Journey review</h1>
  <p class="intro">
    Inspect one accepted-evidence journey end to end. Canonical Product facts,
    daemon-local proposals, and derived evidence are labeled separately and
    never merged.
  </p>

  <form
    class="journey-lookup"
    onsubmit={(event) => {
      event.preventDefault();
      void load(journeyIdInput);
    }}
  >
    <label for="journey-id-input">Journey identifier</label>
    <input
      id="journey-id-input"
      type="text"
      bind:value={journeyIdInput}
      autocomplete="off"
    />
    <button type="submit" disabled={loading}>Load journey</button>
  </form>

  <div aria-live="polite" role="status" class="live-region">{liveMessage}</div>

  {#if loading}
    <p class="state-note">Loading journey…</p>
  {:else if loadError}
    <p class="state-note error-note" role="alert">{loadError}</p>
  {:else if view}
    <section aria-labelledby="journey-summary-heading" class="summary">
      <h2 id="journey-summary-heading">Summary</h2>
      <dl>
        <div>
          <dt>Source</dt>
          <dd>
            <span class="authority-tag" data-authority="derived_evidence"
              >Derived evidence</span
            >
            {view.source.provider} — {view.source.source_uri}
          </dd>
        </div>
        <div>
          <dt>Evidence identity</dt>
          <dd>{view.source.evidence_identity}</dd>
        </div>
        <div>
          <dt>Content digest</dt>
          <dd class="digest">{view.source.content_digest}</dd>
        </div>
        <div>
          <dt>Accepted at</dt>
          <dd>{view.source.accepted_at}</dd>
        </div>
        <div>
          <dt>Canonical Decisions</dt>
          <dd>{view.recall_packet.canonical_decisions?.length ?? 0}</dd>
        </div>
        <div>
          <dt>Local candidates</dt>
          <dd>{view.recall_packet.decision_candidates?.length ?? 0}</dd>
        </div>
      </dl>
      <a
        class="trace-download"
        href={traceDownloadUrl(view.journey_id)}
        download={`journey-${view.journey_id}-trace.json`}
      >
        Download sanitized trace (JSON)
      </a>
    </section>

    {#if (view.caveats ?? []).length > 0}
      <section aria-labelledby="journey-caveats-heading" class="caveats">
        <h2 id="journey-caveats-heading">Recovery state</h2>
        <ul>
          {#each view.caveats ?? [] as caveat (caveat.code)}
            <li>
              <strong>{caveat.code}</strong>: {caveat.detail}
            </li>
          {/each}
        </ul>
      </section>
    {/if}

    <section aria-labelledby="journey-phases-heading" class="phases">
      <h2 id="journey-phases-heading">Ordered phase trace</h2>
      <ol>
        {#each view.phases as phase (phase.phase_id)}
          <li class="phase" data-status={phase.status}>
            <h3>{phase.phase_id}</h3>
            <p class="phase-meta">
              {phase.component} / {phase.function}
              {#if phase.attempt !== undefined}
                — attempt {phase.attempt}{/if}
            </p>
            <p>
              <span class="phase-state">{phaseStateLabel(phase)}</span>
              <span class="authority-tag" data-authority={phase.authority.after}>
                {AUTHORITY_LABELS[phase.authority.before]} →
                {AUTHORITY_LABELS[phase.authority.after]}
              </span>
            </p>
            <p class="phase-persistence">
              Persistence: {phase.persistence.plane} / {phase.persistence
                .operation}
            </p>
            {#if (phase.reason_codes ?? []).length > 0}
              <p class="phase-reasons">
                Reason codes: {(phase.reason_codes ?? []).join(", ")}
              </p>
            {/if}
          </li>
        {/each}
      </ol>
    </section>

    <section
      aria-labelledby="journey-canonical-heading"
      class="recall canonical-region"
    >
      <h2 id="journey-canonical-heading">
        Canonical Decisions
        <span class="authority-tag" data-authority="canonical">Canonical</span>
      </h2>
      {#if (view.recall_packet.canonical_decisions ?? []).length === 0}
        <p class="state-note">
          No canonical Decision exists for this journey. This is an explicit
          state, not an error.
        </p>
      {:else}
        <ul>
          {#each view.recall_packet.canonical_decisions ?? [] as decision (decision.decision_id)}
            <li>
              <strong>{decision.decision_id}</strong>: {decision.title}
              {#if decision.promoted_at}
                <span class="promoted-at">
                  promoted {decision.promoted_at}</span
                >
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </section>

    <section
      aria-labelledby="journey-candidates-heading"
      class="recall proposal-region"
    >
      <h2 id="journey-candidates-heading">
        Local candidates
        <span class="authority-tag" data-authority="proposal"
          >Local proposal</span
        >
      </h2>
      {#if visibleCandidates.length === 0}
        <p class="state-note">
          No active local candidate. This is an explicit state, not an error.
        </p>
      {:else}
        <ul>
          {#each visibleCandidates as candidate (candidate.candidate_id)}
            <li class="candidate" data-suppressed={candidate.suppressed}>
              <strong>{candidate.title}</strong>
              {#if candidate.suppressed}
                <span class="disposition">Durably suppressed locally</span>
              {/if}
              <div class="candidate-actions">
                {#each actionsFor(candidate.candidate_id) as allowed (allowed.command)}
                  <button
                    type="button"
                    disabled={actionBusy}
                    onclick={() =>
                      beginAction(allowed.command, allowed.target_id)}
                  >
                    {ACTION_LABELS[allowed.command]}
                  </button>
                {/each}
              </div>
            </li>
          {/each}
        </ul>
      {/if}
      {#if hiddenCandidates.length > 0}
        <p class="state-note">
          {hiddenCandidates.length} candidate(s) hidden for this session.
        </p>
      {/if}
    </section>

    {#if reasonFor}
      <form
        class="reason-form"
        aria-labelledby="reason-heading"
        onsubmit={(event) => {
          event.preventDefault();
          submitReason();
        }}
      >
        <h2 id="reason-heading">
          Reason for {ACTION_LABELS[reasonFor.action]}
        </h2>
        <label for="action-reason">Reason (required)</label>
        <input
          id="action-reason"
          type="text"
          bind:value={reasonText}
          autocomplete="off"
        />
        <button type="submit" disabled={actionBusy}>Confirm</button>
        <button
          type="button"
          disabled={actionBusy}
          onclick={() => (reasonFor = null)}
        >
          Cancel
        </button>
      </form>
    {/if}
  {:else}
    <p class="state-note">
      Enter a journey identifier to load its review view.
    </p>
  {/if}
</section>

<style>
  .journey-review {
    max-width: 60rem;
    padding: 1rem;
  }

  .journey-lookup {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    align-items: center;
  }

  .live-region {
    min-height: 1.25rem;
    margin: 0.5rem 0;
  }

  /* Authority is communicated by label text plus a shape/border treatment,
     never color alone. */
  .authority-tag {
    display: inline-block;
    border: 1px solid currentColor;
    border-radius: 0.25rem;
    padding: 0 0.35rem;
    font-size: 0.8rem;
    margin-left: 0.35rem;
  }
  .authority-tag[data-authority="canonical"] {
    border-style: double;
    border-width: 3px;
    font-weight: 700;
  }
  .authority-tag[data-authority="proposal"] {
    border-style: dashed;
  }
  .authority-tag[data-authority="evidence"],
  .authority-tag[data-authority="derived_evidence"] {
    border-style: dotted;
  }

  .phase {
    border-left: 3px solid currentColor;
    margin: 0.75rem 0;
    padding-left: 0.75rem;
  }
  .phase[data-status="failed"] {
    border-left-style: double;
  }
  .phase[data-status="deferred"],
  .phase[data-status="unavailable"] {
    border-left-style: dashed;
  }

  .digest {
    font-family: monospace;
    overflow-wrap: anywhere;
  }

  .candidate-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    margin-top: 0.25rem;
  }

  .state-note {
    font-style: italic;
  }

  .error-note {
    font-weight: 600;
  }

  :focus-visible {
    outline: 3px solid currentColor;
    outline-offset: 2px;
  }

  @media (max-width: 480px) {
    .journey-review {
      padding: 0.5rem;
    }
    .journey-lookup {
      flex-direction: column;
      align-items: stretch;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .journey-review * {
      transition: none !important;
      animation: none !important;
    }
  }
</style>
