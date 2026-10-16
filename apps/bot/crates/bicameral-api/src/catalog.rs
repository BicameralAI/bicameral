//! Canonical constraint catalog read model.
//!
//! A [`CatalogConstraint`] is the queryable, versioned read model materialized
//! from a promoted #481 [`CanonicalConstraint`](crate::constraint::CanonicalConstraint)
//! once it has been classified with a [`ConstraintType`] and [`ConstraintScope`].
//! This module owns the catalog's shape plus the pure query and supersession
//! primitives; the runtime owns the materialized map (built during replay).
//!
//! Mirrors the **shape and semantics** of the SDK `CatalogConstraint`,
//! `ConstraintQuery`, `matchesQuery`, and `supersedeConstraint`
//! (`bicameral-sdk/src/constraints/index.ts`) — serialized snake_case per bot
//! convention (the bot has never matched the SDK's camelCase wire keys). The
//! conflict-edge field `conflicts_with` is storage only; computing it is GH #484.

use crate::constraint::{Attribution, CanonicalReviewState};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// The kind of constraint. Mirrors SDK `ConstraintType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintType {
    Product,
    Business,
    Technical,
    Security,
    Compliance,
}

/// The breadth a constraint applies to. Mirrors SDK `ConstraintScope`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintScope {
    Global,
    Service,
    Feature,
    Customer,
}

/// A versioned, queryable canonical constraint in the catalog read model.
///
/// Mirrors the shape of the SDK `CatalogConstraint` plus the `conflicts_with`
/// storage edge. `constraint_type` is the only field whose SDK key (`type`) is
/// reproduced verbatim — `type` is a Rust keyword.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogConstraint {
    pub id: Uuid,
    /// The reviewed decision this constraint was promoted from (#481 edge).
    pub decision_id: Uuid,
    pub statement: String,
    #[serde(rename = "type")]
    pub constraint_type: ConstraintType,
    pub scope: ConstraintScope,
    pub review_state: CanonicalReviewState,
    pub version: u32,
    #[serde(default)]
    pub affected_surfaces: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<Uuid>,
    pub attribution: Attribution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<Uuid>,
    /// Conflict edges — storage only; computing these is GH #484.
    #[serde(default)]
    pub conflicts_with: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A lookup over the catalog. Mirrors SDK `ConstraintQuery`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConstraintQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub types: Option<Vec<ConstraintType>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<ConstraintScope>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affected_surface: Option<String>,
    #[serde(default)]
    pub include_deprecated: bool,
}

/// The result of a catalog query. Mirrors SDK `ConstraintQueryResult`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintQueryResult {
    pub constraints: Vec<CatalogConstraint>,
    pub total: usize,
}

/// Whether a constraint matches a query. Exact port of SDK `matchesQuery`.
///
/// Deprecated constraints are excluded unless `include_deprecated`. A
/// present-but-empty `types`/`scopes` filter matches nothing (mirrors JS
/// empty-array truthiness: `query.types && !includes`).
pub fn matches_query(c: &CatalogConstraint, q: &ConstraintQuery) -> bool {
    if !q.include_deprecated && c.review_state == CanonicalReviewState::Deprecated {
        return false;
    }
    if let Some(ref types) = q.types {
        if !types.contains(&c.constraint_type) {
            return false;
        }
    }
    if let Some(ref scopes) = q.scopes {
        if !scopes.contains(&c.scope) {
            return false;
        }
    }
    if let Some(ref surface) = q.affected_surface {
        if !c.affected_surfaces.contains(surface) {
            return false;
        }
    }
    true
}

/// Supersede `current` with `replacement`. Exact port of SDK `supersedeConstraint`.
///
/// Pure and immutable: returns new values. The current constraint becomes
/// `Superseded` and points to its replacement; the replacement points back and
/// carries the next version. Catalog truth is versioned, not edited.
pub fn supersede_constraint(
    current: &CatalogConstraint,
    replacement: &CatalogConstraint,
    at: DateTime<Utc>,
) -> (CatalogConstraint, CatalogConstraint) {
    let superseded = CatalogConstraint {
        review_state: CanonicalReviewState::Superseded,
        superseded_by: Some(replacement.id),
        updated_at: at,
        ..current.clone()
    };
    let new_replacement = CatalogConstraint {
        supersedes: Some(current.id),
        version: current.version + 1,
        updated_at: at,
        ..replacement.clone()
    };
    (superseded, new_replacement)
}

/// Run a query over a materialized catalog map (keyed by promotion idempotency key).
pub fn query_catalog(
    catalog: &HashMap<String, CatalogConstraint>,
    q: &ConstraintQuery,
) -> ConstraintQueryResult {
    let constraints: Vec<CatalogConstraint> = catalog
        .values()
        .filter(|c| matches_query(c, q))
        .cloned()
        .collect();
    let total = constraints.len();
    ConstraintQueryResult { constraints, total }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraint::ActorType;

    fn attribution() -> Attribution {
        Attribution {
            actor_id: "rev-1".to_string(),
            actor_type: ActorType::Human,
            display_name: None,
        }
    }

    fn constraint(
        constraint_type: ConstraintType,
        scope: ConstraintScope,
        review_state: CanonicalReviewState,
        surfaces: &[&str],
    ) -> CatalogConstraint {
        let at = Utc::now();
        CatalogConstraint {
            id: Uuid::new_v4(),
            decision_id: Uuid::new_v4(),
            statement: "S".to_string(),
            constraint_type,
            scope,
            review_state,
            version: 1,
            affected_surfaces: surfaces.iter().map(|s| s.to_string()).collect(),
            evidence_refs: Vec::new(),
            attribution: attribution(),
            supersedes: None,
            superseded_by: None,
            conflicts_with: Vec::new(),
            created_at: at,
            updated_at: at,
        }
    }

    #[test]
    fn constraint_type_round_trips_snake_case() {
        let cases = [
            (ConstraintType::Product, "\"product\""),
            (ConstraintType::Business, "\"business\""),
            (ConstraintType::Technical, "\"technical\""),
            (ConstraintType::Security, "\"security\""),
            (ConstraintType::Compliance, "\"compliance\""),
        ];
        for (variant, json) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), json);
            assert_eq!(
                serde_json::from_str::<ConstraintType>(json).unwrap(),
                variant
            );
        }
    }

    #[test]
    fn constraint_type_inverse_coverage() {
        assert!(serde_json::from_str::<ConstraintType>("\"legal\"").is_err());
        assert!(serde_json::from_str::<ConstraintType>("\"Product\"").is_err());
    }

    #[test]
    fn constraint_scope_round_trips_snake_case() {
        let cases = [
            (ConstraintScope::Global, "\"global\""),
            (ConstraintScope::Service, "\"service\""),
            (ConstraintScope::Feature, "\"feature\""),
            (ConstraintScope::Customer, "\"customer\""),
        ];
        for (variant, json) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), json);
            assert_eq!(
                serde_json::from_str::<ConstraintScope>(json).unwrap(),
                variant
            );
        }
    }

    #[test]
    fn constraint_scope_inverse_coverage() {
        assert!(serde_json::from_str::<ConstraintScope>("\"team\"").is_err());
        assert!(serde_json::from_str::<ConstraintScope>("\"Global\"").is_err());
    }

    #[test]
    fn catalog_constraint_renames_type_field() {
        let c = constraint(
            ConstraintType::Product,
            ConstraintScope::Global,
            CanonicalReviewState::Accepted,
            &[],
        );
        let json = serde_json::to_value(&c).unwrap();
        assert!(json.get("type").is_some(), "SDK key 'type' must be present");
        assert!(
            json.get("constraint_type").is_none(),
            "Rust field name must not leak to the wire"
        );
        assert_eq!(json["type"], "product");
    }

    #[test]
    fn matches_query_excludes_deprecated_by_default() {
        let c = constraint(
            ConstraintType::Product,
            ConstraintScope::Global,
            CanonicalReviewState::Deprecated,
            &[],
        );
        let default_q = ConstraintQuery::default();
        assert!(!matches_query(&c, &default_q));
        let inclusive = ConstraintQuery {
            include_deprecated: true,
            ..ConstraintQuery::default()
        };
        assert!(matches_query(&c, &inclusive));
    }

    #[test]
    fn matches_query_filters_type_scope_surface() {
        let c = constraint(
            ConstraintType::Security,
            ConstraintScope::Service,
            CanonicalReviewState::Accepted,
            &["api/auth"],
        );
        // Type filter
        assert!(matches_query(
            &c,
            &ConstraintQuery {
                types: Some(vec![ConstraintType::Security]),
                ..ConstraintQuery::default()
            }
        ));
        assert!(!matches_query(
            &c,
            &ConstraintQuery {
                types: Some(vec![ConstraintType::Product]),
                ..ConstraintQuery::default()
            }
        ));
        // Scope filter
        assert!(!matches_query(
            &c,
            &ConstraintQuery {
                scopes: Some(vec![ConstraintScope::Global]),
                ..ConstraintQuery::default()
            }
        ));
        // Surface filter
        assert!(matches_query(
            &c,
            &ConstraintQuery {
                affected_surface: Some("api/auth".to_string()),
                ..ConstraintQuery::default()
            }
        ));
        assert!(!matches_query(
            &c,
            &ConstraintQuery {
                affected_surface: Some("api/billing".to_string()),
                ..ConstraintQuery::default()
            }
        ));
    }

    #[test]
    fn matches_query_empty_type_filter_matches_nothing() {
        let c = constraint(
            ConstraintType::Product,
            ConstraintScope::Global,
            CanonicalReviewState::Accepted,
            &[],
        );
        let q = ConstraintQuery {
            types: Some(Vec::new()),
            ..ConstraintQuery::default()
        };
        assert!(
            !matches_query(&c, &q),
            "an empty types filter must match nothing (JS empty-array truthiness)"
        );
    }

    #[test]
    fn supersede_constraint_is_immutable_and_versions() {
        let current = constraint(
            ConstraintType::Technical,
            ConstraintScope::Feature,
            CanonicalReviewState::Accepted,
            &[],
        );
        let replacement = constraint(
            ConstraintType::Technical,
            ConstraintScope::Feature,
            CanonicalReviewState::Accepted,
            &[],
        );
        let at = Utc::now();
        let (superseded, new_replacement) = supersede_constraint(&current, &replacement, at);

        assert_eq!(superseded.review_state, CanonicalReviewState::Superseded);
        assert_eq!(superseded.superseded_by, Some(replacement.id));
        assert_eq!(superseded.updated_at, at);
        assert_eq!(new_replacement.supersedes, Some(current.id));
        assert_eq!(new_replacement.version, current.version + 1);
        assert_eq!(new_replacement.updated_at, at);
        // Inputs unchanged (immutability).
        assert_eq!(current.review_state, CanonicalReviewState::Accepted);
        assert_eq!(current.superseded_by, None);
        assert_eq!(replacement.supersedes, None);
        assert_eq!(replacement.version, 1);
    }

    #[test]
    fn query_catalog_counts_matches() {
        let mut catalog = HashMap::new();
        catalog.insert(
            "a".to_string(),
            constraint(
                ConstraintType::Product,
                ConstraintScope::Global,
                CanonicalReviewState::Accepted,
                &[],
            ),
        );
        catalog.insert(
            "b".to_string(),
            constraint(
                ConstraintType::Security,
                ConstraintScope::Global,
                CanonicalReviewState::Accepted,
                &[],
            ),
        );
        catalog.insert(
            "c".to_string(),
            constraint(
                ConstraintType::Security,
                ConstraintScope::Service,
                CanonicalReviewState::Accepted,
                &[],
            ),
        );
        let result = query_catalog(
            &catalog,
            &ConstraintQuery {
                types: Some(vec![ConstraintType::Security]),
                ..ConstraintQuery::default()
            },
        );
        assert_eq!(result.total, 2);
        assert!(result
            .constraints
            .iter()
            .all(|c| c.constraint_type == ConstraintType::Security));
    }
}
