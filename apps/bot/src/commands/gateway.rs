use bicameral_config::{BicameralConfig, BicameralPaths, WorkspaceDiscovery};
use bicameral_runtime::daemon::{run as run_daemon, run_operator, DaemonConfig};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
enum GatewayStartupScope {
    OperatorRegistry,
    ExplicitWorkspace(PathBuf),
}

const GATEWAY_PORT_ENV: &str = "BICAMERAL_GATEWAY_PORT";

fn resolve_startup_scope(workspace: Option<&Path>) -> GatewayStartupScope {
    match workspace {
        Some(path) => GatewayStartupScope::ExplicitWorkspace(path.to_path_buf()),
        None => GatewayStartupScope::OperatorRegistry,
    }
}

pub async fn start(workspace: Option<&Path>) -> anyhow::Result<()> {
    match resolve_startup_scope(workspace) {
        GatewayStartupScope::OperatorRegistry => {
            run_operator_daemon(operator_config_from_port(
                std::env::var(GATEWAY_PORT_ENV).ok().as_deref(),
            )?)
            .await
        }
        GatewayStartupScope::ExplicitWorkspace(start) => {
            let (paths, config) = WorkspaceDiscovery::discover(&start)?;
            run_workspace_daemon(paths, config).await
        }
    }
}

fn operator_config_from_port(value: Option<&str>) -> anyhow::Result<BicameralConfig> {
    let mut config = BicameralConfig::default();
    let Some(value) = value else {
        return Ok(config);
    };
    let port = value
        .parse::<u16>()
        .map_err(|_| anyhow::anyhow!("{GATEWAY_PORT_ENV} must be a non-zero u16"))?;
    if port == 0 {
        anyhow::bail!("{GATEWAY_PORT_ENV} must be a non-zero u16");
    }
    config.gateway.port = port;
    Ok(config)
}

async fn run_operator_daemon(config: BicameralConfig) -> anyhow::Result<()> {
    println!(
        "Starting operator-scoped Bicameral daemon/gateway at {}:{}",
        config.gateway.host, config.gateway.port
    );
    let pairing_watcher = spawn_pairing_watcher(&config);
    let exit = run_operator(config.gateway).await?;
    if let Some(watcher) = pairing_watcher {
        watcher.abort();
    }
    println!("Daemon exited: {:?}", exit);
    Ok(())
}

async fn run_workspace_daemon(
    paths: BicameralPaths,
    config: BicameralConfig,
) -> anyhow::Result<()> {
    println!(
        "Starting explicit-workspace Bicameral daemon/gateway at {}:{}",
        config.gateway.host, config.gateway.port
    );
    let pairing_watcher = spawn_pairing_watcher(&config);
    let exit = run_daemon(DaemonConfig { paths, config }).await?;
    if let Some(watcher) = pairing_watcher {
        watcher.abort();
    }
    println!("Daemon exited: {:?}", exit);
    Ok(())
}

fn spawn_pairing_watcher(config: &BicameralConfig) -> Option<tokio::task::JoinHandle<()>> {
    let display_host = if config.gateway.host == "::1" {
        "[::1]".to_string()
    } else {
        config.gateway.host.clone()
    };
    let base_url = format!("http://{}:{}", display_host, config.gateway.port);
    super::pairing::spawn_foreground_watcher(base_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateway_without_explicit_workspace_starts_operator_daemon() {
        assert_eq!(
            resolve_startup_scope(None),
            GatewayStartupScope::OperatorRegistry
        );
    }

    #[test]
    fn operator_service_may_bind_an_isolated_loopback_port() {
        let config = operator_config_from_port(Some("41042")).unwrap();
        assert_eq!(config.gateway.port, 41_042);
        assert!(operator_config_from_port(Some("0")).is_err());
        assert!(operator_config_from_port(Some("not-a-port")).is_err());
    }
}
