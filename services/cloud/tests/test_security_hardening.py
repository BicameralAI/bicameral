from __future__ import annotations

from fastapi.testclient import TestClient

from bicameral_cloud.app import app
from bicameral_cloud.security_headers import CONTENT_SECURITY_POLICY


def test_https_responses_receive_security_headers() -> None:
    with TestClient(app, base_url="https://testserver") as client:
        for path in ("/health", "/signup", "/not-found"):
            response = client.get(path)
            assert (
                response.headers["strict-transport-security"]
                == "max-age=31536000"
            )
            assert (
                response.headers["referrer-policy"]
                == "strict-origin-when-cross-origin"
            )
            assert (
                response.headers["content-security-policy"]
                == CONTENT_SECURITY_POLICY
            )
            assert response.headers["x-content-type-options"] == "nosniff"


def test_hsts_is_not_sent_over_plain_http() -> None:
    with TestClient(app, base_url="http://testserver") as client:
        response = client.get("/health")

    assert "strict-transport-security" not in response.headers
    assert response.headers["referrer-policy"] == "strict-origin-when-cross-origin"
    assert response.headers["content-security-policy"] == CONTENT_SECURITY_POLICY
    assert response.headers["x-content-type-options"] == "nosniff"


def test_security_txt_is_public_and_rfc_9116_shaped() -> None:
    with TestClient(app, base_url="https://testserver") as client:
        response = client.get("/.well-known/security.txt")
        schema = client.get("/openapi.json").json()

    assert response.status_code == 200
    assert response.headers["content-type"].startswith("text/plain")
    assert "Contact: mailto:devs@bicameral-ai.com" in response.text
    assert "Expires: 2027-06-30T23:59:59Z" in response.text
    assert "Preferred-Languages: en" in response.text
    assert (
        "Canonical: https://app.bicameral-ai.com/.well-known/security.txt"
        in response.text
    )
    assert "/.well-known/security.txt" not in schema["paths"]


def test_signup_options_remains_minimal_method_rejection() -> None:
    with TestClient(app, base_url="https://testserver") as client:
        response = client.options("/signup")

    assert response.status_code == 405
    assert response.headers["allow"] == "GET"
    assert response.headers["content-type"].startswith("application/json")
    assert response.json() == {"detail": "Method Not Allowed"}
