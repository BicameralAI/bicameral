//! Locally served hosted-pairing approval surface (#790, cloud#353).
//!
//! The daemon is the pairing authority. This module serves the minimal
//! loopback web page a person uses to approve or deny a pending hosted
//! pairing request, so normal onboarding does not require the terminal.
//!
//! Authority is unchanged: the page only calls the existing LOCAL-only
//! pending-view and decision endpoints, which already fail closed for any
//! non-loopback `Origin` and require the one-time local confirmation nonce.
//! Bicameral Cloud may *open* this page (via `local_confirmation_url`) but
//! cannot read it (no CORS) or frame it (`frame-ancestors 'none'`), and so can
//! never submit a decision. `bicameral pairing approve` remains the fallback.
//!
//! These routes are deliberately outside the browser data plane: they carry
//! no CORS/PNA headers. The LOCAL-only pairing endpoints the page calls also
//! refuse a non-loopback `Host` (see `host_is_loopback_or_absent`).

use crate::state::AppState;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;

/// Path of the local approval page. Cloud receives it, with the request
/// handle, as `LocalConfirmationAffordance::local_confirmation_url`.
pub const LOCAL_PAIRING_PATH: &str = "/local/pairing";
const LOCAL_PAIRING_SCRIPT_PATH: &str = "/local/pairing.js";

const PAGE_HTML: &str = include_str!("../assets/local-pairing.html");
const PAGE_JS: &str = include_str!("../assets/local-pairing.js");

/// Strict policy: same-origin script/fetch only, inline style only, no
/// framing, no forms, no referrer.
const PAGE_CSP: &str = "default-src 'none'; script-src 'self'; connect-src 'self'; \
style-src 'unsafe-inline'; img-src 'self' data:; base-uri 'none'; form-action 'none'; \
frame-ancestors 'none'";

/// Same-origin relative URL for the local approval page of one request.
pub fn local_confirmation_url(request_handle: &str) -> String {
    let mut url = String::from(LOCAL_PAIRING_PATH);
    url.push_str("?request=");
    url.extend(request_handle.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
            (b as char).to_string()
        }
        _ => format!("%{b:02X}"),
    }));
    url
}

pub fn local_approval_routes() -> Router<AppState> {
    Router::new()
        .route(LOCAL_PAIRING_PATH, get(local_pairing_page))
        .route(LOCAL_PAIRING_SCRIPT_PATH, get(local_pairing_script))
}

/// The page is only meaningful when addressed as loopback. Refusing other
/// `Host` values keeps a DNS-rebound name from serving the surface.
fn is_loopback_host_header(headers: &HeaderMap) -> bool {
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let host = host.trim();
    let name = if let Some(rest) = host.strip_prefix('[') {
        match rest.split_once(']') {
            Some((inner, _)) => inner,
            None => return false,
        }
    } else {
        host.split(':').next().unwrap_or_default()
    };
    crate::is_loopback_host(&name.to_ascii_lowercase())
}

/// LOCAL-only pairing endpoints: a browser always sends `Host`, so a
/// non-loopback value means a DNS-rebound name reading the pending view (and
/// its nonce). In-process and CLI callers that omit `Host` stay allowed.
pub(crate) fn host_is_loopback_or_absent(headers: &HeaderMap) -> bool {
    headers.get(header::HOST).is_none() || is_loopback_host_header(headers)
}

fn page_response(headers: &HeaderMap, content_type: &'static str, body: &'static str) -> Response {
    if !is_loopback_host_header(headers) {
        return (
            StatusCode::MISDIRECTED_REQUEST,
            "Bicameral approval is only available at http://127.0.0.1 on this computer.",
        )
            .into_response();
    }
    let mut response = (StatusCode::OK, body).into_response();
    let h = response.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(PAGE_CSP),
    );
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn local_pairing_page(headers: HeaderMap) -> Response {
    page_response(&headers, "text/html; charset=utf-8", PAGE_HTML)
}

async fn local_pairing_script(headers: HeaderMap) -> Response {
    page_response(&headers, "text/javascript; charset=utf-8", PAGE_JS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_url_is_relative_and_encodes_handle() {
        assert_eq!(
            local_confirmation_url("pair-abc_123"),
            "/local/pairing?request=pair-abc_123"
        );
        assert_eq!(
            local_confirmation_url("a b&c"),
            "/local/pairing?request=a%20b%26c"
        );
    }

    #[test]
    fn host_guard_accepts_only_loopback() {
        let check = |host: &str| {
            let mut h = HeaderMap::new();
            h.insert(header::HOST, HeaderValue::from_str(host).unwrap());
            is_loopback_host_header(&h)
        };
        assert!(check("127.0.0.1:37373"));
        assert!(check("localhost:37373"));
        assert!(check("LOCALHOST"));
        assert!(check("[::1]:37373"));
        assert!(!check("evil.example:37373"));
        assert!(!check("127.0.0.1.evil.example"));
        assert!(!check("[::2]:37373"));
        assert!(!is_loopback_host_header(&HeaderMap::new()));
    }

    #[test]
    fn local_endpoints_allow_absent_host_but_refuse_rebound_names() {
        assert!(host_is_loopback_or_absent(&HeaderMap::new()));
        let mut h = HeaderMap::new();
        h.insert(header::HOST, HeaderValue::from_static("127.0.0.1:37373"));
        assert!(host_is_loopback_or_absent(&h));
        h.insert(
            header::HOST,
            HeaderValue::from_static("rebound.example:37373"),
        );
        assert!(!host_is_loopback_or_absent(&h));
    }

    #[test]
    fn page_never_embeds_secrets_or_cross_origin_calls() {
        assert!(!PAGE_JS.contains("http://"));
        assert!(!PAGE_JS.contains("https://"));
        assert!(!PAGE_HTML.contains("https://"));
        assert!(PAGE_JS.contains("'/api/v1/hosted-bridge/pair'"));
        assert!(PAGE_JS.contains("'/decision'"));
        assert!(PAGE_HTML.contains("src=\"/local/pairing.js\""));
    }
}
