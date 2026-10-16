//! Shared application state for the gateway.

use crate::evidence_repository::{
    EvidenceRepository, FileEvidenceRepository, MemoryEvidenceRepository,
};
use crate::live_projection_work::LiveProjectionWorkStore;
use bicameral_api::candidate::{Decision, DecisionCandidate};
use bicameral_api::event_store::{
    EventStoreAdapter, EventStoreEntry, EventStoreError, SubstrateCapability, SubstrateHealth,
    SubstrateKind,
};
use bicameral_api::evidence::{BindingEvidence, EvidenceReference, SourceEvidence};
use bicameral_api::projection::ProjectionState;
use bicameral_api::reasoning_projection::{DeterministicProjectionProvider, ProjectionProvider};
use bicameral_api::review::ReviewState;
use bicameral_api::source::{Source as SourceRecord, SourceSnapshot as SnapshotRecord};
use bicameral_api::tool_request::{ToolRequest, ToolResponse};
use bicameral_audit::AuditStore;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use uuid::Uuid;

/// Read-model maps rebuilt from event-store replay.
///
/// Lets the gateway refresh its in-memory Ledger projection from canonical
/// events after a `/v2/tool-requests` write without depending on the runtime
/// crate's replay internals. The event store stays the single source of truth;
/// this is a disposable projection derived from it.
#[derive(Default)]
pub struct ReplayedProjection {
    pub sources: HashMap<String, SourceRecord>,
    pub snapshots: HashMap<String, SnapshotRecord>,
    pub evidence: HashMap<Uuid, SourceEvidence>,
    pub candidates: HashMap<Uuid, DecisionCandidate>,
    pub decisions: HashMap<Uuid, Decision>,
    pub bindings: HashMap<Uuid, Vec<BindingEvidence>>,
    pub binding_content_hashes: HashMap<Uuid, Option<String>>,
}

/// Trait for dispatching `ToolRequest`s to the runtime.
///
/// The gateway does not directly depend on `bicameral-runtime` (cyclic dep).
/// Instead, the daemon injects a concrete implementation that calls
/// `dispatch_with_workspace` from the runtime crate. The same bridge serves
/// the v2 ToolRequest HTTP surface (`/v2/capabilities`, `/v2/tool-requests`).
pub trait RuntimeDispatch: Send + Sync {
    fn dispatch(&self, request: &ToolRequest, workspace_root: Option<&Path>) -> ToolResponse;

    /// Pre-serialized capability report returned by `GET /v2/capabilities`.
    fn capabilities(&self) -> serde_json::Value;

    /// Operator-local capability readiness projection.
    fn capability_readiness(
        &self,
    ) -> Result<Vec<bicameral_api::capability_readiness::DaemonCapabilityReadinessV1>, String> {
        Ok(Vec::new())
    }

    /// Submit verifier-produced local MCP setup evidence to the daemon-owned
    /// readiness reducer. Remote/browser routes must never call this method.
    fn submit_mcp_readiness(
        &self,
        _observation: bicameral_api::capability_readiness::McpReadinessObservation,
    ) -> Result<bicameral_api::capability_readiness::DaemonCapabilityReadinessV1, String> {
        Err("unsupported_capability: MCP readiness reducer unavailable".into())
    }

    /// Replay the daemon event store into a fresh read-model projection.
    ///
    /// Returns `None` for implementations that do not materialize canonical
    /// state (e.g. graph-only test doubles). The gateway uses this to refresh
    /// its read-only Ledger projection after a canonical `/v2/tool-requests`
    /// write, so a materialized Decision renders without waiting for a restart.
    fn replay_projection(&self) -> Option<ReplayedProjection> {
        None
    }
}

/// Simple token-bucket rate limiter for the ingest endpoint.
///
/// Uses atomic operations for lock-free check-and-refill.
/// Tokens refill at `refill_per_sec` per second up to `burst`.
pub struct TokenBucket {
    /// Available tokens × 1000 (fixed-point to avoid floats in atomics).
    tokens_milli: AtomicU64,
    last_refill_ms: AtomicU64,
    burst_milli: u64,
    refill_per_ms: u64,
}

impl TokenBucket {
    pub fn new(burst: u32, refill_per_sec: u32) -> Self {
        let burst_milli = u64::from(burst) * 1000;
        Self {
            tokens_milli: AtomicU64::new(burst_milli),
            last_refill_ms: AtomicU64::new(Self::now_ms()),
            burst_milli,
            refill_per_ms: u64::from(refill_per_sec),
        }
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    /// Try to consume one token. Returns `true` if allowed, `false` if rate-limited.
    pub fn check(&self) -> bool {
        let now = Self::now_ms();
        let last = self.last_refill_ms.load(Ordering::Relaxed);
        let elapsed = now.saturating_sub(last);

        if elapsed > 0 {
            let refill = elapsed * self.refill_per_ms;
            let current = self.tokens_milli.load(Ordering::Relaxed);
            let new_tokens = (current + refill).min(self.burst_milli);
            self.tokens_milli.store(new_tokens, Ordering::Relaxed);
            self.last_refill_ms.store(now, Ordering::Relaxed);
        }

        let current = self.tokens_milli.load(Ordering::Relaxed);
        if current >= 1000 {
            self.tokens_milli.fetch_sub(1000, Ordering::Relaxed);
            true
        } else {
            false
        }
    }
}

/// In-memory event store for the gateway process (fallback when no config is provided).
///
/// Uses a sync Mutex (not tokio RwLock) because EventStoreAdapter is not async
/// and the critical section is tiny (Vec push / clone).
pub struct GatewayEventStore {
    entries: Mutex<Vec<EventStoreEntry>>,
}

impl Default for GatewayEventStore {
    fn default() -> Self {
        Self::new()
    }
}

impl GatewayEventStore {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
        }
    }
}

impl EventStoreAdapter for GatewayEventStore {
    fn append(&self, entry: EventStoreEntry) -> Result<(), EventStoreError> {
        self.entries
            .lock()
            .map_err(|e| EventStoreError::Io {
                reason: e.to_string(),
            })?
            .push(entry);
        Ok(())
    }

    fn replay(&self) -> Result<Vec<EventStoreEntry>, EventStoreError> {
        Ok(self
            .entries
            .lock()
            .map_err(|e| EventStoreError::Io {
                reason: e.to_string(),
            })?
            .clone())
    }

    fn substrate_kind(&self) -> SubstrateKind {
        SubstrateKind::Memory
    }

    fn health_check(&self) -> Result<SubstrateHealth, EventStoreError> {
        let count = self.entries.lock().map(|e| e.len()).unwrap_or(0);
        Ok(SubstrateHealth {
            kind: SubstrateKind::Memory,
            reachable: true,
            last_synced: None,
            entry_count: count,
            freshness: None,
            warnings: Vec::new(),
            capabilities: self.capabilities(),
        })
    }

    fn capabilities(&self) -> BTreeSet<SubstrateCapability> {
        BTreeSet::new()
    }
}

/// Ingest rate limit: requests per second.
const INGEST_RATE_PER_SECOND: u32 = 10;

/// Ingest rate limit: burst capacity.
const INGEST_BURST_SIZE: u32 = 20;

/// Bounded lifetime of a transient Google OAuth auth session (#1133).
///
/// An authorization attempt is a single short-lived browser round trip;
/// PKCE/state material must not stay eligible for completion beyond this
/// window. Expired sessions are rejected before token exchange, secret
/// writes, or `ConnectionRegistry` mutation, and the native GET loopback
/// never resolves an expired state token.
pub const GOOGLE_AUTH_SESSION_TTL: std::time::Duration = std::time::Duration::from_secs(600);

/// Metadata for a pending auth session.
#[derive(Debug, Clone)]
pub struct AuthSession {
    pub provider: String,
    pub created_at: std::time::Instant,
    pub state_token: String,
    /// Ephemeral RFC 7636 verifier for this provider-auth attempt.
    /// Never persisted, logged, returned to the browser, or sent to Cloud.
    pub pkce_verifier: Option<String>,
    /// Exact Google application config bound when the authorization URL was created.
    /// Daemon-memory only; never returned to the browser or Cloud.
    pub google_oauth_config: Option<bicameral_config::GoogleOAuthConfig>,
}

impl AuthSession {
    /// Whether this session has outlived [`GOOGLE_AUTH_SESSION_TTL`].
    ///
    /// Monotonic-clock based (`Instant`), so wall-clock adjustments cannot
    /// extend or shorten an attempt; a daemon restart clears the map anyway.
    pub fn is_expired(&self, now: std::time::Instant) -> bool {
        now.saturating_duration_since(self.created_at) >= GOOGLE_AUTH_SESSION_TTL
    }
}

/// Active hosted bridge pairing session.
///
/// Tracks a paired browser-origin session with the local daemon. Unpaired
/// or expired sessions are rejected before any projection reads or governed
/// action requests.
#[derive(Debug, Clone)]
pub struct HostedBridgePairingSession {
    pub session_token: String,
    /// Durable relationship that minted this ephemeral session, when present.
    pub pairing_id: Option<String>,
    pub origin: String,
    pub cloud_account_id: String,
    pub paired_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub revoked: bool,
}

/// Default lifetime of a pending pairing request before it is treated as
/// expired and cleaned up (#790). Short-lived by design: a pending request is
/// only a confirmation prompt awaiting an explicit local decision.
pub const PENDING_PAIRING_TTL_SECONDS: i64 = 300;

/// Internal decision state of a pending pairing request (#790).
///
/// Transient daemon-memory request state only — NOT canonical Decision /
/// project / workspace / provider / source / signoff / event-store state, and
/// never persisted to Cloud.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingPairingDecision {
    /// Awaiting an explicit local approve/deny decision.
    Pending,
    /// Locally approved; a session has been minted and bound.
    Approved,
    /// Locally denied. Terminal.
    Denied,
}

/// A transient, daemon-memory pending pairing request created by
/// `POST /hosted-bridge/pair` and resolved by an explicit local decision (#790).
///
/// Retains only the minimum metadata needed to render the local confirmation
/// surface, bind approval + status retrieval to the initiator, and expire
/// abandoned requests. It never persists to Cloud and is not canonical state.
#[derive(Debug, Clone)]
pub struct PendingPairingRecord {
    /// Opaque handle identifying this request (map key).
    pub request_handle: String,
    /// Opaque proof the initiating browser presents to poll status.
    pub poll_proof: String,
    /// Same-origin nonce the local confirmation surface presents to decide.
    pub local_confirmation_nonce: String,
    /// SHA-256 digest of the normalized browser-displayed pairing code. The
    /// plaintext code is returned once to the initiating browser and is never
    /// retained in daemon state.
    pub pairing_code_digest: [u8; 32],
    /// Requesting Cloud origin (bound; approvals/polls must match).
    pub origin: String,
    /// Requesting Cloud account (bound; approvals/polls must match).
    pub cloud_account_id: String,
    /// Cloud team id for enrollment (row 12). Required to mint/enroll;
    /// absent → enroll fails closed with a clear error.
    pub team_id: Option<String>,
    /// Short-lived Cloud user bearer for the one enroll POST. Held only until
    /// local approval extracts it for the bounded Cloud call (or until
    /// deny/expiry clears it). Never part of retry-safe approved state, never
    /// logged, and never returned on bridge responses.
    pub cloud_access_token: Option<String>,
    /// When the request was created.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// When the pending request expires. Once approved, the retry-safe status
    /// exchange remains eligible through `session_expires_at` instead (#1109).
    pub expires_at: chrono::DateTime<chrono::Utc>,
    /// Session TTL to apply when the request is approved.
    pub requested_ttl_seconds: u64,
    /// Current internal decision state.
    pub decision: PendingPairingDecision,
    /// Bridge session token — populated ONLY after local approval. Never
    /// serialized directly; released to the initiator only via the status
    /// exchange.
    pub session_token: Option<String>,
    /// Session expiry recorded at approval time.
    pub session_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Durable opaque pairing_id minted at LOCAL_APPROVED.
    pub pairing_id: Option<String>,
}

impl PendingPairingRecord {
    /// Whether this request can no longer participate in the status exchange.
    ///
    /// Pending/denied requests use the short confirmation TTL. An approved
    /// request remains retry-safe only through the daemon-authored live session
    /// expiry, allowing a bounded cross-visit restore proof to re-acquire the
    /// transient token. Missing approved-session expiry fails conservatively to
    /// the original pending expiry.
    pub fn is_expired(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        let effective_expiry = match self.decision {
            PendingPairingDecision::Approved => self.session_expires_at.unwrap_or(self.expires_at),
            PendingPairingDecision::Pending | PendingPairingDecision::Denied => self.expires_at,
        };
        now >= effective_expiry
    }
}

/// Internal representation of a locally registered project.
///
/// Holds the full internal state including sensitive fields (filesystem paths,
/// provider configuration) that must NEVER be exposed through the hosted bridge.
/// Use [`project_readiness_projection`] to convert to safe
/// [`bicameral_api::hosted_bridge::ProjectReadinessEntry`] values.
#[derive(Debug, Clone)]
pub struct LocalProjectRegistration {
    /// Display name exposed through the bridge projection.
    pub name: String,

    /// Whether the project's local event store is accessible and healthy.
    pub event_store_healthy: bool,

    /// Whether the project has at least one workspace binding.
    /// When `false`, code-grounding is not yet ready — this is a readiness
    /// state, not a pairing failure.
    pub has_workspace_binding: bool,

    /// Local filesystem path to the project workspace. NEVER exposed via bridge.
    pub workspace_path: Option<PathBuf>,

    /// Provider-specific configuration (may contain secrets). NEVER exposed.
    pub provider_config: Option<serde_json::Value>,

    /// Setup state summary. When `has_workspace_binding` is false, this should
    /// be `"code_grounding_not_ready"` rather than any error implying pairing
    /// failure.
    pub setup_state: String,
}

/// Hosted-safe replay of canonical Product membership for one paired account.
///
/// The projection carries only Product identity and whether daemon-owned setup
/// has materialized a local registration. It never exposes roles, actors,
/// event references, provider configuration, or local paths.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct HostedProductMembershipProjection {
    pub product_id: String,
    pub setup_incomplete: bool,
}

#[derive(serde::Deserialize)]
struct CreatedProductMembershipIdentity {
    product_id: String,
    creator_cloud_account_id: String,
}

#[derive(serde::Deserialize)]
struct JoinedProductMembershipIdentity {
    product_id: String,
    cloud_account_id: String,
}

/// Replay canonical creator/join membership for the validated bridge account.
///
/// Malformed unrelated events are ignored, matching the runtime replay posture.
/// Registrations affect setup readiness only; they never grant membership.
pub fn product_membership_projection(
    entries: &[EventStoreEntry],
    cloud_account_id: &str,
    registrations: &[LocalProjectRegistration],
) -> Vec<HostedProductMembershipProjection> {
    let mut product_ids = BTreeSet::new();
    for entry in entries {
        match entry.event_type {
            bicameral_api::event_store::EventType::CreateProduct => {
                let Ok(identity) = serde_json::from_value::<CreatedProductMembershipIdentity>(
                    entry.payload.clone(),
                ) else {
                    continue;
                };
                if identity.creator_cloud_account_id == cloud_account_id {
                    product_ids.insert(identity.product_id);
                }
            }
            bicameral_api::event_store::EventType::ProductMemberAdded => {
                let Ok(identity) = serde_json::from_value::<JoinedProductMembershipIdentity>(
                    entry.payload.clone(),
                ) else {
                    continue;
                };
                if identity.cloud_account_id == cloud_account_id {
                    product_ids.insert(identity.product_id);
                }
            }
            _ => {}
        }
    }

    let registered_ids = registrations
        .iter()
        .map(|registration| registration.name.as_str())
        .collect::<BTreeSet<_>>();
    product_ids
        .into_iter()
        .map(|product_id| HostedProductMembershipProjection {
            setup_incomplete: !registered_ids.contains(product_id.as_str()),
            product_id,
        })
        .collect()
}

/// Convert internal project registrations into safe bridge-facing projection
/// entries. Strips all sensitive fields (filesystem paths, provider secrets,
/// SourceEvidence bodies, Decision Ledger event payloads).
pub fn project_readiness_projection(
    registrations: &[LocalProjectRegistration],
) -> Vec<bicameral_api::hosted_bridge::ProjectReadinessEntry> {
    registrations
        .iter()
        .map(|reg| bicameral_api::hosted_bridge::ProjectReadinessEntry {
            name: reg.name.clone(),
            event_store_healthy: reg.event_store_healthy,
            has_workspace_binding: reg.has_workspace_binding,
            setup_state: reg.setup_state.clone(),
        })
        .collect()
}

/// Convert internal project registrations into safe workspace metadata (#614).
///
/// Strips all sensitive fields and adds aggregate counts from the gateway
/// materialized state. No local paths, provider secrets, SourceEvidence
/// bodies, Decision Ledger event payloads, or authority-bearing fields.
pub fn safe_workspace_metadata_projection(
    registrations: &[LocalProjectRegistration],
    decision_count: u32,
    candidate_count: u32,
) -> Vec<bicameral_api::hosted_bridge::SafeWorkspaceMetadata> {
    registrations
        .iter()
        .map(|reg| bicameral_api::hosted_bridge::SafeWorkspaceMetadata {
            project_name: reg.name.clone(),
            event_store_healthy: reg.event_store_healthy,
            has_workspace_binding: reg.has_workspace_binding,
            setup_state: reg.setup_state.clone(),
            code_grounding_ready: reg.has_workspace_binding && reg.event_store_healthy,
            decision_count,
            candidate_count,
            event_store_kind: None,
        })
        .collect()
}

/// Shared state accessible by all gateway route handlers.
#[derive(Clone)]
pub struct AppState {
    pub candidates: Arc<RwLock<HashMap<Uuid, DecisionCandidate>>>,
    pub decisions: Arc<RwLock<HashMap<Uuid, Decision>>>,
    pub reviews: Arc<RwLock<HashMap<Uuid, ReviewState>>>,
    pub sources: Arc<RwLock<HashMap<String, SourceRecord>>>,
    pub snapshots: Arc<RwLock<HashMap<String, SnapshotRecord>>>,
    pub evidence: Arc<RwLock<HashMap<Uuid, SourceEvidence>>>,
    /// Typed evidence references (MCP sessions, tool calls, command output, etc.).
    pub evidence_references: Arc<RwLock<HashMap<Uuid, EvidenceReference>>>,
    /// Binding evidence grouped by decision ID.
    pub bindings: Arc<RwLock<HashMap<Uuid, Vec<BindingEvidence>>>>,
    /// Content hashes recorded at bind time, keyed by `BindingEvidence.id`.
    pub binding_content_hashes: Arc<RwLock<HashMap<Uuid, Option<String>>>>,
    pub event_store: Arc<dyn EventStoreAdapter>,
    /// Daemon-local screened evidence body repository. This is separate
    /// from the canonical Product event store and never exposed to Cloud.
    pub evidence_repository: Arc<dyn EvidenceRepository>,
    /// Daemon-local retry, projection-outcome, and candidate working state.
    /// Never canonical and never reconstructed by Product event replay.
    pub projection_work_store: Option<Arc<LiveProjectionWorkStore>>,
    /// Daemon-owned reasoning provider used by the live projection path.
    /// Defaults to the deterministic alpha evaluator and remains non-canonical.
    pub projection_provider: Arc<dyn ProjectionProvider>,
    pub audit: Arc<RwLock<AuditStore>>,
    /// Token-bucket rate limiter for the ingest endpoint.
    pub ingest_rate_limiter: Arc<TokenBucket>,
    /// Optional V1 reviewable-context projection state.
    ///
    /// When populated, dashboard Ledger View reads from the projection
    /// instead of the in-memory maps. When `None`, the gateway falls back
    /// to the existing in-memory read path. The projection is rebuildable
    /// from event-store replay and is always disposable.
    pub projection: Arc<RwLock<Option<ProjectionState>>>,
    /// Workspace root directory (where `.bicameral/` lives or will be created).
    /// `None` when the gateway starts in setup mode without an existing workspace.
    pub workspace_root: Arc<RwLock<Option<PathBuf>>>,
    /// Pending OAuth auth sessions keyed by session ID.
    pub auth_sessions: Arc<RwLock<HashMap<String, AuthSession>>>,
    /// Secret material store (tokens, keys). Never logged or returned in API
    /// responses.
    pub secrets_store: Arc<dyn bicameral_config::SecretsStore>,
    /// Legacy/operator Google OAuth configuration. Production native-client
    /// startup is Cloud-first; this is used only under an explicit override.
    pub google_oauth_config: Option<bicameral_config::GoogleOAuthConfig>,
    /// Bounded daemon-memory cache of validated non-secret Cloud app config.
    pub google_app_config_cache: crate::google_app_config::GoogleAppConfigCache,
    /// Explicit recovery/dev override allowing legacy env OAuth config.
    pub google_oauth_allow_env_override: bool,
    /// Test-only Google token-endpoint override (#1132). Deterministic tests
    /// point the REAL exchange transport at a local stub server; the field is
    /// compiled only for test builds, so no production configuration, env
    /// variable, or authorization-code content can select a synthetic
    /// exchange. Production always posts to the canonical Google endpoint.
    #[cfg(test)]
    pub google_token_endpoint_override: Option<String>,
    /// Test-only Cloud app-config origin override (#1135): fresh-install
    /// tests point the REAL Cloud-first resolver at a local stub server.
    /// Compiled only for test builds; production always resolves the
    /// governed Cloud origin.
    #[cfg(test)]
    pub google_cloud_origin_override: Option<String>,
    /// Test-only Drive "about" endpoint override (#1279) for the
    /// live-credential-revocation check; with none set, the check skips
    /// the live call. Compiled only for test builds; production always
    /// targets the canonical Drive endpoint.
    #[cfg(test)]
    pub google_drive_about_endpoint_override: Option<String>,
    /// In-memory store for active device-flow auth sessions.
    pub device_flow_store: crate::github_device_flow::DeviceFlowStore,
    /// Daemon-memory Google resource-selection sessions (#1085/#1135),
    /// keyed by opaque selection-session id. Creation-attempt context
    /// only: never persisted, never ConnectionRegistry state, never
    /// canonical.
    pub resource_selection_sessions:
        Arc<RwLock<HashMap<String, crate::google_resource_selection::ResourceSelectionSession>>>,
    /// Injected Drive `files.get` transport used to validate a selected
    /// folder (#1085). Production uses the live REST probe; tests inject
    /// deterministic doubles. Only daemon code touches it; access tokens
    /// pass through transiently and are never stored by the probe.
    pub drive_resource_probe: Arc<dyn crate::google_resource_selection::DriveResourceProbe>,
    /// HTTP client shared across connection handlers.
    pub http_client: reqwest::Client,
    /// Configurable GitHub API endpoints (overridable for testing).
    pub github_endpoints: crate::github_device_flow::GitHubEndpoints,
    /// Optional runtime dispatch for graph/locator commands and the v2
    /// ToolRequest HTTP surface.
    ///
    /// When set by the daemon, gateway routes can delegate `ToolRequest`
    /// dispatch to the runtime without a direct crate dependency.
    /// When `None`, runtime-delegated routes return typed `unsupported`.
    pub runtime_dispatch: Option<Arc<dyn RuntimeDispatch>>,
    /// Hosted bridge pairing sessions keyed by session token.
    ///
    /// Tracks active browser-origin pairing sessions. Unpaired or expired
    /// sessions are rejected by the hosted bridge gateway guard before any
    /// projection reads or governed mutations.
    pub hosted_bridge_sessions: Arc<RwLock<HashMap<String, HostedBridgePairingSession>>>,

    /// Transient pending pairing requests keyed by opaque request handle (#790).
    ///
    /// Daemon-memory only: created by `POST /hosted-bridge/pair`, resolved by an
    /// explicit local approve/deny, and cleaned up on expiry. Never persisted to
    /// Cloud and never canonical governance state.
    pub hosted_bridge_pending: Arc<RwLock<HashMap<String, PendingPairingRecord>>>,

    /// Operator-local durable pairing_id relationship store (Alpha file map).
    /// Survives process restart; not Cloud multi-instance durability.
    pub durable_pairing_store: Arc<crate::durable_pairing_store::DurablePairingStore>,

    /// Alpha TOFU daemon instance identity (stable across restarts).
    pub daemon_instance_id: String,

    /// Global limiter for local pairing-code lookup attempts. Code possession
    /// is not authority, but bounded lookup prevents brute-force enumeration
    /// of short human-entered codes.
    pub pairing_code_rate_limiter: Arc<TokenBucket>,

    /// Locally registered projects visible through the hosted bridge.
    ///
    /// Contains internal representations including sensitive paths and provider
    /// config. Bridge endpoints MUST use [`project_readiness_projection`] to
    /// strip sensitive fields before returning data to the remote origin.
    pub project_registrations: Arc<std::sync::RwLock<Vec<LocalProjectRegistration>>>,

    /// Operator-configured custom trusted origins for hosted bridge pairing.
    ///
    /// These supplement `DEFAULT_TRUSTED_ORIGINS` from `bicameral_api::hosted_bridge`.
    /// Origins must be exact scheme+host+port strings (e.g. "https://my.corp.com").
    pub custom_trusted_origins: Arc<RwLock<Vec<String>>>,

    /// Aggregate telemetry for hosted-dashboard protocol compatibility checks.
    ///
    /// Records only protocol-version integers and outcome counts — never a
    /// workspace, ledger, source, evidence, or local path (#711 / ADR-0025).
    pub hosted_dashboard_compat_telemetry:
        Arc<RwLock<bicameral_api::hosted_bridge::HostedDashboardCompatibilityTelemetry>>,
}

impl AppState {
    /// Create with an in-memory event store (backward-compatible default).
    pub fn new(audit: AuditStore) -> Self {
        Self::with_adapter(Arc::new(GatewayEventStore::new()), audit)
    }

    /// Create with a caller-supplied event-store adapter.
    pub fn with_adapter(adapter: Arc<dyn EventStoreAdapter>, audit: AuditStore) -> Self {
        Self {
            candidates: Arc::new(RwLock::new(HashMap::new())),
            decisions: Arc::new(RwLock::new(HashMap::new())),
            reviews: Arc::new(RwLock::new(HashMap::new())),
            sources: Arc::new(RwLock::new(HashMap::new())),
            snapshots: Arc::new(RwLock::new(HashMap::new())),
            evidence: Arc::new(RwLock::new(HashMap::new())),
            evidence_references: Arc::new(RwLock::new(HashMap::new())),
            bindings: Arc::new(RwLock::new(HashMap::new())),
            binding_content_hashes: Arc::new(RwLock::new(HashMap::new())),
            event_store: adapter,
            evidence_repository: Arc::new(MemoryEvidenceRepository::new()),
            projection_work_store: None,
            projection_provider: Arc::new(DeterministicProjectionProvider),
            audit: Arc::new(RwLock::new(audit)),
            ingest_rate_limiter: Arc::new(TokenBucket::new(
                INGEST_BURST_SIZE,
                INGEST_RATE_PER_SECOND,
            )),
            projection: Arc::new(RwLock::new(None)),
            workspace_root: Arc::new(RwLock::new(None)),
            auth_sessions: Arc::new(RwLock::new(HashMap::new())),
            secrets_store: Arc::new(bicameral_config::InMemorySecretsStore::new()),
            google_oauth_config: bicameral_config::GoogleOAuthConfig::from_env(),
            #[cfg(test)]
            google_token_endpoint_override: None,
            #[cfg(test)]
            google_cloud_origin_override: None,
            #[cfg(test)]
            google_drive_about_endpoint_override: None,
            google_app_config_cache: crate::google_app_config::new_google_app_config_cache(),
            google_oauth_allow_env_override: crate::google_app_config::legacy_env_override_enabled(
            ),
            device_flow_store: crate::github_device_flow::DeviceFlowStore::new(),
            resource_selection_sessions: Arc::new(RwLock::new(HashMap::new())),
            drive_resource_probe: Arc::new(
                crate::google_resource_selection::HttpDriveResourceProbe::new(
                    reqwest::Client::new(),
                ),
            ),
            http_client: reqwest::Client::new(),
            github_endpoints: crate::github_device_flow::GitHubEndpoints::default(),
            runtime_dispatch: None,
            hosted_bridge_sessions: Arc::new(RwLock::new(HashMap::new())),
            hosted_bridge_pending: Arc::new(RwLock::new(HashMap::new())),
            durable_pairing_store: Arc::new(
                crate::durable_pairing_store::DurablePairingStore::in_memory(),
            ),
            daemon_instance_id: format!("daemon-{}", uuid::Uuid::new_v4()),
            pairing_code_rate_limiter: Arc::new(TokenBucket::new(5, 1)),
            project_registrations: Arc::new(std::sync::RwLock::new(Vec::new())),
            // Seed from env so local Vite smoke can opt in without shipping
            // loopback origins in DEFAULT_TRUSTED_ORIGINS.
            custom_trusted_origins: Arc::new(RwLock::new(
                bicameral_api::hosted_bridge::extra_trusted_origins_from_env(),
            )),
            hosted_dashboard_compat_telemetry: Arc::new(RwLock::new(Default::default())),
        }
    }

    /// Replace the durable pairing store (production injects the operator-home file).
    pub fn with_durable_pairing_store(
        mut self,
        store: Arc<crate::durable_pairing_store::DurablePairingStore>,
    ) -> Self {
        self.durable_pairing_store = store;
        self
    }

    /// Override the Alpha TOFU daemon instance identity (tests / production load).
    pub fn with_daemon_instance_id(mut self, daemon_instance_id: impl Into<String>) -> Self {
        self.daemon_instance_id = daemon_instance_id.into();
        self
    }

    /// Replace the secrets store.
    ///
    /// Production daemon construction must inject the OS-backed store
    /// here; the in-memory default that `new` installs exists only so
    /// deterministic tests get volatile storage without touching the OS
    /// keyring. There is no runtime fallback between the two: whichever
    /// store is installed at construction is the credential authority for
    /// the process lifetime.
    /// Replace the Drive resource probe (#1085). Tests inject
    /// deterministic doubles; production keeps the live REST probe.
    pub fn with_drive_resource_probe(
        mut self,
        probe: Arc<dyn crate::google_resource_selection::DriveResourceProbe>,
    ) -> Self {
        self.drive_resource_probe = probe;
        self
    }

    pub fn with_secrets_store(mut self, store: Arc<dyn bicameral_config::SecretsStore>) -> Self {
        self.secrets_store = store;
        self
    }

    /// Replace the default in-memory evidence repository.
    pub fn with_evidence_repository(mut self, repository: Arc<dyn EvidenceRepository>) -> Self {
        self.evidence_repository = repository;
        self
    }

    /// Configure daemon-local projection and candidate working state.
    pub fn with_projection_work_store(mut self, store: Arc<LiveProjectionWorkStore>) -> Self {
        self.projection_work_store = Some(store);
        self
    }

    /// Configure the daemon-owned projection provider.
    ///
    /// This is an execution seam only. Provider output remains proposal state and
    /// cannot write canonical Product history directly.
    pub fn with_projection_provider(mut self, provider: Arc<dyn ProjectionProvider>) -> Self {
        self.projection_provider = provider;
        self
    }

    /// Set the V1 reviewable-context projection state.
    ///
    /// When populated, dashboard Ledger View reads from the projection.
    pub fn with_projection(mut self, state: ProjectionState) -> Self {
        self.projection = Arc::new(RwLock::new(Some(state)));
        self
    }

    /// Create state with a known workspace root.
    pub fn with_workspace_root(audit: AuditStore, root: PathBuf) -> Self {
        let evidence_root = root.join(".bicameral/sources/evidence");
        let mut state = Self::new(audit)
            .with_evidence_repository(Arc::new(FileEvidenceRepository::new(evidence_root)));
        state.workspace_root = Arc::new(RwLock::new(Some(root)));
        state
    }

    /// Hydrate in-memory maps from replayed event-store state.
    ///
    /// Called on daemon startup so the dashboard reflects previously
    /// persisted events after a restart/replay cycle.
    #[allow(clippy::too_many_arguments)]
    pub async fn hydrate(
        &self,
        replayed_sources: HashMap<String, SourceRecord>,
        replayed_snapshots: HashMap<String, SnapshotRecord>,
        replayed_evidence: HashMap<Uuid, SourceEvidence>,
        replayed_candidates: HashMap<Uuid, DecisionCandidate>,
        replayed_decisions: HashMap<Uuid, Decision>,
        replayed_bindings: HashMap<Uuid, Vec<BindingEvidence>>,
        replayed_content_hashes: HashMap<Uuid, Option<String>>,
    ) {
        let mut sources = self.sources.write().await;
        for (k, v) in replayed_sources {
            sources.insert(k, v);
        }

        let mut snapshots = self.snapshots.write().await;
        for (k, v) in replayed_snapshots {
            snapshots.insert(k, v);
        }

        let mut evidence = self.evidence.write().await;
        for (k, v) in replayed_evidence {
            evidence.insert(k, v);
        }

        let mut candidates = self.candidates.write().await;
        for (k, v) in replayed_candidates {
            candidates.insert(k, v);
        }

        let mut decisions = self.decisions.write().await;
        for (k, v) in replayed_decisions {
            decisions.insert(k, v);
        }

        let mut bindings = self.bindings.write().await;
        for (k, v) in replayed_bindings {
            bindings.insert(k, v);
        }

        let mut content_hashes = self.binding_content_hashes.write().await;
        for (k, v) in replayed_content_hashes {
            content_hashes.insert(k, v);
        }
    }

    /// Replace the in-memory read models with a freshly replayed projection.
    ///
    /// Unlike [`hydrate`], this clears each map first so a canonical write that
    /// transitions state (e.g. a candidate `Proposed` -> `Promoted`) is
    /// reflected rather than shadowed by a stale row. Called after a canonical
    /// `/v2/tool-requests` write so the read-only Ledger View mirrors the event
    /// store without a restart. Any populated V1 projection is invalidated so
    /// the next read rebuilds it from these refreshed maps.
    pub async fn refresh_projection(&self, replayed: ReplayedProjection) {
        *self.sources.write().await = replayed.sources;
        *self.snapshots.write().await = replayed.snapshots;
        *self.evidence.write().await = replayed.evidence;
        *self.candidates.write().await = replayed.candidates;
        *self.decisions.write().await = replayed.decisions;
        *self.bindings.write().await = replayed.bindings;
        *self.binding_content_hashes.write().await = replayed.binding_content_hashes;
        *self.projection.write().await = None;
    }
}

#[cfg(test)]
mod product_membership_projection_tests {
    use super::*;
    use bicameral_api::event_store::EventType;
    use serde_json::json;

    fn event(event_type: EventType, payload: serde_json::Value) -> EventStoreEntry {
        EventStoreEntry::new(event_type, payload, "test".into())
    }

    fn pairing_record(
        decision: PendingPairingDecision,
        expires_at: chrono::DateTime<chrono::Utc>,
        session_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> PendingPairingRecord {
        PendingPairingRecord {
            request_handle: "req".into(),
            poll_proof: "proof".into(),
            local_confirmation_nonce: "nonce".into(),
            pairing_code_digest: [0; 32],
            origin: "https://app.bicameral-ai.com".into(),
            cloud_account_id: "acct".into(),
            team_id: None,
            cloud_access_token: None,
            created_at: expires_at - chrono::Duration::minutes(5),
            expires_at,
            requested_ttl_seconds: 8 * 60 * 60,
            decision,
            session_token: (decision == PendingPairingDecision::Approved).then(|| "token".into()),
            session_expires_at,
            pairing_id: None,
        }
    }

    #[test]
    fn approved_pairing_restore_proof_lives_until_session_expiry() {
        let now = chrono::Utc::now();
        let record = pairing_record(
            PendingPairingDecision::Approved,
            now - chrono::Duration::minutes(1),
            Some(now + chrono::Duration::hours(1)),
        );
        assert!(!record.is_expired(now));
        assert!(record.is_expired(now + chrono::Duration::hours(1)));
    }

    #[test]
    fn pending_and_missing_approved_expiry_fail_closed_on_request_ttl() {
        let now = chrono::Utc::now();
        let pending = pairing_record(
            PendingPairingDecision::Pending,
            now - chrono::Duration::seconds(1),
            None,
        );
        let malformed_approved = pairing_record(
            PendingPairingDecision::Approved,
            now - chrono::Duration::seconds(1),
            None,
        );
        assert!(pending.is_expired(now));
        assert!(malformed_approved.is_expired(now));
    }

    #[test]
    fn projection_is_account_scoped_replayable_and_marks_registration_setup() {
        let entries = vec![
            event(
                EventType::CreateProduct,
                json!({
                    "product_id": "prd_created",
                    "creator_cloud_account_id": "acct_peer"
                }),
            ),
            event(
                EventType::ProductMemberAdded,
                json!({
                    "product_id": "prd_joined",
                    "cloud_account_id": "acct_peer"
                }),
            ),
            event(
                EventType::ProductMemberAdded,
                json!({
                    "product_id": "prd_other",
                    "cloud_account_id": "acct_other"
                }),
            ),
            event(EventType::ProductMemberAdded, json!({"malformed": true})),
        ];
        let registrations = vec![LocalProjectRegistration {
            name: "prd_joined".into(),
            event_store_healthy: true,
            has_workspace_binding: false,
            workspace_path: Some(PathBuf::from("/private/local-only")),
            provider_config: Some(json!({"token": "never-return"})),
            setup_state: "code_grounding_not_ready".into(),
        }];

        let projection = product_membership_projection(&entries, "acct_peer", &registrations);

        assert_eq!(
            projection,
            vec![
                HostedProductMembershipProjection {
                    product_id: "prd_created".into(),
                    setup_incomplete: true,
                },
                HostedProductMembershipProjection {
                    product_id: "prd_joined".into(),
                    setup_incomplete: false,
                },
            ]
        );
        let serialized = serde_json::to_string(&projection).unwrap();
        assert!(!serialized.contains("acct_other"));
        assert!(!serialized.contains("local-only"));
        assert!(!serialized.contains("never-return"));
    }
}
