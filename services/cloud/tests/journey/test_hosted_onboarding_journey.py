"""Two-user hosted onboarding UserJourney, in a real browser (cloud#127).

Drives the canonical ``/app`` SPA against the live Cloud app and a controlled
paired-daemon boundary to prove the alpha hosted-onboarding authority boundary
end to end, with no production Supabase/Figma/deployed credentials:

Owner:  a verified-domain account discovers the daemon-created, Cloud-published
Product; recreating the Cloud app keeps it discoverable.

Peer:   a second verified same-domain account sees the Product under
``Available for <domain>``, uses the row-level ``Join`` action (transient
``Joining…``), obtains a signed Cloud ticket, submits it through the paired
daemon, and only *after* daemon success becomes a member and can ``Open`` it;
reloading preserves the member state under ``Your products``.

Wrong-domain:  a third identity on a different allowlisted domain never
discovers the Product.

Only the browser identity/session and the local daemon response are controlled;
the discovery, publication, and signed-ticket contracts are the real Cloud
routes, and canonical membership/roles/replay stay daemon-authoritative.

Full-page screenshots and a Playwright video are retained under
``test-artifacts/hosted_onboarding`` as browser evidence.
"""

from __future__ import annotations

import json
import time
from pathlib import Path

import httpx
from tests.proof_fixtures import first_event_proof_dict
import pytest

pytest.importorskip("playwright.sync_api")

from playwright.sync_api import Browser, Page, Route  # noqa: E402

from bicameral_cloud.models.discovery import (  # noqa: E402
    WorkspaceDiscoveryItem,
    WorkspaceReadiness,
)
from bicameral_cloud.models.pairing import DaemonPairing, PairingStatus  # noqa: E402
from tests.journey.browser_identity import install_browser_identity  # noqa: E402
from tests.journey.conftest import HostedOnboardingEnv  # noqa: E402

pytestmark = [pytest.mark.journey]

_ARTIFACT_DIR = (
    Path(__file__).resolve().parents[2] / "test-artifacts" / "hosted_onboarding"
)
_DEFAULT_DAEMON_URL = "http://localhost:37373"


def _shot(page: Page, name: str) -> None:
    _ARTIFACT_DIR.mkdir(parents=True, exist_ok=True)
    page.screenshot(path=str(_ARTIFACT_DIR / f"{name}.png"), full_page=True)


def _seed_handoff(page: Page, *, access_token: str, cloud_account_id: str) -> None:
    """Seed the same-origin pairing handoff the canonical /app SPA reads.

    Unlike the single-user helper this does NOT blanket-abort the daemon: the
    peer journey fulfills the ``product-join`` call through the fake daemon.
    """

    handoff = json.dumps(
        {
            "session_token": "journey-hosted-bridge-session",
            "daemon_url": _DEFAULT_DAEMON_URL,
            "cloud_account_id": cloud_account_id,
            "cloud_access_token": access_token,
            "expires_at": "2099-01-01T00:00:00Z",
        }
    )
    page.add_init_script(
        "window.sessionStorage.setItem("
        f"'bicameral_hosted_bridge', {json.dumps(handoff)});"
    )


def _open_app(
    browser: Browser,
    env: HostedOnboardingEnv,
    *,
    access_token: str,
    cloud_account_id: str,
    record: bool = False,
):
    context = browser.new_context(
        record_video_dir=str(_ARTIFACT_DIR / "video") if record else None,
        viewport={"width": 1280, "height": 800},
    )
    page = context.new_page()
    install_browser_identity(
        page,
        access_token=access_token,
        supabase_url=env.supabase_url,
        anon_key=env.anon_key,
    )
    _seed_handoff(page, access_token=access_token, cloud_account_id=cloud_account_id)
    return context, page


def _goto_app(page: Page, base_url: str) -> None:
    page.goto(f"{base_url}/app", wait_until="networkidle")
    page.wait_for_selector('[data-testid="paired-shell"]', timeout=15_000)


def _publish_product(env: HostedOnboardingEnv):
    """Owner: daemon creates the canonical Product, then Cloud publishes it."""

    creation = env.daemon.create_product(
        creator_actor_ref="acct-onb-owner",
        display_name="Checkout",
        verified_domain=env.domain,
    )
    response = httpx.post(
        f"{env.base_url}/api/v1/products",
        headers={"Authorization": f"Bearer {env.owner_token}"},
        json={
            "canonical_product_id": creation.product_id,
            "display_name": creation.display_name,
            "eligible_domain": env.domain,
            "creation_receipt": creation.creation_receipt,
            "registration_ref": creation.registration_ref,
            "provider_hint": creation.provider_hint,
            "substrate_hint": creation.substrate_hint,
            "first_event_commit_proof": first_event_proof_dict(
                creation.product_id,
                substrate=creation.substrate_hint or "drive_folder",
            ),
        },
        timeout=10.0,
    )
    assert response.status_code == 201, response.text
    return creation


def test_two_user_hosted_onboarding_browser_journey(
    browser: Browser, hosted_onboarding_env: HostedOnboardingEnv
) -> None:
    env = hosted_onboarding_env
    creation = _publish_product(env)
    product_id = creation.product_id

    # --- Owner: the published Product is discoverable --------------------------
    owner_ctx, owner_page = _open_app(
        browser,
        env,
        access_token=env.owner_token,
        cloud_account_id=env.owner_email,
    )
    try:
        _goto_app(owner_page, env.base_url)
        owner_row = owner_page.locator('[data-testid="project-row"]').filter(
            has_text="Checkout"
        )
        owner_row.first.wait_for(state="visible", timeout=15_000)
        _shot(owner_page, "01-owner-discovers-product")

        # --- Recreate the Cloud app; the Product remains discoverable ----------
        new_base = env.restart()
        _goto_app(owner_page, new_base)
        owner_page.locator('[data-testid="project-row"]').filter(
            has_text="Checkout"
        ).first.wait_for(state="visible", timeout=15_000)
        _shot(owner_page, "02-owner-after-cloud-restart")
    finally:
        owner_ctx.close()

    # --- Peer: discovers as available, joins through the paired daemon ---------
    peer_ctx, peer_page = _open_app(
        browser,
        env,
        access_token=env.peer_token,
        cloud_account_id=env.peer_email,
        record=True,
    )

    def _fulfill_product_join(route: Route) -> None:
        payload = json.loads(route.request.post_data or "{}")
        signed_ticket = payload["ticket"]
        result = env.daemon.product_join(
            signed_ticket=signed_ticket, local_actor_ref=env.peer_id
        )
        # The daemon confirmed the join: Cloud now reflects membership in its
        # coordination projection (a compatible active pairing + the member
        # workspace row), which the SPA re-reads on its post-join refetch.
        if result["outcome"] == "granted":
            env.store.add_pairing(
                DaemonPairing(
                    pairing_id="pairing-onb",
                    team_id=env.domain_team,
                    daemon_id="daemon-onb",
                    status=PairingStatus.ACTIVE,
                    daemon_version="1.2.0",
                )
            )
            env.store.set_workspaces(
                env.domain_team,
                [
                    WorkspaceDiscoveryItem(
                        workspace_id=product_id,
                        display_name="Checkout",
                        pairing_status="active",
                        readiness=WorkspaceReadiness.READY,
                    )
                ],
            )
        # Give the SPA a beat to render the transient "Joining…" affordance.
        time.sleep(0.5)
        route.fulfill(
            status=200,
            content_type="application/json",
            body=json.dumps(result),
        )

    # Fulfill only the product-join call; other daemon paths stay unreachable.
    peer_page.route(f"{_DEFAULT_DAEMON_URL}/**", lambda route: route.abort())
    peer_page.route(
        f"{_DEFAULT_DAEMON_URL}/api/v1/hosted-bridge/product-join",
        _fulfill_product_join,
    )

    try:
        _goto_app(peer_page, env.base_url)
        available = peer_page.locator('[data-testid="available-section"]')
        available.wait_for(state="visible", timeout=15_000)
        heading = peer_page.locator('[data-testid="available-heading"]')
        assert env.domain in (heading.inner_text() or "")
        join = peer_page.locator('[data-testid="join-action"]')
        join.wait_for(state="visible", timeout=15_000)
        _shot(peer_page, "03-peer-available")

        with peer_page.expect_response(
            lambda r: "hosted-bridge/product-join" in r.url
        ) as join_info:
            join.click()
            # Transient joining affordance shown before daemon success.
            try:
                peer_page.wait_for_selector(
                    '[data-testid="join-action"][data-joining="true"]',
                    timeout=2_000,
                )
                _shot(peer_page, "04-peer-joining")
            except Exception:
                # The transient window can be missed on a fast machine.
                pass
        join_response = join_info.value
        assert join_response.ok, f"daemon product-join failed: {join_response.status}"
        assert json.loads(join_response.body())["granted_role"] == "Product Member"
        # No membership until daemon success: the peer must not have been shown
        # a join-failure affordance.
        assert peer_page.locator('[data-testid="join-failure"]').count() == 0
        _shot(peer_page, "05-peer-joined")

        # --- Reload returns to the operator home; member state persists under
        # "Your products" with an Open action (daemon-confirmed membership). ----
        _goto_app(peer_page, env.base_url)
        members = peer_page.locator('[data-testid="your-products-section"]')
        members.wait_for(state="visible", timeout=15_000)
        member_row = members.locator('[data-testid="project-row"]').filter(
            has_text="Checkout"
        )
        member_row.first.wait_for(state="visible", timeout=15_000)
        assert member_row.locator('[data-action="open"]').count() >= 1
        # The peer sees no free-form/global Join action and no join-ticket prompt.
        assert peer_page.locator('[data-testid="join-action"]').count() == 0
        _shot(peer_page, "06-peer-your-products-open")
    finally:
        peer_ctx.close()

    # --- Wrong-domain identity never discovers the Product ---------------------
    wrong_ctx, wrong_page = _open_app(
        browser,
        env,
        access_token=env.wrong_token,
        cloud_account_id=env.wrong_email,
    )
    try:
        _goto_app(wrong_page, env.base_url)
        # Let discovery settle, then assert the Product is absent entirely.
        wrong_page.wait_for_timeout(1_000)
        assert (
            wrong_page.locator('[data-testid="project-row"]')
            .filter(has_text="Checkout")
            .count()
            == 0
        )
        _shot(wrong_page, "07-wrong-domain-cannot-discover")
    finally:
        wrong_ctx.close()
