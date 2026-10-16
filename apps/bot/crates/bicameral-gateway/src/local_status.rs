//! Local accountability/readiness projection.
//!
//! The local diagnostic route and its hosted-browser mirror report the same
//! daemon-authored, non-secret facts needed by install qualification and
//! diagnostics. The browser mirror is mounted only through the governed
//! browser data plane, so trusted-origin CORS/PNA is applied without granting
//! Cloud authority over the local machine.
//!
//! Both surfaces deliberately exclude filesystem paths, provider/account
//! identity, credential material, pairing codes/nonces, event payloads, and
//! evidence bodies.

use crate::state::{AppState, PendingPairingDecision};
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use bicameral_config::{BicameralPaths, ConnectionRegistry, ConnectionStatus};
use serde::Serialize;
use std::collections::BTreeMap;

pub const LOCAL_STATUS_SCHEMA: &str = "bicameral.local-status.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalEnvironmentKind {
    Native,
    Wsl2,
}

#[derive(Debug, Serialize)]
pub struct LocalStatusResponse {
    pub schema: &'static str,
    pub version: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
    pub environment: LocalEnvironmentKind,
    pub daemon_instance_id: String,
    pub workspace_configured: bool,
    pub gateway_reachable: bool,
    pub pending_pairings: usize,
    pub active_pairings: usize,
    pub providers: BTreeMap<String, ConnectionStatus>,
}

/// Local diagnostic surface. This route intentionally remains outside CORS.
pub fn local_status_routes() -> Router<AppState> {
    Router::new().route("/api/v1/local/status", get(local_status))
}

/// Browser-safe mirror of the same redacted projection.
///
/// This router MUST be mounted only from `browser_data_plane_routes()` so the
/// existing trusted-origin CORS + Private Network Access boundary applies.
pub fn browser_local_status_routes() -> Router<AppState> {
    Router::new().route("/api/v1/hosted-bridge/local-status", get(local_status))
}

async fn local_status(State(state): State<AppState>) -> Json<LocalStatusResponse> {
    let now = chrono::Utc::now();
    let pending_pairings = state
        .hosted_bridge_pending
        .read()
        .await
        .values()
        .filter(|record| {
            record.decision == PendingPairingDecision::Pending && !record.is_expired(now)
        })
        .count();
    let active_pairings = state
        .hosted_bridge_sessions
        .read()
        .await
        .values()
        .filter(|session| !session.revoked && session.expires_at > now)
        .count();
    let workspace_configured = state.workspace_root.read().await.is_some();

    Json(LocalStatusResponse {
        schema: LOCAL_STATUS_SCHEMA,
        version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        environment: detect_environment(),
        daemon_instance_id: state.daemon_instance_id.clone(),
        workspace_configured,
        gateway_reachable: true,
        pending_pairings,
        active_pairings,
        providers: load_provider_statuses(),
    })
}

fn detect_environment() -> LocalEnvironmentKind {
    let os_release = if std::env::consts::OS == "linux" {
        std::fs::read_to_string("/proc/sys/kernel/osrelease").ok()
    } else {
        None
    };
    classify_environment(std::env::consts::OS, os_release.as_deref())
}

fn classify_environment(os: &str, os_release: Option<&str>) -> LocalEnvironmentKind {
    if os == "linux"
        && os_release
            .map(|value| value.to_ascii_lowercase().contains("microsoft"))
            .unwrap_or(false)
    {
        LocalEnvironmentKind::Wsl2
    } else {
        LocalEnvironmentKind::Native
    }
}

fn load_provider_statuses() -> BTreeMap<String, ConnectionStatus> {
    let registry = BicameralPaths::connections_file()
        .and_then(|path| ConnectionRegistry::load_from_file(&path).ok())
        .unwrap_or_default();
    provider_statuses(&registry)
}

fn provider_statuses(registry: &ConnectionRegistry) -> BTreeMap<String, ConnectionStatus> {
    registry
        .connections
        .iter()
        .map(|(provider_id, metadata)| (provider_id.clone(), metadata.status.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_audit::store::AuditStore;
    use bicameral_config::{AuthMethod, ConnectionMetadata};

    fn test_state() -> AppState {
        let dir = tempfile::tempdir().unwrap();
        let audit = AuditStore::open(dir.path()).unwrap();
        AppState::new(audit)
    }

    #[test]
    fn classifies_wsl2_from_kernel_release() {
        assert_eq!(
            classify_environment("linux", Some("6.6.87.2-microsoft-standard-WSL2")),
            LocalEnvironmentKind::Wsl2
        );
        assert_eq!(
            classify_environment("linux", Some("6.8.0-85-generic")),
            LocalEnvironmentKind::Native
        );
        assert_eq!(
            classify_environment("windows", Some("microsoft")),
            LocalEnvironmentKind::Native
        );
    }

    #[test]
    fn provider_projection_exposes_status_only() {
        let mut registry = ConnectionRegistry::default();
        registry.upsert(
            "google",
            ConnectionMetadata {
                status: ConnectionStatus::Connected,
                account_label: Some("secret-ish@example.com".into()),
                capabilities: vec!["drive.read".into()],
                updated_at: Some(chrono::Utc::now()),
                auth_method: Some(AuthMethod::Oauth),
            },
        );

        let projected = provider_statuses(&registry);
        assert_eq!(projected.get("google"), Some(&ConnectionStatus::Connected));
        let json = serde_json::to_string(&projected).unwrap();
        assert!(!json.contains("secret-ish@example.com"));
        assert!(!json.contains("drive.read"));
        assert!(!json.contains("oauth"));
    }

    #[tokio::test]
    async fn default_status_is_non_secret_and_reachable() {
        let response = local_status(State(test_state())).await.0;
        assert_eq!(response.schema, LOCAL_STATUS_SCHEMA);
        assert_eq!(response.version, env!("CARGO_PKG_VERSION"));
        assert!(response.gateway_reachable);
        assert_eq!(response.pending_pairings, 0);
        assert_eq!(response.active_pairings, 0);

        let json = serde_json::to_string(&response).unwrap();
        for forbidden in [
            "workspace_path",
            "access_token",
            "refresh_token",
            "client_secret",
            "pairing_code",
            "nonce",
            "bearer",
            "credential",
        ] {
            assert!(
                !json.contains(forbidden),
                "leaked forbidden field: {forbidden}"
            );
        }
    }
}
