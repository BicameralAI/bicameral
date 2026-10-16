"""In-memory coordination store for alpha.

Provides the same interface that a Supabase-backed store would expose.
Cloud-owned records: users, teams, memberships, pairings, sessions,
workspace discovery metadata. Cloud must NOT store Decision Ledger entries,
provider credentials, source bindings, signoff state, or compliance
enforcement outcomes (ADR-0006).
"""

from __future__ import annotations

import uuid

from bicameral_cloud.models.audit import AuditEvent
from bicameral_cloud.models.discovery import WorkspaceDiscoveryItem
from bicameral_cloud.models.pairing import (
    DaemonPairing,
    HostedSession,
    PairingStatus,
    SessionStatus,
)
from bicameral_cloud.models.project import (
    ProjectCoordinationRecord,
    ProjectMembership,
)
from bicameral_cloud.models.registration import (
    ProjectRegistrationKind,
    ProjectRegistrationRequest,
    ProjectRegistrationStatus,
)
from bicameral_cloud.models.tenancy import (
    AuthenticatedUser,
    EligibleTeam,
    Team,
    TeamEligibilitySource,
    TeamMembership,
    TeamRole,
)


class Store:
    """In-memory store for cloud coordination records."""

    def __init__(self) -> None:
        self._users_by_token: dict[str, AuthenticatedUser] = {}
        self._users_by_id: dict[str, AuthenticatedUser] = {}
        self._teams: dict[str, Team] = {}
        self._memberships: dict[tuple[str, str], TeamMembership] = {}
        self._pairings: dict[str, DaemonPairing] = {}
        self._sessions: dict[str, HostedSession] = {}
        self._workspaces: dict[str, list[WorkspaceDiscoveryItem]] = {}
        self._registration_requests: dict[str, ProjectRegistrationRequest] = {}
        self._projects: dict[str, ProjectCoordinationRecord] = {}
        self._projects_by_join_code: dict[str, str] = {}
        self._project_members: dict[tuple[str, str], ProjectMembership] = {}
        self._audit_events: list[AuditEvent] = []
        # PairingAuthorizationV1 one-shot consume + Alpha TOFU (cloud#305).
        self._consumed_pairing_authorizations: set[str] = set()
        # Keyed by (cloud_account_id, team_id, daemon_instance_id) so wire-input
        # daemon_instance_id cannot collide across authenticated tenancy.
        self._daemon_tofu_roots: dict[tuple[str, str, str], str] = {}

    # --- user/token ---
    def add_user(self, token: str, user: AuthenticatedUser) -> None:
        self._users_by_token[token] = user
        self._users_by_id[user.user_id] = user

    def upsert_user(self, user: AuthenticatedUser) -> None:
        """Insert or update a user by user_id (used by Supabase auth path)."""
        self._users_by_id[user.user_id] = user

    def get_user_by_token(self, token: str) -> AuthenticatedUser | None:
        return self._users_by_token.get(token)

    def get_user_by_id(self, user_id: str) -> AuthenticatedUser | None:
        return self._users_by_id.get(user_id)

    # --- teams ---
    def create_team(
        self, display_name: str, created_by: str, domain: str | None = None
    ) -> Team:
        """Create a team and add the creator as owner.

        When ``domain`` is set, the team is scoped to that verified email
        domain and becomes discoverable to design-partner-allowed users sharing
        it (cloud#95 domain-gated eligibility).
        """
        team_id = f"team-{uuid.uuid4().hex[:12]}"
        team = Team(
            team_id=team_id,
            display_name=display_name,
            created_by=created_by,
            domain=domain,
        )
        self._teams[team_id] = team
        self.add_membership(
            TeamMembership(
                user_id=created_by,
                team_id=team_id,
                role=TeamRole.OWNER,
            )
        )
        return team

    def add_team(self, team: Team) -> None:
        """Register a pre-built team record (e.g. a domain-scoped team).

        Unlike ``create_team`` this does not synthesize an owner membership, so
        it can model a team discoverable purely by domain eligibility (cloud#95).
        """
        self._teams[team.team_id] = team

    def get_team(self, team_id: str) -> Team | None:
        return self._teams.get(team_id)

    # --- membership ---
    def add_membership(self, membership: TeamMembership) -> None:
        self._memberships[(membership.user_id, membership.team_id)] = membership

    def get_membership(self, user_id: str, team_id: str) -> TeamMembership | None:
        return self._memberships.get((user_id, team_id))

    def remove_membership(self, user_id: str, team_id: str) -> bool:
        """Remove a membership. Returns True if it existed."""
        key = (user_id, team_id)
        if key in self._memberships:
            del self._memberships[key]
            return True
        return False

    def list_team_members(self, team_id: str) -> list[TeamMembership]:
        return [m for m in self._memberships.values() if m.team_id == team_id]

    def list_user_teams(self, user_id: str) -> list[TeamMembership]:
        return [m for m in self._memberships.values() if m.user_id == user_id]

    def list_teams_for_domain(self, domain: str) -> list[Team]:
        """Return teams scoped to a verified email domain (cloud#95).

        Domain comparison is case-insensitive; the stored ``Team.domain`` is
        already normalized to lowercase.
        """
        normalized = domain.strip().lower()
        if not normalized:
            return []
        return [t for t in self._teams.values() if t.domain == normalized]

    def list_eligible_teams(
        self, user_id: str, email_domain: str | None
    ) -> list[EligibleTeam]:
        """Teams a user may discover Projects in (cloud#95 domain/team gating).

        The union of explicit team memberships and domain-scoped teams matching
        the user's verified email domain. Explicit membership wins on role when
        both apply. Deterministically ordered by ``team_id`` so repeated calls
        and different eligible users observe an identical list (ADR-0007).
        """
        eligible: dict[str, EligibleTeam] = {}
        for membership in self.list_user_teams(user_id):
            eligible[membership.team_id] = EligibleTeam(
                team_id=membership.team_id,
                role=membership.role,
                source=TeamEligibilitySource.MEMBERSHIP,
            )
        if email_domain:
            for team in self.list_teams_for_domain(email_domain):
                if team.team_id not in eligible:
                    eligible[team.team_id] = EligibleTeam(
                        team_id=team.team_id,
                        role=TeamRole.MEMBER,
                        source=TeamEligibilitySource.DOMAIN,
                    )
        return sorted(eligible.values(), key=lambda e: e.team_id)

    def resolve_pairing_tenancy(self, user: AuthenticatedUser) -> tuple[str, str]:
        """Return ``(team_id, source)`` for hosted pair initiation (bot#1290).

        Alpha invariant: one verified email domain maps to one Cloud team.
        Zero eligible teams → auto-create a domain-scoped team.
        More than one eligible team → fail closed (caller maps to HTTP 409).
        """

        eligible = self.list_eligible_teams(user.user_id, user.verified_email_domain)
        if len(eligible) == 1:
            return eligible[0].team_id, "existing"
        if len(eligible) > 1:
            raise ValueError("multiple_teams")
        domain = user.verified_email_domain
        if not domain:
            raise ValueError("missing_domain")
        existing = self.list_teams_for_domain(domain)
        if len(existing) == 1:
            return existing[0].team_id, "existing"
        if len(existing) > 1:
            raise ValueError("multiple_teams")
        team = self.create_team(
            display_name=domain,
            created_by=user.user_id,
            domain=domain,
        )
        return team.team_id, "created"

    # --- pairings ---
    def add_pairing(self, pairing: DaemonPairing) -> None:
        self._pairings[pairing.pairing_id] = pairing

    def get_pairing(self, pairing_id: str) -> DaemonPairing | None:
        return self._pairings.get(pairing_id)

    def get_active_pairing_for_team(self, team_id: str) -> DaemonPairing | None:
        for p in self._pairings.values():
            if p.team_id == team_id and p.status == PairingStatus.ACTIVE:
                return p
        return None

    def list_pairings_for_team(self, team_id: str) -> list[DaemonPairing]:
        return [p for p in self._pairings.values() if p.team_id == team_id]

    def revoke_pairing(self, pairing_id: str) -> DaemonPairing | None:
        pairing = self._pairings.get(pairing_id)
        if pairing is not None:
            pairing.status = PairingStatus.REVOKED
        return pairing

    def enroll_daemon_identity(
        self,
        pairing_id: str,
        *,
        user_id: str,
        identity_key_id: str,
        identity_public_key_b64: str,
    ) -> DaemonPairing | None:
        """Bind the daemon identity public key to this Cloud account (row 12).

        Alpha pairings live in this process-local store (ADR-0008). Enrollment
        has the same durability: it is not shared across Cloud Run instances and
        is erased on scale-to-zero. Multi-instance publication verify is not
        claimed until pairings persist.
        """
        pairing = self._pairings.get(pairing_id)
        if pairing is None or pairing.status != PairingStatus.ACTIVE:
            return None
        if pairing.enrolled_by_user_id and pairing.enrolled_by_user_id != user_id:
            return None
        pairing.enrolled_by_user_id = user_id
        pairing.identity_key_id = identity_key_id
        pairing.identity_public_key_b64 = identity_public_key_b64
        return pairing

    def identity_keys_for_user(self, user_id: str) -> list[DaemonPairing]:
        """Active pairings with an identity key enrolled for this caller."""
        return [
            pairing
            for pairing in self._pairings.values()
            if pairing.status == PairingStatus.ACTIVE
            and pairing.enrolled_by_user_id == user_id
            and pairing.identity_key_id
            and pairing.identity_public_key_b64
        ]

    def consume_pairing_authorization(
        self, *, pairing_id: str, nonce: str, digest_hex: str
    ) -> bool:
        """Mark authorization nonce/digest consumed for this pairing.

        Returns False if already consumed (replay). Process-local (ADR-0008).
        """
        keys = (
            f"nonce:{pairing_id}:{nonce}",
            f"digest:{pairing_id}:{digest_hex}",
        )
        if any(key in self._consumed_pairing_authorizations for key in keys):
            return False
        self._consumed_pairing_authorizations.update(keys)
        return True

    @staticmethod
    def daemon_tofu_root_key(
        *,
        cloud_account_id: str,
        team_id: str,
        daemon_instance_id: str,
    ) -> tuple[str, str, str]:
        """Tenancy-scoped Alpha TOFU lookup key (account + team + daemon)."""
        return (cloud_account_id, team_id, daemon_instance_id)

    def get_daemon_tofu_root(
        self,
        *,
        cloud_account_id: str,
        team_id: str,
        daemon_instance_id: str,
    ) -> str | None:
        return self._daemon_tofu_roots.get(
            self.daemon_tofu_root_key(
                cloud_account_id=cloud_account_id,
                team_id=team_id,
                daemon_instance_id=daemon_instance_id,
            )
        )

    def establish_daemon_tofu_root(
        self,
        *,
        cloud_account_id: str,
        team_id: str,
        daemon_instance_id: str,
        issuer_key_id: str,
    ) -> None:
        """Record Alpha TOFU root for a tenancy-scoped daemon instance."""
        key = self.daemon_tofu_root_key(
            cloud_account_id=cloud_account_id,
            team_id=team_id,
            daemon_instance_id=daemon_instance_id,
        )
        existing = self._daemon_tofu_roots.get(key)
        if existing is None:
            self._daemon_tofu_roots[key] = issuer_key_id
            return
        if existing != issuer_key_id:
            raise ValueError("daemon TOFU root already established")

    # --- sessions ---
    def add_session(self, session: HostedSession) -> None:
        self._sessions[session.session_id] = session

    def get_session(self, session_id: str) -> HostedSession | None:
        return self._sessions.get(session_id)

    def list_sessions_for_team(self, team_id: str) -> list[HostedSession]:
        return [s for s in self._sessions.values() if s.team_id == team_id]

    def close_session(self, session_id: str) -> HostedSession | None:
        session = self._sessions.get(session_id)
        if session is not None:
            session.status = SessionStatus.CLOSED
        return session

    # --- workspace discovery ---
    def set_workspaces(
        self, team_id: str, workspaces: list[WorkspaceDiscoveryItem]
    ) -> None:
        self._workspaces[team_id] = workspaces

    def get_workspaces(self, team_id: str) -> list[WorkspaceDiscoveryItem]:
        return self._workspaces.get(team_id, [])

    # --- project registration requests (create/join, issue #66) ---
    def add_registration_request(
        self, request: ProjectRegistrationRequest
    ) -> None:
        """Persist a create/join coordination request (alpha adapter)."""
        self._registration_requests[request.request_id] = request

    def get_registration_request(
        self, request_id: str
    ) -> ProjectRegistrationRequest | None:
        return self._registration_requests.get(request_id)

    def list_registration_requests_for_team(
        self,
        team_id: str,
        *,
        kind: ProjectRegistrationKind | None = None,
        status: ProjectRegistrationStatus | None = None,
    ) -> list[ProjectRegistrationRequest]:
        """Return team-scoped requests, deterministically ordered."""
        results = [
            r
            for r in self._registration_requests.values()
            if r.team_id == team_id
            and (kind is None or r.kind == kind)
            and (status is None or r.status == status)
        ]
        results.sort(key=lambda r: (r.created_at, r.request_id))
        return results

    def find_pending_registration_request(
        self,
        *,
        team_id: str,
        requested_by: str,
        kind: ProjectRegistrationKind,
    ) -> ProjectRegistrationRequest | None:
        """Find an existing pending request so create/join stay idempotent."""
        for request in self._registration_requests.values():
            if (
                request.team_id == team_id
                and request.requested_by == requested_by
                and request.kind == kind
                and request.status == ProjectRegistrationStatus.PENDING
            ):
                return request
        return None

    # --- project coordination records (create/join, issue #88) ---
    def add_project(self, project: ProjectCoordinationRecord) -> None:
        """Persist a Project coordination record and index its join code."""
        self._projects[project.project_id] = project
        self._projects_by_join_code[project.join_code] = project.project_id

    def get_project(self, project_id: str) -> ProjectCoordinationRecord | None:
        return self._projects.get(project_id)

    def get_project_by_join_code(
        self, join_code: str
    ) -> ProjectCoordinationRecord | None:
        project_id = self._projects_by_join_code.get(join_code)
        if project_id is None:
            return None
        return self._projects.get(project_id)

    def list_projects_for_team(
        self, team_id: str
    ) -> list[ProjectCoordinationRecord]:
        """Return team-scoped Project records, deterministically ordered."""
        results = [p for p in self._projects.values() if p.team_id == team_id]
        results.sort(key=lambda p: (p.created_at, p.project_id))
        return results

    def add_project_member(self, membership: ProjectMembership) -> None:
        self._project_members[(membership.project_id, membership.user_id)] = (
            membership
        )

    def is_project_member(self, project_id: str, user_id: str) -> bool:
        return (project_id, user_id) in self._project_members

    def list_project_members(
        self, project_id: str
    ) -> list[ProjectMembership]:
        return [
            m
            for m in self._project_members.values()
            if m.project_id == project_id
        ]

    # --- audit events ---
    def add_audit_event(self, event: AuditEvent) -> None:
        self._audit_events.append(event)

    def list_audit_events_for_team(
        self,
        team_id: str,
        *,
        session_id: str | None = None,
        action: str | None = None,
    ) -> list[AuditEvent]:
        """Return audit events scoped to team, optionally filtered."""
        results: list[AuditEvent] = []
        for ev in self._audit_events:
            if ev.team_id != team_id:
                continue
            if session_id is not None and ev.session_id != session_id:
                continue
            if action is not None and ev.action.value != action:
                continue
            results.append(ev)
        return results


_store: Store | None = None


def get_store() -> Store:
    """Singleton store accessor for FastAPI dependency injection."""
    global _store  # noqa: PLW0603
    if _store is None:
        _store = Store()
    return _store


def reset_store() -> Store:
    """Reset and return a fresh store (for testing)."""
    global _store  # noqa: PLW0603
    _store = Store()
    return _store
