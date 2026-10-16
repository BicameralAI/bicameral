"""Browser evidence for truthful /connect onboarding states (cloud#353).

Each screen must reflect exactly one current state: setup guidance never sits
beside a pending pairing, a pending pairing names a real local approval
surface, and the page advances on its own when Bicameral becomes reachable.
Cloud may open the daemon's local approval page but never submits a decision.
"""

from __future__ import annotations

import json

import pytest

pytest.importorskip("playwright.sync_api")

from tests.journey.browser_identity import (  # noqa: E402
    DEFAULT_DAEMON_URL,
    install_browser_identity,
)
from tests.journey.conftest import HostedProjectCreationEnv  # noqa: E402

pytestmark = [pytest.mark.journey]

LINUX_UA = (
    "Mozilla/5.0 (X11; Ubuntu; Linux x86_64; rv:131.0) Gecko/20100101 Firefox/131.0"
)


def _new_page(browser, env: HostedProjectCreationEnv):
    context = browser.new_context(
        viewport={"width": 1280, "height": 900}, user_agent=LINUX_UA
    )
    page = context.new_page()
    install_browser_identity(
        page,
        access_token=env.access_token,
        supabase_url=env.supabase_url,
        anon_key=env.anon_key,
    )
    page.route(
        f"{env.base_url}/api/v1/auth/pairing-tenancy",
        lambda route: route.fulfill(
            status=200,
            content_type="application/json",
            body='{"team_id":"team-353","source":"existing"}',
        ),
    )
    return context, page


def _headers(origin: str) -> dict[str, str]:
    return {
        "access-control-allow-origin": origin,
        "access-control-allow-methods": "GET, POST, OPTIONS",
        "access-control-allow-headers": "content-type, x-csrf-token",
        "content-type": "application/json",
    }


class FakeDaemon:
    """Scriptable loopback daemon double for the hosted bridge pairing calls."""

    def __init__(self, origin: str, *, confirmation: dict | None = None) -> None:
        self.available = False
        self.headers = _headers(origin)
        self.confirmation = confirmation
        self.pair_posts = 0
        self.decision_calls = 0

    def route(self, route) -> None:
        request = route.request
        if "/pair/decision" in request.url:
            self.decision_calls += 1
        if not self.available:
            route.abort()
            return
        if request.method == "OPTIONS":
            route.fulfill(status=204, headers=self.headers, body="")
            return
        if request.method == "POST" and request.url.endswith("/pair"):
            self.pair_posts += 1
            body = {
                "status": "pending",
                "request_handle": "req-353",
                "poll_proof": "proof-353",
                "pairing_code": "K7Q2-M9XP",
            }
            if self.confirmation is not None:
                body["confirmation"] = self.confirmation
            route.fulfill(status=202, headers=self.headers, body=json.dumps(body))
            return
        route.fulfill(status=200, headers=self.headers, body='{"status":"pending"}')


def _assert_no_setup_guidance(page) -> None:
    assert page.locator("#setup-inline").is_hidden()
    assert page.locator("#platform").is_hidden()
    assert not page.get_by_text(
        "isn’t available on this device", exact=False
    ).is_visible()
    assert not page.get_by_text(
        "Run this in a terminal to install", exact=False
    ).is_visible()


def test_missing_daemon_advances_to_pending_without_any_click_and_drops_setup(
    browser, hosted_project_creation_env: HostedProjectCreationEnv
) -> None:
    env = hosted_project_creation_env
    context, page = _new_page(browser, env)
    daemon = FakeDaemon(env.base_url)
    page.route(f"{DEFAULT_DAEMON_URL}/**", daemon.route)

    page.goto(f"{env.base_url}/connect", wait_until="networkidle")
    page.get_by_text(
        "Bicameral isn’t available on this device yet.", exact=True
    ).wait_for(timeout=10_000)
    assert page.locator("#setup-inline").is_visible()

    # The user runs the installer in a terminal; nothing is clicked here.
    daemon.available = True
    page.get_by_text("Approve this browser in Bicameral.", exact=True).wait_for(
        timeout=15_000
    )

    _assert_no_setup_guidance(page)
    # Older daemon (no local page advertised): the exact terminal path + code.
    assert page.locator("#pairing-command").inner_text() == (
        "~/.local/bin/bicameral pairing approve"
    )
    assert page.get_by_text("K7Q2-M9XP", exact=True).is_visible()
    assert page.locator("#approval-local").is_hidden()
    assert daemon.pair_posts == 1
    assert daemon.decision_calls == 0
    context.close()


def test_pending_with_local_approval_page_offers_one_action_and_cli_fallback(
    browser, hosted_project_creation_env: HostedProjectCreationEnv
) -> None:
    env = hosted_project_creation_env
    context, page = _new_page(browser, env)
    daemon = FakeDaemon(
        env.base_url,
        confirmation={
            "local_confirmation_url": "/local/pairing?request=req-353",
            "terminal_command": "bicameral pairing approve",
        },
    )
    daemon.available = True
    page.route(f"{DEFAULT_DAEMON_URL}/**", daemon.route)

    page.goto(f"{env.base_url}/connect", wait_until="networkidle")
    page.get_by_text("Approve this browser in Bicameral.", exact=True).wait_for(
        timeout=10_000
    )

    link = page.get_by_role("link", name="Open approval page")
    assert link.is_visible()
    assert link.get_attribute("href") == (
        f"{DEFAULT_DAEMON_URL}/local/pairing?request=req-353"
    )
    assert "noopener" in (link.get_attribute("rel") or "")
    # Terminal path stays available but collapsed; it is not the primary action.
    assert not page.get_by_text("K7Q2-M9XP", exact=True).is_visible()
    page.get_by_text("Or approve from a terminal", exact=True).click()
    assert page.get_by_text("K7Q2-M9XP", exact=True).is_visible()
    assert page.locator("#developer-help").is_hidden()
    _assert_no_setup_guidance(page)
    assert daemon.decision_calls == 0
    context.close()


def test_foreign_local_confirmation_url_is_never_linked(
    browser, hosted_project_creation_env: HostedProjectCreationEnv
) -> None:
    env = hosted_project_creation_env
    context, page = _new_page(browser, env)
    daemon = FakeDaemon(
        env.base_url,
        confirmation={"local_confirmation_url": "https://evil.example/approve"},
    )
    daemon.available = True
    page.route(f"{DEFAULT_DAEMON_URL}/**", daemon.route)

    page.goto(f"{env.base_url}/connect", wait_until="networkidle")
    page.get_by_text("Approve this browser in Bicameral.", exact=True).wait_for(
        timeout=10_000
    )
    assert page.locator("#approval-local").is_hidden()
    assert page.get_by_text("K7Q2-M9XP", exact=True).is_visible()
    context.close()


def test_previously_seen_daemon_that_is_down_reads_as_stopped_not_missing(
    browser, hosted_project_creation_env: HostedProjectCreationEnv
) -> None:
    env = hosted_project_creation_env
    context, page = _new_page(browser, env)
    page.add_init_script("window.localStorage.setItem('bicameral_device_seen', '1');")
    page.route(f"{DEFAULT_DAEMON_URL}/**", lambda route: route.abort())

    page.goto(f"{env.base_url}/connect", wait_until="networkidle")
    page.get_by_text("Bicameral isn’t running on this device.", exact=True).wait_for(
        timeout=10_000
    )
    assert page.get_by_text("installed but isn’t running", exact=False).is_visible()
    assert not page.get_by_text(
        "isn’t available on this device", exact=False
    ).is_visible()
    context.close()


def test_daemon_stopping_mid_pairing_recovers_automatically(
    browser, hosted_project_creation_env: HostedProjectCreationEnv
) -> None:
    env = hosted_project_creation_env
    context, page = _new_page(browser, env)
    daemon = FakeDaemon(env.base_url)
    daemon.available = True
    page.route(f"{DEFAULT_DAEMON_URL}/**", daemon.route)

    page.goto(f"{env.base_url}/connect", wait_until="networkidle")
    page.get_by_text("Approve this browser in Bicameral.", exact=True).wait_for(
        timeout=10_000
    )

    daemon.available = False
    # A brief blip is tolerated; a sustained outage drops to stopped recovery.
    page.get_by_text("Bicameral stopped responding.", exact=True).wait_for(
        timeout=20_000
    )
    assert page.locator("#pairing").is_hidden()
    assert page.get_by_text("installed but isn’t running", exact=False).is_visible()

    daemon.available = True
    page.get_by_text("Approve this browser in Bicameral.", exact=True).wait_for(
        timeout=15_000
    )
    _assert_no_setup_guidance(page)
    assert daemon.pair_posts == 2
    assert daemon.decision_calls == 0
    context.close()


def test_probe_ok_but_pair_post_failing_stops_after_bounded_attempts(
    browser, hosted_project_creation_env: HostedProjectCreationEnv
) -> None:
    """OPTIONS answers but POST /pair keeps failing: no endless pair loop."""

    env = hosted_project_creation_env
    context, page = _new_page(browser, env)
    headers = _headers(env.base_url)
    state = {"posts": 0}

    def daemon_route(route) -> None:
        request = route.request
        if request.method == "OPTIONS":
            route.fulfill(status=204, headers=headers, body="")
            return
        if request.method == "POST" and request.url.endswith("/pair"):
            state["posts"] += 1
        route.abort()

    page.route(f"{DEFAULT_DAEMON_URL}/**", daemon_route)
    page.goto(f"{env.base_url}/connect", wait_until="networkidle")
    page.get_by_text(
        "Bicameral could not complete pairing.", exact=True
    ).wait_for(timeout=30_000)
    posts_at_stop = state["posts"]
    assert posts_at_stop == 3
    page.wait_for_timeout(5_000)
    assert state["posts"] == posts_at_stop
    context.close()
