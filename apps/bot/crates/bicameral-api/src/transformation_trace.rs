//! Typed, non-authoritative Integrations transformation-trace contract.
//!
//! The trace records how screened provider evidence crossed Integrations. It is
//! daemon-local review evidence only. It never creates Product authority.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const TRANSFORMATION_TRACE_SCHEMA: &str = "bicameral.transformation-trace/v1";
pub const TRANSFORMATION_TRACE_SET_SCHEMA: &str = "bicameral.transformation-trace-set/v1";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransformationTraceSet {
    pub schema: String,
    pub journey_id: String,
    pub trace_id: String,
    pub records: Vec<TransformationTraceRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransformationTraceRecord {
    pub schema: String,
    pub journey_id: String,
    pub trace_id: String,
    pub phase_id: String,
    pub occurred_at: String,
    pub component: TransformationTraceComponent,
    pub contract: TransformationTraceContract,
    pub input: TransformationTracePayload,
    pub output: TransformationTracePayload,
    pub delta: TransformationTraceDelta,
    pub evaluation: TransformationTraceEvaluation,
    pub persistence: TransformationTracePersistence,
    pub authority: TransformationTraceAuthority,
    pub timing: TransformationTraceTiming,
    pub receipt: TransformationTraceReceipt,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransformationTraceComponent {
    pub id: String,
    pub repository: String,
    pub revision: String,
    pub function: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransformationTraceContract {
    pub id: String,
    pub semantic_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransformationTracePayload {
    pub schema: String,
    pub digest: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransformationTraceDelta {
    #[serde(default)]
    pub preserved: Vec<TransformationTraceDeltaItem>,
    #[serde(default)]
    pub added: Vec<TransformationTraceDeltaItem>,
    #[serde(default)]
    pub transformed: Vec<TransformationTraceDeltaItem>,
    #[serde(default)]
    pub removed: Vec<TransformationTraceDeltaItem>,
    #[serde(default)]
    pub redacted: Vec<TransformationTraceDeltaItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransformationTraceDeltaItem {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransformationTraceEvaluation {
    #[serde(default)]
    pub hard_gates: Vec<TransformationTraceHardGate>,
    #[serde(default)]
    pub advisories: Vec<TransformationTraceAdvisory>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransformationTraceHardGate {
    pub gate_id: String,
    pub result: String,
    pub reason_code: String,
    #[serde(default)]
    pub evidence: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransformationTraceAdvisory {
    pub advisory_id: String,
    pub result: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    pub authority: String,
    pub failure_mode: String,
    pub scope: String,
    pub confidence: String,
    pub recommended_effect: String,
    pub basis: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransformationTracePersistence {
    pub plane: String,
    pub operation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransformationTraceAuthority {
    pub before: String,
    pub after: String,
    pub transition: String,
    pub canonical: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransformationTraceTiming {
    pub started_at: String,
    pub completed_at: String,
    pub duration_ms: u64,
    pub attempt: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransformationTraceReceipt {
    pub status: String,
    #[serde(default)]
    pub reason_codes: Vec<String>,
    pub retryable: bool,
    #[serde(default)]
    pub correlation_ids: BTreeMap<String, String>,
}
