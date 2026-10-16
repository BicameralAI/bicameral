<script lang="ts">
  type Props = {
    caption?: string;
    columns: string[];
    children?: import("svelte").Snippet;
    "data-testid"?: string;
  };
  let { caption, columns, children, "data-testid": testId }: Props = $props();
</script>

<div class="frame" data-testid={testId}>
  <table class="table">
    {#if caption}<caption class="sr-only">{caption}</caption>{/if}
    <thead>
      <tr>
        {#each columns as col}
          <th>{col}</th>
        {/each}
      </tr>
    </thead>
    <tbody>
      {#if children}{@render children()}{/if}
    </tbody>
  </table>
</div>

<style>
  .frame {
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    overflow: hidden;
    background: var(--surface);
  }
  .table {
    width: 100%;
    border-collapse: collapse;
  }
  th {
    text-align: left;
    padding: 0.5rem 0.75rem;
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--ink-500);
    background: var(--canvas-muted);
    border-bottom: 1px solid var(--line);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    border: 0;
  }
</style>
