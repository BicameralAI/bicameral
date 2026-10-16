<script lang="ts">
  /**
   * Ledger View — canonical decision state and review surface.
   *
   * Shows Decision Ledger state with signoff plus reviewable context.
   * Proposed Decisions are visually distinct (collision/sensitivity pending).
   * Demotion actions emit governed review commands, not direct storage writes.
   * Does NOT expose manual Decision creation.
   */
  import { fetchLedgerView, submitReviewCommand, findCodeImpact, acceptBinding, refreshEvidence, DeferredCommandError } from "../lib/api";
  import type {
    LedgerReviewItem,
    LedgerDecision,
    LedgerCandidate,
    DecisionCommandKind,
    CandidateCommandKind,
    CodeEvidenceSection,
    EvidenceRefreshOutcome,
    FreshnessWarning,
    GraphEvidenceState,
    FindCodeImpactResult,
  } from "../lib/types";
  import CodeImpactPanel from "../lib/CodeImpactPanel.svelte";
  import GraphReadinessPanel from "../lib/GraphReadinessPanel.svelte";

  let items = $state<LedgerReviewItem[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let deferredNotice = $state<string | null>(null);
  let selectedId = $state<string | null>(null);

  let codeImpactResult = $state<FindCodeImpactResult | null>(null);
  let codeImpactLoading = $state(false);
  let codeImpactError = $state<string | null>(null);

  let refreshOutcome = $state<EvidenceRefreshOutcome | null>(null);
  let refreshLoading = $state(false);
  let refreshError = $state<string | null>(null);

  async function load() {
    loading = true;
    error = null;
    try {
      items = await fetchLedgerView();
    } catch (e: any) {
      error = e.message;
      items = [];
    } finally {
      loading = false;
    }
  }

  load();

  function isDecision(item: LedgerReviewItem): item is LedgerDecision {
    return item.kind === "decision";
  }

  function signoffClass(s: string) {
    switch (s) {
      case "proposed": return "badge-proposed";
      case "approved": return "badge-approved";
      case "rejected": return "badge-rejected";
      case "collision_pending": return "badge-collision";
      case "superseded": return "badge-superseded";
      default: return "";
    }
  }

  async function handleCommand(targetId: string, command: DecisionCommandKind | CandidateCommandKind | "demote_decision") {
    deferredNotice = null;
    try {
      await submitReviewCommand({ target_id: targetId, command });
      await load();
    } catch (e: unknown) {
      if (e instanceof DeferredCommandError) {
        deferredNotice = e.message;
      } else {
        error = e instanceof Error ? e.message : "Unknown error";
      }
    }
  }

  function selectedItem(): LedgerReviewItem | undefined {
    return items.find(i => i.id === selectedId);
  }

  function evidenceStateClass(state: GraphEvidenceState): string {
    switch (state) {
      case "verified": return "evidence-verified";
      case "not_found": return "evidence-warning";
      case "unknown_not_indexed": return "evidence-info";
      case "unknown_stale": return "evidence-warning";
      case "ambiguous": return "evidence-warning";
      case "unsupported": return "evidence-info";
      case "approximate_candidate": return "evidence-info";
    }
  }

  function resetCodeImpact() {
    codeImpactResult = null;
    codeImpactLoading = false;
    codeImpactError = null;
    refreshOutcome = null;
    refreshLoading = false;
    refreshError = null;
  }

  async function handleRefreshEvidence() {
    const sel = selectedItem();
    if (!sel || !isDecision(sel)) return;
    refreshLoading = true;
    refreshError = null;
    try {
      refreshOutcome = await refreshEvidence(sel.id);
    } catch (e: any) {
      refreshError = e.message;
    } finally {
      refreshLoading = false;
    }
  }

  async function handleFindCodeImpact() {
    const sel = selectedItem();
    if (!sel || !isDecision(sel)) return;
    codeImpactLoading = true;
    codeImpactError = null;
    codeImpactResult = null;
    try {
      codeImpactResult = await findCodeImpact(sel.id);
    } catch (e: any) {
      codeImpactError = e.message;
    } finally {
      codeImpactLoading = false;
    }
  }

  async function handleAcceptBinding(candidateTarget: string) {
    const sel = selectedItem();
    if (!sel || !isDecision(sel)) return;
    try {
      await acceptBinding(sel.id, candidateTarget);
      await handleFindCodeImpact();
      await load();
    } catch (e: any) {
      codeImpactError = e.message;
    }
  }

  function freshnessColor(f: string) {
    switch (f) {
      case "fresh": return "var(--ok-600)";
      case "stale": return "var(--warn-600)";
      case "offline": return "var(--danger-600)";
      default: return "var(--ink-400)";
    }
  }
</script>

<section>
  <header class="page-header">
    <h1>Decisions</h1>
    <p class="subtitle">
      Decision Ledger — Canonical state with signoff and review context.
    </p>
  </header>

  {#if deferredNotice}
    <div class="deferred-notice">
      <span class="deferred-badge">coming soon</span>
      <span>{deferredNotice}</span>
      <button class="dismiss-btn" onclick={() => (deferredNotice = null)}>&times;</button>
    </div>
  {/if}

  {#if loading}
    <div class="empty">Loading ledger…</div>
  {:else if error}
    <div class="error">{error}</div>
  {:else if items.length === 0}
    <div class="empty">
      <div class="empty-icon">⊘</div>
      <div class="empty-title">Ledger is empty</div>
      <div class="empty-body">
        No Decisions or queued candidates. Use the Ingestion Gate to promote
        source-derived candidates into the Decision Ledger through governed
        review.
      </div>
    </div>
  {:else}
    <div class="ledger-layout">
      <aside class="ledger-list">
        {#each items as item}
          <button
            class="ledger-row"
            class:selected={selectedId === item.id}
            class:is-proposed={isDecision(item) && item.signoff === "proposed"}
            class:is-collision={isDecision(item) && item.signoff === "collision_pending"}
            onclick={() => { selectedId = item.id; resetCodeImpact(); }}
          >
            <div class="row-top">
              {#if isDecision(item)}
                <span class="status-icon">●</span>
                <span class="badge {signoffClass(item.signoff)}">
                  {item.signoff.replace("_", " ")}
                </span>
                {#if item.signoff === "proposed" || item.signoff === "collision_pending"}
                  <span class="status-icon warning">⚡</span>
                {/if}
              {:else}
                <span class="status-icon">●</span>
                <span class="badge badge-candidate">candidate</span>
                <span class="status-icon warning">⚡</span>
              {/if}
            </div>
            <div class="row-summary">{item.summary}</div>
            {#if isDecision(item)}
              <div class="row-feature">{item.feature}</div>
              {#if item.source_freshness !== "fresh"}
                <div class="row-freshness" style="color: {freshnessColor(item.source_freshness)}">
                  source: {item.source_freshness}
                </div>
              {/if}
            {:else if item.feature_hint}
              <div class="row-feature">{item.feature_hint}</div>
            {/if}
          </button>
        {/each}
      </aside>

      {#if selectedId}
        {@const sel = selectedItem()}
        {#if sel}
          <div class="detail-pane">
            {#if isDecision(sel)}
              <h2>{sel.summary}</h2>
              
              <div class="state-grid">
                <div class="state-cell">
                  <span class="state-label">Signoff</span>
                  <span class="state-value memory">{sel.signoff.replace("_", " ")}</span>
                </div>
                <div class="state-cell">
                  <span class="state-label">Tracking</span>
                  <span class="state-value {sel.tracking_state === 'tracked' ? 'emerald' : 'neutral'}">{sel.tracking_state}</span>
                </div>
                <div class="state-cell">
                  <span class="state-label">Source</span>
                  <span class="state-value {sel.source_freshness === 'fresh' ? 'emerald' : sel.source_freshness === 'stale' ? 'amber' : 'red'}">{sel.source_freshness}</span>
                </div>
              </div>

              <div class="section">
                <h3>Graph Readiness</h3>
                <GraphReadinessPanel
                  readiness={sel.graph_readiness}
                  {refreshOutcome}
                  {refreshLoading}
                  {refreshError}
                  onRefresh={handleRefreshEvidence}
                />
              </div>

              <dl class="detail-meta">
                <dt>ID</dt><dd><code>{sel.id}</code></dd>
                <dt>Feature</dt><dd>{sel.feature}</dd>
                {#if sel.parent_id}<dt>Parent</dt><dd><code>{sel.parent_id}</code></dd>{/if}
              </dl>
              {#if sel.discovered}
                <span class="discovered-tag">agent-discovered</span>
              {/if}
              {#if sel.conflicts_with && sel.conflicts_with.length > 0}
                <div class="conflicts">
                  <h3>Conflicts</h3>
                  <p>Overlaps with: {sel.conflicts_with.join(", ")}</p>
                </div>
              {/if}

              {#if sel.sources.length > 0}
                <div class="section">
                  <h3>Sources</h3>
                  {#each sel.sources as ev}
                    <div class="evidence-item">
                      <div class="ev-meta">
                        <code>{ev.source_uri}</code>
                        <span class="pointer-type">{ev.pointer_type}: {ev.pointer_value}</span>
                      </div>
                      <blockquote>{ev.excerpt}</blockquote>
                    </div>
                  {/each}
                </div>
              {/if}

              {#if sel.freshness_warning}
                <div class="section">
                  <div class="freshness-warning-box">
                    <div class="freshness-warning-msg">{sel.freshness_warning.message}</div>
                    <div class="freshness-action-bar">
                      {#each sel.freshness_warning.actions as act}
                        <button
                          class="btn-freshness-action"
                          onclick={() => handleCommand(sel.id, act === "refresh_source" ? "refresh_source" : act === "revalidate_binding" ? "bind_to_code" : act === "untrack" ? "untrack_decision" : "request_context")}
                        >
                          {act.replace(/_/g, " ")}
                        </button>
                      {/each}
                    </div>
                  </div>
                </div>
              {/if}

              {#if sel.code_evidence}
                <div class="section">
                  <h3>Code Evidence</h3>
                  <div class="code-evidence-div {evidenceStateClass(sel.code_evidence.evidence_state)}">
                    <div class="evidence-header">
                      <span class="evidence-label">{sel.code_evidence.evidence_label}</span>
                      <span class="evidence-state-tag">{sel.code_evidence.evidence_state.replace(/_/g, " ")}</span>
                    </div>
                    <div class="evidence-copy">{sel.code_evidence.evidence_copy}</div>
                    {#if sel.code_evidence.graph_snapshot_id}
                      <div class="evidence-meta">
                        <span>Snapshot: <code>{sel.code_evidence.graph_snapshot_id}</code></span>
                        {#if sel.code_evidence.validated_sha}
                          <span> @ <code>{sel.code_evidence.validated_sha}</code></span>
                        {/if}
                      </div>
                    {/if}
                    {#if sel.code_evidence.scope_sufficiency}
                      <div class="evidence-meta">Scope: {sel.code_evidence.scope_sufficiency}</div>
                    {/if}
                    {#if sel.code_evidence.allowed_actions.length > 0}
                      <div class="evidence-action-bar">
                        {#each sel.code_evidence.allowed_actions as act}
                          <button class="btn-evidence-action">{act.replace(/_/g, " ")}</button>
                        {/each}
                      </div>
                    {/if}
                  </div>
                </div>
              {/if}

              {#if sel.signoff === "approved"}
                <div class="section">
                  <h3>Code Impact Discovery</h3>
                  <CodeImpactPanel
                    result={codeImpactResult}
                    loading={codeImpactLoading}
                    error={codeImpactError}
                    onAcceptBinding={handleAcceptBinding}
                    onFindCodeImpact={handleFindCodeImpact}
                  />
                </div>
              {/if}

              {#if sel.regions && sel.regions.length > 0}
                <div class="section">
                  <h3>Code Regions</h3>
                  {#each sel.regions as r}
                    <div class="region">
                      <code>{r.file}:{r.start_line}-{r.end_line}</code>
                    </div>
                  {/each}
                </div>
              {:else if !sel.code_evidence}
                <div class="section">
                  <h3>Code Evidence</h3>
                  <div class="no-regions">no code regions bound yet</div>
                </div>
              {/if}

              <div class="section">
                <h3>Actions</h3>
                <div class="command-bar">
                  {#each sel.allowed_commands as cmd}
                    <button
                      class="btn-command"
                      onclick={() => handleCommand(sel.id, cmd)}
                    >
                      {cmd.replace(/_/g, " ")}
                    </button>
                  {/each}
                </div>
              </div>

              <div class="section deferred-section">
                <h3>
                  Deferred capabilities
                  <span class="deferred-badge-inline">coming soon</span>
                </h3>
                <ul class="deferred-list">
                  <li>Preflight checks</li>
                  <li>Compliance resolution</li>
                  <li>Source tracking management</li>
                </ul>
              </div>
            {:else}
              <h2>{sel.summary}</h2>
              <dl class="detail-meta">
                <dt>ID</dt><dd><code>{sel.id}</code></dd>
                <dt>Review state</dt><dd><span class="badge badge-review-state">{sel.review_state}</span></dd>
                {#if sel.feature_hint}<dt>Feature hint</dt><dd>{sel.feature_hint}</dd>{/if}
              </dl>

              {#if sel.sources.length > 0}
                <div class="section">
                  <h3>Sources</h3>
                  {#each sel.sources as ev}
                    <div class="evidence-item">
                      <div class="ev-meta"><code>{ev.source_uri}</code></div>
                      <blockquote>{ev.excerpt}</blockquote>
                    </div>
                  {/each}
                </div>
              {/if}

              <div class="section">
                <h3>Actions</h3>
                <div class="command-bar">
                  {#each sel.allowed_commands as cmd}
                    <button
                      class="btn-command"
                      onclick={() => handleCommand(sel.id, cmd)}
                    >
                      {cmd.replace(/_/g, " ")}
                    </button>
                  {/each}
                </div>
              </div>
            {/if}
          </div>
        {/if}
      {/if}
    </div>
  {/if}
</section>

<style>
  section { max-width: 1100px; }

  .page-header { margin-bottom: 1.25rem; }
  h1 { font-size: 1.125rem; font-weight: 600; margin: 0 0 0.25rem; color: var(--ink-900); }
  .subtitle { color: var(--ink-500); font-size: 0.875rem; margin: 0; }

  .deferred-notice {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.625rem 1rem;
    margin-bottom: 1rem;
    background: var(--warn-50);
    border: 1px solid #fde68a;
    border-radius: var(--radius-md);
    font-size: 0.8125rem;
    color: var(--warn-600);
  }
  .deferred-badge {
    display: inline-block;
    padding: 0.125rem 0.5rem;
    background: var(--warn-50);
    border: 1px solid #fde68a;
    border-radius: var(--radius-sm);
    font-size: 0.6875rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    color: var(--warn-600);
    flex-shrink: 0;
  }
  .dismiss-btn {
    margin-left: auto;
    background: none;
    border: none;
    color: var(--warn-600);
    font-size: 1.125rem;
    cursor: pointer;
    padding: 0 0.25rem;
    line-height: 1;
  }

  .empty {
    text-align: center;
    padding: 3rem 1rem;
    color: var(--ink-400);
  }
  .empty-icon { font-size: 2rem; margin-bottom: 0.5rem; }
  .empty-title { font-size: 1rem; font-weight: 600; color: var(--ink-500); }
  .empty-body { font-size: 0.8125rem; max-width: 440px; margin: 0.5rem auto 0; line-height: 1.5; }

  .error { color: var(--danger-600); padding: 1rem; background: var(--danger-50); border: 1px solid #fecaca; border-radius: var(--radius-lg); }

  .ledger-layout { display: flex; gap: 1.25rem; min-height: 400px; }

  .ledger-list {
    width: 340px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: 0px;
    overflow: auto;
  }

  .ledger-row {
    display: flex; flex-direction: column; gap: 0.25rem;
    padding: 0.75rem 1rem;
    background: var(--surface);
    border: 1px solid var(--line);
    cursor: pointer;
    text-align: left;
    color: var(--ink-900);
    font-size: 0.8125rem;
    font-family: inherit;
    transition: all var(--dur-fast) var(--ease-out);
  }
  .ledger-row:hover { border-color: var(--line-strong); }
  .ledger-row.selected { border-color: var(--accent-500); background: var(--accent-50); }

  .ledger-row.is-proposed { border-left: 2px solid var(--warn-500); }
  .ledger-row.is-collision { border-left: 2px solid var(--memory-600); }

  .row-top { display: flex; gap: 0.5rem; align-items: center; }
  .row-summary { font-weight: 600; }
  .row-feature { color: var(--ink-400); font-size: 0.75rem; }
  .row-freshness { font-size: 0.6875rem; font-weight: 500; }

  .status-icon {
    font-size: 0.875rem;
    width: 1rem;
    text-align: center;
    flex-shrink: 0;
  }

  .status-icon {
    color: var(--ok-500);
  }

  .status-icon.warning {
    color: var(--warn-500);
  }

  .badge {
    padding: 0.125rem 0.5rem;
    border-radius: var(--radius-sm);
    font-size: 0.6875rem;
    font-weight: 500;
    border: 1px solid;
    font-family: var(--font-mono);
  }

  .badge-proposed { background: var(--warn-50); color: var(--warn-600); border-color: #fde68a; }
  .badge-approved { background: var(--ok-50); color: var(--ok-600); border-color: #a7f3d0; }
  .badge-rejected { background: var(--danger-50); color: var(--danger-600); border-color: #fecaca; }
  .badge-collision { background: var(--memory-50); color: var(--memory-600); border-color: var(--memory-200); }
  .badge-superseded { background: var(--canvas-muted); color: var(--ink-500); border-color: var(--line-strong); }

  .badge-candidate { background: var(--accent-50); color: var(--accent-600); border-color: var(--accent-300); }
  .badge-review-state { background: var(--canvas-muted); color: var(--ink-500); border-color: var(--line-strong); }

  .state-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 1px;
    border: 1px solid var(--line);
    background: var(--line);
    margin-bottom: 1rem;
  }

  .state-cell {
    background: var(--surface);
    padding: 0.75rem 1rem;
  }

  .state-label {
    display: block;
    font-family: var(--font-mono);
    font-size: 0.625rem;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--ink-400);
    margin-bottom: 0.25rem;
  }

  .state-value {
    font-size: 0.75rem;
    font-weight: 500;
  }

  .state-value.emerald { color: var(--ok-600); }
  .state-value.amber { color: var(--warn-600); }
  .state-value.red { color: var(--danger-600); }
  .state-value.memory { color: var(--memory-600); }
  .state-value.neutral { color: var(--ink-500); }

  .detail-pane {
    flex: 1;
    background: var(--surface);
    border: 1px solid var(--line);
    padding: 1.25rem;
    overflow: auto;
  }
  .detail-pane h2 { margin: 0 0 0.75rem; font-size: 1rem; font-weight: 600; color: var(--ink-900); }

  .detail-meta {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0.25rem 0.75rem;
    font-size: 0.8125rem;
    color: var(--ink-500);
  }
  .detail-meta dt { font-weight: 500; color: var(--ink-400); }
  .detail-meta dd { margin: 0; }
  .detail-meta code { color: var(--accent-500); font-size: 0.75rem; font-family: var(--font-mono); }

  .discovered-tag {
    display: inline-block;
    margin-top: 0.5rem;
    padding: 0.12rem 0.4rem;
    background: var(--memory-100);
    color: var(--memory-700);
    border: 1px solid var(--memory-300);
    border-radius: var(--radius-sm);
    font-size: 0.68rem;
    font-weight: 500;
  }

  .conflicts {
    margin-top: 1rem;
    padding: 0.75rem 1rem;
    background: var(--warn-50);
    border: 1px solid #fde68a;
    border-radius: var(--radius-lg);
    color: var(--warn-600);
    font-size: 0.82rem;
  }
  .conflicts h3 { color: var(--warn-600); font-size: 0.82rem; margin-bottom: 0.25rem; text-transform: none; letter-spacing: 0; }
  .conflicts p { margin: 0; }

  .section { margin-top: 1.25rem; }
  h3 { margin: 0 0 0.5rem; font-size: 0.82rem; font-weight: 600; color: var(--ink-400); text-transform: uppercase; letter-spacing: 0.03em; }

  .no-regions {
    padding: 0.5rem 0.75rem;
    background: var(--canvas-muted);
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    color: var(--ink-400);
    font-size: 0.82rem;
    font-style: italic;
  }

  .evidence-item { margin-bottom: 0.5rem; font-size: 0.82rem; }
  .ev-meta { display: flex; gap: 0.5rem; align-items: center; }
  .ev-meta code { color: var(--accent-600); font-size: 0.75rem; }
  .pointer-type { color: var(--ink-400); font-size: 0.72rem; }
  blockquote {
    margin: 0.2rem 0 0 0;
    padding: 0.4rem 0.75rem;
    border-left: 3px solid var(--line);
    color: var(--ink-700);
    font-size: 0.82rem;
    font-style: italic;
    background: var(--canvas-muted);
    border-radius: 0 var(--radius-md) var(--radius-md) 0;
  }

  .region { font-size: 0.82rem; margin-bottom: 0.2rem; }
  .region code { color: var(--accent-600); }

  .code-evidence-section {
    padding: 0.75rem;
    background: var(--canvas-muted);
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    margin-bottom: 0.5rem;
  }
  .evidence-state-row { display: flex; align-items: center; gap: 0.5rem; margin-bottom: 0.35rem; }
  .evidence-badge {
    padding: 0.15rem 0.5rem;
    border-radius: var(--radius-sm);
    font-size: 0.72rem;
    font-weight: 500;
    border: 1px solid;
  }
  .evidence-verified { background: var(--ok-50); color: var(--ok-600); border-color: #a7f3d0; }
  .evidence-not_found { background: var(--danger-50); color: var(--danger-600); border-color: #fecaca; }
  .evidence-unknown_not_indexed { background: var(--warn-50); color: var(--warn-600); border-color: #fde68a; }
  .evidence-unknown_stale { background: var(--warn-50); color: var(--warn-600); border-color: #fde68a; }
  .evidence-ambiguous { background: var(--memory-100); color: var(--memory-700); border-color: var(--memory-200); }
  .evidence-unsupported { background: var(--canvas-muted); color: var(--ink-500); border-color: var(--line-strong); }
  .evidence-approximate_candidate { background: var(--memory-100); color: var(--memory-700); border-color: var(--memory-300); }
  .snapshot-id { color: var(--ink-400); font-size: 0.72rem; font-family: var(--font-mono); }
  .evidence-copy { margin: 0.25rem 0 0.35rem; font-size: 0.8rem; color: var(--ink-700); line-height: 1.4; }
  .scope-sufficiency { font-size: 0.75rem; color: var(--ink-500); margin-bottom: 0.35rem; }
  .evidence-actions { display: flex; flex-wrap: wrap; gap: 0.25rem; }
  .action-tag {
    padding: 0.1rem 0.4rem;
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-sm);
    font-size: 0.7rem;
    color: var(--ink-500);
  }

  .command-bar { display: flex; flex-wrap: wrap; gap: 0.35rem; }
  .btn-command {
    padding: 0.35rem 0.65rem;
    border: 1px solid var(--line);
    background: var(--surface);
    color: var(--ink-900);
    border-radius: var(--radius-md);
    font-size: 0.78rem;
    font-family: inherit;
    cursor: pointer;
    transition: all var(--dur-fast) var(--ease-out);
  }
  .btn-command:hover { background: var(--canvas-muted); border-color: var(--accent-300); }

  /* Freshness warning box */
  .freshness-warning-box {
    padding: 0.75rem 1rem;
    background: var(--warn-50);
    border: 1px solid #fde68a;
    border-radius: var(--radius-lg);
    font-size: 0.82rem;
  }
  .freshness-warning-msg { color: var(--warn-600); margin-bottom: 0.5rem; }
  .freshness-action-bar { display: flex; flex-wrap: wrap; gap: 0.35rem; }
  .btn-freshness-action {
    padding: 0.3rem 0.6rem;
    border: 1px solid #fde68a;
    background: var(--warn-50);
    color: var(--warn-600);
    border-radius: var(--radius-sm);
    font-size: 0.75rem;
    font-family: inherit;
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease-out);
  }
  .btn-freshness-action:hover { background: #fde68a; }

  /* Code evidence section */
  .code-evidence-div {
    padding: 0.75rem 1rem;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    font-size: 0.82rem;
  }
  .code-evidence-div.evidence-verified {
    border-color: #a7f3d0;
    background: var(--ok-50);
  }
  .code-evidence-div.evidence-warning {
    border-color: #fde68a;
    background: var(--warn-50);
  }
  .code-evidence-div.evidence-info {
    border-color: var(--memory-300);
    background: var(--memory-50);
  }
  .evidence-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 0.35rem; }
  .evidence-label { font-weight: 600; color: var(--ink-900); }
  .evidence-state-tag {
    padding: 0.08rem 0.35rem;
    border-radius: var(--radius-sm);
    font-size: 0.65rem;
    font-weight: 500;
    background: var(--canvas-muted);
    color: var(--ink-500);
    border: 1px solid var(--line-strong);
    text-transform: uppercase;
    letter-spacing: 0.02em;
  }
  .evidence-copy { color: var(--ink-700); line-height: 1.45; margin-bottom: 0.35rem; }
  .evidence-meta { font-size: 0.75rem; color: var(--ink-500); margin-bottom: 0.2rem; }
  .evidence-meta code { color: var(--accent-600); font-size: 0.72rem; }
  .evidence-action-bar { display: flex; flex-wrap: wrap; gap: 0.3rem; margin-top: 0.5rem; }
  .btn-evidence-action {
    padding: 0.25rem 0.55rem;
    border: 1px solid var(--line);
    background: var(--surface);
    color: var(--ink-900);
    border-radius: var(--radius-sm);
    font-size: 0.72rem;
    font-family: inherit;
    cursor: pointer;
    transition: all var(--dur-fast) var(--ease-out);
  }
  .btn-evidence-action:hover { background: var(--canvas-muted); border-color: var(--accent-300); }

  .deferred-section {
    opacity: 0.7;
    border-top: 1px dashed var(--line);
    padding-top: 0.75rem;
  }
  .deferred-section h3 {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .deferred-badge-inline {
    display: inline-block;
    padding: 0.1rem 0.4rem;
    background: var(--warn-50);
    border: 1px solid #fde68a;
    border-radius: var(--radius-sm);
    font-size: 0.625rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    color: var(--warn-600);
  }
  .deferred-list {
    list-style: none;
    padding: 0;
    margin: 0.5rem 0 0;
    font-size: 0.75rem;
    color: var(--ink-500);
  }
  .deferred-list li {
    padding: 0.2rem 0;
  }
  .deferred-list li::before {
    content: "\2022";
    margin-right: 0.4rem;
    color: var(--line-strong);
  }
</style>
