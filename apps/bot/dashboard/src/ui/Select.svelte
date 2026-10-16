<script lang="ts">
  type Option = { value: string; label: string };

  type Props = {
    id?: string;
    value?: string;
    options: Option[];
    disabled?: boolean;
    "data-testid"?: string;
  };

  let {
    id,
    value = $bindable(""),
    options,
    disabled = false,
    "data-testid": testId,
  }: Props = $props();
</script>

<select {id} class="ui-select" {disabled} data-testid={testId} bind:value>
  {#each options as opt (opt.value)}
    <option value={opt.value}>{opt.label}</option>
  {/each}
</select>

<style>
  .ui-select {
    width: 100%;
    font-family: var(--font-sans);
    font-size: var(--text-sm);
    color: var(--ink-900);
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    padding: 0.5rem 0.75rem;
  }
</style>
