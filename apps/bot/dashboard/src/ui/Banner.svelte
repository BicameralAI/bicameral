<script lang="ts">
  type Tone = "info" | "warn" | "danger" | "ok";
  type Props = {
    tone?: Tone;
    children?: import("svelte").Snippet;
  };
  let { tone = "info", children }: Props = $props();
</script>

<div class="banner tone-{tone}" role="status">
  {#if children}{@render children()}{/if}
</div>

<style>
  .banner {
    padding: var(--sp-2) var(--sp-3);
    border-radius: var(--radius-md);
    font-size: var(--text-sm);
    border: 1px solid var(--line);
  }
  .tone-info {
    background: var(--accent-50);
    color: var(--accent-700);
  }
  .tone-warn {
    background: var(--warn-50);
    color: var(--warn-600);
  }
  .tone-danger {
    background: var(--danger-50);
    color: var(--danger-600);
  }
  .tone-ok {
    background: var(--ok-50);
    color: var(--ok-600);
  }
</style>
