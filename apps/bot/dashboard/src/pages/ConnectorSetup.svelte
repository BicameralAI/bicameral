<script lang="ts">
  import type {
    ConnectorDescriptor,
    ConnectorInstruction,
    ConnectorCredential,
    RuntimeConfigEntry,
  } from "../lib/connector-types";

  interface Props {
    connector: ConnectorDescriptor;
    onback: () => void;
  }

  let { connector, onback }: Props = $props();

  let currentStep = $state(0);
  let secretValues = $state<Record<string, string>>({});
  let secretVisible = $state<Record<string, boolean>>({});
  let configValues = $state<Record<string, string>>({});
  let verifyState = $state<"idle" | "pending" | "done">("idle");
  let copied = $state<Record<string, boolean>>({});

  let instructions = $derived(connector.instructions ?? []);

  let currentInstruction = $derived(instructions[currentStep]);

  function credentialForStep(
    step: ConnectorInstruction,
  ): ConnectorCredential | undefined {
    if (step.action !== "paste_secret") return undefined;
    return connector.credentials[0];
  }

  function runtimeConfigEntries(): RuntimeConfigEntry[] {
    return connector.runtime_config ?? [];
  }

  function validationError(cred: ConnectorCredential, value: string): string | null {
    if (!value) return null;
    if (cred.validation) {
      const re = new RegExp(cred.validation);
      if (!re.test(value)) return `Does not match expected format (${cred.validation})`;
    }
    return null;
  }

  function handleVerify() {
    verifyState = "pending";
    setTimeout(() => {
      verifyState = "done";
    }, 1200);
  }

  function maskValue(value: string): string {
    if (value.length <= 4) return "••••";
    return "••••" + value.slice(-4);
  }

  async function copyToClipboard(text: string, key: string) {
    try {
      await navigator.clipboard.writeText(text);
      copied[key] = true;
      setTimeout(() => {
        copied[key] = false;
      }, 2000);
    } catch {
      /* clipboard not available in all contexts */
    }
  }

  function stepLabel(action: string): string {
    switch (action) {
      case "open_url":
        return "Open provider";
      case "paste_secret":
        return "Add credential";
      case "register_webhook":
        return "Register webhook";
      case "configure":
        return "Configure";
      case "verify":
        return "Verify";
      default:
        return action;
    }
  }

  function referenceLabel(kind: string): string {
    switch (kind) {
      case "events-api":
        return "Documentation";
      case "verify":
        return "Signing secrets";
      case "api":
        return "Documentation";
      case "auth":
        return "Authentication";
      case "sessions-v3":
        return "Sessions API";
      default:
        return kind;
    }
  }
</script>

<section class="setup-root">
  <button class="back-btn" onclick={onback}>← Back to Channels</button>

  <header class="setup-header">
    <h1>{connector.name} Setup</h1>
    <div class="header-meta">
      <span class="badge channel">{connector.channel}</span>
      <span class="badge version">v{connector.version}</span>
      <span class="badge trust">{connector.trust_tier}</span>
    </div>
    <p class="setup-desc">{connector.setup_summary ?? connector.description}</p>
  </header>

  <!-- Stepper -->
  {#if instructions.length > 0}
    <div class="stepper-section">
      <h2>Setup steps</h2>
      <div class="stepper-nav">
        {#each instructions as step, i}
          <button
            class="step-dot"
            class:active={currentStep === i}
            class:completed={currentStep > i}
            onclick={() => (currentStep = i)}
          >
            <span class="step-num">{i + 1}</span>
            <span class="step-label">{stepLabel(step.action)}</span>
          </button>
          {#if i < instructions.length - 1}
            <span class="step-line" class:done={currentStep > i}></span>
          {/if}
        {/each}
      </div>

      <div class="step-content">
        <p class="step-text">{currentInstruction.text}</p>

        <!-- open_url action -->
        {#if currentInstruction.action === "open_url" && currentInstruction.link}
          <a class="action-link" href={currentInstruction.link} target="_blank" rel="noopener">
            Open {connector.name} settings ↗
          </a>
        {/if}

        <!-- register_webhook action -->
        {#if currentInstruction.action === "register_webhook"}
          {#if connector.webhook?.receiver}
            <div class="webhook-receiver">
              <label for="webhook-receiver">{connector.webhook.receiver.label}</label>
              <div class="copyable-field">
                <code id="webhook-receiver" class="receiver-url">https://&lt;your-bicameral-host&gt;/webhook/{connector.id}</code>
                <button
                  class="copy-btn"
                  onclick={() =>
                    copyToClipboard(
                      `https://<your-bicameral-host>/webhook/${connector.id}`,
                      "receiver",
                    )}
                >
                  {copied["receiver"] ? "Copied" : "Copy"}
                </button>
              </div>
              <p class="field-hint">
                Replace &lt;your-bicameral-host&gt; with your operator-provisioned receiver address.
              </p>
            </div>
          {/if}
          {#if currentInstruction.link}
            <a class="action-link" href={currentInstruction.link} target="_blank" rel="noopener">
              Provider setup guide ↗
            </a>
          {/if}
        {/if}

        <!-- paste_secret action -->
        {#if currentInstruction.action === "paste_secret"}
          {@const cred = credentialForStep(currentInstruction)}
          {#if cred}
            <div class="credential-input">
              <label for="cred-{cred.key}">{cred.label}</label>
              <div class="masked-input-row">
                {#if secretVisible[cred.key]}
                  <input
                    id="cred-{cred.key}"
                    type="text"
                    bind:value={secretValues[cred.key]}
                    placeholder="Paste credential value"
                  />
                {:else}
                  <input
                    id="cred-{cred.key}"
                    type="password"
                    bind:value={secretValues[cred.key]}
                    placeholder="Paste credential value"
                  />
                {/if}
                <button
                  class="toggle-vis"
                  onclick={() =>
                    (secretVisible[cred.key] = !secretVisible[cred.key])}
                >
                  {secretVisible[cred.key] ? "Hide" : "Show"}
                </button>
              </div>
              {#if secretValues[cred.key] && validationError(cred, secretValues[cred.key])}
                <p class="validation-error">
                  This does not match the expected key format.
                </p>
              {/if}
              {#if secretValues[cred.key] && !validationError(cred, secretValues[cred.key])}
                <p class="field-preview">
                  Stored as: <code>{maskValue(secretValues[cred.key])}</code>
                </p>
              {/if}
              <p class="boundary-note">
                This value stays in your local setup path and is not saved to
                the Decision Ledger.
              </p>
            </div>
          {/if}
        {/if}

        <!-- configure action -->
        {#if currentInstruction.action === "configure"}
          {#each runtimeConfigEntries() as entry}
            <div class="config-input">
              <label for="cfg-{entry.key}">{entry.label}</label>
              <input
                id="cfg-{entry.key}"
                type="text"
                bind:value={configValues[entry.key]}
                placeholder={entry.description ?? entry.label}
              />
              {#if entry.description}
                <p class="field-hint">{entry.description}</p>
              {/if}
              {#if entry.required}
                <p class="field-hint required-hint">Required</p>
              {/if}
            </div>
          {/each}
        {/if}

        <!-- verify action -->
        {#if currentInstruction.action === "verify"}
          <div class="verify-section">
            <button
              class="verify-btn"
              class:pending={verifyState === "pending"}
              onclick={handleVerify}
              disabled={verifyState === "pending"}
            >
              {#if verifyState === "idle"}
                Test connection
              {:else if verifyState === "pending"}
                Testing…
              {:else}
                Test connection
              {/if}
            </button>
            {#if verifyState === "done"}
              <div class="verify-result">
                <p class="verify-note">
                  <strong>Local-only test:</strong> Live connection verification
                  still needs real credentials and a provisioned receiver. This
                  only confirms the setup form is complete.
                </p>
              </div>
            {/if}
            <p class="boundary-note">
              Live wiring is not enabled in this screen yet.
            </p>
          </div>
        {/if}

        <div class="step-nav">
          <button
            class="step-prev"
            disabled={currentStep === 0}
            onclick={() => (currentStep = Math.max(0, currentStep - 1))}
          >
            ← Previous
          </button>
          <button
            class="step-next"
            disabled={currentStep === instructions.length - 1}
            onclick={() =>
              (currentStep = Math.min(
                instructions.length - 1,
                currentStep + 1,
              ))}
          >
            Next →
          </button>
        </div>
      </div>
    </div>
  {/if}

  <!-- Configuration tips -->
  {#if connector.configuration_tips && connector.configuration_tips.length > 0}
    <div class="wire-gates-section">
      <h2>Configuration tips</h2>
      <ul class="wire-list">
        {#each connector.configuration_tips as tip}
          <li>{tip}</li>
        {/each}
      </ul>
    </div>
  {/if}

  <!-- References -->
  {#if connector.references && connector.references.length > 0}
    <div class="references-section">
      <h2>Resources</h2>
      <ul class="ref-list">
        {#each connector.references as ref}
          <li>
            <span class="ref-kind">{ref.label ?? referenceLabel(ref.kind)}</span>
            <a href={ref.url} target="_blank" rel="noopener">{ref.url}</a>
          </li>
        {/each}
      </ul>
    </div>
  {/if}

  <!-- Live readiness -->
  {#if connector.live_readiness}
    <div class="readiness-section">
      <h2>Status</h2>
      <p>{connector.live_readiness}</p>
    </div>
  {/if}
</section>

<style>
  .setup-root {
    max-width: 760px;
  }

  .back-btn {
    background: none;
    border: none;
    color: var(--accent-500);
    font-size: 0.8125rem;
    cursor: pointer;
    padding: 0;
    margin-bottom: 1rem;
    font-family: inherit;
  }

  .back-btn:hover {
    text-decoration: underline;
  }

  .setup-header {
    margin-bottom: 1.5rem;
  }

  .setup-header h1 {
    font-size: 1.25rem;
    font-weight: 700;
    color: var(--ink-900);
  }

  .header-meta {
    display: flex;
    gap: 0.5rem;
    margin-top: 0.375rem;
    margin-bottom: 0.375rem;
  }

  .badge {
    font-size: 0.6875rem;
    padding: 0.125rem 0.5rem;
    border-radius: 9999px;
    font-weight: 500;
  }

  .badge.channel {
    background: var(--warn-50);
    color: var(--warn-600);
  }

  .badge.version {
    background: var(--canvas-muted);
    color: var(--ink-700);
  }

  .badge.trust {
    background: var(--ok-50);
    color: var(--ok-600);
  }

  .setup-desc {
    font-size: 0.8125rem;
    color: var(--ink-700);
    line-height: 1.4;
  }

  /* Stepper */
  .stepper-section {
    margin-bottom: 1.5rem;
  }

  .stepper-section h2 {
    font-size: 0.875rem;
    font-weight: 600;
    color: var(--ink-900);
    margin-bottom: 0.75rem;
  }

  .stepper-nav {
    display: flex;
    align-items: center;
    gap: 0;
    margin-bottom: 1rem;
  }

  .step-dot {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    padding: 0.375rem 0.75rem;
    border: 1px solid var(--line);
    border-radius: 9999px;
    background: #fff;
    font-family: inherit;
    font-size: 0.75rem;
    color: var(--ink-500);
    cursor: pointer;
    transition: all 0.15s;
    white-space: nowrap;
  }

  .step-dot:hover {
    border-color: var(--accent-200);
  }

  .step-dot.active {
    border-color: var(--accent-500);
    background: var(--accent-50);
    color: var(--accent-500);
    font-weight: 600;
  }

  .step-dot.completed {
    border-color: var(--ok-500);
    color: var(--ok-600);
    background: var(--ok-50);
  }

  .step-num {
    width: 1.25rem;
    height: 1.25rem;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    font-size: 0.6875rem;
    font-weight: 600;
    background: var(--canvas-muted);
  }

  .step-dot.active .step-num {
    background: var(--accent-500);
    color: #fff;
  }

  .step-dot.completed .step-num {
    background: var(--ok-500);
    color: #fff;
  }

  .step-line {
    width: 1.5rem;
    height: 2px;
    background: var(--line);
    flex-shrink: 0;
  }

  .step-line.done {
    background: var(--ok-500);
  }

  .step-content {
    padding: 1rem 1.25rem;
    border: 1px solid var(--line);
    border-radius: 0.5rem;
    background: #fff;
  }

  .step-text {
    font-size: 0.8125rem;
    color: var(--ink-900);
    line-height: 1.5;
    margin-bottom: 0.75rem;
  }

  .action-link {
    display: inline-block;
    font-size: 0.8125rem;
    color: var(--accent-500);
    text-decoration: none;
    padding: 0.375rem 0.75rem;
    border: 1px solid var(--accent-500);
    border-radius: 0.375rem;
    margin-bottom: 0.5rem;
    transition: all 0.15s;
  }

  .action-link:hover {
    background: var(--accent-50);
  }

  /* Webhook receiver */
  .webhook-receiver {
    margin-bottom: 0.75rem;
  }

  .webhook-receiver label {
    display: block;
    font-size: 0.75rem;
    font-weight: 500;
    color: var(--ink-700);
    margin-bottom: 0.25rem;
  }

  .copyable-field {
    display: flex;
    gap: 0.5rem;
    align-items: center;
  }

  .receiver-url {
    flex: 1;
    font-size: 0.75rem;
    padding: 0.5rem;
    background: var(--canvas-muted);
    border: 1px solid var(--line);
    border-radius: 0.375rem;
    overflow-x: auto;
    white-space: nowrap;
  }

  .copy-btn {
    font-size: 0.75rem;
    padding: 0.375rem 0.75rem;
    border: 1px solid var(--line);
    border-radius: 0.375rem;
    background: #fff;
    cursor: pointer;
    font-family: inherit;
    transition: all 0.15s;
    white-space: nowrap;
  }

  .copy-btn:hover {
    background: var(--canvas-muted);
  }

  /* Credential input */
  .credential-input,
  .config-input {
    margin-bottom: 0.75rem;
  }

  .credential-input label,
  .config-input label {
    display: block;
    font-size: 0.75rem;
    font-weight: 500;
    color: var(--ink-700);
    margin-bottom: 0.25rem;
  }

  .masked-input-row {
    display: flex;
    gap: 0.5rem;
  }

  .masked-input-row input,
  .config-input input {
    flex: 1;
    font-size: 0.8125rem;
    padding: 0.5rem;
    border: 1px solid var(--line);
    border-radius: 0.375rem;
    font-family: inherit;
    background: #fff;
  }

  .masked-input-row input:focus,
  .config-input input:focus {
    outline: none;
    border-color: var(--accent-500);
    box-shadow: 0 0 0 2px rgba(8, 145, 178, 0.1);
  }

  .toggle-vis {
    font-size: 0.75rem;
    padding: 0.375rem 0.625rem;
    border: 1px solid var(--line);
    border-radius: 0.375rem;
    background: #fff;
    cursor: pointer;
    font-family: inherit;
    white-space: nowrap;
  }

  .toggle-vis:hover {
    background: var(--canvas-muted);
  }

  .field-hint {
    font-size: 0.6875rem;
    color: var(--ink-500);
    margin-top: 0.25rem;
    line-height: 1.4;
  }

  .required-hint {
    color: var(--danger-600);
  }

  .field-preview {
    font-size: 0.6875rem;
    color: var(--ok-600);
    margin-top: 0.25rem;
  }

  .field-preview code {
    font-size: 0.65rem;
    background: var(--ok-50);
    padding: 0.0625rem 0.25rem;
    border-radius: 2px;
  }

  .validation-error {
    font-size: 0.6875rem;
    color: var(--danger-600);
    margin-top: 0.25rem;
  }

  .boundary-note {
    font-size: 0.6875rem;
    color: var(--ink-500);
    font-style: italic;
    line-height: 1.4;
    margin-top: 0.5rem;
  }

  /* Verify */
  .verify-section {
    margin-bottom: 0.5rem;
  }

  .verify-btn {
    font-size: 0.8125rem;
    padding: 0.5rem 1rem;
    border: 1px solid var(--accent-500);
    border-radius: 0.375rem;
    background: var(--accent-500);
    color: #fff;
    cursor: pointer;
    font-family: inherit;
    font-weight: 500;
    transition: all 0.15s;
  }

  .verify-btn:hover:not(:disabled) {
    background: var(--accent-600);
  }

  .verify-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .verify-btn.pending {
    background: var(--ink-500);
    border-color: var(--ink-500);
  }

  .verify-result {
    margin-top: 0.75rem;
    padding: 0.75rem;
    background: var(--warn-50);
    border: 1px solid #fde68a;
    border-radius: 0.375rem;
  }

  .verify-note {
    font-size: 0.75rem;
    color: var(--warn-600);
    line-height: 1.5;
  }

  /* Step nav */
  .step-nav {
    display: flex;
    justify-content: space-between;
    margin-top: 1rem;
    padding-top: 0.75rem;
    border-top: 1px solid var(--canvas-muted);
  }

  .step-prev,
  .step-next {
    font-size: 0.8125rem;
    padding: 0.375rem 0.75rem;
    border: 1px solid var(--line);
    border-radius: 0.375rem;
    background: #fff;
    cursor: pointer;
    font-family: inherit;
    transition: all 0.15s;
  }

  .step-prev:hover:not(:disabled),
  .step-next:hover:not(:disabled) {
    background: var(--canvas-muted);
  }

  .step-prev:disabled,
  .step-next:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  /* Wire gates */
  .wire-gates-section {
    margin-bottom: 1.5rem;
  }

  .wire-gates-section h2 {
    font-size: 0.875rem;
    font-weight: 600;
    color: var(--ink-900);
    margin-bottom: 0.5rem;
  }

  .wire-list {
    list-style: none;
    padding: 0;
  }

  .wire-list li {
    font-size: 0.75rem;
    color: var(--ink-700);
    padding: 0.5rem 0.75rem;
    border-left: 3px solid var(--line);
    margin-bottom: 0.5rem;
    line-height: 1.5;
    background: var(--canvas-muted);
  }

  /* References */
  .references-section {
    margin-bottom: 1.5rem;
  }

  .references-section h2 {
    font-size: 0.875rem;
    font-weight: 600;
    color: var(--ink-900);
    margin-bottom: 0.5rem;
  }

  .ref-list {
    list-style: none;
    padding: 0;
  }

  .ref-list li {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.75rem;
    padding: 0.25rem 0;
  }

  .ref-kind {
    font-size: 0.6875rem;
    font-weight: 500;
    color: var(--ink-500);
    background: var(--canvas-muted);
    padding: 0.0625rem 0.375rem;
    border-radius: 3px;
    white-space: nowrap;
  }

  .ref-list a {
    color: var(--accent-500);
    text-decoration: none;
    word-break: break-all;
  }

  .ref-list a:hover {
    text-decoration: underline;
  }

  /* Live readiness */
  .readiness-section {
    padding: 0.75rem 1rem;
    background: var(--accent-50);
    border: 1px solid var(--accent-200);
    border-radius: 0.5rem;
    margin-bottom: 1.5rem;
  }

  .readiness-section h2 {
    font-size: 0.875rem;
    font-weight: 600;
    color: var(--accent-700);
    margin-bottom: 0.375rem;
  }

  .readiness-section p {
    font-size: 0.75rem;
    color: var(--accent-600);
    line-height: 1.5;
  }
</style>
