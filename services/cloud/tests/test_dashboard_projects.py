"""Regression guard: `/connect` must NOT render project discovery (cloud#118).

`/app` is the canonical project-discovery surface. The hosted `/connect`
shell keeps only signup/auth, design-partner gating, and the daemon pairing
handoff to `/app`; it must never re-grow a duplicate project list, create form,
join form, or project-discovery API client. These tests fail if any of that
duplicated surface reappears in the server-rendered shell.
"""

from __future__ import annotations

import pytest

# Markup ids / labels that only ever belonged to the removed inline panel.
FORBIDDEN_PROJECT_MARKUP = [
    'id="projects-panel"',
    'id="projects-status"',
    'id="projects-list"',
    'id="projects-empty"',
    'id="create-project-btn"',
    'id="join-project-btn"',
    'id="create-project-form"',
    'id="join-project-form"',
    'id="create-project-name"',
    'id="create-team-select"',
    'id="join-team-select"',
    'id="create-project-feedback"',
    'id="join-project-feedback"',
    ">Create Project<",
    ">Join Project<",
    "Bind to local workspace",
    "binding-badge",
]

# JS symbols / API clients that drove the removed inline project discovery.
FORBIDDEN_PROJECT_CLIENT = [
    "initProjectsPanel",
    "refreshProjects",
    "renderProjects",
    "submitCreateProject",
    "submitJoinProject",
    "queryDaemonBindingCapability",
    "populateTeamSelect",
    "deriveBindingUiState",
    "bindCtaAvailable",
    "/api/v1/projects",
    "/project-records",
    "/api/v1/auth/me/teams",
    "/v2/capabilities",
]


@pytest.mark.parametrize("needle", FORBIDDEN_PROJECT_MARKUP)
def test_dashboard_has_no_project_discovery_markup(client, store, needle):
    resp = client.get("/connect")

    assert resp.status_code == 200
    assert needle not in resp.text, (
        f"/connect must not render project-discovery markup: {needle!r} "
        "belongs to /app now (cloud#118)."
    )


@pytest.mark.parametrize("needle", FORBIDDEN_PROJECT_CLIENT)
def test_dashboard_has_no_project_discovery_client(client, store, needle):
    resp = client.get("/connect")

    assert resp.status_code == 200
    assert needle not in resp.text, (
        f"/connect must not embed a project-discovery client: {needle!r} "
        "belongs to /app now (cloud#118)."
    )


def test_dashboard_still_renders_pairing_handoff_to_app(client, store):
    """The active shell keeps pending-first pairing and hands off to `/app`."""
    resp = client.get("/connect")

    assert resp.status_code == 200
    assert 'id="experience"' in resp.text
    assert 'id="pairing-code"' in resp.text
    assert "beginPairing" in resp.text
    assert "pollPairing" in resp.text
    assert "/api/v1/hosted-bridge/pair/status" in resp.text
    # Successful approval hands the transient session to the canonical /app SPA.
    # The query value is navigation intent only: it opens existing Storage UI and
    # must not carry pairing/provider authority or secrets.
    assert "bicameral_hosted_bridge" in resp.text
    assert "window.location.href = '/app?onboarding=drive'" in resp.text
