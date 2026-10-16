<script lang="ts">
  type Props = {
    children?: import("svelte").Snippet;
    footer?: import("svelte").Snippet;
    "data-testid"?: string;
  };
  let { children, footer, "data-testid": testId }: Props = $props();
</script>

<aside class="sidebar" data-testid={testId} aria-label="Workspace">
  <div class="body">
    {#if children}{@render children()}{/if}
  </div>
  {#if footer}
    <div class="footer">{@render footer()}</div>
  {/if}
</aside>

<style>
  .sidebar {
    width: var(--shell-sidebar-w, 13rem);
    flex-shrink: 0;
    box-sizing: border-box;
    padding: 0.75rem 0.5rem;
    border-right: 1px solid var(--line);
    background: var(--surface);
    min-height: 100%;
    display: flex;
    flex-direction: column;
  }
  .body {
    flex: 1;
    min-height: 0;
  }
  .footer {
    margin-top: auto;
  }
</style>
