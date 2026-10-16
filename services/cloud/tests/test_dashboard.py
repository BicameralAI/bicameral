"""Route and serving smoke for hosted shell surfaces (issue #38).

Deliberately not a UI snapshot suite: progressive-connect copy, rail presence,
and client-side state labels are product chrome and change often. Pairing
protocol and browser behavior live in conformance / journey harnesses when
needed — not as GET-/connect HTML string scrapes.
"""

from __future__ import annotations


def test_root_redirects_to_signup(client, store):
    resp = client.get("/", follow_redirects=False)

    assert resp.status_code == 307
    assert resp.headers["location"] == "/signup"


def test_signup_renders_html(client, store):
    resp = client.get("/signup")

    assert resp.status_code == 200
    assert "text/html" in resp.headers["content-type"]
    assert "Continue with Google" in resp.text


def test_login_alias_renders_signup(client, store):
    resp = client.get("/login")

    assert resp.status_code == 200
    assert "Continue with Google" in resp.text


def test_callback_renders_html(client, store):
    resp = client.get("/auth/callback")

    assert resp.status_code == 200
    assert "text/html" in resp.headers["content-type"]


def test_connect_renders_html_without_auth(client, store):
    """Bootstrap shell loads without a session (client JS gates the signed-in body)."""
    resp = client.get("/connect")

    assert resp.status_code == 200
    assert "text/html" in resp.headers["content-type"]


def test_restore_renders_quiet_rebind_shell(client, store):
    """Cross-visit restore is separate from the install/pair bootstrap."""
    resp = client.get("/restore")

    assert resp.status_code == 200
    assert "text/html" in resp.headers["content-type"]
    assert "Restoring your local connection" in resp.text
    assert "bicameral_hosted_bridge_restore" in resp.text
    assert "curl -fsSL" not in resp.text
    assert "pairing code" not in resp.text.lower()


def test_dashboard_route_is_gone(client, store):
    """Product path is /connect only — /dashboard must not be a browser link."""
    resp = client.get("/dashboard", follow_redirects=False)

    assert resp.status_code == 404


def test_connect_does_not_expose_provider_secrets(client, store):
    resp = client.get("/connect")

    forbidden_fields = [
        "provider_secret",
        "api_key",
        "refresh_token",
        "jwt_secret",
        "service_key",
        "workspace_path",
        "local_path",
        "filesystem_path",
        "source_content",
        "source_body",
        "decision_ledger_payload",
        "ledger_entry",
    ]
    for field in forbidden_fields:
        assert field not in resp.text


def test_connect_handoff_is_not_written_to_local_storage(client, store):
    """Authority-bearing bridge handoff must remain transient."""
    resp = client.get("/connect")

    assert "localStorage.setItem('bicameral_hosted_bridge'" not in resp.text
    assert "session_token=" not in resp.text


def test_connect_durable_restore_record_contains_proof_not_tokens(client, store):
    """#1109 durable state is bounded rebind proof, never a token vault."""
    resp = client.get("/connect")

    marker = "window.localStorage.setItem(RESTORE_KEY, JSON.stringify({"
    assert marker in resp.text
    restore_payload = resp.text.split(marker, 1)[1].split("}));", 1)[0]
    assert "request_handle" in restore_payload
    assert "poll_proof" in restore_payload
    assert "cloud_account_id" in restore_payload
    assert "expires_at" in restore_payload
    assert "session_token" not in restore_payload
    assert "cloud_access_token" not in restore_payload


def test_restore_reauthenticates_cloud_and_revalidates_daemon_status(client, store):
    resp = client.get("/restore")

    assert "/api/v1/auth/session" in resp.text
    assert "/api/v1/hosted-bridge/pair/status?" in resp.text
    assert "cloud_account_id" in resp.text
    assert "window.sessionStorage.setItem(HANDOFF_KEY" in resp.text


# ── Cloud serves the dashboard SPA (cloud serves UI) ────────────────────────


def test_app_serves_spa_index(client, store):
    resp = client.get("/app")

    assert resp.status_code == 200
    assert "text/html" in resp.headers["content-type"]
    assert "/assets/index-" in resp.text


def test_app_client_route_falls_back_to_index(client, store):
    resp = client.get("/app/decisions")

    assert resp.status_code == 200
    assert "/assets/index-" in resp.text


def test_spa_assets_are_served(client, store):
    index = client.get("/app").text
    import re

    match = re.search(r"/assets/(index-[^\"']+\.js)", index)
    assert match, "SPA index should reference hashed JS bundle"

    asset = client.get(match.group(0))
    assert asset.status_code == 200
    assert "javascript" in asset.headers["content-type"]


def test_app_version_metadata_is_served(client, store):
    resp = client.get("/app/version.json")

    assert resp.status_code == 200
    assert resp.headers["content-type"].startswith("application/json")
    assert resp.headers["cache-control"] == "no-store"
    body = resp.json()
    assert body["source_repository"] == "BicameralAI/bicameral-bot"
    assert body["bot_commit"]
    assert body["bundle_sha256"]
    assert len(body["hosted_bridge_contract_sha256"]) == 64
    assert body["built_at"]

    from datetime import datetime

    datetime.fromisoformat(body["built_at"].replace("Z", "+00:00"))


def test_release_identity_is_machine_readable_and_uncached(
    client, store, monkeypatch, tmp_path
):
    import bicameral_cloud.app as app_module

    identity = tmp_path / "release-identity.json"
    identity.write_text(
        '{"schema_version":1,"release_id":"release-123","components":{}}',
        encoding="utf-8",
    )
    monkeypatch.setattr(app_module, "_SPA_RELEASE_IDENTITY", identity)

    response = client.get("/app/release-identity.json")

    assert response.status_code == 200
    assert response.headers["content-type"].startswith("application/json")
    assert response.headers["cache-control"] == "no-store"
    assert response.json()["release_id"] == "release-123"


def test_app_version_json_is_metadata_not_spa_fallback(client, store):
    resp = client.get("/app/version.json")

    assert resp.status_code == 200
    assert resp.headers["content-type"].startswith("application/json")
    assert "<!doctype html" not in resp.text.lower()
    assert "/assets/index-" not in resp.text
