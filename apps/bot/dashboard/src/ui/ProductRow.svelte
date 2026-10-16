<script lang="ts">
  /**
   * Product ledger row — maps Figma Product Ledger states
   * (node 112:585 / journey operator-home-join-user-journey.md).
   */
  import Pill from "./Pill.svelte";
  import Button from "./Button.svelte";
  import InlineAlert from "./InlineAlert.svelte";

  export type ProductReadiness =
    | "ready"
    | "needs_setup"
    | "needs_access"
    | "repair"
    | "offline"
    | "available"
    | "joining";

  type Props = {
    name: string;
    storeLabel?: string;
    /** e.g. "Google Drive" / "GitHub" for event store column */
    storeProvider?: string;
    readiness: ProductReadiness;
    updatedAt?: string;
    /** Join failure recovery copy; row stays under Available */
    joinFailure?: string | null;
    joinRetryable?: boolean;
    onopen?: () => void;
    onjoin?: () => void;
    onsetup?: () => void;
    onrepair?: () => void;
    onretry?: () => void;
  };

  let {
    name,
    storeLabel,
    storeProvider,
    readiness,
    updatedAt,
    joinFailure = null,
    joinRetryable = true,
    onopen,
    onjoin,
    onsetup,
    onrepair,
    onretry,
  }: Props = $props();

  const status = $derived.by(() => {
    switch (readiness) {
      case "ready":
        return { tone: "green" as const, label: "Ready" };
      case "needs_setup":
        return { tone: "amber" as const, label: "Needs setup" };
      case "needs_access":
      case "repair":
        return { tone: "amber" as const, label: "Needs access" };
      case "offline":
        return { tone: "neutral" as const, label: "Offline" };
      case "joining":
        return { tone: "purple" as const, label: "Joining" };
      case "available":
        return null;
      default:
        return { tone: "neutral" as const, label: "—" };
    }
  });

  const storeText = $derived(storeProvider ?? storeLabel ?? "—");
</script>

<tr class="product-row" class:has-failure={!!joinFailure}>
  <td class="name">{name}</td>
  <td class="store">
    <span class="store-text">{storeText}</span>
    {#if storeLabel && storeProvider}
      <span class="store-sub mono">{storeLabel}</span>
    {/if}
  </td>
  <td class="status">
    {#if joinFailure}
      <InlineAlert tone={joinRetryable ? "warn" : "danger"}>{joinFailure}</InlineAlert>
    {:else if status}
      <Pill tone={status.tone} variant="outline">{status.label}</Pill>
    {/if}
  </td>
  <td class="updated mono">{updatedAt ?? "—"}</td>
  <td class="actions">
    {#if joinFailure && joinRetryable}
      <Button variant="primary" size="sm" onclick={() => onretry?.()}>Try again</Button>
    {:else if joinFailure && !joinRetryable}
      <!-- terminal: copy only -->
    {:else if readiness === "ready"}
      <Button variant="secondary" size="sm" onclick={() => onopen?.()}>Open</Button>
    {:else if readiness === "needs_setup"}
      <Button variant="primary" size="sm" onclick={() => onsetup?.()}>Continue setup</Button>
    {:else if readiness === "needs_access" || readiness === "repair"}
      <Button variant="primary" size="sm" onclick={() => onrepair?.()}>Repair</Button>
    {:else if readiness === "offline"}
      <!-- no unsafe primary action -->
    {:else if readiness === "available"}
      <Button
        variant="primary"
        size="sm"
        onclick={() => onjoin?.()}
        aria-label="Join {name}"
      >
        Join
      </Button>
    {:else if readiness === "joining"}
      <Button variant="primary" size="sm" disabled loading aria-busy="true">
        Joining…
      </Button>
    {/if}
  </td>
</tr>

<style>
  .product-row td {
    padding: 1rem 0.85rem;
    border-bottom: 1px solid var(--line);
    font-size: var(--text-sm);
    vertical-align: middle;
  }
  .product-row:last-child td {
    border-bottom: 0;
  }
  .name {
    font-weight: 600;
    color: var(--ink-900);
  }
  .store {
    color: var(--ink-700);
  }
  .store-text {
    display: block;
  }
  .store-sub {
    display: block;
    margin-top: 2px;
    color: var(--ink-400);
    font-size: var(--text-xs);
  }
  .mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
  }
  .updated {
    color: var(--ink-500);
    white-space: nowrap;
  }
  .actions {
    text-align: right;
    white-space: nowrap;
  }
  .has-failure .status {
    max-width: 16rem;
  }
</style>
