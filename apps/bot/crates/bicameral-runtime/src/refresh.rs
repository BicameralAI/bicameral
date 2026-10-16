//! Source refresh eligibility — scopes automatic refresh to tracked Decisions.
//!
//! Per ADR-0022 and issue #190: automatic refresh only considers tracked
//! Decisions by default. Unpromoted candidates and raw source context can be
//! manually refreshed but are not auto-enqueued.
//!
//! The refresh boundary is a read-model query, not an enforcement gate.
//! It answers "which Decisions should the refresh queue consider?" without
//! claiming compliance, correctness, or enforcement from freshness.
//!
//! Per issue #194: code-source refresh delegates to the `code_source_refresh`
//! module which wires to `GraphStore`/`GraphSnapshot`/`GraphSlice` readiness
//! concepts. The refresh entry carries `has_code_bindings` so callers can
//! route code-source refresh through the graph readiness path.

use bicameral_api::candidate::Decision;
use bicameral_api::dashboard::SignoffState;
use uuid::Uuid;

use crate::replay::ReplayedState;

/// Refresh request mode — automatic (daemon-driven) or manual (user-initiated).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshMode {
    /// Automatic refresh — only tracked, non-terminal Decisions are eligible.
    Auto,
    /// Manual refresh — the user explicitly requested a refresh for a specific
    /// Decision or source. Manual refresh bypasses the tracking filter but
    /// still requires the Decision to exist in the ledger.
    Manual,
}

/// A single refresh-eligible entry returned by the query.
#[derive(Debug, Clone)]
pub struct RefreshEntry {
    pub decision_id: Uuid,
    pub title: String,
    pub source_id: Option<String>,
    pub signoff_state: SignoffState,
    /// Whether this Decision has code bindings that should be refreshed
    /// through the graph readiness path (`code_source_refresh` module).
    pub has_code_bindings: bool,
}

/// Query the replayed ledger state for Decisions eligible for source refresh.
///
/// In `Auto` mode, returns only tracked Decisions whose signoff state is
/// not terminally resolved (proposed, approved, or collision-pending).
///
/// In `Manual` mode, returns all Decisions (the caller explicitly asked).
pub fn query_refresh_eligible(state: &ReplayedState, mode: RefreshMode) -> Vec<RefreshEntry> {
    state
        .decisions
        .values()
        .filter(|d| match mode {
            RefreshMode::Auto => d.is_auto_refresh_eligible(),
            RefreshMode::Manual => true,
        })
        .map(|d| {
            let has_code_bindings = state
                .bindings
                .get(&d.id)
                .map(|bs| !bs.is_empty())
                .unwrap_or(false);
            RefreshEntry {
                decision_id: d.id,
                title: d.title.clone(),
                source_id: d.source_id.clone(),
                signoff_state: d.signoff_state,
                has_code_bindings,
            }
        })
        .collect()
}

/// Check whether a single Decision is eligible for automatic refresh.
///
/// Convenience wrapper for callers that already hold a `Decision` reference.
pub fn is_auto_eligible(decision: &Decision) -> bool {
    decision.is_auto_refresh_eligible()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_api::candidate::{DecisionCandidate, DecisionLevel, TrackingState};
    use bicameral_api::dashboard::SignoffState;

    fn make_decision(signoff: SignoffState, tracking: TrackingState) -> Decision {
        let candidate = DecisionCandidate::new(
            "test decision".into(),
            "body".into(),
            DecisionLevel::Product,
            "jira://PROJ-1".into(),
        );
        let mut d = Decision::from_candidate(&candidate, "agent".into());
        d.signoff_state = signoff;
        d.tracking_state = tracking;
        d
    }

    // ── TrackingState.is_auto_refresh_eligible ───────────────────────

    #[test]
    fn tracked_state_is_auto_refresh_eligible() {
        assert!(TrackingState::Tracked.is_auto_refresh_eligible());
    }

    #[test]
    fn untracked_state_is_not_auto_refresh_eligible() {
        assert!(!TrackingState::Untracked.is_auto_refresh_eligible());
    }

    // ── Decision.is_auto_refresh_eligible ────────────────────────────

    #[test]
    fn proposed_tracked_decision_is_eligible() {
        let d = make_decision(SignoffState::Proposed, TrackingState::Tracked);
        assert!(d.is_auto_refresh_eligible());
    }

    #[test]
    fn approved_tracked_decision_is_eligible() {
        let d = make_decision(SignoffState::Approved, TrackingState::Tracked);
        assert!(d.is_auto_refresh_eligible());
    }

    #[test]
    fn collision_pending_tracked_decision_is_eligible() {
        let d = make_decision(SignoffState::CollisionPending, TrackingState::Tracked);
        assert!(d.is_auto_refresh_eligible());
    }

    #[test]
    fn rejected_tracked_decision_is_not_eligible() {
        let d = make_decision(SignoffState::Rejected, TrackingState::Tracked);
        assert!(!d.is_auto_refresh_eligible());
    }

    #[test]
    fn superseded_tracked_decision_is_not_eligible() {
        let d = make_decision(SignoffState::Superseded, TrackingState::Tracked);
        assert!(!d.is_auto_refresh_eligible());
    }

    #[test]
    fn proposed_untracked_decision_is_not_eligible() {
        let d = make_decision(SignoffState::Proposed, TrackingState::Untracked);
        assert!(!d.is_auto_refresh_eligible());
    }

    // ── query_refresh_eligible ───────────────────────────────────────

    fn make_replayed_state(decisions: Vec<Decision>) -> ReplayedState {
        let mut state = ReplayedState::default();
        for d in decisions {
            state.decisions.insert(d.id, d);
        }
        state
    }

    #[test]
    fn auto_mode_returns_only_eligible_decisions() {
        let eligible = make_decision(SignoffState::Proposed, TrackingState::Tracked);
        let ineligible_rejected = make_decision(SignoffState::Rejected, TrackingState::Tracked);
        let ineligible_untracked = make_decision(SignoffState::Proposed, TrackingState::Untracked);

        let state = make_replayed_state(vec![
            eligible.clone(),
            ineligible_rejected,
            ineligible_untracked,
        ]);

        let results = query_refresh_eligible(&state, RefreshMode::Auto);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].decision_id, eligible.id);
    }

    #[test]
    fn manual_mode_returns_all_decisions() {
        let d1 = make_decision(SignoffState::Proposed, TrackingState::Tracked);
        let d2 = make_decision(SignoffState::Rejected, TrackingState::Tracked);
        let d3 = make_decision(SignoffState::Proposed, TrackingState::Untracked);

        let state = make_replayed_state(vec![d1, d2, d3]);

        let results = query_refresh_eligible(&state, RefreshMode::Manual);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn auto_mode_includes_approved_and_collision_pending() {
        let approved = make_decision(SignoffState::Approved, TrackingState::Tracked);
        let collision = make_decision(SignoffState::CollisionPending, TrackingState::Tracked);
        let superseded = make_decision(SignoffState::Superseded, TrackingState::Tracked);

        let state = make_replayed_state(vec![approved.clone(), collision.clone(), superseded]);

        let results = query_refresh_eligible(&state, RefreshMode::Auto);
        assert_eq!(results.len(), 2);
        let ids: Vec<Uuid> = results.iter().map(|r| r.decision_id).collect();
        assert!(ids.contains(&approved.id));
        assert!(ids.contains(&collision.id));
    }

    #[test]
    fn empty_ledger_returns_empty() {
        let state = ReplayedState::default();
        let results = query_refresh_eligible(&state, RefreshMode::Auto);
        assert!(results.is_empty());
    }
}
