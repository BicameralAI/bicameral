//! Service installation and status checking.
//!
//! Supports systemd user units on Linux and launchd on macOS.
//! Adapted from ZeroClaw's service module.

use std::path::Path;

const SERVICE_LABEL: &str = "com.bicameral.daemon";
const SERVICE_UNIT: &str = "bicameral";
const SERVICE_INSTANCE_ENV: &str = "BICAMERAL_SERVICE_INSTANCE";
const GATEWAY_PORT_ENV: &str = "BICAMERAL_GATEWAY_PORT";

#[derive(Debug, Clone, PartialEq, Eq)]
struct ServiceIdentity {
    launchd_label: String,
    systemd_unit: String,
}

fn service_identity_from_value(value: Option<&str>) -> anyhow::Result<ServiceIdentity> {
    let Some(instance) = value.filter(|value| !value.is_empty()) else {
        return Ok(ServiceIdentity {
            launchd_label: SERVICE_LABEL.to_string(),
            systemd_unit: SERVICE_UNIT.to_string(),
        });
    };
    if instance.len() > 63
        || !instance
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        anyhow::bail!("{SERVICE_INSTANCE_ENV} must contain 1-63 ASCII letters, digits, '-' or '_'");
    }
    Ok(ServiceIdentity {
        launchd_label: format!("{SERVICE_LABEL}.{instance}"),
        systemd_unit: format!("{SERVICE_UNIT}-{instance}"),
    })
}

fn service_identity() -> anyhow::Result<ServiceIdentity> {
    service_identity_from_value(std::env::var(SERVICE_INSTANCE_ENV).ok().as_deref())
}

fn gateway_port_from_env() -> anyhow::Result<Option<u16>> {
    let Some(value) = std::env::var(GATEWAY_PORT_ENV).ok() else {
        return Ok(None);
    };
    let port = value
        .parse::<u16>()
        .map_err(|_| anyhow::anyhow!("{GATEWAY_PORT_ENV} must be a non-zero u16"))?;
    if port == 0 {
        anyhow::bail!("{GATEWAY_PORT_ENV} must be a non-zero u16");
    }
    Ok(Some(port))
}

/// Check if the Bicameral daemon is currently running.
pub fn is_running() -> bool {
    #[cfg(target_os = "linux")]
    {
        let Ok(identity) = service_identity() else {
            return false;
        };
        std::process::Command::new("systemctl")
            .args(["--user", "is-active", "--quiet", &identity.systemd_unit])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(target_os = "macos")]
    {
        let Ok(identity) = service_identity() else {
            return false;
        };
        std::process::Command::new("launchctl")
            .args([
                "print",
                &format!("gui/{}/{}", get_uid(), identity.launchd_label),
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        false
    }
}

/// Get the status of the Bicameral daemon service.
pub fn status() -> ServiceStatus {
    if is_running() {
        ServiceStatus::Running
    } else {
        ServiceStatus::Stopped
    }
}

/// Start the Bicameral daemon service.
pub fn start() -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let identity = service_identity()?;
        let output = std::process::Command::new("systemctl")
            .args(["--user", "start", &identity.systemd_unit])
            .output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to start service: {}", stderr.trim());
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        let identity = service_identity()?;
        let plist_path = std::env::var("HOME")
            .map(|h| {
                std::path::PathBuf::from(h)
                    .join("Library/LaunchAgents")
                    .join(format!("{}.plist", identity.launchd_label))
            })
            .map_err(|_| anyhow::anyhow!("HOME not set"))?;
        let output = std::process::Command::new("launchctl")
            .args(["load", "-w"])
            .arg(&plist_path)
            .output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to start service: {}", stderr.trim());
        }
        // `launchctl load` exits 0 even when launchd rejects the plist, so a
        // successful exit is not a loaded service (bot#1179). Verify the job
        // actually registered before reporting success.
        if !is_running() {
            anyhow::bail!(
                "launchd did not accept the service after load; \
                 run `plutil -lint {}` and `launchctl print gui/{}/{}` to diagnose",
                plist_path.display(),
                get_uid(),
                identity.launchd_label
            );
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        anyhow::bail!("Service start not supported on this platform");
    }
}

/// Stop the Bicameral daemon service.
pub fn stop() -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let identity = service_identity()?;
        let output = std::process::Command::new("systemctl")
            .args(["--user", "stop", &identity.systemd_unit])
            .output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to stop service: {}", stderr.trim());
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        let identity = service_identity()?;
        let plist_path = std::env::var("HOME")
            .map(|h| {
                std::path::PathBuf::from(h)
                    .join("Library/LaunchAgents")
                    .join(format!("{}.plist", identity.launchd_label))
            })
            .map_err(|_| anyhow::anyhow!("HOME not set"))?;
        let output = std::process::Command::new("launchctl")
            .args(["unload"])
            .arg(&plist_path)
            .output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to stop service: {}", stderr.trim());
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        anyhow::bail!("Service stop not supported on this platform");
    }
}

/// Uninstall the Bicameral daemon service configuration.
///
/// Stops the service if running, removes the unit file (systemd) or plist
/// (launchd), and reloads the daemon manager. Idempotent: succeeds even if
/// the service is not installed.
pub fn uninstall() -> anyhow::Result<UninstallResult> {
    #[cfg(target_os = "linux")]
    {
        uninstall_systemd()
    }
    #[cfg(target_os = "macos")]
    {
        uninstall_launchd()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Ok(UninstallResult {
            service_was_running: false,
            config_removed: false,
            config_path: None,
        })
    }
}

/// Result of a service uninstall operation.
#[derive(Debug)]
pub struct UninstallResult {
    /// Whether the service was running and was stopped.
    pub service_was_running: bool,
    /// Whether a service config file was found and removed.
    pub config_removed: bool,
    /// Path to the config file that was removed (if any).
    pub config_path: Option<std::path::PathBuf>,
}

/// Return the path to the service configuration file, if it exists.
pub fn service_config_path() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "linux")]
    {
        let identity = service_identity().ok()?;
        let unit_path = dirs_config()?
            .join("systemd/user")
            .join(format!("{}.service", identity.systemd_unit));
        if unit_path.exists() {
            Some(unit_path)
        } else {
            None
        }
    }
    #[cfg(target_os = "macos")]
    {
        let identity = service_identity().ok()?;
        let plist_path = std::env::var("HOME").ok().map(|h| {
            std::path::PathBuf::from(h)
                .join("Library/LaunchAgents")
                .join(format!("{}.plist", identity.launchd_label))
        })?;
        if plist_path.exists() {
            Some(plist_path)
        } else {
            None
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

/// Install the Bicameral daemon as a user-level service.
pub fn install(binary_path: &Path) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        install_systemd(binary_path)?;
    }
    #[cfg(target_os = "macos")]
    {
        install_launchd(binary_path)?;
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = binary_path;
        anyhow::bail!("Service installation not supported on this platform");
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStatus {
    Running,
    Stopped,
    Unknown,
}

impl std::fmt::Display for ServiceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Running => write!(f, "running"),
            Self::Stopped => write!(f, "stopped"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

#[cfg(target_os = "linux")]
fn install_systemd(binary_path: &Path) -> anyhow::Result<()> {
    let operator_home = operator_home_from_env()?;
    let identity = service_identity()?;
    let gateway_port = gateway_port_from_env()?;
    let unit_dir = dirs_config()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine user config directory"))?
        .join("systemd/user");
    std::fs::create_dir_all(&unit_dir)?;

    let unit_content = systemd_unit_content(binary_path, &operator_home, &identity, gateway_port);
    let unit_path = unit_dir.join(format!("{}.service", identity.systemd_unit));

    std::fs::write(&unit_path, unit_content)?;

    let link_output = std::process::Command::new("systemctl")
        .args(["--user", "link"])
        .arg(&unit_path)
        .output()?;
    if !link_output.status.success() {
        let visible = std::process::Command::new("systemctl")
            .args(["--user", "cat", &identity.systemd_unit])
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !visible {
            anyhow::bail!(
                "Failed to link systemd user unit: {}",
                String::from_utf8_lossy(&link_output.stderr).trim()
            );
        }
    }
    let reload = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()?;
    if !reload.success() {
        anyhow::bail!("Failed to reload systemd user manager");
    }

    // Without `enable`, `WantedBy=default.target` is inert: the daemon runs
    // only until the next logout/reboot and onboarding later finds it stopped
    // (install#19). Enabling is idempotent and user-scoped.
    let enable = std::process::Command::new("systemctl")
        .args(["--user", "enable", &identity.systemd_unit])
        .output()?;
    if !enable.status.success() {
        anyhow::bail!(
            "Failed to enable systemd user unit: {}",
            String::from_utf8_lossy(&enable.stderr).trim()
        );
    }

    tracing::info!(
        unit = %identity.systemd_unit,
        "Systemd user unit installed and enabled"
    );
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_launchd(binary_path: &Path) -> anyhow::Result<()> {
    let operator_home = operator_home_from_env()?;
    let identity = service_identity()?;
    let gateway_port = gateway_port_from_env()?;
    let agents_dir = operator_home.join("Library/LaunchAgents");
    std::fs::create_dir_all(&agents_dir)?;

    let plist_content = launchd_plist_content(binary_path, &operator_home, &identity, gateway_port);
    let plist_path = agents_dir.join(format!("{}.plist", identity.launchd_label));
    let tmp_path = agents_dir.join(format!("{}.plist.tmp", identity.launchd_label));
    std::fs::write(&tmp_path, plist_content)?;
    if let Err(err) = validate_launchd_plist(&tmp_path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err);
    }
    std::fs::rename(&tmp_path, &plist_path)?;

    tracing::info!("LaunchAgent installed at {:?}", plist_path);
    Ok(())
}

/// Reject an invalid LaunchAgent plist before it can replace the installed
/// one. `plutil` ships with macOS; if it cannot run, installation fails
/// closed rather than reporting an unverified install (bot#1179).
#[cfg(target_os = "macos")]
fn validate_launchd_plist(path: &Path) -> anyhow::Result<()> {
    let output = std::process::Command::new("plutil")
        .args(["-lint"])
        .arg(path)
        .output()
        .map_err(|err| {
            anyhow::anyhow!("plutil unavailable; refusing to report a valid install: {err}")
        })?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "generated LaunchAgent plist failed plutil -lint: {} {}",
            detail.trim(),
            stderr.trim()
        );
    }
    Ok(())
}

#[allow(dead_code)] // Compiled on macOS for cross-platform template tests.
fn systemd_unit_content(
    binary_path: &Path,
    operator_home: &Path,
    identity: &ServiceIdentity,
    gateway_port: Option<u16>,
) -> String {
    let binary = binary_path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%");
    let home = operator_home
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%");
    let port_environment = gateway_port
        .map(|port| format!("Environment=\"{GATEWAY_PORT_ENV}={port}\"\n"))
        .unwrap_or_default();
    format!(
        "[Unit]\n\
         Description=Bicameral Daemon ({instance})\n\
         After=network.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         Environment=\"HOME={home}\"\n\
         {port_environment}\
         ExecStart=\"{binary}\" gateway start\n\
         Restart=on-failure\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        instance = identity.systemd_unit,
    )
}

#[allow(dead_code)] // Compiled on Linux for cross-platform template tests.
fn launchd_plist_content(
    binary_path: &Path,
    operator_home: &Path,
    identity: &ServiceIdentity,
    gateway_port: Option<u16>,
) -> String {
    fn xml_escape(path: &Path) -> String {
        path.to_string_lossy()
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }
    let binary = xml_escape(binary_path);
    let home = xml_escape(operator_home);
    let port_environment = gateway_port
        .map(|port| {
            format!("        <key>{GATEWAY_PORT_ENV}</key>\n        <string>{port}</string>\n")
        })
        .unwrap_or_default();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{binary}</string>
        <string>gateway</string>
        <string>start</string>
    </array>
    <key>EnvironmentVariables</key>
    <dict>
        <key>HOME</key>
        <string>{home}</string>
{port_environment}    </dict>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
</dict>
</plist>"#,
        label = identity.launchd_label,
        binary = binary,
        port_environment = port_environment
    )
}

fn operator_home_from_env() -> anyhow::Result<std::path::PathBuf> {
    std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .map_err(|_| anyhow::anyhow!("HOME not set"))
}

#[cfg(target_os = "macos")]
fn get_uid() -> u32 {
    unsafe { libc::getuid() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_templates_are_operator_scoped_and_preserve_binary_paths_with_spaces() {
        let binary = Path::new("/Applications/Bicameral Alpha/bicameral");
        let operator_home = Path::new("/Users/Topology Operator");
        let identity = service_identity_from_value(Some("topology_42")).unwrap();

        let systemd = systemd_unit_content(binary, operator_home, &identity, Some(41_042));
        assert!(
            systemd.contains("ExecStart=\"/Applications/Bicameral Alpha/bicameral\" gateway start")
        );
        assert!(systemd.contains("Description=Bicameral Daemon (bicameral-topology_42)"));
        assert!(systemd.contains("Environment=\"HOME=/Users/Topology Operator\""));
        assert!(systemd.contains("Environment=\"BICAMERAL_GATEWAY_PORT=41042\""));
        assert!(!systemd.contains("--workspace"));
        assert!(!systemd.contains("WorkingDirectory"));

        let launchd = launchd_plist_content(binary, operator_home, &identity, Some(41_042));
        assert!(launchd.contains("<string>com.bicameral.daemon.topology_42</string>"));
        assert!(launchd.contains("<string>/Applications/Bicameral Alpha/bicameral</string>"));
        assert!(launchd.contains("<key>EnvironmentVariables</key>"));
        assert!(launchd.contains("<key>HOME</key>"));
        assert!(launchd.contains("<string>/Users/Topology Operator</string>"));
        assert!(launchd.contains("<key>BICAMERAL_GATEWAY_PORT</key>"));
        assert!(launchd.contains("<string>41042</string>"));
        assert!(!launchd.contains("--workspace"));
        assert!(!launchd.contains("WorkingDirectory"));
    }

    #[test]
    fn launchd_plist_has_no_standalone_backslash_and_closes_dict_cleanly() {
        let binary = Path::new("/usr/local/bin/bicameral");
        let operator_home = Path::new("/Users/operator");
        let identity = service_identity_from_value(None).unwrap();

        // Empty port_environment path (bot#1179): launchd rejected the shipped
        // plist because a literal line-continuation backslash leaked out of the
        // raw-string template into the EnvironmentVariables dict.
        let without_port = launchd_plist_content(binary, operator_home, &identity, None);
        assert!(!without_port.contains('\\'));
        assert!(!without_port.contains("BICAMERAL_GATEWAY_PORT"));
        assert!(without_port.contains("<string>/Users/operator</string>\n    </dict>"));

        let with_port = launchd_plist_content(binary, operator_home, &identity, Some(41_042));
        assert!(!with_port.contains('\\'));
        assert!(with_port.contains("<string>41042</string>\n    </dict>"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn generated_plists_pass_plutil_lint() {
        let binary = Path::new("/usr/local/bin/bicameral");
        let operator_home = Path::new("/Users/operator");
        let identity = service_identity_from_value(Some("lint_check")).unwrap();

        for port in [None, Some(41_042)] {
            let content = launchd_plist_content(binary, operator_home, &identity, port);
            let dir = std::env::temp_dir();
            let path = dir.join(format!(
                "bicameral-plist-lint-{:?}.plist",
                port.unwrap_or(0)
            ));
            std::fs::write(&path, &content).unwrap();
            let status = std::process::Command::new("plutil")
                .args(["-lint"])
                .arg(&path)
                .status()
                .expect("plutil is a macOS platform binary");
            let _ = std::fs::remove_file(&path);
            assert!(
                status.success(),
                "plutil rejected template for port {port:?}"
            );
        }
    }

    #[test]
    fn service_instance_is_bounded_and_defaults_to_production_identity() {
        assert_eq!(
            service_identity_from_value(None).unwrap(),
            ServiceIdentity {
                launchd_label: "com.bicameral.daemon".into(),
                systemd_unit: "bicameral".into(),
            }
        );
        assert!(service_identity_from_value(Some("../other")).is_err());
        assert!(service_identity_from_value(Some(&"a".repeat(64))).is_err());
    }
}

#[cfg(target_os = "linux")]
fn uninstall_systemd() -> anyhow::Result<UninstallResult> {
    let identity = service_identity()?;
    let was_running = is_running();
    if was_running {
        let _ = stop(); // best-effort stop
    }

    // Disable the unit so it doesn't start on next boot
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "disable", &identity.systemd_unit])
        .status();

    let _ = std::process::Command::new("systemctl")
        .args(["--user", "unlink", &identity.systemd_unit])
        .status();

    let unit_path = dirs_config().map(|d| {
        d.join("systemd/user")
            .join(format!("{}.service", identity.systemd_unit))
    });

    let config_removed = if let Some(ref path) = unit_path {
        if path.exists() {
            std::fs::remove_file(path)?;
            // Reload after removing the unit file
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "daemon-reload"])
                .status();
            true
        } else {
            false
        }
    } else {
        false
    };

    Ok(UninstallResult {
        service_was_running: was_running,
        config_removed,
        config_path: unit_path,
    })
}

#[cfg(target_os = "macos")]
fn uninstall_launchd() -> anyhow::Result<UninstallResult> {
    let identity = service_identity()?;
    let was_running = is_running();

    let plist_path = std::env::var("HOME").ok().map(|h| {
        std::path::PathBuf::from(h)
            .join("Library/LaunchAgents")
            .join(format!("{}.plist", identity.launchd_label))
    });

    // Unload the agent if running
    if was_running {
        if let Some(ref path) = plist_path {
            let _ = std::process::Command::new("launchctl")
                .args(["unload"])
                .arg(path)
                .status();
        }
    }

    let config_removed = if let Some(ref path) = plist_path {
        if path.exists() {
            std::fs::remove_file(path)?;
            true
        } else {
            false
        }
    } else {
        false
    };

    Ok(UninstallResult {
        service_was_running: was_running,
        config_removed,
        config_path: plist_path,
    })
}

#[cfg(target_os = "linux")]
fn dirs_config() -> Option<std::path::PathBuf> {
    std::env::var("XDG_CONFIG_HOME")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| std::path::PathBuf::from(h).join(".config"))
        })
}
