# Maintainers and continuing contributors

This roster identifies Bicameral's decision owner, repository maintainers, and continuing engineering contributors. **Listing someone here does not automatically grant GitHub access or code-owner authority.**

## Maintainer and contributor roster

**Role** identifies the person's organizational function. **Scope and responsibilities** define the work they are accountable for. **Decision authority** describes what they may decide under the governance charter. **Repository privileges** describe recorded code-ownership eligibility, not a verified GitHub access grant; actual roles/rulesets are configured separately in GitHub. An assignment, a CODEOWNERS entry, and an active GitHub permission are distinct.

| Person / GitHub | Role | Scope and responsibilities | Decision authority | Repository privileges / status |
| --- | --- | --- | --- | --- |
| Jin Kuan ([@jinhongkuan](https://github.com/jinhongkuan)) | Product & Governance Decision Owner | Product direction; scope, policy, license, architecture and publication decisions; final escalation | Final product/governance authority under [GOVERNANCE.md](GOVERNANCE.md), subject to required review and security controls; not unilateral PR self-approval | Listed in generated default CODEOWNERS. Actual GitHub role/ruleset enforcement must be verified in settings. Decision-owner role confirmed |
| Kevin Knapp ([@Knapp-Kevin](https://github.com/Knapp-Kevin)) | Lead Engineer & Technical Maintainer | Technical leadership; cross-component architecture stewardship within accepted records; compatibility, quality, CI/security engineering, reviews and maintainer coordination | Technical review, implementation coordination and escalation within approved architecture; cannot independently change license, product scope, publication authority or approved architecture | Listed in generated default CODEOWNERS. Actual GitHub role/ruleset enforcement must be verified in settings. Technical lead charter confirmed |
| Silong Tan ([@silongtan](https://github.com/silongtan)) | Engineering Contributor; prospective Component Maintainer | Core Bicameral engineering, implementation, tests and repairs; specific ownership domain unassigned | May propose changes and review as permitted by actual access; **no appointed component ownership or independent release authority recorded** | Continuing team member; GitHub identity verified. Formal maintainer scope and permissions pending decision |
| Aritro Roy, WhatBytes ([@officialaritro](https://github.com/officialaritro)) | Engineering Contributor; prospective Component Maintainer | Product development, testing, maintenance and technical follow-through; specific ownership domain unassigned | Contribution and review within actually granted access; **no code-owner appointment, merge exemption or publication authority recorded** | Continued engagement communicated October 15, 2026; repository access and potential maintainer appointment must be verified independently |
| Mayur Sonar, WhatBytes ([@Mayurss331](https://github.com/Mayurss331)) | Engineering Contributor; prospective Component Maintainer | Product development, tests, maintenance and remediation; specific ownership domain unassigned | Contribution and review within actually granted access; **no code-owner appointment, merge exemption or publication authority recorded** | Continued engagement communicated October 15, 2026; repository privileges subject to explicit security/least-privilege clearance |

**Access evidence:** `governance/structure.json` generates `.github/CODEOWNERS` with Jin and Kevin as default owners. That file is **not proof** of either person's current GitHub permission level, nor of enforced branch protections. A GitHub administrator must independently check effective repository roles, teams, rulesets, and active code-owner review enforcement before marking access as verified.

Jin confirmed continued engagement of Aritro and Mayur in [the October 15 discussion](https://bicameralgroup.slack.com/archives/D0AUDB31NMQ/p1792080983544719). The roster records participation, not automatic repository access or code ownership. Silong's formal component-maintainer scope is also pending.

## Component maintenance charter

A component maintainer is accountable for an explicitly assigned component, not merely granted a merge button. Assigned responsibilities include technical review, contract compatibility, tests, dependency and documentation hygiene, issue triage, regression/rollback response and security or rights escalation. Appointments require recorded scope, a designated backup where staffed, decision-owner approval and separately approved least-privilege permissions. Ownership assignments for Bot, MCP, Integrations, Cloud, Installer and product-owned tooling are **not inferred** from continuing-contributor status; until appointed, the existing CODEOWNERS remain unchanged. See [GOVERNANCE.md](GOVERNANCE.md) for the full role contract and limits.

Formal maintainer privileges and component ownership are subject to [GOVERNANCE.md](GOVERNANCE.md), the final maintainer/contributor policy, and approved repository access. The current default code owners remain `@jinhongkuan` and `@Knapp-Kevin` in [`governance/structure.json`](governance/structure.json), from which `.github/CODEOWNERS` is generated. Do not add prospective component maintainers to CODEOWNERS until formal code-ownership and access approvals are recorded. Effective GitHub access for all participants requires separate verification in repository and organization settings.

## Organizational contributors

### WhatBytes

**Role:** Engineering contributor  
**Contribution:** Bicameral product engineering, maintenance, testing, and ongoing development.

WhatBytes has provided engineering resources to Bicameral, including Aritro Roy and Mayur Sonar. Jin confirmed their continued six-month engagement during the open-source transition in October 2026. Bicameral acknowledges WhatBytes' engineering contributions and ongoing involvement.

This acknowledgment does not create organizational code ownership, independent merge authority, licensing or publication authority, an exclusive partnership, or any exemption from security reviews and contribution policies.

Changes to formal maintainer authority are high-risk changes under GOVERNANCE.md and require decision-owner approval.
