//! Daemon-owned local workspace binding materialization (bicameral-bot#732).
//!
//! This module is the daemon-side authority that turns a surface-authored
//! [`WorkspaceBindingProposal`] into durable, operator-local per-project
//! [`LocalWorkspaceBinding`] state. It builds directly on the merged protocol
//! contract from bicameral-bot#731:
//!
//! - [`classify_bind_request`] remains the single, pure outcome mapping. This
//!   module only *gathers daemon facts* (registration, session scope, candidate
//!   path safety, current durable state, capability) and *persists* on success;
//!   it never re-implements the typed-outcome decision tree.
//! - The local `candidate_path` is persisted only in the operator-local store
//!   and is never echoed in a [`WorkspaceBindResponse`] or any hosted-safe view.
//!
//! # Authority boundary
//!
//! MCP / CLI / dashboard surfaces may *propose* a binding, but only the daemon
//! validates and materializes it. There is no surface-writable persistence
//! path: every bind flows through [`bind_workspace`], which is the sole writer
//! of [`LocalWorkspaceBinding`] records. The dispatcher runs authority
//! validation (hosted bridge is rejected for `workspace.bind`) before routing
//! here, so a surface cannot bypass daemon validation.
//!
//! # Project identity is not inferred from the filesystem
//!
//! Candidate path safety is a *local usability* check only (absolute, exists,
//! is a readable directory). It deliberately does **not** walk upward looking
//! for ambient `.bicameral/` markers to infer project identity — identity comes
//! only from an explicit [`ProjectRegistration`], surfaced here through the
//! [`WorkspaceBindingRegistry`] seam.
//!
//! # Rebinding semantics
//!
//! Rebinding a project that is already `local_workspace_bound` returns the typed
//! [`WorkspaceBindErrorKind::AlreadyBound`] outcome and does **not** mutate the
//! stored binding. Replacement is not implicit: an operator must first move the
//! project out of the bound state (repair/unbind, tracked by later slices)
//! before a new bind is accepted. This keeps the daemon the sole authority over
//! binding transitions and avoids silently overwriting a good binding.

use std::io;
use std::path::{Path, PathBuf};

use bicameral_api::workspace_binding::{
    classify_bind_request, DaemonBindingFacts, ProjectSafeDisplayMetadata, WorkspaceBindRequest,
    WorkspaceBindResponse, WorkspaceBindingSourceSurface, WorkspaceBindingState,
    WorkspaceResolveErrorKind, WorkspaceResolveErrorResponse, WorkspaceResolveRequest,
    WorkspaceResolveResponse,
};
use serde::{Deserialize, Serialize};

/// Daemon-advertised workspace-binding capability version.
///
/// Callers may set [`WorkspaceBindRequest::required_daemon_capability`]; a
/// request requiring a higher version than this is rejected with
/// [`WorkspaceBindErrorKind::DaemonCapabilityMismatch`](bicameral_api::workspace_binding::WorkspaceBindErrorKind::DaemonCapabilityMismatch).
pub const DAEMON_WORKSPACE_BINDING_CAPABILITY: u32 = 1;

// ── Durable operator-local record ──────────────────────────────────────────

/// Operator-local, per-project durable workspace binding record.
///
/// This is the durable materialization of a successful `workspace.bind`. It is
/// **operator-local**: it lives only in the operator-local store (see
/// [`FileWorkspaceBindingStore`]) and is never cloud-owned, never crosses the
/// hosted bridge, and is not `BindingEvidence` or source authority. The
/// local-only `candidate_path` is retained here for daemon local code grounding
/// and never surfaced through hosted-safe projections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalWorkspaceBinding {
    /// Opaque, already-registered project identifier this binding grounds.
    pub project_id: String,

    /// **LOCAL-ONLY.** Absolute candidate folder path bound for local grounding.
    /// Never crosses the hosted bridge; persisted only in the operator-local
    /// store.
    pub candidate_path: String,

    /// Durable lifecycle state. A materialized record is
    /// [`WorkspaceBindingState::LocalWorkspaceBound`].
    pub state: WorkspaceBindingState,

    /// Which local surface authored the proposal that produced this binding.
    pub source_surface: WorkspaceBindingSourceSurface,

    /// Redacted, hosted-safe display metadata carried for local convenience.
    pub display: ProjectSafeDisplayMetadata,

    /// Daemon capability version that materialized the binding.
    pub daemon_capability: u32,

    /// RFC 3339 timestamp of materialization (operator-local receipt).
    pub bound_at: String,
}

// ── Registry seam ──────────────────────────────────────────────────────────

/// Read-only view of local project registration the daemon consults before
/// binding.
///
/// Project identity is never inferred from the filesystem — it comes only from
/// explicit registration, surfaced through this seam so the classifier can
/// reject unregistered or wrong-project proposals with typed errors.
pub trait WorkspaceBindingRegistry {
    /// Whether a project with `project_id` is registered locally.
    fn is_registered(&self, project_id: &str) -> bool;

    /// The project id the daemon considers current for this session, if the
    /// daemon scopes a session to a single project. `None` means no session
    /// scoping is enforced (any registered project may be targeted).
    fn session_project_id(&self) -> Option<String>;
}

/// A simple in-memory [`WorkspaceBindingRegistry`] snapshot.
///
/// The daemon builds one of these from its live project registrations before
/// each bind so the classifier evaluates a consistent, point-in-time view.
#[derive(Debug, Clone, Default)]
pub struct StaticWorkspaceBindingRegistry {
    registered_project_ids: Vec<String>,
    session_project_id: Option<String>,
}

impl StaticWorkspaceBindingRegistry {
    /// Build a registry from the set of registered project ids and an optional
    /// session-scoped project id.
    pub fn new(
        registered_project_ids: impl IntoIterator<Item = String>,
        session_project_id: Option<String>,
    ) -> Self {
        Self {
            registered_project_ids: registered_project_ids.into_iter().collect(),
            session_project_id,
        }
    }
}

impl WorkspaceBindingRegistry for StaticWorkspaceBindingRegistry {
    fn is_registered(&self, project_id: &str) -> bool {
        self.registered_project_ids
            .iter()
            .any(|id| id == project_id)
    }

    fn session_project_id(&self) -> Option<String> {
        self.session_project_id.clone()
    }
}

// ── Persistence seam ───────────────────────────────────────────────────────

/// Operator-local persistence for [`LocalWorkspaceBinding`] records.
///
/// The daemon is the sole writer. Implementations must be durable per-project
/// (keyed by `project_id`) and must never expose the local `candidate_path`
/// outside operator-local storage.
pub trait WorkspaceBindingStore {
    /// Load the current binding for `project_id`, if any.
    fn load(&self, project_id: &str) -> io::Result<Option<LocalWorkspaceBinding>>;

    /// Persist (create or replace) a binding record.
    fn persist(&self, binding: &LocalWorkspaceBinding) -> io::Result<()>;

    /// List every operator-local binding record.
    ///
    /// Used by `workspace.resolve` as a derived path index. This is not a new
    /// identity key: records remain keyed by `project_id`. Callers must not
    /// treat listing as Product discovery.
    fn list(&self) -> io::Result<Vec<LocalWorkspaceBinding>>;
}

/// File-backed operator-local [`WorkspaceBindingStore`].
///
/// Records are written as `<root>/<sanitized project_id>.json`. The root is an
/// operator-local directory (e.g. `~/.bicameral/workspace-bindings/`), never a
/// cloud location and never inside a bound workspace itself.
#[derive(Debug, Clone)]
pub struct FileWorkspaceBindingStore {
    root: PathBuf,
}

impl FileWorkspaceBindingStore {
    /// Create a store rooted at `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Deterministic per-project record path.
    fn record_path(&self, project_id: &str) -> PathBuf {
        self.root
            .join(format!("{}.json", sanitize_project_id(project_id)))
    }
}

impl WorkspaceBindingStore for FileWorkspaceBindingStore {
    fn load(&self, project_id: &str) -> io::Result<Option<LocalWorkspaceBinding>> {
        let path = self.record_path(project_id);
        match std::fs::read(&path) {
            Ok(bytes) => {
                let binding: LocalWorkspaceBinding = serde_json::from_slice(&bytes)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                Ok(Some(binding))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn persist(&self, binding: &LocalWorkspaceBinding) -> io::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        let path = self.record_path(&binding.project_id);
        let json = serde_json::to_vec_pretty(binding)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, json)
    }

    fn list(&self) -> io::Result<Vec<LocalWorkspaceBinding>> {
        let entries = match std::fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut bindings = Vec::new();
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let bytes = std::fs::read(&path)?;
            let binding: LocalWorkspaceBinding = serde_json::from_slice(&bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            bindings.push(binding);
        }
        Ok(bindings)
    }
}

/// Map a project id to a filesystem-safe file stem. Non-alphanumeric (except
/// `-`, `_`, `.`) characters are replaced with `_` so an opaque id never
/// escapes the store directory.
fn sanitize_project_id(project_id: &str) -> String {
    project_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// ── Candidate path policy ──────────────────────────────────────────────────

/// Result of the daemon's local candidate-path safety policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidatePathAssessment {
    /// Whether the candidate path is usable for local code grounding.
    pub safe: bool,
    /// Human-readable reason (diagnostic only; never crosses the hosted bridge).
    pub reason: String,
}

/// Evaluate whether a candidate path is usable for local code grounding.
///
/// This is a *local usability* check only. It requires an absolute path that
/// exists and is a readable directory. It deliberately does **not** walk upward
/// to infer project identity from ambient `.bicameral/` paths — identity comes
/// only from explicit registration. Unsafe / nonexistent / unreadable paths are
/// reported as unsafe so the classifier rejects them without mutating state.
pub fn evaluate_candidate_path(candidate_path: &str) -> CandidatePathAssessment {
    let unsafe_with = |reason: &str| CandidatePathAssessment {
        safe: false,
        reason: reason.to_string(),
    };

    if candidate_path.trim().is_empty() {
        return unsafe_with("candidate path is empty");
    }

    let path = Path::new(candidate_path);
    if !path.is_absolute() {
        return unsafe_with("candidate path must be absolute");
    }

    let metadata = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return unsafe_with("candidate path does not exist");
        }
        Err(_) => return unsafe_with("candidate path is unreadable"),
    };

    // Resolve through a symlink target's metadata to require a real directory.
    let resolved = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return unsafe_with("candidate path target is unreadable"),
    };
    let _ = metadata;

    if !resolved.is_dir() {
        return unsafe_with("candidate path is not a directory");
    }

    // Readability: the daemon must be able to enumerate the directory to ground
    // local code. A permission error here is treated as unsafe.
    if std::fs::read_dir(path).is_err() {
        return unsafe_with("candidate path directory is not readable");
    }

    CandidatePathAssessment {
        safe: true,
        reason: "candidate path is an absolute, readable directory".to_string(),
    }
}

// ── Orchestration ──────────────────────────────────────────────────────────

/// Failure returned by [`bind_workspace`].
#[derive(Debug)]
pub enum WorkspaceBindError {
    /// A typed protocol outcome (already-bound, repair-required, unregistered,
    /// wrong-project, unsafe-path, confirmation-missing, capability-mismatch).
    /// State is never mutated when this is returned.
    Rejected(bicameral_api::workspace_binding::WorkspaceBindErrorResponse),
    /// An operator-local store I/O failure — the bind did not persist. This is
    /// an internal error, not a protocol rejection.
    Store(String),
}

/// Materialize a workspace binding: gather daemon facts, apply the pure
/// [`classify_bind_request`] mapping, and persist on success.
///
/// `now_rfc3339` is injected so callers control the receipt timestamp
/// (deterministic in tests). Persistence happens **only** when the classifier
/// returns success; every rejection leaves stored state untouched.
pub fn bind_workspace(
    request: &WorkspaceBindRequest,
    registry: &dyn WorkspaceBindingRegistry,
    store: &dyn WorkspaceBindingStore,
    now_rfc3339: &str,
) -> Result<WorkspaceBindResponse, WorkspaceBindError> {
    let proposal = &request.proposal;

    // Current durable state comes from the operator-local store. A missing
    // record means the project is unbound.
    let existing = store
        .load(&proposal.project_id)
        .map_err(|e| WorkspaceBindError::Store(format!("failed to read binding store: {e}")))?;
    let current_state = existing
        .as_ref()
        .map(|b| b.state)
        .unwrap_or(WorkspaceBindingState::LocalWorkspaceUnbound);

    let assessment = evaluate_candidate_path(&proposal.candidate_path);

    let facts = DaemonBindingFacts {
        project_registered: registry.is_registered(&proposal.project_id),
        session_project_id: registry.session_project_id(),
        current_state,
        daemon_capability: DAEMON_WORKSPACE_BINDING_CAPABILITY,
        candidate_path_safe: assessment.safe,
    };

    let response = classify_bind_request(request, &facts).map_err(WorkspaceBindError::Rejected)?;

    // Success: materialize the durable operator-local record. This is the only
    // write path for LocalWorkspaceBinding.
    let binding = LocalWorkspaceBinding {
        project_id: proposal.project_id.clone(),
        candidate_path: proposal.candidate_path.clone(),
        state: WorkspaceBindingState::LocalWorkspaceBound,
        source_surface: proposal.source_surface,
        display: proposal.display.clone(),
        daemon_capability: DAEMON_WORKSPACE_BINDING_CAPABILITY,
        bound_at: now_rfc3339.to_string(),
    };
    store
        .persist(&binding)
        .map_err(|e| WorkspaceBindError::Store(format!("failed to persist binding: {e}")))?;

    Ok(response)
}

// ── Reverse lookup (workspace.resolve, mcp#831) ────────────────────────────

/// Failure returned by [`resolve_workspace`].
#[derive(Debug)]
pub enum WorkspaceResolveError {
    /// Typed protocol outcome (unbound, repair-required, ambiguous, unsafe-path).
    /// State is never mutated.
    Rejected(WorkspaceResolveErrorResponse),
    /// An operator-local store I/O failure. Internal, not a protocol rejection.
    Store(String),
}

/// Canonical path key used for exact-match reverse lookup.
///
/// Prefers `fs::canonicalize` (resolves symlinks) so `/var/folders` and
/// `/private/var/folders` compare equal. If the path does not exist, falls
/// back to component-normalized form. Parent and child paths remain distinct.
fn path_equality_key(path: &str) -> PathBuf {
    let raw = Path::new(path);
    std::fs::canonicalize(raw).unwrap_or_else(|_| raw.components().collect())
}

/// Reverse-lookup a unique `local_workspace_bound` record for `candidate_path`.
///
/// Equality is exact canonical path only. Parent/child folders do not match.
/// Zero matches → `unbound`. A unique `repair_required` match → `repair_required`.
/// Two or more matches → `ambiguous` (never pick first/nearest). The response
/// never echoes `candidate_path` and never invents a Product id.
pub fn resolve_workspace(
    request: &WorkspaceResolveRequest,
    store: &dyn WorkspaceBindingStore,
) -> Result<WorkspaceResolveResponse, WorkspaceResolveError> {
    let rejected = |kind: WorkspaceResolveErrorKind,
                    message: &str,
                    state: WorkspaceBindingState,
                    match_count: u32| {
        WorkspaceResolveError::Rejected(WorkspaceResolveErrorResponse {
            error: kind,
            message: message.to_string(),
            state,
            match_count,
        })
    };

    let candidate = request.candidate_path.trim();
    if candidate.is_empty() {
        return Err(rejected(
            WorkspaceResolveErrorKind::UnsafePath,
            "Candidate path is empty.",
            WorkspaceBindingState::LocalWorkspaceUnbound,
            0,
        ));
    }
    let candidate_path = Path::new(candidate);
    if !candidate_path.is_absolute() {
        return Err(rejected(
            WorkspaceResolveErrorKind::UnsafePath,
            "Candidate path must be absolute.",
            WorkspaceBindingState::LocalWorkspaceUnbound,
            0,
        ));
    }

    let hint_key = path_equality_key(candidate);
    let records = store
        .list()
        .map_err(|e| WorkspaceResolveError::Store(format!("failed to list binding store: {e}")))?;

    let matches: Vec<LocalWorkspaceBinding> = records
        .into_iter()
        .filter(|binding| path_equality_key(&binding.candidate_path) == hint_key)
        .collect();

    match matches.as_slice() {
        [] => Err(rejected(
            WorkspaceResolveErrorKind::Unbound,
            "No local workspace binding matches this folder.",
            WorkspaceBindingState::LocalWorkspaceUnbound,
            0,
        )),
        [binding] if binding.state == WorkspaceBindingState::LocalWorkspaceBound => {
            Ok(WorkspaceResolveResponse {
                product_id: binding.project_id.clone(),
                state: WorkspaceBindingState::LocalWorkspaceBound,
                display: binding.display.clone(),
            })
        }
        [binding] if binding.state == WorkspaceBindingState::LocalWorkspaceRepairRequired => {
            Err(rejected(
                WorkspaceResolveErrorKind::RepairRequired,
                "The unique matching workspace binding must be repaired before use.",
                WorkspaceBindingState::LocalWorkspaceRepairRequired,
                1,
            ))
        }
        [binding] => Err(rejected(
            WorkspaceResolveErrorKind::Unbound,
            "No local workspace binding matches this folder.",
            binding.state,
            1,
        )),
        many => Err(rejected(
            WorkspaceResolveErrorKind::Ambiguous,
            "Multiple products are bound to this folder; refuse to pick one.",
            WorkspaceBindingState::LocalWorkspaceBound,
            many.len() as u32,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_api::workspace_binding::{WorkspaceBindErrorKind, WorkspaceBindingProposal};
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// In-memory store that records persist calls, for bypass/no-mutation tests.
    #[derive(Default)]
    struct MemStore {
        records: RefCell<HashMap<String, LocalWorkspaceBinding>>,
        persist_calls: RefCell<usize>,
    }

    impl WorkspaceBindingStore for MemStore {
        fn load(&self, project_id: &str) -> io::Result<Option<LocalWorkspaceBinding>> {
            Ok(self.records.borrow().get(project_id).cloned())
        }
        fn persist(&self, binding: &LocalWorkspaceBinding) -> io::Result<()> {
            *self.persist_calls.borrow_mut() += 1;
            self.records
                .borrow_mut()
                .insert(binding.project_id.clone(), binding.clone());
            Ok(())
        }
        fn list(&self) -> io::Result<Vec<LocalWorkspaceBinding>> {
            Ok(self.records.borrow().values().cloned().collect())
        }
    }

    fn proposal_in(dir: &Path) -> WorkspaceBindingProposal {
        WorkspaceBindingProposal {
            project_id: "proj_ada".into(),
            display: ProjectSafeDisplayMetadata {
                display_name: "Ada Service".into(),
                project_slug: Some("ada-service".into()),
                candidate_label: Some("ada-service".into()),
            },
            candidate_path: dir.to_string_lossy().into_owned(),
            source_surface: WorkspaceBindingSourceSurface::Mcp,
            reason: "Open editor workspace matches the registered project.".into(),
            confidence: 0.9,
        }
    }

    fn request_in(dir: &Path, confirmed: bool) -> WorkspaceBindRequest {
        WorkspaceBindRequest {
            proposal: proposal_in(dir),
            confirmed,
            required_daemon_capability: None,
            expected_current_state: Some(WorkspaceBindingState::LocalWorkspaceUnbound),
        }
    }

    fn registered() -> StaticWorkspaceBindingRegistry {
        StaticWorkspaceBindingRegistry::new(["proj_ada".to_string()], None)
    }

    #[test]
    fn confirmed_bind_persists_operator_local_record() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemStore::default();
        let resp = bind_workspace(
            &request_in(dir.path(), true),
            &registered(),
            &store,
            "2026-07-08T00:00:00Z",
        )
        .unwrap();
        assert_eq!(resp.state, WorkspaceBindingState::LocalWorkspaceBound);
        assert_eq!(*store.persist_calls.borrow(), 1);
        let stored = store.records.borrow().get("proj_ada").cloned().unwrap();
        assert_eq!(stored.candidate_path, dir.path().to_string_lossy());
        assert_eq!(stored.state, WorkspaceBindingState::LocalWorkspaceBound);
    }

    #[test]
    fn missing_confirmation_does_not_mutate_state() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemStore::default();
        let err = bind_workspace(
            &request_in(dir.path(), false),
            &registered(),
            &store,
            "2026-07-08T00:00:00Z",
        )
        .unwrap_err();
        match err {
            WorkspaceBindError::Rejected(e) => {
                assert_eq!(e.error, WorkspaceBindErrorKind::ConfirmationMissing);
            }
            other => panic!("expected confirmation_missing, got {other:?}"),
        }
        assert_eq!(*store.persist_calls.borrow(), 0);
        assert!(store.records.borrow().is_empty());
    }

    #[test]
    fn unregistered_project_is_rejected_without_mutation() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemStore::default();
        let empty = StaticWorkspaceBindingRegistry::new(Vec::<String>::new(), None);
        let err = bind_workspace(&request_in(dir.path(), true), &empty, &store, "t").unwrap_err();
        match err {
            WorkspaceBindError::Rejected(e) => {
                assert_eq!(e.error, WorkspaceBindErrorKind::UnregisteredProject);
            }
            other => panic!("expected unregistered_project, got {other:?}"),
        }
        assert_eq!(*store.persist_calls.borrow(), 0);
    }

    #[test]
    fn nonexistent_path_is_unsafe_and_not_persisted() {
        let store = MemStore::default();
        let mut req = request_in(Path::new("/nonexistent/path/xyz-732"), true);
        req.proposal.candidate_path = "/nonexistent/path/xyz-732".into();
        let err = bind_workspace(&req, &registered(), &store, "t").unwrap_err();
        match err {
            WorkspaceBindError::Rejected(e) => {
                assert_eq!(e.error, WorkspaceBindErrorKind::UnsafePath);
            }
            other => panic!("expected unsafe_path, got {other:?}"),
        }
        assert_eq!(*store.persist_calls.borrow(), 0);
    }

    #[test]
    fn file_candidate_path_is_unsafe() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not-a-dir");
        std::fs::write(&file, b"x").unwrap();
        let assessment = evaluate_candidate_path(&file.to_string_lossy());
        assert!(!assessment.safe);
    }

    #[test]
    fn relative_path_is_unsafe() {
        assert!(!evaluate_candidate_path("relative/dir").safe);
    }

    #[test]
    fn rebinding_bound_project_returns_already_bound() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemStore::default();
        // First bind succeeds.
        bind_workspace(&request_in(dir.path(), true), &registered(), &store, "t1").unwrap();
        // Second bind returns already_bound and does not persist again.
        let err =
            bind_workspace(&request_in(dir.path(), true), &registered(), &store, "t2").unwrap_err();
        match err {
            WorkspaceBindError::Rejected(e) => {
                assert_eq!(e.error, WorkspaceBindErrorKind::AlreadyBound);
                assert_eq!(e.state, WorkspaceBindingState::LocalWorkspaceBound);
            }
            other => panic!("expected already_bound, got {other:?}"),
        }
        assert_eq!(*store.persist_calls.borrow(), 1);
    }

    #[test]
    fn wrong_project_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemStore::default();
        let registry = StaticWorkspaceBindingRegistry::new(
            ["proj_ada".to_string()],
            Some("proj_other".into()),
        );
        let err =
            bind_workspace(&request_in(dir.path(), true), &registry, &store, "t").unwrap_err();
        match err {
            WorkspaceBindError::Rejected(e) => {
                assert_eq!(e.error, WorkspaceBindErrorKind::WrongProject);
            }
            other => panic!("expected wrong_project, got {other:?}"),
        }
        assert_eq!(*store.persist_calls.borrow(), 0);
    }

    #[test]
    fn file_store_round_trips_binding() {
        let dir = tempfile::tempdir().unwrap();
        let ws = tempfile::tempdir().unwrap();
        let store = FileWorkspaceBindingStore::new(dir.path());
        assert!(store.load("proj_ada").unwrap().is_none());
        bind_workspace(&request_in(ws.path(), true), &registered(), &store, "t").unwrap();
        let loaded = store.load("proj_ada").unwrap().unwrap();
        assert_eq!(loaded.state, WorkspaceBindingState::LocalWorkspaceBound);
        assert_eq!(loaded.candidate_path, ws.path().to_string_lossy());
    }

    #[test]
    fn response_never_carries_candidate_path() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemStore::default();
        let resp =
            bind_workspace(&request_in(dir.path(), true), &registered(), &store, "t").unwrap();
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("candidate_path").is_none());
        assert!(!json_contains_absolute_path(&json));
    }

    fn json_contains_absolute_path(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::String(s) => s.starts_with('/'),
            serde_json::Value::Array(items) => items.iter().any(json_contains_absolute_path),
            serde_json::Value::Object(map) => map.values().any(json_contains_absolute_path),
            _ => false,
        }
    }

    fn stored_binding(
        project_id: &str,
        dir: &Path,
        state: WorkspaceBindingState,
    ) -> LocalWorkspaceBinding {
        LocalWorkspaceBinding {
            project_id: project_id.into(),
            candidate_path: dir.to_string_lossy().into_owned(),
            state,
            source_surface: WorkspaceBindingSourceSurface::Mcp,
            display: ProjectSafeDisplayMetadata {
                display_name: project_id.into(),
                project_slug: None,
                candidate_label: None,
            },
            daemon_capability: DAEMON_WORKSPACE_BINDING_CAPABILITY,
            bound_at: "t".into(),
        }
    }

    #[test]
    fn resolve_unique_bound_folder_returns_product_id() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemStore::default();
        store
            .persist(&stored_binding(
                "proj_ada",
                dir.path(),
                WorkspaceBindingState::LocalWorkspaceBound,
            ))
            .unwrap();
        let resp = resolve_workspace(
            &WorkspaceResolveRequest {
                candidate_path: dir.path().to_string_lossy().into_owned(),
            },
            &store,
        )
        .unwrap();
        assert_eq!(resp.product_id, "proj_ada");
        assert_eq!(resp.state, WorkspaceBindingState::LocalWorkspaceBound);
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("candidate_path").is_none());
        assert!(!json_contains_absolute_path(&json));
    }

    #[test]
    fn resolve_unbound_folder_is_typed_unbound() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemStore::default();
        let err = resolve_workspace(
            &WorkspaceResolveRequest {
                candidate_path: dir.path().to_string_lossy().into_owned(),
            },
            &store,
        )
        .unwrap_err();
        match err {
            WorkspaceResolveError::Rejected(e) => {
                assert_eq!(e.error, WorkspaceResolveErrorKind::Unbound);
                assert_eq!(e.match_count, 0);
            }
            other => panic!("expected unbound, got {other:?}"),
        }
    }

    #[test]
    fn resolve_two_products_same_path_is_ambiguous() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemStore::default();
        store
            .persist(&stored_binding(
                "proj_ada",
                dir.path(),
                WorkspaceBindingState::LocalWorkspaceBound,
            ))
            .unwrap();
        store
            .persist(&stored_binding(
                "proj_other",
                dir.path(),
                WorkspaceBindingState::LocalWorkspaceBound,
            ))
            .unwrap();
        let err = resolve_workspace(
            &WorkspaceResolveRequest {
                candidate_path: dir.path().to_string_lossy().into_owned(),
            },
            &store,
        )
        .unwrap_err();
        match err {
            WorkspaceResolveError::Rejected(e) => {
                assert_eq!(e.error, WorkspaceResolveErrorKind::Ambiguous);
                assert_eq!(e.match_count, 2);
                let json = serde_json::to_value(&e).unwrap();
                assert!(json.get("product_id").is_none());
                assert!(json.get("candidate_path").is_none());
            }
            other => panic!("expected ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn resolve_unique_repair_required_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemStore::default();
        store
            .persist(&stored_binding(
                "proj_ada",
                dir.path(),
                WorkspaceBindingState::LocalWorkspaceRepairRequired,
            ))
            .unwrap();
        let err = resolve_workspace(
            &WorkspaceResolveRequest {
                candidate_path: dir.path().to_string_lossy().into_owned(),
            },
            &store,
        )
        .unwrap_err();
        match err {
            WorkspaceResolveError::Rejected(e) => {
                assert_eq!(e.error, WorkspaceResolveErrorKind::RepairRequired);
                assert_eq!(e.state, WorkspaceBindingState::LocalWorkspaceRepairRequired);
            }
            other => panic!("expected repair_required, got {other:?}"),
        }
    }

    #[test]
    fn resolve_does_not_match_parent_or_child_folder() {
        let parent = tempfile::tempdir().unwrap();
        let child = parent.path().join("child");
        std::fs::create_dir(&child).unwrap();
        let store = MemStore::default();
        store
            .persist(&stored_binding(
                "proj_parent",
                parent.path(),
                WorkspaceBindingState::LocalWorkspaceBound,
            ))
            .unwrap();
        let err = resolve_workspace(
            &WorkspaceResolveRequest {
                candidate_path: child.to_string_lossy().into_owned(),
            },
            &store,
        )
        .unwrap_err();
        match err {
            WorkspaceResolveError::Rejected(e) => {
                assert_eq!(e.error, WorkspaceResolveErrorKind::Unbound);
            }
            other => panic!("expected unbound for nested path, got {other:?}"),
        }
    }

    #[test]
    fn resolve_relative_path_is_unsafe() {
        let store = MemStore::default();
        let err = resolve_workspace(
            &WorkspaceResolveRequest {
                candidate_path: "relative/dir".into(),
            },
            &store,
        )
        .unwrap_err();
        match err {
            WorkspaceResolveError::Rejected(e) => {
                assert_eq!(e.error, WorkspaceResolveErrorKind::UnsafePath);
            }
            other => panic!("expected unsafe_path, got {other:?}"),
        }
    }

    #[test]
    fn file_store_list_feeds_resolve() {
        let store_dir = tempfile::tempdir().unwrap();
        let ws = tempfile::tempdir().unwrap();
        let store = FileWorkspaceBindingStore::new(store_dir.path());
        bind_workspace(&request_in(ws.path(), true), &registered(), &store, "t").unwrap();
        let resp = resolve_workspace(
            &WorkspaceResolveRequest {
                candidate_path: ws.path().to_string_lossy().into_owned(),
            },
            &store,
        )
        .unwrap();
        assert_eq!(resp.product_id, "proj_ada");
    }
}
