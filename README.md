# Bicameral

**A governed, local-first foundation for AI-assisted software development.**

> **Project status: private consolidation staging.** This repository is being established as the prospective home of the Bicameral open-source product. It does **not** yet contain a runnable consolidated release. Architecture and contribution processes described below are targets, not claims of completed implementation. Do not make this repository public, migrate restricted sources, or publish artifacts without the required approvals.

Bicameral is being organized around a single coherent product boundary: a local runtime and dashboard, agent-facing MCP interfaces, provider integrations, and an optional cloud service. The goal is to make product behavior understandable, reproducible, and independently verifiable, without requiring a private governance control plane to build or use the core product.

## What belongs in Bicameral

The planned open-source distribution brings together these product components, subject to source-provenance, security, and publication review:

| Component | Intended responsibility |
| --- | --- |
| **Local runtime (Bot)** | Local daemon, gateway, user-facing dashboard, product state and authorization |
| **MCP** | Agent-facing protocol and transport interfaces |
| **Integrations** | Provider-neutral adapters that supply evidence and candidate data |
| **Cloud** | Optional hosted/relay functionality with documented consent, identity and tenancy boundaries, plus a reproducible self-host path |
| **Installer** | Installation, bootstrap and distribution surfaces |
| **Product governance** | Public contracts, policies, schemas, fixtures, ADRs and deterministic validators needed to verify product behavior |

**Authority boundary:** The local product remains responsible for canonical product state, admission and authorization. MCP and integrations provide typed evidence or candidate inputs; they do not silently turn those inputs into authoritative decisions. Hosted functionality must respect explicit consent and tenancy separation. Public contracts and tests must make these distinctions verifiable.

## Repository layout (proposed)

The component tree will be introduced only as cleared source and working build configurations become available. The paths below are **not yet populated**:

```text
apps/dashboard/          Product dashboard
crates/                  Local Rust runtime, gateway and protocol types
packages/mcp/            Agent-facing MCP transport
packages/integrations/   Provider-neutral integrations
services/cloud/          Optional cloud services and self-hosting assets
installer/               Installation and distribution
governance/              Product contracts, policies, schemas and fixtures
tools/governance/        Reusable deterministic product validators
tools/release/           Product release tooling
docs/                    Architecture, development, security and self-hosting
migration/               Public-safe source provenance and migration metadata
.github/                 Contribution and CI policy (once approved)
```

This is one product source repository, **not one mandatory process or service**. Components will keep explicit contracts, independently testable boundaries and traceable release artifacts. Toolchain workspaces and lockfiles will follow the actual dependency graph rather than being invented in advance.

## Current availability

**No supported installation, build, self-hosting, release, or public contribution workflow is available from this repository yet.** Existing work remains in its original repositories while the clean-source transition is evaluated. Instructions, compatibility statements and release links will be added after end-to-end verification, including applicable onboarding, Join, consent and recovery paths.

Do not infer production readiness from repository creation or documentation alone.

## Open-source transition and licensing

**Apache License 2.0 is the authorized outbound license for the consolidated Bicameral product**, including Bot, Cloud, MCP and Integrations. The product owner has confirmed that the necessary copyright-holder permissions are already secured. Apache-2.0 is a settled direction, not an outstanding licensing choice.

MCP and Integrations currently contain Business Source License text in their source repositories. Migration must apply the authorized Apache-2.0 license to the curated product sources, preserve applicable third-party notices and attribution, and document the approval and provenance in the migration record. This documentation step is **not a request to obtain copyright permissions again**. Previously published BSL releases are not retroactively changed by this README.

The Apache-2.0 LICENSE and notices will accompany the cleared consolidated source; this README alone does not replace source license files or authorize a public release.

Migration will use a **curated, auditable source snapshot**, not a wholesale merge of private Git histories. Sensitive incident records, customer data, secrets, internal compliance evidence and private operational configuration must not enter the public history. Source-to-destination provenance, exclusions and validation evidence must be tracked, with sensitive evidence retained privately.

### Explicit boundaries

Navigator, Atlas, FinOps, TARA, related Qortara/Cortera development-support products, and MythologIQ's Event Radar are **not Bicameral product modules**. They must not be copied into this repository, made a required runtime dependency, or granted product governance or release authority by implication. Any separately owned interface or source exception requires an explicit rights decision. Only reviewed, Bicameral-owned, public-safe Factory contracts and reusable validators may be extracted.

## Governance and contributions

The intended contributor model is transparent, maintainer-reviewed and security-conscious. Before inviting public contributions, the project will publish contributor guidance, disclosure instructions, governance and maintainer policies, a code of conduct, ownership rules, CI controls and a release policy.

The proposed controls include independent current-head review, stronger scrutiny for security/governance/release changes, deterministic contract checks and isolated untrusted-fork CI with no secrets or production privileges. Contributor reputation signals may inform human review but must not become an automatic eligibility or merge decision. DCO versus CLA and remaining policy decisions must be finalized by authorized owners.

**Public pull requests are not being solicited yet.** Security disclosures should not be posted as public issues; a private reporting channel will be documented before launch.

## Transition tracking

The authoritative transition plan, decision register, rights/security gates and acceptance criteria are tracked in:

- [Factory #969: Consolidate Bicameral into an auditable open-source product monorepo](https://github.com/BicameralAI/bicameral-factory/issues/969)
- [Executive recommendation and workstream details](https://github.com/BicameralAI/bicameral-factory/issues/969#issuecomment-6198207047)

Publication requires an auditable source inventory and record of the already-authorized license, verified exclusions, clean-history and secrets review, reproducible build/self-host checks, product release equivalence, safe contributor-workflow testing, and an **explicit exact-commit go/no-go** by the designated product, security and IP authorities. Until then, this repository remains private staging.

---

*This README describes the intended Bicameral product and the controls governing its formation. It is not a release announcement or permission to disclose restricted material.*
