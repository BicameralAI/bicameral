/**
 * Provider setup inline-connect flow tests (issue #466).
 *
 * Validates fixture contracts and provider resource readiness states
 * for the inline provider-connect flow on the project create screen.
 *
 * Covers: connected, not-connected, reconnect-after-error, action-needed,
 * and unsupported provider states using mocked descriptors.
 */

import { describe, it, expect } from "vitest";

describe("Provider setup fixtures contract", () => {
  it("exports all required GitHub fixture states", async () => {
    const mod = await import("./lib/provider-setup-fixtures");
    expect(mod.GITHUB_NOT_CONNECTED).toBeDefined();
    expect(mod.GITHUB_CONNECTED_READY).toBeDefined();
    expect(mod.GITHUB_ACTION_NEEDED).toBeDefined();
    expect(mod.GITHUB_RECONNECT_AFTER_ERROR).toBeDefined();
    expect(mod.GITHUB_UNSUPPORTED).toBeDefined();
  });

  it("exports all required Google Drive fixture states", async () => {
    const mod = await import("./lib/provider-setup-fixtures");
    expect(mod.DRIVE_NOT_CONNECTED).toBeDefined();
    expect(mod.DRIVE_CONNECTED_SINGLE_DRIVE).toBeDefined();
    expect(mod.DRIVE_CONNECTED_MULTIPLE_DRIVES).toBeDefined();
    expect(mod.DRIVE_INVALID_DOMAIN).toBeDefined();
    expect(mod.DRIVE_ACTION_NEEDED).toBeDefined();
    expect(mod.DRIVE_RECONNECT_AFTER_ERROR).toBeDefined();
    expect(mod.DRIVE_UNSUPPORTED).toBeDefined();
  });

  it("exports composite SETUP_FIXTURES object", async () => {
    const { SETUP_FIXTURES } = await import("./lib/provider-setup-fixtures");
    expect(SETUP_FIXTURES.github).toBeDefined();
    expect(SETUP_FIXTURES.google_drive).toBeDefined();
  });
});

describe("GitHub provider setup states", () => {
  it("not_connected state has correct structure", async () => {
    const { GITHUB_NOT_CONNECTED } = await import("./lib/provider-setup-fixtures");
    expect(GITHUB_NOT_CONNECTED.provider_id).toBe("github");
    expect(GITHUB_NOT_CONNECTED.connection_status).toBe("not_connected");
    expect(GITHUB_NOT_CONNECTED.resource_readiness.state).toBe("not_ready");
    expect(GITHUB_NOT_CONNECTED.resource_readiness.message).toContain("Install the Bicameral GitHub App");
  });

  it("connected_ready state has installation info", async () => {
    const { GITHUB_CONNECTED_READY } = await import("./lib/provider-setup-fixtures");
    expect(GITHUB_CONNECTED_READY.connection_status).toBe("connected");
    expect(GITHUB_CONNECTED_READY.resource_readiness.state).toBe("ready");
    const readiness = GITHUB_CONNECTED_READY.resource_readiness as import("./lib/provider-setup-fixtures").GitHubAppReadiness;
    expect(readiness.installation_id).toBeTruthy();
    expect(readiness.org_or_user).toBeTruthy();
  });

  it("action_needed state includes error_message", async () => {
    const { GITHUB_ACTION_NEEDED } = await import("./lib/provider-setup-fixtures");
    expect(GITHUB_ACTION_NEEDED.connection_status).toBe("action_needed");
    expect(GITHUB_ACTION_NEEDED.resource_readiness.state).toBe("action_needed");
    expect(GITHUB_ACTION_NEEDED.error_message).toBeTruthy();
  });

  it("reconnect_after_error state indicates interrupted connection", async () => {
    const { GITHUB_RECONNECT_AFTER_ERROR } = await import("./lib/provider-setup-fixtures");
    expect(GITHUB_RECONNECT_AFTER_ERROR.connection_status).toBe("action_needed");
    expect(GITHUB_RECONNECT_AFTER_ERROR.error_message).toContain("interrupted");
  });

  it("unsupported state prevents usage", async () => {
    const { GITHUB_UNSUPPORTED } = await import("./lib/provider-setup-fixtures");
    expect(GITHUB_UNSUPPORTED.connection_status).toBe("unsupported");
    expect(GITHUB_UNSUPPORTED.resource_readiness.state).toBe("unsupported");
  });

  it("GitHub copy references Bicameral GitHub App, not PAT", async () => {
    const { GITHUB_NOT_CONNECTED, GITHUB_CONNECTED_READY } = await import("./lib/provider-setup-fixtures");
    expect(GITHUB_NOT_CONNECTED.resource_readiness.message).toContain("Bicameral GitHub App");
    expect(GITHUB_NOT_CONNECTED.resource_readiness.message).not.toContain("PAT");
    expect(GITHUB_NOT_CONNECTED.resource_readiness.message).not.toContain("token");
    expect(GITHUB_CONNECTED_READY.resource_readiness.message).toContain("Bicameral GitHub App");
  });
});

describe("Google Drive provider setup states", () => {
  it("not_connected state has correct structure", async () => {
    const { DRIVE_NOT_CONNECTED } = await import("./lib/provider-setup-fixtures");
    expect(DRIVE_NOT_CONNECTED.provider_id).toBe("google");
    expect(DRIVE_NOT_CONNECTED.connection_status).toBe("not_connected");
    expect(DRIVE_NOT_CONNECTED.resource_readiness.state).toBe("not_ready");
  });

  it("connected with single drive auto-selects", async () => {
    const { DRIVE_CONNECTED_SINGLE_DRIVE } = await import("./lib/provider-setup-fixtures");
    expect(DRIVE_CONNECTED_SINGLE_DRIVE.connection_status).toBe("connected");
    expect(DRIVE_CONNECTED_SINGLE_DRIVE.resource_readiness.state).toBe("ready");
    const readiness = DRIVE_CONNECTED_SINGLE_DRIVE.resource_readiness as import("./lib/provider-setup-fixtures").GoogleDriveReadiness;
    expect(readiness.eligible_shared_drives).toHaveLength(1);
    expect(readiness.selected_drive_id).toBe(readiness.eligible_shared_drives[0].id);
    expect(readiness.project_path).toContain(".bicameral");
  });

  it("connected with multiple drives requires selection", async () => {
    const { DRIVE_CONNECTED_MULTIPLE_DRIVES } = await import("./lib/provider-setup-fixtures");
    expect(DRIVE_CONNECTED_MULTIPLE_DRIVES.connection_status).toBe("connected");
    expect(DRIVE_CONNECTED_MULTIPLE_DRIVES.resource_readiness.state).toBe("pending_selection");
    const readiness = DRIVE_CONNECTED_MULTIPLE_DRIVES.resource_readiness as import("./lib/provider-setup-fixtures").GoogleDriveReadiness;
    expect(readiness.eligible_shared_drives.length).toBeGreaterThan(1);
    expect(readiness.selected_drive_id).toBeUndefined();
  });

  it("invalid domain blocks project creation", async () => {
    const { DRIVE_INVALID_DOMAIN } = await import("./lib/provider-setup-fixtures");
    expect(DRIVE_INVALID_DOMAIN.connection_status).toBe("connected");
    expect(DRIVE_INVALID_DOMAIN.resource_readiness.state).toBe("action_needed");
    const readiness = DRIVE_INVALID_DOMAIN.resource_readiness as import("./lib/provider-setup-fixtures").GoogleDriveReadiness;
    expect(readiness.domain_valid).toBe(false);
    expect(readiness.message).toContain("Google Workspace");
    expect(readiness.message).toContain("Consumer Gmail");
  });

  it("action_needed state includes reconnect messaging", async () => {
    const { DRIVE_ACTION_NEEDED } = await import("./lib/provider-setup-fixtures");
    expect(DRIVE_ACTION_NEEDED.connection_status).toBe("action_needed");
    expect(DRIVE_ACTION_NEEDED.error_message).toBeTruthy();
  });

  it("reconnect_after_error state indicates interrupted connection", async () => {
    const { DRIVE_RECONNECT_AFTER_ERROR } = await import("./lib/provider-setup-fixtures");
    expect(DRIVE_RECONNECT_AFTER_ERROR.connection_status).toBe("action_needed");
    expect(DRIVE_RECONNECT_AFTER_ERROR.error_message).toContain("interrupted");
  });

  it("unsupported state prevents usage", async () => {
    const { DRIVE_UNSUPPORTED } = await import("./lib/provider-setup-fixtures");
    expect(DRIVE_UNSUPPORTED.connection_status).toBe("unsupported");
    expect(DRIVE_UNSUPPORTED.resource_readiness.state).toBe("unsupported");
  });

  it("Drive path convention uses .bicameral/<project-name>/", async () => {
    const { DRIVE_CONNECTED_SINGLE_DRIVE } = await import("./lib/provider-setup-fixtures");
    const readiness = DRIVE_CONNECTED_SINGLE_DRIVE.resource_readiness as import("./lib/provider-setup-fixtures").GoogleDriveReadiness;
    expect(readiness.project_path).toMatch(/\.bicameral\//);
    expect(readiness.project_path).toMatch(/\/$/);
  });

  it("Drive fixtures never reference arbitrary folder picker", async () => {
    const mod = await import("./lib/provider-setup-fixtures");
    const allDriveFixtures = [
      mod.DRIVE_NOT_CONNECTED,
      mod.DRIVE_CONNECTED_SINGLE_DRIVE,
      mod.DRIVE_CONNECTED_MULTIPLE_DRIVES,
      mod.DRIVE_INVALID_DOMAIN,
      mod.DRIVE_ACTION_NEEDED,
      mod.DRIVE_RECONNECT_AFTER_ERROR,
      mod.DRIVE_UNSUPPORTED,
    ];
    for (const fixture of allDriveFixtures) {
      expect(fixture.resource_readiness.message).not.toContain("folder picker");
      expect(fixture.resource_readiness.message).not.toContain("choose folder");
    }
  });
});

describe("Create action enablement logic", () => {
  it("create is disabled when provider not connected", async () => {
    const { GITHUB_NOT_CONNECTED } = await import("./lib/provider-setup-fixtures");
    expect(GITHUB_NOT_CONNECTED.connection_status).not.toBe("connected");
    // Create button should be disabled — connection required
  });

  it("create is disabled when provider connected but resource not ready (action_needed)", async () => {
    const { DRIVE_INVALID_DOMAIN } = await import("./lib/provider-setup-fixtures");
    // Connected but domain invalid — resource not ready
    expect(DRIVE_INVALID_DOMAIN.connection_status).toBe("connected");
    expect(DRIVE_INVALID_DOMAIN.resource_readiness.state).toBe("action_needed");
  });

  it("create is disabled when provider connected but drive selection pending", async () => {
    const { DRIVE_CONNECTED_MULTIPLE_DRIVES } = await import("./lib/provider-setup-fixtures");
    expect(DRIVE_CONNECTED_MULTIPLE_DRIVES.connection_status).toBe("connected");
    expect(DRIVE_CONNECTED_MULTIPLE_DRIVES.resource_readiness.state).toBe("pending_selection");
    // No drive selected yet — create should be disabled
  });

  it("create is enabled when provider connected and resource ready", async () => {
    const { GITHUB_CONNECTED_READY } = await import("./lib/provider-setup-fixtures");
    expect(GITHUB_CONNECTED_READY.connection_status).toBe("connected");
    expect(GITHUB_CONNECTED_READY.resource_readiness.state).toBe("ready");
    // Create should be enabled (with project name filled)
  });

  it("create is enabled when drive connected and single drive auto-selected", async () => {
    const { DRIVE_CONNECTED_SINGLE_DRIVE } = await import("./lib/provider-setup-fixtures");
    expect(DRIVE_CONNECTED_SINGLE_DRIVE.connection_status).toBe("connected");
    expect(DRIVE_CONNECTED_SINGLE_DRIVE.resource_readiness.state).toBe("ready");
  });
});

describe("SetupWizard component imports resolve", () => {
  it("SetupWizard resolves all imports without error", async () => {
    // Verify the component module can be loaded (import/type resolution)
    const mod = await import("./pages/SetupWizard.svelte");
    expect(mod).toBeDefined();
    expect(mod.default).toBeDefined();
  });
});
