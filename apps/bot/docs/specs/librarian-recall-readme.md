# Librarian Recall Requirements

Date: 2026-06-19
Status: draft requirements note

## Summary

the design partner's "librarian" requirement reframes Bicameral as a reliable decision recall
and HITL decision-making layer. Bicameral should surface relevant reviewed
constraints before work starts, flag possible contradictions, and route changes
through governed review. It should not auto-update the source of truth or turn
agent guesses into authority.

Core invariant:

```text
The librarian may retrieve, explain, and flag contradictions over the configured
reviewed corpus. It may not invent authority, claim global completeness, or
update canonical Decisions without governed review.
```

## Engineering Requirements

### 1. Corpus-Scoped Source Of Truth

- Canonical state remains the selected event-store substrate, likely Git for
  the design partner's workflow.
- Git markdown docs are the reviewed corpus, not just scraped context.
- Only approved review and materialization paths can change canonical Decisions.
- Local DB, MCP cache, search index, dashboard, and projections are derived
  state.

### 2. Librarian Recall Contract

- Add a first-class recall/preflight contract: given a task, ticket, diff, PR, or
  session, return all relevant reviewed constraints within the configured corpus.
- The response must include matched Decisions, evidence references, relevance
  reasons, searched corpus/version, freshness/readiness, and `unknown_scope`
  when completeness cannot be guaranteed.
- Completeness means "complete for the configured reviewed corpus and declared
  scope," not global organizational omniscience.

### 3. MCP Session-Start Preflight

- MCP should expose the librarian as the agent's first runtime step.
- `preflight.run` should perform constraint lookup before implementation.
- `search.query` can support follow-up recall.
- `ingest.submit_local` can submit newly discovered assumptions or evidence.
- MCP remains a thin client. Bot owns validation, search/read models, policy,
  and materialization.

### 4. Constraint Packaging

Reviewed constraints should be packageable as structured Decision context:

- decision text;
- rationale;
- rejected alternatives;
- source evidence;
- feature or product scope;
- tenant, security, billing, deployment, or similar risk tags;
- code-touchpoint hints where available;
- grounding state such as `source_only`, `grounded_local`, or `unknown`.

Source-only constraints must be useful without pretending they are verified code
compliance facts.

### 5. HITL Decision-Making

- New candidate constraints from sessions, PRs, Linear, Slack, or changelogs
  enter as `DecisionCandidate`, not as Decisions.
- A Product Owner or authorized reviewer decides whether to accept, reject,
  approve signoff, reopen, or supersede.
- Automated extraction may recommend, summarize, and route. It must not silently
  promote.

### 6. Contradiction Monitor

- Linear and Slack monitoring should observe statements and compare them against
  known Decisions.
- Possible contradictions become candidates, collision evidence, or review
  items.
- The monitor may notify or queue triage.
- It must not auto-edit Git docs or auto-update the Decision Ledger.

### 7. Noise Policy

Default behavior should be conservative in claims and selective in interruptions.

Interrupt when:

- a task is about to act on a relevant constraint;
- a contradiction touches high-risk scope;
- evidence is stale but action depends on it;
- an accepted Decision is likely contradicted.

Suppress or log low-signal uncertainty instead of creating a speculative review
queue.

### 8. Freshness And Proof

- Accepted Decisions are tracked by default.
- The system should know whether cited sources are fresh, stale, unavailable, or
  outside the configured corpus.
- Recall results need a proof of searched scope: corpus identity, version,
  indexed sources, skipped sources, and unresolved visibility.

### 9. Metrics

Primary metric: reduction in context regurgitation.

Track:

- fewer repeated PR, Linear, and changelog explanations;
- fewer agent prompts re-ingesting the same docs;
- fewer review comments asking whether something was already decided;
- preflight calls, matched constraints, missed constraints, user corrections,
  and later accepted candidates.

## ADR Alignment

- ADR-0001 is reaffirmed: the event store substrate is authority, and Git is a
  strong fit for repo-centric teams.
- ADR-0002 is reaffirmed: MCP is a `ToolRequest` client, not authority.
- ADR-0004 is reaffirmed: HITL boundaries must preserve extraction, binding, and
  compliance separately.
- ADR-0007 is reaffirmed: source evidence to canonical Decision must pass
  through governance.
- ADR-0019 is retained only for the separate Code Evidence/Compliance profile;
  ADR-0028 defines the SMR alpha preflight result.
- ADR-0022 is reaffirmed: V1 is signoff plus reviewable context, not enforcement
  memory.

## Runtime Contract

The durable runtime and strategy artifacts are:

```text
Constraint Lookup RecallPacket Contract
Preflight Impact-to-Decision Recall Strategy
Alpha Recall Multi-Signal Fusion
SMR Alpha Runtime Contract
```

They define corpus coverage, recall completeness semantics, MCP rendering
boundaries, optional recall-expansion signals, impact-to-decision strategy,
ranking/fusion inputs, searched-scope proof, and forbidden authority paths.
The SMR runtime contract fixes the required input, corpus, applicability,
coverage, and proposed-claim identities without making CodeGraph mandatory. The
impact-recall strategy is captured in
[Preflight Impact-to-Decision Recall Strategy](preflight-impact-to-decision-recall-strategy.md).
The first recall-quality boundary is captured in
[Alpha Recall Multi-Signal Fusion And Recall-Influence Boundary](alpha-recall-multi-signal-fusion.md).
