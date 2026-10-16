<script lang="ts">
  type Props = {
    label: string;
    icon?: string;
    active?: boolean;
    disabled?: boolean;
    badge?: string;
    href?: string;
    onclick?: () => void;
    "data-testid"?: string;
  };
  let {
    label,
    icon,
    active = false,
    disabled = false,
    badge,
    onclick,
    "data-testid": testId,
  }: Props = $props();
</script>

<button
  type="button"
  class="nav-item"
  class:active
  {disabled}
  data-testid={testId}
  aria-current={active ? "page" : undefined}
  {onclick}
>
  {#if icon}<span class="icon" aria-hidden="true">{icon}</span>{/if}
  <span class="label">{label}</span>
  {#if badge}<span class="badge">{badge}</span>{/if}
</button>

<style>
  .nav-item {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    width: 100%;
    text-align: left;
    border: 0;
    background: transparent;
    padding: 0.45rem 0.65rem;
    border-radius: var(--radius-sm);
    font-family: var(--font-sans);
    font-size: var(--text-sm);
    color: var(--ink-700);
    cursor: pointer;
    border-left: 2px solid transparent;
  }
  .nav-item:hover:not(:disabled) {
    background: var(--canvas-muted);
  }
  .nav-item.active {
    background: var(--accent-50);
    color: var(--accent-700);
    font-weight: 600;
    border-left-color: var(--accent-500);
  }
  .nav-item:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .icon {
    width: 1.1rem;
    text-align: center;
    opacity: 0.85;
  }
  .label {
    flex: 1;
  }
  .badge {
    font-size: var(--text-xs);
    color: var(--ink-500);
  }
</style>
