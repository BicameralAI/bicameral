<script lang="ts">
  /**
   * Workspace Binding Gate — fetches the daemon-owned workspace readiness
   * projection (#733) for a selected hosted project, renders the binding CTA /
   * bound / repair-required / unsupported card, and polls the hosted bridge
   * until the daemon reports the project bound (bicameral-bot#734).
   *
   * Renderer-only: it consumes existing daemon-owned state and introduces no
   * new durable UI state beyond the loading / error / polling presentation
   * handled here. It never binds a workspace; binding happens on a LOCAL
   * surface (MCP / CLI). No local absolute path or secret is ever rendered.
   *
   * Issue: #734
   */
  import { untrack } from "svelte";
  import type { ProjectEntry } from "../lib/project-fixtures";
  import type { HostedDashboardCompatibility } from "../lib/hosted-bridge-types";
  import {
    loadWorkspaceBindingView,
    shouldPoll,
    type WorkspaceBindingView,
    type LoadWorkspaceReadinessOptions,
  } from "../lib/workspace-binding-readiness";
  import WorkspaceBindingCard from "./WorkspaceBindingCard.svelte";

  type Props = {
    project: ProjectEntry;
    /** Bridge session token for the paired daemon. */
    sessionToken?: string;
    /** Daemon protocol compatibility (used to fail closed when unsupported). */
    compatibility?: HostedDashboardCompatibility;
    /** Poll interval in ms while the project is unbound. Default 5000. */
    pollIntervalMs?: number;
    /**
     * Injectable readiness loader (defaults to the live daemon fetch). Tests
     * pass a stub so polling behavior is deterministic without a network.
     */
    load?: (opts: LoadWorkspaceReadinessOptions) => Promise<WorkspaceBindingView>;
    onback?: () => void;
    /** Fired when readiness first resolves to bound. */
    onbound?: () => void;
  };

  let {
    project,
    sessionToken,
    compatibility,
    pollIntervalMs = 5000,
    load = loadWorkspaceBindingView,
    onback,
    onbound,
  }: Props = $props();

  let view = $state<WorkspaceBindingView>({ kind: "loading" });
  let polling = $state(false);
  let boundNotified = false;

  async function refresh(): Promise<void> {
    if (!sessionToken) {
      // No paired session token means readiness cannot be resolved. Surface a
      // neutral, retryable error rather than pretending the project is bound.
      view = {
        kind: "error",
        message:
          "No active hosted bridge pairing; reconnect to check workspace " +
          "readiness.",
        retryable: true,
      };
      return;
    }
    const next = await load({
      sessionToken,
      projectId: project.id,
      teamId: project.team_id,
      displayName: project.name,
      compatibility,
      polling,
    });
    view = next;
    if (next.kind === "bound" && !boundNotified) {
      boundNotified = true;
      onbound?.();
    }
  }

  // Fetch on mount and whenever the selected project or session changes, then
  // poll while the project remains unbound. Cleanup clears the interval. The
  // body runs untracked so writing presentation state (view/polling) never
  // re-triggers this effect — it re-runs only when the identity inputs change.
  $effect(() => {
    // Track the identity inputs so the effect re-runs on change.
    void project.id;
    void sessionToken;

    return untrack(() => {
      boundNotified = false;
      polling = true;
      view = { kind: "loading" };

      let cancelled = false;
      const tick = async () => {
        if (cancelled) return;
        await refresh();
        if (cancelled) return;
        if (!shouldPoll(view)) {
          polling = false;
          clearInterval(timer);
        }
      };
      void tick();
      const timer = setInterval(() => void tick(), pollIntervalMs);

      return () => {
        cancelled = true;
        polling = false;
        clearInterval(timer);
      };
    });
  });
</script>

<div class="binding-gate" data-testid="workspace-binding-gate">
  <div class="gate-header">
    {#if onback}
      <button class="back-link" onclick={onback} data-testid="binding-back">
        ← Back to projects
      </button>
    {/if}
    <span class="project-name" data-testid="binding-project-name">{project.name}</span>
  </div>

  <WorkspaceBindingCard
    {view}
    onretry={() => void refresh()}
    onrefresh={() => void refresh()}
    onopen={onbound}
  />
</div>

<style>
  .binding-gate {
    flex: 1;
    padding: 2rem 1.25rem;
    overflow: auto;
  }

  .gate-header {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    max-width: 560px;
    margin: 0 auto 1rem;
  }

  .back-link {
    padding: 0;
    border: none;
    background: none;
    font-family: inherit;
    font-size: 0.8125rem;
    color: var(--ink-500);
    cursor: pointer;
  }

  .back-link:hover {
    color: var(--ink-900);
  }

  .project-name {
    font-size: 0.9375rem;
    font-weight: 600;
    color: var(--ink-900);
  }
</style>
