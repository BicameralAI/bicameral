<script lang="ts">
  type Props = {
    children?: import("svelte").Snippet;
    "data-testid"?: string;
  };
  let { children, "data-testid": testId }: Props = $props();
</script>

<h2 class="group-heading" data-testid={testId}>
  {#if children}{@render children()}{/if}
</h2>

<style>
  .group-heading {
    margin: var(--sp-5) 0 var(--sp-2);
    font-size: var(--text-xs);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--ink-500);
  }
</style>
