use bicameral_runtime::update::{self, UpdateCheck};

pub async fn run(check_only: bool) -> anyhow::Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    println!("Current version: {}", current_version);
    println!("Checking for updates...");

    let check = update::check_latest(current_version).await?;

    match check {
        UpdateCheck::UpToDate { current } => {
            println!("Already up to date (v{}).", current);
        }
        UpdateCheck::Skipped { reason } => {
            println!("Update check skipped: {}", reason);
        }
        UpdateCheck::Available(release) => {
            println!(
                "New version available: v{} (current: v{})",
                release.version, current_version
            );

            if check_only {
                println!();
                println!("Run `bicameral update` to install.");
                return Ok(());
            }

            println!();
            println!("Downloading {}...", release.asset_name);
            let verified_release = update::download_and_verify(&release).await?;

            let current_binary = std::env::current_exe()?;
            println!(
                "Installing binary and dashboard assets at {}...",
                current_binary.display()
            );
            let backup = update::replace_binary(&current_binary, &verified_release)?;

            println!();
            println!("Updated to v{}.", release.version);
            println!("Previous version backed up to: {}", backup.display());
            println!();
            println!("Verification: SHA256 checksum verified against release SHA256SUMS.");
            println!(
                "For full provenance verification, run: gh attestation verify {}",
                current_binary.display()
            );
        }
    }

    Ok(())
}
