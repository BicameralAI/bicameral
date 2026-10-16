<script lang="ts">
  type Tone = "teal" | "purple" | "green" | "amber" | "red" | "neutral";

  type Props = {
    tone?: Tone;
    /** filled (default) | outline — Figma status chips use outline */
    variant?: "filled" | "outline";
    children?: import("svelte").Snippet;
    "data-testid"?: string;
  };

  let {
    tone = "neutral",
    variant = "filled",
    children,
    "data-testid": testId,
  }: Props = $props();
</script>

<span
  class="ui-pill pill-{tone}"
  class:outline={variant === "outline"}
  data-testid={testId}
>
  {#if children}{@render children()}{/if}
</span>

<style>
  .ui-pill {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
    padding: 0.2rem 0.55rem;
    border-radius: var(--radius-sm);
    font-size: var(--text-xs);
    font-weight: 600;
    line-height: 1.3;
    white-space: nowrap;
    border: 1px solid transparent;
  }
  .pill-teal {
    background: var(--accent-50);
    color: var(--accent-700);
  }
  .pill-purple {
    background: var(--memory-50);
    color: var(--memory-700);
  }
  .pill-green {
    background: var(--ok-50);
    color: var(--ok-600);
  }
  .pill-amber {
    background: var(--warn-50);
    color: var(--warn-600);
  }
  .pill-red {
    background: var(--danger-50);
    color: var(--danger-600);
  }
  .pill-neutral {
    background: var(--canvas-muted);
    color: var(--ink-700);
  }

  /* Figma Product Ledger status chips */
  .outline {
    background: var(--surface);
  }
  .outline.pill-green {
    border-color: color-mix(in srgb, var(--ok-500) 55%, var(--line));
    color: var(--ok-600);
  }
  .outline.pill-amber {
    border-color: color-mix(in srgb, var(--warn-500) 55%, var(--line));
    color: var(--warn-600);
  }
  .outline.pill-red {
    border-color: color-mix(in srgb, var(--danger-500) 45%, var(--line));
    color: var(--danger-600);
  }
  .outline.pill-neutral {
    border-color: var(--line);
    color: var(--ink-500);
  }
  .outline.pill-teal {
    border-color: color-mix(in srgb, var(--accent-500) 45%, var(--line));
    color: var(--accent-700);
  }
  .outline.pill-purple {
    border-color: color-mix(in srgb, var(--memory-500) 45%, var(--line));
    color: var(--memory-700);
  }
</style>
