//! CORS and Private Network Access (PNA) preflight for the **browser data
//! plane** (hosted SPA → local daemon).
//!
//! Applied via `browser_data_plane::browser_data_plane_routes` to every path
//! prefix in `BROWSER_DATA_PLANE_PREFIXES` (hosted-bridge, setup, connections).
//! Not a free-for-all on the whole gateway: authority routes stay CORS-free.
//!
//! Validates `Origin` against `DEFAULT_TRUSTED_ORIGINS` plus operator/env
//! custom origins on `AppState`.
//!
//! # Private Network Access (PNA)
//!
//! Browsers making requests from a public HTTPS page to a loopback/private
//! network address send an extra preflight header:
//!
//!   `Access-Control-Request-Private-Network: true`
//!
//! The server must respond with:
//!
//!   `Access-Control-Allow-Private-Network: true`
//!
//! for the actual request to proceed. This module adds that header when the
//! PNA request header is present and the origin is trusted.
//!
//! # Scope
//!
//! - Browser data plane only (see `browser_data_plane`).
//! - Ingest, ToolRequest, raw local dashboard, etc. must not use this layer.
//! - Untrusted origins receive no CORS headers (browser enforces same-origin).

use crate::state::AppState;
use axum::body::Body;
use axum::extract::State;
use axum::http::header::{
    ACCESS_CONTROL_ALLOW_CREDENTIALS, ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS,
    ACCESS_CONTROL_ALLOW_ORIGIN, ACCESS_CONTROL_MAX_AGE, ORIGIN, VARY,
};
use axum::http::{HeaderMap, HeaderValue, Method, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use bicameral_api::hosted_bridge::{validate_origin, OriginValidationOutcome};

/// Header name for the PNA preflight request signal.
const ACCESS_CONTROL_REQUEST_PRIVATE_NETWORK: &str = "access-control-request-private-network";

/// Header name for the PNA preflight response signal.
const ACCESS_CONTROL_ALLOW_PRIVATE_NETWORK: &str = "access-control-allow-private-network";

/// Allowed methods for hosted bridge CORS preflight.
const ALLOWED_METHODS: &str = "GET, POST, OPTIONS";

/// Allowed headers for hosted bridge CORS preflight.
const ALLOWED_HEADERS: &str = "content-type, x-bridge-session-token, x-csrf-token";

/// Max preflight cache age in seconds (1 hour).
const PREFLIGHT_MAX_AGE: &str = "3600";

/// Axum middleware that adds CORS and PNA headers to hosted bridge responses.
///
/// For OPTIONS preflight requests with a trusted origin, returns an empty 204
/// response with CORS/PNA headers. For non-preflight requests with a trusted
/// origin, passes through to the inner handler and appends CORS headers to the
/// response. Untrusted or missing origins pass through with no CORS headers
/// (the browser will block the cross-origin request).
pub async fn hosted_bridge_cors_middleware(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let origin = request
        .headers()
        .get(ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let is_options = request.method() == Method::OPTIONS;
    let has_pna_request = request
        .headers()
        .get(ACCESS_CONTROL_REQUEST_PRIVATE_NETWORK)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    // If no Origin header, pass through without CORS headers.
    let origin_str = match origin {
        Some(ref o) if !o.is_empty() => o.as_str(),
        _ => {
            if is_options {
                // OPTIONS without Origin: return 204 (benign, no CORS headers).
                return StatusCode::NO_CONTENT.into_response();
            }
            return next.run(request).await;
        }
    };

    // Validate origin against trusted allowlist.
    let custom_origins = state.custom_trusted_origins.read().await;
    let origin_outcome = validate_origin(origin_str, &custom_origins);
    drop(custom_origins);

    match origin_outcome {
        OriginValidationOutcome::Trusted => {
            if is_options {
                // Return preflight response with CORS + PNA headers.
                let mut headers = cors_headers(origin_str);
                if has_pna_request {
                    headers.insert(
                        ACCESS_CONTROL_ALLOW_PRIVATE_NETWORK,
                        HeaderValue::from_static("true"),
                    );
                }
                return (StatusCode::NO_CONTENT, headers).into_response();
            }

            // Non-preflight: run inner handler, then append CORS headers.
            let mut response = next.run(request).await;
            let headers = response.headers_mut();
            append_cors_headers(headers, origin_str);
            if has_pna_request {
                headers.insert(
                    ACCESS_CONTROL_ALLOW_PRIVATE_NETWORK,
                    HeaderValue::from_static("true"),
                );
            }
            response
        }
        OriginValidationOutcome::Rejected { .. } => {
            if is_options {
                // Untrusted origin preflight: return 204 with no CORS headers.
                // Browser will block the actual request.
                return StatusCode::NO_CONTENT.into_response();
            }
            // Untrusted origin non-preflight: pass through without CORS
            // headers. The existing handler may reject based on origin in the
            // request body (for pairing) or session validation (for other
            // routes). The browser will block the response due to missing
            // CORS headers.
            next.run(request).await
        }
    }
}

/// Build a full set of CORS preflight response headers for a trusted origin.
fn cors_headers(origin: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Ok(val) = HeaderValue::from_str(origin) {
        headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, val);
    }
    headers.insert(
        ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static(ALLOWED_METHODS),
    );
    headers.insert(
        ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static(ALLOWED_HEADERS),
    );
    headers.insert(
        ACCESS_CONTROL_ALLOW_CREDENTIALS,
        HeaderValue::from_static("true"),
    );
    headers.insert(
        ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static(PREFLIGHT_MAX_AGE),
    );
    headers.insert(VARY, HeaderValue::from_static("Origin"));
    headers
}

/// Append CORS headers to an existing response header map.
fn append_cors_headers(headers: &mut HeaderMap, origin: &str) {
    if let Ok(val) = HeaderValue::from_str(origin) {
        headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, val);
    }
    headers.insert(
        ACCESS_CONTROL_ALLOW_CREDENTIALS,
        HeaderValue::from_static("true"),
    );
    headers.insert(VARY, HeaderValue::from_static("Origin"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::{get, post};
    use axum::Router;
    use bicameral_audit::store::AuditStore;
    use tower::ServiceExt;

    fn test_state() -> AppState {
        let dir = tempfile::tempdir().unwrap();
        let audit = AuditStore::open(dir.path()).unwrap();
        AppState::new(audit)
    }

    fn cors_test_app(state: AppState) -> Router {
        use crate::routes::hosted_bridge_routes;
        Router::new()
            .merge(hosted_bridge_routes())
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                hosted_bridge_cors_middleware,
            ))
            .with_state(state)
    }

    // ── AC1: OPTIONS /api/v1/hosted-bridge/pair from trusted origin ──

    #[tokio::test]
    async fn options_pair_trusted_origin_returns_cors_headers() {
        let app = cors_test_app(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/api/v1/hosted-bridge/pair")
                    .header("origin", "https://app.bicameral-ai.com")
                    .header("access-control-request-method", "POST")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let h = resp.headers();
        assert_eq!(
            h.get("access-control-allow-origin").unwrap(),
            "https://app.bicameral-ai.com"
        );
        assert!(h
            .get("access-control-allow-methods")
            .unwrap()
            .to_str()
            .unwrap()
            .contains("POST"));
        assert!(h
            .get("access-control-allow-headers")
            .unwrap()
            .to_str()
            .unwrap()
            .contains("content-type"));
        assert!(h
            .get("access-control-allow-headers")
            .unwrap()
            .to_str()
            .unwrap()
            .contains("x-bridge-session-token"));
        assert_eq!(h.get("access-control-allow-credentials").unwrap(), "true");
        assert!(h.get("access-control-max-age").is_some());
        assert_eq!(h.get("vary").unwrap(), "Origin");
    }

    #[tokio::test]
    async fn options_pair_all_default_trusted_origins_accepted() {
        for origin in bicameral_api::hosted_bridge::DEFAULT_TRUSTED_ORIGINS {
            let app = cors_test_app(test_state());
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("OPTIONS")
                        .uri("/api/v1/hosted-bridge/pair")
                        .header("origin", *origin)
                        .header("access-control-request-method", "POST")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::NO_CONTENT);
            assert_eq!(
                resp.headers()
                    .get("access-control-allow-origin")
                    .unwrap()
                    .to_str()
                    .unwrap(),
                *origin,
                "CORS should reflect trusted origin: {origin}"
            );
        }
    }

    // ── AC2: PNA preflight headers present for trusted origin ──

    #[tokio::test]
    async fn options_with_pna_request_returns_pna_allow() {
        let app = cors_test_app(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/api/v1/hosted-bridge/pair")
                    .header("origin", "https://app.bicameral-ai.com")
                    .header("access-control-request-method", "POST")
                    .header("access-control-request-private-network", "true")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            resp.headers()
                .get("access-control-allow-private-network")
                .unwrap(),
            "true",
            "PNA allow header must be present for trusted origin preflight"
        );
        assert_eq!(
            resp.headers().get("access-control-allow-origin").unwrap(),
            "https://app.bicameral-ai.com"
        );
    }

    #[tokio::test]
    async fn options_without_pna_request_no_pna_allow() {
        let app = cors_test_app(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/api/v1/hosted-bridge/pair")
                    .header("origin", "https://app.bicameral-ai.com")
                    .header("access-control-request-method", "POST")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert!(
            resp.headers()
                .get("access-control-allow-private-network")
                .is_none(),
            "PNA header should not be present when not requested"
        );
    }

    // ── AC3: Untrusted origins do not receive CORS headers ──

    #[tokio::test]
    async fn options_untrusted_origin_no_cors_headers() {
        let app = cors_test_app(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/api/v1/hosted-bridge/pair")
                    .header("origin", "https://evil.example.com")
                    .header("access-control-request-method", "POST")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // Returns 204 but without CORS headers — browser blocks.
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert!(
            resp.headers().get("access-control-allow-origin").is_none(),
            "Untrusted origin must not get access-control-allow-origin"
        );
        assert!(
            resp.headers()
                .get("access-control-allow-private-network")
                .is_none(),
            "Untrusted origin must not get PNA allow header"
        );
    }

    #[tokio::test]
    async fn options_untrusted_origin_with_pna_no_pna_allow() {
        let app = cors_test_app(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/api/v1/hosted-bridge/pair")
                    .header("origin", "https://evil.example.com")
                    .header("access-control-request-method", "POST")
                    .header("access-control-request-private-network", "true")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert!(
            resp.headers()
                .get("access-control-allow-private-network")
                .is_none(),
            "Untrusted origin must not get PNA allow header even when requested"
        );
    }

    // ── AC4: Non-preflight trusted origin gets CORS on actual response ──

    #[tokio::test]
    async fn post_pair_trusted_origin_response_has_cors_headers() {
        let app = cors_test_app(test_state());
        let body = serde_json::json!({
            "origin": "https://app.bicameral-ai.com",
            "cloud_account_id": "acct-123",
            "csrf_token": "csrf-abc",
            "team_id": "team-1",
            "cloud_access_token": "cloud-token-1",
            "client_protocol_version":
                bicameral_api::hosted_bridge::HOSTED_BRIDGE_PROTOCOL_VERSION,
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/hosted-bridge/pair")
                    .header("content-type", "application/json")
                    .header("origin", "https://app.bicameral-ai.com")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // #790: valid pair now returns a 202 pending request.
        assert_eq!(resp.status(), StatusCode::ACCEPTED);
        assert_eq!(
            resp.headers().get("access-control-allow-origin").unwrap(),
            "https://app.bicameral-ai.com"
        );
        assert_eq!(
            resp.headers()
                .get("access-control-allow-credentials")
                .unwrap(),
            "true"
        );
    }

    #[tokio::test]
    async fn post_pair_untrusted_origin_response_no_cors_headers() {
        let app = cors_test_app(test_state());
        let body = serde_json::json!({
            "origin": "https://evil.example.com",
            "cloud_account_id": "acct-123",
            "csrf_token": "csrf-abc",
            "team_id": "team-1",
            "cloud_access_token": "cloud-token-1",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/hosted-bridge/pair")
                    .header("content-type", "application/json")
                    .header("origin", "https://evil.example.com")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Handler rejects at origin validation, but no CORS headers either way.
        assert!(
            resp.headers().get("access-control-allow-origin").is_none(),
            "Untrusted origin actual request must not get CORS headers"
        );
    }

    // ── AC5: Custom trusted origins work with CORS ──

    #[tokio::test]
    async fn options_custom_trusted_origin_returns_cors_headers() {
        let state = test_state();
        {
            let mut custom = state.custom_trusted_origins.write().await;
            custom.push("https://custom.corp.example.com".to_string());
        }
        let app = cors_test_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/api/v1/hosted-bridge/pair")
                    .header("origin", "https://custom.corp.example.com")
                    .header("access-control-request-method", "POST")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            resp.headers().get("access-control-allow-origin").unwrap(),
            "https://custom.corp.example.com"
        );
    }

    // ── AC6: CORS works on non-pair bridge endpoints ──

    #[tokio::test]
    async fn options_status_endpoint_trusted_origin() {
        let app = cors_test_app(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/api/v1/hosted-bridge/status")
                    .header("origin", "https://app.bicameral-ai.com")
                    .header("access-control-request-method", "GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            resp.headers().get("access-control-allow-origin").unwrap(),
            "https://app.bicameral-ai.com"
        );
    }

    #[tokio::test]
    async fn options_operator_home_endpoint_trusted_origin() {
        let app = cors_test_app(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/api/v1/hosted-bridge/operator-home")
                    .header("origin", "https://app.bicameral-ai.com")
                    .header("access-control-request-method", "GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            resp.headers().get("access-control-allow-origin").unwrap(),
            "https://app.bicameral-ai.com"
        );
    }

    // ── AC7: No origin header → no CORS headers, pass through ──

    #[tokio::test]
    async fn no_origin_header_no_cors_headers() {
        let app = cors_test_app(test_state());
        let body = serde_json::json!({
            "origin": "https://app.bicameral-ai.com",
            "cloud_account_id": "acct-123",
            "csrf_token": "csrf-abc",
            "team_id": "team-1",
            "cloud_access_token": "cloud-token-1",
            "client_protocol_version":
                bicameral_api::hosted_bridge::HOSTED_BRIDGE_PROTOCOL_VERSION,
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/hosted-bridge/pair")
                    .header("content-type", "application/json")
                    // No Origin header.
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Should succeed (non-browser client) without CORS headers.
        assert_eq!(resp.status(), StatusCode::ACCEPTED);
        assert!(
            resp.headers().get("access-control-allow-origin").is_none(),
            "No Origin header → no CORS headers"
        );
    }
}
