<script lang="ts">
  /**
   * Code Impact Panel — renders find_code_impact discovery results.
   *
   * Shows loading, success (verified/approximate candidates), graph
   * unavailable/stale, unsupported, not found, needs narrowing,
   * multiple candidates, and error states.
   *
   * Verified candidates expose accept-binding action.
   * Non-verified candidates do not expose accept-binding.
   *
   * UI copy avoids: compliant, noncompliant, safe, blocked, violates, no-conflict.
   */
  import type {
    FindCodeImpactResult,
    CodeImpactCandidate,
    LocatorEvidenceState,
    LocatorConfidence,
  } from "./types";

  type Props = {
    result: FindCodeImpactResult | null;
    loading: boolean;
    error: string | null;
    onAcceptBinding: (candidateTarget: string) => void;
    onFindCodeImpact: () => void;
  };

  let { result, loading, error, onAcceptBinding, onFindCodeImpact }: Props = $props();

  function statusLabel(status: FindCodeImpactResult["status"]): string {
    switch (status) {
      case "found": return "Candidates located";
      case "needs_human_selection": return "Selection required";
      case "not_found": return "No candidates located";
      case "graph_unavailable": return "Graph unavailable";
      case "graph_stale": return "Graph outdated";
      case "unsupported": return "Discovery unavailable";
    }
  }

  function statusClass(status: FindCodeImpactResult["status"]): string {
    switch (status) {
      case "found": return "status-found";
      case "needs_human_selection": return "status-selection";
      case "not_found": return "status-not-found";
      case "graph_unavailable": return "status-unavailable";
      case "graph_stale": return "status-stale";
      case "unsupported": return "status-unsupported";
    }
  }

  function evidenceLabel(state: LocatorEvidenceState): string {
    switch (state) {
      case "verified": return "verified";
      case "not_found": return "not found";
      case "unknown_not_indexed": return "not indexed";
      case "unknown_stale": return "stale";
      case "ambiguous": return "ambiguous";
      case "unsupported": return "unsupported";
      case "approximate_candidate": return "approximate";
    }
  }

  function evidenceClass(state: LocatorEvidenceState): string {
    switch (state) {
      case "verified": return "ev-verified";
      case "not_found": return "ev-not-found";
      case "unknown_not_indexed": return "ev-info";
      case "unknown_stale": return "ev-warning";
      case "ambiguous": return "ev-warning";
      case "unsupported": return "ev-info";
      case "approximate_candidate": return "ev-approximate";
    }
  }

  function confidenceLabel(c: LocatorConfidence): string {
    return c;
  }

  function candidateAllowsBinding(candidate: CodeImpactCandidate): boolean {
    return candidate.evidence_state === "verified";
  }
</script>

{#if !result && !loading && !error}
  <div class="impact-trigger">
    <button class="btn-find-impact" onclick={onFindCodeImpact}>
      Find code impact
    </button>
    <span class="impact-hint">Discover code regions related to this decision</span>
  </div>
{:else if loading}
  <div class="impact-loading">
    <span class="loading-indicator"></span>
    <span>Searching for code impact…</span>
  </div>
{:else if error}
  <div class="impact-error">
    <span class="error-label">Discovery error</span>
    <span class="error-detail">{error}</span>
    <button class="btn-retry" onclick={onFindCodeImpact}>Retry</button>
  </div>
{:else if result}
  <div class="impact-result">
    <div class="impact-header">
      <span class="impact-status {statusClass(result.status)}">{statusLabel(result.status)}</span>
      {#if result.phase !== "discovery_requested"}
        <span class="impact-phase">{result.phase.replace(/_/g, " ")}</span>
      {/if}
    </div>

    {#if result.caveats.length > 0}
      <div class="impact-caveats">
        {#each result.caveats as caveat}
          <div class="caveat-item">
            <span class="caveat-kind">{caveat.kind.replace(/_/g, " ")}</span>
            <span class="caveat-detail">{caveat.detail}</span>
          </div>
        {/each}
      </div>
    {/if}

    {#if result.candidates.length > 0}
      <div class="impact-candidates">
        <h4>Candidate regions</h4>
        {#each result.candidates as candidate, idx}
          <div class="candidate-card">
            <div class="candidate-header">
              <span class="candidate-target">
                <code>{candidate.file}</code>
                {#if candidate.range}
                  <span class="candidate-range">:{candidate.range}</span>
                {/if}
              </span>
              <span class="candidate-evidence {evidenceClass(candidate.evidence_state)}">
                {evidenceLabel(candidate.evidence_state)}
              </span>
            </div>

            {#if candidate.symbol}
              <div class="candidate-symbol">
                <code>{candidate.symbol}</code>
              </div>
            {/if}

            <div class="candidate-meta">
              <span class="confidence-badge confidence-{candidate.confidence}">
                {confidenceLabel(candidate.confidence)}
              </span>
              {#if candidate.validated_sha}
                <span class="candidate-sha">@ <code>{candidate.validated_sha}</code></span>
              {/if}
            </div>

            {#if candidate.reasons.length > 0}
              <div class="candidate-reasons">
                {#each candidate.reasons as reason}
                  <span class="reason-tag">{reason.detail}</span>
                {/each}
              </div>
            {/if}

            {#if candidateAllowsBinding(candidate)}
              <div class="candidate-actions">
                <button
                  class="btn-accept-binding"
                  onclick={() => onAcceptBinding(candidate.target)}
                >
                  Accept binding
                </button>
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {:else if result.status === "not_found"}
      <div class="impact-empty">
        No candidate code regions located for this decision.
      </div>
    {:else if result.status === "graph_unavailable"}
      <div class="impact-empty">
        Code graph is not available. Build or configure a graph to enable discovery.
      </div>
    {:else if result.status === "unsupported"}
      <div class="impact-empty">
        Code-impact discovery is not available for this decision in its current state.
      </div>
    {/if}

    {#if result.next_actions.length > 0}
      <div class="impact-next-actions">
        <h4>Suggested actions</h4>
        {#each result.next_actions as action}
          <div class="next-action-item">
            <span class="next-action-desc">{action.description}</span>
            {#if action.command_hint}
              <code class="next-action-hint">{action.command_hint}</code>
            {/if}
          </div>
        {/each}
      </div>
    {/if}

    {#if result.snapshot}
      <div class="impact-snapshot">
        {#if result.snapshot.graph_snapshot_id}
          <span>Snapshot: <code>{result.snapshot.graph_snapshot_id}</code></span>
        {/if}
        {#if result.snapshot.commit_sha}
          <span>@ <code>{result.snapshot.commit_sha}</code></span>
        {/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .impact-trigger {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    padding: var(--sp-2) 0;
  }
  .btn-find-impact {
    padding: var(--sp-2) var(--sp-3);
    border: 1px solid var(--accent-500);
    background: var(--accent-50);
    color: var(--accent-600);
    border-radius: var(--radius-md);
    font-size: var(--text-sm);
    font-weight: 500;
    font-family: var(--font-sans);
    cursor: pointer;
    transition:
      background var(--dur-fast) var(--ease-out),
      border-color var(--dur-fast) var(--ease-out);
  }
  .btn-find-impact:hover { background: var(--accent-100); border-color: var(--accent-400); }
  .impact-hint { color: var(--ink-400); font-size: var(--text-xs); }

  .impact-loading {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-3);
    background: var(--canvas-muted);
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    font-size: var(--text-sm);
    color: var(--ink-500);
  }
  .loading-indicator {
    width: 14px;
    height: 14px;
    border: 2px solid var(--line);
    border-top-color: var(--accent-500);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }

  .impact-error {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-3);
    background: var(--danger-50);
    border: 1px solid #fecaca;
    border-radius: var(--radius-md);
    font-size: var(--text-sm);
  }
  .error-label { font-weight: 600; color: var(--danger-600); }
  .error-detail { color: var(--danger-600); }
  .btn-retry {
    margin-left: auto;
    padding: var(--sp-1) var(--sp-2);
    border: 1px solid #fecaca;
    background: var(--surface);
    color: var(--danger-600);
    border-radius: var(--radius-sm);
    font-size: var(--text-xs);
    font-family: var(--font-sans);
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease-out);
  }
  .btn-retry:hover { background: var(--danger-50); }

  .impact-result {
    padding: var(--sp-3);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--canvas-muted);
  }

  .impact-header {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin-bottom: var(--sp-2);
  }
  .impact-status {
    display: inline-flex;
    align-items: center;
    padding: 2px var(--sp-2);
    border-radius: var(--radius-sm);
    font-size: var(--text-xs);
    font-weight: 600;
    line-height: 1.5;
    text-transform: uppercase;
    letter-spacing: 0.02em;
    border: 1px solid transparent;
    white-space: nowrap;
  }
  .status-found { background: var(--ok-50); color: var(--ok-600); border-color: #a7f3d0; }
  .status-selection { background: var(--warn-50); color: var(--warn-600); border-color: #fde68a; }
  .status-not-found { background: var(--danger-50); color: var(--danger-600); border-color: #fecaca; }
  .status-unavailable { background: var(--canvas-muted); color: var(--ink-500); border-color: var(--line); }
  .status-stale { background: var(--warn-50); color: var(--warn-600); border-color: #fde68a; }
  .status-unsupported { background: var(--canvas-muted); color: var(--ink-500); border-color: var(--line); }
  .impact-phase { font-size: var(--text-xs); color: var(--ink-400); }

  .impact-caveats {
    margin-bottom: var(--sp-2);
    padding: var(--sp-2) var(--sp-3);
    background: var(--warn-50);
    border: 1px solid #fde68a;
    border-radius: var(--radius-md);
  }
  .caveat-item { display: flex; gap: var(--sp-1); align-items: baseline; font-size: var(--text-sm); }
  .caveat-kind { font-weight: 500; color: var(--warn-600); }
  .caveat-detail { color: var(--warn-600); }

  .impact-candidates { margin-top: var(--sp-2); }
  .impact-candidates h4 {
    margin: 0 0 var(--sp-1);
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--ink-500);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .candidate-card {
    padding: var(--sp-2) var(--sp-3);
    margin-bottom: var(--sp-1);
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
  }
  .candidate-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--sp-1);
  }
  .candidate-target code { font-family: var(--font-mono); font-size: 0.85em; color: var(--memory-600); }
  .candidate-range { color: var(--ink-400); font-size: var(--text-xs); font-family: var(--font-mono); }

  .candidate-evidence {
    display: inline-flex;
    align-items: center;
    padding: 2px var(--sp-2);
    border-radius: var(--radius-sm);
    font-size: var(--text-xs);
    font-weight: 600;
    line-height: 1.5;
    border: 1px solid;
    white-space: nowrap;
  }
  .ev-verified { background: var(--accent-50); color: var(--accent-600); border-color: var(--accent-200); }
  .ev-not-found { background: var(--danger-50); color: var(--danger-600); border-color: #fecaca; }
  .ev-info { background: var(--canvas-muted); color: var(--ink-500); border-color: var(--line); }
  .ev-warning { background: var(--warn-50); color: var(--warn-600); border-color: #fde68a; }
  .ev-approximate { background: var(--memory-50); color: var(--memory-600); border-color: var(--memory-200); }

  .candidate-symbol { margin-bottom: var(--sp-1); }
  .candidate-symbol code { font-family: var(--font-mono); font-size: 0.85em; color: var(--memory-600); }

  .candidate-meta {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin-bottom: var(--sp-1);
  }
  .confidence-badge {
    display: inline-flex;
    align-items: center;
    padding: 2px var(--sp-2);
    border-radius: var(--radius-sm);
    font-size: var(--text-xs);
    font-weight: 600;
    line-height: 1.5;
    border: 1px solid;
    text-transform: uppercase;
    letter-spacing: 0.02em;
    white-space: nowrap;
  }
  .confidence-high { background: var(--ok-50); color: var(--ok-600); border-color: #a7f3d0; }
  .confidence-medium { background: var(--warn-50); color: var(--warn-600); border-color: #fde68a; }
  .confidence-low { background: var(--danger-50); color: var(--danger-600); border-color: #fecaca; }
  .candidate-sha { font-size: var(--text-xs); color: var(--ink-400); }
  .candidate-sha code { font-family: var(--font-mono); font-size: 0.85em; color: var(--memory-600); }

  .candidate-reasons {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-1);
    margin-bottom: var(--sp-1);
  }
  .reason-tag {
    padding: 2px var(--sp-2);
    background: var(--canvas-muted);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    font-size: var(--text-xs);
    color: var(--ink-500);
  }

  .candidate-actions { margin-top: var(--sp-2); }
  .btn-accept-binding {
    padding: var(--sp-1) var(--sp-2);
    border: 1px solid var(--accent-500);
    background: var(--accent-50);
    color: var(--accent-600);
    border-radius: var(--radius-sm);
    font-size: var(--text-xs);
    font-weight: 500;
    font-family: var(--font-sans);
    cursor: pointer;
    transition:
      background var(--dur-fast) var(--ease-out),
      border-color var(--dur-fast) var(--ease-out);
  }
  .btn-accept-binding:hover { background: var(--accent-100); border-color: var(--accent-400); }

  .impact-empty {
    padding: var(--sp-3);
    text-align: center;
    color: var(--ink-400);
    font-size: var(--text-sm);
    font-style: italic;
  }

  .impact-next-actions { margin-top: var(--sp-2); }
  .impact-next-actions h4 {
    margin: 0 0 var(--sp-1);
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--ink-500);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .next-action-item {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    padding: var(--sp-1) 0;
    font-size: var(--text-sm);
  }
  .next-action-desc { color: var(--ink-700); }
  .next-action-hint {
    font-family: var(--font-mono);
    font-size: 0.85em;
    color: var(--accent-600);
    background: var(--accent-50);
    padding: 2px var(--sp-1);
    border-radius: var(--radius-sm);
  }

  .impact-snapshot {
    margin-top: var(--sp-2);
    padding-top: var(--sp-1);
    border-top: 1px dashed var(--line);
    font-size: var(--text-xs);
    color: var(--ink-400);
  }
  .impact-snapshot code { font-family: var(--font-mono); font-size: 0.85em; color: var(--memory-600); }
</style>
