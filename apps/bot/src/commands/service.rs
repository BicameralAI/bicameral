use bicameral_runtime::service;
use std::env;

pub async fn install() -> anyhow::Result<()> {
    let binary_path = env::current_exe()?;
    service::install(&binary_path)?;
    println!("Bicameral daemon service installed.");
    println!("Start with: bicameral service start");
    Ok(())
}

pub async fn start() -> anyhow::Result<()> {
    service::start()?;
    println!("Bicameral daemon started.");
    Ok(())
}

pub async fn stop() -> anyhow::Result<()> {
    service::stop()?;
    println!("Bicameral daemon stopped.");
    Ok(())
}

pub async fn status() -> anyhow::Result<()> {
    let status = service::status();
    println!("Bicameral daemon: {}", status);
    Ok(())
}

pub async fn uninstall() -> anyhow::Result<()> {
    let result = service::uninstall()?;

    if result.service_was_running {
        println!("Stopped running daemon service.");
    }
    if result.config_removed {
        if let Some(path) = result.config_path {
            println!("Removed service configuration: {}", path.display());
        } else {
            println!("Removed service configuration.");
        }
    } else {
        println!("No service configuration found (already uninstalled).");
    }

    Ok(())
}
