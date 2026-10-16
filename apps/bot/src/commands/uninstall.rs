use bicameral_config::paths::BicameralPaths;
use bicameral_runtime::service;
use std::path::PathBuf;

pub async fn run(remove_all: bool) -> anyhow::Result<()> {
    println!("Bicameral Uninstall");
    println!("===================");
    println!();

    let mut removed: Vec<String> = Vec::new();
    let mut preserved: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // 1. Stop and remove the daemon service (idempotent).
    match service::uninstall() {
        Ok(result) => {
            if result.service_was_running {
                removed.push("Stopped running daemon service".into());
            }
            if result.config_removed {
                if let Some(path) = result.config_path {
                    removed.push(format!("Removed service config: {}", path.display()));
                }
            } else {
                // No config found — that's fine, idempotent
            }
        }
        Err(e) => {
            warnings.push(format!("Could not fully remove service config: {}", e));
        }
    }

    // 2. Remove the installed binary.
    match std::env::current_exe() {
        Ok(binary_path) => {
            // Also check for a .old backup from a previous update
            let backup_path = binary_path.with_extension("old");
            if backup_path.exists() {
                std::fs::remove_file(&backup_path).ok();
                removed.push(format!("Removed backup binary: {}", backup_path.display()));
            }

            if binary_path.exists() {
                match std::fs::remove_file(&binary_path) {
                    Ok(()) => {
                        removed.push(format!("Removed binary: {}", binary_path.display()));
                    }
                    Err(e) => {
                        warnings.push(format!(
                            "Could not remove binary at {}: {} (remove manually)",
                            binary_path.display(),
                            e
                        ));
                    }
                }
            }
        }
        Err(e) => {
            warnings.push(format!("Could not determine binary path: {}", e));
        }
    }

    // 3. Remove user config directory (~/.config/bicameral/).
    if let Some(user_config) = BicameralPaths::user_config_dir() {
        if remove_all && user_config.exists() {
            match std::fs::remove_dir_all(&user_config) {
                Ok(()) => {
                    removed.push(format!("Removed user config: {}", user_config.display()));
                }
                Err(e) => {
                    warnings.push(format!(
                        "Could not remove user config at {}: {}",
                        user_config.display(),
                        e
                    ));
                }
            }
        } else if user_config.exists() {
            preserved.push(format!("User config: {}", user_config.display()));
        }
    }

    // 4. Handle ~/.bicameral/ (global decision data).
    let home_bicameral = home_bicameral_dir();
    if let Some(ref data_dir) = home_bicameral {
        if remove_all && data_dir.exists() {
            match std::fs::remove_dir_all(data_dir) {
                Ok(()) => {
                    removed.push(format!("Removed data directory: {}", data_dir.display()));
                }
                Err(e) => {
                    warnings.push(format!(
                        "Could not remove data directory at {}: {}",
                        data_dir.display(),
                        e
                    ));
                }
            }
        } else if data_dir.exists() {
            preserved.push(format!("Data directory: {}", data_dir.display()));
        }
    }

    // 5. Note: workspace .bicameral/ directories are per-project and are
    //    always preserved. They belong to the project, not the install.
    preserved.push("Per-project .bicameral/ workspace directories (not touched)".into());

    // Print summary
    println!("Removed:");
    if removed.is_empty() {
        println!("  (nothing to remove)");
    } else {
        for item in &removed {
            println!("  - {}", item);
        }
    }

    println!();
    println!("Preserved:");
    for item in &preserved {
        println!("  - {}", item);
    }

    if !warnings.is_empty() {
        println!();
        println!("Warnings:");
        for w in &warnings {
            println!("  ! {}", w);
        }
    }

    if !remove_all {
        println!();
        println!("Decision data and user config were preserved.");
        println!("To remove everything: bicameral uninstall --all");
    }

    Ok(())
}

fn home_bicameral_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".bicameral"))
}
