"""Cloud dashboard ↔ local daemon pending-pairing conformance harness.

Exercises the bot#790 pending-first contract end to end against an in-process
fake daemon: Cloud initiates a pairing request (``202`` pending, no token), the
daemon is the sole approval authority, and Cloud polls for a transient token that
is released only on approval and never persisted, logged, or placed in a URL.
"""

from __future__ import annotations

import json
from collections.abc import Callable, Iterator

import pytest
from fastapi.testclient import TestClient

from bicameral_cloud.config import CloudSettings, reset_settings
from bicameral_cloud.models import discovery
from bicameral_cloud.store import reset_store
from tests.conformance.bridge_client import (
    initiate_pairing,
    poll_pairing_status,
    run_pairing,
)
from tests.conformance.fake_daemon import closed_loopback_url, run_fake_daemon
from tests.conformance.fake_supabase import (
    FAKE_JWT_SECRET,
    mint_supabase_jwt,
    run_fake_supabase,
)

ALLOWED_DESIGN_PARTNER_DOMAIN = "bicameral-ai.com"
CLOUD_ORIGIN = "https://app.bicameral-ai.com"

pytestmark = pytest.mark.conformance

FORBIDDEN_OUTPUT_TOKENS = set(discovery.FORBIDDEN_DISCOVERY_FIELDS) | {
    'provider_secret',
    'api_key',
    'refresh_token',
    'jwt_secret',
    'service_key',
    'supabase_service_key',
    'source_body',
    'source_content',
    'decision_ledger_payload',
    'ledger_entry',
    'event_store',
    'events.jsonl',
    'graph_index',
    '.bicameral/',
    '/home/',
    'local_path',
    'filesystem_path',
    'workspace_path',
}


@pytest.fixture()
def cloud_auth_client() -> Iterator[tuple[TestClient, Callable[[str], str]]]:
    reset_store()
    with run_fake_supabase({ALLOWED_DESIGN_PARTNER_DOMAIN}) as supabase_url:
        settings = reset_settings(
            CloudSettings(
                supabase_url=supabase_url,
                supabase_jwt_secret=FAKE_JWT_SECRET,
                supabase_anon_key='test-anon-key',
                supabase_service_key='fake-supabase-role-token',
            )
        )
        assert settings.supabase_browser_auth_configured
        assert settings.supabase_design_partner_allowlist_enabled

        from bicameral_cloud.app import app

        client = TestClient(app)

        def mint_jwt(email: str, sub: str = 'user-123') -> str:
            return mint_supabase_jwt(email, sub=sub)

        yield client, mint_jwt


def _contains_forbidden_token(surface: object, forbidden: set[str]) -> bool:
    text = (
        json.dumps(surface, sort_keys=True, default=str)
        if not isinstance(surface, str)
        else surface
    )
    lowered = text.lower()
    return any(token.lower() in lowered for token in forbidden)


def test_authenticated_allowlisted_dashboard_loads(
    cloud_auth_client: tuple[TestClient, Callable[[str], str]]
) -> None:
    client, mint_jwt = cloud_auth_client

    config_response = client.get('/api/v1/auth/config')
    assert config_response.status_code == 200
    config = config_response.json()
    assert config['supabase_browser_auth_configured'] is True
    assert 'supabase_service_key' not in config_response.text
    assert 'test-jwt-secret' not in config_response.text

    token = mint_jwt('operator@bicameral-ai.com')
    session_response = client.get(
        '/api/v1/auth/session',
        headers={'Authorization': f'Bearer {token}'},
    )
    assert session_response.status_code == 200
    session = session_response.json()
    assert session['email'] == 'operator@bicameral-ai.com'
    assert session['user_id'] == 'user-123'

    dashboard_response = client.get('/connect')
    assert dashboard_response.status_code == 200
    assert 'text/html' in dashboard_response.headers['content-type']
    assert 'Frictionless user entry' in dashboard_response.text
    assert 'Checking for Bicameral' in dashboard_response.text
    assert 'Install Bicameral' in dashboard_response.text
    assert 'I already installed it — try again' in dashboard_response.text
    assert 'Developer / troubleshooting options' in dashboard_response.text


def test_non_allowlisted_domain_rejected(
    cloud_auth_client: tuple[TestClient, Callable[[str], str]]
) -> None:
    client, mint_jwt = cloud_auth_client
    token = mint_jwt('operator@example.com')

    response = client.get(
        '/api/v1/auth/session',
        headers={'Authorization': f'Bearer {token}'},
    )

    assert response.status_code == 403


def test_pair_request_is_pending_first_and_tokenless() -> None:
    """POST /pair returns a pending envelope and never a session token."""
    with run_fake_daemon('approves') as daemon_base_url:
        initiated = initiate_pairing(
            daemon_base_url,
            origin=CLOUD_ORIGIN,
            cloud_account_id='cloud-account-123',
        )

    assert initiated.state == 'pending'
    assert initiated.session_token is None
    assert initiated.pairing_code == '7K2M-PQ8R'
    assert initiated.confirmation is not None
    assert initiated.confirmation['requesting_origin'] == CLOUD_ORIGIN
    summary = initiated.confirmation['capability_summary']
    assert summary['allowed']
    assert summary['forbidden']


def test_local_approval_releases_transient_token_once() -> None:
    """After local approval, the status poll yields the transient handoff token."""
    with run_fake_daemon('approves') as daemon_base_url:
        outcome = run_pairing(
            daemon_base_url,
            origin=CLOUD_ORIGIN,
            cloud_account_id='cloud-account-123',
        )

    assert outcome.state == 'approved'
    assert outcome.session_token == 'transient-session-token'
    assert outcome.expires_at is not None
    assert not _contains_forbidden_token(
        {
            'session_token': outcome.session_token,
            'capabilities': outcome.capabilities,
        },
        FORBIDDEN_OUTPUT_TOKENS,
    )


def test_status_poll_sends_origin_handle_and_proof() -> None:
    """A mismatched handle/proof is rejected; the correct triple is accepted."""
    with run_fake_daemon('approves') as daemon_base_url:
        rejected = poll_pairing_status(
            daemon_base_url,
            request_handle='wrong-handle',
            poll_proof='wrong-proof',
            cloud_account_id='cloud-account-123',
            origin=CLOUD_ORIGIN,
        )
        assert rejected.state == 'unavailable'

        approved = poll_pairing_status(
            daemon_base_url,
            cloud_account_id='cloud-account-123',
            origin=CLOUD_ORIGIN,
        )
        assert approved.state == 'approved'


def test_pairing_denied_maps_to_denied_recovery() -> None:
    with run_fake_daemon('denies') as daemon_base_url:
        outcome = run_pairing(
            daemon_base_url,
            origin=CLOUD_ORIGIN,
            cloud_account_id='cloud-account-123',
        )

    assert outcome.state == 'denied'
    assert outcome.session_token is None


def test_pairing_expiry_and_revocation_map_to_recovery() -> None:
    with run_fake_daemon('expires') as daemon_base_url:
        expired = run_pairing(
            daemon_base_url,
            origin=CLOUD_ORIGIN,
            cloud_account_id='cloud-account-123',
        )
    assert expired.state == 'expired'
    assert expired.session_token is None

    with run_fake_daemon('revoked') as daemon_base_url:
        revoked = run_pairing(
            daemon_base_url,
            origin=CLOUD_ORIGIN,
            cloud_account_id='cloud-account-123',
        )
    assert revoked.state == 'revoked'
    assert revoked.session_token is None


def test_incompatible_daemon_requires_update() -> None:
    """A daemon below the pending-pairing floor yields update-required."""
    with run_fake_daemon('incompatible') as daemon_base_url:
        outcome = run_pairing(
            daemon_base_url,
            origin=CLOUD_ORIGIN,
            cloud_account_id='cloud-account-123',
        )

    assert outcome.state == 'update_required'
    assert outcome.session_token is None


def test_old_client_protocol_is_rejected_not_immediately_paired() -> None:
    """Declaring an older protocol must never yield an immediate token."""
    with run_fake_daemon('approves') as daemon_base_url:
        outcome = initiate_pairing(
            daemon_base_url,
            origin=CLOUD_ORIGIN,
            cloud_account_id='cloud-account-123',
            client_protocol_version=2,
        )

    assert outcome.state == 'update_required'
    assert outcome.session_token is None


def test_daemon_unavailable_before_pairing() -> None:
    outcome = initiate_pairing(
        closed_loopback_url(),
        origin=CLOUD_ORIGIN,
        cloud_account_id='cloud-account-123',
    )

    assert outcome.state == 'unavailable'
    assert outcome.session_token is None


def test_daemon_500_renders_repair_not_raw_error() -> None:
    with run_fake_daemon('error_500') as daemon_base_url:
        outcome = initiate_pairing(
            daemon_base_url,
            origin=CLOUD_ORIGIN,
            cloud_account_id='cloud-account-123',
        )

    assert outcome.state == 'repair'
    assert outcome.repair_hint == (
        'Check that the daemon is running and hosted bridge is enabled.'
    )
    assert outcome.repair_message == 'Daemon rejected the pairing request (500).'
    for leaky_text in ('Traceback', 'events.jsonl', '/home/'):
        assert leaky_text not in (outcome.repair_message or '')
        assert leaky_text not in (outcome.repair_hint or '')


def test_no_local_or_sensitive_data_leaks_to_browser_output(
    cloud_auth_client: tuple[TestClient, Callable[[str], str]],
) -> None:
    client, mint_jwt = cloud_auth_client
    token = mint_jwt('operator@bicameral-ai.com')
    auth_headers = {'Authorization': f'Bearer {token}'}

    surfaces: list[object] = []
    surfaces.append(client.get('/connect').text)
    surfaces.append(client.get('/api/v1/auth/config').json())
    surfaces.append(client.get('/api/v1/auth/session', headers=auth_headers).json())

    with run_fake_daemon('approves') as daemon_base_url:
        outcome = run_pairing(
            daemon_base_url,
            origin=CLOUD_ORIGIN,
            cloud_account_id='cloud-account-123',
        )
        surfaces.append(outcome.confirmation)
        surfaces.append(outcome.capabilities)

    assert not any(
        _contains_forbidden_token(surface, FORBIDDEN_OUTPUT_TOKENS)
        for surface in surfaces
    )
