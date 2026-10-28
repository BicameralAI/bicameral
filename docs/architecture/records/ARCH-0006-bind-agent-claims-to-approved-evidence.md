---
id: ARCH-0006
title: Bind consequential agent claims to exact approved Product evidence
kind: decision
status: rejected
date: 2026-10-26
owners: [@Knapp-Kevin]
governs: [agent-claim-evidence-binding]
depends_on: [ARCH-0003, ARCH-0004, ARCH-0005]
supersedes: []
references: []
---

# ARCH-0006: Bind consequential agent claims to exact approved Product evidence

> **Rejected on 2026-10-28** by the decision owner. A claim-level verifier adds a new component whose core check (whether a sentence follows from its evidence) is open-ended, and there is no recorded evidence yet of real misstatements to justify it. ARCH-0005's rules address the wrong-source failures at lower cost. The Phase 0 harness (#56, #58) stays as a test-only measurement. Revisit with a new record if real cases of agents misstating approved Product facts appear.

## Context

This record carries proposed Bot ADR-0036 into this repository. It was drafted on 2026-10-22 and has not been reviewed. Links to private trackers are removed. The text is otherwise unchanged except that ADR-0033, ADR-0034 and ADR-0035 now read ARCH-0003, ARCH-0004 and ARCH-0005. Implementation is deferred behind the ARCH-0005 reconstruction baseline.

Retrieval correctness does not guarantee the final human/agent explanation is correct. Even if the daemon supplies an exact approved `GovernedDomain` claim, a model can reverse negation, omit a guard, equate a tracked snapshot with approval, substitute a stale source, fabricate a broader Product rule, or present unknown coverage as global assurance.

ARCH-0004 defines governing authority as **Signoff of the SourceSnapshot's GovernedDomain**. Its semantic content is defined by ARCH-0003: claims, atoms, claim assertions (with polarity), and `AtomRelationV1` relations, backed by the SourceSnapshot content identity. Do not resurrect a separate `Decision Ledger` or `DecisionCandidate` vocabulary merely because an older RecallPacket/reader issue still uses it.

## Decision

Introduce a **read-only, daemon-owned final-claim support boundary** for consequential statements presented as approved Product requirements or permissions. It composes the approved semantic reconstruction in ARCH-0005 with the existing supported reader/transport contract. It **does not** promote, authorize or rewrite product facts.

For a consequential output claim to be labeled as canonical/approved, a consumer must receive and retain a support binding to:

- the exact approved snapshot and `GovernedDomain` semantic digest, plus replayed approval evidence;
- immutable claim/atom/assertion/relation identifiers necessary to substantiate the asserted meaning, including polarity and guards;
- authenticated Product/query scope and the evaluated snapshot or current authorized lookup basis;
- versioned resolver and evidence-binding policy/contract, authorized provenance reference, and declared unavailable/unknown coverage.

The checker shall use **typed verdicts or supported existing result fields**, distinguishing at minimum fully supported, refuted, insufficient support, ambiguous/underdetermined, out-of-scope and unavailable. Final enum vocabulary and transport shape require a governed exact-revision spec; do not infer a new authorization state from these review terms. If no supported exact binding exists, **abstain from the claim that it is approved Product truth** and surface the narrow missing question or limitation. The result can still present a clearly labeled advisory interpretation.

Verification must evaluate the *actual rendered claim* against source support, not merely the existence of a relevant retrieval hit. For requirements with complex guards, causal state, alternatives, or quantifiers outside supported verification, report unsupported/underdetermined rather than claiming semantically checked.

### Trust and privacy boundaries

1. Only the Bot/daemon determines approval and scoped evidence support. MCP, hosted UI and caller models render or relay; they do not reclassify source content as approved.
2. Semantic checkers, LLMs, graph inference and fuzzy similarity can propose evidence links but never manufacture signoff or prove entailment by confidence score.
3. A `CheckRecord` with `conflict: false` is an exact pairwise result, not global non-conflict. Missing check is not false.
4. Record authorized identifiers/digests/reason codes in portable receipts. Source excerpts may be rendered only within existing access/disclosure policy; no raw private bodies, secret paths, prompts or provider credentials in default remote outputs.
5. Changes to source, signoff, snapshot or applicable policy invalidate previously issued *current* claims when rechecked. Historical citations remain historical, not ongoing authorization.
6. Failure or missing model support must not become a fabricated PASS.

### Negative qualification corpus

- An unapproved tracked domain is described as "signed off".
- Exact source says "must not"; final answer says "may".
- Approved claim has an exception/guard; final answer drops the exception.
- Same cited source contains relevant content but does not support the particular claimed conclusion.
- A newer snapshot exists but is not approved; citation silently shifts to it.
- Superseded snapshot is quoted as current authorization.
- Two conflicting/unknown inputs, incomplete scope, unavailable source or stale projection produce an unsupported "all clear".
- Cross-Product/unauthorized reference reveals existence or contents through claimed evidence.
- The agent gets a correct structured read packet but produces an inaccurate final sentence.
- A verified exact citation is necessary but not sufficient when the semantic entailment of the claimed interpretation cannot be established.

Evaluation measures retrieval, approval selection, relation fidelity, semantic support and final-answer accuracy **separately**; do not define a favorable score from a fixture-only result.

### Phased application

- **Phase 0:** pin current approved governance vocabulary, source/claim identity and negative gold cases; no new endpoint or dependency.
- **Phase 1:** after the ARCH-0005 replay/scoped diagnostic baseline is proven, enforce support associations or safe abstention for one existing reader path; compare against original exact evidence in a bounded, testable set.
- **Phase 2:** only after exact semantic contract review and privacy/security tests, achieve parity across other supported transports and real customer-like consumption.
- **Out of scope:** LLM-led approvals, universal natural-language theorem proving, TypeGraph adoption, historical `as_of` API activation, unconditional blocking of unrelated low-consequence chat, new product lifecycle state.

## Consequences

- **Benefit:** addresses last-mile false product-truth assertions that graph retrieval alone cannot solve.
- **Risk:** source-grounding checks can over-abstain or miss subtle semantic errors. Restrict claimable coverage to tested predicates and explicitly disclose unsupported classes.
- **Cost:** exact source-revision evidence bindings, structured verification receipts, and negative consumer-evaluation tests.
- **Alternative rejected:** accept an LLM paraphrase when search returned the right document. Citation presence does not imply the answer faithfully reflects it.
- **Alternative rejected:** automatically attach a high confidence score to graph-retrieved content and equate confidence to Signoff.
- **State refinement:** no canonical events, approved semantics, lifecycle transitions, or Product Signoff changes.

This record is a proposed design, **not an implemented verifier**. Phase 0 changes no product behavior and may land before this record is accepted. Black-box tests should explicitly distinguish correct retrieval from correct user-visible statements.
