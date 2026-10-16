<script lang="ts">
  type Props = {
    title: string;
    description?: string;
    icon?: string;
    /** bordered card frame (Figma empty products) */
    framed?: boolean;
    children?: import("svelte").Snippet;
    "data-testid"?: string;
  };
  let {
    title,
    description,
    icon = "○",
    framed = false,
    children,
    "data-testid": testId,
  }: Props = $props();
</script>

<div class="empty" class:framed data-testid={testId}>
  <div class="icon" aria-hidden="true">{icon}</div>
  <h2 class="title">{title}</h2>
  {#if description}<p class="desc">{description}</p>{/if}
  {#if children}<div class="actions">{@render children()}</div>{/if}
</div>

<style>
  .empty {
    text-align: center;
    padding: var(--sp-8) var(--sp-4);
    max-width: 36rem;
    margin: 0 auto;
  }
  .framed {
    max-width: none;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--surface);
    padding: var(--sp-8) var(--sp-6);
  }
  .icon {
    font-size: 1.75rem;
    color: var(--ink-400);
    margin-bottom: var(--sp-3);
  }
  .title {
    margin: 0 0 var(--sp-2);
    font-size: var(--text-md);
    font-weight: 600;
  }
  .desc {
    margin: 0 0 var(--sp-4);
    font-size: var(--text-base);
    color: var(--ink-500);
    max-width: 32rem;
    margin-left: auto;
    margin-right: auto;
  }
  .actions {
    display: flex;
    gap: var(--sp-2);
    justify-content: center;
    flex-wrap: wrap;
  }
</style>
