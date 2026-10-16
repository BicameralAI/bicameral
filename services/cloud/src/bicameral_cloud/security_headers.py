"""Shared HTTP response-security policy for Bicameral Cloud.

The hosted dashboard shell contains inline CSS and JavaScript and loads the
Supabase browser client from jsDelivr. The CSP therefore retains narrowly
documented inline allowances until the shell is migrated to nonce- or hash-bound
assets.
"""

from __future__ import annotations

from fastapi import Request, Response
from starlette.middleware.base import BaseHTTPMiddleware, RequestResponseEndpoint

CONTENT_SECURITY_POLICY = "; ".join(
    (
        "default-src 'self'",
        "base-uri 'self'",
        "object-src 'none'",
        "frame-ancestors 'none'",
        "form-action 'self'",
        "script-src 'self' 'unsafe-inline' https://cdn.jsdelivr.net",
        "style-src 'self' 'unsafe-inline'",
        "img-src 'self' data:",
        "font-src 'self'",
        (
            "connect-src 'self' https://*.supabase.co wss://*.supabase.co "
            "http://localhost:* http://127.0.0.1:*"
        ),
    )
)


class SecurityHeadersMiddleware(BaseHTTPMiddleware):
    """Apply the response-security baseline without weakening stricter routes."""

    async def dispatch(
        self,
        request: Request,
        call_next: RequestResponseEndpoint,
    ) -> Response:
        response = await call_next(request)

        if (
            request.url.scheme == "https"
            and "strict-transport-security" not in response.headers
        ):
            response.headers["Strict-Transport-Security"] = "max-age=31536000"

        defaults = {
            "Referrer-Policy": "strict-origin-when-cross-origin",
            "Content-Security-Policy": CONTENT_SECURITY_POLICY,
            "X-Content-Type-Options": "nosniff",
        }
        for name, value in defaults.items():
            if name.lower() not in response.headers:
                response.headers[name] = value

        return response
