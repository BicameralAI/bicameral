/**
 * Provider setup fixtures for the inline provider-connect flow
 * in the project create screen.
 *
 * These fixtures model:
 * - Provider connection states (connected, not-connected, action-needed, unsupported)
 * - Provider resource readiness (shared-drive selection, GitHub App install)
 * - Reconnect-after-error states
 *
 * Spec refs:
 *   docs/specs/project-setup-ui.md (inline provider connect)
 *   #466 acceptance criteria
 *
 * Boundary: fixtures carry display/metadata only — no tokens, secrets,
 * or credential material. Live provider discovery and broker flows are
 * non-goals (use mocked descriptors/readiness states).
 */

import type { ConnectionStatus } from "./types";

// ── Provider resource readiness ──────────────────────────────────

export type ProviderResourceState =
  | "ready"
  | "pending_selection"
  | "not_ready"
  | "action_needed"
  | "unsupported";

export type SharedDriveEntry = {
  id: string;
  name: string;
  domain: string;
};

export type GitHubAppReadiness = {
  state: ProviderResourceState;
  installation_id?: string;
  org_or_user?: string;
  message: string;
};

export type GoogleDriveReadiness = {
  state: ProviderResourceState;
  eligible_shared_drives: SharedDriveEntry[];
  selected_drive_id?: string;
  project_path?: string;
  domain_valid: boolean;
  message: string;
};

export type ProviderSetupState = {
  provider_id: string;
  connection_status: ConnectionStatus;
  resource_readiness: GitHubAppReadiness | GoogleDriveReadiness;
  error_message?: string;
};

// ── GitHub App fixtures ──────────────────────────────────────────

export const GITHUB_NOT_CONNECTED: ProviderSetupState = {
  provider_id: "github",
  connection_status: "not_connected",
  resource_readiness: {
    state: "not_ready",
    message: "Install the Bicameral GitHub App to continue.",
  } as GitHubAppReadiness,
};

export const GITHUB_CONNECTED_READY: ProviderSetupState = {
  provider_id: "github",
  connection_status: "connected",
  resource_readiness: {
    state: "ready",
    installation_id: "inst_12345",
    org_or_user: "acme",
    message: "Bicameral GitHub App installed for acme.",
  } as GitHubAppReadiness,
};

export const GITHUB_ACTION_NEEDED: ProviderSetupState = {
  provider_id: "github",
  connection_status: "action_needed",
  resource_readiness: {
    state: "action_needed",
    message: "GitHub App permissions need to be updated. Reinstall the app.",
  } as GitHubAppReadiness,
  error_message: "The Bicameral GitHub App requires updated permissions.",
};

export const GITHUB_RECONNECT_AFTER_ERROR: ProviderSetupState = {
  provider_id: "github",
  connection_status: "action_needed",
  resource_readiness: {
    state: "not_ready",
    message: "Connection interrupted. Install the Bicameral GitHub App to retry.",
  } as GitHubAppReadiness,
  error_message: "Connection was interrupted. Please try again.",
};

export const GITHUB_UNSUPPORTED: ProviderSetupState = {
  provider_id: "github",
  connection_status: "unsupported",
  resource_readiness: {
    state: "unsupported",
    message: "GitHub event store is not available in this environment.",
  } as GitHubAppReadiness,
};

// ── Google Drive fixtures ────────────────────────────────────────

export const DRIVE_NOT_CONNECTED: ProviderSetupState = {
  provider_id: "google",
  connection_status: "not_connected",
  resource_readiness: {
    state: "not_ready",
    eligible_shared_drives: [],
    domain_valid: false,
    message: "Connect Google Drive to continue.",
  } as GoogleDriveReadiness,
};

export const DRIVE_CONNECTED_SINGLE_DRIVE: ProviderSetupState = {
  provider_id: "google",
  connection_status: "connected",
  resource_readiness: {
    state: "ready",
    eligible_shared_drives: [
      { id: "drive_001", name: "Engineering", domain: "acme.com" },
    ],
    selected_drive_id: "drive_001",
    project_path: "Engineering/.bicameral/acme-checkout/",
    domain_valid: true,
    message: "Shared drive auto-selected.",
  } as GoogleDriveReadiness,
};

export const DRIVE_CONNECTED_MULTIPLE_DRIVES: ProviderSetupState = {
  provider_id: "google",
  connection_status: "connected",
  resource_readiness: {
    state: "pending_selection",
    eligible_shared_drives: [
      { id: "drive_001", name: "Engineering", domain: "acme.com" },
      { id: "drive_002", name: "Product", domain: "acme.com" },
      { id: "drive_003", name: "Design", domain: "acme.com" },
    ],
    domain_valid: true,
    message: "Select a shared drive for the project event store.",
  } as GoogleDriveReadiness,
};

export const DRIVE_INVALID_DOMAIN: ProviderSetupState = {
  provider_id: "google",
  connection_status: "connected",
  resource_readiness: {
    state: "action_needed",
    eligible_shared_drives: [],
    domain_valid: false,
    message: "Team projects require a Google Workspace account with an allowed company domain. Consumer Gmail accounts are not supported.",
  } as GoogleDriveReadiness,
};

export const DRIVE_ACTION_NEEDED: ProviderSetupState = {
  provider_id: "google",
  connection_status: "action_needed",
  resource_readiness: {
    state: "action_needed",
    eligible_shared_drives: [],
    domain_valid: true,
    message: "Google Drive access has expired. Reconnect to continue.",
  } as GoogleDriveReadiness,
  error_message: "Drive access has expired. Please reconnect.",
};

export const DRIVE_RECONNECT_AFTER_ERROR: ProviderSetupState = {
  provider_id: "google",
  connection_status: "action_needed",
  resource_readiness: {
    state: "not_ready",
    eligible_shared_drives: [],
    domain_valid: true,
    message: "Connection interrupted. Connect Google Drive to retry.",
  } as GoogleDriveReadiness,
  error_message: "Connection was interrupted. Please try again.",
};

export const DRIVE_UNSUPPORTED: ProviderSetupState = {
  provider_id: "google",
  connection_status: "unsupported",
  resource_readiness: {
    state: "unsupported",
    eligible_shared_drives: [],
    domain_valid: false,
    message: "Google Drive event store is not available in this environment.",
  } as GoogleDriveReadiness,
};

// ── Composite fixture sets for test scenarios ────────────────────

export const SETUP_FIXTURES = {
  github: {
    not_connected: GITHUB_NOT_CONNECTED,
    connected_ready: GITHUB_CONNECTED_READY,
    action_needed: GITHUB_ACTION_NEEDED,
    reconnect_after_error: GITHUB_RECONNECT_AFTER_ERROR,
    unsupported: GITHUB_UNSUPPORTED,
  },
  google_drive: {
    not_connected: DRIVE_NOT_CONNECTED,
    connected_single_drive: DRIVE_CONNECTED_SINGLE_DRIVE,
    connected_multiple_drives: DRIVE_CONNECTED_MULTIPLE_DRIVES,
    invalid_domain: DRIVE_INVALID_DOMAIN,
    action_needed: DRIVE_ACTION_NEEDED,
    reconnect_after_error: DRIVE_RECONNECT_AFTER_ERROR,
    unsupported: DRIVE_UNSUPPORTED,
  },
} as const;
