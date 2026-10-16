"""Authentication boundary for daemon Product-creation commit proofs."""

from __future__ import annotations

import base64
import binascii
import json
from dataclasses import dataclass

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

from bicameral_cloud.config import CloudSettings
from bicameral_cloud.models.pairing import DaemonPairing
from bicameral_cloud.models.product_directory import ProductCreationCommitProof


class ProductCreationProofVerificationError(RuntimeError):
    """The proof is malformed, untrusted, or has an invalid signature."""


class ProductCreationProofVerifierUnavailable(RuntimeError):
    """Cloud has no usable daemon proof trust configuration."""


@dataclass(frozen=True)
class ProductCreationProofVerificationKey:
    key_id: str
    public_key: bytes
    state: str


class Ed25519ProductCreationProofVerifier:
    def __init__(
        self, keys: tuple[ProductCreationProofVerificationKey, ...]
    ) -> None:
        self._keys = {
            key.key_id: key
            for key in keys
            if key.state in {"active", "retiring"}
        }
        if not self._keys:
            raise ProductCreationProofVerifierUnavailable(
                "no active Product creation proof verification key is configured"
            )

    @classmethod
    def from_settings(
        cls, settings: CloudSettings
    ) -> Ed25519ProductCreationProofVerifier:
        raw = settings.product_creation_proof_verification_keys_json
        if not raw:
            raise ProductCreationProofVerifierUnavailable(
                "Product creation proof verification is not configured"
            )
        try:
            payload: object = json.loads(raw)
            if not isinstance(payload, list):
                raise ValueError("verification keys must be a list")
            keys: list[ProductCreationProofVerificationKey] = []
            for item in payload:
                if not isinstance(item, dict):
                    raise ValueError("verification key must be an object")
                key_id = item.get("key_id")
                public_key_b64 = item.get("public_key_b64")
                state = item.get("state")
                if (
                    not isinstance(key_id, str)
                    or not isinstance(public_key_b64, str)
                    or not isinstance(state, str)
                ):
                    raise ValueError("verification key fields are invalid")
                public_key = base64.b64decode(public_key_b64, validate=True)
                if len(public_key) != 32:
                    raise ValueError("Ed25519 public keys must be 32 bytes")
                keys.append(
                    ProductCreationProofVerificationKey(
                        key_id=key_id,
                        public_key=public_key,
                        state=state,
                    )
                )
        except (ValueError, TypeError, binascii.Error, json.JSONDecodeError) as exc:
            raise ProductCreationProofVerifierUnavailable(
                "Product creation proof verification configuration is invalid"
            ) from exc
        return cls(tuple(keys))

    @classmethod
    def from_enrolled_pairings(
        cls, pairings: list[DaemonPairing]
    ) -> Ed25519ProductCreationProofVerifier:
        """Trust store is this caller's enrolled daemon identity keys only."""
        keys: list[ProductCreationProofVerificationKey] = []
        for pairing in pairings:
            if not pairing.identity_key_id or not pairing.identity_public_key_b64:
                continue
            try:
                public_key = base64.b64decode(
                    pairing.identity_public_key_b64, validate=True
                )
            except (ValueError, binascii.Error) as exc:
                raise ProductCreationProofVerifierUnavailable(
                    "enrolled daemon identity public key is invalid"
                ) from exc
            if len(public_key) != 32:
                raise ProductCreationProofVerifierUnavailable(
                    "enrolled daemon identity public key is invalid"
                )
            keys.append(
                ProductCreationProofVerificationKey(
                    key_id=pairing.identity_key_id,
                    public_key=public_key,
                    state="active",
                )
            )
        return cls(tuple(keys))

    def verify(self, proof: ProductCreationCommitProof) -> None:
        key = self._keys.get(proof.signer_key_id)
        if key is None:
            raise ProductCreationProofVerificationError(
                "Product creation proof signer is not trusted"
            )
        try:
            signature = base64.b64decode(proof.signature, validate=True)
            digest = bytes.fromhex(proof.proof_digest.removeprefix("sha256:"))
            Ed25519PublicKey.from_public_bytes(key.public_key).verify(
                signature, digest
            )
        except (InvalidSignature, ValueError, binascii.Error) as exc:
            raise ProductCreationProofVerificationError(
                "Product creation proof signature is invalid"
            ) from exc


__all__ = [
    "Ed25519ProductCreationProofVerifier",
    "ProductCreationProofVerificationError",
    "ProductCreationProofVerificationKey",
    "ProductCreationProofVerifierUnavailable",
]
