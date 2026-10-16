<script lang="ts">
  /**
   * MCP agent-host readiness — compact rows with Claude/Codex marks.
   * Dashboard displays status + setup command only (no install authority).
   */
  import Pill from "./Pill.svelte";
  import Button from "./Button.svelte";
  import BrandMark from "./BrandMark.svelte";

  export type McpReadinessState =
    | "not_installed"
    | "prerequisite_missing"
    | "installed_pending_host_action"
    | "active_verified"
    | "repair_required";

  type AgentId = "claude" | "codex";

  type Props = {
    state: McpReadinessState;
    subjectId?: string;
    reasonCodes?: string[];
    observedAt?: string;
    lastVerifiedAt?: string | null;
    setupCommand?: string;
    oncontinue?: () => void;
    oncopy?: () => void;
    "data-testid"?: string;
  };

  let {
    state,
    subjectId = "claude",
    reasonCodes = [],
    observedAt: _observedAt,
    lastVerifiedAt: _lastVerifiedAt = null,
    setupCommand = "bicameral setup mcp",
    oncontinue,
    oncopy,
    "data-testid": testId,
  }: Props = $props();

  const agents: { id: AgentId; name: string }[] = [
    { id: "claude", name: "Claude" },
    { id: "codex", name: "Codex" },
  ];

  const activeId = $derived(
    (subjectId || "claude").toLowerCase().includes("codex")
      ? "codex"
      : "claude",
  );

  const meta = $derived.by(() => {
    switch (state) {
      case "active_verified":
        return { tone: "green" as const, label: "Ready" };
      case "installed_pending_host_action":
        return { tone: "amber" as const, label: "Pending host action" };
      case "prerequisite_missing":
        return { tone: "amber" as const, label: "Prerequisite missing" };
      case "repair_required":
        return { tone: "red" as const, label: "Repair required" };
      case "not_installed":
      default:
        return { tone: "neutral" as const, label: "Not installed" };
    }
  });

  function rowStatus(id: AgentId): { tone: typeof meta.tone; label: string } {
    if (id === activeId) return meta;
    return { tone: "neutral", label: "Supported" };
  }
</script>

<section class="mcp-block" data-testid={testId} aria-label="MCP readiness">
  <div class="agent-list">
    {#each agents as agent (agent.id)}
      {@const row = rowStatus(agent.id)}
      <div
        class="conn-row"
        class:active={agent.id === activeId}
        data-agent={agent.id}
      >
        <div class="id">
          <BrandMark brand={agent.id} size={32} />
          <div class="text">
            <p class="name">{agent.name}</p>
            <p class="detail">
              {#if agent.id === activeId}
                {#if state === "active_verified"}
                  MCP verified
                {:else if reasonCodes[0]}
                  {reasonCodes[0]}
                {:else}
                  MCP host
                {/if}
              {:else}
                MCP host
              {/if}
            </p>
          </div>
        </div>
        <Pill tone={row.tone} variant="outline">{row.label}</Pill>
        <div class="actions">
          {#if agent.id === activeId && state !== "active_verified"}
            <Button variant="secondary" size="sm" onclick={() => oncopy?.()}
              >Copy setup</Button
            >
          {:else if agent.id === activeId && state === "active_verified" && oncontinue}
            <Button
              variant="dark"
              size="sm"
              onclick={oncontinue}
              data-testid="mcp-continue-products">Open products</Button
            >
          {/if}
        </div>
      </div>
    {/each}
  </div>

  {#if state !== "active_verified"}
    <div class="cmd-row">
      <code class="cmd">{setupCommand}</code>
      <Button variant="secondary" size="sm" onclick={() => oncopy?.()}>Copy</Button>
    </div>
  {/if}
</section>

<style>
  .mcp-block {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .agent-list {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .conn-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    align-items: center;
    gap: 0.75rem 1rem;
    padding: 0.75rem 1rem;
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background: var(--surface);
  }
  .conn-row.active {
    border-color: var(--line-strong);
  }
  .id {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    min-width: 0;
  }
  .name {
    margin: 0;
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--ink-900);
  }
  .detail {
    margin: 0.1rem 0 0;
    font-size: var(--text-xs);
    color: var(--ink-500);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    min-width: 5.5rem;
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
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    padding: 0.4rem 0.65rem;
    background: var(--ink-900);
    color: #fff;
    border-radius: var(--radius-md);
    overflow: auto;
    white-space: nowrap;
  }

  @media (max-width: 640px) {
    .conn-row {
      grid-template-columns: minmax(0, 1fr) auto;
    }
    .actions {
      grid-column: 1 / -1;
      justify-content: flex-start;
      min-width: 0;
    }
  }
</style>
