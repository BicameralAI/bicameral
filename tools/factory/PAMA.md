# Proportional Adaptive Mutation Authority
## A Systems-Agnostic Architecture for Safe Agent Evolution

**Document type:** Conceptual architecture and governance framework  
**Audience:** AI system architects, product leaders, security engineers, governance researchers, and teams building adaptive agent platforms  
**Status:** Shareable conceptual paper, revised for external framework alignment  
**Revision date:** 2026-05-28  
**Scope:** Agent adaptation, memory mutation, procedural learning, delegated authority, evidence, operational safety, and governance alignment  
**Framework alignment:** OWASP Top 10 for Agentic Applications 2026, NIST AI RMF and Generative AI Profile, and EU AI Act obligation-support analysis  

---

## Abstract

An adaptive agent system must eventually change itself in some meaningful way. It must retain preferences, improve routing decisions, learn useful procedural patterns, revise incorrect assumptions, and sometimes propose new reusable capabilities. Without adaptation, agents remain dependent on repeated human correction and increasingly expensive context reconstruction. With unrestricted adaptation, agents can silently convert inference into fact, habit into policy, convenience into authority, and repeated error into durable behavior.

This paper proposes **Proportional Adaptive Mutation Authority**, a systems-agnostic architecture for allowing agents to learn while controlling the consequence of what they learn.

The central principle is simple:

> **Adaptation should be broadly available to authorized agents. Authority to make a mutation durable, influential, shared, or action-enabling should increase in proportion to the mutation’s consequence.**

This approach rejects two unproductive extremes. The first is a permanently read-only learning model in which agents may retrieve memory but cannot refine their behavior in durable, inspectable ways. The second is a self-editing model in which an agent may rewrite memory, procedure, policy, or action permissions merely because it observed a pattern or completed a task.

Instead, the architecture distinguishes an **observation** from a **trusted memory**, a **trusted memory** from a **reusable procedure**, a **procedure** from a **permission to act**, and a **permission to act** from a **permission to change governance itself**.

Low-risk, reversible learning should occur with little or no interruption. High-impact mutations should require evidence, validation, review, explicit authorization, and later monitoring. The system should concentrate human friction where consequences become meaningful, not where ordinary learning begins.

---

## 1. The Problem: Agents Must Learn, but Learning Is Not Innocent

Modern agents increasingly operate across email, tasks, documents, code, calendars, external tools, customer records, internal systems, and security controls. As their responsibilities expand, static prompts and short-lived context windows become inadequate.

An agent that manages recurring work must eventually recognize patterns:

- A user prefers one communication style over another.
- A category of inbound request usually maps to a specific workflow.
- A certain troubleshooting process repeatedly resolves a class of failures.
- A previous recommendation was corrected and should not be repeated.
- A safety rule prevented a real threat.
- A proposed procedure proved useful across several validated executions.

These are valuable forms of adaptation. Yet each has a different consequence.

Learning that a user prefers concise summaries is not equivalent to learning that a customer has approved a contractual change. Remembering that a verified procedure fixes a recurring build failure is not equivalent to granting an agent permission to deploy automatically. Capturing a new threat indicator is not equivalent to allowing a security agent to relax an enforcement policy it finds inconvenient.

When systems fail to distinguish these differences, they tend to fall into one of three traps.

### 1.1 The stagnant agent

The system avoids adaptation because memory mutation appears risky. The agent retrieves prior context but cannot form durable, useful improvement loops. Users must repeat preferences, re-teach workflows, and manually transfer knowledge between sessions. The system is safe only in the narrow sense that it cannot become meaningfully better or meaningfully worse.

### 1.2 The casually self-editing agent

The system permits agents to append memories, revise procedures, create tools, alter routing logic, or expand autonomous behavior with weak validation. It feels productive because useful patterns can be captured immediately. Over time, however, observations become assumptions, assumptions become defaults, and defaults become hidden authority.

This is especially dangerous because a self-editing agent can appear successful while slowly degrading reliability. Repeated behavior is not necessarily correct behavior. A mistake repeated ten times is not validation. It is merely an organized mistake.

### 1.3 The over-governed agent

The system recognizes mutation risk and therefore requires human approval for every retained preference, observation, association, correction, or procedure proposal. In theory, this is controlled. In practice, it produces approval fatigue. Users begin to approve prompts without reviewing them, and governance becomes a ritual rather than a protection.

The architecture proposed here addresses all three traps by treating adaptation as normal, but mutation authority as graduated.

---

## 2. Core Thesis

The architecture is built around four separations.

### 2.1 Adaptation is not authority

An agent may observe, infer, propose, or learn without automatically receiving authority to apply a durable or consequential change.

### 2.2 Memory is not procedure

A remembered fact or preference does not automatically become an executable workflow. A procedure must include scope, inputs, expected outputs, validation, permitted effects, failure behavior, and version history.

### 2.3 Procedure is not permission

A validated procedure may describe how an action should be performed. It does not automatically authorize the agent to perform that action without review.

### 2.4 Permission is not governance

Even where an agent has authority to act, it should not be able to expand, weaken, or redefine that authority without a separate governance decision.

These separations yield the governing rule:

> **Review should be applied at promotion and consequence boundaries, not at every observation boundary.**

A system can permit low-risk adaptation to occur rapidly while preserving strict control over what becomes durable truth, shared operational capability, external action, or governance policy.

---

## 3. Design Objectives

A proportional adaptive mutation framework should satisfy the following objectives.

### 3.1 Universal but scoped adaptability

Every authorized agent role should be capable of proposing adaptive changes within an explicitly defined domain. A planning assistant, communications agent, operations agent, supportive companion, coding agent, or security agent may all benefit from learning, but each requires a different adaptive charter.

### 3.2 Consequence-proportional control

A mutation should be evaluated based on what it changes and what downstream behavior it may influence. Agent identity is relevant, but insufficient on its own.

### 3.3 Low friction for reversible learning

Temporary context, low-risk preferences, and recommendation-only associations should generally not create repeated approval prompts. They should be visible, correctable, scoped, and prevented from triggering higher-authority consequences.

### 3.4 Evidence before durable influence

A durable mutation should carry provenance. A promoted procedure should carry validation. An identity-bearing fact should carry authority. A policy change should carry explicit approval and revocation capability.

### 3.5 Measurable improvement

A system should not claim learning because context retrieval succeeded or because an agent generated a plausible response. It should distinguish retrieval quality, execution success, user acceptance, corrections, downstream failure, and replay validation.

### 3.6 Reversibility and inspection

Learned state should be inspectable. The user or authorized reviewer should be able to understand what the system retained, why it retained it, what it influenced, whether it was promoted or blocked, and how it can be corrected, demoted, revoked, or removed.

---

## 4. Non-Goals

This framework does not propose unrestricted self-modification. It does not assume all agents should be autonomous in their external actions. It does not treat repeated behavior as truth. It does not replace identity, access control, privacy, security review, or approval systems.

In particular, this architecture does not authorize:

- Automatic promotion of inferred personal, customer, medical, financial, legal, or security facts into canonical memory.
- Self-approved expansion of an agent’s privileges.
- Silent weakening of enforcement policies.
- Autonomous communication, deployment, deletion, spending, or external representation merely because an agent learned a useful pattern.
- A single undifferentiated “memory store” where preferences, evidence, procedures, permissions, and policy are mixed together.

The purpose is disciplined adaptation, not magical permission laundering.

---

## 5. Conceptual Architecture

A complete adaptive authority system contains six conceptual roles. A particular implementation may combine or rename these roles, but their responsibilities should remain distinct.

### 5.1 Agent role

An agent performs work, consumes context, observes outcomes, receives corrections, and proposes mutations.

Examples include:

- A personal workflow agent.
- A communications or intake agent.
- A supportive or educational agent.
- An operations or execution agent.
- A security monitoring or policy agent.
- A specialist agent for a bounded business domain.

An agent is allowed to propose adaptation only within its charter. The agent is not presumed to be an impartial authority over its own proposed changes.

### 5.2 Protected memory boundary

The protected memory boundary controls ownership, identity, access, visibility, encryption where applicable, provenance, retention, deletion, lineage, and permissions.

It answers questions such as:

- Who owns this retained memory?
- Which agent roles may retrieve it?
- Is it private to one user, one project, one persona, or shared across agents?
- What evidence supports it?
- What is its lifecycle state?
- Has it been corrected, superseded, revoked, or blocked?

The protected memory boundary is not merely storage. It is the containment layer preventing adaptive state from becoming an ungoverned pile of convenient assertions.

### 5.3 Context assembly layer

The context assembly layer turns eligible retained information into task-specific context.

It should preserve the native shape of knowledge rather than converting all information into generic prose. Useful shapes include:

- user preference;
- conversation thread;
- task state;
- policy;
- document hierarchy;
- structured table;
- graph relationship;
- code artifact;
- execution trace;
- incident record;
- validation result;
- reusable capability;
- decision or approval evidence.

This layer should build the minimum sufficient context required for an agent’s current task and expose the provenance, freshness, visibility, and confidence needed for safe use.

### 5.4 Lifecycle evaluator

The lifecycle evaluator reviews adaptive signals and recommends whether retained state should be discarded, kept transient, retained tentatively, reinforced, promoted, decayed, superseded, blocked, or archived.

It considers evidence such as:

- recurrence;
- recent use;
- validation outcomes;
- user confirmations;
- user corrections;
- failure association;
- freshness;
- sensitivity;
- scope;
- conflicts with stronger knowledge;
- downstream utility.

The lifecycle evaluator is advisory for consequential changes. It must not certify its own recommendation as proof that the recommendation was correct.

### 5.5 Authority policy evaluator

The authority policy evaluator determines how a proposed mutation may be handled.

Possible results include:

- Apply transiently.
- Apply and log.
- Retain as tentative.
- Require substantiation.
- Require explicit review.
- Veto.

The policy evaluator considers the proposing agent’s charter, mutation target, lifecycle strength, downstream authority, evidence, sensitivity, reversibility, scope, conflict state, and applicable governance requirements.

### 5.6 Lineage and outcome monitor

The lineage and outcome monitor records what was proposed, what evidence supported it, what decision was made, what was applied, what later used it, and what happened afterward.

Without lineage, learning cannot be explained. Without later outcome monitoring, promoted behavior cannot be corrected when it becomes stale or harmful.

---

## 6. The Adaptive Mutation Loop

The architecture can be expressed as a closed but controlled feedback loop:

```text
Observed work, user correction, validated result, or repeated pattern
        ↓
Agent proposes mutation within its charter
        ↓
Mutation is classified by target, strength, scope and authority
        ↓
Lifecycle evaluator recommends retention, reinforcement, promotion, decay or block
        ↓
Authority evaluator selects automatic, substantiated, review-required or veto handling
        ↓
Approved mutation is persisted with lineage and visibility controls
        ↓
Context assembly may use eligible retained state in future work
        ↓
Later outcomes, corrections and failures feed back into lifecycle evaluation
```

This loop is intentionally not a shortcut from “agent noticed a pattern” to “agent now acts differently forever.”

The system should permit an observation to exist at low strength, allow evidence to reinforce it, require substantiation before it becomes a default, and require explicit authority before it affects external action or governance.

---

## 7. Mutation Target Classes

The most important factor in determining authority is the target of the mutation. The following taxonomy separates low-impact learning from high-impact system change.

| Class | Target Type | Examples | Default Treatment |
| --- | --- | --- | --- |
| M0 | Execution-local context | Current-session correlation, temporary intent interpretation, local retrieval hint | Apply transiently; expire automatically |
| M1 | Low-risk personal preference | Formatting preference, terminology, view preference, pacing preference | Tentative, visible, reversible |
| M2 | Operational association | Likely project routing, recurring task association, workflow suggestion, context recommendation | Evidence-backed; may influence recommendations |
| M3 | Reusable procedure or capability | Validated checklist, repair sequence, reusable workflow, approved context bundle | Requires validation and promotion treatment |
| M4 | Shared fact or identity-bearing state | Relationship, entitlement, commitment status, permission-affecting fact, cross-agent rule | Requires authoritative evidence and controlled review |
| M5 | Governance, security, or autonomous-action authority | Policy exemption, trust elevation, autonomous send/deploy/delete permission, relaxed security control | Explicit authorization; no self-approval |

### 7.1 Why the target class matters more than the agent name

A trusted security agent proposing a relaxed enforcement rule is more dangerous than a general assistant storing a temporary preference. A supportive agent retaining a user-approved reading preference is less dangerous than an operations agent promoting a procedure that can execute external tools.

Agents may have domain expertise. Expertise does not dissolve the consequence of what they propose.

---

## 8. Mutation Strength and Lifecycle

An adaptive system should not convert a first observation directly into a default behavior. It should support lifecycle strength.

```text
Observed → Tentative → Reinforced → Promoted → Canonical
                   ↘ Decaying → Archived / Deprecated / Blocked
```

| Strength | Meaning | Appropriate Influence |
| --- | --- | --- |
| Observed | A signal has been captured but not trusted | Inspection or low-weight evaluation only |
| Tentative | A plausible pattern or preference has initial support | Visible suggestion or limited recommendation influence |
| Reinforced | A pattern has repeated support, validation, or confirmation | Stronger recommendation ranking and context assembly |
| Promoted | A reviewed or policy-approved reusable memory or capability | Default governed use within defined scope |
| Canonical | Authoritative, source-backed truth or policy state | High-confidence control or context use under strict rules |

### 8.1 Demotion is as important as promotion

A system capable only of reinforcement will eventually become overconfident. Corrections, failures, stale data, conflicts, or changing circumstances must be able to demote, suspend, supersede, revoke, or block retained state.

Examples:

- A workflow association may be reinforced for months and then become stale after a team restructure.
- A reusable procedure may work until a dependency changes.
- A communication-routing pattern may repeatedly be corrected by the user.
- A security signal may turn out to be a benign false positive.
- A policy exception may no longer be justified.

Learning without forgetting, correction, or revocation is merely accumulation.

---

## 9. Downstream Authority Classes

A mutation’s strength does not fully describe its danger. A reinforced preference affecting suggestion wording is different from a tentative memory that can trigger an external action. Therefore each mutation must declare the maximum downstream authority it can influence.

| Class | Authority | Meaning |
| --- | --- | --- |
| A0 | Retrieval Only | May appear in memory inspection or context recall |
| A1 | Recommendation Influence | May affect rankings, suggestions, routing recommendations, or planning |
| A2 | Draft Generation | May shape drafts for human review but cannot execute them |
| A3 | Local Workflow Mutation | May update internal state such as assignment, status, task linkage, or scheduling within authorized constraints |
| A4 | External Action | May affect messages, deployments, purchases, deletions, bookings, provider state, or representations outside the internal system |
| A5 | Governance Change | May alter privileges, policies, trust, enforcement, or future autonomous authority |

### 9.1 Strength and authority must be evaluated together

A mutation may be low-confidence but high-consequence. That does not mean it should simply be ignored; it means it may be retained as an observation while prevented from influencing dangerous behavior.

A mutation may also be highly validated but still require explicit permission. For example, a procedure for sending a routine customer acknowledgment may be reliable, yet enabling unattended outbound sending is still an authority decision.

This yields an important rule:

> **Validation can justify trust in a capability. It does not automatically grant permission to use that capability autonomously.**

---

## 10. Proportional Handling Lanes

A useful governance model does not ask a human to approve every learned signal. It uses lanes.

### 10.1 Lane 1: Transient automatic handling

Designed for M0 mutations and narrowly scoped context interpretations.

Examples:

- A current email appears related to a task already being discussed.
- A file appears relevant to the present coding issue.
- The user’s current request appears to prefer a particular output structure.

Controls:

- Session-local or short-lived.
- No durable authority.
- No external effect.
- May be logged for debugging or evaluation, but not promoted automatically.

### 10.2 Lane 2: Tentative low-risk retention

Designed for low-risk M1 preferences and selected recommendation-only M2 associations.

Examples:

- The user repeatedly prefers short status summaries.
- A recurring phrase usually refers to the same internal project.
- A user corrects the agent’s preferred terminology.

Controls:

- Visible and removable.
- Limited to retrieval or recommendation influence.
- No external actions.
- May be reinforced through explicit confirmation or repeated validated use.
- Must preserve scope, such as personal versus project-level.

### 10.3 Lane 3: Evidence-backed reinforcement

Designed for operational associations with meaningful utility but reversible consequences.

Examples:

- A type of intake message repeatedly maps to a specific project suggestion.
- A recurring task benefits from a known context bundle.
- A user repeatedly confirms a proposed categorization or routing behavior.

Controls:

- Evidence references required.
- Correction and conflict checks.
- Recommendation influence only unless a separate policy permits local workflow effects.
- Later outcomes capable of reducing trust.

### 10.4 Lane 4: Promotion and review

Designed for reusable capabilities, shared knowledge, and meaningful local workflow behavior.

Examples:

- A troubleshooting method repeatedly succeeds under verification.
- A structured intake procedure consistently produces approved tasks.
- An operations agent proposes a reusable execution checklist.
- A recurring context pattern is candidate for a default operational bundle.

Controls:

- Structured evidence.
- Validation results or controlled replay where feasible.
- Versioned artifact.
- Explicit authority ceiling and prohibited effects.
- Review or an approved policy pathway.
- Rollback, supersession, and monitoring.

### 10.5 Lane 5: Restricted authority

Designed for mutations that affect external action or governance.

Examples:

- Permission to send replies without review.
- Permission to deploy changes.
- Permission to delete or modify external data.
- Change to identity or access policy.
- Security control exemption.
- Trust elevation or enforcement reduction.

Controls:

- Explicit authorized review.
- No self-approval by proposing agent.
- Evidence and risk analysis.
- Versioned policy decision.
- Revocation or kill switch.
- Post-change monitoring.
- Fail-closed treatment when inputs are incomplete or ambiguous.

---

## 11. Adaptive Charters

Every agent role should operate under a versioned adaptive charter. A charter specifies what the agent may learn with low friction, what it may propose only under evidence, and what it may never authorize for itself.

### 11.1 Workflow and communications agent

Natural adaptive scope:

- Presentation and organization preferences.
- Communication workflow suggestions.
- Task and project association recommendations.
- User-confirmed operational routines.
- Candidate response or intake procedures, without autonomous execution.

Elevated control boundary:

- External communications.
- Customer or stakeholder commitments.
- Formal work promotion.
- Assignment changes with material consequence.
- Automated response authority.

### 11.2 Supportive or educational agent

Natural adaptive scope:

- Preferred pacing and format.
- User-approved learning or accessibility preferences.
- Communication sensitivity preferences explicitly established by the user.

Elevated control boundary:

- Clinical claims.
- Diagnoses.
- Psychological interpretations.
- Risk assessments treated as durable facts.
- Sensitive personal conclusions inferred from conversation.

A supportive system may adapt to the user’s preferred experience without claiming authority over the user’s inner life.

### 11.3 Operations agent

Natural adaptive scope:

- Procedural observations.
- Candidate verification routines.
- Workflow sequencing.
- Tool-use recommendations.
- Reusable capability candidates.

Elevated control boundary:

- Promoted executable procedures.
- Expanded tool authority.
- Changes to shared operations.
- External execution without approval.
- Systemwide workflow mutation.

Operations agents are natural candidates for useful procedural learning. They are also natural candidates for producing a very efficient disaster if procedural promotion lacks validation.

### 11.4 Security agent

Natural adaptive scope:

- Threat observations.
- New indicators.
- Detection candidates.
- Failed-control observations.
- Protective recommendations.

Asymmetric control boundary:

- A recommendation that strengthens protection may be evaluated under proportional policy.
- A recommendation that weakens enforcement, raises trust, bypasses controls, expands privileges, or broadens autonomous security action must require explicit authorization.

A useful security principle follows:

> **A protective agent may learn faster toward caution than toward permission.**

### 11.5 Future specialist agents

New agents should not inherit unspecified mutation authority. Each agent should declare:

- adaptive domain;
- permitted low-risk mutations;
- required evidence classes;
- prohibited self-expansion;
- sharing limits;
- sensitivity handling;
- maximum downstream authority absent separate review.

---

## 12. The Adaptive Mutation Contract

A system needs a structured proposal object so mutations can be evaluated consistently, explained later, and audited.

A conceptual contract is shown below:

```json
{
  "proposal_id": "unique-proposal-reference",
  "proposing_agent": {
    "id": "agent-instance-or-role-reference",
    "role": "operations-agent",
    "charter_version": "1.0"
  },
  "target": {
    "reference": "stable-memory-or-capability-reference",
    "domain": "adaptive-operational-memory",
    "class": "M3_REUSABLE_PROCEDURE",
    "scope": "project"
  },
  "mutation": {
    "operation": "promote",
    "current_strength": "reinforced",
    "proposed_strength": "promoted",
    "downstream_authority": "A1_RECOMMENDATION_INFLUENCE",
    "reversibility": "versioned-revocable"
  },
  "basis": {
    "justification": "Repeated validated procedure reduced resolution failures",
    "evidence_refs": ["evidence-1", "evidence-2"],
    "validation_result_refs": ["validation-1"],
    "correction_refs": [],
    "confidence": 0.88,
    "freshness_requirement": "revalidate-after-dependency-change",
    "sensitivity": "internal"
  },
  "recommended_handling": "require_review",
  "policy_decision_ref": null
}
```

### 12.1 Required properties of the contract

The contract must make visible:

- who is proposing the change;
- under which charter;
- what is being changed;
- whether the mutation is transient, reinforced, promoted, or canonical;
- what downstream behavior it may influence;
- what evidence supports it;
- what corrections or failures weaken it;
- how fresh the supporting basis must be;
- how reversible the change is;
- what policy decision was applied.

### 12.2 Safe compatibility behavior

When an older system submits an underspecified mutation, the system must not assign it a low-risk classification merely because fields are missing. Unknown consequence must not be converted into presumed safety.

The only reasonable exception is explicitly transient, non-durable, no-side-effect behavior that cannot escape its local context.

---

## 13. Evidence and Validation

An adaptive system earns trust through the quality of its evidence, not the confidence of its language.

### 13.1 Evidence categories

Different mutations require different kinds of support.

| Evidence Type | Example | Appropriate Uses |
| --- | --- | --- |
| Explicit user preference | User selects a default view | Personal preference retention |
| User correction | User rejects an assignment or categorization | Negative reinforcement, demotion |
| Operational outcome | Task completed successfully and accepted | Procedure evaluation |
| Controlled replay | Procedure repeated under known inputs | Capability promotion evidence |
| Authoritative external record | Signed agreement, verified system state | Identity-bearing or canonical facts |
| Security evidence | Validated alert, confirmed incident | Protective mutation proposal |
| Policy approval | Authorized decision record | Privilege or authority change |

### 13.2 Evidence requirements should rise with consequence

M0 and certain M1 mutations may require no durable evidence because they expire or remain user-editable preferences.

M2 associations should preserve evidence when they influence recommendations.

M3 capabilities should require outcomes, validation, or controlled replay.

M4 facts should require authoritative sources or explicit review.

M5 authority changes should require explicit governance evidence, risk consideration, authorization, and revocation.

### 13.3 Do not fabricate proof

A system must not create durable memory with placeholder evidence references or label context completeness as task success. These errors undermine the entire purpose of governed learning.

The framework requires that unsubstantiated learning be visibly unsubstantiated and restricted accordingly. An observation may be legitimate without being proven. The dishonesty begins only when the system presents it as substantiated or permits it to act as though it were.

---

## 14. Evaluation: Measuring What Actually Improved

Adaptive systems need separate measures for separate claims.

A retrieval subsystem may successfully produce a complete context bundle. That does not prove the agent made the correct decision. An agent may complete an action. That does not prove the action was appropriate. A user may approve one result. That does not prove the learned procedure generalizes.

A mature evaluation model distinguishes at least:

| Dimension | Question |
| --- | --- |
| Bundle completeness | Did the requested context bundle include its required fields? |
| Retrieval relevance | Was the retrieved context actually useful for the task? |
| Freshness validity | Was the context current enough for its intended use? |
| Policy compliance | Did the agent remain inside its authority and rules? |
| Execution success | Did the action technically complete? |
| Human acceptance | Did an authorized user accept the result or resulting decision? |
| Correction required | Was the learned assumption or action later corrected? |
| Downstream failure | Did later harm or failure trace back to the retained mutation? |
| Replay validation | Did a candidate procedure succeed under controlled re-evaluation? |
| Revocation outcome | Was a promoted capability safely removed or replaced when necessary? |

### 14.1 Why this matters

If a context bundle is complete but leads to a rejected decision, it should not reinforce the resulting procedure.

If a procedure technically succeeds but violates policy, it should not be promoted.

If a recommendation is repeatedly accepted only because users are not reviewing it carefully, the system must not confuse passive approval with proof.

The system should claim improvement only where evidence supports the particular type of improvement claimed.

---

## 15. Governed Reusable Capabilities

One of the most valuable results of adaptation is procedural reuse. An agent may discover a reliable way to solve a recurring problem, prepare an intake workflow, assemble a context bundle, perform a validation sequence, or produce a useful draft.

A safe system should be able to retain such procedures. It should not retain them as unbounded agent-authored habits.

### 15.1 Capability artifact

A governed reusable capability is a versioned artifact representing a validated procedure or repeatable operational pattern.

It should contain:

```text
capability identifier
capability type
version
originating agent
permitted agent roles
purpose and supported use cases
input contract
output contract
authority ceiling
prohibited effects
evidence references
validation results
correction and failure history
promotion decision
lifecycle state
freshness and revalidation requirements
rollback or revocation state
usage and outcome references
```

### 15.2 Capability authority ceiling

Every capability should declare the maximum authority it may influence.

Examples:

- “May retrieve context and produce recommendations only.”
- “May draft a response for review but may not send.”
- “May update an internal status under an approved rule.”
- “May not execute external tools without separate authorization.”

A capability does not receive new authority because it is reliable. Authority remains a separate policy decision.

### 15.3 Capability promotion flow

```text
Repeated behavior or procedure candidate
        ↓
Outcome and correction evidence gathered
        ↓
Validation or replay conducted where appropriate
        ↓
Lifecycle evaluator recommends promotion
        ↓
Authority evaluator checks scope, ceiling and required review
        ↓
Approved versioned capability becomes available
        ↓
Usage and later outcomes are monitored
        ↓
Capability may be reinforced, revised, demoted, superseded or revoked
```

### 15.4 Why this is better than informal agent skills

Informal agent-authored skills can be useful, but may conceal authority, scope, stale assumptions, unsupported claims, or unsafe execution behavior.

A governed capability exposes:

- why it exists;
- who created it;
- what it is allowed to do;
- what validated it;
- what it must never do;
- when it must be revalidated;
- what happens if it fails.

That is the difference between a useful note and an accountable operational capability.

---

## 16. Example Domain Applications

### 16.1 Communications and work intake

A communications agent may observe that a certain category of inbound request often corresponds to an existing project or requires a specific task template.

A proportional adaptation sequence might look like this:

```text
Inbound conversation received
        ↓
Agent suggests category, project association, urgency, and task extraction
        ↓
User confirms or corrects suggestions
        ↓
Work completes or conversation is reopened
        ↓
Outcome signals are recorded
        ↓
Repeated validated pattern becomes an M2 recommendation association
        ↓
If reliable and bounded, a task-template capability may become an M3 promotion candidate
```

Permitted low-friction learning:

- Recommendation that messages of a known type usually map to a given project.
- Tentative preference for a task presentation format.
- Correction weighting when urgency is repeatedly overestimated.

Not automatically permitted:

- Claiming a customer commitment exists.
- Sending an external response.
- Resolving a conversation without evidence.
- Promoting autonomous reply permissions.

### 16.2 Operations and engineering execution

An operations agent may repeatedly use a troubleshooting sequence that resolves a build or deployment problem.

A safe adaptive path:

```text
Procedure attempted
        ↓
Verification confirms outcome
        ↓
Failures and corrections are retained
        ↓
Repeated reliable sequence becomes a capability candidate
        ↓
Reviewed capability is promoted with a version and authority ceiling
        ↓
Future agent sessions may recommend or apply it only within its approved scope
```

This permits practical learning while ensuring a successful procedure does not suddenly confer tool access or deployment authority.

### 16.3 Supportive or educational interaction

A supportive agent may learn that a user prefers shorter prompts, fewer questions at once, audio-friendly language, or a particular pacing of instruction.

These may be eligible for low-friction retention as reversible preferences.

It must not infer or promote sensitive conclusions such as diagnosis, trauma history, psychological state, or treatment needs without an appropriate authoritative and consent-based framework.

The purpose is adaptation to user preference, not automated interpretation of a person.

### 16.4 Security and policy evolution

A security agent may identify a new threat pattern or recommend stronger detection based on validated incidents.

Permitted adaptation may include:

- retaining threat evidence;
- proposing new detection logic;
- recommending stricter handling;
- associating repeated enforcement events with a risk class.

Restricted adaptation includes:

- weakening a policy;
- adding an exception;
- elevating trust;
- bypassing enforcement;
- expanding autonomous response authority.

The asymmetric treatment is essential because false caution is often reversible, while silently expanded permission may produce immediate compromise.

---

## 17. Minimizing Friction Without Abandoning Control

A common objection to governance frameworks is that they slow systems down until users ignore them. That objection is valid when governance is placed at the wrong boundaries.

A proportional design reduces friction through four techniques.

### 17.1 Auto-apply harmless, reversible state

Users should not approve every session-level association or editable display preference. These changes should be transparent, bounded, and removable.

### 17.2 Limit early learning to recommendation influence

Many useful patterns can improve suggestions without triggering any action. This allows the system to learn in practice while its beliefs remain contestable.

### 17.3 Request review when a mutation becomes influential

Promotion into default behavior, reusable capability, shared fact, or consequential workflow state is where review becomes worthwhile.

### 17.4 Require explicit authority for external or governance effects

External representation, destructive actions, privilege changes, and policy alterations warrant intentional authorization. The user’s attention should be reserved for precisely these moments.

### 17.5 Measure governance friction

A serious implementation should monitor:

- number of review requests by mutation class;
- false escalations into review;
- user reversals after automatic retention;
- unsafe proposals blocked;
- time and effort required for promoted capability review;
- rate of corrected or revoked learned state;
- evidence that automatic low-risk lanes actually reduce repetitive user burden.

Governance should itself be evaluated. Otherwise it becomes another untested system assumption wearing a formal shirt.

---

## 18. Inspection and User Control

A system that adapts must make its meaningful adaptation visible.

An adaptive-state interface should allow authorized users to examine:

- retained preferences;
- operational associations;
- procedure candidates;
- promoted capabilities;
- policy-restricted proposals;
- blocked or vetoed mutations;
- decayed, superseded, revoked, or corrected state;
- evidence and validation supporting each meaningful item;
- which recommendations or actions were influenced by retained state.

The user should be able to understand:

- What did the system learn?
- Why did it retain that?
- How strongly does it trust it?
- What is it allowed to influence?
- Did it ever lead to an action?
- Was it later corrected or found to be stale?
- Can it be removed, demoted, revised, or revoked?

Inspection should not become an endless list of ephemeral traces. Low-risk transient observations may be aggregated or naturally expire. Attention should center on retained state that meaningfully shapes future behavior.

---

## 19. Security and Governance Invariants

Certain rules should remain true regardless of implementation technology.

1. **No mutation becomes more authoritative merely because it was proposed by a trusted agent.**

2. **No high-impact mutation should be auto-approved because classification data is missing.**

3. **No reusable capability implicitly grants external-action or governance authority.**

4. **No agent should approve its own expansion of privilege, trust, exception, or autonomy.**

5. **No evidence-bearing claim should rely on placeholder or fabricated provenance.**

6. **No promotion decision should rely only on recurrence, retrieval success, or model confidence.**

7. **No security-relaxing mutation should be treated symmetrically with a protective observation.**

8. **No learned state should be immune to correction, staleness, conflict, revocation, or failure evidence.**

9. **No context assembly should hide whether a memory is tentative, promoted, canonical, stale, corrected, or disputed.**

10. **No system should claim safe self-improvement without measuring downstream outcomes and authority compliance.**

---


## 20. Standards and Regulatory Alignment Position

This architecture is intended to support secure design, risk-management documentation, assurance evidence, and regulatory analysis for adaptive agent systems. It is not, by itself, a certification, conformity assessment, legal opinion, or claim of compliance with any law or standard.

Three external reference frameworks are particularly relevant:

1. **OWASP Top 10 for Agentic Applications 2026**, published through the OWASP GenAI Security Project's Agentic Security Initiative, identifies critical security risks for autonomous and agentic AI systems, including goal hijack, tool misuse, identity and privilege abuse, memory and context poisoning, insecure inter-agent communication, cascading failures, human-agent trust exploitation, and rogue agents.[^owasp-top10]
2. **NIST AI Risk Management Framework 1.0**, its Playbook, and the **Generative AI Profile, NIST AI 600-1**, provide voluntary risk-management guidance for trustworthy AI design, development, use, evaluation, and generative-AI-specific risk treatment. The Playbook structures suggested actions across the functions **Govern, Map, Measure, and Manage**.[^nist-rmf][^nist-playbook][^nist-genai]
3. **The European Union AI Act** establishes legal obligations based on role and use-case classification. For high-risk systems, the European Commission identifies requirements including risk assessment and mitigation, activity logging, technical documentation, deployer information, human oversight, and robustness, cybersecurity, and accuracy. The Act also includes transparency requirements and post-market monitoring and serious-incident responsibilities where applicable.[^eu-ai-act]

The framework alignment in this paper therefore uses three different lenses:

| Lens | Question addressed | What this architecture can provide |
| --- | --- | --- |
| OWASP agentic security | How could adaptive agents be attacked, corrupted, misused, or become unsafe? | Threat-facing controls over mutation, memory, authority, tools, lineage, and containment |
| NIST AI RMF | How should an organization govern, map, measure, and manage adaptive-agent risk? | Risk taxonomy, control evidence, metrics, lifecycle decisions, review boundaries, and monitoring |
| EU AI Act | What legal obligations may apply to a particular deployment in the EU? | Technical and procedural building blocks supporting traceability, oversight, risk controls, documentation, and monitoring |

### 20.1 Alignment is not compliance

The presence of controls described in this paper does not establish compliance. Compliance depends upon at least:

- the legal role of the organization, such as provider, deployer, importer, distributor, or other actor;
- the deployed use case and whether it falls within a regulated classification;
- the model, system, connected tools, affected persons, and operational environment;
- actual implemented controls, testing evidence, documentation, monitoring, incident response, and governance accountability;
- applicable jurisdiction, sectoral laws, contractual obligations, privacy obligations, and changes to regulatory guidance.

A system may implement proportional mutation authority and still fail compliance through poor data governance, inadequate documentation, inaccessible oversight, insufficient cybersecurity, unsupported user claims, or a deployment in a regulated domain for which additional obligations apply.

The correct external position is:

> **Proportional Adaptive Mutation Authority is a compliance-supporting architecture for adaptive agent systems. It is not an independent claim of conformity with a standard or regulation.**

---

## 21. OWASP Agentic Security Threat Crosswalk

OWASP's agentic security work is especially relevant because adaptive mutation creates a persistent pathway between untrusted input, agent reasoning, memory, future behavior, and tool or external-action authority. A malicious instruction or incorrect inference is no longer harmful only once. If it is promoted into memory or procedure, it may recur across future tasks with increasing apparent legitimacy.

OWASP's 2026 Agentic Top 10 emphasizes least agency and observability: unnecessary autonomy increases attack surface, while lack of visibility into goals, tool use, and agent behavior allows small failures to become system-wide failures.[^owasp-top10]

### 21.1 Crosswalk to the OWASP Agentic Top 10

| OWASP risk | Mutation-authority relevance | Required architectural controls | Residual gap requiring implementation evidence |
| --- | --- | --- | --- |
| **ASI01: Agent Goal Hijack** | An attacker may steer an agent into proposing durable mutations, unsafe capability changes, or expanded action requests through poisoned documents, communications, tool output, or peer-agent messages. | Bind mutations to agent charter, stated intent, evidence source, authority ceiling, policy decision, and audit lineage. Require review for goal-changing or high-impact mutation paths. | Must implement adversarial tests for injected mutation proposals, goal-drift detection, and policy pause or veto behavior. |
| **ASI02: Tool Misuse and Exploitation** | A learned procedure may normalize unsafe tool calls or expand the agent's practical ability to modify external systems. | Downstream authority classes, capability authority ceilings, explicit external-action authorization, tool-specific policy checks, and monitored reuse. | Must prove enforcement at execution time, not only in stored capability metadata. |
| **ASI03: Identity and Privilege Abuse** | An agent may attempt to learn a broader identity role, inherit access, or convert past approval into future privilege. | Versioned charters, scope-bound authority, prohibition on self-approved privilege expansion, identity-bound approvals, and fail-closed treatment of incomplete authority claims. | Requires concrete identity binding, delegated-authority validation, and privilege escalation test cases. |
| **ASI04: Agentic Supply Chain Vulnerabilities** | Imported procedures, tools, memory providers, templates, or external agent artifacts may carry poisoned mutation logic or deceptive capabilities. | Source provenance, trust classification, validation before promotion, version and hash tracking where appropriate, and limited authority for untrusted imports. | Requires supply-chain intake policy, dependency inventory, update review, and artifact scanning controls. |
| **ASI05: Unexpected Code Execution** | Procedural learning can accidentally promote code-running behavior or pass untrusted outputs into interpreters or tool runtimes. | Capability authority ceilings, restricted external-execution handling, validation/replay boundaries, and explicit code-execution classification. | Requires sandboxing, command policy enforcement, payload validation, and execution-specific red-team testing. |
| **ASI06: Memory and Context Poisoning** | This is the central threat addressed by mutation authority: malicious or mistaken inputs may persist and influence later decisions. | Evidence-bearing mutation contracts, lifecycle strength, tentative versus canonical separation, conflict checks, correction, revocation, lineage, freshness, and no placeholder proof. | Requires poisoning detection, trusted-source weighting, contamination analysis, and evaluation against persistence attacks. |
| **ASI07: Insecure Inter-Agent Communication** | One agent may send mutation proposals, corrections, or operational context to another without adequate authenticity or scope controls. | Per-agent charters, sender identity, scope restrictions, evidence provenance, authority decision records, and no automatic acceptance of peer-agent assertions. | Requires authenticated inter-agent messaging, message integrity, replay protection, and trust-boundary tests. |
| **ASI08: Cascading Failures** | A bad promoted memory or procedure can contaminate many agents, tasks, or automated actions. | Scope classifications, promotion gates, capability versioning, usage tracing, demotion/revocation, circuit-breaker concepts, and monitoring of downstream failure. | Requires blast-radius controls, rollback testing, cross-agent contamination simulation, and emergency suspension procedures. |
| **ASI09: Human-Agent Trust Exploitation** | Users may over-trust polished explanations for mutations or approve high-impact changes under misleading framing. | Visible evidence, explanation of authority impact, distinction between suggestion and executed change, meaningful human oversight, and review only where attention matters. | Requires human-factors evaluation of review interfaces, manipulation-resistant approval summaries, and audit of approval quality. |
| **ASI10: Rogue Agents** | An agent may pursue a mutation or capability inconsistent with its charter, even without active attacker control. | Charter validation, behavior monitoring, mutation veto, authority ceilings, lineage, suspension/revocation, and separate governance for autonomy. | Requires runtime anomaly detection, agent disablement/containment, investigation evidence, and recovery procedures. |

### 21.2 OWASP-specific security requirements for adaptation

A mature implementation should add explicit threat controls that are not fully supplied by lifecycle governance alone.

#### Treat adaptive inputs as potentially hostile

Evidence used in a mutation proposal can originate from email, documents, web retrieval, tool results, databases, calendar events, peer agents, imported procedures, or user-entered instructions. The system should track the source class and trust boundary of every evidence reference. Untrusted natural-language input should never directly authorize promotion or external action.

#### Separate memory eligibility from action eligibility

A poisoned observation might be safely retained as disputed evidence for later investigation, while being barred from recommendation or execution influence. This is safer than either deleting all suspicious evidence or allowing it to influence behavior because it exists in memory.

#### Enforce least agency at the capability boundary

A reusable capability should declare the least authority necessary for its purpose. A capability intended to suggest a remediation plan should not receive shell execution authority. A capability intended to draft a message should not send one. A capability intended to detect security concern should not weaken the control that raised it.

#### Monitor contamination and propagation

A mutation accepted in one scope should not silently spread into other agents, projects, or users. The system should be able to answer:

- Which agents consumed this retained mutation?
- Which recommendations or actions did it influence?
- Did any later correction or failure trace back to it?
- Can its use be suspended globally while it is reviewed?

### 21.3 OWASP assurance artifacts

A conforming implementation effort should produce at least:

| Artifact | Purpose |
| --- | --- |
| Agentic threat model for adaptive mutation | Identify attack paths from input, tools, agents, and imported artifacts into retained state and future action |
| Memory/context poisoning test suite | Demonstrate resistance to adversarial persistence and malicious evidence promotion |
| Tool authority and capability-ceiling matrix | Prove learned procedures cannot silently gain dangerous execution rights |
| Identity and inter-agent trust model | Document authenticating principals, delegated authority, proposal origin, and peer-agent trust treatment |
| Cascade/containment playbook | Define suspension, rollback, revocation, blast-radius review, and recovery after harmful mutation |
| Human-approval abuse testing | Evaluate whether a user can distinguish benign learning from consequential authority changes |

---

## 22. NIST AI RMF and Generative AI Profile Mapping

NIST AI RMF is voluntary, but it provides a useful organizational discipline for adaptive agents because it separates risk work into four functions: **Govern**, **Map**, **Measure**, and **Manage**. The Playbook explicitly describes its suggestions as voluntary and adaptable to an organization's use case rather than a universal checklist.[^nist-playbook]

Proportional Adaptive Mutation Authority aligns closely with the AI RMF because it is not merely a runtime filter. It is an approach to risk governance, context classification, measurement, and ongoing control across an adaptive system lifecycle.

### 22.1 Govern

**NIST purpose:** Establish policies, roles, accountability, organizational practices, and risk-culture foundations.

| Architecture contribution | Governance meaning | Required implementation evidence |
| --- | --- | --- |
| Versioned adaptive charters for each agent role | Defines permitted learning domains and prohibited self-expansion | Charter registry, change control, owner, review cadence, approved versions |
| Mutation target and authority taxonomy | Establishes a shared language for consequences | Policy definitions, decision tables, exception handling |
| Prohibition on self-authorized privilege or governance expansion | Prevents conflicts of interest in agent adaptation | Enforced policy rules and negative test evidence |
| Promotion and restricted-authority review lanes | Concentrates human accountability at meaningful decisions | Approval roles, reviewer eligibility, override procedure, audit records |
| Visibility, revocation, and correction rights | Preserves human control over retained adaptive state | Inspection UI/API, authorization tests, deletion/revocation evidence |

**Gap that must be resolved in a deployment:** The architecture does not itself appoint accountable owners, define organizational risk appetite, set acceptable residual-risk thresholds, establish training requirements, or determine when legal and compliance review is mandatory.

### 22.2 Map

**NIST purpose:** Understand context, intended purposes, foreseeable uses and misuse, affected parties, impacts, and risk characteristics.

| Architecture contribution | Mapping value | Required implementation evidence |
| --- | --- | --- |
| Mutation target classes M0 through M5 | Classifies what kind of adaptive change is proposed | Use-case inventory and target-class assignment rules |
| Downstream authority classes A0 through A5 | Identifies how learned state can affect behavior | Data-flow and action-authority diagrams |
| Agent charters | Identifies appropriate domain boundaries for agent roles | Role definitions and out-of-domain rejection tests |
| Source-shaped context and evidence | Preserves whether information originated as a conversation, task, policy, code artifact, external result, or approval | Evidence schema and provenance capture |
| Sensitivity, sharing scope, and freshness concepts | Surfaces privacy, cross-agent propagation, and stale-context concerns | Data classification and retention mapping |

**Required addition for external deployment:** Before use in a regulated or high-impact environment, the organization must map affected people, foreseeable harms, misuse cases, deployment role, connected systems, external-action paths, privacy impacts, and domain-specific laws. Mutation severity does not substitute for use-case risk classification.

### 22.3 Measure

**NIST purpose:** Analyze, assess, benchmark, and monitor identified risks using appropriate methods and metrics.

The architecture's strongest measurement claim is that it refuses to equate context completeness with safe or successful work. Measurement should distinguish:

| Metric family | Example measures |
| --- | --- |
| Context quality | bundle completeness, source freshness, retrieval relevance, conflict rate |
| Mutation quality | evidence sufficiency, correct target-class assignment, false promotion rate, stale-retention rate |
| Governance effectiveness | prohibited authority escalation blocks, review override frequency, unauthorized-action prevention, audit completeness |
| Human oversight quality | approval reversal rate, misunderstood-review rate, time spent on consequential versus low-risk reviews |
| Outcome quality | execution success, human acceptance, correction required, downstream failure attributable to retained state |
| Capability reliability | replay validation success, post-promotion failure, revocation rate, version drift, out-of-scope invocation blocks |
| Security resilience | poisoning-detection performance, goal-hijack resistance, inter-agent spoofing resistance, containment recovery time |

The Generative AI Profile is relevant because adaptive agents inherit generative-AI risks while adding memory persistence, tools, delegated decisions, and autonomous-action pressure. NIST states that the GenAI Profile is intended to help organizations identify unique risks posed by generative AI and select risk-management actions suited to their goals and priorities.[^nist-genai]

**Required implementation evidence:**

- documented evaluation datasets and scenarios;
- red-team and misuse tests;
- threshold definitions for promotion, demotion, suspension, and revocation;
- results separated by mutation class and authority level;
- uncertainty and known limitation reporting;
- evidence that metrics reflect outcomes rather than self-reported agent confidence.

### 22.4 Manage

**NIST purpose:** Prioritize, respond to, monitor, and manage risks based on assessed impact and available resources.

| Architecture contribution | Management function | Required implementation evidence |
| --- | --- | --- |
| Handling lanes from automatic to veto | Matches control intensity to consequence | Policy-engine behavior and test matrix |
| Demotion, supersession, blocking, and revocation | Allows response to correction or harm | Operational workflows and rollback verification |
| Capability authority ceilings | Constrains impact even after promotion | Enforcement and out-of-scope invocation tests |
| Restricted treatment of external action and governance change | Prevents learned state from becoming hidden autonomy | Authorization controls, deny tests, monitoring |
| Lineage and downstream-use tracing | Supports incident analysis and containment | Audit queries and response playbooks |

**Required operational addition:** A deployed system needs incident response ownership, escalation severity, notification duties, rollback timing, emergency disablement, post-incident review, residual-risk acceptance, and decommissioning or data-removal procedures.

### 22.5 NIST evidence package

A deployable implementation should generate a minimum assurance package:

| NIST function | Evidence package |
| --- | --- |
| Govern | Policy charter, accountable roles, authority matrix, approval rules, exception process, training records where required |
| Map | System context inventory, data and action flows, use-case classification, affected-person analysis, misuse/threat cases, connected tool register |
| Measure | Evaluation plan, test fixtures, red-team results, metric thresholds, model/system limitations, human-review usability findings |
| Manage | Release decision, monitoring plan, incident response, rollback/revocation procedure, residual-risk register, change-management history |

---

## 23. European Union AI Act Obligation-Support Analysis

The EU AI Act requires a separate analysis because it is law rather than voluntary guidance. A mutation-governance architecture does not determine whether a deployed system falls into a prohibited, high-risk, transparency-risk, general-purpose AI, or minimal/no-risk category. That determination depends on the system's role, intended purpose, deployment context, functionality, and the legal classification of the relevant use case.

The European Commission identifies high-risk examples that include AI used in education, employment and worker management, certain essential services, biometrics, law enforcement, migration and border control, administration of justice, and safety components in critical infrastructure or products. For high-risk systems, it identifies obligations including risk assessment and mitigation, data quality, logging, technical documentation, information to deployers, human oversight, and robustness, cybersecurity and accuracy.[^eu-ai-act]

### 23.1 Applicability gate

Before relying on this architecture in an EU-facing deployment, the deploying organization should complete a legal and operational applicability assessment:

| Question | Why it matters |
| --- | --- |
| What is the intended purpose of the agent system? | Risk classification depends on intended purpose and use. |
| Is the organization acting as a provider, deployer, or another regulated actor? | Obligations differ by role. |
| Does the system affect employment, education, essential services, legal decisions, healthcare, security, biometric use, critical infrastructure, or other high-impact domains? | These areas may trigger elevated requirements or separate sectoral controls. |
| Does the agent communicate directly with humans or generate public-interest content? | Transparency obligations may apply. |
| Is the system built on or providing a general-purpose AI model? | GPAI provider obligations are distinct from application-layer obligations. |
| Does adaptive mutation alter system purpose, capability, autonomy, or risk profile? | Substantial changes may require renewed assessment and documentation. |

Mutation classes M0 through M5 describe the internal consequence of a learned change. They do **not** replace legal classification of the overall AI system or its deployed use.

### 23.2 High-risk obligation-support matrix

| EU AI Act consideration identified by the Commission | How proportional mutation authority supports it | What remains required for an actual deployment |
| --- | --- | --- |
| Adequate risk assessment and mitigation | Target/authority classification, threat crosswalk, policy lanes, restricted mutations, monitoring and revocation | Formal risk-management system tied to intended purpose and applicable legal category |
| Data quality to minimize discriminatory outcomes | Evidence provenance, correction tracking, source authority, lifecycle demotion for invalid data | Dataset governance, representativeness analysis, bias testing and domain-specific quality controls |
| Logging for traceability | Mutation lineage, approval evidence, capability version/use history, outcome records | Logging completeness requirements, retention, security, audit access, export and operational procedures |
| Technical documentation | Adaptive contract, charters, authority matrix, capability artifacts, testing records | Complete technical file addressing system purpose, design, performance, risks and conformity obligations |
| Clear information to deployers | Inspection model, authority ceilings, known limitations and decision boundaries | Deployer instructions, training, operating conditions, prohibited uses, human-review guidance |
| Human oversight | Consequence-based review lanes, override, correction, revocation and inspection | Qualified oversight roles, realistic intervention mechanisms and demonstrated usability |
| Robustness, cybersecurity and accuracy | OWASP-aligned threat controls, poisoning resistance, authority enforcement, outcome metrics | Full security testing, accuracy/performance evaluation, resilience requirements and ongoing maintenance |
| Post-market monitoring | Outcome monitoring, correction, failure association, demotion and revocation | Formal post-market plan, incident tracking, reporting triggers and responsible parties |
| Serious incident and malfunction reporting | Lineage and outcome evidence can support investigation | Legal reporting workflow and thresholds where obligations apply |

### 23.3 Transparency and human interaction

The Commission states that humans should be informed when interacting with systems such as chatbots where necessary to preserve trust, and that transparency rules come into effect in August 2026.[^eu-ai-act]

For adaptive agent systems, transparency should include more than disclosing that an AI is present. Where relevant to the interaction, a deployment should clarify:

- whether the agent is acting as an assistant, representative, recommender, or autonomous actor;
- whether a communication was drafted by an AI, approved by a human, or sent under an automated policy;
- whether adaptive memory materially influenced a recommendation or action;
- how users can seek review, correction, or escalation where a learned behavior affects them;
- whether generated public-interest content or synthetic media requires marking or labelling.

This paper does not define a universal disclosure interface because the correct disclosure depends on use case and applicable obligations. It does establish that adaptive authority must remain inspectable and must not conceal changes in an agent's capacity to act.

### 23.4 Post-deployment monitoring and incident response

The Commission identifies human oversight and monitoring by deployers, post-market monitoring by providers, and reporting of serious incidents and malfunctioning after high-risk systems are placed on the market, where applicable.[^eu-ai-act]

Proportional mutation authority supplies several useful mechanisms for such monitoring:

- versioned retained mutations and promoted capabilities;
- records of evidence, review and authority decisions;
- mapping from retained state to later recommendations or actions;
- correction, conflict, failure, supersession and revocation events;
- ability to suspend or revoke harmful learned capability.

A deployment still requires operational procedures for:

- monitoring ownership and frequency;
- incident classification;
- preservation and analysis of logs;
- notification and reporting;
- capability rollback;
- model or policy updates;
- communication to affected users or deployers;
- reassessment after substantial change.

### 23.5 Current timeline caution

As reflected on the European Commission's AI Act policy page at the time of this revision:

- the AI Act entered into force on 1 August 2024;
- prohibited-practice and AI literacy obligations began applying on 2 February 2025;
- governance rules and GPAI obligations became applicable on 2 August 2025;
- transparency rules are described as applying from August 2026;
- following the political agreement described by the Commission in May 2026, rules for systems used in certain high-risk areas are stated to apply from 2 December 2027, while rules for high-risk systems embedded in regulated products are stated to apply from 2 August 2028.[^eu-ai-act]

This timeline is legally and politically sensitive and may continue to be refined through implementation acts, guidelines, standards, or legislative developments. A real deployment should re-check current official sources rather than relying solely on this paper's revision date.

### 23.6 EU AI Act documentation package supported by this architecture

Where applicable to the classified use case, this architecture can support development of:

| Evidence artifact | Adaptive-system content |
| --- | --- |
| Intended-purpose and role assessment | Agent purpose, autonomy level, mutation scope, actors, external actions and deployed environment |
| Risk register | Memory poisoning, tool misuse, privilege expansion, inaccurate promotion, human-overreliance, cascade risk |
| Human oversight plan | Review lanes, reviewer roles, override, correction, revocation and emergency suspension |
| Traceability log schema | Mutation proposals, evidence, lifecycle state, capability versions, approvals, use and outcomes |
| Technical documentation input | Agent charters, authority taxonomy, policy engine rules, evaluation and limitations |
| Robustness/cybersecurity evaluation | OWASP-aligned attack tests, authority enforcement, contamination and rollback scenarios |
| Post-deployment monitoring plan | Failure signals, corrections, incident thresholds, capability demotion and monitoring cadence |

### 23.7 Regulatory gap statement

This architecture intentionally does not define:

- legal system classification;
- conformity assessment procedure;
- sector-specific compliance requirements;
- fundamental-rights impact assessment methodology;
- data-protection impact assessment;
- model-provider obligations for general-purpose AI;
- formal incident-reporting deadlines or authorities;
- country-specific enforcement procedures.

Those must be performed by qualified legal, compliance and operational stakeholders for the actual system and use case.

---

## 24. Framework-Derived Control Additions and Unresolved Gaps

The core architecture remains sound, but the external alignment review requires additional explicit controls before an implementation could credibly present itself as mature.

### 24.1 Required control additions

| Control addition | Why required | Framework driver |
| --- | --- | --- |
| Adversarial memory and context poisoning test suite | Adaptive state creates persistent corruption risk | OWASP ASI06; NIST Measure |
| Goal-drift and mutation-intent validation | Mutation proposals can be redirected through poisoned context | OWASP ASI01; NIST Map/Measure |
| Tool and capability authority enforcement at execution time | Stored authority ceilings are insufficient if runtimes ignore them | OWASP ASI02/ASI05; NIST Manage |
| Identity-bound proposal and approval controls | Adaptive systems may inherit or expand privilege improperly | OWASP ASI03; NIST Govern |
| Authenticated inter-agent proposal protocol | Peer agents cannot be treated as trusted untyped text sources | OWASP ASI07 |
| Cascade containment and emergency suspension | Harmful promoted state can propagate across agents and actions | OWASP ASI08/ASI10; NIST Manage |
| Human-factors evaluation for approval interfaces | Consequential review can fail through over-trust or fatigue | OWASP ASI09; NIST Measure; EU human oversight |
| Use-case and affected-person risk classification | Internal mutation class cannot substitute for deployment risk assessment | NIST Map; EU AI Act |
| Structured technical documentation and monitoring package | Traceability must be operationally usable and auditable | NIST Govern/Manage; EU AI Act |
| Transparency/disclosure analysis for human-facing deployment | Adaptive agents may communicate or influence people directly | EU AI Act transparency obligations |

### 24.2 Unresolved gaps before external assurance claims

The following cannot be claimed simply by adopting this architecture:

1. Resistance to adversarial mutation or context poisoning until tested.
2. Effective human oversight until review usability and override behavior are demonstrated.
3. Safe cross-agent propagation until authenticated communication and containment are implemented.
4. Reliable capability promotion until outcome-based validation and revocation are proven.
5. Compliance with any jurisdictional obligation until the actual use case, actor role and documentation are assessed.
6. Safe autonomy for external actions until that authority receives separate governance, testing and monitoring.
7. Fundamental-rights protection in high-impact deployments until affected-person and impact analysis is completed.

These gaps are not a weakness of acknowledging external frameworks. They are the point of doing the alignment honestly.

---

## 25. Reference Sources

[^owasp-top10]: OWASP GenAI Security Project, *OWASP Top 10 for Agentic Applications for 2026*, published December 9, 2025, through the Agentic Security Initiative. Official resource page: [OWASP Top 10 for Agentic Applications for 2026](https://genai.owasp.org/resource/owasp-top-10-for-agentic-applications-for-2026/). The resource identifies ASI01 through ASI10 and emphasizes least agency and observability for autonomous and agentic systems.

[^nist-rmf]: National Institute of Standards and Technology, *AI Risk Management Framework*. Official framework page: [NIST AI Risk Management Framework](https://www.nist.gov/itl/ai-risk-management-framework). NIST describes AI RMF as voluntary guidance intended to incorporate trustworthiness considerations into AI design, development, use, and evaluation.

[^nist-playbook]: National Institute of Standards and Technology, *NIST AI RMF Playbook*. Official resource page: [NIST AI RMF Playbook](https://airc.nist.gov/airmf-resources/playbook/). NIST states that the Playbook offers voluntary suggestions aligned to the four AI RMF functions: Govern, Map, Measure, and Manage.

[^nist-genai]: National Institute of Standards and Technology, *Artificial Intelligence Risk Management Framework: Generative Artificial Intelligence Profile*, NIST AI 600-1, released July 26, 2024. Official framework page describing and linking the profile: [NIST AI Risk Management Framework](https://www.nist.gov/itl/ai-risk-management-framework).

[^eu-ai-act]: European Commission, *AI Act: Shaping Europe’s digital future*. Official policy page: [European Commission AI Act overview and implementation timeline](https://digital-strategy.ec.europa.eu/en/policies/regulatory-framework-ai). Accessed for this revision on 2026-05-28.


## 24. Implementation Roadmap

The architecture should be delivered in stages.

### Phase 0: Truthfulness remediation

Before writable adaptation expands:

- Eliminate placeholder evidence in live mutation paths.
- Replace synthetic production timestamps with truthful temporal records.
- Separate retrieval and bundle-quality metrics from execution and outcome metrics.
- Establish tests proving these corrections.

### Phase 1: Typed authority model

Implement:

- mutation target classes;
- lifecycle strength states;
- downstream authority classes;
- handling decisions;
- scope and reversibility;
- adaptive mutation contract;
- initial agent charters.

### Phase 2: Policy, lineage, and inspection

Implement:

- proportional authority evaluation;
- automatic, substantiated, review, and veto lanes;
- lineage and policy decision records;
- correction, conflict, supersession, decay, revocation, and block behavior;
- inspection surfaces.

### Phase 3: Lifecycle recommendations

Implement:

- lifecycle evaluator inputs and outputs;
- retention, reinforcement, decay, promotion, demotion, and supersession candidates;
- strict separation between recommendation and durable application;
- prevention of circular validation.

### Phase 4: Governed reusable capabilities

Implement:

- candidate procedural artifacts;
- validation and replay evidence;
- versioned capability promotion;
- authority ceilings and prohibited effects;
- monitored reuse;
- revision, demotion, and revocation.

### Phase 5: Domain expansion and advanced authority

Expand carefully into:

- communications-derived learning;
- operational workflow improvement;
- supportive preference adaptation;
- security evidence evolution;
- separately governed external-action and autonomous communication capabilities.

External-action autonomy should remain its own deliberate architecture decision, even when the adaptive substrate is mature enough to support it.

---

## 25. Verification Framework

A complete implementation should validate both usefulness and safety.

### 21.1 Foundation verification

Confirm:

- real provenance;
- truthful timestamps;
- distinct outcome metrics;
- correct lineage;
- access boundaries;
- retention behavior.

### 21.2 Authority matrix verification

Test every combination that matters:

- low-risk temporary state handled automatically;
- reversible preference retained visibly;
- operational recommendation reinforced only with evidence;
- procedure promotion requiring validation;
- shared fact requiring authoritative handling;
- external-action or governance proposal requiring restricted review.

### 21.3 Agent charter verification

For each role, test what it is allowed to propose and what must be blocked or escalated.

### 21.4 Friction verification

Determine whether users are interrupted at the correct boundaries rather than for ordinary low-risk learning.

### 21.5 Lifecycle verification

Confirm that:

- corrections reduce trust;
- failures demote or block unsafe reuse;
- stale capabilities require revalidation;
- recommendations cannot self-validate;
- promoted state is revocable.

### 21.6 Capability verification

Test:

- candidate creation;
- substantiation;
- promotion;
- scoped reuse;
- authority ceiling enforcement;
- failure-triggered demotion;
- supersession and revocation.

### 21.7 External authority boundary verification

Confirm that no retained memory or promoted capability silently enables:

- outbound communication;
- destructive action;
- privilege change;
- policy weakening;
- security bypass;
- unapproved external representation.

---

## 26. Risks and Mitigations

| Risk | Consequence | Mitigation |
| --- | --- | --- |
| Too much review friction | Users bypass governance or disable learning | Automatic lanes for reversible low-risk mutations; measure review burden |
| Too little review | Inference becomes durable harmful behavior | Target/authority classes; promotion gates; evidence and lineage |
| Repeated error appears validated | System reinforces bad habit | Corrections, failure signals, replay validation, no recurrence-only promotion |
| Sensitive inference becomes fact | Harmful or invasive behavior | Agent charters, sensitivity rules, authoritative-source requirements |
| Procedure grants hidden autonomy | External effects occur without consent | Capability authority ceilings; separate action authorization |
| Security agent relaxes controls | Increased compromise risk | Asymmetric review rule for trust expansion and enforcement reduction |
| Stale memory remains influential | Wrong recommendations or actions | Freshness requirements, decay, supersession, revalidation |
| Governance becomes opaque | Loss of user trust and accountability | Inspection, rationale, evidence, use history, correction and revocation |
| Evaluation overstates improvement | False confidence in adaptive system | Separate metrics for retrieval, execution, acceptance, correction and failure |

---

## 27. Strategic Implication

The industry often describes agents as “self-improving” when they merely write notes, generate reusable instructions, or retrieve previous conversations. Those capabilities are valuable, but they do not fully answer the central problem.

The real question is not whether an agent can learn.

The real question is:

> **How can an agent system improve its future behavior without silently accumulating unsupported truth, uncontrolled procedure, hidden privilege, or unreviewed consequence?**

Proportional Adaptive Mutation Authority offers an answer:

- Let agents observe broadly.
- Let low-risk preferences and associations improve experience with little friction.
- Require evidence when learning begins to shape recommendations meaningfully.
- Require validation and controlled promotion for reusable capabilities.
- Require explicit authority for external actions and governance changes.
- Preserve lineage, correction, revocation, and measurable outcome monitoring throughout.

This architecture makes adaptive agents more useful without forcing users to choose between stagnation and surrender.

---

## 28. Conclusion

Adaptive agents should not be frozen tools. They should learn, refine, and develop reusable competence over time. But a capable adaptive system must recognize that every retained mutation carries a different degree of influence and a different potential for harm.

A preference can be learned quietly.  
An operational association can be reinforced with evidence.  
A procedure can be promoted after validation.  
An external action can be authorized only under deliberate policy.  
A governance change must never be smuggled through the back door of “learning.”

The future of trustworthy agent systems is not unrestricted self-improvement and not permanent immobility. It is **disciplined evolution**: learning that is visible, scoped, evidence-aware, reversible where possible, reviewed where consequential, and always accountable to the humans and systems it affects.
