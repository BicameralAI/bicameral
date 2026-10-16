<script lang="ts">
  type Props = {
    message: string;
    tone?: "info" | "ok" | "warn" | "danger";
    ondismiss?: () => void;
  };
  let { message, tone = "info", ondismiss }: Props = $props();
</script>

<div class="toast tone-{tone}" role="status">
  <span class="msg">{message}</span>
  {#if ondismiss}
    <button type="button" class="x" onclick={ondismiss} aria-label="Dismiss">✕</button>
  {/if}
</div>

<style>
  .toast {
    position: fixed;
    bottom: var(--sp-4);
    right: var(--sp-4);
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    max-width: min(24rem, calc(100vw - 2rem));
    padding: var(--sp-2) var(--sp-4);
    border-radius: var(--radius-md);
    font-size: var(--text-sm);
    box-shadow: var(--shadow-2);
    z-index: 50;
  }
  .tone-info,
  .tone-ok {
    background: var(--ink-900);
    color: #fff;
  }
  .tone-warn {
    background: var(--warn-600);
    color: #fff;
  }
  .tone-danger {
    background: var(--danger-600);
    color: #fff;
  }
  .x {
    border: 0;
    background: none;
    color: inherit;
    opacity: 0.7;
    cursor: pointer;
    font-size: 0.75rem;
  }
</style>
