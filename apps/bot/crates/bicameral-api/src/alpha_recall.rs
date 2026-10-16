//! Canonical daemon-owned `ToolResponse.result` envelope for alpha staged
//! recall — issue #817 (parent #789).
//!
//! `preflight.run` (and the accepted alpha recall command path) returns this
//! envelope as the `alpha_recall` field of the tool result. The daemon authors
//! the whole envelope: MCP, dashboard, cloud, and browser surfaces render it
//! and must not compute recall, synthesize fixture-backed success, own
//! freshness, or widen authority beyond request construction and rendering.
//!
//! The envelope draws an explicit line between:
//!
//! - [`AlphaRecallProduct`] — daemon-authored product / read-model content
//!   (staged recall payload and caveats), and
//! - [`AlphaRecallWitness`] — validation-only correlation (`RuntimeTrace`
//!   identifiers and an optional topology receipt) that later real-process
//!   validation reads but which is **never** product state.
//!
//! Read-only authority is stated on the envelope itself
//! ([`AlphaRecallAuthority`]): a recall lookup appends no canonical event and
//! mutates no Decision, binding, compliance, signoff, or durable lifecycle
//! state.
//!
//! Protocol/shape mismatch is explicit. [`AlphaRecallEnvelope::check_compatibility`]
//! returns a typed [`AlphaRecallMismatch`] rather than allowing any
//! fixture-backed or legacy fallback success path.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::recall::RecallPacket;
use crate::staged_preflight::StagedPreflightResponse;
use crate::tool_request::TraceStatus;

/// Canonical envelope contract version.
///
/// A breaking change to the envelope shape MUST bump this so a client can
/// refuse an envelope it cannot render instead of guessing.
///
/// v2 (issue #827): the product packet now carries reviewable
/// `decision_candidates` with informed-review fields and a typed
/// `decision_attention` signal. A v1 consumer cannot render these and MUST
/// refuse via [`AlphaRecallEnvelope::check_compatibility`] rather than
/// rendering an incomplete success.
pub const ALPHA_RECALL_ENVELOPE_VERSION: u32 = 2;

/// Canonical JSON key at which the daemon nests this envelope inside
/// `ToolResponse.result`.
///
/// The daemon returns the alpha recall envelope as `result.alpha_recall`. A
/// thin MCP client reads exactly this key (see
/// [`AlphaRecallEnvelope::from_tool_result`]) rather than guessing at the
/// nested response shape.
pub const ALPHA_RECALL_RESULT_KEY: &str = "alpha_recall";

// ── Top-level envelope ──────────────────────────────────────────────────────

/// Canonical daemon-owned `ToolResponse.result` envelope for alpha staged
/// recall.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlphaRecallEnvelope {
    /// Envelope contract version (see [`ALPHA_RECALL_ENVELOPE_VERSION`]).
    pub envelope_version: u32,

    /// `ToolRequest` protocol version the daemon produced this envelope under.
    ///
    /// Pinned by the daemon so a thin client can refuse an incompatible
    /// protocol instead of rendering a fabricated result.
    pub protocol_version: String,

    /// Explicit read-only authority guarantee for this envelope.
    pub authority: AlphaRecallAuthority,

    /// Daemon-authored product / read-model content.
    pub product: AlphaRecallProduct,

    /// Validation-only witnesses. Never product state.
    pub witness: AlphaRecallWitness,
}

impl AlphaRecallEnvelope {
    /// Build a read-only alpha recall envelope at the current contract version.
    pub fn new(
        protocol_version: impl Into<String>,
        product: AlphaRecallProduct,
        witness: AlphaRecallWitness,
    ) -> Self {
        Self {
            envelope_version: ALPHA_RECALL_ENVELOPE_VERSION,
            protocol_version: protocol_version.into(),
            authority: AlphaRecallAuthority::read_only(),
            product,
            witness,
        }
    }

    /// Verify this envelope can be rendered by a client that supports the given
    /// `ToolRequest` protocol version(s) and envelope contract version.
    ///
    /// Returns an explicit typed [`AlphaRecallMismatch`] — never a silent
    /// fallback — when incompatible. There is no fixture-backed or legacy
    /// success path.
    pub fn check_compatibility(
        &self,
        client_protocols: &[&str],
        client_envelope_version: u32,
    ) -> Result<(), AlphaRecallMismatch> {
        if !client_protocols.contains(&self.protocol_version.as_str()) {
            return Err(AlphaRecallMismatch {
                kind: AlphaRecallMismatchKind::IncompatibleProtocol,
                detail: format!(
                    "daemon protocol '{}' not in client-supported {:?}; refusing to render \
                     (no legacy fallback)",
                    self.protocol_version, client_protocols
                ),
            });
        }
        if self.envelope_version != client_envelope_version {
            return Err(AlphaRecallMismatch {
                kind: AlphaRecallMismatchKind::UnsupportedEnvelopeVersion,
                detail: format!(
                    "alpha recall envelope version {} is not client-supported version {}; \
                     refusing to render (no legacy fallback)",
                    self.envelope_version, client_envelope_version
                ),
            });
        }
        Ok(())
    }

    /// Extract the canonical alpha recall envelope from a daemon
    /// `ToolResponse.result` value.
    ///
    /// A thin MCP client uses this to render the nested daemon-owned envelope
    /// (`result.alpha_recall`) rather than guessing at the response shape. It
    /// fails closed with a typed [`AlphaRecallMismatch`] — never a
    /// silently-successful render with missing daemon-owned content, and never
    /// a legacy MCP-local fallback:
    ///
    /// - a `None` result carries no daemon-owned content →
    ///   [`AlphaRecallMismatchKind::MissingEnvelope`];
    /// - a result missing the canonical [`ALPHA_RECALL_RESULT_KEY`] key →
    ///   [`AlphaRecallMismatchKind::MissingEnvelope`];
    /// - an `alpha_recall` value that does not deserialize into this envelope →
    ///   [`AlphaRecallMismatchKind::MalformedEnvelope`].
    pub fn from_tool_result(
        result: Option<&serde_json::Value>,
    ) -> Result<Self, AlphaRecallMismatch> {
        let result = result.ok_or_else(|| AlphaRecallMismatch {
            kind: AlphaRecallMismatchKind::MissingEnvelope,
            detail: "daemon result is absent; there is no alpha recall envelope to render \
                     (no legacy fallback)"
                .into(),
        })?;
        let envelope_value =
            result
                .get(ALPHA_RECALL_RESULT_KEY)
                .ok_or_else(|| AlphaRecallMismatch {
                    kind: AlphaRecallMismatchKind::MissingEnvelope,
                    detail: format!(
                        "daemon result is missing the canonical '{ALPHA_RECALL_RESULT_KEY}' \
                     envelope; refusing to synthesize recall (no legacy fallback)"
                    ),
                })?;
        serde_json::from_value(envelope_value.clone()).map_err(|e| AlphaRecallMismatch {
            kind: AlphaRecallMismatchKind::MalformedEnvelope,
            detail: format!(
                "daemon '{ALPHA_RECALL_RESULT_KEY}' value does not match the canonical alpha \
                 recall envelope shape: {e} (no legacy fallback)"
            ),
        })
    }
}

// ── Read-only authority guarantee ───────────────────────────────────────────

/// Authority mode of the alpha recall lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlphaRecallAuthorityMode {
    /// The recall lookup is strictly read-only.
    ReadOnly,
}

/// Explicit read-only authority guarantee carried by the envelope.
///
/// The flags are always false for a recall lookup — they are stated explicitly
/// so a surface never has to infer the authority posture.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AlphaRecallAuthority {
    /// Authority mode — always [`AlphaRecallAuthorityMode::ReadOnly`].
    pub mode: AlphaRecallAuthorityMode,

    /// Whether the lookup appended any canonical event. Always `false`.
    pub appended_canonical_events: bool,

    /// Whether the lookup created or mutated any Decision, binding, compliance,
    /// signoff, or durable lifecycle state. Always `false`.
    pub mutated_lifecycle_state: bool,
}

impl AlphaRecallAuthority {
    /// The read-only guarantee: no canonical events, no lifecycle mutation.
    pub fn read_only() -> Self {
        Self {
            mode: AlphaRecallAuthorityMode::ReadOnly,
            appended_canonical_events: false,
            mutated_lifecycle_state: false,
        }
    }
}

impl Default for AlphaRecallAuthority {
    fn default() -> Self {
        Self::read_only()
    }
}

// ── Daemon-authored product content ─────────────────────────────────────────

/// Daemon-authored product / read-model content for the recall lookup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlphaRecallProduct {
    /// Staged capture / projection / lookup / enforcement / session-directive
    /// result. Unsupported stages are explicit, never inferred by a client.
    pub staged: StagedPreflightResponse,

    /// The read-only recall packet: canonical decisions, decision candidates,
    /// searched/unknown scope, and allowed next actions.
    pub packet: RecallPacket,

    /// Daemon-authored caveats describing missing / stale / partial /
    /// unavailable source, graph, index, or freshness state. Clients render
    /// these directly and must not synthesize their own limitation verdict.
    #[serde(default)]
    pub caveats: Vec<AlphaRecallCaveat>,
}

/// Scope a caveat applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlphaRecallCaveatScope {
    /// Source corpus scope.
    Source,
    /// Code graph / snapshot scope.
    Graph,
    /// Commit-driven freshness scope.
    Freshness,
    /// Decision index scope.
    Index,
    /// Requested query scope.
    Query,
}

/// Classification of a caveat's limitation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlphaRecallCaveatStatus {
    /// The scope was requested but nothing was available.
    Missing,
    /// The scope was available but potentially out of date.
    Stale,
    /// The scope was only partially searched.
    Partial,
    /// The scope could not be searched at all.
    Unavailable,
}

/// A single daemon-authored recall caveat.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AlphaRecallCaveat {
    /// Which scope the caveat applies to.
    pub scope: AlphaRecallCaveatScope,

    /// The limitation classification.
    pub status: AlphaRecallCaveatStatus,

    /// Human-readable detail. Must not leak local filesystem paths, provider
    /// secrets, or source/evidence bodies.
    pub detail: String,
}

// ── Validation-only witnesses ───────────────────────────────────────────────

/// Validation-only witnesses. These correlate runtime behavior for later
/// real-process validation and are **never** product state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlphaRecallWitness {
    /// `RuntimeTrace` correlation fields for this response.
    pub runtime_trace: AlphaRecallTraceWitness,

    /// Optional topology receipt witness for later real-process validation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topology_receipt: Option<AlphaRecallTopologyWitness>,
}

/// `RuntimeTrace` correlation echo carried inside the envelope.
///
/// Mirrors the correlation identifiers of the `ToolResponse.trace`
/// [`crate::tool_request::RuntimeTrace`]. It is an operational receipt only: it
/// cannot create Decisions, approve signoff, resolve compliance, or emit
/// blocking governance results.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AlphaRecallTraceWitness {
    /// Correlates with `ToolRequest.request_id` and `ToolResponse.request_id`.
    pub request_id: Uuid,

    /// The dispatched command name (e.g. `"preflight.run"`).
    pub command: String,

    /// The surface that submitted the request (e.g. `"mcp"`, `"cli"`).
    pub caller_surface: String,

    /// Outcome status mirroring the response's `RuntimeTrace`.
    pub status: TraceStatus,
}

/// Topology receipt witness for later real-process validation (cloud#135).
///
/// Correlation only. The fields mirror the disposable real-process topology
/// receipt consumed by `scripts/validate_hosted_workspace_binding.py`, letting a
/// later real-process run correlate this envelope against that receipt. It is
/// never product state and never carries local filesystem paths or secrets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AlphaRecallTopologyWitness {
    /// Whether a real-process topology receipt was consumed to produce this
    /// witness. `false` for the deterministic in-process contract slice.
    pub consumed: bool,

    /// Receipt run identifier, when a receipt was consumed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,

    /// Receipt status, when a receipt was consumed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,

    /// Receipt evidence level, when a receipt was consumed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_level: Option<String>,

    /// Receipt component labels, when a receipt was consumed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<String>,

    /// Topology anchor (e.g. the cloud#135 merge commit or receipt digest).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
}

impl AlphaRecallTopologyWitness {
    /// No receipt supplied in-process. The alpha recall envelope is a
    /// deterministic contract slice and does not orchestrate real processes;
    /// later real-process validation supplies the consumed receipt.
    pub fn not_consumed() -> Self {
        Self {
            consumed: false,
            run_id: None,
            status: None,
            evidence_level: None,
            components: Vec::new(),
            anchor: None,
        }
    }
}

// ── Protocol / shape mismatch ───────────────────────────────────────────────

/// Kind of protocol/shape mismatch for the alpha recall envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlphaRecallMismatchKind {
    /// The envelope's `ToolRequest` protocol version is not client-supported.
    IncompatibleProtocol,
    /// The envelope contract version is not client-supported.
    UnsupportedEnvelopeVersion,
    /// The daemon result carried no canonical alpha recall envelope (absent
    /// result or missing `alpha_recall` key).
    MissingEnvelope,
    /// The `alpha_recall` value did not match the canonical envelope shape.
    MalformedEnvelope,
}

/// Explicit, visible protocol/shape mismatch outcome.
///
/// There is no fixture-backed or legacy fallback success path: a client that
/// cannot speak the envelope's protocol or contract version must surface this
/// typed mismatch instead of rendering a fabricated result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AlphaRecallMismatch {
    /// The mismatch classification.
    pub kind: AlphaRecallMismatchKind,

    /// Human-readable detail.
    pub detail: String,
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staged_preflight::StagedPreflightResponse;

    fn sample_product() -> AlphaRecallProduct {
        AlphaRecallProduct {
            staged: StagedPreflightResponse::alpha_from_lookup(
                vec!["dec-817".into()],
                vec!["graph not configured".into()],
                None,
            ),
            packet: RecallPacket::new_alpha(),
            caveats: vec![AlphaRecallCaveat {
                scope: AlphaRecallCaveatScope::Graph,
                status: AlphaRecallCaveatStatus::Unavailable,
                detail: "graph readiness unavailable; source-only lookup".into(),
            }],
        }
    }

    fn sample_witness() -> AlphaRecallWitness {
        AlphaRecallWitness {
            runtime_trace: AlphaRecallTraceWitness {
                request_id: Uuid::nil(),
                command: "preflight.run".into(),
                caller_surface: "mcp".into(),
                status: TraceStatus::Ok,
            },
            topology_receipt: Some(AlphaRecallTopologyWitness::not_consumed()),
        }
    }

    fn sample_envelope() -> AlphaRecallEnvelope {
        AlphaRecallEnvelope::new("v2", sample_product(), sample_witness())
    }

    #[test]
    fn envelope_roundtrips_through_json() {
        let envelope = sample_envelope();
        let json = serde_json::to_string_pretty(&envelope).unwrap();
        let parsed: AlphaRecallEnvelope = serde_json::from_str(&json).unwrap();

        let a = serde_json::to_value(&envelope).unwrap();
        let b = serde_json::to_value(&parsed).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn envelope_pins_contract_version_and_protocol() {
        let envelope = sample_envelope();
        assert_eq!(envelope.envelope_version, ALPHA_RECALL_ENVELOPE_VERSION);
        assert_eq!(envelope.protocol_version, "v2");
    }

    #[test]
    fn product_and_witness_are_distinct_sections() {
        let envelope = sample_envelope();
        let v = serde_json::to_value(&envelope).unwrap();

        // Product content lives under `product`, witnesses under `witness`.
        assert!(v["product"]["staged"].is_object());
        assert!(v["product"]["packet"].is_object());
        assert!(v["witness"]["runtime_trace"].is_object());
        assert!(v["witness"]["topology_receipt"].is_object());

        // Witness correlation must not leak into product content.
        assert!(v["product"].get("runtime_trace").is_none());
        assert!(v["product"].get("topology_receipt").is_none());
    }

    #[test]
    fn authority_states_read_only_no_mutation() {
        let envelope = sample_envelope();
        assert_eq!(envelope.authority.mode, AlphaRecallAuthorityMode::ReadOnly);
        assert!(!envelope.authority.appended_canonical_events);
        assert!(!envelope.authority.mutated_lifecycle_state);

        let v = serde_json::to_value(&envelope).unwrap();
        assert_eq!(v["authority"]["mode"], "read_only");
        assert_eq!(v["authority"]["appended_canonical_events"], false);
        assert_eq!(v["authority"]["mutated_lifecycle_state"], false);
    }

    #[test]
    fn topology_witness_default_is_not_consumed() {
        let witness = AlphaRecallTopologyWitness::not_consumed();
        assert!(!witness.consumed);
        assert!(witness.run_id.is_none());
        assert!(witness.components.is_empty());
    }

    #[test]
    fn caveats_roundtrip_all_scope_and_status_variants() {
        for scope in [
            AlphaRecallCaveatScope::Source,
            AlphaRecallCaveatScope::Graph,
            AlphaRecallCaveatScope::Freshness,
            AlphaRecallCaveatScope::Index,
            AlphaRecallCaveatScope::Query,
        ] {
            for status in [
                AlphaRecallCaveatStatus::Missing,
                AlphaRecallCaveatStatus::Stale,
                AlphaRecallCaveatStatus::Partial,
                AlphaRecallCaveatStatus::Unavailable,
            ] {
                let caveat = AlphaRecallCaveat {
                    scope,
                    status,
                    detail: "detail".into(),
                };
                let json = serde_json::to_string(&caveat).unwrap();
                let parsed: AlphaRecallCaveat = serde_json::from_str(&json).unwrap();
                assert_eq!(caveat, parsed);
            }
        }
    }

    #[test]
    fn compatible_client_renders_envelope() {
        let envelope = sample_envelope();
        assert!(envelope
            .check_compatibility(&["v2"], ALPHA_RECALL_ENVELOPE_VERSION)
            .is_ok());
    }

    #[test]
    fn incompatible_protocol_is_explicit_not_fallback() {
        let envelope = sample_envelope();
        let err = envelope
            .check_compatibility(&["v1"], ALPHA_RECALL_ENVELOPE_VERSION)
            .expect_err("incompatible protocol must be refused");
        assert_eq!(err.kind, AlphaRecallMismatchKind::IncompatibleProtocol);

        // Future/unknown protocol also fails closed.
        let future = AlphaRecallEnvelope::new("v99", sample_product(), sample_witness());
        assert!(future
            .check_compatibility(&["v2"], ALPHA_RECALL_ENVELOPE_VERSION)
            .is_err());
    }

    #[test]
    fn unsupported_envelope_version_is_explicit_not_fallback() {
        let envelope = sample_envelope();
        let err = envelope
            .check_compatibility(&["v2"], ALPHA_RECALL_ENVELOPE_VERSION + 1)
            .expect_err("unsupported envelope version must be refused");
        assert_eq!(
            err.kind,
            AlphaRecallMismatchKind::UnsupportedEnvelopeVersion
        );
    }

    #[test]
    fn envelope_never_claims_compliance_or_blocking() {
        let envelope = sample_envelope();
        let json = serde_json::to_string(&envelope).unwrap();
        for term in ["compliant", "\"block\"", "\"pause\"", "\"warn\""] {
            assert!(
                !json.contains(term),
                "alpha recall envelope must not contain '{term}'"
            );
        }
    }

    /// A thin client extracts the nested envelope from a canonical
    /// `ToolResponse.result` shape (`result.alpha_recall`).
    #[test]
    fn from_tool_result_extracts_nested_envelope() {
        let envelope = sample_envelope();
        let result = serde_json::json!({
            "staged": { "some": "legacy sibling" },
            "live_recall": { "packet": "legacy sibling" },
            ALPHA_RECALL_RESULT_KEY: serde_json::to_value(&envelope).unwrap(),
        });

        let extracted = AlphaRecallEnvelope::from_tool_result(Some(&result))
            .expect("canonical nested result must yield the envelope");
        assert_eq!(
            serde_json::to_value(&extracted).unwrap(),
            serde_json::to_value(&envelope).unwrap()
        );
    }

    /// A result missing the canonical key fails closed — the client must not
    /// synthesize a successful render from missing daemon-owned content.
    #[test]
    fn from_tool_result_missing_key_fails_closed() {
        let result = serde_json::json!({
            "staged": { "some": "legacy sibling" },
            "live_recall": { "packet": "legacy sibling" },
        });
        let err = AlphaRecallEnvelope::from_tool_result(Some(&result))
            .expect_err("missing alpha_recall key must be a visible mismatch");
        assert_eq!(err.kind, AlphaRecallMismatchKind::MissingEnvelope);
    }

    /// An absent result carries no daemon-owned content and fails closed.
    #[test]
    fn from_tool_result_absent_result_fails_closed() {
        let err = AlphaRecallEnvelope::from_tool_result(None)
            .expect_err("absent result must be a visible mismatch");
        assert_eq!(err.kind, AlphaRecallMismatchKind::MissingEnvelope);
    }

    /// A malformed / legacy-shaped `alpha_recall` value fails closed rather
    /// than being coerced into a fabricated render.
    #[test]
    fn from_tool_result_malformed_envelope_fails_closed() {
        let result = serde_json::json!({
            // Legacy MCP-local shape: not the canonical envelope.
            ALPHA_RECALL_RESULT_KEY: { "mcp_local_recall": true, "score": 0.9 },
        });
        let err = AlphaRecallEnvelope::from_tool_result(Some(&result))
            .expect_err("legacy/malformed alpha_recall must be a visible mismatch");
        assert_eq!(err.kind, AlphaRecallMismatchKind::MalformedEnvelope);
    }
}
