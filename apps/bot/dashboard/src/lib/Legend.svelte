<script lang="ts">
  /** Global legend for signoff state badges. */
</script>

<div class="legend">
  <span class="legend-group">
    <span class="legend-label">Signoff</span>
    <span class="badge proposed">proposed</span>
    <span class="badge approved">approved</span>
    <span class="badge rejected">rejected</span>
    <span class="badge collision">collision pending</span>
    <span class="badge superseded">superseded</span>
  </span>
</div>

<style>
  .legend {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    font-size: 0.68rem;
    margin-left: auto;
    flex-wrap: wrap;
  }

  .legend-group {
    display: flex;
    align-items: center;
    gap: 0.3rem;
  }

  .legend-label {
    color: var(--ink-400);
    font-weight: 500;
    margin-right: 0.15rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-size: 0.62rem;
  }

  .badge {
    padding: 0.12rem 0.4rem;
    border-radius: 4px;
    font-weight: 500;
    white-space: nowrap;
    border: 1px solid;
  }

  .proposed { background: var(--warn-50); color: var(--warn-600); border-color: #fcd34d; }
  .approved { background: var(--ok-50); color: var(--ok-600); border-color: #6ee7b7; }
  .rejected { background: var(--danger-50); color: var(--danger-600); border-color: #fca5a5; }
  .collision { background: var(--memory-50); color: var(--memory-700); border-color: var(--memory-300); }
  .superseded { background: var(--canvas-muted); color: var(--ink-500); border-color: var(--line-strong); }

</style>
