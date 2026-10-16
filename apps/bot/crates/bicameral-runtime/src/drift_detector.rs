//! Read-only contradiction detector (#484).
//!
//! `detect_contradictions` surfaces *candidate* contradictions between accepted
//! catalog constraints that share a work surface. It is **read-only**: it emits
//! no events and mutates no state (consistent with the preflight doctrine in
//! `preflight.rs`). Persisting a finding and writing the `conflicts_with` edges
//! happens only through the explicit `RaiseContradiction` event (see `replay`).
//!
//! Detection is deterministic and structural (surface overlap + type-derived
//! severity). Semantic / natural-language contradiction analysis is GH #485.

use crate::replay::ReplayedState;
use bicameral_api::catalog::{CatalogConstraint, ConstraintType};
use bicameral_api::constraint::CanonicalReviewState;
use bicameral_api::drift::{ContradictionReport, FindingSeverity, FindingStatus};
use chrono::Utc;
use uuid::Uuid;

/// Rank a constraint type by how much risk a contradiction on it carries.
fn type_rank(t: ConstraintType) -> u8 {
    match t {
        ConstraintType::Technical => 0,
        ConstraintType::Product | ConstraintType::Business => 1,
        ConstraintType::Security => 2,
        ConstraintType::Compliance => 3,
    }
}

/// Severity of a contradiction between two constraints: the higher-risk type.
pub fn severity_for(a: ConstraintType, b: ConstraintType) -> FindingSeverity {
    match type_rank(a).max(type_rank(b)) {
        0 => FindingSeverity::Low,
        1 => FindingSeverity::Medium,
        2 => FindingSeverity::High,
        _ => FindingSeverity::Critical,
    }
}

/// Whether two constraints share at least one affected surface.
fn surfaces_overlap(a: &CatalogConstraint, b: &CatalogConstraint) -> bool {
    a.affected_surfaces
        .iter()
        .any(|s| b.affected_surfaces.contains(s))
}

/// Detect candidate contradictions across the accepted catalog (read-only).
///
/// Each pair of `Accepted` constraints sharing ≥1 affected surface yields one
/// `Open` [`ContradictionReport`]. Pairs are taken once (`i < j` over
/// id-sorted constraints), so there are no self-pairs or duplicates.
pub fn detect_contradictions(state: &ReplayedState) -> Vec<ContradictionReport> {
    let mut accepted: Vec<&CatalogConstraint> = state
        .catalog
        .values()
        .filter(|c| c.review_state == CanonicalReviewState::Accepted)
        .collect();
    accepted.sort_by_key(|c| c.id);

    let mut reports = Vec::new();
    for i in 0..accepted.len() {
        for j in (i + 1)..accepted.len() {
            let a = accepted[i];
            let b = accepted[j];
            if !surfaces_overlap(a, b) {
                continue;
            }
            let shared: Vec<&String> = a
                .affected_surfaces
                .iter()
                .filter(|s| b.affected_surfaces.contains(s))
                .collect();
            let mut evidence_refs = a.evidence_refs.clone();
            for e in &b.evidence_refs {
                if !evidence_refs.contains(e) {
                    evidence_refs.push(*e);
                }
            }
            let now = Utc::now();
            reports.push(ContradictionReport {
                id: Uuid::new_v4(),
                summary: format!(
                    "Constraints {} and {} co-locate on surface(s): {}",
                    a.id,
                    b.id,
                    shared
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                severity: severity_for(a.constraint_type, b.constraint_type),
                status: FindingStatus::Open,
                related_constraint_ids: vec![a.id, b.id],
                evidence_refs,
                suggested_resolution: None,
                owner_recommendation: None,
                detected_at: now,
                updated_at: now,
            });
        }
    }
    reports
}

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_api::catalog::ConstraintScope;
    use bicameral_api::constraint::{ActorType, Attribution};

    fn cat(
        constraint_type: ConstraintType,
        review_state: CanonicalReviewState,
        surfaces: &[&str],
    ) -> CatalogConstraint {
        let at = Utc::now();
        CatalogConstraint {
            id: Uuid::new_v4(),
            decision_id: Uuid::new_v4(),
            statement: "S".to_string(),
            constraint_type,
            scope: ConstraintScope::Service,
            review_state,
            version: 1,
            affected_surfaces: surfaces.iter().map(|s| s.to_string()).collect(),
            evidence_refs: Vec::new(),
            attribution: Attribution {
                actor_id: "a".to_string(),
                actor_type: ActorType::Human,
                display_name: None,
            },
            supersedes: None,
            superseded_by: None,
            conflicts_with: Vec::new(),
            created_at: at,
            updated_at: at,
        }
    }

    fn state_with(cs: Vec<CatalogConstraint>) -> ReplayedState {
        let mut state = ReplayedState::default();
        for (i, c) in cs.into_iter().enumerate() {
            state.catalog.insert(format!("k{i}"), c);
        }
        state
    }

    #[test]
    fn detects_overlapping_accepted_pair() {
        let a = cat(
            ConstraintType::Security,
            CanonicalReviewState::Accepted,
            &["api/auth"],
        );
        let b = cat(
            ConstraintType::Product,
            CanonicalReviewState::Accepted,
            &["api/auth", "api/other"],
        );
        let (ia, ib) = (a.id, b.id);
        let reports = detect_contradictions(&state_with(vec![a, b]));
        assert_eq!(reports.len(), 1);
        let mut ids = reports[0].related_constraint_ids.clone();
        ids.sort();
        let mut expected = vec![ia, ib];
        expected.sort();
        assert_eq!(ids, expected);
    }

    #[test]
    fn ignores_non_overlapping_pair() {
        let a = cat(
            ConstraintType::Security,
            CanonicalReviewState::Accepted,
            &["api/auth"],
        );
        let b = cat(
            ConstraintType::Product,
            CanonicalReviewState::Accepted,
            &["api/billing"],
        );
        assert!(detect_contradictions(&state_with(vec![a, b])).is_empty());
    }

    #[test]
    fn ignores_non_accepted() {
        let a = cat(
            ConstraintType::Security,
            CanonicalReviewState::Accepted,
            &["api/auth"],
        );
        let b = cat(
            ConstraintType::Product,
            CanonicalReviewState::Superseded,
            &["api/auth"],
        );
        assert!(detect_contradictions(&state_with(vec![a, b])).is_empty());
    }

    #[test]
    fn severity_from_higher_risk_type() {
        assert_eq!(
            severity_for(ConstraintType::Security, ConstraintType::Technical),
            FindingSeverity::High
        );
        assert_eq!(
            severity_for(ConstraintType::Compliance, ConstraintType::Product),
            FindingSeverity::Critical
        );
        assert_eq!(
            severity_for(ConstraintType::Technical, ConstraintType::Technical),
            FindingSeverity::Low
        );
    }

    #[test]
    fn no_self_pair_and_deterministic() {
        let a = cat(
            ConstraintType::Security,
            CanonicalReviewState::Accepted,
            &["api/auth"],
        );
        // Single constraint → no pair.
        assert!(detect_contradictions(&state_with(vec![a])).is_empty());

        // Same catalog → same report count across runs.
        let x = cat(
            ConstraintType::Security,
            CanonicalReviewState::Accepted,
            &["s"],
        );
        let y = cat(
            ConstraintType::Product,
            CanonicalReviewState::Accepted,
            &["s"],
        );
        let st = state_with(vec![x, y]);
        assert_eq!(
            detect_contradictions(&st).len(),
            detect_contradictions(&st).len()
        );
        assert_eq!(detect_contradictions(&st).len(), 1);
    }
}
