# Runtime Architecture: Product Creation and Discovery

- **Date:** 2026-07-25
- **Status:** proposed — governance ready with warnings; terminal provider-edge
  and browser evidence, Release Unit assignment, and product acceptance remain
  pending
- **Level:** L2 cross-repository runtime contract
- **Owns:** the production hosted Product creation, provider connection,
  event-store selection, Cloud publication, local registration, discovery
  readiness, and recovery sequence.
- **Canonical machine contract:**
  `product-creation-and-discovery-runtime-architecture.contract.json`
- **Governance profile:** `spec-governance-gate: strict` (proposed restoration;
  judgment resolutions require durable human answers)
- **Governance gate:** `bic:spec-governance-gate`; Qor advisory/no-op pending
  factory policy revision; UserJourney `hosted_project_creation`; Journey
  Boundary Review `passed`.
- **UserJourney:** `hosted_project_creation`
- **Related authority:**
  - `domain-product-onboarding-contract.md`
  - `explicit-project-registration-hosted-local-discovery.md`
  - `project-setup-ui.md`
  - `oauth-and-integration-connections.md`
  - `operator-home-join-user-journey.md`
  - `hosted-and-dev-dashboard-mode-runtime-architecture.md`
  - ADR-0001, ADR-0007, ADR-0023

## Reading Guide

Read **Decision Boundary**, **State Refinement**, and **Journey Boundary
Review** first. The main invariant is:

> A canonical Product creation event is appended only through the explicitly
> selected and validated Product event-store adapter, and Cloud publication is
> forbidden until that adapter returns a provider-confirmed remote durability
> proof for the exact event. Cloud discovery, browser state, process working
> directory, and a daemon's previously active adapter are never routing or
> durability authority.

This document uses **Product** for the canonical domain object,
**ProjectRegistration** for operator-local routing state, and **projection**
only for derived read models.

## Runtime Problem

The production hosted dashboard bypasses the dev-only `SetupWizard`. Its
replacement collects only a Product name, omits GitHub/Google Drive selection
and provider authorization, and then creates through an adapter supplied to the
daemon dispatch path. The resulting Cloud row can be projected as `offline`
when local registration or binding is merely incomplete. An offline member row
has no primary recovery action.

This creates six coupled failures:

1. Product intent is underspecified before the canonical commit.
2. Canonical routing can inherit unrelated daemon/process context.
3. Provider OAuth exists behind routes but is not part of hosted creation.
4. Cloud membership, pairing, registration, provider access, and replay are
   collapsed into one readiness label.
5. A post-commit partial failure becomes a dead end instead of an idempotent
   continuation.
6. A local adapter write can be projected to Supabase before the selected
   provider confirms remote durability of the first event.

Existing specs agree that the selected event-store substrate is canonical and
that provider connections are operator-level, but they do not settle the
ordering of selection versus `product.create`. They also disagree on whether
missing provider access temporarily navigates to Settings or is completed
inside a resumable Product creation experience.

## Goals

1. Provide a dedicated, staged production Product creation route.
2. Require explicit GitHub repository or Google Drive folder selection.
3. Reuse the operator-level Connection service for real provider
   authorization; never implement Product-specific OAuth.
4. Resolve and validate the selected adapter before canonical replay/append.
5. Preserve a creation draft across OAuth, cancellation, refresh, and retry.
6. Keep canonical creation, Cloud publication, ProjectRegistration, and replay
   as independent commit points with explicit recovery.
7. Project an absent registration as `needs_setup`/`Continue setup`, never as
   a row-level `offline` state.
8. Treat a paired daemon as a prerequisite for Operator Home. An unpaired or
   unavailable session exits to pairing/reconnect before Product rows render.
9. Prove the flow using the shipped SPA and real runtime boundaries.
10. Require a bounded first-event commit proof before any Cloud/Supabase
    Product row can be written.

## Non-Goals

- Cloud does not become Product or event-store authority.
- Provider connection does not imply Product membership, event-store
  selection, source binding, or ingestion approval.
- This flow does not expose provider tokens, local paths, canonical events, or
  source content to Cloud.
- The dev-only `SetupWizard` is not promoted back into production.
- This spec does not define source-ingestion bindings.
- This release does not add provider substrates beyond Git-backed stores and
  Google Drive folders.
- This spec does not rename the canonical `Product` domain object to
  `Project`.

## Decision Boundary

### D1 — canonical creation ordering

**Recommendation:** provider readiness, resource selection, descriptor
validation, and adapter resolution happen before `product.create` appends the
canonical event.

**Trade-off:** creation has more pre-commit steps and cannot offer instant
name-only creation. In exchange, every created Product is durably addressable,
replayable, and cannot be stranded in an unintended adapter.

**Alternative rejected pending human decision:** mint Product identity in a
separate daemon control-plane store and attach the Product event store later.
That creates a second canonical persistence model not granted by current ADRs
or specs.

**Strict status:** caller confirmed the recommendation on 2026-07-25. The
answer is durably recorded in the committed Decision Log below.

### D2 — connection UX placement

**Confirmed direction:** Product creation includes a dedicated embedded
connection step for the selected GitHub or Google Drive substrate and resumes
the same creation attempt after authorization. “Embedded” does not mean bespoke
OAuth: creation uses the same connection routes, registry, secret store,
provider protocol implementation, and typed outcomes as the existing
Connections page. The Connections page remains independently accessible for
global management, but creation never navigates the user there.

**Interaction contract:**

- If the selected provider is already connected with the required capability,
  the step confirms the safe account label and advances to resource selection.
- If it is absent or needs action, the step renders `Connect GitHub` or
  `Connect Google Drive` inside the creation route.
- GitHub device flow may open the provider verification page or show its code;
  Google OAuth may open provider consent. The creation route and draft remain
  intact.
- Callback or polling completion returns to the same creation attempt and
  advances to repository/folder selection.
- Denial, expiry, provider outage, or cancellation keeps the user in creation
  with retry, change-provider, and cancel-creation actions.
- The embedded step owns no credential state. It consumes only redacted
  `ConnectionStartOutcome`, `ConnectionCompletionOutcome`, and connection
  readiness from the shared backend.

**Trade-off:** the creation route must support callback/poll resumption and
draft persistence. Navigating to Settings would be simpler, but it breaks task
continuity and creates a second navigation/recovery state.

**Strict status:** caller confirmed with the dedicated embedded carve-out on
2026-07-26. The answer is durably recorded in the committed Decision Log.

### D3 — post-create success state

**Confirmed direction:** journey success is first observable as `needs_setup`
when canonical creation/publication succeeded but the durable local
registration does not yet exist. It becomes `ready` only after the daemon opens
and replays the registered adapter. Once a registration exists, an open/replay
failure is `repair_required`, not incomplete registration. Operator Home has no
`offline` Product row: an unpaired or unavailable daemon exits the user to
pairing/reconnect.

**Trade-off:** Product rows expose more truthful progression, but the
application shell must gate Operator Home on daemon reachability and the
readiness mapper must consume independent facts instead of one coarse status.

**Strict status:** caller confirmed with refinements on 2026-07-25. The answer
is durably recorded in the committed Decision Log.

### D4 — first-event durability boundary

**Confirmed direction:** a successful local adapter write is not sufficient to
publish a Product to Cloud. The selected adapter must return a bounded
`ProductCreationCommitProof` that confirms the exact `CreateProduct` event is
durable at the selected provider resource. Only then may the hosted
orchestrator call the Cloud/Supabase publication API.

Provider confirmation is substrate-specific:

- GitHub/Git: the push is accepted at the selected remote/ref and a remote
  read-back resolves the pushed revision and exact event blob/digest.
- Google Drive: the upload returns a provider file/revision/etag and a metadata
  or content read-back confirms the exact event digest in the selected folder.

The proof is a validation witness, not Product lifecycle state. It contains
only safe opaque identifiers, provider revision/etag, event digest, substrate,
resource fingerprint, and attempt/Product binding. It contains no token, local
path, event body, repository/folder content, or provider authorization code.

**Failure contract:**

- no provider confirmation means no Cloud/Supabase write;
- provider confirmation followed by Cloud failure retries publication with the
  same proof and does not append a second event;
- a lost adapter response reconstructs proof by reading the same remote
  revision/event and does not append again;
- a lost Cloud response retries an idempotent upsert keyed by Product id and
  proof digest.

**Strict status:** caller confirmed the model on 2026-07-26. Exact provider API
receipt shapes remain adapter-owned, but the ordering and safe proof envelope
are normative.

## Existing Infrastructure Inventory

| Existing surface | Reuse | Required correction |
| --- | --- | --- |
| `HostedProjectRegistration.svelte` | Name validation and submission feedback | Replace inline name-only card with dedicated staged hosted route |
| `hosted-project-registration.ts` | Idempotent create/publish/continue orchestration and pending operation storage | Include validated provider/substrate/resource routing before create |
| `ProductCreateParams` | Existing event-store and required-connection hints | Make explicit descriptor/routing required for production creation |
| `product_creation::create_product` | Authority-first validation, idempotency, atomic Product/role event | Call only after explicit adapter resolution |
| `EventStoreAdapter::append` | Common canonical append seam | Return a provider-confirmed append receipt instead of local-write success only |
| `product_registration` | Continue-setup and local registration | Remove default provider/substrate for explicit hosted creation |
| Connection routes/types | Shared start, callback, status vocabulary | Wire real GitHub flow and one global registry |
| `github_device_flow` | Implemented device-flow start/poll/token machinery | Invoke it from `connection_start` |
| Google OAuth backend | Authorization URL, callback, token persistence | Invoke from hosted creation and resume draft |
| Cloud discovery | Tenant-safe non-canonical directory | Stop inventing GitHub metadata; compose local readiness honestly |
| Operator Home action model | Ready/open and setup continuation concepts | Give incomplete post-create rows a recovery action |

## Runtime Topology

```text
Signed-in operator
      |
      | name + substrate + resource + consent
      v
Hosted SPA -----------------------> Bicameral Cloud
   |                                  |
   | typed commands                   | non-canonical publication
   v                                  v
Hosted bridge                    Cloud/Supabase directory
   |
   v
Local daemon ---> Connection service ---> GitHub / Google
   |
   +-----------> selected EventStoreAdapter/resource
   |
   +-----------> operator-global ProjectRegistration registry
```

The canonical component and relationship definitions are `components[]` and
`relationships[]` in the JSON companion. This diagram is only a review
projection.

## Runtime State, Authority, Persistence, And Routing

| State id | Artifact | Classification | Authority / persistence | Key invariant |
| --- | --- | --- | --- | --- |
| `STATE-CREATION-DRAFT` | Resumable creation input | local scratch | Browser session, keyed by Cloud account + attempt | Contains no provider secret; never proves Product creation |
| `STATE-CONNECTION` | Operator provider connection | canonical state | Local Connection service; global registry + secret store | Operator-scoped, not workspace- or Product-scoped |
| `STATE-CANONICAL-PRODUCT` | CreateProduct and membership history | accepted event | Daemon writes selected event store | Explicit descriptor resolves adapter before append |
| `STATE-FIRST-EVENT-COMMIT-PROOF` | Provider-confirmed exact-event durability witness | audit receipt/log | Daemon derives from selected adapter/provider response | Required before Cloud publication; never Product state |
| `STATE-CLOUD-PUBLICATION` | Hosted Product preview | derived projection | Cloud/Supabase | Non-canonical and independently retryable |
| `STATE-PROJECT-REGISTRATION` | Local Product routing | canonical local state | Daemon operator registry | Keyed by Product id; no implicit discovery |
| `STATE-DISCOVERY-PROJECTION` | Operator Home row | derived read model | Browser composition | Never invent provider/substrate or render row-level Offline |

## State Refinement Check

### Smallest honest release state

The minimum lifecycle is:

```text
draft
  -> connection_ready
  -> canonical_product_created
  -> first_event_provider_confirmed
  -> cloud_published
  -> local_registration_pending
  -> ready

local_registration_pending -> repair_required -> ready
daemon unavailable         -> pairing/reconnect screen
```

The canonical JSON does not model these labels as one monolithic Product state.
They are independent artifacts and transitions:

- `CreateProduct` is canonical and irreversible except by future governed
  lifecycle behavior.
- first-event commit proof is a bounded validation witness over the canonical
  append, not an additional Product state.
- Cloud publication is a retryable non-canonical fact.
- ProjectRegistration is operator-local routing state.
- `ready`, `needs_setup`, and `repair_required` are Product-row projection
  values derived from the independent facts.
- daemon pairing/reachability is shell routing state, not a Product-row state.

### States refined

- Existing `pending_registration` is rendered as `needs_setup`; no new
  canonical Product state is added.
- Existing `registration_repair_required` is rendered as `repair_required`.
- row-level `offline` is retired. Daemon/session loss routes to
  pairing/reconnect and prevents Operator Home from rendering.
- `connection_ready` is operator connection state, not Product lifecycle.

### Transition authority

| Transition | Allowed actor/component | Durable effect |
| --- | --- | --- |
| `TRANS-DRAFT-CONNECTION-READY` | Operator + Connection service | Operator-global connection metadata/secret |
| `TRANS-CREATE-CANONICAL-PRODUCT` | Authorized daemon command | One CreateProduct event in selected store |
| `TRANS-PROVE-FIRST-EVENT` | Selected adapter + daemon | Bounded provider-confirmed commit proof |
| `TRANS-PUBLISH-CLOUD-DIRECTORY` | Hosted SPA + Cloud | Non-canonical Cloud upsert |
| `TRANS-REGISTER-PRODUCT` | Local daemon | Operator-local ProjectRegistration |
| `TRANS-PROJECT-READY` | Daemon observation + SPA projection | No canonical write |
| `TRANS-PROJECT-REPAIR` | Daemon observation + SPA projection | Preserve canonical facts; expose recovery |

### Invariants

1. No canonical append occurs before explicit adapter validation.
2. No Cloud/Supabase Product publication occurs before a valid
   provider-confirmed proof for the exact first event.
3. Retrying one creation attempt cannot mint another Product.
4. Post-create retries use the original Product id and commit proof.
5. Cloud remains non-canonical.
6. Connection state remains operator-global and local.
7. Discovery labels never replace their underlying authority facts, and
   pairing state never masquerades as a Product state.
8. Provider denial before canonical append creates no Product.
9. Provider/access failure after canonical append preserves the Product and
   offers continuation/repair.

### Deferred vocabulary

- Product archival/deletion.
- Migration between event-store substrates.
- Multiple canonical event stores per Product.
- Product mutation while the daemon is unreachable.
- Provider resource delegation across operators.

## State Minimization Review

- Rejected a new canonical `creating` Product state. Before `CreateProduct`,
  only a browser draft exists.
- Rejected a canonical `published` Product state. Publication is a Cloud
  projection fact.
- Reused `pending_registration` and `registration_repair_required` instead of
  adding `needs_binding`, `needs_provider`, and `needs_replay` as separate
  canonical states. Failure reason is projection metadata with distinct
  actions.
- Retired row-level `offline`; pairing/reconnect is an application-shell route.
- Kept connection lifecycle independent because it has different authority,
  persistence, sharing, revocation, and recovery.

## Authority Ownership Matrix

| Actor/component | May | Must not |
| --- | --- | --- |
| Operator | Choose Product name/resource; grant provider consent | Directly write Product events |
| Hosted SPA | Collect intent; orchestrate; render derived readiness | Mint identity; store secrets; select implicit adapter |
| Cloud | Authenticate Cloud account; publish/query safe directory rows | Hold provider tokens; become Product/workspace authority |
| Bridge | Authenticate and transport typed commands | Decide routing or lifecycle success |
| Daemon | Validate authority/routing; append canonical events; register/replay | Fall back to current adapter/cwd |
| Connection service | Authenticate provider; persist local secret/readiness | Imply membership, source binding, or Product selection |
| Provider | Authorize account/resource access | Mint Bicameral Product identity |
| Event store | Persist/replay accepted Product events | Store provider credentials |

## Command And Protocol Contracts

### `CreateProductIntent`

Required before dispatch:

```yaml
attempt_id: stable idempotency token
display_name: non-empty Product display name
event_store:
  substrate: git | drive_folder
  provider: github | google
  descriptor_ref: opaque daemon-resolvable local reference
required_connection:
  provider: github | google
  capability: event_store.git | event_store.drive
cloud_actor:
  asserted_cloud_account_id: safe Cloud identifier
```

Excluded:

- provider tokens or authorization codes;
- raw local paths;
- repository/folder contents;
- source bindings;
- Cloud role assertions not validated by daemon actor context.

Validation fails before append when:

- the descriptor is absent or not explicit;
- provider/substrate pairing is invalid;
- the Connection service lacks the required capability;
- the resource is inaccessible;
- the actor is unpaired, wrong-account, or unauthorized.

### Connection start/completion

The hosted creation route consumes the existing typed
`ConnectionStartOutcome` and `ConnectionCompletionOutcome`. GitHub start must
invoke the real device flow; Google uses the configured local-loopback flow.
Responses never contain tokens.

All connection metadata reads and writes resolve to the operator-global
registry. An active workspace must not change the registry path.

### Cloud publication

`ProductPublication` contains only safe directory metadata and a reference to a
validated `ProductCreationCommitProof`. It is idempotent by Product id and
proof digest and may be retried without rerunning `product.create`.

### `ProductCreationCommitProof`

The selected adapter returns an `AppendReceipt`; the daemon validates and
projects it into this substrate-neutral safe envelope:

```yaml
schema_version: 2  # JSON number
attempt_id: stable creation attempt
product_id: canonical Product id
event_id: UUID of the CreateProduct EventStoreEntry
substrate: git | drive_folder
resource_fingerprint: sha256 of normalized selected-resource identity
provider_revision: opaque commit sha | file revision/etag
event_digest: sha256 of serialize_event_store_entry (pretty JSON of the ledger EventStoreEntry; not ProductCreationResult)
durability: provider_confirmed_remote
confirmed_at: ISO-8601 with Z
signer_key_id: sha256 of raw 32-byte pubkey  # under the wax
signature_algorithm: ed25519                 # under the wax; verifier hardcodes Ed25519
proof_digest: sha256 of compact sorted JSON of every field except proof_digest and signature
signature: standard base64 of 64-byte Ed25519 over the raw 32-byte digest
```

Production v2 proofs are `substrate: drive_folder` only. `DriveApiAdapter` appends `serialize_event_store_entry` JSON bytes and hashes those same bytes as `event_digest`. `GitAdapter` still writes YAML under ADR-0001 and **must not emit** a v2 `ProductCreationCommitProof` until Git appends those exact JSON bytes. A Git proof must not attest bytes that were never appended.

`protocol/onboarding/schemas/create-product-ledger-event.schema.json` is the Drive ledger file. `protocol/onboarding/vectors/product-creation-commit-proof-v2.golden.json` is the literal Bot↔Cloud v2 canonical-bytes/digest/signature vector. `product-creation-result.schema.json` is the SPA command result. The fixture validator fails the build if a schema or fixture is unmapped (bot#1193). Person↔stamp enrollment is cloud#302, not this envelope.

The envelope is valid only after provider-side read-back. A local file write,
local Git commit, queued upload, HTTP request acceptance without a durable
revision, or Cloud publication is not provider confirmation.

### Continue setup

`product.continue_setup` consumes the original Product id and validated
descriptor reference. It may retry Cloud publication, local registration, and
open/replay. It may not create another Product or silently change the selected
event store.

## Substrate Capability Matrix

| Capability | GitHub/Git | Google Drive |
| --- | --- | --- |
| Canonical Product events | Required | Required |
| Provider-confirmed first-event proof | Remote push + revision/blob read-back | File revision/etag + metadata/content read-back |
| Explicit resource descriptor | Repository | Drive folder |
| Operator connection | GitHub device flow | Google OAuth |
| Deterministic append/replay | Required | Required |
| Resource permission validation | Required | Required |
| Local/offline replay after cached access | Adapter-specific | Adapter-specific |
| Cloud canonical authority | Never | Never |

## Runtime Scenarios

The canonical scenarios are in `scenarios[]`:

- `SCN-CREATE-CONNECTED`
- `SCN-CREATE-REQUIRES-OAUTH`
- `SCN-POST-CREATE-PARTIAL-FAILURE`
- `SCN-DISCOVER-AND-OPEN`
- `SCN-FORBID-IMPLICIT-ROUTING`

### Happy path

1. Operator opens the dedicated Create Product route. SPA creates
   `STATE-CREATION-DRAFT`.
2. Operator chooses GitHub/Git or Google Drive and a resource.
3. SPA asks the local Connection service for operator-level readiness.
4. If missing, the same Connection service completes real provider auth and
   persists `STATE-CONNECTION`; the SPA resumes the retained draft.
5. Daemon validates actor, connection capability, selected descriptor, and
   adapter access.
6. Daemon appends one idempotent CreateProduct event to the selected adapter,
   committing `STATE-CANONICAL-PRODUCT`.
7. Adapter confirms remote provider durability and daemon returns
   `STATE-FIRST-EVENT-COMMIT-PROOF` for the exact event.
8. SPA validates the safe proof envelope and publishes safe metadata to Cloud,
   committing the independent
   `STATE-CLOUD-PUBLICATION`.
9. Daemon persists `STATE-PROJECT-REGISTRATION`.
10. Daemon opens/replays the registered adapter. SPA projects the row as
   `ready` with `Open`.

### Partial failure and recovery

| Failure point | User-visible state | Correct recovery | Forbidden recovery |
| --- | --- | --- | --- |
| OAuth denied/expired before create | Creation draft with retry/cancel | Restart shared connection flow | Create with default Git |
| Resource validation fails | Draft with repair/change resource | Revalidate or choose another resource | Append to active adapter |
| Local append succeeds; provider confirmation fails | Creation recovery; no Cloud row | Read back/retry same event by attempt id | Publish to Supabase or append another event |
| Canonical append succeeds; Cloud fails | Needs setup / Continue setup | Retry publication by Product id | Rerun CreateProduct |
| Cloud succeeds; registration fails | Needs setup / Continue setup | Retry local registration | Treat Cloud row as registration |
| Registration succeeds; replay fails | Repair required | Repair access then reopen/replay | Remove registration or create another Product |
| Daemon unavailable/unpaired | Pairing/reconnect screen; Operator Home hidden | Restore paired session | Render Product rows or prompt for provider OAuth |

## User Journey Contract

```yaml
user_journey:
  id: hosted_project_creation
  name: Hosted Product Creation
  actor: signed_in_operator
  entrypoint: /dashboard/products/new
  success_state: product_visible_as_ready_or_needs_setup_with_safe_continuation
  surfaces:
    - cloud_dashboard_shell
    - bot_hosted_spa
    - cloud_product_directory_api
    - hosted_local_daemon_bridge
    - local_connection_service
    - selected_event_store_adapter
    - local_product_registration
  boundary_contracts:
    - HostedHandoffEnvelope
    - CreateProductIntent
    - ConnectionStartOutcome
    - ConnectionCompletionOutcome
    - ProductCreationCommitProof
    - ProductPublication
    - ProjectRegistrationReceipt
    - ProjectDiscoveryItem
  required_evidence:
    - browser_lifecycle_acceptance
    - producer_consumer_contract_tests
    - multi_product_multi_adapter_restart_replay
    - provider_edge_integration
    - first_event_before_cloud_publication
    - deployment_version_check
```

### Journey Boundary Review

```yaml
journey_boundary_review:
  status: passed
  journey_ids:
    - hosted_project_creation
  authoritative_commit_points:
    - operator_connection
    - canonical_product_creation
    - provider_confirmed_first_event
    - cloud_directory_publication
    - local_product_registration
  independently_observable_facts:
    - provider_connection_ready
    - canonical_product_exists
    - first_event_commit_proof_exists
    - cloud_publication_exists
    - local_registration_exists
    - adapter_replay_ready
  partial_failure_reviewed: true
  retry_and_idempotency_reviewed: true
  validation_witness_separated_from_product_state: true
  user_confirmed: true
  source_ref: docs/specs/product-creation-and-discovery-runtime-architecture.md#strict-decision-log
```

Sensitive data that must not cross to Cloud or browser persistence:

- provider tokens, refresh tokens, authorization codes, or client secrets;
- local filesystem paths;
- canonical event payloads or Decision Ledger content;
- repository/folder contents;
- source binding credentials.

Journey success is a user-visible actionable Product row whose Cloud
publication was authorized by a valid first-event commit proof. Product
lifecycle state is the independently persisted canonical/registration state.
Commit proofs, tests, topology probes, and deployment digests are validation
witnesses and never Product lifecycle transitions.

## Discovery Projection Truth Table

The projection consumes independent dimensions rather than one overloaded
status.

| Shell prerequisite | Membership | Registration/replay | Provider/resource | Surface/result | Primary action |
| --- | --- | --- | --- | --- | --- |
| unpaired/unavailable | any | not evaluated | not evaluated | Pairing/reconnect screen; Operator Home is not rendered | Pair/reconnect |
| paired | available, not member | n/a | n/a | Available | Join |
| paired | member | no durable registration | pre-create validated or join setup pending | Needs setup | Continue setup |
| paired | member | registered, replay failed | repair needed | Repair required | Repair access |
| paired | member | registered + replay ready | ready | Ready | Open |

`event_store_kind` and provider labels remain `unknown` until safe daemon
registration metadata supplies them. Cloud discovery must not invent GitHub.

## Security, Privacy, And Forbidden Flows

Forbidden:

1. Create without a validated explicit event-store descriptor.
2. Fall back to the active adapter, current working directory, default Git, or
   a fixture.
3. Store connection metadata under a Product workspace.
4. Return fabricated GitHub authorization codes.
5. Send provider tokens, local paths, or canonical events to Cloud.
6. Treat Cloud publication, membership, pairing, or a test receipt as proof
   that Product replay is ready.
7. Re-run canonical creation after a post-create partial failure.
8. Render any Product row as Offline instead of routing daemon loss to
   pairing/reconnect.
9. Publish a Cloud/Supabase Product row before provider-confirmed remote
   durability of its exact CreateProduct event.
10. Treat a local write, local Git commit, request acceptance, or test harness
    receipt as provider confirmation.

## Runtime Architecture Conformance

The canonical `conformance[]` defines:

| Requirement | Status | Summary |
| --- | --- | --- |
| `CONF-CREATE-UI-01` | missing | Dedicated staged hosted creation UI |
| `CONF-EXPLICIT-ROUTING-01` | missing | Resolve explicit adapter before append |
| `CONF-CREATE-CONTRACT-01` | partial | Carry routing/connection contract end-to-end |
| `CONF-GITHUB-OAUTH-01` | partial | Invoke real device flow |
| `CONF-GOOGLE-OAUTH-01` | partial | Resume hosted creation through real OAuth |
| `CONF-CONNECTION-REGISTRY-01` | missing | One operator-global connection registry |
| `CONF-DISCOVERY-STATE-01` | missing | Truthful setup/repair rows and pairing-route enforcement |
| `CONF-DISCOVERY-METADATA-01` | missing | No invented GitHub metadata |
| `CONF-IDEMPOTENT-RECOVERY-01` | partial | Restart-safe post-commit convergence |
| `CONF-FIRST-EVENT-PROOF-01` | missing | Adapter returns safe provider-confirmed exact-event proof |
| `CONF-SUPABASE-ORDERING-01` | missing | Cloud publication is impossible before valid proof |
| `CONF-JOURNEY-01` | unresolved | Real browser/provider/runtime acceptance |

## Implementation Sequencing

No implementation issue may be created until D1–D4 and the Journey Boundary
Review have durable human confirmation.

1. **Contract and adapter resolution — `HITL_BLOCKED` until D1**
   - Extend creation intent with an opaque descriptor reference.
   - Resolve adapter before dispatch; deny missing/implicit routing.
   - Remove default Git/provider behavior for hosted creation.
   - Covers `CONF-EXPLICIT-ROUTING-01`, `CONF-CREATE-CONTRACT-01`.

2. **Connection service correctness**
   - Invoke real GitHub device-flow start.
   - Normalize reads/writes to the operator-global registry.
   - Add controlled Google/GitHub failure coverage.
   - Covers `CONF-GITHUB-OAUTH-01`, `CONF-GOOGLE-OAUTH-01`,
     `CONF-CONNECTION-REGISTRY-01`.

3. **Dedicated hosted creation route — `HITL_BLOCKED` until D2**
   - Name, substrate, connection, resource, review, progress/recovery steps.
   - Preserve draft across callback/poll/navigation.
   - Covers `CONF-CREATE-UI-01`.

4. **Truthful discovery projection**
   - Derive row/action from independent facts.
   - Remove hardcoded GitHub metadata.
   - Covers `CONF-DISCOVERY-STATE-01`, `CONF-DISCOVERY-METADATA-01`.

5. **Idempotent continuation**
   - Persist/recover commit receipts across browser and daemon restart.
   - Covers `CONF-IDEMPOTENT-RECOVERY-01`.

6. **First-event durability and publication gate**
   - Extend adapter append results with substrate-specific durable receipts.
   - Validate and normalize a safe `ProductCreationCommitProof`.
   - Gate every Cloud publication/retry on that proof.
   - Covers `CONF-FIRST-EVENT-PROOF-01`,
     `CONF-SUPABASE-ORDERING-01`, and recovery conformance.

7. **Terminal journey validation**
   - Shipped SPA + Cloud + real daemon + two adapters + provider-edge flow.
   - Real-process disposable Git proves remote protocol ordering but remains
     supporting evidence; GitHub and Drive provider-edge runs prove terminal
     provider confirmation.
   - Deployment version confirmation remains separately diagnosable.
   - Covers `CONF-JOURNEY-01`.

## ADR Implications

- **ADR-0001 — reaffirm and clarify:** Git and Drive remain supported
  event-store substrates; add the explicit pre-append routing invariant if it
  is not already normative.
- **ADR-0007 — reaffirm:** canonical state materializes only through the
  selected event-store substrate.
- **ADR-0023 — amend/supersede transitional discovery language:** cwd and
  implicit `.bicameral/` discovery cannot route production Product creation.
- **Candidate ADR — explicit event-store selection precedes Product identity
  minting:** required if D1 is confirmed because the ordering is durable,
  surprising, and hard to reverse.

## Acceptance Criteria

1. Production Create opens a dedicated staged route, not an inline name-only
   panel.
2. Create cannot commit until a GitHub repository or Drive folder is selected,
   connected, and validated.
3. GitHub start returns a provider-issued device code/session.
4. Google authorization resumes the same creation draft.
5. Connection registry behavior is independent of workspace/current Product.
6. Missing explicit routing produces a typed denial and zero canonical events.
7. One daemon can create/restart/replay two Products in different resources
   without cross-routing.
8. A post-create failure before the durable registration write renders Needs
   setup; a failure after registration renders Repair with a safe action.
9. Retrying after each commit point yields one Product and one CreateProduct
   event.
10. No Product row renders Offline; daemon/session loss routes to
    pairing/reconnect.
11. Unknown provider/substrate remains unknown; the UI never invents GitHub.
12. Cloud receives no provider secrets, local paths, or canonical event data.
13. Browser lifecycle acceptance exercises production implementations; fixture
   tests are not terminal journey evidence.
14. Cloud/Supabase contains no Product row until the selected provider confirms
    the exact first event at the selected remote resource.
15. Lost adapter and Cloud responses recover with one Product, one
    CreateProduct event, one proof digest, and an idempotent Cloud row.

## Validation Plan

- Validate the canonical companion with
  `scripts/runtime_architecture_contract.py validate`.
- Unit-test the full discovery truth table and action mapping.
- Contract-test browser create payload ↔ daemon handler ↔ adapter resolver.
- Integration-test real GitHub device-flow start/poll outcomes with controlled
  provider endpoints.
- Integration-test Google callback and draft resumption.
- Restart/replay test browser, daemon, and Cloud failure after every commit
  point.
- Adapter contract tests reject local-only receipts and mismatched
  revision/event digests.
- Harness gate test attempts Cloud publication without a proof and must fail
  before any Supabase request is sent.
- Real-process Git topology pushes to a disposable remote, reads back the
  remote revision and exact event digest, then authorizes Cloud publication.
  This validates protocol ordering but is not GitHub provider-edge evidence.
- Provider-edge runs prove GitHub remote/ref confirmation and Drive
  revision/etag read-back against controlled real provider resources.
- Multi-Product topology test with distinct adapter resources and no cwd
  dependence.
- Browser lifecycle acceptance using the shipped SPA and real local daemon.
- Separately verify the deployed bundle/version.

## Open Questions — Strict Caller Q&A

Ask and durably answer one at a time:

None. D1, D2, D3, and D4 were confirmed by the caller and are recorded below.

## Strict Decision Log

| Decision | Affected dimensions | Caller answer | Status | Durable source |
| --- | --- | --- | --- | --- |
| D1 — validate provider/resource and resolve the selected adapter before `product.create` | runtime, authority | Confirmed as recommended on 2026-07-25 | confirmed, durable | This committed spec section |
| D2 — connection experience during creation | user_journey, runtime | Confirmed on 2026-07-26: provide a dedicated embedded GitHub/Google Drive connection step inside creation, backed by the existing shared Connection service; never navigate to Settings; retain and resume the creation attempt | confirmed, durable | This committed spec section |
| D3 — Product-row readiness and pairing routing | state_machine, user_journey | Confirmed with refinements: no row-level Offline; unpaired/unavailable routes to pairing/reconnect; absent registration is Needs setup; existing registration plus failed replay is Repair; successful replay is Ready | confirmed, durable | This committed spec section |
| D4 — first-event durability before Cloud publication | runtime, authority, persistence, user_journey | Confirmed on 2026-07-26: require provider-confirmed remote durability for the exact first event before any Cloud/Supabase Product publication; retain proof as a bounded validation witness | confirmed, durable | This committed spec section |

### What “incomplete ProjectRegistration” means

`ProjectRegistration` is an atomic durable record keyed by
`(product_id, cloud_account_id)`. It contains the Product/account identity,
opaque registration reference, validated non-secret provider/substrate
metadata, opaque descriptor reference, Governance Policy Version, and
registration timestamp.

It is incomplete only in the workflow sense: no durable registration record
exists yet. Before the daemon may write it, canonical membership or creator
authority must exist, provider access must be authorized, and the selected
event-store descriptor must validate.

| Durable registration | Prerequisites/probe | Projection |
| --- | --- | --- |
| absent | provider authorization absent | Continue setup: Connect provider |
| absent | provider authorized; descriptor not yet validated | Continue setup: Validate event store |
| absent | provider denied or descriptor invalid/unreachable | Repair |
| absent | provider authorized and descriptor valid | Atomically write registration, then open/replay |
| present | adapter open/replay succeeds | Ready/Open |
| present | adapter open/replay fails | Repair; retain registration |

Under confirmed D1, a newly created Product has already passed provider and
descriptor validation. Therefore its only legitimate post-create
`Needs setup` window is recovery after canonical creation but before the
registration write completes. A local workspace binding is orthogonal and does
not make `ProjectRegistration` incomplete.

## Request Coverage

| Request | Covered by |
| --- | --- |
| Restore human-decision governance as strict | Governance profile and strict Caller Q&A; factory implementation remains a separate owner-surface change |
| Fix newly created Product showing Offline | State refinement, discovery truth table, `CONF-DISCOVERY-STATE-01` |
| Replace awkward popup | Dedicated route goal, `CONF-CREATE-UI-01` |
| Select GitHub versus Drive | Decision boundary, creation contract, substrate matrix |
| Complete OAuth | Connection scenarios and provider conformance requirements |
| Compare specs with implementation | Runtime problem, infrastructure inventory, conformance table |
| Detail the ideal flow and gaps | Happy path, partial failure table, journey review, acceptance criteria |
| Require first event on the event substrate as proof | D4, commit-proof contract, Cloud publication gate, harness validation plan |
