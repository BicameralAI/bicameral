//! Event-store adapter trait and types.
//!
//! The event store is the substrate-neutral materialization boundary.
//! Accepted governance results are materialized here — and only here.
//! No ingest handler, mod, or gateway route may write canonical decisions
//! directly; they must go through governance and then the event-store adapter.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

/// Substrate-neutral event store adapter trait.
///
/// Implementations may back onto git (JSONL event log in a repo), a local
/// folder compatible with Google Drive sync, or future cloud substrates.
/// The interface must not hardcode git assumptions.
pub trait EventStoreAdapter: Send + Sync {
    /// Append an accepted event to the store.
    fn append(&self, entry: EventStoreEntry) -> Result<(), EventStoreError>;

    /// Replay all events in order.
    fn replay(&self) -> Result<Vec<EventStoreEntry>, EventStoreError>;

    /// Get the current substrate kind.
    fn substrate_kind(&self) -> SubstrateKind;

    /// Check connectivity/freshness of the substrate.
    fn health_check(&self) -> Result<SubstrateHealth, EventStoreError>;

    /// Declare the optional capabilities this substrate supports beyond
    /// the baseline append/replay/health contract.
    fn capabilities(&self) -> BTreeSet<SubstrateCapability>;
}

/// A single entry in the event store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventStoreEntry {
    pub id: Uuid,
    pub event_type: EventType,
    pub payload: serde_json::Value,
    pub timestamp: DateTime<Utc>,
    pub actor: String,
    pub governance_result_id: Option<Uuid>,
}

impl EventStoreEntry {
    pub fn new(event_type: EventType, payload: serde_json::Value, actor: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            event_type,
            payload,
            timestamp: Utc::now(),
            actor,
            governance_result_id: None,
        }
    }
}

/// Exact bytes written on Drive JSON append (`{event_id}.json`).
///
/// `event_digest` on a v2 `ProductCreationCommitProof` is SHA-256 of these
/// bytes. Pretty-printed serde JSON is the Drive on-disk contract; do not hash
/// a different serializer or a `ProductCreationResult`.
///
/// `GitAdapter` still writes YAML (ADR-0001 event-first layout) and is **not**
/// a v2 proof substrate until it appends these same bytes. A Git proof must
/// not claim exact-byte durability against YAML that was actually appended.
pub fn serialize_event_store_entry(entry: &EventStoreEntry) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec_pretty(entry)
}

/// `sha256:<hex>` of [`serialize_event_store_entry`].
pub fn event_store_entry_digest(entry: &EventStoreEntry) -> Result<String, serde_json::Error> {
    use sha2::{Digest, Sha256};
    let bytes = serialize_event_store_entry(entry)?;
    let hex: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(format!("sha256:{hex}"))
}

/// Event types aligned with MCP v0.2 spec §4.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    /// Candidate ingested with source evidence.
    Ingest,
    /// Candidate accepted → promoted to Decision(proposed).
    AcceptCandidate,
    /// Candidate rejected by governance or human.
    RejectCandidate,
    /// Decision signoff approved → Decision(approved).
    ///
    /// Ownership approval only — does not imply compliance, enforcement,
    /// or code readiness (ADR-0022).
    ApproveSignoff,
    /// Decision signoff rejected (ownership rejection only).
    RejectSignoff,
    /// Compliance state resolved (reflected/partial/drifted).
    ResolveCompliance,
    /// Decision bound to code region.
    BindToCode,
    /// Decision superseded by a newer decision.
    SupersedeDecision,
    /// Binding revalidated or replaced at a later snapshot.
    RevalidateBinding,
    /// Candidate edited (title/body changed).
    EditCandidate,
    /// Reviewed decision promoted to a canonical constraint.
    PromoteToCanonical,
    /// Canonical constraint classified with a type + scope (enters the catalog).
    ClassifyConstraint,
    /// One catalog constraint superseded by another (immutable, versioned).
    SupersedeConstraint,
    /// A contradiction finding raised into the reviewable state.
    RaiseContradiction,
    /// A human-driven status transition on a contradiction finding.
    TransitionFinding,
    /// A durable semantic mapping recorded from source language to a constraint.
    RecordSemanticMapping,
    /// A correction-capture finding raised for a changed grounded code region.
    RaiseCorrectionFinding,
    /// Decision tracking stopped — automatic refresh ceases for this Decision.
    ///
    /// Emitted by `reject_signoff`, `supersede_decision`, explicit user
    /// untrack, or a future demotion policy. Evidence may go stale without
    /// creating a governance failure.
    UntrackSource,
    /// External adapter evidence ingested through the bot-owned gateway.
    ///
    /// External ingest creates source/evidence/candidate records but never
    /// carries local actor authority, review commands, signoff approval,
    /// compliance resolution, or policy grants.
    ExternalIngest,
    /// Screened external evidence accepted independently of projection.
    ///
    /// Replay restores the acceptance fact and content locator only. It must
    /// never schedule projection work or reconstruct candidates.
    EvidenceAccepted,
    /// Explicit approval-gated correction request referencing a prior RecallPacket.
    ///
    /// Creates a correction candidate for the governed review path. Does not
    /// create accepted Decision, BindingEvidence, signoff, compliance, or
    /// enforcement state. Approval proof is single-use.
    RequestCorrection,
    /// Canonical hosted Product creation (issue #766).
    ///
    /// A single, atomic event that mints the Product identity, materializes the
    /// creator's roles (exactly Policy Owner + Product Owner) in one membership
    /// event, and records the initial Governance Policy Version. Replay
    /// reconstructs Product identity and creator authority from this payload
    /// alone, without consulting Cloud as canonical state; the payload's
    /// `idempotency_key` dedups re-emitted creation events on replay.
    CreateProduct,
    /// Canonical Product Member auto-join granted (issue #767).
    ///
    /// The sole durable fact written by `product.join`: it records the granted
    /// Product Member role, the verified domain, the active Governance Policy
    /// Version, and the single-use ticket nonce used for replay defense. It
    /// never carries a hosted session/refresh token, a signature, or Cloud
    /// membership state.
    ProductMemberAdded,
    /// Canonical Product auto-join policy updated after creation.
    ///
    /// `CreateProduct` atomically carries the initial verified-domain policy.
    /// This event records a later replacement containing the enabled flag,
    /// allowed domains, and Governance Policy Version that `product.join` re-evaluates in
    /// canonical state.
    ProductAutoJoinPolicySet,
    /// Explicit local `ProjectRegistration` written after a canonical join
    /// completes provider access + event-store descriptor validation (issue
    /// #769).
    ///
    /// The sole durable local fact written by `product.continue_setup`: it maps
    /// a joined Product to its validated provider/substrate routing so the
    /// daemon can open the adapter and replay canonical Product state. It is
    /// operator-local (never Cloud/SPA authority) and carries only non-secret
    /// metadata and opaque references — never a provider token, raw event-store
    /// descriptor, or local filesystem path. Setup/registration is orthogonal
    /// to membership: this event never grants, changes, or revokes a Product
    /// Member role.
    ProjectRegistrationWritten,
    // Legacy variants kept for backward-compatible replay.
    DecisionAccepted,
    DecisionRejected,
    EvidenceAttached,
    ReviewCompleted,
    PolicyUpdated,
}

/// Which substrate backs the event store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubstrateKind {
    /// Git-backed JSONL event log in a repository.
    Git,
    /// Local folder compatible with Google Drive sync.
    DriveFolder,
    /// In-memory (testing only).
    Memory,
}

impl std::fmt::Display for SubstrateKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Git => write!(f, "git"),
            Self::DriveFolder => write!(f, "drive_folder"),
            Self::Memory => write!(f, "memory"),
        }
    }
}

impl std::fmt::Display for SubstrateCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DurableReplay => write!(f, "durable_replay"),
            Self::BranchLocalProposals => write!(f, "branch_local_proposals"),
            Self::PrNativeReview => write!(f, "pr_native_review"),
            Self::NativeMergeConflictResolution => {
                write!(f, "native_merge_conflict_resolution")
            }
            Self::CiMergeGateEnforcement => write!(f, "ci_merge_gate_enforcement"),
        }
    }
}

/// Optional capabilities a substrate may advertise beyond the baseline
/// append/replay/health contract.
///
/// The baseline contract (append, replay, health_check) is always present on
/// every `EventStoreAdapter`. Capabilities declared here represent additional
/// substrate-specific behaviors that higher layers can query at runtime to
/// decide which governance or resolution workflows are available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubstrateCapability {
    /// Events survive process restarts and are recoverable from durable storage.
    DurableReplay,
    /// The substrate supports branch-local proposal isolation (e.g. git branches).
    BranchLocalProposals,
    /// Review discussion is native to the substrate (e.g. GitHub PR reviews).
    PrNativeReview,
    /// The substrate can detect and surface merge conflicts natively.
    NativeMergeConflictResolution,
    /// CI or merge-gate enforcement is available on the substrate.
    CiMergeGateEnforcement,
}

/// Freshness assessment of the substrate's local state.
///
/// Used by shared-store substrates (Drive, future cloud adapters) to surface
/// staleness, offline state, and sync-unknown conditions. Git substrates
/// typically report `Fresh` because the working copy is the authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessState {
    /// Local data is current (sync marker within threshold).
    Fresh,
    /// Sync marker exists but is older than the configured threshold.
    Stale,
    /// Folder exists but no sync marker — freshness cannot be determined.
    Unknown,
    /// Folder is not reachable or does not exist.
    Offline,
}

/// Health/freshness status of the event-store substrate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubstrateHealth {
    pub kind: SubstrateKind,
    pub reachable: bool,
    pub last_synced: Option<DateTime<Utc>>,
    pub entry_count: usize,
    #[serde(default)]
    pub freshness: Option<FreshnessState>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub capabilities: BTreeSet<SubstrateCapability>,
}

/// Portable ingest event payload — the domain data recorded in an `Ingest` event.
///
/// Uses portable identifiers (source URI, snapshot content-address, evidence UUID)
/// rather than substrate-specific paths. Each adapter decides how and where to
/// materialize retained content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestPayload {
    pub source: crate::source::Source,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<crate::source::SourceSnapshot>,
    pub evidence: Vec<crate::evidence::SourceEvidence>,
    pub candidate: crate::candidate::DecisionCandidate,
    /// Typed evidence references (MCP sessions, tool calls, command output,
    /// transcript segments, etc.). These carry `SourceKind` and optional
    /// kind-specific pointer/metadata, preserving the evidence chain from
    /// source capture through to promoted Decisions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_references: Vec<crate::evidence::EvidenceReference>,
    /// Informational provenance metadata capturing which surface submitted
    /// this evidence and the associated actor/session/request context.
    /// Never creates governance authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<crate::evidence::SubmissionProvenance>,
}

/// Payload for a `BindToCode` event — verified file/range binding evidence.
///
/// Only verified `GraphEvidenceState` may produce a materialized binding.
/// The payload preserves the snapshot identity used during validation so
/// replay and audit can detect stale or mismatched evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindToCodePayload {
    pub decision_id: uuid::Uuid,
    pub binding: crate::evidence::BindingEvidence,
    pub graph_snapshot_id: String,
    pub validated_sha: String,
    pub evidence_state: crate::evidence::GraphEvidenceState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_start: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_end: Option<u32>,
}

/// Payload for a `ResolveCompliance` event — records the evaluated compliance
/// state for a decision based on its bindings and current grounding evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveCompliancePayload {
    pub decision_id: uuid::Uuid,
    pub compliance_state: crate::dashboard::ComplianceState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Payload for a `PromoteToCanonical` event — a reviewed decision promoted into
/// a canonical constraint.
///
/// Self-contained: replay reconstructs the [`CanonicalConstraint`] from this
/// payload alone (matching the `IngestPayload`/`BindToCodePayload` precedent),
/// preserving reviewer, source decision, evidence, and reason. `idempotency_key`
/// dedups re-emitted promotion events on replay.
///
/// [`CanonicalConstraint`]: crate::constraint::CanonicalConstraint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromoteToCanonicalPayload {
    pub decision_id: uuid::Uuid,
    pub title: String,
    pub statement: String,
    #[serde(default)]
    pub evidence_refs: Vec<uuid::Uuid>,
    #[serde(default)]
    pub affected_surfaces: Vec<String>,
    pub reviewer: crate::constraint::Attribution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<crate::constraint::DecisionReason>,
    pub idempotency_key: String,
}

/// Payload for a `ClassifyConstraint` event — sets a promoted canonical
/// constraint's catalog `type` + `scope`, materializing it into the catalog.
///
/// `idempotency_key` links to the #481 `PromoteToCanonical` event whose
/// canonical constraint is being classified. `affected_surfaces`, when present,
/// overrides the value carried from promotion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifyConstraintPayload {
    pub idempotency_key: String,
    pub constraint_type: crate::catalog::ConstraintType,
    pub scope: crate::catalog::ConstraintScope,
    pub attribution: crate::constraint::Attribution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affected_surfaces: Option<Vec<String>>,
}

/// Payload for a `SupersedeConstraint` event — supersedes one catalog
/// constraint with another. Both keys reference already-classified catalog
/// entries by their promotion `idempotency_key`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupersedeConstraintPayload {
    pub current_idempotency_key: String,
    pub replacement_idempotency_key: String,
}

/// Payload for a `RaiseContradiction` event — durably records a contradiction
/// finding and (on replay) writes the `conflicts_with` edges between the
/// related catalog constraints. Self-contained per the payload precedent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaiseContradictionPayload {
    pub report: crate::drift::ContradictionReport,
}

/// Payload for a `TransitionFinding` event — a human-driven status transition
/// on an already-raised finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionFindingPayload {
    pub request: crate::drift::ReviewTransitionRequest,
}

/// Payload for a `RecordSemanticMapping` event — durably records a semantic
/// mapping (idempotent by mapping id on replay).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordSemanticMappingPayload {
    pub mapping: crate::semantic::SemanticMapping,
}

/// Payload for a `RaiseCorrectionFinding` event — durably records a
/// correction-capture finding (idempotent by finding id on replay).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaiseCorrectionFindingPayload {
    pub finding: crate::grounding::CorrectionFinding,
}

/// Payload for a `CreateProduct` event — the canonical, atomic hosted Product
/// creation record (issue #766).
///
/// Self-contained: replay reconstructs the Product identity, the creator's
/// atomic role set, and the initial Governance Policy Version from this payload
/// alone (matching the `IngestPayload`/`PromoteToCanonicalPayload` precedent).
/// `idempotency_key` dedups re-emitted creation events on replay so a retry
/// never materializes a second Product or duplicate role grant.
///
/// The payload is hosted-safe by construction: it carries only Product identity,
/// the creator's Cloud account, coarse display labels, and safe hints — never
/// provider secrets, Supabase tokens, raw event-store credentials, or local
/// paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductCreatedPayload {
    /// Canonical Product identity minted by the daemon.
    pub product_id: String,
    /// Cloud account of the creator, bound from the validated hosted session.
    pub creator_cloud_account_id: String,
    /// Atomic creator role set: exactly `["Policy Owner", "Product Owner"]`.
    pub creator_roles: Vec<String>,
    /// Canonical, versioned Governance Policy Version created with the Product.
    pub governance_policy_version: String,
    /// Canonical creation receipt id.
    pub receipt_id: String,
    /// Idempotency key that dedups re-emitted creation events on replay.
    pub idempotency_key: String,
    /// Human-facing Product display name (non-authoritative label).
    pub product_display_name: String,
    /// Non-canonical team/company label for hosted directory projection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_display_name: Option<String>,
    /// Verified domain used for same-domain discovery eligibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_domain: Option<String>,
    /// Safe event-store hint surfaced in hosted projections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_store_hint: Option<crate::product_creation::EventStoreHint>,
    /// Safe required-connection hint surfaced in hosted projections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_connection: Option<crate::product_creation::RequiredConnection>,
    /// Canonical creation timestamp (RFC 3339).
    pub created_at: String,
}

/// Payload for a `RevalidateBinding` event — binding revalidation or replacement.
///
/// Preserves old `BindingEvidence` records immutably. Replay accumulates
/// lineage/revalidation history rather than mutating prior events. A
/// revalidation at a later verified snapshot does not create a duplicate
/// initial binding; it extends the binding's audit trail.
///
/// Failed or non-verified revalidation remains distinguishable from a durable
/// accepted revalidation via `RevalidationOutcome`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevalidateBindingPayload {
    /// The decision this binding belongs to.
    pub decision_id: uuid::Uuid,
    /// The binding being revalidated (the original/prior binding id).
    pub binding_id: uuid::Uuid,
    /// Root binding id in a lineage chain (equals `binding_id` for first revalidation).
    pub root_binding_id: uuid::Uuid,
    /// Binding type/target kind from the original binding.
    pub binding_type: crate::evidence::BindingType,
    /// Target string from the binding (file path, symbol, etc.).
    pub target: String,
    /// Graph snapshot id at which the original binding was created.
    pub original_graph_snapshot_id: String,
    /// Git SHA at which the original binding was validated.
    pub original_validated_sha: String,
    /// Graph snapshot id at which this revalidation was performed.
    pub current_graph_snapshot_id: String,
    /// Git SHA at which this revalidation was performed.
    pub current_validated_sha: String,
    /// Evidence state from the revalidation inspection.
    pub evidence_state: crate::evidence::GraphEvidenceState,
    /// Content hash of the file/range at revalidation time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    /// Line range start at revalidation time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_start: Option<u32>,
    /// Line range end at revalidation time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_end: Option<u32>,
    /// Outcome of the revalidation attempt.
    pub outcome: RevalidationOutcome,
    /// Symbol identity fields for symbol bindings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_identity: Option<SymbolIdentity>,
    /// If this revalidation produces a replacement binding, its id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replacement_binding_id: Option<uuid::Uuid>,
    /// Governance result that authorized this revalidation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub governance_id: Option<uuid::Uuid>,
}

/// Outcome of a binding revalidation attempt.
///
/// Distinguishes durable accepted revalidation from failed/non-verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevalidationOutcome {
    /// Binding confirmed valid at the new snapshot (durable accepted).
    Confirmed,
    /// Binding replaced — a new binding supersedes at the new snapshot.
    Replaced,
    /// Revalidation failed — evidence not verified at the new snapshot.
    Failed,
    /// Revalidation could not be performed (stale index, unsupported, etc.).
    Inconclusive,
}

/// Symbol identity fields carried when symbol bindings are involved.
///
/// Provides stable identity for tracking symbol renames and structural changes
/// across snapshots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolIdentity {
    /// Symbol name (unqualified).
    pub symbol_name: String,
    /// Fully qualified name when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qualified_name: Option<String>,
    /// Structural hash for rename-resilient identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structural_hash: Option<String>,
    /// Resolver identity (e.g. tree-sitter parser version + query).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolver_identity: Option<String>,
    /// Parser metadata (language, version).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parser_metadata: Option<String>,
}

/// Payload for an `UntrackSource` event — stops automatic refresh for a Decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UntrackSourcePayload {
    pub decision_id: uuid::Uuid,
    /// Why tracking was stopped. Informational for audit trail.
    pub reason: UntrackReason,
    /// Optional free-form detail for the audit log.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Why automatic refresh tracking was stopped for a Decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UntrackReason {
    /// Signoff was rejected by a reviewer.
    SignoffRejected,
    /// Decision was superseded by a newer Decision.
    Superseded,
    /// Explicit user or policy request to stop tracking.
    ExplicitUntrack,
    /// Future demotion policy (placeholder).
    DemotionPolicy,
}

/// Typed reason for rejecting an ingest request at the governance gate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestRejection {
    pub reason: IngestRejectionReason,
    pub message: String,
}

/// Why an ingest request was rejected before it could become a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngestRejectionReason {
    EmptyEvidence,
    InvalidPayload,
    GovernanceRejected,
    /// Payload contains sensitive data (secret/PHI/PAN). HARD gate — non-bypassable.
    SensitiveData,
    /// Payload contains prompt-injection canary markers. HARD gate.
    InjectionCanary,
    /// Rate limit exceeded — token bucket exhausted.
    RateLimitExceeded,
    /// Payload contains forbidden canonical fields that only governance or the
    /// event store may produce (e.g. `decision_id`, `signoff_state`, `compliance_state`).
    ForbiddenCanonicalField,
    /// Payload shaped as a legacy MCP-only direct payload (not a valid ToolRequest).
    LegacyMcpPayload,
    /// Payload shaped as an external adapter envelope routed through the local
    /// ingest path — must use `ExternalIngestEnvelope` instead.
    ExternalAdapterPayload,
    /// Evidence contains unsafe content (e.g. attempts to inject verified state,
    /// or evidence pointers that claim governance authority).
    UnsafeEvidence,
}

/// Why an external adapter ingest was rejected at the bot-owned gateway.
///
/// External ingest has its own rejection vocabulary because the forbidden
/// operations differ from local ingest: external adapters cannot carry
/// authority context, review commands, signoff approval, compliance
/// resolution, or policy grants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalIngestRejectionReason {
    /// Envelope is missing required fields or has malformed content.
    InvalidPayload,
    /// Evidence list is empty — at least one evidence item is required.
    EmptyEvidence,
    /// Payload contains injection canary markers.
    InjectionCanary,
    /// Payload contains sensitive data (secrets, PII, etc.).
    SensitiveData,
    /// Payload attempts to carry local actor authority context.
    AuthorityInjection,
    /// Payload attempts to carry review commands.
    ReviewCommandInjection,
    /// Payload attempts to carry signoff approval.
    SignoffInjection,
    /// Payload attempts to carry compliance resolution.
    ComplianceInjection,
    /// Payload attempts to carry policy grants.
    PolicyGrantInjection,
    /// Rate limit exceeded.
    RateLimitExceeded,
}

/// Rejection response for the external adapter ingest gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalIngestRejection {
    pub reason: ExternalIngestRejectionReason,
    pub message: String,
}

/// Why a managed provider-backed ingestion was rejected before processing.
///
/// Managed source ingestion checks provider connection and source binding
/// state separately. Each prerequisite has its own typed rejection so
/// callers can surface specific remediation actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedIngestRejectionReason {
    /// The required provider connection is not established.
    /// Remediation: connect the provider via Settings → Connections.
    ProviderNotConnected,
    /// The provider connection exists but requires user action (e.g. token
    /// refresh, re-authorization).
    ProviderActionNeeded,
    /// The source binding for the requested resource is not approved or does
    /// not exist in the project configuration.
    SourceBindingNotReady,
    /// The managed source flow requested is not supported in the current
    /// runtime version. This is a typed rejection, not a panic or empty success.
    UnsupportedManagedFlow,
}

/// Typed rejection for managed provider-backed ingestion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedIngestRejection {
    pub reason: ManagedIngestRejectionReason,
    pub message: String,
    /// Provider identifier from the request.
    pub provider_id: String,
    /// Source binding identifier from the request, if provided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_binding_id: Option<String>,
}

/// Errors from event-store operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum EventStoreError {
    #[error("substrate not reachable: {reason}")]
    Unreachable { reason: String },
    #[error("serialization error: {reason}")]
    Serialization { reason: String },
    #[error("io error: {reason}")]
    Io { reason: String },
    #[error("conflict: {reason}")]
    Conflict { reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraint::{ActorType, Attribution, DecisionReason};

    #[test]
    fn promote_to_canonical_event_type_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&EventType::PromoteToCanonical).unwrap(),
            "\"promote_to_canonical\""
        );
        assert_eq!(
            serde_json::from_str::<EventType>("\"promote_to_canonical\"").unwrap(),
            EventType::PromoteToCanonical
        );
    }

    #[test]
    fn promote_to_canonical_payload_round_trips() {
        let decision_id = Uuid::new_v4();
        let ev = Uuid::new_v4();
        let payload = PromoteToCanonicalPayload {
            decision_id,
            title: "Boundary rule".to_string(),
            statement: "Core owns promotion mechanics".to_string(),
            evidence_refs: vec![ev],
            affected_surfaces: vec!["api/promotion".to_string()],
            reviewer: Attribution {
                actor_id: "rev-1".to_string(),
                actor_type: ActorType::Human,
                display_name: None,
            },
            reason: Some(DecisionReason {
                code: "ok".to_string(),
                detail: None,
            }),
            idempotency_key: "idem-7".to_string(),
        };
        let json = serde_json::to_value(&payload).unwrap();
        let back: PromoteToCanonicalPayload = serde_json::from_value(json).unwrap();
        assert_eq!(back.decision_id, decision_id);
        assert_eq!(back.statement, "Core owns promotion mechanics");
        assert_eq!(back.evidence_refs, vec![ev]);
        assert_eq!(back.affected_surfaces, vec!["api/promotion".to_string()]);
        assert_eq!(back.reviewer.actor_id, "rev-1");
        assert_eq!(back.reviewer.actor_type, ActorType::Human);
        assert_eq!(back.reason.unwrap().code, "ok");
        assert_eq!(back.idempotency_key, "idem-7");
    }

    #[test]
    fn classify_and_supersede_event_types_serialize_snake_case() {
        assert_eq!(
            serde_json::to_string(&EventType::ClassifyConstraint).unwrap(),
            "\"classify_constraint\""
        );
        assert_eq!(
            serde_json::to_string(&EventType::SupersedeConstraint).unwrap(),
            "\"supersede_constraint\""
        );
        assert_eq!(
            serde_json::from_str::<EventType>("\"classify_constraint\"").unwrap(),
            EventType::ClassifyConstraint
        );
        assert_eq!(
            serde_json::from_str::<EventType>("\"supersede_constraint\"").unwrap(),
            EventType::SupersedeConstraint
        );
    }

    #[test]
    fn classify_constraint_payload_round_trips() {
        use crate::catalog::{ConstraintScope, ConstraintType};
        let payload = ClassifyConstraintPayload {
            idempotency_key: "idem-9".to_string(),
            constraint_type: ConstraintType::Security,
            scope: ConstraintScope::Service,
            attribution: Attribution {
                actor_id: "classifier".to_string(),
                actor_type: ActorType::Agent,
                display_name: None,
            },
            affected_surfaces: Some(vec!["api/auth".to_string()]),
        };
        let back: ClassifyConstraintPayload =
            serde_json::from_value(serde_json::to_value(&payload).unwrap()).unwrap();
        assert_eq!(back.idempotency_key, "idem-9");
        assert_eq!(back.constraint_type, ConstraintType::Security);
        assert_eq!(back.scope, ConstraintScope::Service);
        assert_eq!(back.attribution.actor_type, ActorType::Agent);
        assert_eq!(back.affected_surfaces, Some(vec!["api/auth".to_string()]));
    }

    #[test]
    fn supersede_constraint_payload_round_trips() {
        let payload = SupersedeConstraintPayload {
            current_idempotency_key: "old".to_string(),
            replacement_idempotency_key: "new".to_string(),
        };
        let back: SupersedeConstraintPayload =
            serde_json::from_value(serde_json::to_value(&payload).unwrap()).unwrap();
        assert_eq!(back.current_idempotency_key, "old");
        assert_eq!(back.replacement_idempotency_key, "new");
    }

    #[test]
    fn drift_event_types_serialize_snake_case() {
        assert_eq!(
            serde_json::to_string(&EventType::RaiseContradiction).unwrap(),
            "\"raise_contradiction\""
        );
        assert_eq!(
            serde_json::to_string(&EventType::TransitionFinding).unwrap(),
            "\"transition_finding\""
        );
        assert_eq!(
            serde_json::from_str::<EventType>("\"raise_contradiction\"").unwrap(),
            EventType::RaiseContradiction
        );
        assert_eq!(
            serde_json::from_str::<EventType>("\"transition_finding\"").unwrap(),
            EventType::TransitionFinding
        );
    }

    #[test]
    fn raise_contradiction_payload_round_trips() {
        use crate::constraint::{ActorType, Attribution};
        use crate::drift::{ContradictionReport, FindingSeverity, FindingStatus};
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let now = chrono::Utc::now();
        let payload = RaiseContradictionPayload {
            report: ContradictionReport {
                id: Uuid::new_v4(),
                summary: "overlap".to_string(),
                severity: FindingSeverity::High,
                status: FindingStatus::Open,
                related_constraint_ids: vec![a, b],
                evidence_refs: vec![Uuid::new_v4()],
                suggested_resolution: None,
                owner_recommendation: Some(Attribution {
                    actor_id: "owner".to_string(),
                    actor_type: ActorType::Human,
                    display_name: None,
                }),
                detected_at: now,
                updated_at: now,
            },
        };
        let back: RaiseContradictionPayload =
            serde_json::from_value(serde_json::to_value(&payload).unwrap()).unwrap();
        assert_eq!(back.report.related_constraint_ids, vec![a, b]);
        assert_eq!(back.report.status, FindingStatus::Open);
        assert_eq!(back.report.severity, FindingSeverity::High);
    }

    #[test]
    fn raise_correction_finding_event_and_payload_round_trip() {
        use crate::constraint::LifecycleReviewState;
        use crate::drift::FindingSeverity;
        use crate::grounding::{CodeRegionRef, CorrectionFinding, SuggestedAction};
        assert_eq!(
            serde_json::to_string(&EventType::RaiseCorrectionFinding).unwrap(),
            "\"raise_correction_finding\""
        );
        let did = Uuid::new_v4();
        let payload = RaiseCorrectionFindingPayload {
            finding: CorrectionFinding {
                id: Uuid::new_v4(),
                summary: "region changed".to_string(),
                code_region: CodeRegionRef {
                    provider: "github".to_string(),
                    repo: Some("o/r".to_string()),
                    path: "src/lib.rs".to_string(),
                    start_line: Some(10),
                    end_line: Some(20),
                    git_ref: Some("main".to_string()),
                    url: None,
                },
                source_doc: None,
                pr: None,
                related_decision_ids: vec![did],
                related_constraint_ids: Vec::new(),
                evidence_refs: Vec::new(),
                confidence_bps: 9000,
                severity: FindingSeverity::High,
                review_state: LifecycleReviewState::Proposed,
                suggested_action: SuggestedAction::UpdateSourceDoc,
                reported_by: None,
                detected_at: chrono::Utc::now(),
            },
        };
        let back: RaiseCorrectionFindingPayload =
            serde_json::from_value(serde_json::to_value(&payload).unwrap()).unwrap();
        assert_eq!(back.finding.related_decision_ids, vec![did]);
        assert_eq!(back.finding.severity, FindingSeverity::High);
        assert_eq!(
            back.finding.suggested_action,
            SuggestedAction::UpdateSourceDoc
        );
    }

    #[test]
    fn record_semantic_mapping_event_and_payload_round_trip() {
        use crate::constraint::{ActorType, Attribution, LifecycleReviewState};
        use crate::semantic::{Provenance, SemanticMapping, SourcePhraseRef};
        assert_eq!(
            serde_json::to_string(&EventType::RecordSemanticMapping).unwrap(),
            "\"record_semantic_mapping\""
        );
        let constraint_id = Uuid::new_v4();
        let payload = RecordSemanticMappingPayload {
            mapping: SemanticMapping {
                id: Uuid::new_v4(),
                source_phrase: SourcePhraseRef {
                    text: "encrypt PII".to_string(),
                    source_id: None,
                    locator: None,
                },
                derived_constraint_id: constraint_id,
                provenance: Provenance {
                    captured_at: chrono::Utc::now(),
                    captured_by: Attribution {
                        actor_id: "m".to_string(),
                        actor_type: ActorType::Agent,
                        display_name: None,
                    },
                    capture_method: "rule-pack".to_string(),
                    pipeline_version: None,
                    source_hash: None,
                },
                review_state: LifecycleReviewState::Accepted,
                confidence_bps: 9000,
                examples: Vec::new(),
            },
        };
        let back: RecordSemanticMappingPayload =
            serde_json::from_value(serde_json::to_value(&payload).unwrap()).unwrap();
        assert_eq!(back.mapping.derived_constraint_id, constraint_id);
        assert_eq!(back.mapping.review_state, LifecycleReviewState::Accepted);
        assert_eq!(back.mapping.confidence_bps, 9000);
    }

    #[test]
    fn transition_finding_payload_round_trips() {
        use crate::constraint::{ActorType, Attribution};
        use crate::drift::{FindingStatus, ReviewTransitionRequest};
        let rid = Uuid::new_v4();
        let payload = TransitionFindingPayload {
            request: ReviewTransitionRequest {
                report_id: rid,
                to_status: FindingStatus::Acknowledged,
                actor: Attribution {
                    actor_id: "r".to_string(),
                    actor_type: ActorType::Human,
                    display_name: None,
                },
                note: Some("ack".to_string()),
                at: chrono::Utc::now(),
            },
        };
        let back: TransitionFindingPayload =
            serde_json::from_value(serde_json::to_value(&payload).unwrap()).unwrap();
        assert_eq!(back.request.report_id, rid);
        assert_eq!(back.request.to_status, FindingStatus::Acknowledged);
    }
}
