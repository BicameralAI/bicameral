from bicameral_cloud.routes.connect_local_accountability import (
    LINUX_BOOTSTRAP_COMMAND,
    LOCAL_STATUS_PATH,
    LOCAL_STATUS_SCHEMA,
    _CONNECT_HTML,
)
from bicameral_cloud.routes.connect_onboarding import _CONNECT_HTML as _BASE_CONNECT_HTML


def test_linux_bootstrap_owns_missing_downloader_prerequisite() -> None:
    assert 'command -v curl' in LINUX_BOOTSTRAP_COMMAND
    assert 'command -v wget' in LINUX_BOOTSTRAP_COMMAND
    assert 'command -v apt-get' in LINUX_BOOTSTRAP_COMMAND
    assert 'sudo apt-get install -y curl' in LINUX_BOOTSTRAP_COMMAND
    assert 'sh "$tmp"' in LINUX_BOOTSTRAP_COMMAND


def test_linux_bootstrap_does_not_pipe_package_manager_output_to_shell() -> None:
    assert 'sudo apt-get update >&2' in LINUX_BOOTSTRAP_COMMAND
    assert 'sudo apt-get install -y curl >&2' in LINUX_BOOTSTRAP_COMMAND
    assert '| sh' not in LINUX_BOOTSTRAP_COMMAND


def test_connect_uses_prerequisite_aware_command_only_for_linux() -> None:
    old = "commands = ['curl -fsSL https://install.bicameral-ai.com/install.sh | sh'];"
    assert _BASE_CONNECT_HTML.count(old) == 1
    assert _CONNECT_HTML.count('sudo apt-get install -y curl') == 1
    # macOS keeps its existing curl bootstrap; this slice changes Ubuntu/Linux only.
    assert _CONNECT_HTML.count(old) == 1
    assert _CONNECT_HTML != _BASE_CONNECT_HTML


def test_connect_prefers_daemon_authored_local_status() -> None:
    assert f"DAEMON_URL + '{LOCAL_STATUS_PATH}'" in _CONNECT_HTML
    assert LOCAL_STATUS_SCHEMA in _CONNECT_HTML
    assert 'let lastLocalStatus = null;' in _CONNECT_HTML
    assert "const options = { method:'GET', cache:'no-store' };" in _CONNECT_HTML
    assert 'body.gateway_reachable !== true' in _CONNECT_HTML
    assert 'lastLocalStatus = body;' in _CONNECT_HTML


def test_malformed_status_is_not_treated_as_connected() -> None:
    malformed_guard = (
        f"body.schema !== '{LOCAL_STATUS_SCHEMA}' || body.gateway_reachable !== true"
    )
    assert malformed_guard in _CONNECT_HTML
    guard_at = _CONNECT_HTML.index(malformed_guard)
    guarded_tail = _CONNECT_HTML[guard_at : guard_at + 300]
    assert 'lastLocalStatus = null;' in guarded_tail
    assert 'return false;' in guarded_tail


def test_legacy_probe_is_only_compatibility_fallback() -> None:
    status_fetch_at = _CONNECT_HTML.index(f"DAEMON_URL + '{LOCAL_STATUS_PATH}'")
    fallback_gate_at = _CONNECT_HTML.index('if (!statusRouteAbsent) return false;')
    legacy_probe_at = _CONNECT_HTML.index(
        'const response = await fetch(PAIR_URL, options);', fallback_gate_at
    )
    assert status_fetch_at < fallback_gate_at < legacy_probe_at
    assert "response.status === 404" in _CONNECT_HTML[status_fetch_at:fallback_gate_at]


def test_pairing_stops_on_explicit_device_connected_state() -> None:
    assert 'id="device-summary"' in _CONNECT_HTML
    assert 'id="device-environment"' in _CONNECT_HTML
    assert 'id="device-os"' in _CONNECT_HTML
    assert 'id="device-version"' in _CONNECT_HTML
    assert 'id="device-daemon"' in _CONNECT_HTML
    assert 'id="device-workspace"' in _CONNECT_HTML
    assert 'id="device-connection"' in _CONNECT_HTML
    assert 'id="device-drive"' in _CONNECT_HTML
    assert 'id="continue-drive"' in _CONNECT_HTML
    assert "'This device is connected'" in _CONNECT_HTML
    assert "'Your Bicameral daemon is running and paired with your account.'" in _CONNECT_HTML
    assert 'renderDeviceConnected();' in _CONNECT_HTML
    assert "window.setTimeout(() => { window.location.href = '/app?onboarding=drive'; }, 450);" not in _CONNECT_HTML


def test_device_connected_uses_daemon_status_and_deliberate_drive_handoff() -> None:
    assert "status.environment === 'wsl2' ? 'WSL 2'" in _CONNECT_HTML
    assert "status.environment === 'native' ? 'Native'" in _CONNECT_HTML
    assert "status.workspace_configured ? 'Configured' : 'Not configured'" in _CONNECT_HTML
    assert "status.providers && status.providers.google" in _CONNECT_HTML
    assert "continueDrive.onclick = () => { window.location.href = '/app?onboarding=drive'; };" in _CONNECT_HTML
