"""Production Cloud wire models for the Bot onboarding contract (cloud#136).

These are the Cloud production request/response models that serialize and
validate against the pinned bot#787 onboarding contract fixtures. They are
**not** a shadow canonical schema: the ``bicameral-bot`` repository stays the
canonical authority (see :mod:`bicameral_cloud.contracts.onboarding`), and
``tests/test_onboarding_contract.py`` binds every model to the vendored,
fingerprint-verified bot schema so a drifted shape fails closed.

Every model forbids unknown fields (``extra="forbid"``) so undeclared boundary
material is rejected rather than silently carried, mirroring the producer's
``additionalProperties: false`` / ``deny_unknown_fields``.
"""

from __future__ import annotations

from datetime import datetime
from enum import StrEnum
from typing import Literal

from pydantic import BaseModel, ConfigDict, field_serializer, model_validator

from bicameral_cloud.models.product_join_ticket import ProductJoinTicket

ProviderHintValue = Literal["github", "google", "unknown"]
SubstrateHintValue = Literal["git", "drive_folder", "unknown"]
CreatorRole = Literal["Policy Owner", "Product Owner"]


class _OnboardingModel(BaseModel):
    """Fail-closed base: undeclared fields are rejected at the boundary."""

    model_config = ConfigDict(extra="forbid")


def _iso_z(value: datetime) -> str:
    return value.isoformat().replace("+00:00", "Z")


class OnboardingEventStoreHint(_OnboardingModel):
    """Safe, non-secret routing labels; no provider credentials or paths."""

    substrate: SubstrateHintValue
    provider: ProviderHintValue


class RequiredConnection(_OnboardingModel):
    provider: Literal["github", "google", "none"]


class ProductCreationResult(_OnboardingModel):
    """Daemon-canonical result of ``product.create`` (creator binding source)."""

    product_id: str
    creator_cloud_account_id: str
    creator_roles: list[CreatorRole]
    membership_event_ref: str
    governance_policy_version: str
    receipt_id: str
    created_at: datetime

    @field_serializer("created_at", when_used="json")
    def _ser_created_at(self, value: datetime) -> str:
        return _iso_z(value)


class ProductCreationPublication(_OnboardingModel):
    """Cloud, non-canonical, retryable publication; idempotent by identity."""

    publication_id: str
    product_id: str
    creator_cloud_account_id: str
    team_display_name: str | None = None
    verified_domain: str | None = None
    event_store_hint: OnboardingEventStoreHint | None = None
    canonical: Literal[False]
    idempotency_key: str
    canonical_creation_receipt_ref: str
    published_at: datetime

    @field_serializer("published_at", when_used="json")
    def _ser_published_at(self, value: datetime) -> str:
        return _iso_z(value)


class HostedProjectPreview(_OnboardingModel):
    """Non-canonical Cloud directory projection for discovery/routing."""

    preview_id: str
    product_id: str
    product_display_name: str
    team_display_name: str | None = None
    verified_domain: str | None = None
    event_store_hint: OnboardingEventStoreHint | None = None
    required_connection: RequiredConnection | None = None
    canonical: Literal[False]
    expires_at: datetime

    @field_serializer("expires_at", when_used="json")
    def _ser_expires_at(self, value: datetime) -> str:
        return _iso_z(value)


class ProductJoinRequest(_OnboardingModel):
    """Daemon-inbound join request carrying a bounded ``ProductJoinTicket``."""

    request_id: str
    ticket: ProductJoinTicket
    local_actor_ref: str
    requested_at: datetime

    @field_serializer("requested_at", when_used="json")
    def _ser_requested_at(self, value: datetime) -> str:
        return _iso_z(value)


class JoinOutcome(StrEnum):
    GRANTED = "granted"
    REJECTED = "rejected"


class JoinFailureCode(StrEnum):
    INVALID_SIGNATURE = "invalid_signature"
    WRONG_AUDIENCE = "wrong_audience"
    WRONG_PRODUCT = "wrong_product"
    WRONG_ACCOUNT = "wrong_account"
    MISSING_PAIRING = "missing_pairing"
    WRONG_DOMAIN = "wrong_domain"
    POLICY_CHANGED = "policy_changed"
    EXPIRED_TICKET = "expired_ticket"
    NOT_YET_VALID = "not_yet_valid"
    REPLAYED_TICKET = "replayed_ticket"


class ProductJoinFailure(_OnboardingModel):
    code: JoinFailureCode
    message: str


class ProductJoinResult(_OnboardingModel):
    """Daemon-authoritative join outcome: Product Member only, or typed reject."""

    request_id: str
    product_id: str
    cloud_account_id: str
    outcome: JoinOutcome
    granted_role: Literal["Product Member"] | None = None
    membership_event_ref: str | None = None
    receipt_id: str | None = None
    failure: ProductJoinFailure | None = None
    decided_at: datetime

    @field_serializer("decided_at", when_used="json")
    def _ser_decided_at(self, value: datetime) -> str:
        return _iso_z(value)

    @model_validator(mode="after")
    def _enforce_outcome_invariants(self) -> ProductJoinResult:
        """Granted => Product Member + canonical refs, no failure; else reject."""
        if self.outcome is JoinOutcome.GRANTED:
            if self.granted_role != "Product Member":
                raise ValueError("granted result must grant Product Member only")
            if not self.membership_event_ref or not self.receipt_id:
                raise ValueError("granted result must carry canonical refs")
            if self.failure is not None:
                raise ValueError("granted result must not carry a failure")
        else:
            if self.failure is None:
                raise ValueError("rejected result must carry a typed failure")
            if (
                self.granted_role is not None
                or self.membership_event_ref is not None
                or self.receipt_id is not None
            ):
                raise ValueError("rejected result must not grant membership")
        return self


__all__ = [
    "CreatorRole",
    "HostedProjectPreview",
    "JoinFailureCode",
    "JoinOutcome",
    "OnboardingEventStoreHint",
    "ProductCreationPublication",
    "ProductCreationResult",
    "ProductJoinFailure",
    "ProductJoinRequest",
    "ProductJoinResult",
    "ProviderHintValue",
    "RequiredConnection",
    "SubstrateHintValue",
]
