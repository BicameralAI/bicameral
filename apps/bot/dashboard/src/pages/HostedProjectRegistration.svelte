<script lang="ts">
  /**
   * Hosted Product Create panel (bot#799 / v0.1.17 T-B #1024).
   *
   * Collects Product name. Alpha gate: Create is disabled until the event
   * substrate (Google Drive) is connected; an ! affordance routes to
   * Connections. Parent coordinates daemon create/publish. Retry states are
   * presentation-only and never claim canonical Product lifecycle authority.
   */
  import type { RegistrationOutcome } from "../lib/hosted-project-registration";

  type Props = {
    busy?: boolean;
    result?: RegistrationOutcome | null;
    /** When false, Create is disabled and ! opens Connections. Default true preserves legacy callers. */
    eventSubstrateConnected?: boolean;
    eventSubstrateLabel?: string;
    /**
     * Google Drive folder step (#1264). `ready` means the daemon validated a
     * folder for this Product; only then is Create enabled. Legacy callers
     * that never pass a phase keep the pre-#1264 behavior.
     */
    folderPhase?: "idle" | "starting" | "picking" | "ready" | "failed";
    folderName?: string;
    folderMessage?: string;
    onselectfolder?: () => void;
    onresetfolder?: () => void;
    onsubmitcreate?: (input: { displayName: string; projectRef?: string }) => void;
    oncancel?: () => void;
    onrequestconnect?: () => void;
  };

  let {
    busy = false,
    result = null,
    eventSubstrateConnected = true,
    eventSubstrateLabel = "Google Drive",
    folderPhase,
    folderName = "",
    folderMessage = "",
    onselectfolder,
    onresetfolder,
    onsubmitcreate,
    oncancel,
    onrequestconnect,
  }: Props = $props();

  // The folder step participates only when a caller wires it (hosted create).
  const folderStepActive = $derived(folderPhase !== undefined);
  const folderReady = $derived(!folderStepActive || folderPhase === "ready");

  let displayName = $state("");

  const published = $derived(result?.phase === "published" ? result : null);
  const retryable = $derived(
    result &&
      (result.phase === "create_retry_needed" ||
        result.phase === "publication_retry_needed")
      ? result
      : null,
  );
  const problem = $derived(result?.phase === "failed" ? result : null);
  const createEnabled = $derived(
    !busy && displayName.trim() !== "" && eventSubstrateConnected && folderReady,
  );

  function submitCreate(event: Event) {
    event.preventDefault();
    const name = displayName.trim();
    if (!name || busy || !eventSubstrateConnected || !folderReady) return;
    onsubmitcreate?.({ displayName: name });
  }

  function retryCreate() {
    const name = displayName.trim();
    if (!name || busy || !eventSubstrateConnected || !folderReady) return;
    onsubmitcreate?.({ displayName: name });
  }
</script>

<section class="registration" data-testid="project-registration" data-mode="create">
  {#if published}
    <div class="status-card success-card" data-testid="registration-published">
      <span class="status-badge success-badge">Published</span>
      <h3 class="status-title" data-testid="registration-next-step">
        {published.nextStep?.title}
      </h3>
      <p class="status-detail">{published.nextStep?.detail}</p>
      <p
        class="axis"
        data-testid="registration-axis"
        data-axis={published.nextStep?.axis}
      >
        Next step lives on: <strong>Local daemon · registration and replay</strong>
      </p>
      <button
        class="btn secondary"
        onclick={oncancel}
        data-testid="registration-done"
      >
        Done
      </button>
    </div>
  {:else if retryable}
    <div class="status-card retry-card" data-testid="registration-retry-needed">
      <span class="status-badge retry-badge">
        {retryable.phase === "publication_retry_needed"
          ? "Not shared yet"
          : "Confirmation needed"}
      </span>
      <h3 class="status-title">
        {retryable.phase === "publication_retry_needed"
          ? "The Product exists locally"
          : "Retry the same Create intent safely"}
      </h3>
      <p class="status-detail" data-testid="registration-retry-detail">
        {retryable.message}
      </p>
      {#if retryable.supportId}
        <p class="support-id" data-testid="registration-support-id">
          Support ID: <code>{retryable.supportId}</code>
        </p>
      {/if}
      <p class="axis">
        Retrying reuses the original idempotency keys. It does not authorize
        Cloud to create Product identity or creator roles.
      </p>
      <div class="reg-actions">
        <button
          type="button"
          class="btn primary"
          onclick={retryCreate}
          data-testid="registration-retry"
          disabled={busy || displayName.trim() === ""}
        >
          {busy
            ? "Retrying…"
            : retryable.phase === "publication_retry_needed"
              ? "Retry sharing"
              : "Retry create"}
        </button>
        <button
          type="button"
          class="btn secondary"
          onclick={oncancel}
          data-testid="registration-cancel"
          disabled={busy}
        >
          Close
        </button>
      </div>
    </div>
  {:else}
    <form class="reg-form" data-testid="create-project-form" onsubmit={submitCreate}>
      <h3 class="reg-title">Create a product</h3>
      <p class="reg-help">
        Creates the canonical Product in your paired local daemon first, then
        publishes safe discovery metadata to Bicameral Cloud. Cloud never owns
        Product identity, creator roles, policy, registration, or ledger events.
      </p>
      {#if problem}
        <p class="reg-error" data-testid="registration-error">{problem.message}</p>
      {/if}
      <label class="reg-label" for="create-project-name">Product name</label>
      <input
        id="create-project-name"
        class="reg-input"
        data-testid="create-project-name"
        bind:value={displayName}
        placeholder="e.g. Checkout Service"
        disabled={busy}
        autocomplete="off"
      />
      <div
        class="substrate-row"
        data-testid="create-event-substrate"
        data-connected={eventSubstrateConnected ? "true" : "false"}
      >
        <span class="substrate-label">
          Event store (alpha): <strong>{eventSubstrateLabel}</strong>
        </span>
        {#if eventSubstrateConnected}
          <span class="substrate-ok" data-testid="create-substrate-ready">Connected</span>
        {:else}
          <button
            type="button"
            class="substrate-alert"
            data-testid="create-substrate-alert"
            title="{eventSubstrateLabel} is not connected. Open Settings → Storage to connect."
            aria-label="{eventSubstrateLabel} not connected. Open Settings to connect storage."
            onclick={() => onrequestconnect?.()}
          >
            !
          </button>
          <span class="substrate-hint">Connect to enable Create</span>
        {/if}
      </div>
      {#if folderStepActive && eventSubstrateConnected}
        <div
          class="folder-row"
          data-testid="create-drive-folder"
          data-phase={folderPhase}
        >
          <span class="substrate-label">
            Product folder in {eventSubstrateLabel}
          </span>
          {#if folderPhase === "ready"}
            <span class="substrate-ok" data-testid="create-drive-folder-ready"
              >{folderName || "Folder selected"}</span
            >
            <button
              type="button"
              class="btn secondary btn-inline"
              data-testid="create-drive-folder-change"
              onclick={() => onresetfolder?.()}
              disabled={busy}>Change</button
            >
          {:else if folderPhase === "starting" || folderPhase === "picking"}
            <span class="substrate-hint" data-testid="create-drive-folder-pending"
              >Choose a folder in the Google window, then come back here.</span
            >
            <button
              type="button"
              class="btn secondary btn-inline"
              data-testid="create-drive-folder-cancel"
              onclick={() => onresetfolder?.()}
              disabled={busy}>Cancel</button
            >
          {:else}
            {#if folderPhase === "failed" && folderMessage}
              <span class="reg-error" data-testid="create-drive-folder-error"
                >{folderMessage}</span
              >
            {/if}
            <button
              type="button"
              class="btn secondary btn-inline"
              data-testid="create-drive-folder-choose"
              onclick={() => onselectfolder?.()}
              disabled={busy}
              >{folderPhase === "failed" ? "Try again" : "Choose folder"}</button
            >
          {/if}
        </div>
      {/if}
      <div class="reg-actions">
        <button
          type="submit"
          class="btn primary"
          data-testid="create-project-submit"
          disabled={!createEnabled}
        >
          {busy ? "Creating locally…" : "Create product"}
        </button>
        <button
          type="button"
          class="btn secondary"
          onclick={oncancel}
          data-testid="registration-cancel"
          disabled={busy}
        >
          Cancel
        </button>
      </div>
    </form>
  {/if}
</section>

<style>
  .registration {
    max-width: 720px;
    margin: 0 auto 1.25rem;
  }

  .reg-form,
  .status-card {
    padding: 1.25rem;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--surface);
  }

  .folder-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex-wrap: wrap;
    margin: 0.5rem 0 0.75rem;
    font-size: 0.85rem;
  }
  .btn-inline {
    padding: 0.2rem 0.6rem;
    font-size: 0.8rem;
  }

  .substrate-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
    margin: 0.75rem 0 0.25rem;
    font-size: var(--text-sm);
    color: var(--ink-700);
  }

  .substrate-ok {
    color: var(--ok-600);
    font-weight: 600;
  }

  .substrate-alert {
    width: 1.5rem;
    height: 1.5rem;
    border-radius: var(--radius-pill);
    border: 1px solid var(--warn-600);
    background: var(--warn-50);
    color: var(--warn-600);
    font-weight: 700;
    cursor: pointer;
    line-height: 1;
  }

  .substrate-alert:hover {
    background: var(--warn-500);
    color: #fff;
  }

  .substrate-hint {
    color: var(--ink-500);
    font-size: var(--text-xs);
  }

  .btn.primary:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .success-card {
    border-color: var(--accent-200);
  }

  .retry-card {
    border-color: var(--warn-600);
  }

  .reg-title,
  .status-title {
    font-size: 1rem;
    font-weight: 600;
    color: var(--ink-900);
    margin-bottom: 0.5rem;
  }

  .reg-help,
  .status-detail {
    font-size: 0.8125rem;
    color: var(--ink-500);
    line-height: 1.5;
    margin-bottom: 1rem;
  }

  .support-id {
    margin: -0.25rem 0 1rem;
    color: var(--ink-500);
    font-size: 0.75rem;
  }

  .support-id code {
    color: var(--ink-700);
    font-family: var(--font-mono);
    overflow-wrap: anywhere;
  }

  .reg-label {
    display: block;
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--ink-700);
    margin-bottom: 0.25rem;
  }

  .reg-input {
    width: 100%;
    box-sizing: border-box;
    padding: 0.5rem 0.75rem;
    margin-bottom: 0.875rem;
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    font-family: inherit;
    font-size: 0.8125rem;
    color: var(--ink-900);
    background: var(--canvas);
  }

  .reg-input:focus-visible,
  .btn:focus-visible {
    outline: 2px solid var(--accent-500);
    outline-offset: 2px;
  }

  .reg-error {
    font-size: 0.8125rem;
    color: var(--warn-600);
    margin-bottom: 0.875rem;
  }

  .reg-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.75rem;
  }

  .btn {
    padding: 0.5rem 1rem;
    border-radius: var(--radius-md);
    font-family: inherit;
    font-size: 0.8125rem;
    font-weight: 600;
    cursor: pointer;
  }

  .btn.primary {
    border: none;
    background: var(--accent-500);
    color: var(--surface);
  }

  .btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .btn.secondary {
    border: 1px solid var(--line);
    background: var(--surface);
    color: var(--ink-900);
  }

  .status-badge {
    display: inline-block;
    font-size: 0.6875rem;
    font-weight: 600;
    padding: 0.125rem 0.5rem;
    border-radius: var(--radius-pill);
    margin-bottom: 0.75rem;
  }

  .success-badge {
    color: var(--accent-600);
    background: var(--accent-50);
  }

  .retry-badge {
    color: var(--warn-600);
    background: var(--warn-50);
  }

  .axis {
    font-size: 0.75rem;
    color: var(--ink-500);
    line-height: 1.5;
    margin-bottom: 1rem;
  }

  @media (max-width: 560px) {
    .registration {
      margin-inline: 0.75rem;
    }

    .reg-actions {
      flex-direction: column;
    }

    .btn {
      width: 100%;
    }
  }
</style>
