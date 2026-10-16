# Why SMR and Spec Governance

**Status:** product/strategy reference (not a runtime contract)  
**Audience:** collaborators, implementers, and anyone asking “why is Bicameral betting on SMR?”  
**Read time:** ~5 minutes  
**Technical realization:** [ADR-0028](../adr/0028-smr-alpha-recall-boundary.md), [ADR-0029](../adr/0029-complete-smr-hypergraph-projection-to-stable-alloy.md), [Claim Semantic Trace ↔ Alloy bridge](../specs/smr-alpha-claim-semantic-trace-alloy-bridge.md), [SMR alpha runtime contract](../specs/smr-alpha-runtime-contract.md), [semantic-resolution capability contract](../specs/semantic-resolution-capability-contract.md)

This document is the product **why**. It does not authorize implementation,
merge, release, or capability activation. Those remain governed specs, slices,
and (when claimed) an explicit product activation decision.

---

## Motivation

Software teams already lose fidelity when business intent crosses complex state
transitions, service boundaries, and data dependencies. AI agents amplify this
pre-existing communication gap by compressing implementation time and silently
choosing behavior that the team never specified.

The cost of unresolved ambiguity goes up when code is generated quickly. At the
same time, LLMs make translation, model construction, counterexample
explanation, and continuous grounding newly feasible. Spec governance—applying
formal-verification discipline to business logic at decision boundaries—is now
both urgent and broadly applicable, not only for large enterprises and
safety-critical systems.

---

## The formal-verification frame

Consider a product discussion that establishes three facts:

1. failed payments must be recoverable;
2. captured payments cannot be retried;
3. support may initiate recovery.

The discussion never decides whether a customer may retry a payment that failed
**before** capture. Two implementations remain compatible with the known
specification:

```text
                         FailedBeforeCapture
                         /                 \
                        /                   \
          Customer.retry                     Support.retry
                      /                       \
                     v                         v
                Processing                 Processing
                 Model A                     Model B
```

Let **T** represent the governed facts already known, and let **p** mean
“Customer may retry FailedBeforeCapture.” A contradiction checker asks whether
both **T + p** and **T + not-p** remain possible:

| T + p | T + not-p | Meaning | System response |
|---|---|---|---|
| Possible | Impossible | **Entailed** | Proceed with provenance |
| Impossible | Possible | **Refuted** | Surface the contradiction |
| Possible | Possible | **Underdetermined** | Escalate the exact missing decision |
| Impossible | Impossible | **Existing facts conflict** | Return the minimal conflicting claims |

### Underdetermined is the HITL release valve

Instead of letting an agent choose Model A or Model B, Bicameral identifies the
smallest behavior on which the models differ and asks a precise question:

> When a payment fails before capture, may the customer retry directly, or must
> support initiate recovery?

This is more precise than a generic request for clarification. The human is
invoked exactly where the admissible model space forks into materially different
product behavior; irrelevant ambiguity may remain open.

### Conflict is the contradiction checker

When no model can satisfy the governed claims together, Bicameral surfaces the
smallest conflicting claim set, its scope, and its provenance—rather than
selecting a winner or inventing a resolution.

---

## Core novelty: democratizing formal verification

We operationalize formal verification to the degree that it is **possible and
necessary**. We do not require every team to author a complete mathematical
specification. We apply the discipline of model checking at the decision
boundaries where alternative interpretations would produce materially different
software.

Two product surfaces carry that idea into the stack:

| Surface | Role |
|---|---|
| **Spec activity upstream** (`bicameral-integrations` and related intake) | Detect new specification activity and surface contradictions **before** they calcify into code. Product conversations, documents, and collaboration signals become scoped, reviewable candidates; consequential conflicts route back to humans who hold decision authority. |
| **Governed context at implementation** (`bicameral-mcp` preflight / Bot daemon SMR path) | Feed accepted decisions, constraints, unresolved scope, and provenance into the coding boundary. Agents receive governed context without authority to invent missing product semantics. |

SMR (State Machine Resolver) is the **deterministic analysis core** of that bet:
source-grounded candidate structure, alignment with accepted product meaning,
bounded formal checks, reverse provenance, and fail-closed honesty—not
code-compliance theater and not LLM-authored Alloy.

Technical boundaries for alpha (do not overclaim):

- SMR recalls and analyzes **specification context**; it does not prove code
  implements or complies with a spec ([ADR-0028](../adr/0028-smr-alpha-recall-boundary.md)).
- The LLM may propose a source-grounded claim trace; it must not own quantifiers,
  closure, or formal checks ([ADR-0029](../adr/0029-complete-smr-hypergraph-projection-to-stable-alloy.md)).
- Results are bounded evidence classes, never automatic promotion or “all clear.”

---

## Activation hint (product language)

What Bicameral may **eventually** claim if the path ships honestly:

- **Possible future claim:** Teams can detect underdetermined and conflicting
  product semantics early, escalate the smallest missing decision to humans, and
  feed governed constraints into implementation without agents inventing
  product law.
- **Not claimable from partial work:** “Full formal verification of all business
  logic,” “global safety,” “code compliance,” or “SMR activation” merely because
  a kernel suite is green.
- **Status:** hint (see Factory
  `blueprints/product-design-to-governed-implementation.md`)

---

## How to use this doc

| Reader | Use |
|---|---|
| New collaborator | Start here for the 5-minute product bet |
| Implementer | Then read ADR-0028 / 0029 and the runtime specs for authority and APIs |
| Composition stack (#985) | Kernel handoff on the land-path issue; this doc is the *why* behind fail-closed underdetermined escalation |

Related journeys and land paths evolve in issues and release trains; this file
should stay stable as the **motivation reference**.
