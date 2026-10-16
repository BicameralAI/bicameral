<script lang="ts">
  /**
   * Graph Readiness Panel — renders daemon-authored graph readiness
   * and evidence refresh/currentness state.
   *
   * Shows ready/indexing/stale/failed/not_configured states with explicit
   * caveats for non-ready states. Provides refresh action that does NOT
   * mutate signoff or compliance.
   *
   * UI copy avoids: compliant, noncompliant, safe, blocked, violates, no-conflict.
   */
  import type {
    GraphReadinessView,
    EvidenceRefreshOutcome,
    CodeSourceCurrentness,
  } from "./types";

  type Props = {
    readiness: GraphReadinessView | null | undefined;
    refreshOutcome: EvidenceRefreshOutcome | null;
    refreshLoading: boolean;
    refreshError: string | null;
    onRefresh: () => void;
  };

  let { readiness, refreshOutcome, refreshLoading, refreshError, onRefresh }: Props = $props();

  function readinessLabel(r: GraphReadinessView["readiness"]): string {
    switch (r) {
      case "ready": return "Graph available";
      case "indexing": return "Indexing in progress";
      case "stale": return "Graph outdated";
      case "failed": return "Graph unavailable";
      case "not_configured": return "Not configured";
    }
  }

  function readinessClass(r: GraphReadinessView["readiness"]): string {
    switch (r) {
      case "ready": return "readiness-ready";
      case "indexing": return "readiness-indexing";
      case "stale": return "readiness-stale";
      case "failed": return "readiness-failed";
      case "not_configured": return "readiness-not-configured";
    }
  }

  function readinessCaveat(view: GraphReadinessView): string | null {
    switch (view.readiness) {
      case "ready":
        if (view.scope_coverage === "partial") {
          return "Partial coverage — source-only evidence for unindexed regions";
        }
        return null;
      case "indexing":
        return "Results may be incomplete while indexing completes";
      case "stale":
        return "Graph data is behind current HEAD — evidence may not reflect latest code";
      case "failed":
        return "Graph indexing failed — code evidence is source-only until resolved";
      case "not_configured":
        return "No graph configured — code evidence is source-only";
    }
  }

  function readinessNextAction(view: GraphReadinessView): string | null {
    switch (view.readiness) {
      case "ready": return null;
      case "indexing": return "Wait for indexing to complete, then refresh";
      case "stale": return "Refresh graph to current HEAD";
      case "failed": return "Check graph provider configuration and retry";
      case "not_configured": return "Configure a graph provider for this workspace";
    }
  }

  function currentnessLabel(c: CodeSourceCurrentness): string {
    switch (c) {
      case "current": return "current";
      case "stale": return "stale";
      case "moved": return "moved";
      case "deleted": return "deleted";
      case "unavailable": return "unavailable";
    }
  }

  function currentnessClass(c: CodeSourceCurrentness): string {
    switch (c) {
      case "current": return "currentness-current";
      case "stale": return "currentness-stale";
      case "moved": return "currentness-moved";
      case "deleted": return "currentness-deleted";
      case "unavailable": return "currentness-unavailable";
    }
  }

  function refreshStatusLabel(status: EvidenceRefreshOutcome["status"]): string {
    switch (status) {
      case "refreshed": return "Evidence checked";
      case "ineligible": return "Not eligible for refresh";
      case "not_found": return "Decision not found";
      case "unsupported": return "Refresh unavailable";
    }
  }
</script>

{#if !readiness}
  <div class="graph-readiness-empty">
    <span class="empty-label">Graph readiness</span>
    <span class="empty-detail">No graph readiness data available for this decision</span>
  </div>
{:else}
  <div class="graph-readiness-panel">
    <div class="readiness-header">
      <span class="readiness-badge {readinessClass(readiness.readiness)}">
        {readinessLabel(readiness.readiness)}
      </span>
      {#if readiness.graph_snapshot_id}
        <span class="snapshot-ref">
          snapshot: <code>{readiness.graph_snapshot_id}</code>
        </span>
      {/if}
    </div>

    {#if readiness.detail}
      <div class="readiness-detail">{readiness.detail}</div>
    {/if}

    {#if readinessCaveat(readiness)}
      <div class="readiness-caveat">{readinessCaveat(readiness)}</div>
    {/if}

    {#if readinessNextAction(readiness)}
      <div class="readiness-next-action">
        <span class="next-action-label">Next step:</span>
        <span>{readinessNextAction(readiness)}</span>
      </div>
    {/if}

    {#if readiness.readiness === "ready" || readiness.readiness === "stale"}
      <div class="refresh-section">
        {#if refreshLoading}
          <span class="refresh-loading">Checking evidence currentness…</span>
        {:else if refreshError}
          <div class="refresh-error">
            <span>{refreshError}</span>
            <button class="btn-retry" onclick={onRefresh}>Retry</button>
          </div>
        {:else if refreshOutcome}
          <div class="refresh-outcome">
            <span class="refresh-status">{refreshStatusLabel(refreshOutcome.status)}</span>
            {#if refreshOutcome.detail}
              <span class="refresh-detail">{refreshOutcome.detail}</span>
            {/if}
            {#if refreshOutcome.binding_results.length > 0}
              <div class="binding-results">
                {#each refreshOutcome.binding_results as result}
                  <div class="binding-result-row">
                    <code class="binding-id">{result.binding_id}</code>
                    <span class="currentness-badge {currentnessClass(result.currentness)}">
                      {currentnessLabel(result.currentness)}
                    </span>
                    {#if result.detail}
                      <span class="binding-detail">{result.detail}</span>
                    {/if}
                  </div>
                {/each}
              </div>
            {/if}
          </div>
          <button class="btn-refresh" onclick={onRefresh}>
            Re-check evidence
          </button>
        {:else}
          <button class="btn-refresh" onclick={onRefresh}>
            Check evidence currentness
          </button>
        {/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .graph-readiness-panel {
    border: 1px solid var(--line);
    border-radius: 8px;
    padding: 0.75rem 1rem;
    background: var(--canvas-muted);
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .graph-readiness-empty {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    color: var(--ink-400);
    font-size: 0.85rem;
  }

  .empty-label {
    font-weight: 600;
    color: var(--ink-500);
  }

  .readiness-header {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    flex-wrap: wrap;
  }

  .readiness-badge {
    font-size: 0.8rem;
    font-weight: 600;
    padding: 0.2rem 0.6rem;
    border-radius: 4px;
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .readiness-ready {
    background: var(--ok-50);
    color: var(--ok-600);
  }

  .readiness-indexing {
    background: var(--accent-50);
    color: var(--accent-700);
  }

  .readiness-stale {
    background: var(--warn-50);
    color: var(--warn-600);
  }

  .readiness-failed {
    background: var(--danger-50);
    color: var(--danger-600);
  }

  .readiness-not-configured {
    background: var(--canvas-muted);
    color: var(--ink-500);
  }

  .snapshot-ref {
    font-size: 0.75rem;
    color: var(--ink-500);
  }

  .snapshot-ref code {
    font-size: 0.75rem;
    background: var(--canvas-muted);
    padding: 0.1rem 0.3rem;
    border-radius: 3px;
  }

  .readiness-detail {
    font-size: 0.82rem;
    color: var(--ink-700);
  }

  .readiness-caveat {
    font-size: 0.8rem;
    color: var(--warn-600);
    background: var(--warn-50);
    border: 1px solid #fde68a;
    border-radius: 4px;
    padding: 0.35rem 0.6rem;
  }

  .readiness-next-action {
    font-size: 0.8rem;
    color: var(--ink-700);
    display: flex;
    gap: 0.4rem;
  }

  .next-action-label {
    font-weight: 600;
    color: var(--ink-500);
  }

  .refresh-section {
    margin-top: 0.25rem;
    padding-top: 0.5rem;
    border-top: 1px solid var(--line);
  }

  .refresh-loading {
    font-size: 0.82rem;
    color: var(--ink-500);
    font-style: italic;
  }

  .refresh-error {
    font-size: 0.82rem;
    color: var(--danger-600);
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .refresh-outcome {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    margin-bottom: 0.5rem;
  }

  .refresh-status {
    font-size: 0.82rem;
    font-weight: 600;
    color: var(--ink-700);
  }

  .refresh-detail {
    font-size: 0.78rem;
    color: var(--ink-500);
  }

  .binding-results {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    margin-top: 0.25rem;
  }

  .binding-result-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.78rem;
  }

  .binding-id {
    font-size: 0.75rem;
    background: var(--canvas-muted);
    padding: 0.1rem 0.3rem;
    border-radius: 3px;
    color: var(--ink-700);
  }

  .currentness-badge {
    font-size: 0.72rem;
    font-weight: 600;
    padding: 0.1rem 0.4rem;
    border-radius: 3px;
    text-transform: uppercase;
  }

  .currentness-current {
    background: var(--ok-50);
    color: var(--ok-600);
  }

  .currentness-stale {
    background: var(--warn-50);
    color: var(--warn-600);
  }

  .currentness-moved {
    background: var(--accent-50);
    color: var(--accent-700);
  }

  .currentness-deleted {
    background: var(--danger-50);
    color: var(--danger-600);
  }

  .currentness-unavailable {
    background: var(--canvas-muted);
    color: var(--ink-500);
  }

  .binding-detail {
    color: var(--ink-500);
  }

  .btn-refresh,
  .btn-retry {
    font-size: 0.78rem;
    padding: 0.3rem 0.7rem;
    border: 1px solid var(--line-strong);
    border-radius: 4px;
    background: #fff;
    color: var(--ink-700);
    cursor: pointer;
    transition: background 0.1s;
  }

  .btn-refresh:hover,
  .btn-retry:hover {
    background: var(--canvas-muted);
    border-color: var(--ink-400);
  }
</style>
