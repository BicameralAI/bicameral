//! Operator-home persistence and Cloud bootstrap for `product.join` trust
//! anchors (bot#1012 / v0.1.16).
//!
//! The operator-scoped daemon historically started with
//! `BicameralConfig::default()` (`product_join: None`), so same-domain join
//! always failed closed with `unsupported_capability` even when Cloud minting
//! worked. This module:
//!
//! 1. Loads `~/.bicameral/product-join-trust.json` when present;
//! 2. Otherwise fetches Cloud `GET /api/v1/product-join/verification-keys`
//!    from `BICAMERAL_CLOUD_ORIGIN` (default `https://app.bicameral-ai.com`),
//!    persists the public trust, and returns it.
//!
//! Private signing material never enters this store — only issuer + public
//! verifying keys. Audience defaults to product-scoped (`daemon:{product_id}`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use bicameral_config::{BicameralPaths, ProductJoinTrustConfig, ProductJoinVerifyingKeyConfig};
use serde::Deserialize;

const DEFAULT_CLOUD_ORIGIN: &str = "https://app.bicameral-ai.com";
const BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(5);

/// Cloud verification metadata (public half only).
#[derive(Debug, Deserialize)]
struct CloudVerificationMetadata {
    issuer: String,
    #[serde(default)]
    keys: Vec<CloudVerificationKey>,
}

#[derive(Debug, Deserialize)]
struct CloudVerificationKey {
    key_id: String,
    public_key_b64: String,
    #[serde(default)]
    state: String,
}

/// Resolve trust for the operator-scoped daemon: disk first, then Cloud bootstrap.
pub async fn resolve_operator_product_join_config() -> Option<ProductJoinTrustConfig> {
    if let Some(config) = load_from_operator_home() {
        tracing::info!(
            issuer = %config.issuer,
            keys = config.verifying_keys.len(),
            "product.join trust loaded from operator home"
        );
        return Some(config);
    }

    let origin = std::env::var("BICAMERAL_CLOUD_ORIGIN")
        .unwrap_or_else(|_| DEFAULT_CLOUD_ORIGIN.to_string());
    match bootstrap_from_cloud(&origin).await {
        Ok(config) => {
            if let Err(err) = save_to_operator_home(&config) {
                tracing::warn!(
                    error = %err,
                    "product.join trust bootstrapped from Cloud but failed to persist"
                );
            } else {
                tracing::info!(
                    issuer = %config.issuer,
                    keys = config.verifying_keys.len(),
                    origin = %origin,
                    "product.join trust bootstrapped from Cloud and persisted"
                );
            }
            Some(config)
        }
        Err(err) => {
            tracing::warn!(
                error = %err,
                origin = %origin,
                "product.join trust unavailable (no operator-home file and Cloud bootstrap failed)"
            );
            None
        }
    }
}

/// Load `~/.bicameral/product-join-trust.json` if it exists and is usable.
pub fn load_from_operator_home() -> Option<ProductJoinTrustConfig> {
    let path = BicameralPaths::product_join_trust_file()?;
    load_from_path(&path)
}

fn load_from_path(path: &Path) -> Option<ProductJoinTrustConfig> {
    let raw = std::fs::read_to_string(path).ok()?;
    let config: ProductJoinTrustConfig = serde_json::from_str(&raw).ok()?;
    if config.issuer.trim().is_empty() || config.verifying_keys.is_empty() {
        return None;
    }
    Some(config)
}

/// Persist trust to operator home (creates parent dir).
pub fn save_to_operator_home(config: &ProductJoinTrustConfig) -> std::io::Result<PathBuf> {
    let path = BicameralPaths::product_join_trust_file().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "operator home unavailable")
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_string_pretty(config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&path, body)?;
    Ok(path)
}

/// Fetch Cloud verification metadata and map to daemon trust config.
pub async fn bootstrap_from_cloud(origin: &str) -> Result<ProductJoinTrustConfig, String> {
    let origin = origin.trim_end_matches('/');
    if origin.is_empty() {
        return Err("empty Cloud origin".into());
    }
    let url = format!("{origin}/api/v1/product-join/verification-keys");
    let client = reqwest::Client::builder()
        .timeout(BOOTSTRAP_TIMEOUT)
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("fetch verification-keys: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "verification-keys HTTP {}",
            response.status().as_u16()
        ));
    }
    let meta: CloudVerificationMetadata = response
        .json()
        .await
        .map_err(|e| format!("parse verification-keys: {e}"))?;
    config_from_cloud_metadata(meta)
}

fn config_from_cloud_metadata(
    meta: CloudVerificationMetadata,
) -> Result<ProductJoinTrustConfig, String> {
    if meta.issuer.trim().is_empty() {
        return Err("Cloud verification metadata missing issuer".into());
    }
    let verifying_keys: Vec<ProductJoinVerifyingKeyConfig> = meta
        .keys
        .into_iter()
        .filter(|k| {
            let state = k.state.trim();
            state.is_empty() || state == "active" || state == "overlap"
        })
        .filter(|k| !k.key_id.trim().is_empty() && !k.public_key_b64.trim().is_empty())
        .map(|k| ProductJoinVerifyingKeyConfig {
            key_id: k.key_id,
            public_key_b64: k.public_key_b64,
        })
        .collect();
    if verifying_keys.is_empty() {
        return Err("Cloud verification metadata has no usable keys".into());
    }
    Ok(ProductJoinTrustConfig {
        issuer: meta.issuer,
        audience: "product_scoped".to_string(),
        verifying_keys,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn maps_cloud_metadata_to_product_scoped_trust() {
        let meta = CloudVerificationMetadata {
            issuer: "https://app.bicameral-ai.com".into(),
            keys: vec![
                CloudVerificationKey {
                    key_id: "prod-2026-08".into(),
                    public_key_b64: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
                    state: "active".into(),
                },
                CloudVerificationKey {
                    key_id: "retired".into(),
                    public_key_b64: "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB=".into(),
                    state: "retired".into(),
                },
            ],
        };
        let config = config_from_cloud_metadata(meta).expect("usable");
        assert_eq!(config.issuer, "https://app.bicameral-ai.com");
        assert_eq!(config.audience, "product_scoped");
        assert_eq!(config.verifying_keys.len(), 1);
        assert_eq!(config.verifying_keys[0].key_id, "prod-2026-08");
    }

    #[test]
    fn load_from_path_rejects_empty_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trust.json");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(
            f,
            r#"{{"issuer":"https://app.bicameral-ai.com","audience":"product_scoped","verifying_keys":[]}}"#
        )
        .unwrap();
        assert!(load_from_path(&path).is_none());
    }

    #[test]
    fn load_from_path_accepts_valid_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trust.json");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(
            f,
            r#"{{"issuer":"https://app.bicameral-ai.com","audience":"product_scoped","verifying_keys":[{{"key_id":"k1","public_key_b64":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="}}]}}"#
        )
        .unwrap();
        let config = load_from_path(&path).expect("load");
        assert_eq!(config.verifying_keys[0].key_id, "k1");
    }
}
