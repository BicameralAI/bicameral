"""Hosted browser smoke and visual evidence for Cloud-owned connection UX.

Product Create/Join behavior is accepted by the exact real-process terminal
journey. This gate owns two narrower browser boundaries:

1. Cloud serves the recorded Bot bundle and `/app` boots through a valid hosted
   handoff; and
2. Cloud `/connect` renders the APC1 frictionless pre-pair experience at desktop
   and narrow widths, automatically probing the daemon before showing setup.

The deterministic daemon responses used for presentation evidence do not grant
pairing authority or replace terminal pairing acceptance.
"""

from __future__ import annotations

import json
import re
from pathlib import Path

import httpx
import pytest

pytest.importorskip("playwright.sync_api")

from bicameral_cloud.routes.connect_local_accountability import (  # noqa: E402
    LINUX_BOOTSTRAP_COMMAND,
)
from tests.journey.browser_identity import (  # noqa: E402
    DEFAULT_DAEMON_URL,
    install_browser_identity,
    install_hosted_bridge_handoff,
)
from tests.journey.conftest import HostedProjectCreationEnv  # noqa: E402

pytestmark = [pytest.mark.journey]

_ARTIFACT_DIR = (
    Path(__file__).resolve().parents[2] / "test-artifacts" / "hosted_project_creation"
)
_BOT_COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
_BUNDLE_SHA256_RE = re.compile(r"^[0-9a-f]{64}$")


def _install_identity(page, env: HostedProjectCreationEnv) -> None:
    install_browser_identity(
        page,
        access_token=env.access_token,
        supabase_url=env.supabase_url,
        anon_key=env.anon_key,
    )


def _cors_headers(origin: str) -> dict[str, str]:
    return {
        "access-control-allow-origin": origin,
        "access-control-allow-methods": "GET, POST, OPTIONS",
        "access-control-allow-headers": "content-type, x-csrf-token",
        "content-type": "application/json",
    }


def _install_pending_daemon(page, origin: str) -> None:
    """Provide deterministic pending daemon facts for presentation evidence."""

    def handler(route) -> None:
        request = route.request
        headers = _cors_headers(origin)
        if request.method == "OPTIONS":
            route.fulfill(status=204, headers=headers, body="")
            return
        if request.url.endswith("/api/v1/hosted-bridge/pair"):
            route.fulfill(
                status=202,
                headers=headers,
                body=json.dumps(
                    {
                        "status": "pending",
                        "request_handle": "journey-request-handle",
                        "poll_proof": "journey-poll-proof",
                        "pairing_code": "7K2M-PQ8R",
                    }
                ),
            )
            return
        if "/api/v1/hosted-bridge/pair/status?" in request.url:
            route.fulfill(
                status=200,
                headers=headers,
                body=json.dumps({"status": "pending"}),
            )
            return
        route.abort()

    page.route(f"{DEFAULT_DAEMON_URL}/**", handler)


def test_cloud_dashboard_progressive_connect_visual_evidence(
    browser, hosted_project_creation_env: HostedProjectCreationEnv
) -> None:
    """Capture headed desktop+narrow evidence for frictionless user entry."""

    env = hosted_project_creation_env
    _ARTIFACT_DIR.mkdir(parents=True, exist_ok=True)

    # Desktop: daemon is already available, so no install or manual connect click
    # is required. The browser advances directly to local approval.
    desktop = browser.new_context(viewport={"width": 1280, "height": 900})
    desktop_page = desktop.new_page()
    _install_identity(desktop_page, env)
    _install_pending_daemon(desktop_page, env.base_url)
    desktop_page.goto(f"{env.base_url}/connect", wait_until="networkidle")
    desktop_page.wait_for_selector("#experience:not([hidden])", timeout=15_000)
    desktop_page.get_by_text("Approve this browser in Bicameral.", exact=True).wait_for(
        timeout=10_000
    )
    assert desktop_page.locator("#pairing-code").inner_text() == "7K2M-PQ8R"
    assert not desktop_page.get_by_role(
        "link", name=re.compile("Install Bicameral")
    ).is_visible()
    assert desktop_page.get_by_text("Cloud coordinates setup.").is_visible()
    desktop_page.screenshot(
        path=str(_ARTIFACT_DIR / "connect-desktop-pending.png"), full_page=True
    )
    desktop_page.get_by_role("button", name="Cancel").click()
    desktop_page.get_by_text("Nothing was connected.", exact=True).wait_for(
        timeout=5_000
    )
    assert desktop_page.get_by_role(
        "button", name="I already installed it — try again"
    ).is_visible()
    desktop.close()

    # Desktop recovery: unavailable daemon produces guided setup immediately.
    recovery = browser.new_context(viewport={"width": 1280, "height": 900})
    recovery_page = recovery.new_page()
    _install_identity(recovery_page, env)
    recovery_page.route(f"{DEFAULT_DAEMON_URL}/**", lambda route: route.abort())
    recovery_page.goto(f"{env.base_url}/connect", wait_until="networkidle")
    recovery_page.wait_for_selector("#experience:not([hidden])", timeout=15_000)
    recovery_page.get_by_text(
        "Bicameral isn’t available on this device yet.", exact=True
    ).wait_for(timeout=10_000)
    assert not recovery_page.get_by_role(
        "link", name=re.compile("Install Bicameral")
    ).is_visible()
    assert recovery_page.locator("#setup-commands").get_by_text(
        LINUX_BOOTSTRAP_COMMAND, exact=True
    ).is_visible()
    assert recovery_page.get_by_role(
        "button", name="I already installed it — try again"
    ).is_visible()
    # CLI is available but deliberately demoted behind troubleshooting.
    assert recovery_page.locator("#developer-help").evaluate("el => !el.open")
    recovery_page.screenshot(
        path=str(_ARTIFACT_DIR / "connect-desktop-guided-setup.png"), full_page=True
    )
    recovery.close()

    # Narrow layout: same auto-detect model with no wizard or manual connect gate.
    narrow = browser.new_context(viewport={"width": 390, "height": 844})
    narrow_page = narrow.new_page()
    _install_identity(narrow_page, env)
    _install_pending_daemon(narrow_page, env.base_url)
    narrow_page.goto(f"{env.base_url}/connect", wait_until="networkidle")
    narrow_page.wait_for_selector("#experience:not([hidden])", timeout=15_000)
    narrow_page.get_by_text("Approve this browser in Bicameral.", exact=True).wait_for(
        timeout=10_000
    )
    assert narrow_page.locator("#pairing-code").inner_text() == "7K2M-PQ8R"
    narrow_page.screenshot(
        path=str(_ARTIFACT_DIR / "connect-narrow-pending.png"), full_page=True
    )
    narrow.close()


def test_vendored_dashboard_bundle_boots(
    browser, hosted_project_creation_env: HostedProjectCreationEnv
) -> None:
    env = hosted_project_creation_env
    context = browser.new_context()
    context.tracing.start(screenshots=True, snapshots=True, sources=True)
    page = context.new_page()
    page_errors: list[str] = []
    failed_assets: list[str] = []
    loaded_assets: set[str] = set()

    page.on("pageerror", lambda error: page_errors.append(str(error)))
    page.on(
        "response",
        lambda response: _record_asset_response(
            response.url,
            response.status,
            env.base_url,
            loaded_assets,
            failed_assets,
        ),
    )

    _install_identity(page, env)
    install_hosted_bridge_handoff(
        page,
        access_token=env.access_token,
        cloud_account_id=env.user_email,
    )

    trace_stopped = False
    try:
        page.goto(f"{env.base_url}/connect", wait_until="networkidle")
        # Existing restore state may redirect before the connect body renders.
        page.goto(f"{env.base_url}/app", wait_until="networkidle")
        page.wait_for_selector(
            '[data-testid="paired-shell"], [data-testid="hosted-project-shell"], [data-testid="app-header"]',
            timeout=15_000,
        )

        metadata = _load_bundle_metadata(env.base_url)
        assert metadata["source_repository"] == "BicameralAI/bicameral-bot"
        assert _BOT_COMMIT_RE.fullmatch(str(metadata["bot_commit"]))
        assert _BUNDLE_SHA256_RE.fullmatch(str(metadata["bundle_sha256"]))
        assert any(path.endswith(".js") for path in loaded_assets)
        assert any(path.endswith(".css") for path in loaded_assets)
        assert failed_assets == []
        assert page_errors == []
    except Exception:
        _ARTIFACT_DIR.mkdir(parents=True, exist_ok=True)
        context.tracing.stop(path=str(_ARTIFACT_DIR / "trace.zip"))
        trace_stopped = True
        page.screenshot(path=str(_ARTIFACT_DIR / "screenshot.png"), full_page=True)
        raise
    finally:
        if not trace_stopped:
            context.tracing.stop()
        context.close()


def _record_asset_response(
    url: str,
    status: int,
    base_url: str,
    loaded_assets: set[str],
    failed_assets: list[str],
) -> None:
    parsed = httpx.URL(url)
    base = httpx.URL(base_url)
    if parsed.host != base.host or parsed.port != base.port:
        return
    if not parsed.path.startswith("/assets/"):
        return
    loaded_assets.add(parsed.path)
    if status >= 400:
        failed_assets.append(parsed.path)


def _load_bundle_metadata(base_url: str) -> dict[str, object]:
    response = httpx.get(f"{base_url}/app/version.json", timeout=10.0)
    assert response.status_code == 200
    assert "application/json" in response.headers.get("content-type", "")
    payload = response.json()
    assert isinstance(payload, dict)
    return payload
