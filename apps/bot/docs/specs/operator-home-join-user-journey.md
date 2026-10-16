# Operator Home — Per-Product Same-Domain Join (User Journey)

Issue: [bot#768](https://github.com/BicameralAI/bicameral-bot/issues/768).
Contract: [`domain-product-onboarding-contract.md`](domain-product-onboarding-contract.md).
Local dashboard setup spec (reconciled): [`project-setup-ui.md`](project-setup-ui.md).

The canonical [Figma Product ledger state set](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=112-585)
contains the live static UI specs and annotated journey frames in this document.
Every state below links directly to its source node; checked-in screenshot
copies are intentionally not used.

## Scope

The primary hosted Operator Home replaces the retired global/free-form
"Join Project" flow with **row-level, per-Product same-domain join**. This
document specifies the static UI for every row state and the screen-to-screen
journey across success, error, and continuation paths.

Out of scope (bot#768 non-goals): Cloud directory persistence, ticket signing,
daemon membership materialization, manual invitations, owner approval, quorum,
contradiction resolution, policy transfer, MCP onboarding.

## Authority model (why the UI looks like this)

- The **local daemon** is authoritative for Product identity, roles, membership,
  policy, and canonical events. Membership only exists after a daemon-canonical
  membership event + receipt.
- **Hosted Cloud** authenticates the account, evaluates same-domain discovery
  eligibility, renders a non-canonical `HostedProjectPreview`, and mints a
  narrow, short-lived `ProductJoinTicket`. Cloud never writes canonical
  membership, and a directory row is never treated as membership.
- The SPA carries only the narrow ticket to the daemon — never the full hosted
  session token — and moves a row from `Available` into `Your products` only
  after a `granted` daemon `ProductJoinResult`.

These invariants drive the two orthogonal groups and the "never claim membership
before daemon success" rule below.

## Static UI spec

### Header

- Title: **Products**.
- Secondary action **Create product** (canonical daemon create/publication;
  unchanged by this slice). Hidden in the empty state in favour of the primary
  empty-state Create button.
- No global `Join Project` button, team-ID field, invite-code input,
  pending-review, owner-approval, or approval-queue language anywhere in the
  primary hosted path.

### Groups

| Group | Membership source | Heading |
| --- | --- | --- |
| `Your products` | daemon-canonical member (`membership !== "available"`) | `Your products` |
| Available non-members | eligible same-domain, not joined (`membership === "available"`) | `Available for <verified-domain>` (falls back to `Available to join` when domains differ/absent) |

### Row action (single primary action)

| Row state | Action | Label |
| --- | --- | --- |
| available (eligible, not joined) | join | `Join` |
| member · ready · setup complete | open | `Open` |
| member · ready · setup incomplete | continue setup | `Continue setup` |
| member · needs access / broken local dependency | repair | `Repair` |
| member · offline | none | *(no unsafe primary action)* |

### Join affordance states (available rows)

- **Idle** — a `Join` button, `aria-label="Join <name>"`, keyboard focusable.
- **In flight** — the same button, `disabled`, `aria-busy="true"`, label
  `Joining…`. The row stays under `Available`; membership is never visually
  claimed. Duplicate submits are prevented (button disabled + parent guard).
- **Typed failure** — the row stays available and shows concise recovery copy
  (`role="alert"`). Retryable failures render a `Try again` button; the single
  non-retryable failure (`wrong_domain`) renders copy only.

### Recovery copy (typed Cloud/daemon failures → AC5)

| Code | Retryable | Copy |
| --- | --- | --- |
| `expired_ticket` | yes | That took too long, so the request expired. Try joining again. |
| `replayed_ticket` | yes | That join request was already used. Try joining again. |
| `wrong_product` | yes | This product's details changed. Refresh and try joining again. |
| `wrong_account` | yes | We couldn't confirm this is your account on this device. Sign in again, then try joining. |
| `wrong_domain` | no | This product isn't open to your email domain. |
| `unavailable` (transport) | yes | We couldn't reach your local Bicameral app. Make sure it's running, then try again. |

Copy is deliberately free of bridge/ledger/ticket/governance jargon.

## Annotated journey frames

Every success/error/continuation state below is represented by a live design
node and exercised from `FIXTURE_PROJECTS_DOMAIN_MIX`
(`operator-home-fixtures.ts`).

### Frame 1 — Empty state → Members + available (desktop)

[Empty state](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=114-622)
and [default grouped view](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=113-664).

- **Empty** (`No products yet`): guides the operator to **Create product** or
  wait for same-domain eligibility — no invite/team-id entry.
- **Default**: `Your products` lists members with `Open` / `Continue setup` /
  `Repair`; `Available for acme.example` lists eligible non-members with a
  row-level `Join`.

### Frame 2 — Join in flight → retryable failure (desktop)

[Joining in flight](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=114-2090)
and [retryable failure](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=114-2188).

- **Joining**: `platform-core`'s action is disabled and reads `Joining…`; the
  row remains under `Available` (no premature membership).
- **Retryable failure**: on a typed transient rejection the row stays available,
  shows the recovery copy, and offers `Try again`.

### Frame 3 — Retryable vs non-retryable failure (desktop)

[Retryable failure](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=114-2188)
and [wrong-domain terminal failure](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=114-2287).

- **`expired_ticket`** → recovery copy + `Try again`.
- **`wrong_domain`** → recovery copy only, no retry (not recoverable by retry).

### Frame 4 — Narrow / responsive layout

[Narrow responsive layout](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=116-331).

- At ≤640px the row table collapses to stacked rows; grouping, per-row actions,
  and the join affordance remain intact and accessible.

### Continuation frames (post-grant routing)

On a `granted` daemon result the SPA refreshes discovery and routes by derived
readiness (owned by `HostedDashboard.handleJoin`):

- ready + setup complete → **Open** the Product.
- ready + setup incomplete → **Continue setup** (local registration/provider
  binding).
- needs access → **Repair**.

These continuations reuse the member-row actions in Frame 1; the joined Product
appears under `Your products` only after the refresh reflects the daemon grant.

## Reconciliation note

- Supersedes the global/free-form Join, team-ID/invite entry, and
  pending/owner-approval flows in the primary hosted UI. Those affordances are
  removed from `OperatorHome`, `ProjectSelector`, and `HostedProjectRegistration`
  (create-only). The dev-only `SetupWizard` join tab is gated out of the
  production hosted build (see `production-hosted-gate.test.ts`) and is not part
  of the primary hosted path.
- The [Figma Product ledger state set](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=112-585)
  is the static-spec source of truth and each journey state must retain a direct
  node-specific link when the design changes.
