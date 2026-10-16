//! Workspace discovery — find the nearest `.bicameral/` directory.

use crate::paths::BicameralPaths;
use crate::schema::BicameralConfig;
use std::path::{Path, PathBuf};

/// Workspace discovery utilities.
pub struct WorkspaceDiscovery;

impl WorkspaceDiscovery {
    /// Walk up from `start` to find the nearest directory containing `.bicameral/`.
    pub fn find_workspace_root(start: &Path) -> Option<PathBuf> {
        let mut current = start.to_path_buf();
        loop {
            if current.join(".bicameral").is_dir() {
                return Some(current);
            }
            if !current.pop() {
                return None;
            }
        }
    }

    /// Discover paths and load config from the nearest workspace.
    pub fn discover(start: &Path) -> anyhow::Result<(BicameralPaths, BicameralConfig)> {
        let root = Self::find_workspace_root(start)
            .ok_or_else(|| anyhow::anyhow!("No .bicameral/ workspace found from {:?}", start))?;

        let paths = BicameralPaths::from_workspace_root(&root);

        let config = if paths.config_file.exists() {
            BicameralConfig::load_from_file(&paths.config_file)?
        } else {
            BicameralConfig::default()
        };

        Ok((paths, config))
    }

    /// Initialize a new workspace at the given path.
    pub fn init(root: &Path, config: &BicameralConfig) -> anyhow::Result<BicameralPaths> {
        let paths = BicameralPaths::from_workspace_root(root);
        paths.ensure_dirs()?;
        config.save_to_file(&paths.config_file)?;
        Ok(paths)
    }
}
