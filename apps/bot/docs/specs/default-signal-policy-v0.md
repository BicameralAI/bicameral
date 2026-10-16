# Default Signal Policy v0

Date: 2026-06-18
Status: accepted alpha cut line
Level: L2 product/runtime contract
Related: #423, #428, docs/specs/alpha-ingestion-gate-ui-mapping.md

Governance gate: bic:spec-governance-gate advisory/no-op; Qor integration disabled pending factory policy revision.

## Purpose

Define the smallest honest alpha routing contract for captured evidence before
candidate review.

Alpha does not yet have a distinct warning lane, a logged-only diagnostics
surface, a policy tuning workflow, or a promote-from-warning/rejected workflow.
The policy therefore routes only captured evidence items and defers richer
advisory signal classification to a later stage.

## Scope

This spec owns:

- alpha item-route vocabulary for captured evidence;
- default mapping from producer emission type to bot-owned route;
- bot-owned hard reject gates;
- the boundary between item routing and upstream blocked/setup state.

This spec does not own:

- source, connection, SourceBinding, or projection-provider lifecycle state;
- Ingestion Gate layout or component design;
- workspace/team policy profiles;
- policy simulation or user-tuned routing;
- durable semantics for `routing_hint`, confidence surfaces,
  `advisory_governance_result`, or `noisy_source_gate`.

## Decision

`DefaultSignalPolicy v0` routes only captured evidence items.

Alpha route vocabulary:

- `candidate`
- `logged_only`
- `rejected`

`action_needed`, blocked refresh, disconnected providers, missing scope,
projection provider unavailable, projection deferred, and other upstream repair
states are not item routes. They belong to connection, source, SourceBinding,
or projection setup state and may prevent item creation entirely.

## Invariants

- Only bot-owned policy chooses the final route.
- Integrations, mods, connectors, and descriptors may classify or recommend,
  but they do not create accepted Decisions, approve signoff, resolve
  compliance, create blocking enforcement, or write canonical events directly.
- Signal Policy routes only captured evidence items. If the system does not
  have the evidence payload, it must surface upstream blocked/setup state
  instead of inventing an item-level route.
- Alpha does not maintain a separate warning or attention route.

## Default Routing Table

| Producer input | Default bot route |
| --- | --- |
| `emission_type = candidate` | `candidate` |
| `emission_type = evidence` | `logged_only` |
| `emission_type = hint` | `logged_only` |
| `emission_type = advisory` | `logged_only` |
| malformed payload | `rejected` |
| sensitive-data hard gate | `rejected` |

The producer emission type is the only normative routing input in alpha.

## Advisory Metadata Boundary

`routing_hint`, confidence surfaces, `advisory_governance_result`, and
`noisy_source_gate` may be preserved as provenance or debug metadata, but alpha
does not commit to interpreting them as a first-class routing contract.

Allowed alpha effects:

- preserve the fields for audit/debug or later experimentation;
- support bot-owned hard reject gates;
- support bot-owned suppression from candidate to `logged_only` when a future
  implementation wants a conservative local noise gate.

Forbidden alpha effects:

- promoting `evidence`, `hint`, or `advisory` into `candidate` solely because a
  hint or confidence score says so;
- manufacturing a separate warning queue without a defined user workflow;
- treating advisory metadata as source/setup blocked state;
- allowing mod-authored policy to create canonical authority.

## Upstream Blocked State

The following are not item routes:

- provider disconnected or token expired;
- missing runtime configuration;
- missing or invalid SourceBinding;
- provider/source unavailable before evidence capture;
- insufficient scope to fetch the evidence payload;
- projection provider unavailable or projection deferred.

These conditions should surface as connection, source, setup, or projection
repair state. In many cases they prevent a captured item from existing at all.

## Next Stage

Richer advisory signal classification is deferred until Bicameral has a real
review workflow for it. The next stage may add a distinct attention/review lane
for captured evidence items whose advisory severity should become visible work.

That stage must define:

- the new route or state name;
- the user action it enables;
- where the item is stored and inspected;
- how it differs from `candidate`, `logged_only`, and `rejected`;
- how source/setup/projection blocked state remains separate.
