# Cloud Auth, Tenancy, and Database Boundary

**Status:** accepted

**Related:** #12

Bicameral Cloud alpha will use Supabase Auth with Google login and Supabase Postgres for cloud-owned identity, tenancy, session, pairing, compatibility, and coordination-audit records. Cloud tenancy is real team tenancy from the start because team membership informs hosted workspace discovery, but cloud team roles are not daemon workspace roles and do not grant local action authority. Product data reads and writes go through the Bicameral Cloud API; browser clients may use Supabase for auth session handling but must not query or mutate product tables directly, and Supabase Row Level Security is not part of the alpha enforcement boundary.

Cloud-owned records include users, teams, memberships, hosted sessions, daemon pairings, compatibility records, and coordination/transport audit metadata. Cloud must not store or become authoritative for projects, workspaces, Decision Ledger entries, provider credentials, source bindings, signoff state, canonical projections, or local compliance-enforcement outcomes. Application-level authorization is the alpha enforcement point for cloud tenancy and session operations; database constraints and indexes may support it, but local workspace authority remains daemon-owned under ADR-0005.

**Consequences**

A cloud `team_owner` may manage team membership and daemon pairings, and a cloud `team_member` may discover team-visible hosted workspace surfaces. Neither role implies `workspace_owner` or `workspace_member` authority inside any daemon workspace. A team can contain multiple exposed workspaces with different daemon-owned owner/member sets, so cloud supplies authenticated user, team, and session identity while each daemon workspace remains responsible for resolving action authority.

Governance gate: bic:spec-governance-gate advisory/no-op; Qor integration disabled pending factory policy revision.
