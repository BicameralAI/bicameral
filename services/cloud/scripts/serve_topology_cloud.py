"""Disposable HTTP/TLS listeners sharing one Cloud app and coordination store."""

from __future__ import annotations

import argparse
import asyncio
import signal
from collections.abc import Iterator
from contextlib import contextmanager

import uvicorn


class TopologyServer(uvicorn.Server):
    @contextmanager
    def capture_signals(self) -> Iterator[None]:
        # One process-level handler shuts down both listeners together.
        yield


async def serve(http_port: int, tls_port: int, key: str, certificate: str) -> None:
    from bicameral_cloud.app import app

    servers = [
        TopologyServer(
            uvicorn.Config(
                app,
                host="127.0.0.1",
                port=http_port,
                access_log=False,
                lifespan="off",
            )
        ),
        TopologyServer(
            uvicorn.Config(
                app,
                host="127.0.0.1",
                port=tls_port,
                access_log=False,
                ssl_keyfile=key,
                ssl_certfile=certificate,
            )
        ),
    ]

    def stop() -> None:
        for server in servers:
            server.should_exit = True

    loop = asyncio.get_running_loop()
    for sig in (signal.SIGINT, signal.SIGTERM):
        loop.add_signal_handler(sig, stop)
    tasks = [asyncio.create_task(server.serve()) for server in servers]
    try:
        await asyncio.wait(tasks, return_when=asyncio.FIRST_COMPLETED)
    finally:
        stop()
        await asyncio.gather(*tasks)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("http_port", type=int)
    parser.add_argument("tls_port", type=int)
    parser.add_argument("key")
    parser.add_argument("certificate")
    args = parser.parse_args()
    asyncio.run(serve(args.http_port, args.tls_port, args.key, args.certificate))
