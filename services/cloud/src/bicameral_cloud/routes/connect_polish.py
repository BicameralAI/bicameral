'''APC1 frictionless user-entry surface for Cloud /connect.

This route is intentionally layered ahead of the frozen Alpha /connect route.
It changes presentation and orchestration only. The local daemon remains the
pairing and approval authority, and existing hosted-bridge contracts are reused
unchanged.
'''

# ruff: noqa: E501
from __future__ import annotations

import os

from fastapi import APIRouter
from fastapi.responses import HTMLResponse

from bicameral_cloud.contracts import HOSTED_BRIDGE_PROTOCOL_VERSION

router = APIRouter(tags=['connect-polish'])

CLOUD_ORIGIN = 'https://app.bicameral-ai.com'
CLIENT_PROTOCOL_VERSION = HOSTED_BRIDGE_PROTOCOL_VERSION
DEFAULT_DAEMON_PORT = int(os.environ.get('BICAMERAL_DAEMON_PORT', '37373'))
INSTALL_URL = 'https://install.bicameral-ai.com/'

_CONNECT_HTML = f'''<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Bicameral — Connect this device</title>
  <style>
    :root {{
      --ink:#0f1115; --muted:#667085; --faint:#98a2b3; --line:#e4e7ec;
      --surface:#fff; --panel:#f8fafc; --page:#fbfcfd; --cyan:#0891b2;
      --cyan-strong:#0e7490; --ok:#16a34a; --warn:#d97706; --error:#dc2626;
      font-family:Inter,ui-sans-serif,system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;
    }}
    * {{ box-sizing:border-box; }}
    body {{ margin:0; min-height:100vh; background:var(--page); color:var(--ink); }}
    [hidden] {{ display:none !important; }}
    code {{ font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace; }}
    .topbar {{ display:flex; align-items:center; justify-content:space-between; padding:16px 28px; border-bottom:1px solid var(--line); background:var(--surface); }}
    .brand {{ display:flex; align-items:center; gap:12px; }}
    .brand-mark {{ width:34px; height:34px; border-radius:8px; background:var(--ink); display:grid; place-items:center; }}
    .brand-mark span {{ width:16px; height:16px; border:2.5px solid #fff; border-right-color:var(--cyan); border-radius:50%; }}
    .brand-word {{ font-family:ui-monospace,SFMono-Regular,Menlo,monospace; font-weight:700; }}
    .brand-tag {{ font-size:.7rem; font-weight:700; letter-spacing:.14em; color:var(--cyan); }}
    .topbar-right {{ display:flex; align-items:center; gap:12px; }}
    .avatar {{ width:34px; height:34px; border-radius:50%; border:1px solid #c4b5fd; color:#7c3aed; display:grid; place-items:center; font-size:.75rem; font-weight:700; }}
    main {{ width:min(720px,calc(100vw - 40px)); margin:0 auto; padding:64px 0 80px; }}
    .overline {{ margin:0 0 10px; color:var(--cyan-strong); font-size:.72rem; font-weight:750; letter-spacing:.14em; text-transform:uppercase; }}
    h1 {{ margin:0; font-size:clamp(2rem,5vw,3rem); line-height:1.04; letter-spacing:-.035em; }}
    .lead {{ margin:16px 0 28px; max-width:620px; color:var(--muted); font-size:1rem; line-height:1.55; }}
    .card {{ background:var(--surface); border:1px solid var(--line); border-radius:14px; padding:24px; box-shadow:0 1px 2px rgba(16,24,40,.04); }}
    .card[data-tone="checking"],.card[data-tone="pending"] {{ border-color:#a5f3fc; }}
    .status-line {{ display:flex; align-items:center; gap:10px; margin:0 0 8px; }}
    .dot {{ width:11px; height:11px; border-radius:50%; background:var(--faint); flex:none; }}
    [data-tone="checking"] .dot,[data-tone="pending"] .dot {{ background:var(--cyan); }}
    [data-tone="ok"] .dot {{ background:var(--ok); }}
    [data-tone="warn"] .dot {{ background:var(--warn); }}
    [data-tone="error"] .dot {{ background:var(--error); }}
    [data-tone="checking"] .dot {{ animation:pulse 1.1s ease-in-out infinite; }}
    @keyframes pulse {{ 0%,100%{{opacity:1}} 50%{{opacity:.35}} }}
    .status-title {{ font-size:1.2rem; font-weight:750; }}
    .detail {{ margin:0 0 18px; color:var(--muted); line-height:1.5; }}
    .actions {{ display:flex; gap:10px; flex-wrap:wrap; margin-top:18px; }}
    .btn {{ appearance:none; border:1px solid transparent; border-radius:8px; padding:11px 17px; font:inherit; font-weight:650; cursor:pointer; text-decoration:none; display:inline-flex; align-items:center; justify-content:center; }}
    .primary {{ background:var(--cyan); color:#fff; }}
    .primary:hover {{ background:var(--cyan-strong); }}
    .secondary {{ background:var(--surface); color:var(--ink); border-color:var(--line); }}
    .pairing {{ margin:16px 0; padding:16px; border:1px solid #bae6fd; background:#ecfeff; border-radius:10px; }}
    .pairing code {{ display:block; font-size:clamp(1.35rem,5vw,1.8rem); font-weight:780; letter-spacing:.08em; margin:6px 0 10px; }}
    .platform {{ margin:14px 0 0; color:var(--muted); font-size:.86rem; }}
    details {{ margin-top:18px; border-top:1px solid var(--line); padding-top:14px; }}
    summary {{ cursor:pointer; color:var(--muted); font-size:.86rem; font-weight:650; }}
    .dev-copy {{ color:var(--muted); font-size:.86rem; line-height:1.5; }}
    .cmd {{ padding:10px 12px; margin:8px 0; border:1px solid var(--line); border-radius:8px; background:var(--panel); overflow:auto; }}
    .authority {{ margin:14px 2px 0; color:var(--faint); font-size:.82rem; line-height:1.45; }}
    .loading {{ color:var(--muted); }}
    @media(max-width:640px) {{
      .topbar {{ padding:13px 16px; }} .brand-tag {{ display:none; }}
      main {{ width:min(100% - 28px,720px); padding:40px 0 56px; }}
      .card {{ padding:20px; }} .actions .btn {{ width:100%; }}
    }}
  </style>
</head>
<body>
  <header class="topbar">
    <div class="brand"><span class="brand-mark" aria-hidden="true"><span></span></span><span class="brand-word">bicameral</span><span class="brand-tag">HOSTED</span></div>
    <div class="topbar-right"><span id="avatar" class="avatar" hidden></span><button id="signout" class="btn secondary" type="button" hidden>Sign out</button></div>
  </header>
  <main>
    <p id="session-status" class="loading">Checking your session…</p>
    <div id="signin" hidden><a class="btn primary" href="/signup">Sign in</a></div>
    <div id="experience" hidden>
      <p class="overline">Frictionless user entry</p>
      <h1>Connect Bicameral</h1>
      <p class="lead">We’ll look for Bicameral on this device first. If it’s already here, you’ll go straight to pairing. If it isn’t available yet, we’ll guide you through setup and keep checking in the background.</p>
      <section id="card" class="card" data-tone="checking" aria-live="polite">
        <p class="overline" id="eyebrow">This device</p>
        <p class="status-line"><span class="dot"></span><span id="title" class="status-title">Checking for Bicameral…</span></p>
        <p id="detail" class="detail">Looking for the local Bicameral service.</p>
        <div id="pairing" class="pairing" hidden>
          <p class="overline">Local approval</p>
          <strong>Confirm this pairing on your device</strong>
          <code id="pairing-code"></code>
          <p class="detail">Local approval remains required. This browser cannot approve itself.</p>
        </div>
        <p id="platform" class="platform" hidden></p>
        <div class="actions">
          <a id="install" class="btn primary" href="{INSTALL_URL}" target="_blank" rel="noopener" hidden>Install Bicameral</a>
          <button id="retry" class="btn secondary" type="button" hidden>I already installed it — try again</button>
          <button id="cancel" class="btn secondary" type="button" hidden>Cancel</button>
        </div>
        <details id="developer-help">
          <summary>Developer / troubleshooting options</summary>
          <p class="dev-copy">These commands are fallback tools, not the normal onboarding path.</p>
          <div class="cmd"><code>curl -fsSL https://install.bicameral-ai.com/install.sh | sh</code></div>
          <div class="cmd"><code>bicameral service install</code></div>
          <div class="cmd"><code>bicameral service start</code></div>
        </details>
      </section>
      <p class="authority">Cloud coordinates setup. The local daemon remains the pairing and approval authority.</p>
    </div>
  </main>
  <script src="https://cdn.jsdelivr.net/npm/@supabase/supabase-js@2"></script>
  <script>
    const CLOUD_ORIGIN = window.location.origin || '{CLOUD_ORIGIN}';
    const CLIENT_PROTOCOL_VERSION = {CLIENT_PROTOCOL_VERSION};
    const DAEMON_URL = 'http://localhost:{DEFAULT_DAEMON_PORT}';
    const PAIR_URL = DAEMON_URL + '/api/v1/hosted-bridge/pair';
    const INSTALL_URL = '{INSTALL_URL}';
    const RESTORE_KEY = 'bicameral_hosted_bridge_restore';
    const MAX_SETUP_POLLS = 30;
    const SETUP_POLL_MS = 2000;
    const PAIR_POLL_MS = 2000;
    const DAEMON_PROBE_TIMEOUT_MS = 6000;
    const DAEMON_PERMISSION_RETRY_TIMEOUT_MS = 1500;
    const PERMISSION_GRANT_SETTLE_MS = 120;
    let flow = null;
    let pairTimer = null;
    let setupTimer = null;
    let setupAttempts = 0;
    let pairGeneration = 0;
    let setupGeneration = 0;
    let pairingRequestInFlight = false;
    let lastLocalNetworkPermission = 'unknown';

    function setView(tone, eyebrow, title, detail) {{
      document.getElementById('card').dataset.tone = tone;
      document.getElementById('eyebrow').textContent = eyebrow;
      document.getElementById('title').textContent = title;
      document.getElementById('detail').textContent = detail;
    }}
    function showActions({{ install=false, retry=false, cancel=false }}={{}}) {{
      document.getElementById('install').hidden = !install;
      document.getElementById('retry').hidden = !retry;
      document.getElementById('cancel').hidden = !cancel;
    }}
    function detectPlatform() {{
      const ua = (navigator.userAgent || '').toLowerCase();
      const platform = (navigator.platform || '').toLowerCase();
      if (ua.includes('windows') || platform.includes('win')) return 'windows';
      if (ua.includes('mac os') || platform.includes('mac')) return 'macos';
      if (ua.includes('linux') || platform.includes('linux')) return 'linux';
      return 'unknown';
    }}
    function configureInstaller() {{
      const platform = detectPlatform();
      const label = platform === 'windows' ? 'Windows' : platform === 'macos' ? 'macOS' : platform === 'linux' ? 'Linux' : 'this device';
      const node = document.getElementById('platform');
      node.hidden = false;
      node.textContent = 'Detected ' + label + '. Setup will open the supported Bicameral installer experience.';
      const install = document.getElementById('install');
      install.href = INSTALL_URL;
      install.textContent = platform === 'windows' ? 'Install Bicameral for Windows' : 'Install Bicameral';
      return platform;
    }}
    async function localNetworkPermissionState() {{
      if (!navigator.permissions || typeof navigator.permissions.query !== 'function') return 'unknown';
      try {{
        const status = await navigator.permissions.query({{ name:'local-network-access' }});
        return status && status.state ? status.state : 'unknown';
      }} catch (_) {{
        return 'unknown';
      }}
    }}
    function localNetworkPermissionBlocked() {{
      return lastLocalNetworkPermission === 'denied';
    }}
    function showLocalNetworkPermissionRecovery() {{
      document.getElementById('platform').hidden = true;
      setView('warn','Browser permission needed','Allow this browser to find Bicameral.','Local Network Access is blocked for this site. Enable it in your browser site settings, then try again. Bicameral may already be running.');
      showActions({{ retry:true }});
    }}
    function sleep(ms) {{ return new Promise((resolve) => window.setTimeout(resolve, ms)); }}
    async function probeDaemon(timeoutMs) {{
      const controller = typeof AbortController === 'function' ? new AbortController() : null;
      const timer = controller ? window.setTimeout(() => controller.abort(), timeoutMs) : null;
      try {{
        const options = {{ method:'OPTIONS', cache:'no-store' }};
        if (controller) options.signal = controller.signal;
        const response = await fetch(PAIR_URL, options);
        return response.ok || response.status === 204;
      }} catch (_) {{
        return false;
      }} finally {{
        if (timer) window.clearTimeout(timer);
      }}
    }}
    async function daemonReachable() {{
      const permissionBeforeProbe = await localNetworkPermissionState();
      lastLocalNetworkPermission = permissionBeforeProbe;
      if (permissionBeforeProbe === 'denied') return false;
      if (permissionBeforeProbe === 'prompt') {{
        setView('checking','Browser permission','Allow local network access to continue.','Chrome may ask for permission to find Bicameral on this device. Choose Allow to continue.');
      }}
      if (await probeDaemon(DAEMON_PROBE_TIMEOUT_MS)) {{
        lastLocalNetworkPermission = await localNetworkPermissionState();
        return true;
      }}
      lastLocalNetworkPermission = await localNetworkPermissionState();
      if (permissionBeforeProbe === 'prompt' && lastLocalNetworkPermission === 'granted') {{
        await sleep(PERMISSION_GRANT_SETTLE_MS);
        if (await probeDaemon(DAEMON_PERMISSION_RETRY_TIMEOUT_MS)) return true;
        lastLocalNetworkPermission = await localNetworkPermissionState();
      }}
      return false;
    }}
    function invalidateSetupRun() {{
      setupGeneration += 1;
      if (setupTimer) window.clearTimeout(setupTimer);
      setupTimer = null;
    }}
    function invalidatePairRun() {{
      pairGeneration += 1;
      if (pairTimer) window.clearTimeout(pairTimer);
      pairTimer = null;
      pairingRequestInFlight = false;
    }}
    function clearPairingUi() {{
      document.getElementById('pairing').hidden = true;
      if (flow) flow.pairing = null;
    }}
    function showSetupNeeded() {{
      configureInstaller();
      const detail = lastLocalNetworkPermission === 'prompt'
        ? 'Install or start Bicameral, then leave this page open. If Chrome asks to find devices on your network, choose Allow. We’ll continue automatically when Bicameral is ready.'
        : 'Install or start Bicameral, then leave this page open. We’ll keep checking and continue automatically when it is ready.';
      setView('warn','Setup needed','Bicameral isn’t available on this device yet.',detail);
      showActions({{ install:true, retry:true }});
    }}
    async function probeAndAdvance() {{
      invalidateSetupRun();
      invalidatePairRun();
      clearPairingUi();
      document.getElementById('platform').hidden = true;
      setView('checking','This device','Checking for Bicameral…','Looking for the local Bicameral service.');
      showActions();
      if (await daemonReachable()) {{
        setView('ok','Bicameral ready','Bicameral is ready. Connecting…','The local service is available. Starting the existing secure pairing flow.');
        await beginPairing();
        return true;
      }}
      if (localNetworkPermissionBlocked()) {{
        showLocalNetworkPermissionRecovery();
        return false;
      }}
      showSetupNeeded();
      return false;
    }}
    function beginSetupPolling() {{
      invalidateSetupRun();
      const runId = setupGeneration;
      setupAttempts = 0;
      setView('checking','Finishing setup','Waiting for Bicameral…','Keep this page open. We’ll continue automatically when the local service starts.');
      showActions({{ retry:true }});
      const poll = async () => {{
        if (runId !== setupGeneration) return;
        setupAttempts += 1;
        const reachable = await daemonReachable();
        if (runId !== setupGeneration) return;
        if (reachable) {{
          invalidateSetupRun();
          setView('ok','Bicameral ready','Bicameral is ready. Connecting…','Setup is complete. Continuing to pairing.');
          await beginPairing();
          return;
        }}
        if (localNetworkPermissionBlocked()) {{
          invalidateSetupRun();
          showLocalNetworkPermissionRecovery();
          return;
        }}
        if (setupAttempts >= MAX_SETUP_POLLS) {{
          invalidateSetupRun();
          configureInstaller();
          setView('warn','Still waiting','Bicameral hasn’t started yet.','You can retry now or reopen the installer. Your sign-in state is preserved.');
          showActions({{ install:true,retry:true }});
          return;
        }}
        setupTimer = window.setTimeout(poll, SETUP_POLL_MS);
      }};
      setupTimer = window.setTimeout(poll, SETUP_POLL_MS);
    }}
    function csrfToken() {{
      if (window.crypto && typeof window.crypto.randomUUID === 'function') return window.crypto.randomUUID();
      if (window.crypto && window.crypto.getRandomValues) {{
        const bytes = new Uint8Array(16);
        window.crypto.getRandomValues(bytes);
        return Array.from(bytes,b=>b.toString(16).padStart(2,'0')).join('');
      }}
      return 'csrf-' + Date.now() + '-' + Math.random().toString(36).slice(2);
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
    function showRecoveryState(state) {{
      clearPairingUi();
      if (state === 'denied') {{ setView('error','Connection declined','Local approval was declined.','Nothing was connected. Start a new request when you are ready.'); showActions({{ retry:true }}); return; }}
      if (state === 'expired') {{ setView('warn','Pairing expired','Connection expired.','The local approval window expired. Start a new connection request.'); showActions({{ retry:true }}); return; }}
      if (state === 'revoked') {{ setView('error','Pairing revoked','Connection revoked.','This browser session was disconnected by the local service.'); showActions({{ retry:true }}); return; }}
      if (state === 'account_mismatch') {{ setView('error','Account mismatch','Use the matching account.','The local pairing request belongs to a different signed-in account. Sign out, then use the matching account.'); showActions(); document.getElementById('signout').hidden = false; return; }}
      if (state === 'tenancy_blocked') {{ setView('error','Team required','Cannot resolve Cloud team.','Pairing needs exactly one Cloud team for this signed-in account.'); showActions({{ retry:true }}); return; }}
      if (state === 'update_required') {{ configureInstaller(); setView('warn','Update needed','Update Bicameral to continue.','The local service does not support this pairing contract yet. Install the current supported version, then try again.'); showActions({{ install:true,retry:true }}); return; }}
      setView('error','Couldn’t connect','Bicameral could not complete pairing.','Try again.');
      showActions({{ retry:true }});
    }}
    async function beginPairing() {{
      if (!flow || pairingRequestInFlight) return;
      invalidatePairRun();
      const runId = pairGeneration;
      pairingRequestInFlight = true;
      showActions();
      let tenancy;
      try {{
        const tenancyResponse = await fetch('/api/v1/auth/pairing-tenancy', {{ method:'POST', headers:{{ Authorization:'Bearer ' + flow.cloudAccessToken }} }});
        tenancy = await tenancyResponse.json().catch(() => ({{}}));
        if (runId !== pairGeneration) return;
        if (!tenancyResponse.ok || !tenancy.team_id) {{ pairingRequestInFlight = false; showRecoveryState('tenancy_blocked'); return; }}
      }} catch (_) {{
        if (runId !== pairGeneration) return;
        pairingRequestInFlight = false;
        showRecoveryState('tenancy_blocked');
        return;
      }}
      flow.teamId = tenancy.team_id;
      const csrf = csrfToken();
      let response;
      try {{
        response = await fetch(PAIR_URL, {{ method:'POST', headers:{{ 'Content-Type':'application/json','x-csrf-token':csrf }}, body:JSON.stringify({{ origin:CLOUD_ORIGIN, cloud_account_id:flow.cloudAccountId, csrf_token:csrf, team_id:flow.teamId, cloud_access_token:flow.cloudAccessToken, client_protocol_version:CLIENT_PROTOCOL_VERSION }}) }});
      }} catch (_) {{
        if (runId !== pairGeneration) return;
        pairingRequestInFlight = false;
        configureInstaller();
        setView('warn','Connection changed','Bicameral is no longer reachable.','Restart it and we’ll continue from here.');
        showActions({{ install:true,retry:true }});
        return;
      }}
      if (runId !== pairGeneration) return;
      const body = await response.json().catch(() => ({{}}));
      if (runId !== pairGeneration) return;
      pairingRequestInFlight = false;
      if (response.status === 202 || body.status === 'pending') {{
        if (!body.request_handle || !body.poll_proof || !body.pairing_code) {{ showRecoveryState('update_required'); return; }}
        flow.pairing = {{ requestHandle:body.request_handle, pollProof:body.poll_proof, pairingCode:body.pairing_code, runId:runId }};
        document.getElementById('pairing-code').textContent = body.pairing_code;
        document.getElementById('pairing').hidden = false;
        setView('pending','Local confirmation required','Approve this browser on your device.','Bicameral is waiting for explicit local approval.');
        showActions({{ cancel:true }});
        schedulePairPoll(runId);
        return;
      }}
      if (!response.ok) {{ showRecoveryState(recoveryStateForPairError(response.status, body)); return; }}
      showRecoveryState('update_required');
    }}
    function schedulePairPoll(runId) {{
      if (runId !== pairGeneration || !flow || !flow.pairing) return;
      if (pairTimer) window.clearTimeout(pairTimer);
      pairTimer = window.setTimeout(() => pollPairing(runId), PAIR_POLL_MS);
    }}
    async function pollPairing(runId) {{
      if (runId !== pairGeneration || !flow || !flow.pairing) return;
      const pairing = flow.pairing;
      const params = new URLSearchParams({{ request_handle:pairing.requestHandle, poll_proof:pairing.pollProof, cloud_account_id:flow.cloudAccountId }});
      let response;
      try {{
        response = await fetch(DAEMON_URL + '/api/v1/hosted-bridge/pair/status?' + params.toString(), {{ method:'GET' }});
      }} catch (_) {{
        if (runId !== pairGeneration || !flow || flow.pairing !== pairing) return;
        invalidatePairRun();
        clearPairingUi();
        configureInstaller();
        setView('warn','Connection interrupted','Bicameral stopped responding.','Restart the local service and retry.');
        showActions({{ install:true,retry:true }});
        return;
      }}
      if (runId !== pairGeneration || !flow || flow.pairing !== pairing) return;
      const body = await response.json().catch(() => ({{}}));
      if (runId !== pairGeneration || !flow || flow.pairing !== pairing) return;
      const status = (body.status || '').toLowerCase();
      if (status === 'pending' || status === '') {{ schedulePairPoll(runId); return; }}
      if (status === 'approved' && body.session_token) {{
        invalidatePairRun();
        document.getElementById('pairing').hidden = true;
        try {{
          window.sessionStorage.setItem('bicameral_hosted_bridge', JSON.stringify({{ session_token:body.session_token, daemon_url:DAEMON_URL, cloud_account_id:flow.cloudAccountId, cloud_access_token:flow.cloudAccessToken, team_id:flow.teamId, expires_at:body.expires_at || null }}));
        }} catch (_) {{
          clearPairingUi();
          setView('error','Browser storage unavailable','This browser could not store the transient handoff.','Enable site storage and try again.');
          showActions({{ retry:true }});
          return;
        }}
        try {{
          if (body.expires_at) window.localStorage.setItem(RESTORE_KEY, JSON.stringify({{ request_handle:pairing.requestHandle, poll_proof:pairing.pollProof, daemon_url:DAEMON_URL, cloud_account_id:flow.cloudAccountId, team_id:flow.teamId, expires_at:body.expires_at }}));
        }} catch (_) {{
          // Durable restore is optional. Never block a valid transient handoff.
        }}
        clearPairingUi();
        setView('ok','Connected','You’re connected.','Opening Bicameral…');
        window.location.href = '/app';
        return;
      }}
      invalidatePairRun();
      showRecoveryState(recoveryStateForStatus(status));
    }}
    function cancelPairing() {{
      invalidatePairRun();
      clearPairingUi();
      setView('idle','Pairing cancelled','Nothing was connected.','You can start again whenever you are ready.');
      showActions({{ retry:true }});
    }}
    function readRestoreCandidate() {{
      try {{
        const raw = window.localStorage.getItem(RESTORE_KEY);
        if (!raw) return null;
        const value = JSON.parse(raw);
        const expiry = Date.parse(value.expires_at || '');
        if (!value.request_handle || !value.poll_proof || !value.cloud_account_id || !value.team_id || !Number.isFinite(expiry) || expiry <= Date.now()) {{
          window.localStorage.removeItem(RESTORE_KEY);
          return null;
        }}
        return value;
      }} catch (_) {{
        try {{ window.localStorage.removeItem(RESTORE_KEY); }} catch (_) {{}}
        return null;
      }}
    }}
    async function main() {{
      const sessionStatus = document.getElementById('session-status');
      const signin = document.getElementById('signin');
      const experience = document.getElementById('experience');
      const configResponse = await fetch('/api/v1/auth/config');
      const config = await configResponse.json();
      if (!config.supabase_browser_auth_configured) {{ sessionStatus.textContent = 'Bicameral Cloud sign-in is not configured.'; return; }}
      const client = window.supabase.createClient(config.supabase_url, config.supabase_anon_key);
      const session = await client.auth.getSession();
      const token = session.data.session && session.data.session.access_token;
      document.getElementById('signout').hidden = false;
      document.getElementById('signout').addEventListener('click', async () => {{
        invalidateSetupRun();
        invalidatePairRun();
        try {{ window.localStorage.removeItem(RESTORE_KEY); }} catch (_) {{}}
        await client.auth.signOut();
        window.location.href='/signup';
      }});
      if (!token) {{ sessionStatus.textContent='Sign in to connect this device.'; signin.hidden=false; return; }}
      const authResponse = await fetch('/api/v1/auth/session', {{ headers:{{ Authorization:'Bearer ' + token }} }});
      if (!authResponse.ok) {{
        await client.auth.signOut();
        if (authResponse.status === 403) sessionStatus.textContent='This Google account is not enabled for the design partner phase.';
        else if (authResponse.status === 503) sessionStatus.textContent='Cloud can’t verify design-partner access right now. Try again later.';
        else sessionStatus.textContent='Your session is no longer valid. Please sign in again.';
        signin.hidden=false;
        return;
      }}
      const user = await authResponse.json();
      const cloudAccountId = user.user_id || user.email;
      const restore = readRestoreCandidate();
      if (restore) {{
        if (restore.cloud_account_id === cloudAccountId) {{ window.location.replace('/restore'); return; }}
        try {{ window.localStorage.removeItem(RESTORE_KEY); }} catch (_) {{}}
      }}
      sessionStatus.hidden = true;
      experience.hidden = false;
      const avatar = document.getElementById('avatar');
      avatar.textContent=(user.email || '?').slice(0,2).toUpperCase();
      avatar.hidden=false;
      flow = {{ cloudAccountId:cloudAccountId, cloudAccessToken:token, teamId:null, pairing:null }};
      await probeAndAdvance();
    }}
    document.getElementById('retry').addEventListener('click', async () => {{ await probeAndAdvance(); }});
    document.getElementById('install').addEventListener('click', () => {{ beginSetupPolling(); }});
    document.getElementById('cancel').addEventListener('click', cancelPairing);
    main().catch((error) => {{
      document.getElementById('session-status').hidden = false;
      document.getElementById('session-status').textContent = 'Connect could not initialize: ' + error.message;
    }});
  </script>
</body>
</html>
'''


@router.get('/connect', response_class=HTMLResponse, include_in_schema=False)
async def connect_polish() -> HTMLResponse:
    '''Render APC1 frictionless connect onboarding without changing daemon authority.'''
    return HTMLResponse(content=_CONNECT_HTML)
