<script lang="ts">
  type Props = {
    padding?: "sm" | "md" | "lg";
    children?: import("svelte").Snippet;
    "data-testid"?: string;
  };
  let { padding = "md", children, "data-testid": testId }: Props = $props();
</script>

<div class="ui-card p-{padding}" data-testid={testId}>
  {#if children}{@render children()}{/if}
</div>

<style>
  .ui-card {
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
  }
  .p-sm {
    padding: var(--sp-3);
  }
  .p-md {
    padding: var(--sp-5);
  }
  .p-lg {
    padding: var(--sp-6);
  }
</style>
