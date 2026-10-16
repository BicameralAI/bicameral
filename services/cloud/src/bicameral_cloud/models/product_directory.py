"""Durable hosted Product directory models (cloud#125).

A **Product directory record** is a Cloud-owned coordination/projection row used
for Product discovery and routing. It is durable (Supabase-backed in production)
but **never canonical Product state**: the bot/daemon remains authoritative for
Product creation, membership, roles, policy, and events (ADR-0005/0006).

Publication is accepted only after the daemon has returned the canonical Product
ID and a creation receipt. Only safe metadata is persisted — display name,
canonical Product ID, eligible domain, an opaque ``registration_ref``, and
allowlisted provider/substrate hints. Provider credentials, raw event-store
descriptors, local filesystem paths, and unredacted actor data are rejected at
the boundary (``assert_no_cloud_leakage``) so they never enter Cloud.

Discovery eligibility (who may *discover* a Product, gated by verified email
domain) is deliberately kept separate from canonical Product membership: a
domain-eligible account is **not** inferred to be a Product Member.
"""

from __future__ import annotations

import base64
import binascii
import hashlib
import json
import re
from datetime import UTC, datetime
from typing import Any, Literal
from uuid import UUID

from pydantic import (
    BaseModel,
    ConfigDict,
    Field,
    ValidationInfo,
    field_validator,
    model_validator,
)

from bicameral_cloud.models.registration import assert_no_cloud_leakage


def _normalize_required_domain(value: str) -> str:
    normalized = value.strip().lower().lstrip("@")
    if not normalized:
        raise ValueError("eligible_domain must be a non-empty email domain")
    return normalized


class ProductCreationCommitProof(BaseModel):
    """Safe witness for provider-confirmed durability of ``CreateProduct``."""

    model_config = ConfigDict(extra="forbid")

    schema_version: Literal[2]
    attempt_id: str
    product_id: str
    event_id: str
    substrate: Literal["drive_folder"]
    resource_fingerprint: str
    provider_revision: str
    event_digest: str
    durability: Literal["provider_confirmed_remote"]
    confirmed_at: str
    proof_digest: str
    signer_key_id: str
    signature_algorithm: Literal["ed25519"]
    signature: str

    @field_validator(
        "attempt_id",
        "product_id",
        "provider_revision",
        "confirmed_at",
        "signer_key_id",
    )
    @classmethod
    def _required_text(cls, value: str) -> str:
        normalized = value.strip()
        if not normalized:
            raise ValueError("proof field must not be empty")
        return normalized

    @field_validator("event_id")
    @classmethod
    def _event_id_is_uuid(cls, value: str) -> str:
        UUID(value)
        return value

    @field_validator(
        "resource_fingerprint",
        "event_digest",
        "proof_digest",
    )
    @classmethod
    def _sha256_digest(cls, value: str) -> str:
        if not value.startswith("sha256:") or len(value) != 71:
            raise ValueError("proof digest must be sha256:<64 lowercase hex>")
        if any(ch not in "0123456789abcdef" for ch in value[7:]):
            raise ValueError("proof digest must be sha256:<64 lowercase hex>")
        return value

    @field_validator("signature")
    @classmethod
    def _ed25519_signature(cls, value: str) -> str:
        try:
            decoded = base64.b64decode(value, validate=True)
        except (ValueError, binascii.Error) as exc:
            raise ValueError("signature must be canonical base64") from exc
        if len(decoded) != 64:
            raise ValueError("Ed25519 signature must be 64 bytes")
        return value

    @field_validator("confirmed_at")
    @classmethod
    def _confirmed_at_z(cls, value: str) -> str:
        normalized = value.strip()
        if not re.fullmatch(
            r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z",
            normalized,
        ):
            raise ValueError("confirmed_at must be ISO-8601 with Z")
        try:
            datetime.fromisoformat(normalized.replace("Z", "+00:00"))
        except ValueError as exc:
            raise ValueError("confirmed_at must be a valid ISO-8601 instant") from exc
        return normalized

    @model_validator(mode="after")
    def _validate_confirmation_and_digest(self) -> ProductCreationCommitProof:
        material: dict[str, Any] = self.model_dump(
            exclude={
                "proof_digest",
                "signature",
            }
        )
        encoded = json.dumps(
            material,
            sort_keys=True,
            separators=(",", ":"),
            ensure_ascii=False,
        ).encode()
        expected = f"sha256:{hashlib.sha256(encoded).hexdigest()}"
        if self.proof_digest != expected:
            raise ValueError("proof_digest does not bind the complete proof")
        return self


class ProductPublication(BaseModel):
    """Validated intent to publish a Product into the durable directory.

    Created only after the daemon returned the canonical Product ID and a
    creation receipt. Carries only safe coordination metadata.
    """

    canonical_product_id: str
    display_name: str
    eligible_domain: str
    creation_receipt: str
    first_event_commit_proof: ProductCreationCommitProof
    registration_ref: str | None = None
    provider_hint: str | None = None
    substrate_hint: str | None = None

    @model_validator(mode="after")
    def _proof_matches_publication(self) -> ProductPublication:
        proof = self.first_event_commit_proof
        if proof.product_id != self.canonical_product_id:
            raise ValueError(
                "first-event proof does not match canonical_product_id"
            )
        if self.substrate_hint is not None and proof.substrate != self.substrate_hint:
            raise ValueError("first-event proof does not match substrate_hint")
        return self

    @field_validator("canonical_product_id", "creation_receipt")
    @classmethod
    def _non_empty(cls, v: str) -> str:
        v = v.strip()
        if not v:
            raise ValueError("value must not be empty")
        return v

    @field_validator("display_name")
    @classmethod
    def _validate_display_name(cls, v: str) -> str:
        v = v.strip()
        if not v:
            raise ValueError("display_name must not be empty")
        return assert_no_cloud_leakage(v, field="display_name")

    @field_validator("eligible_domain")
    @classmethod
    def _validate_domain(cls, v: str) -> str:
        return _normalize_required_domain(v)

    @field_validator("registration_ref", "provider_hint", "substrate_hint")
    @classmethod
    def _validate_optional_label(
        cls, v: str | None, info: ValidationInfo
    ) -> str | None:
        if v is None:
            return None
        v = v.strip()
        if not v:
            return None
        return assert_no_cloud_leakage(v, field=info.field_name or "value")


class ProductDirectoryRecord(BaseModel):
    """A durable, non-canonical Cloud Product directory row.

    Shaped to mirror the ``public.product_directory`` table. Persisted server
    side only; never returned verbatim to the browser (see ``HostedProjectPreview``).
    """

    canonical_product_id: str
    display_name: str
    eligible_domain: str
    registration_ref: str | None = None
    provider_hint: str | None = None
    substrate_hint: str | None = None
    creation_commit_proof_digest: str | None = None
    creation_event_id: str | None = None
    creation_event_digest: str | None = None
    enabled: bool = True
    created_at: datetime = Field(default_factory=lambda: datetime.now(UTC))
    updated_at: datetime = Field(default_factory=lambda: datetime.now(UTC))

    @field_validator("eligible_domain")
    @classmethod
    def _validate_domain(cls, value: str) -> str:
        return _normalize_required_domain(value)

    @field_validator("creation_event_id")
    @classmethod
    def _validate_creation_event_id(cls, value: str | None) -> str | None:
        if value is not None:
            UUID(value)
        return value

    @field_validator("creation_commit_proof_digest", "creation_event_digest")
    @classmethod
    def _validate_optional_digest(cls, value: str | None) -> str | None:
        if value is None:
            return None
        if not value.startswith("sha256:") or len(value) != 71:
            raise ValueError("creation digest must be sha256:<64 lowercase hex>")
        if any(ch not in "0123456789abcdef" for ch in value[7:]):
            raise ValueError("creation digest must be sha256:<64 lowercase hex>")
        return value


class ProductPublicationReceipt(BaseModel):
    """Safe response for an accepted Product publication.

    Confirms the durable directory row without echoing the creation receipt or
    any provider/substrate secret material.
    """

    canonical_product_id: str
    display_name: str
    eligible_domain: str
    idempotent: bool
