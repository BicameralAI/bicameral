"""In-process fake daemon modeling the local workspace-binding runtime contract.

This fixture models the *runtime/semantic* contract the hosted lifecycle depends
on but which physically lives in other repos (bicameral-bot#747 daemon
capability, bicameral-mcp#706 MCP user path). True cross-repo execution is too
heavy for the Cloud suite, so this focused fake daemon stands in for the local
daemon's two binding-relevant surfaces (issue #90):

- ``GET /v2/capabilities`` — the daemon's truthful workspace-binding capability
  handshake (bicameral-bot#747). Returns ``workspace_binding_available`` as a
  real boolean, or an error the client must treat as *unknown* (fail closed).
- ``POST /v2/workspace/bind`` — the local, daemon-owned bind the MCP user path
  (bicameral-mcp#706) drives. The daemon is the sole binding authority
  (ADR-0005); Cloud never persists binding. The bind response carries only a
  safe readiness projection and never echoes the caller's local path.

Binding truth stays local: the ``local_path`` handed to the daemon is used only
to model that a bind happened; it is never reflected back into a
Cloud/browser-facing projection (the redaction invariant is asserted in the
conformance test).

Mirrors the existing ``fake_daemon.run_fake_daemon`` harness so the tests
exercise the real ``httpx`` client code path against a genuine loopback server
rather than monkeypatching transport.
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
from fastapi import FastAPI
from fastapi.responses import JSONResponse

# ``available``    -> daemon advertises workspace binding (capability True).
# ``unsupported``  -> daemon reachable but does NOT advertise binding (False).
# ``error``        -> capability query fails (500); client must fail closed to
#                     unknown, exactly like an unreachable daemon.
CapabilityMode = Literal["available", "unsupported", "error"]

# Daemon runtime-contract paths mirrored from the dashboard JS
# (routes/dashboard.py: DAEMON_CAPABILITIES_PATH) and the MCP user path.
CAPABILITIES_PATH = "/v2/capabilities"
WORKSPACE_BIND_PATH = "/v2/workspace/bind"


def _build_app(mode: CapabilityMode) -> FastAPI:
    app = FastAPI()

    @app.get("/healthz")
    async def healthz() -> dict[str, str]:
        return {"status": "ok"}

    @app.get(CAPABILITIES_PATH)
    async def capabilities() -> JSONResponse:
        if mode == "error":
            return JSONResponse(
                status_code=500,
                content={"error_type": "daemon_internal_error"},
            )
        return JSONResponse(
            status_code=200,
            content={
                "protocol_version": "2.0",
                "workspace_binding_available": mode == "available",
            },
        )

    @app.post(WORKSPACE_BIND_PATH)
    async def workspace_bind(payload: dict[str, object]) -> JSONResponse:
        # Fail closed: the daemon only binds when it advertises the capability.
        if mode != "available":
            return JSONResponse(
                status_code=409,
                content={
                    "bound": False,
                    "error_type": "binding_unsupported",
                    "detail": (
                        "daemon does not advertise workspace binding"
                    ),
                },
            )
        project_id = payload.get("project_id")
        # Binding is local and daemon-owned: the caller's local_path is NOT
        # echoed back. Only a safe readiness projection crosses the boundary.
        return JSONResponse(
            status_code=200,
            content={
                "bound": True,
                "project_id": project_id,
                "readiness": "ready",
            },
        )

    return app


@contextmanager
def run_fake_binding_daemon(mode: CapabilityMode) -> Iterator[str]:
    """Run the fake binding daemon on loopback and yield its base URL."""

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

    thread = threading.Thread(target=_run, name="fake-binding-daemon", daemon=True)
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
    else:  # pragma: no cover - startup failure surfaced to the caller
        server.should_exit = True
        thread.join(timeout=5.0)
        sock.close()
        raise RuntimeError("fake binding daemon failed to start") from last_error

    try:
        yield base_url
    finally:
        server.should_exit = True
        thread.join(timeout=5.0)
        sock.close()
