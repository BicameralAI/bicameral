"""FastAPI application for Bicameral Cloud.

Cloud is an oracle/optimization layer (ADR-0001). It coordinates auth,
tenancy, pairing, workspace discovery, and bridge routing, but does not
own Decision authority, signoff authority, compliance authority,
event-store write authority, or local workspace binding.
"""

from __future__ import annotations

from pathlib import Path

from fastapi import FastAPI, HTTPException
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles

from bicameral_cloud.config import CloudSettings, get_settings
from bicameral_cloud.rate_limit import RateLimitMiddleware
from bicameral_cloud.routes.audit import router as audit_router
from bicameral_cloud.routes.auth import router as auth_router
from bicameral_cloud.routes.bridge import router as bridge_router
from bicameral_cloud.routes.compatibility import router as compatibility_router
from bicameral_cloud.routes.connect_polish_apc1 import router as connect_polish_router
from bicameral_cloud.routes.dashboard import router as dashboard_router
from bicameral_cloud.routes.discovery import router as discovery_router
from bicameral_cloud.routes.health import router as health_router
from bicameral_cloud.routes.oauth_scope_apc1 import router as oauth_scope_router
from bicameral_cloud.routes.pairing import router as pairing_router
from bicameral_cloud.routes.product_join_ticket import (
    router as product_join_ticket_router,
)
from bicameral_cloud.routes.product_publication import (
    router as product_publication_router,
)
from bicameral_cloud.routes.project_coordination import (
    router as project_coordination_router,
)
from bicameral_cloud.routes.project_registration import (
    router as project_registration_router,
)
from bicameral_cloud.routes.projects import router as projects_router
from bicameral_cloud.routes.provider_app_config import (
    router as provider_app_config_router,
)
from bicameral_cloud.routes.security import router as security_router
from bicameral_cloud.routes.session import router as session_router
from bicameral_cloud.routes.signup_polish_apc1 import router as signup_polish_router
from bicameral_cloud.routes.team import router as team_router
from bicameral_cloud.security_headers import SecurityHeadersMiddleware


def _docs_kwargs(settings: CloudSettings) -> dict[str, str | None]:
    """Return the OpenAPI/docs URL settings for FastAPI.

    Off by default (all None) so the hosted API surface is not publicly
    enumerable in production (pentest OpenAPI-exposure remediation). Set
    BICAMERAL_CLOUD_ENABLE_DOCS=1 in non-production to restore FastAPI's default
    /openapi.json, /docs, and /redoc.
    """
    if settings.docs_enabled:
        return {
            "openapi_url": "/openapi.json",
            "docs_url": "/docs",
            "redoc_url": "/redoc",
        }
    return {"openapi_url": None, "docs_url": None, "redoc_url": None}


_settings = get_settings()
_docs = _docs_kwargs(_settings)

app = FastAPI(
    title="Bicameral Cloud",
    description="Hosted optimization and coordination layer for Bicameral",
    version="0.1.0",
    openapi_url=_docs["openapi_url"],
    docs_url=_docs["docs_url"],
    redoc_url=_docs["redoc_url"],
)
# Order matters: add_middleware wraps outermost-last, so SecurityHeaders is added
# last to remain the outermost layer, applying its headers even to a 429 the
# rate limiter returns before the request reaches a route.
app.add_middleware(
    RateLimitMiddleware,
    requests=_settings.rate_limit_requests,
    window_seconds=_settings.rate_limit_window_seconds,
)
app.add_middleware(SecurityHeadersMiddleware)

app.include_router(health_router)
app.include_router(security_router)
# APC1 overlays the progressive signup and /connect browser surfaces. The OAuth
# reliability overlay must stay first so explicit Google identity scopes and
# callback diagnostics apply before the frozen Alpha routes.
app.include_router(oauth_scope_router)
app.include_router(signup_polish_router)
app.include_router(connect_polish_router)
app.include_router(dashboard_router)
app.include_router(auth_router)
app.include_router(team_router)
app.include_router(pairing_router)
app.include_router(session_router)
app.include_router(compatibility_router)
app.include_router(discovery_router)
app.include_router(projects_router)
app.include_router(product_publication_router)
app.include_router(product_join_ticket_router)
app.include_router(project_registration_router)
app.include_router(project_coordination_router)
app.include_router(bridge_router)
app.include_router(audit_router)
app.include_router(provider_app_config_router)

# ── Dashboard SPA (cloud serves UI) ────────────────────────────────────────
# The Svelte dashboard (bicameral-bot/dashboard) is built and vendored into
# static/dashboard by scripts/vendor_dashboard.sh, then shipped in the image via
# the Dockerfile's `COPY src`. We serve its hashed assets and the SPA entrypoint
# here. Product data still flows through the paired daemon via the bridge; this
# only serves the client. Browser pairing entry is Cloud /connect
# (routes/dashboard.py); there is no /dashboard browser route.
_SPA_DIR = Path(__file__).parent / "static" / "dashboard"
_SPA_INDEX = _SPA_DIR / "index.html"
_SPA_ASSETS = _SPA_DIR / "assets"
_SPA_VERSION = _SPA_DIR / "version.json"
_SPA_RELEASE_IDENTITY = _SPA_DIR / "release-identity.json"

if _SPA_ASSETS.is_dir():
    app.mount("/assets", StaticFiles(directory=_SPA_ASSETS), name="spa-assets")


@app.get("/app/version.json", include_in_schema=False)
async def dashboard_version() -> FileResponse:
    """Serve vendored dashboard build metadata."""

    if not _SPA_VERSION.is_file():
        raise HTTPException(
            status_code=503,
            detail=(
                "Dashboard version metadata not built. "
                "Run scripts/vendor_dashboard.sh."
            ),
        )
    return FileResponse(
        _SPA_VERSION,
        media_type="application/json",
        headers={"Cache-Control": "no-store"},
    )


@app.get("/app/release-identity.json", include_in_schema=False)
async def release_identity() -> FileResponse:
    """Report the immutable release tuple embedded in the deployed image."""

    if not _SPA_RELEASE_IDENTITY.is_file():
        raise HTTPException(
            status_code=503,
            detail=(
                "Release identity was not materialized by the deployment controller."
            ),
        )
    return FileResponse(
        _SPA_RELEASE_IDENTITY,
        media_type="application/json",
        headers={"Cache-Control": "no-store"},
    )


@app.get("/app", include_in_schema=False)
@app.get("/app/{path:path}", include_in_schema=False)
async def dashboard_app(path: str = "") -> FileResponse:
    """Serve the vendored dashboard SPA (client-side routed)."""

    if not _SPA_INDEX.is_file():
        raise HTTPException(
            status_code=503,
            detail="Dashboard bundle not built. Run scripts/vendor_dashboard.sh.",
        )
    return FileResponse(_SPA_INDEX)
