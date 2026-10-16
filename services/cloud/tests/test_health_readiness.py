from __future__ import annotations

import base64
import json

from fastapi.testclient import TestClient

from bicameral_cloud.app import app
from bicameral_cloud.config import CloudSettings, reset_settings

ISSUER = "https://app.bicameral-ai.com"
ACTIVE_KEY_ID = "join-key-1"
ACTIVE_SEED = b"\x33" * 32
PROOF_KEY_ID = "proof-key-1"
PROOF_PUBLIC_KEY = b"\x44" * 32
ENV_VAR_NAMES = (
    "PRODUCT_JOIN_TICKET_ISSUER",
    "PRODUCT_JOIN_SIGNING_KEY_ID",
    "PRODUCT_JOIN_SIGNING_PRIVATE_KEY_B64",
    "PRODUCT_CREATION_PROOF_VERIFICATION_KEYS_JSON",
)


def _proof_keys_json() -> str:
    return json.dumps(
        [
            {
                "key_id": PROOF_KEY_ID,
                "public_key_b64": base64.b64encode(PROOF_PUBLIC_KEY).decode(),
                "state": "active",
            }
        ]
    )


def _apply_settings(*, join_configured: bool) -> None:
    seed_b64 = base64.b64encode(ACTIVE_SEED).decode()
    reset_settings(
        CloudSettings(
            supabase_url=None,
            supabase_jwt_secret=None,
            supabase_anon_key=None,
            supabase_service_key=None,
            product_join_ticket_issuer=ISSUER if join_configured else None,
            product_join_signing_key_id=ACTIVE_KEY_ID if join_configured else None,
            product_join_signing_private_key_b64=(seed_b64 if join_configured else None),
            product_creation_proof_verification_keys_json=_proof_keys_json(),
        )
    )


def test_ready_returns_503_and_names_unready_capability() -> None:
    _apply_settings(join_configured=False)
    response = TestClient(app).get("/health/ready")

    assert response.status_code == 503
    detail = response.json()["detail"]
    assert detail["status"] == "not_ready"
    assert detail["capabilities"]["product_join"] is False
    assert detail["capabilities"]["product_creation_proof"] is True


def test_ready_returns_200_when_every_capability_is_configured() -> None:
    _apply_settings(join_configured=True)
    response = TestClient(app).get("/health/ready")

    assert response.status_code == 200
    body = response.json()
    assert body["status"] == "ready"
    assert all(body["capabilities"].values())


def test_ready_payload_discloses_no_configuration_values() -> None:
    _apply_settings(join_configured=True)
    text = TestClient(app).get("/health/ready").text

    for name in ENV_VAR_NAMES:
        assert name not in text
    assert ISSUER not in text
    assert ACTIVE_KEY_ID not in text
    assert base64.b64encode(ACTIVE_SEED).decode() not in text
    assert PROOF_KEY_ID not in text


def test_liveness_still_answers_when_readiness_fails() -> None:
    _apply_settings(join_configured=False)
    client = TestClient(app)

    assert client.get("/health").status_code == 200
    assert client.get("/health/ready").status_code == 503
