<script lang="ts">
  type Variant = "primary" | "secondary" | "ghost" | "danger" | "dark";
  type Size = "sm" | "md";

  type Props = {
    variant?: Variant;
    size?: Size;
    type?: "button" | "submit" | "reset";
    disabled?: boolean;
    loading?: boolean;
    fullWidth?: boolean;
    onclick?: (e: MouseEvent) => void;
    "data-testid"?: string;
    "aria-label"?: string;
    "aria-busy"?: boolean | "true" | "false";
    children?: import("svelte").Snippet;
  };

  let {
    variant = "primary",
    size = "md",
    type = "button",
    disabled = false,
    loading = false,
    fullWidth = false,
    onclick,
    "data-testid": testId,
    "aria-label": ariaLabel,
    "aria-busy": ariaBusy,
    children,
  }: Props = $props();
</script>

<button
  {type}
  class="ui-btn"
  class:primary={variant === "primary"}
  class:secondary={variant === "secondary"}
  class:ghost={variant === "ghost"}
  class:danger={variant === "danger"}
  class:dark={variant === "dark"}
  class:sm={size === "sm"}
  class:full={fullWidth}
  disabled={disabled || loading}
  data-testid={testId}
  aria-label={ariaLabel}
  aria-busy={ariaBusy ?? (loading ? "true" : undefined)}
  {onclick}
>
  {#if loading}<span class="spin" aria-hidden="true"></span>{/if}
  {#if children}{@render children()}{/if}
</button>

<style>
  .ui-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--sp-2);
    border-radius: var(--radius-md);
    font-family: var(--font-sans);
    font-size: var(--text-sm);
    font-weight: 600;
    line-height: 1.2;
    padding: 0.5rem 0.875rem;
    border: 1px solid transparent;
    cursor: pointer;
    transition: background 120ms var(--ease-out), border-color 120ms var(--ease-out),
      opacity 120ms var(--ease-out);
  }
  .ui-btn.sm {
    padding: 0.35rem 0.65rem;
    font-size: var(--text-xs);
  }
  .ui-btn.full {
    width: 100%;
  }
  .ui-btn:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .primary {
    background: var(--accent-500);
    color: #fff;
  }
  .primary:hover:not(:disabled) {
    background: var(--accent-600);
  }
  .secondary {
    background: var(--surface);
    border-color: var(--line);
    color: var(--ink-900);
  }
  .secondary:hover:not(:disabled) {
    border-color: var(--line-strong);
    background: var(--canvas-muted);
  }
  .ghost {
    background: transparent;
    color: var(--ink-700);
  }
  .ghost:hover:not(:disabled) {
    background: var(--canvas-muted);
  }
  .danger {
    background: var(--danger-50);
    border-color: var(--danger-500);
    color: var(--danger-600);
  }
  .dark {
    background: var(--ink-900);
    color: #fff;
  }
  .dark:hover:not(:disabled) {
    background: var(--ink-700);
  }
  .spin {
    width: 0.75rem;
    height: 0.75rem;
    border: 2px solid currentColor;
    border-right-color: transparent;
    border-radius: 50%;
    animation: spin 0.6s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
