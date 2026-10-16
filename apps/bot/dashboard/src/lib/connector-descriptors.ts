/**
 * Static connector descriptors for the Channels page.
 *
 * These mirror the authoritative config.json files from
 * bicameral-integrations/connectors/{slack,devin}/config.json.
 * The dashboard renders from this descriptor-shaped data — it does not
 * perform live wiring, store secrets, or add backend routes.
 *
 * Boundary: integrations descriptors declare requirements;
 * the operator runtime receives any secret/token values.
 */

import type { ConnectorDescriptor } from "./connector-types";

export const CONNECTOR_DESCRIPTORS: ConnectorDescriptor[] = [
  {
    id: "slack",
    name: "Slack",
    version: "0.1.0",
    channel: "beta",
    description:
      "Connect the official Bicameral Slack app to review important channel conversations alongside your decisions.",
    icon: "slack",
    summary: "Review Slack conversations in Bicameral.",
    setup_summary:
      "The official Bicameral Slack app is not available yet.",
    status_copy: "Coming soon",
    configuration_tips: [
      "Slack will use the official Bicameral app, not a custom app you create.",
      "Start with one pilot workspace or channel before widening access.",
      "Bicameral will read selected conversations only; posting back to Slack is not enabled here.",
    ],
    resources: [
      {
        label: "Documentation",
        url: "https://api.slack.com/apis/connections/events-api",
      },
      {
        label: "Website",
        url: "https://slack.com",
      },
    ],
    category: "communication",
    trust_tier: "T2",
    status: "candidate",
    available: false,
    modes: ["webhook"],
    credentials: [
      {
        key: "slack_webhook",
        type: "webhook_secret",
        label: "Slack signing secret",
        required: true,
        modes: ["webhook"],
        header:
          "X-Slack-Signature (v0= hex HMAC-SHA256 over 'v0:{timestamp}:{raw_body}') + X-Slack-Request-Timestamp",
        obtain: {
          url: "https://api.slack.com/authentication/verifying-requests-from-slack",
          steps: [
            "Install the official Bicameral Slack app when it becomes available.",
            "Choose the workspace and channels Bicameral should review.",
          ],
        },
      },
    ],
    webhook: {
      signature_scheme:
        "v0= hex HMAC-SHA256 over 'v0:{X-Slack-Request-Timestamp}:{raw_body}', 5-minute replay window",
      header: "X-Slack-Signature",
      events: ["message"],
      setup: {
        url: "https://api.slack.com/apis/connections/events-api",
        steps: [
          "Install the official Bicameral Slack app.",
          "Choose the workspace and channels Bicameral should review.",
        ],
      },
      receiver: {
        required: true,
        label: "Bicameral webhook receiver URL (operator-provisioned)",
        provisioned_by: "operator",
      },
    },
    data: {
      emits: ["message"],
      pii_posture:
        "Slack messages can contain sensitive team context. Bicameral will review selected conversations and avoid storing connector secrets in the Decision Ledger.",
      redaction: "Message content is prepared for review before it becomes evidence.",
    },
    ingestion_gate: {
      source_title_template: "Slack: {channel_name}",
      source_type_label: "Slack message",
      summary_fields: ["channel_name", "author_ref", "message_excerpt"],
      evidence_labels: {
        message: "Message",
        thread: "Thread",
        permalink: "Slack permalink",
      },
      empty_state: "No Slack evidence has produced candidates yet.",
    },
    instructions: [
      {
        action: "register_webhook",
        text: "Install the official Bicameral Slack app when it becomes available.",
        link: "https://api.slack.com/apis/connections/events-api",
      },
      {
        action: "paste_secret",
        text: "Choose the workspace and channels Bicameral should review.",
      },
      {
        action: "verify",
        text: "Confirm the Slack app connection.",
      },
    ],
    references: [
      {
        kind: "events-api",
        label: "Documentation",
        url: "https://api.slack.com/apis/connections/events-api",
      },
      {
        kind: "verify",
        label: "Signing secrets",
        url: "https://api.slack.com/authentication/verifying-requests-from-slack",
      },
    ],
    wire_gates: [
      "Use the official Bicameral Slack app.",
      "Start with one pilot workspace or channel.",
      "Posting back to Slack is not enabled here.",
    ],
    live_readiness: "Coming soon. Slack setup will use the official Bicameral app.",
  },
  {
    id: "devin",
    name: "Devin",
    version: "0.1.0",
    channel: "beta",
    description:
      "Bring Devin session activity into Bicameral so agent work can be reviewed alongside product decisions.",
    icon: "devin",
    summary: "Review Devin sessions as part of your decision history.",
    setup_summary:
      "Create a Devin service-user key, provide the sessions endpoint, and test the local setup form.",
    status_copy: "Ready to configure",
    configuration_tips: [
      "Use a service-user API key from your Devin organization settings.",
      "Paste the sessions endpoint for the organization you want to review.",
      "Live polling is not enabled here; the setup form only prepares the required values.",
    ],
    resources: [
      {
        label: "Documentation",
        url: "https://docs.devin.ai/api-reference/overview",
      },
      {
        label: "Website",
        url: "https://devin.ai",
      },
    ],
    category: "developer-ai",
    trust_tier: "T1",
    status: "live-ready",
    available: true,
    modes: ["active"],
    credentials: [
      {
        key: "devin",
        type: "api_key",
        label: "Service-User API key",
        required: true,
        modes: ["active"],
        header: "Authorization: Bearer <cog_ Service-User key>",
        validation: "^cog_[A-Za-z0-9]+$",
        obtain: {
          url: "https://docs.devin.ai/api-reference/authentication",
          steps: [
            "In your Devin organization settings, create a service-user API key.",
            "Paste the key into Bicameral during setup.",
          ],
        },
      },
    ],
    runtime_config: [
      {
        key: "base_url",
        label: "Sessions endpoint",
        required: true,
        description:
          "Use the Devin sessions endpoint for your organization.",
      },
    ],
    data: {
      emits: ["session"],
      pii_posture:
        "Session free-text (title, structured_output, message trail) may carry secrets/PII — passed through redact-and-pass before emit. FX-SEC-001 producer screen is the un-bypassable backstop.",
      redaction:
        "redact-and-pass (free-text scrubbed of secret/PHI/PAN + email/phone; FX-SEC-001 is the hard backstop)",
    },
    ingestion_gate: {
      source_title_template: "Devin: {session_title}",
      source_type_label: "Devin session",
      summary_fields: ["session_title", "session_status", "pr_url"],
      evidence_labels: {
        session: "Session",
        pr_url: "Pull request",
      },
      empty_state: "No Devin evidence has produced candidates yet.",
    },
    instructions: [
      {
        action: "open_url",
        text: "Create a service-user API key in your Devin organization settings.",
        link: "https://docs.devin.ai/api-reference/authentication",
      },
      {
        action: "paste_secret",
        text: "Paste the service-user API key.",
      },
      {
        action: "configure",
        text: "Add the sessions endpoint for the Devin organization you want Bicameral to review.",
      },
      {
        action: "verify",
        text: "Check that the setup form has the information needed for a live Devin connection.",
      },
    ],
    references: [
      {
        kind: "api",
        label: "Documentation",
        url: "https://docs.devin.ai/api-reference/overview",
      },
      {
        kind: "auth",
        label: "Authentication",
        url: "https://docs.devin.ai/api-reference/authentication",
      },
      {
        kind: "sessions-v3",
        label: "Sessions API",
        url: "https://docs.devinenterprise.com/api-reference/v3/sessions/organizations-sessions",
      },
    ],
    wire_gates: [
      "Use a service-user key with only the access needed for session review.",
      "Keep the API key in your local operator runtime, not in the dashboard.",
      "Live polling still needs a real Devin key and operator review.",
    ],
    live_readiness:
      "Ready to configure. Live polling still needs a real Devin key and operator review.",
  },
  {
    id: "linear",
    name: "Linear",
    version: "0.1.0",
    channel: "beta",
    description:
      "Bring Linear issues, comments, and project updates into Bicameral so product decisions can reference tracked work.",
    icon: "linear",
    summary: "Review Linear issues and project activity in Bicameral.",
    setup_summary:
      "Linear integration is not live yet. This setup page shows the configuration that will be needed when the connector is available.",
    status_copy: "Candidate",
    configuration_tips: [
      "Use a Linear API key scoped to the workspace you want Bicameral to review.",
      "Webhook mode delivers real-time updates; polling mode is also planned.",
      "Bicameral will read selected project data only; writing back to Linear is not enabled here.",
    ],
    resources: [
      {
        label: "Documentation",
        url: "https://developers.linear.app/docs",
      },
      {
        label: "Website",
        url: "https://linear.app",
      },
    ],
    category: "project-tracker",
    trust_tier: "T2",
    status: "candidate",
    available: false,
    modes: ["webhook", "active"],
    credentials: [
      {
        key: "linear_api_key",
        type: "api_key",
        label: "Linear API key",
        required: true,
        modes: ["active"],
        header: "Authorization: Bearer <lin_api_ key>",
        validation: "^lin_api_[A-Za-z0-9]+$",
        obtain: {
          url: "https://linear.app/settings/api",
          steps: [
            "Go to Linear workspace settings → API → Personal API keys.",
            "Create a new key and copy it.",
          ],
        },
      },
      {
        key: "linear_webhook_secret",
        type: "webhook_secret",
        label: "Linear webhook signing secret",
        required: true,
        modes: ["webhook"],
        header: "Linear-Signature (HMAC-SHA256 hex digest of raw body)",
        obtain: {
          url: "https://developers.linear.app/docs/graphql/webhooks",
          steps: [
            "In Linear settings → API → Webhooks, create a new webhook.",
            "Copy the signing secret for verification.",
          ],
        },
      },
    ],
    webhook: {
      signature_scheme:
        "HMAC-SHA256 hex digest of raw request body, verified against Linear-Signature header",
      header: "Linear-Signature",
      events: ["Issue", "Comment", "Project", "ProjectUpdate"],
      setup: {
        url: "https://developers.linear.app/docs/graphql/webhooks",
        steps: [
          "Create a webhook in Linear workspace settings → API → Webhooks.",
          "Point the webhook URL at your Bicameral operator endpoint.",
          "Select the resource types Bicameral should receive (Issues, Comments, Projects).",
        ],
      },
      receiver: {
        required: true,
        label: "Bicameral webhook receiver URL (operator-provisioned)",
        provisioned_by: "operator",
      },
    },
    data: {
      emits: ["issue", "comment", "status_update", "project"],
      pii_posture:
        "Linear issues and comments may contain internal product context. Bicameral reviews selected project data and avoids storing connector secrets in the Decision Ledger.",
      redaction:
        "Issue and comment text is prepared for review before it becomes evidence.",
    },
    ingestion_gate: {
      source_title_template: "Linear: {issue_identifier}",
      source_type_label: "Linear issue",
      summary_fields: ["issue_identifier", "issue_title", "status", "team_name"],
      evidence_labels: {
        issue: "Issue",
        comment: "Comment",
        status_update: "Status update",
        project: "Project",
      },
      empty_state: "No Linear evidence has produced candidates yet.",
    },
    instructions: [
      {
        action: "open_url",
        text: "Create an API key in your Linear workspace settings.",
        link: "https://linear.app/settings/api",
      },
      {
        action: "paste_secret",
        text: "Paste the Linear API key.",
      },
      {
        action: "configure",
        text: "Set up a webhook or configure polling for the projects Bicameral should review.",
      },
      {
        action: "verify",
        text: "Confirm the Linear connection setup is complete.",
      },
    ],
    references: [
      {
        kind: "api",
        label: "Documentation",
        url: "https://developers.linear.app/docs",
      },
      {
        kind: "auth",
        label: "Authentication",
        url: "https://developers.linear.app/docs/graphql/working-with-the-graphql-api#authentication",
      },
      {
        kind: "webhooks",
        label: "Webhooks",
        url: "https://developers.linear.app/docs/graphql/webhooks",
      },
    ],
    wire_gates: [
      "Use a scoped API key with only the access needed for project review.",
      "Keep the API key in your local operator runtime, not in the dashboard.",
      "Live ingestion requires operator credentials and runtime review.",
    ],
    live_readiness:
      "Candidate — not live yet. Setup shows required configuration; live webhook and polling ingestion are planned.",
  },
];
