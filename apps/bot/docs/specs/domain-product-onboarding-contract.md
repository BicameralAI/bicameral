# Domain-Discovered Product Onboarding Contract

Date: 2026-07-12

Status: draft

Level: L2 cross-repo contract

Source issue: [BicameralAI/bicameral-bot#765](https://github.com/BicameralAI/bicameral-bot/issues/765)

Owns: the single authoritative end-to-end onboarding contract for creating a
Product, publishing it to hosted discovery, discovering it, joining it by
same-domain auto-join, registering it locally, and the idempotency/authority
rules at every hop. This document is the reconciled source of truth that the
runtime slices (bot#766, bot#767, bot#768, bot#769) and Cloud slices
(cloud#125, cloud#126, cloud#127) implement. Where an older spec conflicts, this
contract wins for the alpha onboarding flow.

Governance gate: `bic:spec-governance-gate` advisory/no-op; Qor integration
disabled pending factory policy revision.

## Reading Guide

Read `Runtime Invariant`, `Authority At Every Hop`, and `Forbidden / Legacy
Flows` first. The main invariant is:

```text
The local daemon is authoritative for Product identity, roles, membership,
policy evaluation, and canonical events. Hosted cloud discovers, previews,
issues narrow join tickets, and publishes non-canonical directory rows.
Cloud never becomes a canonical state writer.
```

This contract does not implement the runtime slices. It codifies the approved
decisions, contracts, and deterministic fixtures so the slices land against one
reconciled spec.

## Approved Alpha Decisions

These decisions were approved by the Product Owner on 2026-07-11 (issue #765)
and are binding for alpha:

1. A Product creator atomically becomes both **Policy Owner** and **Product
   Owner** — one indivisible role materialization, never a partial grant.
2. A verified same-domain account may **auto-join as Product Member only**.
   Auto-join never grants Policy Owner or Product Owner.
   **Alpha cut (A27, 2026-09-04):** this Member-only grant is **out of this
   alpha cut**. Every current Product participant is Product Owner. Alpha
   join grants Product Owner. It does **not** grant Policy Owner. Policy
   Owner remains creator-only.
3. Cloud exchanges its authenticated session for a narrow, signed, short-lived
   **`ProductJoinTicket`**; the daemon never receives the full Supabase token.
4. The **local daemon is authoritative** for Product identity, roles,
   membership, policy evaluation, and canonical events.
5. Cloud persists only directory/routing metadata and **publishes a Product
   after canonical daemon creation succeeds**; publication is retryable and
   never creates a second Product.
6. Joining has **no durable pending-approval state** in alpha. There is no
   invite queue and no owner-approval gate on the same-domain path.

## Domain Terms And Authority

This table restates issue #765's glossary as the canonical authority map for the
flow. New terms `Policy Owner`, `HostedProjectPreview`, `ProductJoinTicket`, and
`ProductRegistration` alignment are surfaced as domain entity candidates for the
product glossary (`docs/domain/product-surfaces-and-integrations.md`); see that
file and `CONTEXT.md`.

| Term | Meaning / authority |
| --- | --- |
| `Product` | Canonical governed scope created and persisted by the bot/daemon. |
| `Policy Owner` | Administrative authority that owns and may change a Product's Governance Policy Version. |
| `Product Owner` | Governance role that may approve Product decisions under the active policy. |
| `Product Member` | Governance participant; same-domain auto-join grants only this role. |
| `Governance Policy Version` | Canonical, versioned policy state in the Decision Ledger. |
| `HostedProjectPreview` | Safe Cloud directory projection used for discovery; never canonical membership. |
| `ProductJoinTicket` | Narrow, signed, short-lived Cloud assertion authorizing one eligible account to request daemon membership. |
| `ProjectRegistration` | Local durable routing/provider/event-store registration created after membership (see `explicit-project-registration-hosted-local-discovery.md`). |
| `Hosted cloud` | Authenticates the Cloud account and owns discovery eligibility plus safe directory/routing projections. |
| `Cloud account` | Hosted identity; must be bound to locally confirmed actor context before a daemon mutation. |

`Policy Owner` and `Product Owner` are distinct governance roles that are
materialized together, atomically, for a creator. **This alpha cut:** every
current Product participant is Product Owner; alpha join grants Product
Owner and not Policy Owner. Member-only auto-join is deferred past this
alpha cut.

## Runtime Invariant

- The local daemon is the sole canonical writer of Product identity, role
  assignments, membership, `Governance Policy Version`, and Decision Ledger
  events.
- Hosted cloud may authenticate a `Cloud account`, evaluate discovery
  eligibility, render a `HostedProjectPreview`, mint a `ProductJoinTicket`, and
  publish a non-canonical directory row. It never holds Confidential data and
  never writes canonical state (reaffirms ADR-0003, ADR-0007, ADR-0020,
  ADR-0025).
- A `Cloud account` is never accepted as a local actor on its own; it is bound
  to locally confirmed actor context before any daemon mutation.
- The daemon never receives the full hosted (Supabase) session token. The only
  hosted credential that crosses to the daemon is a `ProductJoinTicket` carrying
  the minimum bounded claims defined below.

## Contract: `product.create`

Creation is daemon-canonical. Cloud publication happens only after canonical
creation succeeds.

```text
operator (local, verified) requests product.create
  -> daemon validates local operator/actor context
  -> daemon creates canonical Product identity
  -> daemon materializes creator roles ATOMICALLY:
       Policy Owner + Product Owner (single membership event, one transaction)
  -> daemon appends canonical creation + membership events to Decision Ledger
  -> daemon returns ProductCreationResult (product_id, creator roles,
       membership_event_ref, receipt)
  -> [only now] cloud publishes a HostedProjectPreview directory row
       referencing the canonical product_id + creation receipt
```

Authority rules:

- Creator-role materialization is atomic: a run either materializes both
  `Policy Owner` and `Product Owner` for the creator or it materializes neither.
  There is no state where a creator holds one role but not the other.
- The canonical `product_id`, roles, and membership come from the daemon. Cloud
  learns `product_id` from the daemon result; Cloud never invents it.
- Publication failure after canonical creation is retryable and must not create
  a second Product (see `Cloud Publication`).

## Contract: `product.join` (same-domain auto-join)

Joining is discovery + narrow-ticket + daemon-authoritative membership. There is
no durable pending-approval state.

```text
cloud account (authenticated, verified domain) opens hosted discovery
  -> cloud renders HostedProjectPreview for eligible same-domain Products
  -> operator selects a Product to Join from Operator Home (per-Product Join)
  -> cloud exchanges its authenticated session for a ProductJoinTicket
       (narrow, signed, short-lived; NOT the full Supabase token)
  -> daemon receives ProductJoinRequest carrying the ticket + locally
       confirmed actor context
  -> daemon validates the ticket:
       issuer/audience, product_id, cloud_account_id, verified_domain,
       expiry, nonce/JTI (single-use), registration_ref
  -> on success: daemon grants Product Member ONLY, appends canonical
       membership event + receipt, returns ProductJoinResult(granted)
  -> on typed failure: daemon returns ProductJoinResult(rejected) with a
       typed failure (expired_ticket | wrong_domain | wrong_account |
       wrong_product | replayed_ticket); no membership is created
```

Authority rules:

- Same-domain auto-join grants `Product Member` only. It never grants
  `Policy Owner` or `Product Owner`.
- There is no durable pending-approval state and no owner-approval gate on the
  same-domain path (decision 6). A join either succeeds (Member granted) or is
  rejected with a typed failure; neither outcome is a persisted queue entry.
- Membership is a canonical daemon event. Cloud is not authoritative for
  membership and must not treat a directory row as membership.

## `ProductJoinTicket` — Minimum Bounded Claims

The ticket carries only the minimum bounded claims needed to authorize one
eligible account to request daemon membership for one Product. Extra claims are
rejected (the schema is fail-closed: `additionalProperties: false`).

| Claim | Meaning |
| --- | --- |
| `issuer` | Hosted issuer identity (who minted the ticket). |
| `audience` | Intended daemon audience (which daemon/product surface it is for). |
| `product_id` | Canonical Product the ticket authorizes a join request for. |
| `cloud_account_id` | The single eligible Cloud account. |
| `verified_domain` | The verified email/identity domain used for same-domain eligibility. |
| `issued_at` | Issuance timestamp. |
| `expiry` | Short-lived expiry timestamp. |
| `nonce` | Single-use nonce / JTI for replay detection. |
| `registration_ref` | Opaque reference the daemon uses to correlate the directory row; not canonical settings. |
| `provider_hint` | Optional safe provider hint (e.g. `github`) when needed for setup routing. |
| `substrate_hint` | Optional safe event-store substrate hint (e.g. `git`) when needed for setup routing. |

The ticket must **not** carry: the full Supabase/hosted session token, refresh
tokens, roles/role grants, membership assertions, provider secrets, local paths,
canonical project settings, or Decision Ledger events. Roles and membership are
daemon-authoritative outputs, not ticket inputs.

## Cloud Publication (non-canonical, retryable)

- Cloud directory rows (`HostedProjectPreview`) are **explicitly non-canonical
  projections**. They are discovery/routing metadata only and never canonical
  membership, roles, or project settings.
- Cloud publishes a Product **after** canonical daemon creation succeeds,
  referencing the daemon's canonical `product_id` and creation receipt.
- Publication is **idempotent by canonical `product_id`** (plus an
  `idempotency_key`). A Cloud publication failure is **retryable without
  creating a second Product**: retrying publish for the same `product_id`
  updates the existing directory row rather than creating a new Product or a
  duplicate row.

## Authority At Every Hop (idempotency + authority)

| Hop | Authority | Idempotency | Non-canonical / must-not |
| --- | --- | --- | --- |
| Create Product | Daemon (canonical identity, roles, membership, events) | Re-issuing create for an existing local registration updates `last_opened_at`/readiness; does not duplicate the canonical Product | Cloud must not invent `product_id` or write creation events |
| Materialize creator roles | Daemon | Atomic single membership event; replaying the event is a no-op | No partial (one-role) creator state |
| Publish to discovery | Cloud (directory row only) | Idempotent by canonical `product_id` + `idempotency_key`; retry updates the row | Directory row is not canonical membership; retry never creates a second Product |
| Discover / preview | Cloud (`HostedProjectPreview`) | Preview is derived/rebuildable | Preview is not event-store truth |
| Mint join ticket | Cloud (`ProductJoinTicket`) | Ticket `nonce`/JTI is single-use; a replayed ticket is a typed failure | Ticket carries only bounded claims; never the full session token |
| Bind actor | Daemon (local actor confirmation) | Re-binding the same account is idempotent | Cloud identity alone is never a local actor |
| Grant membership (join) | Daemon (canonical membership event + receipt) | Idempotent by `(product_id, cloud_account_id)`; a duplicate join is a no-op returning the existing membership | Grants `Product Member` only; never Owner roles; no durable pending state |
| Register locally | Daemon (`ProjectRegistration`) | Idempotent by `(product_id, cloud_account_id)` | Registration is persisted under operator home and routes by canonical `product_id`; it is never a Product-ledger event, team membership, or canonical setting |

## Lifecycle Minimization

Per issue #765 and the state-refinement discipline, discovery eligibility, the
canonical `Product Member` role, `ProjectRegistration`, and optional
`LocalWorkspaceBinding` are **orthogonal states**. This contract adds **no
durable join-request state**:

- Discovery eligibility is a Cloud-side derived property, not a durable membership.
- `Product Member` is a canonical daemon role.
- `ProjectRegistration` is operator-local routing state.
- `LocalWorkspaceBinding` is operator-local code-grounding state.

A same-domain join transitions directly from eligible-and-selected to
`Product Member` (granted) or to a typed rejection. There is no
`join_requested`, `pending_approval`, or `invite_pending` durable state.

## Deterministic Cross-Repo Fixtures

The reconciled object shapes are pinned as deterministic fixtures and
hand-authored contract-spec JSON Schemas under `protocol/onboarding/`. See
[`protocol/onboarding/README.md`](../../protocol/onboarding/README.md).

Objects with fixtures:

- `HostedProjectPreview` — safe Cloud directory projection.
- `ProductJoinTicket` — narrow signed short-lived join assertion.
- `ProductCreationResult` — daemon-canonical creation + atomic creator roles.
- `ProductCreationPublication` — Cloud non-canonical directory publication.
- `ProductJoinRequest` — daemon-inbound join request carrying a ticket.
- `ProductJoinResult` — daemon-authoritative join outcome (granted / typed
  rejection).

Failure coverage (as `ProductJoinResult` rejections): `expired_ticket`,
`wrong_domain`, `wrong_account`, `wrong_product`, `replayed_ticket`, plus the
`granted` success path.

Determinism and boundedness are enforced by
[`scripts/validate_onboarding_contract_fixtures.py`](../../scripts/validate_onboarding_contract_fixtures.py):
every fixture is structurally validated against its schema, canonical-JSON
round-trips deterministically, and `ProductJoinTicket` fixtures are rejected if
they carry any claim outside the bounded set.

Each authoritative schema now carries a stable, versioned `$id`
(`https://bicameral.dev/protocol/onboarding/v1/…`), and
[`protocol/onboarding/manifest.json`](../../protocol/onboarding/manifest.json)
records the contract version (`1.0.0`), source commit, canonical semantic
fingerprint, and per-file schema/fixture digests. The validator recomputes the
fingerprint and fails on schema, fixture, or manifest drift with a diagnosable
message.

> **Production Rust serialization is connected in this foundation slice
> (bot#787).** The shipped `bicameral-api` wire types (`product_creation.rs`,
> `product_join.rs`) round-trip every success and failure fixture under
> `crates/bicameral-api/tests/onboarding_contract_conformance.rs`, using the
> production types directly with no hand-maintained shadow types; the schemas'
> `additionalProperties: false` and the Rust `#[serde(deny_unknown_fields)]`
> both keep the fail-closed shapes rejecting undeclared fields. This is contract
> evidence only: canonical daemon runtime behavior, live journeys, and Cloud
> consumer enforcement (cloud#136/#137) remain out of scope. Executable
> **TypeScript** serialization/round-trip tests remain deferred to the Cloud
> slices, which must preserve the shapes and semantic fingerprint pinned here.

## Forbidden / Legacy Flows

The following older alpha defaults are **removed or marked legacy/deferred** by
this contract:

- **Invite-code / rendezvous join as the alpha default.** The invite/rendezvous
  exchange in `rendezvous-state-machine.md` is legacy and is not the alpha
  onboarding default. The alpha default is per-Product Join from Operator Home
  via `HostedProjectPreview` + `ProductJoinTicket` + same-domain auto-join.
- **Owner-approval / pending-approval queue on join.** No durable
  pending-approval state exists on the same-domain path (decision 6).
- **Global free-form join.** The setup Join tab's free-form invite-code entry is
  replaced by per-Product Join from Operator Home. (This is distinct from the
  running dashboard's project selector, which lists already-registered local
  projects and is unaffected.)
- **Raw hosted (Supabase) token handoff to the daemon.** The daemon receives
  only a `ProductJoinTicket`, never the full session token.
- **Cloud-owned membership.** Cloud directory rows are non-canonical
  projections; membership is a canonical daemon event.
- `InviteBundle` / `Pairing Notice` revival as a product join mechanism
  (already deprecated in `CONTEXT.md`, issue #344).

## Reconciliation Note

Each approved decision maps to its reconciled location in the glossary, ADRs,
runtime specs, user-journey docs, and the linked live designs.

| Approved decision (#765) | ADR | Runtime spec | Glossary | User-journey design |
| --- | --- | --- | --- | --- |
| 1. Creator = atomic Policy Owner + Product Owner | ADR-0022 (Product Owner authority boundary) | This contract, `Contract: product.create` | `CONTEXT.md` (`Product Owner`), `docs/domain/product-surfaces-and-integrations.md` (`Policy Owner`, `Product Owner`) | [Create Product](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=18-77) |
| 2. Same-domain auto-join grants Product Member only | ADR-0022, ADR-0025 (daemon-per-member) | This contract, `Contract: product.join` | `CONTEXT.md` (`Product Member`, `Cloud account`) | [Available Product and row-level Join](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=113-664) |
| 3. Narrow `ProductJoinTicket`; no full Supabase token | ADR-0025 (hosted-tier trust boundary), ADR-0020 (gateway authority) | This contract, `ProductJoinTicket — Minimum Bounded Claims`; `hosted-bridge-user-level-pairing.md` | `CONTEXT.md` (`Hosted cloud`) | [Hosted pairing](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=17-3) and [joining](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=114-2090) |
| 4. Daemon authoritative for identity/roles/membership/policy/events | ADR-0003, ADR-0007, ADR-0020 | This contract, `Runtime Invariant`; `explicit-project-registration-hosted-local-discovery.md` | `CONTEXT.md` (`Local daemon`, `Decision Ledger`) | [Member and availability ledger](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=113-664) |
| 5. Cloud publishes after daemon success; non-canonical, retryable | ADR-0025, ADR-0003 | This contract, `Cloud Publication`; `docs/releases/alpha/hosted-cloud-responsibility.md` | `CONTEXT.md` (`Hosted cloud`) | [Retryable directory/join state](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=114-2188) |
| 6. No durable pending-approval state | ADR-0023 (setup state machine) | This contract, `Lifecycle Minimization`; supersedes rendezvous invite/pending defaults | `CONTEXT.md` (`InviteBundle` legacy note) | [Wrong-domain terminal state](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=114-2287) |

Canonical [Figma Product ledger state set](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=112-585)
for all user-journey annotations in this contract.

## Related Context

- `CONTEXT.md` — repo-level glossary and terminology guardrail.
- `docs/domain/product-surfaces-and-integrations.md` — product-surface glossary
  (new: `Policy Owner`, `HostedProjectPreview`, `ProductJoinTicket`).
- ADR-0022 (Product Owner authority boundary), ADR-0023 (installation/setup
  state machine), ADR-0025 (hosted-tier trust boundary and SOC 2 control scope).
- `docs/specs/explicit-project-registration-hosted-local-discovery.md` —
  registration + hosted/local discovery contract.
- `docs/specs/hosted-bridge-user-level-pairing.md` — user-level pairing contract.
- `docs/specs/rendezvous-state-machine.md` — legacy invite/rendezvous flow
  (not the alpha onboarding default; see `Forbidden / Legacy Flows`).
- `docs/specs/project-setup-ui.md` — setup UI (Join tab reconciled to per-Product
  Join from Operator Home).
- `docs/releases/alpha/hosted-cloud-responsibility.md` — cloud responsibility
  cut line.
- Canonical [Figma Product ledger state set](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=112-585).

## Acceptance Criteria Mapping

| #765 acceptance criterion | Where satisfied |
| --- | --- |
| One linked source-of-truth: create, publish, discover, join, register, replay, reopen | This document (`Contract: product.create`, `Contract: product.join`, `Cloud Publication`, `Authority At Every Hop`), linking `explicit-project-registration-*` for register/replay/reopen |
| Contradictory invite/owner-approval/raw-token/Cloud-membership/global-join removed or marked legacy | `Forbidden / Legacy Flows`; reconciled headers in `rendezvous-state-machine.md`, `project-setup-ui.md`, `hosted-bridge-user-level-pairing.md`, `hosted-cloud-responsibility.md` |
| Creator role atomic: Policy Owner + Product Owner | `Contract: product.create`; `ProductCreationResult` fixture |
| Same-domain auto-join grants Product Member only | `Contract: product.join`; `ProductJoinResult` granted fixture |
| Cross-repo fixtures serialize deterministically; cover success + expired/wrong-domain/wrong-account/wrong-product/replayed-ticket | `protocol/onboarding/` fixtures + `scripts/validate_onboarding_contract_fixtures.py` |
| Idempotency and authority at every hop | `Authority At Every Hop` |
| User-journey design references use canonical live nodes | `Reconciliation Note`, `Related Context` |
