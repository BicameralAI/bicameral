# ADR-0004: HITL Boundaries for Probabilistic Governance

> Carries forward `bicameral-daemon` ADR-0004, adapted to `bicameral-bot` as the local runtime authority boundary.

**Date:** 2026-05-27
**Status:** proposed
**Level:** L1
**Related:** ADR-0001, ADR-0002, ADR-0003, ADR-0005, ADR-0006, ADR-0007

## Context

Bicameral reduces cognitive debt only if it avoids manufacturing new confusion.
The product currently bridges two probabilistic domains:

1. **Decision extraction** — deciding whether a transcript, ticket, Slack thread,
   or agent session contains a binding implementation constraint.
2. **Compliance and grounding** — deciding whether code reflects, violates, or
   depends on that extracted constraint.

If these domains are collapsed into a single confidence score, the product can
produce confident-looking but wrong governance: a fuzzy extracted decision bound
to the wrong symbol, or a correct decision evaluated against an unsupported code
region. That is worse than missing the decision because it adds new cognitive
debt to the ledger.

## Decision

Treat human-in-the-loop review as a tactical debt-control boundary, not as an
exception path. The bot runtime and any cloud advisory service must keep the confidence surfaces for extraction,
grounding, and compliance separate, and route ambiguity to the right owner/member capability or reviewer assignment.

### Confidence surfaces

Every automated governance claim that can affect shared team state must preserve
at least these independent judgments:

1. `extraction_confidence` — confidence that the source contains a real decision
   and that the extracted decision text/rationale captures it.
2. `binding_confidence` — confidence that the decision is correctly bound to the
   intended code region or symbol.
3. `compliance_confidence` — confidence that current code reflects, violates, or
   is pending relative to that decision.

These are not interchangeable. High extraction confidence does not make a weak
code binding safe; high binding confidence does not prove product intent.

### HITL routing

- **Owner/member signoff review:** approves or rejects the meaning of extracted
  decisions. This review answers: "is this actually the decision?"
- **Compliance review:** resolves dependency, binding, drift, and blast
  radius claims according to workspace policy. This review answers: "is compliance resolved?"
- **Binding evidence review:** accepts, corrects, or rejects code bindings and compliance
  evidence according to workspace policy. This review answers: "is this the right code evidence?"

ADR-0006 maps these review boundaries onto two minimal UI surfaces for the spike:

- **Ingestion Gate:** candidate extraction review, source excerpt linking, and owner/member edits before ingest.
- **Ledger View:** canonical signoff/compliance review, collision resolution, bound code-region evidence, and resolved compliance workflow over the Decision Ledger.

The bot may use confidence to reduce review friction, but not to silently
accept a probabilistic claim into the Decision Ledger or approve signoff when
it would affect merge blocking, ownership approval, or cross-branch conflict detection.

ADR-0005 implements these HITL boundaries through configurable governance policy.
Required reviewers and automation levels may vary by connector, source trust,
decision level, event store substrate, and enforcement capability, but the policy
must preserve the distinction between candidate extraction, candidate acceptance,
signoff approval, grounding, and compliance.

ADR-0007 owns the substrate-neutral governance flow and command vocabulary. This ADR only defines the HITL safety boundaries that governance policy must preserve across review surfaces and event store substrates.

### Candidate acceptance and signoff rule

A `DecisionCandidate` may enter the Decision Ledger only when governance policy
accepts an `accept_candidate` command and the selected event store substrate
materializes the resulting event. That creates a canonical proposed Decision; it
does not approve signoff. Signoff approval requires a separate `approve_signoff`
transition according to workspace policy. A code compliance claim may become
blocking only when grounding evidence is resolved and the selected substrate can
enforce a block. Until then, the system should surface advisory states such as:

- `needs_signoff_review`
- `needs_grounding_review`
- `needs_compliance_review`
- `evidence_strength = weak`
- `ComplianceVerdict = pending | ungrounded`

### Audit rule

When the bot or cloud advisory path escalates or blocks, it must preserve why:

- source excerpt or source URL that produced the candidate decision;
- candidate code region and symbol evidence;
- confidence surface values or qualitative reasons;
- reviewer action and resulting state transition.

The audit trail must also preserve the responsibility chain for any accepted,
blocking, or externally visible governance result:

- extractor/provider identity when available;
- governance policy version or policy source;
- reviewer identity or capability class;
- materializing event store adapter;
- actor who accepted, approved, rejected, superseded, or resolved the claim;
- whether automation was advisory, queued, or authoritative.

The system must not describe an automated recommendation as a human decision
until an accountable reviewer or configured low-risk policy has accepted that
transition.

## Consequences

Positive:

- Prevents Bicameral from adding false-confidence debt to the ledger.
- Gives each human reviewer the smallest useful judgment instead of asking them
  to audit the entire pipeline.
- Keeps the selected event store substrate as the predictable durable authority
  while allowing hosted services to optimize extraction, routing, and review UX.
- Makes false positives measurable: rejected candidate events, rejected Decisions,
  corrected bindings, and compliance verdict edits become product telemetry.

Tradeoffs:

- More intermediate states than a binary pass/fail compliance gate.
- CI cannot immediately block every possible drift claim; some claims remain
  advisory until grounding is resolved.
- Dashboard and protocol surfaces must expose uncertainty honestly instead of
  hiding it behind one score.
- Audit records must preserve enough responsibility-chain detail to debug
  accepted, blocking, or externally visible governance results.

## Relationship to Existing ADRs

- **ADR-0001:** The selected event store substrate stores accepted domain events
  and reviewable state transitions, not opaque AI certainty.
- **ADR-0002:** MCP remains the local tool surface. MCP should present the
  separated confidence surfaces instead of collapsing them into one status.
- **ADR-0003:** The bot owns local grounding while cloud owns hosted graph and
  conflict infrastructure. Both must distinguish advisory probabilistic warnings
  from review-resolved blocking claims.
- **ADR-0005:** Governance policy connects these HITL boundaries to event authority
  substrates, ingestion automation settings, and review UX surfaces.
- **ADR-0007:** The substrate-neutral governance flow defines the command/result
  vocabulary; this ADR constrains when those results may be accepted into durable
  history, approve signoff, or become blocking.

## Rejected Alternatives

- **Single confidence score:** rejected because extraction, binding, and
  compliance fail independently.
- **Fully automatic approval:** rejected because decision meaning is the
  canonical human authority boundary.
- **Always-blocking CI from day one:** rejected because incorrect grounding can
  create more confusion than the drift it is trying to prevent.
