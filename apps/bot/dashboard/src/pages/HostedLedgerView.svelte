<script lang="ts">
  import { fetchHostedLedgerProjection } from "../lib/api";
  import type {
    HostedLedgerProjection,
    RenderableLedgerEntry,
    RenderableLedgerEvent,
  } from "../lib/hosted-bridge-types";

  type Props = {
    sessionToken?: string;
  };

  let { sessionToken }: Props = $props();

  let projection = $state<HostedLedgerProjection | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);

  async function load() {
    if (!sessionToken) {
      projection = null;
      error = "Pairing token unavailable. Re-pair the hosted dashboard.";
      loading = false;
      return;
    }

    loading = true;
    error = null;
    try {
      projection = await fetchHostedLedgerProjection(sessionToken);
    } catch (e: unknown) {
      projection = null;
      error = e instanceof Error ? e.message : "Could not load hosted ledger.";
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    load();
  });

  function stateLabel(value: string): string {
    return value.replace(/_/g, " ");
  }

  function formatDate(iso?: string): string {
    if (!iso) return "-";
    try {
      return new Date(iso).toLocaleString();
    } catch {
      return iso;
    }
  }

  function rowClass(entry: RenderableLedgerEntry): string {
    if (entry.signoff_state === "approved") return "approved";
    if (entry.source_freshness === "stale") return "stale";
    if (entry.review_state.includes("review")) return "review";
    return "neutral";
  }

  function eventLabel(event: RenderableLedgerEvent): string {
    return stateLabel(event.event_type);
  }
</script>

<section class="hosted-ledger" data-testid="hosted-ledger-view">
  <header class="page-header">
    <h1>Decisions</h1>
    <p class="subtitle">Read-only hosted projection from the paired local daemon.</p>
  </header>

  {#if loading}
    <div class="empty">Loading hosted ledger...</div>
  {:else if error}
    <div class="error">{error}</div>
  {:else if !projection || projection.decisions.length === 0}
    <div class="empty">
      <div class="empty-title">Ledger is empty</div>
      <div class="empty-body">No renderable Decisions were returned by the paired daemon.</div>
    </div>
  {:else}
    <div class="summary-grid" data-testid="hosted-ledger-summary">
      <div><span class="metric">{projection.summary.decision_count}</span><span class="label">Decisions</span></div>
      <div><span class="metric">{projection.summary.needs_signoff_count}</span><span class="label">Needs signoff</span></div>
      <div><span class="metric">{projection.summary.stale_source_count}</span><span class="label">Stale sources</span></div>
      <div><span class="metric">{projection.summary.bound_count}</span><span class="label">Bound</span></div>
    </div>

    <div class="ledger-table" data-testid="hosted-ledger-decisions">
      <div class="table-header">
        <span>Decision</span>
        <span>Review</span>
        <span>Evidence</span>
        <span>Updated</span>
      </div>
      {#each projection.decisions as entry (entry.decision_id)}
        <article class="ledger-row {rowClass(entry)}">
          <div class="decision-cell">
            <span class="title">{entry.title}</span>
            <span class="meta">{entry.level} - {entry.decision_id}</span>
          </div>
          <div class="state-cell">
            <span class="badge">{stateLabel(entry.signoff_state)}</span>
            <span class="substate">{stateLabel(entry.review_state)}</span>
          </div>
          <div class="count-cell">
            <span>{entry.source_evidence_count} source</span>
            <span>{entry.binding_count} binding</span>
          </div>
          <div class="date-cell">{formatDate(entry.promoted_at)}</div>
        </article>
      {/each}
    </div>

    {#if projection.events.length > 0}
      <section class="event-section">
        <h2>Recent ledger events</h2>
        <div class="event-list" data-testid="hosted-ledger-events">
          {#each projection.events as event (event.id)}
            <div class="event-row">
              <span>{eventLabel(event)}</span>
              <time datetime={event.timestamp}>{formatDate(event.timestamp)}</time>
            </div>
          {/each}
        </div>
      </section>
    {/if}
  {/if}
</section>

<style>
  .hosted-ledger { max-width: 1100px; }
  .page-header { margin-bottom: 1rem; }
  h1 { font-size: 1.25rem; font-weight: 700; color: #18181b; }
  h2 { font-size: 0.875rem; font-weight: 600; color: #3f3f46; margin: 1rem 0 0.5rem; }
  .subtitle, .empty-body { font-size: 0.8125rem; color: #71717a; margin-top: 0.25rem; }
  .empty, .error { padding: 1rem; border: 1px solid #e5e5e5; background: #fff; font-size: 0.8125rem; color: #71717a; }
  .error { border-color: #fecaca; color: #991b1b; background: #fef2f2; }
  .empty-title { font-weight: 600; color: #18181b; }
  .summary-grid { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); border: 1px solid #e5e5e5; background: #fff; margin-bottom: 1rem; }
  .summary-grid > div { padding: 0.875rem 1rem; border-right: 1px solid #e5e5e5; }
  .summary-grid > div:last-child { border-right: none; }
  .metric { display: block; font-size: 1.25rem; font-weight: 700; color: #18181b; }
  .label, .meta, .substate, .count-cell, .date-cell, .event-row time { font-size: 0.75rem; color: #71717a; }
  .ledger-table { border: 1px solid #e5e5e5; background: #fff; }
  .table-header, .ledger-row { display: grid; grid-template-columns: minmax(260px, 1.7fr) minmax(150px, 0.8fr) minmax(120px, 0.7fr) minmax(160px, 0.8fr); gap: 1rem; align-items: center; }
  .table-header { padding: 0.625rem 1rem; border-bottom: 1px solid #e5e5e5; font-size: 0.75rem; font-weight: 600; color: #71717a; text-transform: uppercase; }
  .ledger-row { padding: 0.875rem 1rem; border-bottom: 1px solid #f4f4f5; border-left: 3px solid transparent; }
  .ledger-row:last-child { border-bottom: none; }
  .ledger-row.approved { border-left-color: #059669; }
  .ledger-row.review { border-left-color: #7c3aed; }
  .ledger-row.stale { border-left-color: #d97706; }
  .decision-cell, .state-cell, .count-cell { display: flex; flex-direction: column; gap: 0.25rem; min-width: 0; }
  .title { font-size: 0.875rem; font-weight: 600; color: #18181b; overflow-wrap: anywhere; }
  .badge { width: fit-content; padding: 0.175rem 0.5rem; border: 1px solid #a5f3fc; background: #ecfeff; color: #0e7490; font-size: 0.6875rem; font-weight: 600; }
  .event-list { border: 1px solid #e5e5e5; background: #fff; }
  .event-row { display: flex; justify-content: space-between; gap: 1rem; padding: 0.625rem 1rem; border-bottom: 1px solid #f4f4f5; font-size: 0.8125rem; }
  .event-row:last-child { border-bottom: none; }
  @media (max-width: 780px) {
    .summary-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .summary-grid > div { border-bottom: 1px solid #e5e5e5; }
    .table-header { display: none; }
    .ledger-row { grid-template-columns: 1fr; gap: 0.625rem; }
  }
</style>
