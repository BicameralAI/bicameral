<script lang="ts">
  type Props = {
    tone?: "warn" | "danger" | "info";
    children?: import("svelte").Snippet;
    "data-testid"?: string;
  };
  let { tone = "warn", children, "data-testid": testId }: Props = $props();
</script>

<p class="alert tone-{tone}" role="alert" data-testid={testId}>
  {#if children}{@render children()}{/if}
</p>

<style>
  .alert {
    margin: 0;
    font-size: var(--text-xs);
    line-height: 1.35;
    max-width: 18rem;
  }
  .tone-warn {
    color: var(--warn-600);
  }
  .tone-danger {
    color: var(--danger-600);
  }
  .tone-info {
    color: var(--ink-700);
  }
</style>
