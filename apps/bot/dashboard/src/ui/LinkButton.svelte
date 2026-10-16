<script lang="ts">
  type Props = {
    onclick?: () => void;
    disabled?: boolean;
    children?: import("svelte").Snippet;
    "data-testid"?: string;
  };
  let { onclick, disabled = false, children, "data-testid": testId }: Props =
    $props();
</script>

<button type="button" class="link-btn" {disabled} data-testid={testId} {onclick}>
  {#if children}{@render children()}{/if}
</button>

<style>
  .link-btn {
    border: 0;
    background: none;
    padding: 0;
    font-family: var(--font-sans);
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--accent-600);
    cursor: pointer;
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .link-btn:hover:not(:disabled) {
    color: var(--accent-700);
  }
  .link-btn:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
</style>
