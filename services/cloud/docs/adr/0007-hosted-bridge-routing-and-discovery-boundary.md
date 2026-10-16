# Hosted Bridge Routing and Discovery Boundary

**Status:** accepted

**Related:** #9, #10, #11

Bicameral Cloud alpha will provide team-visible workspace discovery through paired daemons, but it will not perform per-user workspace filtering and it will not grant workspace action authority. Every member of a cloud team sees the same daemon-exposed workspace list for that team, with minimal discovery metadata by default. A daemon workspace owner must explicitly expose a workspace to a paired cloud team; pairing a daemon alone does not publish every local workspace.

Cloud discovery metadata is intentionally small: stable workspace id, display name, daemon or pairing status, and coarse readiness are acceptable by default. Branch names, file paths, repository remotes, provider names, issue titles, actor/member lists, and recent activity require explicit daemon exposure policy. Workspace actions remain daemon-authorized per workspace, and canonical local action, signoff, Decision Ledger, source-binding, and compliance-enforcement flows do not transit cloud, even encrypted.

Cloud bridge routing is hard allowlist and default-deny. Alpha cloud may route only explicitly allowed dashboard-intent command families such as projection reads, readiness refreshes, workspace discovery, preview requests, receipt fetches, and session close. Authority-bearing commands, including signoff, Decision Ledger writes, canonical governance events, source-binding mutations, and compliance-enforcement outcomes, must never be routed through cloud. Cloud validates origin, route, session binding, schema version, compatibility envelope, and size limits; the daemon authorizes routed intent semantics.

Alpha supports the current compatible protocol envelope plus one previous daemon minor version. Too-old daemons receive a blocking upgrade state, while newer daemons are usable only when capability negotiation proves the required action contract is compatible; otherwise mutable controls are disabled or blocked. Projection caching is deferred for alpha, so cloud does not persist dashboard projection snapshots beyond the operational records needed for sessions, pairing, compatibility, routing, and audit. Customer-owned domains are deferred for alpha; only explicit Bicameral-owned trusted origins may pair with real user daemons, with staging or developer origins limited to internal/developer mode and arbitrary preview origins blocked from pairing.

Governance gate: bic:spec-governance-gate advisory/no-op; Qor integration disabled pending factory policy revision.
