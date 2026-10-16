# ADR-0031: External Assurance Provider Boundary and PrismPM Integration

**Date:** 2026-09-18

**Status:** accepted

**Level:** L1

**Related:** ADR-0002, ADR-0007, ADR-0016, ADR-0017, ADR-0019, ADR-0020,
ADR-0024, ADR-0030, `CONTEXT.md`,
https://github.com/UOR-Foundation/PrismPM,
https://github.com/UOR-Foundation/calculator-example

## Context

Bicameral distinguishes canonical Product authority from evidence, projections,
reasoning outputs, integration state, and other derived surfaces. The Decision
Ledger remains the replayable authority spine for accepted Decisions and
governance facts. External evidence may inform governance, but does not become
canonical authority merely because an external system labels it verified.

Some external systems can provide substantially stronger technical assurance
than ordinary source integrations. Examples include systems capable of:

- deterministic artifact reconstruction;
- exact artifact and release identity verification;
- provenance and supply-chain validation;
- schema and conformance checking;
- formal or bounded property verification;
- deployment-state comparison;
- observed-versus-declared drift evidence;
- reproducibility checks;
- runtime acceptance evidence; and
- explicit coverage and non-coverage reporting.

PrismPM is one such system. It treats an authoritative formal system model as
the source from which implementation artifacts, interfaces, deployment
projections, verification evidence, and release artifacts are derived. Its
Calculator reference application demonstrates the approach across application
behavior, API surfaces, persistence, observability, deployment, migration,
rollback, browser acceptance, provenance, and release lifecycle.

These capabilities overlap strongly with Bicameral's need for trustworthy
technical evidence, but PrismPM and Bicameral have different authority models.
PrismPM is model-first: a declared formal model is authoritative for the system
it produces. Bicameral is governance-first: observations and evidence enter
review and may eventually become governed Decisions through the Decision
Ledger.

Bicameral therefore must not adopt PrismPM's model as a second canonical
authority, nor should PrismPM become a required implementation dependency of
the Bicameral runtime.

The integration opportunity is narrower:

> An external assurance system may provide strong, subject-bound technical
> evidence to Bicameral while Bicameral retains sole authority over what that
> evidence means within Product governance.

## Decision

### 1. Bicameral supports an external assurance-provider boundary

Bicameral may integrate with external verification and assurance systems
through a provider-neutral boundary. The conceptual capability is
`ExternalAssuranceProvider`.

The provider performs a bounded technical evaluation against an exact subject
and returns structured evidence. PrismPM is the first candidate implementation
of this boundary.

The abstraction must not encode PrismPM-specific authority assumptions into
Bicameral's canonical domain model.

### 2. External assurance providers are evidence producers, not authority

An external assurance provider may produce:

- `SourceSnapshot`s;
- `EvidenceReference`s;
- technical verification results;
- provenance;
- coverage information;
- diagnostic evidence;
- candidate grounding material;
- input suitable for later `BindingEvidence` validation; and
- other explicitly typed derived evidence.

An external assurance provider must not directly:

- write the Decision Ledger;
- promote or demote Decisions;
- create authoritative Product governance facts;
- grant Product membership;
- modify pairing or identity authority;
- alter Product Owner authority;
- bypass `ToolRequest` or governance routing;
- declare Bicameral Decisions correct;
- turn provider conclusions into accepted `BindingEvidence` without Bicameral
  validation; or
- become canonical Product state.

Provider evidence enters the normal Bicameral governance lifecycle.

### 3. PrismPM is initially integrated, not linked as a core Rust dependency

The initial PrismPM implementation must be process-isolated from the Bicameral
core runtime. Bicameral consumes a documented PrismPM command/result surface
rather than linking PrismPM crates directly into the principal Bot dependency
graph.

A direct `prismpm` crate dependency inside Bicameral core is explicitly out of
scope for the initial integration.

The initial shape is:

```text
Bicameral
    |
    | bounded assurance request
    v
ExternalAssuranceProvider adapter
    |
    | exact provider invocation
    v
PrismPM
    |
    | structured result + native evidence
    v
Bicameral evidence admission
    |
    v
SourceSnapshot / EvidenceReference
    |
    v
normal review and governance
```

This preserves replaceability and prevents PrismPM's SDK, formal-language
toolchain, lifecycle machinery, or dependency model from becoming an implicit
Bicameral runtime requirement.

### 4. Every assurance request binds an exact subject

No provider result may be admitted as trustworthy technical evidence unless its
subject is unambiguous.

A request must identify the relevant subject using the strongest available
stable identity, such as:

- content digest;
- artifact digest;
- release digest;
- repository plus commit SHA;
- `GraphSnapshot`;
- immutable OCI reference;
- canonical file/content identity; or
- another equivalent immutable identifier.

Branch names, mutable tags, display names, or paths alone are insufficient
durable subject identity. A result for subject A must never be presented as
evidence for subject B merely because the two appear equivalent.

### 5. Provider identity and execution provenance are required

Every admitted provider result must identify:

- provider kind;
- provider version;
- provider configuration identity;
- exact subject identity;
- invocation or operation type;
- execution timestamp;
- result status;
- evidence digest or native attestation identity when available; and
- execution provenance sufficient to distinguish actual execution from caller
  assertion.

Where applicable, the result should additionally identify toolchain identity,
oracle identity, trust-root identity, environment or execution profile, and
dependency or SDK identity.

Bicameral must not convert an unprotected caller claim that a provider ran into
verified provider evidence.

### 6. Assurance semantics remain typed

Bicameral must not normalize all external results into a generic
`verified = true` state.

The integration contract must preserve meaningful distinctions such as:

- observed;
- reproduced;
- generated;
- schema-valid;
- conformance-passed;
- runtime-validated;
- provenance-validated;
- externally attested;
- formally established;
- accepted by a provider-specific lifecycle;
- unknown;
- inconclusive; and
- unavailable.

Provider-specific semantics may be retained in native evidence while Bicameral
exposes a smaller stable vocabulary for cross-provider use. Normalization must
never claim a stronger assurance level than the provider established.

### 7. Coverage and non-coverage are first-class evidence

Where a provider can report them, Bicameral must preserve:

- assertions covered by the evaluation;
- assertions explicitly not covered;
- conditions or assumptions required for the result; and
- partial or inconclusive scope.

A successful provider result must not be interpreted as universal proof about
the subject. For example, artifact integrity verification must not silently
become release authorization, production acceptance, security compliance, or
customer-journey correctness.

PrismPM's explicit distinction between covered and uncovered claims is the
preferred initial model for this behavior.

### 8. Provider failure fails informationally, not canonically

If an optional assurance provider is unavailable, times out, crashes, returns
malformed evidence, cannot evaluate the subject, returns an unsupported schema,
has an unknown version, or produces an identity mismatch, Bicameral records a
typed unavailable, invalid, or inconclusive result as appropriate.

Provider failure must not:

- fabricate a passing result;
- silently fall back to a weaker provider;
- write canonical state;
- automatically reverse an existing Decision; or
- imply that the underlying subject failed merely because the provider failed.

Likewise, provider success does not automatically establish a Bicameral
Decision.

### 9. PrismPM remains optional

PrismPM is not required to operate Bicameral. A Product that does not use
PrismPM retains the complete existing Bicameral governance lifecycle.

No Alpha installation, pairing, Decision promotion, Decision Ledger replay,
ordinary source ingestion, MCP use, or hosted Product journey may depend on
PrismPM. Initial PrismPM work is explicitly non-release-blocking.

### 10. Initial PrismPM integration is read-oriented

The first implementation may invoke PrismPM operations that inspect or verify
an exact subject and return evidence. Candidate operations include:

- `inspect`;
- `verify`;
- `verify-release`;
- `conformance`; and
- read-only status or equivalent bounded verification operations.

Initial Bicameral integration must not autonomously invoke PrismPM operations
that mutate external deployment state, including deploy, rollback, destroy,
`finalize-contract`, push, promotion, or other provider-side mutations.

Any future provider-mediated mutation requires a separate authority decision.

### 11. Provider-native evidence remains available

Bicameral may normalize external evidence for query, comparison, and governance
use, but must retain a reference to the original provider evidence or an
immutable digest thereof.

Normalization must not erase details necessary to determine what the provider
actually claimed, what version produced the result, what subject was evaluated,
and what the provider did not establish.

### 12. No provider becomes privileged by implementation accident

The assurance-provider contract must allow future implementations other than
PrismPM, including specialized supply-chain verifiers, formal verification
systems, policy engines, deployment verifiers, test/acceptance systems, or
compliance evidence systems.

A provider may support capabilities that another provider does not. Bicameral
must not infer semantic equivalence merely because two providers implement the
same integration interface.

The interface standardizes evidence transport and identity, not the truth
strength of every provider result.

## Initial evidence contract

The provider-neutral result should contain, conceptually:

```text
provider
provider_version
provider_profile_digest

subject_kind
subject_id

operation
assurance_kind
result

covered_assertions[]
uncovered_assertions[]
assumptions[]

input_identities[]
output_evidence_digest

execution_provenance
observed_at

native_schema
native_evidence_reference
```

Exact wire representation is an implementation follow-on and is not frozen by
this ADR.

The contract must preserve enough information for Bicameral to answer:

- What exactly was evaluated?
- Against which exact subject?
- Which provider and configuration performed the evaluation?
- What did it establish?
- What did it explicitly not establish?
- What evidence supports that result?

## PrismPM first slice

The first PrismPM integration is deliberately narrow. One bounded Bicameral
artifact or test fixture will be selected as the subject.

The proof-of-concept must demonstrate:

1. Bicameral identifies the exact immutable subject.
2. The PrismPM adapter invokes an exact pinned PrismPM implementation.
3. PrismPM produces native structured verification evidence.
4. Bicameral validates provider and result identity.
5. Native evidence is retained or content-addressed.
6. Bicameral creates derived evidence without granting canonical authority.
7. Covered and uncovered claims remain distinguishable.
8. A deliberately changed subject causes the evidence binding to fail.
9. Provider absence or failure produces a typed unavailable or inconclusive
   result.
10. No Decision Ledger state is written merely because verification ran.

The proof-of-concept is successful only if removing PrismPM leaves Bicameral's
canonical governance lifecycle intact.

## Security and trust boundary

PrismPM or another `ExternalAssuranceProvider` is an external technical evidence
producer. Its execution must not receive broader filesystem, credential,
network, or Product authority than required for the specific operation.

Initial integrations should prefer:

- immutable or read-only subject input;
- pinned provider identity;
- bounded execution;
- explicit timeout and output limits;
- no implicit credential inheritance;
- no ambient write access to canonical Bicameral state; and
- fail-closed evidence admission when identity or schema validation fails.

Provider output remains untrusted until Bicameral validates the integration
contract. A cryptographically valid provider attestation proves only what its
declared semantics establish.

## Consequences

### Positive

- Bicameral can consume high-quality external technical assurance without
  recreating specialized verification engines.
- PrismPM can contribute artifact, release, conformance, provenance,
  reproducibility, and deployment evidence while remaining outside Bicameral
  authority.
- The architecture preserves provider replaceability.
- Bicameral does not inherit PrismPM's full formal-model and SDK dependency
  graph.
- Products that do not use PrismPM remain unaffected.
- Assurance semantics become more precise than a generic verified/unverified
  flag.
- Explicit non-coverage reduces false certainty.
- Evidence can remain bound to exact releases, commits, artifacts, and
  `GraphSnapshot`s.
- The same abstraction can later support other assurance systems.

### Negative / accepted costs

- A process boundary requires adapter, schema, lifecycle, and diagnostics work.
- Provider-specific semantics cannot be completely flattened into one universal
  vocabulary.
- Bicameral must preserve native evidence where normalization is lossy.
- Operators may need an independently installed or provisioned assurance
  provider.
- Provider/version compatibility becomes an integration concern.
- Some potentially useful provider capabilities remain unavailable initially
  because mutation is deliberately excluded.

## Alternatives considered

### Direct PrismPM Rust dependency

Rejected for the initial integration.

Although both systems are Rust-based, language compatibility does not justify
coupling Bicameral core to PrismPM's model authority, formal verification stack,
SDK lifecycle, and dependency graph.

A direct library integration may be reconsidered only if it provides a material
benefit unavailable through the provider contract and receives a separate
architectural review.

### Make PrismPM a Bicameral release gate

Rejected. PrismPM is optional external assurance and must not become a
Bicameral release prerequisite merely because an integration exists.

Specific future release policies may choose to require specific assurance
evidence, but that policy decision is separate from this provider architecture.

### Treat PrismPM results as Decisions

Rejected. Technical verification evidence and governed organizational
Decisions are different authority classes.

### Build PrismPM capabilities natively in Bicameral

Rejected as the default approach. Bicameral should not duplicate a specialized
assurance engine merely to avoid an integration boundary.

Bicameral owns evidence admission and governance semantics. Providers own their
specialized technical evaluation.

## Non-goals

This ADR does not:

- adopt PrismPM's formal model as Bicameral's canonical model;
- require PrismPM for any Product;
- make PrismPM a direct Bot dependency;
- authorize PrismPM to write the Decision Ledger;
- make formal verification mandatory for Bicameral use;
- replace source ingestion;
- replace code grounding;
- replace `GraphSnapshot` semantics;
- replace acceptance testing;
- make provider results synonymous with compliance;
- permit autonomous provider-driven deployment;
- make PrismPM part of Alpha release composition; or
- define a universal scoring system for assurance evidence.

## Follow-ons

1. Define the minimal provider-neutral `ExternalAssuranceProvider` request and
   result contract.
2. Select one bounded artifact or fixture for the PrismPM proof-of-concept.
3. Implement a process-isolated PrismPM adapter.
4. Preserve PrismPM native evidence and map only stable cross-provider fields
   into Bicameral evidence.
5. Add negative tests for subject mismatch, malformed evidence, unknown provider
   version, provider failure, and overclaiming of coverage.
6. Demonstrate that assurance evidence cannot directly write canonical Product
   state.
7. Evaluate the proof-of-concept before adding any second PrismPM capability.
8. Consider deployment or mutation capabilities only under a separate ADR.
