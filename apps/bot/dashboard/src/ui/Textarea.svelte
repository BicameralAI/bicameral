<script lang="ts">
  type Props = {
    id?: string;
    value?: string;
    placeholder?: string;
    disabled?: boolean;
    rows?: number;
    "data-testid"?: string;
  };

  let {
    id,
    value = $bindable(""),
    placeholder = "",
    disabled = false,
    rows = 3,
    "data-testid": testId,
  }: Props = $props();
</script>

<textarea
  {id}
  {rows}
  {placeholder}
  {disabled}
  class="ui-textarea"
  data-testid={testId}
  bind:value
></textarea>

<style>
  .ui-textarea {
    width: 100%;
    box-sizing: border-box;
    font-family: var(--font-sans);
    font-size: var(--text-sm);
    color: var(--ink-900);
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    padding: 0.5rem 0.75rem;
    resize: vertical;
  }
  .ui-textarea:focus {
    outline: 2px solid var(--accent-200);
    border-color: var(--accent-500);
  }
</style>
