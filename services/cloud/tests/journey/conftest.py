"""Fixtures for the browser UserJourney harnesses (issue #114).

Wires the isolated realistic test store, the fake Supabase allowlist surface,
a locally minted authenticated identity, and the live Cloud app server into a
single ``hosted_project_creation_env`` fixture, plus a Playwright browser.

No production credentials are used: Supabase auth is validated locally against
a test HS256 secret and the design-partner allowlist is a loopback fake.
"""

from __future__ import annotations

import base64
import contextlib
from collections.abc import Callable, Iterator
from dataclasses import dataclass

import pytest

# The whole journey package requires Playwright; skip cleanly when absent so the
# rest of the suite (and environments without a browser) is unaffected.
pytest.importorskip("playwright.sync_api")

from cryptography.hazmat.primitives import serialization  # noqa: E402
from cryptography.hazmat.primitives.asymmetric.ed25519 import (  # noqa: E402
    Ed25519PrivateKey,
)
from playwright.sync_api import Browser, sync_playwright  # noqa: E402

from bicameral_cloud.config import CloudSettings, reset_settings  # noqa: E402
from bicameral_cloud.models.tenancy import Team  # noqa: E402
from bicameral_cloud.product_directory_repository import (  # noqa: E402
    get_inmemory_product_directory_repository,
    get_product_directory_repository,
    reset_inmemory_product_directory_repository,
)
from bicameral_cloud.store import Store, reset_store  # noqa: E402
from tests.proof_fixtures import enroll_identity_for_user  # noqa: E402
from tests.conformance.fake_product_daemon import FakeProductDaemon  # noqa: E402
from tests.conformance.fake_supabase import (  # noqa: E402
    FAKE_JWT_SECRET,
    mint_supabase_jwt,
    run_fake_supabase,
)
from tests.journey.app_server import run_cloud_app  # noqa: E402

ALLOWED_DOMAIN = "bicameral-ai.com"
USER_ID = "user-journey-114"
USER_EMAIL = f"journey-user@{ALLOWED_DOMAIN}"
TEAM_ID = "team-journey-114"
TEAM_DISPLAY_NAME = "Journey Team"
ANON_KEY = "anon-key-journey-test"
SERVICE_KEY = "service-key-journey-test"

# --- Two-user hosted onboarding journey (cloud#127) -------------------------- #
ONBOARDING_DOMAIN = "bicameral-ai.com"
OTHER_DOMAIN = "rival-ai.com"
OWNER_ONB_ID = "acct-onb-owner"
OWNER_ONB_EMAIL = f"owner@{ONBOARDING_DOMAIN}"
PEER_ONB_ID = "acct-onb-peer"
PEER_ONB_EMAIL = f"peer@{ONBOARDING_DOMAIN}"
WRONG_ONB_ID = "acct-onb-wrong"
WRONG_ONB_EMAIL = f"intruder@{OTHER_DOMAIN}"
ONBOARDING_DOMAIN_TEAM = "team-onb-domain"
# Controlled Cloud join-ticket signing key (shared with the fake daemon).
TICKET_ISSUER = "https://cloud.bicameral.example"
TICKET_KEY_ID = "join-2026-07"
TICKET_SEED = bytes(range(1, 33))


@dataclass(frozen=True)
class HostedProjectCreationEnv:
    """Everything a journey needs to drive the hosted dashboard."""

    base_url: str
    supabase_url: str
    anon_key: str
    access_token: str
    store: Store
    team_id: str
    user_email: str


@pytest.fixture(scope="session")
def browser() -> Iterator[Browser]:
    """A shared headless Chromium browser for the journey."""

    with sync_playwright() as playwright:
        instance = playwright.chromium.launch(headless=True)
        try:
            yield instance
        finally:
            instance.close()


@pytest.fixture()
def hosted_project_creation_env() -> Iterator[HostedProjectCreationEnv]:
    """Isolated, realistic hosted-project-creation environment on loopback.

    Seeds a domain-scoped team so the authenticated design-partner user is
    eligible to coordinate/create without a prior membership row or join code
    (cloud#95 domain gating), then serves the live Cloud app.
    """

    with run_fake_supabase([ALLOWED_DOMAIN]) as supabase_url:
        reset_settings(
            CloudSettings(
                supabase_url=supabase_url,
                supabase_jwt_secret=FAKE_JWT_SECRET,
                supabase_anon_key=ANON_KEY,
                supabase_service_key=SERVICE_KEY,
            )
        )
        store = reset_store()
        store.add_team(
            Team(
                team_id=TEAM_ID,
                display_name=TEAM_DISPLAY_NAME,
                created_by="seed",
                domain=ALLOWED_DOMAIN,
            )
        )
        access_token = mint_supabase_jwt(
            USER_EMAIL, sub=USER_ID, ttl_minutes=30
        )
        with run_cloud_app() as base_url:
            yield HostedProjectCreationEnv(
                base_url=base_url,
                supabase_url=supabase_url,
                anon_key=ANON_KEY,
                access_token=access_token,
                store=store,
                team_id=TEAM_ID,
                user_email=USER_EMAIL,
            )


@dataclass
class HostedOnboardingEnv:
    """Two-user hosted onboarding topology for the cloud#127 browser journey.

    Holds the live Cloud base URL, the shared in-memory ``Store`` (Cloud's
    non-canonical coordination projection tier), the authoritative fake local
    daemon, minted identities for owner/peer/wrong-domain, and a ``restart``
    hook that recreates the Cloud app process while the daemon's canonical state
    and the durable directory persist.
    """

    base_url: str
    supabase_url: str
    anon_key: str
    store: Store
    daemon: FakeProductDaemon
    owner_token: str
    owner_email: str
    peer_token: str
    peer_email: str
    peer_id: str
    wrong_token: str
    wrong_email: str
    domain: str
    domain_team: str
    _restart: Callable[[], str]

    def restart(self) -> str:
        """Recreate the Cloud app; canonical/daemon + durable state persist."""
        self.base_url = self._restart()
        return self.base_url


def _onboarding_signing_settings(supabase_url: str) -> CloudSettings:
    return CloudSettings(
        supabase_url=supabase_url,
        supabase_jwt_secret=FAKE_JWT_SECRET,
        supabase_anon_key=ANON_KEY,
        supabase_service_key=SERVICE_KEY,
        product_join_ticket_issuer=TICKET_ISSUER,
        product_join_ticket_ttl_seconds=300,
        product_join_signing_key_id=TICKET_KEY_ID,
        product_join_signing_private_key_b64=base64.b64encode(TICKET_SEED).decode(),
    )


@pytest.fixture()
def hosted_onboarding_env() -> Iterator[HostedOnboardingEnv]:
    """Deterministic two-user hosted onboarding environment on loopback.

    Allowlists both the onboarding domain and a *different* domain (so the
    wrong-domain identity is a genuine cross-tenant design partner, not merely
    someone the gate rejects), pins the controlled Cloud ticket signer, and
    routes the durable directory to the in-memory repository (no live Supabase).
    """

    from bicameral_cloud.app import app

    with run_fake_supabase([ONBOARDING_DOMAIN, OTHER_DOMAIN]) as supabase_url:
        reset_settings(_onboarding_signing_settings(supabase_url))
        store = reset_store()
        reset_inmemory_product_directory_repository()
        store.add_team(
            Team(
                team_id=ONBOARDING_DOMAIN_TEAM,
                display_name="Acme",
                created_by="seed",
                domain=ONBOARDING_DOMAIN,
            )
        )
        enroll_identity_for_user(
            store, OWNER_ONB_ID, team_id=ONBOARDING_DOMAIN_TEAM
        )
        # The durable directory route must not reach a real Supabase; use the
        # in-memory repository the pinned discovery source already reads.
        app.dependency_overrides[get_product_directory_repository] = (
            get_inmemory_product_directory_repository
        )

        daemon = FakeProductDaemon(
            verification_keys={
                TICKET_KEY_ID: (
                    Ed25519PrivateKey.from_private_bytes(TICKET_SEED)
                    .public_key()
                    .public_bytes(
                        encoding=serialization.Encoding.Raw,
                        format=serialization.PublicFormat.Raw,
                    )
                )
            },
            issuer=TICKET_ISSUER,
        )

        stack = contextlib.ExitStack()
        base_url = stack.enter_context(run_cloud_app())

        def _restart() -> str:
            nonlocal base_url
            stack.close()
            new_stack = contextlib.ExitStack()
            base_url = new_stack.enter_context(run_cloud_app())
            stack.__dict__.update(new_stack.__dict__)
            return base_url

        try:
            yield HostedOnboardingEnv(
                base_url=base_url,
                supabase_url=supabase_url,
                anon_key=ANON_KEY,
                store=store,
                daemon=daemon,
                owner_token=mint_supabase_jwt(
                    OWNER_ONB_EMAIL, sub=OWNER_ONB_ID, ttl_minutes=30
                ),
                owner_email=OWNER_ONB_EMAIL,
                peer_token=mint_supabase_jwt(
                    PEER_ONB_EMAIL, sub=PEER_ONB_ID, ttl_minutes=30
                ),
                peer_email=PEER_ONB_EMAIL,
                peer_id=PEER_ONB_ID,
                wrong_token=mint_supabase_jwt(
                    WRONG_ONB_EMAIL, sub=WRONG_ONB_ID, ttl_minutes=30
                ),
                wrong_email=WRONG_ONB_EMAIL,
                domain=ONBOARDING_DOMAIN,
                domain_team=ONBOARDING_DOMAIN_TEAM,
                _restart=_restart,
            )
        finally:
            stack.close()
            app.dependency_overrides.pop(get_product_directory_repository, None)
