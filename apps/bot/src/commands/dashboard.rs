use bicameral_api::event_store::SubstrateKind;
use bicameral_config::{BicameralConfig, WorkspaceDiscovery};
use bicameral_runtime::daemon::{run, run_setup_mode, DaemonConfig};
use std::path::Path;

pub async fn start(workspace: Option<&Path>) -> anyhow::Result<()> {
    let start = workspace
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap());

    match WorkspaceDiscovery::discover(&start) {
        Ok((paths, config)) => {
            if dashboard_needs_setup(&config) {
                println!("Bicameral workspace needs event-store setup.");
                run_dashboard_setup_mode(&start).await?;
                return Ok(());
            }

            let url = format!("http://{}:{}", config.gateway.host, config.gateway.port);
            println!("Starting Bicameral daemon API at {url}");
            println!("Dashboard UI is served by Bicameral Cloud, not this daemon.");

            let daemon_config = DaemonConfig { paths, config };
            let exit = run(daemon_config).await?;

            println!("Daemon exited: {exit:?}");
        }
        Err(_) => {
            // Setup mode: no workspace found
            println!("No .bicameral/ workspace found.");
            run_dashboard_setup_mode(&start).await?;
        }
    }

    Ok(())
}

fn dashboard_needs_setup(config: &BicameralConfig) -> bool {
    matches!(config.event_store.substrate, SubstrateKind::DriveFolder)
        && config.event_store.drive_folder_path.is_none()
}

async fn run_dashboard_setup_mode(start: &Path) -> anyhow::Result<()> {
    let config = BicameralConfig::default();
    let url = format!("http://{}:{}", config.gateway.host, config.gateway.port);
    println!("Starting Bicameral daemon API in setup mode at {url}");
    println!("Dashboard UI is served by Bicameral Cloud, not this daemon.");

    let exit = run_setup_mode(&config.gateway, start).await?;
    println!("Daemon exited: {exit:?}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_folder_without_path_routes_dashboard_to_setup() {
        let mut config = BicameralConfig::default();
        config.event_store.substrate = SubstrateKind::DriveFolder;
        config.event_store.drive_folder_path = None;

        assert!(dashboard_needs_setup(&config));
    }

    #[test]
    fn drive_folder_with_path_can_start_normally() {
        let mut config = BicameralConfig::default();
        config.event_store.substrate = SubstrateKind::DriveFolder;
        config.event_store.drive_folder_path = Some("/tmp/bicameral-events".into());

        assert!(!dashboard_needs_setup(&config));
    }
}
