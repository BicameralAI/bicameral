"""Health route test."""

from __future__ import annotations

from fastapi.testclient import TestClient

from bicameral_cloud.app import app


def test_health():
    client = TestClient(app)
    resp = client.get("/health")
    assert resp.status_code == 200
    assert resp.json() == {"status": "ok"}
