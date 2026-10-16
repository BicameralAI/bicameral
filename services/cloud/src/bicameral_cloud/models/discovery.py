"""Workspace discovery models (ADR-0007, issue #19).

Cloud discovery metadata is intentionally small: stable workspace id, display
name, daemon/pairing status, and coarse readiness. Branch names, file paths,
repository remotes, provider names, issue titles, actor/member lists, and
recent activity require explicit daemon exposure policy and are NOT included
in the default discovery response.
"""

from __future__ import annotations

import enum
import hashlib
from datetime import UTC, datetime, timedelta
from typing import cast

from pydantic import (
    BaseModel,
    SerializerFunctionWrapHandler,
    field_serializer,
    model_serializer,
)

from bicameral_cloud.models.tenancy import TeamRole


class WorkspaceReadiness(enum.StrEnum):
    """Coarse readiness indicator — not a compliance or signoff state."""

    READY = "ready"
    DEGRADED = "degraded"
    UNAVAILABLE = "unavailable"
    UNKNOWN = "unknown"


class WorkspaceDiscoveryItem(BaseModel):
    """Minimal discovery metadata for a team-visible workspace.

    Per ADR-0007, default discovery metadata is limited to:
    - stable workspace id
    - display name
    - daemon/pairing status
    - coarse readiness

    Forbidden in default response: file paths, branch names, repository
    remotes, provider secrets, SourceEvidence bodies, Decision Ledger event
    payloads, actor/member lists, issue titles, unreviewed authority fields.
    """

    workspace_id: str
    display_name: str
    pairing_status: str
    readiness: WorkspaceReadiness = WorkspaceReadiness.UNKNOWN


class WorkspaceDiscoveryResponse(BaseModel):
    """Response for team-visible workspace discovery."""

    team_id: str
    workspaces: list[WorkspaceDiscoveryItem]


class DiscoveryEligibility(enum.StrEnum):
    """Why a Product/project is discoverable to the requesting account.

    An eligibility/visibility axis kept explicitly separate from canonical
    Product membership (cloud#125). ``domain`` means the account's verified
    email domain matches the Product's eligible domain; ``membership`` means an
    explicit cloud team membership makes it discoverable. Domain eligibility is
    never read as membership.
    """

    DOMAIN = "domain"
    MEMBERSHIP = "membership"


class ProductMembershipStatus(enum.StrEnum):
    """Canonical Product membership status as known to Cloud.

    Cloud never owns canonical membership (ADR-0006). Durable directory Products
    discovered by domain are ``not_member`` from Cloud's perspective; team-scoped
    coordination items carry ``member``. Membership is never inferred from domain
    discovery eligibility.
    """

    MEMBER = "member"
    NOT_MEMBER = "not_member"


class ProjectPairingStatus(enum.StrEnum):
    """SPA-facing pairing status, mirroring dashboard pairing state.

    See CONTEXT.md dashboard pairing states. Cloud renders this so the SPA can
    decide whether to show a pairing/install CTA; it does not grant authority.
    """

    UNPAIRED = "unpaired"
    UNAVAILABLE = "unavailable"
    PAIRED = "paired"


class BindingReadiness(enum.StrEnum):
    """Coarse daemon binding readiness for a discoverable project.

    Composes team pairing status, the daemon compatibility envelope, and the
    daemon-reported workspace readiness (hosted-project-discovery-contract.md,
    cloud#64). It is NOT a compliance or signoff state. `ready`/`degraded`/
    `unknown` reuse WorkspaceReadiness; `upgrade_required` reuses the
    compatibility verdict; `unpaired` mirrors the dashboard pairing state;
    `unbound` marks a known-but-not-exposed/ready workspace.
    """

    UNPAIRED = "unpaired"
    UPGRADE_REQUIRED = "upgrade_required"
    UNBOUND = "unbound"
    DEGRADED = "degraded"
    READY = "ready"
    UNKNOWN = "unknown"


class HostedProjectEventStoreHint(BaseModel):
    """Safe non-secret routing labels for a Product preview."""

    substrate: str = "unknown"
    provider: str = "unknown"


class HostedProjectPreview(BaseModel):
    """Non-canonical preview consumed by the per-Product Join UI."""

    preview_id: str
    product_id: str
    product_display_name: str
    verified_domain: str
    event_store_hint: HostedProjectEventStoreHint
    canonical: bool = False
    expires_at: datetime

    @field_serializer("expires_at", when_used="json")
    def _serialize_expiry(self, value: datetime) -> str:
        return value.isoformat().replace("+00:00", "Z")

    @classmethod
    def from_directory(
        cls,
        *,
        product_id: str,
        display_name: str,
        verified_domain: str,
        provider_hint: str | None,
        substrate_hint: str | None,
    ) -> HostedProjectPreview:
        digest = hashlib.sha256(
            f"{product_id}\0{verified_domain}".encode()
        ).hexdigest()[:20]
        return cls(
            preview_id=f"preview_{digest}",
            product_id=product_id,
            product_display_name=display_name,
            verified_domain=verified_domain,
            event_store_hint=HostedProjectEventStoreHint(
                provider=provider_hint
                if provider_hint in {"github", "google"}
                else "unknown",
                substrate=substrate_hint
                if substrate_hint in {"git", "drive_folder"}
                else "unknown",
            ),
            expires_at=datetime.now(UTC).replace(microsecond=0)
            + timedelta(minutes=5),
        )


class ProjectListItem(BaseModel):
    """Safe SPA-facing metadata for a discoverable project.

    "Project" is the SPA-facing name for the daemon-exposed workspace surface
    (hosted-project-discovery-contract.md). Fields are a subset of the Cloud
    project record with FORBIDDEN_DISCOVERY_FIELDS excluded: no file/local
    paths, remotes, provider secrets, source bodies, or local daemon payloads.
    """

    project_id: str
    team_id: str | None
    display_name: str
    # Discovery eligibility is distinct from canonical membership/readiness
    # (cloud#125): it explains why the account can see the Product, not that it
    # is a member of it.
    discovery_eligibility: DiscoveryEligibility = DiscoveryEligibility.MEMBERSHIP
    membership_status: ProductMembershipStatus = ProductMembershipStatus.MEMBER
    # Cloud team role, when the item is a team-scoped coordination projection.
    # Absent for domain-discovered durable directory Products (no inferred role).
    membership_role: TeamRole | None = None
    pairing_status: ProjectPairingStatus
    binding_readiness: BindingReadiness = BindingReadiness.UNKNOWN
    daemon_compatible: bool = False
    is_member: bool | None = None
    verified_domain: str | None = None
    preview: HostedProjectPreview | None = None

    @model_serializer(mode="wrap")
    def _serialize_available_product_fields(
        self, handler: SerializerFunctionWrapHandler
    ) -> dict[str, object]:
        payload = cast(dict[str, object], handler(self))
        for field in ("is_member", "verified_domain", "preview"):
            if payload[field] is None:
                del payload[field]
        return payload


class ProjectListResponse(BaseModel):
    """Response for cross-team project discovery consumed by the SPA."""

    projects: list[ProjectListItem]
    selected_project_id: str | None = None


# Fields that must NEVER appear in default discovery metadata.
# Used by tests to verify no leakage.
FORBIDDEN_DISCOVERY_FIELDS = frozenset(
    {
        "file_path",
        "local_path",
        "filesystem_path",
        "branch_name",
        "repository_remote",
        "remote_url",
        "provider_secret",
        "provider_name",
        "source_evidence_body",
        "decision_ledger_payload",
        "decision_event",
        "actor_list",
        "member_list",
        "issue_title",
        "authority_field",
        "signoff_state",
        "compliance_state",
    }
)
