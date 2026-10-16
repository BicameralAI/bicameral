<script lang="ts">
  type Tone = "warn" | "danger" | "info" | "ok";
  type Props = {
    tone?: Tone;
    title?: string;
    actionLabel?: string;
    onaction?: () => void;
    children?: import("svelte").Snippet;
    "data-testid"?: string;
  };
  let {
    tone = "warn",
    title,
    actionLabel,
    onaction,
    children,
    "data-testid": testId,
  }: Props = $props();
</script>

<div class="callout tone-{tone}" data-testid={testId} role="status">
  <button
    type="button"
    class="bang"
    class:clickable={!!onaction}
    onclick={() => onaction?.()}
    aria-label={actionLabel ?? title ?? "Attention"}
    disabled={!onaction}
  >
    !
  </button>
  <div class="body">
    {#if title}<p class="title">{title}</p>{/if}
    {#if children}<div class="msg">{@render children()}</div>{/if}
    {#if actionLabel && onaction}
      <button type="button" class="link" onclick={onaction}>{actionLabel}</button>
    {/if}
  </div>
</div>

<style>
  .callout {
    display: flex;
    gap: var(--sp-3);
    align-items: flex-start;
    padding: var(--sp-3);
    border-radius: var(--radius-md);
    border: 1px solid var(--line);
  }
  .tone-warn {
    background: var(--warn-50);
    border-color: color-mix(in srgb, var(--warn-500) 35%, var(--line));
  }
  .tone-danger {
    background: var(--danger-50);
  }
  .tone-info {
    background: var(--accent-50);
  }
  .tone-ok {
    background: var(--ok-50);
  }
  .bang {
    flex-shrink: 0;
    width: 1.5rem;
    height: 1.5rem;
    border-radius: var(--radius-pill);
    border: 1px solid var(--warn-600);
    background: var(--surface);
    color: var(--warn-600);
    font-weight: 700;
    font-size: var(--text-sm);
    line-height: 1;
  }
  .bang.clickable {
    cursor: pointer;
  }
  .bang.clickable:hover {
    background: var(--warn-500);
    color: #fff;
  }
  .bang:disabled {
    cursor: default;
  }
  .title {
    margin: 0 0 var(--sp-1);
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--ink-900);
  }
  .msg {
    font-size: var(--text-sm);
    color: var(--ink-700);
  }
  .link {
    margin-top: var(--sp-2);
    border: 0;
    background: none;
    padding: 0;
    color: var(--accent-600);
    font-size: var(--text-sm);
    font-weight: 600;
    cursor: pointer;
    text-decoration: underline;
  }
</style>
