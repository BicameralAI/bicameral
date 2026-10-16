<script lang="ts">
  import Dot from "./Dot.svelte";

  type Props = {
    connected?: boolean;
    label?: string;
    syncLabel?: string;
  };
  let {
    connected = true,
    label = "Connected",
    syncLabel = "Sync current",
  }: Props = $props();
</script>

<div class="session" data-testid="session-bar">
  <p class="section-lab">Connection</p>
  <p class="line">
    <Dot tone={connected ? "green" : "red"} />
    <span>{connected ? label : "Disconnected"}</span>
  </p>
  {#if connected && syncLabel}
    <p class="sync">{syncLabel}</p>
  {/if}
</div>

<style>
  .session {
    margin-top: auto;
    padding: var(--sp-3) var(--sp-2) 0;
    border-top: 1px solid var(--line);
  }
  .section-lab {
    margin: 0 0 var(--sp-2);
    font-size: var(--text-xs);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--ink-400);
  }
  .line {
    margin: 0;
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    font-size: var(--text-sm);
    color: var(--ink-700);
  }
  .sync {
    margin: var(--sp-1) 0 0;
    padding-left: calc(7px + var(--sp-2));
    font-size: var(--text-xs);
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--ok-600);
  }
</style>
