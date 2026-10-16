"""Cloud tenancy models.

Cloud team roles (team_owner, team_member) are distinct from daemon workspace
roles and do not grant local action authority (ADR-0006).
"""

from __future__ import annotations

import enum
from datetime import UTC, datetime

from pydantic import BaseModel, Field, field_validator


def email_domain(email: str) -> str | None:
    """Return the lowercased domain part of an email, or ``None``.

    Shared by the design-partner allowlist (which verifies a domain is allowed)
    and domain-gated Project discovery (which ties an allowed identity to the
    teams scoped to its verified domain).
    """

    _, separator, domain = email.strip().lower().rpartition("@")
    if not separator or not domain:
        return None
    return domain


def _normalize_domain(value: str | None) -> str | None:
    if value is None:
        return None
    value = value.strip().lower()
    return value or None


class TeamRole(enum.StrEnum):
    """Cloud team role — not a daemon workspace role."""

    OWNER = "team_owner"
    MEMBER = "team_member"


class TeamEligibilitySource(enum.StrEnum):
    """Why a team is discoverable to a user (Project Discovery PRD, cloud#95).

    ``membership`` — an explicit cloud team-membership record.
    ``domain`` — the user's verified Supabase email domain matches the team's
    scoped domain, so they are eligible without an explicit membership row or a
    join code (server-side domain-gated eligibility).
    """

    MEMBERSHIP = "membership"
    DOMAIN = "domain"


class TeamMembership(BaseModel):
    """Cloud team membership record."""

    user_id: str
    team_id: str
    role: TeamRole
    joined_at: datetime = Field(default_factory=lambda: datetime.now(UTC))


class EligibleTeam(BaseModel):
    """A team a user may discover Projects in, with the eligibility source.

    Not persisted: derived per request from explicit memberships unioned with
    domain-scoped teams (cloud#95). It grants discovery/coordination visibility
    only — never daemon workspace authority (ADR-0006/0007).
    """

    team_id: str
    role: TeamRole
    source: TeamEligibilitySource


class AuthenticatedUser(BaseModel):
    """Authenticated cloud user identity."""

    user_id: str
    email: str
    display_name: str | None = None
    email_verified: bool = True

    @property
    def verified_email_domain(self) -> str | None:
        """Lowercased verified email domain, used for domain-gated eligibility."""

        if not self.email_verified:
            return None
        return email_domain(self.email)


class Team(BaseModel):
    """Cloud team record.

    ``domain`` is the verified email domain the team is scoped to. When set,
    any design-partner-allowed user whose verified Supabase email domain matches
    is eligible to discover the team's Projects without an explicit membership
    row or a join code (Project Discovery PRD, cloud#95). It is a coordination
    boundary only, never daemon/local workspace authority (ADR-0006).
    """

    team_id: str
    display_name: str
    created_by: str
    domain: str | None = None
    created_at: datetime = Field(default_factory=lambda: datetime.now(UTC))

    @field_validator("domain")
    @classmethod
    def _normalize_team_domain(cls, v: str | None) -> str | None:
        return _normalize_domain(v)


class CreateTeamRequest(BaseModel):
    """Request body for creating a new team."""

    display_name: str
    domain: str | None = None

    @field_validator("domain")
    @classmethod
    def _normalize_request_domain(cls, v: str | None) -> str | None:
        return _normalize_domain(v)


class AddMemberRequest(BaseModel):
    """Request body for adding a team member."""

    user_id: str
    role: TeamRole = TeamRole.MEMBER
