//! Browser → local daemon **data plane** (hosted SPA).
//!
//! # Rule
//!
//! Anything the Cloud or Vite SPA calls on the loopback daemon from a browser
//! (via `fetchFromHostedDaemon` / `HOSTED_GATEWAY_BASE`) is **browser data
//! plane** and MUST sit behind trusted-origin CORS + Private Network Access.
//!
//! Authority / local-only routes (ingest, ToolRequest, raw dashboard API, …)
//! MUST NOT get CORS — they are not part of this plane.
//!
//! # Drift prevention
//!
//! - Path prefixes here are the allowlist inventory. Gateway OPTIONS tests and
//!   the dashboard `browser-data-plane.ts` inventory must stay in lockstep.
//! - New SPA→daemon endpoints: add a prefix (or nest under an existing one),
//!   mount under `browser_data_plane_routes()`, and extend the probe list.
//! - Prefer nesting under `/api/v1/hosted-bridge/` when adding surfaces so CORS
//!   is automatic.

use crate::hosted_bridge_cors;
use crate::routes;
use crate::state::AppState;
use axum::Router;

/// URL path prefixes the hosted SPA may call on the local daemon.
///
/// Keep in lockstep with `dashboard/src/lib/browser-data-plane.ts`.
pub const BROWSER_DATA_PLANE_PREFIXES: &[&str] = &[
    "/api/v1/hosted-bridge/",
    "/api/v1/setup/",
    "/api/v1/connections",
];

/// Representative paths used in OPTIONS CORS probes (one per prefix family).
pub const BROWSER_DATA_PLANE_CORS_PROBES: &[&str] = &[
    "/api/v1/hosted-bridge/pair",
    "/api/v1/setup/capability-readiness",
    "/api/v1/connections",
];

/// Prefixes that must **never** receive browser CORS (authority / local plane).
pub const AUTHORITY_PLANE_NO_CORS_PROBES: &[&str] =
    &["/health", "/api/v1/ingest", "/api/v1/local/status"];

/// True if `path` is under the browser data plane inventory.
pub fn is_browser_data_plane_path(path: &str) -> bool {
    let path = path.split('?').next().unwrap_or(path);
    BROWSER_DATA_PLANE_PREFIXES.iter().any(|prefix| {
        if prefix.ends_with('/') {
            path.starts_with(prefix) || path == prefix.trim_end_matches('/')
        } else {
            path == *prefix || path.starts_with(&format!("{prefix}/"))
        }
    })
}

/// Apply trusted-origin CORS + PNA to a route set.
pub fn with_hosted_cors(router: Router<AppState>, state: AppState) -> Router<AppState> {
    router.layer(axum::middleware::from_fn_with_state(
        state,
        hosted_bridge_cors::hosted_bridge_cors_middleware,
    ))
}

/// All routes the hosted SPA is allowed to call cross-origin on the daemon.
///
/// **Mount new browser-facing routes here** (or under hosted-bridge inside
/// `hosted_bridge_routes`). Do not `merge` SPA paths outside this function
/// without CORS — CI will fail the inventory probes.
pub fn browser_data_plane_routes(state: AppState) -> Router<AppState> {
    let connection_routes =
        routes::connection_routes().layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::google_client_credential::google_client_credential_bootstrap_middleware,
        ));

    Router::new()
        .merge(routes::hosted_bridge_routes())
        .merge(crate::local_status::browser_local_status_routes())
        .merge(routes::setup_routes())
        .merge(crate::google_resource_selection::resource_selection_routes())
        .merge(connection_routes)
        .layer(axum::middleware::from_fn_with_state(
            state,
            hosted_bridge_cors::hosted_bridge_cors_middleware,
        ))
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
    fn inventory_covers_bridge_setup_and_connections() {
        assert!(is_browser_data_plane_path("/api/v1/hosted-bridge/pair"));
        assert!(is_browser_data_plane_path(
            "/api/v1/hosted-bridge/pair/status?x=1"
        ));
        assert!(is_browser_data_plane_path(
            "/api/v1/hosted-bridge/local-status"
        ));
        assert!(is_browser_data_plane_path(
            "/api/v1/setup/capability-readiness"
        ));
        assert!(is_browser_data_plane_path("/api/v1/connections"));
        assert!(is_browser_data_plane_path(
            "/api/v1/connections/google/start"
        ));
    }

    #[test]
    fn inventory_excludes_authority_plane() {
        assert!(!is_browser_data_plane_path("/health"));
        assert!(!is_browser_data_plane_path("/api/v1/ingest"));
        assert!(!is_browser_data_plane_path("/api/v1/local/status"));
        assert!(!is_browser_data_plane_path("/api/v1/dashboard/ledger"));
    }

    #[test]
    fn probes_are_subset_of_inventory() {
        for probe in BROWSER_DATA_PLANE_CORS_PROBES {
            assert!(
                is_browser_data_plane_path(probe),
                "probe {probe} must match BROWSER_DATA_PLANE_PREFIXES"
            );
        }
    }

    #[tokio::test]
    async fn trusted_origin_can_read_redacted_local_status() {
        let state = test_state();
        let app = browser_data_plane_routes(state.clone()).with_state(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/hosted-bridge/local-status")
                    .header("origin", "https://app.bicameral-ai.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .unwrap(),
            "https://app.bicameral-ai.com"
        );

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["schema"], crate::local_status::LOCAL_STATUS_SCHEMA);
        assert_eq!(value["gateway_reachable"], true);
    }

    #[tokio::test]
    async fn untrusted_origin_cannot_read_local_status_through_cors() {
        let state = test_state();
        let app = browser_data_plane_routes(state.clone()).with_state(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/hosted-bridge/local-status")
                    .header("origin", "https://attacker.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response
            .headers()
            .get("access-control-allow-origin")
            .is_none());
    }
}
