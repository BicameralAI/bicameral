//! Daemon lifecycle — start, run, and shutdown the Bicameral local runtime.
//!
//! Adapted from ZeroClaw's daemon module. The daemon owns the main event loop,
//! spawns the gateway, and coordinates graceful shutdown via signal handlers
//! and a watch channel for in-process reload requests.

use bicameral_api::action_authorization::ActionAuthorizationPolicy;
use bicameral_api::product_creation::HostedActorContext;
use bicameral_api::tool_request::{
    AuthMethod, AuthorityContext, ToolCommand, ToolRequest, ToolResponse,
};
use bicameral_audit::store::AuditStore;
use bicameral_config::{BicameralConfig, BicameralPaths, GatewayConfig};
use bicameral_gateway::state::{LocalProjectRegistration, RuntimeDispatch};
use bicameral_gateway::{serve, AppState};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

const OPERATIONAL_LOG_RETENTION_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Rebuild the hosted-safe projection from the operator registry plus the
/// independent workspace-binding store.
fn project_registrations_from_store(
    router: &crate::project_registry::ProjectEventStoreRouter,
    workspace_bindings_dir: Option<&Path>,
) -> anyhow::Result<Vec<LocalProjectRegistration>> {
    use crate::workspace_binding::WorkspaceBindingStore;
    use bicameral_api::workspace_binding::WorkspaceBindingState;

    let registrations = router.store().list()?;
    let binding_store =
        workspace_bindings_dir.map(crate::workspace_binding::FileWorkspaceBindingStore::new);

    let by_product =
        registrations
            .into_iter()
            .fold(std::collections::BTreeMap::new(), |mut records, record| {
                records
                    .entry(record.registration.product_id.clone())
                    .or_insert(record);
                records
            });
    let projected = by_product
        .into_values()
        .map(|record| {
            let registration = record.registration;
            let (has_workspace_binding, setup_state) = match binding_store.as_ref() {
                Some(store) => match store.load(&registration.product_id) {
                    Ok(Some(binding))
                        if binding.state == WorkspaceBindingState::LocalWorkspaceBound =>
                    {
                        (true, "normal")
                    }
                    Ok(Some(_)) => (true, "local_workspace_repair_required"),
                    Ok(None) => (false, "code_grounding_not_ready"),
                    // A damaged/missing-permission binding record is a repair
                    // condition for that Product. It must not erase the
                    // independently durable ProjectRegistration from the live
                    // registry or make later repair impossible.
                    Err(_) => (false, "local_workspace_repair_required"),
                },
                None => (false, "code_grounding_not_ready"),
            };

            LocalProjectRegistration {
                // The current hosted projection keys selection by `name`; for
                // hydrated registrations this must be the opaque product id,
                // never the descriptor/account label or a local path.
                name: registration.product_id.clone(),
                event_store_healthy: router
                    .adapter_for(&registration.product_id)
                    .and_then(|adapter| {
                        adapter.health_check().map_err(|error| {
                            crate::project_registry::ProjectRegistryError::Io(error.to_string())
                        })
                    })
                    .is_ok_and(|health| health.reachable),
                has_workspace_binding,
                workspace_path: None,
                provider_config: None,
                setup_state: setup_state.to_string(),
            }
        })
        .collect();
    Ok(projected)
}

/// Import routes from the daemon's explicitly configured Product store.
///
/// This is the only compatibility reader for legacy
/// `ProjectRegistrationWritten` events. New registrations are written solely
/// to the operator registry. Canonical Product creation events are also
/// backfilled so Products created before the registry fix become routable.
fn migrate_current_product_routes(
    adapter: &dyn bicameral_api::event_store::EventStoreAdapter,
    route: &crate::project_registry::EventStoreRoute,
    store: &dyn crate::project_registry::ProjectRegistrationStore,
) -> anyhow::Result<usize> {
    let entries = adapter.replay()?;
    let mut migrated = 0;

    let legacy =
        crate::product_registration::LegacyProductRegistrationState::from_entries(&entries);
    for registration in legacy.registrations.into_values() {
        store.write(&crate::project_registry::ProjectRouteRecord {
            registration,
            event_store: route.clone(),
            first_event_commit_proof: None,
        })?;
        migrated += 1;
    }

    let products = crate::product_creation::replay_products(adapter)?;
    for product in products.by_id.into_values() {
        store.write(
            &crate::project_registry::ProjectRouteRecord::from_product_record(
                &product,
                route.clone(),
            ),
        )?;
        migrated += 1;
    }
    Ok(migrated)
}

/// Concrete `RuntimeDispatch` implementation that calls the runtime's
/// `dispatch_with_workspace` with the daemon's event-store adapter.
struct DaemonDispatch {
    adapter: Arc<dyn bicameral_api::event_store::EventStoreAdapter>,
    /// Daemon-owned Ed25519 proof signer over the OS-keyring boundary
    /// (bot#1087). `None` only in deterministic tests; operator-scoped
    /// `product.create` fails closed without it.
    proof_signer: Option<Arc<dyn bicameral_gateway::proof_signing::ProofSigner>>,
    /// True when this process is the user-level registry daemon rather than an
    /// explicit single-workspace compatibility process.
    operator_scoped: bool,
    /// Product-id router backed by the operator-local Project registry.
    project_router: Option<Arc<crate::project_registry::ProjectEventStoreRouter>>,
    /// Reopenable route represented by `adapter`; used when registering a
    /// Product created or joined through this daemon workspace.
    current_event_store_route: Option<crate::project_registry::EventStoreRoute>,
    commit_watcher: crate::commit_watcher::CommitWatcherStore,
    product_commit_watchers: crate::commit_watcher::ProductCommitWatcherStore,
    capability_readiness: Option<Arc<crate::capability_readiness::FileCapabilityReadinessStore>>,
    /// Redacted, bounded operator-local operational diagnostics.
    operational_logs: Option<Arc<crate::operational_log::OperationalLogStore>>,
    /// Operator-local per-project workspace-binding store directory
    /// (`~/.bicameral/workspace-bindings/`). `None` disables `workspace.bind`
    /// (returns typed `unsupported_capability`) when the operator home cannot
    /// be resolved.
    workspace_bindings_dir: Option<PathBuf>,
    /// Live local project registrations consulted before binding.
    project_registrations: Arc<std::sync::RwLock<Vec<LocalProjectRegistration>>>,
    /// Daemon-local trust anchors for `product.join` ticket verification.
    /// `None` when unconfigured, keeping `product.join` fail-closed.
    product_join_trust: Option<Arc<crate::product_join::ProductJoinTrust>>,
    /// Daemon-owned, in-memory `ConfirmationChallenge` authority (issue #829).
    ///
    /// The sole minter/validator/consumer of single-use promotion challenges.
    /// It is transient: a fresh store is constructed per daemon process start,
    /// so a restart rotates the epoch and invalidates every pending challenge.
    /// Threaded into `recall.promote_decision_candidate` so the shipped daemon
    /// enforces the two-phase confirmation flow over `/v2/tool-requests`.
    confirmation_challenges: Arc<crate::confirmation_challenge_store::ConfirmationChallengeStore>,
    /// Daemon-owned CandidateSet session + selection stack (issues #987 / #989).
    ///
    /// Transient session store lives inside the service. Authority snapshots are
    /// projected only from trusted Product membership/policy boundaries — never
    /// from MCP request payloads. Process-bound atomic adapter backs commit when
    /// no product-routed ledger is resolved.
    ///
    /// Held on the dispatch struct so prepare/commit can reproject live
    /// membership and so tests/tools can inspect projection state.
    candidate_selection_authority:
        Arc<crate::candidate_selection_authority::CandidateSelectionAuthorityRegistry>,
    /// Daemon-owned product currentness (spec binding + accepted model base).
    product_currentness: Arc<crate::candidate_selection_authority::ProductCurrentnessStore>,
    candidate_selection: Arc<crate::candidate_selection_service::CandidateSelectionService>,
    /// Process-bound atomic event store (same concrete instance as `adapter`
    /// when dual-opened at startup). Used for selection commit without product
    /// routing, and as the service default ledger.
    atomic_adapter: Arc<dyn crate::atomic_event_store::AtomicEventStoreAdapter>,
    projection_work_store: Arc<bicameral_gateway::live_projection_work::LiveProjectionWorkStore>,
    candidates: Arc<
        tokio::sync::RwLock<
            std::collections::HashMap<uuid::Uuid, bicameral_api::candidate::DecisionCandidate>,
        >,
    >,
    reviews: Arc<
        tokio::sync::RwLock<
            std::collections::HashMap<uuid::Uuid, bicameral_api::review::ReviewState>,
        >,
    >,
}

struct ResolvedRequestRoute {
    adapter: Arc<dyn bicameral_api::event_store::EventStoreAdapter>,
    workspace_root: Option<PathBuf>,
}

impl crate::dispatch::CandidateSnapshotProvider
    for bicameral_gateway::live_projection_work::LiveProjectionWorkStore
{
    fn candidate_snapshot(
        &self,
        candidate_id: uuid::Uuid,
    ) -> Result<Option<bicameral_api::candidate::DecisionCandidate>, String> {
        self.candidate_payload(candidate_id)
            .map_err(|error| error.to_string())
    }
}

impl RuntimeDispatch for DaemonDispatch {
    fn dispatch(&self, request: &ToolRequest, workspace_root: Option<&Path>) -> ToolResponse {
        // Product-scoped operator routing (search/bind/ingest after product.select)
        // must leave the same redacted operational fingerprint evidence as
        // product.create/publish. Topology multi-product-routing asserts these
        // receipts; previously only create/publish wrote them.
        let response = self.dispatch_body(request, workspace_root);
        self.record_product_scoped_operational_log(request, &response);
        response
    }

    fn capabilities(&self) -> serde_json::Value {
        // `workspace.bind` is only routable when the daemon resolved an
        // operator-local workspace-binding store; capability discovery must
        // advertise availability truthfully (bicameral-bot#747).
        let report = crate::dispatch::capability_report_with_workspace_binding(
            bicameral_api::reasoning_projection::ProjectionProviderReadiness::not_configured(),
            self.workspace_bindings_dir.is_some(),
        );
        serde_json::to_value(report).expect("CapabilityReport serializes to JSON")
    }

    fn capability_readiness(
        &self,
    ) -> Result<Vec<bicameral_api::capability_readiness::DaemonCapabilityReadinessV1>, String> {
        let Some(store) = &self.capability_readiness else {
            return Ok(Vec::new());
        };
        store.list().map_err(|error| error.to_string())
    }

    fn submit_mcp_readiness(
        &self,
        observation: bicameral_api::capability_readiness::McpReadinessObservation,
    ) -> Result<bicameral_api::capability_readiness::DaemonCapabilityReadinessV1, String> {
        let Some(store) = &self.capability_readiness else {
            return Err("unsupported_capability: MCP readiness reducer unavailable".into());
        };
        store.submit(observation).map_err(|error| error.to_string())
    }

    fn replay_projection(&self) -> Option<bicameral_gateway::state::ReplayedProjection> {
        if self.operator_scoped {
            // The operator daemon has no process-global Product projection.
            // Project-scoped reads replay through the request's routed adapter.
            return None;
        }
        // The event store is canonical; the gateway's read models are a
        // disposable projection of it. Rebuild them from replay so a Decision
        // materialized over `/v2/tool-requests` renders in the read-only Ledger
        // View without a restart. Replay failure leaves the projection stale
        // rather than clearing it — restart hydration recovers either way.
        match crate::replay::replay_from_adapter(self.adapter.as_ref()) {
            Ok(rs) => Some(bicameral_gateway::state::ReplayedProjection {
                sources: rs.sources,
                snapshots: rs.snapshots,
                evidence: rs.evidence,
                candidates: rs.candidates,
                decisions: rs.decisions,
                bindings: rs.bindings,
                binding_content_hashes: rs.binding_content_hashes,
            }),
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "Could not refresh Ledger projection after write; restart replay will recover"
                );
                None
            }
        }
    }
}

impl DaemonDispatch {
    fn record_product_scoped_operational_log(
        &self,
        request: &ToolRequest,
        response: &ToolResponse,
    ) {
        let should_record = request.product_id.is_some()
            || matches!(
                request.command,
                ToolCommand::ProductCreate(_) | ToolCommand::ProductPublish(_)
            );
        if !should_record {
            return;
        }
        let Some(logs) = &self.operational_logs else {
            return;
        };
        // Substrate kind is diagnostic metadata only; multi-product routes share
        // the operator daemon process adapter when a product-specific adapter is
        // not threaded through every early return.
        if let Err(error) =
            logs.record_product_command(request, response, self.adapter.substrate_kind())
        {
            tracing::warn!(
                request_id = %response.request_id,
                error_kind = ?error.kind(),
                "Operational diagnostic receipt write failed"
            );
        }
    }

    fn dispatch_body(&self, request: &ToolRequest, workspace_root: Option<&Path>) -> ToolResponse {
        let watcher_observation = match request.product_id.as_deref() {
            Some(product_id) => self.product_commit_watchers.latest_for(product_id),
            None => self.commit_watcher.latest(),
        };

        // Workspace bind/resolve need the daemon's operator-local registry +
        // store. Build a point-in-time context only for those commands; all
        // other commands take the standard path.
        if matches!(
            request.command,
            ToolCommand::WorkspaceBind(_) | ToolCommand::WorkspaceResolve(_)
        ) {
            if let ToolCommand::WorkspaceBind(bind_request) = &request.command {
                if request
                    .product_id
                    .as_deref()
                    .is_some_and(|product_id| product_id != bind_request.proposal.project_id)
                {
                    return ToolResponse::rejected(
                        request.request_id,
                        "product_context_mismatch: ToolRequest.product_id does not match workspace.bind proposal project_id"
                            .into(),
                    );
                }
            }
            if let Some(dir) = &self.workspace_bindings_dir {
                let registered: Vec<String> = self
                    .project_registrations
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .iter()
                    .map(|r| r.name.clone())
                    .collect();
                let registry =
                    crate::workspace_binding::StaticWorkspaceBindingRegistry::new(registered, None);
                let store = crate::workspace_binding::FileWorkspaceBindingStore::new(dir.clone());
                let ctx = crate::dispatch::WorkspaceBindingDispatchContext {
                    registry: &registry,
                    store: &store,
                };
                let response = crate::dispatch::dispatch_with_workspace_binding(
                    request,
                    self.adapter.as_ref(),
                    workspace_root,
                    watcher_observation.as_ref(),
                    &ctx,
                );
                if matches!(request.command, ToolCommand::WorkspaceBind(_))
                    && response.status == bicameral_api::tool_request::ToolResponseStatus::Ok
                {
                    self.refresh_project_registrations();
                }
                return response;
            }
        }

        // Product Member auto-join needs the daemon's configured trust anchors
        // to verify the signed ticket. Build a point-in-time context only for
        // `product.join`; when unconfigured the command falls through and fails
        // closed with `unsupported_capability`.
        if let ToolCommand::ProductJoin(params) = &request.command {
            if let Some(trust) = &self.product_join_trust {
                let product_id = match crate::product_join::authenticated_routing_product_id(
                    request, params, trust,
                ) {
                    Ok(product_id) => product_id,
                    Err(response) => return *response,
                };
                let mut routed_request = request.clone();
                routed_request.product_id = Some(product_id);
                let route = match self.resolve_request_route(&routed_request, workspace_root) {
                    Ok(route) => route,
                    Err(response) => return *response,
                };
                let ctx = crate::product_join::ProductJoinDispatchContext { trust };
                return crate::dispatch::dispatch_with_product_join(
                    &routed_request,
                    route.adapter.as_ref(),
                    route.workspace_root.as_deref(),
                    watcher_observation.as_ref(),
                    &ctx,
                );
            }
        }

        // Local setup continuation (issue #769) opens + replays the registered
        // Product. The daemon supplies a replay probe over its event-store
        // adapter; membership, provider access, and descriptor facts are read
        // from canonical state / daemon-set metadata inside the handler.
        if let ToolCommand::ProductContinueSetup(params) = &request.command {
            let (Some(router), Some(current_route)) =
                (&self.project_router, &self.current_event_store_route)
            else {
                return ToolResponse::rejected(
                    request.request_id,
                    "product_registry_unavailable: product.continue_setup requires an operator registry and a reopenable current event store"
                        .into(),
                );
            };
            let setup_adapter = match router.adapter_for(&params.product_id) {
                Ok(adapter) => adapter,
                Err(crate::project_registry::ProjectRegistryError::NotRegistered { .. }) => {
                    self.adapter.clone()
                }
                Err(error) => {
                    return ToolResponse::error(
                        request.request_id,
                        format!("daemon_unavailable: Product route resolution failed: {error}"),
                    );
                }
            };
            let probe = DaemonReplayProbe {
                router: router.clone(),
            };
            let ctx = crate::product_registration::ProductSetupDispatchContext {
                replay_probe: &probe,
                registration_store: router.store(),
                event_store_route: current_route,
            };
            let response = crate::dispatch::dispatch_with_product_setup(
                request,
                setup_adapter.as_ref(),
                workspace_root,
                watcher_observation.as_ref(),
                &ctx,
            );
            if response.status == bicameral_api::tool_request::ToolResponseStatus::Ok
                && response
                    .result
                    .as_ref()
                    .and_then(|value| value.get("registration_ref"))
                    .is_some_and(|value| !value.is_null())
            {
                self.refresh_project_registrations();
            }
            return response;
        }

        // Hosted Product creation needs the daemon-confirmed hosted actor
        // context derived from the validated bridge session. Authority is never
        // caller-supplied: the account binding, paired state, and authorization
        // come from the daemon's view of the session (`AuthMethod::HostedBridge`
        // authority is only minted by the gateway after validating a paired
        // session). Local/non-hosted surfaces get no actor context and receive
        // a typed `missing_actor_context` denial (#766).
        if matches!(
            request.command,
            ToolCommand::ProductCreate(_) | ToolCommand::ProductPublish(_)
        ) {
            // Operator-scoped create (#1264): the daemon has no process-global
            // durable store, so the create must carry a Google Drive folder
            // descriptor that THIS daemon validated through the shipped
            // resource-selection flow. The validated route becomes both the
            // adapter the first event is appended to and the persisted,
            // reopenable Product route. Fail closed on absent, malformed, or
            // unvalidated descriptors; the browser can transport a descriptor
            // but never mint one.
            let mut operator_create_route: Option<crate::project_registry::EventStoreRoute> = None;
            if self.operator_scoped {
                if let ToolCommand::ProductCreate(params) = &request.command {
                    match crate::product_creation::operator_create_route(request, params) {
                        Ok(route) => operator_create_route = Some(route),
                        Err(rejection) => {
                            return ToolResponse::rejected(request.request_id, rejection.into());
                        }
                    }
                }
            }
            let mut create_commit: Option<crate::product_creation::ProductCreateCommit> = None;
            let routed_adapter = match &request.command {
                ToolCommand::ProductPublish(params) => {
                    if request
                        .product_id
                        .as_deref()
                        .is_some_and(|product_id| product_id != params.product_id)
                    {
                        return ToolResponse::rejected(
                            request.request_id,
                            "product_context_mismatch: ToolRequest.product_id does not match product.publish product_id"
                                .into(),
                        );
                    }
                    let Some(router) = &self.project_router else {
                        let response = ToolResponse::rejected(
                            request.request_id,
                            "product_registry_unavailable: daemon has no operator-local Product registry"
                                .into(),
                        );
                        return self.record_product_completion(
                            request,
                            response,
                            self.adapter.substrate_kind(),
                        );
                    };
                    match router.adapter_for(&params.product_id) {
                        Ok(adapter) => adapter,
                        Err(crate::project_registry::ProjectRegistryError::NotRegistered {
                            ..
                        }) => {
                            let response = ToolResponse::rejected(
                                request.request_id,
                                "product_not_registered: canonical Product route is absent from the operator registry; reopen the Product workspace once to repair registration"
                                    .into(),
                            );
                            return self.record_product_completion(
                                request,
                                response,
                                self.adapter.substrate_kind(),
                            );
                        }
                        Err(error) => {
                            let response = ToolResponse::error(
                                request.request_id,
                                format!(
                                    "daemon_unavailable: Product route resolution failed: {error}"
                                ),
                            );
                            return self.record_product_completion(
                                request,
                                response,
                                self.adapter.substrate_kind(),
                            );
                        }
                    }
                }
                ToolCommand::ProductCreate(_) => match &operator_create_route {
                    // Operator-scoped create (bot#1087): the daemon-validated
                    // Drive route must open as the EXACT provider adapter with
                    // readback, and the daemon signer must exist, before any
                    // append. No local, memory, or fixture substrate can
                    // satisfy this path.
                    Some(route) => {
                        let (adapter, confirmed_append) = route.open_for_create();
                        let Some(confirmed_append) = confirmed_append else {
                            return ToolResponse::rejected(
                                request.request_id,
                                "product_event_store_unconfirmable: the validated Google Drive route could not be opened as the provider-confirmed event store; reconnect Google and retry product.create with the same idempotency key"
                                    .into(),
                            );
                        };
                        let Some(signer) = self.proof_signer.clone() else {
                            return ToolResponse::error(
                                request.request_id,
                                "proof_signer_unavailable: daemon proof-signing key is not available; no canonical event was written"
                                    .into(),
                            );
                        };
                        create_commit = Some(crate::product_creation::ProductCreateCommit {
                            confirmed_append,
                            signer,
                        });
                        adapter
                    }
                    None => self.adapter.clone(),
                },
                _ => unreachable!(),
            };
            let product_ctx = crate::dispatch::HostedProductDispatchContext {
                actor: hosted_actor_from_authority(&request.authority),
                commit: create_commit,
            };
            let response = crate::dispatch::dispatch_with_hosted_product(
                request,
                routed_adapter.as_ref(),
                workspace_root,
                watcher_observation.as_ref(),
                &product_ctx,
            );
            let response = if response.status == bicameral_api::tool_request::ToolResponseStatus::Ok
            {
                match &request.command {
                    ToolCommand::ProductCreate(params) => self.persist_created_product_route(
                        request,
                        params,
                        response,
                        operator_create_route.clone(),
                    ),
                    ToolCommand::ProductPublish(params) => {
                        self.attach_publication_commit_proof(request, params, response)
                    }
                    _ => response,
                }
            } else {
                response
            };
            let error_code = crate::operational_log::safe_error_code(&response);
            tracing::info!(
                event = "product_command_completed",
                request_id = %response.request_id,
                command = request.command.command_name(),
                status = ?response.status,
                error_code = error_code.as_deref().unwrap_or("none"),
                event_store_substrate = %routed_adapter.substrate_kind(),
                "Product command completed"
            );
            // Operational receipts are written once by the outer dispatch wrapper.
            return response;
        }

        if matches!(
            request.command,
            ToolCommand::CandidateHide(_) | ToolCommand::CandidateSuppress(_)
        ) {
            if self.operator_scoped {
                return ToolResponse::rejected(
                    request.request_id,
                    "product_working_state_not_scoped: candidate hide/suppress is unavailable in operator mode until daemon-local working state is keyed by product_id"
                        .into(),
                );
            }
            if let Err(denial) =
                ActionAuthorizationPolicy::evaluate(&request.authority, &request.command)
            {
                return ToolResponse::rejected(
                    request.request_id,
                    format!("{}: {}", denial.rejection_code(), denial.message),
                );
            }
        }

        match &request.command {
            ToolCommand::CandidateHide(params) => {
                let candidate_id = match uuid::Uuid::parse_str(&params.candidate_id) {
                    Ok(value) => value,
                    Err(_) => {
                        return ToolResponse::rejected(
                            request.request_id,
                            "unknown_target: candidate_id is not a valid identifier".into(),
                        );
                    }
                };
                let scope = request
                    .authority
                    .session_id
                    .as_ref()
                    .map(|session| format!("session:{session}"))
                    .unwrap_or_else(|| format!("actor:{}", request.authority.actor_id));
                return match self
                    .projection_work_store
                    .hide_candidate(&scope, candidate_id)
                {
                    Ok(true) => ToolResponse::success(
                        request.request_id,
                        serde_json::json!({
                            "candidate_id": candidate_id,
                            "disposition": "hidden",
                            "scope": scope,
                            "persistence": "daemon_session",
                        }),
                    ),
                    Ok(false) => ToolResponse::rejected(
                        request.request_id,
                        format!("unknown_target: candidate '{candidate_id}' does not exist"),
                    ),
                    Err(error) => ToolResponse::error(
                        request.request_id,
                        format!("daemon_unavailable: candidate hide failed: {error}"),
                    ),
                };
            }
            ToolCommand::CandidateSuppress(params) => {
                if params.reason.trim().is_empty() {
                    return ToolResponse::rejected(
                        request.request_id,
                        "malformed_payload: suppression reason must not be empty".into(),
                    );
                }
                let candidate_id = match uuid::Uuid::parse_str(&params.candidate_id) {
                    Ok(value) => value,
                    Err(_) => {
                        return ToolResponse::rejected(
                            request.request_id,
                            "unknown_target: candidate_id is not a valid identifier".into(),
                        );
                    }
                };
                let mut candidates = match self.candidates.try_write() {
                    Ok(guard) => guard,
                    Err(_) => {
                        return ToolResponse::error(
                            request.request_id,
                            "retryable_state_busy: candidate projection is currently being updated"
                                .into(),
                        );
                    }
                };
                let mut reviews = match self.reviews.try_write() {
                    Ok(guard) => guard,
                    Err(_) => {
                        return ToolResponse::error(
                            request.request_id,
                            "retryable_state_busy: candidate review state is currently being updated".into(),
                        );
                    }
                };
                return match self.projection_work_store.suppress_candidate(
                    candidate_id,
                    &params.reason,
                    params.expires_at,
                    chrono::Utc::now(),
                ) {
                    Ok(Some(suppression)) => {
                        candidates.remove(&candidate_id);
                        reviews.remove(&candidate_id);
                        ToolResponse::success(
                        request.request_id,
                        serde_json::json!({
                            "candidate_id": candidate_id,
                            "disposition": "suppressed",
                            "semantic_fingerprint": suppression.semantic_fingerprint,
                            "expires_at": suppression.expires_at,
                            "persistence": "daemon_local_working_state",
                            "invalidation": [
                                "evidence_acceptance_key",
                                "source_uri",
                                "content_digest",
                                "semantic_fingerprint",
                                "expiry"
                            ],
                        }),
                    )
                    }
                    Ok(None) => ToolResponse::rejected(
                        request.request_id,
                        format!("unknown_target: candidate '{candidate_id}' does not exist"),
                    ),
                    Err(bicameral_gateway::live_projection_work::LiveProjectionWorkError::InvalidSuppressionExpiry) => {
                        ToolResponse::rejected(
                            request.request_id,
                            "invalid_parameter: suppression expiry must be in the future".into(),
                        )
                    }
                    Err(error) => ToolResponse::error(
                        request.request_id,
                        format!("daemon_unavailable: candidate suppression failed: {error}"),
                    ),
                };
            }
            _ => {}
        }

        let route = match self.resolve_request_route(request, workspace_root) {
            Ok(route) => route,
            Err(response) => return *response,
        };
        let routed_workspace_root = route.workspace_root;
        let workspace_root = routed_workspace_root.as_deref();
        let routed_adapter = route.adapter;
        if matches!(
            request.command,
            ToolCommand::ReviewAcceptCandidate(_) | ToolCommand::ReviewRejectCandidate(_)
        ) {
            return crate::dispatch::dispatch_candidate_review_with_provider(
                request,
                routed_adapter.as_ref(),
                self.projection_work_store.as_ref(),
            );
        }

        // Candidate promotion runs the daemon-owned single-use confirmation
        // flow (issue #829): the daemon is the sole authority that mints,
        // validates, and single-use-consumes the challenge. Threading the
        // daemon-local challenge store here makes `/v2/tool-requests` promotion
        // require an explicit confirmed resubmission before any canonical write.
        if matches!(
            request.command,
            ToolCommand::RecallPromoteDecisionCandidate(_)
        ) {
            return crate::dispatch::dispatch_with_confirmation_challenges(
                request,
                routed_adapter.as_ref(),
                workspace_root,
                watcher_observation.as_ref(),
                self.confirmation_challenges.as_ref(),
            );
        }

        // Managed MCP CandidateSet selection/session (#987/#989): prepare,
        // commit, heartbeat, and end all need the daemon-local session store
        // and selection service. Commit prefers a product-routed atomic ledger.
        // Prepare/commit reproject live Product membership + currentness before
        // the service evaluates may_promote / owner / drift.
        if matches!(
            request.command,
            ToolCommand::CandidateSelectionPrepare(_)
                | ToolCommand::CandidateSelectionCommit(_)
                | ToolCommand::CandidateSessionHeartbeat(_)
                | ToolCommand::CandidateSessionEnd(_)
        ) {
            self.refresh_selection_authority(request, routed_adapter.as_ref());
            let product_atomic = request.product_id.as_deref().and_then(|product_id| {
                self.project_router
                    .as_ref()
                    .and_then(|router| router.atomic_adapter_for(product_id).ok())
            });
            let atomic_ref: &dyn crate::atomic_event_store::AtomicEventStoreAdapter =
                match product_atomic.as_ref() {
                    Some(store) => store.as_ref(),
                    None => self.atomic_adapter.as_ref(),
                };
            let selection_ctx = crate::dispatch::CandidateSelectionDispatchContext {
                service: self.candidate_selection.as_ref(),
                atomic_event_store: Some(atomic_ref),
            };
            return crate::dispatch::dispatch_with_candidate_selection(
                request,
                routed_adapter.as_ref(),
                workspace_root,
                watcher_observation.as_ref(),
                &selection_ctx,
            );
        }

        // Search must see daemon-local working candidates: external-ingest
        // proposals have no candidate-bearing canonical event to replay from.
        if matches!(request.command, ToolCommand::SearchQuery(_)) {
            let working = self
                .projection_work_store
                .active_candidate_payloads(chrono::Utc::now())
                .unwrap_or_else(|error| {
                    tracing::warn!(
                        error = %error,
                        "Working-candidate load failed; search covers canonical state only"
                    );
                    Vec::new()
                });
            return crate::dispatch::dispatch_with_working_candidates(
                request,
                routed_adapter.as_ref(),
                workspace_root,
                watcher_observation.as_ref(),
                working,
            );
        }

        crate::dispatch::dispatch_with_workspace_and_commit_watcher(
            request,
            routed_adapter.as_ref(),
            workspace_root,
            watcher_observation.as_ref(),
        )
    }

    /// Reproject CandidateSet selection authority from trusted Product replay
    /// and daemon product currentness before prepare/commit.
    ///
    /// Heartbeat/end do not require promotion rights; they are skipped.
    /// Projection failures leave the registry without a grant so the service
    /// fails closed with `actor_unauthorized` rather than inventing authority.
    fn refresh_selection_authority(
        &self,
        request: &ToolRequest,
        event_store: &dyn bicameral_api::event_store::EventStoreAdapter,
    ) {
        let (product_id, actor_id, set_binding) = match &request.command {
            ToolCommand::CandidateSelectionPrepare(selection) => {
                let access = bicameral_api::candidate_session::CandidateSessionAccessV1::new(
                    selection.product_id.clone(),
                    selection.host_session_id.clone(),
                    selection.session_lease_id.clone(),
                    selection.candidate_set_generation,
                );
                let binding = self
                    .candidate_selection
                    .sessions()
                    .enumerate(&access, chrono::Utc::now())
                    .ok()
                    .map(|set| set.binding);
                (
                    selection.product_id.clone(),
                    selection.actor_id.clone(),
                    binding,
                )
            }
            ToolCommand::CandidateSelectionCommit(commit) => {
                let product_id = request
                    .product_id
                    .clone()
                    .or_else(|| {
                        self.candidate_selection
                            .preview_product_id(commit.selection_operation_id)
                    })
                    .unwrap_or_default();
                (product_id, commit.actor_id.clone(), None)
            }
            _ => return,
        };
        if product_id.trim().is_empty() || actor_id.trim().is_empty() {
            return;
        }
        let _ = crate::candidate_selection_authority::project_live_authority(
            self.candidate_selection_authority.as_ref(),
            self.product_currentness.as_ref(),
            event_store,
            &product_id,
            &actor_id,
            set_binding.as_ref(),
        );
    }

    fn resolve_request_route(
        &self,
        request: &ToolRequest,
        compatibility_workspace_root: Option<&Path>,
    ) -> Result<ResolvedRequestRoute, Box<ToolResponse>> {
        let Some(product_id) = request.product_id.as_deref() else {
            if self.operator_scoped {
                return Err(Box::new(ToolResponse::rejected(
                    request.request_id,
                    "product_context_required: operator-scoped daemon requests must carry an explicit canonical product_id"
                        .into(),
                )));
            }
            return Ok(ResolvedRequestRoute {
                adapter: self.adapter.clone(),
                workspace_root: compatibility_workspace_root.map(Path::to_path_buf),
            });
        };

        let Some(router) = &self.project_router else {
            return Err(Box::new(ToolResponse::rejected(
                request.request_id,
                "product_registry_unavailable: daemon has no operator-local Product registry"
                    .into(),
            )));
        };
        let adapter = match router.adapter_for(product_id) {
            Ok(adapter) => adapter,
            Err(crate::project_registry::ProjectRegistryError::NotRegistered { .. }) => {
                return Err(Box::new(ToolResponse::rejected(
                    request.request_id,
                    format!(
                        "product_not_registered: '{product_id}' is absent from the operator registry"
                    ),
                )));
            }
            Err(error) => {
                return Err(Box::new(ToolResponse::error(
                    request.request_id,
                    format!("daemon_unavailable: Product route resolution failed: {error}"),
                )));
            }
        };

        let workspace_root = self
            .workspace_bindings_dir
            .as_ref()
            .and_then(|dir| {
                use crate::workspace_binding::WorkspaceBindingStore;
                let store = crate::workspace_binding::FileWorkspaceBindingStore::new(dir);
                store.load(product_id).ok().flatten()
            })
            .filter(|binding| {
                binding.state
                    == bicameral_api::workspace_binding::WorkspaceBindingState::LocalWorkspaceBound
            })
            .map(|binding| PathBuf::from(binding.candidate_path));

        Ok(ResolvedRequestRoute {
            adapter,
            workspace_root,
        })
    }

    fn record_product_completion(
        &self,
        request: &ToolRequest,
        response: ToolResponse,
        substrate: bicameral_api::event_store::SubstrateKind,
    ) -> ToolResponse {
        let error_code = crate::operational_log::safe_error_code(&response);
        tracing::info!(
            event = "product_command_completed",
            request_id = %response.request_id,
            command = request.command.command_name(),
            status = ?response.status,
            error_code = error_code.as_deref().unwrap_or("none"),
            event_store_substrate = %substrate,
            "Product command completed"
        );
        // Operational receipts are written once by the outer dispatch wrapper.
        response
    }

    fn persist_created_product_route(
        &self,
        request: &ToolRequest,
        params: &bicameral_api::product_creation::ProductCreateParams,
        response: ToolResponse,
        validated_route: Option<crate::project_registry::EventStoreRoute>,
    ) -> ToolResponse {
        let Some(router) = &self.project_router else {
            return ToolResponse::error(
                request.request_id,
                "daemon_unavailable: canonical Product was created but the operator registry is unavailable; retry product.create with the same idempotency key"
                    .into(),
            );
        };
        // Operator-scoped creates persist the daemon-validated Drive route
        // (#1264); workspace-scoped creates keep persisting the configured
        // workspace event-store route.
        let operator_scoped_create = validated_route.is_some();
        let Some(route) = validated_route.or_else(|| self.current_event_store_route.clone()) else {
            return ToolResponse::error(
                request.request_id,
                "daemon_unavailable: canonical Product was created in a non-reopenable event store; retry product.create after configuring durable storage"
                    .into(),
            );
        };
        let Some(value) = response
            .result
            .as_ref()
            .and_then(|value| value.get("result"))
        else {
            return ToolResponse::error(
                request.request_id,
                "runtime_error: canonical Product creation response omitted its result".into(),
            );
        };
        let result = match serde_json::from_value::<
            bicameral_api::product_creation::ProductCreationResult,
        >(value.clone())
        {
            Ok(result) => result,
            Err(error) => {
                return ToolResponse::error(
                    request.request_id,
                    format!("runtime_error: canonical Product result was invalid: {error}"),
                );
            }
        };
        // The signed, provider-confirmed commit proof travels with the route
        // record so `product.publish` returns the same proof after restart
        // (bot#1087). An operator-scoped create without one is not a success.
        let minted_proof = match response
            .result
            .as_ref()
            .and_then(|value| value.get("first_event_commit_proof"))
            .filter(|value| !value.is_null())
            .cloned()
        {
            None => None,
            Some(value) => match serde_json::from_value::<
                bicameral_api::product_creation::ProductCreationCommitProof,
            >(value)
            {
                Ok(proof) => Some(proof),
                Err(error) => {
                    return ToolResponse::error(
                        request.request_id,
                        format!(
                            "runtime_error: commit proof in the creation response was invalid: {error}"
                        ),
                    );
                }
            },
        };
        if operator_scoped_create && minted_proof.is_none() {
            return ToolResponse::error(
                request.request_id,
                "commit_proof_missing: operator-scoped product.create completed without a provider-confirmed commit proof; retry product.create with the same idempotency key"
                    .into(),
            );
        }
        // A retry converges on the proof already recorded for this Product
        // rather than replacing it with a re-confirmation of the same facts.
        let recorded_proof = router
            .store()
            .find_product(&result.product_id)
            .ok()
            .flatten()
            .and_then(|existing| existing.first_event_commit_proof)
            .filter(|proof| proof.product_id == result.product_id);
        let mut record =
            crate::project_registry::ProjectRouteRecord::from_creation(&result, params, route);
        record.first_event_commit_proof = recorded_proof.or(minted_proof);
        let mut response = response;
        if let (Some(proof), Some(result_value)) =
            (&record.first_event_commit_proof, response.result.as_mut())
        {
            if let Ok(value) = serde_json::to_value(proof) {
                result_value["first_event_commit_proof"] = value;
            }
        }
        match router.store().write(&record) {
            Ok(()) => response,
            Err(error) => ToolResponse::error(
                request.request_id,
                format!(
                    "daemon_unavailable: canonical Product was created but its operator route could not be persisted: {error}; retry product.create with the same idempotency key"
                ),
            ),
        }
    }

    /// Attach the Product's signed commit proof to a successful
    /// `product.publish` response (bot#1087).
    ///
    /// The proof recorded at create time is returned as-is. A Google Drive
    /// API route without a recorded proof (created before the producer, or
    /// interrupted before persistence) is reconciled by re-confirming the
    /// exact canonical `CreateProduct` object on the provider and minting
    /// over those facts; nothing is appended. Reconciliation failure is a
    /// typed error, never a publication that silently omits the proof.
    /// Non-v2 substrates (Git, synced folder) publish without a proof.
    fn attach_publication_commit_proof(
        &self,
        request: &ToolRequest,
        params: &bicameral_api::product_creation::ProductPublishParams,
        mut response: ToolResponse,
    ) -> ToolResponse {
        let Some(router) = &self.project_router else {
            return response;
        };
        let Ok(Some(mut record)) = router.store().find_product(&params.product_id) else {
            return response;
        };
        let proof = match record.first_event_commit_proof.clone() {
            Some(proof) if proof.product_id == params.product_id => proof,
            _ => {
                if !matches!(
                    record.event_store,
                    crate::project_registry::EventStoreRoute::GoogleDriveApi { .. }
                ) {
                    return response;
                }
                let Some(signer) = self.proof_signer.clone() else {
                    return ToolResponse::error(
                        request.request_id,
                        "proof_signer_unavailable: daemon proof-signing key is not available; the publication cannot carry a commit proof"
                            .into(),
                    );
                };
                let (adapter, confirmed_append) = record.event_store.open_for_create();
                let Some(confirmed_append) = confirmed_append else {
                    return ToolResponse::error(
                        request.request_id,
                        "product_event_store_unconfirmable: the Product's Google Drive route could not be opened as the provider-confirmed event store; reconnect Google and retry product.publish"
                            .into(),
                    );
                };
                let commit = crate::product_creation::ProductCreateCommit {
                    confirmed_append,
                    signer,
                };
                match crate::product_creation::reconcile_commit_proof(
                    &params.product_id,
                    adapter.as_ref(),
                    &commit,
                ) {
                    Ok(Some(proof)) => {
                        record.first_event_commit_proof = Some(proof.clone());
                        if let Err(error) = router.store().write(&record) {
                            tracing::warn!(
                                event = "commit_proof_persist_failed",
                                error = %error,
                                "Reconciled commit proof could not be persisted; it will be reconciled again on the next publish"
                            );
                        }
                        proof
                    }
                    Ok(None) => return response,
                    Err(error) => {
                        return ToolResponse::error(
                            request.request_id,
                            format!(
                                "commit_proof_unavailable: {}; retry product.publish once the provider is reachable",
                                error.safe_message()
                            ),
                        );
                    }
                }
            }
        };
        if let Some(publication) = response
            .result
            .as_mut()
            .and_then(|value| value.get_mut("publication"))
        {
            if let Ok(value) = serde_json::to_value(&proof) {
                publication["first_event_commit_proof"] = value;
            }
        }
        response
    }

    /// Reconcile the live bridge projection after a durable registration or
    /// workspace-binding mutation. Failure never changes the canonical command
    /// outcome; the warning is redaction-safe and restart hydration retries the
    /// projection from the same durable stores.
    fn refresh_project_registrations(&self) {
        let Some(router) = &self.project_router else {
            return;
        };
        match project_registrations_from_store(router, self.workspace_bindings_dir.as_deref()) {
            Ok(registrations) => {
                let count = registrations.len();
                *self
                    .project_registrations
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = registrations;
                tracing::debug!(count, "Refreshed durable project registration readiness");
            }
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "Could not refresh project registration readiness; restart hydration will retry"
                );
            }
        }
    }
}

/// Daemon replay probe for `product.continue_setup`: opens the registered
/// Product's event-store adapter and replays canonical state.
///
/// Resolution is keyed by canonical Product id through the operator registry;
/// it never depends on the daemon process cwd or its default adapter.
struct DaemonReplayProbe {
    router: Arc<crate::project_registry::ProjectEventStoreRouter>,
}

impl crate::product_registration::ProductReplayProbe for DaemonReplayProbe {
    fn probe(
        &self,
        registration: &bicameral_api::product_registration::ProjectRegistration,
    ) -> bicameral_api::product_registration::ReplayOutcome {
        match self
            .router
            .adapter_for(&registration.product_id)
            .and_then(|adapter| {
                adapter.replay().map(|_| ()).map_err(|error| {
                    crate::project_registry::ProjectRegistryError::Io(error.to_string())
                })
            }) {
            Ok(_) => bicameral_api::product_registration::ReplayOutcome::Succeeded,
            Err(_) => bicameral_api::product_registration::ReplayOutcome::Failed,
        }
    }
}

/// Derive the daemon-confirmed hosted actor context from a request's authority.
///
/// Only a [`AuthMethod::HostedBridge`] authority yields a context: that
/// authority is minted by the gateway solely from a validated, paired bridge
/// session, so reaching dispatch with it means the session is paired and
/// authorized for its own Cloud account. The bound Cloud account is read from
/// the daemon-derived `actor_id` (`bridge:<cloud_account_id>`), never from
/// caller-supplied fields. Any other auth method returns `None`, which the
/// handler surfaces as a typed `missing_actor_context` denial (#766).
fn hosted_actor_from_authority(authority: &AuthorityContext) -> Option<HostedActorContext> {
    if authority.auth_method != AuthMethod::HostedBridge {
        return None;
    }
    let cloud_account_id = authority
        .actor_id
        .strip_prefix("bridge:")
        .unwrap_or(authority.actor_id.as_str())
        .to_string();
    let verified_domain = authority
        .audit_metadata
        .get("verified_domain")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    Some(HostedActorContext {
        actor_id: authority.actor_id.clone(),
        cloud_account_id,
        verified_domain,
        paired: true,
        authorized: true,
    })
}

/// Why the daemon's main loop exited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonExit {
    Shutdown,
    Reload,
}

/// Configuration for the daemon process.
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub paths: BicameralPaths,
    pub config: BicameralConfig,
}

/// Production secrets store: always the OS keyring (#1084).
///
/// Returns the concrete OS-backed type so the compiler, not runtime
/// configuration, guarantees production daemon construction can never
/// silently fall back to volatile in-memory storage. Deterministic tests
/// inject `InMemorySecretsStore` through `AppState::with_secrets_store`
/// instead of routing through this function.
pub fn production_secrets_store() -> Arc<bicameral_config::OsKeyringSecretsStore> {
    Arc::new(bicameral_config::OsKeyringSecretsStore::new())
}

/// Start one operator-scoped daemon without selecting a Product from process
/// cwd. Registered Product routes and workspace bindings are hydrated from the
/// operator home after startup.
pub async fn run_operator(gateway_config: GatewayConfig) -> anyhow::Result<DaemonExit> {
    let operator_home = BicameralPaths::operator_home()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine Bicameral operator home"))?;
    let paths = BicameralPaths::from_workspace_root(operator_home.join("runtime"));
    paths.ensure_dirs()?;

    let mut config = BicameralConfig::default();
    config.event_store.substrate = bicameral_api::event_store::SubstrateKind::Memory;
    config.gateway = gateway_config;
    // bot#1012: load or Cloud-bootstrap product.join trust so stock installs
    // can verify join tickets (product-scoped audience for multi-Product).
    if let Some(product_join) =
        crate::product_join_trust_store::resolve_operator_product_join_config().await
    {
        config.product_join = Some(product_join);
    }

    run_inner(DaemonConfig { paths, config }, None).await
}

/// Run the Bicameral daemon.
///
/// This starts the gateway in the same process (v0.1 simplification per plan
/// section 7.1) and waits for a shutdown signal. Returns the exit reason so
/// the caller can decide whether to restart.
pub async fn run(daemon_config: DaemonConfig) -> anyhow::Result<DaemonExit> {
    let workspace_root = daemon_config.paths.workspace_root.clone();
    run_inner(daemon_config, Some(workspace_root)).await
}

async fn run_inner(
    daemon_config: DaemonConfig,
    workspace_root: Option<PathBuf>,
) -> anyhow::Result<DaemonExit> {
    tracing::info!("Starting Bicameral daemon");

    // Construct the event-store adapter from configuration. Dual-open so the
    // same concrete instance backs both baseline EventStoreAdapter paths and
    // candidate_selection.commit atomic batches.
    let dual = crate::event_store_adapters::adapter_from_config_dual(
        &daemon_config.config.event_store,
        &daemon_config.paths,
    );
    let adapter = dual.event_store.clone();
    let atomic_adapter = dual.atomic;
    let current_event_store_route = crate::project_registry::EventStoreRoute::from_config(
        &daemon_config.config.event_store,
        &daemon_config.paths,
    );
    let project_router = BicameralPaths::project_registry_dir().map(|root| {
        let store: Arc<dyn crate::project_registry::ProjectRegistrationStore> = Arc::new(
            crate::project_registry::FileProjectRegistrationStore::new(root),
        );
        Arc::new(crate::project_registry::ProjectEventStoreRouter::new(store))
    });
    if let (Some(router), Some(route)) = (&project_router, &current_event_store_route) {
        match migrate_current_product_routes(adapter.as_ref(), route, router.store()) {
            Ok(count) if count > 0 => {
                tracing::info!(
                    count,
                    "Migrated current Product routes into operator registry"
                );
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "Current Product route migration failed; registered Products remain usable"
                );
            }
        }
    }
    tracing::info!(
        substrate = %adapter.substrate_kind(),
        "Event-store adapter constructed from config"
    );

    let audit = AuditStore::open(&daemon_config.paths.audit_dir)?;
    let projection_repository = Arc::new(
        bicameral_gateway::projection_work_repository::SqliteProjectionWorkRepository::open(
            daemon_config
                .paths
                .sources_dir
                .join("projection-working-state.sqlite3"),
        )?,
    );
    let projection_work_store = Arc::new(
        bicameral_gateway::live_projection_work::LiveProjectionWorkStore::open(
            projection_repository,
        )?,
    );
    let durable_pairing_store =
        match bicameral_gateway::durable_pairing_store::default_durable_pairings_path() {
            Some(path) => {
                Arc::new(bicameral_gateway::durable_pairing_store::DurablePairingStore::open(path)?)
            }
            None => {
                Arc::new(bicameral_gateway::durable_pairing_store::DurablePairingStore::in_memory())
            }
        };
    let daemon_instance_id =
        match bicameral_gateway::durable_pairing_store::default_daemon_instance_path() {
            Some(path) => {
                bicameral_gateway::durable_pairing_store::load_or_mint_daemon_instance_id(path)?
            }
            None => format!("daemon-{}", uuid::Uuid::new_v4()),
        };
    let mut state = AppState::with_adapter(adapter.clone(), audit)
        .with_secrets_store(production_secrets_store())
        .with_evidence_repository(Arc::new(
            bicameral_gateway::evidence_repository::FileEvidenceRepository::new(
                daemon_config.paths.sources_dir.join("evidence"),
            ),
        ))
        .with_projection_work_store(projection_work_store.clone())
        .with_durable_pairing_store(durable_pairing_store)
        .with_daemon_instance_id(daemon_instance_id);
    state.workspace_root = std::sync::Arc::new(tokio::sync::RwLock::new(workspace_root.clone()));

    // Inject runtime dispatch so gateway routes can delegate to the runtime.
    let commit_watcher_store = crate::commit_watcher::CommitWatcherStore::default();
    let product_commit_watcher_store = match BicameralPaths::product_watcher_state_dir() {
        Some(root) => crate::commit_watcher::ProductCommitWatcherStore::open(root)?,
        None => crate::commit_watcher::ProductCommitWatcherStore::default(),
    };
    let product_join_trust = daemon_config
        .config
        .product_join
        .as_ref()
        .and_then(crate::product_join::ProductJoinTrust::from_config)
        .map(Arc::new);
    if product_join_trust.is_some() {
        tracing::info!("product.join trust anchors configured");
    }
    let operational_logs = BicameralPaths::operational_logs_dir()
        .map(crate::operational_log::OperationalLogStore::new)
        .map(Arc::new);
    let capability_readiness = BicameralPaths::capability_readiness_dir()
        .map(crate::capability_readiness::FileCapabilityReadinessStore::open)
        .transpose()?
        .map(Arc::new);
    if let Some(logs) = &operational_logs {
        if let Err(error) = logs.enforce_retention() {
            tracing::warn!(
                error_kind = ?error.kind(),
                "Initial operational diagnostic retention enforcement failed"
            );
        }
    }
    // CandidateSet selection stack is process-local. Authority is reprojected
    // live on prepare/commit from Product ledger membership + product currentness.
    let candidate_selection_authority =
        Arc::new(crate::candidate_selection_authority::CandidateSelectionAuthorityRegistry::new());
    let product_currentness =
        Arc::new(crate::candidate_selection_authority::ProductCurrentnessStore::new());
    let candidate_selection = Arc::new(
        crate::candidate_selection_service::CandidateSelectionService::new(
            Arc::new(crate::candidate_session_store::CandidateSessionStore::new()),
            candidate_selection_authority.clone(),
            atomic_adapter.clone(),
        ),
    );
    // Stable daemon-owned proof-signing identity (bot#1087): generated once
    // into the OS keyring, never per process. The production composition can
    // only reach the keyring-backed signer.
    let proof_signer: Arc<dyn bicameral_gateway::proof_signing::ProofSigner> = Arc::new(
        bicameral_gateway::proof_signing::KeyringProofSigner::new(production_secrets_store()),
    );
    state.runtime_dispatch = Some(Arc::new(DaemonDispatch {
        adapter: adapter.clone(),
        operator_scoped: workspace_root.is_none(),
        project_router: project_router.clone(),
        current_event_store_route: current_event_store_route.clone(),
        commit_watcher: commit_watcher_store.clone(),
        product_commit_watchers: product_commit_watcher_store.clone(),
        capability_readiness,
        operational_logs: operational_logs.clone(),
        workspace_bindings_dir: BicameralPaths::workspace_bindings_dir(),
        project_registrations: state.project_registrations.clone(),
        product_join_trust,
        // Fresh per process start: a restart rotates the challenge epoch and
        // drops every pending grant, matching the transient authorization
        // semantics of the confirmation challenge (issue #829).
        confirmation_challenges: Arc::new(
            crate::confirmation_challenge_store::ConfirmationChallengeStore::new(),
        ),
        candidate_selection_authority,
        product_currentness,
        candidate_selection,
        atomic_adapter: atomic_adapter.clone(),
        projection_work_store: projection_work_store.clone(),
        candidates: state.candidates.clone(),
        reviews: state.reviews.clone(),
        proof_signer: Some(proof_signer),
    }));

    // Working candidates are hydrated from daemon-local SQLite, never
    // reconstructed from canonical Product replay.
    for candidate in projection_work_store.active_candidate_payloads(chrono::Utc::now())? {
        state
            .candidates
            .write()
            .await
            .insert(candidate.id, candidate.clone());
        state
            .reviews
            .write()
            .await
            .entry(candidate.id)
            .or_insert_with(|| bicameral_api::review::ReviewState::new(candidate.id));
    }

    // Replay event store to hydrate in-memory state from prior events.
    {
        let replayed = crate::replay::replay_from_adapter(state.event_store.as_ref());
        match replayed {
            Ok(rs) => {
                let count = rs.candidates.len() + rs.decisions.len();
                state
                    .hydrate(
                        rs.sources,
                        rs.snapshots,
                        rs.evidence,
                        rs.candidates,
                        rs.decisions,
                        rs.bindings,
                        rs.binding_content_hashes,
                    )
                    .await;
                if count > 0 {
                    tracing::info!(
                        candidates = count,
                        "Hydrated in-memory state from event-store replay"
                    );
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "Event-store replay failed; starting with empty state");
            }
        }
    }

    // Project registrations and workspace bindings are independent durable
    // operator-local records. Hydrate the live hosted projection from both.
    match project_router.as_ref().map(|router| {
        project_registrations_from_store(
            router,
            BicameralPaths::workspace_bindings_dir().as_deref(),
        )
    }) {
        Some(Ok(registrations)) => {
            let count = registrations.len();
            *state
                .project_registrations
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = registrations;
            if count > 0 {
                tracing::info!(count, "Hydrated durable project registrations");
            }
        }
        None => {}
        Some(Err(error)) => {
            tracing::warn!(
                error = %error,
                "Project registration hydration failed; starting with an empty hosted projection"
            );
        }
    }

    // Record daemon start
    {
        let mut audit = state.audit.write().await;
        let _ = audit.record(
            bicameral_audit::AuditAction::GatewayStarted,
            "daemon".to_string(),
            None,
            "Bicameral daemon started".to_string(),
        );
    }

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Poll local HEAD movement and update the daemon-owned freshness read-model.
    // This is intentionally memory-only and recomputable: it downgrades
    // currentness labels for preflight responses but never appends events or
    // mutates binding/decision/governance authority.
    let watcher_handle = if let Some(watcher_root) = workspace_root {
        let watcher_shutdown_rx = shutdown_rx.clone();
        let watcher_store = commit_watcher_store.clone();
        Some(tokio::spawn(async move {
            run_commit_watcher(watcher_root, watcher_store, watcher_shutdown_rx).await;
        }))
    } else if let (Some(router), Some(bindings_dir)) = (
        project_router.clone(),
        BicameralPaths::workspace_bindings_dir(),
    ) {
        let watcher_shutdown_rx = shutdown_rx.clone();
        let watcher_store = product_commit_watcher_store.clone();
        Some(tokio::spawn(async move {
            run_product_commit_watchers(router, bindings_dir, watcher_store, watcher_shutdown_rx)
                .await;
        }))
    } else {
        None
    };
    let retention_handle = operational_logs.map(|logs| {
        let retention_shutdown_rx = shutdown_tx.subscribe();
        tokio::spawn(async move {
            run_operational_log_retention(logs, retention_shutdown_rx).await;
        })
    });

    // Spawn gateway
    let gateway_config = daemon_config.config.gateway.clone();
    let gateway_state = state.clone();
    let mut gateway_handle =
        tokio::spawn(async move { serve(&gateway_config, gateway_state, shutdown_rx).await });

    // Wait for exit signal, or exit non-zero if the gateway stops serving so
    // the service manager restarts us instead of reporting a dead "running".
    let exit = wait_for_exit_or_gateway_failure(&mut gateway_handle).await;

    // Signal shutdown to gateway
    let _ = shutdown_tx.send(true);

    // Wait for gateway to finish (already joined when it failed)
    if exit.is_ok() {
        let _ = gateway_handle.await;
    }
    if let Some(handle) = watcher_handle {
        let _ = handle.await;
    }
    if let Some(handle) = retention_handle {
        let _ = handle.await;
    }
    let exit_reason = exit?;

    tracing::info!(reason = ?exit_reason, "Bicameral daemon exiting");
    Ok(exit_reason)
}

async fn run_operational_log_retention(
    logs: Arc<crate::operational_log::OperationalLogStore>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let first_check = tokio::time::Instant::now() + OPERATIONAL_LOG_RETENTION_INTERVAL;
    let mut interval = tokio::time::interval_at(first_check, OPERATIONAL_LOG_RETENTION_INTERVAL);

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    break;
                }
            }
            _ = interval.tick() => {
                if let Err(error) = logs.enforce_retention() {
                    tracing::warn!(
                        error_kind = ?error.kind(),
                        "Periodic operational diagnostic retention enforcement failed"
                    );
                }
            }
        }
    }
}

async fn run_commit_watcher(
    workspace_root: PathBuf,
    store: crate::commit_watcher::CommitWatcherStore,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut watcher = crate::commit_watcher::CommitWatcher::new(workspace_root);
    let _ = watcher.poll_once();
    let mut interval = tokio::time::interval(Duration::from_secs(1));

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    break;
                }
            }
            _ = interval.tick() => {
                if let Some(observation) = watcher.poll_once() {
                    tracing::info!(
                        old_head_sha = %observation.old_head_sha,
                        new_head_sha = %observation.new_head_sha,
                        touched_targets = observation.target_count(),
                        over_budget = observation.is_over_budget(),
                        "Commit watcher observed local HEAD movement"
                    );
                    store.replace(observation);
                }
            }
        }
    }
}

async fn run_product_commit_watchers(
    router: Arc<crate::project_registry::ProjectEventStoreRouter>,
    workspace_bindings_dir: PathBuf,
    store: crate::commit_watcher::ProductCommitWatcherStore,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    use crate::workspace_binding::WorkspaceBindingStore;
    use bicameral_api::workspace_binding::WorkspaceBindingState;

    let binding_store =
        crate::workspace_binding::FileWorkspaceBindingStore::new(workspace_bindings_dir);
    let mut watchers = crate::commit_watcher::ProductCommitWatchers::with_store(store);
    let mut interval = tokio::time::interval(Duration::from_secs(1));

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    break;
                }
            }
            _ = interval.tick() => {
                match router.store().list() {
                    Ok(registrations) => {
                            let products: Vec<crate::commit_watcher::ProductWatcherRegistration> =
                                registrations
                                .into_iter()
                                .map(|record| {
                                    let product_id = record.registration.product_id;
                                    match binding_store.load(&product_id) {
                                        Ok(Some(binding))
                                            if binding.state
                                                == WorkspaceBindingState::LocalWorkspaceBound =>
                                        {
                                            crate::commit_watcher::ProductWatcherRegistration::enabled(
                                                product_id,
                                                PathBuf::from(binding.candidate_path),
                                            )
                                        }
                                        Ok(_) => {
                                            crate::commit_watcher::ProductWatcherRegistration::disabled(
                                                product_id,
                                            )
                                        }
                                        Err(error) => {
                                            tracing::warn!(
                                                product_id = %product_id,
                                                error_kind = ?error.kind(),
                                                "Product commit watcher could not load workspace binding"
                                            );
                                            crate::commit_watcher::ProductWatcherRegistration::disabled(
                                                product_id,
                                            )
                                        }
                                    }
                                })
                                .collect();
                            if let Err(error) = watchers.reconcile_and_poll(products) {
                                tracing::warn!(
                                    error_kind = ?error.kind(),
                                    "Product commit watcher state update failed"
                                );
                            }
                    }
                    Err(error) => {
                        tracing::warn!(
                            error = %error,
                            "Product commit watchers retained prior state because registry replay failed"
                        );
                    }
                }
            }
        }
    }
}

/// Run the gateway in setup mode when no workspace exists.
///
/// Serves health, setup, and dashboard-config endpoints so the user can
/// create a workspace from the UI. No event-store replay or audit is
/// performed because there is no workspace yet.
pub async fn run_setup_mode(
    gateway_config: &GatewayConfig,
    workspace_root: &Path,
) -> anyhow::Result<DaemonExit> {
    tracing::info!("Starting Bicameral daemon in setup mode (no workspace)");

    let tmp = tempfile::tempdir()?;
    let audit = AuditStore::open(tmp.path())?;
    let durable_pairing_store =
        match bicameral_gateway::durable_pairing_store::default_durable_pairings_path() {
            Some(path) => {
                Arc::new(bicameral_gateway::durable_pairing_store::DurablePairingStore::open(path)?)
            }
            None => {
                Arc::new(bicameral_gateway::durable_pairing_store::DurablePairingStore::in_memory())
            }
        };
    let daemon_instance_id =
        match bicameral_gateway::durable_pairing_store::default_daemon_instance_path() {
            Some(path) => {
                bicameral_gateway::durable_pairing_store::load_or_mint_daemon_instance_id(path)?
            }
            None => format!("daemon-{}", uuid::Uuid::new_v4()),
        };
    let state = AppState::with_workspace_root(audit, workspace_root.to_path_buf())
        .with_secrets_store(production_secrets_store())
        .with_durable_pairing_store(durable_pairing_store)
        .with_daemon_instance_id(daemon_instance_id);

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let gw_config = gateway_config.clone();
    let gw_state = state.clone();
    let mut gateway_handle =
        tokio::spawn(async move { serve(&gw_config, gw_state, shutdown_rx).await });

    let exit = wait_for_exit_or_gateway_failure(&mut gateway_handle).await;

    let _ = shutdown_tx.send(true);
    if exit.is_ok() {
        let _ = gateway_handle.await;
    }
    let exit_reason = exit?;

    tracing::info!(reason = ?exit_reason, "Bicameral daemon (setup mode) exiting");
    Ok(exit_reason)
}

/// Wait for an exit signal, failing instead if the gateway task ends first
/// (e.g. the port could not be bound). A daemon that is alive but not serving
/// looks "running" to the service manager while the browser cannot connect.
async fn wait_for_exit_or_gateway_failure(
    gateway: &mut tokio::task::JoinHandle<anyhow::Result<()>>,
) -> anyhow::Result<DaemonExit> {
    tokio::select! {
        exit = wait_for_exit_signal() => exit,
        joined = gateway => {
            let error = match joined {
                Ok(Ok(())) => anyhow::anyhow!("Gateway stopped unexpectedly"),
                Ok(Err(e)) => e.context("Gateway failed"),
                Err(e) => anyhow::anyhow!("Gateway task failed: {e}"),
            };
            tracing::error!(error = %error, "Gateway is not serving; exiting so the service manager can restart");
            Err(error)
        }
    }
}

async fn wait_for_exit_signal() -> anyhow::Result<DaemonExit> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut sigint = signal(SignalKind::interrupt())?;
        let mut sigterm = signal(SignalKind::terminate())?;

        tokio::select! {
            _ = sigint.recv() => {
                tracing::info!("Received SIGINT, shutting down");
                Ok(DaemonExit::Shutdown)
            }
            _ = sigterm.recv() => {
                tracing::info!("Received SIGTERM, shutting down");
                Ok(DaemonExit::Shutdown)
            }
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await?;
        tracing::info!("Received Ctrl+C, shutting down");
        Ok(DaemonExit::Shutdown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_store_adapters::MemoryAdapter;

    #[tokio::test]
    async fn gateway_bind_failure_fails_the_daemon_instead_of_idling() {
        let mut gateway: tokio::task::JoinHandle<anyhow::Result<()>> =
            tokio::spawn(async { Err(anyhow::anyhow!("Address already in use")) });
        let exit = wait_for_exit_or_gateway_failure(&mut gateway).await;
        let error = exit.expect_err("a gateway failure must surface as a daemon error");
        assert!(format!("{error:#}").contains("Address already in use"));
    }
    use base64::Engine as _;

    /// #1084: production construction is the OS-backed store by type.
    /// The signature returns the concrete `OsKeyringSecretsStore`, so a
    /// volatile fallback cannot be introduced without changing this
    /// function — this test pins that contract and proves construction
    /// is infallible without keystore access.
    #[test]
    fn production_secrets_store_is_os_backed_and_constructs() {
        let store: Arc<bicameral_config::OsKeyringSecretsStore> = production_secrets_store();
        let _trait_object: Arc<dyn bicameral_config::SecretsStore> = store;
    }
    use crate::workspace_binding::{
        FileWorkspaceBindingStore, LocalWorkspaceBinding, WorkspaceBindingStore,
        DAEMON_WORKSPACE_BINDING_CAPABILITY,
    };
    use bicameral_api::event_store::{EventStoreAdapter, EventStoreEntry, EventType};

    fn test_selection_fields() -> (
        Arc<crate::candidate_selection_authority::CandidateSelectionAuthorityRegistry>,
        Arc<crate::candidate_selection_authority::ProductCurrentnessStore>,
        Arc<crate::candidate_selection_service::CandidateSelectionService>,
        Arc<dyn crate::atomic_event_store::AtomicEventStoreAdapter>,
    ) {
        let atomic: Arc<dyn crate::atomic_event_store::AtomicEventStoreAdapter> =
            Arc::new(MemoryAdapter::new());
        let authority = Arc::new(
            crate::candidate_selection_authority::CandidateSelectionAuthorityRegistry::new(),
        );
        let currentness =
            Arc::new(crate::candidate_selection_authority::ProductCurrentnessStore::new());
        let service = Arc::new(
            crate::candidate_selection_service::CandidateSelectionService::new(
                Arc::new(crate::candidate_session_store::CandidateSessionStore::new()),
                authority.clone(),
                atomic.clone(),
            ),
        );
        (authority, currentness, service, atomic)
    }

    use bicameral_api::product_creation::{ProductCreateParams, ProductPublishParams};
    use bicameral_api::product_join::{
        GrantedRole, JoinOutcome, ProductAutoJoinPolicy, ProductJoinCommandParams,
        ProductJoinResult, ProductJoinTicket, ProductMemberAddedPayload, SignedProductJoinTicket,
        PAIRED_CLOUD_ACCOUNT_METADATA_KEY,
    };
    use bicameral_api::product_registration::{
        ConnectionMetadata, ProductContinueSetupParams, ProjectRegistrationWrittenPayload,
        RegisteredProvider, RegisteredSubstrate, DESCRIPTOR_VALIDATION_METADATA_KEY,
        PROVIDER_ACCESS_METADATA_KEY,
    };
    use bicameral_api::workspace_binding::{
        ProjectSafeDisplayMetadata, WorkspaceBindRequest, WorkspaceBindingProposal,
        WorkspaceBindingSourceSurface, WorkspaceBindingState,
    };
    use chrono::Utc;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    fn hosted_route_authority() -> AuthorityContext {
        AuthorityContext {
            actor_id: "bridge:acct_route".into(),
            auth_method: AuthMethod::HostedBridge,
            session_id: Some("route-session".into()),
            workspace: None,
            policy_scope: vec!["hosted_bridge".into()],
            audit_metadata: serde_json::Map::new(),
        }
    }

    fn test_projection_work_store(
        label: &str,
    ) -> Arc<bicameral_gateway::live_projection_work::LiveProjectionWorkStore> {
        let path = std::env::temp_dir().join(format!(
            "bicameral-daemon-{label}-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let repository = Arc::new(
            bicameral_gateway::projection_work_repository::SqliteProjectionWorkRepository::open(
                path,
            )
            .unwrap(),
        );
        Arc::new(
            bicameral_gateway::live_projection_work::LiveProjectionWorkStore::open(repository)
                .unwrap(),
        )
    }

    fn seed_decision(adapter: &dyn EventStoreAdapter) -> String {
        let ingest =
            crate::dispatch::dispatch(&bicameral_api::fixtures::ingest_submit_local(), adapter);
        let candidate_id = ingest.result.unwrap()["candidate_ids"][0]
            .as_str()
            .unwrap()
            .to_string();
        let accept = ToolRequest::new(
            ToolCommand::ReviewAcceptCandidate(bicameral_api::tool_request::ReviewTargetParams {
                target_id: candidate_id,
                reason: Some("operator routing isolation fixture".into()),
            }),
            bicameral_api::fixtures::authority_cli("routing-test", "/unused"),
        );
        let response = crate::dispatch::dispatch(&accept, adapter);
        assert_eq!(
            response.status,
            bicameral_api::tool_request::ToolResponseStatus::Ok
        );
        adapter
            .replay()
            .unwrap()
            .into_iter()
            .find(|entry| entry.event_type == EventType::AcceptCandidate)
            .unwrap()
            .payload["decision_id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn project_route_record(
        product_id: &str,
        events_dir: PathBuf,
    ) -> crate::project_registry::ProjectRouteRecord {
        crate::project_registry::ProjectRouteRecord {
            registration: bicameral_api::product_registration::ProjectRegistration {
                product_id: product_id.into(),
                cloud_account_id: format!("acct_{product_id}"),
                registration_ref: format!("reg_{product_id}"),
                connection: ConnectionMetadata {
                    provider: RegisteredProvider::Github,
                    substrate: RegisteredSubstrate::Git,
                    account_label: None,
                    descriptor_ref: format!("descriptor_{product_id}"),
                },
                governance_policy_version: "gpv-1".into(),
                registered_at: Utc::now(),
            },
            event_store: crate::project_registry::EventStoreRoute::Git {
                events_dir,
                ci_merge_gate_configured: false,
            },
            first_event_commit_proof: None,
        }
    }

    fn hosted_join_fixture(
        product_id: &str,
    ) -> (
        ToolRequest,
        crate::product_join::ProductJoinTrust,
        ProductAutoJoinPolicy,
    ) {
        let key_id = "join-route-test-key";
        let issuer = "https://cloud.bicameral.test";
        let account = "acct_joiner";
        let domain = "acme.test";
        let key_pair = {
            let rng = ring::rand::SystemRandom::new();
            let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
            Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap()
        };
        let now = Utc::now();
        let ticket = ProductJoinTicket {
            issuer: issuer.into(),
            audience: format!("daemon:{product_id}"),
            product_id: product_id.into(),
            cloud_account_id: account.into(),
            verified_domain: domain.into(),
            issued_at: now - chrono::Duration::minutes(1),
            expiry: now + chrono::Duration::minutes(5),
            nonce: uuid::Uuid::new_v4().to_string(),
            registration_ref: "reg_joiner".into(),
            provider_hint: None,
            substrate_hint: None,
        };
        let signed_ticket = SignedProductJoinTicket {
            signature: base64::engine::general_purpose::STANDARD
                .encode(key_pair.sign(&ticket.signing_bytes()).as_ref()),
            ticket,
            key_id: key_id.into(),
        };
        let confirmed_actor = format!("bridge:{account}");
        let params = ProductJoinCommandParams {
            request_id: uuid::Uuid::new_v4().to_string(),
            signed_ticket,
            local_actor_ref: confirmed_actor.clone(),
            requested_at: now,
        };
        let mut authority = hosted_route_authority();
        authority.actor_id = confirmed_actor;
        authority.audit_metadata.insert(
            PAIRED_CLOUD_ACCOUNT_METADATA_KEY.into(),
            serde_json::Value::String(account.into()),
        );
        let request = ToolRequest::new(ToolCommand::ProductJoin(params), authority);
        let trust = crate::product_join::ProductJoinTrust {
            issuer: issuer.into(),
            fixed_audience: None,
            keys: vec![crate::product_join::VerifyingKey {
                key_id: key_id.into(),
                public_key: key_pair.public_key().as_ref().to_vec(),
            }],
        };
        let policy = ProductAutoJoinPolicy {
            product_id: product_id.into(),
            governance_policy_version: "gpv-1".into(),
            auto_join_enabled: true,
            allowed_domains: vec![domain.into()],
        };
        (request, trust, policy)
    }

    fn operator_join_dispatch(
        product_id: &str,
        events_dir: PathBuf,
        trust: crate::product_join::ProductJoinTrust,
    ) -> DaemonDispatch {
        let store: Arc<dyn crate::project_registry::ProjectRegistrationStore> =
            Arc::new(crate::project_registry::MemoryProjectRegistrationStore::new());
        store
            .write(&project_route_record(product_id, events_dir))
            .unwrap();
        let (selection_authority, currentness, selection, atomic) = test_selection_fields();
        DaemonDispatch {
            adapter: Arc::new(MemoryAdapter::new()),
            operator_scoped: true,
            project_router: Some(Arc::new(
                crate::project_registry::ProjectEventStoreRouter::new(store),
            )),
            current_event_store_route: None,
            commit_watcher: crate::commit_watcher::CommitWatcherStore::default(),
            product_commit_watchers: crate::commit_watcher::ProductCommitWatcherStore::default(),
            capability_readiness: None,
            operational_logs: None,
            workspace_bindings_dir: None,
            project_registrations: Arc::new(std::sync::RwLock::new(Vec::new())),
            product_join_trust: Some(Arc::new(trust)),
            confirmation_challenges: Arc::new(
                crate::confirmation_challenge_store::ConfirmationChallengeStore::new(),
            ),
            candidate_selection_authority: selection_authority,
            product_currentness: currentness,
            candidate_selection: selection,
            atomic_adapter: atomic,
            projection_work_store: test_projection_work_store("join-routing"),
            candidates: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            reviews: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            proof_signer: None,
        }
    }

    fn test_proof_signer() -> Arc<dyn bicameral_gateway::proof_signing::ProofSigner> {
        let mut seed = [0u8; 32];
        for (index, byte) in seed.iter_mut().enumerate() {
            *byte = (index + 1) as u8;
        }
        Arc::new(bicameral_gateway::proof_signing::FixedSeedProofSigner::from_seed(&seed).unwrap())
    }

    fn operator_create_dispatch() -> DaemonDispatch {
        let mut dispatch = operator_create_dispatch_without_signer();
        dispatch.proof_signer = Some(test_proof_signer());
        dispatch
    }

    fn operator_create_dispatch_without_signer() -> DaemonDispatch {
        let store: Arc<dyn crate::project_registry::ProjectRegistrationStore> =
            Arc::new(crate::project_registry::MemoryProjectRegistrationStore::new());
        let (selection_authority, currentness, selection, atomic) = test_selection_fields();
        DaemonDispatch {
            adapter: Arc::new(MemoryAdapter::new()),
            operator_scoped: true,
            project_router: Some(Arc::new(
                crate::project_registry::ProjectEventStoreRouter::new(store),
            )),
            current_event_store_route: None,
            commit_watcher: crate::commit_watcher::CommitWatcherStore::default(),
            product_commit_watchers: crate::commit_watcher::ProductCommitWatcherStore::default(),
            capability_readiness: None,
            operational_logs: None,
            workspace_bindings_dir: None,
            project_registrations: Arc::new(std::sync::RwLock::new(Vec::new())),
            product_join_trust: None,
            confirmation_challenges: Arc::new(
                crate::confirmation_challenge_store::ConfirmationChallengeStore::new(),
            ),
            candidate_selection_authority: selection_authority,
            product_currentness: currentness,
            candidate_selection: selection,
            atomic_adapter: atomic,
            projection_work_store: test_projection_work_store("operator-create"),
            candidates: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            reviews: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            proof_signer: None,
        }
    }

    fn operator_create_request(
        descriptor: Option<bicameral_api::product_creation::ProductEventStoreDescriptor>,
        validated_folder: Option<&str>,
    ) -> ToolRequest {
        let mut authority = hosted_route_authority();
        if let Some(folder_id) = validated_folder {
            authority.audit_metadata.insert(
                bicameral_api::product_registration::DESCRIPTOR_VALIDATION_METADATA_KEY.into(),
                serde_json::Value::String("valid".into()),
            );
            authority.audit_metadata.insert(
                bicameral_api::product_registration::VALIDATED_EVENT_STORE_FOLDER_ID_METADATA_KEY
                    .into(),
                serde_json::Value::String(folder_id.into()),
            );
        }
        ToolRequest::new(
            ToolCommand::ProductCreate(ProductCreateParams {
                idempotency_key: "create-operator".into(),
                product_display_name: "Operator Product".into(),
                event_store_descriptor: descriptor,
                asserted_cloud_account_id: Some("acct_route".into()),
                team_display_name: None,
                verified_domain: Some("bicameral-ai.com".into()),
                event_store_hint: None,
                required_connection: None,
            }),
            authority,
        )
    }

    fn google_descriptor(
        folder_id: &str,
    ) -> Option<bicameral_api::product_creation::ProductEventStoreDescriptor> {
        Some(
            bicameral_api::product_creation::ProductEventStoreDescriptor::Google {
                folder_id: folder_id.into(),
            },
        )
    }

    #[test]
    fn operator_daemon_rejects_create_without_a_descriptor() {
        let dispatch = operator_create_dispatch();
        let response = dispatch.dispatch(&operator_create_request(None, None), None);
        assert_eq!(
            response.status,
            bicameral_api::tool_request::ToolResponseStatus::Rejected
        );
        assert!(response
            .message
            .as_deref()
            .unwrap_or("")
            .starts_with("product_event_store_required"));
    }

    #[test]
    fn operator_daemon_rejects_create_with_an_unvalidated_descriptor() {
        let dispatch = operator_create_dispatch();
        // Browser-supplied descriptor with no daemon validation stamp.
        let response = dispatch.dispatch(
            &operator_create_request(google_descriptor("1FolderAbc"), None),
            None,
        );
        assert_eq!(
            response.status,
            bicameral_api::tool_request::ToolResponseStatus::Rejected
        );
        assert!(response
            .message
            .as_deref()
            .unwrap_or("")
            .starts_with("product_event_store_unvalidated"));

        // Stamp for a different folder than the descriptor carries.
        let response = dispatch.dispatch(
            &operator_create_request(google_descriptor("1FolderAbc"), Some("1OtherFolder")),
            None,
        );
        assert_eq!(
            response.status,
            bicameral_api::tool_request::ToolResponseStatus::Rejected
        );
        assert!(response
            .message
            .as_deref()
            .unwrap_or("")
            .starts_with("product_event_store_unvalidated"));
    }

    #[test]
    fn operator_daemon_routes_a_validated_descriptor_to_the_drive_folder_store() {
        let dispatch = operator_create_dispatch();
        let response = dispatch.dispatch(
            &operator_create_request(google_descriptor("1FolderAbc"), Some("1FolderAbc")),
            None,
        );
        // The guard is satisfied and the create is dispatched against the
        // Google Drive API route for that exact folder. In this unit test no
        // Google credential exists, so the durable store fails closed at the
        // first append: never a guard rejection, never a memory-store success.
        let message = response.message.clone().unwrap_or_default();
        assert!(
            !message.starts_with("product_event_store"),
            "guard must not fire for a daemon-validated descriptor: {message}"
        );
        assert_ne!(
            response.status,
            bicameral_api::tool_request::ToolResponseStatus::Ok,
            "a Product must never be created in a non-durable store: {response:?}"
        );
        assert!(
            message.contains("google_drive_api") || message.contains("Google"),
            "create must have been routed to the Drive folder store: {message}"
        );
    }

    #[test]
    fn operator_daemon_rejects_create_when_proof_signer_is_unavailable() {
        // bot#1087: the daemon signer is checked before the Drive route is
        // ever appended to. No registry record and no memory-store success.
        let dispatch = operator_create_dispatch_without_signer();
        let response = dispatch.dispatch(
            &operator_create_request(google_descriptor("1FolderAbc"), Some("1FolderAbc")),
            None,
        );
        assert_ne!(
            response.status,
            bicameral_api::tool_request::ToolResponseStatus::Ok
        );
        let message = response.message.clone().unwrap_or_default();
        assert!(
            message.starts_with("proof_signer_unavailable"),
            "signer absence must be the typed failure: {message}"
        );
        let store = dispatch.project_router.as_ref().unwrap().store();
        assert!(
            store.list().unwrap().is_empty(),
            "no route may be persisted"
        );
    }

    #[test]
    fn operator_daemon_routes_product_join_from_verified_ticket_identity() {
        let product_id = "prd_join_route";
        let events = tempfile::tempdir().unwrap();
        let adapter = crate::event_store_adapters::GitAdapter::new(events.path());
        let (request, trust, policy) = hosted_join_fixture(product_id);
        adapter
            .append(EventStoreEntry::new(
                EventType::ProductAutoJoinPolicySet,
                serde_json::to_value(policy).unwrap(),
                "daemon".into(),
            ))
            .unwrap();
        let dispatch = operator_join_dispatch(product_id, events.path().to_path_buf(), trust);

        assert!(
            request.product_id.is_none(),
            "hosted request supplies no route"
        );
        let response = dispatch.dispatch(&request, None);
        assert_eq!(
            response.status,
            bicameral_api::tool_request::ToolResponseStatus::Ok,
            "{response:?}"
        );
        let result: ProductJoinResult = serde_json::from_value(response.result.unwrap()).unwrap();
        assert_eq!(result.outcome, JoinOutcome::Granted);
        assert_eq!(
            adapter
                .replay()
                .unwrap()
                .iter()
                .filter(|entry| entry.event_type == EventType::ProductMemberAdded)
                .count(),
            1
        );
    }

    #[test]
    fn operator_daemon_rejects_tampered_join_ticket_before_routing() {
        let registered_product = "prd_join_route";
        let events = tempfile::tempdir().unwrap();
        let adapter = crate::event_store_adapters::GitAdapter::new(events.path());
        let (mut request, trust, _) = hosted_join_fixture(registered_product);
        let ToolCommand::ProductJoin(params) = &mut request.command else {
            unreachable!();
        };
        params.signed_ticket.ticket.product_id = "prd_attacker_selected".into();
        let dispatch =
            operator_join_dispatch(registered_product, events.path().to_path_buf(), trust);

        let response = dispatch.dispatch(&request, None);
        let result: ProductJoinResult =
            serde_json::from_value(response.result.expect("typed join result")).unwrap();
        assert_eq!(result.outcome, JoinOutcome::Rejected);
        assert_eq!(
            result.failure.map(|failure| failure.code),
            Some(bicameral_api::product_join::JoinRejectionCode::InvalidSignature)
        );
        assert!(adapter.replay().unwrap().is_empty());
    }

    #[test]
    fn operator_daemon_rejects_outer_and_verified_ticket_product_mismatch() {
        let ticket_product = "prd_join_route";
        let events = tempfile::tempdir().unwrap();
        let adapter = crate::event_store_adapters::GitAdapter::new(events.path());
        let (mut request, trust, _) = hosted_join_fixture(ticket_product);
        request.product_id = Some("prd_outer_conflict".into());
        let dispatch = operator_join_dispatch(ticket_product, events.path().to_path_buf(), trust);

        let response = dispatch.dispatch(&request, None);
        assert_eq!(
            response.status,
            bicameral_api::tool_request::ToolResponseStatus::Rejected
        );
        assert!(response
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("product_context_mismatch"));
        assert!(adapter.replay().unwrap().is_empty());
    }

    #[test]
    fn operator_daemon_rejects_verified_join_for_unregistered_product() {
        let events = tempfile::tempdir().unwrap();
        let adapter = crate::event_store_adapters::GitAdapter::new(events.path());
        let (request, trust, _) = hosted_join_fixture("prd_not_registered");
        let dispatch = operator_join_dispatch("prd_registered", events.path().to_path_buf(), trust);

        let response = dispatch.dispatch(&request, None);
        assert_eq!(
            response.status,
            bicameral_api::tool_request::ToolResponseStatus::Rejected
        );
        assert!(response
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("product_not_registered"));
        assert!(adapter.replay().unwrap().is_empty());
    }

    #[test]
    fn operator_daemon_routes_history_by_product_without_cross_product_bleed() {
        let events_a = tempfile::tempdir().unwrap();
        let events_b = tempfile::tempdir().unwrap();
        let adapter_a = crate::event_store_adapters::GitAdapter::new(events_a.path());
        let adapter_b = crate::event_store_adapters::GitAdapter::new(events_b.path());
        let decision_a = seed_decision(&adapter_a);
        let decision_b = seed_decision(&adapter_b);

        let store: Arc<dyn crate::project_registry::ProjectRegistrationStore> =
            Arc::new(crate::project_registry::MemoryProjectRegistrationStore::new());
        store
            .write(&project_route_record(
                "prd_a",
                events_a.path().to_path_buf(),
            ))
            .unwrap();
        store
            .write(&project_route_record(
                "prd_b",
                events_b.path().to_path_buf(),
            ))
            .unwrap();

        let dispatch = DaemonDispatch {
            adapter: Arc::new(MemoryAdapter::new()),
            operator_scoped: true,
            project_router: Some(Arc::new(
                crate::project_registry::ProjectEventStoreRouter::new(store),
            )),
            current_event_store_route: None,
            commit_watcher: crate::commit_watcher::CommitWatcherStore::default(),
            product_commit_watchers: crate::commit_watcher::ProductCommitWatcherStore::default(),
            capability_readiness: None,
            operational_logs: None,
            workspace_bindings_dir: None,
            project_registrations: Arc::new(std::sync::RwLock::new(Vec::new())),
            product_join_trust: None,
            confirmation_challenges: Arc::new(
                crate::confirmation_challenge_store::ConfirmationChallengeStore::new(),
            ),
            candidate_selection_authority: test_selection_fields().0,
            product_currentness: test_selection_fields().1,
            candidate_selection: test_selection_fields().2,
            atomic_adapter: test_selection_fields().3,
            projection_work_store: test_projection_work_store("routing-isolation"),
            candidates: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            reviews: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            proof_signer: None,
        };

        let response_a = dispatch.dispatch(
            &bicameral_api::fixtures::history_list().for_product("prd_a"),
            None,
        );
        let response_b = dispatch.dispatch(
            &bicameral_api::fixtures::history_list().for_product("prd_b"),
            None,
        );
        let missing_context = dispatch.dispatch(&bicameral_api::fixtures::history_list(), None);
        let history_a = response_a.result.unwrap().to_string();
        let history_b = response_b.result.unwrap().to_string();

        assert!(history_a.contains(&decision_a));
        assert!(!history_a.contains(&decision_b));
        assert!(history_b.contains(&decision_b));
        assert!(!history_b.contains(&decision_a));
        assert_eq!(
            missing_context.status,
            bicameral_api::tool_request::ToolResponseStatus::Rejected
        );
        assert!(missing_context
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("product_context_required"));
    }

    #[test]
    fn operator_daemon_projects_only_the_selected_products_watcher_observation() {
        let events_a = tempfile::tempdir().unwrap();
        let events_b = tempfile::tempdir().unwrap();
        let workspaces = tempfile::tempdir().unwrap();
        let workspace_a = workspaces.path().join("product-a");
        let workspace_b = workspaces.path().join("product-b");
        init_git_workspace(&workspace_a, "src/routes/healthz.rs");
        init_git_workspace(&workspace_b, "src/routes/healthz.rs");

        let store: Arc<dyn crate::project_registry::ProjectRegistrationStore> =
            Arc::new(crate::project_registry::MemoryProjectRegistrationStore::new());
        store
            .write(&project_route_record(
                "prd_a",
                events_a.path().to_path_buf(),
            ))
            .unwrap();
        store
            .write(&project_route_record(
                "prd_b",
                events_b.path().to_path_buf(),
            ))
            .unwrap();

        let binding_dir = workspaces.path().join("bindings");
        let bindings = FileWorkspaceBindingStore::new(&binding_dir);
        for (product_id, workspace) in [("prd_a", &workspace_a), ("prd_b", &workspace_b)] {
            bindings
                .persist(&LocalWorkspaceBinding {
                    project_id: product_id.into(),
                    candidate_path: workspace.display().to_string(),
                    state: WorkspaceBindingState::LocalWorkspaceBound,
                    source_surface: WorkspaceBindingSourceSurface::Cli,
                    display: ProjectSafeDisplayMetadata {
                        display_name: product_id.into(),
                        project_slug: None,
                        candidate_label: None,
                    },
                    daemon_capability: DAEMON_WORKSPACE_BINDING_CAPABILITY,
                    bound_at: Utc::now().to_rfc3339(),
                })
                .unwrap();
        }

        let product_watchers = crate::commit_watcher::ProductCommitWatcherStore::default();
        product_watchers.replace_for(
            "prd_a",
            crate::commit_watcher::CommitWatcherObservation::new(
                "old-a".into(),
                "new-a".into(),
                vec!["src/routes/healthz.rs".into()],
            ),
        );
        let dispatch = DaemonDispatch {
            adapter: Arc::new(MemoryAdapter::new()),
            operator_scoped: true,
            project_router: Some(Arc::new(
                crate::project_registry::ProjectEventStoreRouter::new(store),
            )),
            current_event_store_route: None,
            commit_watcher: crate::commit_watcher::CommitWatcherStore::default(),
            product_commit_watchers: product_watchers,
            capability_readiness: None,
            operational_logs: None,
            workspace_bindings_dir: Some(binding_dir),
            project_registrations: Arc::new(std::sync::RwLock::new(Vec::new())),
            product_join_trust: None,
            confirmation_challenges: Arc::new(
                crate::confirmation_challenge_store::ConfirmationChallengeStore::new(),
            ),
            candidate_selection_authority: test_selection_fields().0,
            product_currentness: test_selection_fields().1,
            candidate_selection: test_selection_fields().2,
            atomic_adapter: test_selection_fields().3,
            projection_work_store: test_projection_work_store("product-watcher-isolation"),
            candidates: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            reviews: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            proof_signer: None,
        };

        let mut request_a = bicameral_api::fixtures::preflight_run().for_product("prd_a");
        let mut request_b = bicameral_api::fixtures::preflight_run().for_product("prd_b");
        for request in [&mut request_a, &mut request_b] {
            let ToolCommand::PreflightRun(params) = &mut request.command else {
                unreachable!();
            };
            params.branch = None;
            params.symbols.clear();
        }

        let response_a = dispatch.dispatch(&request_a, None);
        let response_b = dispatch.dispatch(&request_b, None);
        assert_eq!(
            response_a.status,
            bicameral_api::tool_request::ToolResponseStatus::Ok,
            "{response_a:?}"
        );
        assert_eq!(
            response_b.status,
            bicameral_api::tool_request::ToolResponseStatus::Ok,
            "{response_b:?}"
        );
        assert!(
            response_a
                .result
                .unwrap()
                .to_string()
                .contains("touched_stale"),
            "Product A must consume its own stale observation"
        );
        assert!(
            !response_b
                .result
                .unwrap()
                .to_string()
                .contains("touched_stale"),
            "Product B must not consume Product A's stale observation"
        );
    }

    fn init_git_workspace(path: &Path, relative_file: &str) {
        std::fs::create_dir_all(path.join("src/routes")).unwrap();
        let run = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .args(args)
                .current_dir(path)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
        };
        run(&["init"]);
        run(&["config", "user.email", "test@bicameral.ai"]);
        run(&["config", "user.name", "Bicameral Test"]);
        std::fs::write(path.join(relative_file), "fn healthz() {}\n").unwrap();
        run(&["add", relative_file]);
        run(&["commit", "-m", "initial"]);
    }

    fn migrate_test_router(
        adapter: &dyn EventStoreAdapter,
        event_dir: &Path,
    ) -> Arc<crate::project_registry::ProjectEventStoreRouter> {
        let store: Arc<dyn crate::project_registry::ProjectRegistrationStore> =
            Arc::new(crate::project_registry::MemoryProjectRegistrationStore::new());
        let route = crate::project_registry::EventStoreRoute::Git {
            events_dir: event_dir.to_path_buf(),
            ci_merge_gate_configured: false,
        };
        migrate_current_product_routes(adapter, &route, store.as_ref()).unwrap();
        Arc::new(crate::project_registry::ProjectEventStoreRouter::new(store))
    }

    #[test]
    fn creation_persists_route_used_by_restarted_daemon() {
        let product_workspace = tempfile::tempdir().unwrap();
        let operator_home = tempfile::tempdir().unwrap();
        let product_events = product_workspace.path().join(".bicameral/events");
        let route = crate::project_registry::EventStoreRoute::Git {
            events_dir: product_events.clone(),
            ci_merge_gate_configured: false,
        };
        let store: Arc<dyn crate::project_registry::ProjectRegistrationStore> =
            Arc::new(crate::project_registry::FileProjectRegistrationStore::new(
                operator_home.path().join("projects"),
            ));

        let create_dispatch = DaemonDispatch {
            adapter: Arc::new(crate::event_store_adapters::GitAdapter::new(
                &product_events,
            )),
            operator_scoped: false,
            project_router: Some(Arc::new(
                crate::project_registry::ProjectEventStoreRouter::new(store.clone()),
            )),
            current_event_store_route: Some(route),
            commit_watcher: crate::commit_watcher::CommitWatcherStore::default(),
            product_commit_watchers: crate::commit_watcher::ProductCommitWatcherStore::default(),
            capability_readiness: None,
            operational_logs: None,
            workspace_bindings_dir: None,
            project_registrations: Arc::new(std::sync::RwLock::new(Vec::new())),
            product_join_trust: None,
            confirmation_challenges: Arc::new(
                crate::confirmation_challenge_store::ConfirmationChallengeStore::new(),
            ),
            candidate_selection_authority: test_selection_fields().0,
            product_currentness: test_selection_fields().1,
            candidate_selection: test_selection_fields().2,
            atomic_adapter: test_selection_fields().3,
            projection_work_store: test_projection_work_store("fixture"),
            candidates: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            reviews: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            proof_signer: None,
        };
        let create = ToolRequest::new(
            ToolCommand::ProductCreate(ProductCreateParams {
                idempotency_key: "create-persisted-route".into(),
                product_display_name: "Persisted Route".into(),
                event_store_descriptor: None,
                asserted_cloud_account_id: Some("acct_route".into()),
                team_display_name: None,
                verified_domain: None,
                event_store_hint: None,
                required_connection: None,
            }),
            hosted_route_authority(),
        );
        let created = create_dispatch.dispatch(&create, Some(product_workspace.path()));
        assert_eq!(
            created.status,
            bicameral_api::tool_request::ToolResponseStatus::Ok,
            "{created:?}"
        );
        let product_id = created.result.unwrap()["result"]["product_id"]
            .as_str()
            .unwrap()
            .to_string();

        // New dispatch + unrelated default adapter models process restart from
        // another cwd. Only the persisted operator route can find the Product.
        let restarted = DaemonDispatch {
            adapter: Arc::new(MemoryAdapter::new()),
            operator_scoped: true,
            project_router: Some(Arc::new(
                crate::project_registry::ProjectEventStoreRouter::new(store),
            )),
            current_event_store_route: None,
            commit_watcher: crate::commit_watcher::CommitWatcherStore::default(),
            product_commit_watchers: crate::commit_watcher::ProductCommitWatcherStore::default(),
            capability_readiness: None,
            operational_logs: None,
            workspace_bindings_dir: None,
            project_registrations: Arc::new(std::sync::RwLock::new(Vec::new())),
            product_join_trust: None,
            confirmation_challenges: Arc::new(
                crate::confirmation_challenge_store::ConfirmationChallengeStore::new(),
            ),
            candidate_selection_authority: test_selection_fields().0,
            product_currentness: test_selection_fields().1,
            candidate_selection: test_selection_fields().2,
            atomic_adapter: test_selection_fields().3,
            projection_work_store: test_projection_work_store("fixture"),
            candidates: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            reviews: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            proof_signer: None,
        };
        let publish = ToolRequest::new(
            ToolCommand::ProductPublish(ProductPublishParams {
                product_id,
                idempotency_key: "publish-persisted-route".into(),
            }),
            hosted_route_authority(),
        );
        let published = restarted.dispatch(&publish, None);
        assert_eq!(
            published.status,
            bicameral_api::tool_request::ToolResponseStatus::Ok,
            "{published:?}"
        );
    }

    #[test]
    fn publish_returns_the_recorded_commit_proof_after_restart() {
        // bot#1087: the proof recorded with the operator route at create time
        // is what `product.publish` carries, including from a fresh process
        // that only has the persisted registry.
        let product_workspace = tempfile::tempdir().unwrap();
        let operator_home = tempfile::tempdir().unwrap();
        let product_events = product_workspace.path().join(".bicameral/events");
        let product_adapter = crate::event_store_adapters::GitAdapter::new(&product_events);
        let actor = HostedActorContext {
            actor_id: "bridge:acct_route".into(),
            cloud_account_id: "acct_route".into(),
            verified_domain: None,
            paired: true,
            authorized: true,
        };
        let params = ProductCreateParams {
            idempotency_key: "create-proof-route".into(),
            product_display_name: "Proof Product".into(),
            event_store_descriptor: None,
            asserted_cloud_account_id: Some("acct_route".into()),
            team_display_name: None,
            verified_domain: None,
            event_store_hint: None,
            required_connection: None,
        };
        let created = crate::product_creation::create_product(
            &params,
            Some(&actor),
            &product_adapter,
            Utc::now(),
        )
        .unwrap();
        let product_id = created.result.product_id.clone();

        let golden: serde_json::Value = serde_json::from_str(include_str!(
            "../../../protocol/onboarding/vectors/product-creation-commit-proof-v2.golden.json"
        ))
        .unwrap();
        let mut proof: bicameral_api::product_creation::ProductCreationCommitProof =
            serde_json::from_value(golden["proof"].clone()).unwrap();
        proof.product_id = product_id.clone();

        let registry_dir = operator_home.path().join("projects");
        let store: Arc<dyn crate::project_registry::ProjectRegistrationStore> = Arc::new(
            crate::project_registry::FileProjectRegistrationStore::new(registry_dir.clone()),
        );
        let mut record = crate::project_registry::ProjectRouteRecord::from_creation(
            &created.result,
            &params,
            crate::project_registry::EventStoreRoute::Git {
                events_dir: product_events.clone(),
                ci_merge_gate_configured: false,
            },
        );
        record.first_event_commit_proof = Some(proof.clone());
        store.write(&record).unwrap();

        // Fresh process: a new store handle over the persisted registry dir.
        let restarted_store: Arc<dyn crate::project_registry::ProjectRegistrationStore> = Arc::new(
            crate::project_registry::FileProjectRegistrationStore::new(registry_dir),
        );
        assert_eq!(
            restarted_store
                .find_product(&product_id)
                .unwrap()
                .unwrap()
                .first_event_commit_proof,
            Some(proof.clone()),
            "proof survives the registry round trip"
        );
        let restarted = DaemonDispatch {
            adapter: Arc::new(MemoryAdapter::new()),
            operator_scoped: true,
            project_router: Some(Arc::new(
                crate::project_registry::ProjectEventStoreRouter::new(restarted_store),
            )),
            current_event_store_route: None,
            commit_watcher: crate::commit_watcher::CommitWatcherStore::default(),
            product_commit_watchers: crate::commit_watcher::ProductCommitWatcherStore::default(),
            capability_readiness: None,
            operational_logs: None,
            workspace_bindings_dir: None,
            project_registrations: Arc::new(std::sync::RwLock::new(Vec::new())),
            product_join_trust: None,
            confirmation_challenges: Arc::new(
                crate::confirmation_challenge_store::ConfirmationChallengeStore::new(),
            ),
            candidate_selection_authority: test_selection_fields().0,
            product_currentness: test_selection_fields().1,
            candidate_selection: test_selection_fields().2,
            atomic_adapter: test_selection_fields().3,
            projection_work_store: test_projection_work_store("fixture"),
            candidates: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            reviews: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            proof_signer: None,
        };
        let publish = ToolRequest::new(
            ToolCommand::ProductPublish(ProductPublishParams {
                product_id: product_id.clone(),
                idempotency_key: "publish-proof-route".into(),
            }),
            hosted_route_authority(),
        );
        let published = restarted.dispatch(&publish, None);
        assert_eq!(
            published.status,
            bicameral_api::tool_request::ToolResponseStatus::Ok,
            "{published:?}"
        );
        let publication = published.result.unwrap()["publication"].clone();
        assert_eq!(publication["product_id"], product_id);
        assert_eq!(
            publication["first_event_commit_proof"],
            serde_json::to_value(&proof).unwrap(),
            "publish carries the exact recorded proof"
        );
        let typed: bicameral_api::product_creation::ProductCreationPublication =
            serde_json::from_value(publication).unwrap();
        assert_eq!(typed.first_event_commit_proof, Some(proof));
    }

    #[test]
    fn product_publication_routes_by_registered_product_not_daemon_cwd() {
        let product_workspace = tempfile::tempdir().unwrap();
        let operator_home = tempfile::tempdir().unwrap();
        let product_events = product_workspace.path().join(".bicameral/events");
        let product_adapter = crate::event_store_adapters::GitAdapter::new(&product_events);
        let actor = HostedActorContext {
            actor_id: "bridge:acct_route".into(),
            cloud_account_id: "acct_route".into(),
            verified_domain: None,
            paired: true,
            authorized: true,
        };
        let created = crate::product_creation::create_product(
            &ProductCreateParams {
                idempotency_key: "create-route".into(),
                product_display_name: "Routed Product".into(),
                event_store_descriptor: None,
                asserted_cloud_account_id: Some("acct_route".into()),
                team_display_name: None,
                verified_domain: None,
                event_store_hint: None,
                required_connection: None,
            },
            Some(&actor),
            &product_adapter,
            Utc::now(),
        )
        .unwrap();

        let store: Arc<dyn crate::project_registry::ProjectRegistrationStore> =
            Arc::new(crate::project_registry::FileProjectRegistrationStore::new(
                operator_home.path().join("projects"),
            ));
        store
            .write(&crate::project_registry::ProjectRouteRecord {
                registration: bicameral_api::product_registration::ProjectRegistration {
                    product_id: created.result.product_id.clone(),
                    cloud_account_id: created.result.creator_cloud_account_id.clone(),
                    registration_ref: format!("creation:{}", created.result.receipt_id),
                    connection: ConnectionMetadata {
                        provider: RegisteredProvider::None,
                        substrate: RegisteredSubstrate::Git,
                        account_label: None,
                        descriptor_ref: format!("local:{}", created.result.product_id),
                    },
                    governance_policy_version: created.result.governance_policy_version.clone(),
                    registered_at: Utc::now(),
                },
                event_store: crate::project_registry::EventStoreRoute::Git {
                    events_dir: product_events,
                    ci_merge_gate_configured: false,
                },
                first_event_commit_proof: None,
            })
            .unwrap();

        let dispatch = DaemonDispatch {
            // Simulates the daemon starting from HOME/a different workspace.
            adapter: Arc::new(MemoryAdapter::new()),
            operator_scoped: true,
            project_router: Some(Arc::new(
                crate::project_registry::ProjectEventStoreRouter::new(store),
            )),
            current_event_store_route: None,
            commit_watcher: crate::commit_watcher::CommitWatcherStore::default(),
            product_commit_watchers: crate::commit_watcher::ProductCommitWatcherStore::default(),
            capability_readiness: None,
            operational_logs: None,
            workspace_bindings_dir: None,
            project_registrations: Arc::new(std::sync::RwLock::new(Vec::new())),
            product_join_trust: None,
            confirmation_challenges: Arc::new(
                crate::confirmation_challenge_store::ConfirmationChallengeStore::new(),
            ),
            candidate_selection_authority: test_selection_fields().0,
            product_currentness: test_selection_fields().1,
            candidate_selection: test_selection_fields().2,
            atomic_adapter: test_selection_fields().3,
            projection_work_store: test_projection_work_store("fixture"),
            candidates: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            reviews: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            proof_signer: None,
        };
        let request = ToolRequest::new(
            ToolCommand::ProductPublish(ProductPublishParams {
                product_id: created.result.product_id,
                idempotency_key: "publish-route".into(),
            }),
            AuthorityContext {
                actor_id: "bridge:acct_route".into(),
                auth_method: AuthMethod::HostedBridge,
                session_id: Some("route-session".into()),
                workspace: None,
                policy_scope: vec!["hosted_bridge".into()],
                audit_metadata: serde_json::Map::new(),
            },
        );

        let response = dispatch.dispatch(&request, None);
        assert_eq!(
            response.status,
            bicameral_api::tool_request::ToolResponseStatus::Ok,
            "{response:?}"
        );
    }

    #[test]
    fn product_publication_failure_writes_redacted_operational_receipt() {
        let adapter = Arc::new(MemoryAdapter::new());
        let log_dir = tempfile::tempdir().unwrap();
        let dispatch = DaemonDispatch {
            adapter,
            operator_scoped: true,
            project_router: None,
            current_event_store_route: None,
            commit_watcher: crate::commit_watcher::CommitWatcherStore::default(),
            product_commit_watchers: crate::commit_watcher::ProductCommitWatcherStore::default(),
            capability_readiness: None,
            operational_logs: Some(Arc::new(crate::operational_log::OperationalLogStore::new(
                log_dir.path(),
            ))),
            workspace_bindings_dir: None,
            project_registrations: Arc::new(std::sync::RwLock::new(Vec::new())),
            product_join_trust: None,
            confirmation_challenges: Arc::new(
                crate::confirmation_challenge_store::ConfirmationChallengeStore::new(),
            ),
            candidate_selection_authority: test_selection_fields().0,
            product_currentness: test_selection_fields().1,
            candidate_selection: test_selection_fields().2,
            atomic_adapter: test_selection_fields().3,
            projection_work_store: test_projection_work_store("fixture"),
            candidates: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            reviews: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            proof_signer: None,
        };
        let mut audit_metadata = serde_json::Map::new();
        audit_metadata.insert("surface".into(), serde_json::json!("hosted_bridge"));
        let request = ToolRequest::new(
            ToolCommand::ProductPublish(ProductPublishParams {
                product_id: "prd_sensitive".to_string(),
                idempotency_key: "publish_sensitive".to_string(),
            }),
            AuthorityContext {
                actor_id: "bridge:acct_sensitive".to_string(),
                auth_method: AuthMethod::HostedBridge,
                session_id: Some("session_sensitive".to_string()),
                workspace: Some("/private/workspace".to_string()),
                policy_scope: vec!["hosted_bridge".to_string()],
                audit_metadata,
            },
        );

        let response = dispatch.dispatch(&request, None);
        assert_eq!(
            response.status,
            bicameral_api::tool_request::ToolResponseStatus::Rejected
        );

        let files: Vec<_> = std::fs::read_dir(log_dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(files.len(), 1);
        let receipt = std::fs::read_to_string(&files[0]).unwrap();
        assert!(receipt.contains(&request.request_id.to_string()));
        assert!(receipt.contains("\"error_code\": \"product_registry_unavailable\""));
        assert!(receipt.contains("\"canonical_product_count\": null"));
        for forbidden in [
            "prd_sensitive",
            "publish_sensitive",
            "acct_sensitive",
            "session_sensitive",
            "/private/workspace",
        ] {
            assert!(!receipt.contains(forbidden), "receipt leaked {forbidden}");
        }
    }

    #[test]
    fn product_scoped_search_writes_operational_fingerprint_receipt() {
        // Topology multi-product-routing counts redacted product_id_fingerprint
        // receipts for bind/ingest/search after product.select. Those commands
        // must leave the same operational evidence as product.create/publish.
        let events = tempfile::tempdir().unwrap();
        let store: Arc<dyn crate::project_registry::ProjectRegistrationStore> =
            Arc::new(crate::project_registry::MemoryProjectRegistrationStore::new());
        store
            .write(&project_route_record(
                "prd_topology_alpha",
                events.path().to_path_buf(),
            ))
            .unwrap();
        let router = Arc::new(crate::project_registry::ProjectEventStoreRouter::new(store));
        let log_dir = tempfile::tempdir().unwrap();
        let dispatch = DaemonDispatch {
            adapter: Arc::new(MemoryAdapter::new()),
            operator_scoped: true,
            project_router: Some(router),
            current_event_store_route: None,
            commit_watcher: crate::commit_watcher::CommitWatcherStore::default(),
            product_commit_watchers: crate::commit_watcher::ProductCommitWatcherStore::default(),
            capability_readiness: None,
            operational_logs: Some(Arc::new(crate::operational_log::OperationalLogStore::new(
                log_dir.path(),
            ))),
            workspace_bindings_dir: None,
            project_registrations: Arc::new(std::sync::RwLock::new(Vec::new())),
            product_join_trust: None,
            confirmation_challenges: Arc::new(
                crate::confirmation_challenge_store::ConfirmationChallengeStore::new(),
            ),
            candidate_selection_authority: test_selection_fields().0,
            product_currentness: test_selection_fields().1,
            candidate_selection: test_selection_fields().2,
            atomic_adapter: test_selection_fields().3,
            projection_work_store: test_projection_work_store("product-scoped-ops-log"),
            candidates: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            reviews: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            proof_signer: None,
        };

        let request = bicameral_api::fixtures::search_query().for_product("prd_topology_alpha");
        let response = dispatch.dispatch(&request, None);
        assert_eq!(
            response.status,
            bicameral_api::tool_request::ToolResponseStatus::Ok
        );

        let files: Vec<_> = std::fs::read_dir(log_dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(
            files.len(),
            1,
            "product-scoped search must write one operational receipt"
        );
        let receipt = std::fs::read_to_string(&files[0]).unwrap();
        let expected =
            crate::operational_log::diagnostic_fingerprint("product_id", "prd_topology_alpha");
        assert!(
            receipt.contains(&format!("\"product_id_fingerprint\": \"{expected}\"")),
            "receipt missing product fingerprint: {receipt}"
        );
        assert!(
            !receipt.contains("prd_topology_alpha"),
            "operational receipt leaked raw product id"
        );
    }

    #[test]
    fn durable_registration_and_binding_hydrate_hosted_readiness() {
        let adapter = MemoryAdapter::new();
        let payload = ProjectRegistrationWrittenPayload {
            product_id: "prd_hydrated".to_string(),
            cloud_account_id: "acct_alice".to_string(),
            registration_ref: "reg_opaque".to_string(),
            connection: ConnectionMetadata {
                provider: RegisteredProvider::Github,
                substrate: RegisteredSubstrate::Git,
                account_label: Some("acme".to_string()),
                descriptor_ref: "descriptor:reg_opaque".to_string(),
            },
            governance_policy_version: "gpv-1".to_string(),
            written_at: Utc::now(),
        };
        adapter
            .append(EventStoreEntry::new(
                EventType::ProjectRegistrationWritten,
                serde_json::to_value(payload).unwrap(),
                "daemon".to_string(),
            ))
            .unwrap();

        let binding_dir = tempfile::tempdir().unwrap();
        let binding_store = FileWorkspaceBindingStore::new(binding_dir.path());
        binding_store
            .persist(&LocalWorkspaceBinding {
                project_id: "prd_hydrated".to_string(),
                candidate_path: "/private/work/acme".to_string(),
                state: WorkspaceBindingState::LocalWorkspaceBound,
                source_surface: WorkspaceBindingSourceSurface::Mcp,
                display: ProjectSafeDisplayMetadata {
                    display_name: "Acme".to_string(),
                    project_slug: Some("acme".to_string()),
                    candidate_label: Some("acme".to_string()),
                },
                daemon_capability: DAEMON_WORKSPACE_BINDING_CAPABILITY,
                bound_at: Utc::now().to_rfc3339(),
            })
            .unwrap();

        let event_dir = tempfile::tempdir().unwrap();
        let router = migrate_test_router(&adapter, event_dir.path());
        let registrations = project_registrations_from_store(&router, Some(binding_dir.path()))
            .expect("durable registrations hydrate");

        assert_eq!(registrations.len(), 1);
        assert_eq!(registrations[0].name, "prd_hydrated");
        assert!(registrations[0].event_store_healthy);
        assert!(registrations[0].has_workspace_binding);
        assert_eq!(registrations[0].setup_state, "normal");

        let hosted = bicameral_gateway::state::project_readiness_projection(&registrations);
        assert!(hosted[0].workspace_binding_readiness().code_grounding_ready);
        let serialized = serde_json::to_string(&hosted).unwrap();
        assert!(!serialized.contains("/private/work/acme"));
    }

    #[test]
    fn unreadable_binding_keeps_registration_visible_as_repair_required() {
        let adapter = MemoryAdapter::new();
        let payload = ProjectRegistrationWrittenPayload {
            product_id: "prd_repair".to_string(),
            cloud_account_id: "acct_alice".to_string(),
            registration_ref: "reg_repair".to_string(),
            connection: ConnectionMetadata {
                provider: RegisteredProvider::Github,
                substrate: RegisteredSubstrate::Git,
                account_label: None,
                descriptor_ref: "descriptor:reg_repair".to_string(),
            },
            governance_policy_version: "gpv-1".to_string(),
            written_at: Utc::now(),
        };
        adapter
            .append(EventStoreEntry::new(
                EventType::ProjectRegistrationWritten,
                serde_json::to_value(payload).unwrap(),
                "daemon".to_string(),
            ))
            .unwrap();

        let binding_dir = tempfile::tempdir().unwrap();
        std::fs::write(binding_dir.path().join("prd_repair.json"), b"not-json").unwrap();

        let event_dir = tempfile::tempdir().unwrap();
        let router = migrate_test_router(&adapter, event_dir.path());
        let registrations = project_registrations_from_store(&router, Some(binding_dir.path()))
            .expect("a damaged binding must not hide the durable registration");
        assert_eq!(registrations.len(), 1);
        assert_eq!(registrations[0].name, "prd_repair");
        assert_eq!(
            registrations[0].setup_state,
            "local_workspace_repair_required"
        );
        assert!(!registrations[0].has_workspace_binding);
    }

    #[test]
    fn successful_workspace_bind_updates_live_hosted_readiness() {
        let adapter = Arc::new(MemoryAdapter::new());
        let payload = ProjectRegistrationWrittenPayload {
            product_id: "prd_live".to_string(),
            cloud_account_id: "acct_alice".to_string(),
            registration_ref: "reg_live".to_string(),
            connection: ConnectionMetadata {
                provider: RegisteredProvider::Github,
                substrate: RegisteredSubstrate::Git,
                account_label: None,
                descriptor_ref: "descriptor:reg_live".to_string(),
            },
            governance_policy_version: "gpv-1".to_string(),
            written_at: Utc::now(),
        };
        adapter
            .append(EventStoreEntry::new(
                EventType::ProjectRegistrationWritten,
                serde_json::to_value(payload).unwrap(),
                "daemon".to_string(),
            ))
            .unwrap();

        let candidate = tempfile::tempdir().unwrap();
        let binding_dir = tempfile::tempdir().unwrap();
        let event_dir = tempfile::tempdir().unwrap();
        let router = migrate_test_router(adapter.as_ref(), event_dir.path());
        let registrations = Arc::new(std::sync::RwLock::new(
            project_registrations_from_store(&router, Some(binding_dir.path())).unwrap(),
        ));
        assert!(!registrations.read().unwrap()[0].has_workspace_binding);

        let dispatch = DaemonDispatch {
            adapter,
            operator_scoped: false,
            project_router: Some(router),
            current_event_store_route: Some(crate::project_registry::EventStoreRoute::Git {
                events_dir: event_dir.path().to_path_buf(),
                ci_merge_gate_configured: false,
            }),
            commit_watcher: crate::commit_watcher::CommitWatcherStore::default(),
            product_commit_watchers: crate::commit_watcher::ProductCommitWatcherStore::default(),
            capability_readiness: None,
            operational_logs: None,
            workspace_bindings_dir: Some(binding_dir.path().to_path_buf()),
            project_registrations: registrations.clone(),
            product_join_trust: None,
            confirmation_challenges: Arc::new(
                crate::confirmation_challenge_store::ConfirmationChallengeStore::new(),
            ),
            candidate_selection_authority: test_selection_fields().0,
            product_currentness: test_selection_fields().1,
            candidate_selection: test_selection_fields().2,
            atomic_adapter: test_selection_fields().3,
            projection_work_store: test_projection_work_store("fixture"),
            candidates: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            reviews: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            proof_signer: None,
        };
        let request = ToolRequest::new(
            ToolCommand::WorkspaceBind(WorkspaceBindRequest {
                proposal: WorkspaceBindingProposal {
                    project_id: "prd_live".to_string(),
                    display: ProjectSafeDisplayMetadata {
                        display_name: "Live Product".to_string(),
                        project_slug: None,
                        candidate_label: None,
                    },
                    candidate_path: candidate.path().display().to_string(),
                    source_surface: WorkspaceBindingSourceSurface::Mcp,
                    reason: "operator confirmed".to_string(),
                    confidence: 1.0,
                },
                confirmed: true,
                required_daemon_capability: Some(DAEMON_WORKSPACE_BINDING_CAPABILITY),
                expected_current_state: Some(WorkspaceBindingState::LocalWorkspaceUnbound),
            }),
            bicameral_api::fixtures::authority_mcp("agent-796", "session-796", "/workspace"),
        );

        let response = dispatch.dispatch(&request, Some(candidate.path()));
        assert_eq!(
            response.status,
            bicameral_api::tool_request::ToolResponseStatus::Ok,
            "{response:?}"
        );

        let live = registrations.read().unwrap();
        assert!(live[0].has_workspace_binding);
        assert_eq!(live[0].setup_state, "normal");
    }

    #[test]
    fn successful_continue_setup_updates_live_project_registry() {
        let adapter = Arc::new(MemoryAdapter::new());
        let membership = ProductMemberAddedPayload {
            product_id: "prd_setup".to_string(),
            cloud_account_id: "acct_alice".to_string(),
            local_actor_ref: "bridge:acct_alice".to_string(),
            granted_role: GrantedRole::ProductMember,
            verified_domain: "acme.example".to_string(),
            governance_policy_version: "gpv-1".to_string(),
            ticket_nonce: uuid::Uuid::new_v4().to_string(),
            receipt_id: uuid::Uuid::new_v4().to_string(),
            decided_at: Utc::now(),
        };
        adapter
            .append(EventStoreEntry::new(
                EventType::ProductMemberAdded,
                serde_json::to_value(membership).unwrap(),
                "daemon".to_string(),
            ))
            .unwrap();

        let binding_dir = tempfile::tempdir().unwrap();
        let event_dir = tempfile::tempdir().unwrap();
        let route = crate::project_registry::EventStoreRoute::Git {
            events_dir: event_dir.path().to_path_buf(),
            ci_merge_gate_configured: false,
        };
        let store: Arc<dyn crate::project_registry::ProjectRegistrationStore> =
            Arc::new(crate::project_registry::MemoryProjectRegistrationStore::new());
        let router = Arc::new(crate::project_registry::ProjectEventStoreRouter::new(store));
        let registrations = Arc::new(std::sync::RwLock::new(Vec::new()));
        let dispatch = DaemonDispatch {
            adapter,
            operator_scoped: false,
            project_router: Some(router),
            current_event_store_route: Some(route),
            commit_watcher: crate::commit_watcher::CommitWatcherStore::default(),
            product_commit_watchers: crate::commit_watcher::ProductCommitWatcherStore::default(),
            capability_readiness: None,
            operational_logs: None,
            workspace_bindings_dir: Some(binding_dir.path().to_path_buf()),
            project_registrations: registrations.clone(),
            product_join_trust: None,
            confirmation_challenges: Arc::new(
                crate::confirmation_challenge_store::ConfirmationChallengeStore::new(),
            ),
            candidate_selection_authority: test_selection_fields().0,
            product_currentness: test_selection_fields().1,
            candidate_selection: test_selection_fields().2,
            atomic_adapter: test_selection_fields().3,
            projection_work_store: test_projection_work_store("fixture"),
            candidates: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            reviews: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            proof_signer: None,
        };

        let mut audit_metadata = serde_json::Map::new();
        audit_metadata.insert(
            PAIRED_CLOUD_ACCOUNT_METADATA_KEY.to_string(),
            serde_json::Value::String("acct_alice".to_string()),
        );
        audit_metadata.insert(
            PROVIDER_ACCESS_METADATA_KEY.to_string(),
            serde_json::Value::String("authorized".to_string()),
        );
        audit_metadata.insert(
            DESCRIPTOR_VALIDATION_METADATA_KEY.to_string(),
            serde_json::Value::String("valid".to_string()),
        );
        let request = ToolRequest::new(
            ToolCommand::ProductContinueSetup(ProductContinueSetupParams {
                request_id: uuid::Uuid::new_v4().to_string(),
                product_id: "prd_setup".to_string(),
                registration_ref: "reg_setup".to_string(),
                provider_hint: Some(RegisteredProvider::Github),
                substrate_hint: Some(RegisteredSubstrate::Git),
                account_label: Some("acme".to_string()),
                requested_at: Utc::now(),
            }),
            AuthorityContext {
                actor_id: "bridge:acct_alice".to_string(),
                auth_method: AuthMethod::HostedBridge,
                session_id: Some("session-796".to_string()),
                workspace: None,
                policy_scope: vec!["hosted_bridge".to_string()],
                audit_metadata,
            },
        );

        let response = dispatch.dispatch(&request, None);
        assert_eq!(
            response.status,
            bicameral_api::tool_request::ToolResponseStatus::Ok,
            "{response:?}"
        );
        let live = registrations.read().unwrap();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].name, "prd_setup");
        assert!(!live[0].has_workspace_binding);
        assert_eq!(live[0].setup_state, "code_grounding_not_ready");
    }
}
