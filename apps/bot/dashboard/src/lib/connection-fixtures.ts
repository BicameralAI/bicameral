/**
 * Fixture types and sample data for Connection, SourceBinding,
 * and ToolSubmission.
 *
 * These contracts let downstream dashboard slices (User Settings,
 * Project Settings, Channels, Activity) develop against stable
 * shapes without live provider discovery or real OAuth.
 *
 * Boundary: fixtures carry display/metadata only — no tokens,
 * secrets, or credential material.
 *
 * Spec refs:
 *   docs/specs/oauth-and-integration-connections.md
 *   docs/specs/project-setup-ui.md
 */

import type { ConnectionStatus, AuthMethod, ProjectionSetupState, ProjectionSetupDetail } from "./types";

// ── Connection ───────────────────────────────────────────────────

export type Connection = {
  provider_id: string;
  status: ConnectionStatus;
  account_label?: string;
  capabilities: string[];
  updated_at?: string;
  auth_method?: AuthMethod;
};

// ── SourceBinding ────────────────────────────────────────────────

export type SourceBindingStatus = "healthy" | "action_needed" | "paused" | "revoked";

export type SourceBindingMode = "active" | "passive" | "webhook";

export type SourceBindingFilter = {
  pull_requests?: boolean;
  issues?: boolean;
  commits?: boolean;
  docs?: boolean;
};

export type SourceBinding = {
  id: string;
  provider: string;
  resource: {
    kind: string;
    identifier: string;
    display_label: string;
  };
  ingestion_owner: {
    operator_id: string;
    connection_id: string;
  };
  approved_by: string;
  status: SourceBindingStatus;
  mode: SourceBindingMode;
  filters: SourceBindingFilter;
  dedupe_key: string;
  projection_setup?: ProjectionSetupDetail;
};

// ── ToolSubmission ───────────────────────────────────────────────

export type ToolSurface = "mcp" | "cli" | "agent" | "browser_extension";

export type ToolSubmission = {
  id: string;
  submitted_by: {
    operator_id: string;
    local_actor_label: string;
  };
  surface: {
    kind: ToolSurface;
    tool: string;
  };
  session_uri: string;
  source_uri: string;
  evidence: {
    tool_call_id: string;
    excerpt: string;
  };
  submitted_at: string;
};

// ── DiscoveredProject ────────────────────────────────────────────

export type EventStoreKind = "github" | "google_drive";

export type ProjectReadiness = "ready" | "needs_access" | "incomplete" | "offline";

export type DiscoveredProject = {
  id: string;
  name: string;
  event_store: EventStoreKind;
  provider_resource: string;
  readiness: ProjectReadiness;
  last_updated?: string;
};

// ── Sample data ──────────────────────────────────────────────────

export const SAMPLE_CONNECTIONS: Connection[] = [
  {
    provider_id: "github",
    status: "connected",
    account_label: "jinhongkuan",
    capabilities: ["github.repo.read", "github.repo.write"],
    updated_at: "2026-06-13T00:00:00Z",
    auth_method: "oauth",
  },
  {
    provider_id: "google",
    status: "action_needed",
    account_label: "user@example.com",
    capabilities: ["drive.file", "drive.docs.read"],
    updated_at: "2026-06-10T00:00:00Z",
    auth_method: "oauth",
  },
];

export const SAMPLE_SOURCE_BINDINGS: SourceBinding[] = [
  {
    id: "sb_github_repo_primary",
    provider: "github",
    resource: {
      kind: "repo",
      identifier: "acme/checkout",
      display_label: "acme/checkout",
    },
    ingestion_owner: {
      operator_id: "priya",
      connection_id: "connection.github.priya",
    },
    approved_by: "priya",
    status: "healthy",
    mode: "active",
    filters: {
      pull_requests: true,
      issues: true,
      commits: false,
    },
    dedupe_key: "github:repo:acme/checkout",
    projection_setup: {
      state: "configured",
      provider_id: "local_cli",
      message: "Candidate projection is configured and operational.",
    },
  },
  {
    id: "sb_google_drive_docs",
    provider: "google",
    resource: {
      kind: "folder",
      identifier: "Bicameral Bot Events",
      display_label: "Bicameral Bot Events",
    },
    ingestion_owner: {
      operator_id: "priya",
      connection_id: "connection.google.priya",
    },
    approved_by: "priya",
    status: "action_needed",
    mode: "passive",
    filters: {
      docs: true,
    },
    dedupe_key: "google:folder:bicameral-bot-events",
    projection_setup: {
      state: "provider_not_configured",
      message: "Candidate projection is not configured.",
      repair_hint: "Configure a reasoning provider in Settings to enable candidate projection for this source.",
    },
  },
  {
    id: "sb_linear_issues",
    provider: "linear",
    resource: {
      kind: "team",
      identifier: "ENG",
      display_label: "Engineering",
    },
    ingestion_owner: {
      operator_id: "alex",
      connection_id: "connection.linear.alex",
    },
    approved_by: "alex",
    status: "healthy",
    mode: "webhook",
    filters: {
      issues: true,
    },
    dedupe_key: "linear:team:ENG",
    projection_setup: {
      state: "provider_unavailable",
      provider_id: "local_cli",
      message: "The configured projection provider is unavailable.",
      repair_hint: "Check that the local CLI reasoning provider is running and accessible.",
    },
  },
];

export const SAMPLE_PROJECTION_SETUP: ProjectionSetupDetail[] = [
  {
    state: "configured",
    provider_id: "local_cli",
    message: "Candidate projection is configured and operational.",
  },
  {
    state: "projection_deferred",
    provider_id: "local_cli",
    message: "Evidence was captured but no reviewable candidate was created yet.",
    repair_hint: "The projection provider received evidence but deferred candidate creation. Check provider logs for details.",
  },
  {
    state: "provider_not_configured",
    message: "Candidate projection is not configured.",
    repair_hint: "Configure a reasoning provider in Settings to enable candidate projection.",
  },
  {
    state: "provider_unavailable",
    provider_id: "local_cli",
    message: "The configured projection provider is unavailable.",
    repair_hint: "Check that the local CLI reasoning provider is running and accessible.",
  },
];

export const SAMPLE_DISCOVERED_PROJECTS: DiscoveredProject[] = [
  {
    id: "proj_acme_checkout",
    name: "Acme Checkout",
    event_store: "github",
    provider_resource: "acme/checkout",
    readiness: "ready",
    last_updated: "2026-06-12T18:30:00Z",
  },
  {
    id: "proj_bicameral_bot",
    name: "Bicameral Bot",
    event_store: "google_drive",
    provider_resource: "Bicameral Bot Events",
    readiness: "needs_access",
    last_updated: "2026-06-10T09:00:00Z",
  },
  {
    id: "proj_factory_roadmap",
    name: "Factory Roadmap",
    event_store: "github",
    provider_resource: "bicameral/factory-roadmap",
    readiness: "offline",
  },
];

export const SAMPLE_TOOL_SUBMISSIONS: ToolSubmission[] = [
  {
    id: "ts_mcp_001",
    submitted_by: {
      operator_id: "alex",
      local_actor_label: "alex-macbook",
    },
    surface: {
      kind: "mcp",
      tool: "claude",
    },
    session_uri: "mcp://local/sess_123",
    source_uri: "mcp://local/sess_123",
    evidence: {
      tool_call_id: "call_456",
      excerpt: "Refactored auth module to use connection registry pattern",
    },
    submitted_at: "2026-06-12T14:30:00Z",
  },
];
