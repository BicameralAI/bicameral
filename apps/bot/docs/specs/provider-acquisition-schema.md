# Provider acquisition schema — provisional alpha (#523)

Status: **provisional alpha**. Related: bot #405, bot #462, BicameralAI/bicameral-integrations#173.

This spec reconciles and documents the provisional provider-acquisition schema
shared by **bicameral-bot** and **bicameral-integrations**. It defines the
canonical field vocabulary, enum values, and fixture expectations for the two
provider-fact handoff objects:

- `ProviderResourceDescriptor` — a provider-neutral *resource discovery* fact.
- `ProviderItemEnvelope` — a screened *fetched item* fact.

Both objects are **provisional alpha**. They are purpose-neutral provider facts
and carry **no** Bicameral authority. They are not promoted to a stable contract
until bot and integrations both pass cross-repo conformance against the golden
fixtures referenced here (deferred — see [Open / deferred](#open--deferred)).

Authoritative Rust types:
[`crates/bicameral-api/src/provider_acquisition.rs`](../../crates/bicameral-api/src/provider_acquisition.rs).
Generated JSON Schemas:
[`protocol/schemas/v2/provider-resource-descriptor.schema.json`](../../protocol/schemas/v2/provider-resource-descriptor.schema.json),
[`protocol/schemas/v2/provider-item-envelope.schema.json`](../../protocol/schemas/v2/provider-item-envelope.schema.json).

## 1. Authority boundary (what these objects are not)

A provider-acquisition object describes *what a provider exposes*. It never
describes *what Bicameral has decided*. Specifically, neither object is — and
neither may carry fields that imply — any of:

- a `SourceBinding`, binding approval, or `approved_by` actor;
- a `Source`, `SourceSnapshot`, or `SourceEvidence` materialization;
- a `DecisionCandidate` or `Decision`;
- a review command, signoff, compliance verdict, or governance result;
- event-store configuration or an event-store write;
- setup/connection completion state or bot lifecycle (tracking) state.

These objects occupy a separate protocol surface and must **not** be routed
through `ToolRequest` or `ExternalIngestEnvelope`. They enter governance only
after explicit promotion through the existing `SourceBinding` approval path.

## 2. `ProviderResourceDescriptor` field vocabulary

| Field | Type | Required | Meaning |
|-------|------|----------|---------|
| `provider` | string | yes | Provider system id (`github`, `google_drive`, `linear`, `slack`, …). |
| `resource_id` | string | yes | Provider-scoped unique resource identifier. |
| `display_name` | string | yes | Human-readable resource name. |
| `resource_type` | string | yes | Provider-assigned type (`repository`, `folder`, `channel`, `document`, `board`, `team`, …). |
| `uri` | string | no | Canonical resource URI on the provider. |
| `capabilities` | `ResourceCapability[]` | no | Provider-neutral capabilities (see §4). |
| `permission` | `PermissionState` | no | Access state for the authenticated connection (see §4). |
| `action_needed` | `ActionNeededReason` | no | Hint for why action is needed, when `permission = action_needed` (see §4). |
| `parent` | `ResourceRef` | no | Parent resource reference for nested resources. |
| `freshness` | `FreshnessMetadata` | no | Provider freshness facts (`last_modified`, `etag`, `item_count`). |
| `provider_metadata` | object | no | Screened, namespaced provider-specific extension facts (see §5). |
| `captured_at` | string | yes | ISO-8601 capture time. |

## 3. `ProviderItemEnvelope` field vocabulary

| Field | Type | Required | Meaning |
|-------|------|----------|---------|
| `provider` | string | yes | Provider system id. |
| `resource_id` | string | yes | Owning resource id (matches a descriptor's `resource_id`). |
| `item_id` | string | yes | Provider-scoped unique item id. |
| `item_type` | string | yes | Provider-assigned item type (`issue`, `document`, `message`, …). |
| `content` | string | yes | **Screened** item content (no secrets, tokens, credentials, or PII). |
| `title` | string | no | Item title/summary. |
| `content_hash` | string | no | Content digest (e.g. `sha256:…`) for dedup/change detection. |
| `uri` | string | no | Canonical item URI on the provider. |
| `freshness` | `FreshnessMetadata` | no | Provider freshness facts. |
| `provider_metadata` | object | no | Screened, namespaced provider-specific extension facts (see §5). |
| `fetched_at` | string | yes | ISO-8601 fetch time. |

The envelope is the *conversion boundary* between an integrations Observation
output and bot-side handling: integrations screen and emit a `ProviderItemEnvelope`;
the bot consumes it as a provider fact only. Turning an envelope into accepted
evidence, a candidate, or a decision is a separate, governed step (see §6).

## 4. Canonical enum values

**`ResourceCapability`** (`capabilities[]`):

| Value | Meaning |
|-------|---------|
| `list` | The resource can be listed/enumerated. |
| `read` | Individual items within the resource can be read. |
| `search` | Items within the resource can be searched. |
| `watch` | The provider can deliver change notifications. |
| `incremental_fetch` | Items can be fetched incrementally. |

**`PermissionState`** (`permission`):

| Value | Meaning |
|-------|---------|
| `granted` | The connection can access the resource. |
| `unknown` | Access state has not been determined. |
| `action_needed` | Access requires a user/admin action (see `action_needed`). |
| `denied` | Access is denied. |

**`ActionNeededReason`** (`action_needed`, advisory; only meaningful when
`permission = action_needed`):

| Value | Meaning |
|-------|---------|
| `missing_scope` | The OAuth grant lacks a required scope. |
| `reauth_required` | The provider requires re-authentication. |
| `admin_approval_required` | An administrator must approve access. |
| `plan_upgrade_required` | A paid plan/tier is required. |
| `credential_expired` | The connection credential has expired. |

`permission`/`action_needed` are **advisory provider facts**. They never grant or
withhold any Bicameral authority and never block a Bicameral lifecycle transition
by themselves.

## 5. Namespaced extension / unknown-field handling

Provider-specific facts that do not fit the typed fields go in
`provider_metadata`. Integrations may preserve any screened provider-specific
fields there (e.g. GitHub `default_branch`, Linear `key`, Drive `mime_type`)
**without granting authority**: `provider_metadata` is a free-form object with no
typed authority meaning, so nothing in it is interpreted as binding, review,
signoff, compliance, or event-store state. The bot-side types do not deserialize
any unknown top-level field into authority — unknown top-level fields are dropped
on parse (proven by the invalid fixtures in §8). Integrations **must** screen
`provider_metadata` (and `content`) so it never carries secrets, tokens,
credentials, or PII.

## 6. Separation from Source / SourceSnapshot / SourceEvidence / DecisionCandidate

Provider facts and canonical Bicameral objects are different layers:

| Provider fact (this spec) | Canonical object (governed) |
|---------------------------|-----------------------------|
| `ProviderResourceDescriptor` | `SourceBinding` (created only via the approval path) |
| `ProviderItemEnvelope` | `Source` / `SourceSnapshot` / `SourceEvidence` (materialized only via ingest) |
| (none) | `DecisionCandidate` / `Decision` (created only via the governed candidate path) |

A descriptor or envelope can *inform* a later governed step, but parsing one
never creates any canonical object. This is enforced at the type level: the
bot-side structs have no field that can hold a binding, source, evidence, or
candidate, and any injected authority field is discarded on parse. See the
boundary tests in
[`crates/bicameral-api/tests/provider_acquisition_boundary.rs`](../../crates/bicameral-api/tests/provider_acquisition_boundary.rs).

## 7. Ownership: integrations-owned vs bot-owned

| Integrations-owned (provider mechanics) | Bot-owned (lifecycle interpretation) |
|-----------------------------------------|--------------------------------------|
| Provider discovery, listing, fetching | Whether/when to bind a resource as a Source |
| Auth/OAuth scope acquisition and refresh | Whether a fetched item becomes evidence/candidate |
| Computing `permission` / `action_needed` facts | Whether `action_needed` blocks a setup step (it does not by itself) |
| Screening content & `provider_metadata` | Decision acceptance, supersession, signoff, dismissal |
| Emitting `ProviderResourceDescriptor` / `ProviderItemEnvelope` | Governed promotion through the `SourceBinding` approval path |

Integrations produce provider facts; the bot interprets them within its
governed lifecycle. Neither side may use these objects to short-circuit the
other's authority.

## 8. Fixtures

Golden (valid) fixtures cover ≥3 providers (GitHub, Linear, Google Drive) for
both objects, plus an `action_needed` example:

- Descriptors: `valid-provider-resource-descriptor-{minimal,github,linear,google-drive,action-needed}.json`
- Item envelopes: `valid-provider-item-envelope-{minimal,github,linear,google-drive}.json`

Invalid fixtures pin the authority boundary — each attempts a forbidden path and
must be rejected/discarded on parse:

- Secret-bearing: `invalid-provider-descriptor-secret-fields.json`, `invalid-provider-item-secret-fields.json`
- Authority/lifecycle-bearing: `invalid-provider-{descriptor,item}-{local-authority,review-command,event-store-write,bot-lifecycle}.json`

All fixtures live in
[`protocol/schemas/v2/fixtures/`](../../protocol/schemas/v2/fixtures/) and are
exercised by the boundary test referenced in §6.

## Open / deferred

- **Stable promotion** — deferred until bot and integrations both pass cross-repo
  conformance against these golden fixtures.
- **Live provider discovery** and **source-binding approval wiring** — out of
  scope here; tracked by the connection-flow and setup issues.
- **Cross-repo conformance harness** — deferred until BicameralAI/bicameral-integrations#173
  emits matching fixtures.
