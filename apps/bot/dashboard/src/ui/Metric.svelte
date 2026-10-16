<script lang="ts">
  type Props = {
    label: string;
    value: string;
    hint?: string;
  };
  let { label, value, hint }: Props = $props();
</script>

<div class="metric">
  <p class="lab">{label}</p>
  <p class="val mono">{value}</p>
  {#if hint}<p class="hint">{hint}</p>{/if}
</div>

<style>
  .metric {
    padding: var(--sp-3) var(--sp-4);
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background: var(--surface);
  }
  .lab {
    margin: 0 0 var(--sp-1);
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--ink-500);
  }
  .val {
    margin: 0;
    font-size: var(--text-xl);
    font-weight: 600;
    color: var(--ink-900);
  }
  .mono {
    font-family: var(--font-mono);
  }
  .hint {
    margin: var(--sp-1) 0 0;
    font-size: var(--text-xs);
    color: var(--ink-400);
  }
</style>
