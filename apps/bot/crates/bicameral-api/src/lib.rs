//! Protocol types for the Bicameral governance runtime.
//!
//! This crate defines the typed objects that flow through the Bicameral system:
//! decision candidates, source/binding evidence, review commands, governance
//! results, and audit receipts.
//!
//! Derived from ZeroClaw's API layer, narrowed to Bicameral's governance domain.
//! See NOTICE and runtime/UPSTREAM-ZEROCLAW.md for attribution.

pub mod action_authorization;
pub mod alpha_recall;
pub mod candidate;
pub mod candidate_promotion;
pub mod candidate_selection;
pub mod candidate_session;
pub mod capability;
pub mod capability_readiness;
pub mod catalog;
pub mod confirmation_challenge;
pub mod constraint;
pub mod context;
pub mod dashboard;
pub mod decision_index;
pub mod decision_likeness;
pub mod drift;
pub mod egress;
pub mod egress_projection;
pub mod event_store;
pub mod evidence;
pub mod evidence_acceptance;
pub mod external_ingest;
pub mod fixtures;
pub mod freshness;
pub mod governance;
pub mod grounding;
pub mod hosted_bridge;
pub mod journey_review;
pub mod managed_preflight;
pub mod pairing_authorization;
pub mod product_creation;
pub mod product_join;
pub mod product_registration;
pub mod projection;
pub mod provider_acquisition;
pub mod provider_resource;
pub mod reasoning_projection;
pub mod recall;
pub mod recall_route_merge;
pub mod recall_taxonomy;
pub mod remote_action;
pub mod review;
pub mod semantic;
pub mod semantic_recall;
pub mod source;
pub mod staged_preflight;
#[path = "tool_request.rs"]
mod tool_request_legacy;
pub mod transformation_trace;
pub mod tool_request {
    pub use super::tool_request_legacy::{
        ExternalIngestEnvelope as LegacyExternalIngestEnvelope, *,
    };
    pub use crate::external_ingest::ExternalIngestEnvelope;
}
pub mod workspace_binding;

pub use action_authorization::{
    ActionAuthorizationPolicy, ActionDenial, ActionDenialReason, ActionPermit, AuthorizationResult,
    GovernedActionClass,
};
pub use alpha_recall::{
    AlphaRecallAuthority, AlphaRecallAuthorityMode, AlphaRecallCaveat, AlphaRecallCaveatScope,
    AlphaRecallCaveatStatus, AlphaRecallEnvelope, AlphaRecallMismatch, AlphaRecallMismatchKind,
    AlphaRecallProduct, AlphaRecallTopologyWitness, AlphaRecallTraceWitness, AlphaRecallWitness,
    ALPHA_RECALL_ENVELOPE_VERSION, ALPHA_RECALL_RESULT_KEY,
};
pub use candidate::{
    Authority, CandidateState, Decision, DecisionCandidate, DecisionLevel, ExtractionConfidence,
    ReviewLifecycleState, Strength, TrackingState,
};
pub use candidate_selection::{
    CandidatePromotionEffectV1, CandidatePromotionPolicyV1, CandidateRejectionEffectV1,
    CandidateSelectionAuthorityV1, CandidateSelectionCommitV1, CandidateSelectionConfirmationV1,
    CandidateSelectionFailureCode, CandidateSelectionFailureV1, CandidateSelectionPreviewV1,
    CandidateSelectionReceiptV1, CandidateSelectionResult, CandidateSelectionSignoffMode,
    CandidateSelectionV1, CANDIDATE_SELECTION_COMMIT_COMMAND, CANDIDATE_SELECTION_CONTRACT_VERSION,
    CANDIDATE_SELECTION_PREPARE_COMMAND,
};
pub use capability::{
    AlphaCapabilities, AlphaFeatureCapability, FeatureSupportStatus, MigrationStatus,
};
pub use catalog::{
    matches_query, query_catalog, supersede_constraint, CatalogConstraint, ConstraintQuery,
    ConstraintQueryResult, ConstraintScope, ConstraintType,
};
pub use confirmation_challenge::{
    ChallengeBinding, ChallengeSecret, ConfirmationOutcome, ConfirmationRejection,
    ConfirmationRequiredResponse, ConfirmationToken, CONFIRMATION_CHALLENGE_PROTOCOL_VERSION,
    CONFIRMATION_CHALLENGE_TTL_SECONDS,
};
pub use constraint::{
    ActorType, Attribution, CanonicalConstraint, CanonicalReviewState, DecisionReason,
    LifecycleReviewState, PromotionError, PromotionErrorCode,
};
pub use context::{
    meets_confidence, rank_context_items, ContextItem, ContextPacket, ContextRequest,
    EvidenceSummary, WorkSurfaceKind, WorkSurfaceRef,
};
pub use decision_index::{
    CompilationState, CoverageReport, DecisionIndexRecord, IndexSourceRef, MetadataAcceptanceState,
    MetadataEntry, MetadataExtractionMethod, MetadataProvenance, MetadataQualityLabel,
    MetadataSourceType, QualityLabelKind, METADATA_SCHEMA_VERSION, REQUIRED_METADATA_FIELDS,
};
pub use decision_likeness::{
    evaluate_candidate, first_decision_like_segment, DecisionLikenessEvaluation,
    DecisionLikenessReasonCode, DECISION_LIKENESS_THRESHOLD,
};
pub use drift::{
    can_transition, transition_finding, ContradictionReport, DriftSignal, FindingSeverity,
    FindingStatus, ReviewTransitionRequest, TransitionError, TransitionErrorCode,
};
pub use egress::{
    CanonicalArtifactType, ProfileRegistry, ProfileRejection, ProjectionProfile, ResolvedProfile,
    TargetAdapter, TargetIntegrationDescriptor,
};
pub use egress_projection::{
    ApprovalClass, EgressEligibilityCheck, EgressMode, EgressProjection, EgressReceipt,
    PolicyVerdict, ProjectionPolicy, ProjectionPolicyResult, ProjectionProfileRef,
    ReconciliationState, TargetAdapterRef,
};
pub use event_store::{
    event_store_entry_digest, serialize_event_store_entry, BindToCodePayload,
    ClassifyConstraintPayload, EventStoreAdapter, EventStoreEntry, EventStoreError, EventType,
    ExternalIngestRejection, ExternalIngestRejectionReason, IngestPayload, IngestRejection,
    IngestRejectionReason, ManagedIngestRejection, ManagedIngestRejectionReason,
    ProductCreatedPayload, PromoteToCanonicalPayload, RaiseContradictionPayload,
    RaiseCorrectionFindingPayload, RecordSemanticMappingPayload, RevalidateBindingPayload,
    RevalidationOutcome, SubstrateCapability, SubstrateKind, SupersedeConstraintPayload,
    SymbolIdentity, TransitionFindingPayload, UntrackReason, UntrackSourcePayload,
};
pub use evidence::{
    BindingEvidence, CodeSourceCurrentness, CodeSourceRefreshResult, EvidenceReference,
    EvidenceRefreshOutcome, EvidenceRefreshStatus, FileInspectionResult, GraphEvidenceState,
    GraphReadiness, GraphReadinessView, GraphSnapshot, IndexSource, PublicGraphEvidenceState,
    RustCaptureKind, SnapshotResolution, SourceEvidence, SubmissionProvenance, SymbolKind,
    SymbolLanguage, SymbolOccurrence, SymbolResolutionResult,
};
pub use external_ingest::{
    ExternalCandidateHint, ExternalEvidenceItem, ExternalIngestCapabilities,
    ExternalIngestEnvelope, ExternalRedactionFinding, ExternalRedactionReceipt,
    EXTERNAL_INGEST_CAPABILITIES_ENDPOINT, EXTERNAL_INGEST_CONTRACT_FINGERPRINT,
    EXTERNAL_INGEST_CONTRACT_ID, EXTERNAL_INGEST_DELIVERY_ENDPOINT,
    EXTERNAL_INGEST_PROTOCOL_VERSION, EXTERNAL_INGEST_REQUEST_SCHEMA_SHA256,
    MINIMUM_SUPPORTED_EXTERNAL_INGEST_PROTOCOL_VERSION,
};
pub use freshness::{
    CommitFreshness, FreshnessCaveat, FreshnessCaveatCode, FreshnessInputs, FreshnessNextAction,
    PreflightFreshness, RefreshOutcome, TouchedDetectionMethod, TouchedScope,
};
pub use governance::{GovernancePolicy, GovernanceResult, GovernanceVerdict};
pub use grounding::{
    is_actionable, region_overlaps, CodeRegionRef, CorrectionFinding, PrRef, SourceDocRef,
    SuggestedAction,
};
pub use hosted_bridge::{
    detect_smuggled_fields, AuthoritySmugglingPayload, HostedBridgeCapability,
    HostedBridgeErrorKind, HostedBridgeErrorResponse, HostedBridgePairingRequest,
    HostedBridgePairingResponse, HostedBridgePairingState, HostedBridgeRevokedResponse,
    OperatorHomeSessionResponse, ProjectReadinessEntry, BRIDGE_FORBIDDEN_FIELDS,
};
pub use pairing_authorization::{
    derived_identity_key_id, format_instant_z, pairing_authorization_digest_bytes,
    pairing_authorization_digest_hex, validate_pairing_authorization_local, PairingAuthorizationV1,
    PairingAuthorizationValidationError, PAIRING_AUTHORIZATION_AUDIENCE,
    PAIRING_AUTHORIZATION_MAX_TTL_SECS, PAIRING_AUTHORIZATION_PROFILE,
    PAIRING_AUTHORIZATION_PURPOSE, PAIRING_AUTHORIZATION_SIGNATURE_ALGORITHM,
    PAIRING_AUTHORIZATION_TTL_SECS,
};
pub use product_creation::{
    classify_product_create, contains_forbidden_material, creator_role_labels,
    proof_canonical_bytes_v1_lab, proof_canonical_bytes_v2, proof_digest_v2, validate_hosted_safe,
    EventStoreHint, HostedActorContext, HostedProjectPreview, ProductCreateDenial,
    ProductCreateDenialReason, ProductCreateParams, ProductCreationCommitProof,
    ProductCreationPublication, ProductCreationResult, ProductEventStoreDescriptor,
    ProductPublishParams, ProductRole, RequiredConnection, CREATOR_ROLES,
    FORBIDDEN_RECEIPT_SUBSTRINGS, PROOF_SCHEMA_VERSION_V2,
};
pub use provider_acquisition::{
    ActionNeededReason, FreshnessMetadata, PermissionState, ProviderItemEnvelope,
    ProviderResourceDescriptor, ResourceCapability, ResourceRef,
};
pub use reasoning_projection::{
    projection_request_from_external_ingest, projection_request_from_local_ingest,
    AlphaProjectionOutcome, CandidateDraftInput, CandidateHintInput,
    DeterministicProjectionProvider, ProjectedCandidateProposal, ProjectionDeferralReason,
    ProjectionProvider, ProjectionProviderMode, ProjectionProviderReadiness, ProjectionRequest,
    ProjectionResult, ReasoningMode, ReasoningProjectionService, UnavailableProjectionProvider,
};
pub use recall::{
    ApprovalProof, AuthorityLabel, AuthorityLabelKind, CandidateRelationship,
    CanonicalDecisionEntry, CodeEvidenceRef, CodeEvidenceVerification, ConflictResolution,
    EvidenceFreshness, EvidenceRef, EvidenceRefKind, ForbiddenPreferenceAction,
    InspectedEvidenceItem, NoResultClaim, NoResultLanguage, NotificationCategory,
    NotificationOverride, NotificationPriority, PreferenceAuditRecord, PreferenceCapViolation,
    PreferenceCapViolationKind, PreferenceChangeKind, PreferenceFieldChange, PreferenceScope,
    PromotionOutcome, PromotionRequestOutcome, RankTuple, RankingTier, RecallAction,
    RecallActionKind, RecallExpandScopeParams, RecallExpandScopeResult,
    RecallInspectEvidenceParams, RecallInspectEvidenceResult, RecallPacket,
    RecallPreferenceProfile, RecallPromoteCandidateParams, RecallPromoteCandidateResult,
    RecallProviderProvenance, RecallProviderSignal, RecallProviderSignalKind, RecallProviderStatus,
    RecallRankingMetadata, RecallRefusal, RecallRefusalCode, RecallRoute, RecallScopeKind,
    RecallSearchedScope, RecallSignal, RecallSignalRequest, RecallSignalResult, RecallTrace,
    RecallUnknownProviderScope, RecallUnknownProviderScopeReason, ResolvedPreferenceProfile,
    RouteResult, RouteStatus, RouteWeightOverride, ScopeLevel, ScopeSection, ScopeSectionStatus,
    SearchedScope, SemanticCodeCandidate, SemanticCodeSearchRequest, SemanticCodeSearchResult,
    SemanticCodeTargetType, SemanticScoreKind, SourceClassAdjustment, SourceRef, StopCondition,
    StopConditionKind, SurfacedDecisionCandidate, TierWeightOverride, UnknownScopeEntry,
    ROUTE_WEIGHT_CAP, ROUTE_WEIGHT_FLOOR, SOURCE_CLASS_ADJUSTMENT_CAP, TIER_WEIGHT_CAP,
    TIER_WEIGHT_FLOOR,
};
pub use recall_route_merge::{
    merge_and_rank, CandidateIdentity, CandidateIdentityKind, RankedRecallCandidate,
    RankingTier as RouteMergeRankingTier, ReasonCode,
    RecallPreferenceProfile as RouteMergePreferenceProfile, RouteCandidate, RouteClass,
    TierSortKey,
};
pub use recall_taxonomy::{
    alpha_registry, ChangePolicy, ControlledValue, DeprecationInfo, ResolveResult, TaxonomyKind,
    TaxonomyRegistry, TaxonomyVersion,
};
pub use remote_action::{
    detect_remote_action_smuggled_fields, AuthenticatedRemoteActionEnvelope, CallbackSource,
    RemoteActionKind, RemoteActionOutcome, RemoteActionReceipt, RemoteActionRejection,
    RemoteActionRejectionReason, RemoteActionValidationResult, RemoteSessionBinding,
    ValidatedRemoteAction, REMOTE_ACTION_FORBIDDEN_FIELDS,
};
pub use review::{ReviewCommand, ReviewCommandKind, ReviewState};
pub use semantic::{
    is_confident, resolve_mapping, MappingExample, Provenance, SemanticError, SemanticErrorCode,
    SemanticMapping, SourcePhraseRef,
};
pub use semantic_recall::{
    SemanticEvidenceTier, SemanticRecallCaveat, SemanticRecallCaveatCode, SemanticRecallEntry,
    SemanticRecallInputs, SemanticRecallNextAction, SemanticRecallView,
};
pub use source::SourceKind;
pub use tool_request::{
    AllowIndex, AuthMethod, AuthorityContext, BindingDiscoveryPhase, CheckpointHint,
    CodeImpactCandidate, CodeImpactCaveat, CodeImpactCaveatKind, CodeLocateParams,
    FindCodeImpactHint, FindCodeImpactHintKind, FindCodeImpactNextAction, FindCodeImpactParams,
    FindCodeImpactResult, FindCodeImpactStatus, GraphRefreshParams, GraphRefreshResult,
    GraphRefreshStatus, GraphRefreshTarget, GraphSliceInfo, GraphStatusParams, GraphStatusResult,
    LocatorCandidate, LocatorConfidence, LocatorEvidenceState, LocatorGraphReadiness,
    LocatorReasonCode, LocatorResult, LocatorScopeKind, LocatorStatus, LocatorTargetType,
    PreflightRunParams, RefreshScopeKind, RefreshTrigger, RuntimeTrace, SearchPlanSummary,
    TokenUsage, ToolCommand, ToolRequest, ToolResponse, ToolResponseStatus, TraceStatus,
};
pub use workspace_binding::{
    classify_bind_request, DaemonBindingFacts, ProjectSafeDisplayMetadata, WorkspaceBindErrorKind,
    WorkspaceBindErrorResponse, WorkspaceBindRequest, WorkspaceBindResponse,
    WorkspaceBindingProposal, WorkspaceBindingReadinessView, WorkspaceBindingSourceSurface,
    WorkspaceBindingState, WorkspaceResolveErrorKind, WorkspaceResolveErrorResponse,
    WorkspaceResolveRequest, WorkspaceResolveResponse, WORKSPACE_BINDING_LOCAL_ONLY_FIELDS,
};
