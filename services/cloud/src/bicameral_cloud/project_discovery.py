"""Project discovery adapter (hosted-project-discovery-contract.md, cloud#64/#65).

"Project" is the SPA-facing name for the daemon-exposed workspace surface. This
module projects daemon-exposed workspaces into the SPA-facing project list,
gating visibility by domain/team eligibility (ADR-0007, cloud#95): a user
discovers Projects for every team they are eligible for — explicit cloud team
membership unioned with teams scoped to their verified Supabase email domain —
and every eligible user sees the same project list for a team (no per-user
filtering, no user-entered join code).

Persistence / source of truth (alpha): no Supabase product tables exist for a
project registry or project membership (verified against `supabase/migrations/`
in the merged contract). This adapter therefore serves the registry/membership
tier from the in-memory coordination `Store` (the documented ADR-0008 alpha
posture) and derives project membership from cloud team membership. It is
authoritative only for cloud coordination/tenancy visibility, never for local
project truth — project registration and workspace binding remain daemon-owned
(ADR-0005/0006). Swapping in the future Supabase-backed coordination DB only
requires replacing the `Store` reads below.
"""

from __future__ import annotations

from bicameral_cloud.models.compatibility import (
    CompatibilityVerdict,
    evaluate_compatibility,
)
from bicameral_cloud.models.discovery import (
    BindingReadiness,
    ProjectListItem,
    ProjectPairingStatus,
    WorkspaceReadiness,
)
from bicameral_cloud.models.pairing import DaemonPairing
from bicameral_cloud.models.tenancy import AuthenticatedUser, TeamRole
from bicameral_cloud.store import Store

# Daemon-reported workspace readiness -> coarse binding readiness, when the team
# has a compatible active pairing. `unavailable` maps to `unbound`: the daemon
# knows the workspace but has not exposed/bound it as ready.
_READINESS_TO_BINDING = {
    WorkspaceReadiness.READY: BindingReadiness.READY,
    WorkspaceReadiness.DEGRADED: BindingReadiness.DEGRADED,
    WorkspaceReadiness.UNAVAILABLE: BindingReadiness.UNBOUND,
    WorkspaceReadiness.UNKNOWN: BindingReadiness.UNKNOWN,
}


class LocalAlphaProjectAdapter:
    """Alpha project registry/membership adapter over the in-memory `Store`.

    Stands in for the future Supabase-backed cloud coordination DB (ADR-0006).
    """

    def __init__(self, store: Store) -> None:
        self._store = store

    def list_projects_for_user(
        self, user: AuthenticatedUser
    ) -> list[ProjectListItem]:
        """List every project discoverable to the user across eligible teams.

        Eligibility is Supabase-backed domain/team gating (cloud#95): explicit
        cloud team memberships unioned with teams scoped to the user's verified
        email domain. No user-entered join code is involved — an allowed-domain
        user discovers their eligible Projects immediately after sign-in.

        Deterministic ordering: sorted by (team_id, project_id) so repeated
        calls and every eligible user observe an identical list (ADR-0007).
        """
        items: list[ProjectListItem] = []
        for eligible in self._store.list_eligible_teams(
            user.user_id, user.verified_email_domain
        ):
            items.extend(self._projects_for_team(eligible.team_id, eligible.role))
        items.sort(key=lambda item: (item.team_id, item.project_id))
        return items

    def _projects_for_team(
        self, team_id: str, role: TeamRole
    ) -> list[ProjectListItem]:
        pairing = self._store.get_active_pairing_for_team(team_id)
        pairing_status, binding_default, daemon_compatible = _resolve_team_binding(
            pairing
        )
        projects: list[ProjectListItem] = []
        for workspace in self._store.get_workspaces(team_id):
            if binding_default is None:
                binding = _READINESS_TO_BINDING.get(
                    workspace.readiness, BindingReadiness.UNKNOWN
                )
            else:
                binding = binding_default
            projects.append(
                ProjectListItem(
                    project_id=workspace.workspace_id,
                    team_id=team_id,
                    display_name=workspace.display_name,
                    membership_role=role,
                    pairing_status=pairing_status,
                    binding_readiness=binding,
                    daemon_compatible=daemon_compatible,
                )
            )

        # Cloud-created Project coordination records (issue #88). These are
        # tenancy/coordination records with no daemon-reported readiness, so
        # they always project `unbound` local-workspace readiness until a
        # paired daemon exposes/binds them (ADR-0005/0007). The team pairing
        # status still surfaces so the SPA can render the correct pairing CTA.
        for record in self._store.list_projects_for_team(team_id):
            projects.append(
                ProjectListItem(
                    project_id=record.project_id,
                    team_id=team_id,
                    display_name=record.display_name,
                    membership_role=role,
                    pairing_status=pairing_status,
                    binding_readiness=BindingReadiness.UNBOUND,
                    daemon_compatible=daemon_compatible,
                )
            )
        return projects


def _resolve_team_binding(
    pairing: DaemonPairing | None,
) -> tuple[ProjectPairingStatus, BindingReadiness | None, bool]:
    """Resolve team-level pairing status and a forced binding, if any.

    Returns ``(pairing_status, forced_binding, daemon_compatible)``. When
    ``forced_binding`` is ``None`` the per-workspace readiness decides the
    binding readiness; otherwise every project in the team takes the forced
    value (no pairing -> ``unpaired``; incompatible daemon -> ``upgrade_required``).
    """
    if pairing is None:
        return ProjectPairingStatus.UNPAIRED, BindingReadiness.UNPAIRED, False

    compat = evaluate_compatibility(pairing.daemon_version, pairing.capabilities)
    if compat.verdict == CompatibilityVerdict.UPGRADE_REQUIRED:
        return (
            ProjectPairingStatus.PAIRED,
            BindingReadiness.UPGRADE_REQUIRED,
            False,
        )
    return ProjectPairingStatus.PAIRED, None, True
