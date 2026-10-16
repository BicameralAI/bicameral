"""Tests for auth routes (issue #16).

Covers session info, my-teams, Supabase JWT validation path, and
token rejection.
"""

from __future__ import annotations

import time

import jwt

from bicameral_cloud.config import CloudSettings, reset_settings
from tests.conftest import TEAM_ID, TOKEN_MEMBER, TOKEN_OUTSIDER, USER_ID

SUPABASE_TEST_SECRET = "test-jwt-secret-at-least-32-chars-long"


def test_session_info_returns_user(client, seeded_store):
    resp = client.get(
        "/api/v1/auth/session",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 200
    data = resp.json()
    assert data["user_id"] == USER_ID
    assert data["email"] == "member@example.com"


def test_session_info_rejects_invalid_token(client, store):
    resp = client.get(
        "/api/v1/auth/session",
        headers={"Authorization": "Bearer bad-token"},
    )
    assert resp.status_code == 401


def test_session_info_rejects_missing_bearer(client, store):
    resp = client.get(
        "/api/v1/auth/session",
        headers={"Authorization": "Basic abc123"},
    )
    assert resp.status_code == 401


def test_my_teams_empty(client, seeded_store):
    resp = client.get(
        "/api/v1/auth/me/teams",
        headers={"Authorization": f"Bearer {TOKEN_OUTSIDER}"},
    )
    assert resp.status_code == 200
    data = resp.json()
    assert data["teams"] == []


def test_my_teams_returns_memberships(client, seeded_store):
    resp = client.get(
        "/api/v1/auth/me/teams",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 200
    data = resp.json()
    assert data["user_id"] == USER_ID
    assert len(data["teams"]) == 1
    assert data["teams"][0]["team_id"] == TEAM_ID
    assert data["teams"][0]["role"] == "team_member"


class TestSupabaseJWTAuth:
    """Supabase JWT validation path tests."""

    def _make_jwt(
        self,
        sub: str = "sb-user-1",
        email: str = "alice@example.com",
        exp_offset: int = 3600,
        aud: str = "authenticated",
        metadata: dict[str, str] | None = None,
    ) -> str:
        payload = {
            "sub": sub,
            "email": email,
            "aud": aud,
            "exp": int(time.time()) + exp_offset,
            "iat": int(time.time()),
        }
        if metadata:
            payload["user_metadata"] = metadata
        return jwt.encode(payload, SUPABASE_TEST_SECRET, algorithm="HS256")

    def _enable_supabase(self) -> None:
        reset_settings(
            CloudSettings(
                supabase_url="https://test.supabase.co",
                supabase_jwt_secret=SUPABASE_TEST_SECRET,
                supabase_anon_key=None,
                supabase_service_key=None,
            )
        )

    def _enable_supabase_with_allowlisted_domain(self, monkeypatch) -> list[str]:
        """Enable Supabase auth with a service key, so the design-partner
        allowlist is consulted, and answer its lookup with an allowed domain.

        Returns the domains the allowlist was asked about."""
        import httpx

        queried: list[str] = []

        def fake_get(url, params, headers, timeout):
            queried.append(params["domain"])
            return httpx.Response(
                200, json=[{"id": "allow-domain"}], request=httpx.Request("GET", url)
            )

        monkeypatch.setattr(
            "bicameral_cloud.auth.design_partner_allowlist.httpx.get", fake_get
        )
        reset_settings(
            CloudSettings(
                supabase_url="https://test.supabase.co",
                supabase_jwt_secret=SUPABASE_TEST_SECRET,
                supabase_anon_key=None,
                supabase_service_key="service-role-key",
            )
        )
        return queried

    def _disable_supabase(self) -> None:
        reset_settings(
            CloudSettings(
                supabase_url=None,
                supabase_jwt_secret=None,
                supabase_anon_key=None,
                supabase_service_key=None,
            )
        )

    def test_valid_jwt_resolves_user(self, client, store, monkeypatch):
        queried = self._enable_supabase_with_allowlisted_domain(monkeypatch)
        try:
            token = self._make_jwt(
                sub="sb-user-1",
                email="alice@example.com",
                metadata={"full_name": "Alice"},
            )
            resp = client.get(
                "/api/v1/auth/session",
                headers={"Authorization": f"Bearer {token}"},
            )
            assert resp.status_code == 200
            data = resp.json()
            assert data["user_id"] == "sb-user-1"
            assert data["email"] == "alice@example.com"
            assert data["display_name"] == "Alice"
            assert queried == ["eq.example.com"]
        finally:
            self._disable_supabase()

    def test_expired_jwt_rejected(self, client, store):
        self._enable_supabase()
        try:
            token = self._make_jwt(exp_offset=-100)
            resp = client.get(
                "/api/v1/auth/session",
                headers={"Authorization": f"Bearer {token}"},
            )
            assert resp.status_code == 401
        finally:
            self._disable_supabase()

    def test_wrong_secret_rejected(self, client, store):
        self._enable_supabase()
        try:
            token = jwt.encode(
                {
                    "sub": "user",
                    "email": "x@x.com",
                    "aud": "authenticated",
                    "exp": int(time.time()) + 3600,
                },
                "wrong-secret-that-is-32-chars-long!!",
                algorithm="HS256",
            )
            resp = client.get(
                "/api/v1/auth/session",
                headers={"Authorization": f"Bearer {token}"},
            )
            assert resp.status_code == 401
        finally:
            self._disable_supabase()

    def test_wrong_audience_rejected(self, client, store):
        self._enable_supabase()
        try:
            token = self._make_jwt(aud="anon")
            resp = client.get(
                "/api/v1/auth/session",
                headers={"Authorization": f"Bearer {token}"},
            )
            assert resp.status_code == 401
        finally:
            self._disable_supabase()

    def test_jwt_user_upserted_to_store(self, client, store, monkeypatch):
        queried = self._enable_supabase_with_allowlisted_domain(monkeypatch)
        try:
            token = self._make_jwt(sub="sb-new-user", email="new@example.com")
            resp = client.get(
                "/api/v1/auth/session",
                headers={"Authorization": f"Bearer {token}"},
            )
            assert resp.status_code == 200
            assert store.get_user_by_id("sb-new-user") is not None
            assert queried == ["eq.example.com"]
        finally:
            self._disable_supabase()


def test_auth_config_returns_public_supabase_config(client, store):
    reset_settings(
        CloudSettings(
            supabase_url="https://example.supabase.co",
            supabase_jwt_secret="server-secret-not-public",
            supabase_anon_key="public-anon-key",
            supabase_service_key="service-key-not-public",
        )
    )

    resp = client.get("/api/v1/auth/config")

    assert resp.status_code == 200
    data = resp.json()
    assert data == {
        "supabase_url": "https://example.supabase.co",
        "supabase_anon_key": "public-anon-key",
        "supabase_auth_enabled": True,
        "supabase_browser_auth_configured": True,
    }
    assert "server-secret-not-public" not in resp.text
    assert "service-key-not-public" not in resp.text
    assert "bicameral-ai.com" not in resp.text


def test_auth_config_reports_missing_browser_config(client, store):
    reset_settings(
        CloudSettings(
            supabase_url=None,
            supabase_jwt_secret=None,
            supabase_anon_key=None,
            supabase_service_key=None,
        )
    )

    resp = client.get("/api/v1/auth/config")

    assert resp.status_code == 200
    assert resp.json()["supabase_browser_auth_configured"] is False


class TestDesignPartnerDomainGate:
    def _make_jwt(self, email: str) -> str:
        return jwt.encode(
            {
                "sub": "sb-domain-user",
                "email": email,
                "aud": "authenticated",
                "exp": int(time.time()) + 3600,
            },
            SUPABASE_TEST_SECRET,
            algorithm="HS256",
        )

    def _enable_supabase_without_allowlist(self) -> None:
        reset_settings(
            CloudSettings(
                supabase_url="https://example.supabase.co",
                supabase_jwt_secret=SUPABASE_TEST_SECRET,
                supabase_anon_key="anon-key",
                supabase_service_key=None,
            )
        )

    def _disable_supabase(self) -> None:
        reset_settings(
            CloudSettings(
                supabase_url=None,
                supabase_jwt_secret=None,
                supabase_anon_key=None,
                supabase_service_key=None,
            )
        )

    def test_supabase_auth_without_allowlist_service_key_fails_closed(
        self, client, store
    ):
        self._enable_supabase_without_allowlist()
        try:
            token = self._make_jwt("alice@bicameral-ai.com")
            resp = client.get(
                "/api/v1/auth/session",
                headers={"Authorization": f"Bearer {token}"},
            )
            assert resp.status_code == 503
            assert resp.json()["detail"] == "Design partner allowlist unavailable"
        finally:
            self._disable_supabase()


def _enable_supabase_auth_api() -> None:
    reset_settings(
        CloudSettings(
            supabase_url="https://example.supabase.co",
            supabase_jwt_secret=None,
            supabase_anon_key="anon-key",
            supabase_service_key="service-role-key",
        )
    )


def test_supabase_auth_api_token_validation(client, store, monkeypatch):
    import httpx

    calls = []

    def fake_get(url, headers, timeout, params=None):
        calls.append(
            {"url": url, "headers": headers, "timeout": timeout, "params": params}
        )
        if url.endswith("/auth/v1/user"):
            return httpx.Response(
                200,
                json={
                    "id": "sb-api-user",
                    "email": "alice@bicameral-ai.com",
                    "user_metadata": {"full_name": "Alice Partner"},
                },
            )
        return httpx.Response(200, json=[{"id": "allow-domain"}])

    monkeypatch.setattr("bicameral_cloud.auth.supabase.httpx.get", fake_get)
    _enable_supabase_auth_api()
    try:
        resp = client.get(
            "/api/v1/auth/session",
            headers={"Authorization": "Bearer live-access-token"},
        )
        assert resp.status_code == 200
        assert resp.json() == {
            "user_id": "sb-api-user",
            "email": "alice@bicameral-ai.com",
            "display_name": "Alice Partner",
        }
        assert store.get_user_by_id("sb-api-user") is not None
        assert calls == [
            {
                "url": "https://example.supabase.co/auth/v1/user",
                "headers": {
                    "apikey": "anon-key",
                    "authorization": "Bearer live-access-token",
                    "accept": "application/json",
                },
                "timeout": 5.0,
                "params": None,
            },
            {
                "url": "https://example.supabase.co/rest/v1/design_partner_allowlist",
                "headers": {
                    "apikey": "service-role-key",
                    "authorization": "Bearer service-role-key",
                    "accept": "application/json",
                },
                "timeout": 5.0,
                "params": {
                    "select": "id",
                    "enabled": "eq.true",
                    "domain": "eq.bicameral-ai.com",
                    "limit": "1",
                },
            },
        ]
    finally:
        reset_settings()


def test_supabase_auth_api_rejection_returns_401(client, store, monkeypatch):
    import httpx

    def fake_get(url, headers, timeout):
        return httpx.Response(401, json={"msg": "invalid JWT"})

    monkeypatch.setattr("bicameral_cloud.auth.supabase.httpx.get", fake_get)
    _enable_supabase_auth_api()
    try:
        resp = client.get(
            "/api/v1/auth/session",
            headers={"Authorization": "Bearer bad-token"},
        )
        assert resp.status_code == 401
        assert resp.json()["detail"] == "Invalid or expired token"
    finally:
        reset_settings()


def test_auth_config_enabled_with_anon_key_without_jwt_secret(client, store):
    reset_settings(
        CloudSettings(
            supabase_url="https://example.supabase.co",
            supabase_jwt_secret=None,
            supabase_anon_key="public-anon-key",
            supabase_service_key=None,
        )
    )
    try:
        resp = client.get("/api/v1/auth/config")
        assert resp.status_code == 200
        assert resp.json() == {
            "supabase_url": "https://example.supabase.co",
            "supabase_anon_key": "public-anon-key",
            "supabase_auth_enabled": True,
            "supabase_browser_auth_configured": True,
        }
    finally:
        reset_settings()


def _make_allowlist_jwt(email: str, sub: str = "sb-allowlist-user") -> str:
    return jwt.encode(
        {
            "sub": sub,
            "email": email,
            "aud": "authenticated",
            "exp": int(time.time()) + 3600,
        },
        SUPABASE_TEST_SECRET,
        algorithm="HS256",
    )


def _enable_supabase_table_allowlist() -> None:
    reset_settings(
        CloudSettings(
            supabase_url="https://example.supabase.co",
            supabase_jwt_secret=SUPABASE_TEST_SECRET,
            supabase_anon_key="anon-key",
            supabase_service_key="service-role-key",
        )
    )


def test_supabase_allowlist_table_allows_domain(client, store, monkeypatch):
    import httpx

    calls = []

    def fake_get(url, params, headers, timeout):
        calls.append(
            {"url": url, "params": params, "headers": headers, "timeout": timeout}
        )
        return httpx.Response(200, json=[{"id": "allow-domain"}])

    monkeypatch.setattr(
        "bicameral_cloud.auth.design_partner_allowlist.httpx.get", fake_get
    )
    _enable_supabase_table_allowlist()
    try:
        token = _make_allowlist_jwt("alice@bicameral-ai.com")
        resp = client.get(
            "/api/v1/auth/session",
            headers={"Authorization": f"Bearer {token}"},
        )
        assert resp.status_code == 200
        assert resp.json()["email"] == "alice@bicameral-ai.com"
        assert calls[0]["url"] == (
            "https://example.supabase.co/rest/v1/design_partner_allowlist"
        )
        assert calls[0]["params"] == {
            "select": "id",
            "enabled": "eq.true",
            "domain": "eq.bicameral-ai.com",
            "limit": "1",
        }
        assert calls[0]["headers"]["authorization"] == "Bearer service-role-key"
    finally:
        reset_settings()


def test_supabase_allowlist_table_rejects_missing_domain_row(
    client, store, monkeypatch
):
    import httpx

    def fake_get(url, params, headers, timeout):
        return httpx.Response(200, json=[])

    monkeypatch.setattr(
        "bicameral_cloud.auth.design_partner_allowlist.httpx.get", fake_get
    )
    _enable_supabase_table_allowlist()
    try:
        token = _make_allowlist_jwt("alice@bicameral-ai.com")
        resp = client.get(
            "/api/v1/auth/session",
            headers={"Authorization": f"Bearer {token}"},
        )
        assert resp.status_code == 403
        assert resp.json()["detail"] == "Design partner access required"
    finally:
        reset_settings()


def test_supabase_allowlist_table_failure_fails_closed(client, store, monkeypatch):
    import httpx

    def fake_get(url, params, headers, timeout):
        raise httpx.ConnectError("unavailable")

    monkeypatch.setattr(
        "bicameral_cloud.auth.design_partner_allowlist.httpx.get", fake_get
    )
    _enable_supabase_table_allowlist()
    try:
        token = _make_allowlist_jwt("alice@bicameral-ai.com")
        resp = client.get(
            "/api/v1/auth/session",
            headers={"Authorization": f"Bearer {token}"},
        )
        assert resp.status_code == 503
        assert resp.json()["detail"] == "Design partner allowlist unavailable"
    finally:
        reset_settings()
