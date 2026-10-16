use bicameral_runtime::service;
use std::env;

pub async fn run(install_daemon: bool) -> anyhow::Result<()> {
    println!("Bicameral Onboard");
    println!("=================");
    println!();

    // Resolve the binary path (the currently running executable).
    let binary_path = env::current_exe()?;
    println!("Binary: {}", binary_path.display());

    if install_daemon {
        println!();
        println!("Installing daemon service...");
        service::install(&binary_path)?;
        println!("[ok] Daemon service installed.");
        println!();
        println!("Start the daemon with: bicameral service start");
    } else {
        println!();
        println!("Run with --install-daemon to install the background service.");
    }

    Ok(())
}
