"""Shared test fixtures for Bicameral Cloud tests."""

from __future__ import annotations

import pytest
from fastapi.testclient import TestClient

from bicameral_cloud.app import app
from bicameral_cloud.config import CloudSettings, reset_settings
from tests.proof_fixtures import PROOF_VERIFICATION_KEYS_JSON
from bicameral_cloud.models.discovery import WorkspaceDiscoveryItem, WorkspaceReadiness
from bicameral_cloud.models.pairing import (
    DaemonPairing,
    HostedSession,
    PairingStatus,
    SessionStatus,
)
from bicameral_cloud.models.tenancy import (
    AuthenticatedUser,
    TeamMembership,
    TeamRole,
)
from bicameral_cloud.product_directory_repository import (
    reset_inmemory_product_directory_repository,
)
from bicameral_cloud.store import reset_store

# --- Fixture constants ---
TEAM_ID = "team-alpha"
OTHER_TEAM_ID = "team-beta"
USER_ID = "user-1"
OTHER_USER_ID = "user-2"
OUTSIDER_USER_ID = "user-outsider"
TOKEN_MEMBER = "tok-member-1"
TOKEN_OWNER = "tok-owner-1"
TOKEN_OUTSIDER = "tok-outsider"
PAIRING_ID = "pairing-1"
SESSION_ID = "session-1"
DAEMON_VERSION = "1.2.0"


@pytest.fixture(autouse=True)
def _reset_settings():
    """Ensure Supabase auth is disabled unless a test opts in."""
    reset_settings(
        CloudSettings(
            supabase_url=None,
            supabase_jwt_secret=None,
            supabase_anon_key=None,
            supabase_service_key=None,
            product_creation_proof_verification_keys_json=PROOF_VERIFICATION_KEYS_JSON,
        )
    )


@pytest.fixture(autouse=True)
def _reset_product_directory():
    """Isolate the in-memory Product directory singleton between tests."""
    reset_inmemory_product_directory_repository()


@pytest.fixture(autouse=True)
def _pin_local_project_source():
    """Select the local (in-memory) project discovery source for the suite.

    Production composition uses the durable Supabase directory; the existing
    suite exercises the ``LocalAlphaProjectAdapter`` behaviour and so selects it
    explicitly (cloud#125: "unit tests can select the local adapter explicitly").
    Tests that need the durable path override this dependency themselves.
    """
    from bicameral_cloud.app import app
    from bicameral_cloud.product_directory_repository import (
        get_inmemory_product_directory_repository,
    )
    from bicameral_cloud.routes.projects import (
        LocalAlphaProjectSource,
        get_project_discovery_source,
    )
    from bicameral_cloud.store import get_store

    def _local() -> LocalAlphaProjectSource:
        return LocalAlphaProjectSource(
            get_store(), get_inmemory_product_directory_repository()
        )

    app.dependency_overrides[get_project_discovery_source] = _local
    yield
    app.dependency_overrides.pop(get_project_discovery_source, None)


@pytest.fixture()
def store():
    """Reset and return a fresh in-memory store."""
    return reset_store()


@pytest.fixture()
def client():
    """TestClient for the FastAPI app."""
    return TestClient(app)


@pytest.fixture()
def seeded_store(store):
    """Store pre-seeded with typical team/user/pairing/session/workspace data."""
    # Users
    member = AuthenticatedUser(
        user_id=USER_ID, email="member@example.com", display_name="Member"
    )
    owner = AuthenticatedUser(
        user_id=OTHER_USER_ID, email="owner@example.com", display_name="Owner"
    )
    outsider = AuthenticatedUser(
        user_id=OUTSIDER_USER_ID, email="outsider@example.com", display_name="Outsider"
    )
    store.add_user(TOKEN_MEMBER, member)
    store.add_user(TOKEN_OWNER, owner)
    store.add_user(TOKEN_OUTSIDER, outsider)

    # Team memberships
    store.add_membership(
        TeamMembership(user_id=USER_ID, team_id=TEAM_ID, role=TeamRole.MEMBER)
    )
    store.add_membership(
        TeamMembership(user_id=OTHER_USER_ID, team_id=TEAM_ID, role=TeamRole.OWNER)
    )

    # Daemon pairing
    store.add_pairing(
        DaemonPairing(
            pairing_id=PAIRING_ID,
            team_id=TEAM_ID,
            daemon_id="daemon-1",
            status=PairingStatus.ACTIVE,
            daemon_version=DAEMON_VERSION,
            created_by_user_id=USER_ID,
        )
    )

    # Hosted session
    store.add_session(
        HostedSession(
            session_id=SESSION_ID,
            user_id=USER_ID,
            team_id=TEAM_ID,
            pairing_id=PAIRING_ID,
            status=SessionStatus.OPEN,
            origin="https://dashboard.bicameral-ai.com",
            schema_version="1.0",
        )
    )

    # Team-visible workspaces (minimal discovery metadata only)
    store.set_workspaces(
        TEAM_ID,
        [
            WorkspaceDiscoveryItem(
                workspace_id="ws-1",
                display_name="Frontend App",
                pairing_status="active",
                readiness=WorkspaceReadiness.READY,
            ),
            WorkspaceDiscoveryItem(
                workspace_id="ws-2",
                display_name="Backend Service",
                pairing_status="active",
                readiness=WorkspaceReadiness.DEGRADED,
            ),
        ],
    )

    return store
