# Journey Review and Transformation-Debug Dashboard

Status: implementation contract for checklist Item 6 / Factory D4

Tracking: #899

Stack base: `agent/integrations-trace-journey-review-v2` / PR #897

## Purpose

Ship a daemon-served review surface that lets an operator inspect one accepted-evidence journey from provider acquisition through canonical Decision promotion without confusing derived evidence, daemon-local proposals, or canonical Product history.

The dashboard is a view and governed command surface. It is not a new authority layer.

## Authority model

Every rendered object MUST carry both a text label and a structural presentation treatment:

- `canonical`: accepted evidence facts and promoted Decisions reconstructed from Product event replay;
- `local_proposal`: daemon-local projection outcomes and active candidates from SQLite working state;
- `derived_evidence`: transformation phases, field deltas, advisories, receipts, and Recall composition;
- `unknown`: unavailable or unverified upstream state that MUST NOT be inferred.

Color alone MUST NOT communicate authority.

## Data contracts

The surface consumes only daemon-owned contracts introduced by #896 and #897:

- `GET /api/v1/journeys/{journey_id}` for `JourneyReviewView` and `RecallPacket` composition;
- the finalized transformation-trace attachment/download contract;
- daemon-advertised candidate disposition capabilities;
- governed `v2/tool-requests` for mutations.

The browser MUST NOT reconstruct candidate authority, infer missing phases, read local filesystem paths, or invoke a model.

## Required views

### Journey summary

Display source identity, evidence acceptance identity, content digest, current projection state, canonical Decision count, local candidate count, retry state, and trace completeness.

### Ordered phase trace

Render phases in contract order. Each phase shows:

- phase and function identity;
- input and output contract fingerprints;
- bounded field deltas;
- hard-gate result;
- fail-open advisory result;
- timing;
- digest-chain continuity;
- persistence effect;
- authority before and after;
- explicit missing, partial, stale, or invalid state.

### Recall composition

Render canonical Decisions and daemon-local candidates in separate regions. Evidence references are supporting material, never candidate or Decision authority.

### Recovery state

Explicitly render:

- no candidate;
- deferred provider outcome;
- terminal failed projection;
- retry scheduled;
- missing finalized trace;
- partial or stale trace;
- hidden candidate for the active session/view;
- durable local suppression;
- working-state deletion with canonical evidence retained.

No empty region may be presented as success.

## Governed actions

The surface may render `hide`, `suppress`, `reject`, and `promote` only when the current `JourneyReviewView` advertises that action for that candidate.

Before submission it MUST:

1. re-fetch the journey view;
2. confirm the candidate and action remain advertised;
3. require a reason for suppression and rejection when the command contract requires one;
4. submit through `v2/tool-requests`;
5. refresh the journey view after the response;
6. fail closed on stale capability, unknown candidate, unsupported command, or network ambiguity.

The surface MUST expose no direct Decision creation endpoint.

## Accessibility and responsive acceptance

Required evidence:

- complete keyboard-only journey;
- visible focus and logical focus order;
- semantic headings, landmarks, buttons, and status messages;
- authority distinction that does not depend on color;
- responsive layout from 320 CSS pixels through desktop widths;
- no required horizontal scrolling for primary review content;
- reduced-motion mode with nonessential transitions removed;
- recovery and mutation results announced through an `aria-live` region.

## Security and privacy

- Serve from the daemon gateway and preserve loopback-first deployment.
- Render only sanitized contracts.
- Never expose evidence bytes, redacted values, CAS paths, SQLite paths, secrets, or provider credentials.
- Use `Cache-Control: no-store` for journey and trace responses.
- Escape all displayed text and avoid HTML injection paths.
- Do not load third-party scripts, fonts, analytics, or remote assets.

## Validation gates

The implementation PR MUST include:

- gateway route tests for the dashboard document and security headers;
- contract fixtures for candidate, no-candidate, deferred, failed, retry, missing-trace, and promoted-Decision journeys;
- browser or DOM tests for keyboard, capability gating, stale-action rejection, download, responsive structure, authority labels, and reduced motion;
- repository formatting;
- affected API, Gateway, and Runtime compilation;
- inherited #896 and #897 contract tests;
- strict production Clippy;
- Dependency Review.

## Completion boundary

A green dashboard PR advances Item 6. It does not complete Item 6 until #896, #897, the dashboard slice, and the Integrations transformation-trace contract are reviewed and accepted together.
