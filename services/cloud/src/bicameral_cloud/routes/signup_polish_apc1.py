from __future__ import annotations

# ruff: noqa: E501
from pathlib import Path

from fastapi import APIRouter, HTTPException
from fastapi.responses import HTMLResponse

router = APIRouter(tags=["signup-polish"])

_SIGNUP_HTML = '''<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Bicameral Cloud — Sign in</title>
  <style>
    :root {
      color-scheme: light;
      font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      --ink:#1d1d1b; --muted:#66635d; --line:#d8d5ce; --surface:#f7f7f4; --card:#ffffff;
      --ok:#15803d; --pending:#8a8175;
    }
    * { box-sizing: border-box; }
    body { margin:0; min-height:100vh; display:grid; place-items:center; background:var(--surface); color:var(--ink); }
    main { width:min(640px,calc(100vw - 40px)); padding:48px 0; }
    h1 { font-size:clamp(2rem,5vw,4rem); line-height:1; margin:0 0 16px; }
    p { font-size:1rem; line-height:1.55; color:var(--muted); }
    .steps { margin:28px 0 0; border:1px solid var(--line); border-radius:12px; background:var(--card); overflow:hidden; }
    .step { display:flex; gap:14px; align-items:flex-start; padding:18px 20px; }
    .step + .step { border-top:1px solid var(--line); }
    .mark { width:26px; height:26px; border:1px solid var(--line); border-radius:50%; display:grid; place-items:center; flex:none; font-weight:700; color:var(--pending); }
    .step[data-state="done"] .mark { border-color:var(--ok); color:var(--ok); }
    .step-title { margin:0; color:var(--ink); font-weight:700; }
    .step-detail { margin:3px 0 0; font-size:.9rem; }
    .actions { margin-top:18px; }
    button { appearance:none; border:1px solid var(--ink); background:var(--ink); color:white; padding:12px 16px; border-radius:6px; font:inherit; font-weight:650; cursor:pointer; }
    .panel { border-top:1px solid var(--line); margin-top:28px; padding-top:18px; }
    [hidden] { display:none !important; }
  </style>
</head>
<body>
  <main>
    <h1>Connect Bicameral</h1>
    <p>Connect Bicameral in three distinct steps. Each step confirms only what it actually establishes.</p>

    <div class="steps" aria-label="Connection progress">
      <div class="step" id="cloud-step" data-state="pending">
        <div class="mark" id="cloud-mark">1</div>
        <div>
          <p class="step-title">Bicameral Cloud</p>
          <p class="step-detail" id="cloud-detail">Sign in with your approved Google account.</p>
          <div class="actions"><button id="google" type="button" hidden>Continue with Google</button></div>
        </div>
      </div>
      <div class="step" data-state="pending">
        <div class="mark">2</div>
        <div>
          <p class="step-title">This device</p>
          <p class="step-detail">Bicameral will detect the local service and request secure pairing approval.</p>
        </div>
      </div>
      <div class="step" data-state="pending">
        <div class="mark">3</div>
        <div>
          <p class="step-title">Google Drive</p>
          <p class="step-detail">After device pairing, authorize Drive separately using the existing Google session when appropriate.</p>
        </div>
      </div>
    </div>

    <p id="status">Checking sign-in configuration...</p>
    <div class="panel"><p>Google sign-in establishes Bicameral Cloud identity. Google Drive authorization is a separate provider connection with separate scopes and local token custody.</p></div>
  </main>
  <script src="https://cdn.jsdelivr.net/npm/@supabase/supabase-js@2"></script>
  <script>
    async function main() {
      const status = document.getElementById('status');
      const button = document.getElementById('google');
      const cloudStep = document.getElementById('cloud-step');
      const cloudMark = document.getElementById('cloud-mark');
      const cloudDetail = document.getElementById('cloud-detail');
      const response = await fetch('/api/v1/auth/config');
      const config = await response.json();
      if (!config.supabase_browser_auth_configured) {
        status.textContent = 'Sign-in is not configured yet.';
        return;
      }
      const client = window.supabase.createClient(config.supabase_url, config.supabase_anon_key);
      const session = await client.auth.getSession();
      const token = session.data.session && session.data.session.access_token;
      if (token) {
        const authResponse = await fetch('/api/v1/auth/session', {
          headers: { Authorization: 'Bearer ' + token }
        });
        if (authResponse.ok) {
          cloudStep.dataset.state = 'done';
          cloudMark.textContent = '✓';
          cloudDetail.textContent = 'Signed in with Google.';
          status.textContent = 'Cloud sign-in complete. Continuing to this device...';
          window.location.href = '/connect';
          return;
        }
        await client.auth.signOut();
        if (authResponse.status === 403) {
          status.textContent = 'This Google account is not enabled for the design partner phase. Sign in with an approved account.';
        } else if (authResponse.status === 503) {
          status.textContent = 'Cloud cannot verify design-partner access right now. Try again after server configuration is restored.';
        } else {
          status.textContent = 'Previous session was rejected by Cloud. Sign in again.';
        }
      } else {
        status.textContent = 'Sign in to Bicameral Cloud to continue.';
      }
      button.hidden = false;
      button.addEventListener('click', async () => {
        status.textContent = 'Redirecting to Google...';
        await client.auth.signInWithOAuth({
          provider: 'google',
          options: { redirectTo: window.location.origin + '/auth/callback' }
        });
      });
    }
    main().catch((error) => {
      document.getElementById('status').textContent = 'Sign-in failed to initialize: ' + error.message;
    });
  </script>
</body>
</html>
'''

_APP_ONBOARDING_SCRIPT = '''
<script id="bicameral-onboarding-drive">
(() => {
  const params = new URLSearchParams(window.location.search);
  if (params.get('onboarding') !== 'drive') return;

  const openDriveSetup = () => {
    const button = document.querySelector('[data-testid="host-status-drive"]');
    if (!(button instanceof HTMLButtonElement)) return false;
    button.click();
    params.delete('onboarding');
    const query = params.toString();
    const cleanUrl = window.location.pathname + (query ? '?' + query : '') + window.location.hash;
    window.history.replaceState(null, '', cleanUrl);
    return true;
  };

  if (openDriveSetup()) return;
  const observer = new MutationObserver(() => {
    if (openDriveSetup()) observer.disconnect();
  });
  observer.observe(document.documentElement, { childList: true, subtree: true });
  window.setTimeout(() => observer.disconnect(), 30000);
})();
</script>
'''


def _render_app_with_onboarding_intent() -> str:
    spa_index = Path(__file__).resolve().parents[1] / "static" / "dashboard" / "index.html"
    if not spa_index.is_file():
        raise HTTPException(
            status_code=503,
            detail="Dashboard bundle not built. Run scripts/vendor_dashboard.sh.",
        )
    html = spa_index.read_text(encoding="utf-8")
    if "</body>" not in html:
        raise HTTPException(status_code=503, detail="Dashboard bundle is malformed.")
    return html.replace("</body>", _APP_ONBOARDING_SCRIPT + "\n</body>", 1)


@router.get('/signup', response_class=HTMLResponse)
async def signup_progressive() -> HTMLResponse:
    return HTMLResponse(content=_SIGNUP_HTML)


@router.get('/login', response_class=HTMLResponse)
async def login_progressive() -> HTMLResponse:
    return HTMLResponse(content=_SIGNUP_HTML)


@router.get('/app', response_class=HTMLResponse, include_in_schema=False)
async def app_with_onboarding_intent() -> HTMLResponse:
    '''Serve the canonical SPA with a bounded post-pair navigation hint.'''
    return HTMLResponse(
        content=_render_app_with_onboarding_intent(),
        headers={"Cache-Control": "no-store"},
    )
