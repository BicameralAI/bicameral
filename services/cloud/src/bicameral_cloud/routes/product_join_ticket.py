"""Authenticated bounded Product join-ticket issuance (cloud#126)."""

from __future__ import annotations

import logging
import uuid
from datetime import UTC, datetime, timedelta

from fastapi import APIRouter, Depends, Header, HTTPException, status

from bicameral_cloud.auth.dependencies import get_current_user
from bicameral_cloud.config import CloudSettings, get_settings
from bicameral_cloud.models.product_join_ticket import (
    ProductJoinTicket,
    ProductJoinVerificationMetadata,
    ProviderHint,
    SignedProductJoinTicket,
    SubstrateHint,
)
from bicameral_cloud.models.tenancy import AuthenticatedUser
from bicameral_cloud.product_directory_repository import (
    ProductDirectoryRepository,
    ProductDirectoryUnavailable,
    get_product_directory_repository,
)
from bicameral_cloud.product_join_ticket import (
    ProductJoinSignerError,
    ProductJoinSignerUnavailable,
    ProductJoinTicketSigner,
    get_product_join_ticket_signer,
)
from bicameral_cloud.store import Store, get_store

router = APIRouter(prefix="/api/v1", tags=["product-join"])
logger = logging.getLogger(__name__)


def _error(status_code: int, code: str, message: str) -> HTTPException:
    return HTTPException(
        status_code=status_code,
        detail={"code": code, "message": message},
    )


async def get_join_ticket_user(
    authorization: str | None = Header(None, alias="Authorization"),
    store: Store = Depends(get_store),  # noqa: B008
    settings: CloudSettings = Depends(get_settings),  # noqa: B008
) -> AuthenticatedUser:
    """Endpoint-specific auth mapping with typed, redacted errors."""

    try:
        return await get_current_user(
            authorization=authorization or "",
            store=store,
            settings=settings,
        )
    except HTTPException as exc:
        if exc.status_code == status.HTTP_401_UNAUTHORIZED:
            raise _error(
                status.HTTP_401_UNAUTHORIZED,
                "authentication_required",
                "A valid hosted session is required",
            ) from exc
        if exc.status_code == status.HTTP_403_FORBIDDEN:
            raise _error(
                status.HTTP_403_FORBIDDEN,
                "account_ineligible",
                "The authenticated account is not eligible",
            ) from exc
        raise _error(
            status.HTTP_503_SERVICE_UNAVAILABLE,
            "identity_service_unavailable",
            "Hosted identity verification is unavailable",
        ) from exc


def get_join_ticket_signer_dependency() -> ProductJoinTicketSigner:
    """Resolve the signer while preserving a typed public failure."""

    try:
        return get_product_join_ticket_signer()
    except ProductJoinSignerUnavailable as exc:
        raise _error(
            status.HTTP_503_SERVICE_UNAVAILABLE,
            "ticket_signer_unavailable",
            "Product join-ticket issuance is unavailable",
        ) from exc


def _provider_hint(value: str | None) -> ProviderHint | None:
    try:
        return ProviderHint(value) if value is not None else None
    except ValueError:
        return None


def _substrate_hint(value: str | None) -> SubstrateHint | None:
    try:
        return SubstrateHint(value) if value is not None else None
    except ValueError:
        return None


@router.post(
    "/projects/{product_id}/join-ticket",
    response_model=SignedProductJoinTicket,
    summary="Issue a bounded Product join ticket",
)
@router.post(
    "/products/{product_id}/join-ticket",
    response_model=SignedProductJoinTicket,
    include_in_schema=False,
)
async def issue_product_join_ticket(
    product_id: str,
    user: AuthenticatedUser = Depends(get_join_ticket_user),  # noqa: B008
    directory: ProductDirectoryRepository = Depends(  # noqa: B008
        get_product_directory_repository
    ),
    signer: ProductJoinTicketSigner = Depends(  # noqa: B008
        get_join_ticket_signer_dependency
    ),
) -> SignedProductJoinTicket:
    """Exchange verified domain eligibility for one short-lived assertion.

    Issuance is ephemeral: it reads the non-canonical Product directory and
    creates no membership, pending approval, join request, or other durable
    lifecycle state. The daemon independently verifies the signature and
    canonical policy before it may materialize Product Member.
    """

    if not user.email_verified or not user.verified_email_domain:
        raise _error(
            status.HTTP_403_FORBIDDEN,
            "email_unverified",
            "A verified email domain is required",
        )

    try:
        record = directory.get_by_canonical_id(product_id)
    except ProductDirectoryUnavailable as exc:
        raise _error(
            status.HTTP_503_SERVICE_UNAVAILABLE,
            "product_directory_unavailable",
            "Product eligibility lookup is unavailable",
        ) from exc

    if record is None:
        raise _error(
            status.HTTP_404_NOT_FOUND,
            "product_not_found",
            "The requested Product was not found",
        )
    if not record.enabled:
        raise _error(
            status.HTTP_409_CONFLICT,
            "product_disabled",
            "The requested Product is not accepting joins",
        )
    if record.eligible_domain != user.verified_email_domain:
        raise _error(
            status.HTTP_403_FORBIDDEN,
            "product_ineligible",
            "The authenticated account is not eligible for this Product",
        )
    if not record.registration_ref:
        raise _error(
            status.HTTP_409_CONFLICT,
            "product_not_joinable",
            "The requested Product is not ready for joining",
        )

    issued_at = datetime.now(UTC).replace(microsecond=0)
    ticket = ProductJoinTicket(
        issuer=signer.issuer,
        audience=f"daemon:{record.canonical_product_id}",
        product_id=record.canonical_product_id,
        cloud_account_id=user.user_id,
        verified_domain=user.verified_email_domain,
        issued_at=issued_at,
        expiry=issued_at + timedelta(seconds=signer.ticket_ttl_seconds),
        nonce=f"jti_{uuid.uuid4().hex}",
        registration_ref=record.registration_ref,
        provider_hint=_provider_hint(record.provider_hint),
        substrate_hint=_substrate_hint(record.substrate_hint),
    )
    try:
        return signer.sign(ticket)
    except ProductJoinSignerError as exc:
        logger.error("product_join_ticket_signing_failed")
        raise _error(
            status.HTTP_503_SERVICE_UNAVAILABLE,
            "ticket_signer_unavailable",
            "Product join-ticket issuance is unavailable",
        ) from exc


@router.get(
    "/product-join/verification-keys",
    response_model=ProductJoinVerificationMetadata,
    summary="Get versioned Product join-ticket verification metadata",
)
async def get_product_join_verification_keys(
    signer: ProductJoinTicketSigner = Depends(  # noqa: B008
        get_join_ticket_signer_dependency
    ),
) -> ProductJoinVerificationMetadata:
    """Publish active and overlap public keys; retired keys fail closed."""

    return signer.verification_metadata()
