"""APC1 browser evidence for pairing contract, restore safety, and recovery."""

from __future__ import annotations

import json
import re

import pytest

pytest.importorskip("playwright.sync_api")

from bicameral_cloud.routes.connect_local_accountability import (  # noqa: E402
    LOCAL_STATUS_PATH,
)
from tests.journey.browser_identity import (  # noqa: E402
    DEFAULT_DAEMON_URL,
    install_browser_identity,
)
from tests.journey.conftest import HostedProjectCreationEnv  # noqa: E402

pytestmark = [pytest.mark.journey]


def _new_page(browser, env: HostedProjectCreationEnv, *, user_agent: str | None = None):
    kwargs = {"viewport": {"width": 1280, "height": 900}}
    if user_agent:
        kwargs["user_agent"] = user_agent
    context = browser.new_context(**kwargs)
    page = context.new_page()
    install_browser_identity(
        page,
        access_token=env.access_token,
        supabase_url=env.supabase_url,
        anon_key=env.anon_key,
    )
    return context, page


def _cors_headers(origin: str) -> dict[str, str]:
    return {
        "access-control-allow-origin": origin,
        "access-control-allow-methods": "GET, POST, OPTIONS",
        "access-control-allow-headers": "content-type, x-csrf-token",
        "content-type": "application/json",
    }


def _route_tenancy(page, env: HostedProjectCreationEnv) -> None:
    page.route(
        f"{env.base_url}/api/v1/auth/pairing-tenancy",
        lambda route: route.fulfill(
            status=200,
            content_type="application/json",
            body='{"team_id":"team-apc1","source":"existing"}',
        ),
    )


def test_custom_daemon_pair_body_and_restore_record_are_behaviorally_safe(
    browser, hosted_project_creation_env: HostedProjectCreationEnv
) -> None:
    """Use configured daemon, preserve pair contract, stop on truthful device state."""

    env = hosted_project_creation_env
    context, page = _new_page(browser, env)
    override = "http://127.0.0.1:47474"
    page.add_init_script(
        f"window.localStorage.setItem('bicameral_daemon_url', {json.dumps(override)});"
    )
    _route_tenancy(page, env)

    state = {"pair_body": None, "default_requests": 0}
    headers = _cors_headers(env.base_url)

    def default_daemon_route(route) -> None:
        state["default_requests"] += 1
        route.abort()

    def override_daemon_route(route) -> None:
        request = route.request
        if request.method == "GET" and request.url.endswith(LOCAL_STATUS_PATH):
            route.fulfill(
                status=200,
                headers=headers,
                body=json.dumps(
                    {
                        "schema": "bicameral.local-status.v1",
                        "version": "0.1.35",
                        "os": "linux",
                        "arch": "x86_64",
                        "environment": "native",
                        "daemon_instance_id": "daemon-journey",
                        "workspace_configured": True,
                        "gateway_reachable": True,
                        "pending_pairings": 0,
                        "active_pairings": 0,
                        "providers": {"google": "disconnected"},
                    }
                ),
            )
            return
        if request.method == "OPTIONS":
            route.fulfill(status=204, headers=headers, body="")
            return
        if request.method == "POST" and request.url.endswith("/pair"):
            state["pair_body"] = json.loads(request.post_data or "{}")
            route.fulfill(
                status=202,
                headers=headers,
                body=(
                    '{"status":"pending","request_handle":"req-safe",'
                    '"poll_proof":"proof-safe","pairing_code":"SAFE-01"}'
                ),
            )
            return
        if request.method == "GET" and "/pair/status?" in request.url:
            route.fulfill(
                status=200,
                headers=headers,
                body=(
                    '{"status":"approved","session_token":"session-safe",'
                    '"expires_at":"2099-01-01T00:00:00Z"}'
                ),
            )
            return
        route.fulfill(status=404, headers=headers, body="{}")

    page.route(f"{DEFAULT_DAEMON_URL}/**", default_daemon_route)
    page.route(f"{override}/**", override_daemon_route)

    page.goto(f"{env.base_url}/connect", wait_until="networkidle")
    page.get_by_text("This device is connected", exact=True).wait_for(timeout=10_000)

    assert page.url == f"{env.base_url}/connect"
    assert state["default_requests"] == 0
    assert state["pair_body"] is not None
    assert set(state["pair_body"]) == {
        "origin",
        "cloud_account_id",
        "csrf_token",
        "team_id",
        "cloud_access_token",
        "client_protocol_version",
    }
    assert state["pair_body"]["team_id"] == "team-apc1"
    assert state["pair_body"]["cloud_access_token"] == env.access_token

    summary = page.locator("#device-summary")
    assert summary.is_visible()
    assert summary.get_by_text("Native", exact=True).is_visible()
    assert summary.get_by_text("Linux · x86_64", exact=True).is_visible()
    assert summary.get_by_text("0.1.35", exact=True).is_visible()
    assert summary.get_by_text("Running", exact=True).is_visible()
    assert summary.get_by_text("Configured", exact=True).is_visible()
    assert summary.get_by_text("Not connected yet", exact=True).is_visible()

    restore = page.evaluate(
        "JSON.parse(window.localStorage.getItem('bicameral_hosted_bridge_restore'))"
    )
    assert restore["team_id"] == "team-apc1"
    assert restore["daemon_url"] == override
    assert "cloud_access_token" not in restore

    continue_drive = page.get_by_role("button", name="Continue to Google Drive")
    assert continue_drive.is_visible()
    continue_drive.click()
    page.wait_for_url(re.compile(r".*/app(?:\?.*)?$"), timeout=10_000)

    context.close()


def test_account_mismatch_shows_matching_account_recovery(
    browser, hosted_project_creation_env: HostedProjectCreationEnv
) -> None:
    """Typed account mismatch produces the explicit matching-account recovery UI."""

    env = hosted_project_creation_env
    context, page = _new_page(browser, env)
    _route_tenancy(page, env)
    headers = _cors_headers(env.base_url)

    def daemon_route(route) -> None:
        request = route.request
        if request.method == "OPTIONS":
            route.fulfill(status=204, headers=headers, body="")
            return
        if request.method == "POST" and request.url.endswith("/pair"):
            route.fulfill(
                status=409,
                headers=headers,
                body='{"kind":"account_mismatch"}',
            )
            return
        route.fulfill(status=404, headers=headers, body="{}")

    page.route(f"{DEFAULT_DAEMON_URL}/**", daemon_route)
    page.goto(f"{env.base_url}/connect", wait_until="networkidle")

    page.get_by_text("Use the matching account.", exact=True).wait_for(timeout=10_000)
    assert page.get_by_role("button", name="Sign out").is_visible()

    context.close()


def test_windows_troubleshooting_uses_supported_gateway_command_only(
    browser, hosted_project_creation_env: HostedProjectCreationEnv
) -> None:
    """Windows recovery does not advertise Unix install or unsupported service commands."""

    env = hosted_project_creation_env
    context, page = _new_page(
        browser,
        env,
        user_agent=(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
            "AppleWebKit/537.36 Chrome/153.0.0.0 Safari/537.36"
        ),
    )
    page.route(f"{DEFAULT_DAEMON_URL}/**", lambda route: route.abort())

    page.goto(f"{env.base_url}/connect", wait_until="networkidle")
    page.get_by_text(
        "Bicameral isn’t available on this device yet.", exact=True
    ).wait_for(timeout=10_000)

    page.get_by_text("Developer / troubleshooting options", exact=True).click()
    troubleshooting = page.locator("#troubleshooting-commands")
    assert troubleshooting.get_by_text("bicameral.exe gateway start", exact=True).is_visible()
    assert troubleshooting.get_by_text("curl -fsSL", exact=False).count() == 0
    assert troubleshooting.get_by_text("bicameral service", exact=False).count() == 0

    context.close()
