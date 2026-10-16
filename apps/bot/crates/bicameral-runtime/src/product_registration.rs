//! Runtime side of `product.continue_setup` — local setup continuation after
//! canonical Product creation or Product Member auto-join (issue #769).
//!
//! The pure decision logic and contract types live in
//! [`bicameral_api::product_registration`]; this module supplies the
//! daemon-local glue: operator-registry persistence, the adapter open+replay
//! probe, and a compatibility reader for legacy registration events.
//!
//! Authority boundary: provider-access and descriptor-validation facts are read
//! from daemon-set audit metadata (never SPA-asserted). Canonical membership is
//! an input, never an output — this handler writes only local registration
//! state and never appends, changes, or revokes a membership event. Nothing it
//! returns or logs carries a provider secret, raw descriptor, or filesystem
//! path. Creator authority is replayed from the atomic `CreateProduct` event;
//! joined-member authority is replayed from `ProductMemberAdded`.

use std::collections::BTreeMap;

use bicameral_api::event_store::{
    EventStoreAdapter, EventStoreEntry, EventType, ProductCreatedPayload,
};
use bicameral_api::product_registration::{
    derive_setup_decision, ConnectionMetadata, ContinueSetupRejectionCode, DescriptorValidation,
    ProductContinueSetupParams, ProductContinueSetupResult, ProjectRegistration,
    ProjectRegistrationWrittenPayload, ProviderAccessState, RegisteredProvider,
    RegisteredSubstrate, ReplayOutcome, SetupDecision, SetupEvaluationInput, SetupReadiness,
    DESCRIPTOR_VALIDATION_METADATA_KEY, PROVIDER_ACCESS_METADATA_KEY,
};
use bicameral_api::tool_request::{AuthorityContext, ToolRequest, ToolResponse};
use chrono::Utc;

pub use crate::product_join::PAIRED_CLOUD_ACCOUNT_METADATA_KEY;

// ---------------------------------------------------------------------------
// Adapter open + replay probe (injected so the handler stays deterministic)
// ---------------------------------------------------------------------------

/// Opens the registered event-store adapter for a [`ProjectRegistration`] and
/// replays canonical Product state, reporting whether replay succeeded.
///
/// Injected via [`ProductSetupDispatchContext`] so the dispatch handler is
/// deterministic and fully testable. The production daemon wires an
/// implementation that resolves the descriptor to a concrete adapter and
/// replays it; the failure path yields the typed `Repair(ReplayFailed)` state.
pub trait ProductReplayProbe {
    fn probe(&self, registration: &ProjectRegistration) -> ReplayOutcome;
}

/// Daemon-owned context the dispatcher needs to open + replay a registered
/// Product. Absent (pure-runtime test entrypoints, or a daemon that cannot
/// resolve its registered-adapter opener) means `product.continue_setup` fails
/// closed with `unsupported_capability`.
pub struct ProductSetupDispatchContext<'a> {
    pub replay_probe: &'a dyn ProductReplayProbe,
    pub registration_store: &'a dyn crate::project_registry::ProjectRegistrationStore,
    pub event_store_route: &'a crate::project_registry::EventStoreRoute,
}

// ---------------------------------------------------------------------------
// Legacy event-ledger migration reader
// ---------------------------------------------------------------------------

/// Legacy local registrations materialized from pre-registry event ledgers.
///
/// New code must not write `ProjectRegistrationWritten` events. This projection
/// exists only so daemon startup can import old records into the operator
/// registry without losing existing setup state.
#[derive(Debug, Default, Clone)]
pub struct LegacyProductRegistrationState {
    pub registrations: BTreeMap<(String, String), ProjectRegistration>,
}

impl LegacyProductRegistrationState {
    pub fn apply(&mut self, entry: &EventStoreEntry) {
        if entry.event_type != EventType::ProjectRegistrationWritten {
            return;
        }
        let Ok(payload) =
            serde_json::from_value::<ProjectRegistrationWrittenPayload>(entry.payload.clone())
        else {
            return;
        };
        self.registrations
            .entry((payload.product_id.clone(), payload.cloud_account_id.clone()))
            .or_insert_with(|| payload.to_registration());
    }

    pub fn from_entries(entries: &[EventStoreEntry]) -> Self {
        let mut state = Self::default();
        for entry in entries {
            state.apply(entry);
        }
        state
    }
}

// ---------------------------------------------------------------------------
// Dispatch handler
// ---------------------------------------------------------------------------

fn metadata_str<'a>(authority: &'a AuthorityContext, key: &str) -> Option<&'a str> {
    authority.audit_metadata.get(key)?.as_str()
}

/// The paired Cloud account the daemon derived from the validated bridge
/// session. Read from daemon-set metadata, never SPA-asserted.
fn paired_account(authority: &AuthorityContext) -> Option<String> {
    let account = metadata_str(authority, PAIRED_CLOUD_ACCOUNT_METADATA_KEY)?;
    if account.is_empty() {
        return None;
    }
    Some(account.to_string())
}

/// Dispatch a `product.continue_setup` command.
///
/// Materializes canonical creator/member authority from the Product ledger and
/// durable routing from the operator registry, reads daemon-derived
/// provider/descriptor facts, and applies [`derive_setup_decision`]. On a fresh
/// completion it writes one operator registration, then opens + replays the
/// registered Product. Fail-closed and idempotent: a repeated
/// completion reuses the existing registration, and any failure leaves
/// canonical membership untouched.
pub(crate) fn dispatch_product_continue_setup(
    request: &ToolRequest,
    params: &ProductContinueSetupParams,
    adapter: &dyn EventStoreAdapter,
    ctx: Option<&ProductSetupDispatchContext<'_>>,
) -> ToolResponse {
    let Some(ctx) = ctx else {
        return ToolResponse::rejected(
            request.request_id,
            "unsupported_capability: product.continue_setup requires a registered-adapter \
             replay probe"
                .into(),
        );
    };

    // Redaction-safe log only: product id + typed facts, never the descriptor,
    // registration_ref, account label, provider secret, or a local path.
    let provider_access = ProviderAccessState::from_metadata(metadata_str(
        &request.authority,
        PROVIDER_ACCESS_METADATA_KEY,
    ));
    let descriptor = DescriptorValidation::from_metadata(metadata_str(
        &request.authority,
        DESCRIPTOR_VALIDATION_METADATA_KEY,
    ));
    tracing::debug!(
        product_id = %params.product_id,
        provider_access = %provider_access.as_str(),
        descriptor = %descriptor.as_str(),
        "product.continue_setup request received"
    );

    let Some(cloud_account_id) = paired_account(&request.authority) else {
        // No confirmed pairing → cannot bind an account. Fail closed with no
        // membership claim retained (there is nothing to retain here).
        let result = ProductContinueSetupResult::rejected(
            params.request_id.clone(),
            params.product_id.clone(),
            String::new(),
            ContinueSetupRejectionCode::MembershipRequired,
            false,
            Utc::now(),
        );
        return success_result(request, &result);
    };

    let entries = match adapter.replay() {
        Ok(entries) => entries,
        Err(e) => {
            return ToolResponse::error(
                request.request_id,
                format!("daemon_unavailable: replay failed: {e}"),
            );
        }
    };

    let membership_state = crate::product_join::ProductJoinState::from_entries(&entries);
    let key = (params.product_id.clone(), cloud_account_id.clone());
    let joined_membership = membership_state.memberships.get(&key);
    let created_product_owner = entries.iter().find_map(|entry| {
        if entry.event_type != EventType::CreateProduct {
            return None;
        }
        let payload =
            serde_json::from_value::<ProductCreatedPayload>(entry.payload.clone()).ok()?;
        let creator_roles_are_canonical = matches!(
            payload.creator_roles.as_slice(),
            [policy_owner, product_owner]
                if policy_owner == "Policy Owner" && product_owner == "Product Owner"
        );
        (payload.product_id == params.product_id
            && payload.creator_cloud_account_id == cloud_account_id
            && creator_roles_are_canonical)
            .then_some(payload)
    });
    let membership_present = joined_membership.is_some() || created_product_owner.is_some();
    let existing_record = match ctx
        .registration_store
        .get(&params.product_id, &cloud_account_id)
    {
        Ok(record) => record,
        Err(error) => {
            return ToolResponse::error(
                request.request_id,
                format!("daemon_unavailable: operator registry read failed: {error}"),
            );
        }
    };
    let existing_registration = existing_record.as_ref().map(|record| &record.registration);

    // Only open+replay when a registration already exists; the pure decision
    // ignores the probe otherwise.
    let replay_probe = existing_registration.map(|reg| ctx.replay_probe.probe(reg));

    let input = SetupEvaluationInput {
        membership_present,
        existing_registration,
        provider_access,
        descriptor,
        replay_probe,
    };
    let decision = derive_setup_decision(&input);
    let decided_at = Utc::now();

    match decision {
        SetupDecision::MembershipRequired => {
            let result = ProductContinueSetupResult::rejected(
                params.request_id.clone(),
                params.product_id.clone(),
                cloud_account_id,
                ContinueSetupRejectionCode::MembershipRequired,
                false,
                decided_at,
            );
            success_result(request, &result)
        }
        SetupDecision::AlreadyOpen => {
            let registration_ref = existing_registration.map(|r| r.registration_ref.clone());
            advanced(
                request,
                params,
                cloud_account_id,
                SetupReadiness::Open,
                registration_ref,
                decided_at,
            )
        }
        SetupDecision::Continue(step) => advanced(
            request,
            params,
            cloud_account_id,
            SetupReadiness::ContinueSetup { step },
            None,
            decided_at,
        ),
        SetupDecision::Repair(reason) => {
            // A replay failure retains the registration_ref (the mapping is
            // durable and recoverable); other repairs precede registration.
            let registration_ref = existing_registration.map(|r| r.registration_ref.clone());
            advanced(
                request,
                params,
                cloud_account_id,
                SetupReadiness::Repair { reason },
                registration_ref,
                decided_at,
            )
        }
        SetupDecision::WriteRegistration => {
            let connection = ConnectionMetadata {
                provider: params.provider_hint.unwrap_or(RegisteredProvider::None),
                substrate: params.substrate_hint.unwrap_or(RegisteredSubstrate::Git),
                account_label: params.account_label.clone(),
                // Opaque, daemon-local descriptor reference derived from the
                // routing ref. Never the raw descriptor or a filesystem path.
                descriptor_ref: format!("descriptor:{}", params.registration_ref),
            };
            let payload = ProjectRegistrationWrittenPayload {
                product_id: params.product_id.clone(),
                cloud_account_id: cloud_account_id.clone(),
                registration_ref: params.registration_ref.clone(),
                connection,
                governance_policy_version: joined_membership
                    .map(|membership| membership.governance_policy_version.clone())
                    .or_else(|| {
                        created_product_owner
                            .as_ref()
                            .map(|product| product.governance_policy_version.clone())
                    })
                    .unwrap_or_default(),
                written_at: decided_at,
            };
            let registration = payload.to_registration();
            let record = crate::project_registry::ProjectRouteRecord {
                registration: registration.clone(),
                event_store: ctx.event_store_route.clone(),
                first_event_commit_proof: None,
            };
            if let Err(e) = ctx.registration_store.write(&record) {
                return ToolResponse::error(
                    request.request_id,
                    format!("daemon_unavailable: operator registration write failed: {e}"),
                );
            }

            // Open the newly registered Product and replay canonical state.
            let readiness = match ctx.replay_probe.probe(&registration) {
                ReplayOutcome::Succeeded => SetupReadiness::Open,
                ReplayOutcome::Failed => SetupReadiness::Repair {
                    reason: bicameral_api::product_registration::SetupRepairReason::ReplayFailed,
                },
            };
            advanced(
                request,
                params,
                cloud_account_id,
                readiness,
                Some(params.registration_ref.clone()),
                decided_at,
            )
        }
    }
}

fn advanced(
    request: &ToolRequest,
    params: &ProductContinueSetupParams,
    cloud_account_id: String,
    readiness: SetupReadiness,
    registration_ref: Option<String>,
    decided_at: chrono::DateTime<Utc>,
) -> ToolResponse {
    let result = ProductContinueSetupResult::advanced(
        params.request_id.clone(),
        params.product_id.clone(),
        cloud_account_id,
        readiness,
        registration_ref,
        decided_at,
    );
    success_result(request, &result)
}

fn success_result(request: &ToolRequest, result: &ProductContinueSetupResult) -> ToolResponse {
    match serde_json::to_value(result) {
        Ok(v) => ToolResponse::success(request.request_id, v),
        Err(e) => ToolResponse::error(
            request.request_id,
            format!("runtime_error: failed to serialize product.continue_setup result: {e}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_store_adapters::MemoryAdapter;
    use crate::project_registry::ProjectRegistrationStore;
    use bicameral_api::product_join::{GrantedRole, ProductMemberAddedPayload};
    use bicameral_api::product_registration::{
        ContinueSetupOutcome, ContinueSetupStep, SetupRepairReason,
    };
    use bicameral_api::tool_request::{AuthMethod, ToolCommand, ToolResponseStatus};
    use uuid::Uuid;

    const PRODUCT: &str = "prd_test";
    const ACCOUNT: &str = "acct_alice";

    /// A replay probe with a fixed outcome.
    struct FixedProbe(ReplayOutcome);
    impl ProductReplayProbe for FixedProbe {
        fn probe(&self, _registration: &ProjectRegistration) -> ReplayOutcome {
            self.0
        }
    }

    fn membership_event() -> EventStoreEntry {
        let payload = ProductMemberAddedPayload {
            product_id: PRODUCT.to_string(),
            cloud_account_id: ACCOUNT.to_string(),
            local_actor_ref: format!("bridge:{ACCOUNT}"),
            granted_role: GrantedRole::ProductMember,
            verified_domain: "acme.example".to_string(),
            governance_policy_version: "gpv-1".to_string(),
            ticket_nonce: Uuid::new_v4().to_string(),
            receipt_id: Uuid::new_v4().to_string(),
            decided_at: Utc::now(),
        };
        EventStoreEntry::new(
            EventType::ProductMemberAdded,
            serde_json::to_value(&payload).unwrap(),
            "daemon".to_string(),
        )
    }

    fn creation_event() -> EventStoreEntry {
        EventStoreEntry::new(
            EventType::CreateProduct,
            serde_json::to_value(ProductCreatedPayload {
                product_id: PRODUCT.to_string(),
                creator_cloud_account_id: ACCOUNT.to_string(),
                creator_roles: vec!["Policy Owner".into(), "Product Owner".into()],
                governance_policy_version: "gpv-created".into(),
                receipt_id: "rcpt-created".into(),
                idempotency_key: "idem-created".into(),
                product_display_name: "Acme Product".into(),
                team_display_name: None,
                verified_domain: Some("acme.example".into()),
                event_store_hint: None,
                required_connection: None,
                created_at: Utc::now().to_rfc3339(),
            })
            .unwrap(),
            format!("bridge:{ACCOUNT}"),
        )
    }

    fn params() -> ProductContinueSetupParams {
        ProductContinueSetupParams {
            request_id: Uuid::new_v4().to_string(),
            product_id: PRODUCT.to_string(),
            registration_ref: "reg_ref_opaque".to_string(),
            provider_hint: Some(RegisteredProvider::Github),
            substrate_hint: Some(RegisteredSubstrate::Git),
            account_label: Some("acme-org".to_string()),
            requested_at: Utc::now(),
        }
    }

    fn request(
        params: ProductContinueSetupParams,
        provider: Option<&str>,
        descriptor: Option<&str>,
        paired: bool,
    ) -> ToolRequest {
        let mut audit = serde_json::Map::new();
        if paired {
            audit.insert(
                PAIRED_CLOUD_ACCOUNT_METADATA_KEY.into(),
                serde_json::Value::String(ACCOUNT.to_string()),
            );
        }
        if let Some(p) = provider {
            audit.insert(
                PROVIDER_ACCESS_METADATA_KEY.into(),
                serde_json::Value::String(p.to_string()),
            );
        }
        if let Some(d) = descriptor {
            audit.insert(
                DESCRIPTOR_VALIDATION_METADATA_KEY.into(),
                serde_json::Value::String(d.to_string()),
            );
        }
        let authority = AuthorityContext {
            actor_id: format!("bridge:{ACCOUNT}"),
            auth_method: AuthMethod::HostedBridge,
            session_id: Some("sess-1".to_string()),
            workspace: None,
            policy_scope: vec!["hosted_bridge".to_string()],
            audit_metadata: audit,
        };
        ToolRequest::new(ToolCommand::ProductContinueSetup(params), authority)
    }

    fn dispatch(
        request: &ToolRequest,
        adapter: &MemoryAdapter,
        probe: ReplayOutcome,
    ) -> ToolResponse {
        let registry = crate::project_registry::MemoryProjectRegistrationStore::new();
        dispatch_with_registry(request, adapter, &registry, probe)
    }

    fn dispatch_with_registry(
        request: &ToolRequest,
        adapter: &MemoryAdapter,
        registry: &dyn crate::project_registry::ProjectRegistrationStore,
        probe: ReplayOutcome,
    ) -> ToolResponse {
        let probe = FixedProbe(probe);
        let route = crate::project_registry::EventStoreRoute::Git {
            events_dir: std::path::PathBuf::from("/test/.bicameral/events"),
            ci_merge_gate_configured: false,
        };
        let ctx = ProductSetupDispatchContext {
            replay_probe: &probe,
            registration_store: registry,
            event_store_route: &route,
        };
        dispatch_product_continue_setup(
            request,
            match &request.command {
                ToolCommand::ProductContinueSetup(p) => p,
                _ => unreachable!(),
            },
            adapter,
            Some(&ctx),
        )
    }

    fn result_of(response: &ToolResponse) -> ProductContinueSetupResult {
        assert_eq!(response.status, ToolResponseStatus::Ok, "{response:?}");
        serde_json::from_value(response.result.clone().expect("result payload")).unwrap()
    }

    fn registration_events(adapter: &MemoryAdapter) -> usize {
        adapter
            .replay()
            .unwrap()
            .iter()
            .filter(|e| e.event_type == EventType::ProjectRegistrationWritten)
            .count()
    }

    fn registration_count(
        registry: &dyn crate::project_registry::ProjectRegistrationStore,
    ) -> usize {
        registry.list().unwrap().len()
    }

    #[test]
    fn joined_without_provider_is_continue_setup_and_keeps_membership() {
        let adapter = MemoryAdapter::new();
        adapter.append(membership_event()).unwrap();
        let req = request(params(), Some("absent"), None, true);
        let res = result_of(&dispatch(&req, &adapter, ReplayOutcome::Succeeded));
        assert_eq!(res.outcome, ContinueSetupOutcome::Advanced);
        assert!(res.membership_retained);
        assert_eq!(
            res.readiness,
            Some(SetupReadiness::ContinueSetup {
                step: ContinueSetupStep::ConnectProvider
            })
        );
        assert_eq!(registration_events(&adapter), 0);
    }

    #[test]
    fn provider_authorized_and_descriptor_valid_writes_one_registration_and_opens() {
        let adapter = MemoryAdapter::new();
        let registry = crate::project_registry::MemoryProjectRegistrationStore::new();
        adapter.append(membership_event()).unwrap();
        let req = request(params(), Some("authorized"), Some("valid"), true);
        let res = result_of(&dispatch_with_registry(
            &req,
            &adapter,
            &registry,
            ReplayOutcome::Succeeded,
        ));
        assert_eq!(res.readiness, Some(SetupReadiness::Open));
        assert_eq!(res.registration_ref.as_deref(), Some("reg_ref_opaque"));
        assert_eq!(registration_count(&registry), 1);
        assert_eq!(registration_events(&adapter), 0);
    }

    #[test]
    fn canonical_creator_can_continue_setup_without_auto_join_event() {
        let adapter = MemoryAdapter::new();
        let registry = crate::project_registry::MemoryProjectRegistrationStore::new();
        adapter.append(creation_event()).unwrap();

        let req = request(params(), Some("authorized"), Some("valid"), true);
        let res = result_of(&dispatch_with_registry(
            &req,
            &adapter,
            &registry,
            ReplayOutcome::Succeeded,
        ));

        assert_eq!(res.outcome, ContinueSetupOutcome::Advanced);
        assert!(res.membership_retained);
        assert_eq!(res.readiness, Some(SetupReadiness::Open));
        assert_eq!(registration_events(&adapter), 0);
        let registration = registry.get(PRODUCT, ACCOUNT).unwrap().unwrap();
        assert_eq!(
            registration.registration.governance_policy_version,
            "gpv-created"
        );
    }

    #[test]
    fn provider_denied_is_repair_and_writes_no_registration() {
        let adapter = MemoryAdapter::new();
        adapter.append(membership_event()).unwrap();
        let req = request(params(), Some("denied"), None, true);
        let res = result_of(&dispatch(&req, &adapter, ReplayOutcome::Succeeded));
        assert_eq!(
            res.readiness,
            Some(SetupReadiness::Repair {
                reason: SetupRepairReason::ProviderDenied
            })
        );
        assert!(res.membership_retained);
        assert_eq!(registration_events(&adapter), 0);
    }

    #[test]
    fn invalid_descriptor_is_repair() {
        let adapter = MemoryAdapter::new();
        adapter.append(membership_event()).unwrap();
        let req = request(params(), Some("authorized"), Some("invalid"), true);
        let res = result_of(&dispatch(&req, &adapter, ReplayOutcome::Succeeded));
        assert_eq!(
            res.readiness,
            Some(SetupReadiness::Repair {
                reason: SetupRepairReason::InvalidDescriptor
            })
        );
        assert_eq!(registration_events(&adapter), 0);
    }

    #[test]
    fn unreachable_substrate_is_repair() {
        let adapter = MemoryAdapter::new();
        adapter.append(membership_event()).unwrap();
        let req = request(
            params(),
            Some("authorized"),
            Some("unreachable_substrate"),
            true,
        );
        let res = result_of(&dispatch(&req, &adapter, ReplayOutcome::Succeeded));
        assert_eq!(
            res.readiness,
            Some(SetupReadiness::Repair {
                reason: SetupRepairReason::UnreachableSubstrate
            })
        );
    }

    #[test]
    fn replay_failure_after_write_is_repair_but_registration_persists() {
        let adapter = MemoryAdapter::new();
        let registry = crate::project_registry::MemoryProjectRegistrationStore::new();
        adapter.append(membership_event()).unwrap();
        let req = request(params(), Some("authorized"), Some("valid"), true);
        let res = result_of(&dispatch_with_registry(
            &req,
            &adapter,
            &registry,
            ReplayOutcome::Failed,
        ));
        assert_eq!(
            res.readiness,
            Some(SetupReadiness::Repair {
                reason: SetupRepairReason::ReplayFailed
            })
        );
        // The registration is durable even though replay failed (recoverable).
        assert_eq!(registration_count(&registry), 1);
        assert_eq!(registration_events(&adapter), 0);
        assert_eq!(res.registration_ref.as_deref(), Some("reg_ref_opaque"));
    }

    #[test]
    fn registration_is_idempotent_across_retry_and_restart() {
        let adapter = MemoryAdapter::new();
        let registry = crate::project_registry::MemoryProjectRegistrationStore::new();
        adapter.append(membership_event()).unwrap();
        // First completion writes the registration and opens.
        let first = result_of(&dispatch_with_registry(
            &request(params(), Some("authorized"), Some("valid"), true),
            &adapter,
            &registry,
            ReplayOutcome::Succeeded,
        ));
        assert_eq!(first.readiness, Some(SetupReadiness::Open));

        // Retry with a fresh request id: reuses the existing registration.
        let retry = result_of(&dispatch_with_registry(
            &request(params(), Some("authorized"), Some("valid"), true),
            &adapter,
            &registry,
            ReplayOutcome::Succeeded,
        ));
        assert_eq!(retry.readiness, Some(SetupReadiness::Open));

        // Simulate daemon restart: the operator registry survives while the
        // disposable process state is rebuilt.
        let restart = result_of(&dispatch_with_registry(
            &request(params(), None, None, true),
            &adapter,
            &registry,
            ReplayOutcome::Succeeded,
        ));
        assert_eq!(restart.readiness, Some(SetupReadiness::Open));

        assert_eq!(registration_count(&registry), 1);
        assert_eq!(registration_events(&adapter), 0);
    }

    #[test]
    fn existing_registration_with_failed_replay_resumes_as_repair() {
        let adapter = MemoryAdapter::new();
        let registry = crate::project_registry::MemoryProjectRegistrationStore::new();
        adapter.append(membership_event()).unwrap();
        result_of(&dispatch_with_registry(
            &request(params(), Some("authorized"), Some("valid"), true),
            &adapter,
            &registry,
            ReplayOutcome::Succeeded,
        ));
        // Later, opening the registered substrate fails → deterministic Repair.
        let repaired = result_of(&dispatch_with_registry(
            &request(params(), None, None, true),
            &adapter,
            &registry,
            ReplayOutcome::Failed,
        ));
        assert_eq!(
            repaired.readiness,
            Some(SetupReadiness::Repair {
                reason: SetupRepairReason::ReplayFailed
            })
        );
        assert_eq!(registration_count(&registry), 1);
        assert_eq!(registration_events(&adapter), 0);
    }

    #[test]
    fn continuation_without_membership_is_rejected() {
        let adapter = MemoryAdapter::new();
        // No membership event appended.
        let req = request(params(), Some("authorized"), Some("valid"), true);
        let res = result_of(&dispatch(&req, &adapter, ReplayOutcome::Succeeded));
        assert_eq!(res.outcome, ContinueSetupOutcome::Rejected);
        assert!(res.readiness.is_none());
        assert_eq!(registration_events(&adapter), 0);
    }

    #[test]
    fn missing_probe_context_fails_closed() {
        let adapter = MemoryAdapter::new();
        adapter.append(membership_event()).unwrap();
        let req = request(params(), Some("authorized"), Some("valid"), true);
        let res = dispatch_product_continue_setup(
            &req,
            match &req.command {
                ToolCommand::ProductContinueSetup(p) => p,
                _ => unreachable!(),
            },
            &adapter,
            None,
        );
        assert_eq!(res.status, ToolResponseStatus::Rejected);
        assert_eq!(registration_events(&adapter), 0);
    }

    #[test]
    fn result_and_logs_never_leak_secrets_or_paths() {
        let adapter = MemoryAdapter::new();
        adapter.append(membership_event()).unwrap();
        let res = dispatch(
            &request(params(), Some("authorized"), Some("valid"), true),
            &adapter,
            ReplayOutcome::Succeeded,
        );
        let json = serde_json::to_string(&res).unwrap();
        // The opaque descriptor reference is daemon-local and must not surface
        // in the transport result.
        assert!(!json.contains("descriptor:"));
        assert!(!json.contains("/home/"));
    }
}
