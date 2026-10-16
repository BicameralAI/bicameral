//! Redacted, operator-local operational diagnostics.
//!
//! These records explain daemon behavior but are not governance authority.
//! They intentionally exclude raw Product ids, idempotency keys, actor ids,
//! session tokens, paths, source bodies, and event-store payloads.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use bicameral_api::event_store::SubstrateKind;
use bicameral_api::tool_request::{ToolCommand, ToolRequest, ToolResponse, ToolResponseStatus};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const DEFAULT_MAX_RECORDS: usize = 200;
const DEFAULT_MAX_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductOperationalRecord {
    pub schema_version: u8,
    pub recorded_at: DateTime<Utc>,
    pub request_id: Uuid,
    pub command: String,
    pub status: String,
    pub error_code: Option<String>,
    pub surface: String,
    pub event_store_substrate: String,
    pub product_id_fingerprint: Option<String>,
    pub idempotency_key_fingerprint: Option<String>,
    pub canonical_product_count: Option<u64>,
    pub create_event_count: Option<u64>,
    pub malformed_create_event_count: Option<u64>,
}

#[derive(Debug)]
pub struct OperationalLogStore {
    directory: PathBuf,
    max_records: usize,
    max_age: Duration,
}

impl OperationalLogStore {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
            max_records: DEFAULT_MAX_RECORDS,
            max_age: DEFAULT_MAX_AGE,
        }
    }

    #[cfg(test)]
    fn with_policy(directory: impl Into<PathBuf>, max_records: usize, max_age: Duration) -> Self {
        Self {
            directory: directory.into(),
            max_records,
            max_age,
        }
    }

    pub fn record_product_command(
        &self,
        request: &ToolRequest,
        response: &ToolResponse,
        substrate: SubstrateKind,
    ) -> io::Result<ProductOperationalRecord> {
        let record = ProductOperationalRecord::from_command(request, response, substrate);
        self.ensure_private_directory()?;

        let filename = format!(
            "{}-{}.json",
            record.recorded_at.timestamp_millis(),
            record.request_id
        );
        let path = self.directory.join(filename);
        let content = serde_json::to_vec_pretty(&record)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        file.write_all(&content)?;
        file.write_all(b"\n")?;
        file.sync_all()?;

        self.prune()?;
        Ok(record)
    }

    /// Enforce the configured count and age boundaries without writing a new
    /// record. The daemon calls this at startup and periodically while running
    /// so retention does not depend only on a later Product command.
    pub fn enforce_retention(&self) -> io::Result<()> {
        self.ensure_private_directory()?;
        self.prune()
    }

    fn ensure_private_directory(&self) -> io::Result<()> {
        fs::create_dir_all(&self.directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.directory, fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }

    fn prune(&self) -> io::Result<()> {
        let now = SystemTime::now();
        let mut retained = Vec::new();

        for entry in fs::read_dir(&self.directory)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }

            let modified = entry
                .metadata()?
                .modified()
                .unwrap_or(SystemTime::UNIX_EPOCH);
            if now.duration_since(modified).unwrap_or_default() > self.max_age {
                remove_file_if_present(&path)?;
                continue;
            }
            retained.push((modified, path));
        }

        retained.sort_by_key(|(modified, path)| (*modified, path.clone()));
        let excess = retained.len().saturating_sub(self.max_records);
        for (_, path) in retained.into_iter().take(excess) {
            remove_file_if_present(&path)?;
        }
        Ok(())
    }
}

fn remove_file_if_present(path: &std::path::Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

impl ProductOperationalRecord {
    fn from_command(
        request: &ToolRequest,
        response: &ToolResponse,
        substrate: SubstrateKind,
    ) -> Self {
        let request_product_fingerprint = request
            .product_id
            .as_deref()
            .map(|value| diagnostic_fingerprint("product_id", value));
        let (product_id_fingerprint, idempotency_key_fingerprint) = match &request.command {
            ToolCommand::ProductCreate(params) => (
                response
                    .result
                    .as_ref()
                    .and_then(|result| result.pointer("/result/product_id"))
                    .and_then(|value| value.as_str())
                    .map(|value| diagnostic_fingerprint("product_id", value)),
                Some(diagnostic_fingerprint(
                    "product_create_idempotency_key",
                    &params.idempotency_key,
                )),
            ),
            ToolCommand::ProductPublish(params) => (
                Some(diagnostic_fingerprint("product_id", &params.product_id)),
                Some(diagnostic_fingerprint(
                    "product_publish_idempotency_key",
                    &params.idempotency_key,
                )),
            ),
            _ => (request_product_fingerprint, None),
        };

        let diagnostics = response
            .result
            .as_ref()
            .and_then(|result| result.get("diagnostics"));

        Self {
            schema_version: 1,
            recorded_at: response.responded_at,
            request_id: response.request_id,
            command: request.command.command_name().to_string(),
            status: status_label(response.status).to_string(),
            error_code: safe_error_code(response),
            surface: request
                .authority
                .audit_metadata
                .get("surface")
                .and_then(|value| value.as_str())
                .filter(|value| *value == "hosted_bridge")
                .unwrap_or("local")
                .to_string(),
            event_store_substrate: substrate.to_string(),
            product_id_fingerprint,
            idempotency_key_fingerprint,
            canonical_product_count: diagnostic_count(diagnostics, "canonical_product_count"),
            create_event_count: diagnostic_count(diagnostics, "create_event_count"),
            malformed_create_event_count: diagnostic_count(
                diagnostics,
                "malformed_create_event_count",
            ),
        }
    }
}

fn diagnostic_count(diagnostics: Option<&serde_json::Value>, field: &str) -> Option<u64> {
    diagnostics
        .and_then(|value| value.get(field))
        .and_then(|value| value.as_u64())
}

fn status_label(status: ToolResponseStatus) -> &'static str {
    match status {
        ToolResponseStatus::Ok => "ok",
        ToolResponseStatus::Rejected => "rejected",
        ToolResponseStatus::Error => "error",
    }
}

pub(crate) fn safe_error_code(response: &ToolResponse) -> Option<String> {
    if response.status == ToolResponseStatus::Ok {
        return None;
    }

    let from_diagnostics = response
        .result
        .as_ref()
        .and_then(|result| result.pointer("/diagnostics/error_code"))
        .and_then(|value| value.as_str());
    let candidate = from_diagnostics.or_else(|| {
        response
            .message
            .as_deref()
            .and_then(|message| message.split_once(':').map(|(code, _)| code))
    })?;

    if candidate.len() <= 64
        && candidate
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        Some(candidate.to_string())
    } else {
        Some(status_label(response.status).to_string())
    }
}

pub(crate) fn diagnostic_fingerprint(namespace: &str, value: &str) -> String {
    let digest = Sha256::digest(format!("{namespace}:{value}").as_bytes());
    format!("{digest:x}")[..16].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_api::product_creation::ProductPublishParams;
    use bicameral_api::tool_request::{AuthMethod, AuthorityContext};

    fn request() -> ToolRequest {
        let mut audit_metadata = serde_json::Map::new();
        audit_metadata.insert("surface".into(), serde_json::json!("hosted_bridge"));
        ToolRequest::new(
            ToolCommand::ProductPublish(ProductPublishParams {
                product_id: "prd_sensitive".into(),
                idempotency_key: "publish_sensitive".into(),
            }),
            AuthorityContext {
                actor_id: "bridge:acct_sensitive".into(),
                auth_method: AuthMethod::HostedBridge,
                session_id: Some("session_sensitive".into()),
                workspace: Some("/private/workspace".into()),
                policy_scope: vec!["hosted_bridge".into()],
                audit_metadata,
            },
        )
    }

    fn rejected_response(request_id: Uuid) -> ToolResponse {
        ToolResponse {
            request_id,
            status: ToolResponseStatus::Rejected,
            result: Some(serde_json::json!({
                "diagnostics": {
                    "error_code": "product_not_found",
                    "canonical_product_count": 0,
                    "create_event_count": 0,
                    "malformed_create_event_count": 0
                }
            })),
            message: Some("product_not_found: bounded public message".into()),
            governance_result: None,
            trace: None,
            responded_at: Utc::now(),
        }
    }

    #[test]
    fn product_failure_record_is_private_redacted_and_bounded() {
        let directory = tempfile::tempdir().unwrap();
        let store = OperationalLogStore::with_policy(directory.path(), 1, Duration::from_secs(60));

        let first = request();
        store
            .record_product_command(
                &first,
                &rejected_response(first.request_id),
                SubstrateKind::Git,
            )
            .unwrap();
        let second = request();
        let record = store
            .record_product_command(
                &second,
                &rejected_response(second.request_id),
                SubstrateKind::Git,
            )
            .unwrap();

        let files: Vec<_> = fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(files.len(), 1, "retention must cap local records");

        let serialized = fs::read_to_string(&files[0]).unwrap();
        assert_eq!(record.error_code.as_deref(), Some("product_not_found"));
        assert_eq!(record.surface, "hosted_bridge");
        assert_eq!(record.canonical_product_count, Some(0));
        for forbidden in [
            "prd_sensitive",
            "publish_sensitive",
            "acct_sensitive",
            "session_sensitive",
            "/private/workspace",
        ] {
            assert!(
                !serialized.contains(forbidden),
                "operational log leaked {forbidden}"
            );
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(directory.path()).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(&files[0]).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn every_product_scoped_request_records_only_a_product_fingerprint() {
        let request = bicameral_api::fixtures::search_query().for_product("prd_topology_alpha");
        let response = ToolResponse::success(
            request.request_id,
            serde_json::json!({"surface": "search", "matches": []}),
        );

        let record =
            ProductOperationalRecord::from_command(&request, &response, SubstrateKind::Git);

        assert_eq!(
            record.product_id_fingerprint,
            Some(diagnostic_fingerprint("product_id", "prd_topology_alpha"))
        );
        let serialized = serde_json::to_string(&record).unwrap();
        assert!(!serialized.contains("prd_topology_alpha"));
    }

    #[test]
    fn explicit_retention_enforcement_removes_expired_records() {
        let directory = tempfile::tempdir().unwrap();
        let store =
            OperationalLogStore::with_policy(directory.path(), DEFAULT_MAX_RECORDS, Duration::ZERO);
        let expired = directory.path().join("expired.json");
        fs::write(&expired, "{}\n").unwrap();

        store.enforce_retention().unwrap();

        assert!(
            !expired.exists(),
            "startup and periodic retention must remove expired records"
        );
    }
}
