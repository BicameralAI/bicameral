"""Hosted Project Creation UserJourney, exercised in a real browser (cloud#118).

Journey: ``hosted_project_creation`` / Hosted Project Creation.
Success state: ``project_visible_as_needs_binding_in_app``.

This is the deterministic, executable browser realization of the journey. Since
cloud#118 makes ``/app`` the *only* product project-discovery surface (the
duplicate inline ``/connect`` panel is removed), the journey now drives the
canonical ``/app`` SPA end to end and asserts the whole
create -> refetch -> visible-after-reload path against the *real* Cloud
registration and discovery HTTP contracts (no fixture seeding of the final list
state, no production credentials):

1. A documented command starts all required local services (fake Supabase
   allowlist + live Cloud app) and runs this journey without production creds.
2. Entry is through ``/connect`` (the retained auth/pairing entrypoint): it
   must NOT render a project list/create/join surface anymore (regression guard
   against the duplication returning), then the browser is handed off to the
   canonical ``/app`` SPA using the same ``bicameral_hosted_bridge`` payload
   ``/connect`` writes on pairing.
3. The browser creates a uniquely named Project through the real ``/app`` Create
   UI. Cloud accepts the create (``202``) via the unified create contract.
4. Discovery is proven to be refetched after create: a stale list that never
   refetches fails. The created Project can only render from a fresh
   ``GET /api/v1/projects`` issued after the create POST.
5. The new Project appears in the canonical ``/app`` project table and its
   discovery item reports ``unbound`` local-workspace readiness (the "needs
   binding" state: discoverable but not yet bound by a local daemon).
6. Reloading the browser preserves the new Project row (durable coordination).
7. The dashboard bundle bot commit/digest is recorded; absent/malformed
   version metadata fails.
8. Producer/consumer assertions fail loudly if the receipt or discovery item
   shape drifts from the Cloud contract models.

On failure a Playwright trace, a screenshot, and an HTTP/version summary are
retained under ``test-artifacts/hosted_project_creation`` (acceptance: CI
failure evidence). Existing Cloud tests are unaffected — this journey is gated
behind the ``journey`` marker and skips cleanly when Playwright is absent.
"""

from __future__ import annotations

import json
import re
import uuid
from dataclasses import dataclass, field
from pathlib import Path

import httpx
import pytest

pytest.importorskip("playwright.sync_api")

from playwright.sync_api import Page, Response  # noqa: E402
from playwright.sync_api import TimeoutError as PWTimeoutError

from bicameral_cloud.models.discovery import ProjectListItem  # noqa: E402
from bicameral_cloud.models.registration import (  # noqa: E402
    RegistrationReceipt,
)
from tests.journey.browser_identity import (  # noqa: E402
    install_browser_identity,
    install_hosted_bridge_handoff,
)
from tests.journey.conftest import HostedProjectCreationEnv  # noqa: E402

pytestmark = [pytest.mark.journey]

_ARTIFACT_DIR = (
    Path(__file__).resolve().parents[2]
    / "test-artifacts"
    / "hosted_project_creation"
)
_BOT_COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
_BUNDLE_SHA256_RE = re.compile(r"^[0-9a-f]{64}$")

# Expected wire shapes, derived from the Cloud contract models so a drift in
# either the producer (Cloud routes) or the consumer (this journey) fails loud.
_EXPECTED_RECEIPT_FIELDS = set(RegistrationReceipt.model_fields)
_EXPECTED_DISCOVERY_ITEM_FIELDS = set(ProjectListItem.model_fields) - {
    "is_member",
    "preview",
    "verified_domain",
}

_DISCOVERY_PATH = "/api/v1/projects"
_CREATE_PATH_RE = re.compile(r"^/api/v1/teams/[^/]+/projects$")

# Inline-panel selectors that must NOT exist on the retained `/connect`
# shell after cloud#118 (the project surface lives only on `/app`).
_FORBIDDEN_DASHBOARD_SELECTORS = (
    "#projects-panel",
    "#create-project-btn",
    "#join-project-btn",
    "#projects-list",
)


@dataclass
class _NetworkLog:
    """Ordered record of the real create/discovery contract exchanges."""

    discovery_count: int = 0
    latest_discovery_body: list[dict[str, object]] | None = None
    create_responses: list[tuple[int, dict[str, object]]] = field(
        default_factory=list
    )

    def attach(self, page: Page) -> None:
        page.on("response", self._on_response)

    def _on_response(self, response: Response) -> None:
        request = response.request
        path = httpx.URL(response.url).path
        if request.method == "GET" and path == _DISCOVERY_PATH:
            self.discovery_count += 1
            body = _safe_json(response)
            if isinstance(body, dict) and isinstance(body.get("projects"), list):
                self.latest_discovery_body = body["projects"]
        elif request.method == "POST" and _CREATE_PATH_RE.match(path):
            body = _safe_json(response)
            if isinstance(body, dict):
                self.create_responses.append((response.status, body))


def _safe_json(response: Response) -> object:
    try:
        return response.json()
    except Exception:  # pragma: no cover - non-JSON body
        return None


def test_hosted_project_creation_journey(
    browser, hosted_project_creation_env: HostedProjectCreationEnv
) -> None:
    env = hosted_project_creation_env
    project_name = f"Journey Project {uuid.uuid4().hex[:8]}"

    context = browser.new_context()
    context.tracing.start(screenshots=True, snapshots=True, sources=True)
    page = context.new_page()
    network = _NetworkLog()
    network.attach(page)

    install_browser_identity(
        page,
        access_token=env.access_token,
        supabase_url=env.supabase_url,
        anon_key=env.anon_key,
    )
    install_hosted_bridge_handoff(
        page,
        access_token=env.access_token,
        cloud_account_id=env.user_email,
    )

    version_meta: dict[str, object] | None = None
    try:
        # (2) /connect is the retained auth/pairing entrypoint. After
        # cloud#118 it must NOT render any project list/create/join surface;
        # that duplication reappearing here fails the journey in a real browser.
        page.goto(f"{env.base_url}/connect", wait_until="networkidle")
        page.wait_for_selector("#dashboard-body:not([hidden])", timeout=15_000)
        for selector in _FORBIDDEN_DASHBOARD_SELECTORS:
            assert page.locator(selector).count() == 0, (
                f"/connect must not render project-discovery UI ({selector}); "
                "the canonical surface is /app (cloud#118)."
            )

        # Hand off to the canonical /app SPA (same payload /connect writes).
        page.goto(f"{env.base_url}/app", wait_until="networkidle")
        page.wait_for_selector('[data-testid="paired-shell"]', timeout=15_000)
        page.wait_for_selector('[data-testid="empty-state"]', timeout=15_000)

        discovery_before_create = network.discovery_count

        # (3) Create a uniquely named Product through the real /app Create UI.
        page.get_by_role("button", name="Create product").first.click()
        page.wait_for_selector('[data-testid="create-project-form"]')
        page.fill('[data-testid="create-project-name"]', project_name)

        # Cloud accepts the create via the unified create contract (202).
        with page.expect_response(_is_create_response) as create_info:
            page.click('[data-testid="create-project-submit"]')
        create_response = create_info.value
        status = create_response.status
        receipt = _safe_json(create_response)
        assert isinstance(receipt, dict), "create response was not JSON"
        network.create_responses.append((status, receipt))
        assert status == 202, f"expected 202 create receipt, got {status}"
        _assert_shape(
            receipt,
            _EXPECTED_RECEIPT_FIELDS,
            producer="Cloud POST /api/v1/teams/{team}/projects (RegistrationReceipt)",
        )
        assert receipt["next_step"] == "local_daemon_binding_required", (
            "create receipt must direct the user to local daemon binding, got "
            f"{receipt['next_step']!r}"
        )

        # (4) + (5) Discovery must be refetched after create and surface the new
        # Project in the canonical /app table as unbound ("needs binding").
        row = _row_by_name(page, project_name)
        _wait_for_row(row, project_name)
        assert network.discovery_count > discovery_before_create, (
            "discovery endpoint was not refetched after create "
            f"({network.discovery_count} <= {discovery_before_create})"
        )

        discovered = _discovered_by_name(network.latest_discovery_body, project_name)
        assert discovered is not None, "created Project absent from discovery"
        _assert_shape(
            discovered,
            _EXPECTED_DISCOVERY_ITEM_FIELDS,
            producer="Cloud GET /api/v1/projects (ProjectListItem)",
        )
        assert discovered["binding_readiness"] == "unbound", (
            "a newly created Project must be discoverable as unbound "
            f'("needs binding"), got {discovered["binding_readiness"]!r}'
        )

        # (6) Reload preserves the new Project row (durable coordination state).
        discovery_before_reload = network.discovery_count
        page.reload(wait_until="networkidle")
        page.wait_for_selector('[data-testid="paired-shell"]', timeout=15_000)
        reloaded_row = _row_by_name(page, project_name)
        _wait_for_row(reloaded_row, project_name)
        assert network.discovery_count > discovery_before_reload, (
            "discovery was not refetched after reload"
        )

        # (7) Record the tested dashboard bundle provenance; fail on
        # absent/malformed version metadata.
        version_meta = _assert_bundle_version(env.base_url)

    except Exception:
        _retain_failure_evidence(context, page, network, version_meta)
        raise
    finally:
        context.tracing.stop()
        context.close()


def _is_create_response(response: Response) -> bool:
    return (
        response.request.method == "POST"
        and _CREATE_PATH_RE.match(httpx.URL(response.url).path) is not None
    )


def _row_by_name(page: Page, project_name: str):
    return page.locator('[data-testid="project-row"]').filter(
        has_text=project_name
    )


def _wait_for_row(row, project_name: str) -> None:
    try:
        row.first.wait_for(state="visible", timeout=15_000)
    except PWTimeoutError as exc:  # pragma: no cover - surfaced as failure
        raise AssertionError(
            f"Project row for {project_name!r} never appeared in the /app list"
        ) from exc


def _assert_shape(
    payload: dict[str, object], expected: set[str], *, producer: str
) -> None:
    actual = set(payload)
    if actual != expected:
        missing = expected - actual
        extra = actual - expected
        raise AssertionError(
            f"{producer} shape drift: missing={sorted(missing)} "
            f"unexpected={sorted(extra)}"
        )


def _discovered_by_name(
    projects: list[dict[str, object]] | None, display_name: str
) -> dict[str, object] | None:
    for project in projects or []:
        if project.get("display_name") == display_name:
            return project
    return None


def _assert_bundle_version(base_url: str) -> dict[str, object]:
    response = httpx.get(f"{base_url}/app/version.json", timeout=10.0)
    assert response.status_code == 200, (
        f"dashboard bundle version metadata unavailable: {response.status_code}"
    )
    assert "application/json" in response.headers.get("content-type", ""), (
        "version metadata must be JSON, not the SPA index fallback"
    )
    meta = response.json()
    assert meta.get("source_repository") == "BicameralAI/bicameral-bot"
    bot_commit = meta.get("bot_commit")
    bundle_sha256 = meta.get("bundle_sha256")
    assert isinstance(bot_commit, str) and _BOT_COMMIT_RE.match(bot_commit), (
        f"malformed bot_commit: {bot_commit!r}"
    )
    assert isinstance(bundle_sha256, str) and _BUNDLE_SHA256_RE.match(
        bundle_sha256
    ), f"malformed bundle_sha256: {bundle_sha256!r}"
    return meta


def _retain_failure_evidence(
    context, page: Page, network: _NetworkLog, version_meta: object
) -> None:
    """Persist trace, screenshot, and an HTTP/version summary on failure."""

    try:
        _ARTIFACT_DIR.mkdir(parents=True, exist_ok=True)
        context.tracing.stop(path=str(_ARTIFACT_DIR / "trace.zip"))
        # Re-arm tracing so the outer stop() in `finally` is a no-op-safe call.
        context.tracing.start(screenshots=True, snapshots=True, sources=True)
        page.screenshot(path=str(_ARTIFACT_DIR / "screenshot.png"), full_page=True)
        summary = {
            "discovery_request_count": network.discovery_count,
            "create_responses": [
                {"status": status, "body": body}
                for status, body in network.create_responses
            ],
            "latest_discovery_projects": network.latest_discovery_body,
            "tested_bundle_version": version_meta,
        }
        (_ARTIFACT_DIR / "summary.json").write_text(
            json.dumps(summary, indent=2, default=str), encoding="utf-8"
        )
    except Exception:  # pragma: no cover - evidence capture must never mask
        pass
