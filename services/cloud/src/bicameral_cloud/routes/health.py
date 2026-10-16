"""Health and readiness routes.

`/health` is liveness: it answers whether the process is up. `/health/ready`
reports whether this revision can actually construct every declared capability
from its current configuration. The two signals intentionally remain separate.
"""

from __future__ import annotations

from fastapi import APIRouter, HTTPException, status

from bicameral_cloud.capability_readiness import capability_status
from bicameral_cloud.config import get_settings

router = APIRouter(tags=["health"])


@router.get("/health")
async def health() -> dict[str, str]:
    return {"status": "ok"}


@router.get("/health/ready")
async def readiness() -> dict[str, object]:
    """Report capability booleans and fail with 503 if any are unready.

    The response deliberately contains capability names and booleans only. It
    does not disclose variable names, key identifiers, issuers, or exceptions.
    """

    capabilities = capability_status(get_settings())
    if not all(capabilities.values()):
        raise HTTPException(
            status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
            detail={"status": "not_ready", "capabilities": capabilities},
        )
    return {"status": "ready", "capabilities": capabilities}
