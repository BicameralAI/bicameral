<script lang="ts">
  type Tone = "teal" | "purple" | "green" | "amber" | "red" | "neutral";
  type Props = {
    tone?: Tone;
    label?: string;
  };
  let { tone = "neutral", label }: Props = $props();
</script>

<span class="dot-wrap">
  <span class="dot tone-{tone}" aria-hidden="true"></span>
  {#if label}<span class="lab">{label}</span>{/if}
</span>

<style>
  .dot-wrap {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
  }
  .dot {
    display: inline-block;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .tone-teal {
    background: var(--accent-500);
  }
  .tone-purple {
    background: var(--memory-500);
  }
  .tone-green {
    background: var(--ok-500);
  }
  .tone-amber {
    background: var(--warn-500);
  }
  .tone-red {
    background: var(--danger-500);
  }
  .tone-neutral {
    background: var(--ink-400);
  }
  .lab {
    font-size: var(--text-xs);
    color: var(--ink-700);
  }
</style>
