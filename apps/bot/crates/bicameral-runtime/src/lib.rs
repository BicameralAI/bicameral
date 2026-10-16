//! Local daemon, service lifecycle, and governance engine for Bicameral.
//!
//! The runtime provides the "boring middle": daemon lifecycle, signal handling,
//! governance policy evaluation, event-store substrate management, and the
//! coordination that ties gateway, mods, and audit together.
//!
//! Derived from ZeroClaw's runtime layer (daemon, service, approval, SOP).
//! See NOTICE and runtime/UPSTREAM-ZEROCLAW.md for attribution.

pub mod atomic_event_store;
pub mod bind_to_code;
pub mod candidate_selection_authority;
pub mod candidate_selection_service;
pub mod candidate_session_store;
pub mod capability_readiness;
pub mod code_locator;
pub mod code_source_refresh;
pub mod commit_freshness;
pub mod commit_watcher;
pub mod compliance;
pub mod confirmation_challenge_store;
pub mod context_resolver;
pub mod daemon;
pub mod dispatch;
pub mod drift_detector;
pub mod drive_api_event_store;
#[path = "drive_api_transport.rs"]
pub(crate) mod drive_api_transport_base;
pub mod google_drive_credential_route;
pub mod product_creation_proof;
/// Public Drive transport surface. The underlying transport remains unchanged;
/// only the durable route opener is replaced with Row-5 credential-aware
/// composition so restart refresh can recover the Desktop credential locally.
pub mod drive_api_transport {
    pub use super::drive_api_transport_base::{
        production_drive_event_store, DriveAccessTokenProvider, HttpDriveApiTransport,
        KeyringGoogleAccessTokenProvider, CANONICAL_EVENT_MARKER_KEY, REFRESH_SKEW_SECONDS,
    };
    pub use super::google_drive_credential_route::open_google_drive_api_route;
}
pub mod egress;
pub mod event_store_adapters;
pub mod find_code_impact;
pub mod governance;
pub mod graph_readiness_view;
pub mod graph_snapshot_service;
pub mod grounding;
pub mod history;
pub mod live_recall;
pub mod managed_preflight;
pub mod operational_log;
pub mod preflight;
pub mod product_creation;
pub mod product_join;
pub mod product_join_trust_store;
pub mod product_registration;
pub mod project_registry;
pub mod projection;
pub mod provider_envelope_bridge;
pub mod reasoning_projection;
pub mod recall_admissibility;
pub mod recall_provider;
pub mod refresh;
pub mod remote_action_gateway;
pub mod replay;
pub mod search;
pub mod semantic_chunk_index;
pub mod service;
pub mod symbol_resolver;
pub mod test_fixtures;
pub mod update;
pub mod workspace_binding;

pub use bind_to_code::{
    bind_symbol_to_code, bind_to_code as bind_to_code_fn, BindToCodeRequest, BindToCodeResult,
};
pub use compliance::{
    evaluate_without_inspector, signoff_is_not_compliance_proof, BindingVerdict,
    ComplianceEvaluator, ComplianceVerdict,
};
pub use daemon::{DaemonConfig, DaemonExit};
pub use dispatch::{
    capability_report, capability_report_with_readiness, dispatch, dispatch_with_trace,
    error_with_trace, reject_with_trace, CapabilityReport, DispatchOutcome,
    PreflightStageCapability, DEFERRED_COMMANDS, PREFLIGHT_STAGE_CAPABILITIES, SUPPORTED_COMMANDS,
};
pub use drive_api_event_store::{
    DriveApiAdapter, DriveApiTransport, DriveAppendConfirmation, DriveRemoteObject,
    DriveTransportError, DriveTransportErrorKind,
};
pub use egress::{
    alpha_default_policy, AdapterExecutionResult, ArtifactState, EgressService, TargetAdapter,
};
pub use governance::GovernanceEngine;
pub use graph_readiness_view::{
    build_readiness_view, compliance_for_readiness, is_non_verified_graph_state,
    is_verified_evidence, lifecycle_gate, validate_signoff_not_from_graph_grounding,
};
pub use graph_snapshot_service::{
    GraphSnapshotService, GraphSnapshotServiceError, LocalGraphProvider,
};
pub use grounding::{FileInspector, GroundingError, SnapshotResolver};
// Re-export snapshot resolution types from the API crate for convenience.
pub use bicameral_api::evidence::{
    CodeSourceCurrentness, CodeSourceRefreshResult, GraphReadiness, SnapshotResolution,
};
pub use code_source_refresh::{
    currentness_for_readiness, evaluate_binding_currentness, evaluate_decision_bindings,
};
pub use commit_freshness::{
    bounded_refresh_check, build_preflight_refresh_request, compute_preflight_freshness,
    compute_preflight_freshness_with_watcher, decide_bounded_outcome, derive_touched_scope,
    parse_touched_files_from_diff, BoundedRefresh, BOUNDED_SCOPE_BUDGET,
};
pub use commit_watcher::{
    compute_watcher_preflight_freshness, CommitWatcher, CommitWatcherObservation,
    CommitWatcherStore,
};
pub use event_store_adapters::adapter_from_config;
pub use history::{query_history, HistoryQuery, HistoryResponse};
pub use preflight::{
    build_locator_context, enrich_with_locator, query_preflight, query_preflight_progressive,
    PreflightGraphContext, PreflightLocatorContext, PreflightQuery, PreflightReadiness,
    PreflightRelevance, PreflightResponse, SnapshotScopeEntry, UnknownScopeEntry,
};
pub use product_creation::{
    build_publication, create_product, replay_products, ProductCreateError, ProductCreationOutcome,
    ProductLedger, ProductPublishError, ProductRecord,
};
pub use projection::{
    build_projection, rebuild_projection, MemoryProjectionStore, ProjectedCodeEvidence,
    ProjectedDecision, ProjectedGraphReadiness, ProjectedGraphSnapshot, ProjectedLedgerEvent,
    ProjectedSourceEvidence, ProjectionError, ProjectionState, ProjectionStore,
};
pub use provider_envelope_bridge::{bridge_envelope_to_source, EnvelopeBridgeOutput};
pub use reasoning_projection::{
    projection_request_from_external_ingest, projection_request_from_local_ingest,
    DeterministicProjectionProvider, ProjectionProvider, ReasoningProjectionService,
    UnavailableProjectionProvider,
};
pub use recall_admissibility::{
    RecallAdmissibilityDecision, RecallAdmissibilityGate, RecallAdmissibilityOutcome,
    RecallAdmissibilityReasonCode, RecallAdmissibleUse, RecallAuthorityEffects,
};
pub use recall_provider::{
    fake_semantic_candidate, fake_semantic_no_result_language, fake_semantic_ranking_metadata,
    FakeSemanticCodeSearchFixture, FakeSemanticCodeSearchProvider, LocalSemanticCodeSearchProvider,
    NoopRecallSignalProvider, NoopSemanticCodeSearchProvider, RecallProviders,
    RecallSignalProvider, SemanticCodeSearchProvider,
};
pub use refresh::{is_auto_eligible, query_refresh_eligible, RefreshEntry, RefreshMode};
pub use remote_action_gateway::{
    ActorBinding, ActorBindingRegistry, MemoryActorBindingRegistry, MemoryNonceStore, NonceStore,
    RemoteActionGateway, RemoteActionGatewayConfig, SignatureVerifier,
};
pub use replay::{replay_from_adapter, ReplayedState};
pub use search::{
    enrich_search_with_locator, query_search, SearchCodeContext, SearchQuery, SearchResponse,
};
pub use semantic_chunk_index::{
    LocalSemanticCodeChunkIndexer, SemanticCodeChunkIndex, SemanticCodeChunkIndexRequest,
    SemanticCodeChunkKind, SemanticCodeChunkMetadata, SemanticCodeSkippedFile,
    SemanticCodeSkippedReason, SEMANTIC_CHUNKER_VERSION, SEMANTIC_CHUNK_INDEX_VERSION,
};
pub use symbol_resolver::RustSymbolResolver;
