<script lang="ts">
  /**
   * Production connect-device (bot#1030 / #1035).
   * Left = focused lifecycle step; right = progress rail.
   * Gates: pair daemon → install MCP → Continue (MCP verified only).
   * Read-only MCP projection — never installs or mutates readiness.
   */
  import StepList, { type Step } from "../ui/StepList.svelte";
  import type { McpReadinessState } from "../ui/McpReadinessCard.svelte";
  import CapabilityStatusSection from "../ui/CapabilityStatusSection.svelte";
  import { isMcpProgressible } from "../lib/mcp-readiness";

  export type ConnectTone =
    | "idle"
    | "checking"
    | "error"
    | "warn"
    | "pending"
    | "ok";

  export type ConnectStatus =
    | "unavailable"
    | "checking"
    | "pending"
    | "connected"
    | "update_required";

  type StepId = "install" | "gateway" | "pair" | "mcp";

  type Props = {
    accountLabel?: string;
    status?: ConnectStatus;
    pairingCode?: string | null;
    mcpState?: McpReadinessState;
    mcpHostId?: string;
    mcpReasons?: string[];
    setupCommand?: string;
    oncheck?: () => void;
    onretry?: () => void;
    oncontinue?: () => void;
    oncopy?: (kind: "install" | "update" | "gateway" | "pairing" | "setup") => void;
    "data-testid"?: string;
  };

  let {
    accountLabel: _accountLabel = "jk",
    status = "unavailable",
    pairingCode = null,
    mcpState = "not_installed",
    mcpHostId = "claude",
    mcpReasons = [],
    setupCommand = "bicameral setup mcp",
    oncheck,
    onretry,
    oncontinue,
    oncopy,
    "data-testid": testId,
  }: Props = $props();

  /** Which step’s detail is shown on the left (user can review earlier steps). */
  let focusStep = $state<StepId | null>(null);

  const pairDone = $derived(status === "connected");
  /** Package installed is enough to enter Products; host activation is soft. */
  const mcpDone = $derived(isMcpProgressible(mcpState));
  const canContinue = $derived(pairDone && mcpDone);
  const pairInFlight = $derived(
    status === "pending" || status === "checking",
  );

  /** Gate-driven current step (progress), independent of review focus. */
  const gateCurrent = $derived.by((): StepId => {
    if (!pairDone && !pairInFlight) {
      return focusStep === "gateway" || focusStep === "pair" ? focusStep : "install";
    }
    if (!pairDone) return "pair";
    return "mcp";
  });

  const activeStep = $derived((focusStep ?? gateCurrent) as StepId);

  $effect(() => {
    if (!focusStep) return;
    if (!pairDone && focusStep === "mcp") {
      focusStep = "pair";
      return;
    }
    const order: StepId[] = ["install", "gateway", "pair", "mcp"];
    const fi = order.indexOf(focusStep);
    const gi = order.indexOf(gateCurrent);
    if (fi > gi + 1) focusStep = null;
  });

  /**
   * Exactly one rail step is `current` (= activeStep). Earlier completed
   * steps are `done`; later incomplete steps are `pending`.
   * (Previously pair was hard-coded `current` whenever unpaired, so row 3
   * stayed highlighted while the left panel showed step 1.)
   */
  const lifecycleSteps = $derived.by((): Step[] => {
    const order: StepId[] = ["install", "gateway", "pair", "mcp"];
    const labels: Record<StepId, string> = {
      install: "Install or update Bicameral",
      gateway: "Start local gateway",
      pair: "Pair this browser",
      mcp: "Install MCP",
    };

    // Highest fully completed step index (-1 = none complete yet).
    let completedThrough = -1;
    if (mcpDone) {
      completedThrough = 3;
    } else if (pairDone) {
      completedThrough = 2;
    } else if (pairInFlight || focusStep === "pair") {
      completedThrough = 1; // install + gateway done once pair is in focus/flight
    } else if (focusStep === "gateway") {
      completedThrough = 0;
    }

    const current = activeStep;
    return order.map((id, i) => {
      let state: "done" | "current" | "pending";
      if (id === current) {
        state = "current";
      } else if (i <= completedThrough) {
        state = "done";
      } else {
        state = "pending";
      }
      return { id, label: labels[id], state };
    });
  });

  const tone = $derived.by((): ConnectTone => {
    switch (status) {
      case "connected":
        return "ok";
      case "pending":
        return "pending";
      case "checking":
        return "checking";
      case "update_required":
        return "warn";
      default:
        return "error";
    }
  });

  const pairTitle = $derived(
    status === "pending"
      ? "Enter this code locally"
      : status === "connected"
        ? "Connected"
        : status === "checking"
          ? "Checking"
          : status === "update_required"
            ? "Update required"
            : "Not connected",
  );

  const pairDetail = $derived(
    status === "pending"
      ? "Paste the code into the running gateway terminal, or run bicameral pairing approve."
      : status === "connected"
        ? "Browser is paired to this operator’s local daemon."
        : status === "checking"
          ? "Checking localhost:37373"
          : "Open Bicameral on this device and retry.",
  );

  const detailTitle = $derived.by(() => {
    switch (activeStep) {
      case "install":
        return "Install or update Bicameral";
      case "gateway":
        return "Start the local gateway";
      case "pair":
        return "Pair this browser";
      case "mcp":
        return "Install MCP";
    }
  });

  const detailEyebrow = $derived.by(() => {
    const n =
      activeStep === "install"
        ? 1
        : activeStep === "gateway"
          ? 2
          : activeStep === "pair"
            ? 3
            : 4;
    return `Step ${n} of 4`;
  });

  function selectStep(id: string) {
    focusStep = id as StepId;
  }

  function copy(kind: "install" | "update" | "gateway" | "pairing" | "setup") {
    oncopy?.(kind);
  }

  function nextAfterInstall() {
    focusStep = "gateway";
  }
  function nextAfterGateway() {
    focusStep = "pair";
  }
</script>

<!--
  App.svelte owns the single global hosted header (brand + connection + avatar).
  This surface is body-only so we never stack a second top bar under the shell.
-->
<div class="cloud-connect" data-testid={testId}>
  <main class="dash">
    <p class="overline">Cloud Dashboard</p>
    <h1>Connect Bicameral on this device</h1>

    <div class="layout">
      <!-- LEFT: detail for active step -->
      <section class="card detail-card" aria-label="Step detail">
        <p class="step-eyebrow">{detailEyebrow}</p>
        <h2 class="detail-title">{detailTitle}</h2>

        {#if activeStep === "install"}
          <p class="detail-copy">
            New machines run the install script once. Existing installs can update in place.
          </p>
          <p class="cmd-label">New install</p>
          <div class="cmd">
            <code>curl -fsSL https://install.bicameral-ai.com/install.sh | sh</code>
            <button type="button" class="copy-btn" onclick={() => copy("install")}>Copy</button>
          </div>
          <p class="cmd-label">Already installed</p>
          <div class="cmd">
            <code>bicameral update</code>
            <button type="button" class="copy-btn" onclick={() => copy("update")}>Copy</button>
          </div>
          <div class="detail-actions">
            <button type="button" class="btn btn-cyan" onclick={nextAfterInstall}
              >Next: Start gateway</button
            >
          </div>
        {:else if activeStep === "gateway"}
          <p class="detail-copy">
            Keep the gateway running while you use the hosted app. Pairing and MCP both need
            it.
          </p>
          <div class="cmd">
            <code>bicameral gateway start</code>
            <button type="button" class="copy-btn" onclick={() => copy("gateway")}>Copy</button>
          </div>
          <div class="detail-actions">
            <button type="button" class="btn btn-cyan" onclick={nextAfterGateway}
              >Next: Pair browser</button
            >
          </div>
        {:else if activeStep === "pair"}
          <p class="detail-copy">
            Pairing authorizes this browser only — never projects, code, or approvals. Confirm
            with the code on the local machine.
          </p>
          <div class="status-block" data-tone={tone}>
            <p class="conn-line">
              <span class="status-dot"></span>
              <span class="conn-title">{pairTitle}</span>
            </p>
            <p class="conn-detail">{pairDetail}</p>

            {#if status === "pending" && pairingCode}
              <div class="pairing-code-box" role="status">
                <p class="pairing-code-label">Pairing code</p>
                <div class="pairing-code-row">
                  <code class="pairing-code">{pairingCode}</code>
                  <button type="button" class="copy-btn" onclick={() => copy("pairing")}
                    >Copy</button
                  >
                </div>
                <p class="pairing-code-help">
                  Paste into the gateway terminal, or run
                  <code class="inline">bicameral pairing approve</code>.
                </p>
              </div>
            {/if}

            {#if !pairDone}
              <div class="detail-actions">
                {#if status === "checking"}
                  <button type="button" class="btn btn-cyan" disabled>Checking…</button>
                {:else if status === "pending"}
                  <button type="button" class="btn btn-ghost" onclick={() => onretry?.()}
                    >Cancel</button
                  >
                {:else}
                  <button
                    type="button"
                    class="btn btn-cyan"
                    data-testid="mock-pair-retry"
                    onclick={() =>
                      status === "unavailable" ? onretry?.() : oncheck?.()}
                  >
                    {status === "unavailable" ? "Retry" : "Check connection"}
                  </button>
                {/if}
              </div>
            {:else}
              <div class="detail-actions">
                <button type="button" class="btn btn-cyan" onclick={() => (focusStep = "mcp")}
                  >Next: Install MCP</button
                >
              </div>
            {/if}
          </div>
        {:else}
          <!-- mcp (final step) -->
          <p class="detail-copy">
            Run the setup command below in a terminal, then return here. Status updates when
            the local daemon reports MCP readiness.
          </p>
          {#if !pairDone}
            <p class="locked-hint">Pair the local daemon first (step 3).</p>
            <div class="detail-actions">
              <button type="button" class="btn btn-ghost" onclick={() => (focusStep = "pair")}
                >Go to Pair browser</button
              >
            </div>
          {:else}
            <CapabilityStatusSection
              {mcpState}
              {mcpHostId}
              mcpReasons={mcpReasons}
              {setupCommand}
              oncopy={() => copy("setup")}
              data-testid="mcp-readiness"
            />
            <div class="detail-actions">
              <button
                type="button"
                class="btn btn-cyan"
                data-testid="mcp-continue-products"
                disabled={!canContinue}
                onclick={() => oncontinue?.()}
                title={canContinue
                  ? "Open Products"
                  : "MCP must be installed first"}
              >
                Continue to Products
              </button>
            </div>
            {#if !canContinue}
              <p class="continue-hint">
                Continue enables when MCP status is <strong>Installed</strong>.
              </p>
            {/if}
          {/if}
        {/if}
      </section>

      <!-- RIGHT: purple progress rail -->
      <div class="rail">
        <StepList
          title="Connect lifecycle"
          steps={lifecycleSteps}
          onselect={selectStep}
          data-testid="connect-lifecycle"
        />
      </div>
    </div>
  </main>
</div>

<style>
  .cloud-connect {
    --ink: #0f1115;
    --muted: #6b7280;
    --faint: #9aa1ab;
    --line: #e5e7eb;
    --surface: #ffffff;
    --panel: #f6f7f9;
    --page: #fbfbfc;
    --cyan: #0891b2;
    --cyan-strong: #0e7490;
    --ok: #16a34a;
    --warn: #d97706;
    --error: #dc2626;
    --idle: #9aa1ab;
    flex: 1;
    background: var(--page);
    color: var(--ink);
    font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont,
      "Segoe UI", sans-serif;
  }
  .dash {
    max-width: 1100px;
    margin: 0 auto;
    padding: 48px 32px 80px;
  }
  .overline {
    text-transform: uppercase;
    letter-spacing: 0.14em;
    font-size: 0.72rem;
    font-weight: 700;
    color: var(--cyan);
    margin: 0 0 12px;
  }
  h1 {
    font-size: clamp(1.75rem, 4vw, 2.4rem);
    line-height: 1.08;
    letter-spacing: -0.02em;
    margin: 0 0 36px;
  }

  /* Create-product pattern: detail | purple rail */
  .layout {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(220px, 260px);
    gap: 28px;
    align-items: start;
  }
  .rail {
    position: sticky;
    top: 24px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .card {
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: 12px;
    padding: 28px;
  }
  .step-eyebrow {
    margin: 0 0 8px;
    font-size: 0.72rem;
    font-weight: 700;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--cyan);
  }
  .detail-title {
    margin: 0 0 12px;
    font-size: 1.35rem;
    font-weight: 700;
    letter-spacing: -0.02em;
  }
  .detail-copy {
    margin: 0 0 20px;
    font-size: 0.95rem;
    color: var(--muted);
    line-height: 1.5;
    max-width: 36rem;
  }
  .cmd-label {
    font-size: 0.8rem;
    color: var(--faint);
    margin: 14px 0 6px;
  }
  .cmd-label:first-of-type {
    margin-top: 0;
  }
  .cmd {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 8px;
    padding: 12px 14px;
  }
  .cmd code {
    font-size: 0.88rem;
    overflow-x: auto;
    white-space: nowrap;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .copy-btn {
    flex: none;
    appearance: none;
    border: 1px solid var(--line);
    background: var(--surface);
    color: var(--ink);
    font: inherit;
    font-size: 0.8rem;
    padding: 5px 12px;
    border-radius: 6px;
    cursor: pointer;
  }
  .copy-btn:hover {
    border-color: var(--cyan);
    color: var(--cyan-strong);
  }
  .detail-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    margin-top: 22px;
  }
  .btn {
    appearance: none;
    font: inherit;
    font-weight: 600;
    border-radius: 8px;
    padding: 11px 18px;
    cursor: pointer;
    border: 1px solid transparent;
  }
  .btn:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .btn-cyan {
    background: var(--cyan);
    color: #fff;
  }
  .btn-cyan:hover:not(:disabled) {
    background: var(--cyan-strong);
  }
  .btn-ghost {
    background: var(--surface);
    color: var(--ink);
    border-color: var(--line);
  }

  .status-block {
    padding: 16px;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--panel);
  }
  .conn-line {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0 0 6px;
  }
  .conn-title {
    font-size: 1.1rem;
    font-weight: 700;
  }
  .status-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--idle);
  }
  .status-block[data-tone="ok"] .status-dot {
    background: var(--ok);
  }
  .status-block[data-tone="error"] .status-dot {
    background: var(--error);
  }
  .status-block[data-tone="pending"] .status-dot {
    background: var(--cyan);
  }
  .status-block[data-tone="checking"] .status-dot {
    background: var(--cyan);
    animation: pulse 1.1s ease-in-out infinite;
  }
  @keyframes pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.35;
    }
  }
  .conn-detail {
    margin: 0 0 12px;
    font-size: 0.9rem;
    color: var(--muted);
  }
  .pairing-code-box {
    margin: 0 0 12px;
    padding: 14px;
    border: 1px solid #bae6fd;
    background: #ecfeff;
    border-radius: 8px;
  }
  .pairing-code-label {
    margin: 0 0 6px;
    color: var(--cyan-strong);
    font-size: 0.7rem;
    font-weight: 700;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }
  .pairing-code-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  .pairing-code {
    font-size: 1.4rem;
    font-weight: 750;
    letter-spacing: 0.09em;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .pairing-code-help {
    margin: 10px 0 0;
    font-size: 0.82rem;
    color: var(--muted);
    line-height: 1.4;
  }
  .pairing-code-help .inline {
    font-size: 0.9em;
    background: #fff;
    padding: 0.05rem 0.3rem;
    border-radius: 4px;
  }
  .locked-hint {
    margin: 0 0 12px;
    font-size: 0.9rem;
    color: var(--faint);
  }
  .continue-hint {
    margin: 12px 0 0;
    font-size: 0.85rem;
    color: var(--faint);
  }
  @media (max-width: 820px) {
    .dash {
      padding: 32px 20px 64px;
    }
    .layout {
      grid-template-columns: 1fr;
    }
    .rail {
      position: static;
      order: -1;
    }
  }
</style>
