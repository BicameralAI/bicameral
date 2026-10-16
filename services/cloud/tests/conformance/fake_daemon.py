"""In-process fake daemon for Cloud-to-local pending-pairing conformance tests.

Models the bot#790 pending-first hosted-bridge contract: ``POST /pair`` returns
a ``202`` pending envelope (never a token), and ``GET /pair/status`` releases a
transient session token only on local approval. The fake daemon is the pairing
authority; Cloud only initiates and polls.
"""

from __future__ import annotations

import asyncio
import socket
import threading
import time
from collections.abc import Iterator
from contextlib import contextmanager
from typing import Literal

import httpx
import uvicorn
from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse

# The status outcome the fake daemon reports for an initiated pairing. ``approves``
# simulates a completed local confirmation; the others simulate typed failures.
# ``paired`` is the legacy immediate-projection mode retained for the hosted
# discovery / project-lifecycle conformance suites, which pair only as a fixture
# and then assert on the daemon's redacted discovery projection.
PairingMode = Literal[
    "paired",
    "approves",
    "denies",
    "expires",
    "revoked",
    "incompatible",
    "error_500",
]

# Pending-pairing protocol floor (mirrors the bot pending-pairing minimum).
PENDING_PAIRING_MIN_PROTOCOL_VERSION = 4


def closed_loopback_url() -> str:
    """Return an unused loopback URL that should refuse connections."""

    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    return f"http://127.0.0.1:{port}"


def _confirmation_envelope(origin: str, cloud_account_id: str) -> dict[str, object]:
    """Local-confirmation summary the daemon surfaces to its own window.

    Cloud never approves; it only relays that a local confirmation is required.
    """

    return {
        "terminal_command": "bicameral pairing approve",
        "message": (
            "Paste this code into your local Bicameral gateway or run "
            "`bicameral pairing approve`."
        ),
        "requesting_origin": origin,
        "cloud_account_id": cloud_account_id,
        "capability_summary": {
            "allowed": [
                "List locally registered Bicameral projects",
                "Show setup / readiness status",
            ],
            "forbidden": [
                "Provider secrets or tokens",
                "Local filesystem or workspace/code access",
            ],
        },
    }


def _paired_payload(selected_project_id: object = None) -> dict[str, object]:
    """Legacy immediate-projection body (discovery/lifecycle fixtures only)."""

    payload: dict[str, object] = {
        "pairing_state": "paired",
        "daemon_version": "1.4.0",
        "pairing_id": "pairing-xyz",
        "operator_home": {"display_name": "Operator Home", "ready": True},
        "project_readiness": [
            {
                "workspace_id": "ws-alpha",
                "display_name": "Frontend App",
                "readiness": "ready",
            },
            {
                "workspace_id": "ws-beta",
                "display_name": "Backend Service",
                "readiness": "degraded",
            },
        ],
    }
    if selected_project_id is not None:
        payload["selected_project_id"] = selected_project_id
    return payload


def _build_app(mode: PairingMode) -> FastAPI:
    app = FastAPI()

    @app.get("/healthz")
    async def healthz() -> dict[str, str]:
        return {"status": "ok"}

    @app.post("/api/v1/hosted-bridge/pair")
    async def pair(payload: dict[str, object]) -> JSONResponse:
        if mode == "paired":
            return JSONResponse(
                status_code=200,
                content=_paired_payload(payload.get("selected_project_id")),
            )

        if mode == "error_500":
            return JSONResponse(
                status_code=500,
                content={
                    "error_type": "daemon_internal_error",
                    "detail": (
                        "Traceback (most recent call last): ... "
                        "/home/user/.bicameral/store/events.jsonl"
                    ),
                },
            )

        client_protocol_version = payload.get("client_protocol_version")
        if (
            not isinstance(client_protocol_version, int)
            or client_protocol_version < PENDING_PAIRING_MIN_PROTOCOL_VERSION
        ):
            return JSONResponse(
                status_code=426,
                content={
                    "error_type": "protocol_unsupported",
                    "detail": "update_required",
                },
            )

        if mode == "incompatible":
            return JSONResponse(
                status_code=426,
                content={"error_type": "protocol_unsupported"},
            )

        origin = str(payload.get("origin", ""))
        cloud_account_id = str(payload.get("cloud_account_id", ""))
        # Pending envelope: NO session token is minted here.
        return JSONResponse(
            status_code=202,
            content={
                "status": "pending",
                "request_handle": "handle-abc",
                "poll_proof": "proof-xyz",
                "pairing_code": "7K2M-PQ8R",
                "expires_at": "2099-01-01T00:00:00Z",
                "protocol_version": PENDING_PAIRING_MIN_PROTOCOL_VERSION,
                "confirmation": _confirmation_envelope(origin, cloud_account_id),
            },
        )

    @app.get("/api/v1/hosted-bridge/pair/status")
    async def pair_status(request: Request) -> JSONResponse:
        params = request.query_params
        # The daemon verifies the opaque proof, original origin, and account.
        if (
            params.get("request_handle") != "handle-abc"
            or params.get("poll_proof") != "proof-xyz"
            or not params.get("cloud_account_id")
        ):
            return JSONResponse(
                status_code=403,
                content={"error_type": "poll_proof_rejected"},
            )

        if mode == "denies":
            return JSONResponse(status_code=200, content={"status": "denied"})
        if mode == "expires":
            return JSONResponse(status_code=200, content={"status": "expired"})
        if mode == "revoked":
            return JSONResponse(status_code=200, content={"status": "revoked"})

        # approves: release the transient token exactly once, in the body only.
        return JSONResponse(
            status_code=200,
            content={
                "status": "approved",
                "session_token": "transient-session-token",
                "expires_at": "2099-01-01T00:30:00Z",
                "capabilities": {
                    "hosted_bridge_protocol_version": 4,
                    "daemon_build_version": "1.4.0",
                },
            },
        )

    return app


@contextmanager
def run_fake_daemon(mode: PairingMode) -> Iterator[str]:
    """Run the fake daemon on loopback and yield its base URL."""

    app = _build_app(mode)
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

    thread = threading.Thread(target=_run, name="fake-daemon", daemon=True)
    thread.start()
    ready.wait(timeout=1.0)

    base_url = f"http://127.0.0.1:{port}"
    deadline = time.monotonic() + 5.0
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        try:
            response = httpx.get(f"{base_url}/healthz", timeout=0.2)
            if response.status_code == 200:
                break
        except httpx.HTTPError as exc:  # pragma: no cover - transient startup
            last_error = exc
        time.sleep(0.05)
    else:  # pragma: no cover - startup failure is exercised via tests
        server.should_exit = True
        thread.join(timeout=5.0)
        sock.close()
        raise RuntimeError("fake daemon failed to start") from last_error

    try:
        yield base_url
    finally:
        server.should_exit = True
        thread.join(timeout=5.0)
        sock.close()
