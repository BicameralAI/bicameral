# Hosted Dashboard Shell With Local Daemon Authority

**Status:** accepted

Bicameral Cloud will host the dashboard shell, but it will not become the authority for local workspace state, action authorization, source binding, signoff, compliance enforcement, or accepted event materialization. The hosted dashboard may authenticate users, manage cloud tenancy, coordinate pairing, render local-daemon projections, and route user intent to the paired daemon; the local daemon remains authoritative for project registration, local actor mapping, provider secrets, workspace bindings, mutable dashboard actions, projection truth, and Decision Ledger acceptance. This keeps the product reachable from a browser while preserving the local-code-grounded authority boundary established for Bicameral's runtime.

**Considered Options**

- Cloud-hosted dashboard shell with local-daemon authority.
- Full cloud dashboard authority for workspace state and actions.
- Local-only dashboard with no hosted shell.

**Consequences**

Cloud implementation must negotiate daemon capabilities and compatible projection schemas before enabling mutable controls. Cloud auth, database, cache, custom-domain, token-broker, and version-skew decisions may be designed independently, but they must preserve this authority boundary unless a later ADR explicitly supersedes this one. Cloud-stored records are coordination, tenancy, compatibility, audit, or explicitly non-canonical cache records; they are not the source of truth for local workspace decisions.