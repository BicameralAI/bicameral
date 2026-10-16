# Core Decision Lifecycle

This document contains Bicameral's essential decision-memory vocabulary: the
stable concepts that explain how observed product, code, and collaboration
signals become governed Decisions.

Use this file for terms that are central to the decision lifecycle itself. Do
not add every protocol shape, dashboard component, integration field, or helper
enum here. Those belong in runtime protocol or product surface vocabulary.

## Promotion Rule

Promote a term here only when it is part of the invariant lifecycle from
observation to governed memory:

1. Bicameral observes or receives a signal.
2. The daemon captures cited evidence and context.
3. Reasoning or deterministic projection may produce candidates or context.
4. Governance review accepts, rejects, demotes, refreshes, or signs off.
5. The Decision Ledger materializes accepted authority by replaying canonical
   events.

## Core Terms

`Observation`
: A bounded signal from local work, external systems, source refresh, or human
  input that may become evidence or context. An observation is not authority,
  not a Decision, and not automatically a candidate.

`Source`
: Mutable observed surface identified by URI or stable key, such as a tracker
  issue, transcript, MCP session, command run, pull request, file, diff, or code
  snapshot.

`SourceSnapshot`
: Immutable captured view of a Source, identified by content address.

`EvidenceReference`
: Citeable reference into a SourceSnapshot. It may point at text spans,
  transcript segments, tool calls, command output, file ranges, diff hunks, or
  code snapshot regions.

`SourceEvidence`
: Current concrete evidence pointer into one SourceSnapshot. It is a
  source-backed form of EvidenceReference.

`DecisionContext`
: Daemon-derived context assembled from observations, evidence references,
  known Decisions, candidates, graph readiness, and source state to help decide
  whether a claim should be proposed, reviewed, or retrieved. DecisionContext is
  not itself ledger authority.

`Reasoning Projection`
: Daemon-owned projection stage that may turn captured evidence plus advisory
  local drafts or integration hints into DecisionCandidate material. Projection
  output is a proposal with provenance; it is not a Decision, signoff,
  compliance result, BindingEvidence, or event-store authority.

`ProjectedCandidateProposal`
: Single candidate proposal produced by Reasoning Projection — a title, body,
  suggested level, and tags carried inside a ProjectionResult. It is pre-review
  projection output that may materialize as a DecisionCandidate only through the
  governed promotion path; it is not a Decision, signoff, compliance result,
  source approval, BindingEvidence, enforcement, merge-safety, or event-store
  authority.

`DecisionCandidate`
: Extracted claim awaiting governed promotion into the Decision Ledger.

`CandidateDraft`
: Caller-submitted draft claim inside an accepted local ingest request. It may
  inform Reasoning Projection and may materialize as a reviewable
  DecisionCandidate only after daemon projection, but it is not a tracked
  Decision, signoff, compliance result, or verified evidence.

`Decision`
: Reviewed claim or decision that has been promoted into the Decision Ledger.

`Tracked Decision`
: Promoted Decision whose cited evidence remains within Bicameral's governance
  freshness scope. In V1, candidate promotion makes a Decision tracked by
  default; signoff is a separate ownership lifecycle.

`Decision Ledger`
: Canonical materialized decision state derived by replaying the selected event
  store substrate.

`Signoff`
: Ownership lifecycle on a Decision, separate from candidate acceptance and code
  compliance. Future enforcement may require signoff as one input, but signoff
  alone is not enforcement.

`Governance policy`
: Rules that decide how candidates, review commands, and evidence route to
  review, advisory state, materialization, or enforcement.

`Governance fact`
: Accepted authority transition that affects Decision Ledger state, review
  state, compliance state, signoff, or enforcement.

`BindingEvidence`
: Verified relationship between a Decision and a code-shaped evidence target at
  a specific GraphSnapshot.

`BindingProposal`
: Runtime or codegraph prevalidation output that carries verified graph facts
  for a requested binding before governance acceptance.

`BindingHint`
: Caller-supplied advisory link between a claim or candidate and a code-shaped
  target. It is not binding authority; only bot-validated graph or symbol
  evidence may materialize accepted BindingEvidence.

`GraphSnapshot`
: Named code snapshot against which local or hosted graph evidence is
  validated. In git-backed workspaces this is anchored by repository identity,
  commit SHA, and graph index version; branch names are provenance, not durable
  identity.

## Lifecycle Verbs

`Promote`
: Governed transition that turns a DecisionCandidate into a Decision Ledger
  record.

`Demote`
: Governed Ledger View transition that lowers a Decision's authority without
  recreating it as a candidate.

`Source Refresh`
: Recapture or re-index a Source so Bicameral can compare a newer
  SourceSnapshot or source-specific readiness state against previously cited
  evidence.

`Untrack Source`
: Governed transition that stops automatic Source Refresh for a Decision. It
  does not delete the Decision, remove historical evidence, or imply that the
  cited Source was shredded.

## Boundaries

- Observations may inform DecisionContext but must not silently become
  Decisions.
- DecisionContext can explain, rank, retrieve, or propose; it cannot materialize
  authority.
- CandidateDrafts and integration candidate hints are advisory inputs and must
  not materialize DecisionCandidates without daemon-owned Reasoning Projection.
- Candidates are non-canonical until governance policy accepts a review command
  and the selected event store substrate materializes the event.
- Read models, dashboards, search indexes, hosted graphs, and projections are
  not durable authority.
