<script lang="ts">
  /**
   * Hosted-dashboard daemon protocol compatibility notice.
   *
   * Renders the non-`current` compatibility states keyed on
   * `daemon_protocol_version` (#697 / #711):
   *
   * - `upgrade_required` — daemon 2+ breaking protocol versions behind. Shown as
   *   a blocking notice with a ONE-ACTION repair path: a copy-paste update
   *   command (or auto-update button when the daemon supports it).
   * - `forward_skew` — daemon newer than this dashboard expects. Shown as a
   *   benign "dashboard catching up" warning; reads still render, controls hide.
   * - `supported_previous` — daemon one version behind (N-1). Shown as a soft
   *   update-when-convenient notice; the dashboard remains fully usable.
   *
   * Renderer-only (ADR-0025): this component computes no authority and displays
   * only protocol-version integers + generic copy — never workspace, ledger,
   * source, evidence, or local-path data.
   *
   * Issue: #711
   */
  import type { HostedDashboardCompatibility } from "../lib/hosted-bridge-types";

  type Props = {
    compatibility: HostedDashboardCompatibility;
    onretry?: () => void;
  };

  let { compatibility, onretry }: Props = $props();

  let copied = $state(false);

  async function copyCommand() {
    const cmd = compatibility.repair?.update_command;
    if (!cmd) return;
    try {
      await navigator.clipboard.writeText(cmd);
      copied = true;
      setTimeout(() => (copied = false), 2000);
    } catch {
      copied = false;
    }
  }
</script>

<div
  class="compat-notice compat-{compatibility.state}"
  data-testid="hosted-compat-notice"
  data-compat-state={compatibility.state}
  role="status"
>
  {#if compatibility.state === "hosted_dashboard_upgrade_required"}
    <h2 data-testid="compat-upgrade-required-title">Update required</h2>
    <p class="compat-detail">
      {compatibility.note ??
        "The local daemon is too far behind to use with the hosted dashboard."}
    </p>

    {#if compatibility.repair}
      <div class="repair" data-testid="compat-repair">
        <p class="repair-instruction">{compatibility.repair.instruction}</p>

        {#if compatibility.repair.auto_update_available}
          <button
            class="repair-action"
            data-testid="compat-auto-update"
            onclick={onretry}
          >
            Update daemon
          </button>
        {:else}
          <div class="command-row">
            <code data-testid="compat-update-command"
              >{compatibility.repair.update_command}</code
            >
            <button
              class="repair-action"
              data-testid="compat-copy-command"
              onclick={copyCommand}
            >
              {copied ? "Copied" : "Copy command"}
            </button>
          </div>
        {/if}

        <button
          class="retry-link"
          data-testid="compat-reconnect"
          onclick={onretry}
        >
          Reconnect after updating
        </button>
      </div>
    {/if}
  {:else if compatibility.state === "hosted_dashboard_forward_skew"}
    <h2 data-testid="compat-forward-skew-title">Dashboard is catching up</h2>
    <p class="compat-detail">
      {compatibility.note ??
        "The local daemon is newer than this dashboard expects."}
    </p>
    <p class="compat-subtle">
      Your data is still readable below. Some controls are hidden until the
      dashboard updates to match your daemon.
    </p>
  {:else if compatibility.state === "supported_previous"}
    <p class="compat-detail" data-testid="compat-supported-previous">
      {compatibility.note ??
        "The local daemon is one protocol version behind but still supported."}
    </p>
  {/if}
</div>

<style>
  .compat-notice {
    padding: 1.25rem 1.5rem;
    border-radius: 8px;
    margin: 1rem;
  }

  .compat-hosted_dashboard_upgrade_required {
    background: #fff4f4;
    border: 1px solid #e0a0a0;
  }

  .compat-hosted_dashboard_forward_skew {
    background: #fff9ec;
    border: 1px solid #e0c98a;
  }

  .compat-supported_previous {
    background: #f2f7ff;
    border: 1px solid #a9c4ec;
    padding: 0.75rem 1rem;
  }

  h2 {
    margin: 0 0 0.5rem;
    font-size: 1.1rem;
  }

  .compat-detail {
    margin: 0 0 0.75rem;
  }

  .compat-subtle {
    margin: 0;
    color: #555;
    font-size: 0.9rem;
  }

  .command-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin: 0.5rem 0;
  }

  code {
    background: #1e1e1e;
    color: #f5f5f5;
    padding: 0.35rem 0.6rem;
    border-radius: 4px;
    font-family: ui-monospace, monospace;
  }

  .repair-action {
    cursor: pointer;
    padding: 0.35rem 0.75rem;
    border: 1px solid #888;
    border-radius: 4px;
    background: #fff;
  }

  .retry-link {
    display: inline-block;
    margin-top: 0.5rem;
    background: none;
    border: none;
    color: #2258c8;
    cursor: pointer;
    padding: 0;
    text-decoration: underline;
  }
</style>
