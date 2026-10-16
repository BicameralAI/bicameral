"""Per-instance application-layer rate limiter (pentest rate-limit remediation).

The limiter is a per-instance defense-in-depth control; these tests exercise it
in isolation (a fresh app per test, so each has its own counter state) rather
than through the shared app, which disables it via the root conftest.
"""

from __future__ import annotations

from fastapi import FastAPI
from fastapi.testclient import TestClient

from bicameral_cloud.rate_limit import RateLimitMiddleware


def _app(*, requests: int, window_seconds: int = 60) -> FastAPI:
    app = FastAPI()
    app.add_middleware(
        RateLimitMiddleware, requests=requests, window_seconds=window_seconds
    )

    @app.get("/ping")
    def ping() -> dict:
        return {"ok": True}

    @app.get("/health")
    def health() -> dict:
        return {"ok": True}

    return app


def test_requests_within_limit_pass() -> None:
    client = TestClient(_app(requests=3))
    for _ in range(3):
        assert client.get("/ping").status_code == 200


def test_request_over_limit_is_rejected_with_retry_after() -> None:
    client = TestClient(_app(requests=2))
    assert client.get("/ping").status_code == 200
    assert client.get("/ping").status_code == 200
    blocked = client.get("/ping")
    assert blocked.status_code == 429
    assert blocked.json()["detail"] == "rate limit exceeded"
    assert int(blocked.headers["retry-after"]) >= 1


def test_health_and_probe_paths_are_exempt() -> None:
    client = TestClient(_app(requests=1))
    # The limit is 1, but exempt paths must never be throttled.
    for _ in range(5):
        assert client.get("/health").status_code == 200


def test_nonpositive_limit_disables_the_middleware() -> None:
    client = TestClient(_app(requests=0))
    for _ in range(10):
        assert client.get("/ping").status_code == 200
