<script lang="ts">
  type Props = { label: string; children?: import("svelte").Snippet };
  let { label, children }: Props = $props();
</script>

<div class="section">
  <p class="label">{label}</p>
  {#if children}<div class="items">{@render children()}</div>{/if}
</div>

<style>
  .section {
    margin-bottom: var(--sp-5);
  }
  .label {
    margin: 0 0 var(--sp-2);
    padding: 0 var(--sp-2);
    font-size: var(--text-xs);
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--ink-500);
  }
  .items {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
</style>
