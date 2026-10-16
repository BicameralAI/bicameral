//! Path resolution for Bicameral configuration and data directories.

use std::path::{Path, PathBuf};

/// Resolved paths for a Bicameral workspace.
#[derive(Debug, Clone)]
pub struct BicameralPaths {
    /// Root of the workspace (where `.bicameral/` lives).
    pub workspace_root: PathBuf,
    /// The `.bicameral/` directory itself.
    pub dot_bicameral: PathBuf,
    /// Config file path: `.bicameral/config.yaml`
    pub config_file: PathBuf,
    /// Decisions directory: `.bicameral/decisions/`
    pub decisions_dir: PathBuf,
    /// Candidates inbox: `.bicameral/candidates-inbox/`
    /// Non-canonical pending candidates awaiting governance review.
    /// This is NOT the canonical event store — only `EventStoreAdapter::append()`
    /// materializes accepted governance events.
    pub candidates_inbox_dir: PathBuf,
    /// Audit receipts directory: `.bicameral/audit/`
    pub audit_dir: PathBuf,
    /// Mod manifests directory: `.bicameral/mods/`
    pub mods_dir: PathBuf,
    /// Factory attestations directory: `.bicameral/factory-attestations/`
    pub attestations_dir: PathBuf,
    /// Event-store directory: `.bicameral/events/`
    pub events_dir: PathBuf,
    /// Retained source snapshots/evidence: `.bicameral/sources/`
    pub sources_dir: PathBuf,
}

impl BicameralPaths {
    /// Construct paths from a workspace root.
    pub fn from_workspace_root(root: impl AsRef<Path>) -> Self {
        let workspace_root = root.as_ref().to_path_buf();
        let dot_bicameral = workspace_root.join(".bicameral");
        Self {
            config_file: dot_bicameral.join("config.yaml"),
            decisions_dir: dot_bicameral.join("decisions"),
            candidates_inbox_dir: dot_bicameral.join("candidates-inbox"),
            audit_dir: dot_bicameral.join("audit"),
            mods_dir: dot_bicameral.join("mods"),
            attestations_dir: dot_bicameral.join("factory-attestations"),
            events_dir: dot_bicameral.join("events"),
            sources_dir: dot_bicameral.join("sources"),
            dot_bicameral,
            workspace_root,
        }
    }

    /// User-level config directory (`~/.config/bicameral/`).
    pub fn user_config_dir() -> Option<PathBuf> {
        dirs_path().map(|d| d.join("bicameral"))
    }

    /// Operator home directory (`~/.bicameral/`).
    ///
    /// This is the user-level Bicameral control plane. It manages
    /// operator-private state including project registrations,
    /// connection metadata, and local runtime caches.
    pub fn operator_home() -> Option<PathBuf> {
        home_dir().map(|h| h.join(".bicameral"))
    }

    /// Operator-local per-project workspace-binding store directory
    /// (`~/.bicameral/workspace-bindings/`).
    ///
    /// Holds durable `LocalWorkspaceBinding` records materialized by the daemon
    /// (bicameral-bot#732). This is operator-private local state: it is never
    /// cloud-owned, never crosses the hosted bridge, and is kept out of any
    /// bound workspace's own `.bicameral/` tree.
    pub fn workspace_bindings_dir() -> Option<PathBuf> {
        Self::operator_home().map(|h| h.join("workspace-bindings"))
    }

    /// Operator-local Product registration and routing directory
    /// (`~/.bicameral/projects/`).
    ///
    /// Records here locate canonical Product event stores. They belong to the
    /// operator control plane and must never be written into a Product's own
    /// `.bicameral/events/` ledger.
    pub fn project_registry_dir() -> Option<PathBuf> {
        Self::operator_home().map(|h| h.join("projects"))
    }

    /// Redacted, bounded daemon operational diagnostics
    /// (`~/.bicameral/operational-logs/`).
    ///
    /// These receipts are operator-private troubleshooting state, not
    /// governance authority and not hosted telemetry.
    pub fn operational_logs_dir() -> Option<PathBuf> {
        Self::operator_home().map(|h| h.join("operational-logs"))
    }

    /// Daemon-owned, operator-scoped capability readiness.
    ///
    /// This state exists independently of any Product workspace or dashboard
    /// pairing and must never be written to a Product event store.
    pub fn capability_readiness_dir() -> Option<PathBuf> {
        Self::operator_home().map(|h| h.join("capability-readiness"))
    }

    /// Durable operator-local watcher checkpoints and freshness state, keyed
    /// by canonical Product id.
    pub fn product_watcher_state_dir() -> Option<PathBuf> {
        Self::operator_home().map(|h| h.join("product-watchers"))
    }

    /// Default path for the user-level connection metadata registry.
    ///
    /// Returns `~/.bicameral/connections.yaml`. Connection metadata
    /// stores provider identity, status, capabilities, and timestamps.
    /// Secret material (tokens, keys) belongs in `secrets_store`.
    pub fn connections_file() -> Option<PathBuf> {
        Self::operator_home().map(|h| h.join("connections.yaml"))
    }

    /// Operator-local `product.join` trust anchors
    /// (`~/.bicameral/product-join-trust.json`).
    ///
    /// Public verifying keys + issuer only — never private signing material.
    /// Loaded by the operator-scoped daemon; bootstrapped from Cloud
    /// verification metadata when absent (bot#1012).
    pub fn product_join_trust_file() -> Option<PathBuf> {
        Self::operator_home().map(|h| h.join("product-join-trust.json"))
    }

    /// Ensure all required directories exist.
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dot_bicameral)?;
        std::fs::create_dir_all(&self.decisions_dir)?;
        std::fs::create_dir_all(&self.candidates_inbox_dir)?;
        std::fs::create_dir_all(&self.audit_dir)?;
        std::fs::create_dir_all(&self.mods_dir)?;
        std::fs::create_dir_all(&self.attestations_dir)?;
        std::fs::create_dir_all(&self.events_dir)?;
        std::fs::create_dir_all(&self.sources_dir)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operator_home_is_under_user_home() {
        let home = BicameralPaths::operator_home();
        assert!(home.is_some(), "operator_home should resolve on test hosts");
        let home = home.unwrap();
        assert!(
            home.ends_with(".bicameral"),
            "operator home should end with .bicameral"
        );
    }

    #[test]
    fn connections_file_is_under_operator_home() {
        let conn = BicameralPaths::connections_file();
        assert!(conn.is_some());
        let conn = conn.unwrap();
        assert!(
            conn.ends_with("connections.yaml"),
            "connections file should be connections.yaml"
        );
        let parent = conn.parent().unwrap();
        assert!(
            parent.ends_with(".bicameral"),
            "connections file should live under operator home"
        );
    }

    #[test]
    fn project_registry_is_under_operator_home() {
        let registry = BicameralPaths::project_registry_dir().unwrap();
        assert!(registry.ends_with(".bicameral/projects"));
    }

    #[test]
    fn operational_logs_are_under_operator_home() {
        let logs = BicameralPaths::operational_logs_dir().unwrap();
        assert!(logs.ends_with(".bicameral/operational-logs"));
    }

    #[test]
    fn inbox_dir_is_not_named_events() {
        let paths = BicameralPaths::from_workspace_root("/tmp/test-ws");
        let inbox_name = paths
            .candidates_inbox_dir
            .file_name()
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(
            inbox_name, "candidates-inbox",
            "Inbox directory must be named 'candidates-inbox', not 'events'"
        );
        assert!(
            !inbox_name.contains("event"),
            "Inbox directory name must not contain 'event' to avoid confusion with canonical event store"
        );
    }

    #[test]
    fn capability_readiness_dir_is_operator_scoped() {
        let readiness = BicameralPaths::capability_readiness_dir().unwrap();
        assert!(readiness.ends_with(".bicameral/capability-readiness"));
    }

    #[test]
    fn product_watcher_state_dir_is_operator_scoped() {
        let watcher_state = BicameralPaths::product_watcher_state_dir().unwrap();
        assert!(watcher_state.ends_with(".bicameral/product-watchers"));
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from).or({
        #[cfg(target_os = "windows")]
        {
            std::env::var("USERPROFILE").ok().map(PathBuf::from)
        }
        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    })
}

fn dirs_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(h).join(".config"))
    }
    #[cfg(target_os = "linux")]
    {
        std::env::var("XDG_CONFIG_HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| PathBuf::from(h).join(".config"))
            })
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA").ok().map(PathBuf::from)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        None
    }
}
