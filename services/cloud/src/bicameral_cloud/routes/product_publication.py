"""Durable Product publication route (cloud#125).

Publishes a Product into the durable directory *after* the daemon has returned
the canonical Product ID and a creation receipt. Publication is idempotent for
the same canonical Product ID and never silently remaps an existing identifier.

Authority boundary: this only records safe discovery/routing metadata. It does
not write canonical Product membership or roles, does not create a join-request
queue, and does not issue signed join tickets (all out of scope, ADR-0005/0006).
"""

from __future__ import annotations

from fastapi import APIRouter, Depends, HTTPException, Response, status

from bicameral_cloud.auth.dependencies import get_current_user
from bicameral_cloud.models.product_directory import (
    ProductPublication,
    ProductPublicationReceipt,
)
from bicameral_cloud.models.tenancy import AuthenticatedUser
from bicameral_cloud.product_creation_proof import (
    Ed25519ProductCreationProofVerifier,
    ProductCreationProofVerificationError,
    ProductCreationProofVerifierUnavailable,
)
from bicameral_cloud.product_directory_repository import (
    ProductDirectoryRepository,
    ProductDirectoryUnavailable,
    ProductIdRemapError,
    get_product_directory_repository,
)
from bicameral_cloud.store import Store, get_store

router = APIRouter(prefix="/api/v1", tags=["products"])


def get_product_creation_proof_verifier(
    user: AuthenticatedUser = Depends(get_current_user),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> Ed25519ProductCreationProofVerifier:
    """Verify against this caller's enrolled daemon identity keys only.

    Company-wide ``PRODUCT_CREATION_PROOF_VERIFICATION_KEYS_JSON`` is not the
    production trust store (factory #901 row 12 / cloud#302).

    Enrollment is read from the process-local coordination ``Store`` (ADR-0008).
    Cloud Run scale-out / scale-to-zero does not share or retain that state;
    publication verify is fail-closed (503) when this instance has no enrolled
    key for the caller.
    """
    try:
        return Ed25519ProductCreationProofVerifier.from_enrolled_pairings(
            store.identity_keys_for_user(user.user_id)
        )
    except ProductCreationProofVerifierUnavailable as exc:
        raise HTTPException(
            status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
            detail="Product creation proof verification unavailable",
        ) from exc


@router.post(
    "/products",
    response_model=ProductPublicationReceipt,
    status_code=status.HTTP_201_CREATED,
    summary="Publish a daemon-created Product into the durable directory",
)
async def publish_product(
    publication: ProductPublication,
    response: Response,
    user: AuthenticatedUser = Depends(get_current_user),  # noqa: B008
    directory: ProductDirectoryRepository = Depends(  # noqa: B008
        get_product_directory_repository
    ),
    proof_verifier: Ed25519ProductCreationProofVerifier = Depends(  # noqa: B008
        get_product_creation_proof_verifier
    ),
) -> ProductPublicationReceipt:
    """Idempotently publish a Product for the caller's verified email domain.

    Access requires a valid bearer token and the design-partner domain gate
    (``get_current_user``). The published ``eligible_domain`` must match the
    caller's verified email domain — a caller cannot publish discovery scope for
    a domain it does not own. Re-publishing the same canonical Product ID is a
    no-op refresh (HTTP 200); a first publication returns HTTP 201. Attempting to
    remap an existing identifier is rejected with HTTP 409.
    """
    try:
        proof_verifier.verify(publication.first_event_commit_proof)
    except ProductCreationProofVerificationError as exc:
        raise HTTPException(
            status_code=status.HTTP_422_UNPROCESSABLE_ENTITY,
            detail="Product creation proof signature is invalid",
        ) from exc

    caller_domain = user.verified_email_domain
    if not caller_domain or publication.eligible_domain != caller_domain:
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="eligible_domain must match the caller's verified email domain",
        )

    try:
        record, idempotent = directory.publish(publication)
    except ProductIdRemapError as exc:
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT, detail=str(exc)
        ) from exc
    except ProductDirectoryUnavailable as exc:
        raise HTTPException(
            status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
            detail="Product directory unavailable",
        ) from exc

    if idempotent:
        response.status_code = status.HTTP_200_OK

    return ProductPublicationReceipt(
        canonical_product_id=record.canonical_product_id,
        display_name=record.display_name,
        eligible_domain=record.eligible_domain,
        idempotent=idempotent,
    )
