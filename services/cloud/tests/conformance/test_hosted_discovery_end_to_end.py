"""Hosted discovery end-to-end conformance harness (issue #67).

Exercises the full hosted-discovery leaf of the design-partner flow entirely on
loopback with NO real Supabase credentials and NO Google OAuth:

    allowed design-partner user
        -> Cloud project list returned (Cloud/Supabase-sourced, not daemon-only)
        -> selected project passed into daemon pairing/projection
        -> production hosted auth enforced (no dev/local fallback)
        -> Cloud-served SPA build/version evidence present
        -> no secret / path / source leakage on any browser-facing surface

Fakes:
- ``fake_supabase.run_fake_supabase`` serves the ``design_partner_allowlist``
  table read; a local HS256 secret validates the minted browser token.
- ``fake_daemon.run_fake_daemon`` models the local daemon's hosted-bridge pair
  endpoint and echoes the Cloud-selected project into its projection.

Acceptance criteria coverage:
1. Passes without real Supabase credentials -> everything runs on loopback.
2. Fails if the SPA falls back to dev/local production mode -> asserts Supabase
   (production) auth is enforced for bogus tokens and that Cloud serves the
   vendored production SPA bundle with build/version evidence.
3. Fails if the project list is daemon-only -> asserts the Cloud project list is
   returned from cloud team membership even with no daemon pairing, and is
   disjoint from the live daemon projection.
"""

from __future__ import annotations

import json
import re
from collections.abc import Callable, Iterator

import pytest
from fastapi.testclient import TestClient

from bicameral_cloud.config import CloudSettings, reset_settings
from bicameral_cloud.models import discovery
from bicameral_cloud.models.discovery import WorkspaceDiscoveryItem, WorkspaceReadiness
from bicameral_cloud.models.pairing import DaemonPairing
from bicameral_cloud.models.tenancy import TeamMembership, TeamRole
from bicameral_cloud.store import Store, reset_store
from tests.conformance.bridge_client import attempt_pairing
from tests.conformance.fake_daemon import run_fake_daemon
from tests.conformance.fake_supabase import (
    FAKE_JWT_SECRET,
    mint_supabase_jwt,
    run_fake_supabase,
)

pytestmark = pytest.mark.conformance

ALLOWED_DESIGN_PARTNER_DOMAIN = "bicameral-ai.com"
CLOUD_ORIGIN = "https://app.bicameral-ai.com"
USER_ID = "user-123"
TEAM_ID = "team-cloud"
# Compatible with the alpha protocol window (compatibility.CURRENT_PROTOCOL_*).
COMPATIBLE_DAEMON_VERSION = "1.2.0"

# Cloud/Supabase-sourced project ids. Intentionally disjoint from the fake
# daemon's live projection workspace ids (`ws-alpha`/`ws-beta`) so a daemon-only
# implementation of /api/v1/projects would be detected.
CLOUD_PROJECT_IDS = {"ws-cloud-frontend", "ws-cloud-backend"}

# Tokens that must never appear in any Cloud/browser-facing surface.
FORBIDDEN_OUTPUT_TOKENS = set(discovery.FORBIDDEN_DISCOVERY_FIELDS) | {
    "provider_secret",
    "api_key",
    "refresh_token",
    "jwt_secret",
    "service_key",
    "supabase_service_key",
    "fake-supabase-role-token",
    "source_body",
    "source_content",
    "decision_ledger_payload",
    "event_store",
    "events.jsonl",
    "graph_index",
    ".bicameral/",
    "/home/",
    "local_path",
    "filesystem_path",
    "workspace_path",
}


def _contains_forbidden_token(surface: object, forbidden: set[str]) -> bool:
    text = (
        json.dumps(surface, sort_keys=True, default=str)
        if not isinstance(surface, str)
        else surface
    )
    lowered = text.lower()
    return any(token.lower() in lowered for token in forbidden)


def _seed_cloud_projects(store: Store, *, with_pairing: bool) -> None:
    """Seed a cloud team, membership, and Cloud project workspaces.

    When ``with_pairing`` is False no daemon pairing exists, so a Cloud-sourced
    project list must still be returned (unpaired) while a daemon-only list
    would be empty.
    """

    store.add_membership(
        TeamMembership(user_id=USER_ID, team_id=TEAM_ID, role=TeamRole.MEMBER)
    )
    store.set_workspaces(
        TEAM_ID,
        [
            WorkspaceDiscoveryItem(
                workspace_id="ws-cloud-frontend",
                display_name="Frontend App",
                pairing_status="paired",
                readiness=WorkspaceReadiness.READY,
            ),
            WorkspaceDiscoveryItem(
                workspace_id="ws-cloud-backend",
                display_name="Backend Service",
                pairing_status="paired",
                readiness=WorkspaceReadiness.DEGRADED,
            ),
        ],
    )
    if with_pairing:
        store.add_pairing(
            DaemonPairing(
                pairing_id="pairing-1",
                team_id=TEAM_ID,
                daemon_id="daemon-1",
                daemon_version=COMPATIBLE_DAEMON_VERSION,
            )
        )


@pytest.fixture()
def hosted_env() -> Iterator[tuple[TestClient, Store, Callable[[str], str]]]:
    store = reset_store()
    with run_fake_supabase({ALLOWED_DESIGN_PARTNER_DOMAIN}) as supabase_url:
        settings = reset_settings(
            CloudSettings(
                supabase_url=supabase_url,
                supabase_jwt_secret=FAKE_JWT_SECRET,
                supabase_anon_key="test-anon-key",
                supabase_service_key="fake-supabase-role-token",
            )
        )
        # Production hosted posture: Supabase auth + server-side allowlist.
        assert settings.supabase_auth_enabled
        assert settings.supabase_design_partner_allowlist_enabled

        from bicameral_cloud.app import app

        client = TestClient(app)

        def mint(email: str) -> str:
            return mint_supabase_jwt(email)

        yield client, store, mint


def _auth_headers(token: str) -> dict[str, str]:
    return {"Authorization": f"Bearer {token}"}


def test_allowed_partner_discovers_cloud_projects_and_selection_reaches_daemon(
    hosted_env: tuple[TestClient, Store, Callable[[str], str]],
) -> None:
    client, store, mint = hosted_env
    _seed_cloud_projects(store, with_pairing=True)
    token = mint(f"operator@{ALLOWED_DESIGN_PARTNER_DOMAIN}")

    # Allowed design-partner user authenticates.
    session = client.get("/api/v1/auth/session", headers=_auth_headers(token))
    assert session.status_code == 200
    assert session.json()["user_id"] == USER_ID

    # Cloud project list returned (Cloud/Supabase-sourced coordination metadata).
    projects_response = client.get("/api/v1/projects", headers=_auth_headers(token))
    assert projects_response.status_code == 200
    body = projects_response.json()
    projects = body["projects"]
    assert {p["project_id"] for p in projects} == CLOUD_PROJECT_IDS
    for project in projects:
        assert project["team_id"] == TEAM_ID
        assert project["membership_role"] == TeamRole.MEMBER.value
        assert project["pairing_status"] == "paired"
        assert project["daemon_compatible"] is True

    # Selecting a discoverable Cloud project echoes the selection back.
    selected = "ws-cloud-frontend"
    selected_response = client.get(
        "/api/v1/projects",
        params={"selected_project_id": selected},
        headers=_auth_headers(token),
    )
    assert selected_response.json()["selected_project_id"] == selected

    # A non-discoverable selection is not echoed (Cloud gates the selection).
    unknown_response = client.get(
        "/api/v1/projects",
        params={"selected_project_id": "ws-not-discoverable"},
        headers=_auth_headers(token),
    )
    assert unknown_response.json()["selected_project_id"] is None

    # Selected Cloud project is passed into daemon pairing/projection.
    with run_fake_daemon("paired") as daemon_base_url:
        outcome = attempt_pairing(
            daemon_base_url,
            origin=CLOUD_ORIGIN,
            cloud_account_id=USER_ID,
            selected_project_id=selected,
        )
    assert outcome.state == "paired"
    assert outcome.projection is not None
    assert outcome.projection["selected_project_id"] == selected
    assert "project_readiness" in outcome.projection

    # No secret/path/source leakage on any browser-facing surface.
    surfaces: list[object] = [
        client.get("/connect").text,
        client.get("/api/v1/auth/config").json(),
        session.json(),
        body,
        outcome.projection,
    ]
    assert not any(
        _contains_forbidden_token(surface, FORBIDDEN_OUTPUT_TOKENS)
        for surface in surfaces
    )


def test_non_allowlisted_domain_is_rejected(
    hosted_env: tuple[TestClient, Store, Callable[[str], str]],
) -> None:
    client, store, mint = hosted_env
    _seed_cloud_projects(store, with_pairing=True)
    token = mint("intruder@example.com")

    response = client.get("/api/v1/projects", headers=_auth_headers(token))
    assert response.status_code == 403


def test_project_list_is_cloud_sourced_not_daemon_only(
    hosted_env: tuple[TestClient, Store, Callable[[str], str]],
) -> None:
    client, store, mint = hosted_env
    # No daemon pairing at all: a daemon-only implementation would return [].
    _seed_cloud_projects(store, with_pairing=False)
    token = mint(f"operator@{ALLOWED_DESIGN_PARTNER_DOMAIN}")

    response = client.get("/api/v1/projects", headers=_auth_headers(token))
    assert response.status_code == 200
    projects = response.json()["projects"]

    # Cloud list is present despite no reachable/paired daemon, and carries
    # cloud coordination identity a daemon-only projection would not have.
    assert {p["project_id"] for p in projects} == CLOUD_PROJECT_IDS
    for project in projects:
        assert project["team_id"] == TEAM_ID
        assert project["membership_role"] == TeamRole.MEMBER.value
        assert project["pairing_status"] == "unpaired"
        assert project["binding_readiness"] == "unpaired"
        assert project["daemon_compatible"] is False

    # The Cloud project list is not the daemon's live projection: the ids are
    # disjoint from what the daemon reports when it is reachable.
    with run_fake_daemon("paired") as daemon_base_url:
        outcome = attempt_pairing(
            daemon_base_url,
            origin=CLOUD_ORIGIN,
            cloud_account_id=USER_ID,
        )
    assert outcome.projection is not None
    daemon_ids = {
        item["workspace_id"] for item in outcome.projection["project_readiness"]
    }
    assert CLOUD_PROJECT_IDS.isdisjoint(daemon_ids)


def test_no_dev_local_fallback_and_cloud_served_spa_build_evidence(
    hosted_env: tuple[TestClient, Store, Callable[[str], str]],
) -> None:
    client, store, mint = hosted_env
    _seed_cloud_projects(store, with_pairing=True)

    # Production hosted auth is enforced (no in-memory dev/local token bypass):
    # a non-Supabase bearer token is rejected rather than resolved from a store.
    config = client.get("/api/v1/auth/config").json()
    assert config["supabase_auth_enabled"] is True
    assert config["supabase_browser_auth_configured"] is True

    bogus = client.get(
        "/api/v1/projects", headers=_auth_headers("not-a-supabase-jwt")
    )
    assert bogus.status_code == 401

    # Build/version evidence for the Cloud-served SPA bundle.
    version_response = client.get("/app/version.json")
    assert version_response.status_code == 200
    version = version_response.json()
    assert version["source_repository"] == "BicameralAI/bicameral-bot"
    assert re.fullmatch(r"[0-9a-f]{64}", version["bundle_sha256"])
    assert version["bot_commit"]

    # Cloud serves the vendored production SPA entrypoint (not a dev server).
    app_response = client.get("/app")
    assert app_response.status_code == 200
    assert '<div id="app">' in app_response.text
    assert "/assets/index-" in app_response.text

    # Build/version + SPA surfaces leak no secrets/paths/sources.
    for surface in (version, app_response.text):
        assert not _contains_forbidden_token(surface, FORBIDDEN_OUTPUT_TOKENS)


def test_version_endpoint_is_json_not_spa_fallback(
    hosted_env: tuple[TestClient, Store, Callable[[str], str]],
) -> None:
    # Regression guard: /app/version.json must serve JSON metadata, never the
    # SPA index (which would mask a missing/failed Cloud build).
    client, _store, _mint = hosted_env
    response = client.get("/app/version.json")
    assert response.status_code == 200
    assert "application/json" in response.headers["content-type"]
    assert "<div id=\"app\">" not in response.text
