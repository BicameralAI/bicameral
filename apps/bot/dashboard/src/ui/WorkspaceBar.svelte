<script lang="ts">
  type Props = {
    title?: string;
    context?: string;
  };
  let { title = "Products", context = "WORKSPACE  ·  HOSTED" }: Props = $props();
</script>

<div class="wbar" data-testid="workspace-topbar">
  <strong class="title">{title}</strong>
  <div class="ctx">
    <span class="meta">{context}</span>
    <span class="cmd" aria-hidden="true">⌕ ⌘K</span>
  </div>
</div>

<style>
  .wbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 48px;
    padding: 0 0 var(--sp-3);
    margin-bottom: var(--sp-2);
    border-bottom: 1px solid var(--line);
  }
  .title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--ink-900);
  }
  .ctx {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
  }
  .meta {
    font-size: var(--text-xs);
    letter-spacing: 0.04em;
    color: var(--ink-400);
  }
  .cmd {
    font-size: var(--text-xs);
    color: var(--ink-400);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    padding: 0.2rem 0.45rem;
    background: var(--canvas-muted);
  }
</style>
