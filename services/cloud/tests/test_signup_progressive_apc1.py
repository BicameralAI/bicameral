"""APC1 progressive signup and post-pair onboarding presentation checks."""

from __future__ import annotations


def test_signup_presents_cloud_then_device_as_sequential_steps(client, store):
    response = client.get("/signup")

    assert response.status_code == 200
    assert "Connect Bicameral" in response.text
    assert "Bicameral Cloud" in response.text
    assert "Continue with Google" in response.text
    assert "This device" in response.text
    assert "Google Drive authorization is a separate provider connection" in response.text
    assert 'href="/connect"' not in response.text


def test_login_alias_uses_same_progressive_signup_surface(client, store):
    response = client.get("/login")

    assert response.status_code == 200
    assert "Connect Bicameral" in response.text
    assert "Continue with Google" in response.text
    assert 'href="/connect"' not in response.text


def test_connect_owns_google_signin_without_signup_detour(client, store):
    response = client.get("/connect")

    assert response.status_code == 200
    assert 'id="google-signin"' in response.text
    assert "Continue with Google" in response.text
    assert "signInWithOAuth" in response.text
    assert "provider:'google'" in response.text
    assert "window.location.origin + '/auth/callback'" in response.text
    assert 'href="/signup">Sign in</a>' not in response.text


def test_connect_presents_truthful_progressive_storyboard(client, store):
    response = client.get("/connect")

    assert response.status_code == 200
    assert 'id="cloud-step"' in response.text
    assert 'id="device-step"' in response.text
    assert 'id="drive-step"' in response.text
    assert "Connected to Bicameral Cloud as " in response.text
    assert "Device connected to Bicameral Cloud." in response.text
    assert "Authorize Drive after this device is connected." in response.text


def test_connect_missing_daemon_keeps_setup_and_retry_on_same_page(client, store):
    response = client.get("/connect")

    assert response.status_code == 200
    assert 'id="setup-inline"' in response.text
    assert "Run this in a terminal to install Bicameral and start it." in response.text
    # Stopped (seen before) is distinguished from never installed (cloud#353).
    assert "Bicameral is installed but isn’t running." in response.text
    assert "Bicameral was not found" not in response.text
    assert "curl -fsSL https://install.bicameral-ai.com/install.sh | sh" in response.text
    assert "~/.local/bin/bicameral service install" not in response.text
    assert "~/.local/bin/bicameral service start" not in response.text
    assert "I already installed it — try again" in response.text
    assert response.text.count("const label = platform === 'windows'") == 1
    # Setup guidance is revealed only by setup states, never by configuration.
    assert "function revealSetup(platform, reason)" in response.text
    assert "renderInlineSetup(platform);" not in response.text


def test_connect_handoff_carries_drive_navigation_intent_only_after_pair(client, store):
    response = client.get("/connect")

    assert response.status_code == 200
    assert "window.location.href = '/app?onboarding=drive'" in response.text
    assert "bicameral_hosted_bridge" in response.text
    assert "Connect Google Drive" not in response.text


def test_app_onboarding_intent_opens_existing_drive_settings_without_claiming_state(
    client,
    store,
):
    response = client.get("/app?onboarding=drive")

    assert response.status_code == 200
    assert 'id="bicameral-onboarding-drive"' in response.text
    assert "params.get('onboarding') !== 'drive'" in response.text
    assert '[data-testid="host-status-drive"]' in response.text
    assert "button.click()" in response.text
    assert "params.delete('onboarding')" in response.text
    # The hint may navigate to the existing Drive UI. It must never manufacture
    # provider/device status, credentials, or connection authority itself.
    assert "eventSubstrateConnected = true" not in response.text
    assert "drivePhase = \"connected\"" not in response.text
    assert "cloud_access_token" not in response.text
    assert "refresh_token" not in response.text
    assert "provider_secret" not in response.text


def test_app_without_drive_intent_keeps_same_spa_and_no_forced_navigation(client, store):
    response = client.get("/app")

    assert response.status_code == 200
    assert "/assets/index-" in response.text
    assert 'id="bicameral-onboarding-drive"' in response.text
    assert "if (params.get('onboarding') !== 'drive') return;" in response.text
