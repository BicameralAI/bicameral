"""End-to-end Project discovery lifecycle conformance (issue #90).

This is the completion-validation leaf for the P0 hosted Project discovery
lifecycle. It proves the *split authority model* across the merged
implementation leaves — it does NOT reimplement any of them:

- Cloud/Supabase coordination create/join (cloud#88, ``routes/project_coordination``);
- safe discovery projection (cloud#65, ``routes/projects`` / ``project_discovery``);
- daemon-truthful workspace-binding capability (bicameral-bot#747);
- MCP capability-gated ``workspace.bind`` user path (bicameral-mcp#706);
- hosted dashboard readiness projection (cloud#89, ``dashboard_binding``).

Cross-repo boundary classification per bicameral-factory PR #165:

- **Runtime / semantic contracts** (asserted here): the Cloud coordination API,
  the ``GET /api/v1/projects`` discovery projection, the daemon ``GET
  /v2/capabilities`` handshake + ``workspace.bind`` behavior, and the
  ``dashboard_binding.derive_binding_ui_state`` projection. These are behavior
  contracts and are the conformance target.
- **Deployable-artifact delivery** (NOT a semantic contract): the vendored
  ``/app`` SPA bundle Cloud serves. Per PR #165 its build hash is delivery
  evidence, not a runtime/semantic conformance surface, and it has no JS test
  substrate in this repo. Conformance stays on the API/binding/projection
  contracts; SPA parity is out of scope (see ``test_spa_is_artifact_delivery``).

True cross-repo execution (a real bot daemon + real MCP process) is too heavy
for the Cloud suite, so the daemon/MCP runtime contract is modeled by the
focused loopback fixture ``fake_binding_daemon`` driven through the
``binding_client`` MCP user-path mirror. The remaining real cross-repo
integration command is recorded as explicit validation evidence:

    EXTERNAL_INTEGRATION_COMMAND (not run in this deterministic suite):
      bicameral-bot$ cargo test -p bicameral-runtime workspace_bind
      bicameral-mcp$ pytest tests -k workspace_bind
    Reason: exercises the real Rust daemon binding authority + MCP user path
    against a live loopback daemon; requires both sibling repos checked out and
    built, which is outside the Cloud test environment. The daemon/MCP runtime
    contract they enforce is modeled here by ``fake_binding_daemon`` +
    ``binding_client`` (capability handshake, capability-gated bind, fail-closed
    unsupported/unavailable).
"""

from __future__ import annotations

import json
from collections.abc import Callable, Iterator
from typing import cast

import pytest
from fastapi.testclient import TestClient

from bicameral_cloud.config import CloudSettings, reset_settings
from bicameral_cloud.dashboard_binding import (
    ProjectBindingUiState,
    bind_cta_available,
    derive_binding_ui_state,
)
from bicameral_cloud.models import discovery
from bicameral_cloud.models.discovery import (
    BindingReadiness,
    WorkspaceDiscoveryItem,
    WorkspaceReadiness,
)
from bicameral_cloud.models.pairing import DaemonPairing
from bicameral_cloud.models.tenancy import Team
from bicameral_cloud.store import Store, reset_store
from tests.conformance.binding_client import (
    attempt_workspace_bind,
    query_binding_capability,
)
from tests.conformance.fake_binding_daemon import run_fake_binding_daemon
from tests.conformance.fake_supabase import (
    FAKE_JWT_SECRET,
    mint_supabase_jwt,
    run_fake_supabase,
)

pytestmark = pytest.mark.conformance

ALLOWED_DESIGN_PARTNER_DOMAIN = "bicameral-ai.com"
# A second design-partner-allowed domain that is NOT the team's scoped domain,
# used to prove the tenancy boundary under domain/team gating (cloud#95): an
# allowed identity whose verified domain does not match a team gets no access.
OTHER_ALLOWED_DOMAIN = "other-partner.com"
TEAM_ID = "team-cloud"
CREATOR_ID = "user-creator"
JOINER_ID = "user-joiner"
COMPATIBLE_DAEMON_VERSION = "1.2.0"

# The user's local folder handed to the daemon during bind. It is a local path
# that must NEVER surface on any Cloud/browser-facing projection (redaction).
LOCAL_WORKSPACE_PATH = "/home/operator/code/frontend-app"

# Forbidden tokens spanning every field class the acceptance criteria enumerate:
# local path, provider secret, source graph, Decision Ledger payload, signoff,
# and compliance fields. Built from the shipped FORBIDDEN_DISCOVERY_FIELDS plus
# concrete secret/path/source/ledger literals a leak would surface.
FORBIDDEN_OUTPUT_TOKENS = set(discovery.FORBIDDEN_DISCOVERY_FIELDS) | {
    LOCAL_WORKSPACE_PATH,  # local path
    "/home/",
    ".bicameral/",
    "filesystem_path",
    "workspace_path",
    "provider_secret",  # provider secret
    "api_key",
    "refresh_token",
    "service_key",
    "supabase_service_key",
    "source_graph",  # source graph
    "graph_index",
    "source_body",
    "decision_ledger_payload",  # Decision Ledger payload
    "decision_event",
    "events.jsonl",
    "signoff_state",  # signoff
    "signoff",
    "compliance_state",  # compliance
    "compliance",
}


def _contains_forbidden_token(surface: object, forbidden: set[str]) -> bool:
    text = (
        json.dumps(surface, sort_keys=True, default=str)
        if not isinstance(surface, str)
        else surface
    )
    lowered = text.lower()
    return any(token.lower() in lowered for token in forbidden)


def _auth_headers(token: str) -> dict[str, str]:
    return {"Authorization": f"Bearer {token}"}


def _seed_team(store: Store, *, with_pairing: bool) -> None:
    """Seed a cloud team scoped to a verified email domain (cloud#95).

    Eligibility flows from the team's ``domain`` rather than pre-seeded
    membership rows: any design-partner-allowed user whose verified email
    domain matches discovers the team's Projects immediately after sign-in,
    with no invite code and no explicit membership (ADR-0007 + cloud#95).
    """

    store.add_team(
        Team(
            team_id=TEAM_ID,
            display_name="Cloud Team",
            created_by=CREATOR_ID,
            domain=ALLOWED_DESIGN_PARTNER_DOMAIN,
        )
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
def hosted_env() -> Iterator[tuple[TestClient, Store, Callable[[str, str], str]]]:
    store = reset_store()
    with run_fake_supabase(
        {ALLOWED_DESIGN_PARTNER_DOMAIN, OTHER_ALLOWED_DOMAIN}
    ) as supabase_url:
        settings = reset_settings(
            CloudSettings(
                supabase_url=supabase_url,
                supabase_jwt_secret=FAKE_JWT_SECRET,
                supabase_anon_key="test-anon-key",
                supabase_service_key="fake-supabase-role-token",
            )
        )
        assert settings.supabase_auth_enabled
        assert settings.supabase_design_partner_allowlist_enabled

        from bicameral_cloud.app import app

        client = TestClient(app)

        def mint(user_id: str, email: str) -> str:
            return mint_supabase_jwt(email, sub=user_id)

        yield client, store, mint


def _discover_projects(client: TestClient, token: str) -> list[dict[str, object]]:
    response = client.get("/api/v1/projects", headers=_auth_headers(token))
    assert response.status_code == 200
    return cast("list[dict[str, object]]", response.json()["projects"])


def test_lifecycle_create_discover_unbound_bind_discover_ready(
    hosted_env: tuple[TestClient, Store, Callable[[str, str], str]],
) -> None:
    """Runtime/semantic contract: create -> discover unbound -> bind -> ready.

    Walks the whole split-authority lifecycle on loopback with no real Supabase
    credentials and no live sibling daemon/MCP process.
    """

    client, store, mint = hosted_env
    _seed_team(store, with_pairing=True)
    # Two design-partner-allowed users on the team's verified domain. Neither
    # has an explicit membership row: eligibility comes from domain gating.
    creator = mint(CREATOR_ID, f"creator@{ALLOWED_DESIGN_PARTNER_DOMAIN}")
    joiner = mint(JOINER_ID, f"joiner@{ALLOWED_DESIGN_PARTNER_DOMAIN}")

    # 1. Signed-in hosted user creates a Project through Cloud/Supabase
    #    coordination (cloud#88). Access is domain/team eligibility (cloud#95),
    #    not a prior membership or a join code. Cloud coordinates tenancy only.
    create = client.post(
        f"/api/v1/teams/{TEAM_ID}/project-records",
        json={"display_name": "Frontend App"},
        headers=_auth_headers(creator),
    )
    assert create.status_code == 201
    created = create.json()
    project_id = created["project_id"]
    # A freshly created Project is unbound: Cloud is not the binding authority.
    assert created["binding_readiness"] == BindingReadiness.UNBOUND.value

    # 2. Domain/team discovery is the lifecycle entry point (cloud#95 AC#8): the
    #    created Project appears in safe discovery as unbound before any local
    #    binding, with no invite-code join step.
    projects = _discover_projects(client, creator)
    coordination_item = next(
        p for p in projects if p["project_id"] == project_id
    )
    assert coordination_item["team_id"] == TEAM_ID
    assert coordination_item["binding_readiness"] == BindingReadiness.UNBOUND.value
    # A second same-domain user discovers the identical Project immediately
    # after sign-in — no membership row and no join code (uniform domain/team
    # visibility, ADR-0007 + cloud#95 AC#1/#2).
    assert any(
        p["project_id"] == project_id for p in _discover_projects(client, joiner)
    )

    # 3. MCP/daemon path binds a local workspace when the capability is
    #    advertised (bicameral-bot#747 capability + bicameral-mcp#706 user path).
    with run_fake_binding_daemon("available") as daemon_url:
        capability = query_binding_capability(daemon_url)
        assert capability is True

        # Dashboard readiness projection: unbound + capability available offers
        # the Bind CTA that routes to the local MCP/daemon bind path (cloud#89).
        ui_state = derive_binding_ui_state(BindingReadiness.UNBOUND, capability)
        assert ui_state == ProjectBindingUiState.BINDING_AVAILABLE
        assert bind_cta_available(ui_state) is True

        outcome = attempt_workspace_bind(
            daemon_url,
            project_id=project_id,
            local_path=LOCAL_WORKSPACE_PATH,
        )
        assert outcome.bound is True
        assert outcome.readiness == "ready"
        assert outcome.fail_closed_reason is None

    # Cloud never persisted the binding: the daemon is the sole binding
    # authority (ADR-0005). The Cloud coordination record stays unbound.
    assert store.get_project(project_id) is not None
    post_bind = next(
        p
        for p in _discover_projects(client, creator)
        if p["project_id"] == project_id
    )
    assert post_bind["binding_readiness"] == BindingReadiness.UNBOUND.value

    # 4. The daemon now projects the bound workspace's readiness to Cloud
    #    (redacted, coarse readiness only). Discovery of that Project transitions
    #    to ready. Cloud stores no local binding truth — only the daemon-reported
    #    coarse readiness surfaces (ADR-0006/0007).
    store.set_workspaces(
        TEAM_ID,
        [
            WorkspaceDiscoveryItem(
                workspace_id=project_id,
                display_name="Frontend App",
                pairing_status="paired",
                readiness=WorkspaceReadiness.READY,
            )
        ],
    )
    ready_projects = _discover_projects(client, creator)
    assert any(
        p["project_id"] == project_id
        and p["binding_readiness"] == BindingReadiness.READY.value
        for p in ready_projects
    )

    # Dashboard readiness projection transitions to ready from the redacted
    # daemon readiness (cloud#89 derive_binding_ui_state contract).
    assert (
        derive_binding_ui_state(BindingReadiness.READY, True)
        == ProjectBindingUiState.READY
    )

    # 5. Forbidden fields never appear on any Cloud/browser-facing surface.
    surfaces: list[object] = [
        created,
        projects,
        ready_projects,
        outcome.projection,
        client.get("/connect").text,
    ]
    assert all(
        not _contains_forbidden_token(surface, FORBIDDEN_OUTPUT_TOKENS)
        for surface in surfaces
    )
    # The local path handed to the daemon never crossed the boundary.
    assert outcome.projection is not None
    assert LOCAL_WORKSPACE_PATH not in json.dumps(outcome.projection)


def test_repair_required_from_redacted_daemon_readiness(
    hosted_env: tuple[TestClient, Store, Callable[[str, str], str]],
) -> None:
    """Semantic contract: a degraded/upgrade-required daemon projects repair.

    The hosted dashboard projection transitions to repair_required from coarse,
    redacted daemon readiness — the other branch of acceptance criterion #4.
    """

    client, store, mint = hosted_env
    _seed_team(store, with_pairing=True)
    creator = mint(CREATOR_ID, f"creator@{ALLOWED_DESIGN_PARTNER_DOMAIN}")

    create = client.post(
        f"/api/v1/teams/{TEAM_ID}/project-records",
        json={"display_name": "Backend Service"},
        headers=_auth_headers(creator),
    )
    project_id = create.json()["project_id"]

    store.set_workspaces(
        TEAM_ID,
        [
            WorkspaceDiscoveryItem(
                workspace_id=project_id,
                display_name="Backend Service",
                pairing_status="paired",
                readiness=WorkspaceReadiness.DEGRADED,
            )
        ],
    )
    degraded = next(
        p
        for p in _discover_projects(client, creator)
        if p["project_id"] == project_id
        and p["binding_readiness"] == BindingReadiness.DEGRADED.value
    )
    assert (
        derive_binding_ui_state(
            BindingReadiness(str(degraded["binding_readiness"])), True
        )
        == ProjectBindingUiState.REPAIR_REQUIRED
    )


def test_unsupported_daemon_binding_fails_closed(
    hosted_env: tuple[TestClient, Store, Callable[[str, str], str]],
) -> None:
    """Runtime contract: a reachable daemon that does NOT advertise binding.

    The MCP user path must fail closed (never bind) and the dashboard must
    render the explicit binding_unsupported state with no Bind CTA — never a
    silent no-op (acceptance criterion #6, unsupported MCP/daemon state).
    """

    client, store, mint = hosted_env
    _seed_team(store, with_pairing=True)
    creator = mint(CREATOR_ID, f"creator@{ALLOWED_DESIGN_PARTNER_DOMAIN}")
    create = client.post(
        f"/api/v1/teams/{TEAM_ID}/project-records",
        json={"display_name": "Frontend App"},
        headers=_auth_headers(creator),
    )
    project_id = create.json()["project_id"]

    with run_fake_binding_daemon("unsupported") as daemon_url:
        assert query_binding_capability(daemon_url) is False
        outcome = attempt_workspace_bind(
            daemon_url,
            project_id=project_id,
            local_path=LOCAL_WORKSPACE_PATH,
        )

    assert outcome.bound is False
    assert outcome.fail_closed_reason == "binding_unsupported"
    assert outcome.projection is None

    ui_state = derive_binding_ui_state(BindingReadiness.UNBOUND, False)
    assert ui_state == ProjectBindingUiState.BINDING_UNSUPPORTED
    assert bind_cta_available(ui_state) is False


def test_unavailable_daemon_binding_fails_closed(
    hosted_env: tuple[TestClient, Store, Callable[[str, str], str]],
) -> None:
    """Runtime contract: an unreachable/erroring daemon fails closed.

    An unknown capability never promotes to bindable: the MCP path refuses to
    bind and the dashboard falls back to discoverable_unbound with no CTA
    (acceptance criterion #6, unavailable daemon state).
    """

    client, store, mint = hosted_env
    _seed_team(store, with_pairing=True)
    creator = mint(CREATOR_ID, f"creator@{ALLOWED_DESIGN_PARTNER_DOMAIN}")
    create = client.post(
        f"/api/v1/teams/{TEAM_ID}/project-records",
        json={"display_name": "Frontend App"},
        headers=_auth_headers(creator),
    )
    project_id = create.json()["project_id"]

    # Capability query returns 500 -> treated as unknown (fail closed).
    with run_fake_binding_daemon("error") as daemon_url:
        assert query_binding_capability(daemon_url) is None
        errored = attempt_workspace_bind(
            daemon_url, project_id=project_id, local_path=LOCAL_WORKSPACE_PATH
        )
    assert errored.bound is False
    assert errored.fail_closed_reason == "capability_unavailable"

    ui_state = derive_binding_ui_state(BindingReadiness.UNBOUND, None)
    assert ui_state == ProjectBindingUiState.DISCOVERABLE_UNBOUND
    assert bind_cta_available(ui_state) is False


def test_domain_mismatch_cannot_create_join_or_discover(
    hosted_env: tuple[TestClient, Store, Callable[[str, str], str]],
) -> None:
    """Semantic contract: coordination is gated by domain/team eligibility.

    Under cloud#95 the tenancy boundary is the team's verified domain, not a
    pre-seeded membership. A design-partner-allowed user whose verified domain
    does NOT match the team is rejected before any create/join work runs and
    discovers no Project for that team (ADR-0007), so lifecycle access fails
    closed at the domain boundary — while a matching-domain user is eligible.
    """

    client, store, mint = hosted_env
    _seed_team(store, with_pairing=True)
    creator = mint(CREATOR_ID, f"creator@{ALLOWED_DESIGN_PARTNER_DOMAIN}")
    created = client.post(
        f"/api/v1/teams/{TEAM_ID}/project-records",
        json={"display_name": "Frontend App"},
        headers=_auth_headers(creator),
    ).json()

    # Allowed design-partner identity, but on a different verified domain than
    # the team is scoped to: eligible to sign in, not eligible for this team.
    other_domain_user = mint("user-outsider", f"outsider@{OTHER_ALLOWED_DOMAIN}")
    create_denied = client.post(
        f"/api/v1/teams/{TEAM_ID}/project-records",
        json={"display_name": "Sneaky"},
        headers=_auth_headers(other_domain_user),
    )
    assert create_denied.status_code == 403

    join_denied = client.post(
        f"/api/v1/teams/{TEAM_ID}/project-records/{created['project_id']}/join",
        json={"join_code": created["join_code"]},
        headers=_auth_headers(other_domain_user),
    )
    assert join_denied.status_code == 403

    # No tenant/project data leaks to the domain-mismatched user via discovery.
    assert _discover_projects(client, other_domain_user) == []


def test_legacy_join_code_path_is_deprecated_not_default(
    hosted_env: tuple[TestClient, Store, Callable[[str, str], str]],
) -> None:
    """Invite-code join is retained only as legacy manual-invite compat.

    cloud#95 AC#6/#8: domain/team discovery already surfaces the Project to an
    eligible user with no code, so the retained join-by-code endpoint is not the
    default path. It still works for manual invites but is marked deprecated
    (``Deprecation: true`` header) and is not the discovery entry point.
    """

    client, store, mint = hosted_env
    _seed_team(store, with_pairing=True)
    creator = mint(CREATOR_ID, f"creator@{ALLOWED_DESIGN_PARTNER_DOMAIN}")
    joiner = mint(JOINER_ID, f"joiner@{ALLOWED_DESIGN_PARTNER_DOMAIN}")

    created = client.post(
        f"/api/v1/teams/{TEAM_ID}/project-records",
        json={"display_name": "Frontend App"},
        headers=_auth_headers(creator),
    ).json()
    project_id = created["project_id"]

    # The joiner discovers the Project by domain BEFORE ever seeing a join code.
    assert any(
        p["project_id"] == project_id
        for p in _discover_projects(client, joiner)
    )

    # The legacy join-by-code endpoint still succeeds but is marked deprecated.
    legacy = client.post(
        f"/api/v1/teams/{TEAM_ID}/project-records/{project_id}/join",
        json={"join_code": created["join_code"]},
        headers=_auth_headers(joiner),
    )
    assert legacy.status_code == 200
    assert legacy.headers.get("Deprecation") == "true"


def test_forbidden_fields_redacted_across_lifecycle_surfaces(
    hosted_env: tuple[TestClient, Store, Callable[[str, str], str]],
) -> None:
    """Redaction invariant across every browser-facing lifecycle surface.

    Asserts the six enumerated field classes — local path, provider secret,
    source graph, Decision Ledger payload, signoff, compliance — never appear
    on the coordination create/join responses, the discovery projection, the
    dashboard shell, or the daemon bind projection.
    """

    client, store, mint = hosted_env
    _seed_team(store, with_pairing=True)
    creator = mint(CREATOR_ID, f"creator@{ALLOWED_DESIGN_PARTNER_DOMAIN}")

    created = client.post(
        f"/api/v1/teams/{TEAM_ID}/project-records",
        json={"display_name": "Frontend App"},
        headers=_auth_headers(creator),
    ).json()
    project_id = created["project_id"]

    # Daemon exposes readiness; bind returns a safe projection.
    with run_fake_binding_daemon("available") as daemon_url:
        outcome = attempt_workspace_bind(
            daemon_url, project_id=project_id, local_path=LOCAL_WORKSPACE_PATH
        )
    store.set_workspaces(
        TEAM_ID,
        [
            WorkspaceDiscoveryItem(
                workspace_id=project_id,
                display_name="Frontend App",
                pairing_status="paired",
                readiness=WorkspaceReadiness.READY,
            )
        ],
    )

    surfaces: list[object] = [
        created,
        _discover_projects(client, creator),
        outcome.projection,
        client.get("/connect").text,
        client.get("/api/v1/auth/config").json(),
    ]
    for surface in surfaces:
        assert not _contains_forbidden_token(surface, FORBIDDEN_OUTPUT_TOKENS)

    # Every enumerated forbidden field class is present in the guard set, so the
    # assertion above meaningfully covers all six.
    for token in (
        "local_path",
        "provider_secret",
        "source_graph",
        "decision_ledger_payload",
        "signoff_state",
        "compliance_state",
    ):
        assert token in FORBIDDEN_OUTPUT_TOKENS


def test_spa_is_artifact_delivery_not_semantic_contract(
    hosted_env: tuple[TestClient, Store, Callable[[str, str], str]],
) -> None:
    """Deployable-artifact delivery classification (PR #165), NOT a contract.

    The vendored ``/app`` SPA bundle is delivery evidence: Cloud serves it with
    a build hash, but its behavior is not a runtime/semantic conformance surface
    (no JS test substrate in this repo). Conformance for the lifecycle stays on
    the API/binding/projection contracts exercised by the other tests; here we
    only assert the artifact is delivered and leaks nothing.
    """

    client, _store, _mint = hosted_env

    version = client.get("/app/version.json")
    assert version.status_code == 200
    body = version.json()
    # Delivery evidence only: the bundle came from the bot artifact pipeline.
    assert body["source_repository"] == "BicameralAI/bicameral-bot"
    assert body["bundle_sha256"]

    app_response = client.get("/app")
    assert app_response.status_code == 200
    assert '<div id="app">' in app_response.text

    for surface in (body, app_response.text):
        assert not _contains_forbidden_token(surface, FORBIDDEN_OUTPUT_TOKENS)
