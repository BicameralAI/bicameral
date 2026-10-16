<script lang="ts">
  export type Step = {
    id: string;
    label: string;
    /** done | current | pending */
    state: "done" | "current" | "pending";
  };

  type Props = {
    title?: string;
    steps: Step[];
    /** When set, steps are selectable (progress rail navigation). */
    onselect?: (id: string) => void;
    "data-testid"?: string;
  };
  let { title = "Setup lifecycle", steps, onselect, "data-testid": testId }: Props =
    $props();
</script>

<aside class="step-list" data-testid={testId} aria-label={title}>
  <p class="title">{title}</p>
  <ol class="list">
    {#each steps as step, i (step.id)}
      <li class="step state-{step.state}">
        {#if onselect}
          <button
            type="button"
            class="step-btn"
            class:done={step.state === "done"}
            class:current={step.state === "current"}
            class:pending={step.state === "pending"}
            onclick={() => onselect(step.id)}
          >
            <span class="num" aria-hidden="true">{i + 1}</span>
            <span class="lab">{step.label}</span>
          </button>
        {:else}
          <span class="num" aria-hidden="true">{i + 1}</span>
          <span class="lab">{step.label}</span>
        {/if}
      </li>
    {/each}
  </ol>
</aside>

<style>
  .step-list {
    border: 1px solid color-mix(in srgb, var(--memory-500) 35%, var(--line));
    border-radius: var(--radius-lg);
    padding: var(--sp-4);
    background: var(--surface);
  }
  .title {
    margin: 0 0 var(--sp-3);
    font-size: var(--text-xs);
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--memory-600);
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }
  .step {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    font-size: var(--text-sm);
  }
  .step-btn {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    width: 100%;
    text-align: left;
    border: 0;
    background: transparent;
    padding: 0.4rem 0.35rem;
    margin: 0 -0.35rem;
    border-radius: var(--radius-sm);
    font: inherit;
    cursor: pointer;
    color: inherit;
  }
  .step-btn:hover {
    background: var(--memory-50);
  }
  .step-btn.current {
    background: var(--memory-50);
  }
  .num {
    width: 1.25rem;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--ink-400);
    flex-shrink: 0;
  }
  .state-done .lab,
  .step-btn.done .lab {
    color: var(--ink-900);
    font-weight: 600;
  }
  .state-current .lab,
  .step-btn.current .lab {
    color: var(--ink-900);
    font-weight: 600;
  }
  .state-current .num,
  .step-btn.current .num {
    color: var(--memory-600);
    font-weight: 700;
  }
  .state-pending .lab,
  .step-btn.pending .lab {
    color: var(--ink-400);
  }
</style>
