<script lang="ts">
  type Tab = { id: string; label: string };
  type Props = {
    tabs: Tab[];
    value?: string;
    onchange?: (id: string) => void;
  };
  let { tabs, value = $bindable(tabs[0]?.id ?? ""), onchange }: Props = $props();
</script>

<div class="tabs" role="tablist">
  {#each tabs as tab (tab.id)}
    <button
      type="button"
      role="tab"
      class:active={value === tab.id}
      aria-selected={value === tab.id}
      onclick={() => {
        value = tab.id;
        onchange?.(tab.id);
      }}
    >
      {tab.label}
    </button>
  {/each}
</div>

<style>
  .tabs {
    display: flex;
    gap: 2px;
    border-bottom: 1px solid var(--line);
    margin-bottom: var(--sp-4);
  }
  button {
    border: 0;
    background: none;
    padding: var(--sp-2) var(--sp-3);
    font-size: var(--text-sm);
    color: var(--ink-500);
    cursor: pointer;
    border-bottom: 2px solid transparent;
    margin-bottom: -1px;
  }
  button.active {
    color: var(--ink-900);
    font-weight: 600;
    border-bottom-color: var(--accent-500);
  }
</style>
