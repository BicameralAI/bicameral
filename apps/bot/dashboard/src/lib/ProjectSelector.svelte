<script lang="ts">
  import type { ProjectEntry } from "./project-fixtures";
  import { ACCESS_FAILURE_COPY } from "./project-fixtures";

  type Props = {
    projects?: ProjectEntry[];
    selectedId?: string | null;
    onselect?: (project: ProjectEntry) => void;
    oncreate?: () => void;
    onmanage?: () => void;
  };

  let {
    projects = [],
    selectedId = $bindable<string | null>(null),
    onselect,
    oncreate,
    onmanage,
  }: Props = $props();

  let open = $state(false);

  const selected = $derived(
    projects.find((p) => p.id === selectedId) ?? projects[0] ?? null,
  );

  function toggle() {
    open = !open;
  }

  function handleSelect(project: ProjectEntry) {
    selectedId = project.id;
    open = false;
    onselect?.(project);
  }

  function handleCreate() {
    open = false;
    oncreate?.();
  }

  function handleManage() {
    open = false;
    onmanage?.();
  }

  function readinessLabel(project: ProjectEntry): string {
    if (project.readiness === "ready") return "Ready";
    if (project.readiness === "offline") return "Offline";
    if (project.access_failure) {
      return ACCESS_FAILURE_COPY[project.access_failure];
    }
    return "Needs access";
  }

  function readinessClass(project: ProjectEntry): string {
    if (project.readiness === "ready") return "ready";
    if (project.readiness === "offline") return "offline";
    return "needs-access";
  }
</script>

<div class="project-selector" class:open>
  <button class="selector-trigger" onclick={toggle}>
    <span class="selector-label">Project:</span>
    <span class="selector-value">
      {selected?.name ?? "Select project"}
    </span>
    <span class="selector-chevron">{open ? "▲" : "▼"}</span>
  </button>

  {#if open}
    <div class="selector-dropdown">
      <ul class="project-list">
        {#each projects as project (project.id)}
          <li>
            <button
              class="project-item"
              class:active={project.id === selectedId}
              onclick={() => handleSelect(project)}
            >
              <span class="project-name">{project.name}</span>
              <span class="project-store">{project.event_store_label}</span>
              <span class="project-readiness {readinessClass(project)}">
                {readinessLabel(project)}
              </span>
            </button>
          </li>
        {/each}
      </ul>
      <div class="selector-actions">
        <button class="action-item" onclick={handleCreate}>
          Create project
        </button>
        <button class="action-item" onclick={handleManage}>
          Manage projects
        </button>
      </div>
    </div>
  {/if}
</div>

<style>
  .project-selector {
    position: relative;
  }

  .selector-trigger {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    padding: 0.375rem 0.625rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: #fff;
    font-family: inherit;
    font-size: 0.8125rem;
    color: var(--ink-900);
    cursor: pointer;
    transition: border-color 0.12s;
  }

  .selector-trigger:hover {
    border-color: var(--ink-400);
  }

  .project-selector.open .selector-trigger {
    border-color: var(--accent-500);
  }

  .selector-label {
    color: var(--ink-500);
    font-weight: 500;
  }

  .selector-value {
    font-weight: 600;
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .selector-chevron {
    font-size: 0.625rem;
    color: var(--ink-500);
    margin-left: 0.25rem;
  }

  .selector-dropdown {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    min-width: 320px;
    background: #fff;
    border: 1px solid var(--line);
    border-radius: 8px;
    box-shadow:
      0 4px 12px rgba(0, 0, 0, 0.08),
      0 1px 3px rgba(0, 0, 0, 0.04);
    z-index: 1000;
    overflow: hidden;
  }

  .project-list {
    list-style: none;
    padding: 0.375rem 0;
    margin: 0;
  }

  .project-item {
    display: grid;
    grid-template-columns: 1fr auto auto;
    gap: 0.75rem;
    align-items: center;
    width: 100%;
    padding: 0.5rem 0.75rem;
    border: none;
    background: none;
    font-family: inherit;
    font-size: 0.8125rem;
    color: var(--ink-900);
    cursor: pointer;
    text-align: left;
    transition: background 0.1s;
  }

  .project-item:hover {
    background: var(--canvas-muted);
  }

  .project-item.active {
    background: var(--accent-50);
  }

  .project-name {
    font-weight: 500;
  }

  .project-store {
    color: var(--ink-500);
    font-size: 0.75rem;
  }

  .project-readiness {
    font-size: 0.6875rem;
    font-weight: 600;
    padding: 0.125rem 0.5rem;
    border-radius: 9999px;
  }

  .project-readiness.ready {
    color: var(--ok-600);
    background: var(--ok-50);
  }

  .project-readiness.needs-access {
    color: var(--warn-600);
    background: var(--warn-50);
  }

  .project-readiness.offline {
    color: var(--ink-500);
    background: var(--canvas-muted);
  }

  .selector-actions {
    border-top: 1px solid var(--line);
    padding: 0.375rem 0;
  }

  .action-item {
    display: block;
    width: 100%;
    padding: 0.5rem 0.75rem;
    border: none;
    background: none;
    font-family: inherit;
    font-size: 0.8125rem;
    color: var(--accent-500);
    cursor: pointer;
    text-align: left;
    transition: background 0.1s;
  }

  .action-item:hover {
    background: var(--accent-50);
  }
</style>
