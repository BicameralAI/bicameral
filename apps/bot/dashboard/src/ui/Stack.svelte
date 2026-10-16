<script lang="ts">
  type Props = {
    gap?: 1 | 2 | 3 | 4 | 5 | 6;
    children?: import("svelte").Snippet;
  };
  let { gap = 3, children }: Props = $props();
</script>

<div class="stack g-{gap}">
  {#if children}{@render children()}{/if}
</div>

<style>
  .stack {
    display: flex;
    flex-direction: column;
  }
  .g-1 {
    gap: var(--sp-1);
  }
  .g-2 {
    gap: var(--sp-2);
  }
  .g-3 {
    gap: var(--sp-3);
  }
  .g-4 {
    gap: var(--sp-4);
  }
  .g-5 {
    gap: var(--sp-5);
  }
  .g-6 {
    gap: var(--sp-6);
  }
</style>
