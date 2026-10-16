"""APC1 browser evidence for Chrome Local Network Access recovery."""

from __future__ import annotations

import re
import time

import pytest

pytest.importorskip("playwright.sync_api")

from bicameral_cloud.routes.connect_local_accountability import (  # noqa: E402
    LINUX_BOOTSTRAP_COMMAND,
    LOCAL_STATUS_PATH,
)
from tests.journey.browser_identity import DEFAULT_DAEMON_URL, install_browser_identity  # noqa: E402
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


def _install_local_permission(page, state: str) -> None:
    page.add_init_script(
        f"""
        (() => {{
          if (!navigator.permissions) return;
          const original = navigator.permissions.query.bind(navigator.permissions);
          Object.defineProperty(navigator.permissions, 'query', {{
            configurable: true,
            value: async (descriptor) => {{
              if (descriptor && descriptor.name === 'local-network-access') {{
                return {{ state: {state!r} }};
              }}
              return original(descriptor);
            }},
          }});
        }})();
        """
    )


def _pending_pair(route, pair_posts: list[str]) -> None:
    request = route.request
    # These journey cases intentionally model a pre-local-status daemon. An old
    # daemon has no status route, so return the truthful 404 that activates the
    # bounded compatibility probe rather than feeding pairing JSON to the new
    # status parser.
    if request.method == "GET" and request.url.endswith(LOCAL_STATUS_PATH):
        route.fulfill(status=404, content_type="application/json", body="{}")
        return
    if request.method == "POST" and request.url.endswith("/pair"):
        pair_posts.append(request.url)
        route.fulfill(
            status=202,
            content_type="application/json",
            body=(
                '{"status":"pending","request_handle":"req-apc1",'
                '"poll_proof":"proof-apc1","pairing_code":"APC1-42"}'
            ),
        )
        return
    route.fulfill(
        status=200,
        content_type="application/json",
        body='{"status":"pending"}',
    )


def test_connect_explains_blocked_local_network_permission(
    browser, hosted_project_creation_env: HostedProjectCreationEnv
) -> None:
    """A browser permission block must not be misreported as a missing daemon."""

    env = hosted_project_creation_env
    context, page = _new_page(browser, env)
    _install_local_permission(page, "denied")
    page.route(f"{DEFAULT_DAEMON_URL}/**", lambda route: route.abort())

    page.goto(f"{env.base_url}/connect", wait_until="networkidle")
    page.wait_for_selector("#experience:not([hidden])", timeout=15_000)
    page.get_by_text("Allow this browser to find Bicameral.", exact=True).wait_for(
        timeout=10_000
    )

    assert page.get_by_text(
        "Local Network Access is blocked for this site.", exact=False
    ).is_visible()
    assert not page.get_by_role(
        "link", name=re.compile("Install Bicameral")
    ).is_visible()
    assert page.get_by_role(
        "button", name="I already installed it — try again"
    ).is_visible()
    assert not page.get_by_text(
        "Bicameral isn’t available on this device yet.", exact=True
    ).is_visible()

    context.close()


def test_prompt_without_daemon_shows_setup_with_allow_guidance(
    browser, hosted_project_creation_env: HostedProjectCreationEnv
) -> None:
    """Permission prompt is undecided, so normal setup remains available."""

    env = hosted_project_creation_env
    context, page = _new_page(browser, env)
    _install_local_permission(page, "prompt")
    page.route(f"{DEFAULT_DAEMON_URL}/**", lambda route: route.abort())

    page.goto(f"{env.base_url}/connect", wait_until="networkidle")
    page.wait_for_selector("#experience:not([hidden])", timeout=15_000)
    page.get_by_text(
        "Bicameral isn’t available on this device yet.", exact=True
    ).wait_for(timeout=10_000)

    assert not page.get_by_role(
        "link", name=re.compile("Install Bicameral")
    ).is_visible()
    assert page.locator("#setup-commands").get_by_text(
        LINUX_BOOTSTRAP_COMMAND, exact=True
    ).is_visible()
    assert page.get_by_role(
        "button", name="I already installed it — try again"
    ).is_visible()
    assert page.get_by_text(
        "If Chrome asks to find devices on your network, choose Allow.", exact=False
    ).is_visible()
    assert not page.get_by_text(
        "Local Network Access is blocked for this site.", exact=False
    ).is_visible()

    context.close()


def test_permission_granted_as_probe_ends_gets_one_bounded_retry(
    browser, hosted_project_creation_env: HostedProjectCreationEnv
) -> None:
    """A prompt->granted transition must not fall through to installer UX."""

    env = hosted_project_creation_env
    context, page = _new_page(browser, env)

    page.add_init_script(
        """
        (() => {
          if (!navigator.permissions) return;
          let localQueries = 0;
          const original = navigator.permissions.query.bind(navigator.permissions);
          Object.defineProperty(navigator.permissions, 'query', {
            configurable: true,
            value: async (descriptor) => {
              if (descriptor && descriptor.name === 'local-network-access') {
                localQueries += 1;
                return { state: localQueries === 1 ? 'prompt' : 'granted' };
              }
              return original(descriptor);
            },
          });
        })();
        """
    )

    option_attempts = 0

    def daemon_route(route) -> None:
        nonlocal option_attempts
        if route.request.method == "OPTIONS":
            option_attempts += 1
            if option_attempts == 1:
                route.abort()
            else:
                route.fulfill(status=204, body="")
            return
        _pending_pair(route, [])

    page.route(f"{DEFAULT_DAEMON_URL}/**", daemon_route)

    page.goto(f"{env.base_url}/connect", wait_until="networkidle")
    page.wait_for_selector("#experience:not([hidden])", timeout=15_000)
    page.get_by_text("Approve this browser in Bicameral.", exact=True).wait_for(
        timeout=10_000
    )

    assert option_attempts == 2
    assert page.get_by_text("APC1-42", exact=True).is_visible()
    assert not page.get_by_role(
        "link", name=re.compile("Install Bicameral")
    ).is_visible()
    assert not page.get_by_text(
        "Bicameral isn’t available on this device yet.", exact=True
    ).is_visible()

    context.close()


def test_install_wait_continues_when_daemon_appears(
    browser, hosted_project_creation_env: HostedProjectCreationEnv
) -> None:
    """Installer handoff polling advances automatically when the daemon appears."""

    env = hosted_project_creation_env
    context, page = _new_page(browser, env)
    reachable = False
    pair_posts: list[str] = []

    def daemon_route(route) -> None:
        if route.request.method == "OPTIONS":
            if reachable:
                route.fulfill(status=204, body="")
            else:
                route.abort()
            return
        _pending_pair(route, pair_posts)

    page.route(f"{DEFAULT_DAEMON_URL}/**", daemon_route)
    page.goto(f"{env.base_url}/connect", wait_until="networkidle")
    page.get_by_text(
        "Bicameral isn’t available on this device yet.", exact=True
    ).wait_for(timeout=10_000)

    page.evaluate("beginSetupPolling()")
    reachable = True
    page.get_by_text("Approve this browser in Bicameral.", exact=True).wait_for(
        timeout=8_000
    )

    assert len(pair_posts) == 1
    assert page.get_by_text("APC1-42", exact=True).is_visible()
    context.close()


def test_retry_after_daemon_starts_advances_to_pairing(
    browser, hosted_project_creation_env: HostedProjectCreationEnv
) -> None:
    """Manual retry after startup enters the same single pairing flow."""

    env = hosted_project_creation_env
    context, page = _new_page(browser, env)
    reachable = False
    pair_posts: list[str] = []

    def daemon_route(route) -> None:
        if route.request.method == "OPTIONS":
            if reachable:
                route.fulfill(status=204, body="")
            else:
                route.abort()
            return
        _pending_pair(route, pair_posts)

    page.route(f"{DEFAULT_DAEMON_URL}/**", daemon_route)
    page.goto(f"{env.base_url}/connect", wait_until="networkidle")
    retry = page.get_by_role(
        "button", name="I already installed it — try again"
    )
    retry.wait_for(timeout=10_000)

    reachable = True
    retry.click()
    page.get_by_text("Approve this browser in Bicameral.", exact=True).wait_for(
        timeout=8_000
    )

    assert len(pair_posts) == 1
    context.close()


def test_setup_timeout_is_bounded_and_recoverable(
    browser, hosted_project_creation_env: HostedProjectCreationEnv
) -> None:
    """A daemon that never appears ends in a truthful bounded recovery state."""

    env = hosted_project_creation_env
    context, page = _new_page(browser, env)
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

    page.evaluate("beginSetupPolling()")
    page.get_by_text("Bicameral hasn’t started yet.", exact=True).wait_for(
        timeout=5_000
    )

    assert not page.get_by_role(
        "link", name=re.compile("Install Bicameral")
    ).is_visible()
    assert page.get_by_role(
        "button", name="I already installed it — try again"
    ).is_visible()
    context.close()


def test_slow_daemon_does_not_duplicate_pair_requests(
    browser, hosted_project_creation_env: HostedProjectCreationEnv
) -> None:
    """Sequential setup polling produces one POST /pair even after a slow probe."""

    env = hosted_project_creation_env
    context, page = _new_page(browser, env)
    setup_polling = False
    pair_posts: list[str] = []

    def daemon_route(route) -> None:
        if route.request.method == "OPTIONS":
            if not setup_polling:
                route.abort()
                return
            time.sleep(5)
            route.fulfill(status=204, body="")
            return
        _pending_pair(route, pair_posts)

    page.route(f"{DEFAULT_DAEMON_URL}/**", daemon_route)
    page.goto(f"{env.base_url}/connect", wait_until="networkidle")
    page.get_by_text(
        "Bicameral isn’t available on this device yet.", exact=True
    ).wait_for(timeout=10_000)

    setup_polling = True
    page.evaluate("beginSetupPolling()")
    page.get_by_text("Approve this browser in Bicameral.", exact=True).wait_for(
        timeout=12_000
    )

    assert len(pair_posts) == 1
    context.close()
