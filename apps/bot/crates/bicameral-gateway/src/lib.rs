//! Local HTTP gateway for Bicameral ingest, review, and mod execution.
//!
//! The gateway accepts protocol-shaped inputs via HTTP (file-first ingest,
//! future webhook integration, local dashboard). It does not claim canonical
//! authority — all inputs flow through governance before materialization.
//!
//! Derived from ZeroClaw's gateway layer, narrowed to Bicameral's governance domain.
//! See NOTICE and runtime/UPSTREAM-ZEROCLAW.md for attribution.

pub mod browser_data_plane;
pub mod canary_patterns;
pub mod durable_pairing_store;
pub mod evidence_repository;
pub mod github_device_flow;
pub mod google_app_config;
pub mod google_client_credential;
pub mod google_oauth;
pub mod google_resource_selection;
pub mod hosted_bridge_cors;
pub mod identity_enroll_client;
pub mod integration_trace;
pub mod journey_review;
pub mod live_projection_work;
pub mod local_approval;
pub mod local_status;
pub mod pairing_authorization_mint;
pub mod projection_work_repository;
pub mod proof_signing;
pub mod routes;
pub mod sensitive_patterns;
pub mod state;

pub use browser_data_plane::{
    is_browser_data_plane_path, BROWSER_DATA_PLANE_CORS_PROBES, BROWSER_DATA_PLANE_PREFIXES,
};
pub use state::AppState;

use axum::extract::DefaultBodyLimit;
use axum::Router;
use bicameral_config::GatewayConfig;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::sync::watch;
use tower_http::timeout::TimeoutLayer;

/// Returns true if the host address is a loopback address.
pub fn is_loopback_host(host: &str) -> bool {
    host == "127.0.0.1" || host == "::1" || host == "localhost"
}

/// Start the gateway HTTP server.
///
/// Returns when a shutdown signal is received via the `shutdown_rx` channel.
pub async fn serve(
    config: &GatewayConfig,
    state: AppState,
    mut shutdown_rx: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    if !is_loopback_host(&config.host) {
        tracing::warn!(
            host = %config.host,
            "Gateway binding to non-loopback address. \
             v0.1 has no authentication — exposing the gateway outside \
             localhost allows unauthenticated mutation of candidate state. \
             Use a loopback address (127.0.0.1) unless you understand the risk."
        );
    }

    // Surface opt-in extra hosted-bridge origins (e.g. BICAMERAL_DEV_TRUST_VITE)
    // once at boot so local smoke operators know pairing will accept Vite.
    let extras = bicameral_api::hosted_bridge::extra_trusted_origins_from_env();
    if !extras.is_empty() {
        tracing::warn!(
            origins = ?extras,
            "Hosted bridge custom trusted origins active (dev/local opt-in; not production defaults)"
        );
    }

    let app = build_router(state);

    let addr: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;
    tracing::info!(%addr, "Gateway listening");

    let listener = tokio::net::TcpListener::bind(addr).await?;

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = shutdown_rx.wait_for(|v| *v).await;
            tracing::info!("Gateway shutting down");
        })
        .await?;

    Ok(())
}

/// Maximum request body size, applied uniformly to every route (1 MiB).
///
/// Ingest (evidence payloads) is the heaviest legitimate route and was already
/// bounded at 1 MiB; all other routes carry small command/query JSON. A single
/// global cap means no route silently inherits axum's larger implicit default,
/// minimizing attack surface. Oversized bodies return `413 Payload Too Large`.
const REQUEST_BODY_LIMIT: usize = 1024 * 1024;

/// Global per-request timeout. Bounds handler time so a slow or stalled client
/// (e.g. slow-loris) cannot hold a connection indefinitely; elapsed requests
/// return `408 Request Timeout`. Generous enough for the slowest legitimate
/// operations (graph rebuild, history/query scans).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

fn build_router(state: AppState) -> Router {
    // Body-size and timeout limits are applied globally below; rate limiting +
    // canary/sensitive gates are enforced inside the ingest handlers via the
    // governor token bucket on AppState.
    //
    // Browser data plane (CORS+PNA for trusted origins) is mounted as one
    // unit — see `browser_data_plane`. Do not merge SPA-facing routes outside
    // that helper without updating the inventory and tests.
    Router::new()
        .merge(routes::health_routes())
        // Local-only non-secret machine/runtime accountability projection.
        // It is intentionally outside the browser data plane in this slice.
        .merge(local_status::local_status_routes())
        .merge(routes::v2_routes())
        .merge(routes::ingest_routes())
        .merge(routes::external_ingest_routes())
        .merge(journey_review::journey_review_routes())
        .merge(integration_trace::integration_trace_routes())
        .merge(routes::review_routes())
        .merge(routes::status_routes())
        .merge(routes::dashboard_api_routes())
        .merge(routes::projection_api_routes())
        .merge(routes::query_routes())
        .merge(routes::bind_routes())
        .merge(routes::evidence_refresh_routes())
        .merge(routes::runtime_graph_routes())
        .merge(browser_data_plane::browser_data_plane_routes(state.clone()))
        // Loopback-only pairing approval page (no CORS; daemon authority).
        .merge(local_approval::local_approval_routes())
        // invite_routes() removed: InviteBundle join is deprecated in favor of
        // provider-based project discovery. See issue #344.
        .with_state(state)
        // Bound every API request with a global body-size limit and timeout.
        .layer(DefaultBodyLimit::max(REQUEST_BODY_LIMIT))
        .layer(TimeoutLayer::new(REQUEST_TIMEOUT))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use bicameral_audit::store::AuditStore;
    use tower::ServiceExt;

    fn test_state() -> AppState {
        let dir = tempfile::tempdir().unwrap();
        let audit = AuditStore::open(dir.path()).unwrap();
        AppState::new(audit)
    }

    #[test]
    fn default_gateway_config_is_loopback() {
        let config = GatewayConfig::default();
        assert!(
            is_loopback_host(&config.host),
            "Default gateway host {} is not loopback",
            config.host
        );
    }

    #[test]
    fn is_loopback_host_rejects_external() {
        assert!(!is_loopback_host("0.0.0.0"));
        assert!(!is_loopback_host("192.168.1.1"));
        assert!(!is_loopback_host("10.0.0.1"));
    }

    #[test]
    fn is_loopback_host_accepts_loopback() {
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("::1"));
        assert!(is_loopback_host("localhost"));
    }

    #[tokio::test]
    async fn health_endpoint_returns_ok() {
        let app = build_router(test_state());
        let req = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn local_status_endpoint_returns_ok() {
        let app = build_router(test_state());
        let req = Request::builder()
            .uri("/api/v1/local/status")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["schema"], local_status::LOCAL_STATUS_SCHEMA);
        assert_eq!(value["gateway_reachable"], true);
    }

    #[tokio::test]
    async fn ingest_returns_created_with_candidate() {
        let app = build_router(test_state());
        let body = serde_json::json!({
            "title": "Test decision",
            "description": "A test candidate",
            "level": "architecture",
            "source": "test",
            "evidence": [{"span_start": 0, "span_end": 10, "excerpt": "test excerpt"}]
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/ingest")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn dashboard_ingestion_gate_returns_empty() {
        let app = build_router(test_state());
        let req = Request::builder()
            .uri("/api/v1/dashboard/ingestion-gate")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let items: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(items.is_empty(), "v0.1 ingestion gate should be empty");
    }

    #[tokio::test]
    async fn dashboard_ledger_returns_empty() {
        let app = build_router(test_state());
        let req = Request::builder()
            .uri("/api/v1/dashboard/ledger")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let items: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(items.is_empty(), "v0.1 ledger should be empty");
    }

    #[tokio::test]
    async fn dashboard_command_rejects_unknown_target() {
        let state = test_state();
        let app = build_router(state);
        let body = serde_json::json!({
            "target_id": uuid::Uuid::new_v4().to_string(),
            "command": "reject_candidate"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/dashboard/command")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        // Now validates: unknown target returns 404, not blind 202
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn dashboard_command_reject_candidate_succeeds() {
        let state = test_state();

        // Ingest a candidate first
        {
            let app = build_router(state.clone());
            let body = serde_json::json!({
                "title": "Reject me",
                "description": "A candidate to reject",
                "level": "implementation",
                "source": "test",
                "evidence": [{"span_start": 0, "span_end": 5, "excerpt": "proof"}]
            });
            let req = Request::builder()
                .method("POST")
                .uri("/api/v1/ingest")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap();
            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::CREATED);
        }

        // Get the candidate ID
        let candidate_id = {
            let c = state.candidates.read().await;
            *c.keys().next().unwrap()
        };

        // Reject via dashboard command
        let app = build_router(state.clone());
        let body = serde_json::json!({
            "target_id": candidate_id.to_string(),
            "command": "reject_candidate"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/dashboard/command")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn oversized_request_body_is_rejected() {
        let app = build_router(test_state());
        // 1 MiB + 1 byte body; route doesn't matter — body-limit layer
        // rejects before handler deserialization.
        let huge = vec![b'x'; REQUEST_BODY_LIMIT + 1];
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/ingest")
            .header("content-type", "application/json")
            .body(Body::from(huge))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}
