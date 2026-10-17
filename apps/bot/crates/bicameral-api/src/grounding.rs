//! Code-region grounding & correction capture (#491).
//!
//! Grounds decisions/constraints to exact code regions and captures
//! *correction findings* when a changed code region overlaps a grounded region:
//! Bicameral proposes a source-doc/decision/constraint update rather than
//! silently mutating trusted corpus. The update trigger is **deterministic**
//! (region overlap), not an LLM honoring instructions; findings route through
//! human review.
//!
//! Mirrors the SDK `grounding` contract (`bicameral-sdk/src/grounding/index.ts`),
//! serialized snake_case per bot convention. Reuses [`FindingSeverity`] (#484)
//! and [`Attribution`] (#481). Provider-specific PR/Git/docs mechanics belong to
//! integrations; design-partner-specific source-doc templates belong to `bicameral-sidecar`.

use crate::constraint::{Attribution, LifecycleReviewState};
use crate::drift::FindingSeverity;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A reference to an exact code region. Mirrors SDK `CodeRegionRef`.
///
/// A missing line range means the whole file. `git_ref` mirrors the SDK `ref`
/// field (renamed; `ref` is a Rust keyword).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRegionRef {
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// A reference to a source document. Mirrors SDK `SourceDocRef`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceDocRef {
    pub provider: String,
    pub doc_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// A reference to a pull request. Mirrors SDK `PrRef`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrRef {
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    pub number: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// The suggested corpus update for a correction finding. Mirrors SDK `SuggestedAction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SuggestedAction {
    UpdateSourceDoc,
    UpdateDecision,
    UpdateConstraint,
    NoAction,
}

/// A correction-capture finding: a changed code region may have invalidated a
/// grounded decision/constraint/source-doc. Mirrors SDK `CorrectionFinding`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorrectionFinding {
    pub id: Uuid,
    pub summary: String,
    pub code_region: CodeRegionRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_doc: Option<SourceDocRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr: Option<PrRef>,
    #[serde(default)]
    pub related_decision_ids: Vec<Uuid>,
    #[serde(default)]
    pub related_constraint_ids: Vec<Uuid>,
    #[serde(default)]
    pub evidence_refs: Vec<Uuid>,
    /// Confidence in basis points (0..=10000); integer-faithful mirror of the
    /// SDK 0..1 float, keeping the carrier `Eq`-friendly.
    pub confidence_bps: u16,
    pub severity: FindingSeverity,
    pub review_state: LifecycleReviewState,
    pub suggested_action: SuggestedAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported_by: Option<Attribution>,
    pub detected_at: DateTime<Utc>,
}

/// Whether two code regions overlap. Exact port of SDK `regionOverlaps`.
///
/// Deterministic update trigger: regions overlap iff they name the same
/// provider/repo/path and their line ranges intersect (a missing range on
/// either side means the whole file).
pub fn region_overlaps(a: &CodeRegionRef, b: &CodeRegionRef) -> bool {
    if a.provider != b.provider || a.repo != b.repo || a.path != b.path {
        return false;
    }
    match (a.start_line, a.end_line, b.start_line, b.end_line) {
        (Some(a_start), Some(a_end), Some(b_start), Some(b_end)) => {
            a_start <= b_end && b_start <= a_end
        }
        _ => true,
    }
}

/// Whether a finding is actionable: severe (high/critical) AND confident.
/// Exact port of SDK `isActionable`.
pub fn is_actionable(finding: &CorrectionFinding, threshold_bps: u16) -> bool {
    matches!(
        finding.severity,
        FindingSeverity::High | FindingSeverity::Critical
    ) && finding.confidence_bps >= threshold_bps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(path: &str, start: Option<u32>, end: Option<u32>) -> CodeRegionRef {
        CodeRegionRef {
            provider: "github".to_string(),
            repo: Some("BicameralAI/bicameral-bot".to_string()),
            path: path.to_string(),
            start_line: start,
            end_line: end,
            git_ref: Some("main".to_string()),
            url: None,
        }
    }

    fn finding(severity: FindingSeverity, confidence_bps: u16) -> CorrectionFinding {
        CorrectionFinding {
            id: Uuid::new_v4(),
            summary: "code region changed; product doc may be stale".to_string(),
            code_region: region("src/lib.rs", Some(10), Some(20)),
            source_doc: Some(SourceDocRef {
                provider: "gdrive".to_string(),
                doc_id: "doc-1".to_string(),
                section: Some("Auth".to_string()),
                url: None,
            }),
            pr: None,
            related_decision_ids: vec![Uuid::new_v4()],
            related_constraint_ids: Vec::new(),
            evidence_refs: Vec::new(),
            confidence_bps,
            severity,
            review_state: LifecycleReviewState::Proposed,
            suggested_action: SuggestedAction::UpdateSourceDoc,
            reported_by: None,
            detected_at: Utc::now(),
        }
    }

    #[test]
    fn suggested_action_round_trips_snake_case() {
        for (v, j) in [
            (SuggestedAction::UpdateSourceDoc, "\"update_source_doc\""),
            (SuggestedAction::UpdateDecision, "\"update_decision\""),
            (SuggestedAction::UpdateConstraint, "\"update_constraint\""),
            (SuggestedAction::NoAction, "\"no_action\""),
        ] {
            assert_eq!(serde_json::to_string(&v).unwrap(), j);
            assert_eq!(serde_json::from_str::<SuggestedAction>(j).unwrap(), v);
        }
    }

    #[test]
    fn suggested_action_inverse_coverage() {
        assert!(serde_json::from_str::<SuggestedAction>("\"delete\"").is_err());
        assert!(serde_json::from_str::<SuggestedAction>("\"UpdateSourceDoc\"").is_err());
    }

    #[test]
    fn region_overlaps_same_path_intersecting_ranges() {
        assert!(region_overlaps(
            &region("src/lib.rs", Some(10), Some(20)),
            &region("src/lib.rs", Some(15), Some(25))
        ));
        assert!(!region_overlaps(
            &region("src/lib.rs", Some(10), Some(20)),
            &region("src/lib.rs", Some(21), Some(30))
        ));
    }

    #[test]
    fn region_overlaps_different_path_never() {
        assert!(!region_overlaps(
            &region("src/a.rs", Some(1), Some(5)),
            &region("src/b.rs", Some(1), Some(5))
        ));
    }

    #[test]
    fn region_overlaps_missing_range_means_whole_file() {
        assert!(region_overlaps(
            &region("src/lib.rs", None, None),
            &region("src/lib.rs", Some(100), Some(200))
        ));
    }

    #[test]
    fn region_overlaps_different_repo_never() {
        let mut a = region("src/lib.rs", Some(1), Some(5));
        let mut b = region("src/lib.rs", Some(1), Some(5));
        a.repo = Some("org/x".to_string());
        b.repo = Some("org/y".to_string());
        assert!(!region_overlaps(&a, &b));
    }

    #[test]
    fn is_actionable_requires_severe_and_confident() {
        assert!(is_actionable(&finding(FindingSeverity::High, 9000), 8000));
        assert!(is_actionable(
            &finding(FindingSeverity::Critical, 8000),
            8000
        ));
        // Severe but not confident.
        assert!(!is_actionable(&finding(FindingSeverity::High, 5000), 8000));
        // Confident but not severe.
        assert!(!is_actionable(&finding(FindingSeverity::Low, 9000), 8000));
        assert!(!is_actionable(
            &finding(FindingSeverity::Medium, 9000),
            8000
        ));
    }

    #[test]
    fn correction_finding_round_trips_json() {
        let f = finding(FindingSeverity::High, 8800);
        let back: CorrectionFinding =
            serde_json::from_value(serde_json::to_value(&f).unwrap()).unwrap();
        assert_eq!(back, f);
    }
}
