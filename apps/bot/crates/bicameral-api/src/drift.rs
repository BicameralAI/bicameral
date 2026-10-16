//! Contradiction & drift finding carriers and the HITL review status machine.
//!
//! A [`ContradictionReport`] records a candidate contradiction between accepted
//! constraints and moves through a human-in-the-loop status machine
//! (`open → acknowledged → resolved/dismissed`). Status never advances
//! automatically — only an explicit [`ReviewTransitionRequest`] applied via
//! [`transition_finding`] can change it, and invalid transitions are rejected.
//!
//! Mirrors the **shape and semantics** of the SDK `drift`
//! (`bicameral-sdk/src/drift/index.ts`), serialized snake_case per bot
//! convention. Detection itself lives in the runtime (`drift_detector`); this
//! module owns the carriers and the pure transition logic.

use crate::constraint::Attribution;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Severity of a finding. Mirrors SDK `FindingSeverity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// Review lifecycle state of a finding. Mirrors SDK `FindingStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FindingStatus {
    Open,
    Acknowledged,
    Resolved,
    Dismissed,
}

/// A drift signal observed against a work surface. Mirrors SDK `DriftSignal`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriftSignal {
    pub id: Uuid,
    pub surface_id: String,
    pub summary: String,
    pub severity: FindingSeverity,
    #[serde(default)]
    pub evidence_refs: Vec<Uuid>,
    pub detected_at: DateTime<Utc>,
}

/// A candidate contradiction between accepted constraints, routed to review.
///
/// Mirrors the SDK `ContradictionReport`. Note: the SDK contract has **no**
/// `confidence` field — this carrier follows the SDK, not the issue prose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContradictionReport {
    pub id: Uuid,
    pub summary: String,
    pub severity: FindingSeverity,
    pub status: FindingStatus,
    #[serde(default)]
    pub related_constraint_ids: Vec<Uuid>,
    #[serde(default)]
    pub evidence_refs: Vec<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_resolution: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_recommendation: Option<Attribution>,
    pub detected_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A request to transition a finding's status. Mirrors SDK `ReviewTransitionRequest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewTransitionRequest {
    pub report_id: Uuid,
    pub to_status: FindingStatus,
    pub actor: Attribution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub at: DateTime<Utc>,
}

/// Why a status transition was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TransitionErrorCode {
    InvalidTransition,
}

/// A rejected status transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct TransitionError {
    pub code: TransitionErrorCode,
    pub message: String,
}

/// Whether a finding may move from `from` to `to`. Exact port of the SDK
/// `ALLOWED` map: `open → {acknowledged, dismissed}`,
/// `acknowledged → {resolved, dismissed}`, `resolved`/`dismissed` terminal.
pub fn can_transition(from: FindingStatus, to: FindingStatus) -> bool {
    matches!(
        (from, to),
        (FindingStatus::Open, FindingStatus::Acknowledged)
            | (FindingStatus::Open, FindingStatus::Dismissed)
            | (FindingStatus::Acknowledged, FindingStatus::Resolved)
            | (FindingStatus::Acknowledged, FindingStatus::Dismissed)
    )
}

/// Apply a human-driven status transition. Exact port of SDK `transitionFinding`.
///
/// Immutable: returns a new report on success; the input is never mutated and a
/// finding's status never advances automatically. Invalid transitions are
/// rejected with [`TransitionErrorCode::InvalidTransition`].
pub fn transition_finding(
    report: &ContradictionReport,
    request: &ReviewTransitionRequest,
) -> Result<ContradictionReport, TransitionError> {
    if !can_transition(report.status, request.to_status) {
        return Err(TransitionError {
            code: TransitionErrorCode::InvalidTransition,
            message: format!(
                "Cannot transition {:?} -> {:?}.",
                report.status, request.to_status
            ),
        });
    }
    Ok(ContradictionReport {
        status: request.to_status,
        updated_at: request.at,
        ..report.clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraint::ActorType;

    fn actor() -> Attribution {
        Attribution {
            actor_id: "rev".to_string(),
            actor_type: ActorType::Human,
            display_name: None,
        }
    }

    fn report(status: FindingStatus) -> ContradictionReport {
        let at = Utc::now();
        ContradictionReport {
            id: Uuid::new_v4(),
            summary: "overlap on api/auth".to_string(),
            severity: FindingSeverity::High,
            status,
            related_constraint_ids: vec![Uuid::new_v4(), Uuid::new_v4()],
            evidence_refs: Vec::new(),
            suggested_resolution: None,
            owner_recommendation: None,
            detected_at: at,
            updated_at: at,
        }
    }

    #[test]
    fn finding_severity_round_trips_snake_case() {
        for (v, j) in [
            (FindingSeverity::Low, "\"low\""),
            (FindingSeverity::Medium, "\"medium\""),
            (FindingSeverity::High, "\"high\""),
            (FindingSeverity::Critical, "\"critical\""),
        ] {
            assert_eq!(serde_json::to_string(&v).unwrap(), j);
            assert_eq!(serde_json::from_str::<FindingSeverity>(j).unwrap(), v);
        }
    }

    #[test]
    fn finding_severity_inverse_coverage() {
        assert!(serde_json::from_str::<FindingSeverity>("\"urgent\"").is_err());
        assert!(serde_json::from_str::<FindingSeverity>("\"Low\"").is_err());
    }

    #[test]
    fn finding_status_round_trips_snake_case() {
        for (v, j) in [
            (FindingStatus::Open, "\"open\""),
            (FindingStatus::Acknowledged, "\"acknowledged\""),
            (FindingStatus::Resolved, "\"resolved\""),
            (FindingStatus::Dismissed, "\"dismissed\""),
        ] {
            assert_eq!(serde_json::to_string(&v).unwrap(), j);
            assert_eq!(serde_json::from_str::<FindingStatus>(j).unwrap(), v);
        }
    }

    #[test]
    fn finding_status_inverse_coverage() {
        assert!(serde_json::from_str::<FindingStatus>("\"closed\"").is_err());
        assert!(serde_json::from_str::<FindingStatus>("\"Open\"").is_err());
    }

    #[test]
    fn can_transition_full_matrix() {
        use FindingStatus::*;
        // Legal.
        assert!(can_transition(Open, Acknowledged));
        assert!(can_transition(Open, Dismissed));
        assert!(can_transition(Acknowledged, Resolved));
        assert!(can_transition(Acknowledged, Dismissed));
        // Illegal (incl. terminal states rejecting all).
        assert!(!can_transition(Open, Resolved));
        assert!(!can_transition(Open, Open));
        assert!(!can_transition(Acknowledged, Open));
        assert!(!can_transition(Resolved, Open));
        assert!(!can_transition(Resolved, Acknowledged));
        assert!(!can_transition(Resolved, Dismissed));
        assert!(!can_transition(Dismissed, Acknowledged));
        assert!(!can_transition(Dismissed, Resolved));
    }

    #[test]
    fn transition_finding_valid_updates_status_immutably() {
        let r = report(FindingStatus::Open);
        let req = ReviewTransitionRequest {
            report_id: r.id,
            to_status: FindingStatus::Acknowledged,
            actor: actor(),
            note: Some("looks real".to_string()),
            at: Utc::now(),
        };
        let updated = transition_finding(&r, &req).unwrap();
        assert_eq!(updated.status, FindingStatus::Acknowledged);
        assert_eq!(updated.updated_at, req.at);
        // Input unchanged.
        assert_eq!(r.status, FindingStatus::Open);
    }

    #[test]
    fn transition_finding_invalid_returns_error() {
        let r = report(FindingStatus::Resolved);
        let req = ReviewTransitionRequest {
            report_id: r.id,
            to_status: FindingStatus::Open,
            actor: actor(),
            note: None,
            at: Utc::now(),
        };
        let err = transition_finding(&r, &req).unwrap_err();
        assert_eq!(err.code, TransitionErrorCode::InvalidTransition);
        assert_eq!(r.status, FindingStatus::Resolved);
    }
}
