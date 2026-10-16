//! Pre-flight context resolution contract.
//!
//! A [`ContextPacket`] surfaces the most relevant canonical constraints for a
//! work surface (ticket/branch/pr/session) before work begins, ranked by
//! `relevance * confidence`. This module owns the SDK-mirrored contract shapes
//! plus the pure ranking primitives; the runtime owns the scoring model and the
//! resolution over replayed catalog state (`context_resolver`).
//!
//! Mirrors the **shape and semantics** of the SDK `preflight`
//! (`bicameral-sdk/src/preflight/index.ts`), serialized snake_case per bot
//! convention (the bot has never matched the SDK's camelCase wire keys). This is
//! a read-only retrieval surface — it is parallel to, and does not modify, the
//! ADR-0019 `preflight.rs` decision query.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use uuid::Uuid;

/// The kind of work surface context is requested for. Mirrors SDK `WorkSurfaceKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkSurfaceKind {
    Ticket,
    Branch,
    Pr,
    Session,
}

/// A reference to the work surface. Mirrors SDK `WorkSurfaceRef`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkSurfaceRef {
    pub kind: WorkSurfaceKind,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// A request for pre-flight context. Mirrors SDK `ContextRequest`.
///
/// `query` and `scopes` are free-form retrieval inputs (a file path, symbol,
/// repo, or search term lives here — not as a `WorkSurfaceKind`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextRequest {
    pub surface: WorkSurfaceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_results: Option<usize>,
}

/// A scored, surfaced constraint. Mirrors SDK `ContextItem`.
///
/// Carries no `Eq`/`Hash` — `relevance`/`risk`/`confidence` are `f64`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextItem {
    pub constraint_id: Uuid,
    pub statement: String,
    pub relevance: f64,
    pub risk: f64,
    pub confidence: f64,
    #[serde(default)]
    pub affected_surfaces: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<Uuid>,
}

/// A summary of the evidence behind a packet. Mirrors SDK `EvidenceSummary`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceSummary {
    pub evidence_count: usize,
    #[serde(default)]
    pub sources: Vec<String>,
}

/// The resolved pre-flight context. Mirrors SDK `ContextPacket`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPacket {
    pub surface: WorkSurfaceRef,
    pub items: Vec<ContextItem>,
    pub evidence_summary: EvidenceSummary,
    pub generated_at: DateTime<Utc>,
}

/// Priority key: `relevance * confidence`. Mirrors SDK `priority`.
fn priority(item: &ContextItem) -> f64 {
    item.relevance * item.confidence
}

/// Rank items by `relevance * confidence` (desc), tie-broken by `risk` (desc).
///
/// Pure and immutable: returns a new `Vec` ordered so the highest-priority
/// constraints surface first. Exact port of SDK `rankContextItems`.
pub fn rank_context_items(items: &[ContextItem]) -> Vec<ContextItem> {
    let mut ranked = items.to_vec();
    ranked.sort_by(|a, b| {
        priority(b)
            .partial_cmp(&priority(a))
            .unwrap_or(Ordering::Equal)
            .then(b.risk.partial_cmp(&a.risk).unwrap_or(Ordering::Equal))
    });
    ranked
}

/// Whether an item clears a confidence threshold. Mirrors SDK `meetsConfidence`.
pub fn meets_confidence(item: &ContextItem, threshold: f64) -> bool {
    item.confidence >= threshold
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(relevance: f64, risk: f64, confidence: f64) -> ContextItem {
        ContextItem {
            constraint_id: Uuid::new_v4(),
            statement: "S".to_string(),
            relevance,
            risk,
            confidence,
            affected_surfaces: Vec::new(),
            evidence_refs: Vec::new(),
        }
    }

    #[test]
    fn work_surface_kind_round_trips_snake_case() {
        let cases = [
            (WorkSurfaceKind::Ticket, "\"ticket\""),
            (WorkSurfaceKind::Branch, "\"branch\""),
            (WorkSurfaceKind::Pr, "\"pr\""),
            (WorkSurfaceKind::Session, "\"session\""),
        ];
        for (variant, json) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), json);
            assert_eq!(
                serde_json::from_str::<WorkSurfaceKind>(json).unwrap(),
                variant
            );
        }
    }

    #[test]
    fn work_surface_kind_inverse_coverage() {
        assert!(serde_json::from_str::<WorkSurfaceKind>("\"commit\"").is_err());
        assert!(serde_json::from_str::<WorkSurfaceKind>("\"Pr\"").is_err());
    }

    #[test]
    fn rank_orders_by_relevance_confidence_then_risk() {
        // A: priority 0.9*0.9=0.81. B: priority 1.0*0.5=0.50 (higher raw
        // relevance, lower priority). C: priority 0.81, but lower risk than A.
        let a = item(0.9, 0.8, 0.9);
        let b = item(1.0, 0.9, 0.5);
        let c = item(0.9, 0.2, 0.9);
        let ranked = rank_context_items(&[b.clone(), c.clone(), a.clone()]);
        // A and C tie on priority (0.81); A wins on risk (0.8 > 0.2). B last.
        assert_eq!(ranked[0].risk, a.risk);
        assert_eq!(ranked[1].risk, c.risk);
        assert_eq!(ranked[2].relevance, b.relevance);
    }

    #[test]
    fn rank_is_immutable() {
        let input = vec![item(0.2, 0.1, 0.3), item(0.9, 0.5, 0.9)];
        let _ = rank_context_items(&input);
        // Input order unchanged.
        assert_eq!(input[0].relevance, 0.2);
        assert_eq!(input[1].relevance, 0.9);
    }

    #[test]
    fn meets_confidence_threshold() {
        let it = item(0.5, 0.5, 0.6);
        assert!(meets_confidence(&it, 0.6));
        assert!(meets_confidence(&it, 0.5));
        assert!(!meets_confidence(&it, 0.61));
    }
}
