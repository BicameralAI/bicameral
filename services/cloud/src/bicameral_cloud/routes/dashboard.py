'''Hosted connect, restore, and signup shell routes (ADR-0005, ADR-0006, issue #38).

The hosted shell authenticates users via Supabase, validates design-partner
access server-side, then renders the sole production pre-pair flow that connects
the browser to a local Bicameral daemon via the hosted bridge endpoint.

Product path (docs/prd/hosted-device-pairing-bootstrap.md):
- Cloud `/connect` is the sole production pre-pair bootstrap (rare).
- Cloud `/restore` silently re-attaches a still-valid approved pair (#1109).
- Bot `/app` is post-handoff product only — never a second pair UI.
- There is no `/dashboard` browser route (wiped; do not link it).
- Local daemon is the pairing and restore authority.
- MCP readiness is not device-connection completion and is not rendered here.

The shell must not become authority for local workspace state, source binding,
signoff, compliance enforcement, accepted event materialization, or pairing
approval.
'''

# ruff: noqa: E501
from __future__ import annotations

import os

from fastapi import APIRouter
from fastapi.responses import HTMLResponse, RedirectResponse

from bicameral_cloud.contracts import HOSTED_BRIDGE_PROTOCOL_VERSION

router = APIRouter(tags=['dashboard'])

CLOUD_ORIGIN = 'https://app.bicameral-ai.com'
HOSTED_BRIDGE_CLIENT_PROTOCOL_VERSION = HOSTED_BRIDGE_PROTOCOL_VERSION

# Canonical local daemon port. This MUST match bicameral-bot's default.
DEFAULT_DAEMON_PORT = int(os.environ.get('BICAMERAL_DAEMON_PORT', '37373'))

_BASE_HEAD = '''
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>Bicameral Cloud</title>
<style>
  :root { color-scheme: light dark; font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }
  body { margin: 0; min-height: 100vh; display: grid; place-items: center; background: #f7f7f4; color: #1d1d1b; }
  main { width: min(640px, calc(100vw - 40px)); }
  h1 { font-size: clamp(2rem, 5vw, 4rem); line-height: 1; margin: 0 0 16px; letter-spacing: 0; }
  p { font-size: 1rem; line-height: 1.55; color: #55524c; }
  button, a.button { appearance: none; border: 1px solid #1d1d1b; background: #1d1d1b; color: white; padding: 12px 16px; border-radius: 6px; font: inherit; text-decoration: none; display: inline-flex; align-items: center; gap: 8px; cursor: pointer; }
  button.secondary { background: transparent; color: #1d1d1b; }
  .actions { display: flex; gap: 12px; flex-wrap: wrap; margin-top: 24px; }
  .panel { border-top: 1px solid #d8d5ce; margin-top: 28px; padding-top: 18px; }
  code { background: #ece9e2; padding: 2px 5px; border-radius: 4px; }
  [hidden] { display: none !important; }
</style>
'''

_SIGNUP_HTML = f'''<!doctype html>
<html lang="en">
<head>{_BASE_HEAD}</head>
<body>
  <main>
    <h1>Bicameral Cloud</h1>
    <p>Design partner access for hosted workspace discovery, pairing, and cloud coordination.</p>
    <p id="status">Checking signup configuration...</p>
    <div class="actions">
      <button id="google" type="button" hidden>Continue with Google</button>
      <a class="button" href="/connect">Connect this device</a>
    </div>
    <div class="panel"><p>Access is limited to approved design partner domains.</p></div>
  </main>
  <script src="https://cdn.jsdelivr.net/npm/@supabase/supabase-js@2"></script>
  <script>
    async function main() {{
      const status = document.getElementById('status');
      const button = document.getElementById('google');
      const response = await fetch('/api/v1/auth/config');
      const config = await response.json();
      if (!config.supabase_browser_auth_configured) {{
        status.textContent = 'Signup is not configured yet. SUPABASE_URL and SUPABASE_ANON_KEY are required.';
        return;
      }}
      const client = window.supabase.createClient(config.supabase_url, config.supabase_anon_key);
      const session = await client.auth.getSession();
      const token = session.data.session && session.data.session.access_token;
      // Only leave signup when Cloud accepts the token. A stale browser Supabase
      // session alone must not bounce back to /connect (loop).
      if (token) {{
        const authResponse = await fetch('/api/v1/auth/session', {{
          headers: {{ Authorization: 'Bearer ' + token }}
        }});
        if (authResponse.ok) {{
          window.location.href = '/connect';
          return;
        }}
        // Stale / rejected browser session — clear so Google sign-in can run again.
        await client.auth.signOut();
        if (authResponse.status === 403) {{
          status.textContent = 'This Google account is not enabled for the design partner phase. Sign in with an approved account.';
        }} else if (authResponse.status === 503) {{
          status.textContent = 'Cloud cannot verify design-partner access yet (server allowlist unavailable). Sign out and retry after server config is fixed.';
        }} else {{
          status.textContent = 'Previous session was rejected by Cloud. Sign in again.';
        }}
      }} else {{
        status.textContent = 'Sign in with your design partner Google account.';
      }}
      button.hidden = false;
      button.addEventListener('click', async () => {{
        status.textContent = 'Redirecting to Google...';
        await client.auth.signInWithOAuth({{
          provider: 'google',
          options: {{ redirectTo: window.location.origin + '/auth/callback' }}
        }});
      }});
    }}
    main().catch((error) => {{
      document.getElementById('status').textContent = 'Signup failed to initialize: ' + error.message;
    }});
  </script>
</body>
</html>
'''

_CALLBACK_HTML = f'''<!doctype html>
<html lang="en">
<head>{_BASE_HEAD}</head>
<body>
  <main>
    <h1>Signing you in</h1>
    <p id="status">Validating your Bicameral Cloud access...</p>
    <div class="actions" id="actions" hidden><a class="button" href="/signup">Back to signup</a></div>
  </main>
  <script src="https://cdn.jsdelivr.net/npm/@supabase/supabase-js@2"></script>
  <script>
    async function main() {{
      const status = document.getElementById('status');
      const actions = document.getElementById('actions');
      const response = await fetch('/api/v1/auth/config');
      const config = await response.json();
      if (!config.supabase_browser_auth_configured) {{ status.textContent = 'Signup is not configured yet.'; actions.hidden = false; return; }}
      const client = window.supabase.createClient(config.supabase_url, config.supabase_anon_key);
      const session = await client.auth.getSession();
      const token = session.data.session && session.data.session.access_token;
      if (!token) {{ status.textContent = 'No Supabase session was found. Please sign in again.'; actions.hidden = false; return; }}
      const authResponse = await fetch('/api/v1/auth/session', {{ headers: {{ Authorization: 'Bearer ' + token }} }});
      if (authResponse.ok) {{ window.location.href = '/connect'; return; }}
      if (authResponse.status === 403) {{ status.textContent = 'This Google account is not enabled for the Bicameral Cloud design partner phase.'; actions.hidden = false; return; }}
      status.textContent = 'Authentication failed. Please sign in again.';
      actions.hidden = false;
    }}
    main().catch((error) => {{
      document.getElementById('status').textContent = 'Sign-in failed: ' + error.message;
      document.getElementById('actions').hidden = false;
    }});
  </script>
</body>
</html>
'''

_DASHBOARD_HEAD = '''
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>Bicameral Cloud — Connect this device</title>
<style>
  :root {
    --ink: #0f1115; --muted: #667085; --faint: #98a2b3;
    --line: #e4e7ec; --surface: #fff; --panel: #f8fafc; --page: #fbfcfd;
    --cyan: #0891b2; --cyan-strong: #0e7490; --ok: #16a34a;
    --warn: #d97706; --error: #dc2626; --idle: #98a2b3;
    font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  }
  * { box-sizing: border-box; }
  body { margin: 0; min-height: 100vh; background: var(--page); color: var(--ink); }
  [hidden] { display: none !important; }
  code, .mono { font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace; }

  .topbar { display: flex; align-items: center; justify-content: space-between; padding: 16px 28px; border-bottom: 1px solid var(--line); background: var(--surface); }
  .brand { display: flex; align-items: center; gap: 12px; }
  .brand-mark { width: 34px; height: 34px; border-radius: 8px; background: var(--ink); display: grid; place-items: center; }
  .brand-mark span { width: 16px; height: 16px; border: 2.5px solid #fff; border-right-color: var(--cyan); border-radius: 50%; }
  .brand-word { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-weight: 700; }
  .brand-tag { font-size: .7rem; font-weight: 700; letter-spacing: .14em; color: var(--cyan); }
  .topbar-right { display: flex; align-items: center; gap: 16px; }
  .avatar { width: 34px; height: 34px; border-radius: 50%; border: 1px solid #c4b5fd; color: #7c3aed; display: grid; place-items: center; font-size: .75rem; font-weight: 700; }
  .conn-pill { display: inline-flex; align-items: center; gap: 8px; font-size: .84rem; font-weight: 650; color: var(--muted); }
  .conn-pill .dot { width: 9px; height: 9px; border-radius: 50%; background: var(--idle); }
  .conn-pill[data-tone="ok"] { color: var(--ok); } .conn-pill[data-tone="ok"] .dot { background: var(--ok); }
  .conn-pill[data-tone="error"] { color: var(--error); } .conn-pill[data-tone="error"] .dot { background: var(--error); }
  .conn-pill[data-tone="warn"] { color: var(--warn); } .conn-pill[data-tone="warn"] .dot { background: var(--warn); }
  .conn-pill[data-tone="pending"] { color: var(--cyan-strong); } .conn-pill[data-tone="pending"] .dot { background: var(--cyan); }

  main.dash { width: min(720px, calc(100vw - 40px)); margin: 0 auto; padding: 64px 0 80px; }
  .loading { color: var(--muted); }
  .overline { margin: 0 0 10px; color: var(--cyan-strong); font-size: .72rem; font-weight: 750; letter-spacing: .14em; text-transform: uppercase; }
  h1 { margin: 0; font-size: clamp(2rem, 5vw, 3rem); line-height: 1.04; letter-spacing: -.035em; }
  .lead { margin: 16px 0 28px; max-width: 620px; color: var(--muted); font-size: 1rem; line-height: 1.55; }
  .card { background: var(--surface); border: 1px solid var(--line); border-radius: 14px; padding: 24px; box-shadow: 0 1px 2px rgba(16,24,40,.04); }
  .conn-line { display: flex; align-items: center; gap: 10px; margin: 0 0 8px; }
  .status-dot { width: 11px; height: 11px; border-radius: 50%; background: var(--idle); flex: none; }
  .card[data-tone="ok"] .status-dot { background: var(--ok); }
  .card[data-tone="error"] .status-dot { background: var(--error); }
  .card[data-tone="warn"] .status-dot { background: var(--warn); }
  .card[data-tone="pending"], .card[data-tone="checking"] { border-color: #a5f3fc; }
  .card[data-tone="pending"] .status-dot, .card[data-tone="checking"] .status-dot { background: var(--cyan); }
  .card[data-tone="checking"] .status-dot { animation: pulse 1.1s ease-in-out infinite; }
  @keyframes pulse { 0%,100% { opacity: 1; } 50% { opacity: .35; } }
  .conn-title { font-size: 1.2rem; font-weight: 750; }
  .conn-detail { margin: 0 0 18px; color: var(--muted); line-height: 1.5; }
  .status-actions { display: flex; gap: 10px; flex-wrap: wrap; }
  .btn { appearance: none; border: 1px solid transparent; border-radius: 8px; padding: 11px 17px; font: inherit; font-weight: 650; cursor: pointer; }
  .btn:disabled { opacity: .58; cursor: default; }
  .btn-cyan { background: var(--cyan); color: #fff; } .btn-cyan:hover:not(:disabled) { background: var(--cyan-strong); }
  .btn-ghost { background: var(--surface); color: var(--ink); border-color: var(--line); }
  .btn-dark { background: var(--ink); color: #fff; text-decoration: none; display: inline-flex; }

  .pairing-code-box { margin: 4px 0 18px; padding: 16px; border: 1px solid #bae6fd; background: #ecfeff; border-radius: 10px; }
  .pairing-code-label { margin: 0 0 7px; color: var(--cyan-strong); font-size: .7rem; font-weight: 750; letter-spacing: .12em; text-transform: uppercase; }
  .pairing-code-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .pairing-code { background: transparent; color: var(--ink); font-size: clamp(1.35rem, 5vw, 1.8rem); font-weight: 780; letter-spacing: .08em; }
  .pairing-code-help { margin: 10px 0 0; color: var(--muted); font-size: .83rem; line-height: 1.45; }
  .copy-btn { flex: none; border: 1px solid var(--line); background: var(--surface); color: var(--ink); border-radius: 6px; padding: 6px 11px; font: inherit; font-size: .8rem; cursor: pointer; }
  .copy-feedback { min-height: 1.2em; margin: 8px 0 0; color: var(--ok); font-size: .8rem; }

  .authority-note { margin: 14px 2px 0; color: var(--faint); font-size: .82rem; line-height: 1.45; }
  .recovery { margin-top: 18px; border-top: 1px solid var(--line); padding-top: 14px; }
  .recovery summary { cursor: pointer; color: var(--muted); font-size: .86rem; font-weight: 650; }
  .recovery-copy { margin: 12px 0; color: var(--muted); font-size: .86rem; line-height: 1.5; }
  .cmd { display: flex; align-items: center; justify-content: space-between; gap: 10px; padding: 10px 12px; margin: 8px 0; border: 1px solid var(--line); border-radius: 8px; background: var(--panel); }
  .cmd code { overflow-x: auto; white-space: nowrap; font-size: .82rem; }
  .dash-footer { margin-top: 36px; padding-top: 18px; border-top: 1px solid var(--line); color: var(--faint); font-size: .72rem; letter-spacing: .11em; text-transform: uppercase; }
  .auth-actions { display: flex; gap: 12px; margin-top: 20px; }

  @media (max-width: 640px) {
    .topbar { padding: 13px 16px; }
    .brand-tag, .conn-pill { display: none; }
    main.dash { width: min(100% - 28px, 720px); padding: 40px 0 56px; }
    .card { padding: 20px; }
    .pairing-code-row { align-items: flex-start; flex-direction: column; }
    .cmd { align-items: flex-start; flex-direction: column; }
  }
</style>
'''

_DASHBOARD_HTML = f'''<!doctype html>
<html lang="en">
<head>{_DASHBOARD_HEAD}</head>
<body>
  <header class="topbar">
    <div class="brand">
      <span class="brand-mark" aria-hidden="true"><span></span></span>
      <span class="brand-word">bicameral</span>
      <span class="brand-tag">HOSTED</span>
    </div>
    <div class="topbar-right">
      <span id="conn-pill" class="conn-pill" data-tone="idle"><span class="dot"></span><span id="conn-pill-label">Not connected</span></span>
      <span id="user-avatar" class="avatar" hidden></span>
      <button id="signout" class="btn btn-ghost" type="button" hidden style="padding:7px 13px;font-size:.82rem;">Sign out</button>
    </div>
  </header>

  <main class="dash">
    <p id="status" class="loading">Checking session…</p>
    <div class="auth-actions" id="auth-actions"><a class="btn btn-dark" href="/signup">Sign in</a></div>

    <div id="dashboard-body" hidden>
      <p class="overline">Connect this device</p>
      <h1>Connect Bicameral on this device</h1>
      <p class="lead">Link this browser to Bicameral on this computer. You usually only do this once; later visits go straight to the product when the link is still valid.</p>

      <section class="card" id="status-card" data-tone="idle" aria-live="polite">
        <p class="overline" id="conn-overline">This device</p>
        <p class="conn-line"><span class="status-dot"></span><span class="conn-title" id="conn-title">Ready to connect</span></p>
        <p class="conn-detail" id="conn-detail">Bicameral will check the local service after you choose Connect this device.</p>

        <div class="pairing-code-box" id="pairing-code-box" hidden>
          <p class="pairing-code-label">Pairing code</p>
          <div class="pairing-code-row">
            <code class="pairing-code" id="pairing-code"></code>
            <button class="copy-btn" type="button" data-copy="pairing">Copy</button>
          </div>
          <p class="pairing-code-help">Enter this code locally, verify the requesting origin, and explicitly approve the browser. If Bicameral is running as a background service, use <code>bicameral pairing approve</code>.</p>
          <p class="copy-feedback" data-copy-feedback="pairing"></p>
        </div>

        <div class="status-actions">
          <button id="primary-action" class="btn btn-cyan" type="button">Connect this device</button>
          <button id="secondary-action" class="btn btn-ghost" type="button" hidden>Cancel</button>
        </div>

        <details class="recovery" id="recovery-help">
          <summary>Show help — install or start Bicameral</summary>
          <p class="recovery-copy">If Bicameral is not running on this computer, install or start it, then try Connect again. The browser can only detect a running local service — not whether the binary is installed but stopped.</p>
          <div class="cmd"><code>curl -fsSL https://install.bicameral-ai.com/install.sh | sh</code><button class="copy-btn" type="button" data-copy="install">Copy install</button></div>
          <div class="cmd"><code>bicameral update</code><button class="copy-btn" type="button" data-copy="update">Copy update</button></div>
          <div class="cmd"><code>bicameral gateway start</code><button class="copy-btn" type="button" data-copy="gateway">Copy start</button></div>
          <p class="copy-feedback" data-copy-feedback="recovery"></p>
        </details>
      </section>

      <p class="authority-note">Pairing authorizes this browser only. Local approval stays on your device and does not grant Product, code, or approval authority.</p>
      <footer class="dash-footer">Signed in &nbsp;·&nbsp; Cloud coordinates &nbsp;·&nbsp; Local daemon authorizes</footer>
    </div>
  </main>

  <script src="https://cdn.jsdelivr.net/npm/@supabase/supabase-js@2"></script>
  <script>
    const CLOUD_ORIGIN = window.location.origin || '{CLOUD_ORIGIN}';
    const CLIENT_PROTOCOL_VERSION = {HOSTED_BRIDGE_CLIENT_PROTOCOL_VERSION};
    const DEFAULT_DAEMON_PORT = {DEFAULT_DAEMON_PORT};
    const RESTORE_KEY = 'bicameral_hosted_bridge_restore';
    const DEFAULT_BRIDGE_URL = (function () {{
      let override = '';
      try {{ override = (window.localStorage.getItem('bicameral_daemon_url') || '').trim(); }} catch (e) {{ override = ''; }}
      return override ? override.replace(/\\/+$/, '') : 'http://localhost:' + DEFAULT_DAEMON_PORT;
    }})();

    const COMMANDS = {{
      install: 'curl -fsSL https://install.bicameral-ai.com/install.sh | sh',
      update: 'bicameral update',
      gateway: 'bicameral gateway start',
    }};

    const STATES = {{
      idle: {{ tone: 'idle', overline: 'This device', title: 'Ready to connect', detail: 'Bicameral will check the local service after you choose Connect this device.', primary: {{ label: 'Connect this device', action: 'check' }}, expandHelp: false }},
      checking: {{ tone: 'checking', overline: 'This device', title: 'Connecting', detail: 'Looking for Bicameral on localhost:' + DEFAULT_DAEMON_PORT + '…', primary: {{ label: 'Connecting…', disabled: true }}, expandHelp: false }},
      unavailable: {{ tone: 'error', overline: 'Bicameral unavailable', title: 'Bicameral was not found', detail: 'Open Bicameral on this device, then retry. Install or start commands are under Show help.', primary: {{ label: 'Try again', action: 'check' }}, expandHelp: true }},
      pending: {{ tone: 'pending', overline: 'Local confirmation required', title: 'Enter this code locally', detail: 'The browser request is waiting for explicit approval from Bicameral on this device.', secondary: {{ label: 'Cancel', action: 'cancel' }}, expandHelp: false }},
      update_required: {{ tone: 'warn', overline: 'Unsupported version', title: 'Update Bicameral to continue', detail: 'The local service does not support this pairing contract yet. Use Show help for the update command.', primary: {{ label: 'Try again after update', action: 'check' }}, expandHelp: true }},
      expired: {{ tone: 'warn', overline: 'Pairing expired', title: 'Connection expired', detail: 'The local approval window expired. Start a new connection request.', primary: {{ label: 'Connect again', action: 'check' }}, expandHelp: false }},
      revoked: {{ tone: 'error', overline: 'Pairing revoked', title: 'Connection revoked', detail: 'This browser session was disconnected by the local service.', primary: {{ label: 'Connect again', action: 'check' }}, expandHelp: false }},
      account_mismatch: {{ tone: 'error', overline: 'Account mismatch', title: 'Use the matching account', detail: 'The local pairing request belongs to a different signed-in account.', primary: {{ label: 'Sign out', action: 'signout' }}, expandHelp: false }},
      tenancy_blocked: {{ tone: 'error', overline: 'Team required', title: 'Cannot resolve Cloud team', detail: 'Pairing needs exactly one Cloud team for this signed-in account. Contact Bicameral if this domain has more than one team.', primary: {{ label: 'Try again', action: 'check' }}, expandHelp: false }},
      denied: {{ tone: 'error', overline: 'Connection declined', title: 'Local approval was declined', detail: 'Nothing was connected. Start a new request when you are ready.', primary: {{ label: 'Connect again', action: 'check' }}, expandHelp: false }},
      connected: {{ tone: 'ok', overline: 'This device', title: 'Connected', detail: 'Opening Bicameral…', expandHelp: false }},
    }};

    const PILL_LABEL = {{ idle: 'Not connected', checking: 'Connecting', unavailable: 'Not connected', pending: 'Approval required', update_required: 'Update required', expired: 'Expired', revoked: 'Revoked', account_mismatch: 'Account mismatch', tenancy_blocked: 'Team required', denied: 'Declined', connected: 'Connected' }};

    let currentStateName = 'idle';
    let pollTimer = null;
    let flowCtx = null;

    function readRestoreCandidate() {{
      try {{
        const raw = window.localStorage.getItem(RESTORE_KEY);
        if (!raw) return null;
        const value = JSON.parse(raw);
        const expiresAt = Date.parse(value.expires_at || '');
        if (!value.request_handle || !value.poll_proof || !value.cloud_account_id || !value.team_id || !Number.isFinite(expiresAt) || expiresAt <= Date.now()) {{
          window.localStorage.removeItem(RESTORE_KEY);
          return null;
        }}
        return value;
      }} catch (e) {{
        try {{ window.localStorage.removeItem(RESTORE_KEY); }} catch (_) {{}}
        return null;
      }}
    }}

    async function main() {{
      const status = document.getElementById('status');
      const authActions = document.getElementById('auth-actions');
      const signout = document.getElementById('signout');
      const dashboardBody = document.getElementById('dashboard-body');

      bindCopyButtons();
      wireStatusActions();

      const response = await fetch('/api/v1/auth/config');
      const config = await response.json();
      if (!config.supabase_browser_auth_configured) {{ status.textContent = 'Hosted dashboard shell. Supabase signup is not configured yet.'; return; }}
      const client = window.supabase.createClient(config.supabase_url, config.supabase_anon_key);
      const session = await client.auth.getSession();
      const token = session.data.session && session.data.session.access_token;
      signout.hidden = false;
      signout.addEventListener('click', async () => {{
        try {{ window.localStorage.removeItem(RESTORE_KEY); }} catch (e) {{}}
        await client.auth.signOut();
        window.location.href = '/signup';
      }});
      if (!token) {{
        status.textContent = 'Sign in to connect this device.';
        authActions.hidden = false;
        return;
      }}
      const authResponse = await fetch('/api/v1/auth/session', {{ headers: {{ Authorization: 'Bearer ' + token }} }});
      if (!authResponse.ok) {{
        await client.auth.signOut();
        if (authResponse.status === 403) {{
          status.textContent = 'This Google account is not enabled for the design partner phase.';
        }} else if (authResponse.status === 503) {{
          status.textContent = 'Cloud design-partner allowlist is unavailable on this server (check SUPABASE_SERVICE_KEY).';
        }} else {{
          status.textContent = 'Your session is no longer valid. Please sign in again.';
        }}
        authActions.hidden = false;
        return;
      }}
      const user = await authResponse.json();
      const cloudAccountId = user.user_id || user.email;

      const restore = readRestoreCandidate();
      if (restore) {{
        if (restore.cloud_account_id === cloudAccountId) {{
          window.location.replace('/restore');
          return;
        }}
        try {{ window.localStorage.removeItem(RESTORE_KEY); }} catch (e) {{}}
      }}

      status.hidden = true;
      authActions.hidden = true;
      dashboardBody.hidden = false;
      const avatar = document.getElementById('user-avatar');
      avatar.textContent = (user.email || '?').slice(0, 2).toUpperCase();
      avatar.hidden = false;

      flowCtx = {{ cloudAccountId: cloudAccountId, cloudAccessToken: token, teamId: null }};
      setState('idle');
    }}

    function bindCopyButtons() {{
      document.querySelectorAll('.copy-btn').forEach((button) => {{
        button.addEventListener('click', async () => {{
          const key = button.getAttribute('data-copy');
          const value = key === 'pairing' ? (flowCtx && flowCtx.pairing && flowCtx.pairing.pairingCode) : COMMANDS[key];
          const feedback = key === 'pairing' ? document.querySelector('[data-copy-feedback="pairing"]') : document.querySelector('[data-copy-feedback="recovery"]');
          try {{
            if (!value || !navigator.clipboard) throw new Error('clipboard unavailable');
            await navigator.clipboard.writeText(value);
            if (feedback) feedback.textContent = 'Copied.';
          }} catch (e) {{
            if (feedback) feedback.textContent = 'Select and copy the value above.';
          }}
        }});
      }});
    }}

    function wireStatusActions() {{
      document.getElementById('primary-action').addEventListener('click', () => runAction('primary'));
      document.getElementById('secondary-action').addEventListener('click', () => runAction('secondary'));
    }}

    function runAction(slot) {{
      const state = STATES[currentStateName] || STATES.idle;
      const spec = state[slot];
      if (!spec || !spec.action) return;
      if (spec.action === 'check') checkConnection();
      else if (spec.action === 'cancel') cancelPending();
      else if (spec.action === 'signout') document.getElementById('signout').click();
    }}

    function setState(name) {{
      currentStateName = name;
      const state = STATES[name];
      const card = document.getElementById('status-card');
      card.setAttribute('data-tone', state.tone);
      document.getElementById('conn-overline').textContent = state.overline;
      document.getElementById('conn-title').textContent = state.title;
      document.getElementById('conn-detail').textContent = state.detail;

      const visiblePairingCode = name === 'pending' && flowCtx && flowCtx.pairing ? flowCtx.pairing.pairingCode : '';
      document.getElementById('pairing-code').textContent = visiblePairingCode || '';
      document.getElementById('pairing-code-box').hidden = !visiblePairingCode;

      const recovery = document.getElementById('recovery-help');
      recovery.hidden = false;
      recovery.open = !!state.expandHelp;

      renderActionButton('primary-action', state.primary);
      renderActionButton('secondary-action', state.secondary);

      const pill = document.getElementById('conn-pill');
      pill.setAttribute('data-tone', state.tone === 'idle' || state.tone === 'checking' ? 'idle' : state.tone);
      document.getElementById('conn-pill-label').textContent = PILL_LABEL[name] || 'Not connected';
    }}

    function renderActionButton(id, spec) {{
      const btn = document.getElementById(id);
      if (!spec) {{ btn.hidden = true; return; }}
      btn.hidden = false;
      btn.textContent = spec.label;
      btn.disabled = !!spec.disabled;
    }}

    function generateCsrfToken() {{
      if (window.crypto && typeof window.crypto.randomUUID === 'function') return window.crypto.randomUUID();
      if (window.crypto && window.crypto.getRandomValues) {{
        const bytes = new Uint8Array(16);
        window.crypto.getRandomValues(bytes);
        return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
      }}
      return 'csrf-' + Date.now() + '-' + Math.random().toString(36).slice(2);
    }}

    function stopPolling() {{ if (pollTimer) {{ clearInterval(pollTimer); pollTimer = null; }} }}

    function cancelPending() {{
      stopPolling();
      if (flowCtx) flowCtx.pairing = null;
      setState('idle');
    }}

    async function checkConnection() {{
      if (!flowCtx) return;
      stopPolling();
      flowCtx.pairing = null;
      setState('checking');
      let tenancy;
      try {{
        const tenancyResponse = await fetch('/api/v1/auth/pairing-tenancy', {{
          method: 'POST',
          headers: {{ Authorization: 'Bearer ' + flowCtx.cloudAccessToken }},
        }});
        tenancy = await tenancyResponse.json().catch(function () {{ return {{}}; }});
        if (!tenancyResponse.ok || !tenancy.team_id) {{
          setState('tenancy_blocked');
          return;
        }}
        flowCtx.teamId = tenancy.team_id;
      }} catch (err) {{ setState('tenancy_blocked'); return; }}
      const csrfToken = generateCsrfToken();
      let response;
      try {{
        response = await fetch(DEFAULT_BRIDGE_URL + '/api/v1/hosted-bridge/pair', {{
          method: 'POST',
          headers: {{ 'Content-Type': 'application/json', 'x-csrf-token': csrfToken }},
          body: JSON.stringify({{
            origin: CLOUD_ORIGIN,
            cloud_account_id: flowCtx.cloudAccountId,
            csrf_token: csrfToken,
            team_id: flowCtx.teamId,
            cloud_access_token: flowCtx.cloudAccessToken,
            client_protocol_version: CLIENT_PROTOCOL_VERSION,
          }}),
        }});
      }} catch (err) {{ setState('unavailable'); return; }}

      let body = {{}};
      try {{ body = await response.json(); }} catch (e) {{ body = {{}}; }}

      if (response.status === 202 || body.status === 'pending') {{
        if (!body.request_handle || !body.poll_proof || !body.pairing_code) {{ setState('update_required'); return; }}
        flowCtx.pairing = {{ requestHandle: body.request_handle, pollProof: body.poll_proof, pairingCode: body.pairing_code }};
        setState('pending');
        startPolling();
        return;
      }}

      if (!response.ok) {{ setState(recoveryStateForPairError(response.status, body)); return; }}
      setState('update_required');
    }}

    function startPolling() {{ pollTimer = setInterval(pollStatus, 2000); }}

    async function pollStatus() {{
      if (!flowCtx || !flowCtx.pairing) {{ stopPolling(); return; }}
      const params = new URLSearchParams({{
        request_handle: flowCtx.pairing.requestHandle,
        poll_proof: flowCtx.pairing.pollProof,
        cloud_account_id: flowCtx.cloudAccountId,
      }});
      let response;
      try {{
        response = await fetch(DEFAULT_BRIDGE_URL + '/api/v1/hosted-bridge/pair/status?' + params.toString(), {{ method: 'GET' }});
      }} catch (err) {{ stopPolling(); setState('unavailable'); return; }}

      let body = {{}};
      try {{ body = await response.json(); }} catch (e) {{ body = {{}}; }}
      const status = (body.status || '').toLowerCase();

      if (status === 'approved' && body.session_token) {{
        const pairing = flowCtx.pairing;
        stopPolling();
        flowCtx.pairing = null;
        completeHandoff(body, pairing);
        return;
      }}
      if (status === 'pending' || status === '') return;

      stopPolling();
      flowCtx.pairing = null;
      setState(recoveryStateForStatus(status));
    }}

    function recoveryStateForStatus(status) {{
      switch (status) {{
        case 'denied': return 'denied';
        case 'expired': return 'expired';
        case 'revoked': return 'revoked';
        case 'incompatible':
        case 'update_required': return 'update_required';
        case 'account_mismatch': return 'account_mismatch';
        default: return 'unavailable';
      }}
    }}

    function recoveryStateForPairError(httpStatus, body) {{
      if (httpStatus === 426) return 'update_required';
      const rawKind = body.kind || body.error_type || body.detail || '';
      const kind = (typeof rawKind === 'string' ? rawKind : JSON.stringify(rawKind)).toLowerCase();
      if (kind.indexOf('protocol') !== -1 || kind.indexOf('incompatible') !== -1 || kind.indexOf('unsupported') !== -1) return 'update_required';
      if (kind.indexOf('account') !== -1) return 'account_mismatch';
      if (kind.indexOf('missingteamid') !== -1 || kind.indexOf('team_id') !== -1 || kind.indexOf('missingcloudaccesstoken') !== -1 || kind.indexOf('cloud_access_token') !== -1) return 'tenancy_blocked';
      if (kind.indexOf('revoked') !== -1) return 'revoked';
      if (kind.indexOf('expired') !== -1) return 'expired';
      return 'unavailable';
    }}

    // Authority-bearing tokens remain transient. The durable restore record is
    // only the already-issued opaque status proof plus binding and expiry.
    function completeHandoff(body, pairing) {{
      setState('connected');
      try {{
        window.sessionStorage.setItem('bicameral_hosted_bridge', JSON.stringify({{
          session_token: body.session_token,
          daemon_url: DEFAULT_BRIDGE_URL,
          cloud_account_id: flowCtx.cloudAccountId,
          cloud_access_token: flowCtx.cloudAccessToken,
          team_id: flowCtx.teamId,
          expires_at: body.expires_at || null,
        }}));
      }} catch (e) {{ setState('unavailable'); return; }}

      try {{
        if (pairing && pairing.requestHandle && pairing.pollProof && body.expires_at) {{
          window.localStorage.setItem(RESTORE_KEY, JSON.stringify({{
            request_handle: pairing.requestHandle,
            poll_proof: pairing.pollProof,
            daemon_url: DEFAULT_BRIDGE_URL,
            cloud_account_id: flowCtx.cloudAccountId,
            team_id: flowCtx.teamId,
            expires_at: body.expires_at,
          }}));
        }}
      }} catch (e) {{
        // Restore persistence is optional. Never block a valid transient handoff.
      }}
      window.location.href = '/app';
    }}

    main().catch((error) => {{
      const status = document.getElementById('status');
      status.hidden = false;
      status.textContent = 'Dashboard failed to initialize: ' + error.message;
    }});
  </script>
</body>
</html>
'''  # nosec B608 # f-string builds an HTML document, not a SQL query

_RESTORE_HTML = f'''<!doctype html>
<html lang="en">
<head>{_BASE_HEAD}</head>
<body>
  <main>
    <h1>Opening Bicameral</h1>
    <p id="status">Restoring your local connection…</p>
    <div class="actions" id="actions" hidden>
      <button id="retry" type="button">Try again</button>
      <a class="button" href="/connect">Reconnect this device</a>
    </div>
  </main>
  <script src="https://cdn.jsdelivr.net/npm/@supabase/supabase-js@2"></script>
  <script>
    const RESTORE_KEY = 'bicameral_hosted_bridge_restore';
    const HANDOFF_KEY = 'bicameral_hosted_bridge';

    function clearRestore() {{
      try {{ window.localStorage.removeItem(RESTORE_KEY); }} catch (e) {{}}
    }}

    function failToConnect() {{
      clearRestore();
      window.location.replace('/connect');
    }}

    function readRestore() {{
      try {{
        const raw = window.localStorage.getItem(RESTORE_KEY);
        if (!raw) return null;
        const value = JSON.parse(raw);
        const expiresAt = Date.parse(value.expires_at || '');
        if (!value.request_handle || !value.poll_proof || !value.cloud_account_id || !value.team_id || !value.daemon_url || !Number.isFinite(expiresAt) || expiresAt <= Date.now()) return null;
        return value;
      }} catch (e) {{ return null; }}
    }}

    async function restore() {{
      const status = document.getElementById('status');
      const actions = document.getElementById('actions');
      actions.hidden = true;
      status.textContent = 'Restoring your local connection…';

      const saved = readRestore();
      if (!saved) {{ failToConnect(); return; }}

      const configResponse = await fetch('/api/v1/auth/config');
      const config = await configResponse.json();
      if (!config.supabase_browser_auth_configured) {{ failToConnect(); return; }}
      const client = window.supabase.createClient(config.supabase_url, config.supabase_anon_key);
      const session = await client.auth.getSession();
      const token = session.data.session && session.data.session.access_token;
      if (!token) {{ window.location.replace('/connect'); return; }}

      const authResponse = await fetch('/api/v1/auth/session', {{ headers: {{ Authorization: 'Bearer ' + token }} }});
      if (!authResponse.ok) {{
        if (authResponse.status === 401 || authResponse.status === 403) clearRestore();
        window.location.replace('/connect');
        return;
      }}
      const user = await authResponse.json();
      const cloudAccountId = user.user_id || user.email;
      if (!cloudAccountId || cloudAccountId !== saved.cloud_account_id) {{ failToConnect(); return; }}

      const params = new URLSearchParams({{
        request_handle: saved.request_handle,
        poll_proof: saved.poll_proof,
        cloud_account_id: cloudAccountId,
      }});
      let response;
      try {{
        response = await fetch(saved.daemon_url.replace(/\\/+$/, '') + '/api/v1/hosted-bridge/pair/status?' + params.toString(), {{ method: 'GET' }});
      }} catch (e) {{
        status.textContent = 'Bicameral is not reachable on this device. Start the local service and try again.';
        actions.hidden = false;
        return;
      }}

      let body = {{}};
      try {{ body = await response.json(); }} catch (e) {{ body = {{}}; }}
      const pairingStatus = (body.status || '').toLowerCase();
      if (!response.ok || pairingStatus !== 'approved' || !body.session_token) {{
        failToConnect();
        return;
      }}

      const restoredExpiry = body.expires_at || saved.expires_at;
      if (!restoredExpiry || Date.parse(restoredExpiry) <= Date.now()) {{ failToConnect(); return; }}
      try {{
        window.sessionStorage.setItem(HANDOFF_KEY, JSON.stringify({{
          session_token: body.session_token,
          daemon_url: saved.daemon_url.replace(/\\/+$/, ''),
          cloud_account_id: cloudAccountId,
          cloud_access_token: token,
          team_id: saved.team_id,
          expires_at: restoredExpiry,
        }}));
        // Keep the proof bounded to the daemon-authored live session expiry.
        saved.expires_at = restoredExpiry;
        window.localStorage.setItem(RESTORE_KEY, JSON.stringify(saved));
      }} catch (e) {{
        status.textContent = 'This browser could not restore the local connection state.';
        actions.hidden = false;
        return;
      }}
      window.location.replace('/app');
    }}

    document.getElementById('retry').addEventListener('click', () => restore());
    restore().catch(() => {{
      document.getElementById('status').textContent = 'The local connection could not be restored.';
      document.getElementById('actions').hidden = false;
    }});
  </script>
</body>
</html>
'''


@router.get('/', include_in_schema=False)
async def root() -> RedirectResponse:
    '''Redirect browser visitors to the signup entrypoint.'''
    return RedirectResponse(url='/signup', status_code=307)


@router.get('/signup', response_class=HTMLResponse)
async def signup() -> HTMLResponse:
    '''Render the design partner signup page.'''
    return HTMLResponse(content=_SIGNUP_HTML)


@router.get('/login', response_class=HTMLResponse)
async def login() -> HTMLResponse:
    '''Alias login to the signup page for landing-page integrations.'''
    return HTMLResponse(content=_SIGNUP_HTML)


@router.get('/auth/callback', response_class=HTMLResponse)
async def auth_callback() -> HTMLResponse:
    '''Render Supabase OAuth callback handler.'''
    return HTMLResponse(content=_CALLBACK_HTML)


@router.get('/connect', response_class=HTMLResponse)
async def connect() -> HTMLResponse:
    '''Render the Cloud-owned rare device-pairing bootstrap (product path).'''
    return HTMLResponse(content=_DASHBOARD_HTML)


@router.get('/restore', response_class=HTMLResponse)
async def restore() -> HTMLResponse:
    '''Re-attach a bounded, still-valid hosted pair after transient handoff loss.'''
    return HTMLResponse(content=_RESTORE_HTML)
