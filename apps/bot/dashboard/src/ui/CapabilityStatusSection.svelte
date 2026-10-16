<script lang="ts">
  /**
   * MCP readiness block (read-only projection).
   * Spec: docs/specs/mcp-distribution-and-developer-host-setup.md
   * Continue / Products gate is owned by the parent layout.
   */
  import Pill from "./Pill.svelte";
  import Button from "./Button.svelte";
  import InlineCode from "./InlineCode.svelte";
  import type { McpReadinessState } from "./McpReadinessCard.svelte";

  type Props = {
    mcpState: McpReadinessState;
    mcpHostId?: string;
    mcpReasons?: string[];
    setupCommand?: string;
    /** Compact row for checklist rail vs standalone card */
    compact?: boolean;
    oncopy?: () => void;
    "data-testid"?: string;
  };

  let {
    mcpState,
    mcpHostId = "claude",
    mcpReasons = [],
    setupCommand = "bicameral setup mcp",
    compact = false,
    oncopy,
    "data-testid": testId,
  }: Props = $props();

  const mcp = $derived.by(() => {
    switch (mcpState) {
      case "active_verified":
        return {
          tone: "green" as const,
          label: "Installed",
          hint: "Verified for this operator host.",
        };
      // Install is enough for connect Continue; treat like Installed (older UX).
      case "installed_pending_host_action":
        return {
          tone: "green" as const,
          label: "Installed",
          hint: "",
        };
      case "prerequisite_missing":
        return {
          tone: "amber" as const,
          label: "Prerequisite missing",
          hint: "Install uv / host CLI, then re-run terminal setup.",
        };
      case "repair_required":
        return {
          tone: "red" as const,
          label: "Repair required",
          hint: "Config drift — re-run terminal setup.",
        };
      default:
        return {
          tone: "neutral" as const,
          label: "Not installed",
          hint: "Run setup in the terminal. This page only displays status.",
        };
    }
  });

  /** Package installed (incl. pending host activation) — hide setup command. */
  const ready = $derived(
    mcpState === "active_verified" || mcpState === "installed_pending_host_action",
  );
</script>

<div class="mcp" class:compact data-testid={testId} aria-label="MCP readiness">
  <div class="head">
    <div class="meta">
      <span class="name">MCP</span>
      <span class="host"><InlineCode>{mcpHostId}</InlineCode></span>
    </div>
    <Pill tone={mcp.tone} variant="outline">{mcp.label}</Pill>
  </div>
  {#if mcp.hint && !ready}
    <p class="hint">{mcp.hint}</p>
  {/if}
  {#if mcpReasons.length && !ready}
    <p class="reasons">
      {#each mcpReasons as code, i}
        {#if i > 0}, {/if}<InlineCode>{code}</InlineCode>
      {/each}
    </p>
  {/if}
  {#if !ready}
    <div class="cmd-row">
      <code class="cmd">{setupCommand}</code>
      <Button variant="secondary" size="sm" onclick={() => oncopy?.()}>Copy</Button>
    </div>
  {/if}
</div>

<style>
  .mcp {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 0.75rem;
  }
  .meta {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
  }
  .name {
    font-size: 0.95rem;
    font-weight: 700;
    color: var(--ink-900, #0f1115);
  }
  .hint {
    margin: 0;
    font-size: 0.85rem;
    color: var(--ink-500, #6b7280);
    line-height: 1.4;
  }
  .reasons {
    margin: 0;
    font-size: 0.75rem;
    color: var(--ink-500, #6b7280);
  }
  .cmd-row {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    margin-top: 0.25rem;
  }
  .cmd {
    flex: 1;
    min-width: 0;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 0.78rem;
    padding: 0.5rem 0.65rem;
    background: var(--canvas-muted, #f6f7f9);
    border: 1px solid var(--line, #e5e7eb);
    border-radius: 6px;
    color: var(--ink-900, #0f1115);
    overflow: auto;
    white-space: nowrap;
  }
</style>
