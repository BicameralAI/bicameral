<script lang="ts">
  type Props = {
    topbar?: import("svelte").Snippet;
    sidebar?: import("svelte").Snippet;
    children?: import("svelte").Snippet;
    "data-testid"?: string;
  };
  let { topbar, sidebar, children, "data-testid": testId }: Props = $props();
</script>

<div class="shell" data-testid={testId}>
  {#if topbar}<div class="top">{@render topbar()}</div>{/if}
  <div class="body">
    {#if sidebar}{@render sidebar()}{/if}
    <main class="main">
      {#if children}{@render children()}{/if}
    </main>
  </div>
</div>

<style>
  .shell {
    min-height: 100vh;
    display: flex;
    flex-direction: column;
    background: var(--canvas);
    color: var(--ink-900);
    font-family: var(--font-sans);
  }
  .body {
    display: flex;
    flex: 1;
    min-height: 0;
  }
  .main {
    flex: 1;
    min-width: 0;
    padding: var(--sp-6) var(--sp-6);
    overflow: auto;
  }
</style>
