from __future__ import annotations

import base64
import json

from bicameral_cloud.capability_readiness import REQUIRED_CAPABILITIES, capability_status
from bicameral_cloud.config import CloudSettings

ISSUER = "https://app.bicameral-ai.com"
ACTIVE_KEY_ID = "join-key-1"
ACTIVE_SEED = b"\x11" * 32
PROOF_KEY_ID = "proof-key-1"
PROOF_PUBLIC_KEY = b"\x22" * 32


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


def _settings(
    *,
    issuer: str | None = None,
    key_id: str | None = None,
    private_key_b64: str | None = None,
    proof_keys_json: str | None = None,
) -> CloudSettings:
    return CloudSettings(
        supabase_url=None,
        supabase_jwt_secret=None,
        supabase_anon_key=None,
        supabase_service_key=None,
        product_join_ticket_issuer=issuer,
        product_join_signing_key_id=key_id,
        product_join_signing_private_key_b64=private_key_b64,
        product_creation_proof_verification_keys_json=proof_keys_json,
    )


def _configured_join_settings(**overrides: str | None) -> CloudSettings:
    values: dict[str, str | None] = {
        "issuer": ISSUER,
        "key_id": ACTIVE_KEY_ID,
        "private_key_b64": base64.b64encode(ACTIVE_SEED).decode(),
    }
    values.update(overrides)
    return _settings(**values)


def test_product_join_is_unready_when_signing_config_is_absent() -> None:
    assert capability_status(_settings())["product_join"] is False


def test_product_join_is_ready_with_valid_signing_configuration() -> None:
    assert capability_status(_configured_join_settings())["product_join"] is True


def test_product_join_is_unready_when_private_key_is_malformed() -> None:
    status = capability_status(
        _configured_join_settings(private_key_b64="not-valid-base64!!")
    )
    assert status["product_join"] is False


def test_product_creation_proof_is_unready_without_verification_keys() -> None:
    assert capability_status(_settings())["product_creation_proof"] is False


def test_product_creation_proof_is_ready_with_verification_keys() -> None:
    status = capability_status(_settings(proof_keys_json=_proof_keys_json()))
    assert status["product_creation_proof"] is True


def test_capability_status_exposes_only_names_and_booleans() -> None:
    settings = _configured_join_settings(proof_keys_json=_proof_keys_json())
    status = capability_status(settings)
    serialized = json.dumps(status)

    assert all(isinstance(value, bool) for value in status.values())
    assert ISSUER not in serialized
    assert ACTIVE_KEY_ID not in serialized
    assert base64.b64encode(ACTIVE_SEED).decode() not in serialized
    assert PROOF_KEY_ID not in serialized


def test_every_declared_capability_is_probed() -> None:
    status = capability_status(_settings())
    assert set(status) == {requirement.name for requirement in REQUIRED_CAPABILITIES}
