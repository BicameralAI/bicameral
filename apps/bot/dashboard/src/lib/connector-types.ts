/**
 * Connector descriptor types — mirrors the connector-config.schema.json
 * contract from bicameral-integrations.
 *
 * These types drive the Channels dashboard page. The dashboard renders
 * connector cards from descriptor-shaped data; it never stores secrets
 * or performs live wiring.
 */

export type ConnectorMode = "webhook" | "active" | "passive";

export type ConnectorStatus = "scoped" | "candidate" | "beta" | "live-ready";

export type ConnectorCategory = string;

export type TrustTier = string;

export type ConnectorCredential = {
  key: string;
  type: "api_key" | "oauth2" | "webhook_secret" | "basic";
  label: string;
  required: boolean;
  modes?: ConnectorMode[];
  header?: string;
  validation?: string;
  obtain: {
    url: string;
    steps: string[];
  };
};

export type ConnectorDataDescriptor = {
  emits: string[];
  pii_posture: string;
  redaction?: string;
};

export type InstructionAction =
  | "open_url"
  | "paste_secret"
  | "register_webhook"
  | "configure"
  | "verify";

export type ConnectorInstruction = {
  action: InstructionAction;
  text: string;
  link?: string;
  ref?: string;
};

export type ConnectorReference = {
  kind: string;
  label?: string;
  url: string;
};

export type ConnectorResource = {
  label: string;
  url: string;
};

export type WebhookDescriptor = {
  signature_scheme: string;
  header: string;
  events: string[];
  setup: {
    url: string;
    steps: string[];
  };
  receiver: {
    required: boolean;
    label: string;
    provisioned_by: string;
  };
};

export type RuntimeConfigEntry = {
  key: string;
  label: string;
  required: boolean;
  description?: string;
};

export type IngestionGateHints = {
  source_title_template: string;
  source_type_label: string;
  summary_fields?: string[];
  evidence_labels?: Record<string, string>;
  empty_state?: string;
};

export type ConnectorDescriptor = {
  id: string;
  name: string;
  version: string;
  channel: "beta" | "ga";
  description: string;
  icon?: string;
  summary?: string;
  setup_summary?: string;
  status_copy?: string;
  configuration_tips?: string[];
  resources?: ConnectorResource[];
  category: ConnectorCategory;
  trust_tier: TrustTier;
  status: ConnectorStatus;
  available: boolean;
  modes: ConnectorMode[];
  credentials: ConnectorCredential[];
  data: ConnectorDataDescriptor;
  ingestion_gate?: IngestionGateHints;
  instructions?: ConnectorInstruction[];
  references?: ConnectorReference[];
  wire_gates?: string[];
  webhook?: WebhookDescriptor;
  runtime_config?: RuntimeConfigEntry[];
  live_readiness?: string;
};
