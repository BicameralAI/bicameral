'''Local installation accountability overlay for the hosted /connect surface.

This layer owns the customer-machine side of the hosted install journey:

1. The Linux stage-zero bootstrap must work from a supported clean Ubuntu
   baseline instead of assuming curl already exists.
2. Hosted /connect consumes daemon-authored local readiness when the current
   daemon exposes the redacted ``bicameral.local-status.v1`` contract, rather
   than treating a pairing-endpoint OPTIONS response as machine state.
3. Successful pairing stops on an explicit Device Connected state backed by
   those daemon facts. The user deliberately continues to the existing Drive
   onboarding surface instead of being redirected after a 450 ms flash.

The status payload is kept only in browser memory. Cloud gains no local
authority. A legacy OPTIONS probe remains as a compatibility fallback only when
the status route is absent/unreachable; a malformed status response is never
accepted as connected state.
'''

from __future__ import annotations

import json

from bicameral_cloud.routes.connect_onboarding import (
    _CONNECT_HTML as _BASE_CONNECT_HTML,
)

LOCAL_STATUS_PATH = '/api/v1/hosted-bridge/local-status'
LOCAL_STATUS_SCHEMA = 'bicameral.local-status.v1'

# Stage-zero bootstrap for supported apt-based Linux targets. It downloads the
# real installer to a temporary file, preferring an existing curl/wget. If the
# clean machine has neither, it installs curl with narrowly scoped sudo and
# then continues. Package-manager output is kept off the installer stream, and
# the downloaded script executes only after a successful fetch.
LINUX_BOOTSTRAP_COMMAND = (
    "sh -c 'set -eu; "
    "url=https://install.bicameral-ai.com/install.sh; "
    "tmp=$(mktemp); "
    "trap \"rm -f \\\"$tmp\\\"\" EXIT HUP INT TERM; "
    "if command -v curl >/dev/null 2>&1; then "
    "curl -fsSL \"$url\" -o \"$tmp\"; "
    "elif command -v wget >/dev/null 2>&1; then "
    "wget -q \"$url\" -O \"$tmp\"; "
    "elif command -v apt-get >/dev/null 2>&1; then "
    "echo \"Installing required download tool (curl)...\" >&2; "
    "command -v sudo >/dev/null 2>&1 || { "
    "echo \"sudo is required to install curl on this supported Linux target\" >&2; exit 1; }; "
    "sudo apt-get update >&2; "
    "sudo apt-get install -y curl >&2; "
    "curl -fsSL \"$url\" -o \"$tmp\"; "
    "else "
    "echo \"No supported downloader or package manager was found\" >&2; exit 1; "
    "fi; "
    "sh \"$tmp\"'"
)


def _apply_linux_bootstrap(html: str) -> str:
    hook = "        commands = ['curl -fsSL https://install.bicameral-ai.com/install.sh | sh'];"
    if html.count(hook) != 1:
        raise RuntimeError('Linux bootstrap accountability repair failed closed')

    linux_command = json.dumps(LINUX_BOOTSTRAP_COMMAND)
    replacement = (
        "        commands = [platform === 'linux'\n"
        f"          ? {linux_command}\n"
        "          : 'curl -fsSL https://install.bicameral-ai.com/install.sh | sh'];"
    )
    return html.replace(hook, replacement, 1)


_LEGACY_PROBE_HOOK = '''    async function probeDaemon(timeoutMs) {
      const controller = typeof AbortController === 'function' ? new AbortController() : null;
      const timer = controller ? window.setTimeout(() => controller.abort(), timeoutMs) : null;
      try {
        const options = { method:'OPTIONS', cache:'no-store' };
        if (controller) options.signal = controller.signal;
        const response = await fetch(PAIR_URL, options);
        return response.ok || response.status === 204;
      } catch (_) {
        return false;
      } finally {
        if (timer) window.clearTimeout(timer);
      }
    }'''

_LOCAL_STATUS_PROBE = f'''    let lastLocalStatus = null;
    async function probeDaemon(timeoutMs) {{
      const statusController = typeof AbortController === 'function' ? new AbortController() : null;
      const statusTimer = statusController ? window.setTimeout(() => statusController.abort(), timeoutMs) : null;
      let statusRouteAbsent = false;
      try {{
        const options = {{ method:'GET', cache:'no-store' }};
        if (statusController) options.signal = statusController.signal;
        const response = await fetch(DAEMON_URL + {json.dumps(LOCAL_STATUS_PATH)}, options);
        if (response.status === 404) {{
          statusRouteAbsent = true;
        }} else if (response.ok) {{
          const body = await response.json().catch(() => null);
          if (!body || body.schema !== {json.dumps(LOCAL_STATUS_SCHEMA)} || body.gateway_reachable !== true) {{
            lastLocalStatus = null;
            return false;
          }}
          lastLocalStatus = body;
          return true;
        }} else {{
          lastLocalStatus = null;
          return false;
        }}
      }} catch (_) {{
        // An older daemon may not expose the browser status route (including
        // a CORS-blocked 404). Only that compatibility case may use the old
        // reachability probe below.
        statusRouteAbsent = true;
      }} finally {{
        if (statusTimer) window.clearTimeout(statusTimer);
      }}

      lastLocalStatus = null;
      if (!statusRouteAbsent) return false;

      const legacyController = typeof AbortController === 'function' ? new AbortController() : null;
      const legacyTimer = legacyController ? window.setTimeout(() => legacyController.abort(), timeoutMs) : null;
      try {{
        const options = {{ method:'OPTIONS', cache:'no-store' }};
        if (legacyController) options.signal = legacyController.signal;
        const response = await fetch(PAIR_URL, options);
        return response.ok || response.status === 204;
      }} catch (_) {{
        return false;
      }} finally {{
        if (legacyTimer) window.clearTimeout(legacyTimer);
      }}
    }}'''


def _apply_local_status_probe(html: str) -> str:
    count = html.count(_LEGACY_PROBE_HOOK)
    if count != 1:
        raise RuntimeError(
            f'Local status accountability repair failed closed: legacy probe matched {count} times'
        )
    return html.replace(_LEGACY_PROBE_HOOK, _LOCAL_STATUS_PROBE, 1)


_DEVICE_STYLE = '''
    .device-summary { margin:18px 0 0; border:1px solid #bbf7d0; background:#f0fdf4; border-radius:10px; overflow:hidden; }
    .device-summary-row { display:flex; align-items:center; justify-content:space-between; gap:18px; padding:10px 14px; border-bottom:1px solid #dcfce7; }
    .device-summary-row:last-child { border-bottom:0; }
    .device-summary-row span { color:var(--muted); font-size:.86rem; }
    .device-summary-row strong { text-align:right; font-size:.88rem; font-weight:700; }
    .device-summary-row .good { color:var(--ok); }
'''

_DEVICE_MARKUP = '''        <div id="device-summary" class="device-summary" hidden aria-label="Connected device status">
          <div class="device-summary-row"><span>Environment</span><strong id="device-environment">Detected locally</strong></div>
          <div class="device-summary-row"><span>OS</span><strong id="device-os">Detected locally</strong></div>
          <div class="device-summary-row"><span>Version</span><strong id="device-version">Installed</strong></div>
          <div class="device-summary-row"><span>Daemon</span><strong id="device-daemon" class="good">Running</strong></div>
          <div class="device-summary-row"><span>Workspace</span><strong id="device-workspace">Unknown</strong></div>
          <div class="device-summary-row"><span>Status</span><strong id="device-connection" class="good">Connected</strong></div>
          <div class="device-summary-row"><span>Google Drive</span><strong id="device-drive">Not connected yet</strong></div>
        </div>
'''

# Raw string is intentional. This contains JavaScript regular expressions; a
# normal Python string would translate `\b` into a backspace and corrupt the
# browser-side word-boundary expression.
_DEVICE_FUNCTIONS = r'''
    function localStatusLabel(value) {
      if (!value) return 'Detected locally';
      return String(value)
        .replace(/_/g, ' ')
        .replace(/\b\w/g, (letter) => letter.toUpperCase());
    }
    function renderDeviceConnected() {
      const status = lastLocalStatus;
      const summary = document.getElementById('device-summary');
      const pairing = document.getElementById('pairing');
      const platform = document.getElementById('platform');
      const setup = document.getElementById('setup-inline');
      const troubleshooting = document.getElementById('developer-help');
      const continueDrive = document.getElementById('continue-drive');

      if (pairing) pairing.hidden = true;
      if (platform) platform.hidden = true;
      if (setup) setup.hidden = true;
      if (troubleshooting) troubleshooting.hidden = true;
      showActions();

      setView(
        'ok',
        'Device connected',
        'This device is connected',
        'Your Bicameral daemon is running and paired with your account.'
      );

      const deviceStep = document.getElementById('device-step');
      const deviceMark = document.getElementById('device-mark');
      const deviceDetail = document.getElementById('device-detail');
      const driveStep = document.getElementById('drive-step');
      if (deviceStep) deviceStep.dataset.state = 'done';
      if (deviceMark) deviceMark.textContent = '✓';
      if (deviceDetail) deviceDetail.textContent = 'Device connected to Bicameral Cloud.';
      if (driveStep) driveStep.dataset.state = 'active';

      if (status) {
        const environment = status.environment === 'wsl2' ? 'WSL 2' : status.environment === 'native' ? 'Native' : localStatusLabel(status.environment);
        const os = [localStatusLabel(status.os), status.arch].filter(Boolean).join(' · ');
        document.getElementById('device-environment').textContent = environment;
        document.getElementById('device-os').textContent = os || 'Detected locally';
        document.getElementById('device-version').textContent = status.version || 'Installed';
        document.getElementById('device-workspace').textContent = status.workspace_configured ? 'Configured' : 'Not configured';
        const google = status.providers && status.providers.google;
        document.getElementById('device-drive').textContent = google === 'connected'
          ? 'Connected'
          : google === 'action_needed'
            ? 'Needs attention'
            : 'Not connected yet';
      } else {
        document.getElementById('device-environment').textContent = 'Legacy daemon';
        document.getElementById('device-os').textContent = 'Detected locally';
        document.getElementById('device-version').textContent = 'Update recommended';
        document.getElementById('device-workspace').textContent = 'Not reported';
        document.getElementById('device-drive').textContent = 'Not connected yet';
      }

      if (summary) summary.hidden = false;
      if (continueDrive) {
        continueDrive.hidden = false;
        continueDrive.onclick = () => { window.location.href = '/app?onboarding=drive'; };
      }
    }
'''


def _apply_device_connected_step(html: str) -> str:
    style_hook = '</style>'
    markup_hook = '        <details id="developer-help">'
    action_hook = '          <button id="cancel" class="btn secondary" type="button" hidden>Cancel</button>'
    status_hook = '    let lastLocalStatus = null;'
    redirect_hook = "        window.setTimeout(() => { window.location.href = '/app?onboarding=drive'; }, 450);"

    expected = {
        'style': html.count(style_hook),
        'markup': html.count(markup_hook),
        'action': html.count(action_hook),
        'status': html.count(status_hook),
        'redirect': html.count(redirect_hook),
    }
    invalid = {name: count for name, count in expected.items() if count != 1}
    if invalid:
        raise RuntimeError(
            f'Device-connected accountability repair failed closed: hook counts {invalid!r}'
        )

    html = html.replace(style_hook, _DEVICE_STYLE + '  </style>', 1)
    html = html.replace(markup_hook, _DEVICE_MARKUP + markup_hook, 1)
    html = html.replace(
        action_hook,
        '          <button id="continue-drive" class="btn primary" type="button" hidden>Continue to Google Drive →</button>\n'
        + action_hook,
        1,
    )
    html = html.replace(status_hook, status_hook + _DEVICE_FUNCTIONS, 1)
    html = html.replace(redirect_hook, '        renderDeviceConnected();', 1)
    return html


def _render_connect_html() -> str:
    html = _apply_linux_bootstrap(_BASE_CONNECT_HTML)
    html = _apply_local_status_probe(html)
    html = _apply_device_connected_step(html)
    return html


_CONNECT_HTML = _render_connect_html()
