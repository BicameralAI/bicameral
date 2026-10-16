'''APC1 Google OAuth reliability overlay for production onboarding.

Supabase documents that some Google Workspace accounts require the email scope
to be requested explicitly or the provider callback can fail before a Supabase
session is established. This overlay keeps that repair isolated from pairing and
provider-authority code while applying it to every user-facing Cloud sign-in
entry point.
'''

from __future__ import annotations

from fastapi import APIRouter
from fastapi.responses import HTMLResponse

from bicameral_cloud.routes.connect_local_accountability import (
    _CONNECT_HTML as _BASE_CONNECT_HTML,
)
from bicameral_cloud.routes.dashboard import _CALLBACK_HTML as _BASE_CALLBACK_HTML
from bicameral_cloud.routes.signup_polish_apc1 import (
    _SIGNUP_HTML as _BASE_SIGNUP_HTML,
)

router = APIRouter(tags=['oauth-apc1'])

GOOGLE_EMAIL_SCOPE = 'https://www.googleapis.com/auth/userinfo.email'


def _scope_connect_html(html: str) -> str:
    hook = "options:{ redirectTo: window.location.origin + '/auth/callback' }"
    replacement = (
        "options:{ redirectTo: window.location.origin + '/auth/callback', "
        f"scopes:'{GOOGLE_EMAIL_SCOPE}' }}"
    )
    if html.count(hook) != 1:
        raise RuntimeError('Google OAuth connect scope repair failed closed')
    return html.replace(hook, replacement, 1)


def _scope_signup_html(html: str) -> str:
    hook = "options: { redirectTo: window.location.origin + '/auth/callback' }"
    replacement = (
        "options: { redirectTo: window.location.origin + '/auth/callback', "
        f"scopes: '{GOOGLE_EMAIL_SCOPE}' }}"
    )
    if html.count(hook) != 1:
        raise RuntimeError('Google OAuth signup scope repair failed closed')
    return html.replace(hook, replacement, 1)


def _surface_provider_callback_error(html: str) -> str:
    hook = (
        "      const actions = document.getElementById('actions');\n"
        "      const response = await fetch('/api/v1/auth/config');"
    )
    replacement = """      const actions = document.getElementById('actions');
      const callbackParams = new URLSearchParams(window.location.search);
      const callbackError = callbackParams.get('error');
      const callbackErrorCode = callbackParams.get('error_code');
      const callbackErrorDescription = callbackParams.get('error_description');
      if (callbackError) {
        const detail = callbackErrorDescription || callbackErrorCode || callbackError;
        status.textContent =
          'Google sign-in failed before a Cloud session was created: ' + detail;
        actions.hidden = false;
        return;
      }
      const response = await fetch('/api/v1/auth/config');"""
    if html.count(hook) != 1:
        raise RuntimeError('Google OAuth callback diagnostic repair failed closed')
    return html.replace(hook, replacement, 1)


_CONNECT_HTML = _scope_connect_html(_BASE_CONNECT_HTML)
_SIGNUP_HTML = _scope_signup_html(_BASE_SIGNUP_HTML)
_CALLBACK_HTML = _surface_provider_callback_error(_BASE_CALLBACK_HTML)


@router.get('/connect', response_class=HTMLResponse, include_in_schema=False)
async def connect_with_explicit_google_email_scope() -> HTMLResponse:
    return HTMLResponse(content=_CONNECT_HTML)


@router.get('/signup', response_class=HTMLResponse)
async def signup_with_explicit_google_email_scope() -> HTMLResponse:
    return HTMLResponse(content=_SIGNUP_HTML)


@router.get('/login', response_class=HTMLResponse)
async def login_with_explicit_google_email_scope() -> HTMLResponse:
    return HTMLResponse(content=_SIGNUP_HTML)


@router.get('/auth/callback', response_class=HTMLResponse, include_in_schema=False)
async def callback_with_provider_error_detail() -> HTMLResponse:
    return HTMLResponse(content=_CALLBACK_HTML)
