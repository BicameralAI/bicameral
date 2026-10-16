<script lang="ts">
  import { onMount } from "svelte";

  type Props = {
    onpairing?: () => void;
  };

  let { onpairing }: Props = $props();
  let action: HTMLButtonElement;

  onMount(() => {
    action.focus();
  });

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape" || event.key === "Tab") {
      event.preventDefault();
    }
    if (event.key === "Tab") {
      action.focus();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="disconnect-scrim" data-testid="daemon-disconnect-scrim">
  <div
    class="disconnect-dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="daemon-disconnect-title"
    aria-describedby="daemon-disconnect-description"
  >
    <div class="dialog-copy">
      <h2 id="daemon-disconnect-title">Local daemon disconnected</h2>
      <p id="daemon-disconnect-description">
        Bicameral can’t reach your local daemon. Reconnect this device to continue.
      </p>
    </div>
    <button bind:this={action} type="button" onclick={onpairing}>
      Reconnect device
    </button>
  </div>
</div>

<style>
  .disconnect-scrim {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: grid;
    place-items: center;
    padding: var(--sp-5);
    background: rgba(24, 24, 27, 0.56);
  }

  .disconnect-dialog {
    width: min(32rem, 100%);
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: var(--sp-6);
    padding: var(--sp-6);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    background: var(--surface);
    box-shadow: 0 16px 40px rgba(24, 24, 27, 0.24);
  }

  .dialog-copy {
    min-width: 0;
  }

  h2 {
    color: var(--ink-900);
    font-size: var(--text-lg);
    font-weight: 700;
    line-height: 1.2;
  }

  p {
    max-width: 21rem;
    margin-top: var(--sp-2);
    color: var(--ink-500);
    font-size: var(--text-base);
    line-height: 1.5;
  }

  button {
    min-width: 7.5rem;
    min-height: 3.0625rem;
    flex: 0 0 auto;
    padding: var(--sp-3) var(--sp-5);
    border: 1px solid var(--accent-700);
    border-radius: var(--radius-md);
    background: var(--accent-700);
    color: var(--surface);
    font-family: inherit;
    font-size: var(--text-sm);
    font-weight: 700;
    cursor: pointer;
  }

  button:hover {
    background: var(--accent-900);
  }

  button:focus-visible {
    outline: 3px solid var(--accent-200);
    outline-offset: 3px;
  }

  @media (max-width: 36rem) {
    .disconnect-dialog {
      align-items: stretch;
      flex-direction: column;
    }

    button {
      width: 100%;
    }
  }
</style>
