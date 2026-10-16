//! Durable semantic constraint mapping (#485).
//!
//! A [`SemanticMapping`] links a source phrase (product/customer/compliance
//! language) to a candidate constraint. The durable boundary is structural, not
//! prompt-only: a mapping resolves to a constraint **only when it is accepted
//! AND confident** ([`resolve_mapping`]); otherwise it is `unresolved_ambiguity`
//! routed to human review. Core owns this mapping primitive + the resolve gate;
//! parsing natural language into candidate mappings (terminology / rule packs)
//! belongs in `bicameral-sidecar`.
//!
//! Mirrors the SDK `semantic` contract (`bicameral-sdk/src/semantic/index.ts`),
//! serialized snake_case per bot convention.

use crate::constraint::{Attribution, LifecycleReviewState};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Capture provenance for a mapping. Mirrors the SDK `Provenance`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub captured_at: DateTime<Utc>,
    pub captured_by: Attribution,
    pub capture_method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
}

/// A reference to the source phrase being mapped. Mirrors SDK `SourcePhraseRef`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourcePhraseRef {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
}

/// An illustrative example carried on a mapping. Mirrors SDK `MappingExample`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MappingExample {
    pub phrase: String,
    pub derived_constraint_id: Uuid,
}

/// A durable semantic mapping from source language to a candidate constraint.
///
/// Mirrors SDK `SemanticMapping`. `derived_constraint_id` references a
/// `CatalogConstraint.id` (#482), which is replay-stable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticMapping {
    pub id: Uuid,
    pub source_phrase: SourcePhraseRef,
    pub derived_constraint_id: Uuid,
    pub provenance: Provenance,
    pub review_state: LifecycleReviewState,
    /// Confidence in the mapping, stored as basis points (0..=10000) so the
    /// carrier stays `Eq`/`Hash`-friendly; helpers compare against a bps
    /// threshold. (SDK uses a 0..1 float; bps is the integer-faithful mirror.)
    pub confidence_bps: u16,
    #[serde(default)]
    pub examples: Vec<MappingExample>,
}

/// Why a mapping could not be resolved. Mirrors SDK `SemanticErrorCode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SemanticErrorCode {
    UnresolvedAmbiguity,
}

/// A semantic resolution failure routed to human review. Mirrors SDK `SemanticError`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct SemanticError {
    pub code: SemanticErrorCode,
    pub message: String,
    #[serde(default)]
    pub candidates: Vec<Uuid>,
}

/// Whether a mapping clears a confidence threshold (in basis points).
/// Mirrors SDK `isConfident`.
pub fn is_confident(mapping: &SemanticMapping, threshold_bps: u16) -> bool {
    mapping.confidence_bps >= threshold_bps
}

/// Resolve a mapping to its derived constraint — only when **accepted AND
/// confident**. Otherwise returns `unresolved_ambiguity` for human attention,
/// never a prompt-only guess. Exact port of SDK `resolveMapping`.
pub fn resolve_mapping(
    mapping: &SemanticMapping,
    threshold_bps: u16,
) -> Result<Uuid, SemanticError> {
    if mapping.review_state != LifecycleReviewState::Accepted
        || !is_confident(mapping, threshold_bps)
    {
        return Err(SemanticError {
            code: SemanticErrorCode::UnresolvedAmbiguity,
            message: format!(
                "Mapping {} is not an accepted, confident mapping.",
                mapping.id
            ),
            candidates: vec![mapping.derived_constraint_id],
        });
    }
    Ok(mapping.derived_constraint_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraint::ActorType;

    fn mapping(state: LifecycleReviewState, confidence_bps: u16) -> SemanticMapping {
        SemanticMapping {
            id: Uuid::new_v4(),
            source_phrase: SourcePhraseRef {
                text: "must encrypt PII at rest".to_string(),
                source_id: Some("jira://SEC-1".to_string()),
                locator: None,
            },
            derived_constraint_id: Uuid::new_v4(),
            provenance: Provenance {
                captured_at: Utc::now(),
                captured_by: Attribution {
                    actor_id: "mapper".to_string(),
                    actor_type: ActorType::Agent,
                    display_name: None,
                },
                capture_method: "rule-pack".to_string(),
                pipeline_version: Some("v1".to_string()),
                source_hash: None,
            },
            review_state: state,
            confidence_bps,
            examples: Vec::new(),
        }
    }

    #[test]
    fn semantic_error_code_round_trips_snake_case() {
        assert_eq!(
            serde_json::to_string(&SemanticErrorCode::UnresolvedAmbiguity).unwrap(),
            "\"unresolved_ambiguity\""
        );
        assert_eq!(
            serde_json::from_str::<SemanticErrorCode>("\"unresolved_ambiguity\"").unwrap(),
            SemanticErrorCode::UnresolvedAmbiguity
        );
        assert!(serde_json::from_str::<SemanticErrorCode>("\"ambiguous\"").is_err());
    }

    #[test]
    fn is_confident_threshold() {
        let m = mapping(LifecycleReviewState::Accepted, 7000);
        assert!(is_confident(&m, 7000));
        assert!(is_confident(&m, 6999));
        assert!(!is_confident(&m, 7001));
    }

    #[test]
    fn resolve_requires_accepted_and_confident() {
        let target = mapping(LifecycleReviewState::Accepted, 9000).derived_constraint_id;
        let mut m = mapping(LifecycleReviewState::Accepted, 9000);
        m.derived_constraint_id = target;
        assert_eq!(resolve_mapping(&m, 8000).unwrap(), target);
    }

    #[test]
    fn resolve_rejects_unaccepted_as_unresolved_ambiguity() {
        let m = mapping(LifecycleReviewState::Proposed, 9000);
        let err = resolve_mapping(&m, 8000).unwrap_err();
        assert_eq!(err.code, SemanticErrorCode::UnresolvedAmbiguity);
        assert_eq!(err.candidates, vec![m.derived_constraint_id]);
    }

    #[test]
    fn resolve_rejects_low_confidence_as_unresolved_ambiguity() {
        let m = mapping(LifecycleReviewState::Accepted, 5000);
        let err = resolve_mapping(&m, 8000).unwrap_err();
        assert_eq!(err.code, SemanticErrorCode::UnresolvedAmbiguity);
    }

    #[test]
    fn semantic_mapping_round_trips_json() {
        let m = mapping(LifecycleReviewState::Accepted, 8500);
        let back: SemanticMapping =
            serde_json::from_value(serde_json::to_value(&m).unwrap()).unwrap();
        assert_eq!(back, m);
    }
}
