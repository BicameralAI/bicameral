<script lang="ts">
  type Props = {
    label: string;
    mono?: boolean;
    children?: import("svelte").Snippet;
  };
  let { label, mono = false, children }: Props = $props();
</script>

<div class="kv">
  <span class="lab">{label}</span>
  <span class="val" class:mono>
    {#if children}{@render children()}{/if}
  </span>
</div>

<style>
  .kv {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: var(--sp-4);
    padding: var(--sp-2) 0;
    border-bottom: 1px solid var(--line);
    font-size: var(--text-sm);
  }
  .kv:last-child {
    border-bottom: 0;
  }
  .lab {
    color: var(--ink-500);
    flex-shrink: 0;
  }
  .val {
    color: var(--ink-900);
    text-align: right;
  }
  .mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
  }
</style>
