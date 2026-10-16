use bicameral_api::event_store::{EventStoreAdapter, SubstrateKind};
use bicameral_config::{GatewayConfig, WorkspaceDiscovery};
use bicameral_runtime::service;
use std::net::ToSocketAddrs;
use std::path::Path;

pub async fn run(workspace: Option<&Path>) -> anyhow::Result<()> {
    println!("Bicameral Doctor");
    println!("================");
    println!();

    // Check installed binary
    check_binary();

    // Check workspace/config paths
    let start = workspace
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap());
    let mut gateway_config = GatewayConfig::default();

    match WorkspaceDiscovery::discover(&start) {
        Ok((paths, config)) => {
            gateway_config = config.gateway.clone();
            println!("[ok] Workspace found: {}", paths.workspace_root.display());
            println!("     Config: {}", paths.config_file.display());
            println!();

            // Substrate kind and capability summary
            let substrate = config.event_store.substrate;
            println!("Event Store Substrate:");
            println!("  Kind: {}", substrate_display_name(substrate));
            println!("  Purpose: {}", substrate_purpose(substrate));
            println!();

            println!("  Baseline contract (all substrates):");
            println!("    - Durable append of accepted governance events");
            println!("    - Deterministic replay into materialized state");
            println!("    - Health/freshness reporting");
            println!();

            match substrate {
                SubstrateKind::Git => {
                    println!("  Capabilities (Git substrate):");
                    println!("    [ok] Branch-local proposals");
                    println!("    [ok] PR-native review");
                    println!("    [ok] Native merge conflict resolution");
                    if config.event_store.ci_merge_gate {
                        println!("    [ok] CI/merge-gate enforcement (configured)");
                    } else {
                        println!("    [--] CI/merge-gate enforcement (not configured)");
                        println!("         Set event_store.ci_merge_gate: true when branch");
                        println!("         protection with required checks is enabled.");
                    }
                }
                SubstrateKind::DriveFolder => {
                    println!("  Capabilities (Drive/basic-store substrate):");
                    println!("    [ok] Durable replay");
                    println!("    [--] Branch-local proposals: not available");
                    println!("    [--] PR-native review: not available");
                    println!("    [--] Native merge conflict resolution: not available");
                    println!("    [--] CI/merge-gate enforcement: not available");
                    println!();
                    if let Some(ref folder_path) = config.event_store.drive_folder_path {
                        println!("  Folder: {}", folder_path.display());
                        let adapter =
                            bicameral_runtime::event_store_adapters::DriveFolderAdapter::new(
                                folder_path,
                            );
                        let freshness = adapter.freshness();
                        println!("  Freshness: {:?}", freshness);
                        if let Ok(health) = adapter.health_check() {
                            if !health.warnings.is_empty() {
                                for w in &health.warnings {
                                    println!("  [!!] {}", w);
                                }
                            }
                        }
                        println!();
                    }
                    println!("  Note: Drive substrates satisfy the baseline Decision Ledger");
                    println!("  contract. Governance enforcement surfaces as dashboard warnings");
                    println!("  or paused review states, not CI blocking.");
                    println!();
                    println!("  To join a team on the same Drive-synced folder:");
                    println!("    1. Share a Google Drive folder with your team");
                    println!("    2. Sync it locally (Google Drive Desktop / rclone)");
                    println!("    3. Set event_store.drive_folder_path in .bicameral/config.yaml");
                    println!("    See docs/DRIVE-FOLDER-SETUP.md for details.");
                }
                SubstrateKind::Memory => {
                    println!("  Capabilities (in-memory, testing only):");
                    println!("    [--] No durable capabilities (ephemeral)");
                }
            }
        }
        Err(_) => {
            println!("[!!] No workspace found. Run `bicameral init` to create one.");
        }
    }

    println!();

    // Check service status
    let status = service::status();
    match status {
        service::ServiceStatus::Running => println!("[ok] Daemon service: running"),
        service::ServiceStatus::Stopped => println!("[--] Daemon service: not running"),
        service::ServiceStatus::Unknown => println!("[??] Daemon service: unknown"),
    }

    // Check gateway reachability using the same configured host/port contract as startup.
    check_gateway(&gateway_config).await;

    // Service config
    match service::service_config_path() {
        Some(path) => println!("[ok] Service config: {}", path.display()),
        None => println!("[--] Service config: not installed"),
    }

    // Environment
    println!();
    println!("Environment:");
    println!("  bicameral version: {}", env!("CARGO_PKG_VERSION"));

    Ok(())
}

fn substrate_display_name(kind: SubstrateKind) -> &'static str {
    match kind {
        SubstrateKind::Git => "Git (repository-backed event store)",
        SubstrateKind::DriveFolder => "Drive/basic-store (synced folder)",
        SubstrateKind::Memory => "Memory (ephemeral, testing only)",
    }
}

fn substrate_purpose(kind: SubstrateKind) -> &'static str {
    match kind {
        SubstrateKind::Git => "Extends baseline Decision Ledger with PR-resolution capabilities",
        SubstrateKind::DriveFolder => {
            "Satisfies baseline Decision Ledger contract via shared file store"
        }
        SubstrateKind::Memory => "Ephemeral store for testing; no durable guarantees",
    }
}

fn check_binary() {
    match std::env::current_exe() {
        Ok(path) => {
            if path.exists() {
                println!("[ok] Binary: {}", path.display());
            } else {
                println!(
                    "[!!] Binary path resolved but file missing: {}",
                    path.display()
                );
            }
        }
        Err(e) => {
            println!("[!!] Cannot resolve binary path: {}", e);
        }
    }

    // Check if bicameral is on PATH
    match which_bicameral() {
        Some(p) => println!("[ok] On PATH: {}", p.display()),
        None => println!("[--] bicameral not found on PATH"),
    }
}

fn which_bicameral() -> Option<std::path::PathBuf> {
    let path_var = std::env::var("PATH").ok()?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join("bicameral");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn gateway_health_endpoint(config: &GatewayConfig) -> String {
    let host = if config.host == "::1" {
        "[::1]".to_string()
    } else {
        config.host.clone()
    };
    format!("http://{}:{}/health", host, config.port)
}

async fn check_gateway(config: &GatewayConfig) {
    let gateway_url = gateway_health_endpoint(config);
    println!();
    print!("Gateway reachability ({})... ", gateway_url);

    let reachable = (config.host.as_str(), config.port)
        .to_socket_addrs()
        .ok()
        .into_iter()
        .flatten()
        .any(|addr| {
            std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(2)).is_ok()
        });

    if reachable {
        println!("[ok] reachable");
    } else {
        println!("[--] not reachable (daemon may not be running)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateway_health_endpoint_uses_canonical_default_port() {
        let config = GatewayConfig::default();
        assert_eq!(
            gateway_health_endpoint(&config),
            "http://127.0.0.1:37373/health"
        );
    }

    #[test]
    fn gateway_health_endpoint_uses_custom_configured_port() {
        let config = GatewayConfig {
            host: "localhost".to_string(),
            port: 40123,
        };
        assert_eq!(
            gateway_health_endpoint(&config),
            "http://localhost:40123/health"
        );
    }

    #[test]
    fn gateway_health_endpoint_formats_ipv6_loopback() {
        let config = GatewayConfig {
            host: "::1".to_string(),
            port: 37373,
        };
        assert_eq!(
            gateway_health_endpoint(&config),
            "http://[::1]:37373/health"
        );
    }
}
