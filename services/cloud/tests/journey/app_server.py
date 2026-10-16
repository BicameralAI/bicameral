"""Live Bicameral Cloud app server on loopback for browser UserJourneys.

Runs ``bicameral_cloud.app:app`` in a background thread bound to an ephemeral
loopback port so a real browser (Playwright) can exercise the served dashboard
and the real registration/discovery HTTP contracts. The app keeps its in-memory
singleton ``Store`` and ``CloudSettings`` in this same process, so the harness
can seed an isolated realistic test store and assert coordination state
directly — while every browser-visible mutation still flows through the real
FastAPI routes.

Mirrors the threaded-uvicorn pattern already used by
``tests/conformance/fake_supabase.py`` (a genuine server on a real socket,
never a monkeypatched transport) so the journey is faithful to production
request/response shapes.
"""

from __future__ import annotations

import socket
import threading
import time
from collections.abc import Iterator
from contextlib import contextmanager

import httpx
import uvicorn

from bicameral_cloud.app import app


@contextmanager
def run_cloud_app() -> Iterator[str]:
    """Run the Cloud app on loopback and yield its base URL.

    The caller is responsible for resetting/seeding the singleton store and
    settings before entering (the server thread shares those globals).
    """

    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.bind(("127.0.0.1", 0))
    sock.listen(16)
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

    def _run() -> None:
        server.run(sockets=[sock])

    thread = threading.Thread(target=_run, name="cloud-app", daemon=True)
    thread.start()

    base_url = f"http://127.0.0.1:{port}"
    deadline = time.monotonic() + 10.0
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        try:
            response = httpx.get(f"{base_url}/health", timeout=0.5)
            if response.status_code == 200:
                break
        except httpx.HTTPError as exc:  # pragma: no cover - transient startup
            last_error = exc
        time.sleep(0.05)
    else:  # pragma: no cover - startup failure surfaced to the caller
        server.should_exit = True
        thread.join(timeout=5.0)
        sock.close()
        raise RuntimeError("cloud app failed to start") from last_error

    try:
        yield base_url
    finally:
        server.should_exit = True
        thread.join(timeout=5.0)
        sock.close()
