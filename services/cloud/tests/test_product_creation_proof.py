"""Unit tests for ProductCreationCommitProof verification (cloud#227)."""

from __future__ import annotations

import base64
import hashlib
import json
from pathlib import Path

import pytest
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

from bicameral_cloud.config import CloudSettings
from bicameral_cloud.models.product_directory import ProductCreationCommitProof
from tests.proof_fixtures import first_event_proof, v1_lab_digest_hex
from bicameral_cloud.product_creation_proof import (
    Ed25519ProductCreationProofVerifier,
    ProductCreationProofVerificationError,
    ProductCreationProofVerifierUnavailable,
)


def _settings_with_keys(keys_json: str | None) -> CloudSettings:
    return CloudSettings(
        supabase_url=None,
        supabase_jwt_secret=None,
        supabase_anon_key=None,
        supabase_service_key=None,
        product_creation_proof_verification_keys_json=keys_json,
    )


def _mint_proof(private_key: Ed25519PrivateKey, key_id: str) -> ProductCreationCommitProof:
    material = {
        "schema_version": 2,
        "attempt_id": "attempt-1",
        "product_id": "prod_test",
        "event_id": "00000000-0000-4000-8000-000000000001",
        "substrate": "drive_folder",
        "resource_fingerprint": "sha256:" + ("a" * 64),
        "provider_revision": "rev-1",
        "event_digest": "sha256:" + ("b" * 64),
        "durability": "provider_confirmed_remote",
        "confirmed_at": "2026-08-03T00:00:00Z",
        "signer_key_id": key_id,
        "signature_algorithm": "ed25519",
    }
    encoded = json.dumps(
        material, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode()
    proof_digest = f"sha256:{hashlib.sha256(encoded).hexdigest()}"
    digest_bytes = bytes.fromhex(proof_digest.removeprefix("sha256:"))
    signature = base64.b64encode(private_key.sign(digest_bytes)).decode()
    return ProductCreationCommitProof(
        **material,
        proof_digest=proof_digest,
        signature=signature,
    )


def test_verifier_unavailable_without_keys() -> None:
    with pytest.raises(ProductCreationProofVerifierUnavailable):
        Ed25519ProductCreationProofVerifier.from_settings(_settings_with_keys(None))


def test_valid_proof_accepts() -> None:
    private = Ed25519PrivateKey.generate()
    public_b64 = base64.b64encode(private.public_key().public_bytes_raw()).decode()
    key_id = "sha256:" + ("c" * 64)
    keys = json.dumps(
        [{"key_id": key_id, "public_key_b64": public_b64, "state": "active"}]
    )
    verifier = Ed25519ProductCreationProofVerifier.from_settings(
        _settings_with_keys(keys)
    )
    proof = _mint_proof(private, key_id)
    verifier.verify(proof)  # does not raise


def test_invalid_signature_rejects() -> None:
    private = Ed25519PrivateKey.generate()
    other = Ed25519PrivateKey.generate()
    public_b64 = base64.b64encode(private.public_key().public_bytes_raw()).decode()
    key_id = "sha256:" + ("d" * 64)
    keys = json.dumps(
        [{"key_id": key_id, "public_key_b64": public_b64, "state": "active"}]
    )
    verifier = Ed25519ProductCreationProofVerifier.from_settings(
        _settings_with_keys(keys)
    )
    # signed with other key but claims trusted key_id
    proof = _mint_proof(other, key_id)
    with pytest.raises(ProductCreationProofVerificationError):
        verifier.verify(proof)


def test_v1_lab_digest_is_rejected_by_v2_model() -> None:
    private = Ed25519PrivateKey.generate()
    key_id = "sha256:" + ("e" * 64)
    proof = _mint_proof(private, key_id)
    v1 = v1_lab_digest_hex(proof)
    dumped = proof.model_dump()
    dumped["proof_digest"] = f"sha256:{v1}"
    dumped["signature"] = proof.signature
    with pytest.raises(Exception):
        ProductCreationCommitProof.model_validate(dumped)


def test_v2_git_substrate_is_rejected() -> None:
    proof = first_event_proof("prod-git")
    dumped = proof.model_dump()
    dumped["substrate"] = "git"
    with pytest.raises(Exception):
        ProductCreationCommitProof.model_validate(dumped)


def test_confirmed_at_rejects_offset_and_naive() -> None:
    proof = first_event_proof("prod-z")
    dumped = proof.model_dump()
    dumped["confirmed_at"] = "2026-07-26T06:00:00+00:00"
    with pytest.raises(Exception):
        ProductCreationCommitProof.model_validate(dumped)
    dumped["confirmed_at"] = "2026-07-26T06:00:00"
    with pytest.raises(Exception):
        ProductCreationCommitProof.model_validate(dumped)
    dumped["confirmed_at"] = "2026-02-30T00:00:00Z"
    with pytest.raises(Exception):
        ProductCreationCommitProof.model_validate(dumped)


def test_shared_v2_golden_vector_matches_literal_bytes_digest_and_signature() -> None:
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

    golden_path = Path(__file__).parent / "vectors" / "product-creation-commit-proof-v2.golden.json"
    golden = json.loads(golden_path.read_text())
    proof = ProductCreationCommitProof.model_validate(golden["proof"])
    material = proof.model_dump(exclude={"proof_digest", "signature"})
    encoded = json.dumps(
        material, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode()
    assert encoded.decode() == golden["canonical_json"]
    assert f"sha256:{hashlib.sha256(encoded).hexdigest()}" == golden["proof_digest"]
    assert proof.proof_digest == golden["proof_digest"]
    public = Ed25519PublicKey.from_public_bytes(base64.b64decode(golden["public_key_b64"]))
    public.verify(
        base64.b64decode(golden["signature"]),
        bytes.fromhex(golden["proof_digest"].removeprefix("sha256:")),
    )
