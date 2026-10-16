"""Regression coverage for production Google OAuth callback reliability."""

from __future__ import annotations

EMAIL_SCOPE = "https://www.googleapis.com/auth/userinfo.email"


def test_connect_requests_google_email_scope_explicitly(client, store):
    response = client.get("/connect")

    assert response.status_code == 200
    assert EMAIL_SCOPE in response.text
    assert "provider:'google'" in response.text
    assert "scopes:'https://www.googleapis.com/auth/userinfo.email'" in response.text
    assert "drive.file" not in response.text


def test_signup_and_login_request_google_email_scope_explicitly(client, store):
    for path in ("/signup", "/login"):
        response = client.get(path)
        assert response.status_code == 200
        assert EMAIL_SCOPE in response.text
        assert "scopes: 'https://www.googleapis.com/auth/userinfo.email'" in response.text
        assert "drive.file" not in response.text


def test_callback_surfaces_provider_error_before_session_check(client, store):
    response = client.get(
        "/auth/callback?error=server_error&error_code=unexpected_failure"
        "&error_description=provider%20callback%20failed"
    )

    assert response.status_code == 200
    assert "new URLSearchParams(window.location.search)" in response.text
    assert "callbackErrorDescription" in response.text
    assert "Google sign-in failed before a Cloud session was created" in response.text


def test_oauth_identity_scope_remains_separate_from_drive_provider_scope(client, store):
    response = client.get("/connect")

    assert response.status_code == 200
    assert EMAIL_SCOPE in response.text
    assert "https://www.googleapis.com/auth/drive.file" not in response.text
    assert "Authorize Drive after this device is connected." in response.text
