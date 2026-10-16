# ADR-0010: Snapshot-Scoped Local Code Graph Contract

**Date:** 2026-05-30  
**Status:** proposed  
**Level:** L1  
**Extends:** ADR-0003 local code grounding boundary and ADR-0008 protocol ownership

## Problem

Local code grounding must be faithful. A Bicameral code graph answer is only useful if the caller knows which repository snapshot it describes and whether the claim was verified from that snapshot.

Recent MCP failures exposed the missing contract:

- `validate_symbols` can report a match from a local SQLite index built at one effective commit.
- `bind` can then fail because it resolves through `git show` and tree-sitter at a different effective ref or commit.
- The caller sees a green validation result followed by a rejected bind, with no typed way to detect the snapshot mismatch before materialization.

This is the failure shape behind:

- [`bicameral-mcp` issue #334](https://github.com/BicameralAI/bicameral-mcp/issues/334): `validate_symbols` and `bind` disagree on indexed snapshot.
- [`bicameral-mcp` issue #332](https://github.com/BicameralAI/bicameral-mcp/issues/332): `bind` validates at authoritative SHA and rejects symbols introduced on feature branches after `link_commit`.
- [`bicameral-mcp` issue #280](https://github.com/BicameralAI/bicameral-mcp/issues/280): binding must not fall back to imprecise grounding paths that create plausible but wrong decision edges.

The root problem is not a particular storage engine. The root problem is that graph facts, search candidates, validation results, and binding materialization have not carried the same explicit snapshot identity and evidence semantics.

## Decision

Bicameral code graph claims are evidence-bearing claims over a named snapshot.

The local graph system may be incomplete, lazy, or backed by caches, but it must not present stale, approximate, or cross-snapshot evidence as a verified graph fact. When the local system cannot prove a graph claim for the requested snapshot, it returns an explicit non-verified state instead of substituting a best-effort answer.

The local daemon owns local graph indexing and exact local graph validation. MCP, integrations, and other agent-facing surfaces act primarily as clients that request graph/search/validation operations and receive protocol-shaped evidence.

Graph storage may be content-addressed, incremental, lazy, and shared across snapshots. The API contract is snapshot-scoped; it does not require a full physical graph copy per branch or per commit.

Graph indexes are not canonical event-store artifacts. The local daemon may
persist them in a cache, SQLite database, content-addressed index store, or
other implementation-specific graph store, but accepted governance events store
only reviewed graph evidence and snapshot identity. The git-backed event store
must not grow a canonical `.bicameral/graphsnapshots/` directory for graph
internals.

The code graph validates code facts; it does not own Bicameral's durable
decision-to-code relationship semantics. A verified graph fact may support a
binding proposal, but the selected event store substrate is the only place where
that proposal becomes accepted `BindingEvidence`. SurrealDB materializes those
accepted binding events into queryable enforcement-memory edges, such as
`decision -binds_to-> code_region`, so preflight, Ledger View, compliance reads,
and architecture-viability evaluation can traverse them efficiently.

## Definitions

### GraphSnapshot

A `GraphSnapshot` identifies the code snapshot against which graph claims are made.

Required identity:

- `graph_snapshot_id`
- `repo_id`
- `commit_sha`
- `index_version`

Required provenance when known:

- `ref_name`
- `merge_base_sha`
- `indexed_at`
- `index_source`: `local_daemon | hosted | imported`

`graph_snapshot_id` may be derived from `repo_id`, `commit_sha`, and `index_version`, or assigned by a graph service that preserves those fields. `commit_sha` is the durable snapshot anchor for git-backed repositories. `ref_name` is provenance and user-facing context, not durable identity, because branch names move.

### Symbol

A `Symbol` is a stable-ish program identity, such as:

- language
- qualified name
- file path
- structural signature, when available

It is not by itself proof that the symbol exists at a particular snapshot.

### SymbolOccurrence

A `SymbolOccurrence` is a symbol observed at a specific `GraphSnapshot`.

It carries:

- `graph_snapshot_id`
- file path
- line/range
- blob or content hash
- structural hash, when available
- parser/resolver evidence metadata

### BindingEvidence

`BindingEvidence` is a governance/evidence object that records an accepted
relationship between a decision or candidate and either a verified
`SymbolOccurrence` or a stable symbol identity plus the snapshot where that
identity was validated.

It carries:

- decision or candidate id
- `graph_snapshot_id`
- `validated_ref`
- `validated_sha`
- occurrence evidence or stable symbol evidence

`BindingEvidence` is the canonical record that a verified graph fact influenced
Ledger state. It stores the graph snapshot identity and reviewed evidence
needed for audit; it does not store the full graph index. The code graph does
not own this record after acceptance. The event store replays it, and the
Enforcement Memory Projection turns it into operational relationships for
queries and views.

This ADR governs the separate Code Evidence/Compliance lifecycle. It does not
define SMR alpha completeness, closure, absence, contradiction, all-clear, or
accepted claim authority. `ScopeSufficiencyProof`, `GraphEvidenceState`, and
graph readiness may remain required for code-evidence claims, but CodeGraph is
only optional recall-expansion provenance for SMR alpha (ADR-0028).

### BindingLineage

A `BindingLineage` relates binding observations for the same Decision target
across multiple `GraphSnapshot`s.

Lineage is not the same thing as one accepted `BindingEvidence` record. A
`BindingEvidence` record is evidence that a target was verified at one snapshot.
Lineage is the derived or governed claim that later observations are the same
continuing target rather than a new target, an ambiguous move, or a divergent
branch-local binding.

## Graph Validation Failure Modes

Graph and validation APIs must distinguish evidence state from ranking confidence.

ADR-0010 is the single source of truth for graph readiness and graph evidence
failure modes. Other ADRs may describe how a product surface reacts to these
states, but they should not redefine the state meanings.

### GraphReadiness

Graph readiness is checked before a Ledger View compliance or grounded-conflict
read. It applies to the requested snapshot scope for the view, not to every
Decision row individually.

| `GraphReadiness.state` | Meaning | Automatic daemon action | Surface action | Required actor |
|---|---|---|---|---|
| `ready` | Required graph snapshot is available for the requested compliance or grounded read. | None. | Show grounded compliance, bindings, and grounded conflict state normally. | None. |
| `indexing` | Snapshot or graph neighborhood is being built. | Continue indexing or lazy expansion. | Show view-level indexing state and allow status refresh. | Daemon. |
| `stale` | Index exists but does not match the requested commit, index version, parser version, or resolver version. | Revalidate or rebuild the graph snapshot. | Show view-level stale state; offer rebuild, fetch base ref, or compute merge-base when needed. | Daemon; workspace member when fetching refs is required. |
| `failed` | Indexing or revalidation failed. | None after failure, except retry when recoverable. | Show the failure reason, offer retry, and allow source-only review when policy permits. | Daemon or workspace member, depending on the failure. |
| `not_configured` | Local graph support is unavailable for this workspace. | None. | Configure local graph support, request hosted/team graph check, or proceed source-only when policy permits. | Workspace member or admin. |

### GraphEvidenceState

Allowed graph evidence states:

- `verified`: proven against the requested snapshot.
- `not_found`: the relevant snapshot and required files were inspected, and the symbol or edge is absent.
- `unknown_not_indexed`: required files or neighborhoods have not been parsed or resolved.
- `unknown_stale`: available index data does not match the requested snapshot, parser version, or index version.
- `ambiguous`: multiple occurrences match and the API cannot select one without more caller input.
- `unsupported`: language, file type, generated artifact, or resolver behavior is outside supported exact validation.
- `approximate_candidate`: search or ranking found a possible match, but it is not verified graph evidence.

Only `verified` graph claims may be accepted as `BindingEvidence`, support
compliance conclusions, or produce blocking governance results. The code graph
does not materialize those outcomes by itself; policy/review and the selected
event store substrate do. `approximate_candidate`, `unknown_*`, `ambiguous`,
and `unsupported` results may suggest next steps, but they cannot be promoted
into verified `BindingEvidence`.

Evidence states map to product behavior as follows:

| `GraphEvidenceState` | Classification | Retryable | Surface behavior | Required actor |
|---|---|---|---|---|
| `verified` | Positive verified evidence. | No. | May support accepted `BindingEvidence`, compliance conclusions, or blocking governance results when policy accepts the evidence. | None unless policy requires review. |
| `not_found` | Verified negative evidence: the requested snapshot was inspected and the target symbol, file, edge, or range was absent. | Yes, with a different target or snapshot. | Show that the inspected target was absent. If that contradicts the intended binding, offer binding refinement, retry at another snapshot, or source-only review. | Reviewer or workspace member only when correction is needed. |
| `unknown_not_indexed` | Usually a readiness or lazy-expansion gap. | Yes. | Queue indexing or lazy expansion and show view-level `indexing`, unless the unresolved gap remains scoped to one Decision after retry. | Daemon first. |
| `unknown_stale` | Usually a readiness or revalidation gap. | Yes. | Rebuild or revalidate the snapshot and show view-level `stale`. If accepted binding evidence no longer matches its declared snapshot, mark the Decision `needs_grounding_review`. | Daemon first; reviewer for mismatched accepted evidence. |
| `ambiguous` | Decision-specific grounding issue. | Yes, with disambiguation. | Mark the Decision `needs_grounding_review`, show candidate occurrences, and allow the reviewer to choose an occurrence or refine the binding hint. | Reviewer or workspace member. |
| `unsupported` | Capability limit for exact local validation. | Sometimes. | Block verified binding or compliance from this evidence. If tied to a Decision binding, show `needs_grounding_review`; allow hosted check, source-only/advisory review, or manual binding revision. | Reviewer, workspace member, or admin depending on the action. |
| `approximate_candidate` | Search hint, not graph evidence. | Yes, by exact validation. | Show as a suggestion only. Allow exact validation or query refinement. It must not materialize a binding or produce a blocking governance result. | Daemon or reviewer. |

Graph readiness and graph evidence states are graph facts unless policy or a
reviewer accepts them as governance facts. A failure mode such as `stale`,
`unknown_not_indexed`, or `ambiguous` must not by itself become a durable
Decision lifecycle event.

## Graph Facts vs Governance Facts

A graph fact is a recomputable claim about code evidence at a `GraphSnapshot`.
For example: "this binding still verifies at snapshot X" or "this symbol is
not found at snapshot Y." Graph facts belong in graph stores, local caches,
view models, preflight output, and advisory UI unless they are accepted as
shared authority.

A governance fact is an accepted authority transition that affects Decision
Ledger state, review state, compliance state, signoff, or enforcement. For
example: "this verified binding was accepted as `BindingEvidence`," "this
revalidation resolved `needs_grounding_review`," or "this compliance verdict is
safe enough to block or unblock a merge." Governance facts must be materialized
through the selected event store substrate so they replay from the Decision
Ledger.

Successful revalidation at a later snapshot is therefore derived view state by
default. It becomes durable only when policy or a reviewer accepts it as shared
authority. The original `BindingEvidence` remains immutable and continues to
describe the snapshot where it was first verified; later snapshots may produce
derived currentness, a durable revalidation/compliance event, or a grounding
review state, but they must not rewrite the original evidence.

## Graph Facts vs Governance Facts

A graph fact is a recomputable claim about code evidence at a `GraphSnapshot`.
For example: "this binding still verifies at snapshot X" or "this symbol is
not found at snapshot Y." Graph facts belong in graph stores, local caches,
view models, preflight output, and advisory UI unless they are accepted as
shared authority.

A governance fact is an accepted authority transition that affects Decision
Ledger state, review state, compliance state, signoff, or enforcement. For
example: "this verified binding was accepted as `BindingEvidence`," "this
revalidation resolved `needs_grounding_review`," or "this compliance verdict is
safe enough to block or unblock a merge." Governance facts must be materialized
through the selected event store substrate so they replay from the Decision
Ledger.

Successful revalidation at a later snapshot is therefore derived view state by
default. It becomes durable only when policy or a reviewer accepts it as shared
authority. The original `BindingEvidence` remains immutable and continues to
describe the snapshot where it was first verified; later snapshots may produce
derived currentness, a durable revalidation/compliance event, or a grounding
review state, but they must not rewrite the original evidence.

## Binding Lineage Across Snapshots

An accepted `BindingEvidence` record is valid as verified graph evidence only
for the `GraphSnapshot` where it was produced. It may be used as the starting
point for later revalidation, but it must not be treated as automatically
verified at future snapshots.

When the same target still verifies at a later snapshot, the daemon may present
that as derived currentness in a view or preflight response. It should create a
durable event only when policy or reviewer action needs shared authority: for
example, resolving compliance, clearing `needs_grounding_review`, recording
reviewer-accepted revalidation, or supporting a merge-blocking governance
result. Durable history accumulates accepted binding and revalidation facts; it
does not mutate or replace old binding evidence.

When the target moves, the graph layer may call it the same lineage only if it
has verified evidence at the later snapshot plus enough identity continuity to
distinguish a move from a different target. Depending on binding type and
language support, that continuity may include:

- the previous `binding_id` or lineage root the caller is revalidating;
- original and current `graph_snapshot_id`;
- original and current `validated_sha`;
- stable target kind and binding type;
- file path, range, and content hash before and after the move;
- symbol name and qualified name when available;
- structural hash or resolver-specific symbol identity;
- parser and resolver versions or capture metadata;
- evidence state and validator identity.

If the target content hash or structural hash still matches after a rebase, but
`validated_sha` changes, the graph result is not stale solely because the commit
changed. If validation runs against the rebased snapshot and returns
`verified`, the later observation is verified for that later snapshot. If the
system only has an old validation token or old index for the pre-rebase SHA, the
result is `unknown_stale` or a snapshot mismatch until revalidation runs at the
new snapshot.

If two branches bind the same Decision to different symbols, ranges, or files,
the graph layer should classify that as divergent binding evidence or parallel
lineage, not as a conflict by itself. It becomes a conflict only when governance
or policy accepts evidence that the divergent bindings represent incompatible
Decision meaning, implementation behavior, compliance, or scope. If the
available graph evidence cannot distinguish a move from two possible targets,
the graph state is `ambiguous` and should route to grounding review rather than
`collision_pending`.

The minimum durable payload for auditable binding lineage should include the
fields already required for one binding event plus lineage-aware identifiers and
before/after evidence when a revalidation or replacement is materialized:

- decision id;
- binding id and, when present, lineage root or prior binding id;
- binding type and target kind;
- original and current `graph_snapshot_id`;
- original and current `validated_sha`;
- evidence state;
- file path, line range, and content hash;
- symbol name and qualified name when the binding is symbol-based;
- structural hash or resolver-specific symbol identity when available;
- parser/resolver version and capture metadata when available;
- actor, validator, timestamp, and reviewer/governance result id for durable
  events.

These payload fields preserve replay and audit without persisting the full graph
index. The exact event shape may be a future `RevalidateBinding` event, a
`ResolveCompliance` event carrying binding-verdict evidence, or another
governed event type. Whatever shape is chosen must preserve the distinction
between graph facts and governance facts.

## Required API Contract

### validate_symbols

`validate_symbols` must validate against a named or resolved `GraphSnapshot`. If the caller does not supply a snapshot, the daemon must resolve one first, usually from the active checkout `HEAD`, and return that resolved identity.

Its response must include:

- evidence state
- `validated_ref`
- `validated_sha`
- `graph_snapshot_id`
- matched `SymbolOccurrence` id when verified
- path/range/blob evidence when verified
- parser or resolver evidence metadata

`validate_symbols` may use search to find candidates, but search output remains `approximate_candidate` until exact validation runs against the requested snapshot.

### bind

`bind` must materialize only verified graph evidence.

It must accept one of:

- a verified occurrence or validation token returned by validation; or
- a symbol/path request plus explicit snapshot identity, which `bind` revalidates before materializing.

Validation tokens must bind to at least `graph_snapshot_id`, `validated_sha`,
validated symbol or occurrence identity, validator identity, evidence state,
issue time, and expiry time. They must be integrity-protected by the issuing
validator, and `bind` must verify the token integrity, snapshot identity, and
`evidence_state = verified` before materializing. Expired, revoked, mismatched,
or non-verified tokens must be rejected or revalidated.

If validation occurred at one snapshot and binding is requested at another, `bind` must reject with a typed snapshot mismatch or revalidate at the requested snapshot. It must not silently reinterpret a successful validation result against a different ref or commit.

### preflight

`preflight` must report the graph snapshot scope used for local code grounding and distinguish verified graph findings from unknown or approximate findings.

Preflight may warn on potential conflicts, but blocking local graph claims require verified evidence over the relevant snapshot.

### graph readiness

Compliance read actions that depend on local graph evidence must run through a
graph readiness check before presenting compliance or grounding state. This
includes loading Ledger View sections that display code compliance, drift,
grounded conflict results, or bound code regions.

Graph readiness is a view/workspace precondition, not a per-Decision status
sprinkled across every row. If required snapshots are not indexed, stale, or
being built, the review surface should show an explicit graph-indexing or
graph-refreshing state for the relevant view. If the readiness check fails, the
surface must fail loudly and avoid presenting stale compliance as current.

Individual Decisions need secondary grounding status only when the problem is
specific to that Decision's evidence, such as:

- ambiguous symbol occurrence selection for that Decision;
- unsupported language or generated artifact for that Decision's binding;
- a binding token or `BindingEvidence` record that no longer matches its
  declared `GraphSnapshot`;
- a hosted/team-only graph result attached to that Decision that is advisory or
  unavailable locally.

General graph indexing progress, graph freshness, or missing base/merge-base
snapshots should be reported at the view/readiness level.

## Minimum Local Snapshot Set

The local daemon must be exact about what it can see, not broad by default.

The minimum local set is:

- current `HEAD` for the active checkout;
- authoritative base ref when it is locally available;
- merge-base between `HEAD` and authoritative base when running base-vs-HEAD or branch-delta checks.

The local daemon is not responsible for seeing every branch or every developer's private work unless those refs are locally present and the requested snapshot can be resolved.

## Branch Visibility Boundary

Local binding visibility is snapshot-scoped, not branch-scoped. A branch or PR
name may help the caller ask for the intended code state, but the graph layer's
verified answer is always about a resolved `GraphSnapshot`. In git-backed
workspaces that means `repo_id`, `commit_sha`, and `index_version`; `ref_name`
is provenance because refs move.

When a caller requests a branch, PR head, base ref, merge-base, or other ref,
the local daemon must first resolve it to a snapshot it can inspect. If the ref
is not locally available, cannot be resolved, requires hosted/team branch
visibility, or resolves to an index that is stale for the requested parser or
resolver version, the result is a snapshot-resolution or `GraphReadiness`
failure. It must not be reported as `verified`, and it must not be collapsed
into `not_found`. `not_found` is reserved for the case where the requested
snapshot was available and inspected and the target symbol, file, edge, or range
was absent.

The local branch-spanning checks that may run without hosted/team visibility
are checks over locally resolved snapshots:

- `HEAD` grounding;
- authoritative-base grounding when that ref is present locally;
- merge-base grounding when both sides are locally resolvable;
- base-vs-`HEAD` drift or branch-delta checks over those snapshots;
- same-symbol or same-occurrence contradiction checks over those snapshots.

Active PR visibility, team-wide branch manifests, other developers' private
branches, stale-branch sweeps, cross-repo branch analysis, and Conflict Oracle
behavior require hosted/team graph visibility unless every requested snapshot is
already present and resolvable locally. Hosted/team graph output remains graph
evidence, advisory evidence, or a proposed `GovernanceResult`; it does not
become durable Decision Ledger authority unless bot governance accepts and
materializes the resulting event through the selected event store substrate.

Drive-backed or basic-store workspaces may still bind Decisions to code by
selecting an explicit git-derived `GraphSnapshot` or hosted/imported equivalent.
The Drive event store preserves the accepted Decision Ledger event, binding
evidence, and snapshot identity, but Drive does not add branch-local proposal
isolation, PR-native review, native merge conflict resolution, or CI merge-gate
enforcement. Those are Git substrate capabilities, not different graph evidence
semantics.

Git-backed workspaces may surface branch-local binding events in PR review and
may turn policy-accepted blocking governance results into CI or merge-gate
behavior when the adapter advertises that capability. The graph evidence
underneath is the same evidence vocabulary used by Drive and other substrates:
verified graph facts may block when policy accepts them; missing local branch
visibility, hosted-only scope, stale indexes, unsupported language, ambiguity,
or approximate matches remain readiness/advisory states until explicitly
accepted through governance.

## Free / Local Product Boundary

Free/local Bicameral includes exact grounding for locally available snapshots:

- current-checkout code grounding;
- validate/bind/preflight against a known local snapshot;
- local drift checks;
- base-vs-HEAD checks when the base ref and merge-base are locally available;
- simple same-symbol or same-occurrence contradiction checks across locally available snapshots;
- explicit unknown states when local visibility or index coverage is insufficient.

Local results must be faithful before they are convenient. A local install may defer indexing, request expansion, or return unknown; it must not pretend to have team-wide branch visibility.

## Hosted / Team Product Boundary

Hosted/team graph services own broader visibility and continuously refreshed multi-ref graph work:

- active PR branch indexing;
- cross-branch conflict detection across developers' branches;
- CI/PR merge-blocking integration;
- team-wide stale branch detection;
- continuously refreshed graph snapshots and branch manifests;
- Conflict Oracle behavior over multiple developers' branches;
- expensive graph-neighborhood expansion, historical analysis, and organization-scale dependency intelligence.

Hosted graph services still return evidence, hints, advisories, and `GovernanceResult` proposals. They do not become durable decision authority; bot governance and the selected event store substrate still decide what materializes.

## Substrate Neutrality

Git commits are snapshot locators for git-backed repositories, not the only possible durable authority.

The graph contract must allow future sources such as hosted graph snapshots, archive snapshots, generated source packages, or non-git substrates to provide equivalent snapshot identity and evidence metadata. Git-specific fields such as `commit_sha`, `ref_name`, and `merge_base_sha` are used when the source is git.

## Consequences

Positive:

- `validate_symbols`, `bind`, and `preflight` become an honest check-then-materialize workflow.
- Callers can detect stale, mismatched, unsupported, or incomplete graph evidence before binding.
- Local/free product value remains real without promising team-wide hosted visibility.
- Storage can stay efficient because snapshot-scoped APIs do not imply a full graph copy per branch.

Negative / risks:

- Protocol and MCP APIs must grow snapshot and evidence-state fields.
- Callers must handle unknown and mismatch responses instead of assuming every query returns a ranked answer.
- Lazy indexing will create cold-query paths where the right answer is "unknown until expanded."
- Parser/resolver limitations become explicit product behavior rather than hidden implementation detail.

## Non-Goals

This ADR does not define:

- the full hosted Conflict Oracle implementation;
- a physical graph database schema;
- a full graph-per-branch storage model;
- parser internals or language-specific resolver behavior;
- search ranking quality;
- a guarantee that free/local installs can see other developers' branches;
- imprecise fallback binding that bypasses exact snapshot validation.

## Follow-up Implementation Issues

1. Add `validated_ref`, `validated_sha`, `graph_snapshot_id`, and evidence state to `validate_symbols`.
2. Add explicit snapshot input or validation-token input to `bind`.
3. Make `bind` reject or revalidate snapshot mismatches instead of silently changing effective refs.
4. Add preflight output fields that distinguish verified graph facts from unknown and approximate candidates.
5. Add protocol conformance fixtures for validate-then-bind on `HEAD`, authoritative base, feature branch `HEAD`, and merge-base snapshots.
6. Define exact lazy expansion behavior for local graph indexing without requiring full per-branch graph materialization.
7. Create parser/resolver accuracy fixtures for supported languages and explicit `unsupported` behavior for unsupported constructs.
8. Define hosted multi-branch graph indexing and Conflict Oracle accuracy policy in separate ADRs or design docs.

## References

- [`bicameral-mcp` issue #334](https://github.com/BicameralAI/bicameral-mcp/issues/334): validate/bind snapshot disagreement.
- [`bicameral-mcp` issue #332](https://github.com/BicameralAI/bicameral-mcp/issues/332): feature-branch symbols rejected by authoritative-SHA bind.
- [`bicameral-mcp` issue #280](https://github.com/BicameralAI/bicameral-mcp/issues/280): imprecise binding regression and the requirement for verified grounding.
- [Git object model](https://git-scm.com/book/en/v2/Git-Internals-Git-Objects): commits point to tree snapshots, making commit SHA a natural git-backed snapshot anchor.
- [Martin Fowler on Architecture Decision Records](https://martinfowler.com/bliki/ArchitectureDecisionRecord.html) and [Thoughtworks on lightweight ADRs](https://www.thoughtworks.com/radar/techniques/lightweight-architecture-decision-records): ADRs should capture context, decision, and consequences without becoming full implementation specs.
- [Code Property Graph specification](https://cpg.joern.io/): code graph literature describes program facts as graph data, but snapshot identity remains a product/API contract Bicameral must define.
