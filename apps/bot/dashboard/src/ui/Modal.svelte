<script lang="ts">
  type Props = {
    title: string;
    open?: boolean;
    onclose: () => void;
    children?: import("svelte").Snippet;
    "data-testid"?: string;
  };
  let { title, open = true, onclose, children, "data-testid": testId }: Props =
    $props();

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }
  function backdrop(e: MouseEvent) {
    if (e.target === e.currentTarget) onclose();
  }
</script>

<svelte:window onkeydown={onKey} />

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="backdrop" onclick={backdrop} data-testid={testId}>
    <div class="panel" role="dialog" aria-label={title}>
      <div class="header">
        <h2>{title}</h2>
        <button type="button" class="x" onclick={onclose} aria-label="Close">✕</button>
      </div>
      <div class="body">
        {#if children}{@render children()}{/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(24, 24, 27, 0.35);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
    padding: var(--sp-4);
  }
  .panel {
    width: min(520px, 100%);
    max-height: min(80vh, 720px);
    overflow: auto;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-2);
  }
  .header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--sp-4) var(--sp-5);
    border-bottom: 1px solid var(--line);
  }
  h2 {
    margin: 0;
    font-size: var(--text-md);
    font-weight: 600;
  }
  .x {
    border: 0;
    background: none;
    cursor: pointer;
    color: var(--ink-500);
    font-size: 1rem;
  }
  .body {
    padding: var(--sp-5);
  }
</style>
