<script lang="ts">
  type Props = {
    label: string;
    forId?: string;
    hint?: string;
    error?: string;
    children?: import("svelte").Snippet;
  };

  let { label, forId, hint, error, children }: Props = $props();
</script>

<div class="field">
  <label class="label" for={forId}>{label}</label>
  {#if children}{@render children()}{/if}
  {#if error}
    <p class="error" role="alert">{error}</p>
  {:else if hint}
    <p class="hint">{hint}</p>
  {/if}
</div>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }
  .label {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--ink-700);
  }
  .hint {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--ink-500);
  }
  .error {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--danger-600);
  }
</style>
