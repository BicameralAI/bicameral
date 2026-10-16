//! Pre-flight context resolver (#483).
//!
//! Resolves a [`ContextRequest`] against the replayed constraint catalog (#482),
//! surfacing the most relevant **Accepted** canonical constraints as a ranked
//! [`ContextPacket`]. Read-only: it reads `ReplayedState.catalog` and emits no
//! events and mutates no state — consistent with the ADR-0019 preflight doctrine
//! (`preflight.rs`). It is parallel to, and does not modify, that decision query.
//!
//! The scoring model is intentionally minimal and deterministic (no LLM):
//! relevance from query/scope overlap, risk from constraint type + scope,
//! confidence from evidence count.

use crate::replay::ReplayedState;
use bicameral_api::catalog::{CatalogConstraint, ConstraintScope, ConstraintType};
use bicameral_api::constraint::CanonicalReviewState;
use bicameral_api::context::{
    rank_context_items, ContextItem, ContextPacket, ContextRequest, EvidenceSummary,
};
use chrono::Utc;
use std::collections::BTreeSet;

/// Default cap on surfaced items when the request omits `max_results`.
pub const DEFAULT_MAX_RESULTS: usize = 20;

/// Relevance of a constraint to a request: query/scope overlap. Range `[0, 1]`.
///
/// With no filter (no query, no scopes) every constraint gets a broad floor so
/// the caller still sees the catalog. With a filter, only scope/query hits score
/// above zero — non-matches score exactly `0.0` and are dropped as noise.
pub fn score_relevance(c: &CatalogConstraint, req: &ContextRequest) -> f64 {
    if req.query.is_none() && req.scopes.is_empty() {
        return 0.4;
    }
    let mut score: f64 = 0.0;
    if req.scopes.iter().any(|s| c.affected_surfaces.contains(s)) {
        score += 0.4;
    }
    if let Some(query) = &req.query {
        if c.statement.to_lowercase().contains(&query.to_lowercase()) {
            score += 0.3;
        }
    }
    score.clamp(0.0, 1.0)
}

/// Risk a constraint carries: higher for security/compliance and global scope.
pub fn score_risk(c: &CatalogConstraint) -> f64 {
    let base: f64 = match c.constraint_type {
        ConstraintType::Security | ConstraintType::Compliance => 0.9,
        ConstraintType::Business => 0.6,
        ConstraintType::Product => 0.5,
        ConstraintType::Technical => 0.4,
    };
    let scope_bonus = if c.scope == ConstraintScope::Global {
        0.1
    } else {
        0.0
    };
    (base + scope_bonus).clamp(0.0, 1.0)
}

/// Confidence in a constraint, from how much evidence supports it.
pub fn score_confidence(c: &CatalogConstraint) -> f64 {
    match c.evidence_refs.len() {
        0 => 0.3,
        1 => 0.6,
        _ => 0.9,
    }
}

fn to_item(c: &CatalogConstraint, req: &ContextRequest) -> ContextItem {
    ContextItem {
        constraint_id: c.id,
        statement: c.statement.clone(),
        relevance: score_relevance(c, req),
        risk: score_risk(c),
        confidence: score_confidence(c),
        affected_surfaces: c.affected_surfaces.clone(),
        evidence_refs: c.evidence_refs.clone(),
    }
}

/// Resolve pre-flight context: rank the most relevant Accepted constraints.
pub fn resolve_context(state: &ReplayedState, req: &ContextRequest) -> ContextPacket {
    let filtered = req.query.is_some() || !req.scopes.is_empty();

    let mut items: Vec<ContextItem> = state
        .catalog
        .values()
        .filter(|c| c.review_state == CanonicalReviewState::Accepted)
        .map(|c| to_item(c, req))
        // Noise suppression: drop zero-relevance items when a filter is present.
        .filter(|item| !filtered || item.relevance > 0.0)
        .collect();

    items = rank_context_items(&items);
    let max = req.max_results.unwrap_or(DEFAULT_MAX_RESULTS);
    items.truncate(max);

    let sources: BTreeSet<String> = items
        .iter()
        .flat_map(|item| item.evidence_refs.iter().map(|e| e.to_string()))
        .collect();
    let evidence_summary = EvidenceSummary {
        evidence_count: sources.len(),
        sources: sources.into_iter().collect(),
    };

    ContextPacket {
        surface: req.surface.clone(),
        items,
        evidence_summary,
        generated_at: Utc::now(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_api::constraint::{ActorType, Attribution};
    use bicameral_api::context::{WorkSurfaceKind, WorkSurfaceRef};
    use uuid::Uuid;

    fn attribution() -> Attribution {
        Attribution {
            actor_id: "a".to_string(),
            actor_type: ActorType::Human,
            display_name: None,
        }
    }

    fn cat(
        constraint_type: ConstraintType,
        scope: ConstraintScope,
        review_state: CanonicalReviewState,
        surfaces: &[&str],
        evidence: usize,
    ) -> CatalogConstraint {
        let at = Utc::now();
        CatalogConstraint {
            id: Uuid::new_v4(),
            decision_id: Uuid::new_v4(),
            statement: "constraint statement".to_string(),
            constraint_type,
            scope,
            review_state,
            version: 1,
            affected_surfaces: surfaces.iter().map(|s| s.to_string()).collect(),
            evidence_refs: (0..evidence).map(|_| Uuid::new_v4()).collect(),
            attribution: attribution(),
            supersedes: None,
            superseded_by: None,
            conflicts_with: Vec::new(),
            created_at: at,
            updated_at: at,
        }
    }

    fn state_with(constraints: Vec<CatalogConstraint>) -> ReplayedState {
        let mut state = ReplayedState::default();
        for (i, c) in constraints.into_iter().enumerate() {
            state.catalog.insert(format!("k{i}"), c);
        }
        state
    }

    fn request(query: Option<&str>, scopes: &[&str], max: Option<usize>) -> ContextRequest {
        ContextRequest {
            surface: WorkSurfaceRef {
                kind: WorkSurfaceKind::Branch,
                id: "feat/x".to_string(),
                url: None,
            },
            query: query.map(|q| q.to_string()),
            scopes: scopes.iter().map(|s| s.to_string()).collect(),
            max_results: max,
        }
    }

    #[test]
    fn resolve_surfaces_only_accepted_constraints() {
        let state = state_with(vec![
            cat(
                ConstraintType::Product,
                ConstraintScope::Global,
                CanonicalReviewState::Accepted,
                &[],
                1,
            ),
            cat(
                ConstraintType::Product,
                ConstraintScope::Global,
                CanonicalReviewState::Superseded,
                &[],
                1,
            ),
            cat(
                ConstraintType::Product,
                ConstraintScope::Global,
                CanonicalReviewState::Deprecated,
                &[],
                1,
            ),
        ]);
        let packet = resolve_context(&state, &request(None, &[], None));
        assert_eq!(
            packet.items.len(),
            1,
            "only the Accepted constraint surfaces"
        );
    }

    #[test]
    fn resolve_scores_and_ranks_highest_priority_first() {
        // High: security/global, 2 evidence → high risk + confidence.
        let high = cat(
            ConstraintType::Security,
            ConstraintScope::Global,
            CanonicalReviewState::Accepted,
            &[],
            2,
        );
        // Low: technical/feature, 0 evidence → low confidence.
        let low = cat(
            ConstraintType::Technical,
            ConstraintScope::Feature,
            CanonicalReviewState::Accepted,
            &[],
            0,
        );
        let high_id = high.id;
        let state = state_with(vec![low, high]);
        let packet = resolve_context(&state, &request(None, &[], None));
        assert_eq!(packet.items.len(), 2);
        assert_eq!(
            packet.items[0].constraint_id, high_id,
            "higher relevance*confidence ranks first"
        );
    }

    #[test]
    fn resolve_respects_max_results() {
        // Make one constraint unambiguously top-priority (2 evidence → 0.9
        // confidence) so HashMap order cannot cause a tie flake.
        let top = cat(
            ConstraintType::Security,
            ConstraintScope::Global,
            CanonicalReviewState::Accepted,
            &[],
            2,
        );
        let top_id = top.id;
        let state = state_with(vec![
            cat(
                ConstraintType::Technical,
                ConstraintScope::Feature,
                CanonicalReviewState::Accepted,
                &[],
                0,
            ),
            top,
            cat(
                ConstraintType::Product,
                ConstraintScope::Feature,
                CanonicalReviewState::Accepted,
                &[],
                0,
            ),
        ]);
        let packet = resolve_context(&state, &request(None, &[], Some(1)));
        assert_eq!(packet.items.len(), 1);
        assert_eq!(packet.items[0].constraint_id, top_id);
    }

    #[test]
    fn resolve_drops_zero_relevance_when_filtered() {
        // Constraint affects "api/auth"; a scope filter for "api/billing"
        // matches nothing → zero relevance → dropped.
        let state = state_with(vec![cat(
            ConstraintType::Security,
            ConstraintScope::Service,
            CanonicalReviewState::Accepted,
            &["api/auth"],
            2,
        )]);
        let packet = resolve_context(&state, &request(None, &["api/billing"], None));
        assert!(
            packet.items.is_empty(),
            "a non-matching filter suppresses the constraint as noise"
        );
        // The same constraint surfaces when the scope matches.
        let packet2 = resolve_context(&state, &request(None, &["api/auth"], None));
        assert_eq!(packet2.items.len(), 1);
    }

    #[test]
    fn resolve_builds_evidence_summary() {
        let state = state_with(vec![cat(
            ConstraintType::Product,
            ConstraintScope::Global,
            CanonicalReviewState::Accepted,
            &[],
            2,
        )]);
        let packet = resolve_context(&state, &request(None, &[], None));
        assert_eq!(packet.items.len(), 1);
        assert_eq!(
            packet.evidence_summary.evidence_count, 2,
            "two distinct evidence refs"
        );
        assert_eq!(packet.evidence_summary.sources.len(), 2);
    }

    #[test]
    fn score_risk_higher_for_security_than_technical() {
        let sec = cat(
            ConstraintType::Security,
            ConstraintScope::Service,
            CanonicalReviewState::Accepted,
            &[],
            0,
        );
        let tech = cat(
            ConstraintType::Technical,
            ConstraintScope::Service,
            CanonicalReviewState::Accepted,
            &[],
            0,
        );
        assert!(score_risk(&sec) > score_risk(&tech));
    }

    #[test]
    fn score_confidence_rises_with_evidence_count() {
        let zero = cat(
            ConstraintType::Product,
            ConstraintScope::Global,
            CanonicalReviewState::Accepted,
            &[],
            0,
        );
        let one = cat(
            ConstraintType::Product,
            ConstraintScope::Global,
            CanonicalReviewState::Accepted,
            &[],
            1,
        );
        let two = cat(
            ConstraintType::Product,
            ConstraintScope::Global,
            CanonicalReviewState::Accepted,
            &[],
            2,
        );
        assert!(score_confidence(&zero) < score_confidence(&one));
        assert!(score_confidence(&one) < score_confidence(&two));
    }
}
