<script lang="ts">
  type Props = { children?: import("svelte").Snippet };
  let { children }: Props = $props();
</script>

<code class="code">{#if children}{@render children()}{/if}</code>

<style>
  .code {
    font-family: var(--font-mono);
    font-size: 0.92em;
    background: var(--canvas-muted);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    padding: 0.05rem 0.3rem;
  }
</style>
