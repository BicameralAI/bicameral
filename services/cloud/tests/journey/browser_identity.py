"""Deterministic authenticated browser identity for the hosted dashboard.

The hosted dashboard (``routes/dashboard.py``) authenticates the browser via
the Supabase JS client loaded from a CDN and reads the access token from
``client.auth.getSession()``. To run the journey deterministically with no
production Supabase project and no Google OAuth, we:

- mint a Supabase-shaped HS256 access token locally (validated server-side via
  ``SUPABASE_JWT_SECRET``, exactly like the conformance harness);
- fulfill the Supabase-JS CDN request with a tiny in-page stub whose
  ``getSession()`` returns that minted token, so the real dashboard JS runs
  its real fetch/render code against the live Cloud APIs;
- neutralize non-essential third-party requests (web fonts) and the local
  daemon capability probe so the journey is hermetic and offline.

Only the browser *identity/session state* is faked. The registration and
discovery HTTP contracts under exercise are the real Cloud routes.
"""

from __future__ import annotations

import json

from playwright.sync_api import Page, Route

# Reuse the conformance token minter so the browser identity and the
# server-side validation share one contract.
from tests.conformance.fake_supabase import mint_supabase_jwt

__all__ = [
    "mint_supabase_jwt",
    "install_browser_identity",
    "install_hosted_bridge_handoff",
    "DEFAULT_DAEMON_URL",
]

# The canonical local daemon URL the hosted `/connect` writes into the
# same-origin ``bicameral_hosted_bridge`` handoff on successful pairing.
DEFAULT_DAEMON_URL = "http://localhost:37373"

_SUPABASE_CDN_GLOB = "https://cdn.jsdelivr.net/npm/@supabase/supabase-js*"
_FONTS_GLOBS = (
    "https://fonts.googleapis.com/**",
    "https://fonts.gstatic.com/**",
)


def _supabase_stub_js(access_token: str, supabase_url: str, anon_key: str) -> str:
    """A minimal drop-in for ``window.supabase`` returning a fixed session.

    Matches the shape the dashboard consumes: ``createClient(...).auth`` with
    ``getSession()`` (returns the seeded access token) and ``signOut()``.
    """

    token_json = json.dumps(access_token)
    url_json = json.dumps(supabase_url)
    key_json = json.dumps(anon_key)
    return f"""
    window.supabase = {{
      createClient: function (url, key) {{
        return {{
          _url: {url_json},
          _key: {key_json},
          auth: {{
            getSession: async function () {{
              return {{ data: {{ session: {{ access_token: {token_json} }} }},
                       error: null }};
            }},
            signOut: async function () {{ return {{ error: null }}; }},
          }},
        }};
      }},
    }};
    """


def install_browser_identity(
    page: Page,
    *,
    access_token: str,
    supabase_url: str,
    anon_key: str,
    daemon_capabilities_glob: str = "**/v2/capabilities",
) -> None:
    """Route third-party requests so the dashboard authenticates hermetically.

    - Supabase-JS CDN -> in-page stub returning ``access_token``.
    - Web fonts -> aborted (cosmetic; keeps the journey offline).
    - Local daemon ``/v2/capabilities`` probe -> aborted, so the daemon
      capability resolves to unknown (fail-closed) and a freshly created
      Project renders the existing ``discoverable_unbound`` "needs binding"
      state rather than a bindable/ready state.
    """

    stub = _supabase_stub_js(access_token, supabase_url, anon_key)

    def _fulfill_supabase(route: Route) -> None:
        route.fulfill(
            status=200,
            content_type="application/javascript; charset=utf-8",
            body=stub,
        )

    page.route(_SUPABASE_CDN_GLOB, _fulfill_supabase)
    for glob in _FONTS_GLOBS:
        page.route(glob, lambda route: route.abort())
    page.route(daemon_capabilities_glob, lambda route: route.abort())


def install_hosted_bridge_handoff(
    page: Page,
    *,
    access_token: str,
    cloud_account_id: str,
    daemon_url: str = DEFAULT_DAEMON_URL,
    session_token: str = "journey-hosted-bridge-session",
    expires_at: str = "2099-01-01T00:00:00Z",
) -> None:
    """Seed the same-origin pairing handoff the canonical ``/app`` SPA reads.

    This is the exact ``bicameral_hosted_bridge`` ``sessionStorage`` payload
    that ``/connect`` writes on a successful daemon pairing before handing
    the browser off to ``/app`` (``routes/dashboard.py`` ``completeHandoff``). By
    seeding it directly, the journey enters the canonical ``/app`` surface in
    the handed-off state without needing a live local daemon, then exercises
    the real Cloud create/discovery contracts from there.

    The local daemon itself is not reachable in the hermetic journey, so the
    created Project stays ``unbound`` ("needs binding") until a real daemon
    exposes it — Cloud never binds local workspaces.
    """

    handoff = json.dumps(
        {
            "session_token": session_token,
            "daemon_url": daemon_url,
            "cloud_account_id": cloud_account_id,
            "cloud_access_token": access_token,
            "expires_at": expires_at,
        }
    )
    page.add_init_script(
        "window.sessionStorage.setItem("
        f"'bicameral_hosted_bridge', {json.dumps(handoff)});"
    )
    # The handed-off daemon is not reachable in the hermetic journey: fail
    # closed so the Project renders unbound rather than bindable/ready.
    page.route(f"{daemon_url}/**", lambda route: route.abort())
