# Maintainers and continuing contributors

This roster identifies Bicameral's decision owner, repository maintainers, and continuing engineering contributors. **Listing someone here does not automatically grant GitHub access or code-owner authority.**

| Name | GitHub | Responsibility | Status |
| --- | --- | --- | --- |
| Jin Kuan | [@jinhongkuan](https://github.com/jinhongkuan) | Product and governance decision owner | Confirmed |
| Kevin Knapp | [@Knapp-Kevin](https://github.com/Knapp-Kevin) | Lead Engineer & Technical Maintainer: technical leadership, architecture stewardship within approved scope, cross-component quality, CI/security engineering, OSS transition | Confirmed as technical lead under the approved role charter |
| Silong Tan | [@silongtan](https://github.com/silongtan) | Core Bicameral engineering; prospective component maintainer | Continuing team member; maintainer scope to confirm |
| Aritro Roy (WhatBytes) | [@officialaritro](https://github.com/officialaritro) | Bicameral engineering and open-source contribution; prospective component maintainer | Continuing six-month engagement communicated by Jin, October 15, 2026 |
| Mayur Sonar (WhatBytes) | [@Mayurss331](https://github.com/Mayurss331) | Bicameral engineering and open-source contribution; prospective component maintainer | Continuing six-month engagement communicated by Jin, October 15, 2026; repository privileges remain subject to security and least-privilege clearance |

Jin confirmed the continued engagement of Aritro and Mayur on October 15, 2026. The roster records participation without implying that either engineer has already been formally appointed a code owner or granted repository write permissions. Silong's GitHub identity is verified; his formal component-maintainer and code-ownership scope remain subject to governance approval.

## Component maintenance charter

A component maintainer is accountable for an explicitly assigned component, not merely granted a merge button. Assigned responsibilities include technical review, contract compatibility, tests, dependency and documentation hygiene, issue triage, regression/rollback response and security or rights escalation. Appointments require recorded scope, a designated backup where staffed, decision-owner approval and separately approved least-privilege permissions. Ownership assignments for Bot, MCP, Integrations, Cloud, Installer and product-owned tooling are **not inferred** from continuing-contributor status; until appointed, the existing CODEOWNERS remain unchanged. See [GOVERNANCE.md](GOVERNANCE.md) for the full role contract and limits.

Formal maintainer privileges and component ownership are subject to [GOVERNANCE.md](GOVERNANCE.md), the final maintainer/contributor policy, and approved repository access. The current default code owners remain `@jinhongkuan` and `@Knapp-Kevin` in [`governance/structure.json`](governance/structure.json), from which `.github/CODEOWNERS` is generated. These three identities have been verified against existing BicameralAI repository access. Do not add them to CODEOWNERS until formal code-ownership and access approvals are recorded.

## Organizational contributors

### WhatBytes

**Role:** Engineering contributor  
**Contribution:** Bicameral product engineering, maintenance, testing, and ongoing development.

WhatBytes has provided engineering resources to Bicameral, including Aritro Roy and Mayur Sonar. Jin confirmed their continued six-month engagement during the open-source transition in October 2026. Bicameral acknowledges WhatBytes' engineering contributions and ongoing involvement.

This acknowledgment does not create organizational code ownership, independent merge authority, licensing or publication authority, an exclusive partnership, or any exemption from security reviews and contribution policies.

Changes to formal maintainer authority are high-risk changes under GOVERNANCE.md and require decision-owner approval.
