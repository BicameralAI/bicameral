//! Local commit watcher read-model for commit-driven code freshness.
//!
//! The watcher owns only recomputable freshness observations. It detects local
//! HEAD movement, derives the touched file scope, and lets `preflight.run`
//! prefer that daemon-owned observation before falling back to caller-supplied
//! diff/target context. It never appends governance events or mutates binding,
//! decision, signoff, compliance, no-conflict, or merge-safety authority.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use bicameral_api::evidence::GraphReadiness;
use bicameral_api::freshness::{
    FreshnessInputs, PreflightFreshness, RefreshOutcome, TouchedDetectionMethod, TouchedScope,
};
use bicameral_api::tool_request::{GraphSliceInfo, LocatorSnapshotInfo};
use serde::{Deserialize, Serialize};

use crate::commit_freshness::{scope_fingerprint_for_watcher, BOUNDED_SCOPE_BUDGET};
use crate::graph_snapshot_service::GraphSnapshotService;
use crate::preflight::PreflightQuery;

/// Daemon-owned observation of a local HEAD transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitWatcherObservation {
    pub old_head_sha: String,
    pub new_head_sha: String,
    pub touched_scope: TouchedScope,
}

impl CommitWatcherObservation {
    pub fn new(old_head_sha: String, new_head_sha: String, files: Vec<String>) -> Self {
        Self {
            old_head_sha,
            new_head_sha,
            touched_scope: TouchedScope {
                detection: Some(TouchedDetectionMethod::DiffContext),
                files: dedup_preserving_order(files),
                symbols: Vec::new(),
            },
        }
    }

    pub fn target_count(&self) -> usize {
        self.touched_scope.target_count()
    }

    pub fn is_over_budget(&self) -> bool {
        self.target_count() > BOUNDED_SCOPE_BUDGET
    }
}

/// Shared latest watcher observation injected into runtime dispatch.
#[derive(Debug, Clone, Default)]
pub struct CommitWatcherStore {
    latest: Arc<Mutex<Option<CommitWatcherObservation>>>,
}

impl CommitWatcherStore {
    pub fn latest(&self) -> Option<CommitWatcherObservation> {
        self.latest.lock().ok().and_then(|guard| guard.clone())
    }

    pub fn replace(&self, observation: CommitWatcherObservation) {
        if let Ok(mut guard) = self.latest.lock() {
            *guard = Some(observation);
        }
    }
}

/// Product-keyed watcher observations for the operator-scoped daemon.
///
/// A Product-scoped request must never consume another Product's repository
/// observation. The key is the daemon-validated canonical Product id.
const PRODUCT_WATCHER_STORE_SCHEMA_VERSION: u8 = 1;
const PRODUCT_WATCHER_STORE_FILE: &str = "watcher-state-v1.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductWatcherLifecycle {
    Current,
    Stale,
    Failed,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductWatcherFailure {
    pub reason_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductWatcherState {
    pub product_id: String,
    pub workspace_root: Option<PathBuf>,
    pub lifecycle: ProductWatcherLifecycle,
    pub checkpoint_head_sha: Option<String>,
    pub queued_work: Vec<CommitWatcherObservation>,
    pub failure_backlog: Vec<ProductWatcherFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductWatcherRegistration {
    product_id: String,
    workspace_root: Option<PathBuf>,
}

impl ProductWatcherRegistration {
    pub fn enabled(product_id: impl Into<String>, workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            product_id: product_id.into(),
            workspace_root: Some(workspace_root.into()),
        }
    }

    pub fn disabled(product_id: impl Into<String>) -> Self {
        Self {
            product_id: product_id.into(),
            workspace_root: None,
        }
    }
}

impl<'a> From<(&'a str, &'a Path)> for ProductWatcherRegistration {
    fn from((product_id, workspace_root): (&'a str, &'a Path)) -> Self {
        Self::enabled(product_id, workspace_root)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProductWatcherEnvelope {
    schema_version: u8,
    records: std::collections::BTreeMap<String, ProductWatcherState>,
}

impl Default for ProductWatcherEnvelope {
    fn default() -> Self {
        Self {
            schema_version: PRODUCT_WATCHER_STORE_SCHEMA_VERSION,
            records: std::collections::BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProductCommitWatcherStore {
    state: Arc<Mutex<ProductWatcherEnvelope>>,
    path: Option<Arc<PathBuf>>,
}

impl Default for ProductCommitWatcherStore {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(ProductWatcherEnvelope::default())),
            path: None,
        }
    }
}

impl ProductCommitWatcherStore {
    pub fn open(root: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = root.as_ref().join(PRODUCT_WATCHER_STORE_FILE);
        let state = if path.exists() {
            let bytes = std::fs::read(&path)?;
            let decoded: ProductWatcherEnvelope = serde_json::from_slice(&bytes)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
            if decoded.schema_version != PRODUCT_WATCHER_STORE_SCHEMA_VERSION {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "unsupported Product watcher schema version {}",
                        decoded.schema_version
                    ),
                ));
            }
            decoded
        } else {
            ProductWatcherEnvelope::default()
        };
        Ok(Self {
            state: Arc::new(Mutex::new(state)),
            path: Some(Arc::new(path)),
        })
    }

    pub fn latest_for(&self, product_id: &str) -> Option<CommitWatcherObservation> {
        self.state
            .lock()
            .ok()
            .and_then(|guard| guard.records.get(product_id).cloned())
            .filter(|state| state.lifecycle == ProductWatcherLifecycle::Stale)
            .and_then(|state| state.queued_work.last().cloned())
    }

    pub fn state_for(&self, product_id: &str) -> Option<ProductWatcherState> {
        self.state
            .lock()
            .ok()
            .and_then(|guard| guard.records.get(product_id).cloned())
    }

    fn checkpoint_for(&self, product_id: &str) -> Option<String> {
        self.state_for(product_id)
            .and_then(|state| state.checkpoint_head_sha)
    }

    pub fn replace_for(
        &self,
        product_id: impl Into<String>,
        observation: CommitWatcherObservation,
    ) {
        let product_id = product_id.into();
        let _ = self.record_change(&product_id, None, observation);
    }

    fn record_success(
        &self,
        product_id: &str,
        workspace_root: &Path,
        head_sha: String,
    ) -> std::io::Result<()> {
        self.mutate(|records| {
            let state =
                records
                    .entry(product_id.to_string())
                    .or_insert_with(|| ProductWatcherState {
                        product_id: product_id.to_string(),
                        workspace_root: Some(workspace_root.to_path_buf()),
                        lifecycle: ProductWatcherLifecycle::Current,
                        checkpoint_head_sha: None,
                        queued_work: Vec::new(),
                        failure_backlog: Vec::new(),
                    });
            let before = state.clone();
            state.workspace_root = Some(workspace_root.to_path_buf());
            state.checkpoint_head_sha = Some(head_sha);
            state.lifecycle = if state.queued_work.is_empty() {
                ProductWatcherLifecycle::Current
            } else {
                ProductWatcherLifecycle::Stale
            };
            *state != before
        })
    }

    fn record_change(
        &self,
        product_id: &str,
        workspace_root: Option<&Path>,
        observation: CommitWatcherObservation,
    ) -> std::io::Result<()> {
        self.mutate(|records| {
            let state =
                records
                    .entry(product_id.to_string())
                    .or_insert_with(|| ProductWatcherState {
                        product_id: product_id.to_string(),
                        workspace_root: workspace_root.map(Path::to_path_buf),
                        lifecycle: ProductWatcherLifecycle::Current,
                        checkpoint_head_sha: None,
                        queued_work: Vec::new(),
                        failure_backlog: Vec::new(),
                    });
            let before = state.clone();
            if let Some(workspace_root) = workspace_root {
                state.workspace_root = Some(workspace_root.to_path_buf());
            }
            state.checkpoint_head_sha = Some(observation.new_head_sha.clone());
            if state.queued_work.last() != Some(&observation) {
                state.queued_work.push(observation);
            }
            state.lifecycle = ProductWatcherLifecycle::Stale;
            *state != before
        })
    }

    fn record_failure(
        &self,
        product_id: &str,
        workspace_root: &Path,
        reason_code: &str,
    ) -> std::io::Result<()> {
        self.mutate(|records| {
            let state =
                records
                    .entry(product_id.to_string())
                    .or_insert_with(|| ProductWatcherState {
                        product_id: product_id.to_string(),
                        workspace_root: Some(workspace_root.to_path_buf()),
                        lifecycle: ProductWatcherLifecycle::Current,
                        checkpoint_head_sha: None,
                        queued_work: Vec::new(),
                        failure_backlog: Vec::new(),
                    });
            let before = state.clone();
            state.workspace_root = Some(workspace_root.to_path_buf());
            let failure = ProductWatcherFailure {
                reason_code: reason_code.to_string(),
            };
            if state.failure_backlog.last() != Some(&failure) {
                state.failure_backlog.push(failure);
            }
            state.lifecycle = ProductWatcherLifecycle::Failed;
            *state != before
        })
    }

    fn record_disabled(&self, product_id: &str) -> std::io::Result<()> {
        self.mutate(|records| {
            let existed = records.contains_key(product_id);
            let state =
                records
                    .entry(product_id.to_string())
                    .or_insert_with(|| ProductWatcherState {
                        product_id: product_id.to_string(),
                        workspace_root: None,
                        lifecycle: ProductWatcherLifecycle::Disabled,
                        checkpoint_head_sha: None,
                        queued_work: Vec::new(),
                        failure_backlog: Vec::new(),
                    });
            let before = state.clone();
            state.workspace_root = None;
            state.lifecycle = ProductWatcherLifecycle::Disabled;
            !existed || *state != before
        })
    }

    fn retain_products(
        &self,
        desired: &std::collections::BTreeMap<String, Option<PathBuf>>,
    ) -> std::io::Result<()> {
        self.mutate(|records| {
            let before = records.len();
            records.retain(|product_id, _| desired.contains_key(product_id));
            records.len() != before
        })
    }

    fn mutate(
        &self,
        update: impl FnOnce(&mut std::collections::BTreeMap<String, ProductWatcherState>) -> bool,
    ) -> std::io::Result<()> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| std::io::Error::other("Product watcher state lock unavailable"))?;
        let changed = update(&mut guard.records);
        if changed {
            if let Some(path) = &self.path {
                persist_private_json(path, &guard)?;
            }
        }
        Ok(())
    }
}

/// Polling local HEAD watcher. The daemon drives this on an interval.
#[derive(Debug)]
pub struct CommitWatcher {
    workspace_root: PathBuf,
    last_head_sha: Option<String>,
}

impl CommitWatcher {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            last_head_sha: None,
        }
    }

    fn with_checkpoint(
        workspace_root: impl Into<PathBuf>,
        checkpoint_head_sha: Option<String>,
    ) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            last_head_sha: checkpoint_head_sha,
        }
    }

    /// Poll local git state once.
    ///
    /// The first successful poll records the baseline HEAD and emits no
    /// observation. A later HEAD change emits a touched-file observation from
    /// `git diff --name-only old..new`.
    pub fn poll_once(&mut self) -> Option<CommitWatcherObservation> {
        match self.poll_state() {
            CommitWatcherPoll::Changed(observation) => Some(observation),
            CommitWatcherPoll::Baseline { .. }
            | CommitWatcherPoll::Unchanged { .. }
            | CommitWatcherPoll::Failed { .. } => None,
        }
    }

    fn poll_state(&mut self) -> CommitWatcherPoll {
        let Some(head) = git_output(&self.workspace_root, &["rev-parse", "HEAD"]) else {
            return CommitWatcherPoll::Failed {
                reason_code: "head_unavailable",
            };
        };
        let head = head.trim().to_string();
        if head.is_empty() {
            return CommitWatcherPoll::Failed {
                reason_code: "head_unavailable",
            };
        }

        let Some(previous) = self.last_head_sha.clone() else {
            self.last_head_sha = Some(head.clone());
            return CommitWatcherPoll::Baseline { head_sha: head };
        };

        if previous == head {
            return CommitWatcherPoll::Unchanged { head_sha: head };
        }

        let range = format!("{previous}..{head}");
        let Some(diff) = git_output(&self.workspace_root, &["diff", "--name-only", &range]) else {
            return CommitWatcherPoll::Failed {
                reason_code: "diff_unavailable",
            };
        };
        let files = diff
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect();

        self.last_head_sha = Some(head.clone());
        CommitWatcherPoll::Changed(CommitWatcherObservation::new(previous, head, files))
    }
}

enum CommitWatcherPoll {
    Baseline { head_sha: String },
    Unchanged { head_sha: String },
    Changed(CommitWatcherObservation),
    Failed { reason_code: &'static str },
}

/// Independent repository watchers keyed by canonical Product id.
#[derive(Debug, Default)]
pub struct ProductCommitWatchers {
    watchers: std::collections::BTreeMap<String, CommitWatcher>,
    store: ProductCommitWatcherStore,
}

impl ProductCommitWatchers {
    pub fn with_store(store: ProductCommitWatcherStore) -> Self {
        Self {
            watchers: std::collections::BTreeMap::new(),
            store,
        }
    }

    pub fn reconcile_and_poll<I, R>(&mut self, products: I) -> std::io::Result<()>
    where
        I: IntoIterator<Item = R>,
        R: Into<ProductWatcherRegistration>,
    {
        let desired: std::collections::BTreeMap<String, Option<PathBuf>> = products
            .into_iter()
            .map(|registration| {
                let registration = registration.into();
                (registration.product_id, registration.workspace_root)
            })
            .collect();

        self.watchers.retain(|product_id, watcher| {
            let keep = desired
                .get(product_id)
                .and_then(Option::as_ref)
                .is_some_and(|path| path == &watcher.workspace_root);
            keep
        });

        self.store.retain_products(&desired)?;

        for (product_id, workspace_root) in &desired {
            let Some(workspace_root) = workspace_root else {
                self.store.record_disabled(product_id)?;
                continue;
            };
            let checkpoint = self.store.checkpoint_for(product_id);
            let watcher = self
                .watchers
                .entry(product_id.clone())
                .or_insert_with(|| CommitWatcher::with_checkpoint(workspace_root, checkpoint));
            match watcher.poll_state() {
                CommitWatcherPoll::Baseline { head_sha }
                | CommitWatcherPoll::Unchanged { head_sha } => {
                    self.store
                        .record_success(product_id, workspace_root, head_sha)?;
                }
                CommitWatcherPoll::Changed(observation) => {
                    self.store
                        .record_change(product_id, Some(workspace_root), observation)?;
                }
                CommitWatcherPoll::Failed { reason_code } => {
                    self.store
                        .record_failure(product_id, workspace_root, reason_code)?;
                }
            }
        }
        Ok(())
    }

    pub fn latest_for(&self, product_id: &str) -> Option<CommitWatcherObservation> {
        self.store.latest_for(product_id)
    }

    pub fn state_for(&self, product_id: &str) -> Option<ProductWatcherState> {
        self.store.state_for(product_id)
    }

    pub fn store(&self) -> ProductCommitWatcherStore {
        self.store.clone()
    }
}

/// Compute preflight freshness from the latest daemon watcher observation.
///
/// Returns `None` when the watcher has no usable observation for the current
/// request, allowing the caller to fall back to caller-provided diff/targets.
pub fn compute_watcher_preflight_freshness(
    query: &PreflightQuery,
    svc: Option<&dyn GraphSnapshotService>,
    result_was_empty: bool,
    observation: Option<&CommitWatcherObservation>,
) -> Option<PreflightFreshness> {
    let observation = observation?;
    let svc = svc?;

    let bounded = watcher_bounded_context(svc, observation);
    let touched_scope = if observation.is_over_budget() || query_targets(query).is_empty() {
        observation.touched_scope.clone()
    } else if query_intersects_observation(query, observation) {
        intersecting_touched_scope(query, observation)
    } else {
        TouchedScope::confirmed_untouched()
    };

    let refresh_outcome = if observation.is_over_budget() {
        RefreshOutcome::OverBudget
    } else if touched_scope.detection == Some(TouchedDetectionMethod::ConfirmedUntouched)
        || touched_scope.has_targets()
    {
        RefreshOutcome::Stale
    } else {
        RefreshOutcome::NotRequested
    };

    Some(PreflightFreshness::derive(FreshnessInputs {
        touched_scope,
        refresh_outcome,
        graph_readiness: bounded.graph_readiness,
        graph_snapshot: bounded.graph_snapshot,
        scope: bounded.scope,
        result_was_empty,
    }))
}

struct WatcherBoundedContext {
    graph_readiness: GraphReadiness,
    graph_snapshot: Option<LocatorSnapshotInfo>,
    scope: Option<GraphSliceInfo>,
}

fn watcher_bounded_context(
    svc: &dyn GraphSnapshotService,
    observation: &CommitWatcherObservation,
) -> WatcherBoundedContext {
    let graph_readiness = svc.readiness_view().readiness;
    let graph_snapshot = svc.resolve_head().ok().map(|s| LocatorSnapshotInfo {
        graph_snapshot_id: Some(s.graph_snapshot_id),
        repo_id: Some(s.repo_id),
        commit_sha: Some(s.commit_sha),
        index_version: Some(s.index_version),
        ref_name: s.ref_name,
    });
    let scope = Some(GraphSliceInfo {
        graph_slice_id: None,
        scope_kind: Some("diff".to_string()),
        scope_fingerprint: Some(scope_fingerprint_for_watcher(&observation.touched_scope)),
        completeness: Some(match graph_readiness {
            GraphReadiness::Ready => bicameral_api::tool_request::SliceCompleteness::DiffScoped,
            _ => bicameral_api::tool_request::SliceCompleteness::NotAvailable,
        }),
    });

    WatcherBoundedContext {
        graph_readiness,
        graph_snapshot,
        scope,
    }
}

fn query_intersects_observation(
    query: &PreflightQuery,
    observation: &CommitWatcherObservation,
) -> bool {
    let observed_files = &observation.touched_scope.files;
    query
        .files
        .iter()
        .any(|file| observed_files.iter().any(|observed| observed == file))
}

fn intersecting_touched_scope(
    query: &PreflightQuery,
    observation: &CommitWatcherObservation,
) -> TouchedScope {
    let files: Vec<String> = query
        .files
        .iter()
        .filter(|file| {
            observation
                .touched_scope
                .files
                .iter()
                .any(|observed| observed == *file)
        })
        .cloned()
        .collect();

    TouchedScope {
        detection: Some(TouchedDetectionMethod::DiffContext),
        files: dedup_preserving_order(files),
        symbols: Vec::new(),
    }
}

fn query_targets(query: &PreflightQuery) -> Vec<String> {
    query
        .files
        .iter()
        .chain(query.symbols.iter())
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn dedup_preserving_order(items: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for item in items {
        let trimmed = item.trim();
        if !trimmed.is_empty() && !out.iter().any(|seen| seen == trimmed) {
            out.push(trimmed.to_string());
        }
    }
    out
}

fn git_output(workspace_root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace_root)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout).ok()
}

fn persist_private_json(path: &Path, envelope: &ProductWatcherEnvelope) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Product watcher state path has no parent",
        )
    })?;
    std::fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }

    let temporary = parent.join(format!(
        ".{PRODUCT_WATCHER_STORE_FILE}.tmp-{}",
        std::process::id()
    ));
    let bytes = serde_json::to_vec_pretty(envelope)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    #[cfg(not(unix))]
    std::fs::write(&temporary, &bytes)?;

    std::fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_api::evidence::{
        BindingEvidence, CodeSourceCurrentness, CodeSourceRefreshResult, FileInspectionResult,
        GraphEvidenceState, GraphReadinessView, GraphSnapshot, SnapshotResolution,
        SymbolResolutionResult,
    };
    use bicameral_api::freshness::{CommitFreshness, FreshnessCaveatCode};
    use uuid::Uuid;

    #[test]
    fn product_watcher_observations_are_isolated_by_product() {
        let store = ProductCommitWatcherStore::default();
        let product_a =
            CommitWatcherObservation::new("a-old".into(), "a-new".into(), vec!["src/a.rs".into()]);
        let product_b =
            CommitWatcherObservation::new("b-old".into(), "b-new".into(), vec!["src/b.rs".into()]);

        store.replace_for("product-a", product_a.clone());
        store.replace_for("product-b", product_b.clone());

        assert_eq!(store.latest_for("product-a"), Some(product_a));
        assert_eq!(store.latest_for("product-b"), Some(product_b));
        assert_eq!(store.latest_for("product-c"), None);
    }

    #[test]
    fn product_watchers_poll_independent_repositories() {
        let root = tempfile::tempdir().unwrap();
        let product_a = root.path().join("product-a");
        let product_b = root.path().join("product-b");
        init_repository(&product_a, "a.txt");
        init_repository(&product_b, "b.txt");

        let mut watchers = ProductCommitWatchers::default();
        watchers
            .reconcile_and_poll([
                ("product-a", product_a.as_path()),
                ("product-b", product_b.as_path()),
            ])
            .unwrap();
        assert_eq!(watchers.latest_for("product-a"), None);
        assert_eq!(watchers.latest_for("product-b"), None);

        std::fs::write(product_a.join("a.txt"), "second\n").unwrap();
        git(&product_a, &["add", "a.txt"]);
        git(&product_a, &["commit", "-m", "second"]);
        watchers
            .reconcile_and_poll([
                ("product-a", product_a.as_path()),
                ("product-b", product_b.as_path()),
            ])
            .unwrap();

        let observed_a = watchers.latest_for("product-a").unwrap();
        assert_eq!(observed_a.touched_scope.files, vec!["a.txt"]);
        assert_eq!(watchers.latest_for("product-b"), None);
    }

    #[test]
    fn durable_product_watcher_state_detects_offline_changes_after_restart() {
        let root = tempfile::tempdir().unwrap();
        let state_root = root.path().join("watcher-state");
        let product = root.path().join("product-a");
        init_repository(&product, "a.txt");

        let store = ProductCommitWatcherStore::open(&state_root).unwrap();
        let mut watchers = ProductCommitWatchers::with_store(store);
        watchers
            .reconcile_and_poll([ProductWatcherRegistration::enabled(
                "product-a",
                product.as_path(),
            )])
            .unwrap();

        let baseline = watchers.state_for("product-a").unwrap();
        assert_eq!(baseline.lifecycle, ProductWatcherLifecycle::Current);
        assert!(baseline.queued_work.is_empty());
        drop(watchers);

        std::fs::write(product.join("a.txt"), "offline change\n").unwrap();
        git(&product, &["add", "a.txt"]);
        git(&product, &["commit", "-m", "offline change"]);

        let store = ProductCommitWatcherStore::open(&state_root).unwrap();
        let mut restarted = ProductCommitWatchers::with_store(store);
        restarted
            .reconcile_and_poll([ProductWatcherRegistration::enabled(
                "product-a",
                product.as_path(),
            )])
            .unwrap();

        let restored = restarted.state_for("product-a").unwrap();
        assert_eq!(restored.lifecycle, ProductWatcherLifecycle::Stale);
        assert_eq!(restored.queued_work.len(), 1);
        assert_eq!(restored.queued_work[0].touched_scope.files, vec!["a.txt"]);
        assert_eq!(
            restarted.latest_for("product-a"),
            restored.queued_work.last().cloned()
        );
    }

    #[test]
    fn failed_and_disabled_products_do_not_mutate_another_products_state() {
        let root = tempfile::tempdir().unwrap();
        let state_root = root.path().join("watcher-state");
        let product_a = root.path().join("product-a");
        let product_b = root.path().join("product-b");
        init_repository(&product_a, "a.txt");
        init_repository(&product_b, "b.txt");

        let store = ProductCommitWatcherStore::open(&state_root).unwrap();
        let mut watchers = ProductCommitWatchers::with_store(store);
        watchers
            .reconcile_and_poll([
                ProductWatcherRegistration::enabled("product-a", product_a.as_path()),
                ProductWatcherRegistration::enabled("product-b", product_b.as_path()),
            ])
            .unwrap();

        std::fs::remove_dir_all(&product_a).unwrap();
        std::fs::write(product_b.join("b.txt"), "second\n").unwrap();
        git(&product_b, &["add", "b.txt"]);
        git(&product_b, &["commit", "-m", "second"]);

        watchers
            .reconcile_and_poll([
                ProductWatcherRegistration::enabled("product-a", product_a.as_path()),
                ProductWatcherRegistration::enabled("product-b", product_b.as_path()),
            ])
            .unwrap();

        let failed_a = watchers.state_for("product-a").unwrap();
        let stale_b = watchers.state_for("product-b").unwrap();
        assert_eq!(failed_a.lifecycle, ProductWatcherLifecycle::Failed);
        assert_eq!(failed_a.failure_backlog.len(), 1);
        assert_eq!(stale_b.lifecycle, ProductWatcherLifecycle::Stale);
        assert_eq!(stale_b.failure_backlog, Vec::<ProductWatcherFailure>::new());
        assert_eq!(stale_b.queued_work.len(), 1);

        watchers
            .reconcile_and_poll([
                ProductWatcherRegistration::disabled("product-a"),
                ProductWatcherRegistration::enabled("product-b", product_b.as_path()),
            ])
            .unwrap();

        let disabled_a = watchers.state_for("product-a").unwrap();
        let unchanged_b = watchers.state_for("product-b").unwrap();
        assert_eq!(disabled_a.lifecycle, ProductWatcherLifecycle::Disabled);
        assert_eq!(disabled_a.failure_backlog.len(), 1);
        assert_eq!(unchanged_b, stale_b);

        drop(watchers);
        let reopened = ProductCommitWatcherStore::open(&state_root).unwrap();
        assert_eq!(reopened.state_for("product-a"), Some(disabled_a));
        assert_eq!(reopened.state_for("product-b"), Some(stale_b));
    }

    fn init_repository(path: &Path, file_name: &str) {
        std::fs::create_dir_all(path).unwrap();
        git(path, &["init"]);
        git(path, &["config", "user.email", "test@bicameral.ai"]);
        git(path, &["config", "user.name", "Bicameral Test"]);
        std::fs::write(path.join(file_name), "first\n").unwrap();
        git(path, &["add", file_name]);
        git(path, &["commit", "-m", "first"]);
    }

    fn git(path: &Path, args: &[&str]) {
        let output = Command::new("git")
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
    }

    struct FakeGraphService {
        readiness: GraphReadiness,
    }

    impl GraphSnapshotService for FakeGraphService {
        fn resolve_snapshot(&self, requested_ref: &str) -> SnapshotResolution {
            SnapshotResolution::Resolved {
                snapshot: GraphSnapshot {
                    graph_snapshot_id: "snap-head".into(),
                    repo_id: "repo-1".into(),
                    commit_sha: requested_ref.into(),
                    index_version: "v1".into(),
                    ref_name: Some("HEAD".into()),
                    merge_base_sha: None,
                    indexed_at: None,
                    index_source: None,
                },
            }
        }

        fn resolve_head(
            &self,
        ) -> Result<GraphSnapshot, crate::graph_snapshot_service::GraphSnapshotServiceError>
        {
            Ok(GraphSnapshot {
                graph_snapshot_id: "snap-head".into(),
                repo_id: "repo-1".into(),
                commit_sha: "headsha".into(),
                index_version: "v1".into(),
                ref_name: Some("HEAD".into()),
                merge_base_sha: None,
                indexed_at: None,
                index_source: None,
            })
        }

        fn check_readiness(&self, _resolutions: &[&SnapshotResolution]) -> GraphReadiness {
            self.readiness
        }

        fn readiness_view(&self) -> GraphReadinessView {
            GraphReadinessView::from_readiness(self.readiness, None)
        }

        fn inspect_file(
            &self,
            _snapshot: &GraphSnapshot,
            _path: &str,
            _line_start: Option<u32>,
            _line_end: Option<u32>,
        ) -> FileInspectionResult {
            unreachable!("watcher freshness does not inspect files")
        }

        fn resolve_symbols(
            &self,
            _snapshot: &GraphSnapshot,
            _rel_path: &str,
        ) -> SymbolResolutionResult {
            unreachable!("watcher freshness does not resolve symbols")
        }

        fn evaluate_currentness(
            &self,
            _decision_id: Uuid,
            _binding: &BindingEvidence,
        ) -> CodeSourceRefreshResult {
            unreachable!("watcher freshness does not evaluate bindings")
        }

        fn evaluate_decision_currentness(
            &self,
            _decision_id: Uuid,
            _bindings: &[BindingEvidence],
        ) -> Vec<CodeSourceRefreshResult> {
            unreachable!("watcher freshness does not evaluate bindings")
        }

        fn currentness_for_readiness(&self, _readiness: GraphReadiness) -> CodeSourceCurrentness {
            CodeSourceCurrentness::GraphUnavailable
        }
    }

    fn query(files: &[&str]) -> PreflightQuery {
        PreflightQuery {
            files: files.iter().map(|s| s.to_string()).collect(),
            symbols: Vec::new(),
            diff_context: None,
            branch: None,
        }
    }

    #[test]
    fn touched_file_from_watcher_downgrades_without_caller_diff() {
        let svc = FakeGraphService {
            readiness: GraphReadiness::Ready,
        };
        let observation =
            CommitWatcherObservation::new("old".into(), "new".into(), vec!["src/a.rs".into()]);

        let freshness = compute_watcher_preflight_freshness(
            &query(&["src/a.rs"]),
            Some(&svc),
            false,
            Some(&observation),
        )
        .expect("watcher freshness");

        assert_eq!(freshness.commit_freshness, CommitFreshness::TouchedStale);
        assert_eq!(freshness.currentness, CodeSourceCurrentness::ContentChanged);
        assert_eq!(freshness.evidence_state, GraphEvidenceState::UnknownStale);
        assert_eq!(freshness.touched_scope.files, vec!["src/a.rs"]);
    }

    #[test]
    fn untouched_file_stays_snapshot_scoped() {
        let svc = FakeGraphService {
            readiness: GraphReadiness::Stale,
        };
        let observation =
            CommitWatcherObservation::new("old".into(), "new".into(), vec!["src/a.rs".into()]);

        let freshness = compute_watcher_preflight_freshness(
            &query(&["src/b.rs"]),
            Some(&svc),
            false,
            Some(&observation),
        )
        .expect("watcher freshness");

        assert_eq!(freshness.commit_freshness, CommitFreshness::SnapshotScoped);
        assert_eq!(
            freshness.caveats[0].code,
            FreshnessCaveatCode::SnapshotScopedEvidence
        );
    }

    #[test]
    fn over_budget_commit_is_unknown_stale() {
        let svc = FakeGraphService {
            readiness: GraphReadiness::Ready,
        };
        let files = (0..=BOUNDED_SCOPE_BUDGET)
            .map(|i| format!("src/{i}.rs"))
            .collect();
        let observation = CommitWatcherObservation::new("old".into(), "new".into(), files);

        let freshness = compute_watcher_preflight_freshness(
            &query(&["src/1.rs"]),
            Some(&svc),
            false,
            Some(&observation),
        )
        .expect("watcher freshness");

        assert_eq!(freshness.commit_freshness, CommitFreshness::UnknownStale);
        assert_eq!(freshness.refresh_outcome, RefreshOutcome::OverBudget);
    }

    #[test]
    fn absent_watcher_observation_falls_back_to_existing_path() {
        let svc = FakeGraphService {
            readiness: GraphReadiness::Ready,
        };

        assert!(compute_watcher_preflight_freshness(
            &query(&["src/a.rs"]),
            Some(&svc),
            false,
            None
        )
        .is_none());
    }
}
