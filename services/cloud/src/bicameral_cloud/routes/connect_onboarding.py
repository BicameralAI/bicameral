'''Onboarding remediation layer for the hosted /connect surface (cloud#353).

Served through the OAuth scope overlay (`oauth_scope_apc1`), which wraps this
HTML and owns the `/connect` route.

Layers bounded presentation/orchestration repairs over the APC1 connect HTML,
in the same auditable hook/repair style, so that each rendered screen reflects
exactly one truthful state of the journey:

- setup guidance renders only in setup states and is cleared on every other
  transition (no "Bicameral was not found" beside a pending pairing);
- a pending pairing names a real local approval surface: the daemon-served
  approval page when the daemon advertises one, otherwise the exact terminal
  command (with the deterministic install path) and the code to enter;
- "installed but not running" is distinguished from "not installed" using a
  non-authoritative hint that this browser has reached Bicameral before;
- while setup or restart guidance is shown, the page keeps checking in the
  background and continues automatically when Bicameral answers.

Authority is unchanged. Cloud may open the daemon's loopback approval page but
cannot read it or submit a decision; the browser still learns the outcome only
through the existing pair/status poll. No pairing protocol field is invented:
`confirmation.local_confirmation_url` is the daemon's own optional affordance.
'''

# Embedded HTML/JavaScript literals intentionally preserve reviewed source text.
# ruff: noqa: E501

from __future__ import annotations

from bicameral_cloud.routes.connect_polish_apc1 import (
    _CONNECT_HTML as _APC1_CONNECT_HTML,
)

# Default install location used by install.sh; PATH is not guaranteed in the
# shell the user opens (install#19), so name the binary by its exact path.
UNIX_CLI = '~/.local/bin/bicameral'

_STYLE_HOOK = '''    .loading { color:var(--muted); }'''
# APC1 intended these rules but its hook carried f-string braces and never
# matched the rendered page; they are restored here with literal braces.
_STYLE_REPAIR = _STYLE_HOOK + '''
    .journey { display:grid; gap:8px; margin:0 0 22px; }
    .journey-step { display:flex; align-items:flex-start; gap:11px; padding:12px 14px; border:1px solid var(--line); border-radius:10px; background:var(--surface); }
    .journey-step strong { display:block; font-size:.9rem; }
    .journey-step small { display:block; margin-top:2px; color:var(--muted); line-height:1.35; }
    .journey-mark { width:24px; height:24px; flex:none; display:grid; place-items:center; border:1px solid var(--line); border-radius:50%; color:var(--muted); font-size:.78rem; font-weight:750; }
    .journey-step[data-state="done"] { border-color:#bbf7d0; background:#f0fdf4; }
    .journey-step[data-state="done"] .journey-mark { border-color:var(--ok); background:var(--ok); color:white; }
    .journey-step[data-state="active"] { border-color:#a5f3fc; background:#ecfeff; }
    .journey-step[data-state="active"] .journey-mark { border-color:var(--cyan); color:var(--cyan-strong); }
    .setup-inline { margin:14px 0 0; padding:14px; border:1px solid var(--line); border-radius:10px; background:var(--panel); }
    .setup-inline p { margin:0 0 8px; color:var(--muted); font-size:.88rem; }
    .pairing .btn { margin:4px 0 10px; }
    .pairing details { margin-top:10px; border-top:1px solid #bae6fd; padding-top:10px; }
    .pairing .detail { margin:0 0 10px; }'''

_PAIRING_HOOK = '''        <div id="pairing" class="pairing" hidden>
          <p class="overline">Local approval</p>
          <strong>Confirm this pairing on your device</strong>
          <code id="pairing-code"></code>
          <p class="detail">Local approval remains required. This browser cannot approve itself.</p>
        </div>'''
_PAIRING_REPAIR = '''        <div id="pairing" class="pairing" hidden>
          <div id="approval-local" hidden>
            <a id="open-approval" class="btn primary" href="#" target="_blank" rel="noopener noreferrer">Open approval page</a>
            <p class="detail">Bicameral on this computer shows who is asking. Choose Approve there — this page continues automatically.</p>
          </div>
          <details id="approval-cli" open>
            <summary id="approval-cli-summary">Approve from a terminal</summary>
            <p class="detail">In a terminal on this computer, run:</p>
            <div class="cmd"><code id="pairing-command">bicameral pairing approve</code></div>
            <p class="detail">Enter this code when asked, check the account and origin, then approve:</p>
            <code id="pairing-code"></code>
          </details>
          <p class="detail">This browser can’t approve itself. Only you, on this computer, can.</p>
        </div>'''

_SET_VIEW_HOOK = '''    function setView(tone, eyebrow, title, detail) {
      document.getElementById('card').dataset.tone = tone;'''
_SET_VIEW_REPAIR = '''    function setView(tone, eyebrow, title, detail) {
      // One truthful state per screen: every transition clears setup guidance;
      // setup states reveal it again explicitly after this call.
      const setupPanel = document.getElementById('setup-inline');
      if (setupPanel) setupPanel.hidden = true;
      const platformNode = document.getElementById('platform');
      if (platformNode) platformNode.hidden = true;
      const help = document.getElementById('developer-help');
      if (help) help.hidden = tone === 'pending' || tone === 'ok';
      document.getElementById('card').dataset.tone = tone;'''

_INSTALLER_HOOK = '''    function renderInlineSetup(platform) {
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
_INSTALLER_REPAIR = '''    const DEVICE_SEEN_KEY = 'bicameral_device_seen';
    // Presentation hint only: this browser has reached Bicameral before, so an
    // unreachable daemon is most likely stopped rather than missing.
    function markDeviceSeen() {
      try { window.localStorage.setItem(DEVICE_SEEN_KEY, '1'); } catch (_) {}
    }
    function deviceSeenBefore() {
      try { return window.localStorage.getItem(DEVICE_SEEN_KEY) === '1'; } catch (_) { return false; }
    }
    function renderInlineSetup(platform, reason) {
      const panel = document.getElementById('setup-inline');
      const copy = document.getElementById('setup-copy');
      const root = document.getElementById('setup-commands');
      if (!panel || !copy || !root) return;
      let commands = [];
      if (platform === 'windows') {
        copy.textContent = reason === 'missing'
          ? 'Install the Windows client, then return here. This page continues automatically.'
          : 'Reopen the Windows installer to repair and start Bicameral, then return here. This page continues automatically.';
      } else if (platform === 'linux' || platform === 'macos') {
        commands = ['curl -fsSL https://install.bicameral-ai.com/install.sh | sh'];
        copy.textContent = reason === 'stopped'
          ? 'Bicameral is installed but isn’t running. Run this in a terminal to repair and start it. Keep this page open; it continues automatically.'
          : reason === 'update'
            ? 'Run this in a terminal to update Bicameral, then choose Try again.'
            : 'Run this in a terminal to install Bicameral and start it. Keep this page open; it continues automatically.';
      } else {
        copy.textContent = 'Use the supported installer for this device, then choose Try again.';
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
      const install = document.getElementById('install');
      if (platform === 'windows') {
        install.href = 'https://install.bicameral-ai.com/install-windows.cmd';
        install.textContent = 'Install Bicameral for Windows';
      }
      return platform;
    }
    function revealSetup(platform, reason) {
      renderInlineSetup(platform, reason);
      const label = platform === 'windows' ? 'Windows' : platform === 'macos' ? 'macOS' : platform === 'linux' ? 'Linux' : 'this device';
      const node = document.getElementById('platform');
      node.hidden = false;
      node.textContent = 'Detected ' + label + '.';
    }
    function showStoppedRecovery(title) {
      const platform = configureInstaller();
      const detail = platform === 'windows'
        ? 'Reopen the Windows installer to restart Bicameral. This page continues automatically.'
        : (platform === 'linux' || platform === 'macos')
          ? 'Restart Bicameral with the command below. This page continues automatically.'
          : 'Restart Bicameral on this device. This page continues automatically.';
      setView('warn','Bicameral stopped',title,detail);
      revealSetup(platform, 'stopped');
      showActions({ install: platform === 'windows', retry:true });
      beginSetupPolling({ quiet:true });
    }
    function cliApproveCommand() {
      return detectPlatform() === 'windows'
        ? 'bicameral.exe pairing approve'
        : UNIX_CLI + ' pairing approve';
    }
    function localApprovalUrl(body) {
      const path = body && body.confirmation && body.confirmation.local_confirmation_url;
      // Accept only the daemon's own loopback approval path; never a foreign URL.
      if (typeof path !== 'string' || !/^\\/local\\/pairing\\?request=[A-Za-z0-9._~%-]+$/.test(path)) return null;
      return DAEMON_URL + path;
    }
    function renderApproval(body) {
      const approvalUrl = localApprovalUrl(body);
      document.getElementById('pairing-code').textContent = body.pairing_code;
      document.getElementById('pairing-command').textContent = cliApproveCommand();
      const local = document.getElementById('approval-local');
      const cli = document.getElementById('approval-cli');
      if (approvalUrl) {
        document.getElementById('open-approval').href = approvalUrl;
        local.hidden = false;
        cli.open = false;
        document.getElementById('approval-cli-summary').textContent = 'Or approve from a terminal';
      } else {
        local.hidden = true;
        cli.open = true;
        document.getElementById('approval-cli-summary').textContent = 'Approve from a terminal';
      }
      document.getElementById('pairing').hidden = false;
      return !!approvalUrl;
    }'''

_UNIX_CLI_HOOK = '''    const INSTALL_URL = 'https://install.bicameral-ai.com/';'''
_UNIX_CLI_REPAIR = _UNIX_CLI_HOOK + f'''
    const UNIX_CLI = '{UNIX_CLI}';
    const MAX_QUIET_SETUP_POLLS = 300;
    const MAX_PAIR_POST_FAILURES = 3;
    const MAX_PAIR_POLL_FAILURES = 3;
    let pairPostFailures = 0;'''

_SETUP_NEEDED_HOOK = '''      const platform = configureInstaller();
      const permissionHint = lastLocalNetworkPermission === 'prompt'
        ? ' If Chrome asks to find devices on your network, choose Allow.'
        : '';
      const detail = platform === 'windows'
        ? 'Install Bicameral for Windows, then return here and choose Try again.' + permissionHint
        : 'Run the setup commands below, then choose Try again.' + permissionHint;
      setView('warn','Setup needed','Bicameral isn’t available on this device yet.',detail);
      showActions({ install: platform === 'windows', retry:true });'''
_SETUP_NEEDED_REPAIR = '''      const platform = configureInstaller();
      const reason = deviceSeenBefore() ? 'stopped' : 'missing';
      const permissionHint = lastLocalNetworkPermission === 'prompt'
        ? ' If Chrome asks to find devices on your network, choose Allow.'
        : '';
      const detail = (platform === 'windows'
        ? 'Install Bicameral for Windows, then return here.'
        : reason === 'stopped'
          ? 'Run the setup command below to start it again.'
          : 'Run the setup command below.') + ' We’ll continue automatically when Bicameral is ready.' + permissionHint;
      if (reason === 'stopped') {
        setView('warn','Start Bicameral','Bicameral isn’t running on this device.',detail);
      } else {
        setView('warn','Setup needed','Bicameral isn’t available on this device yet.',detail);
      }
      revealSetup(platform, reason);
      showActions({ install: platform === 'windows', retry:true });
      beginSetupPolling({ quiet:true });'''

_SETUP_POLL_START_HOOK = '''    function beginSetupPolling() {
      invalidateSetupRun();
      const runId = setupGeneration;
      setupAttempts = 0;
      setView('checking','Finishing setup','Waiting for Bicameral…','Keep this page open. We’ll continue automatically when the local service starts.');
      showActions({ retry:true });'''
_SETUP_POLL_START_REPAIR = '''    function beginSetupPolling(options) {
      // Quiet polling keeps the current setup/restart guidance on screen and
      // advances on its own once Bicameral answers.
      const quiet = !!(options && options.quiet);
      invalidateSetupRun();
      const runId = setupGeneration;
      setupAttempts = 0;
      const maxPolls = quiet ? MAX_QUIET_SETUP_POLLS : MAX_SETUP_POLLS;
      if (!quiet) {
        setView('checking','Finishing setup','Waiting for Bicameral…','Keep this page open. We’ll continue automatically when the local service starts.');
        showActions({ retry:true });
      }'''

_SETUP_POLL_PROBE_HOOK = '''        setupAttempts += 1;
        const reachable = await daemonReachable();'''
_SETUP_POLL_PROBE_REPAIR = '''        setupAttempts += 1;
        const reachable = await daemonReachable(quiet);'''

_SETUP_POLL_LIMIT_HOOK = '''        if (setupAttempts >= MAX_SETUP_POLLS) {
          invalidateSetupRun();
          const platform = configureInstaller();
          const recoveryDetail = platform === 'windows'
            ? 'You can retry now or reopen the Windows installer. Your sign-in state is preserved.'
            : 'You can retry now or rerun the setup command shown below. Your sign-in state is preserved.';
          setView('warn','Still waiting','Bicameral hasn’t started yet.',recoveryDetail);
          showActions({ install: platform === 'windows', retry:true });
          return;
        }'''
_SETUP_POLL_LIMIT_REPAIR = '''        if (setupAttempts >= maxPolls) {
          invalidateSetupRun();
          if (quiet) {
            // Stop promising automatic continuation once background checks end.
            document.getElementById('detail').textContent = 'Choose Try again once Bicameral is running.';
            return;
          }
          const platform = configureInstaller();
          const recoveryDetail = platform === 'windows'
            ? 'You can retry now or reopen the Windows installer. Your sign-in state is preserved.'
            : 'You can retry now or rerun the setup command shown below. Your sign-in state is preserved.';
          setView('warn','Still waiting','Bicameral hasn’t started yet.',recoveryDetail);
          revealSetup(platform, deviceSeenBefore() ? 'stopped' : 'missing');
          showActions({ install: platform === 'windows', retry:true });
          return;
        }'''

_REACHABLE_HOOK = '''    async function daemonReachable() {
      const permissionBeforeProbe = await localNetworkPermissionState();
      lastLocalNetworkPermission = permissionBeforeProbe;
      if (permissionBeforeProbe === 'denied') return false;
      if (permissionBeforeProbe === 'prompt') {'''
_REACHABLE_REPAIR = '''    async function daemonReachable(quiet) {
      const permissionBeforeProbe = await localNetworkPermissionState();
      lastLocalNetworkPermission = permissionBeforeProbe;
      if (permissionBeforeProbe === 'denied') return false;
      if (permissionBeforeProbe === 'prompt' && !quiet) {'''

_REACHABLE_OK_HOOK = '''      if (await probeDaemon(DAEMON_PROBE_TIMEOUT_MS)) {
        lastLocalNetworkPermission = await localNetworkPermissionState();
        return true;
      }'''
_REACHABLE_OK_REPAIR = '''      if (await probeDaemon(DAEMON_PROBE_TIMEOUT_MS)) {
        lastLocalNetworkPermission = await localNetworkPermissionState();
        markDeviceSeen();
        return true;
      }'''

_PENDING_HOOK = '''        document.getElementById('pairing-code').textContent = body.pairing_code;
        document.getElementById('pairing').hidden = false;
        setView('pending','Local confirmation required','Approve this browser on your device.','Bicameral is waiting for explicit local approval.');
        showActions({ cancel:true });'''
_PENDING_REPAIR = '''        pairPostFailures = 0;
        setView('pending','Approve on this computer','Approve this browser in Bicameral.','Bicameral on this computer needs your OK before this browser can connect. We’ll continue automatically once you approve.');
        renderApproval(body);
        showActions({ cancel:true });'''

_PAIR_POST_FAIL_HOOK = '''        configureInstaller();
        setView('warn','Connection changed','Bicameral is no longer reachable.','Restart it and we’ll continue from here.');
        showActions({ install:true,retry:true });
        return;'''
_PAIR_POST_FAIL_REPAIR = '''        // A daemon that answers the probe but keeps failing the pair request
        // must not loop forever between "Connecting" and "stopped".
        pairPostFailures += 1;
        if (pairPostFailures >= MAX_PAIR_POST_FAILURES) { showRecoveryState('unavailable'); return; }
        showStoppedRecovery('Bicameral stopped responding.');
        return;'''

_PAIR_POLL_FAIL_HOOK = '''        invalidatePairRun();
        clearPairingUi();
        configureInstaller();
        setView('warn','Connection interrupted','Bicameral stopped responding.','Restart the local service and retry.');
        showActions({ install:true,retry:true });
        return;'''
_PAIR_POLL_FAIL_REPAIR = '''        // Tolerate a brief blip so a code the user is entering stays valid.
        pairing.pollFailures = (pairing.pollFailures || 0) + 1;
        if (pairing.pollFailures < MAX_PAIR_POLL_FAILURES) { schedulePairPoll(runId); return; }
        invalidatePairRun();
        clearPairingUi();
        showStoppedRecovery('Bicameral stopped responding.');
        return;'''

_UPDATE_HOOK = '''      if (state === 'update_required') { configureInstaller(); setView('warn','Update needed','Update Bicameral to continue.','The local service does not support this pairing contract yet. Install the current supported version, then try again.'); showActions({ install:true,retry:true }); return; }'''
_UPDATE_REPAIR = '''      if (state === 'update_required') { const platform = configureInstaller(); setView('warn','Update needed','Update Bicameral to continue.','This version of Bicameral is out of date. Update it, then try again.'); revealSetup(platform, 'update'); showActions({ install:true,retry:true }); return; }'''

_RETRY_GRANT_HOOK = '''        if (await probeDaemon(DAEMON_PERMISSION_RETRY_TIMEOUT_MS)) return true;'''
_RETRY_GRANT_REPAIR = '''        if (await probeDaemon(DAEMON_PERMISSION_RETRY_TIMEOUT_MS)) { markDeviceSeen(); return true; }'''

_POLL_OK_HOOK = '''      const status = (body.status || '').toLowerCase();'''
_POLL_OK_REPAIR = _POLL_OK_HOOK + '''
      pairing.pollFailures = 0;'''

_RETRY_CLICK_HOOK = '''    document.getElementById('retry').addEventListener('click', async () => { await probeAndAdvance(); });'''
_RETRY_CLICK_REPAIR = '''    document.getElementById('retry').addEventListener('click', async () => { pairPostFailures = 0; await probeAndAdvance(); });'''

_REPAIRS: tuple[tuple[str, str, str], ...] = (
    ('style', _STYLE_HOOK, _STYLE_REPAIR),
    ('pairing markup', _PAIRING_HOOK, _PAIRING_REPAIR),
    ('setView', _SET_VIEW_HOOK, _SET_VIEW_REPAIR),
    ('installer', _INSTALLER_HOOK, _INSTALLER_REPAIR),
    ('unix cli', _UNIX_CLI_HOOK, _UNIX_CLI_REPAIR),
    ('setup needed', _SETUP_NEEDED_HOOK, _SETUP_NEEDED_REPAIR),
    ('setup poll start', _SETUP_POLL_START_HOOK, _SETUP_POLL_START_REPAIR),
    ('setup poll probe', _SETUP_POLL_PROBE_HOOK, _SETUP_POLL_PROBE_REPAIR),
    ('setup poll limit', _SETUP_POLL_LIMIT_HOOK, _SETUP_POLL_LIMIT_REPAIR),
    ('reachable', _REACHABLE_HOOK, _REACHABLE_REPAIR),
    ('reachable ok', _REACHABLE_OK_HOOK, _REACHABLE_OK_REPAIR),
    ('pending', _PENDING_HOOK, _PENDING_REPAIR),
    ('pair post failure', _PAIR_POST_FAIL_HOOK, _PAIR_POST_FAIL_REPAIR),
    ('pair poll failure', _PAIR_POLL_FAIL_HOOK, _PAIR_POLL_FAIL_REPAIR),
    ('update required', _UPDATE_HOOK, _UPDATE_REPAIR),
    ('permission retry seen', _RETRY_GRANT_HOOK, _RETRY_GRANT_REPAIR),
    ('pair poll ok', _POLL_OK_HOOK, _POLL_OK_REPAIR),
    ('retry click', _RETRY_CLICK_HOOK, _RETRY_CLICK_REPAIR),
)


def _render_connect_html() -> str:
    html = _APC1_CONNECT_HTML
    for name, hook, repair in _REPAIRS:
        count = html.count(hook)
        if count != 1:
            raise RuntimeError(
                f'connect onboarding remediation failed closed: hook {name!r} matched {count} times'
            )
        html = html.replace(hook, repair, 1)

    forbidden = (
        'Bicameral was not found',
        'on your device',
        'Approve this browser on your device.',
    )
    present = [marker for marker in forbidden if marker in html]
    if present:
        raise RuntimeError(f'connect onboarding remediation left stale copy: {present!r}')
    return html


_CONNECT_HTML = _render_connect_html()

