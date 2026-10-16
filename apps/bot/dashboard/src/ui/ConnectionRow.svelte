<script lang="ts">
  import Pill from "./Pill.svelte";
  import Button from "./Button.svelte";

  type Status = "connected" | "not_connected" | "action_needed" | "unsupported";
  type Props = {
    providerId: string;
    label: string;
    description?: string;
    status: Status;
    accountLabel?: string;
    onconnect?: () => void;
    "data-testid"?: string;
  };

  let {
    providerId,
    label,
    description,
    status,
    accountLabel,
    onconnect,
    "data-testid": testId,
  }: Props = $props();

  const tone = $derived(
    status === "connected"
      ? "green"
      : status === "action_needed"
        ? "amber"
        : status === "unsupported"
          ? "neutral"
          : "neutral",
  );
  const statusLabel = $derived(
    status === "connected"
      ? "Connected"
      : status === "action_needed"
        ? "Action needed"
        : status === "unsupported"
          ? "Unsupported"
          : "Not connected",
  );
</script>

<div class="row" data-testid={testId} data-provider={providerId}>
  <div class="meta">
    <div class="title-row">
      <span class="name">{label}</span>
      <Pill {tone}>{statusLabel}</Pill>
    </div>
    {#if description}<p class="desc">{description}</p>{/if}
    {#if accountLabel}<p class="acct mono">{accountLabel}</p>{/if}
  </div>
  {#if status !== "unsupported" && status !== "connected"}
    <Button variant="secondary" size="sm" onclick={() => onconnect?.()}>
      {status === "action_needed" ? "Reconnect" : "Connect"}
    </Button>
  {:else if status === "connected"}
    <Button variant="ghost" size="sm" onclick={() => onconnect?.()}>Manage</Button>
  {/if}
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-4);
    padding: var(--sp-4);
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background: var(--surface);
  }
  .title-row {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin-bottom: var(--sp-1);
  }
  .name {
    font-size: var(--text-sm);
    font-weight: 600;
  }
  .desc {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--ink-500);
  }
  .acct {
    margin: var(--sp-1) 0 0;
    font-size: var(--text-xs);
    color: var(--ink-700);
  }
  .mono {
    font-family: var(--font-mono);
  }
</style>
