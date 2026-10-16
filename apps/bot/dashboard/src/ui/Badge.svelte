<script lang="ts">
  type Props = {
    tone?: "teal" | "neutral" | "purple";
    children?: import("svelte").Snippet;
  };
  let { tone = "teal", children }: Props = $props();
</script>

<span class="badge tone-{tone}">
  {#if children}{@render children()}{/if}
</span>

<style>
  .badge {
    display: inline-flex;
    align-items: center;
    padding: 0.1rem 0.4rem;
    border-radius: var(--radius-sm);
    font-size: var(--text-xs);
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .tone-teal {
    background: var(--accent-50);
    color: var(--accent-700);
  }
  .tone-purple {
    background: var(--memory-50);
    color: var(--memory-700);
  }
  .tone-neutral {
    background: var(--canvas-muted);
    color: var(--ink-500);
  }
</style>
