<script lang="ts">
  /**
   * Operator-level connections (admin SPA / bot#1030).
   * Alpha event store: Google Drive (local-loopback OAuth).
   * Non-alpha connectors are shown greyed / unsupported — no fake Connect CTAs.
   */
  import {
    ConnectionRow,
    GoogleDriveOAuthFlow,
    PageHeader,
    Stack,
    WorkspaceBar,
    type DriveOAuthPhase,
  } from "../ui";

  export type HostedConnectionRow = {
    providerId: string;
    label: string;
    description: string;
    status: "connected" | "not_connected" | "action_needed" | "unsupported";
    accountLabel?: string;
  };

  type Props = {
    drivePhase?: DriveOAuthPhase;
    driveAccountLabel?: string;
    driveSessionId?: string | null;
    connections?: HostedConnectionRow[];
    onstart?: () => void;
    onopenauth?: () => void;
    oncancel?: () => void;
  };

  let {
    drivePhase = "idle",
    driveAccountLabel = "",
    driveSessionId = null,
    connections = [
      {
        providerId: "github",
        label: "GitHub",
        description: "Source integration (not alpha event store).",
        status: "unsupported" as const,
      },
      {
        providerId: "linear",
        label: "Linear",
        description: "Issue tracking (unsupported in alpha).",
        status: "unsupported" as const,
      },
    ],
    onstart,
    onopenauth,
    oncancel,
  }: Props = $props();

  const secondary = $derived(
    connections.filter((c) => c.providerId !== "google"),
  );
</script>

<div class="hosted-connections" data-testid="hosted-connections">
  <WorkspaceBar title="Connections" />
  <PageHeader
    eyebrow="Account"
    title="Connections"
    description="Operator-level OAuth. Google Drive is the alpha Product event store (local-loopback). Other providers are shown for awareness only."
  />
  <Stack gap={4}>
    <GoogleDriveOAuthFlow
      phase={drivePhase}
      accountLabel={driveAccountLabel}
      sessionId={driveSessionId}
      onstart={onstart}
      onopenauth={onopenauth}
      oncancel={oncancel}
      data-testid="hosted-drive-oauth"
    />
    {#each secondary as c (c.providerId)}
      <div
        class="conn-wrap"
        class:greyed={c.status === "unsupported"}
        data-testid={`connection-${c.providerId}`}
      >
        <ConnectionRow
          providerId={c.providerId}
          label={c.label}
          description={c.description}
          status={c.status}
          accountLabel={c.accountLabel}
        />
      </div>
    {/each}
  </Stack>
</div>

<style>
  .hosted-connections {
    display: flex;
    flex-direction: column;
    gap: var(--sp-4);
  }
  .conn-wrap.greyed {
    opacity: 0.55;
    filter: grayscale(0.35);
    pointer-events: none;
  }
</style>
