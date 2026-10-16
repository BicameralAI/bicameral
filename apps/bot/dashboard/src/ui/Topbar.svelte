<script lang="ts">
  import Badge from "./Badge.svelte";
  import Dot from "./Dot.svelte";
  import Avatar from "./Avatar.svelte";

  type Props = {
    /** Product context in brand trail (optional) */
    productName?: string;
    accountLabel?: string;
    connected?: boolean;
    hosted?: boolean;
    onuser?: () => void;
    children?: import("svelte").Snippet;
  };
  let {
    productName,
    accountLabel,
    connected = true,
    hosted = true,
    onuser,
    children,
  }: Props = $props();
</script>

<header class="topbar">
  <div class="brand">
    <span class="logo-mark" aria-hidden="true">
      <svg viewBox="0 0 24 24" width="16" height="16" fill="none">
        <path
          d="M7 5.5a7 7 0 0 1 10 0M17 18.5a7 7 0 0 1-10 0"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
        />
      </svg>
    </span>
    <span class="wordmark">bicameral</span>
    {#if hosted}<Badge tone="teal">Hosted</Badge>{/if}
    {#if productName}
      <span class="sep" aria-hidden="true">/</span>
      <span class="product">{productName}</span>
    {/if}
  </div>
  <div class="center">
    {#if children}{@render children()}{/if}
  </div>
  <div class="right">
    {#if connected}
      <span class="conn"><Dot tone="green" /> Connected</span>
    {:else}
      <span class="conn off"><Dot tone="red" /> Disconnected</span>
    {/if}
    {#if accountLabel}
      <Avatar label={accountLabel} onclick={() => onuser?.()} data-testid="topbar-user" />
    {/if}
  </div>
</header>

<style>
  .topbar {
    display: flex;
    align-items: center;
    gap: var(--sp-4);
    height: 56px;
    padding: 0 var(--sp-4);
    border-bottom: 1px solid var(--line);
    background: var(--surface);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    min-width: 0;
  }
  .logo-mark {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: var(--radius-sm);
    background: var(--ink-900);
    color: #fff;
  }
  .wordmark {
    font-size: var(--text-sm);
    font-weight: 700;
    letter-spacing: -0.01em;
    color: var(--ink-900);
    text-transform: lowercase;
  }
  .sep {
    color: var(--ink-400);
  }
  .product {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--ink-700);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .center {
    flex: 1;
  }
  .right {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
  }
  .conn {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--ok-600);
  }
  .conn.off {
    color: var(--danger-600);
  }
</style>
