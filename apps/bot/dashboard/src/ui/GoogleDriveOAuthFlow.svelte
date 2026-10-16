<script lang="ts">
  /**
   * Google Drive connection — compact Settings row (logo + status + action).
   * Long protocol essays removed; fail states keep one short line.
   *
   * Presentation only: the caller derives `phase` from daemon-authored
   * outcomes (bot#1263). This component has no way to declare success.
   */
  import type { DrivePhase } from "../lib/drive-connection";
  import Button from "./Button.svelte";
  import Pill from "./Pill.svelte";
  import Spinner from "./Spinner.svelte";
  import BrandMark from "./BrandMark.svelte";

  export type DriveOAuthPhase = DrivePhase;

  type Props = {
    phase: DriveOAuthPhase;
    accountLabel?: string;
    sessionId?: string | null;
    onstart?: () => void;
    onopenauth?: () => void;
    oncancel?: () => void;
    "data-testid"?: string;
  };

  let {
    phase,
    accountLabel = "",
    sessionId: _sessionId = null,
    onstart,
    onopenauth,
    oncancel,
    "data-testid": testId,
  }: Props = $props();

  const status = $derived.by(() => {
    switch (phase) {
      case "connected":
        return { tone: "green" as const, label: "Connected" };
      case "waiting":
      case "authorize":
      case "starting":
        return { tone: "purple" as const, label: "In progress" };
      case "denied":
      case "expired":
      case "provider_unavailable":
        return { tone: "red" as const, label: "Failed" };
      case "action_needed":
        return { tone: "red" as const, label: "Needs attention" };
      default:
        return { tone: "neutral" as const, label: "Not connected" };
    }
  });

  const detail = $derived.by(() => {
    if (phase === "connected" && accountLabel) return accountLabel;
    if (phase === "denied") return "Consent denied";
    if (phase === "expired") return "Session expired";
    if (phase === "provider_unavailable") return "Bicameral could not reach Google";
    if (phase === "action_needed") return "Google needs another try";
    if (phase === "waiting") return "Finish in the Google tab — this updates automatically";
    if (phase === "starting") return "Starting…";
    if (phase === "authorize") return "Open Google to continue";
    return "Event store";
  });
</script>

<section class="conn-row" data-testid={testId} aria-label="Google Drive connection">
  <div class="id">
    <BrandMark brand="google-drive" size={32} />
    <div class="text">
      <p class="name">Google Drive</p>
      <p class="detail">{detail}</p>
    </div>
  </div>

  <div class="meta">
    <Pill tone={status.tone} variant="outline">{status.label}</Pill>
  </div>

  <div class="actions">
    {#if phase === "idle"}
      <Button
        variant="dark"
        size="sm"
        onclick={() => onstart?.()}
        data-testid="drive-oauth-start">Connect</Button
      >
    {:else if phase === "starting"}
      <Spinner />
    {:else if phase === "waiting"}
      <Spinner />
      <Button
        variant="ghost"
        size="sm"
        onclick={() => onopenauth?.()}
        data-testid="drive-oauth-reopen">Reopen Google</Button
      >
      <Button variant="ghost" size="sm" onclick={() => oncancel?.()}>Cancel</Button>
    {:else if phase === "authorize"}
      <Button
        variant="dark"
        size="sm"
        onclick={() => onopenauth?.()}
        data-testid="drive-oauth-open">Authorize</Button
      >
      <Button variant="ghost" size="sm" onclick={() => oncancel?.()}>Cancel</Button>
    {:else if phase === "connected"}
      <span class="disconnect-unavailable" data-testid="drive-disconnect-unavailable">
        Disconnect unavailable in Alpha
      </span>
    {:else}
      <Button
        variant="dark"
        size="sm"
        onclick={() => onstart?.()}
        data-testid="drive-oauth-retry">Retry</Button
      >
      <Button variant="ghost" size="sm" onclick={() => oncancel?.()}>Cancel</Button>
    {/if}
  </div>
</section>

<style>
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
  .id {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    min-width: 0;
  }
  .text {
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
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .meta {
    flex-shrink: 0;
  }
  .actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 0.35rem;
    flex-shrink: 0;
  }
  .disconnect-unavailable {
    color: var(--ink-500);
    font-size: var(--text-xs);
    white-space: nowrap;
  }

  @media (max-width: 640px) {
    .conn-row {
      grid-template-columns: minmax(0, 1fr) auto;
      grid-template-rows: auto auto;
    }
    .meta {
      justify-self: end;
    }
    .actions {
      grid-column: 1 / -1;
      justify-content: flex-start;
    }
  }
</style>
