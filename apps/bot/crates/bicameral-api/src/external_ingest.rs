//! External/source-system ingest protocol types.
//!
//! This module owns the authority-stripped envelope accepted from integrations.
//! It is deliberately separate from `ToolRequest`: external sources may submit
//! evidence, advisory candidate hints, and a value-free redaction receipt, but
//! they cannot submit actor authority, review commands, canonical Decisions,
//! signoff, compliance, policy grants, or event-store instructions.

use serde::{Deserialize, Serialize};

pub const EXTERNAL_INGEST_PROTOCOL_VERSION: u32 = 2;
pub const MINIMUM_SUPPORTED_EXTERNAL_INGEST_PROTOCOL_VERSION: u32 = 2;
pub const EXTERNAL_INGEST_CONTRACT_ID: &str = "external-ingest.delivery.v2";
pub const EXTERNAL_INGEST_DELIVERY_ENDPOINT: &str = "/api/v2/external-ingest";
pub const EXTERNAL_INGEST_CAPABILITIES_ENDPOINT: &str = "/api/external-ingest/capabilities";
pub const EXTERNAL_INGEST_REQUEST_SCHEMA_SHA256: &str =
    "59819fc34f0ec7a54225cb488c9d12794655bd49f9521ae48118f1b965154746";
pub const EXTERNAL_INGEST_CONTRACT_FINGERPRINT: &str =
    "52f2575ac95c71bf4fbfeb14605e9e5b1a30e0df52eb4adb6c9f238a52be9dd0";

const RECEIPT_SCHEMA_VERSION: u32 = 1;
const ALLOWED_FINDING_CATEGORIES: &[&str] = &["credential", "pii", "secret"];
const ALLOWED_FINDING_ACTIONS: &[&str] = &["removed", "tokenized"];

/// External/source-system evidence submission contract.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExternalIngestEnvelope {
    /// Source system identifier (for example `jira`, `linear`, `slack`, `github`).
    pub source_system: String,

    /// Source URI for provenance.
    pub source_uri: String,

    /// Sanitized captured content snapshot.
    pub content: String,

    /// Content hash (SHA-256). The daemon verifies if provided and computes if omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,

    /// Evidence excerpts within the sanitized content.
    #[serde(default)]
    pub evidence: Vec<ExternalEvidenceItem>,

    /// Candidate suggestions. Advisory only, never canonical authority.
    #[serde(default)]
    pub candidate_hints: Vec<ExternalCandidateHint>,

    /// Value-free proof that required pre-Bot redaction executed.
    ///
    /// The receipt carries digests, ruleset identity, categories, actions, counts,
    /// and completion metadata. It must never contain removed values. Malformed
    /// receipts fail during typed deserialization before evidence admission.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(required)]
    pub redaction_receipt: Option<ExternalRedactionReceipt>,
}

/// Non-authoritative compatibility metadata for the external-ingest producer.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExternalIngestCapabilities {
    pub contract_id: String,
    pub protocol_version: u32,
    pub minimum_supported_protocol_version: u32,
    pub supported_protocol_versions: Vec<u32>,
    pub delivery_endpoint: String,
    pub request_schema_sha256: String,
    pub contract_fingerprint: String,
    pub redaction_receipt_required: bool,
}

/// An evidence excerpt from an external source.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExternalEvidenceItem {
    pub excerpt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span_start: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span_end: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
}

/// A candidate hint from an external source. Advisory, not authority.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExternalCandidateHint {
    pub title: String,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
}

/// Deterministic proof that Integrations sanitized the Bot-bound payload.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(try_from = "ExternalRedactionReceiptWire")]
#[schemars(deny_unknown_fields)]
pub struct ExternalRedactionReceipt {
    /// Receipt schema version. Alpha requires version 1.
    #[schemars(range(min = 1, max = 1))]
    pub schema_version: u32,

    /// Redaction engine identity.
    #[schemars(length(min = 1))]
    pub engine: String,

    /// Redaction engine version.
    #[schemars(length(min = 1))]
    pub engine_version: String,

    /// Versioned ruleset identifier.
    #[schemars(length(min = 1))]
    pub ruleset_id: String,

    /// SHA-256 digest of the canonical ruleset manifest.
    #[schemars(regex(pattern = r"^sha256:[0-9a-f]{64}$"))]
    pub ruleset_digest: String,

    /// SHA-256 digest of the canonical pre-redaction payload.
    #[schemars(regex(pattern = r"^sha256:[0-9a-f]{64}$"))]
    pub input_digest: String,

    /// SHA-256 digest of the canonical sanitized payload.
    #[schemars(regex(pattern = r"^sha256:[0-9a-f]{64}$"))]
    pub output_digest: String,

    /// Value-free categories and actions applied.
    #[serde(default)]
    pub findings: Vec<ExternalRedactionFinding>,

    /// Confirms provider/evidence structural identity was preserved.
    #[schemars(schema_with = "true_boolean_schema")]
    pub structural_fields_preserved: bool,

    /// UTC completion timestamp.
    #[schemars(regex(
        pattern = r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$"
    ))]
    pub completed_at: String,
}

fn true_boolean_schema(
    generator: &mut schemars::r#gen::SchemaGenerator,
) -> schemars::schema::Schema {
    use schemars::JsonSchema;

    let mut schema: schemars::schema::SchemaObject = bool::json_schema(generator).into();
    schema.const_value = Some(serde_json::Value::Bool(true));
    schema.into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExternalRedactionReceiptWire {
    schema_version: u32,
    engine: String,
    engine_version: String,
    ruleset_id: String,
    ruleset_digest: String,
    input_digest: String,
    output_digest: String,
    #[serde(default)]
    findings: Vec<ExternalRedactionFinding>,
    structural_fields_preserved: bool,
    completed_at: String,
}

impl TryFrom<ExternalRedactionReceiptWire> for ExternalRedactionReceipt {
    type Error = String;

    fn try_from(wire: ExternalRedactionReceiptWire) -> Result<Self, Self::Error> {
        if wire.schema_version != RECEIPT_SCHEMA_VERSION {
            return Err("unsupported redaction receipt schema_version".into());
        }
        for (name, value) in [
            ("engine", wire.engine.as_str()),
            ("engine_version", wire.engine_version.as_str()),
            ("ruleset_id", wire.ruleset_id.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(format!("redaction receipt {name} must not be blank"));
            }
        }
        for (name, value) in [
            ("ruleset_digest", wire.ruleset_digest.as_str()),
            ("input_digest", wire.input_digest.as_str()),
            ("output_digest", wire.output_digest.as_str()),
        ] {
            if !is_sha256_digest(value) {
                return Err(format!(
                    "redaction receipt {name} must be sha256:<64 lowercase hex>"
                ));
            }
        }
        if !wire.structural_fields_preserved {
            return Err("redaction receipt must preserve structural identity fields".into());
        }
        let completed_at = chrono::DateTime::parse_from_rfc3339(&wire.completed_at)
            .map_err(|_| "redaction receipt completed_at must be RFC3339".to_string())?;
        if completed_at.offset().local_minus_utc() != 0 {
            return Err("redaction receipt completed_at must be UTC".into());
        }
        for finding in &wire.findings {
            if !ALLOWED_FINDING_CATEGORIES.contains(&finding.category.as_str()) {
                return Err("redaction receipt finding category is unsupported".into());
            }
            if !ALLOWED_FINDING_ACTIONS.contains(&finding.action.as_str()) {
                return Err("redaction receipt finding action is unsupported".into());
            }
            if finding.count == 0 {
                return Err("redaction receipt finding count must be positive".into());
            }
        }

        Ok(Self {
            schema_version: wire.schema_version,
            engine: wire.engine,
            engine_version: wire.engine_version,
            ruleset_id: wire.ruleset_id,
            ruleset_digest: wire.ruleset_digest,
            input_digest: wire.input_digest,
            output_digest: wire.output_digest,
            findings: wire.findings,
            structural_fields_preserved: wire.structural_fields_preserved,
            completed_at: wire.completed_at,
        })
    }
}

fn is_sha256_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

/// One value-free redaction finding summary.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExternalRedactionFinding {
    /// Bounded category such as `secret`, `credential`, or `pii`.
    #[schemars(regex(pattern = r"^(credential|pii|secret)$"))]
    pub category: String,

    /// Bounded action such as `tokenized` or `removed`.
    #[schemars(regex(pattern = r"^(removed|tokenized)$"))]
    pub action: String,

    /// Number of findings in this category.
    #[schemars(range(min = 1))]
    pub count: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_receipt_json() -> serde_json::Value {
        serde_json::json!({
            "schema_version": 1,
            "engine": "bicameral-stdlib-redaction",
            "engine_version": "1.0.0",
            "ruleset_id": "fx-sec-001-plus-pii-v1",
            "ruleset_digest": format!("sha256:{}", "1".repeat(64)),
            "input_digest": format!("sha256:{}", "2".repeat(64)),
            "output_digest": format!("sha256:{}", "3".repeat(64)),
            "findings": [{"category": "pii", "action": "tokenized", "count": 1}],
            "structural_fields_preserved": true,
            "completed_at": "2026-07-17T21:00:00Z"
        })
    }

    #[test]
    fn receipt_roundtrips_without_removed_values() {
        let receipt: ExternalRedactionReceipt =
            serde_json::from_value(valid_receipt_json()).unwrap();
        let envelope = ExternalIngestEnvelope {
            source_system: "github".into(),
            source_uri: "https://github.com/BicameralAI/example/issues/7".into(),
            content: "contact [redacted:email]".into(),
            content_hash: None,
            evidence: vec![ExternalEvidenceItem {
                excerpt: "contact [redacted:email]".into(),
                span_start: None,
                span_end: None,
                confidence: None,
            }],
            candidate_hints: vec![],
            redaction_receipt: Some(receipt),
        };

        let json = serde_json::to_string(&envelope).unwrap();
        assert!(json.contains("redaction_receipt"));
        assert!(!json.contains("user@example.com"));
        let parsed: ExternalIngestEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.redaction_receipt.unwrap().schema_version, 1);
    }

    #[test]
    fn malformed_receipts_fail_closed() {
        for mutation in [
            ("schema_version", serde_json::json!(2)),
            ("input_digest", serde_json::json!("sha256:not-a-digest")),
            ("structural_fields_preserved", serde_json::json!(false)),
            ("completed_at", serde_json::json!("yesterday-ish")),
            (
                "completed_at",
                serde_json::json!("2026-07-17T17:00:00-04:00"),
            ),
        ] {
            let mut value = valid_receipt_json();
            value[mutation.0] = mutation.1;
            assert!(serde_json::from_value::<ExternalRedactionReceipt>(value).is_err());
        }
    }

    #[test]
    fn receipt_rejects_unknown_fields_and_unbounded_findings() {
        let mut unknown = valid_receipt_json();
        unknown["removed_value"] = serde_json::json!("must never cross the boundary");
        assert!(serde_json::from_value::<ExternalRedactionReceipt>(unknown).is_err());

        let mut category = valid_receipt_json();
        category["findings"][0]["category"] = serde_json::json!("whatever-the-caller-wants");
        assert!(serde_json::from_value::<ExternalRedactionReceipt>(category).is_err());
    }

    #[test]
    fn published_schema_matches_receipt_runtime_guards() {
        let schema = serde_json::to_value(schemars::schema_for!(ExternalIngestEnvelope)).unwrap();
        let receipt = &schema["properties"]["redaction_receipt"];
        let finding = &schema["definitions"]["ExternalRedactionFinding"];

        assert!(schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field == "redaction_receipt"));
        assert_eq!(receipt["properties"]["schema_version"]["minimum"], 1.0);
        assert_eq!(receipt["properties"]["schema_version"]["maximum"], 1.0);
        assert_eq!(
            receipt["properties"]["ruleset_digest"]["pattern"],
            r"^sha256:[0-9a-f]{64}$"
        );
        assert_eq!(
            receipt["properties"]["structural_fields_preserved"]["const"],
            true
        );
        assert_eq!(finding["properties"]["count"]["minimum"], 1.0);
        assert_eq!(
            finding["properties"]["category"]["pattern"],
            r"^(credential|pii|secret)$"
        );
        assert_eq!(receipt["additionalProperties"], false);
    }
}
