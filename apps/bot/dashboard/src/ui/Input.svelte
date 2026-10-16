<script lang="ts">
  type Props = {
    id?: string;
    value?: string;
    placeholder?: string;
    disabled?: boolean;
    type?: string;
    invalid?: boolean;
    "data-testid"?: string;
    oninput?: (e: Event) => void;
  };

  let {
    id,
    value = $bindable(""),
    placeholder = "",
    disabled = false,
    type = "text",
    invalid = false,
    "data-testid": testId,
    oninput,
  }: Props = $props();
</script>

<input
  {id}
  {type}
  {placeholder}
  {disabled}
  class="ui-input"
  class:invalid
  data-testid={testId}
  bind:value
  {oninput}
/>

<style>
  .ui-input {
    width: 100%;
    box-sizing: border-box;
    font-family: var(--font-sans);
    font-size: var(--text-sm);
    color: var(--ink-900);
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    padding: 0.5rem 0.75rem;
  }
  .ui-input:focus {
    outline: 2px solid var(--accent-200);
    border-color: var(--accent-500);
  }
  .ui-input:disabled {
    opacity: 0.55;
    background: var(--canvas-muted);
  }
  .ui-input.invalid {
    border-color: var(--danger-500);
  }
</style>
