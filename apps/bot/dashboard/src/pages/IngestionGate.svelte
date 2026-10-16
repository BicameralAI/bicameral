<script lang="ts">
  /**
   * Ingestion Gate — source/candidate intake page.
   *
   * Shows existing DecisionCandidates for selected source snapshots.
   * Does NOT provide a manual Decision creation form.
   * Ingest batch-promotes remaining candidates for a selected snapshot.
   * Reject emits durable reject_candidate review events.
   *
   * Display hints: when a connector descriptor declares `ingestion_gate`
   * hints, source rows use connector-specific labels for source_type,
   * evidence pointers, and trust context. Unknown sources fall back to
   * generic rendering.
   */
  import { fetchIngestionGate, submitReviewCommand, DeferredCommandError } from "../lib/api";
  import type { IngestionGateItem, DecisionCandidatePreview, ContextDisposition } from "../lib/types";
  import { CONNECTOR_DESCRIPTORS } from "../lib/connector-descriptors";
  import type { ConnectorDescriptor, IngestionGateHints } from "../lib/connector-types";
  import type { ToolSubmission } from "../lib/connection-fixtures";

  let items = $state<IngestionGateItem[]>([]);
  let toolSubmissions = $state<ToolSubmission[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let deferredNotice = $state<string | null>(null);
  let selectedIdx = $state<number | null>(null);
  let selectedToolIdx = $state<number | null>(null);

  // ── Display hint resolution ─────────────────────────────────────

  /** Map source_type string to a connector descriptor (if known). */
  function resolveConnector(sourceType: string): ConnectorDescriptor | undefined {
    const normalized = sourceType.toLowerCase();
    return CONNECTOR_DESCRIPTORS.find(
      (c) => normalized === c.id || normalized.startsWith(c.id),
    );
  }

  /** Get ingestion_gate hints for a source item, or undefined for unknown sources. */
  function getHints(item: IngestionGateItem): IngestionGateHints | undefined {
    const connector = resolveConnector(item.source.source_type);
    return connector?.ingestion_gate;
  }

  /** Resolve display label for a source_type badge. */
  function sourceTypeLabel(item: IngestionGateItem): string {
    const hints = getHints(item);
    return hints?.source_type_label ?? item.source.source_type;
  }

  /** Resolve display label for an evidence pointer_type. */
  function evidencePointerLabel(item: IngestionGateItem, pointerType: string): string {
    const hints = getHints(item);
    return hints?.evidence_labels?.[pointerType] ?? pointerType;
  }

  /** Get trust context (pii_posture) for the connector, if available. */
  function trustContext(item: IngestionGateItem): string | undefined {
    const connector = resolveConnector(item.source.source_type);
    return connector?.data.pii_posture;
  }

  // ── Core logic ──────────────────────────────────────────────────

  async function load() {
    loading = true;
    error = null;
    try {
      items = await fetchIngestionGate();
    } catch (e: any) {
      error = e.message;
      items = [];
    } finally {
      loading = false;
    }
  }

  load();

  function surfaceLabel(kind: string): string {
    switch (kind) {
      case "mcp": return "MCP Session";
      case "cli": return "CLI";
      case "agent": return "Agent";
      case "browser_extension": return "Browser Extension";
      default: return kind;
    }
  }

  function formatTimestamp(iso: string): string {
    try {
      return new Date(iso).toLocaleString();
    } catch {
      return iso;
    }
  }

  function freshnessColor(f: string, advisory: boolean) {
    if (advisory && f === "stale") return "var(--ink-400)";
    switch (f) {
      case "fresh": return "var(--ok-600)";
      case "stale": return "var(--warn-600)";
      case "offline": return "var(--danger-600)";
      default: return "var(--ink-400)";
    }
  }

  function dispositionLabel(d: ContextDisposition): string {
    switch (d) {
      case "raw_source": return "raw source";
      case "candidate_context": return "candidate";
      case "tracked_decision": return "tracked";
    }
  }

  function isRejected(candidate: DecisionCandidatePreview) {
    return candidate.candidate_state === "rejected";
  }

  async function handleIngest(item: IngestionGateItem) {
    const remaining = item.candidates.filter(c => !isRejected(c));
    if (remaining.length === 0) return;
    deferredNotice = null;
    try {
      await submitReviewCommand({
        target_id: item.snapshot.snapshot_addr,
        command: "ingest_batch",
        reason: `Batch promote ${remaining.length} candidate(s) from ${item.source.uri}`,
      });
      await load();
    } catch (e: unknown) {
      if (e instanceof DeferredCommandError) {
        deferredNotice = e.message;
      } else {
        error = e instanceof Error ? e.message : "Unknown error";
      }
    }
  }

  async function handleReject(candidate: DecisionCandidatePreview) {
    if (!candidate.id) return;
    deferredNotice = null;
    try {
      await submitReviewCommand({
        target_id: candidate.id,
        command: "reject_candidate",
      });
      await load();
    } catch (e: unknown) {
      if (e instanceof DeferredCommandError) {
        deferredNotice = e.message;
      } else {
        error = e instanceof Error ? e.message : "Unknown error";
      }
    }
  }
</script>

<section>
  <header class="page-header">
    <h1>Activity</h1>
    <p class="subtitle">
      Evidence Review — Source-derived candidates awaiting governed promotion.
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
    <div class="empty">Loading sources…</div>
  {:else}
    {#if error}
      <div class="error">{error}</div>
    {:else if items.length === 0 && toolSubmissions.length === 0}
      <div class="empty">
        <div class="empty-icon">⊘</div>
        <div class="empty-title">No activity</div>
        <div class="empty-body">
          Connect a source, run an extractor, or use an MCP tool to produce
          evidence. The Activity view shows source-derived candidates and
          tool submissions — it is not a manual creation form.
        </div>
      </div>
    {/if}

    {#if items.length > 0}
    <div class="gate-layout">
      <aside class="source-list">
        {#each items as item, idx}
          <button
            class="source-card"
            class:selected={selectedIdx === idx}
            onclick={() => { selectedIdx = idx; selectedToolIdx = null; }}
          >
            <div class="source-card-top">
              <span class="source-type">{sourceTypeLabel(item)}</span>
              <span class="disposition-badge disposition-{item.context_disposition}">
                {dispositionLabel(item.context_disposition)}
              </span>
            </div>
            <span class="source-title">{item.source_title}</span>
            <span
              class="freshness"
              style="color: {freshnessColor(item.source_freshness, item.stale_is_advisory)}"
            >
              {item.source_freshness}{#if item.stale_is_advisory && item.source_freshness === "stale"} (advisory){/if}
            </span>
            <span class="candidate-count">
              {item.candidates.length} candidate{item.candidates.length !== 1 ? "s" : ""}
            </span>
          </button>
        {/each}
      </aside>

      {#if selectedIdx !== null && items[selectedIdx]}
        {@const sel = items[selectedIdx]}
        <div class="detail-pane">
          <div class="detail-header">
            <h2>{sel.source_title}</h2>
            <div class="detail-status-row">
              <span class="disposition-badge disposition-{sel.context_disposition}">
                {dispositionLabel(sel.context_disposition)}
              </span>
              {#if sel.tracked}
                <span class="tracking-badge tracked">tracked</span>
              {:else}
                <span class="tracking-badge untracked">untracked</span>
              {/if}
              {#if sel.stale_is_advisory && sel.source_freshness === "stale"}
                <span class="advisory-notice">Staleness is advisory — this context is not within governance freshness scope.</span>
              {/if}
            </div>
            <dl class="snapshot-meta">
              <dt>URI</dt><dd><code>{sel.source.uri}</code></dd>
              <dt>Snapshot</dt><dd><code>{sel.snapshot.snapshot_addr}</code></dd>
              <dt>Ref</dt><dd><code>{sel.snapshot.snapshot_ref}</code></dd>
              <dt>Captured</dt><dd>{sel.snapshot.captured_at}</dd>
            </dl>
          </div>

          {#if sel.projection_state && sel.projection_state.outcome !== "candidate_created"}
            <div class="projection-notice projection-notice-{sel.projection_state.outcome}">
              <span class="projection-notice-label">Projection</span>
              <span class="projection-notice-message">{sel.projection_state.message}</span>
              {#if sel.projection_state.repair_hint}
                <span class="projection-notice-hint">{sel.projection_state.repair_hint}</span>
              {/if}
            </div>
          {/if}

          {#if sel.evidence.length > 0}
            <div class="section">
              <h3>Source evidence</h3>
              {#each sel.evidence as ev}
                <div class="evidence-item">
                  <span class="pointer-type">{evidencePointerLabel(sel, ev.pointer_type)}</span>
                  <code class="pointer-val">{ev.pointer_value}</code>
                  <blockquote>{ev.excerpt}</blockquote>
                </div>
              {/each}
            </div>
          {/if}

          {#if trustContext(sel)}
            <div class="section trust-context">
              <h3>Trust context</h3>
              <p class="trust-posture">{trustContext(sel)}</p>
            </div>
          {/if}

          <div class="section">
            <h3>Candidates</h3>
            {#if sel.candidates.length === 0}
              <p class="empty-inline">No candidates extracted for this snapshot.</p>
            {:else}
              {#each sel.candidates as c}
                <div class="candidate-row" class:rejected={isRejected(c)}>
                  <div class="candidate-summary">{c.summary}</div>
                  {#if c.feature_hint}
                    <span class="feature-hint">{c.feature_hint}</span>
                  {/if}
                  {#if c.conflict_hint}
                    <span class="badge collision">conflict</span>
                  {/if}
                  {#if c.review_state}
                    <span class="badge review-state">{c.review_state}</span>
                  {/if}
                  {#if !isRejected(c)}
                    <button class="btn-reject" onclick={() => handleReject(c)}>
                      Reject
                    </button>
                  {/if}
                </div>
              {/each}
              <div class="ingest-action">
                <button class="btn-ingest" onclick={() => handleIngest(sel)}>
                  Ingest remaining candidates
                </button>
                <span class="ingest-hint">
                  Batch-promotes remaining candidates through governed review.
                </span>
              </div>
            {/if}
          </div>
        </div>
      {/if}
    </div>
    {/if}

    {#if toolSubmissions.length > 0}
      <div class="tool-submissions-section">
        <h2 class="section-heading">Tool Submissions</h2>
        <p class="section-desc">
          Observations submitted by tools and agents. These are not managed
          sources — they carry operator and tool provenance for review.
        </p>
        <div class="tool-layout">
          <aside class="tool-list">
            {#each toolSubmissions as ts, idx}
              <button
                class="tool-card"
                class:selected={selectedToolIdx === idx}
                onclick={() => { selectedToolIdx = idx; selectedIdx = null; }}
              >
                <div class="tool-card-top">
                  <span class="tool-surface-badge">{surfaceLabel(ts.surface.kind)}</span>
                  <span class="tool-kind-label">tool submission</span>
                </div>
                <span class="tool-card-tool">{ts.surface.tool}</span>
                <span class="tool-card-operator">
                  by {ts.submitted_by.local_actor_label}
                </span>
                <span class="tool-card-time">{formatTimestamp(ts.submitted_at)}</span>
              </button>
            {/each}
          </aside>

          {#if selectedToolIdx !== null && toolSubmissions[selectedToolIdx]}
            {@const ts = toolSubmissions[selectedToolIdx]}
            <div class="tool-detail-pane">
              <div class="tool-detail-header">
                <h3 class="tool-detail-title">{ts.surface.tool}</h3>
                <div class="tool-detail-badges">
                  <span class="tool-surface-badge">{surfaceLabel(ts.surface.kind)}</span>
                  <span class="tool-kind-label">tool submission</span>
                </div>
              </div>

              <dl class="tool-meta">
                <dt>Submitted by</dt>
                <dd>{ts.submitted_by.local_actor_label} <span class="meta-secondary">({ts.submitted_by.operator_id})</span></dd>
                <dt>Surface</dt>
                <dd>{surfaceLabel(ts.surface.kind)} / {ts.surface.tool}</dd>
                <dt>Session</dt>
                <dd><code>{ts.session_uri}</code></dd>
                <dt>Source</dt>
                <dd><code>{ts.source_uri}</code></dd>
                <dt>Submitted</dt>
                <dd>{formatTimestamp(ts.submitted_at)}</dd>
              </dl>

              <div class="tool-evidence">
                <h4>Evidence</h4>
                <div class="tool-evidence-card">
                  <span class="tool-call-id">call: {ts.evidence.tool_call_id}</span>
                  <blockquote>{ts.evidence.excerpt}</blockquote>
                </div>
              </div>
            </div>
          {/if}
        </div>
      </div>
    {/if}
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

  .gate-layout { display: flex; gap: 1.25rem; min-height: 400px; }

  .source-list {
    width: 280px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: 0px;
    overflow: auto;
  }

  .source-card {
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
  .source-card:hover { border-color: var(--line-strong); background: var(--canvas-muted); }
  .source-card.selected { border-color: var(--accent-500); background: var(--accent-50); }

  .source-card-top { display: flex; justify-content: space-between; align-items: center; }
  .source-type { color: var(--ink-400); font-size: 0.75rem; text-transform: uppercase; letter-spacing: 0.04em; }

  .disposition-badge {
    padding: 0.125rem 0.5rem;
    border-radius: var(--radius-sm);
    font-size: 0.6875rem;
    font-weight: 500;
    border: 1px solid;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    font-family: var(--font-mono);
  }
  .disposition-raw_source { background: var(--canvas-muted); color: var(--ink-500); border-color: var(--line-strong); }
  .disposition-candidate_context { background: var(--accent-50); color: var(--accent-600); border-color: var(--accent-300); }
  .disposition-tracked_decision { background: var(--ok-50); color: var(--ok-600); border-color: #a7f3d0; }

  .detail-status-row { display: flex; align-items: center; gap: 0.5rem; margin-bottom: 0.75rem; flex-wrap: wrap; }

  .tracking-badge {
    padding: 0.125rem 0.5rem;
    border-radius: var(--radius-sm);
    font-size: 0.6875rem;
    font-weight: 500;
    border: 1px solid;
    font-family: var(--font-mono);
  }
  .tracking-badge.tracked { background: var(--ok-50); color: var(--ok-600); border-color: #a7f3d0; }
  .tracking-badge.untracked { background: var(--canvas-muted); color: var(--ink-500); border-color: var(--line-strong); }

  .advisory-notice {
    font-size: 0.75rem;
    color: var(--ink-400);
    font-style: italic;
  }
  .source-title { font-weight: 600; }
  .freshness { font-size: 0.75rem; }
  .candidate-count { color: var(--ink-500); font-size: 0.75rem; }

  .detail-pane {
    flex: 1;
    background: var(--surface);
    border: 1px solid var(--line);
    padding: 1.25rem;
    overflow: auto;
  }

  .detail-header h2 { margin: 0 0 0.75rem; font-size: 1rem; font-weight: 600; color: var(--ink-900); }

  .snapshot-meta {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0.25rem 0.75rem;
    font-size: 0.8125rem;
    color: var(--ink-500);
  }
  .snapshot-meta dt { font-weight: 500; color: var(--ink-400); }
  .snapshot-meta dd { margin: 0; }
  .snapshot-meta code { color: var(--accent-500); font-size: 0.75rem; font-family: var(--font-mono); }

  .section { margin-top: 1.5rem; }
  h3 { margin: 0 0 0.75rem; font-size: 0.75rem; font-weight: 600; color: var(--ink-400); text-transform: uppercase; letter-spacing: 0.05em; font-family: var(--font-mono); }

  .evidence-item { margin-bottom: 0.75rem; font-size: 0.8125rem; }
  .pointer-type { color: var(--ink-400); font-size: 0.6875rem; margin-right: 0.5rem; font-family: var(--font-mono); }
  .pointer-val { font-size: 0.75rem; color: var(--accent-500); font-family: var(--font-mono); }
  blockquote {
    margin: 0.5rem 0 0 0;
    padding: 0.75rem 1rem;
    border-left: 2px solid var(--memory-600);
    color: var(--ink-700);
    font-size: 0.8125rem;
    font-style: italic;
    background: var(--canvas-muted);
    border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
  }

  .candidate-row {
    display: flex; align-items: center; gap: 0.5rem; flex-wrap: wrap;
    padding: 0.75rem 0;
    border-bottom: 1px solid var(--canvas-muted);
    font-size: 0.8125rem;
  }
  .candidate-row.rejected { opacity: 0.4; text-decoration: line-through; }
  .candidate-summary { flex: 1; min-width: 200px; color: var(--ink-900); }
  .feature-hint { color: var(--ink-400); font-size: 0.75rem; }

  .badge {
    padding: 0.125rem 0.5rem;
    border-radius: var(--radius-sm);
    font-size: 0.6875rem;
    font-weight: 500;
    border: 1px solid;
    font-family: var(--font-mono);
  }
  .collision { background: var(--memory-50); color: var(--memory-600); border-color: var(--memory-200); }
  .review-state { background: var(--canvas-muted); color: var(--ink-500); border-color: var(--line-strong); }

  .btn-reject {
    padding: 0.25rem 0.75rem;
    border: 1px solid #fecaca;
    background: var(--danger-50);
    color: var(--danger-600);
    border-radius: var(--radius-sm);
    font-size: 0.75rem;
    font-family: inherit;
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease-out);
  }
  .btn-reject:hover { background: var(--danger-50); }

  .ingest-action { margin-top: 1rem; display: flex; align-items: center; gap: 0.75rem; }
  .btn-ingest {
    padding: 0.5rem 1rem;
    border: 1px solid #a7f3d0;
    background: var(--ok-50);
    color: var(--ok-600);
    border-radius: var(--radius-sm);
    font-size: 0.8125rem;
    font-weight: 500;
    font-family: inherit;
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease-out);
  }
  .btn-ingest:hover { background: var(--ok-50); }
  .ingest-hint { color: var(--ink-400); font-size: 0.75rem; }

  .empty-inline { color: var(--ink-400); font-size: 0.8125rem; }

  .trust-posture {
    margin: 0;
    padding: 0.75rem 1rem;
    background: var(--warn-50);
    border: 1px solid #fde68a;
    border-radius: var(--radius-md);
    font-size: 0.8125rem;
    line-height: 1.5;
    color: var(--warn-600);
  }

  /* ── Tool Submissions ──────────────────────────────────────────── */

  .tool-submissions-section {
    margin-top: 2rem;
    border-top: 1px solid var(--line);
    padding-top: 1.5rem;
  }

  .section-heading {
    font-size: 1rem;
    font-weight: 600;
    margin: 0 0 0.25rem;
    color: var(--ink-900);
  }

  .section-desc {
    color: var(--ink-500);
    font-size: 0.8125rem;
    margin: 0 0 1rem;
    line-height: 1.5;
  }

  .tool-layout {
    display: flex;
    gap: 1.25rem;
    min-height: 200px;
  }

  .tool-list {
    width: 280px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: 0px;
    overflow: auto;
  }

  .tool-card {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    padding: 0.75rem 1rem;
    background: var(--surface);
    border: 1px solid var(--line);
    cursor: pointer;
    text-align: left;
    color: var(--ink-900);
    font-size: 0.8125rem;
    font-family: inherit;
    transition: all var(--dur-fast) var(--ease-out);
    border-left: 3px solid var(--memory-500);
  }
  .tool-card:hover { border-color: var(--line-strong); background: var(--memory-50); border-left-color: var(--memory-500); }
  .tool-card.selected { border-color: var(--memory-500); background: var(--memory-50); border-left-color: var(--memory-500); }

  .tool-card-top {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .tool-surface-badge {
    padding: 0.125rem 0.5rem;
    border-radius: var(--radius-sm);
    font-size: 0.6875rem;
    font-weight: 500;
    border: 1px solid var(--memory-200);
    background: var(--memory-50);
    color: var(--memory-600);
    text-transform: uppercase;
    letter-spacing: 0.03em;
    font-family: var(--font-mono);
  }

  .tool-kind-label {
    font-size: 0.6875rem;
    color: var(--ink-400);
    text-transform: uppercase;
    letter-spacing: 0.03em;
    font-family: var(--font-mono);
  }

  .tool-card-tool { font-weight: 600; }
  .tool-card-operator { color: var(--ink-500); font-size: 0.75rem; }
  .tool-card-time { color: var(--ink-400); font-size: 0.6875rem; }

  .tool-detail-pane {
    flex: 1;
    background: var(--surface);
    border: 1px solid var(--line);
    padding: 1.25rem;
    overflow: auto;
    border-left: 3px solid var(--memory-500);
  }

  .tool-detail-header {
    margin-bottom: 1rem;
  }

  .tool-detail-title {
    margin: 0 0 0.5rem;
    font-size: 1rem;
    font-weight: 600;
    color: var(--ink-900);
    text-transform: none;
    letter-spacing: normal;
  }

  .tool-detail-badges {
    display: flex;
    gap: 0.5rem;
    align-items: center;
  }

  .tool-meta {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0.25rem 0.75rem;
    font-size: 0.8125rem;
    color: var(--ink-500);
    margin-bottom: 1.25rem;
  }
  .tool-meta dt { font-weight: 500; color: var(--ink-400); }
  .tool-meta dd { margin: 0; }
  .tool-meta code { color: var(--memory-500); font-size: 0.75rem; font-family: var(--font-mono); }

  .meta-secondary { color: var(--ink-400); font-size: 0.75rem; }

  .tool-evidence h4 {
    margin: 0 0 0.5rem;
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--ink-400);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    font-family: var(--font-mono);
  }

  .tool-evidence-card {
    padding: 0.75rem 1rem;
    background: var(--memory-50);
    border: 1px solid var(--memory-100);
    border-radius: var(--radius-md);
  }

  .tool-call-id {
    display: block;
    font-size: 0.6875rem;
    color: var(--memory-500);
    font-family: var(--font-mono);
    margin-bottom: 0.5rem;
  }

  .tool-evidence-card blockquote {
    margin: 0;
    padding: 0.5rem 0.75rem;
    border-left: 2px solid var(--memory-500);
    color: var(--ink-700);
    font-size: 0.8125rem;
    font-style: italic;
    background: var(--memory-50);
    border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
  }

  /* ── Projection notice (ingest response state) ────────────── */

  .projection-notice {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    padding: 0.625rem 0.75rem;
    border-radius: var(--radius-sm);
    margin-bottom: 0.75rem;
    font-size: 0.8125rem;
  }

  .projection-notice-projection_deferred {
    background: var(--warn-50);
    border: 1px solid #fde68a;
  }

  .projection-notice-provider_not_configured {
    background: var(--canvas-muted);
    border: 1px solid var(--line);
  }

  .projection-notice-provider_unavailable {
    background: var(--danger-50);
    border: 1px solid #fecaca;
  }

  .projection-notice-no_candidate {
    background: var(--canvas-muted);
    border: 1px solid var(--line);
  }

  .projection-notice-label {
    font-weight: 600;
    font-size: 0.75rem;
    color: var(--ink-700);
    text-transform: uppercase;
    letter-spacing: 0.02em;
  }

  .projection-notice-message {
    color: var(--ink-700);
  }

  .projection-notice-hint {
    font-size: 0.75rem;
    color: var(--ink-500);
    margin-top: 0.125rem;
  }
</style>
