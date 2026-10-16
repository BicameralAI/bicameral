<script lang="ts">
  type Props = {
    label: string;
    onclick?: () => void;
    "data-testid"?: string;
  };
  let { label, onclick, "data-testid": testId }: Props = $props();

  const initials = $derived(
    label
      .split(/[\s@._-]+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((p) => p[0]?.toUpperCase() ?? "")
      .join("") || "?",
  );
</script>

{#if onclick}
  <button type="button" class="avatar" data-testid={testId} {onclick} title={label}>
    {initials}
  </button>
{:else}
  <span class="avatar static" data-testid={testId} title={label}>{initials}</span>
{/if}

<style>
  .avatar {
    width: 2rem;
    height: 2rem;
    border-radius: var(--radius-pill);
    border: 0;
    background: var(--memory-500);
    color: #fff;
    font-family: var(--font-sans);
    font-size: var(--text-xs);
    font-weight: 700;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    flex-shrink: 0;
  }
  .avatar.static {
    cursor: default;
  }
  .avatar:hover:not(.static) {
    background: var(--memory-600);
  }
</style>
