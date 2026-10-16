<script lang="ts">
  import {
    BRIDGE_ALLOWED_ACTIONS,
    BRIDGE_NOT_ALLOWED_ACTIONS,
  } from "../lib/hosted-bridge-types";

  type Props = {
    origin: string;
    cloudAccountId: string;
    /** Opaque handle for the pending request (#790); shown for traceability. */
    requestHandle?: string;
    /** Pending-request expiry, ISO-8601; rendered when present. */
    expiresAt?: string;
    onconfirm?: () => void;
    oncancel?: () => void;
  };

  let {
    origin,
    cloudAccountId,
    requestHandle,
    expiresAt,
    onconfirm,
    oncancel,
  }: Props = $props();
</script>

<div class="confirmation-screen">
  <div class="confirmation-card" data-testid="local-confirmation">
    <h2>Approve this browser to pair with your local Bicameral?</h2>
    <p class="description">
      This confirmation is served locally by your Bicameral daemon. Approving
      pairs the requesting browser to your local operator home. It does not
      grant a session until you approve here.
    </p>

    <dl class="request-facts">
      <div class="fact">
        <dt>Requesting origin</dt>
        <dd><code data-testid="requesting-origin">{origin}</code></dd>
      </div>
      <div class="fact">
        <dt>Requesting account</dt>
        <dd><code data-testid="requesting-account">{cloudAccountId}</code></dd>
      </div>
      {#if expiresAt}
        <div class="fact">
          <dt>Request expires</dt>
          <dd><code>{expiresAt}</code></dd>
        </div>
      {/if}
      {#if requestHandle}
        <div class="fact">
          <dt>Request handle</dt>
          <dd><code>{requestHandle}</code></dd>
        </div>
      {/if}
    </dl>

    <div class="action-lists">
      <div class="action-section allowed">
        <h3>Allowed</h3>
        <ul>
          {#each BRIDGE_ALLOWED_ACTIONS as action}
            <li>{action}</li>
          {/each}
        </ul>
      </div>

      <div class="action-section not-allowed">
        <h3>Forbidden</h3>
        <ul>
          {#each BRIDGE_NOT_ALLOWED_ACTIONS as action}
            <li>{action}</li>
          {/each}
        </ul>
      </div>
    </div>

    <div class="actions">
      <button class="primary" data-testid="approve-pairing" onclick={onconfirm}>
        Approve
      </button>
      <button class="secondary" data-testid="deny-pairing" onclick={oncancel}>
        Deny
      </button>
    </div>
  </div>
</div>

<style>
  .confirmation-screen {
    display: flex;
    justify-content: center;
    padding: 3rem 1rem;
  }

  .confirmation-card {
    max-width: 520px;
    width: 100%;
    background: #fff;
    border: 1px solid var(--line);
    padding: 2rem 1.75rem;
  }

  h2 {
    font-size: 1rem;
    font-weight: 700;
    color: var(--ink-900);
    margin-bottom: 0.75rem;
  }

  .description {
    font-size: 0.8125rem;
    color: var(--ink-500);
    line-height: 1.5;
    margin-bottom: 1.25rem;
  }

  .request-facts {
    margin: 0 0 1.25rem;
    padding: 0.75rem 1rem;
    background: var(--canvas-muted);
    border: 1px solid var(--line);
    border-radius: 4px;
  }

  .fact {
    display: flex;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.2rem 0;
  }

  .fact dt {
    font-size: 0.75rem;
    color: var(--ink-500);
    font-weight: 600;
  }

  .fact dd {
    margin: 0;
    font-size: 0.75rem;
    color: var(--ink-900);
    text-align: right;
    word-break: break-all;
  }

  .fact dd code {
    background: transparent;
    font-size: 0.75rem;
  }

  .action-lists {
    display: flex;
    gap: 1.5rem;
    margin-bottom: 1.25rem;
  }

  .action-section {
    flex: 1;
  }

  h3 {
    font-size: 0.75rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    margin-bottom: 0.5rem;
  }

  .allowed h3 {
    color: var(--ok-600);
  }

  .not-allowed h3 {
    color: var(--danger-600);
  }

  ul {
    list-style: none;
    padding: 0;
  }

  li {
    font-size: 0.8125rem;
    padding: 0.25rem 0;
    color: var(--ink-700);
    line-height: 1.4;
  }

  .allowed li::before {
    content: "\2713 ";
    color: var(--ok-600);
    font-weight: 600;
    margin-right: 0.25rem;
  }

  .not-allowed li::before {
    content: "\2717 ";
    color: var(--danger-600);
    font-weight: 600;
    margin-right: 0.25rem;
  }

  .actions {
    display: flex;
    gap: 0.75rem;
    padding-top: 0.75rem;
    border-top: 1px solid var(--canvas-muted);
  }

  .primary {
    padding: 0.5rem 1rem;
    background: var(--ink-900);
    color: #fff;
    border: none;
    font-size: 0.8125rem;
    font-family: inherit;
    font-weight: 500;
    cursor: pointer;
  }

  .primary:hover {
    background: var(--ink-700);
  }

  .secondary {
    padding: 0.5rem 1rem;
    background: #fff;
    color: var(--ink-700);
    border: 1px solid var(--line-strong);
    font-size: 0.8125rem;
    font-family: inherit;
    cursor: pointer;
  }

  .secondary:hover {
    background: var(--canvas-muted);
  }

  @media (max-width: 480px) {
    .action-lists {
      flex-direction: column;
      gap: 1rem;
    }
  }
</style>
