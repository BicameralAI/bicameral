'''APC1 exact-head remediation for the frictionless /connect surface.

This module deliberately layers bounded presentation/orchestration repairs over
the reviewed connect-polish HTML without changing pairing authority or protocol
semantics. It exists so the review fixes stay isolated and auditable.
'''

# Embedded HTML/JavaScript literals intentionally preserve reviewed source text.
# ruff: noqa: E501

from __future__ import annotations

from fastapi import APIRouter
from fastapi.responses import HTMLResponse

from bicameral_cloud.routes.connect_polish import _CONNECT_HTML as _BASE_CONNECT_HTML
from bicameral_cloud.routes.connect_polish import DEFAULT_DAEMON_PORT

router = APIRouter(tags=['connect-polish'])


_STATIC_COMMANDS = '''          <div class="cmd"><code>curl -fsSL https://install.bicameral-ai.com/install.sh | sh</code></div>
          <div class="cmd"><code>bicameral service install</code></div>
          <div class="cmd"><code>bicameral service start</code></div>'''

_DAEMON_LITERAL = f"    const DAEMON_URL = 'http://localhost:{DEFAULT_DAEMON_PORT}';"
_DAEMON_OVERRIDE = f'''    const DAEMON_URL = (() => {{
      let override = '';
      try {{ override = (window.localStorage.getItem('bicameral_daemon_url') || '').trim(); }} catch (_) {{ override = ''; }}
      return override ? override.replace(/\\/+$/, '') : 'http://localhost:{DEFAULT_DAEMON_PORT}';
    }})();'''

_AUTH_HOOK = '''    <div id="signin" hidden><a class="btn primary" href="/signup">Sign in</a></div>'''
_AUTH_REPAIR = '''    <div id="signin" hidden>
      <p class="overline">Bicameral Cloud</p>
      <h1>Sign in to Bicameral Cloud</h1>
      <p class="lead">Use your Google account to establish your Bicameral Cloud identity. Device pairing happens next on this same page.</p>
      <button id="google-signin" class="btn primary" type="button">Continue with Google</button>
    </div>'''

_PROGRESS_HOOK = '''      <p class="lead">We’ll look for Bicameral on this device first. If it’s already here, you’ll go straight to pairing. If it isn’t available yet, we’ll guide you through setup and keep checking in the background.</p>'''
_PROGRESS_REPAIR = _PROGRESS_HOOK + '''
      <div class="journey" aria-label="Bicameral setup progress">
        <div class="journey-step" id="cloud-step" data-state="done">
          <span class="journey-mark" id="cloud-mark">✓</span>
          <span><strong>Bicameral Cloud</strong><small id="cloud-detail">Connected with Google.</small></span>
        </div>
        <div class="journey-step" id="device-step" data-state="active">
          <span class="journey-mark" id="device-mark">2</span>
          <span><strong>This device</strong><small id="device-detail">Checking the local Bicameral service.</small></span>
        </div>
        <div class="journey-step" id="drive-step" data-state="pending">
          <span class="journey-mark" id="drive-mark">3</span>
          <span><strong>Google Drive</strong><small>Authorize Drive after this device is connected.</small></span>
        </div>
      </div>'''

_STYLE_HOOK = '''    .loading {{ color:var(--muted); }}'''
_STYLE_REPAIR = _STYLE_HOOK + '''
    .journey {{ display:grid; gap:8px; margin:0 0 22px; }}
    .journey-step {{ display:flex; align-items:flex-start; gap:11px; padding:12px 14px; border:1px solid var(--line); border-radius:10px; background:var(--surface); }}
    .journey-step strong {{ display:block; font-size:.9rem; }}
    .journey-step small {{ display:block; margin-top:2px; color:var(--muted); line-height:1.35; }}
    .journey-mark {{ width:24px; height:24px; flex:none; display:grid; place-items:center; border:1px solid var(--line); border-radius:50%; color:var(--muted); font-size:.78rem; font-weight:750; }}
    .journey-step[data-state="done"] {{ border-color:#bbf7d0; background:#f0fdf4; }}
    .journey-step[data-state="done"] .journey-mark {{ border-color:var(--ok); background:var(--ok); color:white; }}
    .journey-step[data-state="active"] {{ border-color:#a5f3fc; background:#ecfeff; }}
    .journey-step[data-state="active"] .journey-mark {{ border-color:var(--cyan); color:var(--cyan-strong); }}
    .setup-inline {{ margin:14px 0 0; padding:14px; border:1px solid var(--line); border-radius:10px; background:var(--panel); }}
    .setup-inline p {{ margin:0 0 8px; color:var(--muted); font-size:.88rem; }}'''

_SETUP_HOOK = '''        <p id="platform" class="platform" hidden></p>'''
_SETUP_REPAIR = _SETUP_HOOK + '''
        <div id="setup-inline" class="setup-inline" hidden>
          <p id="setup-copy"></p>
          <div id="setup-commands"></div>
        </div>'''

_PLATFORM_HOOK = '''    function configureInstaller() {
      const platform = detectPlatform();
      const label = platform === 'windows' ? 'Windows' : platform === 'macos' ? 'macOS' : platform === 'linux' ? 'Linux' : 'this device';
      const node = document.getElementById('platform');
      node.hidden = false;
      node.textContent = 'Detected ' + label + '. Setup will open the supported Bicameral installer experience.';
      const install = document.getElementById('install');
      install.href = INSTALL_URL;
      install.textContent = platform === 'windows' ? 'Install Bicameral for Windows' : 'Install Bicameral';
      return platform;
    }'''
_PLATFORM_REPAIR = '''    function renderTroubleshootingCommands(platform) {
      const root = document.getElementById('troubleshooting-commands');
      if (!root) return;
      const commands = platform === 'windows'
        ? ['bicameral.exe gateway start']
        : (platform === 'macos' || platform === 'linux')
          ? ['curl -fsSL https://install.bicameral-ai.com/install.sh | sh', 'bicameral update', 'bicameral gateway start']
          : ['bicameral gateway start'];
      root.replaceChildren(...commands.map((command) => {
        const div = document.createElement('div');
        div.className = 'cmd';
        const code = document.createElement('code');
        code.textContent = command;
        div.appendChild(code);
        return div;
      }));
    }
    function renderInlineSetup(platform) {
      const panel = document.getElementById('setup-inline');
      const copy = document.getElementById('setup-copy');
      const root = document.getElementById('setup-commands');
      if (!panel || !copy || !root) return;
      let commands = [];
      if (platform === 'windows') {
        copy.textContent = 'Bicameral was not found. Install the Windows client, then return here and choose Try again.';
      } else if (platform === 'linux') {
        copy.textContent = 'Bicameral was not found. Run this installer in a terminal. It will install or repair Bicameral and start the local service; then choose Try again.';
        commands = ['curl -fsSL https://install.bicameral-ai.com/install.sh | sh'];
      } else if (platform === 'macos') {
        copy.textContent = 'Bicameral was not found. Run this installer in a terminal. It will install or repair Bicameral and start the local service; then choose Try again.';
        commands = ['curl -fsSL https://install.bicameral-ai.com/install.sh | sh'];
      } else {
        copy.textContent = 'Bicameral was not found. Use the supported installer for this device, then choose Try again.';
      }
      root.replaceChildren(...commands.map((command) => {
        const div = document.createElement('div');
        div.className = 'cmd';
        const code = document.createElement('code');
        code.textContent = command;
        div.appendChild(code);
        return div;
      }));
      panel.hidden = false;
    }
    function configureInstaller() {
      const platform = detectPlatform();
      renderTroubleshootingCommands(platform);
      renderInlineSetup(platform);
      const label = platform === 'windows' ? 'Windows' : platform === 'macos' ? 'macOS' : platform === 'linux' ? 'Linux' : 'this device';
      const node = document.getElementById('platform');
      node.hidden = false;
      node.textContent = platform === 'windows'
        ? 'Detected Windows. Use the installer below, then return here.'
        : 'Detected ' + label + '. Follow the commands below, then choose Try again.';
      const install = document.getElementById('install');
      if (platform === 'windows') {
        install.href = 'https://install.bicameral-ai.com/install-windows.cmd';
        install.textContent = 'Install Bicameral for Windows';
      }
      return platform;
    }'''

_SETUP_ACTION_HOOK = '''      configureInstaller();
      const detail = lastLocalNetworkPermission === 'prompt'
        ? 'Install or start Bicameral, then leave this page open. If Chrome asks to find devices on your network, choose Allow. We’ll continue automatically when Bicameral is ready.'
        : 'Install or start Bicameral, then leave this page open. We’ll keep checking and continue automatically when it is ready.';
      setView('warn','Setup needed','Bicameral isn’t available on this device yet.',detail);
      showActions({ install:true, retry:true });'''
_SETUP_ACTION_REPAIR = '''      const platform = configureInstaller();
      const permissionHint = lastLocalNetworkPermission === 'prompt'
        ? ' If Chrome asks to find devices on your network, choose Allow.'
        : '';
      const detail = platform === 'windows'
        ? 'Install Bicameral for Windows, then return here and choose Try again.' + permissionHint
        : 'Run the setup commands below, then choose Try again.' + permissionHint;
      setView('warn','Setup needed','Bicameral isn’t available on this device yet.',detail);
      showActions({ install: platform === 'windows', retry:true });'''

_SETUP_TIMEOUT_HOOK = '''          configureInstaller();
          setView('warn','Still waiting','Bicameral hasn’t started yet.','You can retry now or reopen the installer. Your sign-in state is preserved.');
          showActions({ install:true,retry:true });'''
_SETUP_TIMEOUT_REPAIR = '''          const platform = configureInstaller();
          const recoveryDetail = platform === 'windows'
            ? 'You can retry now or reopen the Windows installer. Your sign-in state is preserved.'
            : 'You can retry now or rerun the setup command shown below. Your sign-in state is preserved.';
          setView('warn','Still waiting','Bicameral hasn’t started yet.',recoveryDetail);
          showActions({ install: platform === 'windows', retry:true });'''

_CLIENT_HOOK = '''      const client = window.supabase.createClient(config.supabase_url, config.supabase_anon_key);
      const session = await client.auth.getSession();'''
_CLIENT_REPAIR = '''      const client = window.supabase.createClient(config.supabase_url, config.supabase_anon_key);
      const googleSignin = document.getElementById('google-signin');
      if (googleSignin) {
        googleSignin.addEventListener('click', async () => {
          sessionStatus.textContent = 'Redirecting to Google…';
          googleSignin.disabled = true;
          await client.auth.signInWithOAuth({
            provider:'google',
            options:{ redirectTo: window.location.origin + '/auth/callback' }
          });
        });
      }
      const session = await client.auth.getSession();'''

_SIGNOUT_HOOK = '''      document.getElementById('signout').hidden = false;'''
_SIGNOUT_REPAIR = '''      document.getElementById('signout').hidden = !token;'''

_NO_TOKEN_HOOK = '''      if (!token) { sessionStatus.textContent='Sign in to connect this device.'; signin.hidden=false; return; }'''
_NO_TOKEN_REPAIR = '''      if (!token) { sessionStatus.textContent='Sign in to Bicameral Cloud to continue.'; signin.hidden=false; return; }'''

_AUTH_DONE_HOOK = '''      sessionStatus.hidden = true;
      experience.hidden = false;'''
_AUTH_DONE_REPAIR = '''      sessionStatus.hidden = true;
      signin.hidden = true;
      experience.hidden = false;
      const cloudStep = document.getElementById('cloud-step');
      const cloudMark = document.getElementById('cloud-mark');
      const cloudDetail = document.getElementById('cloud-detail');
      if (cloudStep) cloudStep.dataset.state = 'done';
      if (cloudMark) cloudMark.textContent = '✓';
      if (cloudDetail) cloudDetail.textContent = 'Connected to Bicameral Cloud as ' + (user.email || 'your Google account') + '.';'''

_HANDOFF_HOOK = "        window.location.href = '/app';"
_HANDOFF_REPAIR = '''        const deviceStep = document.getElementById('device-step');
        const deviceMark = document.getElementById('device-mark');
        const deviceDetail = document.getElementById('device-detail');
        const driveStep = document.getElementById('drive-step');
        if (deviceStep) deviceStep.dataset.state = 'done';
        if (deviceMark) deviceMark.textContent = '✓';
        if (deviceDetail) deviceDetail.textContent = 'Device connected to Bicameral Cloud.';
        if (driveStep) driveStep.dataset.state = 'active';
        window.setTimeout(() => { window.location.href = '/app?onboarding=drive'; }, 450);'''


def _render_connect_html() -> str:
    html = _BASE_CONNECT_HTML
    html = html.replace(
        _STATIC_COMMANDS,
        '          <div id="troubleshooting-commands"></div>',
        1,
    )
    html = html.replace(_DAEMON_LITERAL, _DAEMON_OVERRIDE, 1)
    html = html.replace(_AUTH_HOOK, _AUTH_REPAIR, 1)
    html = html.replace(_PROGRESS_HOOK, _PROGRESS_REPAIR, 1)
    html = html.replace(_STYLE_HOOK, _STYLE_REPAIR, 1)
    html = html.replace(_SETUP_HOOK, _SETUP_REPAIR, 1)
    html = html.replace(_PLATFORM_HOOK, _PLATFORM_REPAIR, 1)
    html = html.replace(_SETUP_ACTION_HOOK, _SETUP_ACTION_REPAIR, 1)
    html = html.replace(_SETUP_TIMEOUT_HOOK, _SETUP_TIMEOUT_REPAIR, 1)
    html = html.replace(_CLIENT_HOOK, _CLIENT_REPAIR, 1)
    html = html.replace(_SIGNOUT_HOOK, _SIGNOUT_REPAIR, 1)
    html = html.replace(_NO_TOKEN_HOOK, _NO_TOKEN_REPAIR, 1)
    html = html.replace(_AUTH_DONE_HOOK, _AUTH_DONE_REPAIR, 1)
    html = html.replace(_HANDOFF_HOOK, _HANDOFF_REPAIR, 1)

    required = (
        "localStorage.getItem('bicameral_daemon_url')",
        'id="google-signin"',
        "signInWithOAuth",
        "document.getElementById('signout').hidden = !token",
        'id="cloud-step"',
        'id="device-step"',
        'id="drive-step"',
        'id="setup-inline"',
        'id="troubleshooting-commands"',
        "install: platform === 'windows'",
        "install-windows.cmd",
        "['bicameral.exe gateway start']",
        "'bicameral update'",
        "'bicameral gateway start'",
        "rerun the setup command shown below",
        "window.location.href = '/app?onboarding=drive'",
    )
    missing = [marker for marker in required if marker not in html]
    if missing:
        raise RuntimeError(f'APC1 connect remediation failed closed: missing {missing!r}')
    return html


_CONNECT_HTML = _render_connect_html()


@router.get('/connect', response_class=HTMLResponse, include_in_schema=False)
async def connect_polish_apc1() -> HTMLResponse:
    '''Render the remediated APC1 connect flow without changing daemon authority.'''
    return HTMLResponse(content=_CONNECT_HTML)
