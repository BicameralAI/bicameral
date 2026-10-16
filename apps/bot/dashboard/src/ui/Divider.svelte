<script lang="ts">
  type Props = { label?: string };
  let { label }: Props = $props();
</script>

{#if label}
  <div class="divider labeled" role="separator">
    <span class="line"></span>
    <span class="lab">{label}</span>
    <span class="line"></span>
  </div>
{:else}
  <hr class="divider plain" />
{/if}

<style>
  .plain {
    border: 0;
    border-top: 1px solid var(--line);
    margin: var(--sp-4) 0;
  }
  .labeled {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    margin: var(--sp-4) 0;
  }
  .line {
    flex: 1;
    height: 1px;
    background: var(--line);
  }
  .lab {
    font-size: var(--text-xs);
    color: var(--ink-400);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
</style>
