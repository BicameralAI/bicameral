//! Cloud identity enrollment HTTP client (factory #901 row 12 / cloud#305).

use bicameral_api::PairingAuthorizationV1;
use serde::Serialize;

/// Request body for `POST /api/v1/teams/{team_id}/pairings/{pairing_id}/identity`.
#[derive(Debug, Clone, Serialize)]
pub struct EnrollIdentityBody {
    pub identity_public_key_b64: String,
    pub identity_key_id: String,
    pub pairing_authorization: PairingAuthorizationV1,
}

/// Outcome of a Cloud enroll attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnrollOutcome {
    Enrolled {
        status: u16,
    },
    Failed {
        status: Option<u16>,
        message: String,
    },
}

impl EnrollOutcome {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Enrolled { .. })
    }

    pub fn status_label(&self) -> String {
        match self {
            Self::Enrolled { status } => format!("enrolled:{status}"),
            Self::Failed { status, message } => match status {
                Some(code) => format!("failed:{code}:{message}"),
                None => format!("failed:{message}"),
            },
        }
    }
}

/// POST enroll with required PairingAuthorizationV1 (#305 shape).
pub async fn enroll_daemon_identity(
    http: &reqwest::Client,
    cloud_origin: &str,
    team_id: &str,
    pairing_id: &str,
    bearer_token: &str,
    body: &EnrollIdentityBody,
) -> EnrollOutcome {
    let origin = cloud_origin.trim_end_matches('/');
    let url = format!("{origin}/api/v1/teams/{team_id}/pairings/{pairing_id}/identity");
    let response = match http
        .post(&url)
        .header("Authorization", format!("Bearer {bearer_token}"))
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(err) => {
            return EnrollOutcome::Failed {
                status: None,
                message: format!("transport error: {err}"),
            }
        }
    };
    let status = response.status().as_u16();
    if response.status().is_success() {
        return EnrollOutcome::Enrolled { status };
    }
    let detail = response
        .text()
        .await
        .unwrap_or_else(|_| "unenrollable response body".into());
    EnrollOutcome::Failed {
        status: Some(status),
        message: truncate_safe(&detail, 240),
    }
}

/// Best-effort: create a Cloud DaemonPairing carrying the daemon-issued id.
///
/// Forward-compatible: sends `pairing_id` when Cloud accepts it. Failures are
/// returned so the caller can fail closed rather than silently claim enrolled.
pub async fn ensure_cloud_pairing(
    http: &reqwest::Client,
    cloud_origin: &str,
    team_id: &str,
    pairing_id: &str,
    daemon_instance_id: &str,
    bearer_token: &str,
) -> EnrollOutcome {
    let origin = cloud_origin.trim_end_matches('/');
    let url = format!("{origin}/api/v1/teams/{team_id}/pairings");
    let body = serde_json::json!({
        "daemon_id": daemon_instance_id,
        "pairing_id": pairing_id,
        "daemon_version": env!("CARGO_PKG_VERSION"),
    });
    let response = match http
        .post(&url)
        .header("Authorization", format!("Bearer {bearer_token}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(err) => {
            return EnrollOutcome::Failed {
                status: None,
                message: format!("pairing create transport error: {err}"),
            }
        }
    };
    let status = response.status().as_u16();
    // 201 created or 409 already-exists are acceptable for ensure.
    if response.status().is_success() || status == 409 {
        return EnrollOutcome::Enrolled { status };
    }
    let detail = response
        .text()
        .await
        .unwrap_or_else(|_| "pairing create failed".into());
    EnrollOutcome::Failed {
        status: Some(status),
        message: truncate_safe(&detail, 240),
    }
}

fn truncate_safe(value: &str, max: usize) -> String {
    let mut out = value.chars().take(max).collect::<String>();
    if value.chars().count() > max {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pairing_authorization_mint::{
        mint_pairing_authorization_v1, MintPairingAuthorizationRequest,
    };
    use crate::proof_signing::KeyringProofSigner;
    use bicameral_config::InMemorySecretsStore;
    use chrono::TimeZone;
    use std::sync::Arc;

    #[tokio::test]
    async fn enroll_client_sends_authorization_object() {
        let received = Arc::new(tokio::sync::Mutex::new(None::<serde_json::Value>));
        let received_clone = received.clone();

        let server = tokio::spawn(async move {
            use axum::extract::State;
            use axum::routing::post;
            use axum::{Json, Router};
            use std::net::SocketAddr;

            async fn enroll(
                State(slot): State<Arc<tokio::sync::Mutex<Option<serde_json::Value>>>>,
                Json(body): Json<serde_json::Value>,
            ) -> (axum::http::StatusCode, Json<serde_json::Value>) {
                *slot.lock().await = Some(body);
                (
                    axum::http::StatusCode::OK,
                    Json(serde_json::json!({"status": "active"})),
                )
            }

            let app = Router::new()
                .route(
                    "/api/v1/teams/:team_id/pairings/:pairing_id/identity",
                    post(enroll),
                )
                .with_state(received_clone);
            let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
                .await
                .unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            addr
        });
        let addr = server.await.unwrap();

        let signer = KeyringProofSigner::new(Arc::new(InMemorySecretsStore::new()));
        let now = chrono::Utc.with_ymd_and_hms(2026, 9, 2, 12, 0, 0).unwrap();
        let auth = mint_pairing_authorization_v1(
            &MintPairingAuthorizationRequest {
                pairing_id: "pairing-xyz",
                protocol_session_id: "protocol-1",
                cloud_account_id: "acct-1",
                approved_origin: "https://app.bicameral-ai.com",
                daemon_instance_id: "daemon-1",
                team_id: "team-1",
                now,
                ttl_secs: Some(300),
            },
            &signer,
        )
        .unwrap();
        let body = EnrollIdentityBody {
            identity_public_key_b64: auth.identity_public_key_b64.clone(),
            identity_key_id: auth.identity_key_id.clone(),
            pairing_authorization: auth,
        };
        let client = reqwest::Client::new();
        let outcome = enroll_daemon_identity(
            &client,
            &format!("http://{addr}"),
            "team-1",
            "pairing-xyz",
            "test-token",
            &body,
        )
        .await;
        assert!(outcome.is_success(), "{outcome:?}");

        let captured = received.lock().await.clone().expect("body captured");
        assert!(
            captured.get("pairing_authorization").is_some(),
            "enroll body must include pairing_authorization: {captured}"
        );
        assert_eq!(
            captured["pairing_authorization"]["profile"],
            "bicameral.pairing_authorization.v1"
        );
        assert_eq!(captured["identity_key_id"], body.identity_key_id);
    }
}
