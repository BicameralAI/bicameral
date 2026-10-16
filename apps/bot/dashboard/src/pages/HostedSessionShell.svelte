<script lang="ts">
  import type { HostedBridgeSessionInfo } from "../lib/hosted-bridge-types";

  type Props = {
    session: HostedBridgeSessionInfo;
    onrepair?: () => void;
    onuselocal?: () => void;
  };

  let { session, onrepair, onuselocal }: Props = $props();

  function formatDateTime(iso: string): string {
    try {
      return new Date(iso).toLocaleString(undefined, {
        year: "numeric",
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      });
    } catch {
      return iso;
    }
  }

  const isExpiringSoon = $derived(() => {
    try {
      const remaining = new Date(session.expiresAt).getTime() - Date.now();
      return remaining > 0 && remaining < 3600_000;
    } catch {
      return false;
    }
  });
</script>

<div class="session-shell">
  <div class="session-header">
    <span class="session-title">Hosted session</span>
    <span
      class="session-badge"
      class:expiring={isExpiringSoon()}
    >
      {session.pairingState === "hosted_bridge_paired" ? "Active" : "Inactive"}
    </span>
  </div>

  <div class="session-details">
    <div class="detail-row">
      <span class="detail-label">Origin</span>
      <span class="detail-value"><code>{session.origin}</code></span>
    </div>
    <div class="detail-row">
      <span class="detail-label">Account</span>
      <span class="detail-value"><code>{session.cloudAccountId}</code></span>
    </div>
    <div class="detail-row">
      <span class="detail-label">Paired at</span>
      <span class="detail-value">{formatDateTime(session.pairedAt)}</span>
    </div>
    <div class="detail-row">
      <span class="detail-label">Expires</span>
      <span class="detail-value" class:expiring-text={isExpiringSoon()}>
        {formatDateTime(session.expiresAt)}
      </span>
    </div>
  </div>

  <div class="session-actions">
    {#if onrepair}
      <button class="action-btn" onclick={onrepair}>Re-pair</button>
    {/if}
    {#if onuselocal}
      <button class="link-btn" onclick={onuselocal}>
        Use local dashboard instead
      </button>
    {/if}
  </div>
</div>

<style>
  .session-shell {
    background: #fff;
    border: 1px solid var(--line);
    padding: 1rem 1.25rem;
    max-width: 460px;
  }

  .session-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 0.75rem;
  }

  .session-title {
    font-size: 0.875rem;
    font-weight: 600;
    color: var(--ink-900);
  }

  .session-badge {
    font-size: 0.75rem;
    font-weight: 500;
    padding: 0.2rem 0.5rem;
    border-radius: 9999px;
    background: var(--ok-50);
    color: var(--ok-600);
  }

  .session-badge.expiring {
    background: var(--warn-50);
    color: var(--warn-600);
  }

  .session-details {
    border-top: 1px solid var(--canvas-muted);
    padding-top: 0.5rem;
  }

  .detail-row {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    padding: 0.25rem 0;
    font-size: 0.8125rem;
  }

  .detail-label {
    color: var(--ink-500);
  }

  .detail-value {
    color: var(--ink-900);
  }

  .detail-value code {
    font-size: 0.75rem;
    background: var(--canvas-muted);
    padding: 0.1rem 0.375rem;
    border-radius: 3px;
  }

  .expiring-text {
    color: var(--warn-600);
    font-weight: 500;
  }

  .session-actions {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding-top: 0.75rem;
    border-top: 1px solid var(--canvas-muted);
    margin-top: 0.5rem;
  }

  .action-btn {
    padding: 0.375rem 0.875rem;
    border: 1px solid var(--line-strong);
    background: #fff;
    color: var(--ink-700);
    font-size: 0.8125rem;
    font-family: inherit;
    cursor: pointer;
  }

  .action-btn:hover {
    background: var(--canvas-muted);
  }

  .link-btn {
    background: none;
    border: none;
    color: var(--accent-500);
    font-size: 0.75rem;
    font-family: inherit;
    cursor: pointer;
    padding: 0;
    text-decoration: underline;
    text-decoration-color: transparent;
  }

  .link-btn:hover {
    text-decoration-color: var(--accent-500);
  }
</style>
