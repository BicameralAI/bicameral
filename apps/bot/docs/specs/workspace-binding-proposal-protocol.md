# Workspace Binding Proposal Protocol

Status: provisional alpha
Owner repo: `BicameralAI/bicameral-bot` (Rust types + generated JSON Schemas + fixtures)
Consumers: `bicameral-bot` daemon, hosted dashboard (`bicameral-cloud`), MCP thin client (`bicameral-mcp`)
Source issue: `BicameralAI/bicameral-bot#731`
Rust module: `crates/bicameral-api/src/workspace_binding.rs`

## Purpose

MCP-assisted local workspace binding lets an edge surface *propose* that the
local daemon bind an already-registered project to a local code folder for
grounding and preflight. This document defines the stable protocol contract and
conformance fixtures so downstream bot, hosted-dashboard, and MCP workers can
align **before daemon persistence lands** (persistence is out of scope for #731).

## Roles

- **MCP may propose binding.** The MCP thin client (in alpha,
  `source_surface: mcp`) authors a transient `WorkspaceBindingProposal` and
  dispatches a `workspace.bind` request through the local `ToolRequest` path.
- **The daemon may validate and materialize binding.** The local daemon is the
  only authority that validates the proposal and materializes the binding. Edge
  surfaces never write the binding.

## What a `WorkspaceBindingProposal` is *not*

A `WorkspaceBindingProposal` is **not**:

- **project identity** — identity still comes from explicit `ProjectRegistration`,
  never from ambient filesystem discovery;
- **`BindingEvidence`** — it never links a decision to a code symbol or range;
- **source authority** — it is not a `SourceBinding` or `SourceEvidence`;
- **cloud authority** — it grants no hosted/cloud membership or authority.

## Durable states vs. transient vocabulary

Durable local workspace states are kept to three (owned by the registration and
runtime-architecture specs):

- `local_workspace_unbound`
- `local_workspace_bound`
- `local_workspace_repair_required`

`WorkspaceBindingProposal` is **transient** protocol vocabulary — a DTO for a
bind request, never a durable lifecycle state or accepted evidence. No new
durable states are introduced.

## Field exposure: local-only vs. hosted-bridge-safe

| Field | Local-only | May cross the hosted bridge |
|---|---|---|
| `WorkspaceBindingProposal.candidate_path` | **yes** | **no** |
| `WorkspaceBindingProposal.project_id` | | yes (opaque id) |
| `WorkspaceBindingProposal.display` (`ProjectSafeDisplayMetadata`) | | yes (redacted display only) |
| `WorkspaceBindingProposal.source_surface` / `reason` / `confidence` | | yes (safe metadata) |
| `WorkspaceBindingState` | | yes (readiness) |

The hosted-safe projection is `WorkspaceBindingReadinessView`; it carries the
durable state and redacted display metadata and **never** the `candidate_path`
or any local absolute path. The local-only field set is exported as
`WORKSPACE_BINDING_LOCAL_ONLY_FIELDS`.

`ProjectSafeDisplayMetadata.candidate_label` is a non-path label (e.g. a folder
basename) and must never be an absolute path.

## Contract

| Object | Purpose |
|---|---|
| `WorkspaceBindingProposal` | Transient bind proposal authored by a local surface |
| `WorkspaceBindRequest` | `workspace.bind` request (proposal + explicit confirmation + optional preconditions) for local `ToolRequest` dispatch |
| `WorkspaceBindResponse` | Success — validated/materialized binding; project-safe fields only |
| `WorkspaceBindErrorResponse` | Typed failure (`WorkspaceBindErrorKind`) |
| `WorkspaceBindingReadinessView` | Hosted-safe readiness projection (no `candidate_path`) |
| `classify_bind_request` | Pure deterministic request → outcome mapping (no filesystem, no persistence) |

### Typed errors (`WorkspaceBindErrorKind`)

- `wrong_project` — proposal targets a project other than the session scope
- `unregistered_project` — target project is not registered locally
- `unsafe_path` — candidate path failed the local safety policy
- `already_bound` — project already has a materialized binding
- `confirmation_missing` — explicit operator confirmation was absent
- `daemon_capability_mismatch` — daemon lacks the required workspace-binding capability
- `repair_required` — binding is broken and must be repaired first

## Conformance fixtures

Under `protocol/schemas/v2/fixtures/`, one per outcome:

- `workspace-binding-unbound.json`
- `workspace-binding-bound.json`
- `workspace-binding-repair-required.json`
- `workspace-binding-rejected-unsafe-path.json`
- `workspace-binding-confirmation-missing.json`

Each fixture is self-contained: a `local` section (the `request`, plus its
`response` **or** `error`) and a `hosted_safe_view`. No `hosted_safe_view`
contains a local absolute path. Conformance is enforced by
`crates/bicameral-api/tests/workspace_binding_fixture_conformance.rs`, which
deserializes the fixtures, reproduces each outcome through `classify_bind_request`,
and guards the hosted-safe views against local paths.

## JSON Schemas

Generated from the Rust types via `schemars` (never edited by hand), under
`protocol/schemas/v2/`:

- `workspace-binding-proposal.schema.json`
- `workspace-bind-request.schema.json`
- `workspace-bind-response.schema.json`
- `workspace-bind-error.schema.json`
- `workspace-binding-readiness-view.schema.json`

Regenerate with:

```bash
WRITE_SCHEMAS=1 cargo test -p bicameral-gateway --test schema_gen
```

## Non-goals (#731)

- Implementing daemon persistence.
- Implementing MCP UI prompts.
- Implementing hosted dashboard UI.
- Browser File System Access API or cloud-side folder selection.

## Related

- `docs/specs/explicit-project-registration-hosted-local-discovery.md`
- `docs/specs/cloud-hosted-dashboard-local-daemon-runtime-architecture.md`
- `docs/specs/hosted-bridge-user-level-pairing.md`
- `docs/domain/runtime-protocol.md`
