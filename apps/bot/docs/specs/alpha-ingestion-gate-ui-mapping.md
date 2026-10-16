# Alpha Ingestion Gate UI Mapping

Date: 2026-06-18
Status: accepted alpha cut line
Level: L2 product/UI contract
Related: #407, #428, docs/specs/default-signal-policy-v0.md, docs/specs/mandatory-reasoning-candidate-projection.md

Governance gate: bic:spec-governance-gate advisory/no-op; Qor integration disabled pending factory policy revision.

## Purpose

Define where alpha surfaces show candidate review, rejected evidence,
projection-deferred state, and provider readiness without turning setup or
projection problems into fake candidate-review work.

## Decision

Alpha Ingestion Gate is a candidate-review surface only. Promotion from this
surface is deferred UX for alpha, not an architectural prohibition: the accepted
alpha first-Decision path is the daemon-authorized MCP RecallPacket promotion
(see `QUICKSTART.md` and `docs/validation/alpha-recall-real-process-topology.md`),
and the daemon remains the single promotion authority regardless of surface. Any
future Tool Surface must submit the same daemon-authorized request and owns no
acceptance or persistence authority.

- `candidate` is visible in the main queue.
- `rejected` is hidden from the main queue and appears only in a collapsed
  inspect-only panel.
- `logged_only` is not user-facing in alpha.
- `projection_deferred`, provider not configured, provider unavailable, and
  other projection setup/repair states appear in ingest/tool responses and
  Settings / Sources / Connections.
- `no_candidate` remains invisible in user-facing alpha UI except through
  debug, trace, or audit paths.

Do not add a warning lane, setup blocker lane, projection retry lane, or
no-candidate archive to the Ingestion Gate in alpha.

## Projection Visibility

When projection is unavailable or deferred, the response should name the
projection state and the next repair action. Product UI may surface the same
non-secret readiness in Settings / Sources / Connections so operators know
which provider setup needs attention.

User-facing language should say:

- candidate projection is not configured;
- the configured projection provider is unavailable;
- evidence was captured but no reviewable candidate was created yet; or
- setup must be repaired before candidate projection can run.

It must not say or imply:

- an item is waiting in candidate review when no DecisionCandidate exists;
- `no_candidate` is a rejected candidate;
- provider setup is SourceBinding approval;
- projection readiness is Decision Ledger authority, signoff, compliance, or a
  blocking work-gate result.

## Ingestion Gate Layout Contract

The page remains a split candidate queue/detail surface:

- left pane: searchable undecided candidate queue grouped by source/provider;
- right pane: selected candidate details, source reference, evidence excerpts,
  and governed review/reject actions;
- collapsed rejected panel below the main queue.

Queue counts reflect undecided candidates only. Accepted/promoted candidates
leave this page and appear in Ledger View. Rejected items do not count in the
main queue.

## State Refinement

Release state:

- candidate queue: undecided DecisionCandidates only;
- rejected panel: inspect-only rejected evidence/candidates;
- projection setup state: Settings / Sources / Connections plus ingest/tool
  responses;
- no-candidate: debug/audit only.

Transitions:

- daemon projection creates DecisionCandidates that enter the candidate queue;
- review commands accept/reject candidates through governed actions;
- projection provider configuration or repair changes projection setup state;
- no user action in the Ingestion Gate changes provider setup state.

Invariants:

- projection/setup state must not masquerade as candidate/rejected review state;
- no DecisionCandidate means no Ingestion Gate queue row;
- provider readiness does not grant source authority or Decision authority.

Minimization review:

- collapsed `projection_deferred`, provider missing, and provider unavailable
  into projection setup/repair state outside the queue;
- rejected a warning lane, blocker lane, retry lane, and no-candidate archive
  for alpha because their user workflow, persistence, and recovery behavior are
  not yet defined.

## Implementation Contract

Alpha can use existing candidate preview/detail DTOs for the Ingestion Gate:

- main queue: proposed/undecided candidates;
- rejected panel: rejected items;
- promoted/accepted items: excluded.

Projection state should be carried by ingest/tool responses and provider
readiness/config views. A later implementation may add a dedicated read model
for projection diagnostics after the product defines the workflow.
