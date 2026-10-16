"""In-process fake Supabase REST for hosted-discovery conformance tests.

Models the *only* Supabase surface Cloud calls server-side during the hosted
design-partner path: the ``design_partner_allowlist`` PostgREST table read
(``auth/design_partner_allowlist.py``). Browser identity is minted locally as an
HS256 access token validated against ``SUPABASE_JWT_SECRET`` (see
``mint_supabase_jwt``), so the whole harness runs with NO real Supabase project,
service-role key, or Google OAuth — only a loopback server and a local secret.

This mirrors ``fake_daemon.run_fake_daemon``: it exercises the real ``httpx``
allowlist code path (a genuine ``Response`` with a bound request) rather than
monkeypatching ``httpx``, so it is faithful to the production request shape.
"""

from __future__ import annotations

import asyncio
import socket
import threading
import time
from collections.abc import Iterable, Iterator
from contextlib import contextmanager
from datetime import UTC, datetime, timedelta

import httpx
import jwt
import uvicorn
from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse

# Local HS256 secret used by the fake browser-identity minter. The matching
# CloudSettings.supabase_jwt_secret makes token validation fully local (no
# Supabase Auth API call), so no real credentials are required.
FAKE_JWT_SECRET = "test-jwt-secret"


def mint_supabase_jwt(
    email: str,
    *,
    secret: str = FAKE_JWT_SECRET,
    sub: str = "user-123",
    ttl_minutes: int = 5,
) -> str:
    """Mint a Supabase-shaped HS256 access token for the fake browser session."""

    now = datetime.now(UTC)
    claims = {
        "sub": sub,
        "email": email,
        "aud": "authenticated",
        "iat": int(now.timestamp()),
        "exp": int((now + timedelta(minutes=ttl_minutes)).timestamp()),
    }
    return jwt.encode(claims, secret, algorithm="HS256")


def _domain_from_eq_param(raw: str | None) -> str | None:
    """Parse a PostgREST ``domain=eq.<value>`` filter parameter."""

    if raw is None:
        return None
    return raw[len("eq.") :] if raw.startswith("eq.") else raw


def _build_app(allowed_domains: set[str]) -> FastAPI:
    app = FastAPI()

    @app.get("/rest/v1/design_partner_allowlist")
    async def design_partner_allowlist(request: Request) -> JSONResponse:
        """Return one row when the queried domain is enabled and allowlisted."""

        enabled = request.query_params.get("enabled")
        domain = _domain_from_eq_param(request.query_params.get("domain"))
        if enabled == "eq.true" and domain in allowed_domains:
            return JSONResponse(status_code=200, content=[{"id": f"row-{domain}"}])
        return JSONResponse(status_code=200, content=[])

    return app


@contextmanager
def run_fake_supabase(allowed_domains: Iterable[str]) -> Iterator[str]:
    """Run the fake Supabase REST on loopback and yield its base URL."""

    app = _build_app({d.strip().lower() for d in allowed_domains})
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.bind(("127.0.0.1", 0))
    sock.listen(8)
    port = sock.getsockname()[1]
    config = uvicorn.Config(
        app,
        host="127.0.0.1",
        port=port,
        log_level="error",
        loop="asyncio",
        lifespan="off",
    )
    server = uvicorn.Server(config)
    ready = threading.Event()

    def _run() -> None:
        ready.set()
        asyncio.run(server.serve(sockets=[sock]))

    thread = threading.Thread(target=_run, name="fake-supabase", daemon=True)
    thread.start()
    ready.wait(timeout=1.0)

    base_url = f"http://127.0.0.1:{port}"
    deadline = time.monotonic() + 5.0
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        try:
            response = httpx.get(
                f"{base_url}/rest/v1/design_partner_allowlist",
                params={"enabled": "eq.true", "domain": "eq.__healthcheck__"},
                timeout=0.2,
            )
            if response.status_code == 200:
                break
        except httpx.HTTPError as exc:  # pragma: no cover - transient startup
            last_error = exc
        time.sleep(0.05)
    else:  # pragma: no cover - startup failure surfaced to the caller
        server.should_exit = True
        thread.join(timeout=5.0)
        sock.close()
        raise RuntimeError("fake supabase failed to start") from last_error

    try:
        yield base_url
    finally:
        server.should_exit = True
        thread.join(timeout=5.0)
        sock.close()
