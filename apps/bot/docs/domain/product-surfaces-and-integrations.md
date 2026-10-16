# Product Surfaces and Integrations

This document contains product-specific surface, connector, channel, and setup
contracts. These terms adapt Bicameral's decision lifecycle to operator
workflows, dashboards, and external systems without becoming the essential
decision ontology.

Use this file for terms that are durable product implementation vocabulary but
not core lifecycle concepts.

## Promotion Rule

Promote a term here when it shapes a user-facing workflow, connector contract,
operator setup path, or cross-repo integration boundary.

Do not promote small enum fields or component-local state as standalone domain
entities unless they carry independent product meaning.

## Surfaces

`Ledger View`
: Human-facing surface for inspecting Decision Ledger state and emitting review
  commands.

`Ingestion Gate`
: Dashboard processing surface for source-derived candidates and evidence. It
  renders reviewable material and emits governed actions, but it does not create
  Decisions directly.

`Channel`
: Product grouping for an external or local evidence path, such as Slack,
  Devin, GitHub, MCP, CLI, or future operator-configured sources.

`Connector`
: Product-facing integration unit that describes how Bicameral can receive,
  display, configure, or eventually wire evidence from a channel.

`Gateway`
: Local edge boundary that adapts operational sources into typed protocol
  objects before they enter daemon governance.

`Egress`
: Outbound publication, notification, sync, or annotation from Bicameral to an
  external system.

`Hosted Dashboard Mode`
: Production cloud-served dashboard runtime. Cloud serves the SPA and coordinates
hosted account/session, pairing handoff, and project preview metadata, while a
paired local daemon remains authoritative for project registration, local
workspace binding, provider secret custody, provider readiness validation,
governed action authorization, accepted event materialization, and projection
truth. Related: #722.

`Development Dashboard Harness`
: Explicit development/testing path for exercising the dashboard SPA without
presenting a complete production local multi-project self-service surface.
Historical implementation labels such as `dev_dashboard_mode` should map to
this harness term when they describe local fixture, handoff, or setup testing.
The harness does not create cloud-owned project authority or production local
dashboard support. Related: #722.

`production local dashboard mode`
: Rejected wording for the hosted-dashboard product line. Local dashboard code
may support daemon setup, operator development, and test harnesses, but
production user-facing dashboard support is Hosted Dashboard Mode paired to a
local daemon rather than a standalone local multi-project self-service product
state. Related: #722.

## Integration Contracts

`ConnectorDescriptor`
: Product surface/integration contract describing a connector's display
  metadata, setup requirements, trust posture, available modes, credential
  requirements, emitted evidence kinds, and live-readiness state. The
  authoritative schema is owned by `bicameral-integrations`; the bot dashboard
  consumes descriptor-shaped data to render Channels, setup flows, and optional
  `ingestion_gate` display hints.

`IngestionGateHints`
: Optional connector descriptor metadata consumed by the Ingestion Gate to
  adapt source labels, evidence-kind summaries, trust posture, and review
  affordances for connector-shaped evidence. It shapes dashboard presentation
  only; it does not grant connector authority, create Decisions, or change
  ingestion policy.

`ConnectorMode`
: Field vocabulary inside ConnectorDescriptor, currently used to classify
  operational mode such as webhook, active, or passive. It is not promoted as a
  standalone domain entity.

`ConnectorSetup`
: Dashboard setup flow rendered from descriptor-shaped data. It may collect
  local form values, validate field shapes, and explain readiness, but it must
  not claim live wiring, store real secrets, or bypass operator runtime
  authority unless an approved runtime API exists.

`RuntimeConfigEntry`
: Descriptor-shaped field describing an operator runtime configuration value
  needed by a connector setup flow. It is product surface vocabulary when it
  appears in setup rendering, not core decision-lifecycle vocabulary.

`InstructionAction`
: Descriptor step action used by connector setup rendering, such as opening a
  provider URL, pasting a secret, registering a webhook, configuring runtime
  values, or verifying a connection. It is component vocabulary under connector
  setup unless it becomes a broader protocol contract.

`ConnectionRegistry`
: User-level Bicameral metadata store for provider connections available to the
  local operator, such as GitHub or Google Drive. It records non-secret
  connection state used by setup, settings, discovery, and integration flows;
  token material remains in the approved secrets store.

`ConnectionMetadata`
: Per-provider non-secret entry in the ConnectionRegistry describing connection
  status, account label, capabilities, update time, and authentication method.
  It tells product surfaces what the operator can select or repair without
  exposing credentials or granting project source authority by itself.

`ConnectionStartOutcome`
: Typed gateway response for starting a provider connection. It tells setup and
  Settings surfaces whether the provider is already connected, whether a real
  OAuth or device-flow URL/code is available, whether operator action is needed
  before start, or whether the provider is unsupported. It does not carry token
  material, project source authority, or Decision Ledger state.

`ConnectionCallbackRequest`
: Request body contract for completing a local-loopback OAuth callback. It
  carries the provider callback code and state needed to resume an auth session;
  it is not a credential store, project setup event, SourceBinding approval, or
  governance command.

`ConnectionCompletionOutcome`
: Terminal result vocabulary for provider auth-session completion. It tells
  setup and Settings surfaces whether a provider became connected, still needs
  action, was denied, expired, is unsupported, is unavailable, or remains
  pending. No variant may expose secrets, source-binding authority, review
  commands, or Decision Ledger state.

`ConnectionRegistryResponse`
: Read-model/API response shape for listing user-level connection metadata. It
  projects ConnectionRegistry and ConnectionMetadata for UI rendering; it is not
  a separate canonical store.

`DeviceFlowSession`
: Runtime auth-session record for provider device-flow connection attempts. It
  carries non-secret verification and polling state for an in-progress provider
  connection, but it does not grant provider access or persist credential
  material.

`GitHubEndpoints`
: Provider configuration contract naming the GitHub device-flow and token
  endpoints used by local connection setup. It is provider wiring metadata, not
  user connection state or source authority.

`ProviderResourceDescriptor`
: Provisional alpha provider-fact handoff object describing a provider resource
  that may later be selected during setup or source-binding approval. It is not
  a SourceBinding, Source, SourceSnapshot, SourceEvidence, DecisionCandidate,
  project config, or event-store write.

`ProviderItemEnvelope`
: Provisional alpha provider-fact handoff object for screened provider content
  and metadata. It may feed later explicit ingest or source-binding workflows,
  but by itself it is not accepted evidence, a candidate, a review command, or
  bot lifecycle authority.

`ProviderResourceState`
: Provider setup/resource readiness state used by the setup surface to determine
  whether an event-store provider's required resources are ready for project
  creation. Values include ready, pending_selection, not_ready, action_needed,
  and unsupported. It is setup display and enablement state only; it does not
  grant source approval, create SourceBinding, write secrets, create Decisions,
  establish signoff or compliance, or imply merge safety.

`SharedDriveEntry`
: Eligible Google Drive shared-drive resource shape used by the setup surface
  for shared-drive selection during project creation. It carries drive identity,
  name, and domain for rendering a selection list. It does not grant source
  approval, create SourceBinding, write secrets, create Decisions, establish
  signoff or compliance, or imply merge safety.

`GitHubAppReadiness`
: GitHub App installation and readiness state used by the setup UI to show
  whether the Bicameral GitHub App is installed, needs action, or is
  unsupported. It carries a ProviderResourceState, optional installation
  identity, and a human-readable message. It does not grant source approval,
  create SourceBinding, write secrets, create Decisions, establish signoff or
  compliance, or imply merge safety.

`GoogleDriveReadiness`
: Drive account, shared-drive, and domain readiness state used by the setup UI
  to show whether the operator's Google Drive connection has eligible shared
  drives, a valid domain, and a selected drive for the project event store. It
  carries a ProviderResourceState, eligible SharedDriveEntry list, optional
  selection and path, domain validity, and a human-readable message. It does not
  grant source approval, create SourceBinding, write secrets, create Decisions,
  establish signoff or compliance, or imply merge safety.

`DefaultSignalPolicy`
: Bot-owned alpha routing contract for captured evidence items before candidate
  review. In alpha it routes captured evidence items only to `candidate`,
  `logged_only`, or `rejected`. Upstream connection, source, and SourceBinding
  readiness are separate setup/refresh concerns and may surface as
  `action_needed` or blocked refresh state, but they are not signal-item routes.

`SourceBinding`
: Project-level approved ingestion configuration for one external resource,
  such as a GitHub repo, Linear team, Slack channel, or Drive folder/document
  set. It records what may enter the project memory, which connection fetches
  it, who approved it, and how recurring ingestion is scoped.

`ToolSubmission`
: Member/tool-submitted observation from MCP, CLI, local agent activity, or
  another operator surface. It carries operator, tool, and evidence provenance
  for review, but it is not recurring ingestion and does not become source
  authority unless a project owner explicitly promotes or configures that path.

`DecisionDetail`
: Ledger View read-model projection for one accepted Decision. It separates
  candidate review, signoff, code evidence, and compliance axes for rendering;
  it must not be used as a mutation contract or as proof that those axes share
  one lifecycle.

`EvidenceRefreshOutcome`
: Read-model/API outcome for source or code evidence refresh attempts. It tells
  surfaces whether refresh is current, stale, unavailable, failed, or requires
  action without mutating accepted Decision, signoff, compliance, or binding
  authority by itself.

`ProjectionSetupState`
: Product-surface enum for projection provider readiness shown in
  Settings / Sources / Connections — configured, projection_deferred,
  provider_not_configured, or provider_unavailable. It is setup/readiness
  display state, not an Ingestion Gate queue item, compliance verdict, source
  approval, or ingestion authority.

`ProjectionSetupDetail`
: Product-surface record pairing a ProjectionSetupState with an optional
  provider id, a human-readable message, and an optional repair hint, used to
  render projection setup status. It informs operator repair flows; it grants no
  ingestion, signoff, or source authority.

`IngestResponseProjectionState`
: Product-surface projection status returned inline on ingest/tool responses —
  whether a candidate was created, projection was deferred, the provider is not
  configured or unavailable, or no candidate resulted — with a message and
  optional repair hint. It explains what projection did for this submission; it
  is not compliance, signoff, source approval, enforcement, or merge-safety
  authority.

## Onboarding Contract Vocabulary

Product-surface vocabulary for the domain-discovered Product auto-join
onboarding flow. The authoritative end-to-end contract, fixtures, and authority
rules live in
[`docs/specs/domain-product-onboarding-contract.md`](../specs/domain-product-onboarding-contract.md)
(issue #765). These terms are product-surface/cross-repo integration vocabulary;
the daemon remains authoritative for identity, roles, membership, and events.

`Policy Owner`
: Administrative authority that owns and may change a Product's Governance
  Policy Version. A Product creator is materialized atomically as both
  `Policy Owner` and `Product Owner` (never a partial grant). Same-domain
  auto-join never grants this role.

`HostedProjectPreview`
: Safe, non-canonical Cloud directory projection used for discovery and routing.
  It is never canonical membership, roles, or project settings, and never event-
  store truth.

`ProductJoinTicket`
: Narrow, signed, short-lived Cloud assertion authorizing one eligible Cloud
  account to request daemon membership for one Product. It carries only the
  minimum bounded claims (issuer/audience, product id, cloud account id, verified
  domain, issued-at/expiry, nonce/JTI, opaque registration ref, and optional safe
  provider/substrate hints). It never carries the full hosted session token,
  roles, or membership assertions.

`ProductCreationPublication`
: Cloud publication of a Product to hosted discovery after canonical daemon
  creation succeeds. Non-canonical directory metadata, idempotent by canonical
  `product_id`; a failed publication is retryable and never creates a second
  Product.

`ProductJoinRequest` / `ProductJoinResult`
: Daemon-inbound join request carrying a `ProductJoinTicket`, and the daemon-
  authoritative outcome. A granted result grants `Product Member` only with a
  canonical membership event + receipt; a rejected result carries a typed failure
  (`expired_ticket`, `wrong_domain`, `wrong_account`, `wrong_product`,
  `replayed_ticket`) and creates no membership. There is no durable
  pending-approval state.

## Deferred Vocabulary

`ChannelRoute` is intentionally not a promoted product entity in the first
stage. Channels may exist as project communication destinations, and connectors
may expose advisory routing hints, but Bicameral does not yet have a durable
route configuration state with owner workflow, storage, authority, recovery, and
review semantics.

Promote `ChannelRoute` only after the product defines who creates it, where it
is stored, what traffic it can affect, what it cannot affect, how it changes
over time, and how it fails safely. Until then, routing hints remain display or
review-policy inputs; they are not durable project routing configuration.

The following RFQ candidates are intentionally collapsed under promoted parent
contracts instead of becoming standalone product terms:

- `ConnectionStatus` and `AuthMethod` remain ConnectionMetadata fields.
- `ConnectionRegistryEntry` remains a ConnectionRegistryResponse row shape.
- `DeviceFlowStore` is runtime storage for DeviceFlowSession records.
- `GitHubDeviceFlowError` is provider-specific error detail for connection
  completion outcomes.
- `SubmissionProvenance` remains advisory metadata unless it gains independent
  user workflow or authority.
- `CandidateReviewSection`, `SignoffSection`, `CodeEvidenceDetailSection`,
  `ComplianceDeferredSection`, and `CodeEvidenceBinding` remain sections inside
  DecisionDetail.
- `EvidenceRefreshStatus` and `EvidenceRefreshParams` remain fields of
  EvidenceRefreshOutcome and refresh commands.
- `inline provider connect` is a UX pattern name for the setup wizard's
  in-screen connection flow (PR #494). It is a generic implementation detail,
  not a promoted domain entity.

Rich advisory signal classification is also intentionally deferred in alpha.
Integrations and mods may carry routing hints, confidence surfaces, noisy-source
signals, or advisory governance metadata, but Bicameral does not yet promise a
first-class warning or attention workflow that interprets those fields as a
durable routing contract. Promote richer advisory routing vocabulary only after
the product defines the review lane, user action, storage, recovery, and audit
semantics for it.

## Boundaries

- Connectors and descriptors declare requirements; they do not grant governance
  authority.
- Secret and token values belong to an approved operator runtime path, not
  integration descriptors, Decision Ledger state, or dashboard-local durable
  storage.
- Connection start and completion responses must start or complete a real
  provider flow, report existing connection state, ask for runtime/configuration
  repair, or reject unsupported providers. They must not preserve credential
  import instructions as a connection fallback.
- Connector setup must be honest about live wiring. Fixture-only or local-only
  verification must not present itself as a live external connection test.
- Product surfaces may render candidates, logged-only or rejected evidence,
  setup steps, projection setup state, and readiness state, but they must use
  governed actions to affect Decisions.
- Routing hints from connectors, integrations, mods, or tool submissions are not
  durable ChannelRoute configuration.
