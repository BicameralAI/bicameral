<script lang="ts">
  /** Minimal geometric icons — no external icon pack. */
  type Name =
    | "home"
    | "activity"
    | "decisions"
    | "channels"
    | "settings"
    | "products"
    | "search"
    | "user"
    | "link"
    | "plus"
    | "check"
    | "x";

  type Props = { name: Name; size?: number };
  let { name, size = 16 }: Props = $props();
</script>

<span class="ico" style:width="{size}px" style:height="{size}px" aria-hidden="true">
  {#if name === "home"}⌂
  {:else if name === "activity"}↯
  {:else if name === "decisions"}◇
  {:else if name === "channels"}#
  {:else if name === "settings"}⚙
  {:else if name === "products"}▦
  {:else if name === "search"}⌕
  {:else if name === "user"}◉
  {:else if name === "link"}☍
  {:else if name === "plus"}+
  {:else if name === "check"}✓
  {:else if name === "x"}✕
  {/if}
</span>

<style>
  .ico {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 0.875em;
    line-height: 1;
    color: inherit;
  }
</style>
