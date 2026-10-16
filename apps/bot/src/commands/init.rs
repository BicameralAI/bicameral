use bicameral_config::{BicameralConfig, WorkspaceDiscovery};
use std::path::Path;

pub async fn run(path: &Path) -> anyhow::Result<()> {
    let abs_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

    if abs_path.join(".bicameral").exists() {
        println!("Workspace already initialized at {}", abs_path.display());
        return Ok(());
    }

    let config = BicameralConfig::default();
    let paths = WorkspaceDiscovery::init(&abs_path, &config)?;

    println!("Initialized Bicameral workspace at {}", abs_path.display());
    println!("  Config: {}", paths.config_file.display());
    println!("  Inbox:  {}", paths.candidates_inbox_dir.display());
    println!("  Audit:  {}", paths.audit_dir.display());
    println!("  Mods:   {}", paths.mods_dir.display());

    Ok(())
}
