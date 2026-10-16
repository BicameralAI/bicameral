"""Cloud project discovery route (issue #65, hosted-project-discovery-contract.md).

Lists the projects a Supabase-authenticated, design-partner-allowed user can
discover across their cloud teams. "Project" is the SPA-facing name for the
daemon-exposed workspace surface (cloud#64 contract; the `Project`/`workspace`
reconciliation RFQ cloud#75 is unresolved, so this follows the merged contract).

Authority boundary: Cloud renders coordination/projection metadata only. It
returns no provider secrets, workspace/file paths, source bodies, or local
daemon payloads (ADR-0006/0007), and never grants daemon workspace authority.
"""

from __future__ import annotations

from typing import Protocol

from fastapi import APIRouter, Depends

from bicameral_cloud.auth.dependencies import get_current_user
from bicameral_cloud.config import get_settings
from bicameral_cloud.models.discovery import (
    BindingReadiness,
    DiscoveryEligibility,
    HostedProjectPreview,
    ProductMembershipStatus,
    ProjectListItem,
    ProjectListResponse,
    ProjectPairingStatus,
)
from bicameral_cloud.models.product_directory import ProductDirectoryRecord
from bicameral_cloud.models.tenancy import AuthenticatedUser
from bicameral_cloud.product_directory_repository import (
    ProductDirectoryRepository,
    ProductDirectoryUnavailable,
    get_product_directory_repository,
)
from bicameral_cloud.project_discovery import LocalAlphaProjectAdapter
from bicameral_cloud.store import Store, get_store

router = APIRouter(prefix="/api/v1", tags=["projects"])


def _directory_item(record: ProductDirectoryRecord) -> ProjectListItem:
    """Project a durable directory Product into a safe discovery item.

    Discovered by verified email domain, so ``discovery_eligibility`` is
    ``domain`` and membership is explicitly ``not_member`` — Cloud never infers
    Product membership from domain eligibility (cloud#125). No daemon binding is
    known for a directory Product until a paired daemon exposes it, so pairing is
    ``unpaired`` and binding is ``unbound``. Only safe fields are surfaced;
    ``registration_ref`` and provider/substrate hints are never returned.
    """
    return ProjectListItem(
        project_id=record.canonical_product_id,
        team_id=None,
        display_name=record.display_name,
        discovery_eligibility=DiscoveryEligibility.DOMAIN,
        membership_status=ProductMembershipStatus.NOT_MEMBER,
        membership_role=None,
        pairing_status=ProjectPairingStatus.UNPAIRED,
        binding_readiness=BindingReadiness.UNBOUND,
        daemon_compatible=False,
        is_member=False,
        verified_domain=record.eligible_domain,
        preview=HostedProjectPreview.from_directory(
            product_id=record.canonical_product_id,
            display_name=record.display_name,
            verified_domain=record.eligible_domain,
            provider_hint=record.provider_hint,
            substrate_hint=record.substrate_hint,
        ),
    )


class ProjectDiscoverySource(Protocol):
    """Source of the discoverable Product list for a request."""

    def list_projects(self, user: AuthenticatedUser) -> list[ProjectListItem]:
        """Return the Products the user may discover, in a stable order."""
        ...


def _directory_products(
    directory: ProductDirectoryRepository, user: AuthenticatedUser
) -> list[ProjectListItem]:
    """Durable directory Products for the caller's verified email domain.

    Discovery is a read: an unavailable directory degrades to an empty list
    rather than failing the whole request.
    """
    domain = user.verified_email_domain
    if not domain:
        return []
    try:
        records = directory.list_for_domain(domain)
    except ProductDirectoryUnavailable:
        return []
    return [_directory_item(record) for record in records]


class DurableDirectoryProjectSource:
    """Production source: durable Supabase-backed Product directory only.

    Never touches ``LocalAlphaProjectAdapter`` / the in-memory ``Store`` so
    production composition does not depend on the in-memory adapter (cloud#125).
    """

    def __init__(self, directory: ProductDirectoryRepository) -> None:
        self._directory = directory

    def list_projects(self, user: AuthenticatedUser) -> list[ProjectListItem]:
        return _directory_products(self._directory, user)


class LocalAlphaProjectSource:
    """Test/development source over the in-memory ``Store``.

    Retained only behind explicit test/dev wiring (cloud#125): it unions the
    ``LocalAlphaProjectAdapter`` team/workspace projection with any durable
    directory Products so local dev stays coherent.
    """

    def __init__(
        self, store: Store, directory: ProductDirectoryRepository
    ) -> None:
        self._store = store
        self._directory = directory

    def list_projects(self, user: AuthenticatedUser) -> list[ProjectListItem]:
        projects = LocalAlphaProjectAdapter(self._store).list_projects_for_user(user)
        existing = {p.project_id for p in projects}
        projects.extend(
            item
            for item in _directory_products(self._directory, user)
            if item.project_id not in existing
        )
        return projects


def get_project_discovery_source(
    store: Store = Depends(get_store),  # noqa: B008
    directory: ProductDirectoryRepository = Depends(  # noqa: B008
        get_product_directory_repository
    ),
) -> ProjectDiscoverySource:
    """Select the discovery source: durable directory in production.

    When the durable directory is configured (production), discovery reads only
    the Supabase-backed directory. Otherwise the in-memory ``LocalAlphaProject``
    source is used for tests/local development, which may also be selected
    explicitly via dependency override (cloud#125 acceptance).
    """
    if get_settings().supabase_product_directory_enabled:
        return DurableDirectoryProjectSource(directory)
    return LocalAlphaProjectSource(store, directory)


@router.get(
    "/projects",
    response_model=ProjectListResponse,
    summary="List projects discoverable to the authenticated user",
)
async def list_projects(
    selected_project_id: str | None = None,
    user: AuthenticatedUser = Depends(get_current_user),  # noqa: B008
    source: ProjectDiscoverySource = Depends(  # noqa: B008
        get_project_discovery_source
    ),
) -> ProjectListResponse:
    """Return the deterministic project list for the authenticated user.

    Access requires a valid bearer token and the design-partner domain gate
    (enforced by ``get_current_user``): missing/invalid token -> 401, disallowed
    domain -> 403. Visibility is derived from Supabase-backed domain eligibility;
    an allowed-domain user discovers eligible Products immediately after sign-in
    with no join code, and every eligible user sees the same list (ADR-0007,
    cloud#95, cloud#125). ``discovery_eligibility`` is reported separately from
    canonical membership/readiness — domain eligibility never implies Product
    membership. ``selected_project_id`` is an optional SPA session hint echoed
    back only when it is discoverable.
    """
    projects = source.list_projects(user)

    resolved_selection: str | None = None
    if selected_project_id is not None and any(
        p.project_id == selected_project_id for p in projects
    ):
        resolved_selection = selected_project_id

    return ProjectListResponse(
        projects=projects,
        selected_project_id=resolved_selection,
    )
