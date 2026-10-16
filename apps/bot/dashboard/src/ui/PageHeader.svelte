<script lang="ts">
  type Props = {
    eyebrow?: string;
    title: string;
    description?: string;
    children?: import("svelte").Snippet;
  };
  let { eyebrow, title, description, children }: Props = $props();
</script>

<!-- Prefer shell topbar for primary title. Eyebrow/description are opt-in only. -->
<header class="page-header">
  <div class="text">
    {#if eyebrow}<p class="eyebrow">{eyebrow}</p>{/if}
    <h1 class="title">{title}</h1>
    {#if description}<p class="desc">{description}</p>{/if}
  </div>
  {#if children}<div class="actions">{@render children()}</div>{/if}
</header>

<style>
  .page-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--sp-3);
    margin-bottom: var(--sp-4);
  }
  .eyebrow {
    margin: 0 0 2px;
    font-family: var(--font-mono);
    font-size: 0.625rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--ink-400);
  }
  .title {
    margin: 0;
    font-size: var(--text-md);
    font-weight: 600;
    color: var(--ink-900);
    letter-spacing: -0.01em;
  }
  .desc {
    margin: var(--sp-1) 0 0;
    font-size: var(--text-sm);
    color: var(--ink-500);
    max-width: 36rem;
  }
  .actions {
    display: flex;
    gap: var(--sp-2);
    flex-shrink: 0;
  }
</style>
