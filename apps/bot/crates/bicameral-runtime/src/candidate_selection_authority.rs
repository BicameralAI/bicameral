//! Daemon-owned current Product authority projection for CandidateSet selection.
//!
//! The registry is populated from trusted canonical Product membership and
//! governance-policy replay plus a daemon-owned product currentness projection.
//! It is not an MCP request cache: callers cannot grant themselves authority by
//! supplying one of these snapshots.

use std::collections::HashMap;
use std::sync::Mutex;

use bicameral_api::candidate_selection::{
    CandidatePromotionPolicyV1, CandidateSelectionAuthorityV1, CandidateSelectionFailureCode,
    CandidateSelectionFailureV1, CandidateSelectionResult,
};
use bicameral_api::candidate_session::CandidateSetBindingV1;
use bicameral_api::event_store::EventStoreAdapter;
use bicameral_api::product_join::GrantedRole;

use crate::candidate_selection_service::CandidateSelectionAuthorityProvider;
use crate::product_join::ProductJoinState;
use crate::replay::{self, ReplayedState};

/// Daemon-owned currentness for one Product (spec binding + accepted model base).
///
/// Seeded when the daemon materializes a CandidateSet (or first prepare sees a
/// set while currentness is empty). Advanced only by daemon-owned product
/// currentness updates — never by MCP request fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductCurrentnessV1 {
    pub product_id: String,
    pub governing_spec_binding_digest: String,
    pub accepted_model_base_position: u64,
    pub accepted_model_base_digest: String,
    pub promotion_policy: CandidatePromotionPolicyV1,
}

/// Membership-derived promotion rights for one Product actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorPromotionRights {
    pub may_promote: bool,
    pub is_current_product_owner: bool,
}

#[derive(Default)]
pub struct CandidateSelectionAuthorityRegistry {
    current: Mutex<HashMap<(String, String), CandidateSelectionAuthorityV1>>,
}

impl CandidateSelectionAuthorityRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the current canonical projection for one Product/actor pair.
    ///
    /// The daemon calls this only from its trusted replay/projection boundary.
    pub fn project_current(&self, snapshot: CandidateSelectionAuthorityV1) {
        self.current
            .lock()
            .expect("candidate selection authority registry poisoned")
            .insert(
                (snapshot.product_id.clone(), snapshot.actor_id.clone()),
                snapshot,
            );
    }

    /// Remove current authority, for example after Product Owner revocation.
    pub fn revoke(&self, product_id: &str, actor_id: &str) {
        self.current
            .lock()
            .expect("candidate selection authority registry poisoned")
            .remove(&(product_id.to_string(), actor_id.to_string()));
    }
}

impl CandidateSelectionAuthorityProvider for CandidateSelectionAuthorityRegistry {
    fn current_authority(
        &self,
        product_id: &str,
        actor_id: &str,
    ) -> CandidateSelectionResult<CandidateSelectionAuthorityV1> {
        self.current
            .lock()
            .expect("candidate selection authority registry poisoned")
            .get(&(product_id.to_string(), actor_id.to_string()))
            .cloned()
            .ok_or_else(|| {
                CandidateSelectionFailureV1::new(
                    CandidateSelectionFailureCode::ActorUnauthorized,
                    "no current canonical Product authority exists for this actor",
                )
            })
    }
}

/// Daemon-owned product currentness projection (independent of MCP requests).
#[derive(Default)]
pub struct ProductCurrentnessStore {
    by_product: Mutex<HashMap<String, ProductCurrentnessV1>>,
}

impl ProductCurrentnessStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace product currentness. Daemon-only boundary.
    pub fn project(&self, currentness: ProductCurrentnessV1) {
        self.by_product
            .lock()
            .expect("product currentness store poisoned")
            .insert(currentness.product_id.clone(), currentness);
    }

    /// Seed currentness from a CandidateSet binding only when the product has
    /// no currentness yet (first set materialization / first prepare).
    ///
    /// Does not overwrite an advanced base — that would erase governing-spec
    /// / accepted-model drift detection.
    pub fn seed_from_binding_if_absent(
        &self,
        binding: &CandidateSetBindingV1,
        promotion_policy: CandidatePromotionPolicyV1,
    ) -> ProductCurrentnessV1 {
        let mut guard = self
            .by_product
            .lock()
            .expect("product currentness store poisoned");
        if let Some(existing) = guard.get(&binding.product_id) {
            return existing.clone();
        }
        let seeded = ProductCurrentnessV1 {
            product_id: binding.product_id.clone(),
            governing_spec_binding_digest: binding.governing_spec_binding_digest.clone(),
            accepted_model_base_position: binding.accepted_model_base_position,
            accepted_model_base_digest: binding.accepted_model_base_digest.clone(),
            promotion_policy,
        };
        guard.insert(binding.product_id.clone(), seeded.clone());
        seeded
    }

    pub fn get(&self, product_id: &str) -> Option<ProductCurrentnessV1> {
        self.by_product
            .lock()
            .expect("product currentness store poisoned")
            .get(product_id)
            .cloned()
    }

    /// Explicitly advance currentness (e.g. accepted model base moved).
    pub fn advance(&self, currentness: ProductCurrentnessV1) {
        self.project(currentness);
    }
}

/// Normalize actor identities so `bridge:<cloud_account>` matches ledger accounts.
pub fn normalize_actor_account(actor_id: &str) -> &str {
    actor_id.strip_prefix("bridge:").unwrap_or(actor_id).trim()
}

/// Whether `actor_id` refers to the same Cloud account as `cloud_account_id`.
pub fn actor_matches_cloud_account(actor_id: &str, cloud_account_id: &str) -> bool {
    let actor = normalize_actor_account(actor_id);
    let account = cloud_account_id.trim();
    !actor.is_empty() && !account.is_empty() && (actor == account || actor_id == cloud_account_id)
}

/// Derive promotion rights from replayed Product creation + membership events.
///
/// Returns `None` when the actor has no canonical Product role (fail closed).
pub fn actor_promotion_rights(
    replayed: &ReplayedState,
    join_state: &ProductJoinState,
    product_id: &str,
    actor_id: &str,
) -> Option<ActorPromotionRights> {
    if product_id.trim().is_empty() || actor_id.trim().is_empty() {
        return None;
    }

    // Creator authority: CreateProduct grants Policy Owner + Product Owner.
    if let Some(product) = replayed.products.get(product_id) {
        if actor_matches_cloud_account(actor_id, &product.creator_cloud_account_id) {
            let is_owner = product
                .creator_roles
                .iter()
                .any(|role| role == "Product Owner");
            let may_promote = is_owner
                || product
                    .creator_roles
                    .iter()
                    .any(|role| role == "Policy Owner");
            if may_promote {
                return Some(ActorPromotionRights {
                    may_promote: true,
                    is_current_product_owner: is_owner,
                });
            }
        }
    }

    // Auto-joined Product Members may promote to proposed (not auto-approve).
    let account = normalize_actor_account(actor_id).to_string();
    if let Some(membership) = join_state
        .memberships
        .get(&(product_id.to_string(), account))
    {
        if membership.granted_role == GrantedRole::ProductMember {
            return Some(ActorPromotionRights {
                may_promote: true,
                is_current_product_owner: false,
            });
        }
    }

    None
}

/// Default promotion policy when Product policy has no auto-approve flag yet.
///
/// Alpha default: auto-approve disabled; policy revision follows the Product's
/// governance policy version when known.
pub fn default_promotion_policy(
    governance_policy_version: Option<&str>,
) -> CandidatePromotionPolicyV1 {
    CandidatePromotionPolicyV1 {
        enabled: false,
        policy_revision: governance_policy_version
            .filter(|v| !v.trim().is_empty())
            .unwrap_or("gpv_1")
            .to_string(),
    }
}

/// Project live authority for one prepare/commit from trusted event-store replay
/// and daemon product currentness.
///
/// - Membership / creator roles come only from the Product ledger.
/// - Currentness digests come from [`ProductCurrentnessStore`], optionally seeded
///   from the active CandidateSet binding when the product has no currentness yet.
/// - On missing rights or currentness, the actor's registry entry is revoked so
///   prepare/commit fail closed with `actor_unauthorized`.
pub fn project_live_authority(
    registry: &CandidateSelectionAuthorityRegistry,
    currentness: &ProductCurrentnessStore,
    event_store: &dyn EventStoreAdapter,
    product_id: &str,
    actor_id: &str,
    set_binding: Option<&CandidateSetBindingV1>,
) -> CandidateSelectionResult<CandidateSelectionAuthorityV1> {
    let entries = event_store.replay().map_err(|error| {
        CandidateSelectionFailureV1::new(
            CandidateSelectionFailureCode::ActorUnauthorized,
            format!("product authority replay failed: {error}"),
        )
    })?;
    let replayed = replay::replay_from_adapter(event_store).map_err(|error| {
        CandidateSelectionFailureV1::new(
            CandidateSelectionFailureCode::ActorUnauthorized,
            format!("product authority materialization failed: {error}"),
        )
    })?;
    let join_state = ProductJoinState::from_entries(&entries);

    let Some(rights) = actor_promotion_rights(&replayed, &join_state, product_id, actor_id) else {
        registry.revoke(product_id, actor_id);
        return Err(CandidateSelectionFailureV1::new(
            CandidateSelectionFailureCode::ActorUnauthorized,
            "actor has no current canonical Product promotion role",
        ));
    };

    let policy = default_promotion_policy(
        replayed
            .products
            .get(product_id)
            .map(|p| p.governance_policy_version.as_str()),
    );

    let product_currentness = if let Some(binding) = set_binding {
        if binding.product_id != product_id {
            registry.revoke(product_id, actor_id);
            return Err(CandidateSelectionFailureV1::new(
                CandidateSelectionFailureCode::ActorUnauthorized,
                "CandidateSet binding product does not match selection product",
            ));
        }
        currentness.seed_from_binding_if_absent(binding, policy)
    } else if let Some(existing) = currentness.get(product_id) {
        existing
    } else {
        registry.revoke(product_id, actor_id);
        return Err(CandidateSelectionFailureV1::new(
            CandidateSelectionFailureCode::ActorUnauthorized,
            "no daemon-owned product currentness is projected for this Product",
        ));
    };

    let snapshot = CandidateSelectionAuthorityV1 {
        product_id: product_id.to_string(),
        actor_id: actor_id.to_string(),
        may_promote: rights.may_promote,
        is_current_product_owner: rights.is_current_product_owner,
        governing_spec_binding_digest: product_currentness.governing_spec_binding_digest,
        accepted_model_base_position: product_currentness.accepted_model_base_position,
        accepted_model_base_digest: product_currentness.accepted_model_base_digest,
        promotion_policy: product_currentness.promotion_policy,
    };
    registry.project_current(snapshot.clone());
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_store_adapters::MemoryAdapter;
    use bicameral_api::event_store::{
        EventStoreAdapter, EventStoreEntry, EventType, ProductCreatedPayload,
    };
    use bicameral_api::product_join::ProductMembership;
    use chrono::Utc;

    fn snapshot() -> CandidateSelectionAuthorityV1 {
        CandidateSelectionAuthorityV1 {
            product_id: "product-a".into(),
            actor_id: "owner-a".into(),
            may_promote: true,
            is_current_product_owner: true,
            governing_spec_binding_digest: "sha256:spec".into(),
            accepted_model_base_position: 4,
            accepted_model_base_digest: "sha256:base".into(),
            promotion_policy: CandidatePromotionPolicyV1 {
                enabled: false,
                policy_revision: "policy-1".into(),
            },
        }
    }

    fn binding() -> CandidateSetBindingV1 {
        CandidateSetBindingV1 {
            product_id: "product-a".into(),
            host_session_id: "host-1".into(),
            governing_spec_binding_digest: "sha256:spec".into(),
            plan_digest: "sha256:plan".into(),
            accepted_model_base_position: 4,
            accepted_model_base_digest: "sha256:base".into(),
        }
    }

    fn create_product_entry(product_id: &str, creator: &str) -> EventStoreEntry {
        EventStoreEntry::new(
            EventType::CreateProduct,
            serde_json::to_value(ProductCreatedPayload {
                product_id: product_id.into(),
                creator_cloud_account_id: creator.into(),
                creator_roles: vec!["Policy Owner".into(), "Product Owner".into()],
                governance_policy_version: "gpv_1".into(),
                receipt_id: "rcpt-1".into(),
                created_at: Utc::now().to_rfc3339(),
                idempotency_key: format!("idem-{product_id}"),
                product_display_name: "Test Product".into(),
                team_display_name: None,
                verified_domain: Some("example.com".into()),
                event_store_hint: None,
                required_connection: None,
            })
            .unwrap(),
            creator.into(),
        )
    }

    #[test]
    fn revoke_removes_current_authority() {
        let registry = CandidateSelectionAuthorityRegistry::new();
        registry.project_current(snapshot());
        assert!(registry.current_authority("product-a", "owner-a").is_ok());
        registry.revoke("product-a", "owner-a");
        assert!(registry.current_authority("product-a", "owner-a").is_err());
    }

    #[test]
    fn seed_from_binding_does_not_overwrite_advanced_currentness() {
        let store = ProductCurrentnessStore::new();
        store.project(ProductCurrentnessV1 {
            product_id: "product-a".into(),
            governing_spec_binding_digest: "sha256:advanced-spec".into(),
            accepted_model_base_position: 99,
            accepted_model_base_digest: "sha256:advanced-base".into(),
            promotion_policy: default_promotion_policy(Some("gpv_2")),
        });
        let seeded = store.seed_from_binding_if_absent(&binding(), default_promotion_policy(None));
        assert_eq!(seeded.accepted_model_base_position, 99);
        assert_eq!(seeded.governing_spec_binding_digest, "sha256:advanced-spec");
    }

    #[test]
    fn project_live_authority_for_product_owner() {
        let adapter = MemoryAdapter::new();
        adapter
            .append(create_product_entry("product-a", "acct_owner"))
            .unwrap();
        let registry = CandidateSelectionAuthorityRegistry::new();
        let currentness = ProductCurrentnessStore::new();
        let authority = project_live_authority(
            &registry,
            &currentness,
            &adapter,
            "product-a",
            "bridge:acct_owner",
            Some(&binding()),
        )
        .expect("owner projected");
        assert!(authority.may_promote);
        assert!(authority.is_current_product_owner);
        assert_eq!(authority.accepted_model_base_position, 4);
        assert_eq!(authority.promotion_policy.policy_revision, "gpv_1");
    }

    #[test]
    fn project_live_authority_rejects_unknown_actor() {
        let adapter = MemoryAdapter::new();
        adapter
            .append(create_product_entry("product-a", "acct_owner"))
            .unwrap();
        let registry = CandidateSelectionAuthorityRegistry::new();
        let currentness = ProductCurrentnessStore::new();
        let err = project_live_authority(
            &registry,
            &currentness,
            &adapter,
            "product-a",
            "stranger",
            Some(&binding()),
        )
        .unwrap_err();
        assert_eq!(err.code, CandidateSelectionFailureCode::ActorUnauthorized);
        assert!(registry.current_authority("product-a", "stranger").is_err());
    }

    #[test]
    fn product_member_may_promote_but_is_not_owner() {
        let adapter = MemoryAdapter::new();
        adapter
            .append(create_product_entry("product-a", "acct_owner"))
            .unwrap();
        let mut join_state = ProductJoinState::default();
        join_state.memberships.insert(
            ("product-a".into(), "acct_member".into()),
            ProductMembership {
                product_id: "product-a".into(),
                cloud_account_id: "acct_member".into(),
                local_actor_ref: "local:member".into(),
                granted_role: GrantedRole::ProductMember,
                verified_domain: "example.com".into(),
                governance_policy_version: "gpv_1".into(),
                membership_event_ref: "mem-member".into(),
                receipt_id: "rcpt-m".into(),
                joined_at: Utc::now(),
            },
        );
        let replayed = replay::replay_from_adapter(&adapter).unwrap();
        let rights = actor_promotion_rights(&replayed, &join_state, "product-a", "acct_member")
            .expect("member rights");
        assert!(rights.may_promote);
        assert!(!rights.is_current_product_owner);
    }

    #[test]
    fn advanced_currentness_is_not_reseeded_from_stale_binding() {
        let adapter = MemoryAdapter::new();
        adapter
            .append(create_product_entry("product-a", "acct_owner"))
            .unwrap();
        let registry = CandidateSelectionAuthorityRegistry::new();
        let currentness = ProductCurrentnessStore::new();
        project_live_authority(
            &registry,
            &currentness,
            &adapter,
            "product-a",
            "acct_owner",
            Some(&binding()),
        )
        .unwrap();
        currentness.advance(ProductCurrentnessV1 {
            product_id: "product-a".into(),
            governing_spec_binding_digest: "sha256:new-spec".into(),
            accepted_model_base_position: 4,
            accepted_model_base_digest: "sha256:base".into(),
            promotion_policy: default_promotion_policy(Some("gpv_1")),
        });
        // Even if prepare re-presents the old set binding, seed-if-absent keeps
        // the advanced product currentness.
        let authority = project_live_authority(
            &registry,
            &currentness,
            &adapter,
            "product-a",
            "acct_owner",
            Some(&binding()),
        )
        .unwrap();
        assert_eq!(authority.governing_spec_binding_digest, "sha256:new-spec");
    }
}
