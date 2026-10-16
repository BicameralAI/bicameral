"""Tests for Google public app config and authenticated credential bootstrap."""

from __future__ import annotations

from datetime import datetime

from fastapi.testclient import TestClient

from bicameral_cloud.app import app
from bicameral_cloud.config import CloudSettings, reset_settings


def _settings(**overrides: object) -> CloudSettings:
    values: dict[str, object] = {
        "supabase_url": None,
        "supabase_jwt_secret": None,
        "supabase_anon_key": None,
        "supabase_service_key": None,
        "google_oauth_native_client_id": "123456.apps.googleusercontent.com",
        "google_oauth_environment": "alpha",
        "google_oauth_config_version": "google-alpha-7",
        "google_oauth_native_client_secret": "desktop-provider-credential",
        "google_oauth_scope_set_version": "drive-file-v1",
        "google_oauth_scopes": ("https://www.googleapis.com/auth/drive.file",),
        "google_oauth_app_config_ttl_seconds": 3600,
        "google_oauth_app_config_enabled": True,
    }
    values.update(overrides)
    return CloudSettings(**values)  # type: ignore[arg-type]


def test_google_app_config_is_bounded_non_secret_contract() -> None:
    reset_settings(_settings())
    client = TestClient(app)

    response = client.get("/api/v1/provider-app-config/google")

    assert response.status_code == 200
    assert response.headers["cache-control"] == "no-store"
    payload = response.json()
    assert payload["schema_version"] == 1
    assert payload["provider"] == "google"
    assert payload["client_type"] == "native"
    assert payload["client_id"] == "123456.apps.googleusercontent.com"
    assert payload["environment"] == "alpha"
    assert payload["config_version"] == "google-alpha-7"
    assert payload["scope_set_version"] == "drive-file-v1"
    assert payload["scopes"] == ["https://www.googleapis.com/auth/drive.file"]
    assert payload["enabled"] is True
    expires_at = datetime.fromisoformat(payload["expires_at"].replace("Z", "+00:00"))
    issued_at = datetime.fromisoformat(payload["issued_at"].replace("Z", "+00:00"))
    assert expires_at > issued_at

    forbidden = {
        "client_secret",
        "authorization_code",
        "access_token",
        "refresh_token",
        "pkce_verifier",
        "connection",
        "connection_registry",
    }
    assert forbidden.isdisjoint(payload)
    assert "desktop-provider-credential" not in response.text


def test_google_credential_rejects_invalid_cloud_bearer(seeded_store) -> None:
    reset_settings(_settings())
    client = TestClient(app)

    response = client.get(
        "/api/v1/provider-app-credential/google",
        headers={"Authorization": "Bearer invalid-token"},
    )

    assert response.status_code == 401
    assert "desktop-provider-credential" not in response.text


def test_google_credential_returns_secret_only_to_authenticated_user(
    seeded_store,
) -> None:
    reset_settings(_settings())
    client = TestClient(app)

    response = client.get(
        "/api/v1/provider-app-credential/google",
        headers={"Authorization": "Bearer tok-member-1"},
    )

    assert response.status_code == 200
    assert response.headers["cache-control"] == "no-store"
    assert response.headers["pragma"] == "no-cache"
    payload = response.json()
    assert payload == {
        "schema_version": 1,
        "provider": "google",
        "client_type": "native",
        "client_id": "123456.apps.googleusercontent.com",
        "environment": "alpha",
        "config_version": "google-alpha-7",
        "client_secret": "desktop-provider-credential",
    }
    forbidden = {
        "authorization_code",
        "access_token",
        "refresh_token",
        "pkce_verifier",
        "connection",
        "connection_registry",
    }
    assert forbidden.isdisjoint(payload)


def test_google_credential_fails_closed_when_secret_not_provisioned(
    seeded_store,
) -> None:
    reset_settings(_settings(google_oauth_native_client_secret=None))
    client = TestClient(app)

    response = client.get(
        "/api/v1/provider-app-credential/google",
        headers={"Authorization": "Bearer tok-member-1"},
    )

    assert response.status_code == 503
    assert "client_secret" in response.json()["detail"]
    assert "client_secret" not in response.json()


def test_credential_binding_matches_public_app_config(seeded_store) -> None:
    reset_settings(_settings(google_oauth_config_version=None))
    client = TestClient(app)

    public = client.get("/api/v1/provider-app-config/google")
    credential = client.get(
        "/api/v1/provider-app-credential/google",
        headers={"Authorization": "Bearer tok-member-1"},
    )

    assert public.status_code == 200
    assert credential.status_code == 200
    for key in ("client_id", "environment", "config_version"):
        assert credential.json()[key] == public.json()[key]


def test_missing_operator_provisioning_is_503_not_partial_payload() -> None:
    reset_settings(_settings(google_oauth_native_client_id=None))
    client = TestClient(app)

    response = client.get("/api/v1/provider-app-config/google")

    assert response.status_code == 503
    assert "client_id" in response.json()["detail"]


def test_explicit_disablement_is_terminal_410() -> None:
    reset_settings(_settings(google_oauth_app_config_enabled=False))
    client = TestClient(app)

    response = client.get("/api/v1/provider-app-config/google")

    assert response.status_code == 410
    assert "disabled" in response.json()["detail"].lower()


def test_derived_config_version_is_stable_and_rotates_with_identity() -> None:
    reset_settings(_settings(google_oauth_config_version=None))
    client = TestClient(app)
    first = client.get("/api/v1/provider-app-config/google")
    assert first.status_code == 200
    first_version = first.json()["config_version"]
    assert first_version.startswith("sha256:")

    reset_settings(
        _settings(
            google_oauth_config_version=None,
            google_oauth_native_client_id="rotated.apps.googleusercontent.com",
        )
    )
    second = client.get("/api/v1/provider-app-config/google")
    assert second.status_code == 200
    assert second.json()["config_version"] != first_version


def test_empty_scope_policy_fails_closed() -> None:
    reset_settings(_settings(google_oauth_scopes=()))
    client = TestClient(app)

    response = client.get("/api/v1/provider-app-config/google")

    assert response.status_code == 503
    assert "scopes" in response.json()["detail"]
