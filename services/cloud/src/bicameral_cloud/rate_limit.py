"""Best-effort per-client request rate limiting for Bicameral Cloud.

This is an in-process, per-instance fixed-window limiter. It bounds abusive
request volume against a single Cloud Run instance and provides application-layer
evidence of a rate-limiting control. It is NOT a global limit: Cloud Run may run
several instances, each holding its own counter, and counters reset on instance
recycle. The authoritative, cross-instance rate limit belongs at the edge
(Cloudflare or Cloud Armor); this middleware is defense-in-depth beneath it.

Limits are configured via BICAMERAL_CLOUD_RATE_LIMIT_REQUESTS and
BICAMERAL_CLOUD_RATE_LIMIT_WINDOW_SECONDS (see config.py). A non-positive request
limit disables the middleware. Health and well-known probe paths are exempt so
platform liveness checks and RFC 9116 discovery are never throttled.
"""

from __future__ import annotations

import time

from fastapi import Request, Response
from starlette.middleware.base import BaseHTTPMiddleware, RequestResponseEndpoint
from starlette.responses import JSONResponse
from starlette.types import ASGIApp

# Paths that must never be throttled: Cloud Run liveness/readiness and the
# public RFC 9116 security.txt discovery surface.
_EXEMPT_PREFIXES = ("/health", "/.well-known/")

# Bound the tracking table so a spray of distinct source addresses cannot grow
# memory without limit; expired entries are pruned when this ceiling is hit.
_MAX_TRACKED_CLIENTS = 10000


class RateLimitMiddleware(BaseHTTPMiddleware):
    """Fixed-window per-client-IP limiter (per-instance, best-effort)."""

    def __init__(
        self,
        app: ASGIApp,
        *,
        requests: int = 300,
        window_seconds: int = 60,
    ) -> None:
        super().__init__(app)
        self._max = requests
        self._window = max(1, window_seconds)
        # client key -> (window_start_monotonic, count_in_window)
        self._hits: dict[str, tuple[float, int]] = {}

    def _client_key(self, request: Request) -> str:
        # Behind the Cloud Run load balancer the real client address is the
        # first X-Forwarded-For entry. This header is only trustworthy behind a
        # trusted proxy; the edge limit remains authoritative.
        forwarded = request.headers.get("x-forwarded-for")
        if forwarded:
            return forwarded.split(",")[0].strip()
        return request.client.host if request.client else "unknown"

    def _prune(self, now: float) -> None:
        expired = [
            key
            for key, (start, _count) in self._hits.items()
            if now - start >= self._window
        ]
        for key in expired:
            del self._hits[key]

    async def dispatch(
        self,
        request: Request,
        call_next: RequestResponseEndpoint,
    ) -> Response:
        if self._max <= 0 or request.url.path.startswith(_EXEMPT_PREFIXES):
            return await call_next(request)

        now = time.monotonic()
        if len(self._hits) >= _MAX_TRACKED_CLIENTS:
            self._prune(now)

        key = self._client_key(request)
        start, count = self._hits.get(key, (now, 0))
        if now - start >= self._window:
            start, count = now, 0
        count += 1
        self._hits[key] = (start, count)

        if count > self._max:
            retry_after = max(1, int(self._window - (now - start)))
            return JSONResponse(
                {"detail": "rate limit exceeded"},
                status_code=429,
                headers={"Retry-After": str(retry_after)},
            )

        return await call_next(request)
