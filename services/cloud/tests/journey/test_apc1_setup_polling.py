"""APC1 browser evidence for guided setup and bounded daemon polling."""

from __future__ import annotations

import time

import pytest

pytest.importorskip("playwright.sync_api")

from bicameral_cloud.routes.connect_local_accountability import LOCAL_STATUS_PATH  # noqa: E402
from tests.journey.browser_identity import (  # noqa: E402
    DEFAULT_DAEMON_URL,
    install_browser_identity,
)
from tests.journey.conftest import HostedProjectCreationEnv  # noqa: E402

pytestmark = [pytest.mark.journey]


def _new_page(browser, env: HostedProjectCreationEnv):
    context = browser.new_context(viewport={"width": 1280, "height": 900})
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


def _start_setup_without_navigation(page) -> None:
    page.locator("#install").evaluate(
        """el => {
          el.addEventListener('click', event => event.preventDefault(), {
            once: true,
            capture: true,
          });
          el.click();
        }"""
    )


def _legacy_status_absent(route, headers: dict[str, str]) -> bool:
    request = route.request
    if request.method == "GET" and request.url.endswith(LOCAL_STATUS_PATH):
        route.fulfill(status=404, headers=headers, body="{}")
        return True
    return False


def test_prompt_without_answer_still_offers_normal_setup(
    browser, hosted_project_creation_env: HostedProjectCreationEnv
) -> None:
    """Permission prompt is undecided, not a diagnosis that hides Install."""

    env = hosted_project_creation_env
    context, page = _new_page(browser, env)
    page.add_init_script(
        """
        (() => {
          if (!navigator.permissions) return;
          const original = navigator.permissions.query.bind(navigator.permissions);
          Object.defineProperty(navigator.permissions, 'query', {
            configurable: true,
            value: async (descriptor) => {
              if (descriptor && descriptor.name === 'local-network-access') {
                return { state: 'prompt' };
              }
              return original(descriptor);
            },
          });
        })();
        """
    )
    page.route(f"{DEFAULT_DAEMON_URL}/**", lambda route: route.abort())

    page.goto(f"{env.base_url}/connect", wait_until="networkidle")
    page.get_by_text(
        "Bicameral isn’t available on this device yet.", exact=True
    ).wait_for(timeout=10_000)

    assert not page.get_by_role("link", name="Install Bicameral").is_visible()
    assert page.get_by_role(
        "button", name="I already installed it — try again"
    ).is_visible()
    assert page.get_by_text(
        "Run the setup command below.", exact=False
    ).is_visible()
    assert page.get_by_text(
        "If Chrome asks to find devices on your network, choose Allow.", exact=False
    ).is_visible()
    assert not page.get_by_text(
        "Local Network Access is blocked for this site.", exact=False
    ).is_visible()

    context.close()


def test_installer_handoff_resumes_pairing_when_daemon_appears(
    browser, hosted_project_creation_env: HostedProjectCreationEnv
) -> None:
    """After Install is chosen, daemon appearance advances without another click."""

    env = hosted_project_creation_env
    context, page = _new_page(browser, env)
    state = {"available": False, "pair_posts": 0}
    headers = _cors_headers(env.base_url)

    def daemon_route(route) -> None:
        request = route.request
        if not state["available"]:
            route.abort()
            return
        if _legacy_status_absent(route, headers):
            return
        if request.method == "OPTIONS":
            route.fulfill(status=204, headers=headers, body="")
            return
        if request.method == "POST" and request.url.endswith("/pair"):
            state["pair_posts"] += 1
            route.fulfill(
                status=202,
                headers=headers,
                body=(
                    '{"status":"pending","request_handle":"req-install",'
                    '"poll_proof":"proof-install","pairing_code":"INSTA-11"}'
                ),
            )
            return
        route.fulfill(status=200, headers=headers, body='{"status":"pending"}')

    page.route(f"{DEFAULT_DAEMON_URL}/**", daemon_route)
    page.goto(f"{env.base_url}/connect", wait_until="networkidle")
    page.get_by_text(
        "Bicameral isn’t available on this device yet.", exact=True
    ).wait_for(timeout=10_000)

    _start_setup_without_navigation(page)
    state["available"] = True

    page.get_by_text("Approve this browser in Bicameral.", exact=True).wait_for(
        timeout=10_000
    )
    assert page.get_by_text("INSTA-11", exact=True).is_visible()
    assert state["pair_posts"] == 1

    context.close()


def test_retry_advances_after_daemon_starts(
    browser, hosted_project_creation_env: HostedProjectCreationEnv
) -> None:
    """Manual Retry remains a bounded recovery path when the daemon starts later."""

    env = hosted_project_creation_env
    context, page = _new_page(browser, env)
    state = {"available": False, "pair_posts": 0}
    headers = _cors_headers(env.base_url)

    def daemon_route(route) -> None:
        request = route.request
        if not state["available"]:
            route.abort()
            return
        if _legacy_status_absent(route, headers):
            return
        if request.method == "OPTIONS":
            route.fulfill(status=204, headers=headers, body="")
            return
        if request.method == "POST" and request.url.endswith("/pair"):
            state["pair_posts"] += 1
            route.fulfill(
                status=202,
                headers=headers,
                body=(
                    '{"status":"pending","request_handle":"req-retry",'
                    '"poll_proof":"proof-retry","pairing_code":"RETRY-1"}'
                ),
            )
            return
        route.fulfill(status=200, headers=headers, body='{"status":"pending"}')

    page.route(f"{DEFAULT_DAEMON_URL}/**", daemon_route)
    page.goto(f"{env.base_url}/connect", wait_until="networkidle")
    page.get_by_text(
        "Bicameral isn’t available on this device yet.", exact=True
    ).wait_for(timeout=10_000)

    state["available"] = True
    page.get_by_role(
        "button", name="I already installed it — try again"
    ).click()

    page.get_by_text("Approve this browser in Bicameral.", exact=True).wait_for(
        timeout=10_000
    )
    assert page.get_by_text("RETRY-1", exact=True).is_visible()
    assert state["pair_posts"] == 1

    context.close()


def test_setup_polling_times_out_to_truthful_recovery(
    browser, hosted_project_creation_env: HostedProjectCreationEnv
) -> None:
    """The setup wait is bounded and returns to a truthful retry/install state."""

    env = hosted_project_creation_env
    context, page = _new_page(browser, env)
    # Sequential polling schedules the next timer only after async probe work
    # settles. Cap browser timers before navigation so the real recursive loop
    # runs all bounded attempts quickly instead of relying on interval-style
    # fake-clock jumps that no longer model the implementation.
    page.add_init_script(
        """
        (() => {
          const realSetTimeout = window.setTimeout.bind(window);
          window.setTimeout = (fn, ms, ...args) =>
            realSetTimeout(fn, Math.min(Number(ms) || 0, 5), ...args);
        })();
        """
    )
    page.route(f"{DEFAULT_DAEMON_URL}/**", lambda route: route.abort())

    page.goto(f"{env.base_url}/connect", wait_until="networkidle")
    page.get_by_text(
        "Bicameral isn’t available on this device yet.", exact=True
    ).wait_for(timeout=10_000)

    _start_setup_without_navigation(page)
    page.get_by_text("Bicameral hasn’t started yet.", exact=True).wait_for(
        timeout=5_000
    )

    assert not page.get_by_role("link", name="Install Bicameral").is_visible()
    assert page.get_by_role(
        "button", name="I already installed it — try again"
    ).is_visible()

    context.close()


def test_slow_daemon_during_setup_creates_one_pair_request(
    browser, hosted_project_creation_env: HostedProjectCreationEnv
) -> None:
    """Sequential polling prevents slow reachability checks from duplicating /pair."""

    env = hosted_project_creation_env
    context, page = _new_page(browser, env)
    state = {"phase": "initial", "pair_posts": 0, "slow_options": 0}
    headers = _cors_headers(env.base_url)

    def daemon_route(route) -> None:
        request = route.request
        if state["phase"] == "initial":
            route.abort()
            return
        if _legacy_status_absent(route, headers):
            return
        if request.method == "OPTIONS":
            state["slow_options"] += 1
            time.sleep(5)
            route.fulfill(status=204, headers=headers, body="")
            return
        if request.method == "POST" and request.url.endswith("/pair"):
            state["pair_posts"] += 1
            route.fulfill(
                status=202,
                headers=headers,
                body=(
                    '{"status":"pending","request_handle":"req-slow",'
                    '"poll_proof":"proof-slow","pairing_code":"SLOW-01"}'
                ),
            )
            return
        route.fulfill(status=200, headers=headers, body='{"status":"pending"}')

    page.route(f"{DEFAULT_DAEMON_URL}/**", daemon_route)
    page.goto(f"{env.base_url}/connect", wait_until="networkidle")
    page.get_by_text(
        "Bicameral isn’t available on this device yet.", exact=True
    ).wait_for(timeout=10_000)

    _start_setup_without_navigation(page)
    state["phase"] = "slow"

    page.get_by_text("Approve this browser in Bicameral.", exact=True).wait_for(
        timeout=15_000
    )
    assert state["slow_options"] == 1
    assert state["pair_posts"] == 1
    assert page.get_by_text("SLOW-01", exact=True).is_visible()

    context.close()
